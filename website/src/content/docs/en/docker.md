---
title: Containers
description: Docker over its UNIX socket — a closed operation set, and the socket-proxy security model.
order: 6
lang: en
---

Docker has no D-Bus interface: its control surface is an HTTP API on the daemon's UNIX socket
(`/var/run/docker.sock`). Gilfoyle speaks HTTP/1.1 to that socket **in-process** — it never
shells out to `docker`, and never starts a privileged helper.

## A closed operation set

The adapter can only build four requests: list containers, inspect one container, and
start/stop/restart one container, plus the `/version` probe. There is deliberately no request
that creates or removes a container — no `POST /containers/create`, no `DELETE /containers/{id}`
— so no configuration can enable one.

## The API version is negotiated

On connect, Gilfoyle calls `GET /version` once and uses the returned API version in every
path (`/v1.45/...`). If the probe fails, the connection is refused; Gilfoyle doesn't assume a
version.

## No polkit

Unlike systemd and PackageKit, Docker has no polkit behind it. Access to the socket is
equivalent to **root** — a `--privileged` container can mount the host. So the guardrails are:

- the tool allowlist (`[tools] enabled` plus per-key grants);
- `[docker] manage_containers`, empty by default, which names the containers that may be
  mutated;
- the host's own access control on the socket.

Since the socket is what actually protects the host, the recommended setup puts a **socket proxy** in
front of the daemon, exposing only the endpoints Gilfoyle uses (`containers/json`,
`containers/{id}/json`, `containers/{id}/start|stop|restart`), and points `[docker] endpoint`
at the proxy's socket. The alternative — adding the `gilfoyle` user to the `docker` group —
hands Gilfoyle full control of the daemon; prefer the proxy.

## The allowlist takes anchored regexes

`[docker] manage_containers` entries are exact names, or `re:<pattern>` regular expressions for
fleets (`re:^worker-[0-9]+$`). A `re:` entry is wrapped in `^(?:…)$`, so it must match the
whole name: `re:web` matches `web` but not `web-backdoor`. Plain entries stay exact, so a name
with a dot (`my.app`) is not silently treated as a pattern.
