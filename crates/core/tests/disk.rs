use gilfoyle_core::{FilesystemUsage, MountPoint, MountPoints};

#[test]
fn mount_point_accepts_absolute_paths() {
    assert_eq!(MountPoint::parse("/").unwrap().as_str(), "/");
    assert_eq!(
        MountPoint::parse("  /var/log ").unwrap().as_str(),
        "/var/log"
    );
}

#[test]
fn mount_point_rejects_relative_or_empty_paths() {
    assert!(MountPoint::parse("").is_err());
    assert!(MountPoint::parse("   ").is_err());
    assert!(MountPoint::parse("var").is_err());
}

#[test]
fn mount_points_root_is_the_root_filesystem() {
    let points = MountPoints::root();
    assert!(!points.is_empty());
    let collected: Vec<&str> = points.iter().map(MountPoint::as_str).collect();
    assert_eq!(collected, vec!["/"]);
}

#[test]
fn mount_points_can_be_empty_or_ordered() {
    let empty = MountPoints::new(Vec::new());
    assert!(empty.is_empty());

    let points = MountPoints::new(vec![
        MountPoint::parse("/").unwrap(),
        MountPoint::parse("/var").unwrap(),
    ]);
    assert!(!points.is_empty());
    let collected: Vec<&str> = points.iter().map(MountPoint::as_str).collect();
    assert_eq!(collected, vec!["/", "/var"]);
}

#[test]
fn measured_reports_usage_and_percentages() {
    let usage = FilesystemUsage::measured("/", "/dev/vda1", "ext4", 1000, 250, 100, 40);
    assert_eq!(usage.mount, "/");
    assert_eq!(usage.device, "/dev/vda1");
    assert_eq!(usage.fstype, "ext4");
    assert!(usage.mounted);
    assert_eq!(usage.total_bytes, 1000);
    assert_eq!(usage.free_bytes, 250);
    assert_eq!(usage.used_bytes, 750);
    assert!((usage.used_percent - 75.0).abs() < 1e-9);
    assert_eq!(usage.inodes_total, 100);
    assert_eq!(usage.inodes_free, 40);
    assert!((usage.inodes_used_percent - 60.0).abs() < 1e-9);
    assert_eq!(usage.error, None);
}

#[test]
fn measured_percentages_are_zero_when_totals_are_zero() {
    let usage = FilesystemUsage::measured("/", "", "", 0, 0, 0, 0);
    assert_eq!(usage.used_bytes, 0);
    assert_eq!(usage.used_percent, 0.0);
    assert_eq!(usage.inodes_used_percent, 0.0);
}

#[test]
fn measured_saturates_when_free_exceeds_total() {
    let usage = FilesystemUsage::measured("/", "", "", 100, 250, 10, 25);
    assert_eq!(usage.used_bytes, 0);
    assert_eq!(usage.used_percent, 0.0);
}

#[test]
fn unmeasured_constructors_explain_themselves() {
    let not_mounted = FilesystemUsage::not_mounted("/mnt/backup");
    assert!(!not_mounted.mounted);
    assert_eq!(not_mounted.total_bytes, 0);
    assert_eq!(not_mounted.error.as_deref(), Some("not mounted"));

    let unsupported = FilesystemUsage::unsupported("/mnt/nfs", "nfs4");
    assert!(unsupported.mounted);
    assert_eq!(unsupported.fstype, "nfs4");
    assert_eq!(
        unsupported.error.as_deref(),
        Some("unsupported filesystem type `nfs4`")
    );

    let failed = FilesystemUsage::failed("/var", "/dev/vdb1", "xfs", "boom");
    assert!(failed.mounted);
    assert_eq!(failed.device, "/dev/vdb1");
    assert_eq!(failed.error.as_deref(), Some("boom"));
}
