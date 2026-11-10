---
title: Threat model
description: What Gilfoyle defends against, and what it does not.
order: 8
lang: en
---

## In scope

An attacker who reaches the SSH port, or who compromises the Gilfoyle process, must not gain
code execution or host privileges.

- **No code execution.** Tools are compiled functions; nothing accepts a command or a path
  to run. The SSH handler refuses `shell`/`exec`/`pty` and all forwarding. systemd and
  PackageKit are reached over in-process D-Bus, and Docker over its UNIX socket, never by
  spawning `systemctl`, `apt` or `docker`.
- **No install, no remove, by construction.** The package operation vocabulary is a closed
  enum with no install/remove variant; upgrades carry `ONLY_TRUSTED`, and a plan that would
  install or remove a package is refused. Docker has no create or remove operation at all.
- **Least privilege.** Run as an unprivileged dedicated user; systemd and package mutations
  are enforced twice (the tool's allowlist and the polkit rule). Docker has no polkit: its
  socket is root-equivalent, so grant it narrowly (a socket proxy) and keep
  `[docker] manage_containers` tight.
- **Auth.** Public keys only, mapped to a principal; per-key tool grants limit what a key
  can reach. The client pins host-key fingerprints, so SSH is not open to MITM.
- **Fail-closed.** No bind, no key or no host key means no start; no allowed package
  operation means no refresh or upgrade.

## Out of scope for now

- Rate limiting is left to the firewall and systemd.
- Key rotation requires a restart.
- OpenSSH certificates are not supported.

The SSH transport adds a pre-authentication parser surface and a C crypto backend (`ring`) —
a deliberate trade.
