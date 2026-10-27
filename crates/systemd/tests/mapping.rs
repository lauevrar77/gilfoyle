use std::sync::Arc;

use gilfoyle_core::{SystemdError, Unit, UnitFilter, UnitType};
use gilfoyle_systemd::{apply_filter, map_dbus_error_name, map_zbus_error, raw_to_unit};

fn unit(name: &str) -> Unit {
    Unit {
        name: name.to_string(),
        description: format!("desc {name}"),
        load_state: "loaded".to_string(),
        active_state: "active".to_string(),
        sub_state: "running".to_string(),
    }
}

#[test]
fn raw_to_unit_maps_the_expected_fields() {
    let raw = (
        "nginx.service".to_string(),
        "nginx web server".to_string(),
        "loaded".to_string(),
        "active".to_string(),
        "running".to_string(),
        String::new(),
        zbus::OwnedObjectPath::try_from("/org/freedesktop/systemd1/unit/nginx_2eservice").unwrap(),
        0,
        String::new(),
        zbus::OwnedObjectPath::try_from("/").unwrap(),
    );
    let mapped = raw_to_unit(&raw);
    assert_eq!(mapped.name, "nginx.service");
    assert_eq!(mapped.description, "nginx web server");
    assert_eq!(mapped.load_state, "loaded");
    assert_eq!(mapped.active_state, "active");
    assert_eq!(mapped.sub_state, "running");
}

#[test]
fn apply_filter_by_unit_type() {
    let units = vec![
        unit("nginx.service"),
        unit("sshd.socket"),
        unit("logrotate.timer"),
    ];
    let filter = UnitFilter {
        unit_type: Some(UnitType::Service),
        ..UnitFilter::default()
    };
    let filtered = apply_filter(units.clone(), &filter);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "nginx.service");

    let filter = UnitFilter {
        unit_type: Some(UnitType::Socket),
        ..UnitFilter::default()
    };
    assert_eq!(apply_filter(units, &filter)[0].name, "sshd.socket");
}

#[test]
fn apply_filter_by_name_is_case_insensitive() {
    let units = vec![unit("nginx.service"), unit("postgresql.service")];
    let filter = UnitFilter {
        name_contains: Some("NGINX".to_string()),
        ..UnitFilter::default()
    };
    let filtered = apply_filter(units, &filter);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "nginx.service");
}

#[test]
fn apply_filter_applies_the_limit_last() {
    let units = vec![unit("a.service"), unit("b.service"), unit("c.service")];
    let filter = UnitFilter {
        name_contains: Some(".service".to_string()),
        limit: Some(2),
        ..UnitFilter::default()
    };
    let filtered = apply_filter(units, &filter);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].name, "a.service");
    assert_eq!(filtered[1].name, "b.service");
}

#[test]
fn apply_filter_passes_everything_by_default() {
    let units = vec![unit("a.service"), unit("b.timer")];
    assert_eq!(apply_filter(units, &UnitFilter::default()).len(), 2);
}

#[test]
fn maps_every_named_error() {
    let cases = [
        (
            "org.freedesktop.systemd1.NoSuchUnit",
            SystemdError::NotFound("u".to_string()),
        ),
        (
            "org.freedesktop.systemd1.UnitMasked",
            SystemdError::Masked("u".to_string()),
        ),
        (
            "org.freedesktop.systemd1.LoadFailed",
            SystemdError::LoadFailed("u".to_string(), "boom".to_string()),
        ),
        (
            "org.freedesktop.systemd1.OnlyByDependency",
            SystemdError::OnlyByDependency("u".to_string()),
        ),
        (
            "org.freedesktop.systemd1.JobFailed",
            SystemdError::JobFailed("u".to_string(), "boom".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.InvalidArgs",
            SystemdError::Invalid("boom".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.AccessDenied",
            SystemdError::Forbidden("u".to_string(), "boom".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired",
            SystemdError::Forbidden("u".to_string(), "boom".to_string()),
        ),
        (
            "org.freedesktop.PolicyKit1.Error.NotAuthorized",
            SystemdError::Forbidden("u".to_string(), "boom".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.ServiceUnknown",
            SystemdError::Unavailable("boom".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.TimedOut",
            SystemdError::Timeout("u".to_string()),
        ),
        (
            "org.freedesktop.DBus.Error.NoReply",
            SystemdError::Timeout("u".to_string()),
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(map_dbus_error_name(name, "boom", "u"), expected, "{name}");
    }
}

#[test]
fn falls_back_to_the_other_variant() {
    assert_eq!(
        map_dbus_error_name("com.example.Boom", "boom", "u"),
        SystemdError::Other {
            name: "com.example.Boom".to_string(),
            message: "boom".to_string(),
        }
    );
}

#[test]
fn maps_fdo_errors() {
    let error = zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied("nope".to_string())));
    assert!(matches!(
        map_zbus_error(error, "nginx.service"),
        SystemdError::Forbidden(_, _)
    ));
}

#[test]
fn maps_io_timeouts_and_other_errors() {
    let timed_out = zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "late",
    )));
    assert_eq!(
        map_zbus_error(timed_out, "u"),
        SystemdError::Timeout("u".to_string())
    );

    let broken = zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "gone",
    )));
    assert!(matches!(
        map_zbus_error(broken, "u"),
        SystemdError::Unavailable(_)
    ));

    assert!(matches!(
        map_zbus_error(zbus::Error::Failure("boom".to_string()), "u"),
        SystemdError::Unavailable(_)
    ));
}
