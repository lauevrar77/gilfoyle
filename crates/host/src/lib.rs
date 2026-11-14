//! Gilfoyle driven adapters that probe host metrics.
//!
//! This crate reads the load average, memory usage and disk usage of the host.
//! The `/proc` files are the only host paths it touches, and it never spawns a
//! command; the disk usage is read with the read-only `statvfs` syscall.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod disk;
mod proc_load;
mod proc_meminfo;
mod proc_mounts;

pub use disk::{DEFAULT_PROC_MOUNTS_PATH, DiskProbe, Filesystem, FsStat, RealFilesystem};
pub use proc_load::{DEFAULT_PROC_LOADAVG_PATH, ProcLoad, parse_loadavg};
pub use proc_meminfo::{DEFAULT_PROC_MEMINFO_PATH, ProcMeminfo, parse_meminfo};
pub use proc_mounts::{MountEntry, is_network_fs, parse_mounts};
