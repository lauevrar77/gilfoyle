---
title: Outils
description: Tout le vocabulaire d'outils, arguments et effets.
order: 5
lang: fr
---

Voici tout le vocabulaire. Si un outil n'est pas dans cette liste, il n'existe pas.

## Hôte

| Outil | Arguments | Effet |
|---|---|---|
| `get_uptime` | — | uptime de l'hôte depuis `/proc/uptime` (depuis le démarrage du noyau) |

```json
{ "uptime_seconds": 1048545.67, "uptime_human": "12d 3h 15m 45s" }
```

## systemd

Joint via **D-Bus in-process** (`org.freedesktop.systemd1`) ; Gilfoyle ne lance jamais
`systemctl`.

| Outil | Arguments | Effet |
|---|---|---|
| `list_units` | `active_state?`, `name_contains?`, `unit_type?`, `limit?` | lister les unités (lecture) |
| `unit_status` | `name` | état détaillé d'une unité (lecture) |
| `start_unit` | `name` | démarrer une unité **gérée** |
| `stop_unit` | `name` | arrêter une unité **gérée** |

Les noms d'unités peuvent être donnés courts (`nginx`) ou pleinement qualifiés
(`nginx.service`). `start_unit` et `stop_unit` attendent la fin de la tâche systemd et
rapportent son résultat. Les mutations n'agissent que sur les unités listées dans
`[systemd] manage_units`, et polkit est l'autorité derrière elles.

## Conteneurs

Docker est joint via son **socket UNIX** avec un client HTTP en processus ; Gilfoyle ne lance
jamais `docker`. Il n'y a délibérément **ni création ni suppression** : l'ensemble des
opérations se limite à lister, inspecter, démarrer, arrêter et redémarrer.

| Outil | Arguments | Effet |
|---|---|---|
| `list_containers` | `state?`, `name_contains?`, `limit?` | lister les conteneurs (lecture) |
| `container_status` | `container` | état détaillé d'un conteneur (lecture) |
| `start_container` | `container` | démarrer un conteneur **géré** |
| `stop_container` | `container` | arrêter un conteneur **géré** |
| `restart_container` | `container` | redémarrer un conteneur **géré** |

`container` est un nom ou un id de conteneur. Les trois mutations sont **refusées par
défaut** : elles n'agissent que sur les conteneurs dont le nom correspond à
`[docker] manage_containers`, et l'action s'exécute sur l'id résolu du conteneur, si bien
qu'un renommage entre la vérification et l'action ne peut pas la rediriger.

## Paquets

Joint via **PackageKit** sur le bus système (`org.freedesktop.PackageKit`), en processus et
sans rien lancer. Le même frontal fonctionne sur APT (Debian) et DNF (Rocky). Gilfoyle
n'installe ni ne supprime jamais de paquet.

| Outil | Arguments | Effet |
|---|---|---|
| `list_packages` | `name_contains?`, `limit?` | lister les paquets installés et versions (lecture) |
| `list_updates` | `min_severity?`, `limit?` | lister les mises à jour disponibles (lecture) |
| `preview_upgrade` | — | simuler une mise à niveau et rapporter ce qu'elle ferait (lecture) |
| `refresh_package_cache` | `force?` (défaut `true`) | rafraîchir le cache de métadonnées, soit `apt update` |
| `upgrade_packages` | — | appliquer les mises à jour, soit `apt upgrade` |

`preview_upgrade` et le garde-fou interne à `upgrade_packages` utilisent le drapeau
`SIMULATE` de PackageKit, qui ne change jamais rien. Si le plan devait installer, supprimer,
rendre obsolète ou rétrograder un paquet, `upgrade_packages` refuse sauf si
`allow_transitive_changes = true`. Les paquets non fiables sont toujours refusés.

## Via le client

Le client ajoute un argument `node` obligatoire à chaque outil (un label, ou `all` pour les
outils de lecture). `all` renvoie une table par nœud, avec une erreur par nœud ayant échoué.
Un outil `list_nodes` rapporte le label, la joignabilité et les outils exposés de chaque nœud.
