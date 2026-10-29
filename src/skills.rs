//! Skills: hand-written playbooks in `skills/<nom>/SKILL.md` (Agent Skills format, US5).
//!
//! The catalog (name + description) goes into the system prompt; the body is returned by
//! the intercepted self-command `cmd_ai skill <nom>`, looked up in the catalog loaded at
//! startup (no file is read at request time, so no path traversal is possible).

use std::fs;
use std::io;
use std::path::Path;

use crate::home::{self, TextFileError};
use crate::text;

/// File name of a skill, inside its folder.
pub const SKILL_FILE: &str = "SKILL.md";
/// Longest skill name.
pub const NAME_MAX_LEN: usize = 64;
/// Longest description, in characters.
pub const DESCRIPTION_MAX_CHARS: usize = 1024;
/// Largest accepted `SKILL.md`.
pub const SKILL_MAX_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters.
pub fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX_LEN
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Loads every valid `skills/<nom>/SKILL.md`, sorted by name, plus French warnings for
/// the folders that were skipped. A missing `skills/` folder is not an error. Hidden
/// entries (`.…`) and plain files are ignored; symbolic links are never followed.
pub fn load_catalog(dir: &Path) -> (Vec<Skill>, Vec<String>) {
    let mut skills = Vec::new();
    let mut warnings = Vec::new();
    match fs::metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (skills, warnings),
        Err(e) => {
            warnings.push(text::skills_dir_unreadable(dir, &e.to_string()));
            return (skills, warnings);
        }
        Ok(meta) if !meta.is_dir() => {
            warnings.push(text::skills_dir_unreadable(dir, text::SKILLS_DIR_NOT_DIR));
            return (skills, warnings);
        }
        Ok(_) => {}
    }
    let mut entries: Vec<fs::DirEntry> = match fs::read_dir(dir) {
        Ok(entries) => entries.flatten().collect(),
        Err(e) => {
            warnings.push(text::skills_dir_unreadable(dir, &e.to_string()));
            return (skills, warnings);
        }
    };
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let raw_name = entry.file_name();
        let shown = raw_name.to_string_lossy().into_owned();
        if shown.starts_with('.') {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                warnings.push(text::skill_warning(
                    &shown,
                    &text::skill_warn_read(&e.to_string()),
                ));
                continue;
            }
        };
        if file_type.is_symlink() {
            warnings.push(text::skill_warning(&shown, text::SKILL_WARN_SYMLINK));
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let Some(name) = raw_name.to_str().filter(|n| valid_skill_name(n)) else {
            warnings.push(text::skill_warning(&shown, text::SKILL_WARN_INVALID_DIR));
            continue;
        };
        match load_skill(&entry.path(), name) {
            Ok(skill) => skills.push(skill),
            Err(detail) => warnings.push(text::skill_warning(&shown, &detail)),
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    (skills, warnings)
}

/// Reads and checks `dir/SKILL.md`, whose `name` must equal the folder name.
fn load_skill(dir: &Path, dir_name: &str) -> Result<Skill, String> {
    let path = dir.join(SKILL_FILE);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(text::SKILL_WARN_MISSING_FILE.to_string());
        }
        Err(e) => return Err(text::skill_warn_read(&e.to_string())),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(text::SKILL_WARN_SYMLINK.to_string());
        }
        Ok(meta) if !meta.is_file() => return Err(text::SKILL_WARN_MISSING_FILE.to_string()),
        Ok(_) => {}
    }
    let content = home::read_text_file(&path, SKILL_MAX_BYTES).map_err(|e| match e {
        TextFileError::NotFound => text::SKILL_WARN_MISSING_FILE.to_string(),
        TextFileError::TooBig => text::skill_warn_too_big(SKILL_MAX_BYTES),
        TextFileError::NotText => text::SKILL_WARN_NOT_TEXT.to_string(),
        TextFileError::Io(detail) => text::skill_warn_read(&detail),
    })?;
    let (name, description, body) = parse_skill_md(&content)?;
    if name != dir_name {
        return Err(text::skill_warn_name_mismatch(&name, dir_name));
    }
    Ok(Skill {
        name,
        description,
        body,
    })
}

/// Parses a `SKILL.md`: `---` frontmatter (YAML subset: `name`, `description`, plain or
/// quoted values, block scalars; other keys ignored), then the body.
/// Returns `(name, description, body)`.
pub fn parse_skill_md(text: &str) -> Result<(String, String, String), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .filter(|&i| lines[i].trim_end() == "---")
        .ok_or_else(|| text::SKILL_NO_FRONTMATTER.to_string())?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.trim_end() == "---")
        .map(|(i, _)| i)
        .ok_or_else(|| text::SKILL_UNCLOSED_FRONTMATTER.to_string())?;
    let fields = frontmatter_fields(&lines[start + 1..end]);
    let name = required_field(&fields, "name")?;
    let description = required_field(&fields, "description")?;
    if !valid_skill_name(&name) {
        return Err(text::skill_bad_name(&name));
    }
    if description.chars().count() > DESCRIPTION_MAX_CHARS {
        return Err(text::skill_description_too_long(DESCRIPTION_MAX_CHARS));
    }
    let body = body_text(&lines[end + 1..]);
    Ok((name, description, body))
}

/// The skill called `name` in the catalog.
pub fn find_skill<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|s| s.name == name)
}

/// One top-level key of the frontmatter: its inline value and its continuation lines.
struct Field<'a> {
    key: &'a str,
    inline: &'a str,
    more: Vec<&'a str>,
}

/// Splits the frontmatter into top-level keys. Indented lines, `- item` lines and blank
/// lines belong to the previous key; `# comments` and other lines are ignored.
fn frontmatter_fields<'a>(lines: &[&'a str]) -> Vec<Field<'a>> {
    let mut fields: Vec<Field<'a>> = Vec::new();
    let mut open = false;
    for &line in lines {
        let continuation = line.trim().is_empty()
            || line.starts_with([' ', '\t'])
            || line == "-"
            || line.starts_with("- ");
        if continuation {
            if open {
                if let Some(field) = fields.last_mut() {
                    field.more.push(line);
                }
            }
            continue;
        }
        open = false;
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, inline)) = split_key(line) {
            fields.push(Field {
                key,
                inline,
                more: Vec::new(),
            });
            open = true;
        }
    }
    fields
}

/// `key: value` → (`key`, `value`); the separator is the first `:` followed by a space,
/// a tab or the end of the line.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let idx = bytes.iter().enumerate().position(|(i, &b)| {
        b == b':' && matches!(bytes.get(i + 1), None | Some(b' ') | Some(b'\t'))
    })?;
    let key = line[..idx].trim().trim_matches(['"', '\'']);
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, &line[idx + 1..]))
}

/// The value of `key`, required once and not blank.
fn required_field(fields: &[Field<'_>], key: &str) -> Result<String, String> {
    let mut found = fields.iter().filter(|f| f.key == key);
    let Some(field) = found.next() else {
        return Err(text::skill_missing_key(key));
    };
    if found.next().is_some() {
        return Err(text::skill_duplicate_key(key));
    }
    let value = normalize_spaces(&scalar_value(field)?);
    if value.is_empty() {
        return Err(text::skill_missing_key(key));
    }
    Ok(value)
}

/// Value of a YAML scalar: double-quoted, single-quoted, block (`|`, `>`) or plain.
/// Multi-line values are folded into one line.
fn scalar_value(field: &Field<'_>) -> Result<String, String> {
    let inline = field.inline.trim();
    if let Some(rest) = inline.strip_prefix('"') {
        let joined = join_continuation(rest, &field.more);
        return double_quoted(&joined).ok_or_else(|| text::skill_bad_quote(field.key));
    }
    if let Some(rest) = inline.strip_prefix('\'') {
        let joined = join_continuation(rest, &field.more);
        return single_quoted(&joined).ok_or_else(|| text::skill_bad_quote(field.key));
    }
    if is_block_header(strip_comment(inline)) {
        let lines: Vec<&str> = field
            .more
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        return Ok(lines.join(" "));
    }
    let mut parts = vec![strip_comment(inline).trim()];
    for line in &field.more {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        parts.push(strip_comment(line).trim());
    }
    let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    Ok(parts.join(" "))
}

/// `first` followed by the trimmed continuation lines, separated by single spaces.
fn join_continuation(first: &str, more: &[&str]) -> String {
    let mut s = first.to_string();
    for line in more {
        s.push(' ');
        s.push_str(line.trim());
    }
    s
}

/// Content of a double-quoted scalar (opening quote already removed), escapes decoded;
/// `None` without closing quote.
fn double_quoted(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => {}
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(decoded) => out.push(decoded),
                        None => {
                            out.push_str("\\u");
                            out.push_str(&hex);
                        }
                    }
                }
                e @ ('"' | '\\' | '/' | ' ') => out.push(e),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            _ => out.push(c),
        }
    }
    None
}

/// Content of a single-quoted scalar (opening quote already removed), `''` → `'`;
/// `None` without closing quote.
fn single_quoted(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                out.push('\'');
            } else {
                return Some(out);
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// Removes a YAML comment: `#` at the start or after a space or a tab.
fn strip_comment(s: &str) -> &str {
    if s.starts_with('#') {
        return "";
    }
    let bytes = s.as_bytes();
    match (1..bytes.len()).find(|&i| bytes[i] == b'#' && matches!(bytes[i - 1], b' ' | b'\t')) {
        Some(i) => &s[..i],
        None => s,
    }
}

/// `|`, `>`, with optional chomping (`+`, `-`) and indentation (1-9) indicators.
fn is_block_header(s: &str) -> bool {
    let s = s.trim();
    let mut chars = s.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    rest.len() <= 2
        && rest.iter().filter(|c| matches!(c, '+' | '-')).count() <= 1
        && rest.iter().filter(|c| c.is_ascii_digit()).count() <= 1
        && rest.iter().all(|c| matches!(c, '+' | '-' | '1'..='9'))
}

/// Control characters become spaces, runs of spaces collapse, ends are trimmed.
fn normalize_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if c == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        out.push(c);
    }
    out.trim_end().to_string()
}

/// The body after the frontmatter, without leading blank lines and trailing whitespace.
fn body_text(lines: &[&str]) -> String {
    let first = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(lines.len());
    lines[first..].join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::test_support::TempDir;

    #[test]
    fn names_follow_the_agent_skills_rule() {
        for ok in ["a", "projet-npm", "x1-y2-z3", "123", &"a".repeat(64)] {
            assert!(valid_skill_name(ok), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "Projet",
            "projet_npm",
            "projet npm",
            "../config",
            "..",
            ".",
            "a/b",
            "a\\b",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!valid_skill_name(bad), "{bad}");
        }
    }

    #[test]
    fn parses_plain_frontmatter_and_body() {
        let md = "---\nname: projet-npm\ndescription: Travailler dans un projet npm\n---\n\n# Protocole\n\n1. npm ci\n";
        let (name, desc, body) = parse_skill_md(md).unwrap();
        assert_eq!(name, "projet-npm");
        assert_eq!(desc, "Travailler dans un projet npm");
        assert_eq!(body, "# Protocole\n\n1. npm ci");
    }

    #[test]
    fn parses_quoted_values_crlf_and_bom() {
        let md = "\u{feff}---\r\nname: \"projet-npm\"\r\ndescription: 'L''outil « npm » : dépendances'\r\nlicense: MIT\r\n---\r\nCorps\r\n";
        let (name, desc, body) = parse_skill_md(md).unwrap();
        assert_eq!(name, "projet-npm");
        assert_eq!(desc, "L'outil « npm » : dépendances");
        assert_eq!(body, "Corps");
    }

    #[test]
    fn double_quoted_escapes() {
        let md = "---\nname: a\ndescription: \"dit \\\"bonjour\\\" \\\\ fin\"\n---\nx";
        assert_eq!(parse_skill_md(md).unwrap().1, "dit \"bonjour\" \\ fin");
    }

    #[test]
    fn ignores_other_keys_comments_and_nested_values() {
        let md = "---\n# commentaire\nname: a\nallowed-tools:\n  - Bash\n  - Read\nmetadata:\n  version: 2\n  name: pas-moi\ntags:\n- x\n- y\ndescription: d # commentaire YAML\n---\nb";
        let (name, desc, body) = parse_skill_md(md).unwrap();
        assert_eq!(
            (name.as_str(), desc.as_str(), body.as_str()),
            ("a", "d", "b")
        );
    }

    #[test]
    fn block_and_multiline_descriptions_are_joined() {
        let folded =
            "---\nname: a\ndescription: >-\n  Première ligne\n  suite de la phrase.\n---\nb";
        assert_eq!(
            parse_skill_md(folded).unwrap().1,
            "Première ligne suite de la phrase."
        );
        let literal = "---\nname: a\ndescription: |\n  ligne un\n  ligne deux\n---\nb";
        assert_eq!(parse_skill_md(literal).unwrap().1, "ligne un ligne deux");
        let plain = "---\nname: a\ndescription: début\n  continuation\n---\nb";
        assert_eq!(parse_skill_md(plain).unwrap().1, "début continuation");
        let quoted = "---\nname: a\ndescription: \"début\n  fin\"\n---\nb";
        assert_eq!(parse_skill_md(quoted).unwrap().1, "début fin");
    }

    #[test]
    fn hash_inside_values_is_kept_when_not_a_comment() {
        let md = "---\nname: a\ndescription: Projets C# et F#\n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().1, "Projets C# et F#");
        let md = "---\nname: a\ndescription: \"issue #12\"\n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().1, "issue #12");
    }

    #[test]
    fn frontmatter_errors() {
        assert_eq!(
            parse_skill_md("name: a\n---\n").unwrap_err(),
            text::SKILL_NO_FRONTMATTER
        );
        assert_eq!(
            parse_skill_md("---\nname: a\ndescription: d\n").unwrap_err(),
            text::SKILL_UNCLOSED_FRONTMATTER
        );
        assert!(
            parse_skill_md("---\ndescription: d\n---\nb")
                .unwrap_err()
                .contains("« name »")
        );
        assert!(
            parse_skill_md("---\nname: a\n---\nb")
                .unwrap_err()
                .contains("« description »")
        );
        assert!(
            parse_skill_md("---\nname: a\ndescription: \"\"\n---\nb")
                .unwrap_err()
                .contains("« description »")
        );
        assert!(
            parse_skill_md("---\nname: a\nname: b\ndescription: d\n---\nb")
                .unwrap_err()
                .contains("plusieurs fois")
        );
        assert!(
            parse_skill_md("---\nname: \"a\ndescription: d\n---\nb")
                .unwrap_err()
                .contains("guillemet")
        );
        assert!(
            parse_skill_md("---\nname: Projet NPM\ndescription: d\n---\nb")
                .unwrap_err()
                .contains("invalide")
        );
        let long = format!("---\nname: a\ndescription: {}\n---\nb", "é".repeat(1025));
        assert!(parse_skill_md(&long).unwrap_err().contains("trop longue"));
        let max = format!("---\nname: a\ndescription: {}\n---\nb", "é".repeat(1024));
        assert!(parse_skill_md(&max).is_ok());
    }

    #[test]
    fn escapes_and_control_characters_are_flattened() {
        let md = "---\nname: a\ndescription: \"caf\\u00e9\\tet\\nthé\"\n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().1, "café et thé");
        let md = "---\nname: a\ndescription:   espaces    multiples  \n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().1, "espaces multiples");
    }

    #[test]
    fn value_on_the_next_lines_and_blank_lines_between_keys() {
        let md = "---\nname: a\n\ndescription:\n  sur la ligne suivante\n\nlicense: MIT\n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().1, "sur la ligne suivante");
    }

    #[test]
    fn leading_blank_lines_before_frontmatter_are_tolerated() {
        let md = "\n\n---\nname: a\ndescription: d\n---\nb";
        assert_eq!(parse_skill_md(md).unwrap().0, "a");
    }

    #[test]
    fn keys_must_be_followed_by_a_space() {
        // "name:a" is not a key in YAML: name is missing.
        let md = "---\nname:a\ndescription: d\n---\nb";
        assert!(parse_skill_md(md).unwrap_err().contains("« name »"));
    }

    #[test]
    fn block_header_detection() {
        for ok in ["|", ">", "|-", ">+", "|2", ">-1", "|1+"] {
            assert!(is_block_header(ok), "{ok}");
        }
        for bad in ["", "|x", "> texte", "|--", "|12", "a"] {
            assert!(!is_block_header(bad), "{bad}");
        }
    }

    #[test]
    fn empty_body_is_allowed() {
        let (_, _, body) = parse_skill_md("---\nname: a\ndescription: d\n---\n").unwrap();
        assert_eq!(body, "");
        let (_, _, body) = parse_skill_md("---\nname: a\ndescription: d\n---").unwrap();
        assert_eq!(body, "");
    }

    #[test]
    fn body_keeps_inner_separators() {
        let md = "---\nname: a\ndescription: d\n---\navant\n---\naprès\n";
        assert_eq!(parse_skill_md(md).unwrap().2, "avant\n---\naprès");
    }

    #[test]
    fn example_skill_is_valid() {
        let md = include_str!("../portable/skills/exemple-projet-npm/SKILL.md");
        let (name, desc, body) = parse_skill_md(md).unwrap();
        assert_eq!(name, "exemple-projet-npm");
        assert!(desc.contains("npm"));
        assert!(body.contains("npm"));
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("portable")
            .join("skills");
        let (skills, warnings) = load_catalog(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "exemple-projet-npm");
    }

    fn write_skill(root: &Path, dir: &str, content: &str) {
        let d = root.join(dir);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(SKILL_FILE), content).unwrap();
    }

    fn skill_md(name: &str, desc: &str) -> String {
        format!("---\nname: {name}\ndescription: {desc}\n---\nCorps de {name}\n")
    }

    #[test]
    fn catalog_is_sorted_and_reports_invalid_skills() {
        let t = TempDir::new("skills-catalog");
        let root = t.path();
        write_skill(root, "zeta", &skill_md("zeta", "dernier"));
        write_skill(root, "alpha", &skill_md("alpha", "premier"));
        write_skill(root, "mauvais-nom", &skill_md("autre-nom", "x"));
        write_skill(root, "Majuscule", &skill_md("majuscule", "x"));
        write_skill(root, "casse", "pas d'en-tête");
        fs::create_dir_all(root.join("vide")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join("README.md"), "ignoré").unwrap();
        let big = format!(
            "{}{}",
            skill_md("gros", "x"),
            "a".repeat(SKILL_MAX_BYTES as usize)
        );
        write_skill(root, "gros", &big);
        let d = root.join("binaire");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(SKILL_FILE), [0xC3, 0x28]).unwrap();

        let (skills, warnings) = load_catalog(root);
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zeta"]);
        assert_eq!(skills[0].description, "premier");
        assert_eq!(skills[0].body, "Corps de alpha");
        let all = warnings.join("\n");
        assert_eq!(warnings.len(), 6, "{all}");
        assert!(
            all.contains("skills/mauvais-nom/SKILL.md") && all.contains("« autre-nom »"),
            "{all}"
        );
        assert!(
            all.contains("skills/Majuscule/SKILL.md") && all.contains("nom de dossier"),
            "{all}"
        );
        assert!(
            all.contains("skills/casse/SKILL.md") && all.contains("en-tête"),
            "{all}"
        );
        assert!(
            all.contains("skills/vide/SKILL.md") && all.contains("absent"),
            "{all}"
        );
        assert!(
            all.contains("skills/gros/SKILL.md") && all.contains("volumineux"),
            "{all}"
        );
        assert!(
            all.contains("skills/binaire/SKILL.md") && all.contains("UTF-8"),
            "{all}"
        );
        assert!(!all.contains(".cache") && !all.contains("README"), "{all}");
    }

    #[test]
    fn missing_skills_dir_is_empty_without_warning() {
        let t = TempDir::new("skills-missing");
        let (skills, warnings) = load_catalog(&t.path().join("skills"));
        assert!(skills.is_empty() && warnings.is_empty());
    }

    #[test]
    fn skills_path_that_is_a_file_warns() {
        let t = TempDir::new("skills-file");
        let f = t.path().join("skills");
        fs::write(&f, "pas un dossier").unwrap();
        let (skills, warnings) = load_catalog(&f);
        assert!(skills.is_empty());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_skills_are_not_followed() {
        let t = TempDir::new("skills-symlink");
        let outside = t.path().join("dehors");
        write_skill(&outside, "secret", &skill_md("secret", "x"));
        let root = t.path().join("skills");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("secret")).unwrap();
        let (skills, warnings) = load_catalog(&root);
        assert!(skills.is_empty());
        assert!(warnings[0].contains("liens symboliques"), "{warnings:?}");
    }

    #[test]
    fn find_skill_by_exact_name() {
        let skills = vec![Skill {
            name: "projet-npm".into(),
            description: "d".into(),
            body: "b".into(),
        }];
        assert_eq!(find_skill(&skills, "projet-npm").unwrap().body, "b");
        assert!(find_skill(&skills, "Projet-npm").is_none());
        assert!(find_skill(&skills, "../projet-npm").is_none());
    }
}
