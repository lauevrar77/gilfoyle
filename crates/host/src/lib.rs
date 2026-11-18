//! Gilfoyle driven adapters that probe host metrics.
//!
//! This crate reads the load average, memory usage and disk usage of the host,
//! and whether a reboot is pending. It never spawns a command: the metrics come
//! from `/proc`, the disk usage from the read-only `statvfs` syscall, and the
//! reboot verdict from `/etc/os-release` and the `/run` markers.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod disk;
mod proc_load;
mod proc_meminfo;
mod proc_mounts;
mod reboot;

pub use disk::{DEFAULT_PROC_MOUNTS_PATH, DiskProbe, Filesystem, FsStat, RealFilesystem};
pub use proc_load::{DEFAULT_PROC_LOADAVG_PATH, ProcLoad, parse_loadavg};
pub use proc_meminfo::{DEFAULT_PROC_MEMINFO_PATH, ProcMeminfo, parse_meminfo};
pub use proc_mounts::{MountEntry, is_network_fs, parse_mounts};
pub use reboot::{
    DEFAULT_OS_RELEASE_PATH, DEFAULT_REBOOT_REQUIRED_PATH, DEFAULT_REBOOT_REQUIRED_PKGS_PATH,
    OsRelease, RebootProbe, parse_os_release, parse_package_list,
};
