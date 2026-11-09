//! Lazy PackageKit connection.
//!
//! Gilfoyle must start even when PackageKit is absent (a minimal host, a
//! container), so the bus is connected and the daemon detected on first use
//! rather than at startup. A failed connection is not cached, so a later call
//! retries.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gilfoyle_core::{
    PackageError, PackageFilter, PackageList, PackageManager, RefreshReport, UpdateFilter,
    UpdateList, UpgradePlan, UpgradeReport,
};
use tokio::sync::OnceCell;

use crate::adapter::PackagekitAdapter;

/// A future produced by a connector.
pub type ConnectFuture =
    Pin<Box<dyn Future<Output = Result<PackagekitAdapter, PackageError>> + Send>>;

/// Produces a PackageKit adapter on demand.
pub type Connector = Arc<dyn Fn() -> ConnectFuture + Send + Sync>;

/// Connects to PackageKit on first use and caches the adapter.
pub struct LazyPackagekit {
    connector: Connector,
    adapter: OnceCell<PackagekitAdapter>,
}

impl LazyPackagekit {
    /// Build a lazy adapter with an explicit connector (used by tests).
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            adapter: OnceCell::new(),
        }
    }

    /// Build a lazy adapter that connects to the system bus.
    pub fn system() -> Self {
        Self::new(Arc::new(|| Box::pin(PackagekitAdapter::connect_system())))
    }

    /// Build a lazy adapter that connects to the system bus with explicit
    /// timeouts and report limit.
    pub fn system_with(call_timeout: Duration, poll_timeout: Duration, limit: usize) -> Self {
        Self::new(Arc::new(move || {
            Box::pin(async move {
                PackagekitAdapter::connect_system()
                    .await
                    .map(|adapter| adapter.with_limits(call_timeout, poll_timeout, limit))
            })
        }))
    }

    async fn adapter(&self) -> Result<&PackagekitAdapter, PackageError> {
        self.adapter.get_or_try_init(|| (self.connector)()).await
    }
}

#[async_trait]
impl PackageManager for LazyPackagekit {
    async fn list_installed(&self, filter: &PackageFilter) -> Result<PackageList, PackageError> {
        self.adapter().await?.list_installed(filter).await
    }

    async fn check_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, PackageError> {
        self.adapter().await?.check_updates(filter).await
    }

    async fn plan(&self, package_ids: &[String]) -> Result<UpgradePlan, PackageError> {
        self.adapter().await?.plan(package_ids).await
    }

    async fn apply(&self, package_ids: &[String]) -> Result<UpgradeReport, PackageError> {
        self.adapter().await?.apply(package_ids).await
    }

    async fn refresh(&self, force: bool) -> Result<RefreshReport, PackageError> {
        self.adapter().await?.refresh(force).await
    }
}
