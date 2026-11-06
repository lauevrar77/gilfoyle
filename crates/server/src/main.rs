//! Gilfoyle composition root.
//!
//! This file only wires adapters together; it contains no logic worth mutating and is
//! excluded from mutation testing (see `.cargo/mutants.toml`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use gilfoyle_core::Allowlist;
use gilfoyle_server::config::{
    DEFAULT_CONFIG_PATH, DEFAULT_HOST_KEY_PATH, RawConfig, resolve_allowlist, resolve_config,
};
use gilfoyle_server::polkit::{POLKIT_RULE_PATH, render_polkit_rule};
use gilfoyle_ssh::hostkey;

#[derive(Parser)]
#[command(
    name = "gilfoyle",
    version,
    about = "Secure MCP server for Linux administration"
)]
struct Cli {
    /// Path to the TOML configuration file.
    #[arg(long, value_name = "PATH", default_value = DEFAULT_CONFIG_PATH)]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the multi-node client, serving MCP on stdio.
    Client {
        /// Path to the client TOML configuration file.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
    /// SSH helpers.
    Ssh {
        #[command(subcommand)]
        command: SshCommand,
    },
    /// systemd administration helpers.
    Systemd {
        #[command(subcommand)]
        command: SystemdCommand,
    },
}

#[derive(Subcommand)]
enum SshCommand {
    /// Generate an ed25519 host key (refusing to overwrite) and print its fingerprint.
    Keygen {
        /// Destination path; defaults to the standard host-key path.
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SystemdCommand {
    /// Print the polkit rule derived from [systemd] manage_units.
    PolkitRule,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Client { config }) => report(client(config).await),
        Some(Command::Ssh {
            command: SshCommand::Keygen { path },
        }) => report(keygen(path)),
        Some(Command::Systemd {
            command: SystemdCommand::PolkitRule,
        }) => report(polkit_rule_command(&cli.config)),
        None => report(run(&cli.config).await),
    }
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gilfoyle: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn client(config: Option<PathBuf>) -> Result<(), String> {
    let path = config
        .or_else(|| std::env::var_os("GILFOYLE_CLIENT_CONFIG").map(PathBuf::from))
        .ok_or_else(|| {
            "no client config: pass --config or set GILFOYLE_CLIENT_CONFIG".to_string()
        })?;
    gilfoyle_client::run(&path).await
}

fn keygen(path: Option<PathBuf>) -> Result<(), String> {
    let path = path.unwrap_or_else(|| PathBuf::from(DEFAULT_HOST_KEY_PATH));
    let key = hostkey::generate().map_err(|error| error.to_string())?;
    hostkey::write(&key, &path).map_err(|error| error.to_string())?;
    println!("{}", hostkey::fingerprint(key.public_key()));
    Ok(())
}

fn polkit_rule_command(config_path: &Path) -> Result<(), String> {
    let raw = load_raw(config_path)?;
    let allowlist =
        resolve_allowlist(&raw.systemd.manage_units).map_err(|error| error.to_string())?;
    print!("{}", render_polkit_rule(&allowlist));
    Ok(())
}

async fn run(config_path: &Path) -> Result<(), String> {
    let raw = load_raw(config_path)?;
    let config =
        resolve_config(raw, &|key| std::env::var(key).ok()).map_err(|error| error.to_string())?;

    init_tracing(&config.log_level);
    warn_if_polkit_out_of_sync(&config.allowlist);

    gilfoyle_server::serve::run(config).await
}

fn warn_if_polkit_out_of_sync(allowlist: &Allowlist) {
    let expected = render_polkit_rule(allowlist);
    match std::fs::read_to_string(POLKIT_RULE_PATH) {
        Ok(installed) if installed == expected => {}
        Ok(_) => tracing::warn!(
            path = POLKIT_RULE_PATH,
            "installed polkit rule differs from [systemd] manage_units; re-run `gilfoyle systemd polkit-rule`"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => tracing::warn!(
            path = POLKIT_RULE_PATH,
            "polkit rule not installed; start and stop will be refused by polkit"
        ),
        Err(error) => tracing::debug!(
            path = POLKIT_RULE_PATH,
            %error,
            "could not read the polkit rule to verify it"
        ),
    }
}

fn load_raw(path: &Path) -> Result<RawConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str::<RawConfig>(&contents)
            .map_err(|error| format!("failed to parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RawConfig::default()),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

fn init_tracing(level: &str) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_writer(std::io::stderr)
        .try_init();
}
