use gilfoyle_core::{AppError, PathAllowlist, PathRef};

#[test]
fn path_ref_accepts_absolute_paths() {
    assert_eq!(PathRef::parse("/").unwrap().as_str(), "/");
    assert_eq!(
        PathRef::parse("  /var/lib/app ").unwrap().as_str(),
        "/var/lib/app"
    );
}

#[test]
fn path_ref_rejects_invalid_paths() {
    assert!(PathRef::parse("").is_err());
    assert!(PathRef::parse("   ").is_err());
    assert!(PathRef::parse("relative/path").is_err());
    assert!(PathRef::parse("/var/../etc").is_err());
    assert!(PathRef::parse("/..").is_err());
}

#[test]
fn allowlist_matches_exact_paths() {
    let allowlist = PathAllowlist::parse(vec!["/var/lib/app/last-run".to_string()]).unwrap();
    assert!(!allowlist.is_empty());
    assert!(allowlist.contains(&PathRef::parse("/var/lib/app/last-run").unwrap()));
    assert!(!allowlist.contains(&PathRef::parse("/var/lib/app/other").unwrap()));
    let exact: Vec<&str> = allowlist.exact_paths().collect();
    assert_eq!(exact, vec!["/var/lib/app/last-run"]);
}

#[test]
fn allowlist_matches_anchored_patterns() {
    let allowlist =
        PathAllowlist::parse(vec!["re:^/var/backups/[^/]+\\.dump$".to_string()]).unwrap();
    assert!(allowlist.contains(&PathRef::parse("/var/backups/nightly.dump").unwrap()));
    assert!(!allowlist.contains(&PathRef::parse("/var/backups/nightly.dump.bak").unwrap()));
    assert!(!allowlist.contains(&PathRef::parse("/etc/passwd").unwrap()));
}

#[test]
fn empty_allowlist_allows_nothing() {
    let allowlist = PathAllowlist::default();
    assert!(allowlist.is_empty());
    assert!(!allowlist.contains(&PathRef::parse("/var/lib/app/last-run").unwrap()));
}

#[test]
fn allowlist_rejects_invalid_entries() {
    assert!(PathAllowlist::parse(vec!["relative".to_string()]).is_err());
    assert!(PathAllowlist::parse(vec!["re:[".to_string()]).is_err());
    assert!(PathAllowlist::parse(vec!["re:".to_string()]).is_err());
}

#[test]
fn ensure_allowed_reports_the_refused_path() {
    let allowlist = PathAllowlist::parse(vec!["/var/lib/app/last-run".to_string()]).unwrap();
    let allowed = PathRef::parse("/var/lib/app/last-run").unwrap();
    assert!(allowlist.ensure_allowed(&allowed).is_ok());

    let denied = PathRef::parse("/etc/shadow").unwrap();
    assert_eq!(
        allowlist.ensure_allowed(&denied).unwrap_err(),
        AppError::PathNotAllowed("/etc/shadow".to_string())
    );
}
