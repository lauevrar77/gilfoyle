# Detect the PackageKit bus and refuse the 2.x dialect

PackageKit's released versions (1.2 through 1.4) expose the bus
`org.freedesktop.PackageKit`, the interface `org.freedesktop.PackageKit.Transaction`, and
the classic enumeration values. The unreleased 2.x branch renamed the bus to
`org.freedesktop.packagekit1` **and re-ordered the enumerations**, which are passed on the
wire as `1 << index` bitfields. The value of, say, the `installed` filter or the
`SIMULATE` flag therefore differs between the two dialects.

Gilfoyle uses a single in-process signal stream and decodes each signal by its member
name, so it handles both the singular `Package` (1.0–1.2.5) and the batched `Packages`
(1.2.6+) shapes without a version check. For the enumerations it targets the 1.x wire
values and **refuses 2.x explicitly**: the daemon is detected on both bus names, and a
2.x daemon yields a clear "unsupported" error rather than a mis-decoded transaction.

Considered options: supporting both dialects (rejected — a full second enumeration table
and signal layout, for an unreleased version we cannot test against); ignoring the
version (rejected — silent mis-decoding).
