# CLAUDE.md — index du projet cmd_ai

> Agent TUI en Rust, portable sur clé USB, dont le **seul tool** est l'exécution de commandes shell (PowerShell, Bash, CMD) sous Windows, Linux et macOS. Le plus léger possible.

## Où lire quoi

| Besoin | Fichier |
| --- | --- |
| Périmètre, choix datés (modèles, tarifs, stack) | [`IDEE.md`](IDEE.md) — cahier des charges, fait foi |
| Règles non négociables | [`.specify/memory/constitution.md`](.specify/memory/constitution.md) — v1.0.0, 9 principes |
| Spec V1 | [`specs/001-cmd-ai-v1/spec.md`](specs/001-cmd-ai-v1/spec.md) |
| Plan, structure du code, crates justifiées | [`specs/001-cmd-ai-v1/plan.md`](specs/001-cmd-ai-v1/plan.md) |
| Contrat entre modules Rust | [`specs/001-cmd-ai-v1/contracts/rust-api.md`](specs/001-cmd-ai-v1/contracts/rust-api.md) |
| Schéma `config.json` | [`specs/001-cmd-ai-v1/contracts/config.md`](specs/001-cmd-ai-v1/contracts/config.md) |
| CLI, codes de sortie, tool `run_command`, auto-commandes | [`specs/001-cmd-ai-v1/contracts/cli-and-tool.md`](specs/001-cmd-ai-v1/contracts/cli-and-tool.md) |
| Tâches et avancement | [`specs/001-cmd-ai-v1/tasks.md`](specs/001-cmd-ai-v1/tasks.md) |
| Recherche (synthèse / sources) | [`research.md`](specs/001-cmd-ai-v1/research.md) / [`research-sources.md`](specs/001-cmd-ai-v1/research-sources.md) |
| Documentation utilisateur (source unique, FR) | [`doc/`](doc/) → site Fumadocs [`doc-site/`](doc-site/) → GitHub Pages |
| Bugs | [`bug/README.md`](bug/README.md) (une fiche par bug + une issue GitHub) — sécurité : [`SECURITY.md`](SECURITY.md) |
| CI / releases | [`.github/workflows/`](.github/workflows/) : `ci.yml`, `release.yml` (tag `vX.Y.Z`), `pages.yml` |

## Spec Kit (1.2.4)

Chaîne : `/speckit-constitution` → `/speckit-specify` → `/speckit-clarify` → `/speckit-plan` → `/speckit-tasks` → `/speckit-analyze` → `/speckit-implement` → `/speckit-converge`. Specs dans `specs/NNN-nom/`. Les overrides `.specify/templates/overrides/` rendent les **tests obligatoires**. Ne jamais éditer `.claude/skills/speckit-*` (réécrits par `specify integration upgrade`).

État calculé à tout moment : `python .claude/hooks/speckit_status.py`.

## Code (Rust, `src/`)

`main.rs` → `lib.rs::run` (câblage) · `text.rs` (tous les textes FR) · `cli.rs` · `home.rs` (dossier portable) · `config.rs` · `secret.rs` (masquage) · `log.rs` · `platform/` (OS, droits, OEM, élévation) · `shell.rs` · **`exec.rs` (seule porte de création de processus)** · `output.rs` · `risk.rs` · `llm/` (SSE, client ureq) · `tool.rs` · `skills.rs` · `memory.rs` · `hooks.rs` · `session.rs` · `agent.rs` (boucle) · `ui/` (`theme.rs` palette unique, `plain.rs` TUI fermé, `tui.rs` ratatui inline). Tests d'intégration : `tests/` (faux serveur Ollama dans `tests/common/`). Paquet portable : `portable/`.

## Commandes

```powershell
cargo fmt; cargo clippy --all-targets -- -D warnings; cargo test   # avant tout commit
cargo build --release                                               # binaire ≤ 5 Mio
cd doc-site; pnpm install; pnpm build                               # site de doc (jamais `pnpm dev` : géré par le propriétaire)
```

## Règles à ne jamais oublier

- Un seul tool (`run_command`) ; skills via `cmd_ai skill <nom>` intercepté ; aucun nouveau tool.
- `std::process::Command::new` interdit hors `src/exec.rs` (clippy `disallowed-methods`).
- Aucun nom de modèle dans `src/` (CI) ; aucune couleur littérale hors `src/ui/theme.rs` (CI).
- Rien n'est écrit hors du dossier cmd_ai ; clé API jamais loguée ni transmise aux enfants.
- Pas de `unwrap`/`expect`/`panic!` hors tests ; pas de tokio/reqwest/openssl (`deny.toml`).
- Textes utilisateur en français (`text.rs`, `doc/`) ; code et commits en anglais.
- Tout bug : issue GitHub + fiche `bug/<n>-<slug>.md` + test de non-régression, référencés ici.

## Bugs connus

Aucun pour l'instant (index : [`bug/README.md`](bug/README.md)).

## Récapitulatif de clôture

Chaque exécution de l'IA se termine par une section `## Récapitulatif de clôture` : checklist `[x]`/`[ ]` (fait / reste à faire), état Spec Kit de chaque spec, bugs ouverts. Imposé par le hook `Stop` (`.claude/hooks/recap_guard.py`, déclaré dans `.claude/settings.json`).
