use std::path::{Path, PathBuf};

use gilfoyle_core::{SystemError, Uptime, UptimeSource};

/// Default path of the kernel uptime pseudo-file.
pub const DEFAULT_PROC_UPTIME_PATH: &str = "/proc/uptime";

/// Adapter reading the host uptime from `/proc/uptime`.
///
/// The path is injectable so the adapter can be tested against a fixture
/// without touching the real kernel file.
#[derive(Debug, Clone)]
pub struct ProcUptime {
    path: PathBuf,
}

impl ProcUptime {
    /// Read from the real `/proc/uptime`.
    pub fn new() -> Self {
        Self::with_path(DEFAULT_PROC_UPTIME_PATH)
    }

    /// Read from an arbitrary path (used by tests and fixtures).
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path this adapter reads from.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for ProcUptime {
    fn default() -> Self {
        Self::new()
    }
}

impl UptimeSource for ProcUptime {
    fn uptime(&self) -> Result<Uptime, SystemError> {
        let raw =
            std::fs::read_to_string(&self.path).map_err(|e| SystemError::Io(e.to_string()))?;
        parse_uptime(&raw)
    }
}

/// Parse the contents of `/proc/uptime`.
///
/// The file contains two whitespace-separated floats: total uptime in seconds,
/// then cumulative idle time. Only the first field is used. Trailing content
/// (including the idle field) is ignored.
pub fn parse_uptime(input: &str) -> Result<Uptime, SystemError> {
    let token = input.split_whitespace().next().ok_or(SystemError::Empty)?;
    let seconds: f64 = token
        .parse()
        .map_err(|_| SystemError::Malformed(token.to_string()))?;
    Uptime::from_seconds(seconds).map_err(|e| SystemError::Invalid(e.to_string()))
}
