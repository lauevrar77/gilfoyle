//! End-to-end: the real client connects to a real node over SSH and calls a tool.

use std::net::SocketAddr;
use std::sync::Arc;

use gilfoyle_client::ClientError;
use gilfoyle_client::config::{JumpConfig, NodeConfig};
use gilfoyle_client::node::Upstream;
use gilfoyle_ssh::adapter::{Limits, ServiceFactory, SshServer};
use gilfoyle_ssh::keys::{AuthorizedKey, AuthorizedKeys};
use gilfoyle_ssh::policy::Principal;
use gilfoyle_ssh::russh::Channel;
use gilfoyle_ssh::russh::keys::{Algorithm, PrivateKey};
use gilfoyle_ssh::russh::server::{
    Auth, ChannelOpenHandle, Msg, Server as _, Session, VerifiedPublicKey,
};
use gilfoyle_ssh::{SUBSYSTEM, hostkey};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::CallToolRequestParams;
use rmcp::schemars;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    value: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EchoResult {
    value: String,
}

#[derive(Clone)]
struct Echo;

#[tool_router]
impl Echo {
    #[tool(description = "Echo a value back")]
    async fn echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<Json<EchoResult>, String> {
        Ok(Json(EchoResult {
            value: params.value,
        }))
    }
}

#[tool_handler(name = "echo", instructions = "test service")]
impl ServerHandler for Echo {}

struct EchoFactory;

impl ServiceFactory for EchoFactory {
    type Service = Echo;

    fn build(&self, _principal: &Principal) -> Echo {
        Echo
    }
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gilfoyle-client-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Start a node with an echo service and return its address, host fingerprint and the
/// path of the client's private key.
async fn start_node() -> (u16, String, std::path::PathBuf) {
    let host = key();
    let fingerprint = hostkey::fingerprint(host.public_key());

    let client_key = key();
    let dir = temp_dir("node");
    let key_path = dir.join("id_ed25519");
    hostkey::write(&client_key, &key_path).unwrap();

    let authorized = AuthorizedKeys::new(vec![AuthorizedKey {
        id: "nestor".to_string(),
        key: client_key.public_key().clone(),
        tools: None,
    }])
    .unwrap();

    let server = SshServer::new(vec![host], EchoFactory, authorized, Limits::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = server.run_on(&listener).await;
    });

    (port, fingerprint, key_path)
}

/// A minimal SSH server that accepts `direct-tcpip` and bridges it to a TCP target.
struct ForwardServer;

impl gilfoyle_ssh::russh::server::Server for ForwardServer {
    type Handler = ForwardConnection;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> ForwardConnection {
        ForwardConnection
    }
}

struct ForwardConnection;

impl gilfoyle_ssh::russh::server::Handler for ForwardConnection {
    type Error = gilfoyle_ssh::russh::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let target = format!("{host_to_connect}:{port_to_connect}");
        tokio::spawn(async move {
            let Ok(mut socket) = tokio::net::TcpStream::connect(target).await else {
                return;
            };
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
        });
        Ok(())
    }
}

/// Start a forwarding server and return its address and host fingerprint.
async fn start_forward_server() -> (u16, String) {
    let host = key();
    let fingerprint = hostkey::fingerprint(host.public_key());
    let config = Arc::new(gilfoyle_ssh::russh::server::Config {
        keys: vec![host],
        ..Default::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut server = ForwardServer;
        let _ = server.run_on_socket(config, &listener).await;
    });

    (port, fingerprint)
}

/// A jump key written to disk, as the client needs a path.
fn jump_key() -> std::path::PathBuf {
    let dir = temp_dir("jump");
    let path = dir.join("id_ed25519");
    hostkey::write(&key(), &path).unwrap();
    path
}

fn node_config(port: u16, fingerprint: String, key_path: std::path::PathBuf) -> NodeConfig {
    NodeConfig {
        label: "node1".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        key: key_path,
        host_key_fingerprint: fingerprint,
        jump: None,
    }
}

#[tokio::test]
async fn connects_and_calls_a_tool() {
    let (port, fingerprint, key_path) = start_node().await;
    let node = node_config(port, fingerprint, key_path);

    let upstream = Upstream::connect(&node, 1).await.unwrap();

    let names: Vec<String> = upstream
        .tools()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert!(names.contains(&"echo".to_string()), "got {names:?}");

    let mut params = CallToolRequestParams::new("echo".to_string());
    params.arguments = json!({ "value": "hi" }).as_object().cloned();
    let result = upstream.peer().call_tool(params).await.unwrap();
    assert_eq!(result.structured_content.unwrap()["value"], "hi");
}

#[tokio::test]
async fn refuses_a_mismatched_host_key() {
    let (port, _fingerprint, key_path) = start_node().await;
    let node = node_config(port, "SHA256:not-the-right-key".to_string(), key_path);

    let result = Upstream::connect(&node, 1).await;
    assert!(
        result.is_err(),
        "a mismatched host key must fail the connection"
    );
}

#[tokio::test]
async fn connects_through_a_jump_host() {
    let (node_port, node_fingerprint, node_key) = start_node().await;
    let (jump_port, jump_fingerprint) = start_forward_server().await;

    let mut node = node_config(node_port, node_fingerprint, node_key);
    node.jump = Some(JumpConfig {
        name: "bastion".to_string(),
        host: "127.0.0.1".to_string(),
        port: jump_port,
        user: "operator".to_string(),
        key: jump_key(),
        host_key_fingerprint: jump_fingerprint,
    });

    let upstream = Upstream::connect(&node, 1).await.unwrap();

    let names: Vec<String> = upstream
        .tools()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert!(names.contains(&"echo".to_string()), "got {names:?}");

    let mut params = CallToolRequestParams::new("echo".to_string());
    params.arguments = json!({ "value": "via-jump" }).as_object().cloned();
    let result = upstream.peer().call_tool(params).await.unwrap();
    assert_eq!(result.structured_content.unwrap()["value"], "via-jump");
}

#[tokio::test]
async fn refuses_a_mismatched_jump_host_key() {
    let (node_port, node_fingerprint, node_key) = start_node().await;
    let (jump_port, _jump_fingerprint) = start_forward_server().await;

    let mut node = node_config(node_port, node_fingerprint, node_key);
    node.jump = Some(JumpConfig {
        name: "bastion".to_string(),
        host: "127.0.0.1".to_string(),
        port: jump_port,
        user: "operator".to_string(),
        key: jump_key(),
        host_key_fingerprint: "SHA256:not-the-right-key".to_string(),
    });

    let error = Upstream::connect(&node, 1).await;
    assert!(
        matches!(error, Err(ClientError::Jump { .. })),
        "expected a jump error"
    );
}

#[tokio::test]
async fn a_gilfoyle_node_refuses_to_be_a_jump_host() {
    let (node_port, node_fingerprint, node_key) = start_node().await;
    // A second gilfoyle node, used as the "jump host": it authenticates the client but
    // refuses the `direct-tcpip` channel, so it cannot relay.
    let (jump_port, jump_fingerprint, jump_key) = start_node().await;

    let mut node = node_config(node_port, node_fingerprint, node_key);
    node.jump = Some(JumpConfig {
        name: "gilfoyle".to_string(),
        host: "127.0.0.1".to_string(),
        port: jump_port,
        user: "operator".to_string(),
        key: jump_key,
        host_key_fingerprint: jump_fingerprint,
    });

    let error = Upstream::connect(&node, 1).await;
    assert!(
        matches!(error, Err(ClientError::Jump { .. })),
        "expected a jump error"
    );
}

// The subsystem constant is the contract between client and node.
#[test]
fn subsystem_is_the_mcp_contract() {
    assert_eq!(SUBSYSTEM, "gilfoyle-mcp");
}
