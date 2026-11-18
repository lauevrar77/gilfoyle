//! Client configuration.

use std::collections::{BTreeMap, BTreeSet};
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
    /// The jump hosts a node may be reached through.
    #[serde(default)]
    pub jumps: Vec<RawJump>,
    /// The nodes to aggregate.
    #[serde(default)]
    pub nodes: Vec<RawNode>,
}

/// Raw `[[jumps]]` entry: an SSH host the client relays through.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawJump {
    /// The jump name; referenced by a node's `jump` field.
    pub name: String,
    /// The jump host.
    pub host: String,
    /// The jump port.
    pub port: u16,
    /// The SSH user to authenticate as on the jump host.
    pub user: String,
    /// Path to the client's private key for the jump host.
    pub key: PathBuf,
    /// The expected `SHA256:` host-key fingerprint of the jump host.
    pub host_key_fingerprint: String,
}

/// Raw `[[nodes]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawNode {
    /// The node label; must be unique and is the value of the `node` argument.
    pub label: String,
    /// The node host, reached from the jump host when one is set.
    pub host: String,
    /// The node port.
    pub port: u16,
    /// Path to the client's private key for this node.
    pub key: PathBuf,
    /// The expected `SHA256:` host-key fingerprint.
    pub host_key_fingerprint: String,
    /// The name of the jump host to relay through, if any.
    #[serde(default)]
    pub jump: Option<String>,
}

/// A fully resolved jump host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpConfig {
    /// The jump name.
    pub name: String,
    /// The jump host.
    pub host: String,
    /// The jump port.
    pub port: u16,
    /// The SSH user to authenticate as on the jump host.
    pub user: String,
    /// Path to the client's private key for the jump host.
    pub key: PathBuf,
    /// The expected host-key fingerprint of the jump host.
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
    /// The jump host to relay through, if any.
    pub jump: Option<JumpConfig>,
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
    let jumps = resolve_jumps(raw.jumps)?;
    if raw.nodes.is_empty() {
        return Err(ClientError::Invalid(
            "at least one node is required".to_string(),
        ));
    }
    let mut labels = BTreeSet::new();
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
        let jump = match &node.jump {
            Some(name) => Some(jumps.get(name).cloned().ok_or_else(|| {
                ClientError::Invalid(format!(
                    "node `{}` references unknown jump `{name}`",
                    node.label
                ))
            })?),
            None => None,
        };
        nodes.push(NodeConfig {
            label: node.label,
            host: node.host,
            port: node.port,
            key: node.key,
            host_key_fingerprint: node.host_key_fingerprint,
            jump,
        });
    }
    Ok(ClientConfig {
        pool_size: raw.pool_size.unwrap_or(DEFAULT_POOL_SIZE).max(1),
        nodes,
    })
}

/// Validate the jump hosts and index them by name. Unreferenced jumps are allowed.
fn resolve_jumps(raw: Vec<RawJump>) -> Result<BTreeMap<String, JumpConfig>, ClientError> {
    let mut jumps = BTreeMap::new();
    for jump in raw {
        if jump.name.trim().is_empty() {
            return Err(ClientError::Invalid(
                "jump name must not be empty".to_string(),
            ));
        }
        if jump.user.trim().is_empty() {
            return Err(ClientError::Invalid(format!(
                "jump `{}` has no user",
                jump.name
            )));
        }
        if jump.host_key_fingerprint.trim().is_empty() {
            return Err(ClientError::Invalid(format!(
                "jump `{}` has no host_key_fingerprint",
                jump.name
            )));
        }
        if jumps.contains_key(&jump.name) {
            return Err(ClientError::Invalid(format!(
                "duplicate jump name: {}",
                jump.name
            )));
        }
        jumps.insert(
            jump.name.clone(),
            JumpConfig {
                name: jump.name,
                host: jump.host,
                port: jump.port,
                user: jump.user,
                key: jump.key,
                host_key_fingerprint: jump.host_key_fingerprint,
            },
        );
    }
    Ok(jumps)
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

    const JUMP: &str = "[[jumps]]\nname = \"bastion\"\nhost = \"bastion\"\nport = 22\nuser = \"laurent\"\nkey = \"/jk\"\nhost_key_fingerprint = \"SHA256:j\"\n";

    fn node(jump: Option<&str>) -> String {
        let jump = jump
            .map(|name| format!("jump = \"{name}\"\n"))
            .unwrap_or_default();
        format!(
            "[[nodes]]\nlabel = \"web1\"\nhost = \"10.0.0.5\"\nport = 2222\nkey = \"/k\"\nhost_key_fingerprint = \"SHA256:abc\"\n{jump}"
        )
    }

    #[test]
    fn resolves_a_jump_and_attaches_it_to_the_node() {
        let config = parse(&format!("{JUMP}{}", node(Some("bastion")))).unwrap();
        let jump = config.nodes[0].jump.as_ref().unwrap();
        assert_eq!(jump.name, "bastion");
        assert_eq!(jump.user, "laurent");
        assert_eq!(jump.port, 22);
    }

    #[test]
    fn a_node_without_a_jump_stays_direct() {
        let config = parse(&format!("{JUMP}{}", node(None))).unwrap();
        assert!(config.nodes[0].jump.is_none());
    }

    #[test]
    fn allows_an_unreferenced_jump() {
        let config = parse(&format!("{JUMP}{}", node(None))).unwrap();
        assert_eq!(config.nodes.len(), 1);
    }

    #[test]
    fn rejects_a_node_with_an_unknown_jump() {
        let config = parse(&format!("{JUMP}{}", node(Some("nope"))));
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }

    #[test]
    fn rejects_a_duplicate_jump_name() {
        let config = parse(&format!("{JUMP}{JUMP}{}", node(None)));
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }

    #[test]
    fn rejects_a_jump_with_an_empty_name() {
        let jump = "[[jumps]]\nname = \"\"\nhost = \"h\"\nport = 22\nuser = \"u\"\nkey = \"/jk\"\nhost_key_fingerprint = \"SHA256:j\"\n";
        let config = parse(&format!("{jump}{}", node(None)));
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }

    #[test]
    fn rejects_a_jump_without_a_user() {
        let jump = "[[jumps]]\nname = \"b\"\nhost = \"h\"\nport = 22\nuser = \"\"\nkey = \"/jk\"\nhost_key_fingerprint = \"SHA256:j\"\n";
        let config = parse(&format!("{jump}{}", node(None)));
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }

    #[test]
    fn rejects_a_jump_without_a_fingerprint() {
        let jump = "[[jumps]]\nname = \"b\"\nhost = \"h\"\nport = 22\nuser = \"u\"\nkey = \"/jk\"\nhost_key_fingerprint = \"\"\n";
        let config = parse(&format!("{jump}{}", node(None)));
        assert!(matches!(config, Err(ClientError::Invalid(_))));
    }
}
