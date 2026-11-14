# Reads are open when they are bounded and non-disclosing; otherwise they are allowlisted

Status: accepted

The project says "reads are open". That held for uptime, units, containers,
packages and host metrics because each is a **bounded, named** capability: the
agent can ask about a finite set the operator can enumerate. A path is the
opposite — an **unbounded namespace**. A read that takes an arbitrary path lets
the agent (or an attacker who held the key) learn whether any file exists, its
size, owner, permissions and timestamps: a metadata oracle over the whole
filesystem.

So the principle is made explicit:

> A read is **open** when it is bounded, named, and does not disclose host
> contents or configuration. Otherwise it is **allowlisted** and empty by
> default.

The classification is frozen:

| Read | Class | Why |
|---|---|---|
| `get_uptime` | open | one number |
| `get_load_and_memory` | open | bounded metrics |
| `get_disk_usage` | open | configured mounts, sizes only |
| `list_units`, `unit_status` | open | a named, enumerable set |
| `list_packages`, `list_updates`, `preview_upgrade` | open | a named, enumerable set |
| `list_containers`, `container_status` | open | a named, enumerable set |
| `path_status` | **allowlisted** | unbounded namespace, host metadata |

This is a fixed design fact, not a config option: a new read must be classified
on purpose. `ALLOWLISTED_READS` in the server config names the allowlisted
reads, and a test pins every tool (`read_only_hint`) to an open read, an
allowlisted read or a mutation, so the classification cannot drift silently.

## `path_status` is metadata only

`[paths] allow` is the allowlist: an entry is an absolute path (exact match) or a
`re:` pattern anchored to the whole path. Empty by default, so every path read
is refused until the operator opts in. The tool:

- takes a path that must be absolute and free of `..` (rejected before matching,
  so a pattern cannot be used to escape with `..`);
- uses `lstat` (`symlink_metadata`), so the final component is never followed: a
  symlink cannot reach an unlisted target. Intermediate components are resolved
  by the kernel, as they are for any path;
- returns **metadata only** — never the contents, and never a directory listing;
- reports `exists: false` only on `ENOENT`; any other error (for instance
  `EACCES`) is an error, not a silent "absent".

`get_uptime` gained a `read_only_hint` it was missing, so every read is
annotated.

## Consequences

- The README's "no path" claim becomes "no command, no script, and no path the
  operator did not declare".
- `path_status` is a read, so a read-only key can call it — but only against the
  allowlisted paths, and with the tool grant plus `[paths] allow` both applying.
- Residual risk is documented in the threat model: the tool is a metadata
  disclosure surface, bounded by the allowlist.
