//! Every French user-facing string of the app (constitution, principe VII), grouped by area.
//!
//! Identifiers, configuration keys and commands stay as they are; everything else is French.

use std::path::Path;

use crate::config::{ConfigError, PermissionMode};
use crate::home::HomeError;
use crate::skills::Skill;

// ---------------------------------------------------------------------------------------
// General
// ---------------------------------------------------------------------------------------

pub const APP_NAME: &str = "cmd_ai";

/// `cmd_ai X.Y.Z` (output of `--version`).
pub fn version_line() -> String {
    format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"))
}

/// Full `--help` text (contract cli-and-tool.md).
pub const HELP: &str = "\
cmd_ai : l'agent qui exécute les commandes shell à votre place.
Décrivez ce que vous voulez en français : cmd_ai propose les commandes adaptées
à cette machine, vous les montre, puis les exécute (après votre accord en mode
plan).

Utilisation : cmd_ai [OPTIONS]

Options :
  --prompt <texte>         exécute cette demande (lue avant toute interface)
  --prompt-file <fichier>  idem, depuis un fichier UTF-8 (chemin relatif au
                           répertoire courant)
  --no-tui                 TUI fermé : sortie texte, une seule tâche, puis fin
  --tui                    TUI ouvert même si config.ui.tui = false
  --plan                   mode plan (défaut) : plan validé avant exécution
  --bypass                 mode bypass : sans confirmation, sauf garde-fous
  --model <rôle|nom>       modèle : un rôle de config.json (ex. fast, fr) ou
                           un nom complet
  --home <dossier>         dossier cmd_ai (sinon CMD_AI_HOME, sinon dossier
                           de l'exécutable)
  --color <auto|truecolor|256|16|none>
                           niveau de couleurs (NO_COLOR force « none » en auto)
  --resume <fichier>       reprend une tâche sauvegardée (relance élevée
                           sous Windows)
  -V, --version            affiche « cmd_ai X.Y.Z »
  -h, --help               affiche cette aide

Le mode par défaut vient de config.json (« permissions »), sinon c'est le
mode plan. --prompt et --prompt-file sont exclusifs. Si la sortie n'est pas
un terminal, le TUI reste fermé.

Codes de sortie :
  0    tâche réussie (ou TUI quitté normalement)
  1    tâche échouée (commande en échec non corrigée, plafond d'étapes)
  2    erreur d'usage (arguments)
  3    plan non validé (refusé, ou pas de terminal pour le valider)
  4    erreur de configuration (config.json absent ou invalide, rôle
       inconnu, dossier introuvable)
  5    erreur du provider ou du réseau (clé, crédits, quota, serveur, TLS,
       proxy)
  130  interruption (Ctrl+C ou Échap)

Dans le TUI : /aide, /plan, /bypass, /model [rôle|nom], /effacer, /quitter
(ou Ctrl+D). Échap annule la tâche en cours ; Ctrl+C annule, puis quitte au
second appui.

Fichiers (tous dans le dossier cmd_ai, rien n'est écrit ailleurs) :
  config.json  configuration (copie de config.example.json)
  log/         journaux de session
  memory/      notes de l'agent
  skills/      playbooks <nom>/SKILL.md

Exemples :
  cmd_ai
  cmd_ai --prompt \"liste les fichiers les plus gros de ce dossier\"
  cmd_ai --prompt-file demande.txt --no-tui
  cmd_ai --bypass --model fast
";

/// The `--help` text.
pub fn help() -> &'static str {
    HELP
}

/// Printed by the panic hook, after the terminal has been restored.
pub const INTERNAL_ERROR: &str = "cmd_ai s'est arrêté sur une erreur interne. Le terminal a \
été restauré et les commandes en cours ont été arrêtées. Vous pouvez signaler le problème en \
joignant le journal du dossier log/ (la clé API y est masquée).";

/// Shown when the user interrupts cmd_ai (exit code 130).
pub const INTERRUPTED: &str = "Interrompu.";

/// Short French meaning of a documented exit code (contract cli-and-tool.md).
pub fn exit_code_meaning(code: i32) -> &'static str {
    match code {
        0 => "tâche réussie",
        1 => "tâche échouée",
        2 => "erreur d'usage",
        3 => "plan non validé",
        4 => "erreur de configuration",
        5 => "erreur du provider ou du réseau",
        130 => "interruption",
        _ => "code inconnu",
    }
}

// ---------------------------------------------------------------------------------------
// Labels (status bar, plan)
// ---------------------------------------------------------------------------------------

pub const MODE_PLAN: &str = "PLAN";
pub const MODE_BYPASS: &str = "BYPASS";
pub const PRIVILEGE_USER: &str = "utilisateur";
pub const PRIVILEGE_ADMIN: &str = "admin";
pub const EPHEMERAL: &str = "éphémère";
pub const EPHEMERAL_NOTICE: &str = "Mode éphémère : le dossier cmd_ai n'est pas inscriptible, \
rien n'est enregistré (ni journal, ni mémoire).";

/// Badge of a permission mode.
pub fn mode_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Plan => MODE_PLAN,
        PermissionMode::Bypass => MODE_BYPASS,
    }
}

/// Label of a privilege level.
pub fn privilege_label(admin: bool) -> &'static str {
    if admin {
        PRIVILEGE_ADMIN
    } else {
        PRIVILEGE_USER
    }
}

/// Generic reason: a text file that is not UTF-8.
pub const FILE_NOT_TEXT: &str = "le fichier n'est pas un texte UTF-8";

/// Generic reason: a file over its size limit.
pub fn file_too_big(max_bytes: u64) -> String {
    format!(
        "fichier trop volumineux (plus de {})",
        human_size(max_bytes)
    )
}

/// `10 Mio`, `256 Kio` or `N octets`.
pub fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB && bytes % MIB == 0 {
        format!("{} Mio", bytes / MIB)
    } else if bytes >= KIB && bytes % KIB == 0 {
        format!("{} Kio", bytes / KIB)
    } else {
        format!("{bytes} octets")
    }
}

// ---------------------------------------------------------------------------------------
// Command line (usage errors, exit code 2)
// ---------------------------------------------------------------------------------------

pub const HELP_HINT: &str = "Aide : cmd_ai --help";

/// Complete usage error message, ready to print.
pub fn usage_error(detail: &str) -> String {
    format!("Erreur d'usage : {detail}.\n{HELP_HINT}")
}

pub fn cli_unknown_option(option: &str) -> String {
    format!("option inconnue « {option} »")
}

pub fn cli_missing_value(option: &str) -> String {
    format!("l'option {option} attend une valeur")
}

pub fn cli_unexpected_value(option: &str) -> String {
    format!("l'option {option} n'accepte pas de valeur")
}

pub fn cli_unexpected_argument(arg: &str) -> String {
    format!(
        "argument inattendu « {arg} » : pour donner une demande, utilisez --prompt \"…\" ou --prompt-file <fichier>"
    )
}

pub fn cli_not_unicode(option: &str) -> String {
    format!("la valeur de {option} n'est pas un texte Unicode valide")
}

pub fn cli_empty_value(option: &str) -> String {
    format!("la valeur de {option} est vide")
}

pub fn cli_repeated(option: &str) -> String {
    format!("l'option {option} est donnée plusieurs fois")
}

pub fn cli_conflict(a: &str, b: &str) -> String {
    format!("les options {a} et {b} sont incompatibles")
}

pub fn cli_invalid(detail: &str) -> String {
    format!("arguments invalides ({detail})")
}

pub fn cli_bad_color(value: &str) -> String {
    format!(
        "niveau de couleurs « {value} » inconnu (valeurs possibles : auto, truecolor, 256, 16, none)"
    )
}

/// No prompt and no interactive terminal (exit code 2).
pub const CLI_NO_TERMINAL: &str = "aucun terminal interactif : donnez la demande avec \
--prompt \"…\" ou --prompt-file <fichier>";

pub fn prompt_file_not_found(path: &Path) -> String {
    format!("Fichier de demande introuvable : {}", path.display())
}

pub fn prompt_file_unreadable(path: &Path, error: &str) -> String {
    format!(
        "Impossible de lire le fichier de demande {} : {error}",
        path.display()
    )
}

pub fn prompt_file_not_text(path: &Path) -> String {
    format!(
        "Le fichier de demande {} n'est pas un texte UTF-8 : enregistrez-le en UTF-8 puis relancez.",
        path.display()
    )
}

pub fn prompt_file_empty(path: &Path) -> String {
    format!(
        "Le fichier de demande {} est vide : écrivez-y votre demande en français.",
        path.display()
    )
}

pub fn prompt_file_too_big(path: &Path, max_bytes: u64) -> String {
    format!(
        "Le fichier de demande {} est trop volumineux (plus de {}).",
        path.display(),
        human_size(max_bytes)
    )
}

// ---------------------------------------------------------------------------------------
// Portable folder
// ---------------------------------------------------------------------------------------

pub fn home_error(error: &HomeError) -> String {
    match error {
        HomeError::NotFound(path) => format!(
            "Dossier cmd_ai introuvable : {}. Vérifiez l'option --home ou la variable CMD_AI_HOME.",
            path.display()
        ),
        HomeError::NotADirectory(path) => format!(
            "{} n'est pas un dossier : l'option --home et la variable CMD_AI_HOME doivent désigner le dossier cmd_ai.",
            path.display()
        ),
        HomeError::NoExecutable(detail) => format!(
            "Impossible de trouver l'emplacement de cmd_ai ({detail}). Indiquez le dossier avec --home <dossier>."
        ),
    }
}

// ---------------------------------------------------------------------------------------
// Configuration (exit code 4)
// ---------------------------------------------------------------------------------------

/// Actionable French message for a configuration error.
pub fn config_error(error: &ConfigError) -> String {
    match error {
        ConfigError::Missing(path) => {
            let dir = path.parent().unwrap_or(path);
            format!(
                "Fichier de configuration introuvable : {}\n\
                 Pour commencer : dans le dossier {}, faites une copie de config.example.json \
                 nommée config.json, puis renseignez « provider.api_key » (ou définissez la \
                 variable d'environnement OLLAMA_API_KEY).",
                path.display(),
                dir.display()
            )
        }
        ConfigError::Io(path, detail) => format!(
            "Impossible de lire {} : {detail}.\nVérifiez que le fichier est accessible (clé USB branchée, droits de lecture).",
            path.display()
        ),
        ConfigError::Parse(detail) => format!(
            "config.json contient une erreur, {detail}.\nCorrigez le fichier en vous aidant de config.example.json."
        ),
        ConfigError::UnsupportedVersion(version) => format!(
            "config.json utilise la version de schéma {version}, que {} ne prend pas en charge \
             (version attendue : 1). Le fichier n'a pas été modifié : mettez cmd_ai à jour ou \
             repartez de config.example.json.",
            version_line()
        ),
        ConfigError::Invalid(detail) => detail.clone(),
    }
}

/// `ligne L, colonne C : détail`
pub fn config_parse_detail(line: usize, column: usize, detail: &str) -> String {
    format!("ligne {line}, colonne {column} : {detail}")
}

pub const CFG_NOT_TEXT: &str = "le fichier n'est pas un texte UTF-8 (enregistrez-le en UTF-8)";
pub const CFG_TOO_BIG: &str = "fichier trop volumineux pour une configuration (plus de 1 Mio)";
pub const CFG_ROOT_NOT_OBJECT: &str =
    "config.json : le fichier doit contenir un objet JSON entre accolades { … }.";

pub fn cfg_missing_key(path: &str) -> String {
    format!("config.json : la clé obligatoire « {path} » est absente (voir config.example.json).")
}

pub fn cfg_unknown_key(path: &str, suggestion: Option<&str>) -> String {
    match suggestion {
        Some(s) => {
            format!("config.json : clé inconnue « {path} » (vouliez-vous dire « {s} » ?).")
        }
        None => format!(
            "config.json : clé inconnue « {path} » (voir config.example.json pour les clés possibles)."
        ),
    }
}

pub const CFG_VERSION_NOT_INTEGER: &str =
    "config.json : « version » doit être un entier (1 pour cette version de cmd_ai).";

pub fn cfg_invalid_value(path: &str, reason: &str) -> String {
    format!("config.json : valeur invalide pour « {path} » : {reason}.")
}

pub const REASON_EMPTY: &str = "la valeur ne doit pas être vide";
pub const REASON_POSITIVE: &str = "la valeur doit être supérieure à 0";
pub const REASON_URL_SCHEME: &str =
    "l'adresse doit commencer par https:// (http:// est réservé à localhost, 127.0.0.1 et [::1])";
pub const REASON_URL_HTTP_REMOTE: &str = "http:// en clair n'est autorisé que vers localhost, \
127.0.0.1 ou [::1] : utilisez https:// pour protéger votre clé API";
pub const REASON_URL_MALFORMED: &str = "adresse mal formée (hôte obligatoire ; ni espace, ni \
identifiants, ni « ? », ni « # »)";
pub const REASON_LOG_FILE_OVER_TOTAL: &str =
    "doit être inférieure ou égale à « limits.log_total_mib »";

pub fn reason_min(min: u64) -> String {
    format!("la valeur doit être au moins {min}")
}

pub fn reason_unknown_shell(shell: &str) -> String {
    format!("shell « {shell} » inconnu (pwsh, powershell, cmd, bash, sh ou zsh)")
}

/// Unknown `--model` / `/model` role (exit code 4).
pub fn cfg_unknown_role(name: &str, roles: &[&str]) -> String {
    let available = if roles.is_empty() {
        "aucun rôle défini dans « provider.roles »".to_string()
    } else {
        format!("rôles disponibles : {}", roles.join(", "))
    };
    format!(
        "Rôle de modèle inconnu « {name} » ({available}). Vous pouvez aussi donner un nom de \
         modèle complet, contenant « : », « / » ou « . »."
    )
}

/// Translates the common serde_json messages (without the "at line … column …" suffix).
pub fn serde_detail(message: &str) -> String {
    let message = message.trim();
    if let Some(rest) = message.strip_prefix("missing field ") {
        return format!("clé obligatoire {} absente", quote_ticks(rest));
    }
    if let Some(rest) = message.strip_prefix("unknown field ") {
        let field = rest.split(',').next().unwrap_or(rest);
        return format!("clé inconnue {}", quote_ticks(field));
    }
    if let Some(rest) = message.strip_prefix("unknown variant ") {
        let (value, expected) = rest.split_once(", expected ").unwrap_or((rest, ""));
        return format!(
            "valeur {} inconnue (valeurs possibles : {})",
            quote_ticks(value),
            expected_list(expected)
        );
    }
    if let Some(rest) = message.strip_prefix("invalid type: ") {
        let (found, expected) = rest.split_once(", expected ").unwrap_or((rest, "?"));
        return format!(
            "type invalide : {}, attendu {}",
            type_words(found),
            type_words(expected)
        );
    }
    if let Some(rest) = message.strip_prefix("invalid value: ") {
        let (found, expected) = rest.split_once(", expected ").unwrap_or((rest, "?"));
        return format!(
            "valeur invalide : {}, attendu {}",
            type_words(found),
            type_words(expected)
        );
    }
    let fixed: &[(&str, &str)] = &[
        (
            "trailing comma",
            "virgule en trop avant la fin de l'objet ou du tableau",
        ),
        (
            "trailing characters",
            "caractères en trop après la fin de l'objet JSON",
        ),
        (
            "key must be a string",
            "les clés doivent être entre guillemets doubles",
        ),
        (
            "expected value",
            "valeur attendue (texte entre guillemets, nombre, true, false, null, objet ou tableau)",
        ),
        ("expected `:`", "« : » attendu après la clé"),
        (
            "expected `,` or `}`",
            "« , » ou « } » attendu (virgule manquante ?)",
        ),
        (
            "expected `,` or `]`",
            "« , » ou « ] » attendu (virgule manquante ?)",
        ),
        (
            "EOF while parsing an object",
            "fin du fichier inattendue : accolade fermante « } » manquante ?",
        ),
        (
            "EOF while parsing a list",
            "fin du fichier inattendue : crochet fermant « ] » manquant ?",
        ),
        (
            "EOF while parsing a string",
            "fin du fichier inattendue : guillemet fermant manquant ?",
        ),
        (
            "EOF while parsing a value",
            "fin du fichier inattendue : fichier vide ou incomplet ?",
        ),
        (
            "invalid escape",
            "séquence d'échappement invalide (dans un chemin Windows, doublez les barres obliques inverses : C:\\\\dossier)",
        ),
        ("invalid number", "nombre invalide"),
        ("number out of range", "nombre hors limites"),
        ("invalid unicode code point", "code Unicode invalide"),
        (
            "control character (\\u0000-\\u001F) found while parsing a string",
            "caractère de contrôle dans un texte (un retour à la ligne s'écrit \\n)",
        ),
        ("recursion limit exceeded", "imbrication trop profonde"),
    ];
    for (en, fr) in fixed {
        if message == *en {
            return (*fr).to_string();
        }
    }
    format!("erreur JSON ({message})")
}

fn quote_ticks(s: &str) -> String {
    format!("« {} »", s.trim().trim_matches('`'))
}

fn expected_list(expected: &str) -> String {
    let e = expected.strip_prefix("one of ").unwrap_or(expected);
    e.replace(" or ", ", ").replace('`', "")
}

fn type_words(s: &str) -> String {
    let words: &[(&str, &str)] = &[
        ("a string", "un texte"),
        ("string", "texte"),
        ("a boolean", "un booléen (true ou false)"),
        ("boolean", "booléen"),
        ("integer", "entier"),
        ("floating point", "nombre décimal"),
        ("u32", "un entier positif"),
        ("u64", "un entier positif"),
        ("a sequence", "un tableau [ … ]"),
        ("sequence", "tableau"),
        ("a map", "un objet { … }"),
        ("map", "objet"),
        ("unit value", "valeur vide"),
        ("null", "null"),
    ];
    let s = s.trim();
    if s.starts_with("struct ") || s.starts_with("enum ") {
        return "un objet { … }".to_string();
    }
    for (en, fr) in words {
        if s == *en {
            return (*fr).to_string();
        }
        if let Some(rest) = s.strip_prefix(&format!("{en} ")) {
            return format!("{fr} {}", rest.replace('`', ""));
        }
    }
    s.replace('`', "")
}

// ---------------------------------------------------------------------------------------
// First run (setup)
// ---------------------------------------------------------------------------------------

pub fn setup_config_missing(config: &Path) -> String {
    format!(
        "Aucune configuration trouvée ({}). cmd_ai peut la créer pour vous à partir de config.example.json.",
        config.display()
    )
}

pub const SETUP_COPY_QUESTION: &str = "Créer config.json à partir de config.example.json ? [o/N] ";

pub fn setup_copied(config: &Path) -> String {
    format!("config.json créé : {}", config.display())
}

pub fn setup_example_missing(example: &Path) -> String {
    format!(
        "Modèle de configuration introuvable : {}. Récupérez config.example.json dans le paquet \
         de cmd_ai et placez-le dans le dossier cmd_ai.",
        example.display()
    )
}

pub fn setup_copy_failed(config: &Path, error: &str) -> String {
    format!("Impossible de créer {} : {error}", config.display())
}

pub fn setup_config_exists(config: &Path) -> String {
    format!(
        "{} existe déjà : cmd_ai ne l'écrase jamais. Modifiez-le directement.",
        config.display()
    )
}

pub const SETUP_EPHEMERAL: &str = "Le dossier cmd_ai n'est pas inscriptible (mode éphémère) : \
config.json ne peut pas être créé. Copiez le dossier sur un support inscriptible, ou créez \
config.json à la main.";

pub fn setup_example_invalid(example: &Path, detail: &str) -> String {
    format!(
        "Le modèle de configuration {} est invalide : {detail}",
        example.display()
    )
}

pub const SETUP_API_KEY_QUESTION: &str = "Collez votre clé API Ollama (créée sur \
https://ollama.com/settings/keys), ou laissez vide pour un serveur Ollama local : ";

pub const SETUP_API_KEY_HINT: &str = "La clé est enregistrée dans config.json, uniquement dans \
le dossier cmd_ai. Vous pouvez aussi utiliser la variable d'environnement OLLAMA_API_KEY. En cas \
de perte de la clé USB, révoquez-la sur https://ollama.com/settings/keys.";

pub const SETUP_API_KEY_SKIPPED: &str =
    "Aucune clé saisie : cmd_ai utilisera le serveur indiqué dans « provider.base_url » sans clé.";

pub fn setup_api_key_saved(config: &Path) -> String {
    format!("Clé API enregistrée dans {}.", config.display())
}

pub const SETUP_DECLINED: &str = "Configuration non créée : faites une copie de \
config.example.json nommée config.json dans le dossier cmd_ai, puis relancez cmd_ai.";

/// True for a French (or English) yes: `o`, `oui`, `y`, `yes` (case-insensitive).
pub fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_lowercase().as_str(),
        "o" | "oui" | "y" | "yes"
    )
}

// ---------------------------------------------------------------------------------------
// Session log
// ---------------------------------------------------------------------------------------

/// Last line written when a log file reaches its cap.
pub fn log_capped(file_max: u64) -> String {
    format!(
        "journal plafonné à {} : la suite de la session n'est pas enregistrée",
        human_size(file_max)
    )
}

// ---------------------------------------------------------------------------------------
// Memory (`cmd_ai memory …`)
// ---------------------------------------------------------------------------------------

/// Header written at the top of `memory/MEMORY.md` (ignored when reading).
pub const MEMORY_FILE_HEADER: &str = "# Mémoire de cmd_ai\n\n\
Une note par ligne, gérée par « cmd_ai memory add | list | forget ».\n";

pub const MEMORY_NONE: &str = "Aucune note en mémoire.";
pub const MEMORY_NOTE_EMPTY: &str = "note vide : rien à mémoriser";
pub const MEMORY_NOTE_MULTILINE: &str =
    "une note tient sur une seule ligne, sans caractère de contrôle";

pub fn memory_note_too_long(max_chars: usize) -> String {
    format!("note trop longue : {max_chars} caractères au plus")
}

pub fn memory_full(max_notes: usize) -> String {
    format!(
        "mémoire pleine ({max_notes} notes) : oubliez d'abord une note avec « cmd_ai memory forget <n> »"
    )
}

pub fn memory_bad_index(n: usize, count: usize) -> String {
    if count == 0 {
        format!("pas de note n° {n} : la mémoire est vide")
    } else {
        format!("pas de note n° {n} : choisissez un numéro entre 1 et {count}")
    }
}

pub fn memory_write_failed(path: &Path, error: &str) -> String {
    format!("impossible d'enregistrer {} : {error}", path.display())
}

/// Result of `cmd_ai memory list`.
pub fn memory_list(notes: &[String]) -> String {
    if notes.is_empty() {
        return MEMORY_NONE.to_string();
    }
    let mut out = format!("Notes en mémoire ({}) :", notes.len());
    for (i, note) in notes.iter().enumerate() {
        out.push_str(&format!("\n{}. {note}", i + 1));
    }
    out
}

/// Result of `cmd_ai memory add`.
pub fn memory_added(n: usize, persisted: bool) -> String {
    if persisted {
        format!("Note n° {n} mémorisée.")
    } else {
        format!(
            "Note n° {n} gardée pour cette session seulement (mode éphémère : rien n'est enregistré)."
        )
    }
}

/// Result of `cmd_ai memory forget`.
pub fn memory_forgotten(n: usize, note: &str) -> String {
    format!("Note n° {n} oubliée : {note}")
}

// ---------------------------------------------------------------------------------------
// Skills (`cmd_ai skill …`)
// ---------------------------------------------------------------------------------------

pub const SKILLS_NONE: &str = "Aucun skill disponible (dossier skills/ vide).";

/// Result of `cmd_ai skill list` (also usable as the catalog of the system prompt).
pub fn skill_list(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return SKILLS_NONE.to_string();
    }
    let mut out = format!("Skills disponibles ({}) :", skills.len());
    for s in skills {
        out.push_str(&format!("\n- {} : {}", s.name, s.description));
    }
    out
}

pub fn skill_not_found(name: &str, available: &[Skill]) -> String {
    if available.is_empty() {
        format!("Skill « {name} » introuvable : aucun skill disponible.")
    } else {
        let names: Vec<&str> = available.iter().map(|s| s.name.as_str()).collect();
        format!(
            "Skill « {name} » introuvable. Skills disponibles : {}.",
            names.join(", ")
        )
    }
}

pub fn skill_invalid_name(name: &str) -> String {
    format!(
        "Nom de skill invalide « {name} » : minuscules, chiffres et tirets seulement, 64 caractères au plus."
    )
}

pub const SKILL_NO_FRONTMATTER: &str =
    "le fichier doit commencer par un en-tête délimité par deux lignes « --- »";
pub const SKILL_UNCLOSED_FRONTMATTER: &str = "en-tête non refermé : ligne « --- » manquante";

pub fn skill_missing_key(key: &str) -> String {
    format!("clé « {key} » absente ou vide dans l'en-tête")
}

pub fn skill_duplicate_key(key: &str) -> String {
    format!("clé « {key} » présente plusieurs fois dans l'en-tête")
}

pub fn skill_bad_quote(key: &str) -> String {
    format!("valeur de « {key} » : guillemet non refermé")
}

pub fn skill_bad_name(name: &str) -> String {
    format!(
        "nom « {name} » invalide : minuscules, chiffres et tirets seulement, 64 caractères au plus"
    )
}

pub fn skill_description_too_long(max_chars: usize) -> String {
    format!("description trop longue : {max_chars} caractères au plus")
}

/// Catalog warning about one skill folder.
pub fn skill_warning(dir: &str, detail: &str) -> String {
    format!("skills/{dir}/SKILL.md ignoré : {detail}.")
}

pub const SKILL_WARN_MISSING_FILE: &str = "fichier SKILL.md absent";
pub const SKILL_WARN_INVALID_DIR: &str =
    "nom de dossier invalide (minuscules, chiffres et tirets seulement, 64 caractères au plus)";
pub const SKILL_WARN_SYMLINK: &str = "les liens symboliques ne sont pas suivis";
pub const SKILL_WARN_NOT_TEXT: &str = FILE_NOT_TEXT;

pub fn skill_warn_name_mismatch(name: &str, dir: &str) -> String {
    format!("le nom « {name} » de l'en-tête doit être identique au nom du dossier « {dir} »")
}

pub fn skill_warn_too_big(max_bytes: u64) -> String {
    file_too_big(max_bytes)
}

pub fn skill_warn_read(error: &str) -> String {
    format!("lecture impossible ({error})")
}

pub const SKILLS_DIR_NOT_DIR: &str = "ce n'est pas un dossier";

pub fn skills_dir_unreadable(dir: &Path, error: &str) -> String {
    format!(
        "Dossier des skills illisible ({}) : {error}. Aucun skill chargé.",
        dir.display()
    )
}

// ---------------------------------------------------------------------------------------
// Resume after elevation (`--resume`)
// ---------------------------------------------------------------------------------------

pub const RESUME_EPHEMERAL: &str = "impossible de sauvegarder la tâche avant l'élévation : le \
dossier cmd_ai n'est pas inscriptible (mode éphémère)";
pub const RESUME_EMPTY_PROMPT: &str = "la tâche sauvegardée ne contient pas de demande";

pub fn resume_write_failed(path: &Path, error: &str) -> String {
    format!(
        "impossible de sauvegarder la tâche dans {} : {error}",
        path.display()
    )
}

pub fn resume_not_found(path: &Path) -> String {
    format!("tâche sauvegardée introuvable : {}", path.display())
}

pub fn resume_unreadable(path: &Path, error: &str) -> String {
    format!(
        "impossible de lire la tâche sauvegardée {} : {error}",
        path.display()
    )
}

pub fn resume_invalid(path: &Path, error: &str) -> String {
    format!(
        "{} n'est pas une tâche sauvegardée par cmd_ai ({error})",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn version_line_is_name_and_cargo_version() {
        assert_eq!(
            version_line(),
            format!("cmd_ai {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn help_lists_every_option_and_exit_code() {
        for opt in [
            "--prompt <texte>",
            "--prompt-file <fichier>",
            "--no-tui",
            "--tui",
            "--plan",
            "--bypass",
            "--model <rôle|nom>",
            "--home <dossier>",
            "--color <auto|truecolor|256|16|none>",
            "--resume <fichier>",
            "-V, --version",
            "-h, --help",
        ] {
            assert!(help().contains(opt), "option absente de l'aide : {opt}");
        }
        for code in ["0 ", "1 ", "2 ", "3 ", "4 ", "5 ", "130"] {
            assert!(help().contains(&format!("  {code}")), "code {code}");
        }
        assert!(help().contains("Utilisation"));
        assert!(help().contains("Échap"));
    }

    #[test]
    fn help_fits_in_a_terminal_width() {
        for line in help().lines() {
            assert!(line.chars().count() <= 80, "ligne trop longue : {line}");
        }
    }

    #[test]
    fn labels() {
        assert_eq!(mode_label(PermissionMode::Plan), "PLAN");
        assert_eq!(mode_label(PermissionMode::Bypass), "BYPASS");
        assert_eq!(privilege_label(false), "utilisateur");
        assert_eq!(privilege_label(true), "admin");
        assert_eq!(EPHEMERAL, "éphémère");
        assert!(EPHEMERAL_NOTICE.contains("éphémère"));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(10 * 1024 * 1024), "10 Mio");
        assert_eq!(human_size(256 * 1024), "256 Kio");
        assert_eq!(human_size(1500), "1500 octets");
        assert_eq!(human_size(0), "0 octets");
    }

    #[test]
    fn usage_error_has_hint() {
        let msg = usage_error(&cli_unknown_option("--x"));
        assert!(msg.starts_with("Erreur d'usage : option inconnue « --x »"));
        assert!(msg.ends_with("cmd_ai --help"));
    }

    #[test]
    fn config_missing_explains_the_copy() {
        let path = PathBuf::from("cle").join("config.json");
        let msg = config_error(&ConfigError::Missing(path));
        assert!(msg.contains("config.example.json"), "{msg}");
        assert!(msg.contains("config.json"), "{msg}");
        assert!(msg.contains("OLLAMA_API_KEY"), "{msg}");
        assert!(msg.contains("cle"), "{msg}");
    }

    #[test]
    fn config_error_variants_are_french_and_specific() {
        let io = config_error(&ConfigError::Io(
            PathBuf::from("config.json"),
            "Accès refusé".into(),
        ));
        assert!(io.contains("Impossible de lire") && io.contains("Accès refusé"));
        let parse = config_error(&ConfigError::Parse(config_parse_detail(3, 7, "virgule")));
        assert!(parse.contains("ligne 3, colonne 7 : virgule"), "{parse}");
        let version = config_error(&ConfigError::UnsupportedVersion(7));
        assert!(version.contains("version de schéma 7") && version.contains("n'a pas été modifié"));
        let invalid = config_error(&ConfigError::Invalid(cfg_unknown_key(
            "provider.modle",
            Some("model"),
        )));
        assert!(invalid.contains("provider.modle") && invalid.contains("« model »"));
    }

    #[test]
    fn unknown_role_lists_roles() {
        let msg = cfg_unknown_role("rapide", &["fast", "fr"]);
        assert!(msg.contains("rapide") && msg.contains("fast, fr"), "{msg}");
        assert!(cfg_unknown_role("x", &[]).contains("aucun rôle"));
    }

    #[test]
    fn serde_messages_are_translated() {
        assert_eq!(
            serde_detail("missing field `model`"),
            "clé obligatoire « model » absente"
        );
        assert_eq!(
            serde_detail("unknown variant `plann`, expected `plan` or `bypass`"),
            "valeur « plann » inconnue (valeurs possibles : plan, bypass)"
        );
        assert_eq!(
            serde_detail("invalid type: string \"10\", expected u32"),
            "type invalide : texte \"10\", attendu un entier positif"
        );
        assert_eq!(
            serde_detail("invalid type: integer `3`, expected a boolean"),
            "type invalide : entier 3, attendu un booléen (true ou false)"
        );
        assert!(serde_detail("trailing comma").contains("virgule"));
        assert!(serde_detail("invalid escape").contains("C:\\\\dossier"));
        assert_eq!(serde_detail("bizarre"), "erreur JSON (bizarre)");
    }

    #[test]
    fn yes_answers() {
        for y in ["o", "O", "oui", " Oui ", "y", "yes"] {
            assert!(is_yes(y), "{y}");
        }
        for n in ["", "n", "non", "no", "peut-être"] {
            assert!(!is_yes(n), "{n}");
        }
    }

    #[test]
    fn memory_texts() {
        assert_eq!(memory_list(&[]), MEMORY_NONE);
        let list = memory_list(&["a".into(), "b".into()]);
        assert_eq!(list, "Notes en mémoire (2) :\n1. a\n2. b");
        assert!(memory_added(3, true).contains("n° 3"));
        assert!(memory_added(3, false).contains("éphémère"));
        assert!(memory_bad_index(4, 0).contains("vide"));
        assert!(memory_bad_index(4, 2).contains("entre 1 et 2"));
        assert!(memory_full(200).contains("200"));
        assert!(!MEMORY_FILE_HEADER.lines().any(|l| l.starts_with("- ")));
    }

    #[test]
    fn skill_texts() {
        let skills = vec![
            Skill {
                name: "a".into(),
                description: "premier".into(),
                body: String::new(),
            },
            Skill {
                name: "b".into(),
                description: "second".into(),
                body: String::new(),
            },
        ];
        assert_eq!(
            skill_list(&skills),
            "Skills disponibles (2) :\n- a : premier\n- b : second"
        );
        assert_eq!(skill_list(&[]), SKILLS_NONE);
        assert!(skill_not_found("c", &skills).contains("a, b"));
        assert!(skill_not_found("c", &[]).contains("aucun"));
        assert!(skill_invalid_name("../x").contains("../x"));
        assert_eq!(
            skill_warning("x", SKILL_WARN_MISSING_FILE),
            "skills/x/SKILL.md ignoré : fichier SKILL.md absent."
        );
    }

    #[test]
    fn general_texts() {
        assert!(INTERNAL_ERROR.contains("log/"));
        assert!(!INTERRUPTED.is_empty());
        for code in [0, 1, 2, 3, 4, 5, 130] {
            assert_ne!(exit_code_meaning(code), "code inconnu", "{code}");
        }
        assert_eq!(exit_code_meaning(42), "code inconnu");
        assert!(cli_invalid("x").contains("(x)"));
        let p = PathBuf::from("config.json");
        assert!(setup_config_exists(&p).contains("ne l'écrase jamais"));
        assert!(SETUP_EPHEMERAL.contains("éphémère"));
        assert!(setup_example_invalid(&p, "ligne 1").contains("ligne 1"));
    }

    #[test]
    fn log_cap_marker_is_short() {
        assert!(log_capped(10 * 1024 * 1024).len() < 150);
        assert!(log_capped(10 * 1024 * 1024).contains("10 Mio"));
    }
}
