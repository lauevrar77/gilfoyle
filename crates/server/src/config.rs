//! Configuration loading and validation.
//!
//! Loading is fail-closed: the server refuses to start without a `bind` address and at
//! least one authorized key. A configuration from the HTTP era (tokens, host/origin
//! allowlists, `allow_public`) is rejected with a migration message.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;

use gilfoyle_core::{
    Allowlist, ContainerAllowlist, DomainError, PackageAllowlist, PackageOperation, UnitName,
};
use gilfoyle_ssh::{AuthorizedKeys, KeyError};
use serde::Deserialize;
use thiserror::Error;

/// Default path of the TOML configuration file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/gilfoyle/config.toml";
/// Default path of the SSH host key.
pub const DEFAULT_HOST_KEY_PATH: &str = "/etc/gilfoyle/ssh_host_ed25519_key";
/// Default log level.
pub const DEFAULT_LOG_LEVEL: &str = "info";
/// Default cap on simultaneous connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 32;
/// Default cap on channels (MCP sessions) per connection.
pub const DEFAULT_MAX_CHANNELS_PER_CONNECTION: usize = 8;
/// Default path of the Docker daemon socket.
pub const DEFAULT_DOCKER_ENDPOINT: &str = "/var/run/docker.sock";
/// Default timeout for a single Docker API call, in seconds.
pub const DEFAULT_DOCKER_CALL_TIMEOUT_SECONDS: u64 = 10;
/// Default timeout for a Docker stop, in seconds.
pub const DEFAULT_DOCKER_STOP_TIMEOUT_SECONDS: u64 = 30;
/// Default number of containers returned by `list_containers`.
pub const DEFAULT_DOCKER_LIMIT: usize = 100;

/// Every tool a node can expose.
pub const KNOWN_TOOLS: [&str; 15] = [
    "get_uptime",
    "list_units",
    "unit_status",
    "start_unit",
    "stop_unit",
    "list_packages",
    "list_updates",
    "preview_upgrade",
    "refresh_package_cache",
    "upgrade_packages",
    "list_containers",
    "container_status",
    "start_container",
    "stop_container",
    "restart_container",
];

/// The known tools as a set.
pub fn known_tools() -> BTreeSet<String> {
    KNOWN_TOOLS.iter().map(|name| name.to_string()).collect()
}

/// Configuration errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// No bind address was configured.
    #[error("no bind address: set `bind` to an `address:port`")]
    MissingBind,
    /// The bind address could not be parsed.
    #[error("invalid bind address: {0}")]
    InvalidBind(String),
    /// No authorized keys were configured.
    #[error("at least one authorized key must be configured (fail-closed)")]
    NoKeys,
    /// An authorized key id was empty.
    #[error("authorized key id must not be empty")]
    EmptyKeyId,
    /// Two authorized keys shared an id.
    #[error("duplicate authorized key id: {0}")]
    DuplicateKeyId(String),
    /// A public key could not be parsed.
    #[error("key `{0}` has an invalid public key: {1}")]
    InvalidPublicKey(String, String),
    /// A key granted a tool that does not exist.
    #[error("key `{0}` grants an unknown tool: `{1}`")]
    UnknownTool(String, String),
    /// `[tools] enabled` listed a tool that does not exist.
    #[error("`[tools] enabled` lists an unknown tool: `{0}`")]
    UnknownEnabledTool(String),
    /// A `[systemd] manage_units` entry was not a valid unit name.
    #[error("invalid unit in [systemd] manage_units: {0}")]
    InvalidManagedUnit(String),
    /// A `[packagekit] allow` entry was not `refresh` or `upgrade`.
    #[error("invalid operation in [packagekit] allow: {0}")]
    InvalidPackageOperation(String),
    /// The Docker endpoint was not an absolute socket path.
    #[error("invalid Docker endpoint; expected an absolute socket path, got: {0}")]
    InvalidDockerEndpoint(String),
    /// A `[docker] manage_containers` entry was not a valid name or `re:` pattern.
    #[error("invalid entry in [docker] manage_containers: {0}")]
    InvalidContainerEntry(String),
    /// A field from the HTTP-era configuration was found.
    #[error("`{0}` is from the old HTTP configuration and is no longer supported; remove it")]
    LegacyField(String),
}

/// Raw TOML shape, all fields optional so a partial file is valid.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    /// Bind address, e.g. `"0.0.0.0:2222"`. Mandatory.
    #[serde(default)]
    pub bind: Option<String>,
    /// Path to the SSH host key.
    #[serde(default)]
    pub host_key: Option<String>,
    /// Log level (tracing filter).
    #[serde(default)]
    pub log_level: Option<String>,
    /// Cap on simultaneous connections.
    #[serde(default)]
    pub max_connections: Option<usize>,
    /// Cap on channels per connection.
    #[serde(default)]
    pub max_channels_per_connection: Option<usize>,
    /// Tool exposure settings.
    #[serde(default)]
    pub tools: RawTools,
    /// systemd settings.
    #[serde(default)]
    pub systemd: RawSystemd,
    /// package settings.
    #[serde(default)]
    pub packagekit: RawPackagekit,
    /// Docker settings.
    #[serde(default)]
    pub docker: RawDocker,
    /// Authorized client keys.
    #[serde(default)]
    pub keys: Vec<RawKey>,
    /// Legacy (HTTP era) field, rejected when present.
    #[serde(default)]
    pub tokens: Option<toml::Value>,
    /// Legacy (HTTP era) field, rejected when present.
    #[serde(default)]
    pub allow_public: Option<bool>,
    /// Legacy (HTTP era) field, rejected when present.
    #[serde(default)]
    pub allowed_hosts: Option<toml::Value>,
    /// Legacy (HTTP era) field, rejected when present.
    #[serde(default)]
    pub allowed_origins: Option<toml::Value>,
    /// Legacy (HTTP era) field, rejected when present.
    #[serde(default)]
    pub token_dir: Option<String>,
}

/// Raw `[systemd]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSystemd {
    /// Units that may be started or stopped. Empty means none may.
    #[serde(default)]
    pub manage_units: Vec<String>,
}

/// Raw `[packagekit]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPackagekit {
    /// Mutating operations that are permitted. Empty means none are.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Whether an upgrade may install or remove packages transitively.
    #[serde(default)]
    pub allow_transitive_changes: bool,
    /// Timeout for a whole package transaction, in seconds.
    #[serde(default)]
    pub poll_timeout_seconds: Option<u64>,
    /// Maximum number of package entries returned in a report.
    #[serde(default)]
    pub default_limit: Option<usize>,
}

/// Raw `[tools]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTools {
    /// Explicit allowlist of enabled tools; `None` means all compiled tools.
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
}

/// Raw `[docker]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDocker {
    /// Docker daemon socket path.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Containers that may be started, stopped or restarted. Entries are names
    /// or `re:<regex>` patterns. Empty means none may.
    #[serde(default)]
    pub manage_containers: Vec<String>,
    /// Timeout for a single API call, in seconds.
    #[serde(default)]
    pub call_timeout_seconds: Option<u64>,
    /// Timeout for a stop, in seconds.
    #[serde(default)]
    pub stop_timeout_seconds: Option<u64>,
    /// Maximum number of containers returned by a listing.
    #[serde(default)]
    pub default_limit: Option<usize>,
}

/// Raw `[[keys]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawKey {
    /// Stable identifier, used as the principal and in logs.
    pub id: String,
    /// An OpenSSH public key line.
    pub public_key: String,
    /// Optional tool grant; `None` inherits `[tools] enabled`.
    #[serde(default)]
    pub tools: Option<Vec<String>>,
}

/// Fully resolved, validated configuration.
#[derive(Debug)]
pub struct Config {
    /// Address the server binds to.
    pub bind: SocketAddr,
    /// Path to the SSH host key.
    pub host_key: PathBuf,
    /// Log level.
    pub log_level: String,
    /// Cap on simultaneous connections.
    pub max_connections: usize,
    /// Cap on channels per connection.
    pub max_channels_per_connection: usize,
    /// Tools enabled globally; each key inherits or narrows this.
    pub enabled_tools: BTreeSet<String>,
    /// Units that may be started or stopped.
    pub allowlist: Allowlist,
    /// Package operations that may be performed.
    pub package_allowlist: PackageAllowlist,
    /// Whether an upgrade may install or remove packages transitively.
    pub allow_transitive_changes: bool,
    /// Timeout for a whole package transaction, in seconds.
    pub package_poll_timeout_seconds: Option<u64>,
    /// Maximum number of package entries returned in a report.
    pub package_default_limit: Option<usize>,
    /// Docker daemon socket path.
    pub docker_endpoint: PathBuf,
    /// Containers that may be started, stopped or restarted.
    pub container_allowlist: ContainerAllowlist,
    /// Timeout for a single Docker API call, in seconds.
    pub docker_call_timeout_seconds: Option<u64>,
    /// Timeout for a Docker stop, in seconds.
    pub docker_stop_timeout_seconds: Option<u64>,
    /// Maximum number of containers returned by a listing.
    pub docker_default_limit: Option<usize>,
    /// Authorized client keys.
    pub keys: AuthorizedKeys,
}

/// Parse a bind address.
pub fn parse_bind(input: &str) -> Result<SocketAddr, ConfigError> {
    input
        .trim()
        .parse()
        .map_err(|_| ConfigError::InvalidBind(input.to_string()))
}

/// Resolve a raw configuration into a validated [`Config`].
///
/// `env` is injected so the resolution logic stays pure and unit-testable.
pub fn resolve_config(
    raw: RawConfig,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, ConfigError> {
    reject_legacy(&raw)?;

    let bind_str = env("GILFOYLE_BIND")
        .or(raw.bind)
        .ok_or(ConfigError::MissingBind)?;
    let bind = parse_bind(&bind_str)?;

    let host_key = PathBuf::from(
        env("GILFOYLE_HOST_KEY")
            .or(raw.host_key)
            .unwrap_or_else(|| DEFAULT_HOST_KEY_PATH.to_string()),
    );

    let log_level = env("GILFOYLE_LOG")
        .or(raw.log_level)
        .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string());

    Ok(Config {
        bind,
        host_key,
        log_level,
        max_connections: raw.max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS),
        max_channels_per_connection: raw
            .max_channels_per_connection
            .unwrap_or(DEFAULT_MAX_CHANNELS_PER_CONNECTION),
        enabled_tools: resolve_enabled(raw.tools.enabled)?,
        allowlist: resolve_allowlist(&raw.systemd.manage_units)?,
        package_allowlist: resolve_package_allowlist(&raw.packagekit.allow)?,
        allow_transitive_changes: raw.packagekit.allow_transitive_changes,
        package_poll_timeout_seconds: raw.packagekit.poll_timeout_seconds,
        package_default_limit: raw.packagekit.default_limit,
        docker_endpoint: resolve_docker_endpoint(raw.docker.endpoint)?,
        container_allowlist: resolve_container_allowlist(&raw.docker.manage_containers)?,
        docker_call_timeout_seconds: raw.docker.call_timeout_seconds,
        docker_stop_timeout_seconds: raw.docker.stop_timeout_seconds,
        docker_default_limit: raw.docker.default_limit,
        keys: resolve_keys(&raw.keys)?,
    })
}

fn reject_legacy(raw: &RawConfig) -> Result<(), ConfigError> {
    let legacy = [
        (raw.tokens.is_some(), "[[tokens]]"),
        (raw.allow_public.is_some(), "allow_public"),
        (raw.allowed_hosts.is_some(), "allowed_hosts"),
        (raw.allowed_origins.is_some(), "allowed_origins"),
        (raw.token_dir.is_some(), "token_dir"),
    ];
    for (present, name) in legacy {
        if present {
            return Err(ConfigError::LegacyField(name.to_string()));
        }
    }
    Ok(())
}

/// Resolve the global tool set, defaulting to every known tool.
pub fn resolve_enabled(enabled: Option<Vec<String>>) -> Result<BTreeSet<String>, ConfigError> {
    match enabled {
        None => Ok(known_tools()),
        Some(names) => {
            let known = known_tools();
            let mut set = BTreeSet::new();
            for name in names {
                if !known.contains(&name) {
                    return Err(ConfigError::UnknownEnabledTool(name));
                }
                set.insert(name);
            }
            Ok(set)
        }
    }
}

/// Normalise the managed-unit allowlist. Invalid names fail the load (fail-closed).
pub fn resolve_allowlist(units: &[String]) -> Result<Allowlist, ConfigError> {
    let mut names = BTreeSet::new();
    for unit in units {
        let name =
            UnitName::parse(unit).map_err(|_| ConfigError::InvalidManagedUnit(unit.clone()))?;
        names.insert(name.as_str().to_string());
    }
    Ok(Allowlist::new(names))
}

/// Normalise the package-operation allowlist. Unknown operations fail the load
/// (fail-closed), and there is no `install` variant to enable.
pub fn resolve_package_allowlist(operations: &[String]) -> Result<PackageAllowlist, ConfigError> {
    let mut resolved = BTreeSet::new();
    for operation in operations {
        let parsed = PackageOperation::parse(operation)
            .map_err(|_| ConfigError::InvalidPackageOperation(operation.clone()))?;
        resolved.insert(parsed);
    }
    Ok(PackageAllowlist::new(resolved))
}

/// Normalise the Docker endpoint. Only an absolute UNIX socket path is
/// accepted; a scheme (`tcp://`, `http://`, …) is refused (fail-closed).
pub fn resolve_docker_endpoint(endpoint: Option<String>) -> Result<PathBuf, ConfigError> {
    let endpoint = endpoint.unwrap_or_else(|| DEFAULT_DOCKER_ENDPOINT.to_string());
    let trimmed = endpoint.trim();
    if trimmed.contains("://") || !trimmed.starts_with('/') {
        return Err(ConfigError::InvalidDockerEndpoint(endpoint));
    }
    Ok(PathBuf::from(trimmed))
}

/// Build the container allowlist. Invalid names or patterns fail the load
/// (fail-closed).
pub fn resolve_container_allowlist(entries: &[String]) -> Result<ContainerAllowlist, ConfigError> {
    ContainerAllowlist::parse(entries.to_vec()).map_err(|error| match error {
        DomainError::InvalidContainerRef(entry) | DomainError::InvalidContainerPattern(entry) => {
            ConfigError::InvalidContainerEntry(entry)
        }
        other => ConfigError::InvalidContainerEntry(other.to_string()),
    })
}

fn resolve_keys(raw: &[RawKey]) -> Result<AuthorizedKeys, ConfigError> {
    if raw.is_empty() {
        return Err(ConfigError::NoKeys);
    }
    let known = known_tools();
    let mut entries = Vec::with_capacity(raw.len());
    for key in raw {
        let mut entry =
            AuthorizedKeys::parse_key(key.id.clone(), &key.public_key).map_err(map_key_error)?;
        if let Some(tools) = &key.tools {
            let mut grant = BTreeSet::new();
            for tool in tools {
                if !known.contains(tool) {
                    return Err(ConfigError::UnknownTool(key.id.clone(), tool.clone()));
                }
                grant.insert(tool.clone());
            }
            entry.tools = Some(grant);
        }
        entries.push(entry);
    }
    AuthorizedKeys::new(entries).map_err(map_key_error)
}

fn map_key_error(error: KeyError) -> ConfigError {
    match error {
        KeyError::EmptyId => ConfigError::EmptyKeyId,
        KeyError::DuplicateId(id) => ConfigError::DuplicateKeyId(id),
        KeyError::InvalidKey(id, message) => ConfigError::InvalidPublicKey(id, message),
    }
}
