use schemars::JsonSchema;
use serde::Serialize;

use crate::error::DomainError;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

/// The uptime of the host machine, expressed as a number of seconds.
///
/// Invariant: the value is always finite and non-negative. The only way to
/// build one is [`Uptime::from_seconds`], which enforces that invariant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uptime {
    seconds: f64,
}

impl Uptime {
    /// Build an [`Uptime`] from a number of seconds.
    ///
    /// Rejects `NaN`, infinities and negative values.
    pub fn from_seconds(seconds: f64) -> Result<Self, DomainError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(DomainError::InvalidUptime(seconds));
        }
        Ok(Self { seconds })
    }

    /// The uptime in seconds.
    pub fn as_seconds(self) -> f64 {
        self.seconds
    }

    /// A human-readable rendering such as `"12d 3h 25m 45s"`.
    ///
    /// Larger units are only shown when they are non-zero, or when a larger
    /// unit is present (so `3600` renders as `"1h 0m 0s"`). Seconds are always
    /// shown. Sub-second precision is truncated.
    pub fn human(self) -> String {
        let total = self.seconds.floor() as u64;
        let days = total / SECONDS_PER_DAY;
        let hours = (total % SECONDS_PER_DAY) / SECONDS_PER_HOUR;
        let minutes = (total % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
        let seconds = total % SECONDS_PER_MINUTE;

        let mut parts: Vec<String> = Vec::new();
        if days > 0 {
            parts.push(format!("{days}d"));
        }
        if hours > 0 || days > 0 {
            parts.push(format!("{hours}h"));
        }
        if minutes > 0 || hours > 0 || days > 0 {
            parts.push(format!("{minutes}m"));
        }
        parts.push(format!("{seconds}s"));
        parts.join(" ")
    }
}

/// The structured result returned by the `get_uptime` tool.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct UptimeReport {
    /// Uptime in seconds, as reported by the kernel.
    pub uptime_seconds: f64,
    /// Human-readable rendering of the same uptime.
    pub uptime_human: String,
}
