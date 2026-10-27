use std::io::Write;

use gilfoyle_core::{SystemError, UptimeSource};
use gilfoyle_system::{DEFAULT_PROC_UPTIME_PATH, ProcUptime, parse_uptime};

#[test]
fn parses_a_plain_value() {
    assert_eq!(parse_uptime("12345.67\n").unwrap().as_seconds(), 12345.67);
}

#[test]
fn parses_with_the_idle_field_present() {
    assert_eq!(
        parse_uptime("12345.67 98765.43\n").unwrap().as_seconds(),
        12345.67
    );
}

#[test]
fn parses_leading_whitespace_and_tabs() {
    assert_eq!(parse_uptime("\t 42.5 \t 1.0").unwrap().as_seconds(), 42.5);
}

#[test]
fn parses_zero() {
    assert_eq!(parse_uptime("0.00 0.00").unwrap().as_seconds(), 0.0);
}

#[test]
fn rejects_empty_input() {
    assert_eq!(parse_uptime("").unwrap_err(), SystemError::Empty);
    assert_eq!(parse_uptime("   \n\t").unwrap_err(), SystemError::Empty);
}

#[test]
fn rejects_malformed_input() {
    assert_eq!(
        parse_uptime("not-a-number").unwrap_err(),
        SystemError::Malformed("not-a-number".to_string())
    );
}

#[test]
fn rejects_negative_input() {
    match parse_uptime("-1.0").unwrap_err() {
        SystemError::Invalid(_) => {}
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn rejects_nan_and_infinity() {
    assert!(matches!(
        parse_uptime("NaN").unwrap_err(),
        SystemError::Invalid(_)
    ));
    assert!(matches!(
        parse_uptime("inf").unwrap_err(),
        SystemError::Invalid(_)
    ));
}

#[test]
fn new_reads_the_real_proc_uptime() {
    let adapter = ProcUptime::new();
    assert_eq!(adapter.path().to_string_lossy(), DEFAULT_PROC_UPTIME_PATH);
    assert!(adapter.uptime().is_ok());
}

#[test]
fn default_matches_new() {
    assert_eq!(ProcUptime::default().path(), ProcUptime::new().path());
}

#[test]
fn reads_from_an_injected_path() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "4242.25 17.5").unwrap();

    let adapter = ProcUptime::with_path(file.path());
    assert_eq!(adapter.uptime().unwrap().as_seconds(), 4242.25);
}

#[test]
fn reports_io_error_for_a_missing_path() {
    let adapter = ProcUptime::with_path("/nonexistent/gilfoyle/uptime");
    match adapter.uptime().unwrap_err() {
        SystemError::Io(_) => {}
        other => panic!("expected Io, got {other:?}"),
    }
}
