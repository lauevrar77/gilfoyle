//! Gilfoyle domain core.
//!
//! This crate is deliberately free of I/O, async runtimes and transport
//! concerns. It defines the domain types, the ports (traits) that adapters
//! implement, and the use cases that orchestrate them. Because the core cannot
//! perform I/O, it structurally cannot execute anything on the host.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod application;
mod domain;
mod error;
mod ports;

pub use application::SystemService;
pub use domain::{Uptime, UptimeReport};
pub use error::{AppError, DomainError, SystemError};
pub use ports::{SystemAdmin, UptimeSource};
