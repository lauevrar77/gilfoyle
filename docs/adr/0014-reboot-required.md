# Report reboot state honestly, and daemon-reload per unit

Status: accepted

An agent that upgrades packages needs to know whether a reboot is pending afterwards. The
natural request — "does this host need a reboot?" — has no portable answer that gilfoyle can
compute in-process.

## Reboot state is distribution-specific

On Debian-family hosts the answer is a marker file: `/run/reboot-required` exists when a
reboot is pending, and `/run/reboot-required.pkgs` lists the packages that asked for it. It is
created and removed by the package manager; gilfoyle only reads it.

On the RPM side (Rocky, Fedora) there is no such marker. The usual mechanism is the
`needs-restarting` tool, which gilfoyle will not spawn (it would reintroduce process
execution). The remaining in-process heuristics — comparing the running kernel to installed
kernel packages, say — are partial (they miss core libraries) and fragile across naming.

So the tool reports the Debian-family verdict, and reports **unknown** everywhere else:

```json
{ "required": true, "packages": ["linux-image-amd64", "libc6"], "origin": "debian" }
```

```json
{ "required": null, "packages": [], "origin": "unsupported" }
```

`required` is `false` only when a Debian-family host has no marker. **Unknown is never
reported as `false`**: a fabricated "no reboot needed" would be worse than an honest "cannot
tell". This mirrors the disk adapter, which reports a non-mounted path as `mounted: false`
rather than measuring the parent filesystem.

## How the host is recognised

The distribution is read from `/etc/os-release` (fields `ID` and `ID_LIKE`) and matched
against the Debian family (`debian`, `ubuntu`). `/etc/os-release` and the `/run` markers are
fixed, well-known, non-disclosing files, and the tool returns the package *names* but never
the file *contents*, so `get_reboot_status` is an **open** read under ADR 0012.

A missing `/etc/os-release` is treated as unsupported rather than an error; any other
read failure (for example `EACCES` on the marker) is a real error, never a silent "no".

## Daemon reload is per unit, not global

The sibling question — "do the unit definitions on disk need a `daemon-reload`?" — has no
manager-level flag: systemd exposes `NeedDaemonReload` on each **Unit**, not on the manager.
A global answer would mean reading the property from every loaded unit on every call, which is
an unbounded amount of work for a read.

So gilfoyle does not add a global tool. It adds `need_daemon_reload` to the existing
`unit_status` result, where the property costs one more D-Bus read on a unit the caller already
named. There is deliberately no `daemon_reload` mutation: reloading the manager is not in the
closed operation set.

## Considered options

- Spawning `needs-restarting -r` (rejected — reintroduces process execution).
- Comparing the running kernel to installed kernel packages (rejected — partial and
  distribution-specific, and it misses the case of core libraries).
- A global `daemon_reload_required` tool (rejected — the property is per unit; a global answer
  would scan every unit).
- Reading `/run/reboot-required` unconditionally and reporting `false` when absent (rejected —
  that is only meaningful on a Debian-family host, and lies elsewhere).
