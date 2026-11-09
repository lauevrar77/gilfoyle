//! Gilfoyle PackageKit adapter.
//!
//! This crate is the only place that talks to PackageKit, and it does so over the
//! system bus in-process: no process is ever spawned. It implements the
//! [`gilfoyle_core`] driven port [`gilfoyle_core::PackageManager`], and it never
//! installs or removes a package — the closed operation vocabulary lives in the
//! domain core.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod adapter;
mod error;
mod lazy;

pub use adapter::{DEFAULT_CALL_TIMEOUT, DEFAULT_LIMIT, DEFAULT_POLL_TIMEOUT, PackagekitAdapter};
pub use error::{map_dbus_error_name, map_zbus_error};
pub use lazy::{ConnectFuture, Connector, LazyPackagekit};
