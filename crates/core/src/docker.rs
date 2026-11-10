//! Docker domain types and the container allowlist.
//!
//! Gilfoyle reaches the Docker daemon over its UNIX socket; this module holds
//! everything that is *not* transport: the validated container reference, the
//! listing/status types, the closed set of actions, and the allowlist of
//! containers that may be started, stopped or restarted.
//!
//! There is deliberately no `create` and no `remove`: the action vocabulary is
//! closed, so no configuration can widen it. Reads are open; mutations are
//! default-deny.

use std::collections::BTreeSet;

use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;

use crate::error::{AppError, DomainError};

/// The maximum length Docker accepts for a container name.
const MAX_CONTAINER_REF_LEN: usize = 255;

/// The prefix that marks an allowlist entry as a regular expression.
const REGEX_PREFIX: &str = "re:";

/// A validated reference to a container: a name or an id.
///
/// Docker accepts either in its API paths. Only the character set of a
/// container name is enforced here (ids are a subset of it), so one type covers
/// both. A leading `/` (as Docker reports names) is accepted and stripped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerRef {
    raw: String,
}

impl ContainerRef {
    /// Normalise and validate a user-supplied container reference.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim().strip_prefix('/').unwrap_or(input.trim());
        if !is_valid_container_name(trimmed) {
            return Err(DomainError::InvalidContainerRef(input.to_string()));
        }
        Ok(Self {
            raw: trimmed.to_string(),
        })
    }

    /// The reference as it appears on the wire.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// Whether a string is a syntactically valid Docker container name.
///
/// Docker requires `[a-zA-Z0-9][a-zA-Z0-9_.-]*`, bounded in length. Container
/// ids (`[0-9a-f]{12,64}`) satisfy it too.
fn is_valid_container_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_CONTAINER_REF_LEN {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(is_container_name_char)
}

/// The characters Docker allows after the first one in a container name.
fn is_container_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// The lifecycle state of a container, as reported by Docker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainerState {
    /// Created but never started.
    Created,
    /// Running.
    Running,
    /// Paused.
    Paused,
    /// Restarting.
    Restarting,
    /// Being removed.
    Removing,
    /// Exited.
    Exited,
    /// Dead.
    Dead,
}

impl ContainerState {
    /// Every state, in a stable order.
    pub const ALL: [ContainerState; 7] = [
        ContainerState::Created,
        ContainerState::Running,
        ContainerState::Paused,
        ContainerState::Restarting,
        ContainerState::Removing,
        ContainerState::Exited,
        ContainerState::Dead,
    ];

    /// The wire value Docker uses.
    pub fn as_str(self) -> &'static str {
        match self {
            ContainerState::Created => "created",
            ContainerState::Running => "running",
            ContainerState::Paused => "paused",
            ContainerState::Restarting => "restarting",
            ContainerState::Removing => "removing",
            ContainerState::Exited => "exited",
            ContainerState::Dead => "dead",
        }
    }

    /// Parse a state filter value. Anything else is rejected (fail-closed).
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let normalised = input.trim().to_ascii_lowercase();
        ContainerState::ALL
            .into_iter()
            .find(|state| state.as_str() == normalised)
            .ok_or_else(|| DomainError::UnknownContainerState(input.to_string()))
    }
}

/// A container as reported by a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Container {
    /// Container id.
    pub id: String,
    /// Container names, without the leading `/` Docker reports.
    pub names: Vec<String>,
    /// Image reference.
    pub image: String,
    /// Lifecycle state (`running`, `exited`, …).
    pub state: String,
    /// Human-readable status (`Up 3 hours`, `Exited (0) 2 minutes ago`).
    pub status: String,
}

/// A listing of containers.
///
/// MCP requires the root of a tool's `outputSchema` to be an object, so a
/// listing is wrapped rather than returned as a bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ContainerList {
    /// The matching containers.
    pub containers: Vec<Container>,
}

/// Detailed status of a single container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ContainerStatus {
    /// Container id.
    pub id: String,
    /// Canonical name, without the leading `/`.
    pub name: String,
    /// Image reference from the container configuration.
    pub image: String,
    /// Lifecycle state (`running`, `exited`, …).
    pub state: String,
    /// Human-readable status; mirrors `state` for a status query, since Docker's
    /// inspect reply carries only the machine state.
    pub status: String,
    /// Health status, when the container defines a health check.
    pub health: Option<String>,
    /// Main process id, when the container is running.
    pub pid: Option<u32>,
    /// Exit code of the main process (0 while running).
    pub exit_code: i32,
    /// How many times the container has restarted.
    pub restart_count: u64,
    /// When the container was created (RFC 3339).
    pub created: String,
    /// When the main process started, if it ever did.
    pub started_at: Option<String>,
    /// When the main process last exited, if it ever did.
    pub finished_at: Option<String>,
}

/// Filter applied to a container listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerFilter {
    /// Only keep containers in this lifecycle state.
    pub state: Option<ContainerState>,
    /// Only keep containers whose name contains this substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Maximum number of containers to return; `None` means no limit.
    pub limit: Option<usize>,
}

impl ContainerFilter {
    /// Apply the client-side parts of the filter.
    pub fn apply(&self, containers: Vec<Container>) -> Vec<Container> {
        let needle = self.name_contains.as_ref().map(|n| n.to_lowercase());
        let mut filtered: Vec<Container> = containers
            .into_iter()
            .filter(|container| match self.state {
                Some(state) => container.state == state.as_str(),
                None => true,
            })
            .filter(|container| match &needle {
                Some(needle) => container
                    .names
                    .iter()
                    .any(|name| name.to_lowercase().contains(needle)),
                None => true,
            })
            .collect();
        if let Some(limit) = self.limit {
            filtered.truncate(limit);
        }
        filtered
    }
}

/// A lifecycle action gilfoyle can perform on a container.
///
/// This is a closed vocabulary: there is deliberately no `Create` or `Remove`
/// variant, so no configuration can enable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContainerAction {
    /// Start a stopped container.
    Start,
    /// Stop a running container.
    Stop,
    /// Restart a container.
    Restart,
}

impl ContainerAction {
    /// Every action, in a stable order.
    pub const ALL: [ContainerAction; 3] = [
        ContainerAction::Start,
        ContainerAction::Stop,
        ContainerAction::Restart,
    ];

    /// The configuration and wire-independent name.
    pub fn as_str(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        }
    }
}

/// The identity of a resolved container: its id, names and current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerIdentity {
    /// Container id.
    pub id: String,
    /// All names reported by Docker, without the leading `/`.
    pub names: Vec<String>,
    /// Current lifecycle state.
    pub state: String,
}

impl ContainerIdentity {
    /// The canonical (primary) name, if Docker reported one.
    pub fn canonical_name(&self) -> &str {
        self.names.first().map(String::as_str).unwrap_or(&self.id)
    }
}

/// The result of a start, stop or restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ContainerActionOutcome {
    /// Canonical container name.
    pub container: String,
    /// Container id.
    pub id: String,
    /// The action performed (`start`, `stop`, `restart`).
    pub action: String,
    /// Lifecycle state before the operation.
    pub state_before: String,
    /// Lifecycle state after the operation.
    pub state_after: String,
}

/// A compiled allowlist regex, keeping its source for display.
#[derive(Debug, Clone)]
struct Pattern {
    source: String,
    regex: Regex,
}

/// The set of containers that may be started, stopped or restarted.
///
/// Entries are held as plain names (matched by equality) or as `re:`-prefixed
/// regular expressions (matched anchored to the whole name). Empty by default,
/// so every mutation is refused until the operator opts in.
#[derive(Debug, Clone, Default)]
pub struct ContainerAllowlist {
    exact: BTreeSet<String>,
    patterns: Vec<Pattern>,
}

impl ContainerAllowlist {
    /// Build an allowlist from raw `[docker] manage_containers` entries.
    ///
    /// A `re:` prefix marks a regular expression; the pattern is wrapped in
    /// `^(?:…)$` so it must match the whole name. Any other entry is an exact
    /// name. Invalid patterns and names fail the build (fail-closed).
    pub fn parse(entries: impl IntoIterator<Item = String>) -> Result<Self, DomainError> {
        let mut exact = BTreeSet::new();
        let mut patterns = Vec::new();
        for entry in entries {
            match entry.strip_prefix(REGEX_PREFIX) {
                Some(source) => {
                    let regex = compile_pattern(source)
                        .ok_or_else(|| DomainError::InvalidContainerPattern(entry.clone()))?;
                    patterns.push(Pattern {
                        source: source.to_string(),
                        regex,
                    });
                }
                None => {
                    if !is_valid_container_name(&entry) {
                        return Err(DomainError::InvalidContainerRef(entry));
                    }
                    exact.insert(entry);
                }
            }
        }
        Ok(Self { exact, patterns })
    }

    /// Whether the allowlist is empty.
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.patterns.is_empty()
    }

    /// The exact names, sorted.
    pub fn exact_names(&self) -> impl Iterator<Item = &str> {
        self.exact.iter().map(String::as_str)
    }

    /// The regular expression sources, in order.
    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.patterns.iter().map(|pattern| pattern.source.as_str())
    }

    /// Whether a container name is managed.
    pub fn contains(&self, name: &str) -> bool {
        self.exact.contains(name)
            || self
                .patterns
                .iter()
                .any(|pattern| pattern.regex.is_match(name))
    }

    /// Fail unless at least one of the container's names is managed.
    pub fn ensure_managed(&self, names: &[String]) -> Result<(), AppError> {
        if names.iter().any(|name| self.contains(name)) {
            Ok(())
        } else {
            Err(AppError::ContainerNotManaged(names.join(", ")))
        }
    }
}

/// Compile an allowlist pattern anchored to the whole name.
fn compile_pattern(source: &str) -> Option<Regex> {
    if source.is_empty() {
        return None;
    }
    Regex::new(&format!("^(?:{source})$")).ok()
}
