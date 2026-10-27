use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    Allowlist, AppError, ServiceCatalog, ServiceControl, SystemAdmin, SystemdAdmin, Unit,
    UnitActionOutcome, UnitFilter, UnitName, UnitStatus, UptimeReport, UptimeSource,
};

/// Application service implementing the uptime driving port on top of a driven port.
///
/// Generic over the [`UptimeSource`] so it is zero-cost in production and
/// trivially testable with a fake source.
pub struct SystemService<S: UptimeSource> {
    source: S,
}

impl<S: UptimeSource> SystemService<S> {
    /// Build a service backed by `source`.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: UptimeSource> SystemAdmin for SystemService<S> {
    fn uptime(&self) -> Result<UptimeReport, AppError> {
        let uptime = self.source.uptime()?;
        Ok(UptimeReport {
            uptime_seconds: uptime.as_seconds(),
            uptime_human: uptime.human(),
        })
    }
}

/// Application service implementing the systemd driving port.
///
/// Normalises unit names, enforces the allowlist on mutations, and delegates to the
/// driven ports. All of this is pure logic, testable with fake ports.
pub struct SystemdService {
    catalog: Arc<dyn ServiceCatalog>,
    control: Arc<dyn ServiceControl>,
    allowlist: Allowlist,
}

impl SystemdService {
    /// Build a service from a catalog port, a control port and the allowlist.
    pub fn new(
        catalog: Arc<dyn ServiceCatalog>,
        control: Arc<dyn ServiceControl>,
        allowlist: Allowlist,
    ) -> Self {
        Self {
            catalog,
            control,
            allowlist,
        }
    }
}

#[async_trait]
impl SystemdAdmin for SystemdService {
    async fn list_units(&self, filter: &UnitFilter) -> Result<Vec<Unit>, AppError> {
        Ok(self.catalog.list_units(filter).await?)
    }

    async fn unit_status(&self, name: &str) -> Result<UnitStatus, AppError> {
        let name = UnitName::parse(name)?;
        Ok(self.catalog.unit_status(&name).await?)
    }

    async fn start_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError> {
        let name = UnitName::parse(name)?;
        self.allowlist.ensure_managed(&name)?;
        Ok(self.control.start_unit(&name).await?)
    }

    async fn stop_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError> {
        let name = UnitName::parse(name)?;
        self.allowlist.ensure_managed(&name)?;
        Ok(self.control.stop_unit(&name).await?)
    }
}
