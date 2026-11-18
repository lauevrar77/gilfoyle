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

/// Timeout for reaching a jump host and opening the forwarding channel to a node.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The originator address reported on a forwarding channel; gilfoyle does not expose it.
const ORIGINATOR: &str = "gilfoyle-client";

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
    /// The jump host's session, kept alive because the node's transport rides on it.
    _jump: Option<Handle<Pinned>>,
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

        let (mut handle, jump) = open_session(node).await?;

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
            _jump: jump,
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

/// Open the SSH session to the node, directly or through a jump host.
///
/// When a jump host is used, the returned handle must be kept alive for as long as the
/// node's sessions live: the node's transport is a `direct-tcpip` channel of that
/// connection. The jump host only relays bytes, so the node's session stays end-to-end
/// encrypted and its host key is pinned as usual.
async fn open_session(
    node: &NodeConfig,
) -> Result<(Handle<Pinned>, Option<Handle<Pinned>>), ClientError> {
    let Some(jump) = &node.jump else {
        let handle = russh::client::connect(
            Arc::new(russh::client::Config::default()),
            (node.host.as_str(), node.port),
            Pinned {
                expected: node.host_key_fingerprint.clone(),
            },
        )
        .await
        .map_err(|error| node_error(&node.label, error))?;
        return Ok((handle, None));
    };

    let key = PrivateKey::read_openssh_file(&jump.key).map_err(|error| ClientError::Key {
        path: jump.key.display().to_string(),
        cause: error.to_string(),
    })?;

    let mut jump_handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        russh::client::connect(
            Arc::new(russh::client::Config::default()),
            (jump.host.as_str(), jump.port),
            Pinned {
                expected: jump.host_key_fingerprint.clone(),
            },
        ),
    )
    .await
    .map_err(|_| jump_error(&jump.name, "connection timed out"))?
    .map_err(|error| jump_error(&jump.name, error))?;

    let auth = jump_handle
        .authenticate_publickey(
            jump.user.clone(),
            PrivateKeyWithHashAlg::new(Arc::new(key), None),
        )
        .await
        .map_err(|error| jump_error(&jump.name, error))?;
    if !auth.success() {
        return Err(jump_error(
            &jump.name,
            "public-key authentication was rejected",
        ));
    }

    let channel = tokio::time::timeout(
        CONNECT_TIMEOUT,
        jump_handle.channel_open_direct_tcpip(
            node.host.clone(),
            u32::from(node.port),
            ORIGINATOR,
            0,
        ),
    )
    .await
    .map_err(|_| jump_error(&jump.name, "opening the forwarding channel timed out"))?
    .map_err(|error| {
        jump_error(
            &jump.name,
            format!(
                "could not open a forwarding channel to {}:{} (is the jump host an \
                 OpenSSH sshd with AllowTcpForwarding?): {error}",
                node.host, node.port
            ),
        )
    })?;

    let handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        russh::client::connect_stream(
            Arc::new(russh::client::Config::default()),
            channel.into_stream(),
            Pinned {
                expected: node.host_key_fingerprint.clone(),
            },
        ),
    )
    .await
    .map_err(|_| node_error(&node.label, "connection timed out"))?
    .map_err(|error| node_error(&node.label, error))?;

    Ok((handle, Some(jump_handle)))
}

fn node_error(label: &str, error: impl std::fmt::Display) -> ClientError {
    ClientError::Node {
        label: label.to_string(),
        cause: error.to_string(),
    }
}

fn jump_error(name: &str, error: impl std::fmt::Display) -> ClientError {
    ClientError::Jump {
        name: name.to_string(),
        cause: error.to_string(),
    }
}
