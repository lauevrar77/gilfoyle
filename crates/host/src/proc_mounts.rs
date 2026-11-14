use gilfoyle_core::DiskError;

/// One entry of `/proc/mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    /// The backing device.
    pub device: String,
    /// The mount point.
    pub mount_point: String,
    /// The filesystem type.
    pub fstype: String,
}

/// Parse the contents of `/proc/mounts`.
///
/// Each line is `device mountpoint fstype options dump pass`; only the first
/// three fields are read. Blank lines are ignored, and C-style octal escapes in
/// the device and mount point (`\040` for a space, …) are decoded.
pub fn parse_mounts(input: &str) -> Result<Vec<MountEntry>, DiskError> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_mount_line)
        .collect()
}

/// Parse one line of `/proc/mounts`.
fn parse_mount_line(line: &str) -> Result<MountEntry, DiskError> {
    let mut fields = line.split_whitespace();
    let device = fields
        .next()
        .ok_or_else(|| DiskError::Malformed(line.to_string()))?;
    let mount_point = fields
        .next()
        .ok_or_else(|| DiskError::Malformed(line.to_string()))?;
    let fstype = fields
        .next()
        .ok_or_else(|| DiskError::Malformed(line.to_string()))?;
    Ok(MountEntry {
        device: unescape(device),
        mount_point: unescape(mount_point),
        fstype: fstype.to_string(),
    })
}

/// Whether a filesystem type is a network filesystem gilfoyle does not probe.
///
/// A `statvfs` on a stale network mount can block indefinitely, so these are
/// reported as unsupported instead of being measured.
pub fn is_network_fs(fstype: &str) -> bool {
    matches!(fstype, "nfs" | "nfs4" | "cifs" | "smb3") || fstype.starts_with("fuse")
}

/// Decode the C-style octal escapes `/proc/mounts` uses in device and mount
/// point fields (for example `\040` for a space).
fn unescape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits: String = chars.by_ref().take(3).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(value) if digits.len() == 3 => out.push(value as char),
            _ => {
                out.push('\\');
                out.push_str(&digits);
            }
        }
    }
    out
}
