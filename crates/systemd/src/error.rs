//! Mapping from `zbus` errors to the domain error type.

use gilfoyle_core::SystemdError;
use zbus::DBusError;

/// Map a `zbus` error to a [`SystemdError`], keeping the raw D-Bus name and message.
pub fn map_zbus_error(error: zbus::Error, unit: &str) -> SystemdError {
    match error {
        zbus::Error::MethodError(name, message, _) => {
            map_dbus_error_name(name.as_str(), message.as_deref().unwrap_or(""), unit)
        }
        zbus::Error::FDO(error) => {
            map_dbus_error_name(&error.name().to_string(), &error.to_string(), unit)
        }
        zbus::Error::InputOutput(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            SystemdError::Timeout(unit.to_string())
        }
        other => SystemdError::Unavailable(other.to_string()),
    }
}

/// Map a raw D-Bus error name (and message) to a [`SystemdError`].
pub fn map_dbus_error_name(name: &str, message: &str, unit: &str) -> SystemdError {
    match name {
        "org.freedesktop.systemd1.NoSuchUnit" => SystemdError::NotFound(unit.to_string()),
        "org.freedesktop.systemd1.UnitMasked" => SystemdError::Masked(unit.to_string()),
        "org.freedesktop.systemd1.LoadFailed" => {
            SystemdError::LoadFailed(unit.to_string(), message.to_string())
        }
        "org.freedesktop.systemd1.OnlyByDependency" => {
            SystemdError::OnlyByDependency(unit.to_string())
        }
        "org.freedesktop.systemd1.JobFailed" => {
            SystemdError::JobFailed(unit.to_string(), message.to_string())
        }
        "org.freedesktop.DBus.Error.InvalidArgs" => SystemdError::Invalid(message.to_string()),
        "org.freedesktop.DBus.Error.AccessDenied"
        | "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
        | "org.freedesktop.PolicyKit1.Error.NotAuthorized" => {
            SystemdError::Forbidden(unit.to_string(), message.to_string())
        }
        "org.freedesktop.DBus.Error.ServiceUnknown" => {
            SystemdError::Unavailable(message.to_string())
        }
        "org.freedesktop.DBus.Error.TimedOut" | "org.freedesktop.DBus.Error.NoReply" => {
            SystemdError::Timeout(unit.to_string())
        }
        other => SystemdError::Other {
            name: other.to_string(),
            message: message.to_string(),
        },
    }
}
