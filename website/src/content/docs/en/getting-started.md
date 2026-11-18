---
title: Getting started
description: What Gilfoyle is, and how the two roles fit together.
order: 1
lang: en
---

Gilfoyle is a secure [MCP](https://modelcontextprotocol.io) server, written in Rust, that
exposes Linux administration tools to an AI agent over **SSH**. Reads are open when they are
bounded, named and non-disclosing; a read over an unbounded namespace (a path) is
allowlisted. The only mutations are starting and stopping allowlisted systemd units and
Docker containers, and refreshing or upgrading allowlisted packages.

It is built so that **it cannot execute anything**. There is no shell, no command
passthrough and no scripting: every tool is a compiled function with fixed behaviour, and
the only host interaction in the whole codebase is reading a few fixed files under `/proc`
(`uptime`, `loadavg`, `meminfo`, `mounts`), the mount table, `/etc/os-release` and the
reboot-required markers under `/run`, one read-only `statvfs` syscall over the configured
mounts, and the metadata of the paths the operator declared (never their contents).

## Two roles

- **Node** — the Gilfoyle process on one host. It serves that host over SSH and exposes the
  compiled tools.
- **Client** — an aggregator that connects to several nodes and presents **one** MCP surface
  (over stdio) to the agent. Every tool takes a `node` argument; `all` fans a read out to
  every node.

```
AI agent ──MCP(stdio)──▶ gilfoyle client ──SSH──▶ node @ host A
                        (aggregator)     ──SSH──▶ node @ host B
```

## Requirements

**Node**

- A Linux host with systemd and an accessible system D-Bus.
- `/proc/uptime`, `/proc/loadavg`, `/proc/meminfo` and `/proc/mounts` for the host tools.
- `/etc/os-release` and the `/run/reboot-required` markers for `get_reboot_status`.
- polkit (`polkitd`) — only to authorise `start_unit` and `stop_unit`.
- PackageKit and a backend (APT, DNF) — only for the package tools.
- Docker — only for the container tools. When absent, the container tools report a clear
  error and the server still starts. The socket is root-equivalent; grant it narrowly.

**Client**

- Nothing beyond the `gilfoyle` binary. The client runs on macOS (Apple Silicon and Intel)
  too — only the node is Linux-only — and releases ship it cross-compiled.

## Where to go next

- [Installing](/docs/installing/) — put a node on a host.
- [Configuration](/docs/configuration/) — the config file, keys and tool grants.
- [Tools](/docs/tools/) — the whole vocabulary.
- [Containers](/docs/docker/) — Docker over its socket, and the socket-proxy model.
- [Threat model](/docs/threat-model/) — what is in scope, and what is not.
