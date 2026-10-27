use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;

use gilfoyle_core::{SystemAdmin, SystemError, SystemService, Uptime, UptimeSource};
use gilfoyle_server::auth::{AuthState, TokenEntry, TokenStore};
use gilfoyle_server::hash::sha256;
use gilfoyle_server::http::{AppState, MAX_BODY_BYTES, build_router};
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-11-25";

#[derive(Clone)]
struct FixedSource(f64);

impl UptimeSource for FixedSource {
    fn uptime(&self) -> Result<Uptime, SystemError> {
        Uptime::from_seconds(self.0).map_err(|error| SystemError::Invalid(error.to_string()))
    }
}

fn entry(id: &str, secret: &str) -> TokenEntry {
    TokenEntry {
        id: id.to_string(),
        hash: sha256(secret.as_bytes()),
    }
}

fn state(tokens: Vec<TokenEntry>, enabled_tools: Option<BTreeSet<String>>) -> AppState {
    state_full(tokens, enabled_tools, Vec::new(), Vec::new())
}

fn state_full(
    tokens: Vec<TokenEntry>,
    enabled_tools: Option<BTreeSet<String>>,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
) -> AppState {
    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(FixedSource(3661.0)));
    AppState {
        admin,
        auth: AuthState::new(Arc::new(TokenStore::new(tokens))),
        allowed_hosts,
        allowed_origins,
        enabled_tools,
    }
}

async fn spawn(state: AppState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = build_router(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    addr
}

async fn post(
    addr: SocketAddr,
    token: Option<&str>,
    session: Option<&str>,
    body: &Value,
) -> reqwest::Response {
    let mut request = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", PROTOCOL)
        .json(body);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    request.send().await.unwrap()
}

/// Read a JSON-RPC payload from a response, transparently unwrapping SSE.
async fn json_body(response: reqwest::Response) -> Value {
    let is_sse = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
    let text = response.text().await.unwrap();
    if is_sse {
        let payloads: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|data| !data.is_empty())
            .collect();
        serde_json::from_str(payloads.last().expect("no SSE data payload")).unwrap()
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "gilfoyle-test", "version": "1.0" }
        }
    })
}

fn tools_list() -> Value {
    json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })
}

fn tools_call(name: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": name, "arguments": {} }
    })
}

async fn initialize_session(addr: SocketAddr, token: &str) -> String {
    let response = post(addr, Some(token), None, &initialize()).await;
    assert_eq!(response.status(), 200, "initialize failed");
    response
        .headers()
        .get("mcp-session-id")
        .expect("missing session id")
        .to_str()
        .unwrap()
        .to_string()
}

async fn send_initialized(addr: SocketAddr, token: &str, session: &str) {
    let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let response = post(addr, Some(token), Some(session), &initialized).await;
    assert!(response.status().is_success(), "got {}", response.status());
}

#[tokio::test]
async fn healthz_is_public_and_secret_free() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"status":"ok"})
    );
    assert_eq!(MAX_BODY_BYTES, 65536);
}

#[tokio::test]
async fn unknown_paths_return_404() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = reqwest::get(format!("http://{addr}/does-not-exist"))
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn mcp_requires_a_token() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = post(addr, None, None, &initialize()).await;
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn mcp_rejects_a_wrong_token() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = post(addr, Some("nope"), None, &initialize()).await;
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn mcp_rejects_a_malformed_scheme() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("authorization", "Basic s3cr3t")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn serves_get_uptime_over_a_full_handshake() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = json_body(post(addr, Some("s3cr3t"), Some(&session), &tools_list()).await).await;
    let names: Vec<String> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["get_uptime".to_string()]);

    let body = json_body(
        post(
            addr,
            Some("s3cr3t"),
            Some(&session),
            &tools_call("get_uptime"),
        )
        .await,
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["uptime_seconds"],
        3661.0
    );
    assert_eq!(
        body["result"]["structuredContent"]["uptime_human"],
        "1h 1m 1s"
    );
}

#[tokio::test]
async fn disabled_tools_are_hidden_and_rejected() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], Some(BTreeSet::new()))).await;
    let session = initialize_session(addr, "s3cr3t").await;

    let body = json_body(post(addr, Some("s3cr3t"), Some(&session), &tools_list()).await).await;
    assert!(body["result"]["tools"].as_array().unwrap().is_empty());

    let body = json_body(
        post(
            addr,
            Some("s3cr3t"),
            Some(&session),
            &tools_call("get_uptime"),
        )
        .await,
    )
    .await;
    assert!(
        body["error"].is_object(),
        "expected a JSON-RPC error: {body}"
    );
}

#[tokio::test]
async fn a_session_cannot_be_hijacked_by_another_token() {
    let addr = spawn(state(
        vec![entry("admin", "s3cr3t"), entry("other", "other-secret")],
        None,
    ))
    .await;
    let session = initialize_session(addr, "s3cr3t").await;
    // The legitimate client binds the session with its first session-bearing request.
    send_initialized(addr, "s3cr3t", &session).await;

    let response = post(addr, Some("other-secret"), Some(&session), &tools_list()).await;
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn default_host_allowlist_rejects_a_foreign_host() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("host", "evil.example")
        .header("authorization", "Bearer s3cr3t")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn configured_host_allowlist_rejects_loopback() {
    let addr = spawn(state_full(
        vec![entry("admin", "s3cr3t")],
        None,
        vec!["example.com".to_string()],
        Vec::new(),
    ))
    .await;
    let response = post(addr, Some("s3cr3t"), None, &initialize()).await;
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn configured_origin_allowlist_rejects_a_foreign_origin() {
    let addr = spawn(state_full(
        vec![entry("admin", "s3cr3t")],
        None,
        Vec::new(),
        vec!["https://example.com".to_string()],
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("origin", "https://evil.example")
        .header("authorization", "Bearer s3cr3t")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn empty_origin_allowlist_is_permissive() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("origin", "https://evil.example")
        .header("authorization", "Bearer s3cr3t")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}
