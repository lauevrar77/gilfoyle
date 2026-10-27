use crate::{AppError, SystemError, Uptime, UptimeReport};

/// Outbound (driven) port: a source of host uptime.
///
/// Implemented by adapters in `gilfoyle-system`.
pub trait UptimeSource: Send + Sync {
    /// Read the current host uptime.
    fn uptime(&self) -> Result<Uptime, SystemError>;
}

/// Inbound (driving) port: the operations gilfoyle exposes to the outside
/// world. Implemented by the application layer, called by transport adapters.
pub trait SystemAdmin: Send + Sync {
    /// Read the current host uptime, ready to be rendered to a client.
    fn uptime(&self) -> Result<UptimeReport, AppError>;
}
