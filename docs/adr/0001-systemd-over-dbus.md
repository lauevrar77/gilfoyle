# Control systemd over in-process D-Bus, not `systemctl`

Gilfoyle must never spawn a process: `std::process::Command` is banned at compile time and
`unsafe` is forbidden, so shelling out to the `systemctl` binary is off the table. To list
units, read their status and start or stop them, we talk to `org.freedesktop.systemd1` on
the system bus in-process using `zbus` (pure Rust, no C). Read calls are unprivileged;
start and stop are authorised by polkit.

Considered options: `systemctl` via `Command` (rejected — it reintroduces exactly the
shell-out surface the project exists to avoid); `libsystemd`/`sd-bus` FFI (rejected — a C
dependency and `unsafe`); reading `/run/systemd` files directly (rejected — undocumented
internals, and they cannot mutate anything).
