# Manage packages through PackageKit over in-process D-Bus

Gilfoyle must never spawn a process: `std::process::Command` is banned at compile time
and `unsafe` is forbidden. Managing packages therefore cannot mean shelling out to
`apt-get` or `dnf`. We talk to **PackageKit** (`org.freedesktop.PackageKit`) on the system
bus in-process using `zbus`, exactly as we do with systemd. PackageKit is a privileged
daemon (D-Bus activated) that mediates the work through polkit, so gilfoyle adds a
destination on a channel it already uses rather than a new privileged binary.

PackageKit is transactional: `CreateTransaction` returns a transaction object, methods
are invoked on it, and results arrive as signals (`Packages`, `Finished`, `ErrorCode`,
...). The adapter models this as a small state machine: subscribe, invoke, then feed the
decoded signals to a pure collector in the core.

Considered options: a gilfoyle-owned root helper that shells out to `apt`/`dnf` (rejected
— it would reintroduce the process-spawning surface and add a privileged binary to
harden); `systemctl start` of pre-provisioned oneshot units (rejected — no structured
result, no control over non-interactive behaviour); PackageKit (chosen — in-process
D-Bus, polkit-authorised, cross-distro between APT and DNF).

Consequences: PackageKit and its backend must be installed on the host; gilfoyle reports
a clear error when the daemon is absent. The trade is a daemon we do not control in
exchange for no new privileged code of our own.
