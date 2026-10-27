# AGENTS.md

Gilfoyle is a Rust MCP server exposing Linux administration tools over Streamable HTTP,
authenticated by bearer token. There is no shell here: tools are compiled functions with
fixed behaviour. Reads are open; the only mutations are starting and stopping systemd
units, and only for units the operator has allowlisted.

## Architecture: dependencies point inward

Four crates, and the dependency graph is the enforcement.

- `gilfoyle-core` — pure. Domain types, ports (traits), use cases. No I/O, no async runtime,
  no `rmcp`, no `axum`, no `zbus`.
- `gilfoyle-system` — driven adapter for the host uptime (`/proc/uptime`).
- `gilfoyle-systemd` — driven adapter for systemd, over in-process D-Bus.
- `gilfoyle-server` — driving adapters (MCP tools, HTTP router, auth, config) and the
  `gilfoyle` binary.

Never import an adapter from the core. A capability is a narrow port (`UptimeSource`,
`ServiceCatalog`, `ServiceControl`); add one trait per capability rather than widening an
existing one.

## Invariants

- Tools are compiled functions. Nothing accepts a command, script or path to execute.
- I/O lives only in adapters. systemd is reached over D-Bus in-process, never by spawning
  `systemctl`.
- `unsafe` is forbidden workspace-wide, and `clippy::disallowed_methods` rejects
  `std::process::Command` at compile time.
- Mutations are default-deny: `start_unit` and `stop_unit` only act on units listed in
  `[systemd] manage_units`, and polkit is the authority behind them.
- Secrets never reach logs or error messages, and token comparison is constant time.
- Configuration is fail-closed: no token means no start, and a non-loopback bind requires
  `allow_public = true`.

## Quality gate

`make check` runs `fmt --check`, `clippy -D warnings`, the tests and the mutation gate. The
mutation score stays at 100%: a surviving mutant fails the run. Add `#[mutants::skip]` only
for I/O that cannot be simulated, always with a written justification. The systemd adapter
is covered by an in-process mock bus (a peer-to-peer socket pair), not by a real daemon.

## Adding a tool

1. Model the result in `gilfoyle-core` and add a method to the driving port.
2. Implement the read or mutation in a driven adapter behind a narrow port.
3. Translate it in the `#[tool_router]` impl in `gilfoyle-server` (a few lines, no logic).
4. Cover it with unit tests; extend the HTTP integration test when the wire shape changes.

## Layout and docs

- `README.md` — what gilfoyle is, configuration, usage, threat model.
- `GLOSSARY.md` — the domain vocabulary; keep it free of implementation detail.
- `docs/adr/` — decisions worth remembering.
- `Makefile` — every task; run `make help`.
- `fuzz/` — an independent workspace; `make fuzz` runs it on nightly with ASan.
