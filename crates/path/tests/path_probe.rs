use std::io::Write;
use std::time::{Duration, SystemTime};

use gilfoyle_core::{PathError, PathKind, PathRef, PathSource};
use gilfoyle_path::{PathProbe, present};

fn stat(path: &str) -> gilfoyle_core::PathStatus {
    let reference = PathRef::parse(path).unwrap();
    PathProbe.stat(&reference).unwrap()
}

#[test]
fn a_missing_path_does_not_exist() {
    let status = stat("/nonexistent/gilfoyle/marker");
    assert!(!status.exists);
    assert_eq!(status.path, "/nonexistent/gilfoyle/marker");
    assert_eq!(status.kind, None);
    assert_eq!(status.size_bytes, None);
    assert_eq!(status.mode, None);
}

#[test]
fn a_regular_file_reports_full_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"hello").unwrap();

    let status = stat(path.to_str().unwrap());
    assert!(status.exists);
    assert_eq!(status.kind, Some(PathKind::File));
    assert_eq!(status.size_bytes, Some(5));
    assert!(status.mode.is_some());
    assert!(status.uid.is_some());
    assert!(status.gid.is_some());
    assert!(status.modified.is_some());
    assert!(status.accessed.is_some());
    assert!(status.changed.is_some());
    assert!(status.age_seconds.is_some());
    assert_eq!(status.symlink_target, None);
}

#[test]
fn a_directory_reports_the_directory_kind() {
    let dir = tempfile::tempdir().unwrap();
    let status = stat(dir.path().to_str().unwrap());
    assert_eq!(status.kind, Some(PathKind::Dir));
    assert_eq!(status.symlink_target, None);
}

#[test]
fn a_symbolic_link_is_reported_without_being_followed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::write(&target, b"contents").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let status = stat(link.to_str().unwrap());
    assert!(status.exists);
    assert_eq!(status.kind, Some(PathKind::Symlink));
    assert_eq!(status.symlink_target.as_deref(), target.to_str());
}

#[test]
fn reports_permission_bits_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker");
    std::fs::write(&path, b"x").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let status = stat(path.to_str().unwrap());
    assert_eq!(status.mode, Some(0o640));
}

#[test]
fn a_non_regular_object_is_other() {
    let status = stat("/dev/null");
    assert_eq!(status.kind, Some(PathKind::Other));
    assert_eq!(status.symlink_target, None);
}

#[test]
fn an_error_other_than_absence_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    let child = format!("{}/child", file.to_str().unwrap());

    match PathProbe.stat(&PathRef::parse(&child).unwrap()) {
        Err(PathError::Failed { path, .. }) => assert_eq!(path, child),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn present_computes_the_age_from_the_modification_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker");
    std::fs::write(&path, b"x").unwrap();
    let metadata = std::fs::symlink_metadata(&path).unwrap();

    let now = metadata.modified().unwrap() + Duration::from_secs(90);
    let status = present(path.to_str().unwrap(), &metadata, now);
    assert_eq!(status.age_seconds, Some(90));
}

#[test]
fn present_reports_no_age_when_the_time_is_in_the_future() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker");
    std::fs::write(&path, b"x").unwrap();
    let metadata = std::fs::symlink_metadata(&path).unwrap();

    let now = SystemTime::UNIX_EPOCH;
    let status = present(path.to_str().unwrap(), &metadata, now);
    assert_eq!(status.age_seconds, None);
    assert!(status.modified.is_some());
}
