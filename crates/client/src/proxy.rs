//! The aggregation proxy: one MCP server over many nodes.

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
    PaginatedRequestParams, ResultType, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};
use serde_json::{Map, Value, json};

use crate::node::Upstream;

/// The `node` value that fans a read out to every node.
pub const ALL_NODES: &str = "all";

/// Tools that may be fanned out across every node.
pub const READ_TOOLS: [&str; 3] = ["get_uptime", "list_units", "unit_status"];

/// The name of the client-provided tool that lists the nodes.
pub const LIST_NODES: &str = "list_nodes";

/// The aggregated MCP server.
#[derive(Clone)]
pub struct GilfoyleProxy {
    nodes: Arc<Vec<Upstream>>,
    labels: Arc<Vec<String>>,
    tools: Arc<Vec<Tool>>,
}

impl GilfoyleProxy {
    /// Build the proxy from the configured labels and the reachable nodes.
    pub fn new(labels: Vec<String>, nodes: Vec<Upstream>) -> Self {
        let mut order: Vec<String> = Vec::new();
        let mut template: HashMap<String, Tool> = HashMap::new();
        let mut exposure: HashMap<String, Vec<String>> = HashMap::new();
        for node in &nodes {
            for tool in node.tools() {
                let name = tool.name.to_string();
                if !template.contains_key(&name) {
                    order.push(name.clone());
                    template.insert(name.clone(), tool.clone());
                }
                exposure.entry(name).or_default().push(node.label.clone());
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

        Self {
            nodes: Arc::new(nodes),
            labels: Arc::new(labels),
            tools: Arc::new(tools),
        }
    }

    async fn dispatch(&self, request: CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        if request.name == LIST_NODES {
            return Ok(self.list_nodes());
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
            return Ok(self.fan_out(&request.name, &arguments).await);
        }

        let upstream = self
            .nodes
            .iter()
            .find(|upstream| upstream.label == node)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown node `{node}`"), None))?;

        upstream
            .peer()
            .call_tool(forward(&request.name, arguments))
            .await
            .map_err(|error| ErrorData::internal_error(format!("node `{node}`: {error}"), None))
    }

    fn list_nodes(&self) -> CallToolResult {
        let nodes: Vec<Value> = self
            .labels
            .iter()
            .map(
                |label| match self.nodes.iter().find(|upstream| &upstream.label == label) {
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
                },
            )
            .collect();
        CallToolResult::structured(json!({ "nodes": nodes }))
    }

    async fn fan_out(&self, name: &str, arguments: &Map<String, Value>) -> CallToolResult {
        let mut out = Map::new();
        for upstream in self.nodes.iter() {
            let value = match upstream
                .peer()
                .call_tool(forward(name, arguments.clone()))
                .await
            {
                Ok(result) => result.structured_content.unwrap_or(Value::Null),
                Err(error) => json!({ "error": error.to_string() }),
            };
            out.insert(upstream.label.clone(), value);
        }
        CallToolResult::structured(Value::Object(out))
    }
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
        info.capabilities.tools = Some(Default::default());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: (*self.tools).clone(),
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
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.dispatch(request).await.map(CallToolResponse::from)
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
