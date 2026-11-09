//! Authorized client keys.

use std::collections::BTreeSet;

use russh::keys::PublicKey;
use thiserror::Error;

/// Errors raised while building or parsing authorized keys.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    /// An authorized key had an empty id.
    #[error("authorized key id must not be empty")]
    EmptyId,
    /// Two authorized keys shared an id.
    #[error("duplicate authorized key id: {0}")]
    DuplicateId(String),
    /// A public key could not be parsed.
    #[error("invalid public key for `{0}`: {1}")]
    InvalidKey(String, String),
}

/// A client public key a node accepts, optionally limited to a set of tools.
#[derive(Debug, Clone)]
pub struct AuthorizedKey {
    /// Stable identifier, used as the principal and in logs.
    pub id: String,
    /// The accepted public key.
    pub key: PublicKey,
    /// The tools this key may call; `None` inherits the global set.
    pub tools: Option<BTreeSet<String>>,
}

/// The set of keys a node accepts.
#[derive(Debug, Clone, Default)]
pub struct AuthorizedKeys {
    entries: Vec<AuthorizedKey>,
}

impl AuthorizedKeys {
    /// Build a set, rejecting empty or duplicate ids.
    pub fn new(entries: Vec<AuthorizedKey>) -> Result<Self, KeyError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for entry in &entries {
            if entry.id.trim().is_empty() {
                return Err(KeyError::EmptyId);
            }
            if !seen.insert(entry.id.as_str()) {
                return Err(KeyError::DuplicateId(entry.id.clone()));
            }
        }
        Ok(Self { entries })
    }

    /// Whether no keys are configured.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of configured keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over the configured keys.
    pub fn iter(&self) -> impl Iterator<Item = &AuthorizedKey> {
        self.entries.iter()
    }

    /// Find the entry that authorizes `key`, comparing key material only.
    ///
    /// The comment is deliberately excluded: two identical keys with different comments
    /// must still match.
    pub fn authorize(&self, key: &PublicKey) -> Option<&AuthorizedKey> {
        self.entries
            .iter()
            .find(|entry| entry.key.key_data() == key.key_data())
    }

    /// Parse an OpenSSH public key line into an entry.
    pub fn parse_key(id: impl Into<String>, line: &str) -> Result<AuthorizedKey, KeyError> {
        let id = id.into();
        let key = PublicKey::from_openssh(line.trim())
            .map_err(|error| KeyError::InvalidKey(id.clone(), error.to_string()))?;
        Ok(AuthorizedKey {
            id,
            key,
            tools: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};

    fn generated(id: &str) -> AuthorizedKey {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone();
        AuthorizedKey {
            id: id.to_string(),
            key,
            tools: None,
        }
    }

    #[test]
    fn authorizes_a_known_key() {
        let entry = generated("nestor");
        let key = entry.key.clone();
        let keys = AuthorizedKeys::new(vec![entry]).unwrap();
        assert_eq!(
            keys.authorize(&key).map(|entry| entry.id.as_str()),
            Some("nestor")
        );
    }

    #[test]
    fn rejects_an_unknown_key() {
        let keys = AuthorizedKeys::new(vec![generated("nestor")]).unwrap();
        let stranger = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone();
        assert!(keys.authorize(&stranger).is_none());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let result = AuthorizedKeys::new(vec![generated("dup"), generated("dup")]);
        assert_eq!(
            result.unwrap_err(),
            KeyError::DuplicateId("dup".to_string())
        );
    }

    #[test]
    fn rejects_empty_ids() {
        let result = AuthorizedKeys::new(vec![generated("  ")]);
        assert_eq!(result.unwrap_err(), KeyError::EmptyId);
    }

    #[test]
    fn reports_emptiness_and_length() {
        assert!(AuthorizedKeys::default().is_empty());
        let keys = AuthorizedKeys::new(vec![generated("a"), generated("b")]).unwrap();
        assert!(!keys.is_empty());
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn parses_an_openssh_line_and_round_trips() {
        let original = generated("nestor");
        let line = original.key.to_openssh().unwrap();
        let parsed = AuthorizedKeys::parse_key("nestor", &line).unwrap();
        assert_eq!(parsed.key.key_data(), original.key.key_data());
        assert!(parsed.tools.is_none());
    }

    #[test]
    fn rejects_a_malformed_key_line() {
        let result = AuthorizedKeys::parse_key("nestor", "not a key");
        assert!(matches!(result, Err(KeyError::InvalidKey(_, _))));
    }
}
