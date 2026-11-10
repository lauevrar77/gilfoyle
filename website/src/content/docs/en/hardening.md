---
title: Hardening
description: The systemd sandbox, the capability and syscall restrictions, and the compile-time guards.
order: 3
lang: en
---

Gilfoyle runs as a dedicated unprivileged user and never escalates. The packaged systemd
unit sandboxes the process, and the Rust build rejects the risky constructs.

## The unit

`packaging/gilfoyle.service` ships hardened by default:

```ini
[Service]
User=gilfoyle
Group=gilfoyle

NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
AmbientCapabilities=
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM

ReadOnlyPaths=/proc/uptime
```

- **No capabilities.** `CapabilityBoundingSet=` and `AmbientCapabilities=` are empty, and
  `NoNewPrivileges=yes` means the process can never gain one.
- **A read-only view of the host.** `ProtectSystem=strict` and `ProtectHome=yes` make the
  filesystem read-only; `/proc/uptime` is the only host file the service may read.
- **A syscall filter.** `SystemCallFilter=@system-service` allows only the service syscalls;
  anything else returns `EPERM`.
- **No memory that is both writable and executable.** `MemoryDenyWriteExecute=yes`.
- **Networking limited to what it needs.** `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`
  — the system bus is reached over `AF_UNIX`.

## Compile-time guards

These are checked when the code is built, not at runtime.

- `unsafe` is forbidden workspace-wide.
- `clippy::disallowed_methods` (see `clippy.toml`) rejects `std::process::Command` and
  `std::process::exit`: any attempt to spawn a process fails the lint.
- The domain core performs no I/O and has no access to the network, the filesystem or the
  process table.

## Supply chain

`cargo-deny` (see `deny.toml`) denies unknown registries and git sources, carries an empty
advisory ignore list, pins a license allowlist, and bans `openssl`.

## Docker has no polkit

Docker is reached over its UNIX socket. Unlike systemd and PackageKit there is no polkit
behind it: access to `/var/run/docker.sock` is equivalent to **root**. The systemd unit
already allows `AF_UNIX`, and `ProtectSystem=strict` does not impede `connect()` on a socket,
so no extra directive is needed — but the socket must be granted narrowly (a socket proxy),
and `[docker] manage_containers` stays default-deny. See [Containers](/docs/docker/).

## What is not hardened

Rate limiting is left to the firewall and systemd, key rotation requires a restart, and
OpenSSH certificates are not supported. The SSH transport adds a pre-authentication parser
surface and a C crypto backend (`ring`) — a deliberate trade.
