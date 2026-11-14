use gilfoyle_core::DiskError;
use gilfoyle_host::{is_network_fs, parse_mounts};

const SAMPLE: &str = "\
/dev/vda1 / ext4 rw,relatime 0 0
proc /proc proc rw,nosuid 0 0
/dev/vdb1 /var xfs rw 0 0
tmpfs /run tmpfs rw,nosuid 0 0
";

#[test]
fn parses_the_first_three_fields() {
    let entries = parse_mounts(SAMPLE).unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].device, "/dev/vda1");
    assert_eq!(entries[0].mount_point, "/");
    assert_eq!(entries[0].fstype, "ext4");
    assert_eq!(entries[2].mount_point, "/var");
}

#[test]
fn ignores_blank_lines() {
    let entries = parse_mounts("/dev/vda1 / ext4 rw 0 0\n\n   \n").unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn decodes_octal_escapes() {
    let entries = parse_mounts("/dev/vda1 /mnt/my\\040disk ext4 rw 0 0\n").unwrap();
    assert_eq!(entries[0].mount_point, "/mnt/my disk");
}

#[test]
fn leaves_non_octal_backslashes_intact() {
    let entries = parse_mounts("/dev/vda1 /mnt/x\\q ext4 rw 0 0\n").unwrap();
    assert_eq!(entries[0].mount_point, "/mnt/x\\q");
    let short = parse_mounts("/dev/vda1 /mnt/a\\04 ext4 rw 0 0\n").unwrap();
    assert_eq!(short[0].mount_point, "/mnt/a\\04");
}

#[test]
fn rejects_a_truncated_line() {
    assert!(matches!(
        parse_mounts("onlyonefield\n").unwrap_err(),
        DiskError::Malformed(_)
    ));
    assert!(matches!(
        parse_mounts("device mountpoint\n").unwrap_err(),
        DiskError::Malformed(_)
    ));
}

#[test]
fn identifies_network_filesystems() {
    assert!(is_network_fs("nfs"));
    assert!(is_network_fs("nfs4"));
    assert!(is_network_fs("cifs"));
    assert!(is_network_fs("smb3"));
    assert!(is_network_fs("fuse.sshfs"));
    assert!(!is_network_fs("ext4"));
    assert!(!is_network_fs("xfs"));
}
