//! Pure request/response mapping tests for the Docker API module.

use bytes::Bytes;
use gilfoyle_core::{Container, ContainerAction, DockerError};
use gilfoyle_docker::api::{
    RawRequest, RawResponse, action_request, find_container, identity_of, inspect_request,
    list_request, map_response, parse_containers, parse_status, parse_version, version_request,
};
use http::Method;

fn response(status: u16, body: &str) -> RawResponse {
    RawResponse {
        status,
        body: Bytes::from(body.to_string()),
    }
}

// --- request building ---

#[test]
fn builds_the_version_and_list_requests() {
    assert_eq!(
        version_request(),
        RawRequest {
            method: Method::GET,
            path: "/version".to_string()
        }
    );
    assert_eq!(
        list_request("1.45"),
        RawRequest {
            method: Method::GET,
            path: "/v1.45/containers/json?all=1".to_string()
        }
    );
}

#[test]
fn builds_inspect_and_action_requests() {
    assert_eq!(
        inspect_request("1.45", "web"),
        RawRequest {
            method: Method::GET,
            path: "/v1.45/containers/web/json".to_string()
        }
    );

    // start carries no timeout.
    assert_eq!(
        action_request("1.45", "abc", ContainerAction::Start, Some(30)),
        RawRequest {
            method: Method::POST,
            path: "/v1.45/containers/abc/start".to_string()
        }
    );

    // stop and restart carry `t=` when a timeout is set.
    assert_eq!(
        action_request("1.45", "abc", ContainerAction::Stop, Some(30)).path,
        "/v1.45/containers/abc/stop?t=30"
    );
    assert_eq!(
        action_request("1.45", "abc", ContainerAction::Restart, Some(7)).path,
        "/v1.45/containers/abc/restart?t=7"
    );
    assert_eq!(
        action_request("1.45", "abc", ContainerAction::Stop, None).path,
        "/v1.45/containers/abc/stop"
    );
}

// --- response mapping ---

#[test]
fn maps_success_statuses() {
    for status in [200, 204, 304] {
        assert_eq!(
            map_response(response(status, "{}"), "web").unwrap(),
            Bytes::from("{}".to_string())
        );
    }
}

#[test]
fn maps_error_statuses_to_typed_errors() {
    assert_eq!(
        map_response(response(400, r#"{"message":"bad parameter"}"#), "web").unwrap_err(),
        DockerError::Invalid("bad parameter".to_string())
    );
    assert_eq!(
        map_response(response(401, r#"{"message":"denied"}"#), "web").unwrap_err(),
        DockerError::Forbidden("web".to_string(), "denied".to_string())
    );
    assert_eq!(
        map_response(response(403, r#"{"message":"denied"}"#), "web").unwrap_err(),
        DockerError::Forbidden("web".to_string(), "denied".to_string())
    );
    assert_eq!(
        map_response(response(404, "no such container"), "web").unwrap_err(),
        DockerError::NotFound("web".to_string())
    );
    assert_eq!(
        map_response(response(409, r#"{"message":"not running"}"#), "web").unwrap_err(),
        DockerError::Conflict("web".to_string(), "not running".to_string())
    );
    assert_eq!(
        map_response(response(500, r#"{"message":"boom"}"#), "web").unwrap_err(),
        DockerError::Other {
            status: 500,
            message: "boom".to_string()
        }
    );
    // A non-JSON error body yields an empty message and stays generic.
    assert_eq!(
        map_response(response(502, "<html>"), "web").unwrap_err(),
        DockerError::Other {
            status: 502,
            message: String::new()
        }
    );
}

// --- parsing ---

#[test]
fn parses_the_api_version() {
    assert_eq!(
        parse_version(br#"{"ApiVersion":"1.45","MinAPIVersion":"1.24"}"#).unwrap(),
        "1.45"
    );
    assert!(matches!(
        parse_version(br#"{"ApiVersion":""}"#).unwrap_err(),
        DockerError::Invalid(_)
    ));
    assert!(matches!(
        parse_version(b"not json").unwrap_err(),
        DockerError::Invalid(_)
    ));
}

#[test]
fn parses_a_container_listing_and_strips_the_slash() {
    let body = br#"[
        {"Id":"abc","Names":["/web"],"Image":"nginx:latest","State":"running","Status":"Up 3 hours"},
        {"Id":"def","Names":["/db","/alias"],"Image":"postgres","State":"exited","Status":"Exited (0)"}
    ]"#;
    let containers = parse_containers(body).unwrap();
    assert_eq!(containers.len(), 2);
    assert_eq!(containers[0].names, vec!["web"]);
    assert_eq!(containers[1].names, vec!["db", "alias"]);
    assert_eq!(containers[1].state, "exited");

    assert!(matches!(
        parse_containers(b"{}").unwrap_err(),
        DockerError::Invalid(_)
    ));
}

#[test]
fn parses_missing_listing_fields_as_defaults() {
    let containers = parse_containers(br#"[{"Id":"abc"}]"#).unwrap();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].id, "abc");
    assert!(containers[0].names.is_empty());
    assert_eq!(containers[0].image, "");
}

fn inspect_body() -> &'static [u8] {
    br#"{
        "Id":"abc",
        "Name":"/web",
        "Created":"2024-05-01T12:00:00.123456789Z",
        "Image":"sha256:deadbeef",
        "RestartCount":3,
        "State":{
            "Status":"running",
            "Pid":4242,
            "ExitCode":0,
            "StartedAt":"2024-05-01T12:00:01Z",
            "FinishedAt":"0001-01-01T00:00:00Z",
            "Health":{"Status":"healthy"}
        },
        "Config":{"Image":"nginx:latest"}
    }"#
}

#[test]
fn parses_a_container_status() {
    let status = parse_status(inspect_body()).unwrap();
    assert_eq!(status.id, "abc");
    assert_eq!(status.name, "web");
    assert_eq!(status.image, "nginx:latest");
    assert_eq!(status.state, "running");
    assert_eq!(status.status, "running");
    assert_eq!(status.health.as_deref(), Some("healthy"));
    assert_eq!(status.pid, Some(4242));
    assert_eq!(status.exit_code, 0);
    assert_eq!(status.restart_count, 3);
    assert_eq!(status.created, "2024-05-01T12:00:00.123456789Z");
    assert_eq!(status.started_at.as_deref(), Some("2024-05-01T12:00:01Z"));
    assert_eq!(status.finished_at, None);
}

#[test]
fn parses_a_stopped_status_without_pid_or_health() {
    let body = br#"{
        "Id":"abc","Name":"/db","Created":"2024-05-01T12:00:00Z",
        "RestartCount":1,
        "State":{"Status":"exited","Pid":0,"ExitCode":137,"StartedAt":"","FinishedAt":"2024-05-01T13:00:00Z"},
        "Config":{"Image":"postgres"}
    }"#;
    let status = parse_status(body).unwrap();
    assert_eq!(status.pid, None);
    assert_eq!(status.exit_code, 137);
    assert_eq!(status.health, None);
    assert_eq!(status.started_at, None);
    assert_eq!(status.finished_at.as_deref(), Some("2024-05-01T13:00:00Z"));
}

#[test]
fn treats_an_empty_health_status_as_absent() {
    let body = br#"{
        "Id":"abc","Name":"/web","Created":"","RestartCount":0,
        "State":{"Status":"running","Pid":1,"ExitCode":0,"StartedAt":"","FinishedAt":"","Health":{"Status":""}},
        "Config":{"Image":"nginx"}
    }"#;
    assert_eq!(parse_status(body).unwrap().health, None);
}

#[test]
fn a_pid_beyond_u32_saturates() {
    let body = br#"{
        "Id":"abc","Name":"/web","Created":"","RestartCount":0,
        "State":{"Status":"running","Pid":4294967296,"ExitCode":0,"StartedAt":"","FinishedAt":""},
        "Config":{"Image":"nginx"}
    }"#;
    assert_eq!(parse_status(body).unwrap().pid, Some(u32::MAX));
}

// --- reference resolution ---

fn listing() -> Vec<Container> {
    parse_containers(
        br#"[
            {"Id":"aaaaaaaaaaaa","Names":["/web"],"Image":"nginx","State":"running"},
            {"Id":"bbbbbbbbbbbb","Names":["/db","/alias"],"Image":"postgres","State":"exited"}
        ]"#,
    )
    .unwrap()
}

#[test]
fn resolves_by_exact_id_then_name_then_prefix() {
    let containers = listing();
    assert_eq!(
        find_container(&containers, "aaaaaaaaaaaa").unwrap().id,
        "aaaaaaaaaaaa"
    );
    assert_eq!(
        find_container(&containers, "web").unwrap().id,
        "aaaaaaaaaaaa"
    );
    assert_eq!(
        find_container(&containers, "alias").unwrap().id,
        "bbbbbbbbbbbb"
    );
    assert_eq!(
        find_container(&containers, "aaaa").unwrap().id,
        "aaaaaaaaaaaa"
    );
    assert!(find_container(&containers, "ghost").is_none());
}

#[test]
fn ambiguous_id_prefix_resolves_to_nothing() {
    let containers = parse_containers(
        br#"[
            {"Id":"aaaa1","Names":["/a"],"Image":"x","State":"running"},
            {"Id":"aaaa2","Names":["/b"],"Image":"x","State":"running"}
        ]"#,
    )
    .unwrap();
    assert!(find_container(&containers, "aaaa").is_none());
    assert_eq!(find_container(&containers, "aaaa1").unwrap().id, "aaaa1");
}

#[test]
fn identity_of_copies_the_listing_entry() {
    let containers = listing();
    let identity = identity_of(&containers[1]);
    assert_eq!(identity.id, "bbbbbbbbbbbb");
    assert_eq!(identity.names, vec!["db", "alias"]);
    assert_eq!(identity.state, "exited");
}
