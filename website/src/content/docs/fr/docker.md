---
title: Conteneurs
description: Docker via son socket UNIX — un ensemble d'opérations fermé, et le modèle de sécurité du proxy de socket.
order: 6
lang: fr
---

Docker n'a pas d'interface D-Bus : sa surface de contrôle est une API HTTP sur le socket UNIX
du démon (`/var/run/docker.sock`). Gilfoyle parle HTTP/1.1 à ce socket **en processus** — il
n'appelle jamais `docker`, et ne démarre jamais d'auxiliaire privilégié.

## Un ensemble d'opérations fermé

L'adaptateur ne peut construire que quatre requêtes : lister les conteneurs, inspecter un
conteneur, et démarrer/arrêter/redémarrer un conteneur, plus la sonde `/version`. Il n'existe
délibérément aucune requête qui crée ou supprime un conteneur — ni `POST /containers/create`,
ni `DELETE /containers/{id}` — si bien qu'aucune configuration ne peut en activer une.

## La version d'API est négociée

À la connexion, Gilfoyle appelle `GET /version` une fois et utilise la version d'API renvoyée
dans chaque chemin (`/v1.45/...`). Si la sonde échoue, la connexion est refusée ; Gilfoyle ne
suppose pas de version.

## Pas de polkit

Contrairement à systemd et PackageKit, Docker n'a pas de polkit derrière. L'accès au socket
équivaut à **root** — un conteneur `--privileged` peut monter l'hôte. Les garde-fous sont donc :

- l'allowlist d'outils (`[tools] enabled` plus les autorisations par clé) ;
- `[docker] manage_containers`, vide par défaut, qui nomme les conteneurs que l'on peut muter ;
- le contrôle d'accès de l'hôte sur le socket.

Comme c'est le socket qui protège réellement l'hôte, le déploiement recommandé place un **proxy de socket**
devant le démon, exposant seulement les points d'accès que Gilfoyle utilise (`containers/json`,
`containers/{id}/json`, `containers/{id}/start|stop|restart`), et pointe `[docker] endpoint`
vers le socket du proxy. L'alternative — ajouter l'utilisateur `gilfoyle` au groupe `docker` —
donne à Gilfoyle le contrôle total du démon ; préfère le proxy.

## L'allowlist accepte des expressions régulières ancrées

Les entrées de `[docker] manage_containers` sont des noms exacts, ou des expressions
régulières `re:<motif>` pour des parcs (`re:^worker-[0-9]+$`). Une entrée `re:` est encadrée
par `^(?:…)$`, elle doit donc correspondre au nom entier : `re:web` correspond à `web` mais
pas à `web-backdoor`. Les entrées simples restent exactes, si bien qu'un nom contenant un
point (`my.app`) n'est pas silencieusement traité comme un motif.
