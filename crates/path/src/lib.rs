//! Gilfoyle driven adapter that reads path metadata.
//!
//! This crate never reads file contents: it reports metadata (existence, size,
//! times, mode, owner, kind) for the paths the operator has declared. The last
//! path component is never followed, so a symbolic link cannot be used to reach
//! an unlisted target.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod path_probe;

pub use path_probe::{PathProbe, present};
