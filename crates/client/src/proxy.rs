//! The aggregation proxy: one MCP server over many nodes.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use futures_util::future::join_all;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
    PaginatedRequestParams, ResultType, ServerConfig, ServerNotification, Tool,
    ToolListChangedNotification, ToolsCapability,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::config::NodeConfig;
use crate::error::ClientError;
use crate::node::Upstream;

/// The `node` value that fans a read out to every node.
pub const ALL_NODES: &str = "all";

/// Tools that may be fanned out across every node.
pub const READ_TOOLS: [&str; 3] = ["get_uptime", "list_units", "unit_status"];

/// The name of the client-provided tool that lists the nodes.
pub const LIST_NODES: &str = "list_nodes";

/// The aggregated MCP server: one surface over many nodes.
#[derive(Clone)]
pub struct GilfoyleProxy {
    inner: Arc<ProxyState>,
}

struct ProxyState {
    configs: BTreeMap<String, NodeConfig>,
    order: Vec<String>,
    pool_size: usize,
    nodes: Mutex<HashMap<String, Arc<Upstream>>>,
}

impl GilfoyleProxy {
    /// Build the proxy from the configured nodes, the pool size and the reachable nodes.
    pub fn new(configs: Vec<NodeConfig>, pool_size: usize, nodes: Vec<Upstream>) -> Self {
        let order = configs.iter().map(|node| node.label.clone()).collect();
        let configs = configs
            .into_iter()
            .map(|node| (node.label.clone(), node))
            .collect();
        let nodes = nodes
            .into_iter()
            .map(|upstream| (upstream.label.clone(), Arc::new(upstream)))
            .collect();
        Self {
            inner: Arc::new(ProxyState {
                configs,
                order,
                pool_size,
                nodes: Mutex::new(nodes),
            }),
        }
    }

    async fn dispatch(
        &self,
        request: CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if request.name == LIST_NODES {
            return Ok(self.list_nodes().await);
        }

        let mut arguments = request.arguments.clone().unwrap_or_default();
        let node = match arguments.remove("node") {
            Some(Value::String(node)) => node,
            _ => return Err(ErrorData::invalid_params("missing `node` argument", None)),
        };

        if node == ALL_NODES {
            if !READ_TOOLS.contains(&request.name.as_ref()) {
                return Err(ErrorData::invalid_params(
                    "`node = \"all\"` is only allowed for read tools",
                    None,
                ));
            }
            return Ok(self.fan_out(&request.name, &arguments, context).await);
        }

        let read_only = READ_TOOLS.contains(&request.name.as_ref());
        self.call_node(&request.name, &node, arguments, read_only, context)
            .await
    }

    async fn call_node(
        &self,
        tool: &str,
        label: &str,
        arguments: Map<String, Value>,
        read_only: bool,
        context: &RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if !self.inner.configs.contains_key(label) {
            return Err(ErrorData::invalid_params(
                format!("unknown node `{label}`"),
                None,
            ));
        }

        let upstream =
            self.inner.get_or_reconnect(label).await.map_err(|error| {
                ErrorData::internal_error(format!("node `{label}`: {error}"), None)
            })?;

        match upstream
            .peer()
            .call_tool(forward(tool, arguments.clone()))
            .await
        {
            Ok(result) => Ok(result),
            Err(_error) if read_only => {
                // A read may be retried once on a fresh connection; a mutation never is.
                let upstream = self.inner.reconnect(label).await.map_err(|error| {
                    ErrorData::internal_error(format!("node `{label}`: {error}"), None)
                })?;
                notify_list_changed(context).await;
                upstream
                    .peer()
                    .call_tool(forward(tool, arguments))
                    .await
                    .map_err(|error| {
                        ErrorData::internal_error(format!("node `{label}`: {error}"), None)
                    })
            }
            Err(error) => Err(ErrorData::internal_error(
                format!("node `{label}`: {error}"),
                None,
            )),
        }
    }

    async fn list_nodes(&self) -> CallToolResult {
        let nodes = self.inner.nodes.lock().await;
        let entries: Vec<Value> = self
            .inner
            .order
            .iter()
            .map(|label| match nodes.get(label) {
                Some(upstream) => json!({
                    "label": label,
                    "reachable": true,
                    "tools": upstream
                        .tools()
                        .iter()
                        .map(|tool| tool.name.to_string())
                        .collect::<Vec<_>>(),
                }),
                None => json!({ "label": label, "reachable": false, "tools": [] }),
            })
            .collect();
        CallToolResult::structured(json!({ "nodes": entries }))
    }

    async fn fan_out(
        &self,
        tool: &str,
        arguments: &Map<String, Value>,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResult {
        let state = Arc::clone(&self.inner);
        let tool = tool.to_string();
        let futures = self.inner.order.iter().cloned().map(|label| {
            let state = Arc::clone(&state);
            let tool = tool.clone();
            let arguments = arguments.clone();
            async move {
                let value = match state.get_or_reconnect(&label).await {
                    Ok(upstream) => {
                        match upstream.peer().call_tool(forward(&tool, arguments)).await {
                            Ok(result) => result.structured_content.unwrap_or(Value::Null),
                            Err(error) => json!({ "error": error.to_string() }),
                        }
                    }
                    Err(error) => json!({ "error": error.to_string() }),
                };
                (label, value)
            }
        });
        let results = join_all(futures).await;
        notify_list_changed(context).await;
        let mut out = Map::new();
        for (label, value) in results {
            out.insert(label, value);
        }
        CallToolResult::structured(Value::Object(out))
    }

    async fn aggregate_tools(&self) -> Vec<Tool> {
        let nodes = self.inner.nodes.lock().await;
        let mut order: Vec<String> = Vec::new();
        let mut template: HashMap<String, Tool> = HashMap::new();
        let mut exposure: HashMap<String, Vec<String>> = HashMap::new();
        for label in &self.inner.order {
            if let Some(upstream) = nodes.get(label) {
                for tool in upstream.tools() {
                    let name = tool.name.to_string();
                    if !template.contains_key(&name) {
                        order.push(name.clone());
                        template.insert(name.clone(), tool.clone());
                    }
                    exposure.entry(name).or_default().push(label.clone());
                }
            }
        }

        let mut tools: Vec<Tool> = Vec::new();
        for name in order {
            if let Some(mut tool) = template.remove(&name) {
                let labels = exposure.get(&name).cloned().unwrap_or_default();
                tool.input_schema = Arc::new(inject_node(&tool.input_schema, &labels));
                tools.push(tool);
            }
        }
        tools.push(list_nodes_tool());
        tools
    }
}

impl ProxyState {
    async fn get_or_reconnect(&self, label: &str) -> Result<Arc<Upstream>, ClientError> {
        if let Some(upstream) = self.nodes.lock().await.get(label).cloned() {
            return Ok(upstream);
        }
        self.reconnect(label).await
    }

    async fn reconnect(&self, label: &str) -> Result<Arc<Upstream>, ClientError> {
        let config = self
            .configs
            .get(label)
            .ok_or_else(|| ClientError::Invalid(format!("unknown node `{label}`")))?;
        let upstream = Arc::new(Upstream::connect(config, self.pool_size).await?);
        self.nodes
            .lock()
            .await
            .insert(label.to_string(), Arc::clone(&upstream));
        Ok(upstream)
    }
}

async fn notify_list_changed(context: &RequestContext<RoleServer>) {
    let notification =
        ServerNotification::ToolListChangedNotification(ToolListChangedNotification::default());
    let _ = context.peer.send_notification(notification).await;
}

impl ServerHandler for GilfoyleProxy {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.server_info.name = "gilfoyle-client".to_string();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.instructions = Some(
            "Administer several gilfoyle nodes. Every tool takes a `node` argument (a label, \
             or `all` for the read tools); use `list_nodes` to discover the nodes."
                .to_string(),
        );
        let mut tools = ToolsCapability::default();
        tools.list_changed = Some(true);
        info.capabilities.tools = Some(tools);
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.aggregate_tools().await,
            result_type: Some(ResultType::COMPLETE),
            meta: None,
            next_cursor: None,
            ttl_ms: None,
            cache_scope: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.dispatch(request, &context)
            .await
            .map(CallToolResponse::from)
    }
}

/// Add a `node` property to a tool's input schema.
fn inject_node(schema: &Map<String, Value>, labels: &[String]) -> Map<String, Value> {
    let mut schema = schema.clone();
    let properties = schema
        .entry("properties".to_string())
        .or_insert_with(|| json!({}));
    if let Value::Object(map) = properties {
        map.insert(
            "node".to_string(),
            json!({ "type": "string", "enum": labels }),
        );
    }
    let required = schema
        .entry("required".to_string())
        .or_insert_with(|| json!([]));
    if let Value::Array(items) = required {
        let node = Value::String("node".to_string());
        if !items.contains(&node) {
            items.push(node);
        }
    }
    schema
}

fn list_nodes_tool() -> Tool {
    let schema = json!({ "type": "object", "properties": {} })
        .as_object()
        .cloned()
        .unwrap_or_default();
    Tool::new(
        LIST_NODES,
        "List the configured nodes and the tools each exposes",
        schema,
    )
}

fn forward(name: &str, arguments: Map<String, Value>) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    params.arguments = Some(arguments);
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_node_adds_a_required_enum() {
        let schema = json!({ "type": "object", "properties": { "name": { "type": "string" } } });
        let schema = schema.as_object().unwrap();
        let injected = inject_node(schema, &["a".to_string(), "b".to_string()]);
        assert_eq!(injected["properties"]["node"]["enum"][0], "a");
        assert_eq!(injected["required"][0], "node");
    }

    #[test]
    fn inject_node_does_not_duplicate_node() {
        let schema = json!({
            "type": "object",
            "properties": { "node": { "type": "string" } },
            "required": ["node"]
        });
        let injected = inject_node(schema.as_object().unwrap(), &["a".to_string()]);
        assert_eq!(injected["required"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn read_tools_do_not_include_mutations() {
        assert!(READ_TOOLS.contains(&"get_uptime"));
        assert!(!READ_TOOLS.contains(&"start_unit"));
    }
}
