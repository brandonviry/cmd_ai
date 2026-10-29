# cmd_ai

> Un agent TUI dont la seule mission est **d'exécuter des commandes à ma place** — PowerShell, Bash ou CMD — sur Windows, Linux et macOS.
> Dans l'esprit de Claude Code, OpenCode ou Mistral Code, mais spécialisé exécution, et **le plus léger possible**.

## 1. Le problème

Les agents existants sont pensés pour le développement : entre le prompt et l'action, il y a toute la tuyauterie de l'IDE. Sur VSCode ou en CLI avec un modèle, la moindre action passe dans la moulinette.

Et surtout, on tape encore des commandes à la main — notamment sur une machine dont on ne connaît pas le shell.

## 2. Le concept

`cmd_ai` prend le parti inverse des agents généralistes :

| Principe | Choix |
| --- | --- |
| Un seul tool | **Exécuter une commande shell** — rien d'autre |
| Un seul provider LLM (V1) | **Ollama** — modèles open weight, en priorité via le cloud (plus rapide) |
| Un seul langage | **Rust** — binaire léger, démarrage quasi instantané |
| Une identité propre | TUI minimaliste, construit autour de la science des couleurs |

Le but : **ne plus jamais taper une commande**. On parle en langage naturel, l'agent traduit en commandes et les exécute.

### Provider LLM (V1) — Ollama

- **V1 mono-provider : Ollama uniquement** — pour stabiliser. D'autres providers pourront s'ajouter plus tard : on cible l'endpoint compatible OpenAI d'Ollama (`/v1/chat/completions`), le portage coûtera donc peu de code. Ollama n'en supporte qu'un sous-ensemble (pas de `tool_choice`, pas de sorties structurées dans le cloud).
- **Philosophie : modèles open weight uniquement** — servis via le cloud, plus rapide qu'une installation locale sur la machine hôte.
- **Accès par API** : clé API, URL de base et modèle paramétrés dans un **fichier JSON** à côté de l'exécutable — même logique portable que le reste. Les noms de modèles sont ceux de l'API directe (`beam`, `gpt-oss:120b`), pas ceux du CLI Ollama (`beam:cloud`).

**Modèles cibles (vérifiés le 28/10/2026) :**

| Rôle | Modèle (nom API) | Pourquoi |
| --- | --- | --- |
| Principal (V1) | **Beam** (Reflection) — `beam` | Gratuit sur Ollama Cloud **pour une durée limitée** (promo sans date de fin), **1M tokens de contexte**, conçu pour l'agentic et le tool use — mais développé principalement pour l'anglais |
| Rapidité | **gpt-oss 120B** — `gpt-oss:120b` | Le plus rapide mesuré sur Ollama Cloud (~290 tok/s, ~340 ms avant le premier token), 128K de contexte — payant au token (0,15 $ / 0,60 $ par million) |
| Français | **Mistral Large 4** — `mistral-large-4` | Multilingue natif, 1M de contexte, raisonnement — payant au token (0,68 $ / 2,09 $ par million) |

- Gemma 3 (dont le 1B) et Mistral Small ne sont **plus servis dans le cloud** (retirés le 15/07/2026) — d'où les remplacements ci-dessus.
- **Choix du modèle manuel** : un modèle par défaut dans `config.json`, plus des rôles nommés (`fast`, `fr`) sélectionnables par option CLI ou depuis le TUI. Pas de routage automatique.
- **Offre Ollama Cloud** (au 28/10/2026, source : ollama.com/pricing) : plan « Pay as you go » à 0 $ — **3 requêtes concurrentes**, facturation au token pour tous les modèles depuis le 31/08/2026 ; seul Beam est gratuit (promo). Pro : 20 $/mois (60 $ de crédits). Les tarifs ont déjà changé une fois : **aucune limite codée en dur** — l'agent ne garde qu'une requête en vol à la fois (les suites de commandes s'exécutent séquentiellement), avec délais d'expiration et reprises.
- **Si Beam devient payant** : pas bloquant, le champ open weight est large — on change le modèle dans le config JSON (en revalidant le tool use : il varie d'un modèle à l'autre).

## 3. Modes d'exécution

### Lancement

- `cmd_ai` → ouvre le TUI interactif.
- `cmd_ai --prompt "…"` ou `cmd_ai --prompt-file <fichier>` → le prompt (texte ou fichier) est lu **avant même d'ouvrir le TUI** et exécuté directement.
- On choisit si la tâche tourne **TUI ouvert** (on suit l'exécution en direct) ou **TUI fermé** (one-shot, résultat en sortie).

### Sortie & progression

- Chaque commande exécutée doit produire **une sortie écrite** : quand elle tourne, on voit clairement, de notre côté, quelles actions ont été faites.
- L'agent enchaîne des suites de commandes → la progression affiche **où on en est** : un **ratio** (`1/4`, puis `2/4`…) et pourquoi pas un pourcentage — pour suivre sans avoir à réfléchir.

### Permissions

Les deux modes doivent exister :

- **Mode plan** — indispensable ici : l'agent enchaîne des *suites* de commandes, il présente donc le plan complet pour validation. On regarde, on dit OK, ça exécute.
- **Mode bypass permissions** — exécution sans confirmation. Plus rapide, à utiliser quand on fait confiance.

### Élévation de droits

L'app doit distinguer et gérer :

- les commandes réservées à l'**administrateur** (sudo, root, Administrateur Windows),
- celles qu'un simple **utilisateur** peut exécuter,
- et l'élévation de droits quand une commande l'exige.

## 4. Portabilité totale

Pas d'installateur : **des exécutables + leurs fichiers**, dans un zip ou sur une clé USB. La clé devient le dossier d'exécution — au lancement, c'est ce dossier qui est lu, rien d'autre. **Une seule clé sert les trois OS** : tous les binaires partagent la même configuration, les mêmes logs, la même mémoire et les mêmes skills.

```text
cmd_ai/
├── cmd_ai.exe             # Windows x64
├── cmd_ai-linux-x86_64    # Linux (binaire statique)
├── cmd_ai-linux-aarch64   # Linux ARM
├── cmd_ai-macos           # macOS (binaire universel Intel + Apple Silicon)
├── cmd_ai.sh              # lanceur Linux/macOS : choisit le bon binaire
├── config.json            # paramétrage : clé API, URL de base, modèle(s)
├── log/                   # journaux
├── memory/                # mémoire de l'agent
└── skills/                # playbooks (voir § 5)
```

- Format de clé conseillé : **exFAT** (en FAT32, Linux refuse d'exécuter le binaire).

**Règle d'or : rien ne se stocke sur la machine hôte.** Mémoire, logs, clé API — tout vit à côté du binaire, jamais dans le système. Un TUI classique stocke sa mémoire sur la machine ; ici, hors de question.

- **Portée exacte** : cmd_ai lui-même n'écrit rien hors de son dossier, et lance les shells dans un environnement qui limite les traces (pas de profil, pas d'historique, pas de cache PowerShell). En revanche, l'OS garde ses propres traces d'exécution (Prefetch, Amcache, SRUM, historique des périphériques USB…) : cmd_ai ne peut pas les empêcher.
- **Clé en lecture seule** : cmd_ai tourne sans logs ni mémoire (mode éphémère, signalé à l'écran) — jamais de repli sur un dossier de l'hôte.
- **Clé API** : les clés Ollama n'expirent jamais. Elle est masquée dans les logs et jamais transmise aux commandes lancées ; une variable d'environnement `OLLAMA_API_KEY` peut remplacer celle du `config.json`. En cas de perte de la clé USB : la révoquer sur ollama.com/settings/keys.

Concrètement :

- **Chez quelqu'un, sur un Mac** dont j'ignore tout des commandes : je branche la clé, la clé API est déjà dans le dossier, je lance. « Ouvre YouTube » → il lance YouTube. « Trouve-moi ce fichier » → il le trouve. « Traduis-moi ça » → il traduit. Zéro commande tapée.
- **À la bibliothèque**, sur une session restreinte où rien ne s'installe : la clé rend l'agent utilisable quand même — et tout repart avec elle.

**Limite connue (V1 non signée)** : sur un PC verrouillé (Smart App Control, AppLocker, interdiction d'exécuter depuis une clé USB) ou un Mac (Gatekeeper), un binaire non signé peut être bloqué. Le scénario bibliothèque n'est donc **pas garanti en V1** : les contournements sont documentés, et la signature de code est prévue avant la 1.0.

## 5. Skills & Hooks

Deux mécanismes complémentaires pour étendre l'agent — et dans les deux cas, **c'est l'humain qui rédige** ; l'agent, lui, exécute.

### Skills — la version LLM

- Des **playbooks rédigés à la main**, dans un format défini, que l'agent lit et suit.
- Le skill sert de protocole : la tâche de l'agent reste l'exécution.
- Exemple : un skill « projet npm » qui décrit les commandes à taper quand on travaille dans tel type de dossier.

### Hooks — la version robuste

- Des **déclencheurs déterministes**, gérés par le code de l'app, pas par le LLM.
- Exemple : un fichier vient d'être modifié → un hook se déclenche pour vérifier s'il est bon ou non.

En résumé : **les hooks sont la version système et robuste ; les skills, la version pilotée par le LLM.**

## 6. Développement

Les choix sont notés ici au fur et à mesure — quand je passerai au dev, je saurai encore ce que je vais utiliser.

### Hébergement

Le projet sera hébergé sur **GitHub**, en dépôt **public** (la doc est servie gratuitement par GitHub Pages). Le dossier du projet doit être **son propre dépôt git** — aujourd'hui, c'est `D:\` entier qui est un dépôt : `git init` dans le projet avant toute chose.

### Méthode : Spec Kit

- Développement piloté par la spécification avec **Spec Kit** (toolkit de GitHub, version 1.2.4) — le workflow complet, avec les noms de commandes dans Claude Code : **`/speckit-constitution`** → `/speckit-specify` → `/speckit-clarify` → `/speckit-plan` → `/speckit-tasks` → `/speckit-analyze` → `/speckit-implement` → `/speckit-converge`. Les specs vivent dans le repo (`specs/001-…`).
- **La constitution d'abord** : les principes directeurs du projet, posés par Spec Kit à l'init. C'est important d'en faire une **bonne constitution** — tout le reste en découle.
- **Les tests font partie de l'implémentation** — les tests unitaires notamment. Attention : dans Spec Kit, les tests sont **optionnels par défaut** ; on les rend obligatoires par un article de la constitution et des modèles personnalisés (`.specify/templates/overrides/`).

### Stack technique (arrêtée le 28/10/2026)

- **TUI** : `ratatui` 0.30 en mode inline (pas d'écran alternatif — l'historique reste dans le terminal) + `crossterm`.
- **HTTP** : `ureq` 3.5, bloquant, avec `rustls` et les certificats du système (proxys et inspection TLS d'entreprise) — **pas de tokio ni de reqwest** (~2 Mo de binaire au lieu de ~5).
- **Concurrence** : threads standard + un canal `mpsc`.
- **Exécution** : `process-wrap` (mode `std`) pour tuer proprement les arbres de processus (Job Objects sous Windows, groupes de processus sous Unix).
- **CLI & config** : `lexopt` pour les arguments ; `serde` + `serde_json` pour un `config.json` versionné.
- **Shells** : `pwsh` 7 s'il est présent, sinon Windows PowerShell 5.1, `cmd` seulement à la demande ; `bash` (ou `sh`) sous Linux et macOS.
- **Élévation** : sous Windows, relancer toute la session cmd_ai en administrateur (une fenêtre UAC ne renvoie pas sa sortie) ; sous Linux/macOS, `sudo -v` en direct dans le terminal, puis `sudo -n` pour les commandes capturées.
- **Skills** : format standard Agent Skills (`SKILL.md`, compatible Claude Code), chargés via une auto-commande interceptée `cmd_ai skill <nom>` — on reste à un seul tool.
- **Builds** : Windows MSVC statique, Linux musl statique, macOS universel ; V1 non signée.

### Structure du repo

La base de travail — le paquet portable du §4 est extrait de ce repo au build :

```text
cmd_ai/
├── CLAUDE.md      # l'index du projet (voir ci-dessous)
├── doc/           # les docs, rédigées en Markdown — la source de vérité
├── doc-site/      # le site de documentation (Fumadocs)
├── bug/           # une fiche par bug, avec son état
└── .github/
    └── workflows/ # GitHub Actions, en YAML
```

(+ le code source et les fichiers de build.)

### Documentation

- La doc est **hébergée via GitHub** (Pages), construite avec **Fumadocs** dans **`doc-site/`**, rédigée en **français**.
- Les docs se rédigent en **Markdown dans `doc/`** — Fumadocs lit ce dossier directement (`fumadocs-mdx` ≥ 15.4.6), via les fichiers nécessaires : **aucune duplication**, rien à recréer dans Fumadocs pour l'hébergement.
- **SEO-friendly** : la doc doit être référencée proprement — vérification Google Search Console par **balise meta** (la vérification DNS est impossible sur `github.io`, et un `robots.txt` placé dans `/cmd_ai/` est ignoré).

### Bugs

- Un dossier **`bug/`** : **chaque bug a sa fiche personnalisée, avec son état**.
- Les fiches sont **appelées aussi dans la doc** — et toujours **référencées dans le CLAUDE.md**.
- Pour **tout bug, une issue GitHub** est créée.

### GitHub Actions & releases

- Si des **GitHub Actions** sont nécessaires : toujours en **YAML** — et il faut le faire.
- Les **releases** sont hébergées sur GitHub.

### CLAUDE.md — l'index du projet

Création d'un **CLAUDE.md à la racine** du repo, qui rassemble tout le contexte nécessaire au développement :

- le contexte du projet (ce que décrit ce document),
- les **références vers les specs Spec Kit** (celles issues de `/speckit-specify`),
- les **références de la doc** (`doc/`, `doc-site/`) et des **bugs** (`bug/`, issues GitHub),
- les GitHub Actions et les releases,
- tout ce qui est lié au projet.

En somme, **l'index du projet** : un moteur d'index qui dit à l'agent de développement où chercher, quoi lire, et où on en est.

### État du projet & récapitulatif de clôture

- À la fin de **chaque exécution de l'IA**, l'agent donne **l'état du projet** : un **récapitulatif de clôture** — comme ça, on sait où on est, sans avoir à tout relire.
- Format : une **checklist propre** qui montre ce qui est **fait** et ce qu'il **reste à faire**. Par exemple : « implémentation : phases 1, 2 et 3 faites — reste la phase 4 dans les tasks » ; « ce spec est fait — est-ce qu'il en reste d'autres ? Oui, ceux-là ».
- La checklist inclut **l'état Spec Kit courant** : à quel niveau on en est (constitution, spécifié, clarifié, planifié, tâches, implémenté, convergé).
- Le récapitulatif est **imposé par un hook `Stop` de Claude Code** (plus fiable qu'une simple consigne dans le CLAUDE.md).
- Le but : **un regard en diagonale suffit**. On voit ce qui a été fait et ce qui reste à faire en survolant la checklist — la lecture exhaustive reste possible, mais elle n'est plus nécessaire.
