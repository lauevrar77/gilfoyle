//! Composition: build the driven adapters, the per-key service factory, and run the
//! embedded SSH server.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use gilfoyle_core::{
    ContainerCatalog, ContainerControl, DockerAdmin, DockerService, PackageAdmin, PackageManager,
    PackageService, ServiceCatalog, ServiceControl, SystemAdmin, SystemService, SystemdAdmin,
    SystemdService,
};
use gilfoyle_docker::LazyDocker;
use gilfoyle_packagekit::{
    DEFAULT_CALL_TIMEOUT, DEFAULT_LIMIT, DEFAULT_POLL_TIMEOUT, LazyPackagekit,
};
use gilfoyle_ssh::{Principal, ServiceFactory, SshServer, hostkey, policy};
use gilfoyle_system::ProcUptime;
use gilfoyle_systemd::LazySystemd;

use crate::config::{Config, DEFAULT_DOCKER_LIMIT};
use crate::mcp::GilfoyleServer;

/// Builds a [`GilfoyleServer`] per authenticated key, with that key's tool grant.
pub struct ServiceBuilder {
    admin: Arc<dyn SystemAdmin>,
    systemd: Arc<dyn SystemdAdmin>,
    packages: Arc<dyn PackageAdmin>,
    docker: Arc<dyn DockerAdmin>,
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
            Arc::clone(&self.packages),
            Arc::clone(&self.docker),
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

    let poll_timeout = Duration::from_secs(
        config
            .package_poll_timeout_seconds
            .unwrap_or(DEFAULT_POLL_TIMEOUT.as_secs()),
    );
    let limit = config.package_default_limit.unwrap_or(DEFAULT_LIMIT);
    let manager: Arc<dyn PackageManager> = Arc::new(LazyPackagekit::system_with(
        DEFAULT_CALL_TIMEOUT,
        poll_timeout,
        limit,
    ));
    let packages: Arc<dyn PackageAdmin> = Arc::new(PackageService::new(
        manager,
        config.package_allowlist.clone(),
        config.allow_transitive_changes,
    ));

    let docker_call_timeout = Duration::from_secs(
        config
            .docker_call_timeout_seconds
            .unwrap_or(gilfoyle_docker::DEFAULT_CALL_TIMEOUT.as_secs()),
    );
    let docker_stop_timeout = Duration::from_secs(
        config
            .docker_stop_timeout_seconds
            .unwrap_or(gilfoyle_docker::DEFAULT_STOP_TIMEOUT.as_secs()),
    );
    let docker_limit = config.docker_default_limit.unwrap_or(DEFAULT_DOCKER_LIMIT);
    let lazy_docker = Arc::new(LazyDocker::socket(
        config.docker_endpoint.clone(),
        docker_call_timeout,
        docker_stop_timeout,
        docker_limit,
    ));
    let container_catalog: Arc<dyn ContainerCatalog> = lazy_docker.clone();
    let container_control: Arc<dyn ContainerControl> = lazy_docker;
    let docker: Arc<dyn DockerAdmin> = Arc::new(DockerService::new(
        container_catalog,
        container_control,
        config.container_allowlist.clone(),
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
        packages,
        docker,
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
