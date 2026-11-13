---
title: Démarrage
description: Ce qu'est Gilfoyle, et comment les deux rôles s'articulent.
order: 1
lang: fr
---

Gilfoyle est un serveur [MCP](https://modelcontextprotocol.io) sécurisé, écrit en Rust, qui
expose des outils d'administration Linux à un agent IA par **SSH**. Les lectures sont
ouvertes ; les seules mutations consistent à démarrer et arrêter des unités systemd et des
conteneurs Docker autorisés, et à rafraîchir ou mettre à jour des paquets autorisés.

Il est conçu pour **ne rien pouvoir exécuter**. Pas de shell, pas de passe-plat de commandes,
pas de script : chaque outil est une fonction compilée au comportement figé, et la seule
interaction avec l'hôte dans tout le code consiste à lire une poignée de fichiers fixes.

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
- `/proc/uptime` pour l'outil d'uptime.
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
