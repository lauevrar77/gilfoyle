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
- The binary is dynamically linked; no static build is produced any more.

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

If `[systemd] manage_units` changed, regenerate the polkit rule and restart.

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
You are installing or updating gilfoyle, a secure MCP server, on this Linux host. Follow the
steps exactly, use sudo where needed, and stop and show me the error if anything fails.
Never log any secret, and never run gilfoyle as root.

Parameters (ask me if a value is missing):
  MODE          = install | update
  GILFOYLE_BIN  = <path to the built `gilfoyle` binary>
  REPO          = <path to the gilfoyle checkout, for packaging/ and docs/>
  BIND          = <address:port to listen on, e.g. 10.0.0.5:2222>
  MANAGED_UNITS = <systemd units that may be started/stopped, or empty for read-only>
  CLIENT_KEYS   = <one or more OpenSSH public keys allowed to connect>

Steps (fresh install):
1. Follow docs/INSTALL.md "Fresh install (node)": create the gilfoyle user, install the
   binary, create /etc/gilfoyle, run `gilfoyle ssh keygen`, chown the host key to gilfoyle.
2. Write /etc/gilfoyle/config.toml (0640 root:gilfoyle) with bind, host_key, [tools] enabled,
   [systemd] manage_units = MANAGED_UNITS, and one [[keys]] per key in CLIENT_KEYS.
3. Install packaging/gilfoyle.service, daemon-reload, enable --now gilfoyle.
4. If MANAGED_UNITS is not empty: install polkit, then install the generated rule with
   `gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules`.
5. Verify: `ss -ltnp | grep gilfoyle` shows BIND; `systemctl status gilfoyle` is active.
6. Report back the node's SSH URL (host:port) and the `SHA256:...` host-key fingerprint
   exactly once so I can configure clients. Report nothing else secret.

Steps (update from the old HTTP build): follow docs/INSTALL.md "From the old HTTP /
bearer-token build" instead of step 1–3 above.
```

## Files the install touches

| Path | Purpose |
|---|---|
| `/usr/local/bin/gilfoyle` | the binary (node and client) |
| `/etc/gilfoyle/config.toml` | node configuration (`0640 root:gilfoyle`) |
| `/etc/gilfoyle/ssh_host_ed25519_key` | node host key (`0600 gilfoyle`) |
| `/etc/gilfoyle/env` | optional `GILFOYLE_*` overrides (not secrets any more) |
| `/etc/systemd/system/gilfoyle.service` | the hardened unit |
| `/etc/polkit-1/rules.d/49-gilfoyle-units.rules` | generated polkit rule |
