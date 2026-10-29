//! Agent notes in `memory/MEMORY.md`, one `- note` per line (US7, FR-072).
//!
//! Managed only through the self-commands `cmd_ai memory add|list|forget`. When the cmd_ai
//! folder is read-only, notes still work for the session but are never written. The
//! caller redacts notes before adding them and frames `prompt_block` as untrusted data.

use std::fs;
use std::path::{Path, PathBuf};

use crate::home;
use crate::text;

/// File name of the notes, in `memory/`.
pub const MEMORY_FILE: &str = "MEMORY.md";
/// Longest note, in characters.
pub const NOTE_MAX_CHARS: usize = 500;
/// Default budget of notes injected in the system prompt (data-model: 25 Kio).
pub const PROMPT_MAX_BYTES: usize = 25 * 1024;
const MEMORY_MAX_BYTES: u64 = 1024 * 1024;

/// The notes of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    path: PathBuf,
    writable: bool,
    max_notes: usize,
    notes: Vec<String>,
}

impl Memory {
    /// Loads `dir/MEMORY.md` (missing or unreadable file = no notes; never fails).
    ///
    /// Only lines starting with `- ` (after indentation) are notes; the first `max_notes`
    /// are kept, cleaned of control characters and cut to [`NOTE_MAX_CHARS`].
    pub fn load(dir: &Path, writable: bool, max_notes: usize) -> Memory {
        let path = dir.join(MEMORY_FILE);
        let notes = match home::read_text_file(&path, MEMORY_MAX_BYTES) {
            Ok(content) => parse_notes(&content, max_notes),
            Err(_) => Vec::new(),
        };
        Memory {
            path,
            writable,
            max_notes,
            notes,
        }
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// True when changes are written to disk (folder writable).
    pub fn persistent(&self) -> bool {
        self.writable
    }

    /// Path of `MEMORY.md`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Adds a one-line note (≤ 500 characters) and returns its number (1-based). An
    /// identical note is not duplicated: its number is returned.
    pub fn add(&mut self, note: &str) -> Result<usize, String> {
        let note = note.trim().replace('\t', " ");
        if note.is_empty() {
            return Err(text::MEMORY_NOTE_EMPTY.to_string());
        }
        if note.chars().any(char::is_control) {
            return Err(text::MEMORY_NOTE_MULTILINE.to_string());
        }
        if note.chars().count() > NOTE_MAX_CHARS {
            return Err(text::memory_note_too_long(NOTE_MAX_CHARS));
        }
        if let Some(i) = self.notes.iter().position(|n| *n == note) {
            return Ok(i + 1);
        }
        if self.notes.len() >= self.max_notes {
            return Err(text::memory_full(self.max_notes));
        }
        self.notes.push(note);
        if let Err(e) = self.save() {
            self.notes.pop();
            return Err(e);
        }
        Ok(self.notes.len())
    }

    /// Removes note `n` (1-based) and returns it; the following notes are renumbered.
    pub fn forget(&mut self, n: usize) -> Result<String, String> {
        if n == 0 || n > self.notes.len() {
            return Err(text::memory_bad_index(n, self.notes.len()));
        }
        let note = self.notes.remove(n - 1);
        if let Err(e) = self.save() {
            self.notes.insert(n - 1, note);
            return Err(e);
        }
        Ok(note)
    }

    /// Numbered notes (`1. note`), at most `max_bytes` bytes. Whole lines are kept while
    /// they fit; a first line that does not fit is cut on a char boundary.
    pub fn prompt_block(&self, max_bytes: usize) -> String {
        let mut out = String::new();
        for (i, note) in self.notes.iter().enumerate() {
            let line = format!("{}. {note}", i + 1);
            let needed = if out.is_empty() {
                line.len()
            } else {
                line.len() + 1
            };
            if out.len() + needed <= max_bytes {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&line);
            } else {
                if out.is_empty() {
                    out.push_str(cut_at_char_boundary(&line, max_bytes));
                }
                break;
            }
        }
        out
    }

    /// Writes every note (temporary file then rename). Read-only home: nothing is written.
    fn save(&self) -> Result<(), String> {
        if !self.writable {
            return Ok(());
        }
        let fail = |e: std::io::Error| text::memory_write_failed(&self.path, &e.to_string());
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(fail)?;
        }
        let mut content = String::from(text::MEMORY_FILE_HEADER);
        content.push('\n');
        for note in &self.notes {
            content.push_str("- ");
            content.push_str(note);
            content.push('\n');
        }
        let tmp = self.path.with_extension("md.tmp");
        if let Err(e) = fs::write(&tmp, content).and_then(|()| fs::rename(&tmp, &self.path)) {
            let _ = fs::remove_file(&tmp);
            return Err(fail(e));
        }
        Ok(())
    }
}

/// The notes of a `MEMORY.md`: lines `- note`, at most `max_notes`.
fn parse_notes(content: &str, max_notes: usize) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("- "))
        .map(clean_note)
        .filter(|note| !note.is_empty())
        .take(max_notes)
        .collect()
}

/// A note read from disk: control characters become spaces, at most `NOTE_MAX_CHARS`.
fn clean_note(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NOTE_MAX_CHARS)
        .collect();
    cleaned.trim().to_string()
}

/// The longest prefix of `s` of at most `max_bytes` bytes ending on a char boundary.
fn cut_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::test_support::TempDir;

    fn mem(t: &TempDir) -> Memory {
        Memory::load(&t.path().join("memory"), true, 200)
    }

    #[test]
    fn missing_file_means_no_notes() {
        let t = TempDir::new("memory-empty");
        let m = mem(&t);
        assert!(m.notes().is_empty());
        assert!(m.persistent());
        assert_eq!(m.path(), t.path().join("memory").join(MEMORY_FILE));
        assert_eq!(m.prompt_block(PROMPT_MAX_BYTES), "");
    }

    #[test]
    fn add_persists_and_reloads() {
        let t = TempDir::new("memory-add");
        let mut m = mem(&t);
        assert_eq!(m.add("le projet utilise pnpm").unwrap(), 1);
        assert_eq!(m.add("  préfère PowerShell 7  ").unwrap(), 2);
        assert_eq!(
            m.notes(),
            ["le projet utilise pnpm", "préfère PowerShell 7"]
        );
        let raw = fs::read_to_string(m.path()).unwrap();
        assert!(raw.contains("- le projet utilise pnpm\n"), "{raw}");
        assert!(raw.contains("- préfère PowerShell 7\n"), "{raw}");
        let again = mem(&t);
        assert_eq!(again.notes(), m.notes());
        assert!(!t.path().join("memory").join("MEMORY.md.tmp").exists());
    }

    #[test]
    fn load_reads_only_dash_lines() {
        let t = TempDir::new("memory-parse");
        let dir = t.path().join("memory");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(MEMORY_FILE),
            "\u{feff}# Titre\n\nTexte libre\n- première\r\n-  deuxième  \n-\n- \n  - indentée\n* étoile\n-collée\n",
        )
        .unwrap();
        let m = Memory::load(&dir, true, 200);
        assert_eq!(m.notes(), ["première", "deuxième", "indentée"]);
    }

    #[test]
    fn notes_read_from_disk_are_cleaned_and_capped() {
        let t = TempDir::new("memory-clean");
        let dir = t.path().join("memory");
        fs::create_dir_all(&dir).unwrap();
        let long = "é".repeat(NOTE_MAX_CHARS + 20);
        fs::write(
            dir.join(MEMORY_FILE),
            format!("- a\u{1b}]0;titre\u{7}b\n- {long}\n"),
        )
        .unwrap();
        let m = Memory::load(&dir, true, 200);
        assert_eq!(m.notes().len(), 2);
        assert!(
            !m.notes()[0].chars().any(char::is_control),
            "{:?}",
            m.notes()[0]
        );
        assert!(m.notes()[0].starts_with('a') && m.notes()[0].ends_with('b'));
        assert_eq!(m.notes()[1].chars().count(), NOTE_MAX_CHARS);
    }

    #[test]
    fn saved_file_has_the_header_and_reloads_identically() {
        let t = TempDir::new("memory-header");
        let mut m = mem(&t);
        m.add("une note").unwrap();
        let raw = fs::read_to_string(m.path()).unwrap();
        assert!(raw.starts_with(text::MEMORY_FILE_HEADER), "{raw}");
        assert!(raw.ends_with("- une note\n"), "{raw}");
        assert_eq!(mem(&t), m);
    }

    #[test]
    fn load_keeps_at_most_max_notes() {
        let t = TempDir::new("memory-load-cap");
        let dir = t.path().join("memory");
        fs::create_dir_all(&dir).unwrap();
        let content: String = (1..=10).map(|i| format!("- note {i}\n")).collect();
        fs::write(dir.join(MEMORY_FILE), content).unwrap();
        let m = Memory::load(&dir, true, 3);
        assert_eq!(m.notes(), ["note 1", "note 2", "note 3"]);
    }

    #[test]
    fn add_rejects_bad_notes() {
        let t = TempDir::new("memory-bad");
        let mut m = mem(&t);
        assert_eq!(m.add("   ").unwrap_err(), text::MEMORY_NOTE_EMPTY);
        assert_eq!(
            m.add("deux\nlignes").unwrap_err(),
            text::MEMORY_NOTE_MULTILINE
        );
        assert_eq!(
            m.add("retour\rchariot").unwrap_err(),
            text::MEMORY_NOTE_MULTILINE
        );
        assert_eq!(
            m.add("échap \u{1b}[31m").unwrap_err(),
            text::MEMORY_NOTE_MULTILINE
        );
        let long = "é".repeat(NOTE_MAX_CHARS + 1);
        assert!(m.add(&long).unwrap_err().contains("trop longue"));
        assert_eq!(m.add(&"é".repeat(NOTE_MAX_CHARS)).unwrap(), 1);
        assert!(!m.path().exists() || mem(&t).notes().len() == 1);
    }

    #[test]
    fn trailing_newline_and_tabs_are_tolerated() {
        let t = TempDir::new("memory-tabs");
        let mut m = mem(&t);
        assert_eq!(m.add("note\tavec tabulation\n").unwrap(), 1);
        assert_eq!(m.notes(), ["note avec tabulation"]);
    }

    #[test]
    fn duplicates_are_not_added() {
        let t = TempDir::new("memory-dup");
        let mut m = mem(&t);
        assert_eq!(m.add("a").unwrap(), 1);
        assert_eq!(m.add("b").unwrap(), 2);
        assert_eq!(m.add(" a ").unwrap(), 1);
        assert_eq!(m.notes().len(), 2);
    }

    #[test]
    fn cap_is_enforced() {
        let t = TempDir::new("memory-cap");
        let mut m = Memory::load(&t.path().join("memory"), true, 2);
        m.add("a").unwrap();
        m.add("b").unwrap();
        let e = m.add("c").unwrap_err();
        assert!(e.contains("pleine") && e.contains('2'), "{e}");
        assert_eq!(m.notes().len(), 2);
    }

    #[test]
    fn forget_renumbers_and_persists() {
        let t = TempDir::new("memory-forget");
        let mut m = mem(&t);
        for n in ["a", "b", "c"] {
            m.add(n).unwrap();
        }
        assert_eq!(m.forget(2).unwrap(), "b");
        assert_eq!(m.notes(), ["a", "c"]);
        assert_eq!(mem(&t).notes(), ["a", "c"]);
        assert_eq!(m.forget(2).unwrap(), "c");
        assert!(m.forget(0).unwrap_err().contains("entre 1 et 1"));
        assert!(m.forget(2).unwrap_err().contains("entre 1 et 1"));
        assert_eq!(m.forget(1).unwrap(), "a");
        assert!(m.forget(1).unwrap_err().contains("vide"));
        assert!(mem(&t).notes().is_empty());
    }

    #[test]
    fn read_only_home_keeps_notes_in_memory_only() {
        let t = TempDir::new("memory-ro");
        let dir = t.path().join("memory");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MEMORY_FILE), "- existante\n").unwrap();
        let mut m = Memory::load(&dir, false, 200);
        assert!(!m.persistent());
        assert_eq!(m.notes(), ["existante"]);
        assert_eq!(m.add("nouvelle").unwrap(), 2);
        assert_eq!(m.forget(1).unwrap(), "existante");
        assert_eq!(m.notes(), ["nouvelle"]);
        assert_eq!(
            fs::read_to_string(dir.join(MEMORY_FILE)).unwrap(),
            "- existante\n"
        );
        let t2 = TempDir::new("memory-ro-absent");
        let mut m2 = Memory::load(&t2.path().join("memory"), false, 200);
        m2.add("x").unwrap();
        assert!(!t2.path().join("memory").exists());
    }

    #[test]
    fn write_failure_rolls_back() {
        let t = TempDir::new("memory-write-fail");
        // `memory` is a file: the directory cannot be created.
        fs::write(t.path().join("memory"), b"").unwrap();
        let mut m = Memory::load(&t.path().join("memory"), true, 200);
        let e = m.add("perdue").unwrap_err();
        assert!(e.contains("impossible d'enregistrer"), "{e}");
        assert!(m.notes().is_empty());
    }

    #[test]
    fn prompt_block_numbers_and_truncates() {
        let t = TempDir::new("memory-prompt");
        let mut m = mem(&t);
        m.add("première note").unwrap();
        m.add("deuxième note").unwrap();
        m.add("troisième").unwrap();
        let full = m.prompt_block(PROMPT_MAX_BYTES);
        assert_eq!(full, "1. première note\n2. deuxième note\n3. troisième");
        // Only whole lines when possible.
        let two = "1. première note\n2. deuxième note";
        assert_eq!(m.prompt_block(two.len()), two);
        assert_eq!(m.prompt_block(two.len() + 3), two);
        assert_eq!(m.prompt_block(0), "");
        // A single line that does not fit is cut on a char boundary.
        for max in 1..20 {
            let block = m.prompt_block(max);
            assert!(block.len() <= max, "{max}: {block:?}");
            assert!("1. première note".starts_with(&block), "{max}: {block:?}");
        }
        assert_eq!(m.prompt_block(5), "1. pr");
        // 'è' takes bytes 8 and 9 of "1. première note".
        assert_eq!(m.prompt_block(8), "1. premi");
        assert_eq!(m.prompt_block(9), "1. premi");
        assert_eq!(m.prompt_block(10), "1. premiè");
    }
}
