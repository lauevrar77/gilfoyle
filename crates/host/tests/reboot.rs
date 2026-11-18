//! Adapter tests for the reboot probe, against injected fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use gilfoyle_core::{RebootError, RebootOrigin, RebootSource};
use gilfoyle_host::{
    DEFAULT_OS_RELEASE_PATH, DEFAULT_REBOOT_REQUIRED_PATH, DEFAULT_REBOOT_REQUIRED_PKGS_PATH,
    RebootProbe, parse_os_release, parse_package_list,
};

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path
}

/// Build a probe over a fixture directory. A `None` marker or package list is
/// left absent.
fn probe(
    os_release: &str,
    marker: Option<&str>,
    packages: Option<&str>,
) -> (tempfile::TempDir, RebootProbe) {
    let dir = tempfile::tempdir().unwrap();
    let os = write(dir.path(), "os-release", os_release);
    let marker_path = match marker {
        Some(contents) => write(dir.path(), "reboot-required", contents),
        None => dir.path().join("reboot-required"),
    };
    let pkgs_path = match packages {
        Some(contents) => write(dir.path(), "reboot-required.pkgs", contents),
        None => dir.path().join("reboot-required.pkgs"),
    };
    let probe = RebootProbe::with_paths(os, marker_path, pkgs_path);
    (dir, probe)
}

#[test]
fn default_reads_the_real_paths() {
    let probe = RebootProbe::new();
    assert_eq!(
        probe.os_release_path().to_string_lossy(),
        DEFAULT_OS_RELEASE_PATH
    );
    assert_eq!(DEFAULT_REBOOT_REQUIRED_PATH, "/run/reboot-required");
    assert_eq!(
        DEFAULT_REBOOT_REQUIRED_PKGS_PATH,
        "/run/reboot-required.pkgs"
    );
}

#[test]
fn a_missing_os_release_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let probe = RebootProbe::with_paths(
        dir.path().join("absent"),
        dir.path().join("reboot-required"),
        dir.path().join("reboot-required.pkgs"),
    );
    let status = probe.reboot_status().unwrap();
    assert_eq!(status.required, None);
    assert_eq!(status.origin, RebootOrigin::Unsupported);
}

#[test]
fn a_non_debian_host_is_unsupported() {
    let (_dir, probe) = probe("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n", None, None);
    let status = probe.reboot_status().unwrap();
    assert_eq!(status.required, None);
    assert_eq!(status.origin, RebootOrigin::Unsupported);
}

#[test]
fn debian_without_the_marker_is_not_required() {
    let (_dir, probe) = probe("ID=debian\n", None, None);
    let status = probe.reboot_status().unwrap();
    assert_eq!(status.required, Some(false));
    assert!(status.packages.is_empty());
    assert_eq!(status.origin, RebootOrigin::Debian);
}

#[test]
fn debian_like_via_id_like_with_a_marker_is_required() {
    let (_dir, probe) = probe(
        "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
        Some("*** System restart required ***\n"),
        Some("libc6\nlinux-image-amd64\n\n"),
    );
    let status = probe.reboot_status().unwrap();
    assert_eq!(status.required, Some(true));
    assert_eq!(
        status.packages,
        vec!["libc6".to_string(), "linux-image-amd64".to_string()]
    );
    assert_eq!(status.origin, RebootOrigin::Debian);
}

#[test]
fn a_marker_without_a_package_list_is_still_required() {
    let (_dir, probe) = probe("ID=ubuntu\n", Some("reboot needed\n"), None);
    let status = probe.reboot_status().unwrap();
    assert_eq!(status.required, Some(true));
    assert!(status.packages.is_empty());
}

#[test]
fn an_unreadable_os_release_is_an_error_not_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    // A directory is always present but cannot be read as a file.
    let probe = RebootProbe::with_paths(
        dir.path(),
        dir.path().join("reboot-required"),
        dir.path().join("reboot-required.pkgs"),
    );
    assert!(matches!(
        probe.reboot_status().unwrap_err(),
        RebootError::Io { .. }
    ));
}

#[test]
fn parse_os_release_reads_quoted_and_bare_values() {
    let release = parse_os_release(
        "# a comment\n\nNAME=\"Debian GNU/Linux\"\nID=debian\nID_LIKE='ubuntu debian'\nNOTE=a=b\n",
    );
    assert_eq!(release.id.as_deref(), Some("debian"));
    assert_eq!(release.id_like.as_deref(), Some("ubuntu debian"));
    assert!(release.is_debian_like());
}

#[test]
fn parse_os_release_tolerates_an_empty_file() {
    let release = parse_os_release("");
    assert_eq!(release.id, None);
    assert_eq!(release.id_like, None);
    assert!(!release.is_debian_like());
}

#[test]
fn parse_os_release_keeps_unbalanced_quotes() {
    // A value with only one quote is kept verbatim: both quotes must match.
    assert_eq!(
        parse_os_release("ID=\"debian\n").id.as_deref(),
        Some("\"debian")
    );
    assert_eq!(
        parse_os_release("ID='debian\n").id.as_deref(),
        Some("'debian")
    );
}

#[test]
fn is_debian_like_matches_id_and_id_like() {
    assert!(parse_os_release("ID=ubuntu\n").is_debian_like());
    assert!(parse_os_release("ID_LIKE=\"debian\"\n").is_debian_like());
    assert!(!parse_os_release("ID=alpine\n").is_debian_like());
    assert!(!parse_os_release("ID_LIKE=\"rhel\"\n").is_debian_like());
}

#[test]
fn parse_package_list_trims_and_drops_blanks() {
    assert_eq!(
        parse_package_list("  libc6 \n\nlinux-image-amd64\n  \n"),
        vec!["libc6".to_string(), "linux-image-amd64".to_string()]
    );
}

#[test]
fn parse_package_list_is_capped() {
    let text: String = (0..150).map(|i| format!("pkg{i}\n")).collect();
    let packages = parse_package_list(&text);
    assert_eq!(packages.len(), 100);
    assert_eq!(packages.first().map(String::as_str), Some("pkg0"));
    assert_eq!(packages.last().map(String::as_str), Some("pkg99"));
}
