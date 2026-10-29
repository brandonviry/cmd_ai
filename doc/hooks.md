---
title: Hooks
description: Déclarer des hooks dans config.json, des déclencheurs déterministes exécutés par cmd_ai et non par le modèle - événements, motifs, variables CMD_AI_*, blocage par le code de sortie 2, et exemples.
---

Un **hook** est une commande que **vous** déclarez dans `config.json`, et que cmd_ai lance tout seul à un moment précis : au démarrage, avant ou après une commande, quand un fichier change, à la fin d'une tâche. Exemple : « quand `package.json` est modifié, vérifie qu'il est toujours valide ».

Les hooks sont la version « système et robuste » des extensions : c'est le code de cmd_ai qui les déclenche, de façon déterministe, **jamais le modèle**. Pour guider l'agent avec des consignes, voyez plutôt les [skills](./skills.md).

## Les règles

- Les hooks sont **rédigés par vous**, dans `config.json`. Le modèle ne peut ni en déclencher, ni en créer, ni en modifier, ni en désactiver, et cmd_ai n'en écrit jamais de lui-même.
- Ils sont **pré-approuvés** : ils s'exécutent sans demande de validation, même en mode plan.
- Ils sont **toujours visibles** : chaque hook s'affiche comme une commande, marqué `[hook]`, avec sa sortie, son code et sa durée.
- Ils passent par la même porte d'exécution que les commandes de l'agent : entrée standard fermée, délai d'expiration, arbre de processus tué en cas de dépassement ou d'annulation, environnement nettoyé (la clé API n'y figure jamais).
- Pour un même événement, les hooks s'exécutent dans l'ordre où ils sont déclarés.

## Les événements

| Événement | Quand | Filtre |
| --- | --- | --- |
| `session_start` | au démarrage d'une session cmd_ai | aucun |
| `pre_command` | juste **avant** chaque commande de l'agent | `match` (motif sur la commande) |
| `post_command` | juste **après** chaque commande de l'agent | `match` (motif sur la commande) |
| `file_changed` | juste après une commande qui a modifié un fichier surveillé | `paths` (fichiers surveillés) |
| `task_end` | à la fin de chaque tâche | aucun |

Autour d'une commande, l'ordre est toujours le même :

```text
hooks pre_command → commande → hooks post_command → hooks file_changed
```

## Le format

### Hooks d'événement (`session_start`, `pre_command`, `post_command`, `task_end`)

| Champ | Type | Obligatoire | Rôle |
| --- | --- | --- | --- |
| `run` | chaîne | oui | la commande à lancer |
| `match` | chaîne | non | motif glob comparé à la commande de l'agent (`pre_command` et `post_command`) ; absent, le hook vaut pour toutes les commandes |
| `shell` | chaîne ou `null` | non | `pwsh`, `powershell`, `cmd`, `bash`, `sh` ou `zsh` ; absent ou `null` : le shell par défaut de la machine |
| `timeout_secs` | entier | non | délai maximal du hook, en secondes |

### Hooks de fichiers (`file_changed`)

| Champ | Type | Obligatoire | Rôle |
| --- | --- | --- | --- |
| `paths` | tableau de chaînes | oui | les fichiers à surveiller, relatifs au répertoire courant de la session |
| `run` | chaîne | oui | la commande à lancer |
| `shell` | chaîne ou `null` | non | comme ci-dessus |
| `timeout_secs` | entier | non | comme ci-dessus |

### Les motifs `match`

Les mêmes que pour les [garde-fous](./modes.md) : `*` remplace n'importe quelle suite de caractères, `?` un seul caractère, la casse et les espaces multiples sont ignorés, et le motif doit correspondre à toute la commande. `git push*` attrape `git push`, `git push origin main`, `GIT PUSH --tags`… mais pas `cd repo && git push` : pour cela, écrivez `*git push*`.

### Comment `file_changed` détecte un changement

Avant chaque commande de l'agent, cmd_ai note la date de modification et la taille des fichiers de `paths` ; après la commande, il compare. Si un fichier a changé, les hooks concernés s'exécutent.

Ce n'est pas une surveillance continue : un fichier modifié par un autre programme entre deux commandes de l'agent ne déclenche rien.

## Les variables `CMD_AI_*`

cmd_ai passe à chaque hook des variables d'environnement qui décrivent l'événement :

| Variable | Événements | Contenu |
| --- | --- | --- |
| `CMD_AI_EVENT` | tous | le nom de l'événement (`pre_command`, `file_changed`…) |
| `CMD_AI_COMMAND` | `pre_command`, `post_command` | le texte de la commande de l'agent |
| `CMD_AI_FILE` | `file_changed` | le chemin du fichier modifié |

Pour les lire, selon le shell du hook : `$CMD_AI_FILE` (bash, sh, zsh), `$env:CMD_AI_FILE` (PowerShell), `%CMD_AI_FILE%` (cmd).

## Bloquer une commande : le code de sortie 2

Un hook `pre_command` qui se termine avec le **code 2** **bloque** la commande : elle n'est pas exécutée, et le modèle reçoit un message `REFUSÉ` qui l'en informe (il peut alors proposer autre chose). Tout autre code laisse la commande s'exécuter ; la sortie du hook reste affichée.

Le code 2 n'a d'effet que pour `pre_command` : après coup (`post_command`, `file_changed`), il est trop tard pour bloquer.

## Exemples

Ce sont des extraits de `config.json` : la section `hooks` se place au premier niveau du fichier, à côté de `provider` (voir [Configuration](./configuration.md)).

### Un état des lieux avant chaque `git push`

```json
"hooks": {
  "pre_command": [
    { "match": "git push*", "run": "git status --short", "timeout_secs": 30 }
  ]
}
```

### Interdire un push direct sur `main` (bash)

```json
"hooks": {
  "pre_command": [
    {
      "match": "*git push*",
      "shell": "bash",
      "run": "if [ \"$(git branch --show-current)\" = main ]; then echo 'Push direct sur main interdit' >&2; exit 2; fi"
    }
  ]
}
```

### La même chose en PowerShell

```json
"hooks": {
  "pre_command": [
    {
      "match": "*git push*",
      "shell": "pwsh",
      "run": "if ((git branch --show-current) -eq 'main') { Write-Output 'Push direct sur main interdit'; exit 2 }"
    }
  ]
}
```

### Vérifier `package.json` dès qu'il change

```json
"hooks": {
  "file_changed": [
    { "paths": ["package.json"], "run": "npm install --dry-run" }
  ]
}
```

### Montrer l'état du dépôt git au démarrage

```json
"hooks": {
  "session_start": [
    { "run": "git status --short", "timeout_secs": 10 }
  ]
}
```

Sans `shell`, le hook utilise le shell par défaut de la machine : `git status --short` s'écrit de la même façon en PowerShell et en bash.

### Afficher le fichier modifié (bash)

```json
"hooks": {
  "file_changed": [
    { "paths": ["config/app.yml"], "shell": "bash", "run": "echo \"Modifié : $CMD_AI_FILE\"" }
  ]
}
```

## Conseils

- **Pensez multi-OS.** Le même `config.json` sert Windows, Linux et macOS. Un hook écrit pour `bash` ne tournera pas sur un Windows sans bash : précisez `shell` et préférez des commandes disponibles partout (`git`, `npm`…).
- **Gardez-les courts et rapides** : ils s'exécutent à chaque fois, sans confirmation. Donnez un `timeout_secs` aux hooks qui interrogent le réseau.
- **Attention à qui peut modifier `config.json`** : un hook s'exécute sans validation. Quelqu'un qui modifie ce fichier sur votre clé peut faire lancer n'importe quelle commande.
