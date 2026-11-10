---
title: Installing
description: Install and update a Gilfoyle node, then wire up polkit.
order: 2
lang: en
---

A node runs as an unprivileged, dedicated user. It never escalates; polkit is the only
authority behind a mutation.

## Build

```sh
cargo build --release -p gilfoyle-server --bin gilfoyle
```

The SSH crypto backend is `ring`, so the build is not pure Rust and releases are dynamically
linked.

## Install

```sh
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle
sudo install -o root -g root -m 0755 target/release/gilfoyle /usr/local/bin/gilfoyle
sudo install -d -o root -g gilfoyle -m 0750 /etc/gilfoyle

# Generate the host key. This prints the SHA256 fingerprint clients pin.
sudo /usr/local/bin/gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
sudo chown gilfoyle:gilfoyle /etc/gilfoyle/ssh_host_ed25519_key

# Write /etc/gilfoyle/config.toml (0640 root:gilfoyle) — see Configuration.

sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service
sudo systemctl daemon-reload && sudo systemctl enable --now gilfoyle
```

`packaging/gilfoyle.service` is a hardened unit: a dedicated user, `NoNewPrivileges`,
`ProtectSystem=strict`, an empty capability set and a `SystemCallFilter`. The system bus is
reached over `AF_UNIX`, which the unit already allows.

## Authorise unit control

`start_unit` and `stop_unit` require the `org.freedesktop.systemd1.manage-units`
authorisation. Gilfoyle runs unprivileged and never escalates, so polkit must be installed
and running.

```sh
sudo apt-get install -y polkitd   # Debian/Ubuntu; equivalent on other distros

gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
```

The rule grants the `gilfoyle` user the right to start and stop only the units in
`[systemd] manage_units`. Re-run the command whenever that list changes.

## Authorise package operations

```sh
gilfoyle packagekit polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules > /dev/null
```

The rule grants only the actions named in `[packagekit] allow`, and explicitly refuses
installing, reinstalling, downgrading and removing packages, trusting new signing keys, and
upgrading the whole distribution.

## Releases

Tagged releases ship a dynamically linked GNU binary, a checksum and the deployment files.
Build an artifact with:

```sh
make package-release
```
