//! The OS's own screen saver list, where JETCARP can be in one: a `.scr` on Windows
//! ([`crate::winsaver`]) and a `.saver` bundle on macOS ([`crate::macsaver`]). GNOME has no such
//! list; its lock screen setting does the job instead.

#[cfg(feature = "library")]
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
#[cfg(feature = "library")]
pub fn install() -> Result<(PathBuf, Vec<String>), String> {
    if cfg!(target_os = "macos") {
        return crate::macsaver::install();
    }
    if !cfg!(windows) {
        return Err("`.scr` screen savers exist only on Windows".into());
    }
    // The `.scr` is a copy of the windowless launcher; it starts `jetcarp host scr` through the
    // recorded host command.
    let launcher = crate::autostart::windows_launcher().ok_or(
        "jetcarpw.exe, JETCARP's windowless launcher, was not found beside jetcarp.exe. Install \
         JETCARP from a release, or build it with `cargo build --release -p jetcarp --features \
         windows-launcher`.",
    )?;
    crate::autostart::record_host_program(&crate::autostart::host_command()?)?;
    crate::winsaver::install(&launcher).map(|p| (p, vec![]))
}

/// Takes JETCARP out of the list (and on macOS puts back the screen saver chosen before it). A
/// note for people, if there is something to say.
pub fn remove() -> Result<Option<String>, String> {
    if cfg!(target_os = "macos") {
        crate::macsaver::remove()
    } else {
        crate::winsaver::remove().map(|()| None)
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
