use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use gilfoyle_core::{DiskError, DiskSource, MountPoint, MountPoints};
use gilfoyle_host::{DEFAULT_PROC_MOUNTS_PATH, DiskProbe, Filesystem, FsStat, RealFilesystem};

/// A filesystem probe with canned answers.
struct FakeFs {
    results: HashMap<String, Result<FsStat, String>>,
}

impl Filesystem for FakeFs {
    fn statvfs(&self, path: &str) -> Result<FsStat, String> {
        match self.results.get(path) {
            Some(result) => result.clone(),
            None => Err(format!("no fake for {path}")),
        }
    }
}

fn point(path: &str) -> MountPoint {
    MountPoint::parse(path).unwrap()
}

fn stat(total: u64, free: u64, inodes: u64, free_inodes: u64) -> FsStat {
    FsStat {
        total_bytes: total,
        free_bytes: free,
        inodes_total: inodes,
        inodes_free: free_inodes,
    }
}

fn probe(
    mounts: &str,
    points: Vec<MountPoint>,
    results: Vec<(&str, Result<FsStat, String>)>,
) -> (tempfile::NamedTempFile, DiskProbe) {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(mounts.as_bytes()).unwrap();
    file.flush().unwrap();
    let mut map = HashMap::new();
    for (path, result) in results {
        map.insert(path.to_string(), result);
    }
    let probe = DiskProbe::with_parts(
        file.path(),
        MountPoints::new(points),
        Arc::new(FakeFs { results: map }),
    );
    (file, probe)
}

#[test]
fn default_reads_the_real_mount_table() {
    assert_eq!(
        DiskProbe::new(MountPoints::root())
            .mounts_path()
            .to_string_lossy(),
        DEFAULT_PROC_MOUNTS_PATH
    );
}

#[test]
fn measures_a_mounted_filesystem() {
    let (_file, probe) = probe(
        "/dev/vda1 / ext4 rw 0 0\n",
        vec![point("/")],
        vec![("/", Ok(stat(1000, 250, 100, 40)))],
    );
    let usage = probe.disk_usage().unwrap();
    assert_eq!(usage.len(), 1);
    assert!(usage[0].mounted);
    assert_eq!(usage[0].device, "/dev/vda1");
    assert_eq!(usage[0].fstype, "ext4");
    assert_eq!(usage[0].total_bytes, 1000);
    assert_eq!(usage[0].free_bytes, 250);
    assert_eq!(usage[0].used_bytes, 750);
    assert_eq!(usage[0].error, None);
}

#[test]
fn reports_a_configured_path_that_is_not_mounted() {
    let (_file, probe) = probe(
        "/dev/vda1 / ext4 rw 0 0\n",
        vec![point("/mnt/backup")],
        vec![],
    );
    let usage = probe.disk_usage().unwrap();
    assert!(!usage[0].mounted);
    assert_eq!(usage[0].mount, "/mnt/backup");
    assert_eq!(usage[0].error.as_deref(), Some("not mounted"));
}

#[test]
fn skips_a_network_filesystem() {
    let (_file, probe) = probe(
        "server:/export /mnt/nfs nfs4 rw 0 0\n",
        vec![point("/mnt/nfs")],
        vec![],
    );
    let usage = probe.disk_usage().unwrap();
    assert!(usage[0].mounted);
    assert_eq!(usage[0].fstype, "nfs4");
    assert!(usage[0].error.as_deref().unwrap().contains("unsupported"));
}

#[test]
fn reports_a_probe_failure_per_entry() {
    let (_file, probe) = probe(
        "/dev/vdb1 /var xfs rw 0 0\n",
        vec![point("/var")],
        vec![("/var", Err("boom".to_string()))],
    );
    let usage = probe.disk_usage().unwrap();
    assert!(usage[0].mounted);
    assert_eq!(usage[0].device, "/dev/vdb1");
    assert_eq!(usage[0].error.as_deref(), Some("boom"));
}

#[test]
fn reports_an_io_error_when_the_mount_table_is_unreadable() {
    let probe = DiskProbe::with_parts(
        "/nonexistent/gilfoyle/mounts",
        MountPoints::root(),
        Arc::new(FakeFs {
            results: HashMap::new(),
        }),
    );
    match probe.disk_usage().unwrap_err() {
        DiskError::Io { path, .. } => {
            assert_eq!(path, "/nonexistent/gilfoyle/mounts");
        }
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn propagates_a_malformed_mount_table() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "truncated").unwrap();
    let probe = DiskProbe::with_parts(
        file.path(),
        MountPoints::root(),
        Arc::new(FakeFs {
            results: HashMap::new(),
        }),
    );
    assert!(matches!(
        probe.disk_usage().unwrap_err(),
        DiskError::Malformed(_)
    ));
}

#[test]
fn real_filesystem_reads_a_real_directory() {
    let dir = tempfile::tempdir().unwrap();
    let stat = RealFilesystem
        .statvfs(dir.path().to_str().unwrap())
        .unwrap();
    assert!(stat.total_bytes > 0);
    assert!(stat.inodes_total > 0);
    assert!(stat.free_bytes <= stat.total_bytes);
}

#[test]
fn real_filesystem_reports_an_error_for_a_missing_path() {
    assert!(
        RealFilesystem
            .statvfs("/nonexistent/gilfoyle/disk")
            .is_err()
    );
}
