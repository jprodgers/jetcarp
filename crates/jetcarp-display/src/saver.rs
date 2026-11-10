//! The OS's own screen saver list, where JETCARP can be in one: a `.scr` on Windows
//! ([`crate::winsaver`]) and a `.saver` bundle on macOS ([`crate::macsaver`]). GNOME has no such
//! list; its lock screen setting does the job instead.

use std::path::PathBuf;

/// Whether this system has a list to be in.
pub const AVAILABLE: bool = cfg!(any(windows, target_os = "macos"));

/// What the CLI and the app call it here.
pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Windows"
    }
}

/// Puts JETCARP in the list and opens the system's screen saver settings. Returns where it went and
/// any warnings.
pub fn install() -> Result<(PathBuf, Vec<String>), String> {
    if cfg!(target_os = "macos") {
        return crate::macsaver::install();
    }
    let exe = crate::autostart::sibling("jetcarp-host")?;
    crate::autostart::record_host_program(&exe)?;
    crate::winsaver::install(&exe).map(|p| (p, vec![]))
}

pub fn remove() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        crate::macsaver::remove()
    } else {
        crate::winsaver::remove()
    }
}

/// One line for people.
pub fn describe() -> String {
    if cfg!(target_os = "macos") {
        crate::macsaver::describe()
    } else {
        crate::winsaver::describe()
    }
}

/// Settings that are copied into the saver when it is installed, so changing them needs another
/// install (macOS), rather than being read each time it starts (Windows).
pub const BAKED: bool = cfg!(target_os = "macos");
