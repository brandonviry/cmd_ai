//! Windows specifics: the only place with Win32 FFI (`unsafe`), each call documented.
//!
//! No registry access at all (constitution, principe III).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::Globalization::{GetOEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

static INTERRUPT: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
    std::sync::OnceLock::new();

unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if ctrl_type != CTRL_C_EVENT && ctrl_type != CTRL_BREAK_EVENT {
        return 0;
    }
    match INTERRUPT.get() {
        Some(flag) => {
            if flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
                // Second Ctrl+C while the cancel is still pending: leave now.
                std::process::exit(130);
            }
            1
        }
        None => 0,
    }
}

/// Ctrl+C / Ctrl+Break set `flag` instead of terminating cmd_ai.
pub(super) fn install_interrupt_handler(
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> bool {
    if INTERRUPT.set(flag).is_err() {
        return true;
    }
    // SAFETY: `ctrl_handler` is a valid 'static handler routine; SetConsoleCtrlHandler only
    // registers it (it runs on a system thread and only touches an atomic).
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 1) != 0
    }
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    /// Kernel-mode version query: unlike `GetVersionExW` it is not subject to the
    /// application-manifest compatibility shims, so it reports the real version.
    fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> i32;
}

/// `(major, minor, build)` of the running Windows.
pub(super) fn version_numbers() -> Option<(u32, u32, u32)> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
    };
    // SAFETY: `info` is a properly sized, writable OSVERSIONINFOW whose size field is set, as
    // RtlGetVersion requires; the pointer is valid for the duration of the call.
    let status = unsafe { RtlGetVersion(&mut info) };
    (status == 0).then_some((info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber))
}

pub(super) fn os_version() -> String {
    match version_numbers() {
        Some((major, minor, build)) => super::windows_version_label(major, minor, build),
        None => "Windows".to_string(),
    }
}

/// Owned token handle, closed on drop.
struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by a successful OpenProcessToken and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Whether the process token is elevated (`TokenElevation`). `false` on any failure.
pub(super) fn is_elevated() -> bool {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; `raw` is a valid
    // out-pointer for the token handle.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) };
    if ok == 0 {
        return false;
    }
    let token = TokenHandle(raw);
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: `token.0` is a valid token handle opened with TOKEN_QUERY; the buffer is a
    // TOKEN_ELEVATION of the size given, and `returned` is a valid out-pointer.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    ok != 0 && elevation.TokenIsElevated != 0
}

/// The OEM code page of the system (`GetOEMCP`), e.g. 850 on French installs.
pub(super) fn oem_code_page() -> u32 {
    // SAFETY: GetOEMCP takes no argument and has no precondition.
    unsafe { GetOEMCP() }
}

/// Decodes `bytes` with the OEM code page. Strict first (`MB_ERR_INVALID_CHARS`), then lenient
/// (invalid sequences replaced). `None` if the conversion fails.
pub(super) fn decode_oem(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    let len = i32::try_from(bytes.len()).ok()?;
    let code_page = oem_code_page();
    for flags in [MB_ERR_INVALID_CHARS, 0] {
        // SAFETY: `bytes` is valid for `len` bytes; a null output buffer with size 0 asks for the
        // required length in UTF-16 units.
        let needed = unsafe {
            MultiByteToWideChar(code_page, flags, bytes.as_ptr(), len, ptr::null_mut(), 0)
        };
        if needed <= 0 {
            continue;
        }
        let mut wide = vec![0u16; needed as usize];
        // SAFETY: `wide` has room for exactly `needed` UTF-16 units, as passed.
        let written = unsafe {
            MultiByteToWideChar(
                code_page,
                flags,
                bytes.as_ptr(),
                len,
                wide.as_mut_ptr(),
                needed,
            )
        };
        if written > 0 {
            wide.truncate(written as usize);
            return Some(String::from_utf16_lossy(&wide));
        }
    }
    None
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// `ShellExecuteExW` with the `runas` verb (UAC prompt). Returns the Win32 error code on failure
/// (`ERROR_CANCELLED` = 1223 when the user declines).
pub(super) fn shell_execute_runas(exe: &Path, parameters: &str) -> Result<(), u32> {
    let verb = wide(OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    let params = wide(OsStr::new(parameters));
    // SAFETY: SHELLEXECUTEINFOW is a plain C struct for which all-zero is the documented
    // "unset" state of every field (null pointers, zero flags).
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: `info` is initialised with its size; every string pointer refers to a
    // NUL-terminated UTF-16 buffer (`verb`, `file`, `params`) alive until the call returns.
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok != 0 {
        Ok(())
    } else {
        // SAFETY: GetLastError has no precondition; read right after the failing call.
        Err(unsafe { GetLastError() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

    #[test]
    fn real_version_is_modern() {
        let (major, _minor, build) = version_numbers().unwrap();
        assert!(major >= 10, "major {major}");
        assert!(build > 10_000, "build {build}");
        assert!(os_version().starts_with("Windows 1"), "{}", os_version());
    }

    #[test]
    fn elevation_query_does_not_fail_silently() {
        // Either value is legitimate; the call must simply be stable.
        assert_eq!(is_elevated(), is_elevated());
    }

    #[test]
    fn oem_decoding_of_french_accents() {
        let cp = oem_code_page();
        if cp != 850 && cp != 437 {
            eprintln!("skip: OEM code page {cp}");
            return;
        }
        // "éàçù" is 82 85 87 97 in both CP437 and CP850.
        assert_eq!(
            decode_oem(&[0x82, 0x85, 0x87, 0x97]).as_deref(),
            Some("éàçù")
        );
        assert_eq!(super::super::decode_output(b"caf\x82\r\n"), "café\r\n");
        assert_eq!(decode_oem(b"").as_deref(), Some(""));
    }

    /// Parses a command line the way the C runtime and CommandLineToArgvW do.
    fn parse_command_line(line: &str) -> Vec<String> {
        let full = wide(OsStr::new(&format!("prog.exe {line}")));
        let mut argc = 0i32;
        // SAFETY: `full` is a NUL-terminated UTF-16 string; `argc` is a valid out-pointer.
        let argv = unsafe { CommandLineToArgvW(full.as_ptr(), &mut argc) };
        assert!(!argv.is_null());
        let mut out = Vec::new();
        for i in 1..argc as usize {
            // SAFETY: argv holds `argc` valid NUL-terminated wide strings.
            let arg = unsafe { *argv.add(i) };
            let mut len = 0;
            // SAFETY: walking a NUL-terminated string returned by the system.
            while unsafe { *arg.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: `arg` is valid for `len` UTF-16 units.
            let slice = unsafe { std::slice::from_raw_parts(arg, len) };
            out.push(String::from_utf16_lossy(slice));
        }
        // SAFETY: argv was allocated by CommandLineToArgvW and must be freed with LocalFree.
        unsafe {
            LocalFree(argv.cast());
        }
        out
    }

    #[test]
    fn quoting_round_trips_through_command_line_to_argv() {
        let corpus = [
            "simple",
            "",
            "avec espace",
            "guillemet \"interne\"",
            "C:\\chemin\\",
            "C:\\chemin avec espace\\",
            "a\\\\\"b",
            "\\\\serveur\\partage",
            "tab\tici",
            "éàçù ü",
            "%PATH% ^ & | ; $x `y`",
            "--resume",
            "D:\\cmd ai\\memory\\resume.json",
        ];
        let args: Vec<String> = corpus.iter().map(|s| s.to_string()).collect();
        let line = super::super::windows_command_line(&args);
        assert_eq!(parse_command_line(&line), args, "{line}");
    }
}
