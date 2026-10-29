---
title: Configuration
description: Toutes les clés de config.json (schéma version 1) avec leurs types et valeurs par défaut, le choix du modèle et des rôles, le proxy et les certificats des réseaux d'entreprise, et les limites.
---

Toute la configuration tient dans un seul fichier, `config.json`, placé **à côté de l'exécutable** dans le dossier de cmd_ai. C'est un fichier JSON en UTF-8, partagé par les trois OS.

## Les règles du fichier

- Seuls `version`, `provider.base_url` et `provider.model` sont **obligatoires**. Tout le reste a une valeur par défaut : n'écrivez que ce que vous voulez changer.
- Les **clés inconnues sont refusées** : une faute de frappe (`"permision"`) donne un message en français qui indique le chemin de la clé fautive, et cmd_ai s'arrête avec le code 4. Rien n'est ignoré en silence.
- Une erreur de syntaxe JSON est signalée avec sa ligne et sa colonne. Le JSON n'accepte pas les commentaires ni les virgules finales.
- cmd_ai **n'écrase jamais** `config.json`. Si `version` vaut autre chose que `1`, il refuse de démarrer (« version non prise en charge », code 4) et laisse le fichier intact.
- Le dossier lu est celui de l'exécutable, jamais le répertoire courant. `--home <dossier>` ou la variable `CMD_AI_HOME` permettent d'en désigner un autre.

## Exemple complet

Voici un fichier avec toutes les sections (les valeurs indiquées sont celles par défaut, sauf `provider` et les exemples de hooks) :

```json
{
  "version": 1,
  "provider": {
    "base_url": "https://ollama.com/v1",
    "api_key": "",
    "model": "beam",
    "roles": { "fast": "gpt-oss:120b", "fr": "mistral-large-4" },
    "max_tokens": 16384,
    "timeout_secs": 300
  },
  "permissions": "plan",
  "ui": { "tui": true, "color": "auto", "theme": "auto", "show_reasoning": false },
  "network": { "proxy": null, "ca_file": null, "connect_timeout_secs": 15 },
  "limits": {
    "command_timeout_secs": 120, "max_steps": 30, "max_fixes": 2, "model_output_kib": 16,
    "capture_mib": 1, "log_file_mib": 10, "log_total_mib": 50, "memory_max_notes": 200
  },
  "hooks": {
    "session_start": [],
    "pre_command": [{ "match": "git push*", "run": "git status --short", "timeout_secs": 30 }],
    "post_command": [],
    "file_changed": [{ "paths": ["package.json"], "run": "npm install --dry-run" }],
    "task_end": []
  }
}
```

`garde_fous` n'apparaît pas ici : sans cette clé, la liste par défaut s'applique (voir [Modes](./modes.md)).

## Référence des clés

### `version`

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `version` | entier | aucun (obligatoire) | doit valoir `1` |

### `provider` : le fournisseur et le modèle

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `provider.base_url` | chaîne | aucun (obligatoire) | URL en `https://…`, sans `/` final ; `http://` seulement vers `localhost`, `127.0.0.1` ou `[::1]` |
| `provider.api_key` | chaîne | `""` | votre clé API ; remplacée par `OLLAMA_API_KEY` si cette variable est définie et non vide ; vide autorisé (Ollama local) |
| `provider.model` | chaîne | aucun (obligatoire) | modèle par défaut, avec le nom de l'API directe (`beam`, pas `beam:cloud`) |
| `provider.roles` | objet | `{}` | rôles nommés : `"rôle": "modèle"` |
| `provider.max_tokens` | entier | `16384` | longueur maximale d'une réponse du modèle, au moins `256` |
| `provider.timeout_secs` | entier | `300` | délai total accordé à une réponse du modèle, en secondes |

L'URL d'Ollama Cloud est `https://ollama.com/v1` (endpoint compatible OpenAI). cmd_ai envoie ses requêtes à `{base_url}/chat/completions`, et la clé API ne part **que** vers cette URL.

### `permissions` : le mode par défaut

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `permissions` | `"plan"` ou `"bypass"` | `"plan"` | remplacé par `--plan` ou `--bypass` en ligne de commande |

Le mode bypass supprime les confirmations : lisez [Modes](./modes.md) avant de le mettre par défaut.

### `ui` : l'interface

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `ui.tui` | booléen | `true` | `false` : TUI fermé par défaut (texte simple), sauf avec `--tui` |
| `ui.color` | `auto`, `truecolor`, `256`, `16` ou `none` | `auto` | niveau de couleurs ; en `auto`, la variable `NO_COLOR` non vide force `none` ; remplacé par `--color` |
| `ui.theme` | `auto`, `dark` ou `light` | `auto` | palette pour fond sombre ou clair ; `auto` lit la variable `COLORFGBG` si elle existe, sinon choisit le thème sombre |
| `ui.show_reasoning` | booléen | `false` | affiche le raisonnement du modèle, en atténué |

Toutes les paires texte/fond de la palette ont un contraste d'au moins 4,5:1, sur fond sombre comme sur fond clair. Si les couleurs vous gênent, `"color": "none"` ou `--color none` les retire : rien ne repose sur la couleur seule.

### `network` : proxy et certificats

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `network.proxy` | chaîne ou `null` | `null` | URL du proxy ; `null` : variables d'environnement, puis réglages du système |
| `network.ca_file` | chaîne ou `null` | `null` | certificat PEM supplémentaire à faire confiance, chemin relatif au dossier de cmd_ai |
| `network.connect_timeout_secs` | entier | `15` | délai de connexion au serveur, en secondes |

Voir plus bas « Réseaux d'entreprise ».

### `limits` : les plafonds

Toutes les limites sont des entiers strictement positifs.

| Clé | Défaut | Rôle |
| --- | --- | --- |
| `limits.command_timeout_secs` | `120` | délai d'une commande, en secondes ; au-delà, elle est tuée avec tout son arbre de processus |
| `limits.max_steps` | `30` | nombre maximal d'étapes d'une tâche, puis arrêt et retour à vous |
| `limits.max_fixes` | `2` | corrections automatiques d'une même commande en échec |
| `limits.model_output_kib` | `16` | sortie d'une commande envoyée au modèle, en Kio (un quart pris au début, le reste à la fin) |
| `limits.capture_mib` | `1` | sortie capturée et journalisée par flux (standard, erreur), en Mio ; le reste est compté et signalé |
| `limits.log_file_mib` | `10` | taille maximale d'un fichier de `log/`, en Mio |
| `limits.log_total_mib` | `50` | taille totale de `log/`, en Mio ; au-delà, les plus anciens fichiers sont supprimés |
| `limits.memory_max_notes` | `200` | nombre maximal de notes de la [mémoire](./memoire.md) |

Quelques plafonds sont fixes en V1 : 2 relances au plus quand le modèle envoie un appel d'outil invalide, 3 reprises au plus en cas d'erreur réseau ou serveur, 25 Kio de mémoire injectés au démarrage.

### `garde_fous`

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `garde_fous` | tableau de motifs | liste par défaut | motifs glob (`*`, `?`), insensibles à la casse ; `[]` désactive tout |

Ce sont les commandes qui demandent une confirmation même en mode bypass. Leur fonctionnement et la liste par défaut sont détaillés dans [Modes](./modes.md).

### `hooks`

| Clé | Type | Défaut |
| --- | --- | --- |
| `hooks.session_start` | tableau de hooks | `[]` |
| `hooks.pre_command` | tableau de hooks | `[]` |
| `hooks.post_command` | tableau de hooks | `[]` |
| `hooks.file_changed` | tableau de hooks de fichiers | `[]` |
| `hooks.task_end` | tableau de hooks | `[]` |

Le format de chaque hook est décrit dans [Hooks](./hooks.md).

## Choisir le modèle

### Les rôles

Il n'y a pas de routage automatique : vous choisissez le modèle à la main. `provider.model` est le modèle par défaut, et `provider.roles` donne des noms courts à d'autres modèles :

```json
"provider": {
  "base_url": "https://ollama.com/v1",
  "model": "beam",
  "roles": { "fast": "gpt-oss:120b", "fr": "mistral-large-4" }
}
```

Vous les sélectionnez avec `--model fast` au lancement, ou `/model fr` dans le TUI.

### La résolution de `--model`

Pour `--model X` (ou `/model X`), cmd_ai applique ces règles dans l'ordre :

1. Sans `X`, il prend `provider.model`.
2. Si `X` est un rôle de `provider.roles`, il prend le modèle de ce rôle.
3. Si `X` contient `:`, `/` ou `.`, il l'utilise tel quel comme nom de modèle (`--model gpt-oss:120b`).
4. Sinon, c'est une erreur « rôle inconnu », avec la liste des rôles disponibles (code 4 au lancement).

Un modèle dont le nom ne contient ni `:`, ni `/`, ni `.` s'utilise donc soit comme modèle par défaut, soit par un rôle.

### Les modèles proposés (au 28/10/2026)

Ce sont ceux de `config.example.json`. Les noms viennent toujours de votre `config.json` : cmd_ai n'en connaît aucun en dur.

| Rôle | Modèle | Pourquoi |
| --- | --- | --- |
| par défaut | `beam` | gratuit sur Ollama Cloud pour une durée limitée, 1 M de tokens de contexte, conçu pour l'usage d'outils ; développé surtout pour l'anglais |
| `fast` | `gpt-oss:120b` | le plus rapide mesuré sur Ollama Cloud, 128 K de contexte ; payant au token |
| `fr` | `mistral-large-4` | multilingue natif, bon en français, 1 M de contexte ; payant au token |

Les tarifs et la gratuité de Beam peuvent changer : vérifiez sur ollama.com/pricing. Si vous changez de modèle par défaut, faites un essai réel (voir [Tests manuels](./tests-manuels.md)) : la qualité des appels d'outils varie d'un modèle à l'autre.

## Réseaux d'entreprise : proxy et certificats

Sur un réseau d'entreprise, la connexion au fournisseur passe souvent par un proxy, et parfois par une inspection TLS.

### Le proxy

Avec `"proxy": null` (le défaut), cmd_ai utilise, dans l'ordre :

1. les variables d'environnement `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` (et `NO_PROXY` pour les exceptions) ;
2. sous Windows, le proxy déclaré dans les réglages du système.

Si cela ne suffit pas, indiquez le proxy explicitement :

```json
"network": { "proxy": "http://proxy.entreprise.example:8080" }
```

Les scripts de configuration automatique (PAC, WPAD) ne sont pas pris en charge en V1 : demandez l'adresse du proxy à votre service informatique.

### Les certificats

cmd_ai vérifie toujours les certificats TLS, et cette vérification **ne se désactive pas**. Il fait confiance aux certificats du système : si votre entreprise a déployé son autorité de certification sur le poste, tout fonctionne sans réglage.

Sinon, ajoutez le certificat de l'autorité (au format PEM) dans le dossier de cmd_ai et indiquez-le :

```json
"network": { "ca_file": "certs/autorite-entreprise.pem" }
```

Le chemin est relatif au dossier de cmd_ai : le certificat voyage avec la clé USB.

### Les messages d'erreur

En cas de réseau coupé, de nom introuvable ou de TLS refusé, cmd_ai n'exécute rien et affiche un message qui pointe vers `network.proxy` ou `network.ca_file`. Les erreurs du fournisseur ont chacune leur message : clé invalide (401), crédits épuisés (402), file d'attente ou quota (429), erreur du serveur (5xx). Toutes donnent le code de sortie 5 (voir [Codes de sortie](./codes-de-sortie.md)).
