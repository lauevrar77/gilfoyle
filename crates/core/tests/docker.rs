use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gilfoyle_core::{
    AppError, Container, ContainerAction, ContainerAllowlist, ContainerCatalog, ContainerControl,
    ContainerFilter, ContainerIdentity, ContainerRef, ContainerState, ContainerStatus, DockerAdmin,
    DockerError, DockerService, DomainError,
};

// --- ContainerRef ---

#[test]
fn container_ref_accepts_names_ids_and_leading_slash() {
    assert_eq!(ContainerRef::parse("nginx").unwrap().as_str(), "nginx");
    assert_eq!(
        ContainerRef::parse("  web_1.api-2  ").unwrap().as_str(),
        "web_1.api-2"
    );
    assert_eq!(ContainerRef::parse("/web").unwrap().as_str(), "web");
    assert_eq!(
        ContainerRef::parse("deadbeefcafe").unwrap().as_str(),
        "deadbeefcafe"
    );
}

#[test]
fn container_ref_rejects_malformed() {
    for bad in [
        "", "   ", "/", "-web", ".web", "web!", "a b", "web/", "éweb",
    ] {
        assert!(
            matches!(
                ContainerRef::parse(bad),
                Err(DomainError::InvalidContainerRef(_))
            ),
            "expected {bad:?} to be invalid"
        );
    }
}

#[test]
fn container_ref_rejects_overlong() {
    let long = "a".repeat(256);
    assert!(ContainerRef::parse(&long).is_err());
    let ok = "a".repeat(255);
    assert!(ContainerRef::parse(&ok).is_ok());
}

// --- ContainerState ---

#[test]
fn container_state_round_trips() {
    for state in ContainerState::ALL {
        assert_eq!(ContainerState::parse(state.as_str()).unwrap(), state);
    }
    assert_eq!(ContainerState::ALL.len(), 7);
}

#[test]
fn container_state_parse_normalises_and_rejects() {
    assert_eq!(
        ContainerState::parse(" RUNNING ").unwrap(),
        ContainerState::Running
    );
    assert_eq!(
        ContainerState::parse("Exited").unwrap(),
        ContainerState::Exited
    );
    assert_eq!(
        ContainerState::parse("nope").unwrap_err(),
        DomainError::UnknownContainerState("nope".to_string())
    );
}

// --- filter ---

fn container(id: &str, names: &[&str], state: &str) -> Container {
    Container {
        id: id.to_string(),
        names: names.iter().map(|name| name.to_string()).collect(),
        image: "nginx:latest".to_string(),
        state: state.to_string(),
        status: "Up".to_string(),
    }
}

#[test]
fn container_filter_applies_state_name_and_limit() {
    let containers = vec![
        container("a", &["web"], "running"),
        container("b", &["Web-2"], "exited"),
        container("c", &["db"], "running"),
    ];

    let running = ContainerFilter {
        state: Some(ContainerState::Running),
        ..ContainerFilter::default()
    }
    .apply(containers.clone());
    assert_eq!(running.len(), 2);

    // Case-insensitive substring over any of the container's names.
    let web = ContainerFilter {
        name_contains: Some("WEB".to_string()),
        ..ContainerFilter::default()
    }
    .apply(containers.clone());
    assert_eq!(web.len(), 2);

    let limited = ContainerFilter {
        limit: Some(1),
        ..ContainerFilter::default()
    }
    .apply(containers);
    assert_eq!(limited.len(), 1);
}

// --- allowlist ---

#[test]
fn empty_allowlist_is_default_deny() {
    let allowlist = ContainerAllowlist::default();
    assert!(allowlist.is_empty());
    assert_eq!(
        allowlist.ensure_managed(&["web".to_string()]).unwrap_err(),
        AppError::ContainerNotManaged("web".to_string())
    );
}

#[test]
fn exact_entries_match_equality_only() {
    let allowlist = ContainerAllowlist::parse(["web".to_string()]).unwrap();
    assert!(!allowlist.is_empty());
    assert!(allowlist.contains("web"));
    assert!(!allowlist.contains("web-2"));
    assert!(!allowlist.contains("Web"));

    // A literal name containing a dot is not a regex.
    let dotted = ContainerAllowlist::parse(["my.app".to_string()]).unwrap();
    assert!(dotted.contains("my.app"));
    assert!(!dotted.contains("myXapp"));
}

#[test]
fn regex_entries_are_anchored_to_the_whole_name() {
    let allowlist =
        ContainerAllowlist::parse(["re:^worker-[0-9]+$".to_string(), "re:web".to_string()])
            .unwrap();
    assert!(allowlist.contains("worker-1"));
    assert!(allowlist.contains("worker-42"));
    assert!(!allowlist.contains("worker-"));
    assert!(!allowlist.contains("worker-x"));
    assert!(!allowlist.contains("xworker-1"));
    assert!(allowlist.contains("web"));
    assert!(!allowlist.contains("web-2"));

    assert_eq!(
        allowlist.patterns().collect::<Vec<_>>(),
        vec!["^worker-[0-9]+$", "web"]
    );
}

#[test]
fn allowlist_exposes_exact_names() {
    let allowlist = ContainerAllowlist::parse(["a".to_string(), "b".to_string()]).unwrap();
    assert_eq!(allowlist.exact_names().collect::<Vec<_>>(), vec!["a", "b"]);
}

#[test]
fn allowlist_rejects_invalid_entries() {
    assert_eq!(
        ContainerAllowlist::parse(["re:(".to_string()]).unwrap_err(),
        DomainError::InvalidContainerPattern("re:(".to_string())
    );
    assert_eq!(
        ContainerAllowlist::parse(["re:".to_string()]).unwrap_err(),
        DomainError::InvalidContainerPattern("re:".to_string())
    );
    assert_eq!(
        ContainerAllowlist::parse(["bad name".to_string()]).unwrap_err(),
        DomainError::InvalidContainerRef("bad name".to_string())
    );
}

#[test]
fn ensure_managed_accepts_any_alias() {
    let allowlist = ContainerAllowlist::parse(["web".to_string()]).unwrap();
    assert!(
        allowlist
            .ensure_managed(&["other".to_string(), "web".to_string()])
            .is_ok()
    );
    assert_eq!(
        allowlist
            .ensure_managed(&["other".to_string()])
            .unwrap_err(),
        AppError::ContainerNotManaged("other".to_string())
    );
}

#[test]
fn identity_canonical_name_falls_back_to_id() {
    let named = ContainerIdentity {
        id: "abc".to_string(),
        names: vec!["web".to_string(), "alias".to_string()],
        state: "running".to_string(),
    };
    assert_eq!(named.canonical_name(), "web");

    let unnamed = ContainerIdentity {
        id: "abc".to_string(),
        names: Vec::new(),
        state: "running".to_string(),
    };
    assert_eq!(unnamed.canonical_name(), "abc");
}

#[test]
fn container_action_names_are_closed() {
    assert_eq!(ContainerAction::Start.as_str(), "start");
    assert_eq!(ContainerAction::Stop.as_str(), "stop");
    assert_eq!(ContainerAction::Restart.as_str(), "restart");
    assert_eq!(ContainerAction::ALL.len(), 3);
}

// --- service ---

#[derive(Default)]
struct FakeCatalog {
    resolves: Mutex<Vec<ContainerIdentity>>,
    status: Mutex<Option<Result<ContainerStatus, DockerError>>>,
    calls: Mutex<Vec<String>>,
}

impl FakeCatalog {
    fn record(&self, call: &str) {
        self.calls.lock().unwrap().push(call.to_string());
    }
    fn push_resolve(&self, identity: ContainerIdentity) {
        self.resolves.lock().unwrap().push(identity);
    }
}

#[async_trait]
impl ContainerCatalog for FakeCatalog {
    async fn list_containers(
        &self,
        filter: &ContainerFilter,
    ) -> Result<Vec<Container>, DockerError> {
        self.record("list");
        Ok(filter.apply(vec![container("a", &["web"], "running")]))
    }

    async fn container_status(
        &self,
        reference: &ContainerRef,
    ) -> Result<ContainerStatus, DockerError> {
        self.record(&format!("status:{}", reference.as_str()));
        self.status
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Err(DockerError::NotFound(reference.as_str().to_string())))
    }

    async fn resolve(&self, reference: &ContainerRef) -> Result<ContainerIdentity, DockerError> {
        self.record(&format!("resolve:{}", reference.as_str()));
        let mut resolves = self.resolves.lock().unwrap();
        if resolves.is_empty() {
            return Err(DockerError::NotFound(reference.as_str().to_string()));
        }
        Ok(resolves.remove(0))
    }
}

#[derive(Default)]
struct FakeControl {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl ContainerControl for FakeControl {
    async fn start_container(&self, id: &str) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("start:{id}"));
        Ok(())
    }
    async fn stop_container(&self, id: &str) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("stop:{id}"));
        Ok(())
    }
    async fn restart_container(&self, id: &str) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("restart:{id}"));
        Ok(())
    }
}

fn service(catalog: Arc<FakeCatalog>, control: Arc<FakeControl>, allow: &[&str]) -> DockerService {
    let catalog: Arc<dyn ContainerCatalog> = catalog;
    let control: Arc<dyn ContainerControl> = control;
    DockerService::new(
        catalog,
        control,
        ContainerAllowlist::parse(allow.iter().map(|s| s.to_string())).unwrap(),
    )
}

fn identity(names: &[&str], state: &str) -> ContainerIdentity {
    ContainerIdentity {
        id: "deadbeef".to_string(),
        names: names.iter().map(|name| name.to_string()).collect(),
        state: state.to_string(),
    }
}

#[tokio::test]
async fn list_and_status_pass_through() {
    let catalog = Arc::new(FakeCatalog::default());
    let svc = service(catalog.clone(), Arc::new(FakeControl::default()), &[]);

    assert_eq!(
        svc.list_containers(&ContainerFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );

    *catalog.status.lock().unwrap() = Some(Ok(ContainerStatus {
        id: "a".to_string(),
        name: "web".to_string(),
        image: "nginx".to_string(),
        state: "running".to_string(),
        status: "running".to_string(),
        health: None,
        pid: Some(1),
        exit_code: 0,
        restart_count: 0,
        created: "2024-01-01T00:00:00Z".to_string(),
        started_at: None,
        finished_at: None,
    }));
    let status = svc.container_status("/web").await.unwrap();
    assert_eq!(status.name, "web");
    assert_eq!(
        catalog.calls.lock().unwrap().clone(),
        vec!["list", "status:web"]
    );
}

#[tokio::test]
async fn start_resolves_checks_allowlist_and_acts_by_id() {
    let catalog = Arc::new(FakeCatalog::default());
    catalog.push_resolve(identity(&["web", "alias"], "exited"));
    catalog.push_resolve(identity(&["web"], "running"));
    let control = Arc::new(FakeControl::default());
    let svc = service(catalog.clone(), control.clone(), &["web"]);

    let outcome = svc.start_container("/web").await.unwrap();
    assert_eq!(outcome.container, "web");
    assert_eq!(outcome.id, "deadbeef");
    assert_eq!(outcome.action, "start");
    assert_eq!(outcome.state_before, "exited");
    assert_eq!(outcome.state_after, "running");
    assert_eq!(
        control.calls.lock().unwrap().clone(),
        vec!["start:deadbeef"]
    );
    assert_eq!(
        catalog.calls.lock().unwrap().clone(),
        vec!["resolve:web", "resolve:deadbeef"]
    );
}

#[tokio::test]
async fn stop_and_restart_map_to_their_control_methods() {
    for (action, expected) in [
        (ContainerAction::Stop, "stop:deadbeef"),
        (ContainerAction::Restart, "restart:deadbeef"),
    ] {
        let catalog = Arc::new(FakeCatalog::default());
        catalog.push_resolve(identity(&["web"], "running"));
        catalog.push_resolve(identity(&["web"], "running"));
        let control = Arc::new(FakeControl::default());
        let svc = service(catalog, control.clone(), &["web"]);
        let outcome = match action {
            ContainerAction::Stop => svc.stop_container("web").await.unwrap(),
            ContainerAction::Restart => svc.restart_container("web").await.unwrap(),
            ContainerAction::Start => unreachable!(),
        };
        assert_eq!(outcome.action, action.as_str());
        assert_eq!(control.calls.lock().unwrap().clone(), vec![expected]);
    }
}

#[tokio::test]
async fn mutation_is_refused_when_not_managed() {
    let catalog = Arc::new(FakeCatalog::default());
    catalog.push_resolve(identity(&["web"], "exited"));
    let control = Arc::new(FakeControl::default());
    let svc = service(catalog, control.clone(), &["db"]);

    assert_eq!(
        svc.stop_container("web").await.unwrap_err(),
        AppError::ContainerNotManaged("web".to_string())
    );
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mutation_is_refused_on_an_empty_allowlist() {
    let catalog = Arc::new(FakeCatalog::default());
    catalog.push_resolve(identity(&["web"], "running"));
    let control = Arc::new(FakeControl::default());
    let svc = service(catalog, control.clone(), &[]);
    assert!(matches!(
        svc.restart_container("web").await.unwrap_err(),
        AppError::ContainerNotManaged(_)
    ));
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_invalid_reference_is_a_domain_error() {
    let svc = service(
        Arc::new(FakeCatalog::default()),
        Arc::new(FakeControl::default()),
        &["web"],
    );
    assert!(matches!(
        svc.start_container("bad name").await.unwrap_err(),
        AppError::Domain(DomainError::InvalidContainerRef(_))
    ));
}

#[tokio::test]
async fn a_missing_container_surfaces_not_found() {
    let svc = service(
        Arc::new(FakeCatalog::default()),
        Arc::new(FakeControl::default()),
        &["web"],
    );
    assert_eq!(
        svc.stop_container("web").await.unwrap_err(),
        AppError::Docker(DockerError::NotFound("web".to_string()))
    );
}
