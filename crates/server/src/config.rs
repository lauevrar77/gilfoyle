//! Configuration loading and validation.
//!
//! Loading is fail-closed: the server refuses to start without at least one
//! usable token, and refuses to bind a non-loopback address unless the
//! operator opts in with `allow_public = true`.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;

use gilfoyle_core::{Allowlist, UnitName};
use serde::Deserialize;
use thiserror::Error;

use crate::auth::TokenEntry;
use crate::hash::{HASH_LEN, parse_hex_32, sha256};

/// Default path of the TOML configuration file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/gilfoyle/config.toml";
/// Default bind address (loopback only).
pub const DEFAULT_BIND: &str = "127.0.0.1:8765";
/// Default log level.
pub const DEFAULT_LOG_LEVEL: &str = "info";
/// Default directory holding per-token hash files.
pub const DEFAULT_TOKEN_DIR: &str = "/etc/gilfoyle/tokens";

/// Configuration errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// The bind address could not be parsed.
    #[error("invalid bind address: {0}")]
    InvalidBind(String),
    /// A non-loopback bind was requested without `allow_public`.
    #[error("refusing to bind non-loopback address {0} without allow_public = true")]
    PublicBindWithoutOptIn(SocketAddr),
    /// No tokens were configured.
    #[error("at least one token must be configured (fail-closed)")]
    NoTokens,
    /// A token id was empty or whitespace.
    #[error("token id must not be empty")]
    EmptyTokenId,
    /// Two tokens shared an id.
    #[error("duplicate token id: {0}")]
    DuplicateTokenId(String),
    /// A configured hash was not 64 hex characters.
    #[error("token `{0}` has an invalid sha256 (expected 64 hex characters)")]
    InvalidHash(String),
    /// No secret could be found for a token.
    #[error(
        "token `{0}` has no secret: set GILFOYLE_TOKEN_<ID>, add a hash file, or set sha256 inline"
    )]
    MissingSecret(String),
    /// A `[systemd] manage_units` entry was not a valid unit name.
    #[error("invalid unit in [systemd] manage_units: {0}")]
    InvalidManagedUnit(String),
}

/// Raw TOML shape, all fields optional so a partial or absent file is valid.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    /// Bind address, e.g. `"127.0.0.1:8765"`.
    #[serde(default)]
    pub bind: Option<String>,
    /// Opt-in required to bind a non-loopback address.
    #[serde(default)]
    pub allow_public: bool,
    /// Host header allowlist for the MCP transport.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Origin header allowlist for the MCP transport.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Log level (tracing filter).
    #[serde(default)]
    pub log_level: Option<String>,
    /// Directory holding per-token hash files.
    #[serde(default)]
    pub token_dir: Option<String>,
    /// Tool exposure settings.
    #[serde(default)]
    pub tools: RawTools,
    /// systemd settings.
    #[serde(default)]
    pub systemd: RawSystemd,
    /// Configured tokens.
    #[serde(default)]
    pub tokens: Vec<RawToken>,
}

/// Raw `[systemd]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSystemd {
    /// Units that may be started or stopped. Empty means none may.
    #[serde(default)]
    pub manage_units: Vec<String>,
}

/// Raw `[tools]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTools {
    /// Explicit allowlist of enabled tools; `None` means "all compiled tools".
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
}

/// Raw `[[tokens]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawToken {
    /// Stable identifier for the token (used for audit and revocation).
    pub id: String,
    /// Optional inline SHA-256 hash (64 hex characters) of the secret.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Fully resolved, validated configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Address the server binds to.
    pub bind: SocketAddr,
    /// Whether binding a non-loopback address was explicitly allowed.
    pub allow_public: bool,
    /// Host header allowlist.
    pub allowed_hosts: Vec<String>,
    /// Origin header allowlist.
    pub allowed_origins: Vec<String>,
    /// Log level.
    pub log_level: String,
    /// Enabled tool allowlist; `None` means all compiled tools.
    pub enabled_tools: Option<BTreeSet<String>>,
    /// Units that may be started or stopped.
    pub allowlist: Allowlist,
    /// Resolved token entries.
    pub tokens: Vec<TokenEntry>,
}

/// Parse a bind address.
pub fn parse_bind(input: &str) -> Result<SocketAddr, ConfigError> {
    input
        .parse()
        .map_err(|_| ConfigError::InvalidBind(input.to_string()))
}

/// Resolve a raw configuration into a validated [`Config`].
///
/// `env` and `read_file` are injected so the resolution logic stays pure and
/// unit-testable without touching the process environment or the filesystem.
pub fn resolve_config(
    raw: RawConfig,
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
) -> Result<Config, ConfigError> {
    let bind_str = env("GILFOYLE_BIND")
        .or(raw.bind)
        .unwrap_or_else(|| DEFAULT_BIND.to_string());
    let bind = parse_bind(&bind_str)?;
    if !raw.allow_public && !bind.ip().is_loopback() {
        return Err(ConfigError::PublicBindWithoutOptIn(bind));
    }

    let log_level = env("GILFOYLE_LOG")
        .or(raw.log_level)
        .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string());

    let token_dir = raw
        .token_dir
        .unwrap_or_else(|| DEFAULT_TOKEN_DIR.to_string());
    let tokens = resolve_tokens(&raw.tokens, env, read_file, Path::new(&token_dir))?;
    let allowlist = resolve_allowlist(&raw.systemd.manage_units)?;

    Ok(Config {
        bind,
        allow_public: raw.allow_public,
        allowed_hosts: raw.allowed_hosts,
        allowed_origins: raw.allowed_origins,
        log_level,
        enabled_tools: raw.tools.enabled.map(|ids| ids.into_iter().collect()),
        allowlist,
        tokens,
    })
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

fn resolve_tokens(
    raw_tokens: &[RawToken],
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
    token_dir: &Path,
) -> Result<Vec<TokenEntry>, ConfigError> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut entries = Vec::with_capacity(raw_tokens.len());
    for raw in raw_tokens {
        if raw.id.trim().is_empty() {
            return Err(ConfigError::EmptyTokenId);
        }
        if !seen.insert(raw.id.as_str()) {
            return Err(ConfigError::DuplicateTokenId(raw.id.clone()));
        }
        let hash = resolve_token_hash(raw, env, read_file, token_dir)?;
        entries.push(TokenEntry {
            id: raw.id.clone(),
            hash,
        });
    }
    if entries.is_empty() {
        return Err(ConfigError::NoTokens);
    }
    Ok(entries)
}

fn resolve_token_hash(
    raw: &RawToken,
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
    token_dir: &Path,
) -> Result<[u8; HASH_LEN], ConfigError> {
    if let Some(hex) = &raw.sha256 {
        return parse_hex_32(hex.trim()).ok_or_else(|| ConfigError::InvalidHash(raw.id.clone()));
    }

    let env_key = format!("GILFOYLE_TOKEN_{}", raw.id.to_uppercase());
    if let Some(plaintext) = env(&env_key) {
        return Ok(sha256(plaintext.as_bytes()));
    }

    let path = token_dir.join(&raw.id);
    if let Some(contents) = read_file(&path) {
        return parse_hex_32(contents.trim())
            .ok_or_else(|| ConfigError::InvalidHash(raw.id.clone()));
    }

    Err(ConfigError::MissingSecret(raw.id.clone()))
}
