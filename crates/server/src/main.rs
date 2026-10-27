//! Gilfoyle composition root.
//!
//! This file only wires adapters together; it contains no logic worth mutating
//! and is excluded from mutation testing (see `mutants.toml`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use gilfoyle_core::{SystemAdmin, SystemService};
use gilfoyle_server::auth::{AuthState, TokenStore};
use gilfoyle_server::config::{DEFAULT_CONFIG_PATH, RawConfig, resolve_config};
use gilfoyle_server::hash::hash_token_hex;
use gilfoyle_server::http::{AppState, build_router};
use gilfoyle_system::ProcUptime;

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
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::HashToken) => hash_token_command(),
        None => match run(&cli.config).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("gilfoyle: {message}");
                ExitCode::FAILURE
            }
        },
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

async fn run(config_path: &Path) -> Result<(), String> {
    let raw = load_raw(config_path)?;
    let config = resolve_config(raw, &|key| std::env::var(key).ok(), &|path| {
        std::fs::read_to_string(path).ok()
    })
    .map_err(|error| error.to_string())?;

    init_tracing(&config.log_level);

    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(ProcUptime::new()));
    let state = AppState {
        admin,
        auth: AuthState::new(Arc::new(TokenStore::new(config.tokens.clone()))),
        allowed_hosts: config.allowed_hosts.clone(),
        allowed_origins: config.allowed_origins.clone(),
        enabled_tools: config.enabled_tools.clone(),
    };

    serve(config.bind, state).await
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
