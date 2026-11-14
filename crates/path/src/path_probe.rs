use std::fs::{FileType, Metadata};
use std::os::unix::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

use gilfoyle_core::{PathError, PathKind, PathRef, PathSource, PathStatus};

/// Adapter reading the metadata of allowlisted paths.
///
/// It uses `lstat` (`std::fs::symlink_metadata`), never `stat`, so the final
/// component is not followed: a symlink is reported as a symlink with its
/// target, and never resolved to an unlisted path.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathProbe;

impl PathSource for PathProbe {
    fn stat(&self, path: &PathRef) -> Result<PathStatus, PathError> {
        match std::fs::symlink_metadata(path.as_str()) {
            Ok(metadata) => Ok(present(path.as_str(), &metadata, SystemTime::now())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PathStatus::missing(path.as_str()))
            }
            Err(error) => Err(PathError::Failed {
                path: path.as_str().to_string(),
                message: error.to_string(),
            }),
        }
    }
}

/// Build the metadata of an existing path, as of `now`.
///
/// `now` is injected so the computed age is deterministic in tests.
pub fn present(path: &str, metadata: &Metadata, now: SystemTime) -> PathStatus {
    let kind = kind_of(metadata.file_type());
    PathStatus {
        path: path.to_string(),
        exists: true,
        kind: Some(kind),
        size_bytes: Some(metadata.len()),
        age_seconds: age_seconds(metadata, now),
        modified: to_rfc3339(metadata.modified()),
        accessed: to_rfc3339(metadata.accessed()),
        changed: rfc3339_from_unix(metadata.ctime(), metadata.ctime_nsec()),
        mode: Some(metadata.mode() & 0o7777),
        uid: Some(metadata.uid()),
        gid: Some(metadata.gid()),
        symlink_target: symlink_target(path, kind),
    }
}

/// The kind of filesystem object behind a `FileType`.
fn kind_of(file_type: FileType) -> PathKind {
    if file_type.is_symlink() {
        PathKind::Symlink
    } else if file_type.is_dir() {
        PathKind::Dir
    } else if file_type.is_file() {
        PathKind::File
    } else {
        PathKind::Other
    }
}

/// The target of a symlink, read without following it.
fn symlink_target(path: &str, kind: PathKind) -> Option<String> {
    match kind {
        PathKind::Symlink => std::fs::read_link(path)
            .ok()
            .map(|target| target.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// The age of the path in seconds, from its modification time.
///
/// `None` when the modification time is unavailable or in the future.
fn age_seconds(metadata: &Metadata, now: SystemTime) -> Option<i64> {
    let modified = metadata.modified().ok()?;
    let age = now.duration_since(modified).ok()?;
    i64::try_from(age.as_secs()).ok()
}

/// Render a file time as RFC 3339 UTC seconds.
fn to_rfc3339(time: std::io::Result<SystemTime>) -> Option<String> {
    let duration = time.ok()?.duration_since(UNIX_EPOCH).ok()?;
    let seconds = i64::try_from(duration.as_secs()).ok()?;
    let datetime = chrono::DateTime::from_timestamp(seconds, duration.subsec_nanos())?;
    Some(datetime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Render a Unix time (seconds + nanoseconds) as RFC 3339 UTC seconds.
fn rfc3339_from_unix(seconds: i64, nanos: i64) -> Option<String> {
    let nanos = u32::try_from(nanos).ok()?;
    let datetime = chrono::DateTime::from_timestamp(seconds, nanos)?;
    Some(datetime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn to_rfc3339_formats_a_known_time() {
        let time = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert_eq!(
            to_rfc3339(Ok(time)).as_deref(),
            Some("2001-09-09T01:46:40Z")
        );
    }

    #[test]
    fn to_rfc3339_is_none_for_errors_and_pre_epoch() {
        assert_eq!(to_rfc3339(Err(std::io::Error::other("boom"))), None);
        assert_eq!(to_rfc3339(Ok(UNIX_EPOCH - Duration::from_secs(1))), None);
    }

    #[test]
    fn rfc3339_from_unix_formats_and_rejects_bad_values() {
        assert_eq!(
            rfc3339_from_unix(0, 0).as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
        assert_eq!(rfc3339_from_unix(0, -1), None);
        assert_eq!(rfc3339_from_unix(0, 2_000_000_000), None);
        assert_eq!(rfc3339_from_unix(i64::MAX, 0), None);
    }
}
