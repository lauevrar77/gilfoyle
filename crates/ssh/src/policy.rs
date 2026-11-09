//! Pure transport policy.
//!
//! Everything here is free of I/O and of `russh` types, so it is trivially testable and
//! fully covered by mutation testing. The `russh` handler in this crate is the thin glue
//! that calls into these functions.

use std::collections::BTreeSet;

/// The subsystem name that carries MCP over an SSH channel.
pub const SUBSYSTEM: &str = "gilfoyle-mcp";

/// The authenticated identity of a connection: the id of the key that signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// The id of the authorized key that authenticated the connection.
    pub key_id: String,
}

impl Principal {
    /// Build a principal for the given key id.
    pub fn new(key_id: impl Into<String>) -> Self {
        Self {
            key_id: key_id.into(),
        }
    }
}

/// Whether a public-key algorithm is accepted for client authentication.
///
/// Ed25519, ECDSA and RSA-SHA2 are accepted; SHA-1 `ssh-rsa` and legacy DSA are not.
pub fn algorithm_allowed(name: &str) -> bool {
    matches!(
        name,
        "ssh-ed25519"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "rsa-sha2-256"
            | "rsa-sha2-512"
    )
}

/// Whether a subsystem request names the MCP subsystem.
///
/// Any other subsystem is refused, which keeps the channel surface at exactly one.
pub fn subsystem_allowed(name: &str) -> bool {
    name == SUBSYSTEM
}

/// The tools a key may call.
///
/// A grant restricts the global set (their intersection); an absent grant inherits the
/// whole global set.
pub fn effective_tools(
    grant: Option<&BTreeSet<String>>,
    global: &BTreeSet<String>,
) -> BTreeSet<String> {
    match grant {
        Some(grant) => grant.intersection(global).cloned().collect(),
        None => global.clone(),
    }
}

/// The grant entries that are not known tools, sorted. A non-empty result fails configuration.
pub fn unknown_tools<'a>(grant: &'a BTreeSet<String>, known: &'a BTreeSet<String>) -> Vec<&'a str> {
    grant.difference(known).map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn accepts_modern_algorithms() {
        assert!(algorithm_allowed("ssh-ed25519"));
        assert!(algorithm_allowed("ecdsa-sha2-nistp256"));
        assert!(algorithm_allowed("rsa-sha2-256"));
        assert!(algorithm_allowed("rsa-sha2-512"));
    }

    #[test]
    fn rejects_legacy_and_unknown_algorithms() {
        assert!(!algorithm_allowed("ssh-rsa"));
        assert!(!algorithm_allowed("ssh-dss"));
        assert!(!algorithm_allowed(""));
    }

    #[test]
    fn accepts_only_the_mcp_subsystem() {
        assert!(subsystem_allowed(SUBSYSTEM));
        assert!(!subsystem_allowed("sftp"));
        assert!(!subsystem_allowed(""));
    }

    #[test]
    fn absent_grant_inherits_the_global_set() {
        let global = set(&["get_uptime", "start_unit"]);
        assert_eq!(effective_tools(None, &global), global);
    }

    #[test]
    fn grant_intersects_the_global_set() {
        let global = set(&["get_uptime", "start_unit"]);
        let grant = set(&["get_uptime", "not_enabled"]);
        assert_eq!(effective_tools(Some(&grant), &global), set(&["get_uptime"]));
    }

    #[test]
    fn empty_grant_yields_no_tools() {
        let global = set(&["get_uptime"]);
        assert!(effective_tools(Some(&BTreeSet::new()), &global).is_empty());
    }

    #[test]
    fn flags_unknown_grant_entries() {
        let known = set(&["get_uptime", "start_unit"]);
        let grant = set(&["get_uptime", "typo"]);
        assert_eq!(unknown_tools(&grant, &known), vec!["typo"]);
    }

    #[test]
    fn accepts_a_fully_known_grant() {
        let known = set(&["get_uptime", "start_unit"]);
        assert!(unknown_tools(&known, &known).is_empty());
    }
}
