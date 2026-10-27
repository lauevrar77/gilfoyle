use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::{AppError, DomainError};

/// The maximum length systemd accepts for a unit name.
const MAX_UNIT_NAME_LEN: usize = 256;

/// The type of a systemd unit, identified by its name suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitType {
    /// `.service`
    Service,
    /// `.socket`
    Socket,
    /// `.timer`
    Timer,
    /// `.mount`
    Mount,
    /// `.target`
    Target,
    /// `.device`
    Device,
    /// `.slice`
    Slice,
    /// `.scope`
    Scope,
    /// `.path`
    Path,
    /// `.swap`
    Swap,
    /// `.automount`
    Automount,
}

impl UnitType {
    /// Every unit type, in a stable order.
    pub const ALL: [UnitType; 11] = [
        UnitType::Service,
        UnitType::Socket,
        UnitType::Timer,
        UnitType::Mount,
        UnitType::Target,
        UnitType::Device,
        UnitType::Slice,
        UnitType::Scope,
        UnitType::Path,
        UnitType::Swap,
        UnitType::Automount,
    ];

    /// The name suffix, without the leading dot.
    pub fn suffix(self) -> &'static str {
        match self {
            UnitType::Service => "service",
            UnitType::Socket => "socket",
            UnitType::Timer => "timer",
            UnitType::Mount => "mount",
            UnitType::Target => "target",
            UnitType::Device => "device",
            UnitType::Slice => "slice",
            UnitType::Scope => "scope",
            UnitType::Path => "path",
            UnitType::Swap => "swap",
            UnitType::Automount => "automount",
        }
    }

    /// Resolve a suffix (with or without a leading dot) to a unit type.
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        let suffix = suffix.strip_prefix('.').unwrap_or(suffix);
        UnitType::ALL.into_iter().find(|t| t.suffix() == suffix)
    }

    /// Parse a unit-type filter value, accepting a leading dot.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        Self::from_suffix(input.trim())
            .ok_or_else(|| DomainError::UnknownUnitType(input.to_string()))
    }
}

/// A validated, fully-qualified unit name, such as `nginx.service`.
///
/// Short names (`nginx`) are normalised to `.service`; a name with an unknown suffix is
/// rejected rather than guessed at. Template names (`foo@.service`) are not supported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitName {
    name: String,
    unit_type: UnitType,
}

impl UnitName {
    /// Normalise and validate a user-supplied unit name.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_UNIT_NAME_LEN {
            return Err(DomainError::InvalidUnitName(input.to_string()));
        }
        if !trimmed.chars().all(is_unit_name_char) {
            return Err(DomainError::InvalidUnitName(input.to_string()));
        }
        // Templates would let a single allowlist entry cover many units; we do not support them.
        if trimmed.contains("@.") {
            return Err(DomainError::InvalidUnitName(input.to_string()));
        }

        match trimmed.rsplit_once('.') {
            Some((prefix, suffix)) => {
                if prefix.is_empty() {
                    return Err(DomainError::InvalidUnitName(input.to_string()));
                }
                match UnitType::from_suffix(suffix) {
                    Some(unit_type) => Ok(Self {
                        name: trimmed.to_string(),
                        unit_type,
                    }),
                    None => Err(DomainError::UnknownUnitSuffix(input.to_string())),
                }
            }
            None => Ok(Self {
                name: format!("{trimmed}.service"),
                unit_type: UnitType::Service,
            }),
        }
    }

    /// The fully-qualified name.
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The type of this unit.
    pub fn unit_type(&self) -> UnitType {
        self.unit_type
    }
}

fn is_unit_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':' | '\\')
}

/// A unit as reported by a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Unit {
    /// Fully-qualified unit name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Whether systemd could load the unit (`loaded`, `error`, `masked`).
    pub load_state: String,
    /// Lifecycle state (`active`, `inactive`, `failed`, …).
    pub active_state: String,
    /// Unit-type-specific refinement of the active state.
    pub sub_state: String,
}

/// Timestamps of a unit, rendered as RFC 3339 UTC (or `null` when unset).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UnitTimestamps {
    /// When the unit's state last changed.
    pub state_change: Option<String>,
    /// When the unit last entered an active state.
    pub active_enter: Option<String>,
    /// When the unit last entered an inactive state.
    pub inactive_enter: Option<String>,
    /// When the main process last started.
    pub exec_main_start: Option<String>,
    /// When the main process last exited.
    pub exec_main_exit: Option<String>,
}

/// Detailed status of a single unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UnitStatus {
    /// Fully-qualified unit name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Whether systemd could load the unit.
    pub load_state: String,
    /// Lifecycle state.
    pub active_state: String,
    /// Unit-type-specific refinement of the active state.
    pub sub_state: String,
    /// Whether the unit is enabled, disabled, static, masked, …
    pub unit_file_state: Option<String>,
    /// Whether the unit may currently be started.
    pub can_start: Option<bool>,
    /// Whether the unit may currently be stopped.
    pub can_stop: Option<bool>,
    /// Main process id, for service units that are running.
    pub main_pid: Option<u32>,
    /// Result of the unit's last run (`success`, `exit-code`, …).
    pub result: Option<String>,
    /// Relevant timestamps.
    pub timestamps: UnitTimestamps,
}

/// Filter applied to a unit listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitFilter {
    /// Only keep units in this active state.
    pub active_state: Option<String>,
    /// Only keep units whose name contains this substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Only keep units of this type.
    pub unit_type: Option<UnitType>,
    /// Maximum number of units to return; `None` means no limit.
    pub limit: Option<usize>,
}

/// The outcome of a start or stop job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct JobResult {
    /// The raw systemd job result (`done`, `failed`, `canceled`, `timeout`, …).
    pub result: String,
}

impl JobResult {
    /// Wrap a raw systemd job result.
    pub fn new(result: impl Into<String>) -> Self {
        Self {
            result: result.into(),
        }
    }

    /// Whether the job succeeded.
    pub fn is_success(&self) -> bool {
        self.result == "done"
    }
}

/// The result of starting or stopping a unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UnitActionOutcome {
    /// Fully-qualified unit name.
    pub unit: String,
    /// The job result.
    pub job_result: JobResult,
    /// Active state before the operation.
    pub active_state_before: String,
    /// Active state after the operation.
    pub active_state_after: String,
}

/// The set of units that may be started or stopped.
///
/// Empty by default, so every mutation is refused until the operator opts in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlist {
    names: BTreeSet<String>,
}

impl Allowlist {
    /// Build an allowlist from fully-qualified unit names.
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            names: names.into_iter().collect(),
        }
    }

    /// Whether the allowlist is empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The managed unit names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Whether a unit is managed.
    pub fn contains(&self, name: &UnitName) -> bool {
        self.names.contains(name.as_str())
    }

    /// Fail unless the unit is managed.
    pub fn ensure_managed(&self, name: &UnitName) -> Result<(), AppError> {
        if self.contains(name) {
            Ok(())
        } else {
            Err(AppError::NotManaged(name.as_str().to_string()))
        }
    }
}

/// Render microseconds since the Unix epoch as RFC 3339 UTC, or `None` when unset.
pub fn micros_to_rfc3339(micros: u64) -> Option<String> {
    if micros == 0 {
        return None;
    }
    let micros = i64::try_from(micros).ok()?;
    let datetime = chrono::DateTime::from_timestamp_micros(micros)?;
    Some(datetime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}
