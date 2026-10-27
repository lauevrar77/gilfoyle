//! The MCP tool handler.
//!
//! Every tool is a compiled function with fixed behaviour. The handler holds
//! only the driving port plus the tool allowlist; it performs no I/O itself.

use std::collections::BTreeSet;
use std::sync::Arc;

use gilfoyle_core::{SystemAdmin, UptimeReport};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Json;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ResultType,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool, tool_handler, tool_router};

/// The gilfoyle MCP server.
#[derive(Clone)]
pub struct GilfoyleServer {
    admin: Arc<dyn SystemAdmin>,
    enabled_tools: Option<BTreeSet<String>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl GilfoyleServer {
    /// Build a server over the given driving port and tool allowlist.
    pub fn new(admin: Arc<dyn SystemAdmin>, enabled_tools: Option<BTreeSet<String>>) -> Self {
        Self {
            admin,
            enabled_tools,
            tool_router: Self::tool_router(),
        }
    }

    fn is_enabled(&self, name: &str) -> bool {
        match &self.enabled_tools {
            Some(enabled) => enabled.contains(name),
            None => true,
        }
    }

    /// Return the uptime of the host machine.
    #[tool(description = "Return the uptime of the host machine")]
    async fn get_uptime(&self) -> Result<Json<UptimeReport>, String> {
        self.admin.uptime().map(Json).map_err(|e| e.to_string())
    }
}

#[tool_handler(name = "gilfoyle")]
impl ServerHandler for GilfoyleServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| self.is_enabled(&tool.name))
            .collect();
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools,
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
        if !self.is_enabled(&request.name) {
            return Err(ErrorData::invalid_params(
                format!("tool `{}` is not enabled", request.name),
                None,
            ));
        }
        let context = ToolCallContext::new(self, request, context);
        self.tool_router.call(context).await
    }
}
