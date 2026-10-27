use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gilfoyle_core::{
    Allowlist, AppError, DomainError, JobResult, ServiceCatalog, ServiceControl, SystemdAdmin,
    SystemdError, SystemdService, Unit, UnitActionOutcome, UnitFilter, UnitName, UnitStatus,
    UnitTimestamps, UnitType, micros_to_rfc3339,
};

#[test]
fn unit_type_suffixes_round_trip() {
    for unit_type in UnitType::ALL {
        assert_eq!(UnitType::from_suffix(unit_type.suffix()), Some(unit_type));
        assert_eq!(
            UnitType::from_suffix(&format!(".{}", unit_type.suffix())),
            Some(unit_type)
        );
    }
    assert_eq!(UnitType::ALL.len(), 11);
}

#[test]
fn unit_type_parse_accepts_dot_and_rejects_unknown() {
    assert_eq!(UnitType::parse("service").unwrap(), UnitType::Service);
    assert_eq!(UnitType::parse(".timer").unwrap(), UnitType::Timer);
    assert_eq!(UnitType::parse("  socket ").unwrap(), UnitType::Socket);
    assert_eq!(
        UnitType::parse("nonsense").unwrap_err(),
        DomainError::UnknownUnitType("nonsense".to_string())
    );
}

#[test]
fn unit_name_normalises_short_names() {
    assert_eq!(UnitName::parse("nginx").unwrap().as_str(), "nginx.service");
    assert_eq!(
        UnitName::parse("nginx").unwrap().unit_type(),
        UnitType::Service
    );
    assert_eq!(
        UnitName::parse("  nginx  ").unwrap().as_str(),
        "nginx.service"
    );
}

#[test]
fn unit_name_keeps_known_suffixes() {
    let name = UnitName::parse("sshd.socket").unwrap();
    assert_eq!(name.as_str(), "sshd.socket");
    assert_eq!(name.unit_type(), UnitType::Socket);
}

#[test]
fn unit_name_keeps_dotted_prefixes() {
    let name = UnitName::parse("foo.bar.service").unwrap();
    assert_eq!(name.as_str(), "foo.bar.service");
    assert_eq!(name.unit_type(), UnitType::Service);
}

#[test]
fn unit_name_rejects_unknown_suffix() {
    assert_eq!(
        UnitName::parse("foo.bar").unwrap_err(),
        DomainError::UnknownUnitSuffix("foo.bar".to_string())
    );
}

#[test]
fn unit_name_rejects_empty_and_invalid() {
    for bad in ["", "   ", "a b", "a/b", "a$b", ".service", "foo@.service"] {
        assert!(
            matches!(UnitName::parse(bad), Err(DomainError::InvalidUnitName(_))),
            "expected {bad:?} to be invalid"
        );
    }
}

#[test]
fn unit_name_rejects_overlong_names() {
    // 256 characters is the maximum systemd accepts; 257 is one too many.
    assert!(UnitName::parse(&"a".repeat(256)).is_ok());
    let long = "a".repeat(257);
    assert!(matches!(
        UnitName::parse(&long),
        Err(DomainError::InvalidUnitName(_))
    ));
}

#[test]
fn micros_to_rfc3339_converts_and_handles_unset() {
    assert_eq!(micros_to_rfc3339(0), None);
    assert_eq!(
        micros_to_rfc3339(1_000_000),
        Some("1970-01-01T00:00:01Z".to_string())
    );
    assert_eq!(
        micros_to_rfc3339(1_698_000_000_000_000),
        Some("2023-10-22T18:40:00Z".to_string())
    );
    assert_eq!(micros_to_rfc3339(u64::MAX), None);
}

#[test]
fn allowlist_membership() {
    let allowlist = Allowlist::new(["nginx.service".to_string()]);
    assert!(!allowlist.is_empty());
    assert!(allowlist.contains(&UnitName::parse("nginx").unwrap()));
    assert!(!allowlist.contains(&UnitName::parse("sshd").unwrap()));
    assert!(
        allowlist
            .ensure_managed(&UnitName::parse("nginx").unwrap())
            .is_ok()
    );
    assert_eq!(
        allowlist
            .ensure_managed(&UnitName::parse("sshd").unwrap())
            .unwrap_err(),
        AppError::NotManaged("sshd.service".to_string())
    );

    let empty = Allowlist::default();
    assert!(empty.is_empty());
    assert_eq!(
        empty
            .ensure_managed(&UnitName::parse("nginx").unwrap())
            .unwrap_err(),
        AppError::NotManaged("nginx.service".to_string())
    );
}

#[test]
fn allowlist_names_are_sorted_and_complete() {
    let allowlist = Allowlist::new([
        "sshd.socket".to_string(),
        "nginx.service".to_string(),
        "nginx.service".to_string(),
    ]);
    assert_eq!(
        allowlist.names().collect::<Vec<_>>(),
        vec!["nginx.service", "sshd.socket"]
    );
}

#[test]
fn job_result_success_is_only_done() {
    assert!(JobResult::new("done").is_success());
    for other in ["failed", "canceled", "timeout", "dependency", "skipped"] {
        assert!(!JobResult::new(other).is_success());
    }
}

fn outcome(unit: &str) -> UnitActionOutcome {
    UnitActionOutcome {
        unit: unit.to_string(),
        job_result: JobResult::new("done"),
        active_state_before: "inactive".to_string(),
        active_state_after: "active".to_string(),
    }
}

fn status(name: &str) -> UnitStatus {
    UnitStatus {
        name: name.to_string(),
        description: "a unit".to_string(),
        load_state: "loaded".to_string(),
        active_state: "active".to_string(),
        sub_state: "running".to_string(),
        unit_file_state: Some("enabled".to_string()),
        can_start: Some(true),
        can_stop: Some(true),
        main_pid: Some(42),
        result: Some("success".to_string()),
        timestamps: UnitTimestamps {
            state_change: None,
            active_enter: None,
            inactive_enter: None,
            exec_main_start: None,
            exec_main_exit: None,
        },
    }
}

#[derive(Default)]
struct FakeCatalog {
    units: Vec<Unit>,
    status: Option<UnitStatus>,
}

#[async_trait]
impl ServiceCatalog for FakeCatalog {
    async fn list_units(&self, _filter: &UnitFilter) -> Result<Vec<Unit>, SystemdError> {
        Ok(self.units.clone())
    }

    async fn unit_status(&self, name: &UnitName) -> Result<UnitStatus, SystemdError> {
        match &self.status {
            Some(status) => Ok(status.clone()),
            None => Err(SystemdError::NotFound(name.as_str().to_string())),
        }
    }
}

struct FakeControl {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl ServiceControl for FakeControl {
    async fn start_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("start:{}", name.as_str()));
        Ok(outcome(name.as_str()))
    }

    async fn stop_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("stop:{}", name.as_str()));
        Ok(outcome(name.as_str()))
    }
}

fn build(catalog: FakeCatalog, control: Arc<FakeControl>, allowlist: &[&str]) -> SystemdService {
    let catalog: Arc<dyn ServiceCatalog> = Arc::new(catalog);
    let control: Arc<dyn ServiceControl> = control;
    SystemdService::new(
        catalog,
        control,
        Allowlist::new(allowlist.iter().map(|s| s.to_string())),
    )
}

#[tokio::test]
async fn lists_units_through_the_catalog() {
    let catalog = FakeCatalog {
        units: vec![Unit {
            name: "nginx.service".to_string(),
            description: "nginx".to_string(),
            load_state: "loaded".to_string(),
            active_state: "active".to_string(),
            sub_state: "running".to_string(),
        }],
        status: None,
    };
    let service = build(
        catalog,
        Arc::new(FakeControl {
            calls: Mutex::new(vec![]),
        }),
        &[],
    );
    let units = service.list_units(&UnitFilter::default()).await.unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].name, "nginx.service");
}

#[tokio::test]
async fn status_normalises_the_name() {
    let catalog = FakeCatalog {
        units: vec![],
        status: Some(status("nginx.service")),
    };
    let service = build(
        catalog,
        Arc::new(FakeControl {
            calls: Mutex::new(vec![]),
        }),
        &[],
    );
    let status = service.unit_status("nginx").await.unwrap();
    assert_eq!(status.name, "nginx.service");
}

#[tokio::test]
async fn status_propagates_not_found() {
    let service = build(
        FakeCatalog::default(),
        Arc::new(FakeControl {
            calls: Mutex::new(vec![]),
        }),
        &[],
    );
    assert_eq!(
        service.unit_status("ghost").await.unwrap_err(),
        AppError::Systemd(SystemdError::NotFound("ghost.service".to_string()))
    );
}

#[tokio::test]
async fn status_rejects_invalid_names() {
    let service = build(
        FakeCatalog::default(),
        Arc::new(FakeControl {
            calls: Mutex::new(vec![]),
        }),
        &[],
    );
    assert!(matches!(
        service.unit_status("bad name").await.unwrap_err(),
        AppError::Domain(DomainError::InvalidUnitName(_))
    ));
}

#[tokio::test]
async fn start_and_stop_reach_a_managed_unit() {
    let control = Arc::new(FakeControl {
        calls: Mutex::new(vec![]),
    });
    let service = build(FakeCatalog::default(), control.clone(), &["nginx.service"]);

    let started = service.start_unit("nginx").await.unwrap();
    assert_eq!(started.unit, "nginx.service");
    let stopped = service.stop_unit("nginx.service").await.unwrap();
    assert_eq!(stopped.unit, "nginx.service");

    assert_eq!(
        control.calls.lock().unwrap().clone(),
        vec!["start:nginx.service", "stop:nginx.service"]
    );
}

#[tokio::test]
async fn start_and_stop_refuse_an_unmanaged_unit() {
    let control = Arc::new(FakeControl {
        calls: Mutex::new(vec![]),
    });
    let service = build(FakeCatalog::default(), control.clone(), &["nginx.service"]);

    assert_eq!(
        service.start_unit("sshd").await.unwrap_err(),
        AppError::NotManaged("sshd.service".to_string())
    );
    assert_eq!(
        service.stop_unit("sshd").await.unwrap_err(),
        AppError::NotManaged("sshd.service".to_string())
    );
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mutations_reject_invalid_names_before_the_allowlist() {
    let control = Arc::new(FakeControl {
        calls: Mutex::new(vec![]),
    });
    let service = build(FakeCatalog::default(), control.clone(), &["nginx.service"]);
    assert!(matches!(
        service.start_unit("foo.bar").await.unwrap_err(),
        AppError::Domain(DomainError::UnknownUnitSuffix(_))
    ));
    assert!(control.calls.lock().unwrap().is_empty());
}
