# Manage Docker containers over the daemon socket

Docker has no D-Bus interface: the canonical control surface is the HTTP API on
the daemon's UNIX socket (`/var/run/docker.sock`). Since gilfoyle must never
spawn a process (`std::process::Command` is banned at compile time and `unsafe`
is forbidden), it cannot shell out to `docker`. It therefore speaks HTTP/1.1 to
the socket **in-process**, with `hyper` and a small `tower` connector of our
own, and never starts a privileged helper.

## A closed operation set

The adapter can only build four requests: list containers, inspect one
container, and start/stop/restart one container, plus the `/version` probe used
to negotiate the API version. There is deliberately no request that creates or
removes a container — no `POST /containers/create`, no `DELETE /containers/{id}`
— so no configuration can enable one. The domain core's action vocabulary
(`ContainerAction`) has exactly three variants, mirroring that.

## The API version is negotiated, never guessed

On connect, gilfoyle calls `GET /version` once and pins the returned
`ApiVersion` into every path (`/v1.45/...`). If the probe fails, the connection
is refused: gilfoyle does not assume a version. The negotiated version is cached
for the lifetime of the adapter.

## Authority without polkit

Unlike systemd and PackageKit, Docker has no polkit behind it. Access to the
socket is equivalent to root: a `--privileged` container can mount the host. The
guardrails are therefore:

- the tool allowlist (`[tools] enabled` plus per-key grants);
- `[docker] manage_containers`, which names the containers that may be mutated
  and is empty by default (default-deny);
- the host's own access control on the socket.

Because the socket is the real boundary, the documentation recommends exposing
the daemon through a **socket proxy** restricted to the endpoints gilfoyle uses
(`containers/json`, `containers/{id}/json`, `containers/{id}/stop|start|restart`),
rather than adding the `gilfoyle` user to the `docker` group. Gilfoyle narrows
the *tool* surface; it cannot make socket access safe. The systemd unit already
allows `AF_UNIX` and `ProtectSystem=strict` does not impede `connect()` on a
socket (the system bus is reached the same way), so no hardening directive is
needed.

## The allowlist accepts anchored regular expressions

`[docker] manage_containers` entries are exact names, or `re:<pattern>` regular
expressions for fleets (`re:^worker-[0-9]+$`). A `re:` entry is wrapped in
`^(?:…)$`, so it must match the whole name: `re:web` matches `web` but not
`web-backdoor`. Plain entries stay exact, so a name that contains a dot
(`my.app`) is not silently treated as a pattern. The `regex` crate is used
because it runs in linear time — no catastrophic backtracking — and the pattern
is compiled at configuration load, so an invalid one refuses startup.

## Considered options

- The `docker` CLI (rejected — reintroduces process spawning and a privileged
  binary to harden).
- `bollard` (rejected — a very large dependency surface that moves Docker
  semantics into a crate, against the project's minimal, auditable grain).
- Hand-rolling HTTP (rejected as the foundation — the future log-streaming
  endpoint needs chunked, long-lived responses, which a real HTTP/1.1 client
  handles; the Docker-specific framing stays ours either way).
