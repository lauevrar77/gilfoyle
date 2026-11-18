//! Reboot-required probe.
//!
//! On Debian-family hosts the presence of `/run/reboot-required` signals a
//! pending reboot, and `/run/reboot-required.pkgs` lists the packages that asked
//! for it. On any other host gilfoyle has no in-process mechanism (the RPM world
//! computes this with a tool gilfoyle will not spawn), so it reports the verdict
//! as *unknown* rather than guessing "no" (see `docs/adr/0014`).

use std::path::{Path, PathBuf};

use gilfoyle_core::{RebootError, RebootSource, RebootStatus};

/// Default path of the distribution identity file.
pub const DEFAULT_OS_RELEASE_PATH: &str = "/etc/os-release";
/// Default path of the reboot-required marker.
pub const DEFAULT_REBOOT_REQUIRED_PATH: &str = "/run/reboot-required";
/// Default path of the list of packages that asked for the reboot.
pub const DEFAULT_REBOOT_REQUIRED_PKGS_PATH: &str = "/run/reboot-required.pkgs";

/// Maximum number of package names reported.
const MAX_PACKAGES: usize = 100;

/// The parts of an `os-release` file gilfoyle reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsRelease {
    /// The `ID` field (`debian`, `ubuntu`, …).
    pub id: Option<String>,
    /// The `ID_LIKE` field, a space-separated list of related ids.
    pub id_like: Option<String>,
}

impl OsRelease {
    /// Whether this is a Debian-family distribution (Debian, Ubuntu, Mint, …).
    pub fn is_debian_like(&self) -> bool {
        if self.id.as_deref().is_some_and(is_debian_id) {
            return true;
        }
        self.id_like
            .as_deref()
            .is_some_and(|list| list.split_whitespace().any(is_debian_id))
    }
}

fn is_debian_id(id: &str) -> bool {
    id == "debian" || id == "ubuntu"
}

/// Parse the `ID` and `ID_LIKE` fields of an `os-release` file.
///
/// The format is `KEY=VALUE`, one per line; a value may be wrapped in matching
/// single or double quotes. A line without `=`, or whose key is not `ID` or
/// `ID_LIKE` (including a `#` comment), is ignored.
pub fn parse_os_release(text: &str) -> OsRelease {
    let mut release = OsRelease::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "ID" => release.id = Some(unquote(value.trim())),
            "ID_LIKE" => release.id_like = Some(unquote(value.trim())),
            _ => {}
        }
    }
    release
}

/// Strip one pair of matching surrounding quotes, if present.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// Parse the newline-separated package names in `/run/reboot-required.pkgs`.
///
/// Blank lines are dropped, the list is capped at [`MAX_PACKAGES`], and the
/// names are treated as opaque labels — gilfoyle never interprets them.
pub fn parse_package_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .take(MAX_PACKAGES)
        .map(str::to_string)
        .collect()
}

/// Adapter reporting whether the host needs a reboot.
///
/// The three paths are injectable so the adapter can be tested against fixtures
/// without touching the real host.
pub struct RebootProbe {
    os_release: PathBuf,
    marker: PathBuf,
    packages: PathBuf,
}

impl RebootProbe {
    /// Read the real `/etc/os-release` and `/run` markers.
    pub fn new() -> Self {
        Self::with_paths(
            DEFAULT_OS_RELEASE_PATH,
            DEFAULT_REBOOT_REQUIRED_PATH,
            DEFAULT_REBOOT_REQUIRED_PKGS_PATH,
        )
    }

    /// Read from arbitrary paths (used by tests and fixtures).
    pub fn with_paths(
        os_release: impl Into<PathBuf>,
        marker: impl Into<PathBuf>,
        packages: impl Into<PathBuf>,
    ) -> Self {
        Self {
            os_release: os_release.into(),
            marker: marker.into(),
            packages: packages.into(),
        }
    }

    /// The distribution identity file this adapter reads.
    pub fn os_release_path(&self) -> &Path {
        &self.os_release
    }
}

impl Default for RebootProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl RebootSource for RebootProbe {
    fn reboot_status(&self) -> Result<RebootStatus, RebootError> {
        let Some(os_release) = read_optional(&self.os_release)? else {
            return Ok(RebootStatus::unsupported());
        };
        if !parse_os_release(&os_release).is_debian_like() {
            return Ok(RebootStatus::unsupported());
        }
        if read_optional(&self.marker)?.is_none() {
            return Ok(RebootStatus::not_required());
        }
        let packages = match read_optional(&self.packages)? {
            Some(text) => parse_package_list(&text),
            None => Vec::new(),
        };
        Ok(RebootStatus::required(packages))
    }
}

/// Read a file, returning `None` when it does not exist and an error otherwise.
fn read_optional(path: &Path) -> Result<Option<String>, RebootError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RebootError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        }),
    }
}
