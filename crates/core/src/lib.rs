//! Gilfoyle domain core.
//!
//! This crate is deliberately free of I/O, async runtimes and transport
//! concerns. It defines the domain types, the ports (traits) that adapters
//! implement, and the use cases that orchestrate them. Because the core cannot
//! perform I/O, it structurally cannot execute anything on the host.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod application;
mod disk;
mod docker;
mod domain;
mod error;
mod host;
mod packagekit;
mod path;
mod ports;
mod systemd;

pub use application::{
    DiskService, DockerService, HostService, PackageService, PathService, SystemService,
    SystemdService,
};
pub use disk::{DiskUsage, FilesystemUsage, MountPoint, MountPoints};
pub use docker::{
    Container, ContainerAction, ContainerActionOutcome, ContainerAllowlist, ContainerFilter,
    ContainerIdentity, ContainerList, ContainerRef, ContainerState, ContainerStatus,
};
pub use domain::{Uptime, UptimeReport};
pub use error::{
    AppError, DiskError, DockerError, DomainError, PathError, SystemError, SystemdError,
};
pub use host::{LoadAndMemory, LoadAverage, Memory};
pub use packagekit::{
    AffectedPackage, Collector, Package, PackageAllowlist, PackageError, PackageEvent,
    PackageFilter, PackageId, PackageList, PackageOperation, PlannedAction, PlannedPackage,
    RefreshReport, Repo, Severity, TransactionKind, Update, UpdateFilter, UpdateList, UpgradePlan,
    UpgradeReport, exit_name, filter_installed, filter_none, info_name, map_error_code,
    map_exit_code, planned_action, restart_name, update_flags,
};
pub use packagekit::{
    error_code as pk_error, exit as pk_exit, filter as pk_filter, flag as pk_flag, info as pk_info,
    restart as pk_restart,
};
pub use path::{PathAllowlist, PathKind, PathRef, PathStatus};
pub use ports::{
    ContainerCatalog, ContainerControl, DiskAdmin, DiskSource, DockerAdmin, HostAdmin, LoadSource,
    MemorySource, PackageAdmin, PackageManager, PathAdmin, PathSource, ServiceCatalog,
    ServiceControl, SystemAdmin, SystemdAdmin, UptimeSource,
};
pub use systemd::{
    Allowlist, JobResult, Unit, UnitActionOutcome, UnitFilter, UnitList, UnitName, UnitStatus,
    UnitTimestamps, UnitType, micros_to_rfc3339,
};
