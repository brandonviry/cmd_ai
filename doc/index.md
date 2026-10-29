---
title: cmd_ai
description: L'agent en terminal qui comprend vos demandes en français et exécute les commandes à votre place, sous Windows, Linux et macOS, depuis une simple clé USB.
---

**Ne tapez plus jamais une commande.** cmd_ai est un agent en terminal : vous lui dites ce que vous voulez en français, il écrit les commandes adaptées à la machine (PowerShell, Bash ou CMD), vous les montre, puis les exécute.

Il tient dans un seul dossier, sans installation, que vous pouvez emporter sur une clé USB et lancer sous Windows, Linux ou macOS.

## La promesse

- **Un seul outil : exécuter des commandes.** cmd_ai ne fait rien d'autre. Il n'a pas d'accès caché aux fichiers ou au web : tout passe par des commandes que vous voyez.
- **Vous gardez la main.** Par défaut, le [mode plan](./modes.md) vous présente la suite complète des commandes et attend votre accord avant d'exécuter quoi que ce soit. Le mode bypass, plus rapide, ne s'active que si vous le décidez.
- **Tout est visible.** Chaque commande s'affiche avant de tourner (texte exact, shell, niveau de droits, explication), puis sa sortie en direct, son code de sortie et sa durée, avec la progression `1/4`, `2/4`…
- **Portable, rien sur l'hôte.** Configuration, clé API, journaux et mémoire vivent dans le dossier de cmd_ai, jamais dans le système. Les [limites honnêtes](./portabilite.md) de cette règle sont documentées.
- **Léger.** Un binaire de quelques Mio par OS, qui démarre instantanément.
- **Modèles open weight** via [Ollama Cloud](https://ollama.com) (seul fournisseur pris en charge en V1), ou via un serveur Ollama local.

## Visite rapide

Vous lancez cmd_ai et vous tapez votre demande :

```text
> quels sont les 5 plus gros fichiers de ce dossier ?

Plan (1 commande) :
  1. pwsh · utilisateur — Lister les 5 fichiers les plus volumineux
     Get-ChildItem -File -Recurse | Sort-Object Length -Descending | Select-Object -First 5 Name, Length
Exécuter ce plan ? [o/N]
```

Vous validez avec `o`, la commande tourne, sa sortie s'affiche, puis le modèle vous résume le résultat en français. (Rendu simplifié : l'affichage réel est en couleurs, avec une barre d'état.)

Trois façons de lancer cmd_ai :

```text
cmd_ai                              TUI interactif : vous tapez vos demandes
cmd_ai --prompt "…"                 une demande, suivie en direct
cmd_ai --prompt-file demande.txt    une demande lue dans un fichier
```

Et pour les scripts, une exécution sans interface ni confirmation :

```text
cmd_ai --no-tui --bypass --prompt "…"
```

Le résultat se lit alors dans les [codes de sortie](./codes-de-sortie.md).

## Par où commencer

| Je veux… | Page |
| --- | --- |
| Installer cmd_ai et créer ma clé API | [Installation](./installation.md) |
| Savoir comment on s'en sert au quotidien | [Utilisation](./utilisation.md) |
| Régler le modèle, le proxy, les limites | [Configuration](./configuration.md) |
| Comprendre le mode plan, le bypass et les garde-fous | [Modes](./modes.md) |
| Lancer des commandes administrateur | [Droits](./droits.md) |
| Écrire des playbooks pour l'agent | [Skills](./skills.md) |
| Déclencher des vérifications automatiques | [Hooks](./hooks.md) |
| Faire retenir des informations à l'agent | [Mémoire](./memoire.md) |
| Utiliser une clé USB sur plusieurs OS | [Portabilité](./portabilite.md) |
| Protéger ma clé API et comprendre les risques | [Sécurité](./securite.md) |
| Interpréter le code de retour dans un script | [Codes de sortie](./codes-de-sortie.md) |
| Vérifier une release à la main | [Tests manuels](./tests-manuels.md) |
| Signaler un bug | [Bugs](./bugs.md) |
| Trouver une réponse rapide | [FAQ](./faq.md) |

## Ce que la V1 ne fait pas (encore)

- **Un seul fournisseur** : Ollama (cloud ou local). D'autres fournisseurs pourront venir plus tard.
- **Binaires non signés** : Windows (SmartScreen, Smart App Control, AppLocker) ou macOS (Gatekeeper) peuvent bloquer le premier lancement. Les contournements sont décrits dans [Portabilité](./portabilite.md) ; la signature de code est prévue avant la 1.0.
- **Pas de terminal virtuel** : une commande qui attend une saisie clavier n'en reçoit pas, sauf si l'agent demande une étape « interactive » qui vous rend le terminal (voir [Utilisation](./utilisation.md)).
- **Pas de routage automatique des modèles** : vous choisissez le modèle à la main (voir [Configuration](./configuration.md)).
