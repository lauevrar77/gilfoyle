use async_trait::async_trait;

use crate::{
    AppError, Container, ContainerActionOutcome, ContainerFilter, ContainerIdentity, ContainerRef,
    ContainerStatus, DockerError, PackageError, PackageFilter, PackageList, RefreshReport,
    SystemError, SystemdError, Unit, UnitActionOutcome, UnitFilter, UnitName, UnitStatus,
    UpdateFilter, UpdateList, UpgradePlan, UpgradeReport, Uptime, UptimeReport,
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

/// Outbound (driven) port: read and mutate packages through PackageKit.
///
/// Implemented by the PackageKit adapter in `gilfoyle-packagekit`. Reads are
/// open; the adapter never installs or removes a package.
#[async_trait]
pub trait PackageManager: Send + Sync {
    /// List installed packages.
    async fn list_installed(&self, filter: &PackageFilter) -> Result<PackageList, PackageError>;
    /// List available updates.
    async fn check_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, PackageError>;
    /// Simulate applying the given updates and report what it would do.
    async fn plan(&self, package_ids: &[String]) -> Result<UpgradePlan, PackageError>;
    /// Apply the given updates.
    async fn apply(&self, package_ids: &[String]) -> Result<UpgradeReport, PackageError>;
    /// Refresh the package metadata cache.
    async fn refresh(&self, force: bool) -> Result<RefreshReport, PackageError>;
}

/// Inbound (driving) port: the package operations gilfoyle exposes.
///
/// Reads are open; the mutations are checked against the allowlist, and an
/// upgrade is refused when its plan crosses the install/remove boundary.
#[async_trait]
pub trait PackageAdmin: Send + Sync {
    /// List installed packages.
    async fn list_packages(&self, filter: &PackageFilter) -> Result<PackageList, AppError>;
    /// List available updates.
    async fn list_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, AppError>;
    /// Simulate an upgrade and report what it would do.
    async fn preview_upgrade(&self) -> Result<UpgradePlan, AppError>;
    /// Refresh the package metadata cache.
    async fn refresh_packages(&self, force: bool) -> Result<RefreshReport, AppError>;
    /// Apply available upgrades.
    async fn upgrade_packages(&self) -> Result<UpgradeReport, AppError>;
}

/// Outbound (driven) port: read-only access to Docker containers.
///
/// Implemented by the Docker adapter in `gilfoyle-docker`.
#[async_trait]
pub trait ContainerCatalog: Send + Sync {
    /// List containers matching a filter.
    async fn list_containers(
        &self,
        filter: &ContainerFilter,
    ) -> Result<Vec<Container>, DockerError>;
    /// Read the detailed status of one container.
    async fn container_status(
        &self,
        reference: &ContainerRef,
    ) -> Result<ContainerStatus, DockerError>;
    /// Resolve a reference to the container's id, names and current state.
    async fn resolve(&self, reference: &ContainerRef) -> Result<ContainerIdentity, DockerError>;
}

/// Outbound (driven) port: start, stop and restart Docker containers.
///
/// Implemented by the Docker adapter in `gilfoyle-docker`. There is no create
/// or remove: the action vocabulary is closed in the core.
#[async_trait]
pub trait ContainerControl: Send + Sync {
    /// Start the container with the given id.
    async fn start_container(&self, id: &str) -> Result<(), DockerError>;
    /// Stop the container with the given id.
    async fn stop_container(&self, id: &str) -> Result<(), DockerError>;
    /// Restart the container with the given id.
    async fn restart_container(&self, id: &str) -> Result<(), DockerError>;
}

/// Inbound (driving) port: the Docker operations gilfoyle exposes.
///
/// References are taken as raw strings and normalised here, and mutations are
/// checked against the allowlist before reaching the adapter.
#[async_trait]
pub trait DockerAdmin: Send + Sync {
    /// List containers matching a filter.
    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<Container>, AppError>;
    /// Read the detailed status of one container.
    async fn container_status(&self, reference: &str) -> Result<ContainerStatus, AppError>;
    /// Start a managed container.
    async fn start_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError>;
    /// Stop a managed container.
    async fn stop_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError>;
    /// Restart a managed container.
    async fn restart_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError>;
}
