# cmd_ai

**Ne tapez plus jamais une commande.** `cmd_ai` est un agent en terminal qui comprend vos demandes en français et exécute les commandes à votre place — PowerShell, Bash ou CMD — sous Windows, Linux et macOS.

- **Un seul outil** : exécuter des commandes, rien d'autre. Léger (un binaire de quelques Mio), démarrage instantané.
- **Vous gardez la main** : mode plan par défaut (vous validez la suite de commandes avant qu'elle tourne), mode bypass quand vous faites confiance.
- **Portable** : un dossier sur une clé USB, avec la configuration, les logs et la mémoire. Rien n'est écrit sur la machine hôte.
- **Modèles open weight** via [Ollama Cloud](https://ollama.com) (V1).

```text
$ cmd_ai --prompt "quels sont les 5 plus gros fichiers de ce dossier ?"
Plan (1 commande) :
  1. pwsh · utilisateur — Lister les 5 fichiers les plus volumineux
     Get-ChildItem -File -Recurse | Sort-Object Length -Descending | Select-Object -First 5 Name, Length
Exécuter ce plan ? [o/N] o
▶ [1/1] …
```

## Démarrage rapide

1. Téléchargez `cmd_ai-vX.Y.Z-portable.zip` depuis les [releases](../../releases) et décompressez-le (sur une clé USB formatée en **exFAT** si vous voulez l'emporter).
2. Copiez `config.example.json` en `config.json` et renseignez `provider.api_key` (clé créée sur ollama.com/settings/keys) — ou lancez `cmd_ai` : il vous propose de le faire.
3. Lancez `cmd_ai.exe` (Windows), `sh cmd_ai.sh` (Linux/macOS) ou double-cliquez `cmd_ai.command` (macOS).

```text
cmd_ai                              TUI interactif
cmd_ai --prompt "…"                 une demande, suivie en direct
cmd_ai --prompt-file demande.txt    une demande depuis un fichier
cmd_ai --no-tui --bypass --prompt … one-shot sans confirmation (scripts)
```

La V1 n'est pas signée : Windows (SmartScreen, Smart App Control) ou macOS (Gatekeeper) peuvent bloquer le premier lancement — voir la [documentation](doc/portabilite.md).

## Documentation

📖 **Documentation en ligne : <https://brandonviry.github.io/cmd_ai/>** — sources Markdown dans [`doc/`](doc/) : installation, configuration, modes, droits administrateur, skills, hooks, mémoire, portabilité, sécurité, codes de sortie.

## Développement

Développé avec [Spec Kit](https://github.com/github/spec-kit) : voir [`CLAUDE.md`](CLAUDE.md) (index du projet), la [constitution](.specify/memory/constitution.md) et [`specs/`](specs/).

```powershell
cargo test
cargo build --release
```

Licence : [MIT](LICENSE).
