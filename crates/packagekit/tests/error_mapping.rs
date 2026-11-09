use std::sync::Arc;

use gilfoyle_core::PackageError;
use gilfoyle_packagekit::{map_dbus_error_name, map_zbus_error};

#[test]
fn maps_polkit_denials() {
    for name in [
        "org.freedesktop.packagekit.Denied",
        "org.freedesktop.packagekit1.Denied",
        "org.freedesktop.DBus.Error.AccessDenied",
        "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired",
        "org.freedesktop.PolicyKit1.Error.NotAuthorized",
    ] {
        assert_eq!(
            map_dbus_error_name(name, "no"),
            PackageError::NotAuthorized("no".to_string()),
            "{name}"
        );
    }
}

#[test]
fn maps_service_timeout_and_unknown_errors() {
    assert_eq!(
        map_dbus_error_name("org.freedesktop.DBus.Error.ServiceUnknown", "x"),
        PackageError::Unavailable("x".to_string())
    );
    assert_eq!(
        map_dbus_error_name("org.freedesktop.DBus.Error.NameHasNoOwner", "x"),
        PackageError::Unavailable("x".to_string())
    );
    assert_eq!(
        map_dbus_error_name("org.freedesktop.DBus.Error.TimedOut", "x"),
        PackageError::Timeout
    );
    assert_eq!(
        map_dbus_error_name("org.freedesktop.DBus.Error.NoReply", "x"),
        PackageError::Timeout
    );
    assert_eq!(
        map_dbus_error_name("com.example.Boom", "boom"),
        PackageError::Other {
            name: "com.example.Boom".to_string(),
            message: "boom".to_string(),
        }
    );
}

#[test]
fn maps_zbus_errors() {
    let timed_out = zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "late",
    )));
    assert_eq!(map_zbus_error(timed_out), PackageError::Timeout);

    let broken = zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "gone",
    )));
    assert!(matches!(
        map_zbus_error(broken),
        PackageError::Unavailable(_)
    ));

    let denied = zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied("nope".to_string())));
    assert!(matches!(
        map_zbus_error(denied),
        PackageError::NotAuthorized(_)
    ));

    assert!(matches!(
        map_zbus_error(zbus::Error::Failure("boom".to_string())),
        PackageError::Unavailable(_)
    ));
}
