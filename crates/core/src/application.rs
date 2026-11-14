use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    Allowlist, AppError, Container, ContainerAction, ContainerActionOutcome, ContainerAllowlist,
    ContainerCatalog, ContainerControl, ContainerFilter, ContainerRef, ContainerStatus, DiskAdmin,
    DiskSource, DockerAdmin, FilesystemUsage, HostAdmin, LoadAndMemory, LoadSource, MemorySource,
    PackageAdmin, PackageAllowlist, PackageError, PackageFilter, PackageList, PackageManager,
    PackageOperation, PathAdmin, PathAllowlist, PathRef, PathSource, PathStatus, RefreshReport,
    ServiceCatalog, ServiceControl, SystemAdmin, SystemdAdmin, Unit, UnitActionOutcome, UnitFilter,
    UnitName, UnitStatus, UpdateFilter, UpdateList, UpgradePlan, UpgradeReport, UptimeReport,
    UptimeSource,
};

/// Application service implementing the host metrics driving port.
///
/// Generic over the two driven ports so it is zero-cost in production and
/// trivially testable with fake sources.
pub struct HostService<L: LoadSource, M: MemorySource> {
    load: L,
    memory: M,
}

impl<L: LoadSource, M: MemorySource> HostService<L, M> {
    /// Build a service backed by a load source and a memory source.
    pub fn new(load: L, memory: M) -> Self {
        Self { load, memory }
    }
}

impl<L: LoadSource, M: MemorySource> HostAdmin for HostService<L, M> {
    fn load_and_memory(&self) -> Result<LoadAndMemory, AppError> {
        Ok(LoadAndMemory {
            load: self.load.load_average()?,
            memory: self.memory.memory()?,
        })
    }
}

/// Application service implementing the disk usage driving port.
pub struct DiskService {
    source: Arc<dyn DiskSource>,
}

impl DiskService {
    /// Build a service backed by a disk source.
    pub fn new(source: Arc<dyn DiskSource>) -> Self {
        Self { source }
    }
}

impl DiskAdmin for DiskService {
    fn disk_usage(&self) -> Result<Vec<FilesystemUsage>, AppError> {
        Ok(self.source.disk_usage()?)
    }
}

/// Application service implementing the path metadata driving port.
///
/// The path is normalised, checked against the allowlist, and only then read.
/// All of this is pure logic, testable with a fake source.
pub struct PathService {
    source: Arc<dyn PathSource>,
    allowlist: PathAllowlist,
}

impl PathService {
    /// Build a service from a path source and the allowlist.
    pub fn new(source: Arc<dyn PathSource>, allowlist: PathAllowlist) -> Self {
        Self { source, allowlist }
    }
}

impl PathAdmin for PathService {
    fn path_status(&self, path: &str) -> Result<PathStatus, AppError> {
        let path = PathRef::parse(path)?;
        self.allowlist.ensure_allowed(&path)?;
        Ok(self.source.stat(&path)?)
    }
}

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

/// Application service implementing the package driving port.
///
/// Reads pass straight through. Mutations are checked against the allowlist,
/// and an upgrade is first simulated: if the plan would install, remove,
/// obsolete or downgrade a package, it is refused unless the operator has
/// explicitly opted in. All of this is pure logic, testable with a fake port.
pub struct PackageService {
    manager: Arc<dyn PackageManager>,
    allowlist: PackageAllowlist,
    allow_transitive: bool,
}

impl PackageService {
    /// Build a service from a manager port, the operation allowlist and the
    /// transitive-change opt-in.
    pub fn new(
        manager: Arc<dyn PackageManager>,
        allowlist: PackageAllowlist,
        allow_transitive: bool,
    ) -> Self {
        Self {
            manager,
            allowlist,
            allow_transitive,
        }
    }

    /// The package ids of every available update.
    async fn update_ids(&self) -> Result<Vec<String>, PackageError> {
        let updates = self.manager.check_updates(&UpdateFilter::default()).await?;
        Ok(updates
            .updates
            .into_iter()
            .map(|update| update.id)
            .collect())
    }
}

#[async_trait]
impl PackageAdmin for PackageService {
    async fn list_packages(&self, filter: &PackageFilter) -> Result<PackageList, AppError> {
        Ok(self.manager.list_installed(filter).await?)
    }

    async fn list_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, AppError> {
        Ok(self.manager.check_updates(filter).await?)
    }

    async fn preview_upgrade(&self) -> Result<UpgradePlan, AppError> {
        let ids = self.update_ids().await?;
        if ids.is_empty() {
            return Ok(UpgradePlan::empty());
        }
        Ok(self.manager.plan(&ids).await?)
    }

    async fn refresh_packages(&self, force: bool) -> Result<RefreshReport, AppError> {
        self.allowlist.ensure_allowed(PackageOperation::Refresh)?;
        Ok(self.manager.refresh(force).await?)
    }

    async fn upgrade_packages(&self) -> Result<UpgradeReport, AppError> {
        self.allowlist.ensure_allowed(PackageOperation::Upgrade)?;
        let ids = self.update_ids().await?;
        if ids.is_empty() {
            return Ok(UpgradeReport::nothing_to_do());
        }
        let plan = self.manager.plan(&ids).await?;
        if plan.is_transitive() && !self.allow_transitive {
            return Err(AppError::Package(PackageError::TransitiveRefused {
                install: plan.would_install,
                remove: plan.would_remove,
                obsolete: plan.would_obsolete,
                downgrade: plan.would_downgrade,
            }));
        }
        Ok(self.manager.apply(&ids).await?)
    }
}

/// Application service implementing the Docker driving port.
///
/// Reads pass straight through. Mutations resolve the reference to a container,
/// check the allowlist against **all** of its names, then act by the resolved
/// id (so a rename or recreation between the check and the action cannot target
/// another container). All of this is pure logic, testable with fake ports.
pub struct DockerService {
    catalog: Arc<dyn ContainerCatalog>,
    control: Arc<dyn ContainerControl>,
    allowlist: ContainerAllowlist,
}

impl DockerService {
    /// Build a service from a catalog port, a control port and the allowlist.
    pub fn new(
        catalog: Arc<dyn ContainerCatalog>,
        control: Arc<dyn ContainerControl>,
        allowlist: ContainerAllowlist,
    ) -> Self {
        Self {
            catalog,
            control,
            allowlist,
        }
    }

    /// Resolve, authorise and perform one lifecycle action on a container.
    async fn mutate(
        &self,
        reference: &str,
        action: ContainerAction,
    ) -> Result<ContainerActionOutcome, AppError> {
        let reference = ContainerRef::parse(reference)?;
        let before = self.catalog.resolve(&reference).await?;
        self.allowlist.ensure_managed(&before.names)?;

        match action {
            ContainerAction::Start => self.control.start_container(&before.id).await?,
            ContainerAction::Stop => self.control.stop_container(&before.id).await?,
            ContainerAction::Restart => self.control.restart_container(&before.id).await?,
        }

        let after = self
            .catalog
            .resolve(&ContainerRef::parse(&before.id)?)
            .await?;
        Ok(ContainerActionOutcome {
            container: before.canonical_name().to_string(),
            id: before.id,
            action: action.as_str().to_string(),
            state_before: before.state,
            state_after: after.state,
        })
    }
}

#[async_trait]
impl DockerAdmin for DockerService {
    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<Container>, AppError> {
        Ok(self.catalog.list_containers(filter).await?)
    }

    async fn container_status(&self, reference: &str) -> Result<ContainerStatus, AppError> {
        let reference = ContainerRef::parse(reference)?;
        Ok(self.catalog.container_status(&reference).await?)
    }

    async fn start_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.mutate(reference, ContainerAction::Start).await
    }

    async fn stop_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.mutate(reference, ContainerAction::Stop).await
    }

    async fn restart_container(&self, reference: &str) -> Result<ContainerActionOutcome, AppError> {
        self.mutate(reference, ContainerAction::Restart).await
    }
}
