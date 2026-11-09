use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use gilfoyle_core::{
    AppError, JobResult, Package, PackageAdmin, PackageFilter, PackageList, RefreshReport,
    SystemAdmin, SystemError, SystemService, SystemdAdmin, SystemdError, Unit, UnitActionOutcome,
    UnitFilter, UnitName, UnitStatus, UnitTimestamps, UpdateFilter, UpdateList, UpgradePlan,
    UpgradeReport, Uptime, UptimeSource,
};
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
        systemd: Arc::new(FakeSystemd::default()),
        packages: Arc::new(FakePackage),
        auth: AuthState::new(Arc::new(TokenStore::new(tokens))),
        allowed_hosts,
        allowed_origins,
        enabled_tools,
    }
}

/// A scriptable stand-in for the package driving port.
#[derive(Clone)]
struct FakePackage;

#[async_trait]
impl PackageAdmin for FakePackage {
    async fn list_packages(&self, filter: &PackageFilter) -> Result<PackageList, AppError> {
        Ok(filter.apply(vec![Package {
            name: "nginx".to_string(),
            version: "1.24.0".to_string(),
            architecture: "amd64".to_string(),
            origin: "debian".to_string(),
        }]))
    }

    async fn list_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, AppError> {
        Ok(filter.apply(Vec::new()))
    }

    async fn preview_upgrade(&self) -> Result<UpgradePlan, AppError> {
        Ok(UpgradePlan::empty())
    }

    async fn refresh_packages(&self, _force: bool) -> Result<RefreshReport, AppError> {
        Ok(RefreshReport {
            success: true,
            exit: "success".to_string(),
            error_code: None,
            error_details: None,
            timed_out: false,
            runtime_ms: 12,
            repos: Vec::new(),
            repos_truncated: false,
        })
    }

    async fn upgrade_packages(&self) -> Result<UpgradeReport, AppError> {
        Ok(UpgradeReport::nothing_to_do())
    }
}

/// A scriptable stand-in for the systemd driving port.
#[derive(Clone, Default)]
struct FakeSystemd {
    units: Vec<Unit>,
    outcome: Option<UnitActionOutcome>,
}

impl FakeSystemd {
    fn with_unit(mut self, name: &str) -> Self {
        self.units.push(Unit {
            name: name.to_string(),
            description: format!("desc {name}"),
            load_state: "loaded".to_string(),
            active_state: "active".to_string(),
            sub_state: "running".to_string(),
        });
        self
    }

    fn with_outcome(mut self, result: &str) -> Self {
        self.outcome = Some(UnitActionOutcome {
            unit: "nginx.service".to_string(),
            job_result: JobResult::new(result),
            active_state_before: "inactive".to_string(),
            active_state_after: "active".to_string(),
        });
        self
    }
}

#[async_trait]
impl SystemdAdmin for FakeSystemd {
    async fn list_units(&self, _filter: &UnitFilter) -> Result<Vec<Unit>, AppError> {
        Ok(self.units.clone())
    }

    async fn unit_status(&self, name: &str) -> Result<UnitStatus, AppError> {
        // Mirror the production service, which normalises the name before the adapter.
        let normalized = UnitName::parse(name).map_err(AppError::Domain)?;
        match self
            .units
            .iter()
            .find(|unit| unit.name == normalized.as_str())
        {
            Some(unit) => Ok(UnitStatus {
                name: unit.name.clone(),
                description: unit.description.clone(),
                load_state: unit.load_state.clone(),
                active_state: unit.active_state.clone(),
                sub_state: unit.sub_state.clone(),
                unit_file_state: Some("enabled".to_string()),
                can_start: Some(true),
                can_stop: Some(true),
                main_pid: Some(7),
                result: Some("success".to_string()),
                timestamps: UnitTimestamps {
                    state_change: None,
                    active_enter: None,
                    inactive_enter: None,
                    exec_main_start: None,
                    exec_main_exit: None,
                },
            }),
            None => Err(AppError::Systemd(SystemdError::NotFound(
                normalized.as_str().to_string(),
            ))),
        }
    }

    async fn start_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError> {
        self.outcome
            .clone()
            .ok_or_else(|| AppError::NotManaged(name.to_string()))
    }

    async fn stop_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError> {
        self.outcome
            .clone()
            .ok_or_else(|| AppError::NotManaged(name.to_string()))
    }
}

fn state_with_systemd(systemd: FakeSystemd) -> AppState {
    AppState {
        admin: Arc::new(SystemService::new(FixedSource(3661.0))) as Arc<dyn SystemAdmin>,
        systemd: Arc::new(systemd),
        packages: Arc::new(FakePackage),
        auth: AuthState::new(Arc::new(TokenStore::new(vec![entry("admin", "s3cr3t")]))),
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        enabled_tools: None,
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
    tools_call_with(name, json!({}))
}

fn tools_call_with(name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
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
    assert_eq!(
        names,
        vec![
            "get_uptime".to_string(),
            "list_packages".to_string(),
            "list_units".to_string(),
            "list_updates".to_string(),
            "preview_upgrade".to_string(),
            "refresh_package_cache".to_string(),
            "start_unit".to_string(),
            "stop_unit".to_string(),
            "unit_status".to_string(),
            "upgrade_packages".to_string(),
        ]
    );

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

async fn call_tool(addr: SocketAddr, session: &str, request: &Value) -> Value {
    json_body(post(addr, Some("s3cr3t"), Some(session), request).await).await
}

#[tokio::test]
async fn lists_units_through_the_tool() {
    let addr = spawn(state_with_systemd(
        FakeSystemd::default().with_unit("nginx.service"),
    ))
    .await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = call_tool(addr, &session, &tools_call_with("list_units", json!({}))).await;
    let units = body["result"]["structuredContent"]["units"]
        .as_array()
        .unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0]["name"], "nginx.service");
}

#[tokio::test]
async fn every_tool_output_schema_is_an_object() {
    // The MCP spec requires the root of `outputSchema` to be an object; a bare array is
    // rejected by strict clients and breaks tool discovery.
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = json_body(post(addr, Some("s3cr3t"), Some(&session), &tools_list()).await).await;
    for tool in body["result"]["tools"].as_array().unwrap() {
        if let Some(schema) = tool.get("outputSchema") {
            assert_eq!(
                schema["type"], "object",
                "tool `{}` has a non-object outputSchema",
                tool["name"]
            );
        }
    }
}

#[tokio::test]
async fn rejects_an_unknown_unit_type_filter() {
    let addr = spawn(state_with_systemd(FakeSystemd::default())).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let request = tools_call_with("list_units", json!({ "unit_type": "nonsense" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown unit type"),
        "{body}"
    );
}

#[tokio::test]
async fn reports_unit_status_through_the_tool() {
    let addr = spawn(state_with_systemd(
        FakeSystemd::default().with_unit("nginx.service"),
    ))
    .await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let request = tools_call_with("unit_status", json!({ "name": "nginx" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["structuredContent"]["name"], "nginx.service");
    assert_eq!(body["result"]["structuredContent"]["main_pid"], 7);
}

#[tokio::test]
async fn unit_status_failure_is_a_tool_error() {
    let addr = spawn(state_with_systemd(FakeSystemd::default())).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let request = tools_call_with("unit_status", json!({ "name": "ghost" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ghost.service"),
        "{body}"
    );
}

#[tokio::test]
async fn starts_a_unit_through_the_tool() {
    let addr = spawn(state_with_systemd(
        FakeSystemd::default().with_outcome("done"),
    ))
    .await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let request = tools_call_with("start_unit", json!({ "name": "nginx" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["structuredContent"]["unit"], "nginx.service");
    assert_eq!(body["result"]["structuredContent"]["job_result"], "done");
    assert_eq!(
        body["result"]["structuredContent"]["active_state_after"],
        "active"
    );
}

#[tokio::test]
async fn a_failed_job_is_a_tool_error() {
    let addr = spawn(state_with_systemd(
        FakeSystemd::default().with_outcome("failed"),
    ))
    .await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let request = tools_call_with("stop_unit", json!({ "name": "nginx" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("did not succeed"),
        "{body}"
    );
}

#[tokio::test]
async fn lists_packages_through_the_tool() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = call_tool(addr, &session, &tools_call_with("list_packages", json!({}))).await;
    let packages = body["result"]["structuredContent"]["packages"]
        .as_array()
        .unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["name"], "nginx");
    assert_eq!(packages[0]["version"], "1.24.0");
}

#[tokio::test]
async fn lists_updates_and_rejects_a_bad_severity() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = call_tool(addr, &session, &tools_call_with("list_updates", json!({}))).await;
    assert_eq!(body["result"]["structuredContent"]["total"], 0);

    let request = tools_call_with("list_updates", json!({ "min_severity": "nonsense" }));
    let body = call_tool(addr, &session, &request).await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown severity"),
        "{body}"
    );
}

#[tokio::test]
async fn previews_an_upgrade_through_the_tool() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = call_tool(addr, &session, &tools_call("preview_upgrade")).await;
    assert_eq!(body["result"]["structuredContent"]["total"], 0);
    assert_eq!(body["result"]["structuredContent"]["would_install"], 0);
}

#[tokio::test]
async fn refreshes_and_upgrades_through_the_tools() {
    let addr = spawn(state(vec![entry("admin", "s3cr3t")], None)).await;
    let session = initialize_session(addr, "s3cr3t").await;
    send_initialized(addr, "s3cr3t", &session).await;

    let body = call_tool(
        addr,
        &session,
        &tools_call_with("refresh_package_cache", json!({ "force": false })),
    )
    .await;
    assert_eq!(body["result"]["structuredContent"]["success"], true);
    assert_eq!(body["result"]["structuredContent"]["runtime_ms"], 12);

    let body = call_tool(addr, &session, &tools_call("upgrade_packages")).await;
    assert_eq!(body["result"]["structuredContent"]["success"], true);
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
