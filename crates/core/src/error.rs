use thiserror::Error;

use crate::packagekit::PackageError;

/// Errors raised while validating domain values.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    /// The supplied uptime was not a finite, non-negative number of seconds.
    #[error("uptime must be a finite non-negative number of seconds, got {0}")]
    InvalidUptime(f64),
    /// A unit name was empty or contained disallowed characters.
    #[error("invalid unit name: {0}")]
    InvalidUnitName(String),
    /// A unit name ended with a suffix that is not a known unit type.
    #[error("unknown unit type suffix in `{0}`")]
    UnknownUnitSuffix(String),
    /// A unit type filter value was not a known unit type.
    #[error("unknown unit type: {0}")]
    UnknownUnitType(String),
    /// A package id was not of the form `name;version;arch;data` (APT) or
    /// `name;version;arch;origin;data` (DNF).
    #[error("invalid package id: {0}")]
    InvalidPackageId(String),
    /// An update severity filter value was not `low`, `medium`, `high` or `critical`.
    #[error("unknown severity: {0}")]
    UnknownSeverity(String),
    /// A package operation was not `refresh` or `upgrade`.
    #[error("unknown package operation: {0}")]
    UnknownPackageOperation(String),
    /// A container reference was empty or contained disallowed characters.
    #[error("invalid container reference: {0}")]
    InvalidContainerRef(String),
    /// A container allowlist entry was not a valid `re:` regular expression.
    #[error("invalid container allowlist pattern: {0}")]
    InvalidContainerPattern(String),
    /// A container state filter value was not a known lifecycle state.
    #[error("unknown container state: {0}")]
    UnknownContainerState(String),
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

/// Errors raised by the systemd adapter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SystemdError {
    /// The system bus could not be reached or the call could not be made.
    #[error("systemd is unavailable: {0}")]
    Unavailable(String),
    /// The unit does not exist.
    #[error("unit `{0}` not found")]
    NotFound(String),
    /// The operation was refused for lack of authorisation (polkit).
    #[error("not authorised for unit `{0}`: {1}")]
    Forbidden(String, String),
    /// The unit is masked.
    #[error("unit `{0}` is masked")]
    Masked(String),
    /// The unit refuses manual start or stop.
    #[error("unit `{0}` cannot be started or stopped manually")]
    OnlyByDependency(String),
    /// The unit failed to load.
    #[error("unit `{0}` failed to load: {1}")]
    LoadFailed(String, String),
    /// The start/stop job failed.
    #[error("job for unit `{0}` did not succeed: {1}")]
    JobFailed(String, String),
    /// systemd rejected the request as invalid.
    #[error("invalid systemd request: {0}")]
    Invalid(String),
    /// The call or the job wait timed out.
    #[error("timed out talking to systemd: {0}")]
    Timeout(String),
    /// Any other systemd error, kept verbatim for diagnosis.
    #[error("systemd error `{name}`: {message}")]
    Other {
        /// The raw D-Bus error name.
        name: String,
        /// The raw D-Bus error message.
        message: String,
    },
}

/// Errors raised by the Docker adapter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DockerError {
    /// The daemon socket could not be reached or the call could not be made.
    #[error("Docker is unavailable: {0}")]
    Unavailable(String),
    /// The container does not exist.
    #[error("container `{0}` not found")]
    NotFound(String),
    /// The operation was refused for lack of authorisation (socket permissions).
    #[error("not authorised for container `{0}`: {1}")]
    Forbidden(String, String),
    /// Docker rejected the request as conflicting with the container's state.
    #[error("container `{0}` is in a conflicting state: {1}")]
    Conflict(String, String),
    /// Docker rejected the request as invalid.
    #[error("invalid Docker request: {0}")]
    Invalid(String),
    /// The call timed out.
    #[error("timed out talking to Docker: {0}")]
    Timeout(String),
    /// Any other Docker error, kept verbatim for diagnosis.
    #[error("Docker error (HTTP {status}): {message}")]
    Other {
        /// The HTTP status code.
        status: u16,
        /// Docker's error message.
        message: String,
    },
}

/// Errors surfaced by the application layer (the driving ports).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum AppError {
    /// A host probe failed.
    #[error(transparent)]
    System(#[from] SystemError),
    /// A domain value was invalid.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A unit is not in the allowlist, so it may not be mutated.
    #[error("unit `{0}` is not a managed unit")]
    NotManaged(String),
    /// A systemd operation failed.
    #[error(transparent)]
    Systemd(#[from] SystemdError),
    /// A mutating package operation is not in the allowlist.
    #[error("package operation `{0}` is not allowed")]
    PackageOperationNotAllowed(String),
    /// A PackageKit operation failed.
    #[error(transparent)]
    Package(#[from] PackageError),
    /// A container is not in the allowlist, so it may not be mutated.
    #[error("container `{0}` is not a managed container")]
    ContainerNotManaged(String),
    /// A Docker operation failed.
    #[error(transparent)]
    Docker(#[from] DockerError),
}
