//! Lazy systemd connection.
//!
//! Gilfoyle must start even when systemd is unreachable (for example in a container), so
//! the bus is connected on first use rather than at startup. A failed connection is not
//! cached, so a later call retries.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use gilfoyle_core::{
    ServiceCatalog, ServiceControl, SystemdError, Unit, UnitActionOutcome, UnitFilter, UnitName,
    UnitStatus,
};
use tokio::sync::OnceCell;

use crate::adapter::SystemdAdapter;

/// A future produced by a connector.
pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<SystemdAdapter, SystemdError>> + Send>>;

/// Produces a systemd adapter on demand.
pub type Connector = Arc<dyn Fn() -> ConnectFuture + Send + Sync>;

/// Connects to systemd on first use and caches the adapter.
pub struct LazySystemd {
    connector: Connector,
    adapter: OnceCell<SystemdAdapter>,
}

impl LazySystemd {
    /// Build a lazy adapter with an explicit connector (used by tests).
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            adapter: OnceCell::new(),
        }
    }

    /// Build a lazy adapter that connects to the system bus.
    pub fn system() -> Self {
        Self::new(Arc::new(|| Box::pin(SystemdAdapter::connect_system())))
    }

    async fn adapter(&self) -> Result<&SystemdAdapter, SystemdError> {
        self.adapter.get_or_try_init(|| (self.connector)()).await
    }
}

#[async_trait]
impl ServiceCatalog for LazySystemd {
    async fn list_units(&self, filter: &UnitFilter) -> Result<Vec<Unit>, SystemdError> {
        self.adapter().await?.list_units(filter).await
    }

    async fn unit_status(&self, name: &UnitName) -> Result<UnitStatus, SystemdError> {
        self.adapter().await?.unit_status(name).await
    }
}

#[async_trait]
impl ServiceControl for LazySystemd {
    async fn start_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.adapter().await?.start_unit(name).await
    }

    async fn stop_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.adapter().await?.stop_unit(name).await
    }
}
