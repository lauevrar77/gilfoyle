---
title: Durcissement
description: Le bac à sable systemd, les restrictions de capacités et d'appels système, et les garde-fous de compilation.
order: 3
lang: fr
---

Gilfoyle tourne sous un utilisateur dédié sans privilèges et ne les élève jamais. L'unité
systemd fournie met le processus en bac à sable, et le build Rust rejette les constructions
risquées.

## L'unité

`packaging/gilfoyle.service` est durcie par défaut :

```ini
[Service]
User=gilfoyle
Group=gilfoyle

NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
AmbientCapabilities=
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM

ReadOnlyPaths=/proc/uptime
```

- **Aucune capacité.** `CapabilityBoundingSet=` et `AmbientCapabilities=` sont vides, et
  `NoNewPrivileges=yes` signifie que le processus ne peut jamais en acquérir.
- **Une vue en lecture seule de l'hôte.** `ProtectSystem=strict` et `ProtectHome=yes` rendent
  le système de fichiers en lecture seule ; `/proc/uptime` est le seul fichier de l'hôte que le
  service peut lire.
- **Un filtre d'appels système.** `SystemCallFilter=@system-service` n'autorise que les appels
  système de service ; tout le reste renvoie `EPERM`.
- **Pas de mémoire à la fois inscriptible et exécutable.** `MemoryDenyWriteExecute=yes`.
- **Un réseau limité au nécessaire.** `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX` — le
  bus système est joint via `AF_UNIX`.

## Garde-fous à la compilation

Ces règles sont vérifiées à la compilation, pas à l'exécution.

- `unsafe` est interdit dans tout le workspace.
- `clippy::disallowed_methods` (voir `clippy.toml`) refuse `std::process::Command` et
  `std::process::exit` : toute tentative de lancer un processus échoue au lint.
- Le cœur du domaine ne fait aucune E/S et n'a accès ni au réseau, ni au système de fichiers,
  ni à la table de processus.

## Chaîne d'approvisionnement

`cargo-deny` (voir `deny.toml`) refuse les registres et sources git inconnus, porte une liste
d'avis ignorés vide, fixe une liste blanche de licences et bannit `openssl`.

## Docker n'a pas de polkit

Docker est joint via son socket UNIX. Contrairement à systemd et PackageKit, il n'y a pas de
polkit derrière : l'accès à `/var/run/docker.sock` équivaut à **root**. L'unité systemd
autorise déjà `AF_UNIX`, et `ProtectSystem=strict` n'empêche pas `connect()` sur un socket :
aucune directive supplémentaire n'est nécessaire — mais le socket doit être accordé étroitement
(un proxy de socket), et `[docker] manage_containers` reste en refus par défaut. Voir
[Conteneurs](/fr/docs/docker/).

## Ce qui n'est pas durci

La limitation de débit est laissée au pare-feu et à systemd, la rotation des clés exige un
redémarrage, et les certificats OpenSSH ne sont pas pris en charge. Le transport SSH ajoute
une surface de parseur avant authentification et un backend crypto C (`ring`) — un compromis
assumé.
