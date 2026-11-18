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
  PackageKit are reached over in-process D-Bus, Docker over its UNIX socket, and disk usage
  through the read-only `statvfs` syscall, never by spawning `systemctl`, `apt` or `docker`.
- **No unbounded read.** A read that names an arbitrary path would be a metadata oracle over
  the whole filesystem. So a read is open only when it is bounded, named and non-disclosing;
  `path_status` is allowlisted (`[paths] allow`, empty by default), returns metadata and
  never contents, does not follow the final path component, and reports a real error — not
  "absent" — on anything but `ENOENT`. The other reads touch a fixed, well-known set of files
  (`/proc`, `/etc/os-release`, the reboot markers under `/run`) and return no file contents.
- **No install, no remove, by construction.** The package operation vocabulary is a closed
  enum with no install/remove variant; upgrades carry `ONLY_TRUSTED`, and a plan that would
  install or remove a package is refused. Docker has no create or remove operation at all.
- **Least privilege.** Run as an unprivileged dedicated user; systemd and package mutations
  are enforced twice (the tool's allowlist and the polkit rule). Docker has no polkit: its
  socket is root-equivalent, so grant it narrowly (a socket proxy) and keep
  `[docker] manage_containers` tight.
- **Auth.** Public keys only, mapped to a principal; per-key tool grants limit what a key
  can reach. The client pins host-key fingerprints, so SSH is not open to MITM.
- **Jump hosts relay, they do not terminate.** A node reached through a `[[jumps]]` entry is
  reached over an SSH `direct-tcpip` channel the client opens on the jump host, whose
  fingerprint the client also pins. The session to the node stays end-to-end encrypted and
  its per-key grants still apply; a compromised jump host can refuse service, not read or
  forge the session (ADR 0013).
- **Fail-closed.** No bind, no key or no host key means no start; no allowed package
  operation means no refresh or upgrade.

## Out of scope for now

- Rate limiting is left to the firewall and systemd.
- Key rotation requires a restart.
- OpenSSH certificates are not supported.
- `path_status` discloses metadata (existence, size, owner, times) for the declared paths;
  this is deliberate, bounded by `[paths] allow`, and off by default.
- `get_reboot_status` discloses the host's distribution identity and reboot state — that is
  the point of the tool — and reports `null` on hosts whose mechanism gilfoyle does not know.

The SSH transport adds a pre-authentication parser surface and a C crypto backend (`ring`) —
a deliberate trade.
