use thiserror::Error;

/// Errors raised while validating domain values.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    /// The supplied uptime was not a finite, non-negative number of seconds.
    #[error("uptime must be a finite non-negative number of seconds, got {0}")]
    InvalidUptime(f64),
}

/// Errors raised by an outbound (driven) adapter while probing the host.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SystemError {
    /// The underlying source could not be read.
    #[error("failed to read uptime source: {0}")]
    Io(String),
    /// The source produced no usable token.
    #[error("uptime source is empty")]
    Empty,
    /// The first token of the source was not a valid number.
    #[error("malformed uptime value: {0}")]
    Malformed(String),
    /// The parsed number was not a valid uptime.
    #[error("invalid uptime value: {0}")]
    Invalid(String),
}

/// Errors surfaced by the application layer (the driving port).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A system probe failed.
    #[error(transparent)]
    System(#[from] SystemError),
}
