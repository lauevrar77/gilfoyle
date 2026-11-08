//! Client configuration.

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::ClientError;

/// Default cap on sessions per node.
pub const DEFAULT_POOL_SIZE: usize = 4;

/// Raw TOML shape of the client configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClientConfig {
    /// Number of MCP sessions to keep open per node.
    #[serde(default)]
    pub pool_size: Option<usize>,
    /// The nodes to aggregate.
    #[serde(default)]
    pub nodes: Vec<RawNode>,
}

/// Raw `[[nodes]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawNode {
    /// The node label; must be unique and is the value of the `node` argument.
    pub label: String,
    /// The node host.
    pub host: String,
    /// The node port.
    pub port: u16,
    /// Path to the client's private key for this node.
    pub key: PathBuf,
    /// The expected `SHA256:` host-key fingerprint.
    pub host_key_fingerprint: String,
}

/// A fully resolved node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    /// The node label.
    pub label: String,
    /// The node host.
    pub host: String,
    /// The node port.
    pub port: u16,
    /// Path to the client's private key.
    pub key: PathBuf,
    /// The expected host-key fingerprint.
    pub host_key_fingerprint: String,
}

/// A fully resolved client configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Sessions per node.
    pub pool_size: usize,
    /// The nodes, in configuration order.
    pub nodes: Vec<NodeConfig>,
}

/// Parse raw TOML into a client configuration.
pub fn parse(contents: &str) -> Result<ClientConfig, ClientError> {
    let raw: RawClientConfig =
        toml::from_str(contents).map_err(|error| ClientError::Invalid(error.to_string()))?;
    resolve(raw)
}

/// Validate and normalise a raw configuration.
pub fn resolve(raw: RawClientConfig) -> Result<ClientConfig, ClientError> {
    if raw.nodes.is_empty() {
        return Err(ClientError::Invalid(
            "at least one node is required".to_string(),
        ));
    }
    let mut labels = std::collections::BTreeSet::new();
    let mut nodes = Vec::with_capacity(raw.nodes.len());
    for node in raw.nodes {
        if node.label.trim().is_empty() {
            return Err(ClientError::Invalid(
                "node label must not be empty".to_string(),
            ));
        }
        if !labels.insert(node.label.clone()) {
            return Err(ClientError::Invalid(format!(
                "duplicate node label: {}",
                node.label
            )));
        }
        if node.host_key_fingerprint.trim().is_empty() {
            return Err(ClientError::Invalid(format!(
                "node `{}` has no host_key_fingerprint",
                node.label
            )));
        }
        nodes.push(NodeConfig {
            label: node.label,
            host: node.host,
            port: node.port,
            key: node.key,
            host_key_fingerprint: node.host_key_fingerprint,
        });
    }
    Ok(ClientConfig {
        pool_size: raw.pool_size.unwrap_or(DEFAULT_POOL_SIZE).max(1),
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml(nodes: &str, pool: Option<usize>) -> String {
        let pool = pool
            .map(|p| format!("pool_size = {p}\n"))
            .unwrap_or_default();
        format!("{pool}{nodes}")
    }

    #[test]
    fn resolves_a_valid_configuration() {
        let config = parse(&toml(
            "[[nodes]]\nlabel = \"web1\"\nhost = \"10.0.0.5\"\nport = 2222\nkey = \"/k\"\nhost_key_fingerprint = \"SHA256:abc\"\n",
            None,
        ))
        .unwrap();
        assert_eq!(config.pool_size, DEFAULT_POOL_SIZE);
        assert_eq!(config.nodes[0].label, "web1");
    }

    #[test]
    fn rejects_an_empty_configuration() {
        assert!(parse("nodes = []").is_err());
    }

    #[test]
    fn rejects_duplicate_labels() {
        let node = "[[nodes]]\nlabel = \"dup\"\nhost = \"h\"\nport = 1\nkey = \"/k\"\nhost_key_fingerprint = \"SHA256:x\"\n";
        assert!(matches!(
            parse(&format!("{node}{node}")),
            Err(ClientError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_a_missing_fingerprint() {
        let config = parse(
            "[[nodes]]\nlabel = \"a\"\nhost = \"h\"\nport = 1\nkey = \"/k\"\nhost_key_fingerprint = \"\"\n",
        );
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }

    #[test]
    fn pool_size_floors_at_one() {
        let config = parse(&toml(
            "[[nodes]]\nlabel = \"a\"\nhost = \"h\"\nport = 1\nkey = \"/k\"\nhost_key_fingerprint = \"SHA256:x\"\n",
            Some(0),
        ))
        .unwrap();
        assert_eq!(config.pool_size, 1);
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(parse("mystery = 1\n").is_err());
    }
}
