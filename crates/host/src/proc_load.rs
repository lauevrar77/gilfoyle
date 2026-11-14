use std::path::{Path, PathBuf};

use gilfoyle_core::{LoadAverage, LoadSource, SystemError};

/// Default path of the kernel load-average pseudo-file.
pub const DEFAULT_PROC_LOADAVG_PATH: &str = "/proc/loadavg";

/// Adapter reading the host load average from `/proc/loadavg`.
///
/// The path is injectable so the adapter can be tested against a fixture
/// without touching the real kernel file.
#[derive(Debug, Clone)]
pub struct ProcLoad {
    path: PathBuf,
}

impl ProcLoad {
    /// Read from the real `/proc/loadavg`.
    pub fn new() -> Self {
        Self::with_path(DEFAULT_PROC_LOADAVG_PATH)
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

impl Default for ProcLoad {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadSource for ProcLoad {
    fn load_average(&self) -> Result<LoadAverage, SystemError> {
        let raw =
            std::fs::read_to_string(&self.path).map_err(|e| SystemError::Io(e.to_string()))?;
        parse_loadavg(&raw)
    }
}

/// Parse the contents of `/proc/loadavg`.
///
/// The file looks like `0.42 0.35 0.30 1/312 12345`: three load averages, then
/// the runnable/total process counts, then the last created pid (ignored).
pub fn parse_loadavg(input: &str) -> Result<LoadAverage, SystemError> {
    let mut fields = input.split_whitespace();
    let one = next_float(&mut fields)?;
    let five = next_float(&mut fields)?;
    let fifteen = next_float(&mut fields)?;
    let counts = fields.next().ok_or(SystemError::Empty)?;
    let (runnable, total) = counts
        .split_once('/')
        .ok_or_else(|| SystemError::Malformed(counts.to_string()))?;
    let runnable = parse_count(runnable)?;
    let total = parse_count(total)?;
    LoadAverage::new(one, five, fifteen, runnable, total)
        .map_err(|error| SystemError::Invalid(error.to_string()))
}

/// Parse the next whitespace-separated field as a float.
fn next_float(fields: &mut std::str::SplitWhitespace<'_>) -> Result<f64, SystemError> {
    let token = fields.next().ok_or(SystemError::Empty)?;
    token
        .parse()
        .map_err(|_| SystemError::Malformed(token.to_string()))
}

/// Parse a process count field.
fn parse_count(token: &str) -> Result<u64, SystemError> {
    token
        .parse()
        .map_err(|_| SystemError::Malformed(token.to_string()))
}
