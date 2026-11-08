//! Server tool behaviour over MCP, on an in-memory transport.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use gilfoyle_core::{
    AppError, JobResult, SystemAdmin, SystemError, SystemService, SystemdAdmin, SystemdError, Unit,
    UnitActionOutcome, UnitFilter, UnitName, UnitStatus, UnitTimestamps, Uptime, UptimeSource,
};
use gilfoyle_server::GilfoyleServer;
use rmcp::RoleClient;
use rmcp::handler::client::ClientHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use serde_json::{Value, json};

#[derive(Clone)]
struct FixedSource(f64);

impl UptimeSource for FixedSource {
    fn uptime(&self) -> Result<Uptime, SystemError> {
        Uptime::from_seconds(self.0).map_err(|error| SystemError::Invalid(error.to_string()))
    }
}

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

#[derive(Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

async fn serve(
    systemd: FakeSystemd,
    enabled: Option<BTreeSet<String>>,
) -> RunningService<RoleClient, TestClient> {
    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(FixedSource(3661.0)));
    let handler = GilfoyleServer::new(admin, Arc::new(systemd), enabled);
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = rmcp::serve_server(handler, server_io).await {
            let _ = running.waiting().await;
        }
    });
    rmcp::serve_client(TestClient, client_io).await.unwrap()
}

fn call(name: &str, arguments: Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    params.arguments = arguments.as_object().cloned();
    params
}

async fn tool_names(client: &RunningService<RoleClient, TestClient>) -> Vec<String> {
    let mut names: Vec<String> = client
        .peer()
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn lists_the_five_tools_and_serves_uptime() {
    let client = serve(FakeSystemd::default(), None).await;
    assert_eq!(
        tool_names(&client).await,
        vec![
            "get_uptime".to_string(),
            "list_units".to_string(),
            "start_unit".to_string(),
            "stop_unit".to_string(),
            "unit_status".to_string(),
        ]
    );

    let result = client
        .peer()
        .call_tool(call("get_uptime", json!({})))
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["uptime_seconds"], 3661.0);
}

#[tokio::test]
async fn lists_units_and_reports_status() {
    let client = serve(FakeSystemd::default().with_unit("nginx.service"), None).await;

    let listed = client
        .peer()
        .call_tool(call("list_units", json!({})))
        .await
        .unwrap();
    let units = listed.structured_content.unwrap();
    assert_eq!(units["units"].as_array().unwrap().len(), 1);
    assert_eq!(units["units"][0]["name"], "nginx.service");

    let status = client
        .peer()
        .call_tool(call("unit_status", json!({ "name": "nginx" })))
        .await
        .unwrap();
    assert_eq!(status.structured_content.unwrap()["name"], "nginx.service");
}

#[tokio::test]
async fn unknown_unit_type_is_a_tool_error() {
    let client = serve(FakeSystemd::default(), None).await;
    let result = client
        .peer()
        .call_tool(call("list_units", json!({ "unit_type": "nonsense" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn missing_unit_status_is_a_tool_error() {
    let client = serve(FakeSystemd::default(), None).await;
    let result = client
        .peer()
        .call_tool(call("unit_status", json!({ "name": "ghost" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn start_reports_the_job_and_a_failed_job_is_an_error() {
    let ok = serve(FakeSystemd::default().with_outcome("done"), None).await;
    let result = ok
        .peer()
        .call_tool(call("start_unit", json!({ "name": "nginx" })))
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["job_result"], "done");

    let failed = serve(FakeSystemd::default().with_outcome("failed"), None).await;
    let result = failed
        .peer()
        .call_tool(call("stop_unit", json!({ "name": "nginx" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn disabled_tools_are_hidden_and_rejected() {
    let client = serve(FakeSystemd::default(), Some(BTreeSet::new())).await;
    assert!(tool_names(&client).await.is_empty());

    let result = client.peer().call_tool(call("get_uptime", json!({}))).await;
    assert!(result.is_err(), "a disabled tool must be rejected");
}
