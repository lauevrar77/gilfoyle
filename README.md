# Gilfoyle

A secure [MCP](https://modelcontextprotocol.io) server, written in Rust, that exposes
read-only Linux administration tools to an AI agent over Streamable HTTP.

Gilfoyle is built so that **it cannot execute anything**. There is no shell, no command
passthrough and no scripting: every tool is a compiled function with fixed behaviour,
and the only host interaction in the whole codebase is reading a handful of fixed
files.

> Status: early. One tool so far (`get_uptime`); the architecture is in place for more.

## Why it is safe by construction

- The domain core performs **no I/O** and has no access to the network, the filesystem
  or the process table.
- `unsafe` is forbidden workspace-wide, and `clippy::disallowed_methods` makes any
  reference to `std::process::Command` a **compile error**.
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

- Rust **1.88** or newer; the pinned toolchain in `.tool-versions` is a recent
  nightly (managed with [asdf](https://asdf-vm.com)), which also runs the fuzzers.
- A Linux host (`/proc/uptime`).

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
enabled = ["get_uptime"]          # omit the section to expose every compiled tool

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
The composition root (`main.rs`) is excluded; everything else must be caught.

## Packaging

`packaging/gilfoyle.service` is a hardened systemd unit (dedicated `gilfoyle` user,
`NoNewPrivileges`, `ProtectSystem=strict`, empty capability set, `SystemCallFilter`,
and `/proc/uptime` as the only readable host path).

## License

Licensed under either of Apache-2.0 or MIT, at your option.
