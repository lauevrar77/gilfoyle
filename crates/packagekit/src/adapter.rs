//! The PackageKit driven adapter, speaking D-Bus in-process.
//!
//! Gilfoyle never spawns `apt`, `dnf` or any other tool: it asks PackageKit to
//! perform the operation over the system bus, exactly as it does with systemd.
//! PackageKit is transactional and signal-driven, so every operation is a small
//! state machine: create a transaction object, subscribe to its signals, invoke
//! the method, then feed the decoded signals to a pure [`Collector`].
//!
//! Only the **1.x** dialect is implemented (bus `org.freedesktop.PackageKit`),
//! which is what every released distribution ships. PackageKit 2.x renamed the
//! bus, the signal layout and the enumeration values, so it is detected and
//! refused rather than mis-decoded.

use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use gilfoyle_core::{
    Collector, PackageError, PackageEvent, PackageFilter, PackageList, PackageManager,
    RefreshReport, TransactionKind, UpdateFilter, UpdateList, UpgradePlan, UpgradeReport,
    filter_installed, filter_none, update_flags,
};
use zbus::{Connection, OwnedObjectPath, Proxy};

use crate::error::map_zbus_error;

/// Default timeout for a single D-Bus call.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Default timeout for a whole transaction.
pub const DEFAULT_POLL_TIMEOUT: Duration = Duration::from_secs(600);
/// Default number of entries kept in a report list.
pub const DEFAULT_LIMIT: usize = 200;
/// How many times a lock-contended operation is retried.
const RETRY_MAX: u32 = 5;
/// Base delay between lock retries (doubled each time).
const RETRY_BASE: Duration = Duration::from_millis(250);

/// The 1.x bus name.
const SERVICE_V1: &str = "org.freedesktop.PackageKit";
/// The 2.x bus name, detected only to report a clear error.
const SERVICE_V2: &str = "org.freedesktop.packagekit1";
/// The root object path.
const ROOT_PATH: &str = "/org/freedesktop/PackageKit";
/// The root interface.
const ROOT_IFACE: &str = "org.freedesktop.PackageKit";
/// The transaction interface.
const TX_IFACE: &str = "org.freedesktop.PackageKit.Transaction";

/// `PkAuthorizeEnum::YES`.
const AUTHORIZE_YES: u32 = 1;

/// The polkit action required to refresh the cache.
const ACTION_REFRESH: &str = "org.freedesktop.packagekit.system-sources-refresh";
/// The polkit action required to update packages.
const ACTION_UPDATE: &str = "org.freedesktop.packagekit.system-update";

type EventStream = BoxStream<'static, PackageEvent>;

/// The method to invoke on a freshly created transaction.
enum Invocation {
    GetPackages(u64),
    GetUpdates(u64),
    RefreshCache(bool),
    UpdatePackages(u64, Vec<String>),
}

/// PackageKit driven adapter. Holds a live bus connection; performs no process spawning.
pub struct PackagekitAdapter {
    connection: Connection,
    destination: String,
    call_timeout: Duration,
    poll_timeout: Duration,
    limit: usize,
    mutations: tokio::sync::Mutex<()>,
}

impl PackagekitAdapter {
    /// Build an adapter over an existing bus connection, detecting the dialect.
    pub async fn new(connection: Connection) -> Result<Self, PackageError> {
        let destination = detect(&connection).await?;
        Ok(Self {
            connection,
            destination,
            call_timeout: DEFAULT_CALL_TIMEOUT,
            poll_timeout: DEFAULT_POLL_TIMEOUT,
            limit: DEFAULT_LIMIT,
            mutations: tokio::sync::Mutex::new(()),
        })
    }

    /// Connect to the system bus and detect PackageKit.
    pub async fn connect_system() -> Result<Self, PackageError> {
        let connection = Connection::system().await.map_err(map_zbus_error)?;
        Self::new(connection).await
    }

    /// Override the call timeout, poll timeout and report limit (used by tests).
    pub fn with_limits(
        mut self,
        call_timeout: Duration,
        poll_timeout: Duration,
        limit: usize,
    ) -> Self {
        self.call_timeout = call_timeout;
        self.poll_timeout = poll_timeout;
        self.limit = limit;
        self
    }

    async fn root(&self) -> Result<Proxy<'_>, PackageError> {
        Proxy::new(
            &self.connection,
            self.destination.as_str(),
            ROOT_PATH,
            ROOT_IFACE,
        )
        .await
        .map_err(map_zbus_error)
    }

    async fn create_transaction(&self) -> Result<Proxy<'static>, PackageError> {
        let root = self.root().await?;
        let path: OwnedObjectPath =
            time_call(self.call_timeout, root.call("CreateTransaction", &())).await?;
        Proxy::new_owned(
            self.connection.clone(),
            self.destination.clone(),
            path,
            TX_IFACE,
        )
        .await
        .map_err(map_zbus_error)
    }

    async fn subscribe(&self, tx: &Proxy<'_>) -> Result<EventStream, PackageError> {
        // One stream for every signal on the transaction object: a single D-Bus
        // stream preserves the order the daemon emitted them in (packages before
        // `Finished`), whereas one stream per signal name would not.
        let stream = tx.receive_all_signals().await.map_err(map_zbus_error)?;
        Ok(Box::pin(
            stream
                .map(|message| decode_signal(&message))
                .flat_map(futures_util::stream::iter),
        ))
    }

    async fn invoke(&self, tx: &Proxy<'_>, invocation: Invocation) -> Result<(), PackageError> {
        let timeout = self.call_timeout;
        match invocation {
            Invocation::GetPackages(filter) => {
                time_call(timeout, tx.call::<_, _, ()>("GetPackages", &(filter,))).await
            }
            Invocation::GetUpdates(filter) => {
                time_call(timeout, tx.call::<_, _, ()>("GetUpdates", &(filter,))).await
            }
            Invocation::RefreshCache(force) => {
                time_call(timeout, tx.call::<_, _, ()>("RefreshCache", &(force,))).await
            }
            Invocation::UpdatePackages(flags, ids) => {
                time_call(
                    timeout,
                    tx.call::<_, _, ()>("UpdatePackages", &(flags, ids)),
                )
                .await
            }
        }
    }

    /// Run one transaction and collect its signals.
    async fn run(
        &self,
        kind: TransactionKind,
        limit: usize,
        invocation: Invocation,
    ) -> Result<Collector, PackageError> {
        let tx = self.create_transaction().await?;
        let mut events = self.subscribe(&tx).await?;
        self.invoke(&tx, invocation).await?;

        let mut collector = Collector::new(kind, limit);
        let outcome = tokio::time::timeout(self.poll_timeout, async {
            while let Some(event) = events.next().await {
                let finished = matches!(&event, PackageEvent::Finished { .. });
                collector.push(event);
                if finished {
                    break;
                }
            }
        })
        .await;

        if outcome.is_err() {
            collector.mark_timed_out();
            let _ = time_call(self.call_timeout, tx.call::<_, _, ()>("Cancel", &())).await;
        }
        Ok(collector)
    }

    /// Fail unless polkit would authorise the action without interaction.
    async fn authorize(&self, action: &str) -> Result<(), PackageError> {
        let root = self.root().await?;
        let result: u32 =
            time_call(self.call_timeout, root.call("CanAuthorize", &(action,))).await?;
        if result == AUTHORIZE_YES {
            Ok(())
        } else {
            Err(PackageError::NotAuthorized(format!(
                "{action} is not authorised (polkit result {result})"
            )))
        }
    }
}

#[async_trait]
impl PackageManager for PackagekitAdapter {
    async fn list_installed(&self, filter: &PackageFilter) -> Result<PackageList, PackageError> {
        let collector = self
            .run(
                TransactionKind::ListInstalled,
                usize::MAX,
                Invocation::GetPackages(filter_installed()),
            )
            .await?;
        let list = collector.into_package_list()?;
        Ok(filter.apply(list.packages))
    }

    async fn check_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, PackageError> {
        let collector = self
            .run(
                TransactionKind::CheckUpdates,
                usize::MAX,
                Invocation::GetUpdates(filter_none()),
            )
            .await?;
        let list = collector.into_update_list()?;
        Ok(filter.apply(list.updates))
    }

    async fn plan(&self, package_ids: &[String]) -> Result<UpgradePlan, PackageError> {
        // SIMULATE is authorised without polkit, so no preflight is needed.
        let collector = self
            .run(
                TransactionKind::Plan,
                self.limit,
                Invocation::UpdatePackages(update_flags(true), package_ids.to_vec()),
            )
            .await?;
        collector.into_upgrade_plan()
    }

    async fn apply(&self, package_ids: &[String]) -> Result<UpgradeReport, PackageError> {
        let _guard = self.mutations.lock().await;
        self.authorize(ACTION_UPDATE).await?;
        retry(|| async {
            self.run(
                TransactionKind::Apply,
                self.limit,
                Invocation::UpdatePackages(update_flags(false), package_ids.to_vec()),
            )
            .await
            .and_then(Collector::into_upgrade_report)
        })
        .await
    }

    async fn refresh(&self, force: bool) -> Result<RefreshReport, PackageError> {
        let _guard = self.mutations.lock().await;
        self.authorize(ACTION_REFRESH).await?;
        retry(|| async {
            self.run(
                TransactionKind::Refresh,
                self.limit,
                Invocation::RefreshCache(force),
            )
            .await
            .and_then(Collector::into_refresh_report)
        })
        .await
    }
}

/// Retry a fallible operation while it reports a held lock, with backoff.
async fn retry<T, F, Fut>(mut operation: F) -> Result<T, PackageError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, PackageError>>,
{
    let mut attempt = 0;
    let mut delay = RETRY_BASE;
    loop {
        match operation().await {
            Err(PackageError::Locked(_)) if attempt + 1 < RETRY_MAX => {
                attempt += 1;
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
            other => return other,
        }
    }
}

/// Wrap a D-Bus call in a timeout and map its error.
async fn time_call<T>(
    timeout: Duration,
    future: impl Future<Output = zbus::Result<T>>,
) -> Result<T, PackageError> {
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(map_zbus_error(error)),
        Err(_) => Err(PackageError::Timeout),
    }
}

/// Detect PackageKit and its dialect, refusing 2.x.
async fn detect(connection: &Connection) -> Result<String, PackageError> {
    match version_of(connection, SERVICE_V1).await {
        Ok((1, _)) => Ok(SERVICE_V1.to_string()),
        Ok((major, minor)) => Err(PackageError::Unsupported(format!(
            "PackageKit {major}.{minor}"
        ))),
        Err(PackageError::Unavailable(_)) => match version_of(connection, SERVICE_V2).await {
            Ok(_) => Err(PackageError::Unsupported(
                "PackageKit 2.x is not supported".to_string(),
            )),
            Err(other) => Err(other),
        },
        Err(other) => Err(other),
    }
}

/// Read the daemon version from the root interface.
async fn version_of(
    connection: &Connection,
    destination: &str,
) -> Result<(u32, u32), PackageError> {
    let root = Proxy::new(connection, destination, ROOT_PATH, ROOT_IFACE)
        .await
        .map_err(map_zbus_error)?;
    let major: u32 = root
        .get_property("VersionMajor")
        .await
        .map_err(map_zbus_error)?;
    let minor: u32 = root
        .get_property("VersionMinor")
        .await
        .map_err(map_zbus_error)?;
    Ok((major, minor))
}

/// Decode a PackageKit signal message into domain events.
fn decode_signal(message: &zbus::Message) -> Vec<PackageEvent> {
    let header = message.header();
    let Some(member) = header.member() else {
        return Vec::new();
    };
    match member.as_str() {
        "Package" => decode_package(message, false),
        "Packages" => decode_package(message, true),
        "Finished" => match message.body().deserialize::<(u32, u32)>() {
            Ok((exit, runtime_ms)) => vec![PackageEvent::Finished { exit, runtime_ms }],
            Err(_) => Vec::new(),
        },
        "ErrorCode" => match message.body().deserialize::<(u32, String)>() {
            Ok((code, details)) => vec![PackageEvent::ErrorCode { code, details }],
            Err(_) => Vec::new(),
        },
        "RepoDetail" => match message.body().deserialize::<(String, String, bool)>() {
            Ok((id, description, enabled)) => {
                vec![PackageEvent::RepoDetail {
                    id,
                    description,
                    enabled,
                }]
            }
            Err(_) => Vec::new(),
        },
        "RequireRestart" => match message.body().deserialize::<(u32, String)>() {
            Ok((kind, _)) => vec![PackageEvent::RequireRestart { kind }],
            Err(_) => Vec::new(),
        },
        "RepoSignatureRequired" => vec![PackageEvent::Untrusted {
            details: String::new(),
        }],
        "EulaRequired" => vec![PackageEvent::Eula {
            details: String::new(),
        }],
        _ => Vec::new(),
    }
}

/// Decode the singular `Package` or batched `Packages` signal.
fn decode_package(message: &zbus::Message, batched: bool) -> Vec<PackageEvent> {
    let package = |info: u32, package_id: String, summary: String| PackageEvent::Package {
        info,
        severity: 0,
        package_id,
        summary,
    };
    if batched {
        match message.body().deserialize::<Vec<(u32, String, String)>>() {
            Ok(items) => items
                .into_iter()
                .map(|(info, package_id, summary)| package(info, package_id, summary))
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        match message.body().deserialize::<(u32, String, String)>() {
            Ok((info, package_id, summary)) => vec![package(info, package_id, summary)],
            Err(_) => Vec::new(),
        }
    }
}
