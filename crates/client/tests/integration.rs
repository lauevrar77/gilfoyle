//! End-to-end: the real client connects to a real node over SSH and calls a tool.

use gilfoyle_client::config::NodeConfig;
use gilfoyle_client::node::Upstream;
use gilfoyle_ssh::adapter::{Limits, ServiceFactory, SshServer};
use gilfoyle_ssh::keys::{AuthorizedKey, AuthorizedKeys};
use gilfoyle_ssh::policy::Principal;
use gilfoyle_ssh::russh::keys::{Algorithm, PrivateKey};
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

fn node_config(port: u16, fingerprint: String, key_path: std::path::PathBuf) -> NodeConfig {
    NodeConfig {
        label: "node1".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        key: key_path,
        host_key_fingerprint: fingerprint,
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

// The subsystem constant is the contract between client and node.
#[test]
fn subsystem_is_the_mcp_contract() {
    assert_eq!(SUBSYSTEM, "gilfoyle-mcp");
}
