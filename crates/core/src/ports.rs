use async_trait::async_trait;

use crate::{
    AppError, SystemError, SystemdError, Unit, UnitActionOutcome, UnitFilter, UnitName, UnitStatus,
    Uptime, UptimeReport,
};

/// Outbound (driven) port: a source of host uptime.
///
/// Implemented by adapters in `gilfoyle-system`.
pub trait UptimeSource: Send + Sync {
    /// Read the current host uptime.
    fn uptime(&self) -> Result<Uptime, SystemError>;
}

/// Inbound (driving) port: the uptime operation gilfoyle exposes.
pub trait SystemAdmin: Send + Sync {
    /// Read the current host uptime, ready to be rendered to a client.
    fn uptime(&self) -> Result<UptimeReport, AppError>;
}

/// Outbound (driven) port: read-only access to systemd units.
///
/// Implemented by the systemd adapter in `gilfoyle-systemd`.
#[async_trait]
pub trait ServiceCatalog: Send + Sync {
    /// List units matching a filter.
    async fn list_units(&self, filter: &UnitFilter) -> Result<Vec<Unit>, SystemdError>;
    /// Read the detailed status of one unit.
    async fn unit_status(&self, name: &UnitName) -> Result<UnitStatus, SystemdError>;
}

/// Outbound (driven) port: start and stop systemd units.
///
/// Implemented by the systemd adapter in `gilfoyle-systemd`.
#[async_trait]
pub trait ServiceControl: Send + Sync {
    /// Start a unit and wait for the job to finish.
    async fn start_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError>;
    /// Stop a unit and wait for the job to finish.
    async fn stop_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError>;
}

/// Inbound (driving) port: the systemd operations gilfoyle exposes.
///
/// Names are taken as raw strings and normalised here, and mutations are checked
/// against the allowlist before reaching the adapter.
#[async_trait]
pub trait SystemdAdmin: Send + Sync {
    /// List units matching a filter.
    async fn list_units(&self, filter: &UnitFilter) -> Result<Vec<Unit>, AppError>;
    /// Read the detailed status of one unit.
    async fn unit_status(&self, name: &str) -> Result<UnitStatus, AppError>;
    /// Start a managed unit.
    async fn start_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError>;
    /// Stop a managed unit.
    async fn stop_unit(&self, name: &str) -> Result<UnitActionOutcome, AppError>;
}
