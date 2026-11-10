//! Lazy Docker connection.
//!
//! Gilfoyle must start even when Docker is absent (a minimal host, a container),
//! so the socket is connected and the API version negotiated on first use rather
//! than at startup. A failed connection is not cached, so a later call retries.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gilfoyle_core::{
    Container, ContainerCatalog, ContainerControl, ContainerFilter, ContainerIdentity,
    ContainerRef, ContainerStatus, DockerError,
};
use tokio::sync::OnceCell;

use crate::adapter::DockerAdapter;

/// A future produced by a connector.
pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<DockerAdapter, DockerError>> + Send>>;

/// Produces a Docker adapter on demand.
pub type Connector = Arc<dyn Fn() -> ConnectFuture + Send + Sync>;

/// Connects to Docker on first use and caches the adapter.
pub struct LazyDocker {
    connector: Connector,
    adapter: OnceCell<DockerAdapter>,
}

impl LazyDocker {
    /// Build a lazy adapter with an explicit connector (used by tests).
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            adapter: OnceCell::new(),
        }
    }

    /// Build a lazy adapter that connects to a socket path.
    pub fn socket(
        endpoint: PathBuf,
        call_timeout: Duration,
        stop_timeout: Duration,
        default_limit: usize,
    ) -> Self {
        Self::new(Arc::new(move || {
            let endpoint = endpoint.clone();
            Box::pin(async move {
                DockerAdapter::connect(endpoint, call_timeout, stop_timeout, default_limit).await
            })
        }))
    }

    async fn adapter(&self) -> Result<&DockerAdapter, DockerError> {
        self.adapter.get_or_try_init(|| (self.connector)()).await
    }
}

#[async_trait]
impl ContainerCatalog for LazyDocker {
    async fn list_containers(
        &self,
        filter: &ContainerFilter,
    ) -> Result<Vec<Container>, DockerError> {
        self.adapter().await?.list_containers(filter).await
    }

    async fn container_status(
        &self,
        reference: &ContainerRef,
    ) -> Result<ContainerStatus, DockerError> {
        self.adapter().await?.container_status(reference).await
    }

    async fn resolve(&self, reference: &ContainerRef) -> Result<ContainerIdentity, DockerError> {
        self.adapter().await?.resolve(reference).await
    }
}

#[async_trait]
impl ContainerControl for LazyDocker {
    async fn start_container(&self, id: &str) -> Result<(), DockerError> {
        self.adapter().await?.start_container(id).await
    }

    async fn stop_container(&self, id: &str) -> Result<(), DockerError> {
        self.adapter().await?.stop_container(id).await
    }

    async fn restart_container(&self, id: &str) -> Result<(), DockerError> {
        self.adapter().await?.restart_container(id).await
    }
}
