//! Integration tests for the systemd adapter against an in-process mock bus.
//!
//! The mock is served over a peer-to-peer D-Bus socket pair: no daemon, no process.

use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gilfoyle_core::{ServiceCatalog, ServiceControl, SystemdError, UnitFilter, UnitName, UnitType};
use gilfoyle_systemd::{Connector, LazySystemd, SystemdAdapter};
use zbus::object_server::SignalEmitter;
use zbus::{Connection, Guid, OwnedObjectPath, connection::Builder};

type RawUnit = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.systemd1")]
enum MockError {
    NoSuchUnit,
    OnlyByDependency,
}

#[derive(Clone)]
struct SharedUnit {
    name: String,
    description: String,
    active_state: Arc<Mutex<String>>,
    sub_state: String,
    can_start: bool,
    can_stop: bool,
    main_pid: u32,
    result: String,
    is_service: bool,
    path: String,
}

impl SharedUnit {
    fn service(name: &str, active: &str) -> Self {
        Self {
            name: name.to_string(),
            description: format!("mock {name}"),
            active_state: Arc::new(Mutex::new(active.to_string())),
            sub_state: "running".to_string(),
            can_start: true,
            can_stop: true,
            main_pid: 4321,
            result: "success".to_string(),
            is_service: true,
            path: format!(
                "/org/freedesktop/systemd1/unit/{}",
                name.replace('.', "_2e")
            ),
        }
    }

    fn socket(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: format!("mock {name}"),
            active_state: Arc::new(Mutex::new("active".to_string())),
            sub_state: "listening".to_string(),
            can_start: false,
            can_stop: false,
            main_pid: 0,
            result: String::new(),
            is_service: false,
            path: format!(
                "/org/freedesktop/systemd1/unit/{}",
                name.replace('.', "_2e")
            ),
        }
    }

    fn tuple(&self) -> RawUnit {
        (
            self.name.clone(),
            self.description.clone(),
            "loaded".to_string(),
            self.active_state.lock().unwrap().clone(),
            self.sub_state.clone(),
            String::new(),
            OwnedObjectPath::try_from(self.path.as_str()).unwrap(),
            0,
            String::new(),
            OwnedObjectPath::try_from("/").unwrap(),
        )
    }
}

struct MockUnit {
    unit: SharedUnit,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl MockUnit {
    #[zbus(property)]
    fn description(&self) -> String {
        self.unit.description.clone()
    }

    #[zbus(property)]
    fn load_state(&self) -> String {
        "loaded".to_string()
    }

    #[zbus(property)]
    fn active_state(&self) -> String {
        self.unit.active_state.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn sub_state(&self) -> String {
        self.unit.sub_state.clone()
    }

    #[zbus(property)]
    fn unit_file_state(&self) -> String {
        "enabled".to_string()
    }

    #[zbus(property)]
    fn can_start(&self) -> bool {
        self.unit.can_start
    }

    #[zbus(property)]
    fn can_stop(&self) -> bool {
        self.unit.can_stop
    }

    #[zbus(property)]
    fn state_change_timestamp(&self) -> u64 {
        1_000_000
    }

    #[zbus(property)]
    fn active_enter_timestamp(&self) -> u64 {
        2_000_000
    }

    #[zbus(property)]
    fn inactive_enter_timestamp(&self) -> u64 {
        0
    }
}

struct MockService {
    unit: SharedUnit,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl MockService {
    #[zbus(property, name = "MainPID")]
    fn main_pid(&self) -> u32 {
        self.unit.main_pid
    }

    #[zbus(property)]
    fn result(&self) -> String {
        self.unit.result.clone()
    }

    #[zbus(property)]
    fn exec_main_start_timestamp(&self) -> u64 {
        3_000_000
    }

    #[zbus(property)]
    fn exec_main_exit_timestamp(&self) -> u64 {
        0
    }
}

struct MockManager {
    units: Vec<SharedUnit>,
    deny_start: bool,
    hang: bool,
}

impl MockManager {
    fn find(&self, name: &str) -> Option<&SharedUnit> {
        self.units.iter().find(|unit| unit.name == name)
    }

    async fn run_job(
        &self,
        name: &str,
        start: bool,
        emitter: SignalEmitter<'_>,
    ) -> Result<OwnedObjectPath, MockError> {
        if self.hang {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let Some(unit) = self.find(name) else {
            return Err(MockError::NoSuchUnit);
        };
        if start && self.deny_start {
            return Err(MockError::OnlyByDependency);
        }
        let job = OwnedObjectPath::try_from("/org/freedesktop/systemd1/job/1").unwrap();
        *unit.active_state.lock().unwrap() = if start { "active" } else { "inactive" }.to_string();
        let result = if start { "done" } else { "failed" };
        emitter
            .job_removed(1, &job, name, result)
            .await
            .expect("emit JobRemoved");
        Ok(job)
    }
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl MockManager {
    async fn list_units(&self) -> Result<Vec<RawUnit>, MockError> {
        if self.hang {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Ok(self.units.iter().map(SharedUnit::tuple).collect())
    }

    async fn list_units_filtered(&self, states: Vec<String>) -> Result<Vec<RawUnit>, MockError> {
        Ok(self
            .units
            .iter()
            .filter(|unit| states.contains(&unit.active_state.lock().unwrap().clone()))
            .map(SharedUnit::tuple)
            .collect())
    }

    async fn get_unit(&self, name: String) -> Result<OwnedObjectPath, MockError> {
        match self.find(&name) {
            Some(unit) => Ok(OwnedObjectPath::try_from(unit.path.as_str()).unwrap()),
            None => Err(MockError::NoSuchUnit),
        }
    }

    async fn get_unit_file_state(&self, _file: String) -> Result<String, MockError> {
        Ok("enabled".to_string())
    }

    async fn start_unit(
        &self,
        name: String,
        _mode: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<OwnedObjectPath, MockError> {
        self.run_job(&name, true, emitter).await
    }

    async fn stop_unit(
        &self,
        name: String,
        _mode: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<OwnedObjectPath, MockError> {
        self.run_job(&name, false, emitter).await
    }

    async fn subscribe(&self) -> Result<(), MockError> {
        Ok(())
    }

    #[zbus(signal)]
    async fn job_removed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        job: &OwnedObjectPath,
        unit: &str,
        result: &str,
    ) -> zbus::Result<()>;
}

fn fixtures() -> Vec<SharedUnit> {
    vec![
        SharedUnit::service("nginx.service", "inactive"),
        SharedUnit::socket("sshd.socket"),
    ]
}

async fn mock_server(
    units: Vec<SharedUnit>,
    deny_start: bool,
    hang: bool,
) -> (Connection, Connection) {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let guid = Guid::generate();

    let mut builder = Builder::unix_stream(server_stream).server(guid).p2p();
    builder = builder.serve_at(
        "/org/freedesktop/systemd1",
        MockManager {
            units: units.clone(),
            deny_start,
            hang,
        },
    );
    for unit in &units {
        builder = builder.serve_at(unit.path.as_str(), MockUnit { unit: unit.clone() });
        if unit.is_service {
            builder = builder.serve_at(unit.path.as_str(), MockService { unit: unit.clone() });
        }
    }
    // The P2P handshake is symmetric: both ends must be built concurrently.
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server, client) = tokio::join!(builder.build(), client_builder.build());
    let server = server.unwrap();
    let client = client.unwrap();
    (client, server)
}

async fn connect(
    units: Vec<SharedUnit>,
    deny_start: bool,
    hang: bool,
) -> (SystemdAdapter, Connection) {
    let (client, server) = mock_server(units, deny_start, hang).await;
    let adapter =
        SystemdAdapter::new(client).with_timeouts(Duration::from_secs(2), Duration::from_secs(2));
    (adapter, server)
}

#[tokio::test]
async fn lists_all_units() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let units = adapter.list_units(&UnitFilter::default()).await.unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].name, "nginx.service");
    assert_eq!(units[1].name, "sshd.socket");
}

#[tokio::test]
async fn filters_units_by_type_and_name() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let filter = UnitFilter {
        unit_type: Some(UnitType::Socket),
        name_contains: Some("SSH".to_string()),
        ..UnitFilter::default()
    };
    let units = adapter.list_units(&filter).await.unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].name, "sshd.socket");
}

#[tokio::test]
async fn filters_units_by_active_state_server_side() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let filter = UnitFilter {
        active_state: Some("active".to_string()),
        ..UnitFilter::default()
    };
    let units = adapter.list_units(&filter).await.unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].name, "sshd.socket");
}

#[tokio::test]
async fn reports_service_status() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let status = adapter
        .unit_status(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(status.name, "nginx.service");
    assert_eq!(status.active_state, "inactive");
    assert_eq!(status.unit_file_state, Some("enabled".to_string()));
    assert_eq!(status.can_start, Some(true));
    assert_eq!(status.main_pid, Some(4321));
    assert_eq!(status.result, Some("success".to_string()));
    assert_eq!(
        status.timestamps.state_change,
        Some("1970-01-01T00:00:01Z".to_string())
    );
    assert_eq!(status.timestamps.inactive_enter, None);
    assert_eq!(
        status.timestamps.exec_main_start,
        Some("1970-01-01T00:00:03Z".to_string())
    );
}

#[tokio::test]
async fn reports_non_service_status_without_service_fields() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let status = adapter
        .unit_status(&UnitName::parse("sshd.socket").unwrap())
        .await
        .unwrap();
    assert_eq!(status.main_pid, None);
    assert_eq!(status.result, None);
    assert_eq!(status.timestamps.exec_main_start, None);
}

#[tokio::test]
async fn unknown_unit_reports_not_found() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    assert_eq!(
        adapter
            .unit_status(&UnitName::parse("ghost.service").unwrap())
            .await
            .unwrap_err(),
        SystemdError::NotFound("ghost.service".to_string())
    );
}

#[tokio::test]
async fn starts_a_unit_and_waits_for_the_job() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let outcome = adapter
        .start_unit(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(outcome.unit, "nginx.service");
    assert_eq!(outcome.job_result.result, "done");
    assert_eq!(outcome.active_state_before, "inactive");
    assert_eq!(outcome.active_state_after, "active");
}

#[tokio::test]
async fn stops_a_unit_and_reports_a_failed_job() {
    let (adapter, _server) = connect(fixtures(), false, false).await;
    let outcome = adapter
        .stop_unit(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(outcome.job_result.result, "failed");
    assert!(!outcome.job_result.is_success());
    assert_eq!(outcome.active_state_after, "inactive");
}

#[tokio::test]
async fn maps_a_refused_start_to_only_by_dependency() {
    let (adapter, _server) = connect(fixtures(), true, false).await;
    assert_eq!(
        adapter
            .start_unit(&UnitName::parse("nginx.service").unwrap())
            .await
            .unwrap_err(),
        SystemdError::OnlyByDependency("nginx.service".to_string())
    );
}

fn counting_connector(client: Connection, calls: Arc<AtomicUsize>) -> Connector {
    Arc::new(move || {
        let client = client.clone();
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(SystemdAdapter::new(client))
        })
    })
}

#[tokio::test]
async fn lazy_connects_once_and_delegates() {
    let (client, _server) = mock_server(fixtures(), false, false).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let lazy = LazySystemd::new(counting_connector(client, calls.clone()));

    assert_eq!(
        lazy.list_units(&UnitFilter::default()).await.unwrap().len(),
        2
    );
    let status = lazy
        .unit_status(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(status.name, "nginx.service");
    let started = lazy
        .start_unit(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(started.job_result.result, "done");
    let stopped = lazy
        .stop_unit(&UnitName::parse("nginx.service").unwrap())
        .await
        .unwrap();
    assert_eq!(stopped.job_result.result, "failed");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lazy_reports_a_connection_failure() {
    let connector: Connector =
        Arc::new(|| Box::pin(async { Err(SystemdError::Unavailable("no bus".to_string())) }));
    let lazy = LazySystemd::new(connector);
    assert_eq!(
        lazy.list_units(&UnitFilter::default()).await.unwrap_err(),
        SystemdError::Unavailable("no bus".to_string())
    );
}

#[tokio::test]
async fn times_out_when_systemd_hangs() {
    let (adapter, _server) = connect(fixtures(), false, true).await;
    let adapter = adapter.with_timeouts(Duration::from_millis(50), Duration::from_millis(50));
    assert_eq!(
        adapter
            .list_units(&UnitFilter::default())
            .await
            .unwrap_err(),
        SystemdError::Timeout(String::new())
    );
}
