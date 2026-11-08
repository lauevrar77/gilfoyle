//! Upstream node connections: a pooled set of MCP sessions per node.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gilfoyle_ssh::russh::client::Handle;
use gilfoyle_ssh::russh::keys::{PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use gilfoyle_ssh::{SUBSYSTEM, hostkey, russh};
use rmcp::RoleClient;
use rmcp::handler::client::ClientHandler;
use rmcp::model::Tool;
use rmcp::service::{Peer, RunningService};

use crate::config::NodeConfig;
use crate::error::ClientError;

/// Timeout for one MCP session handshake (an SSH subsystem request plus initialize).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// The upstream MCP client handler; the default implementation is enough.
#[derive(Clone)]
pub struct UpstreamClient;

impl ClientHandler for UpstreamClient {}

/// Pins the node's host key to an expected fingerprint.
struct Pinned {
    expected: String,
}

impl russh::client::Handler for Pinned {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let actual = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => hostkey::fingerprint(key),
            // Certificates are not part of the trust model: only raw public keys are accepted.
            PublicKeyOrCertificate::Certificate(_) => return Ok(false),
        };
        Ok(actual == self.expected)
    }
}

/// A connected node: a pooled set of MCP sessions plus its tool list.
pub struct Upstream {
    /// The node label.
    pub label: String,
    _handle: Handle<Pinned>,
    _services: Vec<RunningService<RoleClient, UpstreamClient>>,
    peers: Vec<Peer<RoleClient>>,
    next: AtomicUsize,
    tools: Vec<Tool>,
}

impl Upstream {
    /// Connect to a node and open `pool_size` MCP sessions.
    pub async fn connect(node: &NodeConfig, pool_size: usize) -> Result<Self, ClientError> {
        let key = PrivateKey::read_openssh_file(&node.key).map_err(|error| ClientError::Key {
            path: node.key.display().to_string(),
            cause: error.to_string(),
        })?;

        let config = Arc::new(russh::client::Config::default());
        let mut handle = russh::client::connect(
            config,
            (node.host.as_str(), node.port),
            Pinned {
                expected: node.host_key_fingerprint.clone(),
            },
        )
        .await
        .map_err(|error| node_error(&node.label, error))?;

        let auth = handle
            .authenticate_publickey(
                node.label.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key), None),
            )
            .await
            .map_err(|error| node_error(&node.label, error))?;
        if !auth.success() {
            return Err(ClientError::Node {
                label: node.label.clone(),
                cause: "public-key authentication was rejected".to_string(),
            });
        }

        let mut services = Vec::with_capacity(pool_size);
        let mut peers = Vec::with_capacity(pool_size);
        let mut tools = Vec::new();
        for index in 0..pool_size {
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|error| node_error(&node.label, error))?;
            channel
                .request_subsystem(true, SUBSYSTEM)
                .await
                .map_err(|error| node_error(&node.label, error))?;
            let service = tokio::time::timeout(
                HANDSHAKE_TIMEOUT,
                rmcp::serve_client(UpstreamClient, channel.into_stream()),
            )
            .await
            .map_err(|_| ClientError::Node {
                label: node.label.clone(),
                cause: "mcp handshake timed out".to_string(),
            })?
            .map_err(|error| node_error(&node.label, error))?;
            if index == 0 {
                let listed = service
                    .peer()
                    .list_tools(None)
                    .await
                    .map_err(|error| node_error(&node.label, error))?;
                tools = listed.tools;
            }
            peers.push(service.peer().clone());
            services.push(service);
        }

        Ok(Self {
            label: node.label.clone(),
            _handle: handle,
            _services: services,
            peers,
            next: AtomicUsize::new(0),
            tools,
        })
    }

    /// A session from the pool, round-robin.
    pub fn peer(&self) -> &Peer<RoleClient> {
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.peers.len();
        &self.peers[index]
    }

    /// The tools the node reported at connect time.
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }
}

fn node_error(label: &str, error: impl std::fmt::Display) -> ClientError {
    ClientError::Node {
        label: label.to_string(),
        cause: error.to_string(),
    }
}
