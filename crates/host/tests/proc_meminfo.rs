use std::io::Write;

use gilfoyle_core::{MemorySource, SystemError};
use gilfoyle_host::{DEFAULT_PROC_MEMINFO_PATH, ProcMeminfo, parse_meminfo};

const SAMPLE: &str = "\
MemTotal:        1000 kB
MemFree:          100 kB
MemAvailable:     250 kB
Buffers:           10 kB
Cached:           500 kB
SwapTotal:        200 kB
SwapFree:          50 kB
";

#[test]
fn parses_the_reported_fields() {
    let memory = parse_meminfo(SAMPLE).unwrap();
    assert_eq!(memory.total_kib, 1000);
    assert_eq!(memory.free_kib, 100);
    assert_eq!(memory.available_kib, 250);
    assert_eq!(memory.used_kib, 750);
    assert_eq!(memory.swap_total_kib, 200);
    assert_eq!(memory.swap_free_kib, 50);
    assert!((memory.used_percent - 75.0).abs() < 1e-9);
}

#[test]
fn falls_back_to_free_when_available_is_absent() {
    let memory = parse_meminfo("MemTotal: 1000 kB\nMemFree: 400 kB\n").unwrap();
    assert_eq!(memory.available_kib, 400);
    assert_eq!(memory.used_kib, 600);
    assert_eq!(memory.swap_total_kib, 0);
    assert_eq!(memory.swap_free_kib, 0);
}

#[test]
fn ignores_lines_without_a_colon_and_unknown_keys() {
    let memory = parse_meminfo(
        "garbage without colon\nHugePages_Total: 7\nMemTotal: 10 kB\nMemFree: 5 kB\n",
    )
    .unwrap();
    assert_eq!(memory.total_kib, 10);
    assert_eq!(memory.free_kib, 5);
}

#[test]
fn accepts_a_value_without_the_unit_suffix() {
    let memory = parse_meminfo("MemTotal: 10\nMemFree: 5\n").unwrap();
    assert_eq!(memory.total_kib, 10);
}

#[test]
fn rejects_a_missing_total_or_free() {
    assert!(matches!(
        parse_meminfo("MemFree: 5 kB\n").unwrap_err(),
        SystemError::Malformed(_)
    ));
    assert!(matches!(
        parse_meminfo("MemTotal: 5 kB\n").unwrap_err(),
        SystemError::Malformed(_)
    ));
}

#[test]
fn rejects_a_malformed_value() {
    assert_eq!(
        parse_meminfo("MemTotal: oops kB\nMemFree: 5 kB\n").unwrap_err(),
        SystemError::Malformed("oops".to_string())
    );
    assert_eq!(
        parse_meminfo("MemTotal:  kB\nMemFree: 5 kB\n").unwrap_err(),
        SystemError::Malformed("kB".to_string())
    );
    assert_eq!(
        parse_meminfo("MemTotal:\nMemFree: 5 kB\n").unwrap_err(),
        SystemError::Empty
    );
}

#[test]
fn new_reads_the_real_proc_meminfo() {
    let adapter = ProcMeminfo::new();
    assert_eq!(adapter.path().to_string_lossy(), DEFAULT_PROC_MEMINFO_PATH);
    assert!(adapter.memory().is_ok());
}

#[test]
fn default_matches_new() {
    assert_eq!(ProcMeminfo::default().path(), ProcMeminfo::new().path());
}

#[test]
fn reads_from_an_injected_path() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "MemTotal: 2048 kB\nMemFree: 1024 kB\n").unwrap();
    let adapter = ProcMeminfo::with_path(file.path());
    assert_eq!(adapter.memory().unwrap().total_kib, 2048);
}

#[test]
fn reports_io_error_for_a_missing_path() {
    let adapter = ProcMeminfo::with_path("/nonexistent/gilfoyle/meminfo");
    assert!(matches!(adapter.memory(), Err(SystemError::Io(_))));
}
