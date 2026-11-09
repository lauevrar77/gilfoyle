//! Client errors.

use thiserror::Error;

/// Errors raised by the client.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClientError {
    /// The configuration file could not be read.
    #[error("failed to read client config {path}: {cause}")]
    Config {
        /// The path that was read.
        path: String,
        /// The underlying error.
        cause: String,
    },
    /// The configuration was invalid.
    #[error("client config: {0}")]
    Invalid(String),
    /// A private key could not be read.
    #[error("failed to read key {path}: {cause}")]
    Key {
        /// The path that was read.
        path: String,
        /// The underlying error.
        cause: String,
    },
    /// A node could not be reached or did not behave as expected.
    #[error("node `{label}`: {cause}")]
    Node {
        /// The node label.
        label: String,
        /// The underlying error.
        cause: String,
    },
}
