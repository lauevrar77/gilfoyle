# Installing and updating Gilfoyle

Two roles, installed differently:

- **Node** — one per administered host. Runs the `gilfoyle` binary as a systemd service,
  listens on SSH, and opens the compiled tools. This document covers nodes.
- **Client** — one per *operator* (or per AI agent). Runs `gilfoyle client`, connects to
  several nodes over SSH, and serves MCP on stdio. See "Client setup" below.

The node is what the systemd unit and polkit rule below install.

## Prerequisites

- A Linux host with systemd (the unit tools use the system bus).
- The `gilfoyle` binary and the files in this repo (`packaging/`, `docs/`).
- `polkit` **only if** you want to start/stop units (reads work without it).
- `PackageKit` **and a backend** (APT on Debian/Ubuntu, DNF on Rocky) **only if** you want
  the package tools. It is not present on a minimal host; without it the package tools
  report "PackageKit is unavailable". Install order: the backend first, then `packagekit`,
  then verify `packagekitd` is reachable on the system bus.
- The binary is dynamically linked (the SSH crypto backend `ring` needs a C toolchain).

Each answer below (bind address and port, managed units, package operations, client keys)
is a **choice**. Do not invent one: ask, and wait for the answer.

## Fresh install (node)

Run as a user with `sudo`. Adjust the bind address and the client public key.

```sh
# 1. Dedicated unprivileged user
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle

# 2. Binary
sudo install -o root -g root -m 0755 gilfoyle /usr/local/bin/gilfoyle
/usr/local/bin/gilfoyle --version

# 3. Configuration directory
sudo install -d -o root -g gilfoyle -m 0750 /etc/gilfoyle

# 4. Host key (SSH identity of this node). The service runs as `gilfoyle`,
#    so the key must be readable by that user.
sudo /usr/local/bin/gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
sudo chown gilfoyle:gilfoyle /etc/gilfoyle/ssh_host_ed25519_key
# `ssh keygen` printed the SHA256:... fingerprint — give it to every client to pin.

# 5. Node configuration (see below), mode 0640 root:gilfoyle
sudoedit /etc/gilfoyle/config.toml

# 6. Service
sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service
sudo systemctl daemon-reload
sudo systemctl enable --now gilfoyle
systemctl status gilfoyle --no-pager

# 7. polkit, to authorise start/stop (optional; required for the two mutating tools)
sudo apt-get install -y polkitd                     # Debian/Ubuntu
/usr/local/bin/gilfoyle systemd polkit-rule \
  | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null

# 8. packages (optional; if [packagekit] allow is set — needs packagekit + a backend)
sudo apt-get install -y packagekit                  # Debian/Ubuntu; dnf on Rocky
/usr/local/bin/gilfoyle packagekit polkit-rule \
  | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules > /dev/null
```

### `/etc/gilfoyle/config.toml`

```toml
bind = "10.0.0.5:2222"                    # mandatory, address AND port
host_key = "/etc/gilfoyle/ssh_host_ed25519_key"
# optional: max_connections, max_channels_per_connection, log_level

[tools]
enabled = ["get_uptime", "list_units", "unit_status", "start_unit", "stop_unit",
           "list_packages", "list_updates", "preview_upgrade",
           "refresh_package_cache", "upgrade_packages"]

[systemd]
manage_units = ["nginx.service"]          # empty (default) means none may be started/stopped

[packagekit]
allow = []                                # subset of ["refresh", "upgrade"]; empty means none;
                                          # there is no install/remove, ever

[[keys]]
id = "nestor"                             # the principal, used in logs
public_key = "ssh-ed25519 AAAA..."        # a client's public key
# tools = ["get_uptime", "list_units", "unit_status"]   # optional, narrows [tools] enabled
```

`bind` is mandatory. A configuration from the HTTP era (`[[tokens]]`, `allow_public`,
`allowed_hosts`, `allowed_origins`, `token_dir`) is **rejected at startup** with a migration
message.

## Verify the node

There is no HTTP health endpoint any more. Verify by connecting a client (see below), or
check that it is listening:

```sh
ss -ltnp | grep gilfoyle
journalctl -u gilfoyle -n 20 --no-pager
```

## Client setup

On the operator's machine, generate a key **per node** and add its public key to that node's
`[[keys]]`:

```sh
ssh-keygen -t ed25519 -N '' -f ~/.ssh/gilfoyle_web1     # unencrypted, mode 0600
cat ~/.ssh/gilfoyle_web1.pub                            # add to the node's [[keys]]
```

`~/.config/gilfoyle/client.toml`:

```toml
pool_size = 4

[[nodes]]
label = "web1"
host = "10.0.0.5"
port = 2222
key = "~/.ssh/gilfoyle_web1"
host_key_fingerprint = "SHA256:..."       # from the node's `ssh keygen`
```

Run it (this is what the AI client launches):

```sh
gilfoyle client --config ~/.config/gilfoyle/client.toml   # or GILFOYLE_CLIENT_CONFIG=...
```

## Updating

### From a previous SSH build (same config schema)

```sh
sudo install -o root -g root -m 0755 gilfoyle /usr/local/bin/gilfoyle
sudo systemctl restart gilfoyle
systemctl status gilfoyle --no-pager
```

If `[systemd] manage_units` changed, regenerate the unit polkit rule; if `[packagekit] allow`
changed, regenerate the package polkit rule; then restart. Both `gilfoyle * polkit-rule`
commands are in "Fresh install" above.

### From the old HTTP / bearer-token build

The transport, the authentication and the configuration all changed. Do this in order:

```sh
# 1. Stop the old service and back up
sudo systemctl stop gilfoyle
sudo cp -a /etc/gilfoyle /etc/gilfoyle.bak.$(date +%s)

# 2. Install the new binary
sudo install -o root -g root -m 0755 gilfoyle /usr/local/bin/gilfoyle

# 3. Generate the host key (prints the fingerprint for clients)
sudo /usr/local/bin/gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
sudo chown gilfoyle:gilfoyle /etc/gilfoyle/ssh_host_ed25519_key

# 4. Rewrite /etc/gilfoyle/config.toml (0640 root:gilfoyle):
#    - remove: [[tokens]], allow_public, allowed_hosts, allowed_origins, token_dir
#    - add:    bind (address:port), host_key, and one [[keys]] per client public key
#    - keep:   [tools] enabled, [systemd] manage_units, [packagekit] allow
#    The old file is rejected with a migration message until this is done.

# 5. Update the service unit (the environment file is now optional)
sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service

# 6. Regenerate the polkit rule; restart
/usr/local/bin/gilfoyle systemd polkit-rule \
  | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
sudo systemctl daemon-reload
sudo systemctl enable --now gilfoyle
systemctl status gilfoyle --no-pager

# 7. Migrate every client: new client config with a per-node key and the node's
#    pinned fingerprint (see "Client setup"). The old bearer token is no longer used.
```

Note the port change: the HTTP build listened on its `bind` (often `:8765`); the SSH build
uses an SSH port (e.g. `:2222`). Update the firewall and the clients accordingly. Bearer
tokens are gone — delete `/etc/gilfoyle/env`'s token entries once migration is done.

## What to hand an agent

Give a shell-capable agent (with `sudo`) the prompt below, filling in the parameters. It
works for a fresh install and, with `MODE=update`, for a migration from the old build.

```text
You are installing or updating gilfoyle, a secure MCP server, on this Linux host. Use sudo
where needed, and stop and show me the error if anything fails. Never log a secret, and
never run gilfoyle as root.

This host is one of:
  - a Node   — the gilfoyle process administered here (the subject of this prompt);
  - a Client — an aggregator, run on the operator's machine. If that is what I want, say so
               and switch to docs/INSTALL.md "Client setup" instead of the steps below.

Do NOT assume any value below. Ask me each question, one at a time, and wait for my answer
before continuing. If an answer is incomplete or inconsistent, ask again. Only start once
every choice is settled, and echo the final set back to me for confirmation.

Questions (skip one only if its "no" branch is chosen):
  Q1. MODE — fresh install, or update an existing install? If updating, is it from an SSH
      build (same config schema) or from the old HTTP/bearer-token build? See "Updating".
  Q2. GILFOYLE_BIN — the path to the built `gilfoyle` binary.
  Q3. REPO — the path to the gilfoyle checkout (for packaging/ and docs/).
  Q4. BIND — the address and SSH port to listen on, e.g. 10.0.0.5:2222. Confirm the port is
      free and reachable by the clients.
  Q5. CLIENT_KEYS — the OpenSSH public key(s) allowed to connect. Ask for at least one; if I
      give none, refuse to continue.
  Q6. MANAGED_UNITS — the systemd units that may be started/stopped, or none for read-only.
  Q7. PACKAGE_OPS — whether to enable package operations, and which: none, "refresh"
      (apt update), or "refresh" + "upgrade" (apt upgrade). There is never an install or
      remove. If not none, ask whether PackageKit and a backend are already installed.
  Q8. TOOL_GRANTS — whether each client key is narrowed to a subset of tools or inherits
      [tools] enabled.

Then:
1. Fresh install: follow docs/INSTALL.md "Fresh install (node)" with my answers. Update:
   follow the matching section under "Updating".
2. Write /etc/gilfoyle/config.toml (0640 root:gilfoyle) with bind, host_key, [tools] enabled,
   [systemd] manage_units, [packagekit] allow, and one [[keys]] per key in CLIENT_KEYS.
3. Install packaging/gilfoyle.service, daemon-reload, enable --now gilfoyle.
4. If MANAGED_UNITS is not empty: ensure polkit is installed, then
   `gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules`.
5. If PACKAGE_OPS is not empty: ensure PackageKit and its backend are installed, then
   `gilfoyle packagekit polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules`.
6. Verify: `ss -ltnp | grep gilfoyle` shows BIND; `systemctl status gilfoyle` is active.
7. Report back the node's SSH host:port and its `SHA256:...` host-key fingerprint exactly
   once, so I can configure clients. Report nothing else secret.
```

## Files the install touches

| Path | Purpose |
|---|---|
| `/usr/local/bin/gilfoyle` | the binary (node and client) |
| `/etc/gilfoyle/config.toml` | node configuration (`0640 root:gilfoyle`) |
| `/etc/gilfoyle/ssh_host_ed25519_key` | node host key (`0600 gilfoyle`) |
| `/etc/gilfoyle/env` | optional `GILFOYLE_*` overrides (not secrets any more) |
| `/etc/systemd/system/gilfoyle.service` | the hardened unit |
| `/etc/polkit-1/rules.d/49-gilfoyle-units.rules` | generated polkit rule (unit control) |
| `/etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules` | generated polkit rule (packages) |
