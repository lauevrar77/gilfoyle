---
title: Configuration
description: The node and client config files, keys, and tool grants.
order: 4
lang: en
---

Configuration is fail-closed: no `bind`, no key or no host key means no start. It is a TOML
file (default `/etc/gilfoyle/config.toml`, override with `--config`). Environment variables
override file values.

## Node

```toml
bind = "10.0.0.5:2222"                    # mandatory, address AND port
host_key = "/etc/gilfoyle/ssh_host_ed25519_key"
max_connections = 32
max_channels_per_connection = 8
log_level = "info"                        # tracing filter, JSON logs on stderr

[tools]
enabled = [
  "get_uptime",
  "list_units", "unit_status", "start_unit", "stop_unit",
  "list_packages", "list_updates", "preview_upgrade",
  "refresh_package_cache", "upgrade_packages",
  "list_containers", "container_status", "start_container", "stop_container", "restart_container",
]

[systemd]
# Units that may be started or stopped. Empty (the default) means none may.
manage_units = ["nginx.service", "myapp.service"]

[packagekit]
# Mutating package operations that may be performed. Empty means none may.
# There is deliberately no "install" or "remove".
allow = ["refresh", "upgrade"]
allow_transitive_changes = false
poll_timeout_seconds = 600
default_limit = 200

[docker]
# Path to the Docker daemon socket (default /var/run/docker.sock). Only a UNIX
# socket path is accepted; a TCP endpoint is refused.
endpoint = "/var/run/docker.sock"
# Containers that may be started, stopped or restarted. Empty (the default)
# means none may. An entry is an exact name, or a `re:` regular expression
# anchored to the whole name.
manage_containers = ["web", "re:^worker-[0-9]+$"]
call_timeout_seconds = 10
stop_timeout_seconds = 30
default_limit = 100

[[keys]]
id = "nestor"
public_key = "ssh-ed25519 AAAA..."
# tools omitted → inherits [tools] enabled; present → narrows it (intersection)
```

### Authentication and tool grants

Each `[[keys]]` entry is a client public key. The key's `id` is the **principal** used in
logs. The accepted algorithms are ed25519, ecdsa-sha2 and rsa-sha2; `ssh-rsa` (SHA-1) is
refused.

A key with a `tools` list may call only the intersection of that list and `[tools] enabled`;
an unknown tool name fails the configuration. A key without `tools` inherits the global set.
Rotating or revoking a key requires a restart.

### Environment overrides

| Variable | Effect |
|---|---|
| `GILFOYLE_BIND` | overrides `bind` |
| `GILFOYLE_HOST_KEY` | overrides `host_key` |
| `GILFOYLE_LOG` | overrides `log_level` |

## Client

A suggested path is `~/.config/gilfoyle/client.toml`; pass it with
`gilfoyle client --config <path>` (or `GILFOYLE_CLIENT_CONFIG`).

```toml
pool_size = 4

[[nodes]]
label = "web1"
host = "10.0.0.5"
port = 2222
key = "~/.ssh/gilfoyle_web1"              # unencrypted private key, mode 0600
host_key_fingerprint = "SHA256:..."       # from `gilfoyle ssh keygen`
```

The client serves MCP on stdio, holds `pool_size` sessions per node, marks unreachable
nodes, and retries reads (never mutations) across reconnections. A node that is down at
startup stays visible in `list_nodes` and errors on call.
