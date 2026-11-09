//! Gilfoyle SSH transport.
//!
//! This crate is the only place that speaks SSH. It serves the MCP protocol over an
//! embedded `russh` server: a client authenticates by public key, then requests the
//! `gilfoyle-mcp` subsystem, and the resulting channel carries MCP JSON-RPC.
//!
//! SSH is used purely as a protocol. The handler refuses `shell`, `exec`, `pty` and
//! every forwarding request, so a connection can never obtain a shell.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod adapter;
pub mod hostkey;
pub mod keys;
pub mod policy;

pub use adapter::{Limits, ServiceFactory, SshServer};
pub use keys::{AuthorizedKey, AuthorizedKeys, KeyError};
pub use policy::{Principal, SUBSYSTEM};

/// Re-exported so downstream crates and tests can name SSH types without a direct dependency.
pub use russh;
