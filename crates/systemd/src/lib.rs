//! Gilfoyle systemd adapter.
//!
//! This crate is the only place that talks to systemd, and it does so over the system bus
//! in-process: no process is ever spawned. It implements the [`gilfoyle_core`] driven ports
//! [`gilfoyle_core::ServiceCatalog`] and [`gilfoyle_core::ServiceControl`].
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod adapter;
mod error;
mod lazy;
mod mapping;

pub use adapter::{DEFAULT_CALL_TIMEOUT, DEFAULT_JOB_TIMEOUT, SystemdAdapter};
pub use error::{map_dbus_error_name, map_zbus_error};
pub use lazy::{ConnectFuture, Connector, LazySystemd};
pub use mapping::{RawUnit, apply_filter, raw_to_unit};
