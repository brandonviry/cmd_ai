# Recherche (Phase 0) — synthèse

Recherche menée le 2026-10-28, contre-vérifiée sur sources primaires (rapport complet et URL : [research-sources.md](research-sources.md)).

| Sujet | Décision | Raison | Alternatives écartées |
| --- | --- | --- | --- |
| API LLM | `POST {base_url}/chat/completions` (compatible OpenAI, SSE) | prépare d'autres providers ; Ollama Cloud l'expose avec la clé Bearer | `/api/chat` natif NDJSON (second adaptateur plus tard) |
| Sorties structurées | non utilisées | non supportées par Ollama Cloud (schéma ignoré) | `response_format` |
| Plan | argument `plan` du premier appel `run_command`, intercepté par l'app | un seul tool, pas de `tool_choice` | tool `propose_plan` (interdit), texte libre parsé |
| Erreurs provider | 401 clé ; 402 crédits épuisés ; 429 concurrence (reprise) vs quota ; 5xx (reprise) ; réponse vide (1 relance, bug connu de Beam #19270) | comportements observés en 2026 | reprises aveugles |
| HTTP | `ureq` 3.5 bloquant + rustls + certificats système (`platform-verifier`) + proxy env/registre | ~2 Mio vs ~5 Mio pour reqwest+tokio ; réseaux d'entreprise | reqwest, webpki seul |
| Concurrence | threads std + `mpsc` | pas de runtime async | tokio |
| Processus | `process-wrap` (mode `std`) : Job Object / groupe de processus ; kill-on-drop écrit à la main | kill d'arbre fiable | FFI maison |
| `cmd.exe` | `raw_arg` + citation maison | std ne cite pas correctement pour `cmd /c` (CVE-2024-24576) | `.arg()` |
| PowerShell | `-NoProfile -NonInteractive -NoLogo -Command`, préambule `[Console]::OutputEncoding=UTF8`, `PSModuleAnalysisCachePath`, télémétrie off | traces minimales, accents | `-EncodedCommand` |
| Décodage | UTF-8 puis page OEM (`GetOEMCP`) sous Windows | mojibake de PowerShell 5.1/cmd | lossy seul |
| Élévation Windows | relance de toute la session (`ShellExecuteExW runas`, `--resume`) | une fenêtre UAC ne renvoie pas sa sortie ; `sudo` Windows désactivé par défaut | Start-Process -Verb RunAs, gsudo |
| Élévation Unix | `sudo -v` terminal hérité puis `sudo -n` | le mot de passe ne passe jamais par cmd_ai | `sudo -S`, askpass |
| PTY | aucun en V1 ; étape « interactive » = terminal rendu | légèreté | portable-pty |
| TUI | ratatui inline + `insert_before` (+ `scrolling-regions`) | historique conservé dans le terminal | écran alternatif |
| Couleurs | palette OKLCH précalculée, contraste ≥ 4,5:1, NO_COLOR, niveaux truecolor/256/16/aucun | identité mesurable | détection OSC 11 (fragile sous conhost) |
| Skills | format Agent Skills (`SKILL.md`), catalogue au démarrage, corps via `cmd_ai skill <nom>` | compatible Claude Code, un seul tool | second tool |
| Hooks | config JSON inspirée des hooks Claude Code ; `file_changed` par instantané mtime/taille avant/après commande | déterministe, sans thread de surveillance | crate `notify` |
| Portabilité | dossier multi-OS, exFAT conseillé, V1 non signée | FAT32 + udisks `showexec` bloque l'ELF ; SAC/Gatekeeper bloquent les binaires non signés | — |
| Traces hôte | cmd_ai n'écrit rien hors du dossier ; traces de l'OS documentées comme hors de portée | honnêteté (principe III) | promesse « zéro trace » |
| Spec Kit | 1.2.4, skills `/speckit-*`, overrides pour rendre les tests obligatoires | tests optionnels par défaut | — |
| Docs | Fumadocs (`fumadocs-mdx` ≥ 15.4.6) lit `../doc`, export statique GitHub Pages, vérification Google par balise meta | pas de duplication ; DNS impossible sur github.io | Docusaurus |
