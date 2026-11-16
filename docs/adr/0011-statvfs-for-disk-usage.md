# Read disk usage with the read-only `statvfs` syscall

Status: accepted

Filesystem usage has no `/proc` file: the sizes (total, free, available) and
inode counts are only available through the `statvfs(2)` syscall. The load
average and memory, by contrast, come from fixed `/proc` files. So reporting
disk usage is the first host interaction that is not "reading a fixed file".

## A read-only syscall, not a command

The call is a **read**: it cannot write, mount, or unmount anything, and it
takes no command. The vocabulary stays closed — there is no request that
creates, mounts or removes a filesystem. This preserves the project's core
claim: gilfoyle cannot execute anything. What changes is only the sentence
"the only host interaction is reading a handful of fixed files", which becomes
"reading a few fixed files, plus one read-only syscall over the configured
mounts".

The syscall is reached through `rustix` (`rustix::fs::statvfs`), a safe wrapper:
`unsafe` stays forbidden workspace-wide, and `rustix` is portable, so the
macOS client — the same binary — still cross-compiles.

## Only the mount points the operator declares

An unbounded path namespace is exactly what gilfoyle avoids. `[host] disks` is
therefore the set of mount points to report, defaulting to `["/"]`. Paths that
are not currently in `/proc/mounts` are reported as `mounted: false` with an
error, and **never** measured — a `statvfs` on an unmounted mount point would
return the *parent* filesystem's numbers, which is worse than no answer.

Network filesystems (`nfs`, `nfs4`, `cifs`, `smb3`, `fuse*`) are reported as
unsupported rather than probed: a `statvfs` on a stale network mount can block
indefinitely.

## Authority

Disk usage is a bounded, named, non-disclosing read (see ADR 0012), so it is an
**open** read: no allowlist, like uptime. `[host] disks` bounds *what* is
reported, not *whether* the tool may run.

## Considered options

- Shelling out to `df` (rejected — reintroduces process spawning).
- Reading `/proc/self/mountinfo` for sizes (rejected — it does not contain free
  space).
- The `sysinfo` crate (rejected — a large dependency that moves host semantics
  into a crate; `rustix` exposes the one syscall we need).
