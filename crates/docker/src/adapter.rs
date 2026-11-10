//! The Docker driven adapter, speaking HTTP over the daemon's UNIX socket.
//!
//! It never spawns the `docker` CLI: requests are compiled functions against a
//! closed operation set (list, inspect, start, stop, restart). The API version
//! is negotiated once, at connect time, and pinned into every path.

use std::time::Duration;

use async_trait::async_trait;
use gilfoyle_core::{
    Container, ContainerAction, ContainerCatalog, ContainerControl, ContainerFilter,
    ContainerIdentity, ContainerRef, ContainerStatus, DockerError,
};

use crate::api::{
    action_request, find_container, identity_of, inspect_request, list_request, map_response,
    parse_containers, parse_status, parse_version, version_request,
};
use crate::transport::DockerTransport;

/// Default timeout for a single API call.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Default timeout passed to Docker's `stop` and `restart` (`t=`).
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Docker driven adapter. Holds a live socket transport; spawns nothing.
pub struct DockerAdapter {
    transport: DockerTransport,
    version: String,
    stop_timeout: Duration,
    default_limit: usize,
}

impl DockerAdapter {
    /// Connect to the daemon and negotiate the API version.
    ///
    /// A failed version probe is fatal (fail-closed): gilfoyle does not guess a
    /// version. `default_limit` caps how many containers a listing returns.
    pub async fn connect(
        endpoint: impl Into<std::path::PathBuf>,
        call_timeout: Duration,
        stop_timeout: Duration,
        default_limit: usize,
    ) -> Result<Self, DockerError> {
        let transport = DockerTransport::new(endpoint, call_timeout);
        let response = transport.send(version_request()).await?;
        let body = map_response(response, "docker daemon")?;
        let version = parse_version(&body)?;
        Ok(Self {
            transport,
            version,
            stop_timeout,
            default_limit,
        })
    }

    /// Fetch and parse the full container listing.
    async fn list_all(&self) -> Result<Vec<Container>, DockerError> {
        let response = self.transport.send(list_request(&self.version)).await?;
        let body = map_response(response, "containers")?;
        parse_containers(&body)
    }

    /// Perform one lifecycle action by id.
    async fn action(&self, id: &str, action: ContainerAction) -> Result<(), DockerError> {
        let timeout = (action != ContainerAction::Start).then_some(self.stop_timeout.as_secs());
        let response = self
            .transport
            .send(action_request(&self.version, id, action, timeout))
            .await?;
        map_response(response, id)?;
        Ok(())
    }
}

#[async_trait]
impl ContainerCatalog for DockerAdapter {
    async fn list_containers(
        &self,
        filter: &ContainerFilter,
    ) -> Result<Vec<Container>, DockerError> {
        let mut effective = filter.clone();
        effective.limit = Some(
            filter
                .limit
                .unwrap_or(self.default_limit)
                .min(self.default_limit),
        );
        Ok(effective.apply(self.list_all().await?))
    }

    async fn container_status(
        &self,
        reference: &ContainerRef,
    ) -> Result<ContainerStatus, DockerError> {
        let response = self
            .transport
            .send(inspect_request(&self.version, reference.as_str()))
            .await?;
        let body = map_response(response, reference.as_str())?;
        parse_status(&body)
    }

    async fn resolve(&self, reference: &ContainerRef) -> Result<ContainerIdentity, DockerError> {
        let containers = self.list_all().await?;
        let container = find_container(&containers, reference.as_str())
            .ok_or_else(|| DockerError::NotFound(reference.as_str().to_string()))?;
        Ok(identity_of(container))
    }
}

#[async_trait]
impl ContainerControl for DockerAdapter {
    async fn start_container(&self, id: &str) -> Result<(), DockerError> {
        self.action(id, ContainerAction::Start).await
    }

    async fn stop_container(&self, id: &str) -> Result<(), DockerError> {
        self.action(id, ContainerAction::Stop).await
    }

    async fn restart_container(&self, id: &str) -> Result<(), DockerError> {
        self.action(id, ContainerAction::Restart).await
    }
}
