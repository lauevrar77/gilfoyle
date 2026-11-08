//! Host-key generation, loading and fingerprints.

use std::path::Path;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use thiserror::Error;

/// Errors from host-key operations.
#[derive(Debug, Error)]
pub enum HostKeyError {
    /// The key could not be generated.
    #[error("failed to generate a host key: {0}")]
    Generate(String),
    /// The key could not be read.
    #[error("failed to read host key {path}: {cause}")]
    Load {
        /// The path that was read.
        path: String,
        /// The underlying error.
        cause: String,
    },
    /// The key could not be written.
    #[error("failed to write host key {path}: {cause}")]
    Write {
        /// The path that was written.
        path: String,
        /// The underlying error.
        cause: String,
    },
    /// The destination already exists.
    #[error("host key {0} already exists; refusing to overwrite")]
    Exists(String),
}

/// Generate a fresh ed25519 host key.
pub fn generate() -> Result<PrivateKey, HostKeyError> {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .map_err(|error| HostKeyError::Generate(error.to_string()))
}

/// The `SHA256:...` fingerprint of a public key.
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// Load a host key from an OpenSSH private-key file.
pub fn load(path: &Path) -> Result<PrivateKey, HostKeyError> {
    PrivateKey::read_openssh_file(path).map_err(|error| HostKeyError::Load {
        path: path.display().to_string(),
        cause: error.to_string(),
    })
}

/// Write a host key to a file, refusing to overwrite an existing one.
pub fn write(key: &PrivateKey, path: &Path) -> Result<(), HostKeyError> {
    if path.exists() {
        return Err(HostKeyError::Exists(path.display().to_string()));
    }
    key.write_openssh_file(path, LineEnding::LF)
        .map_err(|error| HostKeyError::Write {
            path: path.display().to_string(),
            cause: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_sha256() {
        let key = generate().unwrap();
        let fingerprint = fingerprint(key.public_key());
        assert!(fingerprint.starts_with("SHA256:"), "got {fingerprint}");
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("gilfoyle-hostkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("host_key");
        let _ = std::fs::remove_file(&path);
        let key = generate().unwrap();
        write(&key, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.public_key().key_data(), key.public_key().key_data());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_overwrite() {
        let dir =
            std::env::temp_dir().join(format!("gilfoyle-hostkey-exists-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("host_key");
        std::fs::write(&path, b"x").unwrap();
        let key = generate().unwrap();
        assert!(matches!(write(&key, &path), Err(HostKeyError::Exists(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_reports_a_missing_file() {
        let path = std::env::temp_dir().join("gilfoyle-does-not-exist");
        assert!(matches!(load(&path), Err(HostKeyError::Load { .. })));
    }
}
