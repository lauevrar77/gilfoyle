//! Gilfoyle driven adapters.
//!
//! This is the only place in the workspace that touches the host. Every
//! operation is a read of a fixed path; nothing here can execute a command.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod proc_uptime;

pub use proc_uptime::{DEFAULT_PROC_UPTIME_PATH, ProcUptime, parse_uptime};
