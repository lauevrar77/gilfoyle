---
title: Installation
description: Installer et mettre à jour un nœud Gilfoyle, puis câbler polkit.
order: 2
lang: fr
---

Un nœud tourne sous un utilisateur dédié non privilégié. Il n'élève jamais ses privilèges ;
polkit est la seule autorité derrière une mutation.

## Compilation

```sh
cargo build --release -p gilfoyle-server --bin gilfoyle
```

Le backend crypto SSH est `ring` : la compilation n'est donc pas du Rust pur, et les
versions sont liées dynamiquement.

## Installation

```sh
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin gilfoyle
sudo install -o root -g root -m 0755 target/release/gilfoyle /usr/local/bin/gilfoyle
sudo install -d -o root -g gilfoyle -m 0750 /etc/gilfoyle

# Générer la clé d'hôte. La commande affiche l'empreinte SHA256 que les clients épinglent.
sudo /usr/local/bin/gilfoyle ssh keygen --path /etc/gilfoyle/ssh_host_ed25519_key
sudo chown gilfoyle:gilfoyle /etc/gilfoyle/ssh_host_ed25519_key

# Écrire /etc/gilfoyle/config.toml (0640 root:gilfoyle) — voir Configuration.

sudo install -o root -g root -m 0644 packaging/gilfoyle.service /etc/systemd/system/gilfoyle.service
sudo systemctl daemon-reload && sudo systemctl enable --now gilfoyle
```

`packaging/gilfoyle.service` est une unité durcie : utilisateur dédié, `NoNewPrivileges`,
`ProtectSystem=strict`, ensemble de capacités vide et `SystemCallFilter`. Le bus système est
joint via `AF_UNIX`, ce que l'unité autorise déjà.

## Autoriser le contrôle des unités

`start_unit` et `stop_unit` exigent l'autorisation `org.freedesktop.systemd1.manage-units`.
Gilfoyle tourne non privilégié et n'élève jamais ses privilèges : polkit doit donc être
installé et en cours d'exécution.

```sh
sudo apt-get install -y polkitd   # Debian/Ubuntu ; équivalent sur les autres distros

gilfoyle systemd polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-units.rules > /dev/null
```

La règle accorde à l'utilisateur `gilfoyle` le droit de démarrer et arrêter uniquement les
unités listées dans `[systemd] manage_units`. Relance la commande à chaque modification de
cette liste.

## Autoriser les opérations sur les paquets

```sh
gilfoyle packagekit polkit-rule | sudo tee /etc/polkit-1/rules.d/49-gilfoyle-packagekit.rules > /dev/null
```

La règle n'accorde que les actions nommées dans `[packagekit] allow`, et refuse
explicitement d'installer, réinstaller, rétrograder ou supprimer des paquets, de faire
confiance à de nouvelles clés de signature, et de mettre à niveau toute la distribution.

## Versions

Les versions étiquetées fournissent un binaire GNU lié dynamiquement, une somme de contrôle
et les fichiers de déploiement. Construis un artefact avec :

```sh
make package-release
```

Elles fournissent aussi le **client pour macOS**, cross-compilé depuis Linux sur le même
runner avec `zig` — un tarball par architecture. Aucun SDK Apple n'est nécessaire : `zig`
apporte l'éditeur de liens Mach-O et les stubs libSystem, et `chrono` est compilé sans son
feature `clock`, donc l'édition de liens ne réclame jamais le framework CoreFoundation.

```sh
make client-macos-setup      # une fois : zig, cargo-zigbuild et les cibles Rust Apple
make package-client-macos    # dist/gilfoyle-<version>-{aarch64,x86_64}-apple-darwin.tar.gz
```
