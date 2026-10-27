//! HTTP wiring: the axum router and its middleware stack.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::{Json, Router, middleware, routing::get};
use gilfoyle_core::{SystemAdmin, SystemdAdmin};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tower_http::limit::RequestBodyLimitLayer;

use crate::auth::{AuthState, require_bearer};
use crate::mcp::GilfoyleServer;

/// Maximum accepted request body size.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// Everything the router needs to serve requests.
#[derive(Clone)]
pub struct AppState {
    /// The uptime driving port.
    pub admin: Arc<dyn SystemAdmin>,
    /// The systemd driving port.
    pub systemd: Arc<dyn SystemdAdmin>,
    /// Authentication state.
    pub auth: AuthState,
    /// Host header allowlist for the MCP transport.
    pub allowed_hosts: Vec<String>,
    /// Origin header allowlist for the MCP transport.
    pub allowed_origins: Vec<String>,
    /// Enabled tool allowlist; `None` means all compiled tools.
    pub enabled_tools: Option<BTreeSet<String>>,
}

/// Build the application router.
///
/// `/healthz` is public and secret-free; `/mcp` is wrapped in the bearer
/// middleware. Requests to unknown paths fall through to a 404.
pub fn build_router(state: AppState) -> Router {
    let server = GilfoyleServer::new(
        state.admin.clone(),
        state.systemd.clone(),
        state.enabled_tools.clone(),
    );

    let mut config = StreamableHttpServerConfig::default();
    if !state.allowed_hosts.is_empty() {
        config = config.with_allowed_hosts(state.allowed_hosts.clone());
    }
    if !state.allowed_origins.is_empty() {
        config = config.with_allowed_origins(state.allowed_origins.clone());
    }

    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        config,
    );

    let mcp =
        Router::new()
            .nest_service("/mcp", service)
            .route_layer(middleware::from_fn_with_state(
                state.auth.clone(),
                require_bearer,
            ));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(mcp)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
