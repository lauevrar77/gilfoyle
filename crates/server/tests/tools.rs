//! Server tool behaviour over MCP, on an in-memory transport.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use gilfoyle_core::{
    AppError, Container, ContainerActionOutcome, ContainerFilter, ContainerStatus, DockerAdmin,
    JobResult, Package, PackageAdmin, PackageFilter, PackageList, RefreshReport, SystemAdmin,
    SystemError, SystemService, SystemdAdmin, SystemdError, Unit, UnitActionOutcome, UnitFilter,
    UnitName, UnitStatus, UnitTimestamps, UpdateFilter, UpdateList, UpgradePlan, UpgradeReport,
    Uptime, UptimeSource,
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

#[derive(Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

/// A scriptable stand-in for the Docker driving port.
#[derive(Clone, Default)]
struct FakeDocker {
    outcome: Option<ContainerActionOutcome>,
}

#[async_trait]
impl DockerAdmin for FakeDocker {
    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<Container>, AppError> {
        Ok(filter.apply(vec![Container {
            id: "abc".to_string(),
            names: vec!["web".to_string()],
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "Up 3 hours".to_string(),
        }]))
    }

    async fn container_status(&self, reference: &str) -> Result<ContainerStatus, AppError> {
        Ok(ContainerStatus {
            id: "abc".to_string(),
            name: reference.trim_start_matches('/').to_string(),
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "running".to_string(),
            health: Some("healthy".to_string()),
            pid: Some(7),
            exit_code: 0,
            restart_count: 1,
            created: "2024-05-01T12:00:00Z".to_string(),
            started_at: None,
            finished_at: None,
        })
    }

    async fn start_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.outcome
            .clone()
            .ok_or_else(|| AppError::ContainerNotManaged(reference.to_string()))
    }

    async fn stop_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.outcome
            .clone()
            .ok_or_else(|| AppError::ContainerNotManaged(reference.to_string()))
    }

    async fn restart_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.outcome
            .clone()
            .ok_or_else(|| AppError::ContainerNotManaged(reference.to_string()))
    }
}

async fn serve(
    systemd: FakeSystemd,
    docker: FakeDocker,
    enabled: Option<BTreeSet<String>>,
) -> RunningService<RoleClient, TestClient> {
    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(FixedSource(3661.0)));
    let handler = GilfoyleServer::new(
        admin,
        Arc::new(systemd),
        Arc::new(FakePackage),
        Arc::new(docker),
        enabled,
    );
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
async fn lists_every_tool_and_serves_uptime() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;
    assert_eq!(
        tool_names(&client).await,
        vec![
            "container_status".to_string(),
            "get_uptime".to_string(),
            "list_containers".to_string(),
            "list_packages".to_string(),
            "list_units".to_string(),
            "list_updates".to_string(),
            "preview_upgrade".to_string(),
            "refresh_package_cache".to_string(),
            "restart_container".to_string(),
            "start_container".to_string(),
            "start_unit".to_string(),
            "stop_container".to_string(),
            "stop_unit".to_string(),
            "unit_status".to_string(),
            "upgrade_packages".to_string(),
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
    let client = serve(
        FakeSystemd::default().with_unit("nginx.service"),
        FakeDocker::default(),
        None,
    )
    .await;

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
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;
    let result = client
        .peer()
        .call_tool(call("list_units", json!({ "unit_type": "nonsense" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn missing_unit_status_is_a_tool_error() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;
    let result = client
        .peer()
        .call_tool(call("unit_status", json!({ "name": "ghost" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn start_reports_the_job_and_a_failed_job_is_an_error() {
    let ok = serve(
        FakeSystemd::default().with_outcome("done"),
        FakeDocker::default(),
        None,
    )
    .await;
    let result = ok
        .peer()
        .call_tool(call("start_unit", json!({ "name": "nginx" })))
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["job_result"], "done");

    let failed = serve(
        FakeSystemd::default().with_outcome("failed"),
        FakeDocker::default(),
        None,
    )
    .await;
    let result = failed
        .peer()
        .call_tool(call("stop_unit", json!({ "name": "nginx" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn disabled_tools_are_hidden_and_rejected() {
    let client = serve(
        FakeSystemd::default(),
        FakeDocker::default(),
        Some(BTreeSet::new()),
    )
    .await;
    assert!(tool_names(&client).await.is_empty());

    let result = client.peer().call_tool(call("get_uptime", json!({}))).await;
    assert!(result.is_err(), "a disabled tool must be rejected");
}

#[tokio::test]
async fn lists_packages_and_updates() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;

    let listed = client
        .peer()
        .call_tool(call("list_packages", json!({})))
        .await
        .unwrap();
    let packages = listed.structured_content.unwrap();
    assert_eq!(packages["packages"][0]["name"], "nginx");
    assert_eq!(packages["packages"][0]["version"], "1.24.0");

    let updates = client
        .peer()
        .call_tool(call("list_updates", json!({})))
        .await
        .unwrap();
    assert_eq!(updates.structured_content.unwrap()["total"], 0);
}

#[tokio::test]
async fn a_bad_severity_is_a_tool_error() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;
    let result = client
        .peer()
        .call_tool(call("list_updates", json!({ "min_severity": "nonsense" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn previews_refreshes_and_upgrades_packages() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;

    let preview = client
        .peer()
        .call_tool(call("preview_upgrade", json!({})))
        .await
        .unwrap();
    assert_eq!(preview.structured_content.unwrap()["total"], 0);

    let refreshed = client
        .peer()
        .call_tool(call("refresh_package_cache", json!({ "force": false })))
        .await
        .unwrap();
    assert_eq!(refreshed.structured_content.unwrap()["runtime_ms"], 12);

    let upgraded = client
        .peer()
        .call_tool(call("upgrade_packages", json!({})))
        .await
        .unwrap();
    assert_eq!(upgraded.structured_content.unwrap()["success"], true);
}

#[tokio::test]
async fn lists_containers_and_reports_status() {
    let client = serve(FakeSystemd::default(), FakeDocker::default(), None).await;

    let listed = client
        .peer()
        .call_tool(call("list_containers", json!({})))
        .await
        .unwrap();
    let containers = listed.structured_content.unwrap();
    assert_eq!(containers["containers"][0]["names"][0], "web");
    assert_eq!(containers["containers"][0]["state"], "running");

    let status = client
        .peer()
        .call_tool(call("container_status", json!({ "container": "/web" })))
        .await
        .unwrap();
    assert_eq!(status.structured_content.unwrap()["health"], "healthy");

    let filtered = client
        .peer()
        .call_tool(call("list_containers", json!({ "state": "exited" })))
        .await
        .unwrap();
    assert_eq!(
        filtered.structured_content.unwrap()["containers"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let bad = client
        .peer()
        .call_tool(call("list_containers", json!({ "state": "nonsense" })))
        .await
        .unwrap();
    assert_eq!(bad.is_error, Some(true));
}

#[tokio::test]
async fn docker_mutations_report_the_outcome_and_refuse_unmanaged() {
    let outcome = ContainerActionOutcome {
        container: "web".to_string(),
        id: "abc".to_string(),
        action: "start".to_string(),
        state_before: "exited".to_string(),
        state_after: "running".to_string(),
    };
    let client = serve(
        FakeSystemd::default(),
        FakeDocker {
            outcome: Some(outcome),
        },
        None,
    )
    .await;
    let result = client
        .peer()
        .call_tool(call("start_container", json!({ "container": "web" })))
        .await
        .unwrap();
    let value = result.structured_content.unwrap();
    assert_eq!(value["action"], "start");
    assert_eq!(value["state_after"], "running");

    let denied = serve(FakeSystemd::default(), FakeDocker::default(), None).await;
    let result = denied
        .peer()
        .call_tool(call("restart_container", json!({ "container": "web" })))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}
