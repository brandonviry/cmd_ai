//! Unix specifics (Linux and macOS): the only place with libc FFI (`unsafe`).

/// Effective user is root.
pub(super) fn is_root() -> bool {
    // SAFETY: geteuid takes no argument, cannot fail and has no side effect.
    unsafe { libc::geteuid() == 0 }
}

static INTERRUPT: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
    std::sync::OnceLock::new();

extern "C" fn on_sigint(_sig: libc::c_int) {
    if let Some(flag) = INTERRUPT.get() {
        if flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
            // SAFETY: _exit is async-signal-safe; second Ctrl+C while the cancel is pending.
            unsafe { libc::_exit(130) }
        }
    }
}

/// SIGINT sets `flag` instead of terminating cmd_ai (see `platform::install_interrupt_handler`).
pub(super) fn install_interrupt_handler(
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> bool {
    if INTERRUPT.set(flag).is_err() {
        return true;
    }
    let handler: extern "C" fn(libc::c_int) = on_sigint;
    // SAFETY: `handler` is a valid extern "C" fn for the whole program lifetime and only does an
    // atomic swap (and _exit), both async-signal-safe.
    unsafe { libc::signal(libc::SIGINT, handler as libc::sighandler_t) != libc::SIG_ERR }
}

/// Human readable OS version, read from files only (no process).
pub(super) fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let text = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
            .unwrap_or_default();
        match super::parse_system_version_plist(&text) {
            Some(v) => format!("macOS {v}"),
            None => "macOS".to_string(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ["/etc/os-release", "/usr/lib/os-release"]
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .find_map(|text| super::parse_os_release(&text))
            .unwrap_or_else(|| {
                let os = std::env::consts::OS;
                let mut chars = os.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => "Unix".to_string(),
                }
            })
    }
}
