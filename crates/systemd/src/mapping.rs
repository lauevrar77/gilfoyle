//! Pure mapping from raw D-Bus values to domain types.

use gilfoyle_core::{Unit, UnitFilter};

/// One entry of the `ListUnits` reply: `(name, description, load, active, sub, following,
/// path, job_id, job_type, job_path)`.
pub type RawUnit = (
    String,
    String,
    String,
    String,
    String,
    String,
    zbus::OwnedObjectPath,
    u32,
    String,
    zbus::OwnedObjectPath,
);

/// Map a raw `ListUnits` entry to a [`Unit`], dropping the fields gilfoyle does not expose.
pub fn raw_to_unit(raw: &RawUnit) -> Unit {
    Unit {
        name: raw.0.clone(),
        description: raw.1.clone(),
        load_state: raw.2.clone(),
        active_state: raw.3.clone(),
        sub_state: raw.4.clone(),
    }
}

/// Apply the client-side parts of a filter: unit type, case-insensitive name substring, limit.
///
/// The active-state filter is applied by systemd (via `ListUnitsFiltered`), so it is not
/// re-applied here.
pub fn apply_filter(units: Vec<Unit>, filter: &UnitFilter) -> Vec<Unit> {
    let needle = filter.name_contains.as_ref().map(|n| n.to_lowercase());
    let mut filtered: Vec<Unit> = units
        .into_iter()
        .filter(|unit| match filter.unit_type {
            Some(unit_type) => unit.name.ends_with(&format!(".{}", unit_type.suffix())),
            None => true,
        })
        .filter(|unit| match &needle {
            Some(needle) => unit.name.to_lowercase().contains(needle),
            None => true,
        })
        .collect();
    if let Some(limit) = filter.limit {
        filtered.truncate(limit);
    }
    filtered
}
