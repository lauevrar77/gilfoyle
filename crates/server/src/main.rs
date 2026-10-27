//! Gilfoyle composition root.
//!
//! This file only wires adapters together; it contains no logic worth mutating
//! and is excluded from mutation testing (see `.cargo/mutants.toml`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use gilfoyle_core::{
    Allowlist, ServiceCatalog, ServiceControl, SystemAdmin, SystemService, SystemdAdmin,
    SystemdService,
};
use gilfoyle_server::auth::{AuthState, TokenStore};
use gilfoyle_server::config::{DEFAULT_CONFIG_PATH, RawConfig, resolve_allowlist, resolve_config};
use gilfoyle_server::hash::hash_token_hex;
use gilfoyle_server::http::{AppState, build_router};
use gilfoyle_server::polkit::{POLKIT_RULE_PATH, render_polkit_rule};
use gilfoyle_system::ProcUptime;
use gilfoyle_systemd::LazySystemd;

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
    /// Print the SHA-256 (hex) of a token read from stdin.
    HashToken,
    /// systemd administration helpers.
    Systemd {
        #[command(subcommand)]
        command: SystemdCommand,
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
        Some(Command::HashToken) => hash_token_command(),
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

fn hash_token_command() -> ExitCode {
    use std::io::BufRead;

    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => {
            eprintln!("gilfoyle: no token provided on stdin");
            ExitCode::FAILURE
        }
        Ok(_) => {
            println!("{}", hash_token_hex(line.trim_end_matches(['\n', '\r'])));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("gilfoyle: {error}");
            ExitCode::FAILURE
        }
    }
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
    let config = resolve_config(raw, &|key| std::env::var(key).ok(), &|path| {
        std::fs::read_to_string(path).ok()
    })
    .map_err(|error| error.to_string())?;

    init_tracing(&config.log_level);
    warn_if_polkit_out_of_sync(&config.allowlist);

    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(ProcUptime::new()));
    let lazy = Arc::new(LazySystemd::system());
    let catalog: Arc<dyn ServiceCatalog> = lazy.clone();
    let control: Arc<dyn ServiceControl> = lazy;
    let systemd: Arc<dyn SystemdAdmin> = Arc::new(SystemdService::new(
        catalog,
        control,
        config.allowlist.clone(),
    ));

    let state = AppState {
        admin,
        systemd,
        auth: AuthState::new(Arc::new(TokenStore::new(config.tokens.clone()))),
        allowed_hosts: config.allowed_hosts.clone(),
        allowed_origins: config.allowed_origins.clone(),
        enabled_tools: config.enabled_tools.clone(),
    };

    serve(config.bind, state).await
}

fn warn_if_polkit_out_of_sync(allowlist: &Allowlist) {
    let expected = render_polkit_rule(allowlist);
    match std::fs::read_to_string(POLKIT_RULE_PATH) {
        Ok(installed) if installed == expected => {}
        Ok(_) => tracing::warn!(
            path = POLKIT_RULE_PATH,
            "installed polkit rule differs from [systemd] manage_units; re-run `gilfoyle systemd polkit-rule`"
        ),
        Err(_) => tracing::warn!(
            path = POLKIT_RULE_PATH,
            "polkit rule not installed; start and stop will be refused by polkit"
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

async fn serve(bind: SocketAddr, state: AppState) -> Result<(), String> {
    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| format!("failed to bind {bind}: {error}"))?;
    tracing::info!(%bind, "gilfoyle listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| error.to_string())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
