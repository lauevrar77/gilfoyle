---
title: Configuration
description: Les fichiers de config du nœud et du client, les clés et les autorisations.
order: 4
lang: fr
---

La configuration est en refus par défaut : pas de `bind`, pas de clé ou pas de clé d'hôte
signifie pas de démarrage. C'est un fichier TOML (par défaut
`/etc/gilfoyle/config.toml`, à surcharger avec `--config`). Les variables d'environnement
surchargent les valeurs du fichier.

## Nœud

```toml
bind = "10.0.0.5:2222"                    # obligatoire, adresse ET port
host_key = "/etc/gilfoyle/ssh_host_ed25519_key"
max_connections = 32
max_channels_per_connection = 8
log_level = "info"                        # filtre tracing, journaux JSON sur stderr

[tools]
enabled = [
  "get_uptime",
  "list_units", "unit_status", "start_unit", "stop_unit",
  "list_packages", "list_updates", "preview_upgrade",
  "refresh_package_cache", "upgrade_packages",
  "list_containers", "container_status", "start_container", "stop_container", "restart_container",
]

[systemd]
# Unités que l'on peut démarrer ou arrêter. Vide (par défaut) signifie aucune.
manage_units = ["nginx.service", "myapp.service"]

[packagekit]
# Opérations mutantes autorisées sur les paquets. Vide signifie aucune.
# Il n'y a délibérément pas de « install » ni de « remove ».
allow = ["refresh", "upgrade"]
allow_transitive_changes = false
poll_timeout_seconds = 600
default_limit = 200

[docker]
# Chemin du socket du démon Docker (défaut /var/run/docker.sock). Seul un chemin
# de socket UNIX est accepté ; un point de terminaison TCP est refusé.
endpoint = "/var/run/docker.sock"
# Conteneurs que l'on peut démarrer, arrêter ou redémarrer. Vide (par défaut)
# signifie aucun. Une entrée est un nom exact, ou une expression régulière
# `re:` ancrée au nom entier.
manage_containers = ["web", "re:^worker-[0-9]+$"]
call_timeout_seconds = 10
stop_timeout_seconds = 30
default_limit = 100

[[keys]]
id = "nestor"
public_key = "ssh-ed25519 AAAA..."
# tools omis → hérite de [tools] enabled ; présent → le restreint (intersection)
```

### Authentification et autorisations d'outils

Chaque entrée `[[keys]]` est une clé publique de client. L'`id` de la clé est le
**principal** utilisé dans les journaux. Les algorithmes acceptés sont ed25519, ecdsa-sha2 et
rsa-sha2 ; `ssh-rsa` (SHA-1) est refusé.

Une clé avec une liste `tools` ne peut appeler que l'intersection de cette liste et de
`[tools] enabled` ; un nom d'outil inconnu fait échouer la configuration. Une clé sans `tools`
hérite de l'ensemble global. Faire tourner ou révoquer une clé exige un redémarrage.

### Surcharges d'environnement

| Variable | Effet |
|---|---|
| `GILFOYLE_BIND` | surcharge `bind` |
| `GILFOYLE_HOST_KEY` | surcharge `host_key` |
| `GILFOYLE_LOG` | surcharge `log_level` |

## Client

Un chemin suggéré est `~/.config/gilfoyle/client.toml` ; passe-le avec
`gilfoyle client --config <chemin>` (ou `GILFOYLE_CLIENT_CONFIG`).

```toml
pool_size = 4

[[nodes]]
label = "web1"
host = "10.0.0.5"
port = 2222
key = "~/.ssh/gilfoyle_web1"              # clé privée non chiffrée, mode 0600
host_key_fingerprint = "SHA256:..."       # issue de `gilfoyle ssh keygen`
```

Le client sert MCP sur stdio, maintient `pool_size` sessions par nœud, marque les nœuds
injoignables, et réessaie les lectures (jamais les mutations) à travers les reconnexions. Un
nœud hors ligne au démarrage reste visible dans `list_nodes` et renvoie une erreur à l'appel.
