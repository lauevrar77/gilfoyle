use std::path::{Path, PathBuf};
use std::sync::Arc;

use gilfoyle_core::{DiskError, DiskSource, FilesystemUsage, MountPoint, MountPoints};
use rustix::fs::statvfs;

use crate::proc_mounts::{MountEntry, is_network_fs, parse_mounts};

/// Default path of the kernel mount table.
pub const DEFAULT_PROC_MOUNTS_PATH: &str = "/proc/mounts";

/// Raw filesystem numbers, lifted out of `rustix` so the decisions stay pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsStat {
    /// Total size, in bytes.
    pub total_bytes: u64,
    /// Bytes available to unprivileged processes.
    pub free_bytes: u64,
    /// Total number of inodes.
    pub inodes_total: u64,
    /// Number of free inodes.
    pub inodes_free: u64,
}

/// A filesystem probe, injectable so the adapter can be tested without a disk.
pub trait Filesystem: Send + Sync {
    /// Read the raw `statvfs` numbers for a mounted path.
    fn statvfs(&self, path: &str) -> Result<FsStat, String>;
}

/// The real probe, over the read-only `statvfs` syscall.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFilesystem;

impl Filesystem for RealFilesystem {
    fn statvfs(&self, path: &str) -> Result<FsStat, String> {
        let stat = statvfs(path).map_err(|error| error.to_string())?;
        let block = stat.f_frsize;
        Ok(FsStat {
            total_bytes: stat.f_blocks.saturating_mul(block),
            free_bytes: stat.f_bavail.saturating_mul(block),
            inodes_total: stat.f_files,
            inodes_free: stat.f_ffree,
        })
    }
}

/// Adapter reporting disk usage for the configured mount points.
///
/// The mount table path and the filesystem probe are injectable so the adapter
/// can be tested against a fixture and a fake probe.
pub struct DiskProbe {
    mounts: PathBuf,
    points: MountPoints,
    filesystem: Arc<dyn Filesystem>,
}

impl DiskProbe {
    /// Report on the given mount points, using the real `/proc/mounts` and probe.
    pub fn new(points: MountPoints) -> Self {
        Self {
            mounts: PathBuf::from(DEFAULT_PROC_MOUNTS_PATH),
            points,
            filesystem: Arc::new(RealFilesystem),
        }
    }

    /// Build with an injected mount table, mount points and probe.
    pub fn with_parts(
        mounts: impl Into<PathBuf>,
        points: MountPoints,
        filesystem: Arc<dyn Filesystem>,
    ) -> Self {
        Self {
            mounts: mounts.into(),
            points,
            filesystem,
        }
    }

    /// The mount table this adapter reads.
    pub fn mounts_path(&self) -> &Path {
        &self.mounts
    }

    /// Report usage for one configured mount point.
    fn usage_for(&self, point: &MountPoint, entries: &[MountEntry]) -> FilesystemUsage {
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.mount_point == point.as_str())
        else {
            return FilesystemUsage::not_mounted(point.as_str());
        };
        if is_network_fs(&entry.fstype) {
            return FilesystemUsage::unsupported(point.as_str(), &entry.fstype);
        }
        match self.filesystem.statvfs(point.as_str()) {
            Ok(stat) => FilesystemUsage::measured(
                point.as_str(),
                &entry.device,
                &entry.fstype,
                stat.total_bytes,
                stat.free_bytes,
                stat.inodes_total,
                stat.inodes_free,
            ),
            Err(error) => {
                FilesystemUsage::failed(point.as_str(), &entry.device, &entry.fstype, &error)
            }
        }
    }
}

impl DiskSource for DiskProbe {
    fn disk_usage(&self) -> Result<Vec<FilesystemUsage>, DiskError> {
        let raw = std::fs::read_to_string(&self.mounts).map_err(|error| DiskError::Io {
            path: self.mounts.display().to_string(),
            message: error.to_string(),
        })?;
        let entries = parse_mounts(&raw)?;
        Ok(self
            .points
            .iter()
            .map(|point| self.usage_for(point, &entries))
            .collect())
    }
}
