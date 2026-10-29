//! Command line parsing (contract `contracts/cli-and-tool.md`, lexopt).
//!
//! Errors are complete French messages, ready to print, for exit code 2.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config::{ColorChoice, PermissionMode};
use crate::home::{self, TextFileError};
use crate::text;

/// Largest accepted `--prompt-file`.
pub const PROMPT_FILE_MAX_BYTES: u64 = 1024 * 1024;

/// Where the task prompt comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptSource {
    Text(String),
    File(PathBuf),
}

/// What to do after parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    Run,
    Help,
    Version,
}

/// Parsed arguments; `None` means "not given on the command line".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    pub prompt: Option<PromptSource>,
    /// `--tui` → `Some(true)`, `--no-tui` → `Some(false)`.
    pub tui: Option<bool>,
    pub mode: Option<PermissionMode>,
    pub model: Option<String>,
    pub home: Option<PathBuf>,
    /// Validated and lowercase: `auto`, `truecolor`, `256`, `16` or `none`.
    pub color: Option<String>,
    pub resume: Option<PathBuf>,
    pub action: Action,
}

/// Parses the arguments, `argv[0]` excluded.
///
/// `-h`/`--help` wins over everything that follows it; `-V`/`--version` is reported once
/// the whole line has been checked. Every error is a complete French message
/// (`text::usage_error`) for exit code 2.
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    parse_inner(args).map_err(|detail| text::usage_error(&detail))
}

fn parse_inner(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    use lexopt::Arg::{Long, Short, Value};

    let mut parser = lexopt::Parser::from_args(args);
    let mut out = Args::default();
    let mut version = false;
    loop {
        let arg = match parser.next() {
            Ok(Some(arg)) => arg,
            Ok(None) => break,
            Err(e) => return Err(lexopt_error(e)),
        };
        match arg {
            Short('h') | Long("help") => {
                return Ok(Args {
                    action: Action::Help,
                    ..Args::default()
                });
            }
            Short('V') | Long("version") => version = true,
            Long("prompt") => {
                let text = string_value(&mut parser, "--prompt")?;
                match out.prompt {
                    Some(PromptSource::Text(_)) => return Err(text::cli_repeated("--prompt")),
                    Some(PromptSource::File(_)) => {
                        return Err(text::cli_conflict("--prompt", "--prompt-file"));
                    }
                    None => out.prompt = Some(PromptSource::Text(text)),
                }
            }
            Long("prompt-file") => {
                let path = path_value(&mut parser, "--prompt-file")?;
                match out.prompt {
                    Some(PromptSource::File(_)) => {
                        return Err(text::cli_repeated("--prompt-file"));
                    }
                    Some(PromptSource::Text(_)) => {
                        return Err(text::cli_conflict("--prompt", "--prompt-file"));
                    }
                    None => out.prompt = Some(PromptSource::File(path)),
                }
            }
            Long("tui") => set_flag(&mut out.tui, true, "--tui", "--no-tui")?,
            Long("no-tui") => set_flag(&mut out.tui, false, "--no-tui", "--tui")?,
            Long("plan") => set_flag(&mut out.mode, PermissionMode::Plan, "--plan", "--bypass")?,
            Long("bypass") => {
                set_flag(&mut out.mode, PermissionMode::Bypass, "--bypass", "--plan")?;
            }
            Long("model") => {
                let model = string_value(&mut parser, "--model")?;
                set_once(&mut out.model, model, "--model")?;
            }
            Long("home") => {
                let home = path_value(&mut parser, "--home")?;
                set_once(&mut out.home, home, "--home")?;
            }
            Long("color") => {
                let raw = raw_string_value(&mut parser, "--color")?;
                let color = ColorChoice::parse(&raw).ok_or_else(|| text::cli_bad_color(&raw))?;
                set_once(&mut out.color, color.as_str().to_string(), "--color")?;
            }
            Long("resume") => {
                let resume = path_value(&mut parser, "--resume")?;
                set_once(&mut out.resume, resume, "--resume")?;
            }
            Short(c) => return Err(text::cli_unknown_option(&format!("-{c}"))),
            Long(name) => return Err(text::cli_unknown_option(&format!("--{name}"))),
            Value(v) => return Err(text::cli_unexpected_argument(&v.to_string_lossy())),
        }
    }
    if version {
        out.action = Action::Version;
    }
    Ok(out)
}

/// Maps the errors `lexopt` can raise while looking for the next argument.
fn lexopt_error(e: lexopt::Error) -> String {
    match e {
        lexopt::Error::UnexpectedValue { option, .. } => {
            text::cli_unexpected_value(&dashed(&option))
        }
        lexopt::Error::MissingValue {
            option: Some(option),
        } => text::cli_missing_value(&dashed(&option)),
        lexopt::Error::UnexpectedOption(option) => text::cli_unknown_option(&option),
        lexopt::Error::UnexpectedArgument(arg) => {
            text::cli_unexpected_argument(&arg.to_string_lossy())
        }
        other => text::cli_invalid(&other.to_string()),
    }
}

/// `plan` → `--plan`; options already spelled with dashes are kept.
fn dashed(option: &str) -> String {
    if option.starts_with('-') {
        option.to_string()
    } else {
        format!("--{option}")
    }
}

fn set_flag<T: PartialEq>(
    slot: &mut Option<T>,
    value: T,
    opt: &str,
    other: &str,
) -> Result<(), String> {
    match slot {
        Some(current) if *current != value => Err(text::cli_conflict(other, opt)),
        _ => {
            *slot = Some(value);
            Ok(())
        }
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, opt: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(text::cli_repeated(opt));
    }
    *slot = Some(value);
    Ok(())
}

fn os_value(parser: &mut lexopt::Parser, opt: &str) -> Result<OsString, String> {
    parser.value().map_err(|_| text::cli_missing_value(opt))
}

/// A Unicode value, untrimmed.
fn raw_string_value(parser: &mut lexopt::Parser, opt: &str) -> Result<String, String> {
    os_value(parser, opt)?
        .into_string()
        .map_err(|_| text::cli_not_unicode(opt))
}

/// A Unicode value, trimmed, not empty.
fn string_value(parser: &mut lexopt::Parser, opt: &str) -> Result<String, String> {
    let value = raw_string_value(parser, opt)?;
    let value = value.trim();
    if value.is_empty() {
        return Err(text::cli_empty_value(opt));
    }
    Ok(value.to_string())
}

/// A path (any OS string), not blank.
fn path_value(parser: &mut lexopt::Parser, opt: &str) -> Result<PathBuf, String> {
    let value = os_value(parser, opt)?;
    if value.to_string_lossy().trim().is_empty() {
        return Err(text::cli_empty_value(opt));
    }
    Ok(PathBuf::from(value))
}

/// Reads a `--prompt-file`: UTF-8 (BOM stripped; UTF-16 with BOM accepted), line endings
/// normalized to `\n`, trimmed, refused when empty. Relative paths are relative to the
/// current directory. Errors are complete French sentences.
pub fn read_prompt_file(path: &Path) -> Result<String, String> {
    let content = home::read_text_file(path, PROMPT_FILE_MAX_BYTES).map_err(|e| match e {
        TextFileError::NotFound => text::prompt_file_not_found(path),
        TextFileError::TooBig => text::prompt_file_too_big(path, PROMPT_FILE_MAX_BYTES),
        TextFileError::NotText => text::prompt_file_not_text(path),
        TextFileError::Io(detail) => text::prompt_file_unreadable(path, &detail),
    })?;
    let prompt = content.replace("\r\n", "\n");
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(text::prompt_file_empty(path));
    }
    Ok(prompt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::test_support::TempDir;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(OsString::from))
    }

    fn err(args: &[&str]) -> String {
        let e = parse(args).unwrap_err();
        assert!(e.starts_with("Erreur d'usage : "), "{e}");
        assert!(e.contains("cmd_ai --help"), "{e}");
        e
    }

    #[test]
    fn no_arguments_runs_with_nothing_set() {
        let a = parse(&[]).unwrap();
        assert_eq!(a, Args::default());
        assert_eq!(a.action, Action::Run);
    }

    #[test]
    fn every_option_is_parsed() {
        let a = parse(&[
            "--prompt",
            "liste les fichiers",
            "--no-tui",
            "--bypass",
            "--model",
            "fast",
            "--home",
            "D:\\cmd_ai",
            "--color",
            "256",
            "--resume",
            "memory/resume.json",
        ])
        .unwrap();
        assert_eq!(
            a.prompt,
            Some(PromptSource::Text("liste les fichiers".into()))
        );
        assert_eq!(a.tui, Some(false));
        assert_eq!(a.mode, Some(PermissionMode::Bypass));
        assert_eq!(a.model.as_deref(), Some("fast"));
        assert_eq!(a.home, Some(PathBuf::from("D:\\cmd_ai")));
        assert_eq!(a.color.as_deref(), Some("256"));
        assert_eq!(a.resume, Some(PathBuf::from("memory/resume.json")));
        assert_eq!(a.action, Action::Run);
    }

    #[test]
    fn equals_syntax_is_accepted() {
        let a = parse(&["--prompt=dis bonjour", "--model=modele:1b", "--color=NONE"]).unwrap();
        assert_eq!(a.prompt, Some(PromptSource::Text("dis bonjour".into())));
        assert_eq!(a.model.as_deref(), Some("modele:1b"));
        assert_eq!(a.color.as_deref(), Some("none"));
    }

    #[test]
    fn prompt_file_and_flags() {
        let a = parse(&["--prompt-file", "demande.txt", "--tui", "--plan"]).unwrap();
        assert_eq!(
            a.prompt,
            Some(PromptSource::File(PathBuf::from("demande.txt")))
        );
        assert_eq!(a.tui, Some(true));
        assert_eq!(a.mode, Some(PermissionMode::Plan));
    }

    #[test]
    fn prompt_text_is_trimmed_and_may_start_with_a_dash() {
        let a = parse(&["--prompt", "  -l c'est quoi ?  "]).unwrap();
        assert_eq!(a.prompt, Some(PromptSource::Text("-l c'est quoi ?".into())));
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse(&["--help"]).unwrap().action, Action::Help);
        assert_eq!(parse(&["-h"]).unwrap().action, Action::Help);
        assert_eq!(parse(&["--version"]).unwrap().action, Action::Version);
        assert_eq!(parse(&["-V"]).unwrap().action, Action::Version);
        assert_eq!(parse(&["-V", "--help"]).unwrap().action, Action::Help);
        assert_eq!(parse(&["-hV"]).unwrap().action, Action::Help);
    }

    #[test]
    fn prompt_and_prompt_file_are_exclusive() {
        let e = err(&["--prompt", "a", "--prompt-file", "b.txt"]);
        assert!(e.contains("--prompt") && e.contains("--prompt-file"), "{e}");
        assert!(e.contains("incompatibles"), "{e}");
        let e = err(&["--prompt-file", "b.txt", "--prompt", "a"]);
        assert!(e.contains("incompatibles"), "{e}");
    }

    #[test]
    fn conflicting_flags_are_errors() {
        assert!(err(&["--plan", "--bypass"]).contains("incompatibles"));
        assert!(err(&["--tui", "--no-tui"]).contains("incompatibles"));
        // Same flag twice is harmless.
        assert_eq!(
            parse(&["--plan", "--plan"]).unwrap().mode,
            Some(PermissionMode::Plan)
        );
    }

    #[test]
    fn repeated_value_options_are_errors() {
        for opt in [
            "--prompt",
            "--model",
            "--home",
            "--color",
            "--resume",
            "--prompt-file",
        ] {
            let value = if opt == "--color" { "auto" } else { "x" };
            let e = err(&[opt, value, opt, value]);
            assert!(e.contains(opt), "{e}");
        }
    }

    #[test]
    fn unknown_options_are_errors() {
        let e = err(&["--promt", "x"]);
        assert!(e.contains("option inconnue « --promt »"), "{e}");
        let e = err(&["-x"]);
        assert!(e.contains("option inconnue « -x »"), "{e}");
    }

    #[test]
    fn positional_arguments_are_errors() {
        let e = err(&["liste", "les", "fichiers"]);
        assert!(e.contains("« liste »") && e.contains("--prompt"), "{e}");
    }

    #[test]
    fn missing_values_are_errors() {
        for opt in [
            "--prompt",
            "--prompt-file",
            "--model",
            "--home",
            "--color",
            "--resume",
        ] {
            let e = err(&[opt]);
            assert!(
                e.contains(&format!("l'option {opt} attend une valeur")),
                "{e}"
            );
        }
    }

    #[test]
    fn empty_values_are_errors() {
        for opt in ["--prompt", "--prompt-file", "--model", "--home", "--resume"] {
            let e = err(&[opt, "  "]);
            assert!(e.contains("est vide"), "{opt}: {e}");
        }
    }

    #[test]
    fn flags_reject_values() {
        let e = err(&["--plan=oui"]);
        assert!(e.contains("n'accepte pas de valeur"), "{e}");
    }

    #[test]
    fn bad_color_is_an_error() {
        let e = err(&["--color", "rouge"]);
        assert!(e.contains("« rouge »") && e.contains("truecolor"), "{e}");
        for ok in ["auto", "truecolor", "256", "16", "none"] {
            assert_eq!(parse(&["--color", ok]).unwrap().color.as_deref(), Some(ok));
            assert!(ColorChoice::parse(ok).is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_prompt_is_an_error_but_paths_are_fine() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![0x66, 0xFF, 0x6F]);
        let e = parse_args(vec![OsString::from("--prompt"), bad.clone()]).unwrap_err();
        assert!(e.contains("Unicode"), "{e}");
        let a = parse_args(vec![OsString::from("--home"), bad.clone()]).unwrap();
        assert_eq!(a.home, Some(PathBuf::from(bad)));
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_prompt_is_an_error_but_paths_are_fine() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x66, 0xD800, 0x6F]);
        let e = parse_args(vec![OsString::from("--prompt"), bad.clone()]).unwrap_err();
        assert!(e.contains("Unicode"), "{e}");
        let a = parse_args(vec![OsString::from("--home"), bad.clone()]).unwrap();
        assert_eq!(a.home, Some(PathBuf::from(bad)));
    }

    #[test]
    fn lone_dash_is_positional() {
        let e = err(&["-"]);
        assert!(e.contains("« - »"), "{e}");
    }

    #[test]
    fn help_wins_over_later_errors_but_not_earlier_ones() {
        assert_eq!(
            parse(&["--help", "--inconnue"]).unwrap().action,
            Action::Help
        );
        assert!(parse(&["--inconnue", "--help"]).is_err());
        let v = parse(&["--version", "--bypass"]).unwrap();
        assert_eq!(v.action, Action::Version);
        assert!(parse(&["--version", "--inconnue"]).is_err());
    }

    #[test]
    fn prompt_value_is_taken_even_when_it_looks_like_an_option() {
        let a = parse(&["--prompt", "--no-tui"]).unwrap();
        assert_eq!(a.prompt, Some(PromptSource::Text("--no-tui".into())));
        assert_eq!(a.tui, None);
    }

    #[test]
    fn double_dash_turns_everything_into_positional() {
        let e = err(&["--", "--help"]);
        assert!(e.contains("« --help »"), "{e}");
    }

    // ---- --prompt-file -------------------------------------------------------------

    #[test]
    fn prompt_file_is_read_trimmed_and_bom_stripped() {
        let t = TempDir::new("cli-prompt");
        let p = t.path().join("demande.txt");
        std::fs::write(&p, "\u{feff}\n  affiche la date et l'heure  \r\n\n").unwrap();
        assert_eq!(read_prompt_file(&p).unwrap(), "affiche la date et l'heure");
        std::fs::write(&p, "ligne 1\nligne 2 éàç\n").unwrap();
        assert_eq!(read_prompt_file(&p).unwrap(), "ligne 1\nligne 2 éàç");
        std::fs::write(&p, "ligne 1\r\nligne 2\r\n").unwrap();
        assert_eq!(read_prompt_file(&p).unwrap(), "ligne 1\nligne 2");
    }

    #[test]
    fn prompt_file_written_by_powershell_5_is_accepted() {
        let t = TempDir::new("cli-prompt-utf16");
        let p = t.path().join("demande.txt");
        let bytes: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "traduis ça\r\n"
                    .encode_utf16()
                    .flat_map(|u| u.to_le_bytes()),
            )
            .collect();
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_prompt_file(&p).unwrap(), "traduis ça");
    }

    #[test]
    fn prompt_file_errors_are_french() {
        let t = TempDir::new("cli-prompt-err");
        let missing = t.path().join("absent.txt");
        assert!(
            read_prompt_file(&missing)
                .unwrap_err()
                .contains("introuvable")
        );
        let empty = t.path().join("vide.txt");
        std::fs::write(&empty, " \n\t\n").unwrap();
        assert!(read_prompt_file(&empty).unwrap_err().contains("est vide"));
        let bin = t.path().join("binaire.txt");
        std::fs::write(&bin, [0x61, 0xFF, 0xFE, 0x00, 0xC3]).unwrap();
        assert!(read_prompt_file(&bin).unwrap_err().contains("UTF-8"));
        let big = t.path().join("gros.txt");
        std::fs::write(&big, vec![b'a'; PROMPT_FILE_MAX_BYTES as usize + 1]).unwrap();
        assert!(
            read_prompt_file(&big)
                .unwrap_err()
                .contains("trop volumineux")
        );
        assert!(
            read_prompt_file(t.path())
                .unwrap_err()
                .contains("Impossible de lire")
        );
    }
}
