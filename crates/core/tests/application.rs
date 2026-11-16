use std::sync::Arc;

use gilfoyle_core::{
    AppError, DiskAdmin, DiskError, DiskService, DiskSource, FilesystemUsage, HostAdmin,
    HostService, LoadAverage, LoadSource, Memory, MemorySource, PathAdmin, PathAllowlist,
    PathError, PathRef, PathService, PathSource, PathStatus, SystemError,
};

struct FakeLoad;

impl LoadSource for FakeLoad {
    fn load_average(&self) -> Result<LoadAverage, SystemError> {
        LoadAverage::new(1.0, 2.0, 3.0, 4, 5)
            .map_err(|error| SystemError::Invalid(error.to_string()))
    }
}

struct FakeMemory;

impl MemorySource for FakeMemory {
    fn memory(&self) -> Result<Memory, SystemError> {
        Ok(Memory::new(10, 5, 5, 2, 1))
    }
}

#[test]
fn host_service_combines_load_and_memory() {
    let service = HostService::new(FakeLoad, FakeMemory);
    let report = service.load_and_memory().unwrap();
    assert_eq!(report.load.one, 1.0);
    assert_eq!(report.load.total, 5);
    assert_eq!(report.memory.total_kib, 10);
    assert_eq!(report.memory.swap_total_kib, 2);
}

struct FakeDisk;

impl DiskSource for FakeDisk {
    fn disk_usage(&self) -> Result<Vec<FilesystemUsage>, DiskError> {
        Ok(vec![FilesystemUsage::measured(
            "/",
            "/dev/vda1",
            "ext4",
            100,
            40,
            10,
            4,
        )])
    }
}

#[test]
fn disk_service_forwards_to_the_source() {
    let service = DiskService::new(Arc::new(FakeDisk));
    let usage = service.disk_usage().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].mount, "/");
    assert_eq!(usage[0].used_bytes, 60);
}

struct FakePath;

impl PathSource for FakePath {
    fn stat(&self, path: &PathRef) -> Result<PathStatus, PathError> {
        Ok(PathStatus::missing(path.as_str()))
    }
}

#[test]
fn path_service_enforces_the_allowlist() {
    let allowlist = PathAllowlist::parse(vec!["/var/lib/app/last-run".to_string()]).unwrap();
    let service = PathService::new(Arc::new(FakePath), allowlist);

    let status = service.path_status("/var/lib/app/last-run").unwrap();
    assert_eq!(status.path, "/var/lib/app/last-run");
    assert!(!status.exists);

    assert_eq!(
        service.path_status("/etc/shadow").unwrap_err(),
        AppError::PathNotAllowed("/etc/shadow".to_string())
    );
    assert!(matches!(
        service.path_status("relative").unwrap_err(),
        AppError::Domain(_)
    ));
}
