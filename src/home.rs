//! The portable cmd_ai folder (constitution, principe III).
//!
//! The folder is resolved from `--home`, then `CMD_AI_HOME`, then the directory of the
//! canonicalized executable, never from the current directory. It is writable when a probe
//! file can be created and removed; otherwise cmd_ai runs in ephemeral mode.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Environment variable that overrides the folder (after `--home`).
pub const ENV_HOME: &str = "CMD_AI_HOME";

/// Probe file created then removed to test writability.
pub const PROBE_FILE: &str = ".cmd_ai_write_test";

/// The resolved cmd_ai folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub root: PathBuf,
    pub writable: bool,
}

/// Why the folder could not be resolved (French text: `text::home_error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeError {
    /// `--home` / `CMD_AI_HOME` points to a path that does not exist.
    NotFound(PathBuf),
    /// `--home` / `CMD_AI_HOME` points to something that is not a directory.
    NotADirectory(PathBuf),
    /// The executable location could not be determined (detail).
    NoExecutable(String),
}

impl std::fmt::Display for HomeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::text::home_error(self))
    }
}

impl std::error::Error for HomeError {}

impl Home {
    /// `--home` > `CMD_AI_HOME` > `canonicalize(current_exe).parent()`.
    pub fn resolve(override_dir: Option<&Path>) -> Result<Home, HomeError> {
        Self::resolve_with(
            override_dir,
            std::env::var_os(ENV_HOME),
            std::env::current_exe,
        )
    }

    /// Same as [`Home::resolve`] with injected inputs (testable, no global state).
    pub fn resolve_with(
        override_dir: Option<&Path>,
        env_home: Option<OsString>,
        current_exe: impl FnOnce() -> io::Result<PathBuf>,
    ) -> Result<Home, HomeError> {
        let root = if let Some(dir) = override_dir {
            existing_dir(dir)?
        } else if let Some(env) = env_home.filter(|v| !v.is_empty()) {
            existing_dir(Path::new(&env))?
        } else {
            exe_dir(current_exe)?
        };
        Ok(Home::at(root))
    }

    /// A home rooted at `root`, with its writability probed now.
    pub fn at(root: PathBuf) -> Home {
        let writable = probe_writable(&root);
        Home { root, writable }
    }

    /// `root/config.json`
    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    /// `root/config.example.json`
    pub fn example_config_path(&self) -> PathBuf {
        self.root.join("config.example.json")
    }

    /// `root/log`
    pub fn log_dir(&self) -> PathBuf {
        self.root.join("log")
    }

    /// `root/memory`
    pub fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }

    /// `root/skills`
    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    /// Ephemeral mode: no logs, no persisted memory.
    pub fn ephemeral(&self) -> bool {
        !self.writable
    }
}

/// True when a probe file can be created, written and removed in `dir`.
pub fn probe_writable(dir: &Path) -> bool {
    let probe = dir.join(PROBE_FILE);
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
        .and_then(|mut f| f.write_all(b"cmd_ai"));
    if written.is_err() {
        return false;
    }
    fs::remove_file(&probe).is_ok()
}

fn existing_dir(dir: &Path) -> Result<PathBuf, HomeError> {
    let canonical = fs::canonicalize(dir).map_err(|_| HomeError::NotFound(dir.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(HomeError::NotADirectory(dir.to_path_buf()));
    }
    Ok(simplify_path(canonical))
}

fn exe_dir(current_exe: impl FnOnce() -> io::Result<PathBuf>) -> Result<PathBuf, HomeError> {
    let exe = current_exe().map_err(|e| HomeError::NoExecutable(e.to_string()))?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    match exe.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(simplify_path(parent.to_path_buf())),
        _ => Err(HomeError::NoExecutable(exe.display().to_string())),
    }
}

/// Removes the Windows verbatim prefix added by `canonicalize` (`\\?\C:\…` → `C:\…`,
/// `\\?\UNC\srv\share` → `\\srv\share`) so paths stay readable and usable as working
/// directories. Other paths are returned unchanged.
pub fn simplify_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = path.to_str() {
            if s.len() < 260 {
                if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
                    return PathBuf::from(format!(r"\\{rest}"));
                }
                if let Some(rest) = s.strip_prefix(r"\\?\") {
                    let b = rest.as_bytes();
                    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
                        return PathBuf::from(rest);
                    }
                }
            }
        }
    }
    path
}

/// Why a text file could not be read (callers turn it into a French message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFileError {
    NotFound,
    TooBig,
    NotText,
    Io(String),
}

/// Reads a small text file: UTF-8 (BOM stripped), or UTF-16 with a BOM (files written by
/// Windows PowerShell 5.1). At most `max_bytes` bytes.
pub fn read_text_file(path: &Path, max_bytes: u64) -> Result<String, TextFileError> {
    let file = fs::File::open(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => TextFileError::NotFound,
        _ => TextFileError::Io(e.to_string()),
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| TextFileError::Io(e.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(TextFileError::TooBig);
    }
    decode_text(&bytes).ok_or(TextFileError::NotText)
}

/// Decodes UTF-8 (optional BOM) or UTF-16 LE/BE with BOM; `None` if invalid.
pub fn decode_text(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).ok();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    String::from_utf8(bytes.to_vec()).ok()
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
    String::from_utf16(&units).ok()
}

/// Scratch directories for unit tests, under `target/test-tmp/` (never the host's temp dir).
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> TempDir {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-tmp")
                .join(format!("{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempDir;
    use super::*;

    fn no_exe() -> io::Result<PathBuf> {
        Err(io::Error::other("pas d'exécutable"))
    }

    #[test]
    fn override_dir_wins_over_env_and_exe() {
        let a = TempDir::new("home-a");
        let b = TempDir::new("home-b");
        let exe_dir = TempDir::new("home-exe");
        let exe = exe_dir.path().join("cmd_ai.exe");
        fs::write(&exe, b"").unwrap();
        let home = Home::resolve_with(
            Some(a.path()),
            Some(b.path().as_os_str().to_owned()),
            || Ok(exe.clone()),
        )
        .unwrap();
        assert_eq!(
            fs::canonicalize(&home.root).unwrap(),
            fs::canonicalize(a.path()).unwrap()
        );
    }

    #[test]
    fn env_wins_over_exe() {
        let b = TempDir::new("home-env");
        let home = Home::resolve_with(None, Some(b.path().as_os_str().to_owned()), no_exe).unwrap();
        assert_eq!(
            fs::canonicalize(&home.root).unwrap(),
            fs::canonicalize(b.path()).unwrap()
        );
    }

    #[test]
    fn empty_env_is_ignored() {
        let exe_dir = TempDir::new("home-empty-env");
        let exe = exe_dir.path().join("cmd_ai");
        fs::write(&exe, b"").unwrap();
        let home = Home::resolve_with(None, Some(OsString::new()), || Ok(exe.clone())).unwrap();
        assert_eq!(
            fs::canonicalize(&home.root).unwrap(),
            fs::canonicalize(exe_dir.path()).unwrap()
        );
    }

    #[test]
    fn falls_back_to_executable_directory_not_cwd() {
        let exe_dir = TempDir::new("home-exe-only");
        let exe = exe_dir.path().join("cmd_ai.exe");
        fs::write(&exe, b"").unwrap();
        let home = Home::resolve_with(None, None, || Ok(exe.clone())).unwrap();
        assert_eq!(
            fs::canonicalize(&home.root).unwrap(),
            fs::canonicalize(exe_dir.path()).unwrap()
        );
        assert_ne!(
            fs::canonicalize(&home.root).unwrap(),
            fs::canonicalize(std::env::current_dir().unwrap()).unwrap()
        );
        assert!(home.writable);
        assert!(!home.ephemeral());
    }

    #[test]
    fn executable_error_is_reported() {
        let err = Home::resolve_with(None, None, no_exe).unwrap_err();
        assert!(matches!(err, HomeError::NoExecutable(_)));
        assert!(crate::text::home_error(&err).contains("--home"));
    }

    #[test]
    fn missing_override_is_not_found() {
        let t = TempDir::new("home-missing");
        let missing = t.path().join("nope");
        let err = Home::resolve_with(Some(&missing), None, no_exe).unwrap_err();
        assert_eq!(err, HomeError::NotFound(missing.clone()));
        let msg = crate::text::home_error(&err);
        assert!(msg.contains("introuvable"), "{msg}");
        assert!(msg.contains("nope"), "{msg}");
    }

    #[test]
    fn missing_env_dir_is_not_found() {
        let t = TempDir::new("home-missing-env");
        let missing = t.path().join("absent");
        let err =
            Home::resolve_with(None, Some(missing.as_os_str().to_owned()), no_exe).unwrap_err();
        assert_eq!(err, HomeError::NotFound(missing));
    }

    #[test]
    fn file_override_is_not_a_directory() {
        let t = TempDir::new("home-file");
        let file = t.path().join("fichier.txt");
        fs::write(&file, b"x").unwrap();
        let err = Home::resolve_with(Some(&file), None, no_exe).unwrap_err();
        assert_eq!(err, HomeError::NotADirectory(file));
        assert!(crate::text::home_error(&err).contains("n'est pas un dossier"));
    }

    #[test]
    fn paths_are_relative_to_root() {
        let home = Home {
            root: PathBuf::from("racine"),
            writable: true,
        };
        assert_eq!(home.config_path(), Path::new("racine").join("config.json"));
        assert_eq!(
            home.example_config_path(),
            Path::new("racine").join("config.example.json")
        );
        assert_eq!(home.log_dir(), Path::new("racine").join("log"));
        assert_eq!(home.memory_dir(), Path::new("racine").join("memory"));
        assert_eq!(home.skills_dir(), Path::new("racine").join("skills"));
    }

    #[test]
    fn probe_leaves_no_file_behind() {
        let t = TempDir::new("home-probe");
        assert!(probe_writable(t.path()));
        assert!(!t.path().join(PROBE_FILE).exists());
        assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
    }

    #[test]
    fn probe_fails_on_missing_dir() {
        let t = TempDir::new("home-probe-missing");
        assert!(!probe_writable(&t.path().join("absent")));
        let home = Home::at(t.path().join("absent"));
        assert!(home.ephemeral());
    }

    #[test]
    fn probe_blocked_means_ephemeral() {
        // A directory squatting the probe name makes the probe impossible on every OS.
        let t = TempDir::new("home-probe-blocked");
        fs::create_dir(t.path().join(PROBE_FILE)).unwrap();
        let home = Home::at(t.path().to_path_buf());
        assert!(!home.writable);
        assert!(home.ephemeral());
        assert!(t.path().join(PROBE_FILE).is_dir());
    }

    #[test]
    fn read_only_probe_file_means_ephemeral() {
        let t = TempDir::new("home-probe-ro");
        let probe = t.path().join(PROBE_FILE);
        fs::write(&probe, b"").unwrap();
        let mut perms = fs::metadata(&probe).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&probe, perms.clone()).unwrap();
        let writable = probe_writable(t.path());
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&probe, perms).unwrap();
        // Root on Unix ignores the read-only bit: only assert when it is honoured.
        if fs::OpenOptions::new().write(true).open(&probe).is_ok() && writable {
            return;
        }
        assert!(!writable);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directory_means_ephemeral() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new("home-ro-dir");
        let dir = t.path().join("ro");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let root_user = fs::write(dir.join("x"), b"").is_ok();
        let home = Home::at(dir.clone());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        if root_user {
            return; // root bypasses permissions
        }
        assert!(home.ephemeral());
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\C:\cle\cmd_ai")),
            PathBuf::from(r"C:\cle\cmd_ai")
        );
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\UNC\srv\partage\cmd_ai")),
            PathBuf::from(r"\\srv\partage\cmd_ai")
        );
        let volume = PathBuf::from(r"\\?\Volume{1234}\cmd_ai");
        assert_eq!(simplify_path(volume.clone()), volume);
        let t = TempDir::new("home-verbatim");
        let home = Home::resolve_with(Some(t.path()), None, no_exe).unwrap();
        assert!(!home.root.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn plain_paths_are_unchanged() {
        let p = PathBuf::from("dossier/sous");
        assert_eq!(simplify_path(p.clone()), p);
    }

    #[test]
    fn decode_text_handles_boms() {
        assert_eq!(decode_text(b"abc").as_deref(), Some("abc"));
        assert_eq!(
            decode_text(&[0xEF, 0xBB, 0xBF, b'h', b'i']).as_deref(),
            Some("hi")
        );
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("é!".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_text(&le).as_deref(), Some("é!"));
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("çà".encode_utf16().flat_map(|u| u.to_be_bytes()))
            .collect();
        assert_eq!(decode_text(&be).as_deref(), Some("çà"));
        assert_eq!(decode_text(&[0xFF, 0xFE, 0x41]), None); // odd length
        assert_eq!(decode_text(&[0xC3, 0x28]), None); // invalid UTF-8
        assert_eq!(decode_text(b"").as_deref(), Some(""));
    }

    #[test]
    fn read_text_file_reports_errors() {
        let t = TempDir::new("home-read");
        assert_eq!(
            read_text_file(&t.path().join("absent.txt"), 10),
            Err(TextFileError::NotFound)
        );
        let big = t.path().join("big.txt");
        fs::write(&big, b"0123456789A").unwrap();
        assert_eq!(read_text_file(&big, 10), Err(TextFileError::TooBig));
        assert_eq!(read_text_file(&big, 11).as_deref(), Ok("0123456789A"));
        let bin = t.path().join("bin.dat");
        fs::write(&bin, [0xC3, 0x28]).unwrap();
        assert_eq!(read_text_file(&bin, 10), Err(TextFileError::NotText));
        assert!(matches!(
            read_text_file(t.path(), 10),
            Err(TextFileError::Io(_))
        ));
    }
}
