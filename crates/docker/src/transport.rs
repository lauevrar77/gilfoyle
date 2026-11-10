//! HTTP/1.1 transport over the Docker UNIX socket.
//!
//! This is the thin I/O glue: a `tower` connector that opens every request on
//! the socket path, and a helper that runs one request under a timeout. The
//! request building and response parsing live in [`crate::api`] and are pure.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use gilfoyle_core::DockerError;
use http::Uri;
use http_body_util::{BodyExt, Empty};
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::UnixStream;
use tower_service::Service;

use crate::api::{RawRequest, RawResponse};

/// A connection to the Docker daemon, as hyper needs to see it.
///
/// `TokioIo` adapts the tokio stream to hyper's I/O traits but does not
/// implement the `Connection` marker, so it is wrapped here.
struct UnixIo(TokioIo<UnixStream>);

impl Read for UnixIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Read::poll_read(Pin::new(&mut self.get_mut().0), cx, buf)
    }
}

impl Write for UnixIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Write::poll_write(Pin::new(&mut self.get_mut().0), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Write::poll_flush(Pin::new(&mut self.get_mut().0), cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Write::poll_shutdown(Pin::new(&mut self.get_mut().0), cx)
    }
}

impl Connection for UnixIo {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

/// A connector that opens every request on the Docker UNIX socket.
#[derive(Debug, Clone)]
struct UnixConnector {
    path: Arc<PathBuf>,
}

impl Service<Uri> for UnixConnector {
    type Response = UnixIo;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Uri) -> Self::Future {
        let path = Arc::clone(&self.path);
        Box::pin(async move {
            let stream = UnixStream::connect(&*path).await?;
            Ok(UnixIo(TokioIo::new(stream)))
        })
    }
}

/// Sends [`RawRequest`]s to the Docker daemon over its UNIX socket.
pub struct DockerTransport {
    client: Client<UnixConnector, Empty<Bytes>>,
    call_timeout: Duration,
}

impl DockerTransport {
    /// Build a transport for a socket path, with a per-call timeout.
    pub fn new(endpoint: impl Into<PathBuf>, call_timeout: Duration) -> Self {
        let endpoint = endpoint.into();
        let connector = UnixConnector {
            path: Arc::new(endpoint),
        };
        let client = Client::builder(TokioExecutor::new()).build(connector);
        Self {
            client,
            call_timeout,
        }
    }

    /// Send one request, returning its status and body.
    pub async fn send(&self, request: RawRequest) -> Result<RawResponse, DockerError> {
        let uri = format!("http://localhost{}", request.path)
            .parse::<Uri>()
            .map_err(|error| DockerError::Unavailable(error.to_string()))?;
        let http_request = http::Request::builder()
            .method(request.method)
            .uri(uri)
            .body(Empty::<Bytes>::new())
            .map_err(|error| DockerError::Unavailable(error.to_string()))?;

        let response = match tokio::time::timeout(
            self.call_timeout,
            self.client.request(http_request),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(DockerError::Unavailable(error.to_string())),
            Err(_) => return Err(DockerError::Timeout(request.path)),
        };

        let status = response.status().as_u16();
        let body =
            match tokio::time::timeout(self.call_timeout, response.into_body().collect()).await {
                Ok(Ok(collected)) => collected.to_bytes(),
                Ok(Err(error)) => return Err(DockerError::Unavailable(error.to_string())),
                Err(_) => return Err(DockerError::Timeout(request.path)),
            };
        Ok(RawResponse { status, body })
    }
}
