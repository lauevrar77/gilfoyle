---
title: Démarrage
description: Ce qu'est Gilfoyle, et comment les deux rôles s'articulent.
order: 1
lang: fr
---

Gilfoyle est un serveur [MCP](https://modelcontextprotocol.io) sécurisé, écrit en Rust, qui
expose des outils d'administration Linux à un agent IA par **SSH**. Les lectures sont
ouvertes quand elles sont bornées, nommées et non divulgantes ; une lecture sur un espace non
borné (un chemin) est allowlistée. Les seules mutations consistent à démarrer et arrêter des
unités systemd et des conteneurs Docker autorisés, et à rafraîchir ou mettre à jour des
paquets autorisés.

Il est conçu pour **ne rien pouvoir exécuter**. Pas de shell, pas de passe-plat de commandes,
pas de script : chaque outil est une fonction compilée au comportement figé, et la seule
interaction avec l'hôte dans tout le code consiste à lire quelques fichiers fixes sous `/proc`
(`uptime`, `loadavg`, `meminfo`, `mounts`), la table des montages, `/etc/os-release` et les
marqueurs de redémarrage sous `/run`, un appel système `statvfs` en lecture seule sur les
montages configurés, et les métadonnées des chemins déclarés par l'opérateur (jamais leur
contenu).

## Deux rôles

- **Nœud** — le processus Gilfoyle sur un hôte. Il sert cet hôte via SSH et expose les outils
  compilés.
- **Client** — un agrégateur qui se connecte à plusieurs nœuds et présente **une seule**
  surface MCP (via stdio) à l'agent. Chaque outil prend un argument `node` ; `all` diffuse une
  lecture vers tous les nœuds.

```
agent IA ──MCP(stdio)──▶ client gilfoyle ──SSH──▶ nœud @ hôte A
                         (agrégateur)      ──SSH──▶ nœud @ hôte B
```

## Prérequis

**Nœud**

- Un hôte Linux avec systemd et un bus système D-Bus accessible.
- `/proc/uptime`, `/proc/loadavg`, `/proc/meminfo` et `/proc/mounts` pour les outils hôte.
- `/etc/os-release` et les marqueurs `/run/reboot-required` pour `get_reboot_status`.
- polkit (`polkitd`) — uniquement pour autoriser `start_unit` et `stop_unit`.
- PackageKit et un backend (APT, DNF) — uniquement pour les outils paquets.
- Docker — uniquement pour les outils conteneurs. En son absence, ces outils renvoient une
  erreur claire et le serveur démarre quand même. Le socket équivaut à root ; accorde-le
  étroitement.

**Client**

- Rien de plus que le binaire `gilfoyle`. Le client tourne aussi sur macOS (Apple Silicon et
  Intel) — seul le nœud est Linux uniquement — et les versions le livrent cross-compilé.

## Où aller ensuite

- [Installation](/fr/docs/installing/) — poser un nœud sur un hôte.
- [Configuration](/fr/docs/configuration/) — le fichier de config, les clés et les
  autorisations d'outils.
- [Outils](/fr/docs/tools/) — tout le vocabulaire.
- [Conteneurs](/fr/docs/docker/) — Docker via son socket, et le modèle du proxy de socket.
- [Modèle de menace](/fr/docs/threat-model/) — ce qui est couvert, et ce qui ne l'est pas.
