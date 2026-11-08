//! The `russh` server adapter.
//!
//! Everything transport-specific lives here. The handler refuses every request except the
//! MCP subsystem, and the pure decisions it makes come from [`crate::policy`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rmcp::handler::server::ServerHandler;
use rmcp::serve_server;
use russh::keys::PrivateKey;
use russh::server::{
    Auth, Config, Msg, PublicKeyOffer, Response, Server as _, Session, VerifiedPublicKey,
};
use russh::{Channel, ChannelId, ChannelOpenFailure, Pty};
use tokio::net::TcpListener;

use crate::keys::AuthorizedKeys;
use crate::policy::{self, Principal};

/// Builds one MCP service per authenticated connection.
///
/// Kept generic so this crate never depends on the concrete MCP handler; the composition
/// root supplies the implementation.
pub trait ServiceFactory: Send + Sync + 'static {
    /// The MCP service built for a principal.
    type Service: ServerHandler + Send + 'static;
    /// Build the service a principal's channel serves.
    fn build(&self, principal: &Principal) -> Self::Service;
}

/// Connection and channel caps.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum simultaneous connections.
    pub max_connections: usize,
    /// Maximum simultaneous channels (MCP sessions) per connection.
    pub max_channels_per_connection: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 32,
            max_channels_per_connection: 8,
        }
    }
}

struct Shared<F> {
    factory: F,
    keys: AuthorizedKeys,
    limits: Limits,
    connections: AtomicUsize,
}

/// An embedded SSH server serving MCP over the `gilfoyle-mcp` subsystem.
pub struct SshServer<F: ServiceFactory> {
    shared: Arc<Shared<F>>,
    russh: Arc<Config>,
}

impl<F: ServiceFactory> SshServer<F> {
    /// Build a server from its host keys, service factory and authorized keys.
    pub fn new(
        host_keys: Vec<PrivateKey>,
        factory: F,
        keys: AuthorizedKeys,
        limits: Limits,
    ) -> Self {
        let russh = Arc::new(Config {
            keys: host_keys,
            inactivity_timeout: Some(Duration::from_secs(600)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            auth_rejection_time: Duration::from_secs(1),
            max_auth_attempts: 3,
            ..Default::default()
        });
        Self {
            shared: Arc::new(Shared {
                factory,
                keys,
                limits,
                connections: AtomicUsize::new(0),
            }),
            russh,
        }
    }

    /// Serve on an already-bound listener until it is dropped.
    pub async fn run_on(&self, listener: &TcpListener) -> std::io::Result<()> {
        let mut server = Server {
            shared: Arc::clone(&self.shared),
        };
        server
            .run_on_socket(Arc::clone(&self.russh), listener)
            .await
            .map_err(std::io::Error::other)
    }
}

/// One connection's handler.
struct Connection<F: ServiceFactory> {
    shared: Arc<Shared<F>>,
    principal: Option<Principal>,
    channels: HashMap<ChannelId, Channel<Msg>>,
    over_capacity: bool,
}

impl<F: ServiceFactory> Drop for Connection<F> {
    fn drop(&mut self) {
        self.shared.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Server<F: ServiceFactory> {
    shared: Arc<Shared<F>>,
}

impl<F: ServiceFactory> russh::server::Server for Server<F> {
    type Handler = Connection<F>;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self::Handler {
        let count = self.shared.connections.fetch_add(1, Ordering::SeqCst) + 1;
        Connection {
            shared: Arc::clone(&self.shared),
            principal: None,
            channels: HashMap::new(),
            over_capacity: count > self.shared.limits.max_connections,
        }
    }
}

/// Reject an authentication attempt with no further methods suggested.
fn reject() -> Auth {
    Auth::Reject {
        proceed_with_methods: None,
        partial_success: false,
    }
}

impl<F: ServiceFactory> russh::server::Handler for Connection<F> {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        Ok(reject())
    }

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(reject())
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        _response: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        Ok(reject())
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        offer: PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        if policy::algorithm_allowed(offer.algorithm.as_str()) {
            Ok(Auth::Accept)
        } else {
            Ok(reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        if self.over_capacity || !policy::algorithm_allowed(key.algorithm.as_str()) {
            return Ok(reject());
        }
        match self.shared.keys.authorize(key.public_key) {
            Some(entry) => {
                self.principal = Some(Principal::new(entry.id.clone()));
                Ok(Auth::Accept)
            }
            None => Ok(reject()),
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.channels.len() >= self.shared.limits.max_channels_per_connection {
            reply.reject(ChannelOpenFailure::ResourceShortage).await;
            return Ok(());
        }
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Some(handle) = self.channels.remove(&channel) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        if !policy::subsystem_allowed(name) {
            session.channel_failure(channel)?;
            return Ok(());
        }
        let Some(principal) = self.principal.clone() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        let service = self.shared.factory.build(&principal);
        let stream = handle.into_stream();
        tokio::spawn(async move {
            match serve_server(service, stream).await {
                // Keep the running service alive for the channel's lifetime.
                Ok(running) => {
                    let _ = running.waiting().await;
                }
                Err(error) => tracing::debug!(%error, "mcp session failed to initialize"),
            }
        });
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col: u32,
        _row: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        _value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.remove(&channel);
        Ok(())
    }
}
