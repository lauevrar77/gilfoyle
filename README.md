# Gilfoyle

A secure [MCP](https://modelcontextprotocol.io) server, written in Rust, that exposes Linux
administration tools to an AI agent over **SSH**: reads are open, and the only mutations are
starting and stopping systemd units the operator has allowlisted.

Gilfoyle is built so that **it cannot execute anything**. There is no shell, no command
passthrough and no scripting: every tool is a compiled function with fixed behaviour, and
the only host interaction in the whole codebase is reading a handful of fixed files.

SSH is used purely as a **protocol**: the server serves an MCP subsystem and refuses every
`shell`, `exec`, `pty` and forwarding request. A connection can never obtain a shell.

> Status: early. Tools so far: `get_uptime`, `list_units`, `unit_status`, `start_unit`,
> `stop_unit`.

## Two roles

- **Node** — the Gilfoyle process on one host. It serves that host over SSH and exposes the
  compiled tools.
- **Client** — an aggregator that connects to several nodes and presents **one** MCP surface
  (over stdio) to the AI agent. Every tool takes a `node` argument; `all` fans a read out to
  every node. The client adds a `list_nodes` tool.

```
AI agent ──MCP(stdio)──▶ gilfoyle client ──SSH──▶ gilfoyle node @ host A
                        (aggregator)     ──SSH──▶ gilfoyle node @ host B
```

## Why it is safe by construction

- The domain core performs **no I/O** and has no access to the network, the filesystem or
  the process table.
- SSH is a protocol, not a shell: the server accepts only the `gilfoyle-mcp` subsystem and
  refuses `shell`, `exec`, `pty` and every forwarding request, in code.
- `unsafe` is forbidden workspace-wide in our crates, and `clippy::disallowed_methods`
  makes any reference to `std::process::Command` a **compile error**.
- systemd and PackageKit are reached over **in-process D-Bus**, never by spawning
  `systemctl` or `apt`.
- Authentication is **public key only** (`none`, `password` and `keyboard-interactive` are
  refused). Each key may carry a **tool grant**; a read-only key cannot reach a mutation.
- Mutations are **default-deny**: `start_unit` and `stop_unit` act only on units listed in
  `[systemd] manage_units`, `refresh_package_cache` and `upgrade_packages` act only on the
  operations listed in `[packagekit] allow`, and polkit is the authority behind them.
- **No install, no remove, by construction**: the package operation vocabulary is a closed
  enum with no install/remove variant, and an upgrade that would install or remove a package
  is refused.
- Configuration is fail-closed: no `bind`, no key or no host key means no start.
- The client **pins** each node's host-key fingerprint; a mismatch fails the connection.

## Architecture

Hexagonal (ports and adapters). Dependencies point inward, enforced by the Cargo dependency
graph.

| Crate | Role | I/O |
|---|---|---|
| `gilfoyle-core` | domain types, ports, use cases | none |
| `gilfoyle-system` | driven adapter (host uptime) | read-only |
| `gilfoyle-systemd` | driven adapter (systemd over D-Bus) | system bus |
| `gilfoyle-packagekit` | driven adapter (PackageKit over D-Bus) | system bus |
| `gilfoyle-ssh` | driving adapter: embedded SSH server (russh) + auth/channel policy | network |
| `gilfoyle-client` | driving adapter: multi-node aggregator | network |
| `gilfoyle-server` | MCP handler, config, wiring + the `gilfoyle` binary | network |

## Requirements

**Build.**

- Rust **1.89** or newer (the pinned toolchain in `.tool-versions` is a recent nightly).
- A C toolchain: the SSH crypto backend is `ring`, so the build is not pure Rust and
  releases are dynamically linked (the old static-musl artifact is gone).

**Run (node).**

- A Linux host with **systemd** and an accessible system D-Bus (needed by the unit tools).
- `/proc/uptime` for the uptime tool.
- **polkit** (`polkitd`) — only to authorise `start_unit`/`stop_unit`. Reads need none.
- **PackageKit** *and a backend* (APT on Debian/Ubuntu, DNF on Rocky) — only for the package
  tools. It is not installed by default on a minimal host; when absent the package tools
  report a clear error. A backend package such as `packagekit` (plus `apt`/`dnf`) must be
  present and `packagekitd` reachable on the system bus.
- The system bus, polkit and PackageKit are reached over `AF_UNIX`; the hardened unit
  already allows it.

**Run (client).**

- Nothing beyond the `gilfoyle` binary: the client speaks SSH with its own pinned host-key
  fingerprints. A C crypto backend (`ring`) is linked in.

## Quickstart (node)

```sh
make build                                  # debug build of the workspace

# Installing on a host, generating the host key and authorising clients is documented
# step by step in docs/INSTALL.md. In short, after install:
#   sudo gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
# prints the SHA256 fingerprint that clients pin; client public keys go in [[keys]].
```

## Node configuration (`/etc/gilfoyle/config.toml`)

Configuration is a TOML file (default `/etc/gilfoyle/config.toml`, override with `--config`).
Environment variables override file values.

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
]

[systemd]
# Units that may be started or stopped. Empty (the default) means none may.
manage_units = ["nginx.service", "myapp.service"]

[packagekit]
# Mutating package operations that may be performed. Empty (the default) means
# none may. There is deliberately no "install" or "remove": gilfoyle never
# installs or removes a package.
allow = ["refresh", "upgrade"]
# An upgrade is refused when its plan would install, remove, obsolete or
# downgrade a package. Set this to true to permit such transitive changes.
allow_transitive_changes = false
# Timeout for a whole package transaction, in seconds (default 600).
poll_timeout_seconds = 600
# Maximum number of package entries returned in a report (default 200).
default_limit = 200

[[keys]]
id = "nestor"
public_key = "ssh-ed25519 AAAA..."
# tools omitted → inherits [tools] enabled; present → narrows it (intersection)
```

### Authentication and tool grants

Each `[[keys]]` entry is a client public key. The key's `id` is the **principal** used in
logs. The accepted algorithms are ed25519, ecdsa-sha2 and rsa-sha2; `ssh-rsa` (SHA-1) is
refused. A key with a `tools` list may call only the intersection of that list and
`[tools] enabled`; an unknown tool name fails the configuration. A key without `tools`
inherits the global set. Rotating or revoking a key requires a restart.

### Environment overrides

| Variable | Effect |
|---|---|
| `GILFOYLE_BIND` | overrides `bind` |
| `GILFOYLE_HOST_KEY` | overrides `host_key` |
| `GILFOYLE_LOG` | overrides `log_level` |

## Client configuration

A suggested path is `~/.config/gilfoyle/client.toml`; pass it to the client with
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

Run the client with `gilfoyle client --config <path>` (or `GILFOYLE_CLIENT_CONFIG`). It
serves MCP on stdio, holds `pool_size` sessions per node, marks unreachable nodes, and
retries reads (never mutations) across reconnections. A node that is down at startup stays
visible in `list_nodes` and errors on call.

## Tools

### `get_uptime`

Returns the uptime of the **host machine** (from `/proc/uptime`, i.e. since kernel boot).
Takes no arguments.

```json
{ "uptime_seconds": 1048545.67, "uptime_human": "12d 3h 15m 45s" }
```

### systemd tools

systemd is reached over **in-process D-Bus** (`org.freedesktop.systemd1`); gilfoyle never
spawns `systemctl`.

| Tool | Arguments | Effect |
|---|---|---|
| `list_units` | `active_state?`, `name_contains?`, `unit_type?`, `limit?` | list units (read-only) |
| `unit_status` | `name` | detailed status of one unit (read-only) |
| `start_unit` | `name` | start a **managed** unit |
| `stop_unit` | `name` | stop a **managed** unit |

Unit names may be given short (`nginx`) or fully qualified (`nginx.service`).
`start_unit` and `stop_unit` wait for the systemd job to finish and report its result.

Mutations are **default-deny**: they act only on units listed in `[systemd] manage_units`,
and polkit is the authority behind them. Reads may target any unit.

### Package tools

Packages are managed through **PackageKit** over the system D-Bus
(`org.freedesktop.PackageKit`), in-process and without spawning anything. That makes the
same frontend work on APT (Debian) and DNF (Rocky). Gilfoyle never installs or removes a
package: the operation vocabulary is closed in code, and `apt upgrade` is expressed as an
update of existing packages only.

| Tool | Arguments | Effect |
|---|---|---|
| `list_packages` | `name_contains?`, `limit?` | list installed packages and versions (read-only) |
| `list_updates` | `min_severity?`, `limit?` | list available updates (read-only) |
| `preview_upgrade` | — | simulate an upgrade and report what it would do (read-only) |
| `refresh_package_cache` | `force?` (default `true`) | refresh the metadata cache, i.e. `apt update` |
| `upgrade_packages` | — | apply available upgrades, i.e. `apt upgrade` |

Reads (`list_packages`, `list_updates`, `preview_upgrade`) are open. The two mutations are
**default-deny**: they act only when `[packagekit] allow` names them, and polkit is the
authority behind them.

`preview_upgrade` and the gate inside `upgrade_packages` use PackageKit's `SIMULATE` flag,
which PackageKit authorises without polkit and which never changes anything. If the plan
would install, remove, obsolete or downgrade a package, `upgrade_packages` refuses unless
`allow_transitive_changes = true`. Untrusted packages (missing signature, required signing
key, EULA) are always refused: gilfoyle never answers `InstallSignature` or `AcceptEula`.

### Through the client

The client adds a required `node` argument to every tool (a label, or `all` for the read
tools `get_uptime`, `list_units`, `unit_status`, `list_packages`, `list_updates` and
`preview_upgrade`). `all` returns a per-node map, with a per-node error for any node that
failed. A `list_nodes` tool reports each node's label, reachability and exposed tools.

## Authorising unit control (polkit)

`start_unit` and `stop_unit` require the `org.freedesktop.systemd1.manage-units`
authorisation. Gilfoyle runs unprivileged and never escalates, so **polkit must be installed
and running** for these two tools to work — without it systemd refuses the call.

```sh
# Debian/Ubuntu; on other distributions use the equivalent polkit daemon package.
sudo apt-get install -y polkitd

# Install the rule generated from your configuration.
gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
```

The rule grants the `gilfoyle` user the right to start and stop only the units in
`[systemd] manage_units`. Re-run the command whenever that list changes; gilfoyle warns at
startup when the installed rule is missing or out of sync.

## Authorising package operations (polkit)

`refresh_package_cache` needs `org.freedesktop.packagekit.system-sources-refresh` and
`upgrade_packages` needs `org.freedesktop.packagekit.system-update`. PackageKit asks polkit
for these, but the stock defaults require an interactive administrative authentication,
which a headless system service cannot satisfy, so the operator installs a generated rule:

```sh
gilfoyle packagekit polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules > /dev/null
```

The rule grants the `gilfoyle` user only the actions named in `[packagekit] allow`, and
explicitly refuses installing, reinstalling, downgrading, removing packages, trusting
signing keys and upgrading the whole distribution. Gilfoyle warns at startup when the
installed rule is missing or out of sync. PackageKit only exists on hosts that ship it (and
its backend); on a minimal host the package tools report a clear "PackageKit is
unavailable" error.

## Threat model (summary)

**In scope.** An attacker who reaches the SSH port, or who compromises the gilfoyle process,
must not gain code execution or host privileges.

- **No code execution.** Tools are compiled functions; nothing accepts a command or path to
  run. The SSH handler refuses `shell`/`exec`/`pty` and all forwarding. systemd and
  PackageKit are reached over in-process D-Bus, never by spawning `systemctl` or `apt`.
- **No install, no remove, by construction.** The package operation vocabulary is a closed
  enum with no install/remove variant; upgrades carry `ONLY_TRUSTED`, and a plan that would
  install or remove a package is refused.
- **Least privilege.** Run as an unprivileged dedicated user (see the systemd unit);
  mutations are enforced twice (the tool's allowlist and the polkit rule).
- **Auth.** Public keys only, mapped to a principal; per-key tool grants limit what a key
  can reach. The client pins host-key fingerprints, so SSH is not open to MITM.
- **Fail-closed.** No bind, no key or no host key means no start; no allowed package
  operation means no refresh or upgrade.

**Out of scope for now.** Rate limiting is left to the firewall and systemd; key rotation
requires a restart; OpenSSH certificates are not supported. The SSH transport adds a
pre-authentication parser surface and a C crypto backend (`ring`) — a deliberate trade.

## Quality

```sh
make fmt-check   # formatting
make lint        # clippy -D warnings
make test        # unit + integration tests
make mutants     # cargo-mutants; the score must be 100%
make fuzz        # cargo-fuzz targets on nightly with ASan
make check       # all of the above except fuzzing
make install-hooks
```

The mutation score is held at **100%**. Excluded: the composition root (`main.rs`) and the
thin `russh`/`rmcp` transport glue (`gilfoyle-ssh`'s adapter and hostkey, `gilfoyle-client`'s
session wiring, the server's `serve.rs`); their decisions live in pure, fully-tested modules.
The systemd and PackageKit adapters are tested against **in-process mock buses**
(peer-to-peer socket pairs), so no daemon or package manager is needed.

## Installing on a host

The full procedure — a fresh install, the **migration from the old HTTP build**, and a
copy-paste prompt for an agent — lives in [docs/INSTALL.md](docs/INSTALL.md). The short
version:

```sh
cargo build --release -p gilfoyle-server --bin gilfoyle

sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle
sudo install -o root -g root -m 0755 target/release/gilfoyle /usr/local/bin/gilfoyle
sudo install -d -o root -g gilfoyle -m 0750 /etc/gilfoyle
sudo /usr/local/bin/gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
sudo chown gilfoyle:gilfoyle /etc/gilfoyle/ssh_host_ed25519_key   # the service reads it
# write /etc/gilfoyle/config.toml (0640 root:gilfoyle) — see Node configuration above
sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service
sudo systemctl daemon-reload && sudo systemctl enable --now gilfoyle
gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
# optional: packages, if [packagekit] allow is set and packagekit is installed
gilfoyle packagekit polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules > /dev/null
```

`packaging/gilfoyle.service` is a hardened unit (dedicated `gilfoyle` user, `NoNewPrivileges`,
`ProtectSystem=strict`, empty capability set, `SystemCallFilter`). The system bus is reached
over `AF_UNIX`, which the unit already allows.

## Releases

Tagged releases ship a **dynamically linked GNU** binary, a checksum and the deployment
files:

<https://forge.evrard.online:3000/laurent/gilfoyle/releases>

Build a release artifact with:

```sh
make package-release            # writes dist/gilfoyle-<version>-x86_64-unknown-linux-gnu.tar.gz
```

The tarball holds the `gilfoyle` binary, `gilfoyle.service` and the example polkit rules
(units and packages), plus `README.md` and `INSTALL.md`. Tag the commit (`git tag vX.Y.Z`),
create the release, and attach the `.tar.gz` and its `.sha256`. The client uses the same
binary (`gilfoyle client`).

## License

Licensed under either of Apache-2.0 or MIT, at your option.
