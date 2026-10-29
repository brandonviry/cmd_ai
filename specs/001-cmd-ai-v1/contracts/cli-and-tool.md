# Contrat : CLI, codes de sortie, tool `run_command`, auto-commandes

## CLI

```text
cmd_ai [OPTIONS]
  --prompt <texte>        exécute cette demande (lue avant toute interface)
  --prompt-file <fichier> idem, depuis un fichier UTF-8 (chemin relatif au répertoire courant)
  --no-tui                TUI fermé : sortie texte, une seule tâche, puis fin
  --tui                   TUI ouvert même si config.ui.tui = false
  --plan | --bypass       mode de permissions (défaut : config, sinon plan)
  --model <rôle|nom>      modèle à utiliser (voir config.md)
  --home <dossier>        dossier cmd_ai (sinon CMD_AI_HOME, sinon dossier de l'exécutable)
  --color <auto|truecolor|256|16|none>
  --resume <fichier>      reprend une tâche sauvegardée (relance élevée sous Windows)
  -V, --version           « cmd_ai X.Y.Z »
  -h, --help              aide en français
```

Règles : `--prompt` et `--prompt-file` sont exclusifs (code 2) ; sans prompt et sans terminal interactif → erreur d'usage (code 2) ; stdout non terminal → TUI fermé.

## Codes de sortie

| Code | Sens |
| --- | --- |
| 0 | tâche réussie (ou TUI quitté normalement) |
| 1 | tâche échouée (commande en échec non corrigée, plafond d'étapes) |
| 2 | erreur d'usage (arguments) |
| 3 | plan non validé (refusé, ou pas de terminal pour le valider) |
| 4 | erreur de configuration (`config.json` absent/invalide, rôle inconnu, dossier introuvable) |
| 5 | erreur du provider ou du réseau (clé, crédits, quota, serveur, TLS, proxy) |
| 130 | interruption (Ctrl+C / Échap) |

## Tool `run_command` (unique)

```json
{ "type": "function", "function": {
  "name": "run_command",
  "description": "Exécute UNE commande shell sur la machine de l'utilisateur et renvoie sa sortie. …",
  "parameters": { "type": "object", "required": ["command", "purpose"], "properties": {
    "command":     { "type": "string",  "description": "La commande exacte, pour le shell indiqué." },
    "purpose":     { "type": "string",  "description": "Explication courte en français de ce que fait la commande." },
    "shell":       { "type": "string",  "enum": ["pwsh","powershell","cmd","bash","sh","zsh"], "description": "Facultatif ; par défaut le shell détecté." },
    "needs_admin": { "type": "boolean", "description": "true si la commande exige des droits administrateur/root." },
    "interactive": { "type": "boolean", "description": "true si la commande a besoin du clavier de l'utilisateur." },
    "plan":        { "type": "array", "items": { "type": "string" }, "description": "Au PREMIER appel d'une tâche en plusieurs étapes : la liste complète des commandes prévues, dans l'ordre (la première = command)." }
  } } } }
```

Le résultat renvoyé au modèle (message `tool`) :

```text
code de sortie : 0 · durée : 0,4 s [· délai dépassé] [· annulé] [· sortie tronquée]
<<<SORTIE NON FIABLE (stdout)>>>
…
<<<FIN SORTIE NON FIABLE>>>
<<<SORTIE NON FIABLE (stderr)>>>
…
<<<FIN SORTIE NON FIABLE>>>
```

Refus (plan rejeté, garde-fou refusé, hook bloquant, appel invalide) : message `tool` commençant par `REFUSÉ :` ou `ERREUR :` avec la raison.

## Auto-commandes (interceptées, jamais envoyées à un shell)

| Commande | Effet |
| --- | --- |
| `cmd_ai skill list` | liste nom + description des skills |
| `cmd_ai skill <nom>` | renvoie le corps du `SKILL.md` |
| `cmd_ai memory list` | notes numérotées |
| `cmd_ai memory add <texte>` | ajoute une note (une ligne, ≤ 500 caractères) |
| `cmd_ai memory forget <n>` | supprime la note n |

Acceptées aussi avec le préfixe `./cmd_ai`, `cmd_ai.exe`, `.\cmd_ai.exe`. Elles ne comptent pas dans `n/M` et ne demandent pas de validation.

## Commandes du TUI

`/aide`, `/plan`, `/bypass`, `/model [rôle|nom]`, `/effacer` (oublie l'historique de conversation), `/quitter` (ou Ctrl+D). Échap : annule la tâche en cours. Ctrl+C : annule, puis quitte au second appui.
