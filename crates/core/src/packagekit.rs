//! Package-management domain types, the closed operation vocabulary and the pure
//! transaction state machine.
//!
//! Gilfoyle reaches PackageKit over D-Bus; this module holds everything that is
//! *not* transport: package identifiers, filter/report types, the allowlist of
//! mutating operations, and a collector that turns a stream of decoded
//! [`PackageEvent`]s into a typed outcome. None of it performs I/O, so it is
//! exhaustively unit-testable.
//!
//! The collector is driven by raw PackageKit `info`/`exit`/`code` integers. The
//! numeric values are the wire constants of PackageKit's **1.x** dialect
//! (`1 << index` bitfields and the classic enumeration order). PackageKit 2.x
//! renamed the bus and re-ordered these enumerations, so the adapter refuses it
//! rather than mis-decode it.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::{AppError, DomainError};

/// Classic PackageKit 1.x `PkInfoEnum` wire values.
pub mod info {
    /// Installed package.
    pub const INSTALLED: u32 = 1;
    /// Available (not installed) package.
    pub const AVAILABLE: u32 = 2;
    /// Low-priority update.
    pub const LOW: u32 = 3;
    /// Enhancement update.
    pub const ENHANCEMENT: u32 = 4;
    /// Normal update.
    pub const NORMAL: u32 = 5;
    /// Bug-fix update.
    pub const BUGFIX: u32 = 6;
    /// Important update.
    pub const IMPORTANT: u32 = 7;
    /// Security update.
    pub const SECURITY: u32 = 8;
    /// Blocked update.
    pub const BLOCKED: u32 = 9;
    /// Untrusted package.
    pub const UNTRUSTED: u32 = 23;
    /// Trusted package.
    pub const TRUSTED: u32 = 24;
    /// Critical update.
    pub const CRITICAL: u32 = 26;
    /// Package planned for installation.
    pub const INSTALL: u32 = 27;
    /// Package planned for removal.
    pub const REMOVE: u32 = 28;
    /// Package planned as obsolete.
    pub const OBSOLETE: u32 = 29;
    /// Package planned for downgrade.
    pub const DOWNGRADE: u32 = 30;
}

/// Classic PackageKit 1.x `PkExitEnum` wire values.
pub mod exit {
    /// Successful exit.
    pub const SUCCESS: u32 = 1;
    /// Generic failure.
    pub const FAILED: u32 = 2;
    /// Cancelled.
    pub const CANCELLED: u32 = 3;
    /// A repository signing key is required.
    pub const KEY_REQUIRED: u32 = 4;
    /// A EULA must be accepted.
    pub const EULA_REQUIRED: u32 = 5;
    /// Killed after a failed cancel.
    pub const KILLED: u32 = 6;
    /// An untrusted package was encountered.
    pub const NEED_UNTRUSTED: u32 = 8;
}

/// Classic PackageKit 1.x `PkFilterEnum` indices, used to build bitfields.
pub mod filter {
    /// No filter.
    pub const NONE: u32 = 1;
    /// Only installed packages.
    pub const INSTALLED: u32 = 2;
}

/// Classic PackageKit 1.x `PkTransactionFlagEnum` indices.
pub mod flag {
    /// Only trusted packages may be acted upon.
    pub const ONLY_TRUSTED: u32 = 1;
    /// Simulate the transaction without applying it.
    pub const SIMULATE: u32 = 2;
}

/// Classic PackageKit 1.x `PkRestartEnum` wire values.
pub mod restart {
    /// A system restart is required.
    pub const SYSTEM: u32 = 2;
    /// A session restart is required.
    pub const SESSION: u32 = 3;
    /// An application restart is required.
    pub const APPLICATION: u32 = 4;
}

/// Classic PackageKit 1.x `PkErrorEnum` values gilfoyle reacts to.
pub mod error_code {
    /// The metadata cache is empty.
    pub const NO_CACHE: u32 = 18;
    /// The native package lock could not be acquired.
    pub const CANNOT_GET_LOCK: u32 = 26;
    /// There is nothing to update.
    pub const NO_PACKAGES_TO_UPDATE: u32 = 27;
    /// A package was signed with a bad GPG signature.
    pub const BAD_GPG_SIGNATURE: u32 = 30;
    /// A package is missing a GPG signature.
    pub const MISSING_GPG_SIGNATURE: u32 = 31;
    /// The action was refused for lack of authorisation.
    pub const NOT_AUTHORIZED: u32 = 48;
    /// The native package lock is required.
    pub const LOCK_REQUIRED: u32 = 67;
}

/// The `filter` bitfield selecting installed packages.
pub fn filter_installed() -> u64 {
    1 << filter::INSTALLED
}

/// The `filter` bitfield selecting everything.
pub fn filter_none() -> u64 {
    1 << filter::NONE
}

/// The `transaction_flags` bitfield for an update: always `ONLY_TRUSTED`,
/// plus `SIMULATE` when planning.
pub fn update_flags(simulate: bool) -> u64 {
    let trusted = 1u64 << flag::ONLY_TRUSTED;
    let simulate = if simulate { 1u64 << flag::SIMULATE } else { 0 };
    trusted + simulate
}

/// A parsed PackageKit package identifier.
///
/// PackageKit defines the id as `name;version;arch;data`, but the DNF backend
/// emits an extra `origin` field: `name;version;arch;origin;data`. Both shapes
/// are accepted, because refusing one of them silently empties every result.
///
/// The raw string is kept verbatim so it can be handed straight back to
/// PackageKit (for example to `UpdatePackages`); the fields are exposed for
/// reporting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId {
    raw: String,
    name: String,
    version: String,
    architecture: String,
    origin: String,
}

impl PackageId {
    /// Parse a `name;version;arch;data` (APT) or
    /// `name;version;arch;origin;data` (DNF) identifier.
    ///
    /// Four or five fields are accepted; the name and version must be non-empty.
    /// In both shapes the fourth field is the origin slot.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let parts: Vec<&str> = input.split(';').collect();
        if !matches!(parts.len(), 4 | 5) {
            return Err(DomainError::InvalidPackageId(input.to_string()));
        }
        let (name, version) = (parts[0], parts[1]);
        if name.is_empty() || version.is_empty() {
            return Err(DomainError::InvalidPackageId(input.to_string()));
        }
        Ok(Self {
            raw: input.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            architecture: parts[2].to_string(),
            origin: parts[3].to_string(),
        })
    }

    /// The identifier as it appears on the wire.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The package name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The package version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The package architecture.
    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    /// The origin repository (may be empty).
    pub fn origin(&self) -> &str {
        &self.origin
    }
}

/// One installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Package {
    /// Package name.
    pub name: String,
    /// Installed version.
    pub version: String,
    /// Architecture.
    pub architecture: String,
    /// Origin repository, when known.
    pub origin: String,
}

/// A listing of installed packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PackageList {
    /// The matching packages.
    pub packages: Vec<Package>,
    /// Total number of packages matching the filter.
    pub total: usize,
    /// Whether the returned list was truncated by the limit.
    pub truncated: bool,
}

/// Filter applied to `list_packages`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageFilter {
    /// Only keep packages whose name contains this substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Maximum number of packages to return.
    pub limit: Option<usize>,
}

impl PackageFilter {
    /// Apply the client-side parts of the filter and stamp the totals.
    pub fn apply(&self, mut packages: Vec<Package>) -> PackageList {
        let needle = self.name_contains.as_ref().map(|n| n.to_lowercase());
        packages.retain(|package| match &needle {
            Some(needle) => package.name.to_lowercase().contains(needle),
            None => true,
        });
        let total = packages.len();
        let limit = self.limit.unwrap_or(usize::MAX);
        packages.truncate(limit);
        PackageList {
            truncated: total > packages.len(),
            packages,
            total,
        }
    }
}

/// Severity of an available update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
    /// Critical (security).
    Critical,
}

impl Severity {
    /// Parse a user-supplied severity name.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(DomainError::UnknownSeverity(other.to_string())),
        }
    }

    /// The wire-independent name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    /// Classify a PackageKit 1.x `info` value as an update severity, if it is one.
    pub fn from_info(info: u32) -> Option<Self> {
        match info {
            info::LOW | info::ENHANCEMENT => Some(Self::Low),
            info::NORMAL | info::BUGFIX => Some(Self::Medium),
            info::IMPORTANT => Some(Self::High),
            info::SECURITY | info::CRITICAL => Some(Self::Critical),
            _ => None,
        }
    }
}

/// One available update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Update {
    /// Raw package id, suitable for handing back to PackageKit.
    #[serde(skip)]
    pub id: String,
    /// Package name.
    pub name: String,
    /// Candidate (new) version.
    pub version: String,
    /// PackageKit info classification (for example `security`, `bugfix`).
    pub info: String,
    /// Severity, when the info value carries one.
    pub severity: Option<String>,
}

/// A listing of available updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UpdateList {
    /// The updates.
    pub updates: Vec<Update>,
    /// Total number of updates matching the filter.
    pub total: usize,
    /// Whether the returned list was truncated by the limit.
    pub truncated: bool,
}

/// Filter applied to `list_updates`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateFilter {
    /// Only keep updates at or above this severity.
    pub min_severity: Option<Severity>,
    /// Maximum number of updates to return.
    pub limit: Option<usize>,
}

impl UpdateFilter {
    /// Apply the client-side parts of the filter and stamp the totals.
    pub fn apply(&self, mut updates: Vec<Update>) -> UpdateList {
        updates.retain(|update| match (self.min_severity, &update.severity) {
            (Some(min), Some(severity)) => Severity::from_info_name(severity) >= Some(min),
            (Some(_), None) => false,
            (None, _) => true,
        });
        let total = updates.len();
        let limit = self.limit.unwrap_or(usize::MAX);
        updates.truncate(limit);
        UpdateList {
            truncated: total > updates.len(),
            updates,
            total,
        }
    }
}

impl Severity {
    /// Parse a severity from its wire-independent name (used by filters).
    pub fn from_info_name(name: &str) -> Option<Self> {
        match name {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

/// What applying the update set would do to one package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedAction {
    /// Upgrade an installed package.
    Update,
    /// Install a package that is not installed (transitive dependency).
    Install,
    /// Remove a package.
    Remove,
    /// Mark a package obsolete.
    Obsolete,
    /// Downgrade a package.
    Downgrade,
}

impl PlannedAction {
    /// Whether the action crosses the "never install or remove" boundary.
    pub fn is_transitive(self) -> bool {
        matches!(
            self,
            Self::Install | Self::Remove | Self::Obsolete | Self::Downgrade
        )
    }

    /// The wire-independent name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Obsolete => "obsolete",
            Self::Downgrade => "downgrade",
        }
    }
}

/// One entry of an upgrade plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlannedPackage {
    /// Package name.
    pub name: String,
    /// Candidate version.
    pub version: String,
    /// The action applying the plan would take.
    pub action: String,
}

/// The simulated plan of an upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UpgradePlan {
    /// The planned packages.
    pub planned: Vec<PlannedPackage>,
    /// Total number of planned packages.
    pub total: usize,
    /// Whether the plan was truncated by the limit.
    pub truncated: bool,
    /// How many packages would be newly installed.
    pub would_install: usize,
    /// How many packages would be removed.
    pub would_remove: usize,
    /// How many packages would be made obsolete.
    pub would_obsolete: usize,
    /// How many packages would be downgraded.
    pub would_downgrade: usize,
}

impl Default for UpgradePlan {
    fn default() -> Self {
        Self::empty()
    }
}

impl UpgradePlan {
    /// An empty plan (nothing to do).
    pub fn empty() -> Self {
        Self {
            planned: Vec::new(),
            total: 0,
            truncated: false,
            would_install: 0,
            would_remove: 0,
            would_obsolete: 0,
            would_downgrade: 0,
        }
    }

    /// Whether the plan crosses the "never install or remove" boundary.
    pub fn is_transitive(&self) -> bool {
        self.would_install > 0
            || self.would_remove > 0
            || self.would_obsolete > 0
            || self.would_downgrade > 0
    }
}

/// One repository reported by a cache refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Repo {
    /// Repository id.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Whether the repository is enabled.
    pub enabled: bool,
}

/// The result of refreshing the package cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RefreshReport {
    /// Whether the transaction reported success.
    pub success: bool,
    /// The PackageKit exit status, as a name.
    pub exit: String,
    /// PackageKit error code, when one was reported.
    pub error_code: Option<String>,
    /// PackageKit error details, when available.
    pub error_details: Option<String>,
    /// Whether the transaction exceeded the poll timeout.
    pub timed_out: bool,
    /// How long the transaction ran, in milliseconds.
    pub runtime_ms: u64,
    /// Repositories refreshed.
    pub repos: Vec<Repo>,
    /// Whether the repository list was truncated by the limit.
    pub repos_truncated: bool,
}

/// One package acted upon by an upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AffectedPackage {
    /// Package name.
    pub name: String,
    /// Version acted upon.
    pub version: String,
    /// PackageKit info classification.
    pub info: String,
}

/// The result of applying an upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UpgradeReport {
    /// Whether the transaction reported success.
    pub success: bool,
    /// The PackageKit exit status, as a name.
    pub exit: String,
    /// PackageKit error code, when one was reported.
    pub error_code: Option<String>,
    /// PackageKit error details, when available.
    pub error_details: Option<String>,
    /// Whether the transaction exceeded the poll timeout.
    pub timed_out: bool,
    /// How long the transaction ran, in milliseconds.
    pub runtime_ms: u64,
    /// The strongest restart requirement observed, if any.
    pub restart_required: Option<String>,
    /// Packages acted upon.
    pub affected: Vec<AffectedPackage>,
    /// Whether the affected list was truncated by the limit.
    pub affected_truncated: bool,
}

impl UpgradeReport {
    /// A report for a no-op upgrade (nothing available).
    pub fn nothing_to_do() -> Self {
        Self {
            success: true,
            exit: "success".to_string(),
            error_code: None,
            error_details: None,
            timed_out: false,
            runtime_ms: 0,
            restart_required: None,
            affected: Vec::new(),
            affected_truncated: false,
        }
    }
}

/// Errors raised by the PackageKit adapter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    /// The daemon could not be reached or the call could not be made.
    #[error("PackageKit is unavailable: {0}")]
    Unavailable(String),
    /// The daemon speaks a dialect gilfoyle does not implement.
    #[error("unsupported PackageKit version: {0}")]
    Unsupported(String),
    /// The operation was refused for lack of authorisation (polkit).
    #[error("not authorised: {0}")]
    NotAuthorized(String),
    /// An untrusted (unsigned or unverifiable) package was encountered.
    #[error("refused an untrusted package: {0}")]
    Untrusted(String),
    /// A EULA would have to be accepted; gilfoyle never accepts one.
    #[error("refused a EULA that would require acceptance: {0}")]
    Eula(String),
    /// The native package lock is held by another tool.
    #[error("the package database is locked: {0}")]
    Locked(String),
    /// The metadata cache is empty and must be refreshed first.
    #[error("the package metadata cache is empty; run refresh_package_cache first")]
    NoCache,
    /// The upgrade was refused because it would install or remove packages.
    #[error(
        "refusing to upgrade: the plan would install {install}, remove {remove}, \
         obsolete {obsolete} and downgrade {downgrade} package(s); \
         set allow_transitive_changes = true to permit this"
    )]
    TransitiveRefused {
        /// Packages that would be installed.
        install: usize,
        /// Packages that would be removed.
        remove: usize,
        /// Packages that would be made obsolete.
        obsolete: usize,
        /// Packages that would be downgraded.
        downgrade: usize,
    },
    /// The transaction was cancelled.
    #[error("the package transaction was cancelled")]
    Cancelled,
    /// The transaction timed out.
    #[error("the package transaction timed out")]
    Timeout,
    /// The transaction failed.
    #[error("the package transaction failed ({exit}): {details}")]
    Failed {
        /// The exit status name.
        exit: String,
        /// Details, when available.
        details: String,
    },
    /// Any other D-Bus error, kept verbatim.
    #[error("PackageKit error `{name}`: {message}")]
    Other {
        /// The raw D-Bus error name.
        name: String,
        /// The raw D-Bus error message.
        message: String,
    },
}

/// A mutating package operation gilfoyle can be authorised to perform.
///
/// This is a closed vocabulary: there is deliberately no `Install` or `Remove`
/// variant, so no configuration can enable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageOperation {
    /// Refresh the package metadata cache (`apt update`).
    Refresh,
    /// Apply available upgrades (`apt upgrade`).
    Upgrade,
}

impl PackageOperation {
    /// Every operation, in a stable order.
    pub const ALL: [PackageOperation; 2] = [Self::Refresh, Self::Upgrade];

    /// The configuration name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refresh => "refresh",
            Self::Upgrade => "upgrade",
        }
    }

    /// Parse a configuration value. Anything else is unknown.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "refresh" => Ok(Self::Refresh),
            "upgrade" => Ok(Self::Upgrade),
            other => Err(DomainError::UnknownPackageOperation(other.to_string())),
        }
    }
}

/// The set of mutating package operations that are permitted.
///
/// Empty by default, so every mutation is refused until the operator opts in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageAllowlist {
    operations: BTreeSet<PackageOperation>,
}

impl PackageAllowlist {
    /// Build an allowlist from a set of operations.
    pub fn new(operations: impl IntoIterator<Item = PackageOperation>) -> Self {
        Self {
            operations: operations.into_iter().collect(),
        }
    }

    /// Whether the allowlist is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Whether an operation is allowed.
    pub fn contains(&self, operation: PackageOperation) -> bool {
        self.operations.contains(&operation)
    }

    /// The allowed operations, sorted.
    pub fn operations(&self) -> impl Iterator<Item = PackageOperation> + '_ {
        self.operations.iter().copied()
    }

    /// Fail unless the operation is allowed.
    pub fn ensure_allowed(&self, operation: PackageOperation) -> Result<(), AppError> {
        if self.contains(operation) {
            Ok(())
        } else {
            Err(AppError::PackageOperationNotAllowed(
                operation.as_str().to_string(),
            ))
        }
    }
}

/// A decoded PackageKit transaction signal.
///
/// The adapter decodes D-Bus messages into these pure values; everything after
/// that (classification, limits, gating) is decided by [`Collector`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageEvent {
    /// A `Package`/`Packages` signal element.
    Package {
        /// Raw `info` value.
        info: u32,
        /// Raw `severity` value (0 when the dialect has none).
        severity: u32,
        /// Raw package id.
        package_id: String,
        /// One-line summary.
        summary: String,
    },
    /// A repository was reported (refresh only).
    RepoDetail {
        /// Repository id.
        id: String,
        /// Description.
        description: String,
        /// Whether it is enabled.
        enabled: bool,
    },
    /// A restart requirement was reported.
    RequireRestart {
        /// Raw restart type.
        kind: u32,
    },
    /// A fatal error was reported.
    ErrorCode {
        /// Raw error code.
        code: u32,
        /// Details string.
        details: String,
    },
    /// A repository signing key is required (untrusted).
    Untrusted {
        /// Details.
        details: String,
    },
    /// A EULA must be accepted.
    Eula {
        /// Details.
        details: String,
    },
    /// The transaction finished.
    Finished {
        /// Raw exit status.
        exit: u32,
        /// Runtime in milliseconds.
        runtime_ms: u32,
    },
}

/// The selected result shape of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionKind {
    /// `GetPackages(installed)`.
    ListInstalled,
    /// `GetUpdates`.
    CheckUpdates,
    /// `UpdatePackages(simulate)`.
    Plan,
    /// `RefreshCache`.
    Refresh,
    /// `UpdatePackages`.
    Apply,
}

/// Turns a stream of [`PackageEvent`]s into a typed outcome.
#[derive(Debug, Clone)]
pub struct Collector {
    limit: usize,
    packages: Vec<(u32, u32, String)>,
    repos: Vec<Repo>,
    expects_repos: bool,
    total: usize,
    would_install: usize,
    would_remove: usize,
    would_obsolete: usize,
    would_downgrade: usize,
    restart: Option<u32>,
    error: Option<PackageError>,
    exit: Option<u32>,
    runtime_ms: u32,
    timed_out: bool,
}

impl Collector {
    /// Build a collector for a transaction kind and a result limit.
    pub fn new(kind: TransactionKind, limit: usize) -> Self {
        Self {
            limit,
            packages: Vec::new(),
            repos: Vec::new(),
            expects_repos: kind == TransactionKind::Refresh,
            total: 0,
            would_install: 0,
            would_remove: 0,
            would_obsolete: 0,
            would_downgrade: 0,
            restart: None,
            error: None,
            exit: None,
            runtime_ms: 0,
            timed_out: false,
        }
    }

    /// Mark the transaction as having exceeded the poll timeout.
    pub fn mark_timed_out(&mut self) {
        self.timed_out = true;
    }

    /// Feed one decoded event.
    pub fn push(&mut self, event: PackageEvent) {
        match event {
            PackageEvent::Package {
                info,
                severity,
                package_id,
                ..
            } => {
                self.total += 1;
                match planned_action(info) {
                    PlannedAction::Install => self.would_install += 1,
                    PlannedAction::Remove => self.would_remove += 1,
                    PlannedAction::Obsolete => self.would_obsolete += 1,
                    PlannedAction::Downgrade => self.would_downgrade += 1,
                    PlannedAction::Update => {}
                }
                if self.packages.len() < self.limit {
                    self.packages.push((info, severity, package_id));
                }
            }
            PackageEvent::RepoDetail {
                id,
                description,
                enabled,
            } => {
                if self.expects_repos && self.repos.len() < self.limit {
                    self.repos.push(Repo {
                        id,
                        description,
                        enabled,
                    });
                }
            }
            PackageEvent::RequireRestart { kind } => {
                self.restart = Some(match self.restart {
                    Some(current) => current.max(kind),
                    None => kind,
                });
            }
            PackageEvent::ErrorCode { code, details } => {
                self.record_error(map_error_code(code, &details));
            }
            PackageEvent::Untrusted { details } => {
                self.record_error(PackageError::Untrusted(details));
            }
            PackageEvent::Eula { details } => {
                self.record_error(PackageError::Eula(details));
            }
            PackageEvent::Finished { exit, runtime_ms } => {
                self.exit = Some(exit);
                self.runtime_ms = runtime_ms;
            }
        }
    }

    fn record_error(&mut self, error: PackageError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    /// Whether the transaction has finished.
    pub fn is_finished(&self) -> bool {
        self.exit.is_some()
    }

    /// The runtime reported so far.
    pub fn runtime_ms(&self) -> u64 {
        u64::from(self.runtime_ms)
    }

    /// Validate the terminal state, mapping a failed exit to an error.
    fn finish(&self) -> Result<(), PackageError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        match self.exit {
            Some(exit) if exit == exit::SUCCESS => Ok(()),
            Some(exit) => Err(map_exit_code(exit)),
            None => Err(PackageError::Timeout),
        }
    }

    fn parse_packages(&self) -> Vec<(u32, u32, PackageId)> {
        self.packages
            .iter()
            .filter_map(|(info, severity, id)| {
                PackageId::parse(id).ok().map(|id| (*info, *severity, id))
            })
            .collect()
    }

    /// Finalise a `list_packages` transaction.
    pub fn into_package_list(self) -> Result<PackageList, PackageError> {
        self.finish()?;
        let packages: Vec<Package> = self
            .parse_packages()
            .into_iter()
            .map(|(_, _, id)| package_from(&id))
            .collect();
        Ok(PackageList {
            truncated: self.total > packages.len(),
            total: self.total,
            packages,
        })
    }

    /// Finalise a `list_updates` transaction.
    pub fn into_update_list(self) -> Result<UpdateList, PackageError> {
        self.finish()?;
        let updates: Vec<Update> = self
            .parse_packages()
            .into_iter()
            .map(|(info, _, id)| Update {
                id: id.as_str().to_string(),
                name: id.name().to_string(),
                version: id.version().to_string(),
                info: info_name(info).to_string(),
                severity: Severity::from_info(info).map(|s| s.as_str().to_string()),
            })
            .collect();
        Ok(UpdateList {
            truncated: self.total > updates.len(),
            total: self.total,
            updates,
        })
    }

    /// Finalise an upgrade plan (simulated) transaction.
    pub fn into_upgrade_plan(self) -> Result<UpgradePlan, PackageError> {
        self.finish()?;
        let planned = self
            .parse_packages()
            .into_iter()
            .map(|(info, _, id)| PlannedPackage {
                name: id.name().to_string(),
                version: id.version().to_string(),
                action: planned_action(info).as_str().to_string(),
            })
            .collect();
        Ok(UpgradePlan {
            planned,
            total: self.total,
            truncated: self.total > self.packages.len(),
            would_install: self.would_install,
            would_remove: self.would_remove,
            would_obsolete: self.would_obsolete,
            would_downgrade: self.would_downgrade,
        })
    }

    /// Finalise a cache-refresh transaction.
    pub fn into_refresh_report(self) -> Result<RefreshReport, PackageError> {
        let outcome = self.report_prelude()?;
        Ok(RefreshReport {
            success: outcome.success,
            exit: outcome.exit,
            error_code: outcome.error_code,
            error_details: outcome.error_details,
            timed_out: outcome.timed_out,
            runtime_ms: outcome.runtime_ms,
            repos: self.repos,
            repos_truncated: false,
        })
    }

    /// Finalise an applied-upgrade transaction.
    pub fn into_upgrade_report(self) -> Result<UpgradeReport, PackageError> {
        let outcome = self.report_prelude()?;
        let affected: Vec<AffectedPackage> = self
            .parse_packages()
            .into_iter()
            .map(|(info, _, id)| AffectedPackage {
                name: id.name().to_string(),
                version: id.version().to_string(),
                info: info_name(info).to_string(),
            })
            .collect();
        Ok(UpgradeReport {
            success: outcome.success,
            exit: outcome.exit,
            error_code: outcome.error_code,
            error_details: outcome.error_details,
            timed_out: outcome.timed_out,
            runtime_ms: outcome.runtime_ms,
            restart_required: self.restart.map(restart_name).map(str::to_string),
            affected_truncated: self.total > affected.len(),
            affected,
        })
    }

    /// Shared terminal handling for reports: a timeout is reported as a
    /// non-success report rather than an error; a real failure is an error.
    fn report_prelude(&self) -> Result<ReportPrelude, PackageError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.timed_out && self.exit.is_none() {
            return Ok(ReportPrelude {
                success: false,
                exit: "timeout".to_string(),
                error_code: None,
                error_details: None,
                timed_out: true,
                runtime_ms: u64::from(self.runtime_ms),
            });
        }
        let exit = self.exit.ok_or(PackageError::Timeout)?;
        let success = exit == exit::SUCCESS;
        Ok(ReportPrelude {
            success,
            exit: exit_name(exit).to_string(),
            error_code: None,
            error_details: (!success).then(|| map_exit_code(exit).to_string()),
            timed_out: false,
            runtime_ms: u64::from(self.runtime_ms),
        })
    }
}

/// The shared terminal fields of a report.
struct ReportPrelude {
    success: bool,
    exit: String,
    error_code: Option<String>,
    error_details: Option<String>,
    timed_out: bool,
    runtime_ms: u64,
}

fn package_from(id: &PackageId) -> Package {
    Package {
        name: id.name().to_string(),
        version: id.version().to_string(),
        architecture: id.architecture().to_string(),
        origin: id.origin().to_string(),
    }
}

/// Classify a PackageKit `info` value into a planned action.
pub fn planned_action(info: u32) -> PlannedAction {
    match info {
        info::INSTALL => PlannedAction::Install,
        info::REMOVE => PlannedAction::Remove,
        info::OBSOLETE => PlannedAction::Obsolete,
        info::DOWNGRADE => PlannedAction::Downgrade,
        _ => PlannedAction::Update,
    }
}

/// The wire-independent name of a PackageKit `info` value.
pub fn info_name(info: u32) -> &'static str {
    match info {
        info::INSTALLED => "installed",
        info::AVAILABLE => "available",
        info::LOW => "low",
        info::ENHANCEMENT => "enhancement",
        info::NORMAL => "normal",
        info::BUGFIX => "bugfix",
        info::IMPORTANT => "important",
        info::SECURITY => "security",
        info::BLOCKED => "blocked",
        info::UNTRUSTED => "untrusted",
        info::TRUSTED => "trusted",
        info::CRITICAL => "critical",
        info::INSTALL => "install",
        info::REMOVE => "remove",
        info::OBSOLETE => "obsolete",
        info::DOWNGRADE => "downgrade",
        _ => "unknown",
    }
}

/// The wire-independent name of a PackageKit `exit` value.
pub fn exit_name(exit: u32) -> &'static str {
    match exit {
        exit::SUCCESS => "success",
        exit::FAILED => "failed",
        exit::CANCELLED => "cancelled",
        exit::KEY_REQUIRED => "key-required",
        exit::EULA_REQUIRED => "eula-required",
        exit::KILLED => "killed",
        exit::NEED_UNTRUSTED => "need-untrusted",
        _ => "unknown",
    }
}

/// The wire-independent name of a PackageKit `restart` value.
pub fn restart_name(kind: u32) -> &'static str {
    match kind {
        restart::SYSTEM => "system",
        restart::SESSION => "session",
        restart::APPLICATION => "application",
        _ => "none",
    }
}

/// Map a PackageKit exit value to an error.
pub fn map_exit_code(exit: u32) -> PackageError {
    match exit {
        exit::CANCELLED => PackageError::Cancelled,
        exit::KEY_REQUIRED => PackageError::Untrusted("a signing key is required".to_string()),
        exit::EULA_REQUIRED => PackageError::Eula("a EULA must be accepted".to_string()),
        exit::NEED_UNTRUSTED => PackageError::Untrusted("the package is not trusted".to_string()),
        exit::KILLED => PackageError::Failed {
            exit: exit_name(exit).to_string(),
            details: "the transaction was killed".to_string(),
        },
        other => PackageError::Failed {
            exit: exit_name(other).to_string(),
            details: String::new(),
        },
    }
}

/// Map a PackageKit error code to a domain error.
pub fn map_error_code(code: u32, details: &str) -> PackageError {
    match code {
        error_code::NOT_AUTHORIZED => PackageError::NotAuthorized(details.to_string()),
        error_code::BAD_GPG_SIGNATURE | error_code::MISSING_GPG_SIGNATURE => {
            PackageError::Untrusted(details.to_string())
        }
        error_code::NO_CACHE => PackageError::NoCache,
        error_code::CANNOT_GET_LOCK | error_code::LOCK_REQUIRED => {
            PackageError::Locked(details.to_string())
        }
        error_code::NO_PACKAGES_TO_UPDATE => PackageError::Failed {
            exit: "no-packages-to-update".to_string(),
            details: details.to_string(),
        },
        _ => PackageError::Other {
            name: format!("error-code-{code}"),
            message: details.to_string(),
        },
    }
}
