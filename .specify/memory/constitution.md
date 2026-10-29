# Constitution de cmd_ai

cmd_ai est un agent TUI en Rust, portable sur clé USB, dont le seul tool est l'exécution de commandes shell (PowerShell, Bash, CMD) sous Windows, Linux et macOS, le plus léger possible. `IDEE.md` fixe le **quoi** (périmètre, faits datés) ; cette constitution fixe le **comment**.

**Conventions** : DOIT, NE DOIT PAS, DEVRAIT, NE DEVRAIT PAS et PEUT (pluriels compris) valent MUST, MUST NOT, SHOULD, SHOULD NOT et MAY (RFC 2119). Un DOIT violé est CRITIQUE pour le Constitution Check de `/speckit-plan` et pour `/speckit-analyze` ; un écart à un DEVRAIT se justifie dans le tableau Complexity Tracking du plan. Un principe **NON NÉGOCIABLE** protège une exigence explicite d'`IDEE.md` : l'assouplir exige une version MAJEURE, après modification d'`IDEE.md` par le propriétaire. **Hôte** : la machine où l'on branche la clé. **Dossier** : le dossier de cmd_ai (clé USB ou zip extrait).

## Principes fondamentaux

### I. Zéro commande à taper

- Une fonctionnalité NE DOIT PAS obliger l'utilisateur à taper, copier ou corriger une commande shell : il s'exprime en langage naturel (seuls le lancement de cmd_ai et ses options font exception).
- Au démarrage, cmd_ai DOIT détecter l'OS, l'architecture, les shells disponibles et le niveau de droits, et les fournir au modèle pour que chaque commande soit écrite pour cet hôte.
- Après un échec, l'agent DOIT lire la sortie et proposer (mode plan) ou tenter (mode bypass) une correction, dans la limite du budget de tentatives, au lieu de renvoyer la syntaxe à l'utilisateur.
- Trois lancements DOIVENT exister : `cmd_ai` (TUI interactif), `cmd_ai --prompt "…"` et `cmd_ai --prompt-file <fichier>`. Le prompt est lu avant l'ouverture du TUI, et la tâche tourne au choix TUI ouvert (suivi en direct) ou TUI fermé (one-shot, résultat en sortie).
- **Justification** : ne plus jamais taper de commande est la raison d'être de cmd_ai, surtout sur une machine dont on ignore le shell.
- **Vérification** : l'override de spec impose des user stories rédigées comme des demandes en langage naturel ; `/speckit-analyze` classe CRITIQUE toute exigence où l'utilisateur tape une commande. Des tests d'intégration (modèle simulé) vérifient sur les trois OS le contexte OS/shell/droits envoyé, la correction bornée après échec, et `--prompt` / `--prompt-file` exécutés TUI fermé sans ouvrir le TUI.

### II. Un seul tool, une seule porte d'exécution

- Chaque requête au modèle DOIT déclarer exactement un tool : exécuter une commande shell, avec une explication courte en français. Un autre tool (fichiers, web, édition…) NE DOIT PAS être ajouté ; une réponse purement textuelle (traduire, expliquer) reste permise.
- Toute commande, qu'elle vienne du modèle ou d'un hook, DOIT passer par un module d'exécution unique ; un autre chemin du code NE DOIT PAS lancer de processus.
- Un appel de tool mal formé, tronqué ou ambigu NE DOIT PAS être exécuté, même en partie, ni remplacé par une commande devinée dans du texte libre : il est renvoyé au modèle comme erreur, avec des relances bornées.
- Skills (la version LLM) : des `SKILL.md` lus uniquement dans `skills/`, via l'auto-commande `cmd_ai skill <nom>`. Elle est interceptée par l'app et jamais transmise à un shell, et le nom est validé (ni séparateur ni `..`). Un skill guide l'agent ; il n'accorde aucune permission.
- Hooks (la version robuste) : déclenchés par le code de l'app sur des événements énumérés et documentés, dans un ordre déterministe ; le modèle NE DOIT PAS en déclencher, créer, modifier ni désactiver. Rédigés par l'humain dans `config.json`, ils sont pré-approuvés : ils s'exécutent sans validation, mais toujours de façon visible (principe IV).
- Skills et hooks sont rédigés par l'humain : cmd_ai NE DOIT PAS en créer ni en modifier de sa propre initiative.
- **Justification** : un seul tool garde l'agent léger, prévisible et auditable ; une porte unique rend les principes IV à VI applicables partout.
- **Vérification** : un test unitaire contrôle que la requête sérialisée déclare un seul tool. `clippy.toml` (`disallowed-methods`) interdit de créer un processus, et seul le module d'exécution porte l'`allow` (contrôlé en CI). Le parseur est testé sur des appels malformés (zéro exécution). Un test prouve que `cmd_ai skill` ne lance aucun processus et refuse la traversée de chemin. Des tests en table associent chaque événement à ses hooks. `/speckit-analyze` classe CRITIQUE toute spec qui ajoute un tool ou fait déclencher un hook par le modèle.

### III. Portabilité totale, rien sur l'hôte (NON NÉGOCIABLE)

- cmd_ai se distribue en dossier autonome, sans installateur ni dépendance à installer (binaires statiques ; sous macOS, bibliothèques système seulement). Un même dossier DOIT servir Windows, Linux et macOS : ses binaires partagent `config.json`, `log/`, `memory/` et `skills/` (UTF-8, chemins relatifs au dossier).
- Le dossier DOIT être résolu depuis l'emplacement de l'exécutable, jamais depuis le répertoire courant.
- cmd_ai NE DOIT PAS écrire hors de son dossier, fichiers temporaires compris : ni profil utilisateur, ni `%APPDATA%`, `~/.config`, `~/Library`, dossier temporaire système, registre ou trousseau. Il NE DOIT PAS non plus modifier de lui-même la configuration de l'hôte (PATH, ExecutionPolicy, variables persistantes).
- Les shells enfants DOIVENT être lancés dans un environnement à traces minimales, défini dans un seul module et documenté : sans profil, sans historique, sans cache PowerShell.
- Si le dossier n'est pas inscriptible, cmd_ai DOIT passer en mode éphémère (ni logs ni mémoire), signalé en permanence, et NE DOIT PAS se replier sur un dossier de l'hôte.
- La doc et l'interface NE DOIVENT PAS promettre « zéro trace » : les traces propres à l'OS (Prefetch, Amcache, SRUM, historique des périphériques USB…) sont hors de portée de cmd_ai, et la doc le dit. Les écritures que l'utilisateur demande sont des commandes comme les autres (IV, V), pas une fuite.
- **Justification** : « rien ne se stocke sur la machine hôte » est la règle d'or d'`IDEE.md`. Elle ne vaut que si elle est exacte : ce qui est contrôlable est garanti, le reste est annoncé.
- **Vérification** : un test d'intégration sur les trois OS fait pointer `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `XDG_*` et `TMP`/`TEMP`/`TMPDIR` vers des dossiers sentinelles vides. Ils DOIVENT le rester après une session scriptée (modèle simulé, commandes neutres), hors exceptions imposées par l'OS et listées dans `doc/` ; le code n'utilise aucune API d'écriture du registre (contrôlé en CI). Un test en dossier lecture seule exige le mode éphémère et zéro écriture. La release contrôle l'édition de liens (`ldd`, `dumpbin /dependents`, `otool -L`).

### IV. Exécution visible et robuste

- Avant de tourner, chaque commande DOIT être affichée (texte exact, shell, niveau de droits, explication en français) ; sa sortie (stdout, stderr) DOIT s'écrire en direct, suivie du code de sortie et de la durée. Une commande NE DOIT PAS s'exécuter de façon invisible, hooks compris.
- Une suite de commandes DOIT afficher sa progression en ratio `n/M` (un pourcentage PEUT s'y ajouter) ; si le plan change, M DOIT être mis à jour visiblement.
- Le shell DOIT être choisi de façon déterministe (ordre des Contraintes additionnelles) ; la commande lui DOIT être passée en un seul argument construit par le code, avec des règles de citation centralisées par shell.
- L'entrée standard des commandes capturées DOIT être fermée, et chaque commande DOIT avoir un délai d'expiration (budget, configurable). À l'expiration, sur interruption de l'utilisateur ou à l'annulation, tout l'arbre de processus DOIT être tué, sans orphelin. Les sorties DOIVENT être décodées sans panique (UTF-8 demandé au shell quand c'est possible, octets invalides remplacés).
- Hors mode éphémère, chaque session DOIT être journalisée dans `log/` (horodatage, commande, shell, droits, sortie, code, durée), avec rotation. La sortie transmise au modèle DOIT respecter son budget, début et fin conservés, et la troncature est signalée au modèle comme à l'utilisateur.
- En one-shot TUI fermé, la même trace DOIT sortir sur stdout/stderr, avec des codes de sortie documentés dans `doc/` (succès, échec de la tâche, plan non validé, interruption, erreur de configuration).
- Le code de production NE DOIT PAS paniquer (`unwrap`, `expect`, `panic!` réservés aux tests). Le crochet de panique DOIT restaurer le terminal et tuer les processus enfants, et toute erreur (config invalide, shell introuvable, réseau coupé) DOIT donner un message actionnable en français.
- **Justification** : l'utilisateur ne tape plus rien, il doit donc tout voir. Chez un tiers, un échec doit laisser la machine et le terminal dans l'état où on les a trouvés.
- **Vérification** :
  - instantanés `ratatui` (`TestBackend`) du bloc commande et de `n/M` ;
  - test d'intégration : un plan de 4 commandes donne 4 blocs, les ratios 1/4 à 4/4 et 4 entrées de log ;
  - sur les trois OS : corpus de citation (espaces, guillemets, `$ % ^ & | ;`, accents, chemins Unicode), test d'arbre (le petit-enfant meurt à l'annulation), tests du délai et de l'entrée fermée ;
  - tests unitaires de la troncature et de la table des codes de sortie ;
  - clippy `unwrap_used`, `expect_used` et `panic` en `deny` hors tests.

### V. L'humain garde la main : modes et droits

- Le mode plan et le mode bypass DOIVENT exister ; sans choix explicite, le mode plan DOIT être actif. Le mode actif et le niveau de droits de la session DOIVENT être affichés en permanence.
- Mode plan : la suite complète DOIT être présentée (liste numérotée : commande, shell, droits, explication), et l'exécution NE DOIT PAS commencer avant l'accord explicite de l'utilisateur. Le plan validé est une liste fermée : toute commande ajoutée ou modifiée DOIT être revalidée.
- Mode bypass : il DOIT être activé par une action explicite de l'humain (option CLI, `config.json`, bascule dans le TUI). Le modèle, une sortie, un skill, un hook ou `memory/` NE DOIVENT PAS pouvoir changer de mode. Le bypass supprime la confirmation, sauf pour les commandes de la liste `garde_fous` de `config.json` (formatage, effacement récursif d'une racine, écriture brute sur un disque…), active par défaut et que seul l'humain peut vider ; la doc DOIT avertir du risque d'injection.
- En one-shot sans terminal interactif, le mode plan DOIT afficher le plan puis s'arrêter sans rien exécuter (code de sortie « plan non validé »).
- Au-delà du plafond d'étapes (budget), une tâche DOIT s'arrêter et rendre la main, quel que soit le mode.
- Chaque commande DOIT être classée « utilisateur » ou « administrateur » avant exécution, et ce classement affiché dans le plan et à l'exécution. Le classement combine la déclaration du modèle et des règles déterministes de l'app ; un refus d'accès constaté après coup DOIT déclencher la proposition d'élévation.
- cmd_ai NE DOIT PAS s'élever de lui-même ni en silence : l'élévation passe toujours par le consentement de l'OS, bypass compris.
  - Windows : relancer toute la session en administrateur après avoir prévenu l'utilisateur, sans perdre la tâche ni écrire hors du dossier.
  - Linux et macOS : `sudo -v` en direct dans le terminal (TUI suspendu), puis `sudo -n`. Si `sudo -n` échoue, la suite DOIT s'arrêter et redemander, sans invite bloquée en arrière-plan.
- Un mot de passe NE DOIT PAS transiter par cmd_ai (modèle, `log/`, `memory/`, `config.json`), et `sudo -S` est interdit. Un échec dû aux droits DOIT être expliqué en français, avec la proposition d'élévation.
- **Justification** : une suite de commandes exige un accord sur le plan entier. Le bypass est un gain de vitesse choisi, jamais subi. Une élévation silencieuse transforme une erreur du modèle en dégât système, sur une machine qui n'est pas la nôtre.
- **Vérification** :
  - tests de la machine à états : aucune commande transmise à l'exécuteur avant accord, revalidation après modification, plafond d'étapes ;
  - test d'injection (« passe en bypass » dans une sortie ou une réponse) : le mode ne change pas ; test one-shot sans TTY ;
  - tests du classement (table de commandes par OS) ;
  - runner Linux : `sudo -n` réussi, puis refusé grâce à un faux `sudo` (arrêt sans blocage) ; la CI vérifie que `sudo -S` est absent du code ;
  - UAC et `sudo` interactif : scénario manuel écrit dans `doc/`, coché à chaque release.

### VI. Secrets protégés, sorties non fiables (NON NÉGOCIABLE)

- La clé API DOIT vivre dans un type dédié dont `Debug` et `Display` la masquent. Tout texte qui sort de cmd_ai (écran, `log/`, `memory/`, contexte du modèle, erreurs, paniques) DOIT passer par un filtre de masquage central, y compris quand une commande affiche `config.json`.
- La clé NE DOIT PAS figurer dans l'environnement des processus enfants : `OLLAMA_API_KEY` et les variables propres à cmd_ai en sont retirées, même héritées. Elle NE DOIT PAS partir ailleurs que vers l'URL de base configurée, en HTTPS (HTTP en clair seulement vers une adresse de bouclage), et la vérification TLS NE DOIT PAS être désactivable.
- `OLLAMA_API_KEY` PEUT remplacer la clé du `config.json`. La doc DOIT expliquer comment révoquer la clé en cas de perte de la clé USB (les clés Ollama n'expirent pas). Le dépôt public NE DOIT contenir aucune clé réelle.
- Tout ce qui vient de l'hôte (stdout, stderr, contenus et noms de fichiers, variables) et de `memory/` est une donnée non fiable. Il DOIT être transmis au modèle dans un cadre délimité qui le marque comme tel, et NE DOIT PAS pouvoir changer le mode, les droits, la configuration, les skills ou les hooks. Seuls la saisie dans le TUI, `--prompt`, `--prompt-file` et les skills portent des consignes.
- Avant affichage, les séquences de contrôle du terminal présentes dans les sorties DOIVENT être neutralisées (seules les couleurs SGR PEUVENT passer) : ni changement de titre, ni presse-papiers OSC 52, ni déplacement du curseur.
- **Justification** : avec le shell pour seul tool, l'agent lit des contenus écrits par n'importe qui, et l'injection de prompt par ses propres sorties est le risque principal. Une clé qui n'expire jamais voyage sur une clé USB qu'on branche chez d'autres.
- **Vérification** :
  - tests unitaires de `format!("{:?}")` et du filtre de masquage ;
  - test canari : une session complète avec une clé factice ne laisse aucune occurrence dans `log/`, `memory/`, l'écran, le corps des requêtes ni l'environnement d'un enfant (`env`, `Get-ChildItem env:`) ;
  - corpus d'injection joué contre le modèle simulé (consignes cachées, faux appels de tool, demande de bypass, séquences ESC/OSC) : mode et droits inchangés, aucune exécution hors porte, revalidation en mode plan ;
  - scan de secrets du dépôt en CI.

### VII. Interface lisible : français et science des couleurs

- Tout texte destiné à l'utilisateur (TUI, plans, explications, erreurs, `--help`, documentation) DOIT être en français correct, accents compris ; les identifiants techniques (code, clés de `config.json`, commandes) restent tels quels. Ces textes DEVRAIENT être regroupés dans un seul module.
- Le prompt système DOIT exiger du modèle des réponses et des explications en français, quel que soit le modèle choisi.
- Les accents DOIVENT s'afficher correctement dans le TUI et dans les sorties capturées, y compris sous Windows PowerShell 5.1 et `cmd` (pages de code OEM).
- Le TUI est minimaliste, et son identité repose sur la science des couleurs. Toute couleur DOIT venir d'une palette unique, définie dans un seul module, chaque teinte étant justifiée par des critères perceptifs mesurables (contraste, distinction pour les daltoniens). La couleur NE DOIT PAS être le seul porteur d'une information : mode, droits et état ont aussi un libellé ou un symbole. Le TUI DOIT rester lisible sans couleur.
- **Justification** : l'utilisateur est francophone, et les modèles open weight ne répondent pas tous en français d'eux-mêmes. Une identité fondée sur la science des couleurs se mesure, elle ne se décrète pas.
- **Vérification** :
  - sur le runner Windows, un test capture « éàçù » produit par PowerShell 5.1, `pwsh` et `cmd`, et compare l'UTF-8 obtenu ;
  - les instantanés figent les textes français et un rendu sans couleur ;
  - un test unitaire calcule le contraste de chaque paire texte/fond de la palette et le compare au budget ;
  - la CI refuse toute couleur littérale hors du module palette ;
  - la revue de PR contrôle la langue de `doc/`.

### VIII. Légèreté mesurée

- Les budgets de taille, de démarrage et de dépendances (Contraintes additionnelles) DOIVENT tenir à chaque fusion sur `main` ; un dépassement bloque la CI. Le premier affichage du TUI NE DOIT PAS attendre le réseau.
- L'arbre des dépendances NE DOIT PAS contenir de runtime asynchrone (`tokio`, `async-std`, `smol`…), ni `reqwest`, ni `openssl` : la concurrence passe par les threads standard et un canal `mpsc`.
- Toute nouvelle crate (normale, de build ou proc-macro) DOIT être justifiée dans le Complexity Tracking du plan : besoin, alternative écartée, impact mesuré sur la taille et le démarrage. Ses features par défaut DEVRAIENT être désactivées. Le profil release DOIT viser la taille (`lto`, `codegen-units = 1`, `strip`).
- La solution la plus simple qui tient la promesse DEVRAIT l'emporter : pas d'abstraction ni de code « pour plus tard ».
- **Justification** : « le plus léger possible » est une promesse vérifiable. Sans mesure à chaque PR, la légèreté s'érode une crate à la fois ; et moins de code embarqué, c'est aussi moins de surface d'attaque.
- **Vérification** : le job CI `budgets` mesure sur les trois runners la taille de chaque binaire strippé, la médiane de 10 lancements de `--version`. Il compte les dépendances directes avec `cargo metadata` et les transitives avec `cargo tree -e normal --target <cible>`. `cargo deny check bans` refuse les crates exclues, et le Constitution Check exige une ligne Complexity Tracking par nouvelle crate.

### IX. Tests d'abord, sur les trois OS (NON NÉGOCIABLE)

- Chaque exigence et chaque critère d'acceptation DOIT avoir un test automatisé, écrit avant le code et vu en échec (rouge → vert → refactor). Dans `tasks.md`, la tâche de test précède celle d'implémentation, et une tâche sans ses tests n'est pas terminée.
- Les tests unitaires vivent dans le code (`#[cfg(test)]`). Les tests d'intégration, dans `tests/`, pilotent le vrai binaire face à un serveur HTTP local qui simule Ollama (appels de tool malformés, JSON tronqué, 429, 5xx, délais).
- Les tests NE DOIVENT PAS dépendre du réseau ni d'une vraie clé API, et NE DOIVENT PAS lancer de commande destructrice hors d'un bac à sable qu'ils ont créé.
- Le code propre à un OS DOIT être testé sur le runner de cet OS (Windows, Linux, macOS).
- Chaque DOIT des principes I à VIII DOIT être couvert par le contrôle de sa ligne Vérification. Ce qui ne s'automatise pas (UAC, Gatekeeper, `sudo` interactif) DOIT avoir un scénario manuel écrit dans `doc/`, coché avant chaque release.
- Tout correctif de bug DOIT ajouter un test de non-régression qui échoue sans lui, cité dans sa fiche `bug/`.
- Un test de fumée contre le vrai provider (clé locale, hors CI) DEVRAIT précéder chaque release, et DOIT suivre tout changement du modèle par défaut (le tool use varie d'un modèle à l'autre).
- **Justification** : `IDEE.md` fait des tests une partie de l'implémentation, alors que Spec Kit les rend optionnels par défaut. Un agent qui exécute de vraies commandes ne tolère aucune régression silencieuse.
- **Vérification** : les overrides de spec et de tâches rendent les tests obligatoires. `/speckit-analyze` classe CRITIQUE une exigence sans test, une tâche de test placée après son implémentation ou un DOIT sans contrôle. La CI exécute les tests sur les trois OS à chaque push et PR, et un échec sur un seul OS bloque la fusion.

## Contraintes additionnelles

**Budgets** (en modifier un est un amendement MINEUR ; ils se resserrent par cliquet, jamais ne se relâchent sans justification) :

| Budget | Valeur | Mesure |
| --- | --- | --- |
| Binaire de release strippé, Windows x64 et Linux | ≤ 5 Mio chacun | CI, artefacts de release |
| Binaire macOS universel | ≤ 10 Mio | CI, artefact de release |
| `cmd_ai --version` | médiane ≤ 50 ms | 10 lancements à chaud, runners CI, sans réseau |
| Premier rendu du TUI | aucune E/S réseau avant | test d'intégration (URL injoignable) |
| Dépendances directes / transitives | ≤ 15 (hors dev) / ≤ 150 par cible | `cargo metadata`, `cargo tree` |
| Sortie d'une commande envoyée au modèle | ≤ 16 Kio (4 Kio de début + 12 Kio de fin) | test unitaire |
| Capture et log d'une commande | ≤ 1 Mio par flux, le reste compté et signalé | test unitaire |
| Dossier `log/` | fichier ≤ 10 Mio, total ≤ 50 Mio, puis rotation | test unitaire |
| Délai par défaut d'une commande | 120 s, configurable | test d'intégration |
| Étapes par tâche | 30, puis arrêt et retour à l'humain | test d'intégration |
| Reprises | réseau 3 (backoff, `Retry-After`) ; appel de tool invalide 2 ; correction automatique 2 par commande | serveur simulé |
| Contraste texte/fond de la palette | ≥ 4,5:1 (WCAG AA) | test unitaire |

**Stack et plateformes** (arrêtées dans `IDEE.md` §6 ; versions dans `Cargo.toml` et `Cargo.lock` versionné, toolchain stable épinglée par `rust-toolchain.toml`) :

- Rust ; TUI `ratatui` en mode inline (pas d'écran alternatif) + `crossterm` ; HTTP `ureq` bloquant + `rustls` + certificats du système ; threads standard + `mpsc` ; `process-wrap` (mode `std`) ; `lexopt` ; `serde` + `serde_json`.
- Shells : `pwsh` 7 s'il est présent, sinon Windows PowerShell 5.1, `cmd` seulement à la demande ; `bash`, sinon `sh`, sous Linux et macOS.
- Cibles : Windows x64 MSVC (CRT statique), Linux x86_64 et aarch64 musl statiques, macOS universel. Le lanceur `cmd_ai.sh` est écrit en `sh` POSIX et validé par `shellcheck` ; la clé USB est conseillée en exFAT (documenté).
- Skills au format Agent Skills (`SKILL.md`, compatible Claude Code).
- Le code propre à un OS est isolé dans des modules `#[cfg(...)]` ; `unsafe` est interdit hors d'un module FFI commenté (`// SAFETY:`).
- V1 non signée : les blocages possibles (Smart App Control, AppLocker, exécution depuis USB interdite, Gatekeeper) et leurs contournements DOIVENT être documentés ; la 1.0 NE DOIT PAS être publiée sans binaires signés.
- Remplacer un composant de cette stack DOIT passer par une modification d'`IDEE.md`, puis un amendement MINEUR.

**Provider et modèles** (noms, tarifs et quotas : voir `IDEE.md` et `config.json`, jamais ici) :

- V1 : Ollama uniquement, via l'endpoint compatible OpenAI `/v1/chat/completions`, isolé dans un seul module, sans couche multi-provider. Une fonctionnalité NE DOIT PAS dépendre de `tool_choice` ni de sorties structurées.
- Une seule requête au modèle en vol (les suites s'exécutent en série), avec des délais d'expiration et des reprises bornées et configurables. Limites, quotas et tarifs NE DOIVENT PAS être codés en dur.
- Le modèle se choisit à la main : un modèle par défaut et des rôles nommés (`fast`, `fr`) dans `config.json`, sélectionnables par option CLI ou dans le TUI, sans routage automatique. Les noms de modèles NE DOIVENT PAS être codés en dur : la CI échoue si un nom connu apparaît dans `src/` hors fixtures de test.
- `config.json` DOIT porter une version de schéma et être migré sans perte ; il NE DOIT PAS être écrasé si sa version est inconnue.

## Flux de développement

- **Dépôt** : le projet est son propre dépôt git, public sur GitHub. Une branche par spec (`NNN-nom`) ou par bug ; une fusion sur `main` exige que `cargo fmt --check`, `cargo clippy` et les tests passent en local, puis une CI verte sur les trois OS.
- **Langue** : code, identifiants, commentaires et messages de commit DEVRAIENT être en anglais ; tout ce qui s'adresse à l'utilisateur (TUI, `--help`, `doc/`, fiches `bug/`) DOIT être en français.
- **Spec Kit** : toute fonctionnalité DOIT suivre `/speckit-specify` → `/speckit-clarify` → `/speckit-plan` → `/speckit-tasks` → `/speckit-analyze` → `/speckit-implement` → `/speckit-converge`, avec ses specs dans `specs/NNN-nom/` ; pas de code sans tâche dans `tasks.md`. Seule exception : un correctif de bug qui ne change pas le comportement spécifié (fiche `bug/`, issue, test de non-régression).
- **Périmètre** : une fonctionnalité absente d'`IDEE.md` DOIT y être ajoutée par le propriétaire avant d'être spécifiée.
- **Modèles personnalisés** : `.specify/templates/overrides/spec-template.md` et `tasks-template.md` DOIVENT exister.
  - L'override de spec impose : user stories en langage naturel, tests d'acceptation, OS concernés, budgets touchés, doc et bugs touchés, et une section « Risques d'exécution » (commandes possibles, droits, sorties renvoyées au modèle, comportement hors ligne et en lecture seule).
  - L'override de tâches impose une tâche de test avant chaque tâche d'implémentation, ainsi que les tâches `doc/` et `CLAUDE.md`.
- **Plan** : `/speckit-plan` NE DOIT PAS entrer en Phase 0 avec un `[NEEDS CLARIFICATION]` restant. Son Constitution Check DOIT avoir une ligne par principe (I à IX), jugée selon sa ligne Vérification, avant la Phase 0 et après la Phase 1. Un DOIT violé bloque le plan jusqu'à correction ou amendement ; les écarts à un DEVRAIT et les nouvelles crates vont dans Complexity Tracking.
- **Analyze et converge** : `/speckit-analyze` DOIT rendre zéro constat CRITIQUE avant `/speckit-implement` ; `/speckit-converge` ferme les écarts restants par ajout de tâches.
- **CI** : GitHub Actions en YAML, à chaque push et PR, sur windows, ubuntu et macos : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, tests, build de toutes les cibles, budgets, `cargo deny`, scan de secrets, test des traces sur l'hôte, job `doc`.
- **Releases** : une étiquette `vX.Y.Z` déclenche la CI, qui produit le paquet portable (`IDEE.md` §4), ses sommes SHA-256 et des notes de version (limites de la V1 non signée, contournements), puis publie le tout sur GitHub Releases. Rien n'est publié depuis un build local.
- **Documentation** : `doc/` (Markdown, français) est la source unique ; `doc-site/` (Fumadocs) la lit directement, sans aucune copie, et se déploie sur GitHub Pages.
  - Une PR qui change un comportement visible DOIT mettre `doc/` à jour.
  - Chaque page DOIT porter `title` et `description` en frontmatter, et le site porte la balise meta de vérification Google Search Console.
  - Le job `doc` vérifie le build, les liens, le frontmatter et l'absence de contenu `.md`/`.mdx` sous `doc-site/`.
- **Bugs** : chaque bug DOIT avoir une issue GitHub et une fiche `bug/NNN-slug.md` (état, lien vers l'issue, puis test de non-régression), citée dans `doc/` et dans `CLAUDE.md` ; le job `doc` le vérifie. Une faille de sécurité passe d'abord par un avis de sécurité privé GitHub (`SECURITY.md`) ; son issue publique est créée avec le correctif.
- **CLAUDE.md** : à la racine, c'est l'index du projet (contexte, specs, doc, bugs, workflows, releases, état). Spec Kit ne le modifie jamais : il DOIT être mis à jour dans la PR qui ajoute, déplace ou clôt l'un de ces éléments.
- **Récapitulatif de clôture** : chaque exécution de l'IA DOIT se terminer par une checklist qui donne l'état Spec Kit de chaque spec (constitution, spécifié, clarifié, planifié, tâches, implémenté, convergé), ce qui est fait, ce qui reste à faire et les bugs ouverts. Elle est imposée par un hook `Stop` de Claude Code versionné dans `.claude/settings.json`, qui bloque la fin du tour si elle manque.

## Gouvernance

- **Primauté** : cette constitution prime sur toute autre pratique du projet. `IDEE.md` fait foi pour le périmètre et les faits datés (modèles, tarifs, offres, versions), qui n'ont pas leur place ici ; `CLAUDE.md` guide les agents au quotidien sans la contredire. En cas de conflit entre `IDEE.md` et la constitution, le travail s'arrête et le propriétaire amende l'un ou l'autre.
- **Amendement** : par `/speckit-constitution`, dans une PR dédiée approuvée par le propriétaire. Le Sync Impact Report (commentaire HTML en tête) sert à la revue et est retiré avant le commit ; overrides, `CLAUDE.md` et `doc/` sont alignés dans la même PR.
- **Versionnage** (SemVer) :
  - MAJEURE : principe retiré ou redéfini de façon incompatible, ou assouplissement d'un principe NON NÉGOCIABLE ;
  - MINEURE : principe ou section ajouté, règle nettement étendue, budget ou stack modifié ;
  - CORRECTIVE : formulation sans effet sur les règles.
  - Dates au format ISO (AAAA-MM-JJ).
- **Conformité** : vérifiée par chaque Constitution Check, chaque `/speckit-analyze` (un DOIT violé = CRITIQUE), la CI et la revue de chaque PR. La constitution entière est relue avant chaque release MINEURE et avant la 1.0.

**Version**: 1.0.0 | **Ratifiée**: 2026-10-29 | **Dernière modification**: 2026-10-29
