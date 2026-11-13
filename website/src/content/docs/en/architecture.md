---
title: Architecture
description: Seven crates, and why dependencies only point inward.
order: 7
lang: en
---

Gilfoyle is hexagonal (ports and adapters). Dependencies point inward, and the Cargo
dependency graph is the enforcement.

| Crate | Role | I/O |
|---|---|---|
| `gilfoyle-core` | domain types, ports, use cases | none |
| `gilfoyle-system` | driven adapter (host uptime) | read-only |
| `gilfoyle-systemd` | driven adapter (systemd over D-Bus) | system bus |
| `gilfoyle-packagekit` | driven adapter (PackageKit over D-Bus) | system bus |
| `gilfoyle-docker` | driven adapter (Docker over its UNIX socket) | socket |
| `gilfoyle-ssh` | driving adapter: embedded SSH server + auth/channel policy | network |
| `gilfoyle-client` | driving adapter: multi-node aggregator | network |
| `gilfoyle-server` | MCP handler, config, wiring + the `gilfoyle` binary | network |

Never import an adapter from the core. A capability is a narrow port (`UptimeSource`,
`ServiceCatalog`, `ServiceControl`); add one trait per capability rather than widening an
existing one.

## Adding a tool

1. Model the result in `gilfoyle-core` and add a method to the driving port.
2. Implement the read or mutation in a driven adapter behind a narrow port.
3. Translate it in the `#[tool_router]` impl in `gilfoyle-server` — a few lines, no logic.
4. Cover it with unit tests; extend the SSH integration test when the wire shape changes.

If the tool is a read, add it to `gilfoyle-client`'s read set so it can be fanned out.

## Quality

```sh
make fmt-check   # formatting
make lint        # clippy -D warnings
make test        # unit + integration tests
make mutants     # cargo-mutants; the score must be 100%
make check       # all of the above except fuzzing
```

The mutation score is held at **100%**: a surviving mutant fails the run. Excluded glue is
the composition root (`main.rs`) and the thin `russh`/`rmcp`/`hyper` transport wiring — their
decisions live in pure, tested modules. The systemd and PackageKit adapters are covered by
in-process mock buses, the Docker adapter by a mock HTTP server on a UNIX socket, and the SSH
transport by an in-process `russh` client/server pair.

The gate is split in CI: a pull request mutates only the lines it changes (`--in-diff`), and
the whole workspace runs in **8 shards** on a weekly schedule and on manual dispatch. Both
still require **100%**, but the full gate is not run on every push — so `make check`, which
runs it locally, is what to run before pushing.

Fuzzing (`make fuzz`) runs eight targets on nightly with AddressSanitizer: `uptime_parse`,
`authorized_key`, `config_toml`, `client_config`, `unit_name`, `package_id`, `container_ref`
and `container_allowlist`. `cargo-deny`
(`deny.toml`) denies unknown registries and git sources, keeps an empty advisory ignore list,
and bans `openssl`.
