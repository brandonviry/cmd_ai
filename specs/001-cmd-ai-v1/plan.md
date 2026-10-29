# Implementation Plan: cmd_ai V1

**Branch**: `001-cmd-ai-v1` | **Date**: 2026-10-29 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-cmd-ai-v1/spec.md`

## Summary

Un binaire Rust unique (`cmd_ai`) : un agent dont le seul tool est `run_command`, qui parle à Ollama Cloud via l'endpoint compatible OpenAI (SSE), présente un plan, exécute les commandes par une porte unique (`exec`), affiche tout en direct (TUI inline ratatui ou sortie texte), et ne lit/écrit que dans son dossier portable. Synthèse de la recherche : [research.md](research.md) (sources complètes : [research-sources.md](research-sources.md)).

## Technical Context

**Language/Version**: Rust stable 1.97 (épinglé par `rust-toolchain.toml`), édition 2024.

**Primary Dependencies** (runtime, toutes features par défaut désactivées sauf besoin) : `ratatui` 0.30 (`crossterm`, `scrolling-regions`), `ureq` 3.5 (`rustls`, `platform-verifier` ; `win-system-proxy` sous Windows), `serde` + `serde_json`, `lexopt`, `process-wrap` 10 (`std`, `job-object`, `creation-flags`, `process-group`), `windows-sys` (Windows), `libc` (Unix). Aucune dépendance async.

**Storage**: fichiers dans le dossier portable uniquement (`config.json`, `log/*.log`, `memory/MEMORY.md`, `memory/resume.json`, `skills/*/SKILL.md`).

**Testing**: `cargo test` — unités inline, intégration dans `tests/` (vrai binaire + faux serveur Ollama en `std::net::TcpListener`), instantanés `ratatui::TestBackend` (assertions sur le buffer, sans crate d'instantanés).

**Target Platform**: Windows 10 1809+/11 x64 (MSVC, CRT statique), Linux x86_64/aarch64 (musl statique), macOS universel (x86_64 + aarch64).

**Project Type**: CLI/TUI (binaire unique + bibliothèque interne pour les tests).

**Performance Goals**: `--version` ≤ 50 ms ; premier rendu du TUI sans E/S réseau.

**Constraints**: binaire ≤ 5 Mio ; ≤ 15 dépendances directes ; une requête LLM en vol ; rien hors du dossier.

**Scale/Scope**: V1 = feature 001 entière (7 user stories).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principe | Statut | Comment le design le respecte |
| --- | --- | --- |
| I. Zéro commande à taper | ✅ | Contexte hôte injecté (OS, arch, shell, droits) ; corrections bornées (`max_fixes`) ; `--prompt`/`--prompt-file` lus avant l'UI. |
| II. Un seul tool, une seule porte | ✅ | `tool::schema()` déclare `run_command` seul ; `exec` est le seul module autorisé à créer des processus (`clippy.toml` `disallowed-methods`) ; auto-commandes interceptées dans `agent`. |
| III. Portabilité, rien sur l'hôte (NN) | ✅ | `home` résout depuis `current_exe` ; mode éphémère ; `shell::child_env` ; test des sentinelles. |
| IV. Exécution visible et robuste | ✅ | Événements `CommandStart/Output/End` ; `n/M` ; entrée fermée, délai, kill d'arbre (`process-wrap`) ; `output::sanitize` ; codes de sortie ; pas de `unwrap` hors tests (clippy). |
| V. L'humain garde la main | ✅ | Machine à états de `agent` (plan fermé, revalidation, garde-fous, plafond d'étapes) ; `risk::classify` ; élévation via OS seulement. |
| VI. Secrets, sorties non fiables (NN) | ✅ | `secret::Secret` + `Redactor` appliqué à toute sortie ; env enfant nettoyé ; cadre `<<<SORTIE NON FIABLE …>>>` ; neutralisation des séquences de contrôle. |
| VII. Français et couleurs | ✅ | `text` (tous les textes) ; `ui::theme` (palette unique, test de contraste) ; prompt système en français. |
| VIII. Légèreté mesurée | ✅ | 8 dépendances directes ; profil release `opt-level="z"`, `lto`, `codegen-units=1`, `strip`, `panic="abort"` ; job CI `budgets`. |
| IX. Tests d'abord (NN) | ✅ | Chaque tâche de `tasks.md` commence par ses tests ; CI sur 3 OS. |

Re-check après Phase 1 : ✅ (aucune violation ; nouvelles crates justifiées ci-dessous).

## Project Structure

### Documentation (this feature)

```text
specs/001-cmd-ai-v1/
├── spec.md
├── plan.md               # ce fichier
├── research.md           # synthèse FR de la recherche
├── research-sources.md   # rapport brut (EN) avec toutes les sources
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── rust-api.md       # interfaces entre modules (contrat d'implémentation)
│   ├── config.md         # schéma de config.json
│   └── cli-and-tool.md   # CLI, codes de sortie, tool run_command, auto-commandes
└── tasks.md
```

### Source Code (repository root)

```text
Cargo.toml  Cargo.lock  rust-toolchain.toml  clippy.toml  deny.toml  .cargo/config.toml
src/
├── main.rs            # std::process::exit(cmd_ai::run())
├── lib.rs             # câblage : cli → home → config → log → mode (tui | plain)
├── text.rs            # TOUS les textes utilisateur (français)
├── cli.rs             # lexopt → Args
├── home.rs            # dossier portable, inscriptibilité, mode éphémère
├── config.rs          # Config versionnée, validation, OLLAMA_API_KEY, rôles
├── secret.rs          # Secret (masqué) + Redactor
├── log.rs             # SessionLog (horodatage UTC maison, rotation, masquage)
├── platform/          # mod.rs + windows.rs + unix.rs : OS, arch, droits, OEM, élévation
├── shell.rs           # détection, construction de la commande, env enfant
├── exec.rs            # SEULE porte de création de processus
├── output.rs          # neutralisation, réduction (16 Kio), cadre non fiable
├── risk.rs            # admin, garde-fous (glob), lanceurs GUI, refus d'accès
├── llm/               # mod.rs (types, trait ChatBackend), sse.rs, client.rs (ureq)
├── tool.rs            # schéma run_command, parsing tolérant, auto-commandes
├── skills.rs  memory.rs  hooks.rs  session.rs
├── agent.rs           # boucle agent (générique : ChatBackend, Executor, Frontend)
└── ui/                # mod.rs (trait Frontend, événements), theme.rs, plain.rs, tui.rs
tests/
├── common/mod.rs      # faux serveur Ollama (SSE), dossier cmd_ai temporaire, lanceur du binaire
├── oneshot.rs  portability.rs  secrets.rs  errors.rs
portable/
├── config.example.json  cmd_ai.sh  cmd_ai.command  skills/exemple-projet-npm/SKILL.md
doc/  doc-site/  bug/  .github/  CLAUDE.md
```

**Structure Decision**: projet unique (bibliothèque + binaire) ; le paquet portable est assemblé en CI à partir de `portable/` et des binaires.

## Complexity Tracking

> Nouvelles crates (constitution VIII) — aucune violation de principe.

| Crate | Besoin | Alternative écartée | Impact |
| --- | --- | --- | --- |
| `ratatui` | TUI inline, rendu testable | crossterm brut (tout à réécrire, pas de TestBackend) | ~0,6 Mio |
| `ureq` | HTTPS bloquant + streaming + proxy | `reqwest` (tokio, interdit) ; `minreq` (pas de verifier plateforme) | ~1,9 Mio avec rustls |
| `serde`/`serde_json` | config + protocole JSON | parseur maison (risqué) | ~0,3 Mio |
| `lexopt` | arguments | `clap` (~0,5 Mio) | ~20 Kio |
| `process-wrap` | Job Objects / groupes de processus | code FFI maison | ~50 Kio |
| `windows-sys` | jeton d'élévation, ShellExecuteExW, pages de code | `winapi` (non maintenu) | liaison seulement |
| `libc` | `geteuid` | lecture `/proc` (non portable macOS) | liaison seulement |
