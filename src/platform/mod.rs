//! Host information, privilege detection, output decoding, elevation.
//!
//! All `unsafe` FFI lives in `windows.rs` / `unix.rs`. See specs/001-cmd-ai-v1/contracts/rust-api.md
//! (platform/).
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
use std::path::Path;

/// Ctrl+C sets `flag` (the cancel flag) instead of killing cmd_ai, so that running commands
/// are killed cleanly with their whole tree (constitution IV). A second Ctrl+C while the
/// flag is still set exits with code 130. Returns false if the handler could not be set.
pub fn install_interrupt_handler(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> bool {
    #[cfg(windows)]
    {
        windows::install_interrupt_handler(flag)
    }
    #[cfg(unix)]
    {
        unix::install_interrupt_handler(flag)
    }
}

/// What the model needs to know about the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostInfo {
    /// `"windows"`, `"linux"` or `"macos"` (`std::env::consts::OS`).
    pub os: &'static str,
    /// `std::env::consts::ARCH` (`"x86_64"`, `"aarch64"`…).
    pub arch: &'static str,
    /// Best-effort human readable version, obtained without spawning any process.
    pub os_version: String,
}

/// Privilege level of the current session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Privilege {
    User,
    Admin,
}

/// OS, architecture and a best-effort version string, without spawning any process
/// (Windows: `RtlGetVersion`; Linux: `/etc/os-release`; macOS: `SystemVersion.plist`).
pub fn host_info() -> HostInfo {
    HostInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        os_version: os_version(),
    }
}

#[cfg(windows)]
fn os_version() -> String {
    windows::os_version()
}

#[cfg(unix)]
fn os_version() -> String {
    unix::os_version()
}

#[cfg(not(any(windows, unix)))]
fn os_version() -> String {
    std::env::consts::OS.to_string()
}

/// Privilege level of this process: elevated token on Windows (`TokenElevation`), effective user
/// root on Unix (`geteuid() == 0`). `User` when it cannot be determined.
pub fn current_privilege() -> Privilege {
    #[cfg(windows)]
    let admin = windows::is_elevated();
    #[cfg(unix)]
    let admin = unix::is_root();
    #[cfg(not(any(windows, unix)))]
    let admin = false;
    if admin {
        Privilege::Admin
    } else {
        Privilege::User
    }
}

/// Decodes raw command output: valid UTF-8 as is; otherwise, on Windows, the OEM code page
/// (`GetOEMCP`, what `cmd` and Windows PowerShell 5.1 write to pipes); otherwise lossy UTF-8
/// (invalid bytes replaced by U+FFFD). Never panics.
pub fn decode_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    #[cfg(windows)]
    if let Some(text) = windows::decode_oem(bytes) {
        return text;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Win32 `ERROR_CANCELLED`: the user declined the UAC prompt.
#[cfg(windows)]
const ERROR_CANCELLED: u32 = 1223;

/// Relaunches `exe` elevated through the UAC prompt (`ShellExecuteExW`, verb `runas`), with `args`
/// quoted by [`windows_command_line`]. Returns `Err` with a French message when the user declines
/// or the launch fails. The current process keeps running: the caller decides when to exit.
#[cfg(windows)]
pub fn relaunch_elevated(exe: &Path, args: &[String]) -> Result<(), String> {
    let parameters = windows_command_line(args);
    windows::shell_execute_runas(exe, &parameters).map_err(elevation_error)
}

/// French message for a failed `ShellExecuteExW("runas")`.
#[cfg(windows)]
fn elevation_error(code: u32) -> String {
    if code == ERROR_CANCELLED {
        "Élévation refusée : la demande du contrôle de compte d'utilisateur a été annulée."
            .to_string()
    } else {
        format!("Impossible de relancer cmd_ai en administrateur (erreur Windows {code}).")
    }
}

/// Quotes one argument following the MSVC / `CommandLineToArgvW` rules: unchanged when it is
/// non-empty and has no space, tab, newline, vertical tab or double quote; otherwise wrapped in
/// double quotes, backslashes being doubled only before a quote or the closing quote.
pub fn quote_windows_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty() || arg.contains([' ', '\t', '\n', '\u{0b}', '"']);
    if !needs_quotes {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// Joins arguments into a Windows command line (each one quoted by [`quote_windows_arg`]).
pub fn windows_command_line(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_windows_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PRETTY_NAME` of an os-release file (surrounding quotes removed); `None` when absent or empty.
#[cfg_attr(any(not(unix), target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_os_release(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| {
            let value = value.trim();
            let unquoted = ['"', '\'']
                .iter()
                .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
                .unwrap_or(value);
            unquoted.trim().to_string()
        })
        .filter(|v| !v.is_empty())
}

/// `ProductVersion` of macOS `SystemVersion.plist`, by plain string search (no XML parser).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_system_version_plist(text: &str) -> Option<String> {
    let after_key = &text[text.find("<key>ProductVersion</key>")?..];
    let start = after_key.find("<string>")? + "<string>".len();
    let rest = &after_key[start..];
    let value = rest[..rest.find("</string>")?].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// "Windows 11 (10.0.26200)", "Windows 10 (10.0.19045)", else "Windows major.minor.build".
/// Windows 11 still reports version 10.0: it is told apart by its build number (>= 22000).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_version_label(major: u32, minor: u32, build: u32) -> String {
    match (major, minor) {
        (10, 0) if build >= 22_000 => format!("Windows 11 (10.0.{build})"),
        (10, 0) => format!("Windows 10 (10.0.{build})"),
        _ => format!("Windows {major}.{minor}.{build}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_info_is_filled() {
        let h = host_info();
        assert_eq!(h.os, std::env::consts::OS);
        assert_eq!(h.arch, std::env::consts::ARCH);
        assert!(!h.os.is_empty());
        assert!(!h.arch.is_empty());
        assert!(!h.os_version.trim().is_empty());
        #[cfg(windows)]
        assert!(h.os_version.starts_with("Windows "), "{}", h.os_version);
        #[cfg(target_os = "macos")]
        assert!(h.os_version.starts_with("macOS"), "{}", h.os_version);
    }

    #[test]
    fn privilege_is_stable() {
        assert_eq!(current_privilege(), current_privilege());
    }

    #[test]
    fn decode_utf8_passes_through() {
        assert_eq!(decode_output(b""), "");
        assert_eq!(
            decode_output("bonjour éàçù €\n".as_bytes()),
            "bonjour éàçù €\n"
        );
        assert_eq!(decode_output(b"\r\n\t"), "\r\n\t");
    }

    #[test]
    fn decode_invalid_never_panics() {
        let samples: [&[u8]; 5] = [
            b"\xff\xfe\xfd",
            b"abc\x82\x85",
            b"\xc3",
            b"\xe9t\xe9",
            &[0x80; 64],
        ];
        for s in samples {
            let out = decode_output(s);
            assert!(!out.is_empty(), "{s:?}");
        }
        #[cfg(unix)]
        assert_eq!(decode_output(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn quote_windows_arg_rules() {
        let cases: &[(&str, &str)] = &[
            ("", "\"\""),
            ("abc", "abc"),
            ("--resume", "--resume"),
            ("a b", "\"a b\""),
            ("a\tb", "\"a\tb\""),
            ("a\"b", "\"a\\\"b\""),
            ("C:\\path\\", "C:\\path\\"),
            ("C:\\a b\\", "\"C:\\a b\\\\\""),
            ("a\\\\\"b", "\"a\\\\\\\\\\\"b\""),
            ("a\\b c", "\"a\\b c\""),
            ("éàçù", "éàçù"),
            ("é à", "\"é à\""),
        ];
        for (arg, expected) in cases {
            assert_eq!(quote_windows_arg(arg), *expected, "{arg:?}");
        }
    }

    #[test]
    fn windows_command_line_joins() {
        let args = vec![
            "--resume".to_string(),
            "D:\\cmd ai\\memory\\resume.json".to_string(),
        ];
        assert_eq!(
            windows_command_line(&args),
            "--resume \"D:\\cmd ai\\memory\\resume.json\""
        );
        assert_eq!(windows_command_line(&[]), "");
    }

    #[test]
    fn os_release_parsing() {
        let ubuntu = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n";
        assert_eq!(
            parse_os_release(ubuntu).as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert_eq!(
            parse_os_release("PRETTY_NAME='Alpine Linux v3.20'\n").as_deref(),
            Some("Alpine Linux v3.20")
        );
        assert_eq!(
            parse_os_release("# comment\n  PRETTY_NAME=Arch\n").as_deref(),
            Some("Arch")
        );
        assert_eq!(parse_os_release("NAME=Debian\n"), None);
        assert_eq!(parse_os_release("PRETTY_NAME=\"\"\n"), None);
        assert_eq!(parse_os_release(""), None);
    }

    #[test]
    fn plist_parsing() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>BuildID</key>
	<string>ABC</string>
	<key>ProductName</key>
	<string>macOS</string>
	<key>ProductVersion</key>
	<string>15.1</string>
</dict>
</plist>"#;
        assert_eq!(parse_system_version_plist(plist).as_deref(), Some("15.1"));
        assert_eq!(
            parse_system_version_plist("<key>ProductVersion</key>"),
            None
        );
        assert_eq!(parse_system_version_plist(""), None);
        assert_eq!(
            parse_system_version_plist("<key>ProductVersion</key>\n  <string> 14.6.1 </string>")
                .as_deref(),
            Some("14.6.1")
        );
    }

    #[cfg(windows)]
    #[test]
    fn elevation_errors_are_french_and_distinct() {
        // Never call relaunch_elevated itself in tests: it would open a UAC prompt.
        let declined = elevation_error(ERROR_CANCELLED);
        assert!(declined.starts_with("Élévation refusée"), "{declined}");
        let other = elevation_error(2);
        assert!(
            other.contains("administrateur") && other.contains("erreur Windows 2"),
            "{other}"
        );
        assert_ne!(declined, other);
    }

    #[test]
    fn windows_labels() {
        assert_eq!(
            windows_version_label(10, 0, 26200),
            "Windows 11 (10.0.26200)"
        );
        assert_eq!(
            windows_version_label(10, 0, 22000),
            "Windows 11 (10.0.22000)"
        );
        assert_eq!(
            windows_version_label(10, 0, 19045),
            "Windows 10 (10.0.19045)"
        );
        assert_eq!(windows_version_label(6, 3, 9600), "Windows 6.3.9600");
    }
}
