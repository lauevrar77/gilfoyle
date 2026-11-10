//! Adapter behaviour against an in-process mock Docker HTTP server on a UNIX
//! socket. No daemon is needed.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gilfoyle_core::{
    ContainerCatalog, ContainerControl, ContainerFilter, ContainerRef, DockerError,
};
use gilfoyle_docker::{DockerAdapter, LazyDocker};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

/// A scripted reply from the mock server.
enum Reply {
    /// Answer with an HTTP status and body.
    Http(u16, String),
    /// Accept the request but never answer (to exercise timeouts).
    Hang,
}

type Responder = Arc<dyn Fn(&str, &str) -> Reply + Send + Sync>;

/// A running mock server, with the requests it saw.
struct Mock {
    path: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Mock {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

async fn spawn(responder: Responder) -> Mock {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gilfoyle-docker-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("docker.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();

    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let recorded = Arc::clone(&recorded);
            let responder = Arc::clone(&responder);
            tokio::spawn(async move {
                handle(stream, recorded, responder).await;
            });
        }
    });

    Mock { path, requests }
}

async fn handle(
    mut stream: tokio::net::UnixStream,
    recorded: Arc<Mutex<Vec<String>>>,
    responder: Responder,
) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let n = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    recorded.lock().unwrap().push(format!("{method} {target}"));

    match responder(&method, &target) {
        Reply::Hang => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        Reply::Http(status, body) => {
            let reason = match status {
                200 => "OK",
                204 => "No Content",
                304 => "Not Modified",
                400 => "Bad Request",
                403 => "Forbidden",
                404 => "Not Found",
                409 => "Conflict",
                _ => "Error",
            };
            let response = if status == 204 || status == 304 {
                format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n\r\n")
            } else {
                format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        }
    }
}

const VERSION: &str = "1.45";

const LISTING: &str = r#"[
    {"Id":"aaaaaaaaaaaa","Names":["/web"],"Image":"nginx:latest","State":"running","Status":"Up 3 hours"},
    {"Id":"bbbbbbbbbbbb","Names":["/db","/alias"],"Image":"postgres","State":"exited","Status":"Exited (0)"}
]"#;

const INSPECT: &str = r#"{
    "Id":"aaaaaaaaaaaa","Name":"/web","Created":"2024-05-01T12:00:00Z","RestartCount":3,
    "State":{"Status":"running","Pid":4242,"ExitCode":0,"StartedAt":"2024-05-01T12:00:01Z","FinishedAt":"0001-01-01T00:00:00Z","Health":{"Status":"healthy"}},
    "Config":{"Image":"nginx:latest"}
}"#;

/// A responder that answers the standard endpoints.
fn standard(action_status: u16) -> Responder {
    Arc::new(move |_method, target| {
        let path = target.split('?').next().unwrap_or(target);
        if path == "/version" {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else if path == format!("/v{VERSION}/containers/json") {
            Reply::Http(200, LISTING.to_string())
        } else if path == format!("/v{VERSION}/containers/web/json") {
            Reply::Http(200, INSPECT.to_string())
        } else {
            Reply::Http(action_status, String::new())
        }
    })
}

async fn adapter(responder: Responder) -> (Mock, DockerAdapter) {
    let mock = spawn(responder).await;
    let adapter = DockerAdapter::connect(
        mock.path.clone(),
        Duration::from_secs(2),
        Duration::from_secs(30),
        100,
    )
    .await
    .unwrap();
    (mock, adapter)
}

#[tokio::test]
async fn negotiates_the_version_and_lists_containers() {
    let (mock, adapter) = adapter(standard(204)).await;

    let all = adapter
        .list_containers(&ContainerFilter::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].names, vec!["web"]);

    let running = adapter
        .list_containers(&ContainerFilter {
            state: Some(gilfoyle_core::ContainerState::Running),
            ..ContainerFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(running.len(), 1);

    assert_eq!(
        mock.requests(),
        vec![
            "GET /version".to_string(),
            "GET /v1.45/containers/json?all=1".to_string(),
            "GET /v1.45/containers/json?all=1".to_string(),
        ]
    );
}

#[tokio::test]
async fn listing_is_capped_by_the_configured_limit() {
    let mock = spawn(standard(204)).await;
    let adapter = DockerAdapter::connect(
        mock.path.clone(),
        Duration::from_secs(2),
        Duration::from_secs(30),
        1,
    )
    .await
    .unwrap();

    // No limit requested: the configured cap applies.
    assert_eq!(
        adapter
            .list_containers(&ContainerFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
    // A larger limit is still capped.
    assert_eq!(
        adapter
            .list_containers(&ContainerFilter {
                limit: Some(5),
                ..ContainerFilter::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn reads_a_container_status() {
    let (mock, adapter) = adapter(standard(204)).await;
    let reference = ContainerRef::parse("web").unwrap();
    let status = adapter.container_status(&reference).await.unwrap();
    assert_eq!(status.name, "web");
    assert_eq!(status.health.as_deref(), Some("healthy"));
    assert_eq!(status.pid, Some(4242));
    assert_eq!(
        mock.requests(),
        vec![
            "GET /version".to_string(),
            "GET /v1.45/containers/web/json".to_string()
        ]
    );
}

#[tokio::test]
async fn resolves_and_acts_by_id_with_a_stop_timeout() {
    let (mock, adapter) = adapter(standard(204)).await;
    let reference = ContainerRef::parse("web").unwrap();
    let identity = adapter.resolve(&reference).await.unwrap();
    assert_eq!(identity.id, "aaaaaaaaaaaa");
    assert_eq!(identity.canonical_name(), "web");
    assert_eq!(identity.state, "running");

    adapter.start_container(&identity.id).await.unwrap();
    adapter.stop_container(&identity.id).await.unwrap();
    adapter.restart_container(&identity.id).await.unwrap();

    assert_eq!(
        mock.requests(),
        vec![
            "GET /version".to_string(),
            "GET /v1.45/containers/json?all=1".to_string(),
            "POST /v1.45/containers/aaaaaaaaaaaa/start".to_string(),
            "POST /v1.45/containers/aaaaaaaaaaaa/stop?t=30".to_string(),
            "POST /v1.45/containers/aaaaaaaaaaaa/restart?t=30".to_string(),
        ]
    );
}

#[tokio::test]
async fn a_not_running_container_is_a_conflict() {
    let responder: Responder = Arc::new(|_method, target| {
        if target.starts_with("/version") {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else {
            Reply::Http(409, r#"{"message":"is not running"}"#.to_string())
        }
    });
    let (_mock, adapter) = adapter(responder).await;
    assert_eq!(
        adapter.start_container("aaaaaaaaaaaa").await.unwrap_err(),
        DockerError::Conflict("aaaaaaaaaaaa".to_string(), "is not running".to_string())
    );
}

#[tokio::test]
async fn a_missing_container_is_not_found() {
    let responder: Responder = Arc::new(|_method, target| {
        if target.starts_with("/version") {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else if target.split('?').next().unwrap_or(target)
            == format!("/v{VERSION}/containers/json")
        {
            Reply::Http(200, LISTING.to_string())
        } else {
            Reply::Http(404, r#"{"message":"no such container"}"#.to_string())
        }
    });
    let (_mock, adapter) = adapter(responder).await;
    let reference = ContainerRef::parse("ghost").unwrap();
    assert_eq!(
        adapter.container_status(&reference).await.unwrap_err(),
        DockerError::NotFound("ghost".to_string())
    );
    assert_eq!(
        adapter.resolve(&reference).await.unwrap_err(),
        DockerError::NotFound("ghost".to_string())
    );
}

#[tokio::test]
async fn a_server_error_is_generic() {
    let responder: Responder = Arc::new(|_method, target| {
        if target.starts_with("/version") {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else {
            Reply::Http(500, r#"{"message":"boom"}"#.to_string())
        }
    });
    let (_mock, adapter) = adapter(responder).await;
    assert_eq!(
        adapter
            .list_containers(&ContainerFilter::default())
            .await
            .unwrap_err(),
        DockerError::Other {
            status: 500,
            message: "boom".to_string()
        }
    );
}

#[tokio::test]
async fn a_malformed_body_is_invalid() {
    let responder: Responder = Arc::new(|_method, target| {
        if target.starts_with("/version") {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else {
            Reply::Http(200, "not json".to_string())
        }
    });
    let (_mock, adapter) = adapter(responder).await;
    assert!(matches!(
        adapter
            .list_containers(&ContainerFilter::default())
            .await
            .unwrap_err(),
        DockerError::Invalid(_)
    ));
}

#[tokio::test]
async fn a_hanging_daemon_times_out() {
    let responder: Responder = Arc::new(|_method, _target| Reply::Hang);
    let mock = spawn(responder).await;
    let error = DockerAdapter::connect(
        mock.path.clone(),
        Duration::from_millis(200),
        Duration::from_secs(30),
        100,
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(error, DockerError::Timeout(_)));
}

#[tokio::test]
async fn a_hanging_listing_times_out() {
    let responder: Responder = Arc::new(|_method, target| {
        if target.starts_with("/version") {
            Reply::Http(200, format!(r#"{{"ApiVersion":"{VERSION}"}}"#))
        } else {
            Reply::Hang
        }
    });
    let (_mock, adapter) = adapter(responder).await;
    assert!(matches!(
        adapter
            .list_containers(&ContainerFilter::default())
            .await
            .unwrap_err(),
        DockerError::Timeout(_)
    ));
}

#[tokio::test]
async fn a_failed_version_probe_is_fatal() {
    let responder: Responder =
        Arc::new(|_method, _target| Reply::Http(500, r#"{"message":"down"}"#.to_string()));
    let mock = spawn(responder).await;
    assert_eq!(
        DockerAdapter::connect(
            mock.path.clone(),
            Duration::from_secs(2),
            Duration::from_secs(30),
            100
        )
        .await
        .err()
        .unwrap(),
        DockerError::Other {
            status: 500,
            message: "down".to_string()
        }
    );
}

#[tokio::test]
async fn a_missing_socket_is_unavailable() {
    let path = std::env::temp_dir().join("gilfoyle-docker-does-not-exist.sock");
    let error = DockerAdapter::connect(
        path,
        Duration::from_millis(200),
        Duration::from_secs(30),
        100,
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(error, DockerError::Unavailable(_)));
}

#[tokio::test]
async fn lazy_connects_on_first_use_and_retries_after_failure() {
    let mock = spawn(standard(204)).await;

    // A lazy adapter over the socket connects on the first call.
    let lazy = LazyDocker::socket(
        mock.path.clone(),
        Duration::from_secs(2),
        Duration::from_secs(30),
        100,
    );
    assert_eq!(
        lazy.list_containers(&ContainerFilter::default())
            .await
            .unwrap()
            .len(),
        2
    );

    // A connector that fails once is retried on the next call.
    let attempts = Arc::new(AtomicUsize::new(0));
    let path = mock.path.clone();
    let connector: gilfoyle_docker::Connector = Arc::new(move || {
        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
        let path = path.clone();
        Box::pin(async move {
            if attempt == 0 {
                Err(DockerError::Unavailable("down".to_string()))
            } else {
                DockerAdapter::connect(path, Duration::from_secs(2), Duration::from_secs(30), 100)
                    .await
            }
        })
    });
    let lazy = LazyDocker::new(connector);
    assert!(
        lazy.list_containers(&ContainerFilter::default())
            .await
            .is_err()
    );
    assert!(
        lazy.list_containers(&ContainerFilter::default())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn lazy_delegates_every_operation() {
    let mock = spawn(standard(204)).await;
    let lazy = LazyDocker::socket(
        mock.path.clone(),
        Duration::from_secs(2),
        Duration::from_secs(30),
        100,
    );

    let reference = ContainerRef::parse("web").unwrap();
    assert_eq!(lazy.container_status(&reference).await.unwrap().name, "web");
    assert_eq!(lazy.resolve(&reference).await.unwrap().id, "aaaaaaaaaaaa");
    lazy.start_container("aaaaaaaaaaaa").await.unwrap();
    lazy.stop_container("aaaaaaaaaaaa").await.unwrap();
    lazy.restart_container("aaaaaaaaaaaa").await.unwrap();

    // The actions must actually reach the daemon, not just return Ok.
    let requests = mock.requests();
    assert!(requests.contains(&"POST /v1.45/containers/aaaaaaaaaaaa/start".to_string()));
    assert!(requests.contains(&"POST /v1.45/containers/aaaaaaaaaaaa/stop?t=30".to_string()));
    assert!(requests.contains(&"POST /v1.45/containers/aaaaaaaaaaaa/restart?t=30".to_string()));
}
