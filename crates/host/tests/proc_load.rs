use std::io::Write;

use gilfoyle_core::{LoadSource, SystemError};
use gilfoyle_host::{DEFAULT_PROC_LOADAVG_PATH, ProcLoad, parse_loadavg};

#[test]
fn parses_a_plain_line() {
    let average = parse_loadavg("0.42 0.35 0.30 1/312 12345\n").unwrap();
    assert_eq!(average.one, 0.42);
    assert_eq!(average.five, 0.35);
    assert_eq!(average.fifteen, 0.30);
    assert_eq!(average.runnable, 1);
    assert_eq!(average.total, 312);
}

#[test]
fn parses_without_the_trailing_pid() {
    let average = parse_loadavg("1.0 2.0 3.0 4/56").unwrap();
    assert_eq!(average.total, 56);
}

#[test]
fn rejects_empty_input() {
    assert_eq!(parse_loadavg("").unwrap_err(), SystemError::Empty);
    assert_eq!(parse_loadavg("  \n\t").unwrap_err(), SystemError::Empty);
    assert_eq!(parse_loadavg("1.0 2.0").unwrap_err(), SystemError::Empty);
}

#[test]
fn rejects_a_malformed_average() {
    assert_eq!(
        parse_loadavg("oops 0.1 0.1 1/2").unwrap_err(),
        SystemError::Malformed("oops".to_string())
    );
    assert_eq!(
        parse_loadavg("1.0 oops 0.1 1/2").unwrap_err(),
        SystemError::Malformed("oops".to_string())
    );
    assert_eq!(
        parse_loadavg("1.0 2.0 oops 1/2").unwrap_err(),
        SystemError::Malformed("oops".to_string())
    );
}

#[test]
fn rejects_a_missing_or_malformed_count() {
    assert_eq!(parse_loadavg("1 2 3").unwrap_err(), SystemError::Empty);
    assert_eq!(
        parse_loadavg("1 2 3 12").unwrap_err(),
        SystemError::Malformed("12".to_string())
    );
    assert_eq!(
        parse_loadavg("1 2 3 x/2").unwrap_err(),
        SystemError::Malformed("x".to_string())
    );
    assert_eq!(
        parse_loadavg("1 2 3 1/x").unwrap_err(),
        SystemError::Malformed("x".to_string())
    );
}

#[test]
fn rejects_a_non_finite_average() {
    assert!(matches!(
        parse_loadavg("NaN 2 3 1/2").unwrap_err(),
        SystemError::Invalid(_)
    ));
}

#[test]
fn new_reads_the_real_proc_loadavg() {
    let adapter = ProcLoad::new();
    assert_eq!(adapter.path().to_string_lossy(), DEFAULT_PROC_LOADAVG_PATH);
    assert!(adapter.load_average().is_ok());
}

#[test]
fn default_matches_new() {
    assert_eq!(ProcLoad::default().path(), ProcLoad::new().path());
}

#[test]
fn reads_from_an_injected_path() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "0.01 0.02 0.03 2/100 999").unwrap();
    let adapter = ProcLoad::with_path(file.path());
    assert_eq!(adapter.load_average().unwrap().total, 100);
}

#[test]
fn reports_io_error_for_a_missing_path() {
    let adapter = ProcLoad::with_path("/nonexistent/gilfoyle/loadavg");
    assert!(matches!(adapter.load_average(), Err(SystemError::Io(_))));
}
