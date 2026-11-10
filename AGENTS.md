# AGENTS.md

Gilfoyle is a Rust MCP server exposing Linux administration tools over SSH, authenticated by
public key. There is no shell here: tools are compiled functions with fixed behaviour. Reads
are open; the only mutations are starting and stopping allowlisted systemd units and
refreshing or upgrading packages (never installing or removing them), for operations the
operator has allowlisted.

## Architecture: dependencies point inward

Eight crates, and the dependency graph is the enforcement.

- `gilfoyle-core` — pure. Domain types, ports (traits), use cases. No I/O, no async runtime,
  no `rmcp`, no `russh`, no `zbus`.
- `gilfoyle-system` — driven adapter for the host uptime (`/proc/uptime`).
- `gilfoyle-systemd` — driven adapter for systemd, over in-process D-Bus.
- `gilfoyle-packagekit` — driven adapter for PackageKit, over in-process D-Bus.
- `gilfoyle-docker` — driven adapter for Docker, over its UNIX socket (in-process HTTP).
- `gilfoyle-ssh` — driving adapter: the embedded SSH server (`russh`), the pure
  auth/channel policy, and the MCP-over-channel bridge.
- `gilfoyle-client` — driving adapter: the multi-node aggregator (russh client + MCP proxy).
- `gilfoyle-server` — the MCP tool handler, config, wiring, and the `gilfoyle` binary.

Never import an adapter from the core. A capability is a narrow port (`UptimeSource`,
`ServiceCatalog`, `ServiceControl`); add one trait per capability rather than widening an
existing one.

## Invariants

- Tools are compiled functions. Nothing accepts a command, script or path to execute.
- SSH is a protocol, not a shell: only the `gilfoyle-mcp` subsystem is served; `shell`,
  `exec`, `pty` and all forwarding are refused in code.
- Authentication is public key only. A key's tool grant can only narrow `[tools] enabled`.
- I/O lives only in adapters. systemd and PackageKit are reached over D-Bus in-process, and
  Docker over its UNIX socket (in-process HTTP), never by spawning `systemctl`, `apt` or
  `docker`.
- `unsafe` is forbidden in our crates, and `clippy::disallowed_methods` rejects
  `std::process::Command` at compile time.
- Mutations are default-deny: `start_unit` and `stop_unit` only act on units listed in
  `[systemd] manage_units`, `start_container`/`stop_container`/`restart_container` only on
  containers listed in `[docker] manage_containers`, `refresh_package_cache` and
  `upgrade_packages` only act on the operations in `[packagekit] allow`. polkit is the
  authority behind systemd and PackageKit; Docker's socket is root-equivalent, so it is
  granted narrowly (a socket proxy) and the allowlist is the only in-process guard.
- Never install or remove a package: the operation vocabulary is a closed enum with no
  install/remove variant, every update carries `ONLY_TRUSTED`, and an upgrade whose plan
  would install or remove a package is refused (unless the operator opts into transitive
  changes). Docker likewise has no create or remove operation: the adapter can only list,
  inspect, start, stop and restart.
- Secrets never reach logs or error messages; client keys stay `0600` and host-key
  fingerprints are pinned by the client.
- Configuration is fail-closed: no `bind`, no key or no host key means no start.

## Quality gate

`make check` runs `fmt --check`, `clippy -D warnings`, the tests and the mutation gate. The
mutation score stays at 100%: a surviving mutant fails the run. The excluded glue is the
composition root (`main.rs`) and the thin `russh`/`rmcp`/`hyper` transport wiring
(`gilfoyle-ssh`'s `adapter`/`hostkey`, `gilfoyle-client`'s `node`/`lib`, the server's
`serve`, `gilfoyle-docker`'s `transport`) — their decisions live in pure, tested modules.
The systemd and PackageKit adapters are covered by in-process mock buses, the Docker adapter
by an in-process mock HTTP server on a UNIX socket, and the SSH transport by an in-process
`russh` client/server pair.

## Adding a tool

1. Model the result in `gilfoyle-core` and add a method to the driving port.
2. Implement the read or mutation in a driven adapter behind a narrow port.
3. Translate it in the `#[tool_router]` impl in `gilfoyle-server` (a few lines, no logic).
4. Cover it with unit tests; extend the SSH integration test when the wire shape changes.

If the tool is a read, add it to `gilfoyle-client`'s read set so it can be fanned out.

## Layout and docs

- `README.md` — what gilfoyle is, configuration, usage, threat model.
- `docs/INSTALL.md` — install and update a node; includes a prompt for an install agent.
- `GLOSSARY.md` — the domain vocabulary; keep it free of implementation detail.
- `docs/adr/` — decisions worth remembering (0005/0006 for the SSH transport and key
  authentication; 0007–0009 for the PackageKit surface and its no-install guarantee; 0010 for
  the Docker socket, its closed operation set and its regex allowlist).
- `Makefile` — every task; run `make help`.
- `fuzz/` — an independent workspace; `make fuzz` runs it on nightly with ASan.
