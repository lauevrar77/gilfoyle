//! Composition: build the driven adapters, the per-key service factory, and run the
//! embedded SSH server.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use gilfoyle_core::{
    ServiceCatalog, ServiceControl, SystemAdmin, SystemService, SystemdAdmin, SystemdService,
};
use gilfoyle_ssh::{Principal, ServiceFactory, SshServer, hostkey, policy};
use gilfoyle_system::ProcUptime;
use gilfoyle_systemd::LazySystemd;

use crate::config::Config;
use crate::mcp::GilfoyleServer;

/// Builds a [`GilfoyleServer`] per authenticated key, with that key's tool grant.
pub struct ServiceBuilder {
    admin: Arc<dyn SystemAdmin>,
    systemd: Arc<dyn SystemdAdmin>,
    global: Arc<BTreeSet<String>>,
    grants: HashMap<String, Option<BTreeSet<String>>>,
}

impl ServiceFactory for ServiceBuilder {
    type Service = GilfoyleServer;

    fn build(&self, principal: &Principal) -> GilfoyleServer {
        let grant = self.grants.get(&principal.key_id).and_then(Option::as_ref);
        let tools = policy::effective_tools(grant, &self.global);
        GilfoyleServer::new(
            Arc::clone(&self.admin),
            Arc::clone(&self.systemd),
            Some(tools),
        )
    }
}

/// Run the node until the process is stopped.
pub async fn run(config: Config) -> Result<(), String> {
    let admin: Arc<dyn SystemAdmin> = Arc::new(SystemService::new(ProcUptime::new()));

    let lazy = Arc::new(LazySystemd::system());
    let catalog: Arc<dyn ServiceCatalog> = lazy.clone();
    let control: Arc<dyn ServiceControl> = lazy;
    let systemd: Arc<dyn SystemdAdmin> = Arc::new(SystemdService::new(
        catalog,
        control,
        config.allowlist.clone(),
    ));

    let host_key = hostkey::load(&config.host_key).map_err(|error| error.to_string())?;

    let grants = config
        .keys
        .iter()
        .map(|key| (key.id.clone(), key.tools.clone()))
        .collect();

    let factory = ServiceBuilder {
        admin,
        systemd,
        global: Arc::new(config.enabled_tools.clone()),
        grants,
    };

    let limits = gilfoyle_ssh::Limits {
        max_connections: config.max_connections,
        max_channels_per_connection: config.max_channels_per_connection,
    };

    let server = SshServer::new(vec![host_key], factory, config.keys.clone(), limits);

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|error| format!("failed to bind {}: {error}", config.bind))?;
    tracing::info!(bind = %config.bind, "gilfoyle listening on ssh");

    server
        .run_on(&listener)
        .await
        .map_err(|error| error.to_string())
}
