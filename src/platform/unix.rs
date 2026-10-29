//! Unix specifics (Linux and macOS): the only place with libc FFI (`unsafe`).

/// Effective user is root.
pub(super) fn is_root() -> bool {
    // SAFETY: geteuid takes no argument, cannot fail and has no side effect.
    unsafe { libc::geteuid() == 0 }
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
