//! Proxy behaviour: routing, node schema injection, fan-out and node listing.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gilfoyle_client::GilfoyleProxy;
use gilfoyle_client::config::NodeConfig;
use gilfoyle_client::node::Upstream;
use gilfoyle_ssh::adapter::{Limits, ServiceFactory, SshServer};
use gilfoyle_ssh::hostkey;
use gilfoyle_ssh::keys::{AuthorizedKey, AuthorizedKeys};
use gilfoyle_ssh::policy::Principal;
use gilfoyle_ssh::russh::keys::{Algorithm, PrivateKey};
use rmcp::RoleClient;
use rmcp::handler::client::ClientHandler;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::CallToolRequestParams;
use rmcp::schemars;
use rmcp::service::RunningService;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    value: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EchoResult {
    value: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct UptimeResult {
    uptime_seconds: f64,
}

#[derive(Clone)]
struct Node;

#[tool_router]
impl Node {
    #[tool(description = "Echo a value back")]
    async fn echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<Json<EchoResult>, String> {
        Ok(Json(EchoResult {
            value: params.value,
        }))
    }

    #[tool(description = "Host uptime")]
    async fn get_uptime(&self) -> Result<Json<UptimeResult>, String> {
        Ok(Json(UptimeResult {
            uptime_seconds: 1.0,
        }))
    }
}

#[tool_handler(name = "node", instructions = "test node")]
impl ServerHandler for Node {}

struct NodeFactory;

impl ServiceFactory for NodeFactory {
    type Service = Node;

    fn build(&self, _principal: &Principal) -> Node {
        Node
    }
}

#[derive(Clone)]
struct TestClient {
    tool_list_changed: Arc<AtomicUsize>,
}

impl TestClient {
    fn new() -> Self {
        Self {
            tool_list_changed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ClientHandler for TestClient {
    async fn on_tool_list_changed(&self, _context: rmcp::service::NotificationContext<RoleClient>) {
        self.tool_list_changed.fetch_add(1, Ordering::SeqCst);
    }
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

async fn start_node() -> NodeConfig {
    let host = key();
    let fingerprint = hostkey::fingerprint(host.public_key());

    let client_key = key();
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gilfoyle-proxy-{}-{unique}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let key_path = dir.join("id_ed25519");
    hostkey::write(&client_key, &key_path).unwrap();

    let authorized = AuthorizedKeys::new(vec![AuthorizedKey {
        id: "nestor".to_string(),
        key: client_key.public_key().clone(),
        tools: None,
    }])
    .unwrap();

    let server = SshServer::new(vec![host], NodeFactory, authorized, Limits::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = server.run_on(&listener).await;
    });

    NodeConfig {
        label: "node1".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        key: key_path,
        host_key_fingerprint: fingerprint,
    }
}

async fn serve_proxy() -> (RunningService<RoleClient, TestClient>, Arc<AtomicUsize>) {
    let config = start_node().await;
    let upstream = Upstream::connect(&config, 1).await.unwrap();
    let proxy = GilfoyleProxy::new(vec![config], 1, vec![upstream]);

    let client = TestClient::new();
    let notifications = Arc::clone(&client.tool_list_changed);

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = rmcp::serve_server(proxy, server_io).await {
            let _ = running.waiting().await;
        }
    });
    let running = rmcp::serve_client(client, client_io).await.unwrap();
    (running, notifications)
}

fn call(name: &str, arguments: Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    params.arguments = arguments.as_object().cloned();
    params
}

#[tokio::test]
async fn lists_aggregated_tools_with_a_node_enum() {
    let (client, _notified) = serve_proxy().await;
    let tools = client.peer().list_tools(None).await.unwrap().tools;

    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "echo".to_string(),
            "get_uptime".to_string(),
            "list_nodes".to_string()
        ]
    );

    let echo = tools
        .iter()
        .find(|tool| tool.name == "echo")
        .expect("echo present");
    assert_eq!(echo.input_schema["properties"]["node"]["enum"][0], "node1");
    assert!(
        echo.input_schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("node")),
        "`node` must be required"
    );

    // The advertised server identity comes from get_info.
    let info = client.peer_info().expect("peer info");
    assert_eq!(info.server_info.as_ref().unwrap().name, "gilfoyle-client");
}

#[tokio::test]
async fn routes_a_call_to_the_named_node() {
    let (client, _notified) = serve_proxy().await;
    let result = client
        .peer()
        .call_tool(call("echo", json!({ "node": "node1", "value": "hi" })))
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["value"], "hi");
}

#[tokio::test]
async fn rejects_an_unknown_or_missing_node() {
    let (client, _notified) = serve_proxy().await;

    let unknown = client
        .peer()
        .call_tool(call("echo", json!({ "node": "nope", "value": "hi" })))
        .await;
    assert!(unknown.is_err(), "an unknown node must be rejected");

    let missing = client.peer().call_tool(call("echo", json!({}))).await;
    assert!(missing.is_err(), "a missing node argument must be rejected");
}

#[tokio::test]
async fn lists_nodes_and_fans_out_reads() {
    let (client, notified) = serve_proxy().await;

    let nodes = client
        .peer()
        .call_tool(call("list_nodes", json!({})))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(nodes["nodes"][0]["label"], "node1");
    assert_eq!(nodes["nodes"][0]["reachable"], true);

    let fan = client
        .peer()
        .call_tool(call("get_uptime", json!({ "node": "all" })))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(fan["node1"]["uptime_seconds"], 1.0);

    // A fan-out refreshes the tool list, so the client is notified.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while notified.load(Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected a tools/list_changed notification"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // A mutation may not be fanned out.
    let mutation = client
        .peer()
        .call_tool(call("start_unit", json!({ "node": "all" })))
        .await;
    assert!(mutation.is_err(), "`all` is only for read tools");
}
