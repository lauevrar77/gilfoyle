# Gilfoyle

A secure [MCP](https://modelcontextprotocol.io) server, written in Rust, that exposes Linux
administration tools to an AI agent over Streamable HTTP: reads are open, and the only
mutations are starting and stopping systemd units the operator has allowlisted.

Gilfoyle is built so that **it cannot execute anything**. There is no shell, no command
passthrough and no scripting: every tool is a compiled function with fixed behaviour,
and the only host interaction in the whole codebase is reading a handful of fixed
files.

> Status: early. Tools so far: `get_uptime`, `list_units`, `unit_status`, `start_unit`,
> `stop_unit`.

## Why it is safe by construction

- The domain core performs **no I/O** and has no access to the network, the filesystem
  or the process table.
- `unsafe` is forbidden workspace-wide, and `clippy::disallowed_methods` makes any
  reference to `std::process::Command` a **compile error**.
- systemd is reached over **in-process D-Bus**, never by spawning `systemctl`.
- Mutations are **default-deny**: `start_unit` and `stop_unit` act only on units listed in
  `[systemd] manage_units`, and polkit is the authority behind them.
- Every request must carry a bearer token; tokens are stored as SHA-256 digests and
  compared in constant time.
- Configuration is fail-closed, and the server binds to loopback unless the operator
  explicitly opts into a public bind.

## Architecture

Hexagonal (ports and adapters). Dependencies point inward, enforced by the Cargo
dependency graph.

```
        ┌─────────────────── driving adapters ───────────────────┐
client ─┤  gilfoyle-server: MCP handler · HTTP · auth · config   │
        └───────────────────────────┬────────────────────────────┘
                                    │  driving port: SystemAdmin
        ┌───────────────────────────▼────────────────────────────┐
        │  gilfoyle-core (pure): domain · ports · use cases      │
        └───────────────────────────▲────────────────────────────┘
                                    │  driven port: UptimeSource
        ┌───────────────────────────┴────────────────────────────┐
        │  gilfoyle-system: ProcUptime (reads /proc/uptime)      │
        └────────────────────────────────────────────────────────┘
```

| Crate | Role | I/O |
|---|---|---|
| `gilfoyle-core` | domain types, ports, use cases | none |
| `gilfoyle-system` | driven adapters (host probes) | read-only |
| `gilfoyle-server` | driving adapters + `gilfoyle` binary | network |

## Requirements

- Rust **1.89** or newer; the pinned toolchain in `.tool-versions` is a recent
  nightly (managed with [asdf](https://asdf-vm.com)), which also runs the fuzzers.
- A Linux host: `/proc/uptime` for the uptime tool, and a system D-Bus (`systemd`) for the
  unit tools.

## Quickstart

```sh
# Install the pinned toolchain
asdf plugin add rust https://github.com/asdf-community/asdf-rust.git
asdf install

# Build
make build

# Derive a token hash from a secret (reads the secret from stdin)
printf 'my-long-random-secret' | cargo run -p gilfoyle-server --bin gilfoyle -- hash-token
```

## Configuration

Configuration is a TOML file (default `/etc/gilfoyle/config.toml`, override with
`--config`). Environment variables override file values.

```toml
bind = "127.0.0.1:8765"          # default; non-loopback needs allow_public = true
allow_public = false
allowed_hosts = ["127.0.0.1"]     # Host header allowlist for the MCP transport
allowed_origins = []              # empty disables Origin validation
log_level = "info"                # tracing filter, JSON logs on stderr

[tools]
enabled = ["get_uptime", "list_units", "unit_status", "start_unit", "stop_unit"]

[systemd]
# Units that may be started or stopped. Empty (the default) means none may.
manage_units = ["nginx.service", "myapp.service"]

[[tokens]]
id = "admin"
# Provide the secret in one of three ways (see below); never commit a plaintext token.
```

### Token provisioning

Each `[[tokens]]` entry is resolved from the first source that is present:

1. `sha256` inline — 64 hex characters, produced by `gilfoyle hash-token`.
2. `GILFOYLE_TOKEN_<ID>` environment variable — the plaintext secret, hashed at startup
   (kept in memory only).
3. A hash file at `<token_dir>/<id>` (default `/etc/gilfoyle/tokens/<id>`), mode `0600`,
   containing 64 hex characters.

### Environment overrides

| Variable | Effect |
|---|---|
| `GILFOYLE_BIND` | overrides `bind` |
| `GILFOYLE_LOG` | overrides `log_level` |
| `GILFOYLE_TOKEN_<ID>` | supplies a token secret (plaintext) |

## Usage

```sh
# Health check (public, secret-free)
curl -s http://127.0.0.1:8765/healthz

# MCP handshake
curl -s -X POST http://127.0.0.1:8765/mcp \
  -H 'Authorization: Bearer my-long-random-secret' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
```

## Tools

### `get_uptime`

Returns the uptime of the **host machine** (from `/proc/uptime`, i.e. since kernel
boot — not the uptime of the gilfoyle process). Takes no arguments.

```json
{
  "uptime_seconds": 1048545.67,
  "uptime_human": "12d 3h 15m 45s"
}
```

The same value is also returned as text content alongside the structured content.

### systemd tools

systemd is reached over **in-process D-Bus** (`org.freedesktop.systemd1`); gilfoyle never
spawns `systemctl`.

| Tool | Arguments | Effect |
|---|---|---|
| `list_units` | `active_state?`, `name_contains?`, `unit_type?`, `limit?` | list units (read-only) |
| `unit_status` | `name` | detailed status of one unit (read-only) |
| `start_unit` | `name` | start a **managed** unit |
| `stop_unit` | `name` | stop a **managed** unit |

Unit names may be given short (`nginx`) or fully qualified (`nginx.service`). `start_unit`
and `stop_unit` wait for the systemd job to finish and report its result.

Mutations are **default-deny**: they act only on units listed in `[systemd] manage_units`,
and polkit is the authority behind them. Reads may target any unit.

### Authorising unit control (polkit)

`start_unit` and `stop_unit` require the `org.freedesktop.systemd1.manage-units`
authorisation. Gilfoyle runs unprivileged and never escalates, so **polkit must be
installed and running** for these two tools to work — without it, systemd refuses the call
and the tools report a clear "not authorised" error. Reads (`list_units`, `unit_status`)
work without polkit.

```sh
# Debian/Ubuntu; on other distributions use the equivalent polkit daemon package.
sudo apt-get install -y polkitd

# Install the rule generated from your configuration.
gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
```

The rule grants the `gilfoyle` user the right to start and stop only the units in
`[systemd] manage_units`. Re-run the command whenever that list changes; gilfoyle warns at
startup when the installed rule is missing or out of sync. Note that the rules directory is
typically `root:polkitd 0750`, so gilfoyle may not be able to read it back — a missing rule
is reported, an unreadable one is only logged at debug level.

## Threat model (summary)

**In scope.** An attacker who reaches the HTTP endpoint, or who compromises the
gilfoyle process, must not gain code execution or host privileges.

- **No code execution.** Tools are compiled functions; nothing accepts a command or
  path to run. Enforced by `clippy::disallowed_methods` and `forbid(unsafe_code)`.
- **Least privilege.** Run as an unprivileged dedicated user (see the systemd unit).
  Future mutation tools will delegate to a separate, narrowly-scoped privileged helper
  behind a port rather than escalating in-process.
- **Auth.** Bearer token per request, SHA-256 + constant-time comparison, no secret in
  logs or error messages. Legacy MCP session ids are bound to the token that created
  them, preventing session hijacking.
- **Fail-closed.** No configured token, no start; non-loopback bind requires explicit
  opt-in.

**Out of scope for now.** TLS/mTLS and rate limiting are expected to be terminated by a
reverse proxy in front of gilfoyle. Migration to MCP OAuth 2.1 is planned.

## Quality

```sh
make fmt-check   # formatting
make lint        # clippy -D warnings
make test        # unit + HTTP integration tests
make mutants     # cargo-mutants; the score must be 100%
make fuzz        # cargo-fuzz targets on nightly with ASan
make check       # all of the above except fuzzing
make install-hooks  # git pre-commit hook (fmt + clippy + tests)
```

The mutation score is held at **100%**: a single surviving mutant fails `make mutants`.
The composition root (`main.rs`) is excluded; everything else must be caught. The systemd
adapter is tested against an **in-process mock bus** (a peer-to-peer socket pair), so no
daemon or real unit is needed.

## Installing on a host

```sh
# 1. Build
cargo build --release -p gilfoyle-server --bin gilfoyle

# 2. Dedicated unprivileged user
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle

# 3. Binary and configuration
sudo install -o root -g root -m 0755 target/release/gilfoyle /usr/local/bin/gilfoyle
sudo install -d -o root -g gilfoyle -m 0750 /etc/gilfoyle
# config.toml (0640 root:gilfoyle) — see Configuration above
# env (0600 root:root) with GILFOYLE_TOKEN_<ID>=<secret>

# 4. Service
sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service
sudo systemctl daemon-reload
sudo systemctl enable --now gilfoyle

# 5. polkit, to authorise start/stop (see above)
sudo apt-get install -y polkitd
gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
```

`packaging/gilfoyle.service` is a hardened unit (dedicated `gilfoyle` user,
`NoNewPrivileges`, `ProtectSystem=strict`, empty capability set, `SystemCallFilter`, and
`/proc/uptime` as the only readable host path). The system bus is reached over `AF_UNIX`,
which the unit already allows.

## Releases

Tagged releases ship a **static musl** binary, a checksum, and the deployment files:

<https://forge.evrard.online:3000/laurent/gilfoyle/releases>

```sh
TAG=v0.1.0
PKG=gilfoyle-0.1.0-x86_64-unknown-linux-musl
BASE=http://forge.evrard.online:3000/laurent/gilfoyle/releases/download/$TAG

curl -LO "$BASE/$PKG.tar.gz"
curl -LO "$BASE/$PKG.tar.gz.sha256"
sha256sum -c "$PKG.tar.gz.sha256"
tar xzf "$PKG.tar.gz" && cd "$PKG"
# the tarball holds the binary, the systemd unit, an example polkit rule and the docs;
# then follow "Installing on a host" above
```

## Installing with an agent

Hand the prompt below to an agent that has shell access (with `sudo`) on the target host.
Fill in the parameter block first; the agent will ask about anything left as `<...>`.

```text
You are installing gilfoyle, a secure MCP server, on this Linux host. Follow the steps
exactly, use sudo where needed, and stop and show me the error if anything fails.

Parameters (ask me if a value is missing):
  RELEASE_TAG   = v0.1.0
  PKG           = gilfoyle-0.1.0-x86_64-unknown-linux-musl
  RELEASE_BASE  = http://forge.evrard.online:3000/laurent/gilfoyle/releases/download/<RELEASE_TAG>
  BIND          = <address to listen on, e.g. 127.0.0.1:8765 or 100.x.y.z:8765>
  ALLOWED_HOSTS = <Host header values clients will use, with and without the port>
  ALLOW_PUBLIC  = <true if BIND is not a loopback address, otherwise false>
  TOKEN_ID      = <name for the client token, e.g. nestor>
  MANAGED_UNITS = <systemd units that may be started or stopped, e.g. docker.service;
                   leave empty for read-only>

Steps:
1. Download $PKG.tar.gz and its .sha256 into a temporary directory, verify the checksum
   with `sha256sum -c`, and unpack it.
2. Create the dedicated unprivileged user:
   sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle
3. Install the binary: sudo install -o root -g root -m 0755 gilfoyle /usr/local/bin/gilfoyle
   and check `gilfoyle --version`.
4. Create /etc/gilfoyle (0750 root:gilfoyle). Generate the secret with `openssl rand -hex 32`
   and write /etc/gilfoyle/env (0600 root:root) containing
   GILFOYLE_TOKEN_<TOKEN_ID uppercased>=<secret>.
   Write /etc/gilfoyle/config.toml (0640 root:gilfoyle) with:
     bind, allow_public, allowed_hosts, allowed_origins = [], log_level = "info",
     [systemd] manage_units = MANAGED_UNITS,
     [[tokens]] id = "<TOKEN_ID>".
5. Install gilfoyle.service to /etc/systemd/system/ (0644), run `systemctl daemon-reload`,
   then `systemctl enable --now gilfoyle`, and show `systemctl status gilfoyle`.
6. If MANAGED_UNITS is not empty: install polkit if missing
   (`apt-get install -y polkitd` on Debian/Ubuntu), then install the rule with
   `gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules`.
7. Verify: `curl -s http://<BIND>/healthz` must return {"status":"ok"}; then perform an
   authenticated MCP `initialize` (Authorization: Bearer <secret>) and a `tools/list`, and
   confirm the five tools: get_uptime, list_units, unit_status, start_unit, stop_unit.
8. Report back the MCP URL, the tool list, and the token exactly once so I can store it.
   Never log the token anywhere else, and never run gilfoyle as root.
```

## Packaging

`packaging/gilfoyle.service` is a hardened systemd unit (dedicated `gilfoyle` user,
`NoNewPrivileges`, `ProtectSystem=strict`, empty capability set, `SystemCallFilter`,
and `/proc/uptime` as the only readable host path).

## License

Licensed under either of Apache-2.0 or MIT, at your option.
