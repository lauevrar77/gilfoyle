//! Gilfoyle Docker adapter.
//!
//! This crate is the only place that talks to Docker, and it does so over the
//! daemon's UNIX socket with an in-process HTTP/1.1 client: no process is ever
//! spawned. It implements the [`gilfoyle_core`] driven ports
//! [`gilfoyle_core::ContainerCatalog`] and [`gilfoyle_core::ContainerControl`].
//!
//! The request set is closed — list, inspect, start, stop, restart — so there
//! is no path to create or remove a container; only the domain core's closed
//! action vocabulary decides what may be sent.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod adapter;
pub mod api;
mod lazy;
pub mod transport;

pub use adapter::{DEFAULT_CALL_TIMEOUT, DEFAULT_STOP_TIMEOUT, DockerAdapter};
pub use lazy::{ConnectFuture, Connector, LazyDocker};
pub use transport::DockerTransport;
