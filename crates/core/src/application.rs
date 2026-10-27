use crate::{AppError, SystemAdmin, UptimeReport, UptimeSource};

/// Application service implementing the driving port on top of a driven port.
///
/// Generic over the [`UptimeSource`] so it is zero-cost in production and
/// trivially testable with a fake source.
pub struct SystemService<S: UptimeSource> {
    source: S,
}

impl<S: UptimeSource> SystemService<S> {
    /// Build a service backed by `source`.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: UptimeSource> SystemAdmin for SystemService<S> {
    fn uptime(&self) -> Result<UptimeReport, AppError> {
        let uptime = self.source.uptime()?;
        Ok(UptimeReport {
            uptime_seconds: uptime.as_seconds(),
            uptime_human: uptime.human(),
        })
    }
}
