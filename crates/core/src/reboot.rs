//! Whether the host needs a reboot.
//!
//! The verdict is host- and distribution-specific. On Debian-family hosts it
//! comes from the `/run/reboot-required` marker; on a host whose mechanism
//! gilfoyle does not understand, the answer is `None` (unknown), never `false`
//! — a fabricated "no reboot needed" would be worse than an honest "cannot
//! tell" (see `docs/adr/0014`). This read is bounded, named and non-disclosing,
//! so it is an *open* read (see `docs/adr/0012`).

use schemars::JsonSchema;
use serde::Serialize;

/// How the reboot verdict was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RebootOrigin {
    /// The Debian/Ubuntu `/run/reboot-required` marker.
    Debian,
    /// The host's mechanism is not one gilfoyle understands.
    Unsupported,
}

/// Whether the host needs a reboot, as far as gilfoyle can tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebootStatus {
    /// `Some(true)`/`Some(false)` when the host reports it, `None` when gilfoyle
    /// cannot tell. Unknown is never reported as `false`.
    pub required: Option<bool>,
    /// Packages that asked for the reboot, when the host lists them.
    pub packages: Vec<String>,
    /// How the verdict was reached.
    pub origin: RebootOrigin,
}

impl RebootStatus {
    /// A Debian-family host with a pending reboot, and the packages that asked
    /// for it.
    pub fn required(packages: Vec<String>) -> Self {
        Self {
            required: Some(true),
            packages,
            origin: RebootOrigin::Debian,
        }
    }

    /// A Debian-family host with no pending reboot.
    pub fn not_required() -> Self {
        Self {
            required: Some(false),
            packages: Vec::new(),
            origin: RebootOrigin::Debian,
        }
    }

    /// A host whose reboot mechanism gilfoyle does not understand.
    pub fn unsupported() -> Self {
        Self {
            required: None,
            packages: Vec::new(),
            origin: RebootOrigin::Unsupported,
        }
    }
}
