//! The MCP tool handler.
//!
//! Every tool is a compiled function with fixed behaviour. The handler holds only the
//! driving ports plus the tool allowlist; it performs no I/O itself.

use std::collections::BTreeSet;
use std::sync::Arc;

use gilfoyle_core::{
    PackageAdmin, PackageFilter, PackageList, RefreshReport, Severity, SystemAdmin, SystemdAdmin,
    UnitActionOutcome, UnitFilter, UnitList, UnitStatus, UnitType, UpdateFilter, UpdateList,
    UpgradePlan, UpgradeReport, UptimeReport,
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

/// Default number of entries returned by the package tools.
const DEFAULT_PACKAGES_LIMIT: usize = 200;

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

/// Parameters of `list_packages`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListPackagesParams {
    /// Only return packages whose name contains this substring (case-insensitive).
    #[serde(default)]
    name_contains: Option<String>,
    /// Maximum number of packages to return. Defaults to 200.
    #[serde(default)]
    limit: Option<usize>,
}

impl ListPackagesParams {
    fn into_filter(self) -> PackageFilter {
        PackageFilter {
            name_contains: self.name_contains,
            limit: Some(self.limit.unwrap_or(DEFAULT_PACKAGES_LIMIT)),
        }
    }
}

/// Parameters of `list_updates`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListUpdatesParams {
    /// Only return updates at or above this severity (`low`, `medium`, `high`, `critical`).
    #[serde(default)]
    min_severity: Option<String>,
    /// Maximum number of updates to return. Defaults to 200.
    #[serde(default)]
    limit: Option<usize>,
}

impl ListUpdatesParams {
    fn into_filter(self) -> Result<UpdateFilter, String> {
        let min_severity = match self.min_severity {
            Some(value) => Some(Severity::parse(&value).map_err(|error| error.to_string())?),
            None => None,
        };
        Ok(UpdateFilter {
            min_severity,
            limit: Some(self.limit.unwrap_or(DEFAULT_PACKAGES_LIMIT)),
        })
    }
}

/// Parameters of `refresh_package_cache`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RefreshParams {
    /// Reload the repositories even if the cached metadata looks fresh. Defaults to true.
    #[serde(default = "default_true")]
    force: bool,
}

fn default_true() -> bool {
    true
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
    packages: Arc<dyn PackageAdmin>,
    enabled_tools: Option<BTreeSet<String>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl GilfoyleServer {
    /// Build a server over the given driving ports and tool allowlist.
    pub fn new(
        admin: Arc<dyn SystemAdmin>,
        systemd: Arc<dyn SystemdAdmin>,
        packages: Arc<dyn PackageAdmin>,
        enabled_tools: Option<BTreeSet<String>>,
    ) -> Self {
        Self {
            admin,
            systemd,
            packages,
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
    ) -> Result<Json<UnitList>, String> {
        let filter = params.into_filter()?;
        let units = self
            .systemd
            .list_units(&filter)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Json(UnitList { units }))
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

    /// List installed packages.
    #[tool(
        description = "List installed packages and their versions",
        annotations(read_only_hint = true)
    )]
    async fn list_packages(
        &self,
        Parameters(params): Parameters<ListPackagesParams>,
    ) -> Result<Json<PackageList>, String> {
        self.packages
            .list_packages(&params.into_filter())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// List available updates.
    #[tool(
        description = "List available package updates",
        annotations(read_only_hint = true)
    )]
    async fn list_updates(
        &self,
        Parameters(params): Parameters<ListUpdatesParams>,
    ) -> Result<Json<UpdateList>, String> {
        let filter = params.into_filter()?;
        self.packages
            .list_updates(&filter)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Preview an upgrade without applying it.
    #[tool(
        description = "Preview what upgrading would do, without applying it",
        annotations(read_only_hint = true)
    )]
    async fn preview_upgrade(&self) -> Result<Json<UpgradePlan>, String> {
        self.packages
            .preview_upgrade()
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Refresh the package metadata cache.
    #[tool(
        description = "Refresh the package metadata cache (only when [packagekit] allow includes refresh)",
        annotations(idempotent_hint = true)
    )]
    async fn refresh_package_cache(
        &self,
        Parameters(params): Parameters<RefreshParams>,
    ) -> Result<Json<RefreshReport>, String> {
        self.packages
            .refresh_packages(params.force)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Apply available upgrades.
    #[tool(
        description = "Apply available package upgrades (only when [packagekit] allow includes upgrade)",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn upgrade_packages(&self) -> Result<Json<UpgradeReport>, String> {
        self.packages
            .upgrade_packages()
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[tool_handler(
    name = "gilfoyle",
    instructions = "Administer a Linux host: read the uptime, inspect systemd units, start or stop allowlisted units, and inspect or upgrade packages (never install or remove)."
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_packages_filter_defaults_the_limit() {
        let filter = ListPackagesParams {
            name_contains: Some("nginx".to_string()),
            limit: None,
        }
        .into_filter();
        assert_eq!(filter.name_contains.as_deref(), Some("nginx"));
        assert_eq!(filter.limit, Some(DEFAULT_PACKAGES_LIMIT));

        let filter = ListPackagesParams {
            name_contains: None,
            limit: Some(5),
        }
        .into_filter();
        assert_eq!(filter.name_contains, None);
        assert_eq!(filter.limit, Some(5));
    }

    #[test]
    fn list_updates_filter_parses_and_rejects_severity() {
        let filter = ListUpdatesParams {
            min_severity: Some("high".to_string()),
            limit: None,
        }
        .into_filter()
        .unwrap();
        assert_eq!(filter.min_severity, Some(Severity::High));
        assert_eq!(filter.limit, Some(DEFAULT_PACKAGES_LIMIT));

        assert!(
            ListUpdatesParams {
                min_severity: Some("nope".to_string()),
                limit: None,
            }
            .into_filter()
            .is_err()
        );
    }

    #[test]
    fn refresh_defaults_to_force() {
        assert!(default_true());
    }
}
