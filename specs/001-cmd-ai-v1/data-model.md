# Modèle de données

Les types Rust font foi : [contracts/rust-api.md](contracts/rust-api.md). Résumé des entités et de leurs règles.

| Entité | Où | Règles clés |
| --- | --- | --- |
| `Home` | mémoire | racine = dossier de l'exécutable (sauf `--home`/`CMD_AI_HOME`) ; `writable` testé par fichier sonde ; éphémère sinon |
| `Config` | `config.json` | `version` = 1 ; clés inconnues refusées ; clé API = `Secret` ; `OLLAMA_API_KEY` prioritaire ; jamais réécrit |
| `CommandRequest` | arguments du tool | `command` et `purpose` obligatoires ; `plan` seulement au premier appel |
| `Plan` | état de l'agent | liste fermée approuvée ; une commande hors plan → nouvelle validation ; `n/M` |
| `ExecResult` | exécution | code, sorties plafonnées (1 Mio/flux), durée, `timed_out`, `cancelled`, `truncated` |
| `Skill` | `skills/<nom>/SKILL.md` | `name` = nom du dossier, `[a-z0-9-]`, ≤ 64 ; `description` ≤ 1024 |
| `Memory` | `memory/MEMORY.md` | une note par ligne `- …` ; ≤ 200 notes ; ≤ 25 Kio injectés |
| `ResumeState` | `memory/resume.json` | écrit avant relance élevée, supprimé à la lecture |
| `SessionLog` | `log/AAAA-MM-JJ_HHMMSS.log` | lignes horodatées UTC, masquées ; fichier ≤ 10 Mio ; total ≤ 50 Mio (rotation) |

## Cycle d'une tâche (machine à états de l'agent)

```text
Saisie → Réflexion (LLM) ─┬─ réponse texte ─────────────────────────────→ Terminée
                          └─ appel run_command → Validation ─┬─ refus → message REFUSÉ au modèle → Réflexion
                                                             └─ accord → [élévation] → hooks pre → Exécution → hooks post/file → résultat au modèle → Réflexion
Plafonds : 30 étapes ; 2 corrections par commande ; 2 relances d'appel invalide ; 3 reprises réseau.
```
