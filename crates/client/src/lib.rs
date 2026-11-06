//! Gilfoyle client: aggregates several gilfoyle nodes behind a single MCP surface.
//!
//! The client speaks MCP over stdio to the AI agent, holds SSH connections to each node,
//! and routes every tool call by the `node` argument.
#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod node;
pub mod proxy;

use std::path::Path;

pub use config::{ClientConfig, DEFAULT_POOL_SIZE, NodeConfig};
pub use error::ClientError;
pub use proxy::GilfoyleProxy;

/// Load a client configuration from a TOML file.
pub fn load_config(path: &Path) -> Result<ClientConfig, ClientError> {
    let contents = std::fs::read_to_string(path).map_err(|error| ClientError::Config {
        path: path.display().to_string(),
        cause: error.to_string(),
    })?;
    config::parse(&contents)
}

/// Connect to every configured node (tolerating failures) and serve MCP on stdio.
pub async fn run(path: &Path) -> Result<(), String> {
    let config = load_config(path).map_err(|error| error.to_string())?;

    let mut labels = Vec::with_capacity(config.nodes.len());
    let mut upstreams = Vec::new();
    for node in &config.nodes {
        labels.push(node.label.clone());
        match node::Upstream::connect(node, config.pool_size).await {
            Ok(upstream) => {
                tracing::info!(node = %node.label, "connected");
                upstreams.push(upstream);
            }
            Err(error) => tracing::warn!(node = %node.label, %error, "unreachable at startup"),
        }
    }

    let proxy = GilfoyleProxy::new(labels, upstreams);
    tracing::info!("gilfoyle-client serving mcp on stdio");

    let running = rmcp::serve_server(proxy, rmcp::transport::stdio())
        .await
        .map_err(|error| error.to_string())?;
    let _ = running.waiting().await;
    Ok(())
}
