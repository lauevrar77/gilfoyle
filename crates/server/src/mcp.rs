//! The MCP tool handler.
//!
//! Every tool is a compiled function with fixed behaviour. The handler holds only the
//! driving ports plus the tool allowlist; it performs no I/O itself.

use std::collections::BTreeSet;
use std::sync::Arc;

use gilfoyle_core::{
    SystemAdmin, SystemdAdmin, Unit, UnitActionOutcome, UnitFilter, UnitStatus, UnitType,
    UptimeReport,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ResultType,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;

/// Default number of units returned by `list_units`.
const DEFAULT_LIST_LIMIT: usize = 100;

/// Parameters of `list_units`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListUnitsParams {
    /// Only return units in this active state (for example `active`, `inactive`, `failed`).
    #[serde(default)]
    active_state: Option<String>,
    /// Only return units whose name contains this substring (case-insensitive).
    #[serde(default)]
    name_contains: Option<String>,
    /// Only return units of this type (`service`, `socket`, `timer`, `mount`, ...).
    #[serde(default)]
    unit_type: Option<String>,
    /// Maximum number of units to return. Defaults to 100.
    #[serde(default)]
    limit: Option<usize>,
}

impl ListUnitsParams {
    fn into_filter(self) -> Result<UnitFilter, String> {
        let unit_type = match self.unit_type {
            Some(value) => Some(UnitType::parse(&value).map_err(|error| error.to_string())?),
            None => None,
        };
        Ok(UnitFilter {
            active_state: self.active_state,
            name_contains: self.name_contains,
            unit_type,
            limit: Some(self.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
        })
    }
}

/// Parameters of `unit_status`, `start_unit` and `stop_unit`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UnitNameParams {
    /// The unit name, with or without its type suffix (for example `nginx` or `nginx.service`).
    name: String,
}

/// Turn a start/stop outcome into a tool result, marking a non-`done` job as an error.
fn outcome_result(outcome: UnitActionOutcome) -> Result<Json<UnitActionOutcome>, String> {
    if outcome.job_result.is_success() {
        Ok(Json(outcome))
    } else {
        Err(format!(
            "job for `{}` did not succeed: {} ({} -> {})",
            outcome.unit,
            outcome.job_result.result,
            outcome.active_state_before,
            outcome.active_state_after
        ))
    }
}

/// The gilfoyle MCP server.
#[derive(Clone)]
pub struct GilfoyleServer {
    admin: Arc<dyn SystemAdmin>,
    systemd: Arc<dyn SystemdAdmin>,
    enabled_tools: Option<BTreeSet<String>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl GilfoyleServer {
    /// Build a server over the given driving ports and tool allowlist.
    pub fn new(
        admin: Arc<dyn SystemAdmin>,
        systemd: Arc<dyn SystemdAdmin>,
        enabled_tools: Option<BTreeSet<String>>,
    ) -> Self {
        Self {
            admin,
            systemd,
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

    /// List systemd units.
    #[tool(
        description = "List systemd units, optionally filtered by active state, name or type",
        annotations(read_only_hint = true)
    )]
    async fn list_units(
        &self,
        Parameters(params): Parameters<ListUnitsParams>,
    ) -> Result<Json<Vec<Unit>>, String> {
        let filter = params.into_filter()?;
        self.systemd
            .list_units(&filter)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Show the status of one systemd unit.
    #[tool(
        description = "Show the detailed status of one systemd unit",
        annotations(read_only_hint = true)
    )]
    async fn unit_status(
        &self,
        Parameters(params): Parameters<UnitNameParams>,
    ) -> Result<Json<UnitStatus>, String> {
        self.systemd
            .unit_status(&params.name)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Start a managed systemd unit.
    #[tool(
        description = "Start a systemd unit (only units listed in [systemd] manage_units)",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn start_unit(
        &self,
        Parameters(params): Parameters<UnitNameParams>,
    ) -> Result<Json<UnitActionOutcome>, String> {
        let outcome = self
            .systemd
            .start_unit(&params.name)
            .await
            .map_err(|error| error.to_string())?;
        outcome_result(outcome)
    }

    /// Stop a managed systemd unit.
    #[tool(
        description = "Stop a systemd unit (only units listed in [systemd] manage_units)",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn stop_unit(
        &self,
        Parameters(params): Parameters<UnitNameParams>,
    ) -> Result<Json<UnitActionOutcome>, String> {
        let outcome = self
            .systemd
            .stop_unit(&params.name)
            .await
            .map_err(|error| error.to_string())?;
        outcome_result(outcome)
    }
}

#[tool_handler(
    name = "gilfoyle",
    instructions = "Administer a Linux host: read the uptime, inspect systemd units, and start or stop allowlisted units."
)]
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
