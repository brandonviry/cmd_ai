# Contrat : `config.json` (schéma version 1)

Fichier UTF-8 à la racine du dossier cmd_ai. Clés inconnues refusées (message en français avec le chemin de la clé). Seuls `version`, `provider.base_url` et `provider.model` sont obligatoires ; tout le reste a une valeur par défaut. `config.example.json` (dans `portable/`) est livré avec les releases ; `config.json` n'est jamais écrasé par cmd_ai.

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
  "garde_fous": ["…liste par défaut, voir ci-dessous…"],
  "hooks": {
    "session_start": [],
    "pre_command":  [{ "match": "git push*", "run": "git status --short", "shell": null, "timeout_secs": 30 }],
    "post_command": [],
    "file_changed": [{ "paths": ["package.json"], "run": "npm install --dry-run" }],
    "task_end": []
  }
}
```

| Clé | Type | Défaut | Règle |
| --- | --- | --- | --- |
| `version` | entier | — | `1` ; autre valeur → erreur « version non prise en charge » (code 4), fichier intact |
| `provider.base_url` | chaîne | — | `https://…` ; `http://` seulement vers `localhost`, `127.0.0.1`, `[::1]` ; sans `/` final |
| `provider.api_key` | chaîne | `""` | remplacée par `OLLAMA_API_KEY` si non vide ; vide autorisé (Ollama local) |
| `provider.model` | chaîne | — | modèle par défaut (nom de l'API directe, ex. `beam`) |
| `provider.roles` | objet | `{}` | rôle → modèle ; `--model <rôle>` ou `/model <rôle>` |
| `provider.max_tokens` | entier | 16384 | ≥ 256 |
| `provider.timeout_secs` | entier | 300 | délai total d'une réponse du modèle |
| `permissions` | `"plan"`\|`"bypass"` | `"plan"` | surchargé par `--plan` / `--bypass` |
| `ui.tui` | booléen | `true` | `false` = TUI fermé par défaut |
| `ui.color` | `auto`\|`truecolor`\|`256`\|`16`\|`none` | `auto` | `NO_COLOR` non vide force `none` en `auto` |
| `ui.theme` | `auto`\|`dark`\|`light` | `auto` | `auto` : `COLORFGBG` si présent, sinon sombre |
| `ui.show_reasoning` | booléen | `false` | affiche le raisonnement (atténué) |
| `network.proxy` | chaîne\|null | `null` | URL de proxy ; `null` = variables d'environnement puis système |
| `network.ca_file` | chaîne\|null | `null` | certificat PEM supplémentaire, chemin relatif au dossier cmd_ai |
| `limits.*` | entiers | voir JSON | > 0 |
| `garde_fous` | tableau de motifs | liste par défaut | motifs glob (`*`, `?`), insensibles à la casse ; `[]` désactive |
| `hooks.*` | tableaux | `[]` | voir `Hook` / `FileHook` du contrat Rust |

## Résolution du modèle (`--model X`)

1. `X` absent → `provider.model`.
2. `X` est une clé de `provider.roles` → sa valeur.
3. `X` contient `:` ou `/` ou `.` → nom de modèle brut.
4. Sinon → erreur « rôle inconnu » listant les rôles (code 4).

## Garde-fous par défaut

Motifs qui demandent confirmation même en bypass : effacement récursif d'une racine ou du dossier personnel (`rm -rf /`, `rm -rf ~`, `rm -rf /*`, `Remove-Item … -Recurse … C:\`, `rd /s /q C:\`, `del /s /q C:\`), formatage et partitionnement (`format X:`, `Format-Volume`, `Clear-Disk`, `diskpart`, `mkfs*`, `wipefs`, `fdisk`, `parted`), écriture brute (`dd … of=/dev/…`, `> /dev/sd…`), destruction (`shred`, `cipher /w`, `vssadmin delete`), démarrage (`bcdedit`), registre système (`reg delete HKLM…`), permissions récursives sur `/`, fork bomb, exécution distante pipée (`curl … | sh`, `wget … | sh`, `irm … | iex`, `iwr … | iex`), extinction (`shutdown`, `Stop-Computer`, `Restart-Computer`, `reboot`, `poweroff`).
