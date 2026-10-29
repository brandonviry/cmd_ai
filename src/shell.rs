//! Shell detection (without spawning anything), per-shell argument building and the minimal-trace
//! child environment (constitution, principes III et IV ; FR-031, FR-032).
//!
//! See specs/001-cmd-ai-v1/contracts/rust-api.md (shell).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Prefix added to every PowerShell command so that output is UTF-8 encoded.
pub const POWERSHELL_PRELUDE: &str = "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;$OutputEncoding=[System.Text.Encoding]::UTF8;";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shell {
    Pwsh,
    PowerShell,
    Cmd,
    Bash,
    Sh,
    Zsh,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellInfo {
    pub shell: Shell,
    pub path: PathBuf,
}

/// Arguments to pass to the shell program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellArgs {
    pub args: Vec<String>,
    /// cmd.exe only: appended verbatim to the command line (`CommandExt::raw_arg`).
    pub raw_tail: Option<String>,
}

impl Shell {
    /// Identifier used in the tool schema and the system prompt.
    pub fn name(&self) -> &'static str {
        match self {
            Shell::Pwsh => "pwsh",
            Shell::PowerShell => "powershell",
            Shell::Cmd => "cmd",
            Shell::Bash => "bash",
            Shell::Sh => "sh",
            Shell::Zsh => "zsh",
        }
    }

    /// Parses a shell name (case-insensitive, surrounding spaces and a `.exe` suffix accepted).
    pub fn parse(s: &str) -> Option<Shell> {
        let lower = s.trim().to_ascii_lowercase();
        let name = lower.strip_suffix(".exe").unwrap_or(&lower);
        match name {
            "pwsh" => Some(Shell::Pwsh),
            "powershell" => Some(Shell::PowerShell),
            "cmd" => Some(Shell::Cmd),
            "bash" => Some(Shell::Bash),
            "sh" => Some(Shell::Sh),
            "zsh" => Some(Shell::Zsh),
            _ => None,
        }
    }

    /// `pwsh` or Windows PowerShell.
    pub fn is_powershell(&self) -> bool {
        matches!(self, Shell::Pwsh | Shell::PowerShell)
    }
}

/// Default shell (FR-031), found without launching any process. Windows: `pwsh` 7, else Windows
/// PowerShell 5.1, else `cmd` as a last resort. Unix: `bash`, else `sh` (`zsh` only on request).
pub fn detect_default() -> Option<ShellInfo> {
    detect_with(cfg!(windows), &|k| std::env::var_os(k), &|p| p.is_file())
}

/// Locates a given shell on this host (PATH and well-known locations), without spawning.
pub fn find(shell: Shell) -> Option<ShellInfo> {
    find_with(shell, cfg!(windows), &|k| std::env::var_os(k), &|p| {
        p.is_file()
    })
}

/// Arguments for running `command` with `shell`: the command always travels as ONE argument.
/// - `pwsh`/`powershell`: `-NoProfile -NonInteractive -NoLogo -Command <prelude><command>`
///   ([`POWERSHELL_PRELUDE`] asks for UTF-8 output);
/// - `cmd`: `/d /s /c` and `raw_tail = "<command>"` (passed verbatim with `raw_arg`, since cmd.exe
///   does not follow the MSVC quoting rules);
/// - `bash`/`sh`: `-c <command>`; `zsh`: `-f -c <command>` (`-f`: no startup files).
pub fn build_args(shell: Shell, command: &str) -> ShellArgs {
    match shell {
        Shell::Pwsh | Shell::PowerShell => ShellArgs {
            args: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-NoLogo".to_string(),
                "-Command".to_string(),
                format!("{POWERSHELL_PRELUDE}{command}"),
            ],
            raw_tail: None,
        },
        Shell::Cmd => ShellArgs {
            args: vec!["/d".to_string(), "/s".to_string(), "/c".to_string()],
            raw_tail: Some(format!("\"{command}\"")),
        },
        Shell::Bash | Shell::Sh => ShellArgs {
            args: vec!["-c".to_string(), command.to_string()],
            raw_tail: None,
        },
        Shell::Zsh => ShellArgs {
            args: vec!["-f".to_string(), "-c".to_string(), command.to_string()],
            raw_tail: None,
        },
    }
}

/// Inherited variables never passed to a child (compared case-insensitively).
const REMOVED_VARS: &[&str] = &["OLLAMA_API_KEY", "BASH_ENV", "ENV", "HISTFILE"];
/// Prefix of cmd_ai's own variables, never inherited (hooks get theirs through `extra_env`).
const REMOVED_PREFIX: &str = "CMD_AI_";

fn added_vars() -> [(&'static str, &'static str); 7] {
    let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
    [
        ("PSModuleAnalysisCachePath", null_device),
        ("POWERSHELL_TELEMETRY_OPTOUT", "1"),
        ("POWERSHELL_UPDATECHECK", "Off"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("HISTFILE", ""),
        ("NO_COLOR", "1"),
    ]
}

/// Minimal-trace environment for every child (FR-032), built from `base` (normally
/// `std::env::vars_os()`): removes `OLLAMA_API_KEY`, `CMD_AI_*`, `BASH_ENV`, `ENV`, `HISTFILE`, then
/// sets `PSModuleAnalysisCachePath` (null device), `POWERSHELL_TELEMETRY_OPTOUT=1`,
/// `POWERSHELL_UPDATECHECK=Off`, `PAGER=cat`, `GIT_PAGER=cat`, `HISTFILE=` and `NO_COLOR=1`
/// (replacing inherited values). Names are compared case-insensitively on every OS.
pub fn child_env(base: impl Iterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let added = added_vars();
    let mut out: Vec<(OsString, OsString)> = base
        .filter(|(k, _)| {
            let upper = k.to_string_lossy().to_ascii_uppercase();
            !(REMOVED_VARS.contains(&upper.as_str())
                || upper.starts_with(REMOVED_PREFIX)
                || added
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&upper)))
        })
        .collect();
    out.extend(
        added
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    out
}

type VarFn<'a> = &'a dyn Fn(&str) -> Option<OsString>;
type IsFileFn<'a> = &'a dyn Fn(&Path) -> bool;

fn detect_with(windows: bool, var: VarFn<'_>, is_file: IsFileFn<'_>) -> Option<ShellInfo> {
    let order: &[Shell] = if windows {
        &[Shell::Pwsh, Shell::PowerShell, Shell::Cmd]
    } else {
        &[Shell::Bash, Shell::Sh]
    };
    order
        .iter()
        .find_map(|&shell| find_with(shell, windows, var, is_file))
}

fn find_with(
    shell: Shell,
    windows: bool,
    var: VarFn<'_>,
    is_file: IsFileFn<'_>,
) -> Option<ShellInfo> {
    let path_dirs: Vec<PathBuf> = var("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    let in_path = |file: &str| -> Vec<PathBuf> { path_dirs.iter().map(|d| d.join(file)).collect() };
    let under = |name: &str, parts: &[&str]| -> Option<PathBuf> {
        var(name).map(|root| {
            parts
                .iter()
                .fold(PathBuf::from(root), |p, part| p.join(part))
        })
    };

    let candidates: Vec<PathBuf> = if windows {
        match shell {
            Shell::Pwsh => {
                let mut c = in_path("pwsh.exe");
                c.extend(under("ProgramFiles", &["PowerShell", "7", "pwsh.exe"]));
                c.extend(under("ProgramFiles(x86)", &["PowerShell", "7", "pwsh.exe"]));
                c
            }
            Shell::PowerShell => {
                let mut c: Vec<PathBuf> = under(
                    "SystemRoot",
                    &["System32", "WindowsPowerShell", "v1.0", "powershell.exe"],
                )
                .into_iter()
                .collect();
                c.extend(in_path("powershell.exe"));
                c
            }
            Shell::Cmd => {
                let mut c: Vec<PathBuf> = var("ComSpec")
                    .map(PathBuf::from)
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.eq_ignore_ascii_case("cmd.exe"))
                    })
                    .into_iter()
                    .collect();
                c.extend(under("SystemRoot", &["System32", "cmd.exe"]));
                c.extend(in_path("cmd.exe"));
                c
            }
            Shell::Bash | Shell::Sh | Shell::Zsh => {
                // %SystemRoot%\System32\bash.exe is the WSL launcher, not a Windows shell.
                let system32 = under("SystemRoot", &["System32"]);
                let file = format!("{}.exe", shell.name());
                in_path(&file)
                    .into_iter()
                    .filter(|p| {
                        let parent = p.parent().map(|d| d.to_string_lossy().to_lowercase());
                        let sys = system32
                            .as_ref()
                            .map(|d| d.to_string_lossy().to_lowercase());
                        sys.is_none() || parent != sys
                    })
                    .collect()
            }
        }
    } else {
        let fixed: &[&str] = match shell {
            Shell::Bash => &["/bin/bash", "/usr/bin/bash"],
            Shell::Sh => &["/bin/sh"],
            Shell::Zsh => &["/bin/zsh", "/usr/bin/zsh"],
            Shell::Pwsh => &["/usr/local/bin/pwsh", "/opt/microsoft/powershell/7/pwsh"],
            Shell::PowerShell | Shell::Cmd => return None,
        };
        let mut c: Vec<PathBuf> = fixed.iter().map(PathBuf::from).collect();
        c.extend(in_path(shell.name()));
        if shell == Shell::Pwsh {
            // Prefer PATH for pwsh (user installs), then the fixed locations.
            c.rotate_left(fixed.len());
        }
        c
    };
    candidates
        .into_iter()
        .find(|p| is_file(p))
        .map(|path| ShellInfo { shell, path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [Shell; 6] = [
        Shell::Pwsh,
        Shell::PowerShell,
        Shell::Cmd,
        Shell::Bash,
        Shell::Sh,
        Shell::Zsh,
    ];

    #[test]
    fn names_round_trip() {
        let names = ["pwsh", "powershell", "cmd", "bash", "sh", "zsh"];
        for (shell, name) in ALL.iter().zip(names) {
            assert_eq!(shell.name(), name);
            assert_eq!(Shell::parse(name), Some(*shell));
        }
    }

    #[test]
    fn parse_is_lenient_but_strict_on_unknown() {
        assert_eq!(Shell::parse(" PWSH "), Some(Shell::Pwsh));
        assert_eq!(Shell::parse("PowerShell.exe"), Some(Shell::PowerShell));
        assert_eq!(Shell::parse("cmd.exe"), Some(Shell::Cmd));
        assert_eq!(Shell::parse("Bash"), Some(Shell::Bash));
        assert_eq!(Shell::parse("fish"), None);
        assert_eq!(Shell::parse(""), None);
        assert_eq!(Shell::parse("bash; rm -rf /"), None);
    }

    fn base() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\fake")
        } else {
            PathBuf::from("/fake")
        }
    }

    struct FakeHost {
        vars: Vec<(String, OsString)>,
        files: HashSet<PathBuf>,
    }

    impl FakeHost {
        fn new() -> Self {
            FakeHost {
                vars: Vec::new(),
                files: HashSet::new(),
            }
        }
        fn var(mut self, k: &str, v: impl Into<OsString>) -> Self {
            self.vars.push((k.to_string(), v.into()));
            self
        }
        fn file(mut self, p: PathBuf) -> Self {
            self.files.insert(p);
            self
        }
        fn path(self, dirs: &[PathBuf]) -> Self {
            let joined = std::env::join_paths(dirs).unwrap();
            self.var("PATH", joined)
        }
        fn find(&self, shell: Shell, windows: bool) -> Option<ShellInfo> {
            let var = |k: &str| {
                self.vars
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(k))
                    .map(|(_, v)| v.clone())
            };
            let is_file = |p: &Path| self.files.contains(p);
            find_with(shell, windows, &var, &is_file)
        }
        fn detect(&self, windows: bool) -> Option<ShellInfo> {
            let var = |k: &str| {
                self.vars
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(k))
                    .map(|(_, v)| v.clone())
            };
            let is_file = |p: &Path| self.files.contains(p);
            detect_with(windows, &var, &is_file)
        }
    }

    #[test]
    fn windows_detection_order() {
        let b = base();
        let pf = b.join("Program Files");
        let sysroot = b.join("Windows");
        let ps51 = sysroot
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let pwsh_pf = pf.join("PowerShell").join("7").join("pwsh.exe");
        let comspec = sysroot.join("System32").join("cmd.exe");
        let bin = b.join("bin");

        let host = FakeHost::new()
            .var("ProgramFiles", pf.clone())
            .var("SystemRoot", sysroot.clone())
            .var("ComSpec", comspec.clone())
            .path(std::slice::from_ref(&bin))
            .file(ps51.clone())
            .file(pwsh_pf.clone())
            .file(comspec.clone());
        // pwsh 7 wins when present.
        assert_eq!(
            host.detect(true),
            Some(ShellInfo {
                shell: Shell::Pwsh,
                path: pwsh_pf.clone()
            })
        );

        // pwsh on PATH is preferred over the Program Files location.
        let host2 = FakeHost::new()
            .var("ProgramFiles", pf.clone())
            .path(std::slice::from_ref(&bin))
            .file(bin.join("pwsh.exe"))
            .file(pwsh_pf.clone());
        assert_eq!(
            host2.find(Shell::Pwsh, true).map(|i| i.path),
            Some(bin.join("pwsh.exe"))
        );

        // 32-bit pwsh 7 under Program Files (x86), not on PATH.
        let pf86 = b.join("Program Files (x86)");
        let pwsh_x86 = pf86.join("PowerShell").join("7").join("pwsh.exe");
        let host_x86 = FakeHost::new()
            .var("ProgramFiles", pf.clone())
            .var("ProgramFiles(x86)", pf86.clone())
            .var("SystemRoot", sysroot.clone())
            .file(ps51.clone())
            .file(pwsh_x86.clone());
        assert_eq!(
            host_x86.detect(true),
            Some(ShellInfo {
                shell: Shell::Pwsh,
                path: pwsh_x86
            })
        );

        // Without pwsh: Windows PowerShell 5.1.
        let host3 = FakeHost::new()
            .var("ProgramFiles", pf.clone())
            .var("SystemRoot", sysroot.clone())
            .var("ComSpec", comspec.clone())
            .file(ps51.clone())
            .file(comspec.clone());
        assert_eq!(
            host3.detect(true),
            Some(ShellInfo {
                shell: Shell::PowerShell,
                path: ps51.clone()
            })
        );

        // Neither PowerShell: cmd as a last resort.
        let host4 = FakeHost::new()
            .var("ComSpec", comspec.clone())
            .file(comspec.clone());
        assert_eq!(
            host4.detect(true),
            Some(ShellInfo {
                shell: Shell::Cmd,
                path: comspec.clone()
            })
        );

        // Nothing at all.
        assert_eq!(FakeHost::new().detect(true), None);
    }

    #[test]
    fn windows_find_cmd_and_powershell() {
        let b = base();
        let sysroot = b.join("Windows");
        let cmd = sysroot.join("System32").join("cmd.exe");
        // ComSpec pointing to another program is ignored; SystemRoot location is used.
        let host = FakeHost::new()
            .var("ComSpec", b.join("tcc.exe"))
            .var("SystemRoot", sysroot.clone())
            .file(b.join("tcc.exe"))
            .file(cmd.clone());
        assert_eq!(
            host.find(Shell::Cmd, true).map(|i| i.path),
            Some(cmd.clone())
        );
        // powershell.exe found on PATH when the system location is missing.
        let bin = b.join("tools");
        let host = FakeHost::new()
            .path(std::slice::from_ref(&bin))
            .file(bin.join("powershell.exe"));
        assert_eq!(
            host.find(Shell::PowerShell, true).map(|i| i.path),
            Some(bin.join("powershell.exe"))
        );
    }

    #[test]
    fn windows_bash_skips_wsl_launcher() {
        let b = base();
        let sysroot = b.join("Windows");
        let sys32 = sysroot.join("System32");
        let git = b.join("Git").join("bin");
        let host = FakeHost::new()
            .var("SystemRoot", sysroot.clone())
            .path(&[sys32.clone(), git.clone()])
            .file(sys32.join("bash.exe"))
            .file(git.join("bash.exe"));
        assert_eq!(
            host.find(Shell::Bash, true).map(|i| i.path),
            Some(git.join("bash.exe"))
        );
    }

    #[test]
    fn relative_path_entries_are_ignored() {
        let host = FakeHost::new()
            .path(&[PathBuf::from("."), PathBuf::from("relative")])
            .file(PathBuf::from(".").join("pwsh.exe"))
            .file(PathBuf::from("relative").join("bash"));
        assert_eq!(host.find(Shell::Pwsh, true), None);
        assert_eq!(host.find(Shell::Bash, false), None);
    }

    #[test]
    fn unix_detection_order() {
        let host = FakeHost::new()
            .file(PathBuf::from("/bin/bash"))
            .file(PathBuf::from("/usr/bin/bash"))
            .file(PathBuf::from("/bin/sh"));
        if cfg!(unix) {
            assert_eq!(
                host.detect(false),
                Some(ShellInfo {
                    shell: Shell::Bash,
                    path: PathBuf::from("/bin/bash")
                })
            );
            let host = FakeHost::new()
                .file(PathBuf::from("/usr/bin/bash"))
                .file(PathBuf::from("/bin/sh"));
            assert_eq!(
                host.find(Shell::Bash, false).map(|i| i.path),
                Some(PathBuf::from("/usr/bin/bash"))
            );
            let host = FakeHost::new().file(PathBuf::from("/bin/sh"));
            assert_eq!(
                host.detect(false),
                Some(ShellInfo {
                    shell: Shell::Sh,
                    path: PathBuf::from("/bin/sh")
                })
            );
        }
        // bash found through PATH (e.g. NixOS).
        let dir = base().join("nix").join("bin");
        let host = FakeHost::new()
            .path(std::slice::from_ref(&dir))
            .file(dir.join("bash"));
        assert_eq!(
            host.detect(false),
            Some(ShellInfo {
                shell: Shell::Bash,
                path: dir.join("bash")
            })
        );
        // zsh is never the default, only on request.
        let host = FakeHost::new()
            .path(std::slice::from_ref(&dir))
            .file(dir.join("zsh"));
        assert_eq!(host.detect(false), None);
        assert_eq!(
            host.find(Shell::Zsh, false).map(|i| i.path),
            Some(dir.join("zsh"))
        );
        // Windows-only shells do not exist on Unix.
        assert_eq!(host.find(Shell::Cmd, false), None);
        assert_eq!(host.find(Shell::PowerShell, false), None);
        assert_eq!(FakeHost::new().detect(false), None);
    }

    #[test]
    fn real_detection_does_not_panic_and_returns_existing_file() {
        if let Some(info) = detect_default() {
            assert!(info.path.is_file(), "{info:?}");
            if cfg!(windows) {
                assert!(matches!(
                    info.shell,
                    Shell::Pwsh | Shell::PowerShell | Shell::Cmd
                ));
            } else {
                assert!(matches!(info.shell, Shell::Bash | Shell::Sh));
            }
        }
        for shell in ALL {
            if let Some(info) = find(shell) {
                assert_eq!(info.shell, shell);
                assert!(info.path.is_file(), "{info:?}");
            }
        }
        #[cfg(windows)]
        assert!(
            find(Shell::Cmd).is_some(),
            "cmd.exe should always exist on Windows"
        );
    }

    #[test]
    fn powershell_args() {
        let corpus = [
            "Get-Date",
            "echo 'a b' \"c d\"",
            "$x = 1; Write-Output \"$x %PATH% ^ & | ;\"",
            "Write-Output 'éàçù'",
            "",
        ];
        for shell in [Shell::Pwsh, Shell::PowerShell] {
            for command in corpus {
                let a = build_args(shell, command);
                assert_eq!(
                    a.args,
                    vec![
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-NoLogo".to_string(),
                        "-Command".to_string(),
                        format!("{POWERSHELL_PRELUDE}{command}"),
                    ]
                );
                assert_eq!(a.raw_tail, None);
            }
        }
        assert!(
            POWERSHELL_PRELUDE
                .starts_with("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;")
        );
        assert!(POWERSHELL_PRELUDE.ends_with("$OutputEncoding=[System.Text.Encoding]::UTF8;"));
    }

    #[test]
    fn cmd_args_use_raw_tail() {
        let cases = [
            ("echo bonjour", "\"echo bonjour\""),
            ("dir \"C:\\Program Files\"", "\"dir \"C:\\Program Files\"\""),
            (
                "echo %PATH% ^& | findstr a",
                "\"echo %PATH% ^& | findstr a\"",
            ),
            ("echo éàçù", "\"echo éàçù\""),
            (
                "\"C:\\a b\\x.exe\" \"arg 1\"",
                "\"\"C:\\a b\\x.exe\" \"arg 1\"\"",
            ),
        ];
        for (command, tail) in cases {
            let a = build_args(Shell::Cmd, command);
            assert_eq!(
                a.args,
                vec!["/d".to_string(), "/s".to_string(), "/c".to_string()]
            );
            assert_eq!(a.raw_tail.as_deref(), Some(tail), "{command}");
        }
    }

    #[test]
    fn posix_args() {
        let corpus = [
            "ls -la",
            "echo \"$HOME\" 'a b' | wc -c; true && false || :",
            "printf 'éàçù\\n'",
        ];
        for command in corpus {
            for shell in [Shell::Bash, Shell::Sh] {
                let a = build_args(shell, command);
                assert_eq!(a.args, vec!["-c".to_string(), command.to_string()]);
                assert_eq!(a.raw_tail, None);
            }
            // zsh: -f skips the startup files (no profile, constitution III).
            let a = build_args(Shell::Zsh, command);
            assert_eq!(
                a.args,
                vec!["-f".to_string(), "-c".to_string(), command.to_string()]
            );
        }
    }

    fn env_of(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn lookup<'a>(env: &'a [(OsString, OsString)], key: &str) -> Vec<&'a OsString> {
        env.iter()
            .filter(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
            .collect()
    }

    #[test]
    fn child_env_scrubs_secrets_and_history() {
        let base = env_of(&[
            ("PATH", "/usr/bin"),
            ("HOME", "/home/moi"),
            ("SystemRoot", "C:\\Windows"),
            ("OLLAMA_API_KEY", "sk-canary-123456"),
            ("ollama_api_key", "sk-canary-lower"),
            ("CMD_AI_HOME", "/media/usb/cmd_ai"),
            ("CMD_AI_SECRET", "x"),
            ("cmd_ai_lower", "x"),
            ("BASH_ENV", "/home/moi/.evil"),
            ("ENV", "/home/moi/.shrc"),
            ("HISTFILE", "/home/moi/.bash_history"),
            ("PAGER", "less"),
            ("GIT_PAGER", "delta"),
            ("NO_COLOR", "0"),
            ("ENVIRONMENT", "kept"),
            ("MY_CMD_AI_VAR", "kept"),
            ("OLLAMA_HOST", "kept"),
        ]);
        let env = child_env(base.into_iter());
        for gone in [
            "OLLAMA_API_KEY",
            "CMD_AI_HOME",
            "CMD_AI_SECRET",
            "cmd_ai_lower",
            "BASH_ENV",
            "ENV",
        ] {
            assert!(lookup(&env, gone).is_empty(), "{gone} should be removed");
        }
        assert!(
            !env.iter()
                .any(|(_, v)| v.to_string_lossy().contains("sk-canary"))
        );
        assert_eq!(lookup(&env, "PATH"), vec![&OsString::from("/usr/bin")]);
        assert_eq!(lookup(&env, "HOME"), vec![&OsString::from("/home/moi")]);
        assert_eq!(
            lookup(&env, "SystemRoot"),
            vec![&OsString::from("C:\\Windows")]
        );
        assert_eq!(lookup(&env, "ENVIRONMENT"), vec![&OsString::from("kept")]);
        assert_eq!(lookup(&env, "MY_CMD_AI_VAR"), vec![&OsString::from("kept")]);
        assert_eq!(lookup(&env, "OLLAMA_HOST"), vec![&OsString::from("kept")]);
        // Added (and replacing inherited values, without duplicates).
        let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let expected = [
            ("PSModuleAnalysisCachePath", null_device),
            ("POWERSHELL_TELEMETRY_OPTOUT", "1"),
            ("POWERSHELL_UPDATECHECK", "Off"),
            ("PAGER", "cat"),
            ("GIT_PAGER", "cat"),
            ("HISTFILE", ""),
            ("NO_COLOR", "1"),
        ];
        for (k, v) in expected {
            assert_eq!(lookup(&env, k), vec![&OsString::from(v)], "{k}");
        }
    }

    #[test]
    fn child_env_from_empty_base_only_has_additions() {
        let env = child_env(std::iter::empty());
        assert_eq!(env.len(), 7);
    }

    #[test]
    fn child_env_of_real_environment_has_no_api_key() {
        let env = child_env(std::env::vars_os());
        assert!(lookup(&env, "OLLAMA_API_KEY").is_empty());
        assert!(!env.iter().any(|(k, _)| {
            k.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("CMD_AI_")
        }));
    }
}
