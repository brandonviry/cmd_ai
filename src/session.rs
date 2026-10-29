//! Task saved before an elevated relaunch (Windows) and resumed with `--resume`.
//!
//! The state lives in `memory/resume.json` inside the cmd_ai folder and is deleted once
//! it has been read. The caller passes an already redacted prompt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::PermissionMode;
use crate::home::{self, Home, TextFileError};
use crate::text;

/// File name of the saved task, in `memory/`.
pub const RESUME_FILE: &str = "resume.json";
const RESUME_MAX_BYTES: u64 = 1024 * 1024;

/// What is needed to restart the interrupted task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeState {
    pub prompt: String,
    pub mode: PermissionMode,
    pub model: String,
    pub cwd: PathBuf,
}

/// Writes `memory/resume.json` (temporary file then rename, replacing a previous task)
/// and returns its path. Refused in ephemeral mode: nothing is written.
pub fn save_resume(home: &Home, st: &ResumeState) -> Result<PathBuf, String> {
    if home.ephemeral() {
        return Err(text::RESUME_EPHEMERAL.to_string());
    }
    let dir = home.memory_dir();
    let path = dir.join(RESUME_FILE);
    let fail = |e: &dyn std::fmt::Display| text::resume_write_failed(&path, &e.to_string());
    let json = serde_json::to_string_pretty(st).map_err(|e| fail(&e))?;
    std::fs::create_dir_all(&dir).map_err(|e| fail(&e))?;
    let tmp = dir.join(format!("{RESUME_FILE}.tmp"));
    if let Err(e) = std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(&e));
    }
    Ok(path)
}

/// Reads a saved task, then deletes the file (only once it parsed as a saved task: a
/// foreign file is never deleted). Errors are French sentences.
pub fn load_resume(path: &Path) -> Result<ResumeState, String> {
    let json = home::read_text_file(path, RESUME_MAX_BYTES).map_err(|e| match e {
        TextFileError::NotFound => text::resume_not_found(path),
        TextFileError::Io(detail) => text::resume_unreadable(path, &detail),
        TextFileError::TooBig => text::resume_invalid(path, &text::file_too_big(RESUME_MAX_BYTES)),
        TextFileError::NotText => text::resume_invalid(path, text::FILE_NOT_TEXT),
    })?;
    let mut state: ResumeState = serde_json::from_str(&json).map_err(|e| {
        let full = e.to_string();
        let message = full
            .rsplit_once(" at line ")
            .map_or(full.as_str(), |(m, _)| m);
        text::resume_invalid(path, &text::serde_detail(message))
    })?;
    // It is one of ours: never resume it twice.
    let _ = std::fs::remove_file(path);
    state.prompt = state.prompt.trim().to_string();
    if state.prompt.is_empty() {
        return Err(text::RESUME_EMPTY_PROMPT.to_string());
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::test_support::TempDir;

    fn state() -> ResumeState {
        ResumeState {
            prompt: "installe les mises à jour « système »".into(),
            mode: PermissionMode::Bypass,
            model: "modele-test".into(),
            cwd: PathBuf::from("D:/projets/démo"),
        }
    }

    #[test]
    fn save_then_load_round_trips_and_deletes() {
        let t = TempDir::new("session-roundtrip");
        let home = Home::at(t.path().to_path_buf());
        let path = save_resume(&home, &state()).unwrap();
        assert_eq!(path, home.memory_dir().join(RESUME_FILE));
        assert!(path.is_file());
        let loaded = load_resume(&path).unwrap();
        assert_eq!(loaded, state());
        assert!(
            !path.exists(),
            "le fichier doit être supprimé après lecture"
        );
        assert!(load_resume(&path).unwrap_err().contains("introuvable"));
    }

    #[test]
    fn saved_file_is_json_with_the_mode_spelling() {
        let t = TempDir::new("session-json");
        let home = Home::at(t.path().to_path_buf());
        let path = save_resume(&home, &state()).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["mode"], "bypass");
        assert_eq!(v["model"], "modele-test");
    }

    #[test]
    fn save_overwrites_a_previous_task() {
        let t = TempDir::new("session-overwrite");
        let home = Home::at(t.path().to_path_buf());
        save_resume(&home, &state()).unwrap();
        let mut second = state();
        second.prompt = "deuxième".into();
        let path = save_resume(&home, &second).unwrap();
        assert_eq!(load_resume(&path).unwrap().prompt, "deuxième");
    }

    #[test]
    fn ephemeral_home_cannot_save() {
        let t = TempDir::new("session-ephemeral");
        let home = Home {
            root: t.path().to_path_buf(),
            writable: false,
        };
        let e = save_resume(&home, &state()).unwrap_err();
        assert!(e.contains("éphémère"), "{e}");
        assert!(!home.memory_dir().exists());
    }

    #[test]
    fn foreign_files_are_refused_and_kept() {
        let t = TempDir::new("session-foreign");
        let path = t.path().join("autre.json");
        std::fs::write(&path, r#"{ "nom": "pas une tâche" }"#).unwrap();
        let e = load_resume(&path).unwrap_err();
        assert!(e.contains("n'est pas une tâche sauvegardée"), "{e}");
        assert!(
            path.exists(),
            "un fichier étranger ne doit jamais être supprimé"
        );
        std::fs::write(&path, "pas du json").unwrap();
        assert!(load_resume(&path).is_err());
        assert!(path.exists());
    }

    #[test]
    fn empty_prompt_is_refused() {
        let t = TempDir::new("session-empty");
        let path = t.path().join(RESUME_FILE);
        std::fs::write(
            &path,
            r#"{ "prompt": " ", "mode": "plan", "model": "m:1", "cwd": "." }"#,
        )
        .unwrap();
        assert!(load_resume(&path).unwrap_err().contains("demande"));
    }

    #[test]
    fn oversized_or_binary_files_are_refused_and_kept() {
        let t = TempDir::new("session-bad-file");
        let path = t.path().join(RESUME_FILE);
        std::fs::write(&path, vec![b' '; RESUME_MAX_BYTES as usize + 1]).unwrap();
        assert!(load_resume(&path).unwrap_err().contains("volumineux"));
        assert!(path.exists());
        std::fs::write(&path, [0xC3, 0x28]).unwrap();
        assert!(load_resume(&path).unwrap_err().contains("UTF-8"));
        assert!(path.exists());
    }

    #[test]
    fn unknown_fields_and_bad_modes_are_refused() {
        let t = TempDir::new("session-strict");
        let path = t.path().join(RESUME_FILE);
        std::fs::write(
            &path,
            r#"{ "prompt": "x", "mode": "plan", "model": "m:1", "cwd": ".", "bypass": true }"#,
        )
        .unwrap();
        let e = load_resume(&path).unwrap_err();
        assert!(e.contains("clé inconnue"), "{e}");
        std::fs::write(
            &path,
            r#"{ "prompt": "x", "mode": "root", "model": "m:1", "cwd": "." }"#,
        )
        .unwrap();
        assert!(load_resume(&path).unwrap_err().contains("« root »"));
        assert!(path.exists());
    }

    #[test]
    fn prompt_is_trimmed_on_load() {
        let t = TempDir::new("session-trim");
        let home = Home::at(t.path().to_path_buf());
        let mut st = state();
        st.prompt = "  fais ceci \n".into();
        let path = save_resume(&home, &st).unwrap();
        assert_eq!(load_resume(&path).unwrap().prompt, "fais ceci");
        assert!(
            !home
                .memory_dir()
                .join(format!("{RESUME_FILE}.tmp"))
                .exists()
        );
    }

    #[test]
    fn unreadable_path_is_reported() {
        let t = TempDir::new("session-dir");
        assert!(
            load_resume(t.path())
                .unwrap_err()
                .contains("impossible de lire")
        );
    }
}
