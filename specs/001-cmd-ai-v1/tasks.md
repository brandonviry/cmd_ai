---

description: "Tasks for cmd_ai V1"
---

# Tasks: cmd_ai V1

**Input**: Design documents from `/specs/001-cmd-ai-v1/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Tests are MANDATORY for every feature (constitution, principe IX, NON NÉGOCIABLE). Each implementation task is preceded by its test task (written first, seen failing). A task without its tests is not done. Every phase also includes the `doc/` and `CLAUDE.md` update tasks it requires.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [x] T001 Cargo project `cmd_ai` (lib + bin), `rust-toolchain.toml`, release profile (size), `.cargo/config.toml` (+crt-static MSVC), `clippy.toml` (disallowed-methods: process spawning), `deny.toml` (bans tokio/async-std/smol/reqwest/openssl), `.gitignore` (config.json, log/, memory/, target/, demo/)
- [x] T002 Skeleton: every module of plan.md declared in `src/lib.rs`, `src/secret.rs` complete with tests
- [x] T003 [P] `portable/`: `config.example.json`, `cmd_ai.sh` (POSIX, dispatch uname), `cmd_ai.command`, `skills/exemple-projet-npm/SKILL.md`, `log/.gitkeep`, `memory/.gitkeep`

## Phase 2: Foundational (blocking)

- [x] T010 [P] Tests then impl `src/text.rs` (all French strings, help text)
- [x] T011 [P] Tests then impl `src/home.rs` (resolution order, probe file, ephemeral)
- [x] T012 [P] Tests then impl `src/config.rs` (schema v1, defaults, unknown keys, env override, http-only-loopback, model resolution, default garde_fous)
- [x] T013 [P] Tests then impl `src/log.rs` (UTC timestamp, rotation, redaction, size cap, disabled)
- [x] T014 [P] Tests then impl `src/cli.rs` (all options, exclusivity, errors → code 2)
- [x] T015 [P] Tests then impl `src/platform/` (host info, privilege, OEM decode, Windows relaunch)
- [x] T016 [P] Tests then impl `src/shell.rs` (detection without spawning, args per shell incl. cmd raw_arg quoting, child_env scrubbing)
- [x] T017 Tests then impl `src/exec.rs` (only spawn gate: captured/detached/inherited, stdin null, timeout, tree kill, cancel, capture cap, accents)
- [x] T018 [P] Tests then impl `src/output.rs` (control sequences, reduce 16 KiB head/tail, repeated lines, untrusted frame)
- [x] T019 [P] Tests then impl `src/risk.rs` (glob, garde-fous, admin rules, sudo strip, GUI launchers, access denied)
- [x] T020 [P] Tests then impl `src/llm/sse.rs` + `src/llm/mod.rs` types
- [x] T021 Tests then impl `src/llm/client.rs` (request body, SSE assembly of content/reasoning/tool_calls, usage, error classification) against a local test server
- [x] T022 [P] Tests then impl `src/tool.rs` (schema has exactly one tool, tolerant parsing, self-commands)
- [x] T023 [P] Tests then impl `src/ui/theme.rs` (palette, contrast ≥ 4.5:1 dark+light, colour levels, NO_COLOR)
- [x] T024 [P] Test harness `tests/common/mod.rs` (fake Ollama SSE server, temp cmd_ai folder, binary runner)

## Phase 3: User Story 1 — one-shot (P1) 🎯 MVP

- [x] T030 [US1] Tests (fake backend/executor/frontend) then impl `src/agent.rs` (loop, plan approval, closed plan, bypass + garde-fous, malformed calls, fixes cap, step cap, retries, reasoning)
- [x] T031 [US1] Tests then impl `src/ui/mod.rs` + `src/ui/plain.rs` (TUI fermé output, approval on stdin, Unavailable without TTY)
- [x] T032 [US1] Impl `src/lib.rs::run` wiring + `src/main.rs`; integration tests `tests/oneshot.rs` (approve, bypass, no-TTY code 3, --prompt-file, malformed call, exit codes)

## Phase 4: User Story 2 — TUI (P1)

- [x] T040 [US2] Tests (TestBackend buffer assertions) then impl `src/ui/tui.rs` (inline viewport, insert_before, status bar, progress n/M, plan approval, input line, /commands, Esc cancel, terminal restore, panic hook)

## Phase 5: User Story 3 — portability (P2)

- [x] T050 [US3] Integration tests `tests/portability.rs` (sentinel host dirs stay empty, config next to exe not cwd, missing config code 4, read-only → ephemeral)
- [x] T051 [US3] First-run setup (interactive: copy config.example.json → config.json, ask API key) in `src/lib.rs`
- [x] T052 [US3] Integration tests `tests/secrets.rs` (canary key absent from log/ and child env; OLLAMA_API_KEY override)

## Phase 6: User Story 4 — privileges (P2)

- [x] T060 [US4] Tests then impl elevation flow in `src/agent.rs` + `src/session.rs` (Unix sudo -n / handover sudo -v with fake sudo; Windows resume save/load + relaunch)

## Phase 7: User Story 5 — skills (P3)

- [x] T070 [P] [US5] Tests then impl `src/skills.rs` (frontmatter, name validation, catalog, traversal refused) + agent interception

## Phase 8: User Story 6 — hooks (P3)

- [x] T080 [P] [US6] Tests then impl `src/hooks.rs` (matching, env vars, pre_command code 2 blocks, file snapshot) + agent integration

## Phase 9: User Story 7 — memory & model roles (P3)

- [x] T090 [P] [US7] Tests then impl `src/memory.rs` (add/list/forget, cap, prompt block, read-only) + `--model`/`/model` roles

## Phase 10: Polish & cross-cutting

- [x] T100 [P] `doc/` in French (index, installation, configuration, utilisation, modes, droits, skills, hooks, memoire, portabilite, securite, codes-de-sortie, tests-manuels, bugs, faq) with `title`/`description` frontmatter
- [x] T101 [P] `doc-site/` Fumadocs reading `../doc` and `../bug`, static export, basePath, French, sitemap, Google verification meta; `pnpm build` passes
- [x] T102 [P] `bug/README.md` + `bug/_modele.md`; `.github/ISSUE_TEMPLATE/bug.yml`; `SECURITY.md`
- [x] T103 [P] `.github/workflows/ci.yml` (fmt, clippy, test matrix 3 OS, budgets, guards: no model names in src, no colour literals outside theme, no registry writes, deny, gitleaks), `release.yml` (tag → portable zip + SHA-256), `pages.yml`
- [x] T104 `CLAUDE.md` (project index) + `.claude/hooks/` Stop hook (closing recap) merged in `.claude/settings.json`; `README.md`, `LICENSE` (MIT), `CHANGELOG.md`
- [x] T105 Release build: size budget, `--version` timing, `cargo clippy -D warnings`, full `cargo test`; adversarial code review and fixes; `/speckit-analyze` consistency pass
