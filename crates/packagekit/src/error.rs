//! Mapping from `zbus` errors to the domain error type.

use gilfoyle_core::PackageError;
use zbus::DBusError;

/// Map a `zbus` error to a [`PackageError`], keeping the raw D-Bus name and message.
pub fn map_zbus_error(error: zbus::Error) -> PackageError {
    match error {
        zbus::Error::MethodError(name, message, _) => {
            map_dbus_error_name(name.as_str(), message.as_deref().unwrap_or(""))
        }
        zbus::Error::FDO(error) => {
            map_dbus_error_name(&error.name().to_string(), &error.to_string())
        }
        zbus::Error::InputOutput(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            PackageError::Timeout
        }
        other => PackageError::Unavailable(other.to_string()),
    }
}

/// Map a raw D-Bus error name (and message) to a [`PackageError`].
pub fn map_dbus_error_name(name: &str, message: &str) -> PackageError {
    // The denial domain differs by version: `org.freedesktop.packagekit.Denied`
    // on 1.x, `org.freedesktop.packagekit1.Denied` on 2.x.
    if name.ends_with(".Denied")
        || name == "org.freedesktop.DBus.Error.AccessDenied"
        || name == "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
        || name == "org.freedesktop.PolicyKit1.Error.NotAuthorized"
    {
        return PackageError::NotAuthorized(message.to_string());
    }
    match name {
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => {
            PackageError::Unavailable(message.to_string())
        }
        "org.freedesktop.DBus.Error.TimedOut" | "org.freedesktop.DBus.Error.NoReply" => {
            PackageError::Timeout
        }
        other => PackageError::Other {
            name: other.to_string(),
            message: message.to_string(),
        },
    }
}
