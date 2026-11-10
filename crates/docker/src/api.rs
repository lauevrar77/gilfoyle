//! Pure Docker API request building and response parsing.
//!
//! Everything here is a pure function of bytes and values: no socket, no
//! runtime. The transport turns a [`RawRequest`] into a [`RawResponse`]; these
//! functions build the former, map the latter, and decode Docker's JSON into
//! the domain types.
//!
//! The request set is closed: list, inspect, start, stop, restart and the
//! version probe. There is deliberately no create and no remove.

use bytes::Bytes;
use gilfoyle_core::{Container, ContainerAction, ContainerIdentity, ContainerStatus, DockerError};
use http::Method;
use serde::Deserialize;

/// A Docker API request: an HTTP method and a path (with query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRequest {
    /// HTTP method.
    pub method: Method,
    /// Request path, including any query string.
    pub path: String,
}

/// A raw HTTP response: a status code and a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: Bytes,
}

/// The `GET /version` probe.
pub fn version_request() -> RawRequest {
    RawRequest {
        method: Method::GET,
        path: "/version".to_string(),
    }
}

/// `GET /v{version}/containers/json?all=1`.
pub fn list_request(version: &str) -> RawRequest {
    RawRequest {
        method: Method::GET,
        path: format!("/v{version}/containers/json?all=1"),
    }
}

/// `GET /v{version}/containers/{reference}/json`.
pub fn inspect_request(version: &str, reference: &str) -> RawRequest {
    RawRequest {
        method: Method::GET,
        path: format!("/v{version}/containers/{reference}/json"),
    }
}

/// `POST /v{version}/containers/{reference}/{action}`, with a `t=` timeout for
/// stop and restart.
pub fn action_request(
    version: &str,
    reference: &str,
    action: ContainerAction,
    timeout_seconds: Option<u64>,
) -> RawRequest {
    let mut path = format!("/v{version}/containers/{reference}/{}", action.as_str());
    if action != ContainerAction::Start
        && let Some(timeout) = timeout_seconds
    {
        path.push_str(&format!("?t={timeout}"));
    }
    RawRequest {
        method: Method::POST,
        path,
    }
}

/// Map an HTTP status to success or a typed error.
///
/// `200`, `204` and `304` are success: `204` is a completed action and `304`
/// means the container was already in the requested state.
pub fn map_response(response: RawResponse, reference: &str) -> Result<Bytes, DockerError> {
    match response.status {
        200 | 204 | 304 => Ok(response.body),
        400 => Err(DockerError::Invalid(message_from(&response.body))),
        401 | 403 => Err(DockerError::Forbidden(
            reference.to_string(),
            message_from(&response.body),
        )),
        404 => Err(DockerError::NotFound(reference.to_string())),
        409 => Err(DockerError::Conflict(
            reference.to_string(),
            message_from(&response.body),
        )),
        status => Err(DockerError::Other {
            status,
            message: message_from(&response.body),
        }),
    }
}

/// Extract Docker's `message` field from an error body, if present.
fn message_from(body: &[u8]) -> String {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        message: String,
    }
    serde_json::from_slice::<Body>(body)
        .map(|body| body.message)
        .unwrap_or_default()
}

/// The `ApiVersion` reported by `GET /version`.
#[derive(Deserialize)]
struct RawVersion {
    #[serde(rename = "ApiVersion", default)]
    api_version: String,
}

/// Parse the API version from a `GET /version` reply.
pub fn parse_version(body: &[u8]) -> Result<String, DockerError> {
    let raw: RawVersion =
        serde_json::from_slice(body).map_err(|error| DockerError::Invalid(error.to_string()))?;
    if raw.api_version.is_empty() {
        return Err(DockerError::Invalid(
            "the daemon reported no API version".to_string(),
        ));
    }
    Ok(raw.api_version)
}

/// A container summary as returned by `GET /containers/json`.
#[derive(Deserialize)]
struct RawContainer {
    #[serde(rename = "Id", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "State", default)]
    state: String,
    #[serde(rename = "Status", default)]
    status: String,
}

/// Parse a container listing.
pub fn parse_containers(body: &[u8]) -> Result<Vec<Container>, DockerError> {
    let raw: Vec<RawContainer> =
        serde_json::from_slice(body).map_err(|error| DockerError::Invalid(error.to_string()))?;
    Ok(raw
        .into_iter()
        .map(|container| Container {
            id: container.id,
            names: container
                .names
                .iter()
                .map(|name| canonical_name(name))
                .collect(),
            image: container.image,
            state: container.state,
            status: container.status,
        })
        .collect())
}

/// The `State` object of an inspect reply.
#[derive(Deserialize, Default)]
struct RawState {
    #[serde(rename = "Status", default)]
    status: String,
    #[serde(rename = "Pid", default)]
    pid: u64,
    #[serde(rename = "ExitCode", default)]
    exit_code: i32,
    #[serde(rename = "StartedAt", default)]
    started_at: String,
    #[serde(rename = "FinishedAt", default)]
    finished_at: String,
    #[serde(rename = "Health", default)]
    health: Option<RawHealth>,
}

/// The health object of an inspect reply.
#[derive(Deserialize, Default)]
struct RawHealth {
    #[serde(rename = "Status", default)]
    status: String,
}

/// The `Config` object of an inspect reply.
#[derive(Deserialize, Default)]
struct RawConfig {
    #[serde(rename = "Image", default)]
    image: String,
}

/// An inspect reply (`GET /containers/{id}/json`).
#[derive(Deserialize)]
struct RawInspect {
    #[serde(rename = "Id", default)]
    id: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Created", default)]
    created: String,
    #[serde(rename = "State", default)]
    state: RawState,
    #[serde(rename = "Config", default)]
    config: RawConfig,
    #[serde(rename = "RestartCount", default)]
    restart_count: u64,
}

/// Parse a container status.
pub fn parse_status(body: &[u8]) -> Result<ContainerStatus, DockerError> {
    let raw: RawInspect =
        serde_json::from_slice(body).map_err(|error| DockerError::Invalid(error.to_string()))?;
    let pid = if raw.state.pid == 0 {
        None
    } else {
        Some(u32::try_from(raw.state.pid).unwrap_or(u32::MAX))
    };
    Ok(ContainerStatus {
        id: raw.id,
        name: canonical_name(&raw.name),
        image: raw.config.image,
        state: raw.state.status.clone(),
        status: raw.state.status,
        health: raw
            .state
            .health
            .map(|health| health.status)
            .filter(|status| !status.is_empty()),
        pid,
        exit_code: raw.state.exit_code,
        restart_count: raw.restart_count,
        created: raw.created,
        started_at: normalize_timestamp(&raw.state.started_at),
        finished_at: normalize_timestamp(&raw.state.finished_at),
    })
}

/// Strip Docker's leading `/` from a container name.
fn canonical_name(name: &str) -> String {
    name.strip_prefix('/').unwrap_or(name).to_string()
}

/// Turn Docker's zero time (`0001-01-01T00:00:00Z`) or an empty string into
/// `None`, and pass a real RFC 3339 timestamp through.
fn normalize_timestamp(value: &str) -> Option<String> {
    if value.is_empty() || value.starts_with("0001-01-01T00:00:00") {
        None
    } else {
        Some(value.to_string())
    }
}

/// Find the container a reference points at.
///
/// An exact id wins, then an exact name, then a *unique* id prefix (Docker
/// accepts truncated ids). An ambiguous id prefix resolves to nothing.
pub fn find_container<'a>(containers: &'a [Container], reference: &str) -> Option<&'a Container> {
    if let Some(container) = containers
        .iter()
        .find(|container| container.id == reference)
    {
        return Some(container);
    }
    if let Some(container) = containers
        .iter()
        .find(|container| container.names.iter().any(|name| name == reference))
    {
        return Some(container);
    }
    let mut matches = containers
        .iter()
        .filter(|container| container.id.starts_with(reference));
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// The identity (id, names, state) of a listing entry.
pub fn identity_of(container: &Container) -> ContainerIdentity {
    ContainerIdentity {
        id: container.id.clone(),
        names: container.names.clone(),
        state: container.state.clone(),
    }
}
