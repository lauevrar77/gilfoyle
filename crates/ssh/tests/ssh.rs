//! End-to-end tests: a real `russh` client talks to the embedded server over TCP.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use gilfoyle_ssh::adapter::{Limits, ServiceFactory, SshServer};
use gilfoyle_ssh::keys::{AuthorizedKey, AuthorizedKeys};
use gilfoyle_ssh::policy::{Principal, SUBSYSTEM};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

/// A client handler that records channel accept/refuse replies and trusts any host key.
#[derive(Clone, Default)]
struct TestClient {
    accepted: Arc<Mutex<Vec<russh::ChannelId>>>,
    refused: Arc<Mutex<Vec<russh::ChannelId>>>,
}

impl russh::client::Handler for TestClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn channel_success(
        &mut self,
        channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.accepted.lock().await.push(channel);
        Ok(())
    }

    async fn channel_failure(
        &mut self,
        channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.refused.lock().await.push(channel);
        Ok(())
    }
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

async fn start(keys: AuthorizedKeys) -> SocketAddr {
    let server = SshServer::new(vec![key()], EchoFactory, keys, Limits::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run_on(&listener).await;
    });
    addr
}

async fn connect(
    addr: SocketAddr,
    client_key: PrivateKey,
) -> (russh::client::Handle<TestClient>, TestClient) {
    let client = TestClient::default();
    let mut handle = russh::client::connect(
        Arc::new(russh::client::Config::default()),
        addr,
        client.clone(),
    )
    .await
    .unwrap();
    let auth = handle
        .authenticate_publickey(
            "anything",
            PrivateKeyWithHashAlg::new(Arc::new(client_key), None),
        )
        .await
        .unwrap();
    assert!(auth.success(), "expected authentication to succeed");
    (handle, client)
}

fn authorized(client_key: &PrivateKey) -> AuthorizedKeys {
    AuthorizedKeys::new(vec![AuthorizedKey {
        id: "nestor".to_string(),
        key: client_key.public_key().clone(),
        tools: None,
    }])
    .unwrap()
}

#[tokio::test]
async fn serves_mcp_over_the_subsystem() {
    let client_key = key();
    let addr = start(authorized(&client_key)).await;
    let (handle, client) = connect(addr, client_key).await;

    let channel = handle.channel_open_session().await.unwrap();
    channel.request_subsystem(true, SUBSYSTEM).await.unwrap();
    wait_for(&client.accepted, 1).await;

    let mut stream = channel.into_stream();
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" }
        }
    });
    let mut line = serde_json::to_string(&init).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
    assert_eq!(value["result"]["serverInfo"]["name"], "echo");
}

#[tokio::test]
async fn rejects_an_unauthorized_key() {
    let known = key();
    let addr = start(authorized(&known)).await;

    let stranger = key();
    let client = TestClient::default();
    let mut handle =
        russh::client::connect(Arc::new(russh::client::Config::default()), addr, client)
            .await
            .unwrap();
    let auth = handle
        .authenticate_publickey(
            "anything",
            PrivateKeyWithHashAlg::new(Arc::new(stranger), None),
        )
        .await
        .unwrap();
    assert!(!auth.success(), "a stranger must not authenticate");
}

#[tokio::test]
async fn refuses_a_shell_request() {
    let client_key = key();
    let addr = start(authorized(&client_key)).await;
    let (handle, client) = connect(addr, client_key).await;

    let channel = handle.channel_open_session().await.unwrap();
    let _ = channel.request_shell(true).await;
    wait_for(&client.refused, 1).await;
    assert!(
        client.accepted.lock().await.is_empty(),
        "no shell may be accepted"
    );
}

#[tokio::test]
async fn refuses_a_non_mcp_subsystem() {
    let client_key = key();
    let addr = start(authorized(&client_key)).await;
    let (handle, client) = connect(addr, client_key).await;

    let channel = handle.channel_open_session().await.unwrap();
    channel.request_subsystem(true, "sftp").await.unwrap();
    wait_for(&client.refused, 1).await;
    assert!(
        client.accepted.lock().await.is_empty(),
        "only the MCP subsystem is served"
    );
}

/// Wait until `recorded` has at least `count` entries, or time out.
async fn wait_for(recorded: &Arc<Mutex<Vec<russh::ChannelId>>>, count: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if recorded.lock().await.len() >= count {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {count} record(s)"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
