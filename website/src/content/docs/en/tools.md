---
title: Tools
description: The full tool vocabulary, arguments and effects.
order: 5
lang: en
---

This is the whole vocabulary. If a tool is not on this list, it does not exist.

## Host

| Tool | Arguments | Effect |
|---|---|---|
| `get_uptime` | — | host uptime from `/proc/uptime` (since kernel boot) |

```json
{ "uptime_seconds": 1048545.67, "uptime_human": "12d 3h 15m 45s" }
```

## systemd

Reached over **in-process D-Bus** (`org.freedesktop.systemd1`); Gilfoyle never spawns
`systemctl`.

| Tool | Arguments | Effect |
|---|---|---|
| `list_units` | `active_state?`, `name_contains?`, `unit_type?`, `limit?` | list units (read-only) |
| `unit_status` | `name` | detailed status of one unit (read-only) |
| `start_unit` | `name` | start a **managed** unit |
| `stop_unit` | `name` | stop a **managed** unit |

Unit names may be given short (`nginx`) or fully qualified (`nginx.service`). `start_unit`
and `stop_unit` wait for the systemd job to finish and report its result. Mutations act only
on units listed in `[systemd] manage_units`, and polkit is the authority behind them.

## Containers

Docker is reached over its **UNIX socket** with an in-process HTTP client; Gilfoyle never
spawns `docker`. There is deliberately **no create and no remove**: the operation set is
list, inspect, start, stop and restart.

| Tool | Arguments | Effect |
|---|---|---|
| `list_containers` | `state?`, `name_contains?`, `limit?` | list containers (read-only) |
| `container_status` | `container` | detailed status of one container (read-only) |
| `start_container` | `container` | start a **managed** container |
| `stop_container` | `container` | stop a **managed** container |
| `restart_container` | `container` | restart a **managed** container |

`container` is a container name or id. The three mutations are **default-deny**: they act
only on containers whose name matches `[docker] manage_containers`, and the action runs on
the resolved container id, so a rename between the check and the action cannot retarget it.

## Packages

Reached through **PackageKit** over the system D-Bus (`org.freedesktop.PackageKit`),
in-process and without spawning anything. The same frontend works on APT (Debian) and DNF
(Rocky). Gilfoyle never installs or removes a package.

| Tool | Arguments | Effect |
|---|---|---|
| `list_packages` | `name_contains?`, `limit?` | list installed packages and versions (read-only) |
| `list_updates` | `min_severity?`, `limit?` | list available updates (read-only) |
| `preview_upgrade` | — | simulate an upgrade and report what it would do (read-only) |
| `refresh_package_cache` | `force?` (default `true`) | refresh the metadata cache, i.e. `apt update` |
| `upgrade_packages` | — | apply available upgrades, i.e. `apt upgrade` |

`preview_upgrade` and the gate inside `upgrade_packages` use PackageKit's `SIMULATE` flag,
which never changes anything. If the plan would install, remove, obsolete or downgrade a
package, `upgrade_packages` refuses unless `allow_transitive_changes = true`. Untrusted
packages are always refused.

## Through the client

The client adds a required `node` argument to every tool (a label, or `all` for the read
tools). `all` returns a per-node map, with a per-node error for any node that failed. A
`list_nodes` tool reports each node's label, reachability and exposed tools.
