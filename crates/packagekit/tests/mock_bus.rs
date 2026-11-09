//! Integration tests for the PackageKit adapter against an in-process mock bus.
//!
//! The mock is served over a peer-to-peer D-Bus socket pair: no daemon, no
//! process, no real package manager.

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use gilfoyle_core::{
    PackageError, PackageFilter, PackageManager, UpdateFilter, pk_error, pk_exit, pk_info,
    pk_restart,
};
use gilfoyle_packagekit::{Connector, LazyPackagekit, PackagekitAdapter};
use zbus::object_server::SignalEmitter;
use zbus::{Connection, Guid, OwnedObjectPath, connection::Builder};

const ROOT_PATH: &str = "/org/freedesktop/PackageKit";
const TX_PATH: &str = "/org/freedesktop/PackageKit/transactions/1";

/// The error type of the mock interface.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PackageKit")]
enum MockError {
    Emission,
}

/// The mutable state a mock transaction reads.
#[derive(Clone)]
struct MockState {
    major: u32,
    minor: u32,
    /// Packages returned by `GetPackages`/`GetUpdates`.
    packages: Vec<(u32, String, String)>,
    /// Packages returned by `UpdatePackages`.
    plan: Vec<(u32, String, String)>,
    /// Repositories returned by `RefreshCache`.
    repos: Vec<(String, String, bool)>,
    /// The `CanAuthorize` result.
    authorize: u32,
    /// An error code to emit before finishing.
    error: Option<(u32, String)>,
    /// A `RepoSignatureRequired` signal (untrusted).
    untrusted: bool,
    /// A `EulaRequired` signal.
    eula: bool,
    /// A `RequireRestart(system)` signal.
    restart: bool,
    /// The exit code to finish with.
    exit: u32,
    /// Whether to hang before emitting (to exercise the timeout).
    hang: bool,
    /// How many times to report the database as locked.
    lock_times: u32,
    /// How many transactions have finished so far.
    finishes: Arc<AtomicU32>,
}

impl MockState {
    fn base() -> Self {
        Self {
            major: 1,
            minor: 6,
            packages: Vec::new(),
            plan: Vec::new(),
            repos: Vec::new(),
            authorize: 1,
            error: None,
            untrusted: false,
            eula: false,
            restart: false,
            exit: pk_exit::SUCCESS,
            hang: false,
            lock_times: 0,
            finishes: Arc::new(AtomicU32::new(0)),
        }
    }

    fn with_package(mut self, info: u32, id: &str) -> Self {
        self.packages
            .push((info, id.to_string(), "summary".to_string()));
        self
    }

    fn batched(&self) -> bool {
        self.minor >= 6
    }
}

struct MockRoot {
    state: Arc<MockState>,
}

#[zbus::interface(name = "org.freedesktop.PackageKit")]
impl MockRoot {
    #[zbus(property)]
    fn version_major(&self) -> u32 {
        self.state.major
    }

    #[zbus(property)]
    fn version_minor(&self) -> u32 {
        self.state.minor
    }

    async fn create_transaction(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(TX_PATH).unwrap()
    }

    async fn can_authorize(&self, _action_id: String) -> u32 {
        self.state.authorize
    }
}

struct MockTransaction {
    state: Arc<MockState>,
}

impl MockTransaction {
    async fn finish(&self, emitter: &SignalEmitter<'_>) -> Result<(), MockError> {
        let attempt = self.state.finishes.fetch_add(1, Ordering::SeqCst);
        if attempt < self.state.lock_times {
            Self::error_code(emitter, pk_error::CANNOT_GET_LOCK, "locked")
                .await
                .map_err(|_| MockError::Emission)?;
        }
        if let Some((code, details)) = &self.state.error {
            Self::error_code(emitter, *code, details)
                .await
                .map_err(|_| MockError::Emission)?;
        }
        if self.state.untrusted {
            Self::repo_signature_required(emitter, "pkg;1;amd64;repo;")
                .await
                .map_err(|_| MockError::Emission)?;
        }
        if self.state.eula {
            Self::eula_required(emitter, "eula", "pkg;1;amd64;r;", "vendor", "licence")
                .await
                .map_err(|_| MockError::Emission)?;
        }
        if self.state.restart {
            Self::require_restart(emitter, pk_restart::SYSTEM, "pkg;1;amd64;r;")
                .await
                .map_err(|_| MockError::Emission)?;
        }
        Self::finished(emitter, self.state.exit, 1)
            .await
            .map_err(|_| MockError::Emission)
    }

    async fn emit(
        &self,
        emitter: &SignalEmitter<'_>,
        packages: &[(u32, String, String)],
    ) -> Result<(), MockError> {
        if self.state.batched() {
            Self::packages(emitter, packages.to_vec())
                .await
                .map_err(|_| MockError::Emission)?;
        } else {
            for (info, id, summary) in packages {
                Self::package(emitter, *info, id, summary)
                    .await
                    .map_err(|_| MockError::Emission)?;
            }
        }
        Ok(())
    }

    async fn run(
        &self,
        emitter: &SignalEmitter<'_>,
        packages: &[(u32, String, String)],
    ) -> Result<(), MockError> {
        if self.state.hang {
            // Return without emitting anything: the client must time out.
            return Ok(());
        }
        self.emit(emitter, packages).await?;
        self.finish(emitter).await
    }
}

#[zbus::interface(name = "org.freedesktop.PackageKit.Transaction")]
impl MockTransaction {
    async fn get_packages(
        &self,
        _filter: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), MockError> {
        self.run(&emitter, &self.state.packages.clone()).await
    }

    async fn get_updates(
        &self,
        _filter: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), MockError> {
        self.run(&emitter, &self.state.packages.clone()).await
    }

    async fn refresh_cache(
        &self,
        _force: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), MockError> {
        if self.state.hang {
            return Ok(());
        }
        for (id, description, enabled) in &self.state.repos {
            Self::repo_detail(&emitter, id, description, *enabled)
                .await
                .map_err(|_| MockError::Emission)?;
        }
        self.finish(&emitter).await
    }

    async fn update_packages(
        &self,
        _flags: u64,
        _package_ids: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), MockError> {
        self.run(&emitter, &self.state.plan.clone()).await
    }

    async fn cancel(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), MockError> {
        Self::finished(&emitter, pk_exit::CANCELLED, 0)
            .await
            .map_err(|_| MockError::Emission)
    }

    #[zbus(signal)]
    async fn package(
        emitter: &SignalEmitter<'_>,
        info: u32,
        package_id: &str,
        summary: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn packages(
        emitter: &SignalEmitter<'_>,
        packages: Vec<(u32, String, String)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn finished(emitter: &SignalEmitter<'_>, exit: u32, runtime: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn error_code(emitter: &SignalEmitter<'_>, code: u32, details: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn repo_detail(
        emitter: &SignalEmitter<'_>,
        repo_id: &str,
        description: &str,
        enabled: bool,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn require_restart(
        emitter: &SignalEmitter<'_>,
        kind: u32,
        package_id: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn repo_signature_required(
        emitter: &SignalEmitter<'_>,
        package_id: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn eula_required(
        emitter: &SignalEmitter<'_>,
        eula_id: &str,
        package_id: &str,
        vendor_name: &str,
        license_agreement: &str,
    ) -> zbus::Result<()>;
}

/// Hold the mock server connection alive for the duration of the test process.
fn keep_alive(server: Connection) {
    tokio::spawn(async move {
        let _server = server;
        std::future::pending::<()>().await;
    });
}

/// Build an adapter connected to a fresh mock.
async fn connect(state: MockState) -> Result<PackagekitAdapter, PackageError> {
    let state = Arc::new(state);
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let guid = Guid::generate();

    let builder = Builder::unix_stream(server_stream).server(guid).p2p();
    let builder = builder.serve_at(
        ROOT_PATH,
        MockRoot {
            state: state.clone(),
        },
    );
    let builder = builder.serve_at(
        TX_PATH,
        MockTransaction {
            state: state.clone(),
        },
    );
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server, client) = tokio::join!(builder.build(), client_builder.build());
    keep_alive(server.unwrap());
    let adapter = PackagekitAdapter::new(client.unwrap()).await?.with_limits(
        Duration::from_secs(2),
        Duration::from_secs(2),
        100,
    );
    Ok(adapter)
}

#[tokio::test]
async fn lists_installed_packages_batched() {
    let adapter =
        connect(MockState::base().with_package(pk_info::INSTALLED, "nginx;1.24.0;amd64;debian;"))
            .await
            .unwrap();
    let list = adapter
        .list_installed(&PackageFilter::default())
        .await
        .unwrap();
    assert_eq!(list.total, 1);
    assert_eq!(list.packages[0].name, "nginx");
    assert_eq!(list.packages[0].version, "1.24.0");
}

#[tokio::test]
async fn lists_installed_packages_singular_signal() {
    // PackageKit <= 1.2.5 emits the singular `Package` signal.
    let mut state = MockState::base();
    state.minor = 2;
    let adapter = connect(state.with_package(pk_info::INSTALLED, "curl;8;amd64;debian;"))
        .await
        .unwrap();
    let list = adapter
        .list_installed(&PackageFilter::default())
        .await
        .unwrap();
    assert_eq!(list.packages[0].name, "curl");
}

#[tokio::test]
async fn lists_updates() {
    let adapter =
        connect(MockState::base().with_package(pk_info::SECURITY, "openssl;3;amd64;debian;"))
            .await
            .unwrap();
    let list = adapter
        .check_updates(&UpdateFilter::default())
        .await
        .unwrap();
    assert_eq!(list.total, 1);
    assert_eq!(list.updates[0].info, "security");
}

#[tokio::test]
async fn plans_and_applies_an_upgrade() {
    let mut state = MockState::base();
    state.plan = vec![
        (
            pk_info::SECURITY,
            "openssl;3;amd64;debian;".to_string(),
            String::new(),
        ),
        (
            pk_info::INSTALL,
            "libfoo;1;amd64;debian;".to_string(),
            String::new(),
        ),
    ];
    let adapter = connect(state).await.unwrap();

    let plan = adapter
        .plan(&["openssl;2;amd64;debian;".to_string()])
        .await
        .unwrap();
    assert_eq!(plan.total, 2);
    assert_eq!(plan.would_install, 1);
    assert!(plan.is_transitive());

    let report = adapter
        .apply(&["openssl;2;amd64;debian;".to_string()])
        .await
        .unwrap();
    assert!(report.success);
    assert_eq!(report.affected.len(), 2);
}

#[tokio::test]
async fn refreshes_the_cache() {
    let mut state = MockState::base();
    state.repos = vec![("debian".to_string(), "Debian".to_string(), true)];
    let adapter = connect(state).await.unwrap();
    let report = adapter.refresh(true).await.unwrap();
    assert!(report.success);
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].id, "debian");
}

#[tokio::test]
async fn refuses_untrusted_packages() {
    let mut state = MockState::base();
    state.untrusted = true;
    state.plan = vec![(pk_info::SECURITY, "x;1;amd64;d;".to_string(), String::new())];
    let adapter = connect(state).await.unwrap();
    let error = adapter
        .apply(&["x;1;amd64;d;".to_string()])
        .await
        .unwrap_err();
    assert!(matches!(error, PackageError::Untrusted(_)), "{error:?}");
}

#[tokio::test]
async fn surfaces_an_error_code() {
    let mut state = MockState::base();
    state.error = Some((pk_error::NO_CACHE, "no metadata".to_string()));
    let adapter = connect(state).await.unwrap();
    let error = adapter
        .check_updates(&UpdateFilter::default())
        .await
        .unwrap_err();
    assert_eq!(error, PackageError::NoCache);
}

#[tokio::test]
async fn reports_a_failed_transaction() {
    let mut state = MockState::base();
    state.exit = pk_exit::FAILED;
    state.plan = vec![(pk_info::SECURITY, "x;1;amd64;d;".to_string(), String::new())];
    let adapter = connect(state).await.unwrap();
    let report = adapter.apply(&["x;1;amd64;d;".to_string()]).await.unwrap();
    assert!(!report.success);
    assert_eq!(report.exit, "failed");
}

#[tokio::test]
async fn times_out_a_hung_transaction() {
    let mut state = MockState::base();
    state.hang = true;
    let adapter = connect(state).await.unwrap().with_limits(
        Duration::from_millis(200),
        Duration::from_millis(100),
        100,
    );
    let report = adapter.refresh(true).await.unwrap();
    assert!(report.timed_out);
    assert!(!report.success);
    assert_eq!(report.exit, "timeout");
}

#[tokio::test]
async fn refuses_when_authorisation_is_not_granted() {
    let mut state = MockState::base();
    state.authorize = 2; // PK_AUTHORIZE_ENUM_NO
    let adapter = connect(state).await.unwrap();
    let error = adapter.refresh(true).await.unwrap_err();
    assert!(matches!(error, PackageError::NotAuthorized(_)), "{error:?}");
}

#[tokio::test]
async fn refuses_an_unsupported_daemon_version() {
    let mut state = MockState::base();
    state.major = 2;
    let result = connect(state).await;
    assert!(matches!(result, Err(PackageError::Unsupported(_))));
}

#[tokio::test]
async fn refuses_a_eula() {
    let mut state = MockState::base();
    state.eula = true;
    let adapter = connect(state).await.unwrap();
    let error = adapter.refresh(true).await.unwrap_err();
    assert!(matches!(error, PackageError::Eula(_)), "{error:?}");
}

#[tokio::test]
async fn reports_a_required_restart() {
    let mut state = MockState::base();
    state.restart = true;
    let adapter = connect(state).await.unwrap();
    let report = adapter.refresh(true).await.unwrap();
    assert_eq!(report.exit, "success");
    // The restart signal is only surfaced on the upgrade report.
    let mut state = MockState::base();
    state.restart = true;
    state.plan = vec![(pk_info::SECURITY, "x;1;amd64;d;".to_string(), String::new())];
    let adapter = connect(state).await.unwrap();
    let report = adapter.apply(&["x;1;amd64;d;".to_string()]).await.unwrap();
    assert_eq!(report.restart_required.as_deref(), Some("system"));
}

#[tokio::test]
async fn retries_when_the_database_is_locked() {
    let mut state = MockState::base();
    state.lock_times = 2;
    state.plan = vec![(pk_info::SECURITY, "x;1;amd64;d;".to_string(), String::new())];
    let adapter = connect(state)
        .await
        .unwrap()
        .with_retry_base(Duration::from_millis(1));
    let report = adapter.apply(&["x;1;amd64;d;".to_string()]).await.unwrap();
    assert!(report.success);
}

#[tokio::test]
async fn gives_up_after_the_retry_budget() {
    let mut state = MockState::base();
    state.lock_times = 100;
    let adapter = connect(state)
        .await
        .unwrap()
        .with_retry_base(Duration::from_millis(1));
    let error = adapter.refresh(true).await.unwrap_err();
    assert!(matches!(error, PackageError::Locked(_)), "{error:?}");
}

#[tokio::test]
async fn lazy_delegates_every_operation() {
    let mut state = MockState::base().with_package(pk_info::INSTALLED, "nginx;1;amd64;d;");
    state.plan = vec![(
        pk_info::SECURITY,
        "openssl;3;amd64;d;".to_string(),
        String::new(),
    )];
    state.repos = vec![("debian".to_string(), "Debian".to_string(), true)];
    let connector: Connector = Arc::new(move || {
        let state = state.clone();
        Box::pin(async move { connect(state).await })
    });
    let lazy = LazyPackagekit::new(connector);

    assert_eq!(
        lazy.list_installed(&PackageFilter::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        lazy.check_updates(&UpdateFilter::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        lazy.plan(&["openssl;2;amd64;d;".to_string()])
            .await
            .unwrap()
            .total,
        1
    );
    assert!(
        lazy.apply(&["openssl;2;amd64;d;".to_string()])
            .await
            .unwrap()
            .success
    );
    assert!(lazy.refresh(true).await.unwrap().success);
}
