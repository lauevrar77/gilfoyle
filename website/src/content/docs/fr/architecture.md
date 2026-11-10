---
title: Architecture
description: Sept crates, et pourquoi les dépendances ne pointent que vers l'intérieur.
order: 7
lang: fr
---

Gilfoyle est hexagonal (ports et adaptateurs). Les dépendances pointent vers l'intérieur, et
le graphe de dépendances Cargo est l'application de la règle.

| Crate | Rôle | E/S |
|---|---|---|
| `gilfoyle-core` | types du domaine, ports, cas d'usage | aucune |
| `gilfoyle-system` | adaptateur piloté (uptime de l'hôte) | lecture seule |
| `gilfoyle-systemd` | adaptateur piloté (systemd via D-Bus) | bus système |
| `gilfoyle-packagekit` | adaptateur piloté (PackageKit via D-Bus) | bus système |
| `gilfoyle-docker` | adaptateur piloté (Docker via son socket UNIX) | socket |
| `gilfoyle-ssh` | adaptateur pilotant : serveur SSH embarqué + politique d'auth/canaux | réseau |
| `gilfoyle-client` | adaptateur pilotant : agrégateur multi-nœuds | réseau |
| `gilfoyle-server` | gestionnaire MCP, configuration, câblage + le binaire `gilfoyle` | réseau |

Le cœur n'importe jamais un adaptateur. Une capacité est un port étroit
(`UptimeSource`, `ServiceCatalog`, `ServiceControl`) ; ajoute un trait par capacité plutôt
que d'élargir un trait existant.

## Ajouter un outil

1. Modélise le résultat dans `gilfoyle-core` et ajoute une méthode au port pilotant.
2. Implémente la lecture ou la mutation dans un adaptateur piloté derrière un port étroit.
3. Traduis-la dans l'implémentation `#[tool_router]` de `gilfoyle-server` — quelques
   lignes, sans logique.
4. Couvre-la par des tests unitaires ; étends le test d'intégration SSH quand la forme du
   protocole change.

Si l'outil est une lecture, ajoute-le à l'ensemble de lecture de `gilfoyle-client` pour
qu'il puisse être diffusé.

## Qualité

```sh
make fmt-check   # formatage
make lint        # clippy -D warnings
make test        # tests unitaires + d'intégration
make mutants     # cargo-mutants ; le score doit être de 100 %
make check       # tout ce qui précède sauf le fuzzing
```

Le score de mutation est maintenu à **100 %** : un mutant survivant fait échouer l'exécution.
La glue exclue est la racine de composition (`main.rs`) et le mince câblage de transport
`russh`/`rmcp`/`hyper` — leurs décisions vivent dans des modules purs et testés. Les
adaptateurs systemd et PackageKit sont couverts par des bus simulés en processus, l'adaptateur
Docker par un serveur HTTP simulé sur un socket UNIX, et le transport SSH par une paire
client/serveur `russh` en processus.

Le fuzzing (`make fuzz`) exécute huit cibles en nightly avec AddressSanitizer : `uptime_parse`,
`authorized_key`, `config_toml`, `client_config`, `unit_name`, `package_id`, `container_ref`
et `container_allowlist`. `cargo-deny`
(`deny.toml`) refuse les registres et sources git inconnus, garde une liste d'avis ignorés
vide et bannit `openssl`.
