//! Gilfoyle driving adapters and server library.
//!
//! This crate hosts everything that faces the outside world: the MCP tool
//! handler, the bearer-token middleware, the HTTP router and configuration
//! loading. It is the only crate that touches the network.
#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod hash;
pub mod http;
pub mod mcp;
