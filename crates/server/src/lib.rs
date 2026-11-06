//! Gilfoyle driving adapters and server library.
//!
//! This crate hosts everything that faces the outside world: the MCP tool handler, the
//! SSH transport wiring, and configuration loading. It is the only crate that touches the
//! network.
#![forbid(unsafe_code)]

pub mod config;
pub mod mcp;
pub mod polkit;
pub mod serve;

pub use mcp::GilfoyleServer;
