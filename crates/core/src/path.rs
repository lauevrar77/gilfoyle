//! Path metadata domain types and the watched-path allowlist.
//!
//! Gilfoyle reads metadata (never contents) only for the paths the operator
//! declares in `[paths] allow`. A path is an unbounded namespace, so unlike the
//! other reads this one is *allowlisted* and empty by default (see
//! `docs/adr/0012`).

use std::collections::BTreeSet;

use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;

use crate::error::{AppError, DomainError};

/// The prefix that marks an allowlist entry as a regular expression.
const REGEX_PREFIX: &str = "re:";

/// A validated, absolute path with no `..` component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathRef {
    raw: String,
}

impl PathRef {
    /// Normalise and validate a user-supplied path.
    ///
    /// It must be non-empty, absolute, and free of `..` components, so it can
    /// never name a parent of itself.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        if trimmed.is_empty() || !trimmed.starts_with('/') {
            return Err(DomainError::InvalidPath(input.to_string()));
        }
        if trimmed.split('/').any(|component| component == "..") {
            return Err(DomainError::InvalidPath(input.to_string()));
        }
        Ok(Self {
            raw: trimmed.to_string(),
        })
    }

    /// The path as it appears on the wire.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// The kind of filesystem object a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PathKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link (never followed).
    Symlink,
    /// Anything else (socket, FIFO, device, …).
    Other,
}

/// The metadata of one path.
///
/// Metadata only: gilfoyle never reads the contents. When the path does not
/// exist, `exists` is false and every other field is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PathStatus {
    /// The path queried.
    pub path: String,
    /// Whether the path exists.
    pub exists: bool,
    /// The kind of object, when it exists.
    pub kind: Option<PathKind>,
    /// Size in bytes, when it exists.
    pub size_bytes: Option<u64>,
    /// Age in seconds, from the last modification time, when it exists.
    pub age_seconds: Option<i64>,
    /// Last modification time (RFC 3339).
    pub modified: Option<String>,
    /// Last access time (RFC 3339).
    pub accessed: Option<String>,
    /// Last status-change time (RFC 3339).
    pub changed: Option<String>,
    /// Permission bits (the low 12 bits of the mode).
    pub mode: Option<u32>,
    /// Owning user id.
    pub uid: Option<u32>,
    /// Owning group id.
    pub gid: Option<u32>,
    /// Target of a symbolic link, when the path is one.
    pub symlink_target: Option<String>,
}

impl PathStatus {
    /// A path that does not exist.
    pub fn missing(path: &str) -> Self {
        Self {
            path: path.to_string(),
            exists: false,
            kind: None,
            size_bytes: None,
            age_seconds: None,
            modified: None,
            accessed: None,
            changed: None,
            mode: None,
            uid: None,
            gid: None,
            symlink_target: None,
        }
    }
}

/// A compiled allowlist pattern, keeping its source for display.
#[derive(Debug, Clone)]
struct Pattern {
    regex: Regex,
}

/// The set of paths whose metadata may be read.
///
/// Entries are absolute paths (matched exactly) or `re:`-prefixed regular
/// expressions (matched anchored to the whole path). Empty by default, so every
/// path read is refused until the operator opts in.
#[derive(Debug, Clone, Default)]
pub struct PathAllowlist {
    exact: BTreeSet<String>,
    patterns: Vec<Pattern>,
}

impl PathAllowlist {
    /// Build an allowlist from raw `[paths] allow` entries.
    ///
    /// A `re:` prefix marks a regular expression; any other entry is an exact
    /// absolute path. Invalid patterns and relative or `..`-bearing paths fail
    /// the build (fail-closed).
    pub fn parse(entries: impl IntoIterator<Item = String>) -> Result<Self, DomainError> {
        let mut exact = BTreeSet::new();
        let mut patterns = Vec::new();
        for entry in entries {
            match entry.strip_prefix(REGEX_PREFIX) {
                Some(source) => {
                    let regex = compile_pattern(source)
                        .ok_or_else(|| DomainError::InvalidPathPattern(entry.clone()))?;
                    patterns.push(Pattern { regex });
                }
                None => {
                    let path = PathRef::parse(&entry)
                        .map_err(|_| DomainError::InvalidPathEntry(entry.clone()))?;
                    exact.insert(path.raw);
                }
            }
        }
        Ok(Self { exact, patterns })
    }

    /// Whether the allowlist is empty.
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.patterns.is_empty()
    }

    /// The exact paths, sorted.
    pub fn exact_paths(&self) -> impl Iterator<Item = &str> {
        self.exact.iter().map(String::as_str)
    }

    /// Whether a path is allowed.
    pub fn contains(&self, path: &PathRef) -> bool {
        self.exact.contains(path.as_str())
            || self
                .patterns
                .iter()
                .any(|pattern| pattern.regex.is_match(path.as_str()))
    }

    /// Fail unless the path is allowed.
    pub fn ensure_allowed(&self, path: &PathRef) -> Result<(), AppError> {
        if self.contains(path) {
            Ok(())
        } else {
            Err(AppError::PathNotAllowed(path.as_str().to_string()))
        }
    }
}

/// Compile an allowlist pattern anchored to the whole path.
fn compile_pattern(source: &str) -> Option<Regex> {
    if source.is_empty() {
        return None;
    }
    Regex::new(&format!("^(?:{source})$")).ok()
}
