use std::path::{Path, PathBuf};

use gilfoyle_core::{Memory, MemorySource, SystemError};

/// Default path of the kernel memory pseudo-file.
pub const DEFAULT_PROC_MEMINFO_PATH: &str = "/proc/meminfo";

/// Adapter reading host memory usage from `/proc/meminfo`.
///
/// The path is injectable so the adapter can be tested against a fixture
/// without touching the real kernel file.
#[derive(Debug, Clone)]
pub struct ProcMeminfo {
    path: PathBuf,
}

impl ProcMeminfo {
    /// Read from the real `/proc/meminfo`.
    pub fn new() -> Self {
        Self::with_path(DEFAULT_PROC_MEMINFO_PATH)
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

impl Default for ProcMeminfo {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySource for ProcMeminfo {
    fn memory(&self) -> Result<Memory, SystemError> {
        let raw =
            std::fs::read_to_string(&self.path).map_err(|e| SystemError::Io(e.to_string()))?;
        parse_meminfo(&raw)
    }
}

/// Parse the contents of `/proc/meminfo`.
///
/// Only the fields gilfoyle reports are read; every value is in kibibytes.
/// `MemAvailable` is used when present, falling back to `MemFree` on older
/// kernels.
pub fn parse_meminfo(input: &str) -> Result<Memory, SystemError> {
    let mut total = None;
    let mut free = None;
    let mut available = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line in input.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let kib = parse_kib(value)?;
        match key.trim() {
            "MemTotal" => total = Some(kib),
            "MemFree" => free = Some(kib),
            "MemAvailable" => available = Some(kib),
            "SwapTotal" => swap_total = Some(kib),
            "SwapFree" => swap_free = Some(kib),
            _ => {}
        }
    }

    let total = total.ok_or_else(|| SystemError::Malformed("missing MemTotal".to_string()))?;
    let free = free.ok_or_else(|| SystemError::Malformed("missing MemFree".to_string()))?;
    let available = available.unwrap_or(free);
    Ok(Memory::new(
        total,
        free,
        available,
        swap_total.unwrap_or(0),
        swap_free.unwrap_or(0),
    ))
}

/// Parse the numeric kibibytes of a `/proc/meminfo` value (`"1234 kB"`).
fn parse_kib(value: &str) -> Result<u64, SystemError> {
    let token = value.split_whitespace().next().ok_or(SystemError::Empty)?;
    token
        .parse()
        .map_err(|_| SystemError::Malformed(token.to_string()))
}
