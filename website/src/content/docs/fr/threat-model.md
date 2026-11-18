---
title: Modèle de menace
description: Ce contre quoi Gilfoyle se défend, et ce qu'il ne couvre pas.
order: 8
lang: fr
---

## Dans le périmètre

Un attaquant qui atteint le port SSH, ou qui compromet le processus Gilfoyle, ne doit obtenir
ni exécution de code ni privilèges sur l'hôte.

- **Aucune exécution de code.** Les outils sont des fonctions compilées ; rien n'accepte une
  commande ou un chemin à exécuter. Le gestionnaire SSH refuse `shell`/`exec`/`pty` et toute
  redirection. systemd et PackageKit sont joints via D-Bus in-process, Docker via son socket
  UNIX, et l'occupation disque via l'appel système `statvfs` en lecture seule, jamais en
  lançant `systemctl`, `apt` ou `docker`.
- **Aucune lecture non bornée.** Une lecture qui nomme un chemin arbitraire serait un oracle
  de métadonnées sur tout le système de fichiers. Une lecture n'est donc ouverte que si elle
  est bornée, nommée et non divulgante ; `path_status` est allowlistée (`[paths] allow`, vide
  par défaut), renvoie des métadonnées et jamais le contenu, ne suit pas le dernier composant,
  et renvoie une vraie erreur — pas « absent » — sur tout sauf `ENOENT`. Les autres lectures
  portent sur un ensemble fixe et bien connu de fichiers (`/proc`, `/etc/os-release`, les
  marqueurs de redémarrage sous `/run`) et ne renvoient aucun contenu de fichier.
- **Ni installation, ni suppression, par construction.** Le vocabulaire des opérations sur les
  paquets est une énumération fermée, sans variante install/remove ; les mises à niveau
  portent `ONLY_TRUSTED`, et un plan qui installerait ou supprimerait un paquet est refusé.
  Docker n'a aucune opération de création ni de suppression.
- **Moindre privilège.** Tourner sous un utilisateur dédié non privilégié ; les mutations
  systemd et paquets sont appliquées deux fois (l'allowlist de l'outil et la règle polkit).
  Docker n'a pas de polkit : son socket équivaut à root, donc accorde-le étroitement (un proxy
  de socket) et garde `[docker] manage_containers` serré.
- **Auth.** Clés publiques uniquement, associées à un principal ; les autorisations par clé
  limitent ce qu'une clé peut atteindre. Le client épingle les empreintes de clés d'hôte, donc
  SSH n'est pas ouvert au MITM.
- **Les hôtes de saut relaient, ils ne terminent pas.** Un nœud joint via une entrée
  `[[jumps]]` l'est par un canal SSH `direct-tcpip` que le client ouvre sur l'hôte de saut,
  dont il épingle aussi l'empreinte. La session vers le nœud reste chiffrée de bout en bout et
  ses autorisations par clé s'appliquent toujours ; un hôte de saut compromis peut refuser le
  service, pas lire ni forger la session (ADR 0013).
- **Refus par défaut.** Pas de bind, pas de clé ou pas de clé d'hôte signifie pas de démarrage ;
  aucune opération paquet autorisée signifie aucun rafraîchissement ni mise à niveau.

## Hors périmètre pour l'instant

- La limitation de débit est laissée au pare-feu et à systemd.
- La rotation des clés exige un redémarrage.
- Les certificats OpenSSH ne sont pas pris en charge.
- `path_status` divulgue des métadonnées (existence, taille, propriétaire, dates) pour les
  chemins déclarés ; c'est délibéré, borné par `[paths] allow`, et désactivé par défaut.
- `get_reboot_status` divulgue l'identité de distribution et l'état de redémarrage de l'hôte —
  c'est le but de l'outil — et renvoie `null` sur les hôtes dont le mécanisme est inconnu de
  Gilfoyle.

Le transport SSH ajoute une surface de parseur avant authentification et un backend crypto C
(`ring`) — un compromis assumé.
