//! Host metrics: load average and memory.
//!
//! Both are read from fixed `/proc` files and are bounded, named and
//! non-disclosing, so they are *open* reads (see `docs/adr/0012`).

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::DomainError;

/// The host load average and process counts, from `/proc/loadavg`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct LoadAverage {
    /// Number of jobs in the run queue, averaged over the last minute.
    pub one: f64,
    /// Same, averaged over the last five minutes.
    pub five: f64,
    /// Same, averaged over the last fifteen minutes.
    pub fifteen: f64,
    /// Number of currently runnable (running or waiting) processes.
    pub runnable: u64,
    /// Total number of processes.
    pub total: u64,
}

impl LoadAverage {
    /// Build a load average, rejecting non-finite or negative averages.
    ///
    /// The runnable and total counts are taken as-is; the kernel guarantees
    /// `runnable <= total`.
    pub fn new(
        one: f64,
        five: f64,
        fifteen: f64,
        runnable: u64,
        total: u64,
    ) -> Result<Self, DomainError> {
        for value in [one, five, fifteen] {
            if !value.is_finite() || value < 0.0 {
                return Err(DomainError::InvalidLoadAverage(value));
            }
        }
        Ok(Self {
            one,
            five,
            fifteen,
            runnable,
            total,
        })
    }
}

/// Host memory usage, in kibibytes, from `/proc/meminfo`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Memory {
    /// Total usable RAM.
    pub total_kib: u64,
    /// Memory that is completely free.
    pub free_kib: u64,
    /// Memory available to a new application without swapping.
    pub available_kib: u64,
    /// Memory in use, i.e. `total_kib - available_kib`.
    pub used_kib: u64,
    /// Total swap.
    pub swap_total_kib: u64,
    /// Free swap.
    pub swap_free_kib: u64,
    /// Percentage of memory in use, computed from `total_kib` and `used_kib`.
    pub used_percent: f64,
}

impl Memory {
    /// Build a memory report, deriving `used_kib` and `used_percent`.
    ///
    /// `used_kib` is `total_kib - available_kib`, saturating at zero so a
    /// nonsensical `available > total` cannot underflow. `used_percent` is zero
    /// when the total is zero.
    pub fn new(
        total_kib: u64,
        free_kib: u64,
        available_kib: u64,
        swap_total_kib: u64,
        swap_free_kib: u64,
    ) -> Self {
        let used_kib = total_kib.saturating_sub(available_kib);
        let used_percent = percent(used_kib, total_kib);
        Self {
            total_kib,
            free_kib,
            available_kib,
            used_kib,
            swap_total_kib,
            swap_free_kib,
            used_percent,
        }
    }
}

/// The structured result returned by the `get_load_and_memory` tool.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct LoadAndMemory {
    /// The host load average.
    pub load: LoadAverage,
    /// The host memory usage.
    pub memory: Memory,
}

/// `part / whole` as a percentage, or zero when `whole` is zero.
pub(crate) fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64 * 100.0
}
