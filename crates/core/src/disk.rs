//! Disk usage domain types and the configured mount points.
//!
//! Gilfoyle reports usage only for the mount points the operator declares in
//! `[host] disks` (see `docs/adr/0011`). There is deliberately no request that
//! creates, mounts or unmounts anything: the vocabulary is read-only.

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::DomainError;
use crate::host::percent;

/// A configured mount point, an absolute path such as `/` or `/var`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountPoint {
    raw: String,
}

impl MountPoint {
    /// Normalise and validate a configured mount point.
    ///
    /// It must be a non-empty absolute path. The path is not required to be
    /// mounted here; that is checked when the tool runs.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        if trimmed.is_empty() || !trimmed.starts_with('/') {
            return Err(DomainError::InvalidMountPoint(input.to_string()));
        }
        Ok(Self {
            raw: trimmed.to_string(),
        })
    }

    /// The mount point as configured.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// The mount points to report, in configuration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountPoints {
    points: Vec<MountPoint>,
}

impl MountPoints {
    /// Build from validated mount points.
    pub fn new(points: impl IntoIterator<Item = MountPoint>) -> Self {
        Self {
            points: points.into_iter().collect(),
        }
    }

    /// The default: the root filesystem only.
    pub fn root() -> Self {
        Self {
            points: vec![MountPoint {
                raw: "/".to_string(),
            }],
        }
    }

    /// Whether no mount point is configured.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The configured mount points, in order.
    pub fn iter(&self) -> impl Iterator<Item = &MountPoint> {
        self.points.iter()
    }
}

/// Usage of one filesystem, as reported by `statvfs`.
///
/// Every field is present even when the entry could not be measured: in that
/// case `error` explains why and the sizes are zero, so one bad mount point
/// never hides the others.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct FilesystemUsage {
    /// The configured mount point.
    pub mount: String,
    /// The backing device, when known.
    pub device: String,
    /// The filesystem type, when known.
    pub fstype: String,
    /// Whether the path is currently a mounted filesystem.
    pub mounted: bool,
    /// Total size, in bytes.
    pub total_bytes: u64,
    /// Bytes in use (`total_bytes - free_bytes`).
    pub used_bytes: u64,
    /// Bytes available to unprivileged processes.
    pub free_bytes: u64,
    /// Percentage of bytes in use.
    pub used_percent: f64,
    /// Total number of inodes.
    pub inodes_total: u64,
    /// Number of free inodes.
    pub inodes_free: u64,
    /// Percentage of inodes in use.
    pub inodes_used_percent: f64,
    /// Why the entry could not be measured, if it could not.
    pub error: Option<String>,
}

impl FilesystemUsage {
    /// A measured filesystem.
    ///
    /// `free_bytes` is the space available to unprivileged processes
    /// (`f_bavail`); `used_bytes` is derived from it.
    pub fn measured(
        mount: &str,
        device: &str,
        fstype: &str,
        total_bytes: u64,
        free_bytes: u64,
        inodes_total: u64,
        inodes_free: u64,
    ) -> Self {
        let used_bytes = total_bytes.saturating_sub(free_bytes);
        let inodes_used = inodes_total.saturating_sub(inodes_free);
        Self {
            mount: mount.to_string(),
            device: device.to_string(),
            fstype: fstype.to_string(),
            mounted: true,
            total_bytes,
            used_bytes,
            free_bytes,
            used_percent: percent(used_bytes, total_bytes),
            inodes_total,
            inodes_free,
            inodes_used_percent: percent(inodes_used, inodes_total),
            error: None,
        }
    }

    /// A configured path that is not currently mounted.
    pub fn not_mounted(mount: &str) -> Self {
        Self::unmeasured(mount, "", "", false, "not mounted")
    }

    /// A mounted filesystem whose type gilfoyle does not probe.
    pub fn unsupported(mount: &str, fstype: &str) -> Self {
        Self::unmeasured(
            mount,
            "",
            fstype,
            true,
            &format!("unsupported filesystem type `{fstype}`"),
        )
    }

    /// A mounted filesystem whose usage could not be read.
    pub fn failed(mount: &str, device: &str, fstype: &str, error: &str) -> Self {
        Self::unmeasured(mount, device, fstype, true, error)
    }

    fn unmeasured(mount: &str, device: &str, fstype: &str, mounted: bool, error: &str) -> Self {
        Self {
            mount: mount.to_string(),
            device: device.to_string(),
            fstype: fstype.to_string(),
            mounted,
            total_bytes: 0,
            used_bytes: 0,
            free_bytes: 0,
            used_percent: 0.0,
            inodes_total: 0,
            inodes_free: 0,
            inodes_used_percent: 0.0,
            error: Some(error.to_string()),
        }
    }
}

/// The structured result returned by the `get_disk_usage` tool.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct DiskUsage {
    /// Per-mount usage, in configuration order.
    pub filesystems: Vec<FilesystemUsage>,
}
