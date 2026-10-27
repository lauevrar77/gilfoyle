use gilfoyle_core::{
    AppError, SystemAdmin, SystemError, SystemService, Uptime, UptimeReport, UptimeSource,
};

#[test]
fn from_seconds_accepts_valid_values() {
    assert_eq!(Uptime::from_seconds(0.0).unwrap().as_seconds(), 0.0);
    assert_eq!(Uptime::from_seconds(1.5).unwrap().as_seconds(), 1.5);
    assert_eq!(
        Uptime::from_seconds(1048545.67).unwrap().as_seconds(),
        1048545.67
    );
}

#[test]
fn from_seconds_rejects_negative() {
    assert!(Uptime::from_seconds(-0.5).is_err());
    assert!(Uptime::from_seconds(-1.0).is_err());
}

#[test]
fn from_seconds_rejects_non_finite() {
    assert!(Uptime::from_seconds(f64::NAN).is_err());
    assert!(Uptime::from_seconds(f64::INFINITY).is_err());
    assert!(Uptime::from_seconds(f64::NEG_INFINITY).is_err());
}

#[test]
fn human_renders_all_boundaries() {
    let cases = [
        (0.0, "0s"),
        (1.0, "1s"),
        (45.9, "45s"),
        (59.0, "59s"),
        (60.0, "1m 0s"),
        (61.0, "1m 1s"),
        (3599.0, "59m 59s"),
        (3600.0, "1h 0m 0s"),
        (3661.0, "1h 1m 1s"),
        (86399.0, "23h 59m 59s"),
        (86400.0, "1d 0h 0m 0s"),
        (90061.0, "1d 1h 1m 1s"),
        (1048545.0, "12d 3h 15m 45s"),
    ];
    for (seconds, expected) in cases {
        let uptime = Uptime::from_seconds(seconds).unwrap();
        assert_eq!(uptime.human(), expected, "seconds = {seconds}");
    }
}

#[derive(Clone)]
struct FakeSource {
    result: Result<Uptime, SystemError>,
}

impl UptimeSource for FakeSource {
    fn uptime(&self) -> Result<Uptime, SystemError> {
        self.result.clone()
    }
}

#[test]
fn service_builds_a_report_from_the_source() {
    let service = SystemService::new(FakeSource {
        result: Ok(Uptime::from_seconds(3661.0).unwrap()),
    });
    assert_eq!(
        service.uptime().unwrap(),
        UptimeReport {
            uptime_seconds: 3661.0,
            uptime_human: "1h 1m 1s".to_string(),
        }
    );
}

#[test]
fn service_propagates_source_errors() {
    let service = SystemService::new(FakeSource {
        result: Err(SystemError::Empty),
    });
    assert_eq!(
        service.uptime().unwrap_err(),
        AppError::System(SystemError::Empty)
    );
}
