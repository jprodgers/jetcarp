//! Windows' screen saver list: the host program, copied to a `.scr` file.
//!
//! No administrator rights are needed. The file goes in the user's own folder and Windows' own
//! Install command selects it, which is the same as right-clicking the file and choosing Install.

use std::path::PathBuf;
use std::process::Command;

/// The name Windows shows in the list comes from the file's.
const FILE: &str = "JETCARP.scr";
const KEY: &str = r"HKCU\Control Panel\Desktop";
const VALUE: &str = "SCRNSAVE.EXE";

pub fn scr_path() -> Result<PathBuf, String> {
    Ok(dirs::data_local_dir()
        .ok_or("no local data folder on this system")?
        .join("JETCARP")
        .join(FILE))
}

fn current_saver() -> Option<String> {
    let out = Command::new("reg")
        .args(["query", KEY, "/v", VALUE])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    value_in(&String::from_utf8_lossy(&out.stdout))
}

/// The value in `reg query` output.
fn value_in(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.trim_start().starts_with(VALUE))?
        .split("REG_SZ")
        .nth(1)
        .map(|v| v.trim().to_string())
}

/// One line for people.
pub fn describe() -> String {
    let Ok(path) = scr_path() else {
        return "No local data folder on this system.".into();
    };
    state(&path.display().to_string(), path.exists(), current_saver())
}

/// What [`describe`] says: whether the file is there (`installed`), and the screen saver Windows
/// runs (`selected`, the `SCRNSAVE.EXE` setting; none when it is empty or missing).
fn state(path: &str, installed: bool, selected: Option<String>) -> String {
    let selected = selected.filter(|s| !s.trim().is_empty());
    let ours = selected
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(path));
    match (installed, ours, selected) {
        (true, true, _) => "JETCARP is the selected screen saver.".into(),
        (true, false, None) => format!(
            "JETCARP's screen saver is installed at {path}, but no screen saver is selected."
        ),
        (true, false, Some(other)) => format!(
            "JETCARP's screen saver is installed at {path}, but another one is selected ({other})."
        ),
        (false, true, _) => {
            "JETCARP's screen saver is not installed, but Windows still names it as \
             the screen saver, so none runs; `jetcarp host saver remove` clears that."
                .into()
        }
        (false, false, _) => "JETCARP's screen saver is not installed.".into(),
    }
}

/// What to say when the `.scr` could not be deleted. A screen saver that is running (the Screen
/// Saver Settings dialog's preview runs it) cannot be deleted.
fn not_removed(path: &str, e: &std::io::Error) -> String {
    // 5: access denied; 32: in use by another process.
    if matches!(e.raw_os_error(), Some(5 | 32)) {
        format!(
            "{path} is in use, most likely by the preview in Screen Saver Settings. Close that \
             dialog, then run `jetcarp host saver remove` again. Nothing was changed."
        )
    } else {
        format!("{path}: {e}. Nothing was changed.")
    }
}

/// Copies `host` into place and opens Screen Saver Settings with it selected.
pub fn install(host: &std::path::Path) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("`.scr` screen savers exist only on Windows".into());
    }
    let path = scr_path()?;
    let dir = path.parent().expect("a folder");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::copy(host, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Command::new("rundll32.exe")
        .arg("desk.cpl,InstallScreenSaver")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("rundll32: {e}"))?;
    Ok(path)
}

/// Deletes the file, then stops Windows from running it. The setting is cleared only once the
/// file is gone, so a failure leaves everything as it was (it left the setting cleared and the file
/// there, and the status then said another screen saver was selected).
pub fn remove() -> Result<(), String> {
    if !cfg!(windows) {
        return Err("`.scr` screen savers exist only on Windows".into());
    }
    let path = scr_path()?;
    let shown = path.display().to_string();
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(not_removed(&shown, &e)),
    }
    if current_saver().is_some_and(|v| v.eq_ignore_ascii_case(&shown)) {
        let out = Command::new("reg")
            .args(["delete", KEY, "/v", VALUE, "/f"])
            .output()
            .map_err(|e| format!("reg: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_registry_tools_output() {
        let text = "\r\nHKEY_CURRENT_USER\\Control Panel\\Desktop\r\n    SCRNSAVE.EXE    REG_SZ    C:\\Users\\A B\\AppData\\Local\\JETCARP\\JETCARP.scr\r\n\r\n";
        assert_eq!(
            value_in(text).as_deref(),
            Some("C:\\Users\\A B\\AppData\\Local\\JETCARP\\JETCARP.scr")
        );
        assert_eq!(value_in("nothing here"), None);
    }

    #[test]
    fn the_status_says_what_is_selected() {
        let p = r"C:\Users\a\AppData\Local\JETCARP\JETCARP.scr";
        let ours = Some(p.to_lowercase());
        assert_eq!(
            state(p, true, ours.clone()),
            "JETCARP is the selected screen saver."
        );
        // After a remove that failed with rc1: the setting cleared, the file left.
        assert!(state(p, true, Some(String::new())).ends_with("but no screen saver is selected."));
        assert!(state(p, true, None).ends_with("but no screen saver is selected."));
        let other = state(p, true, Some(r"C:\Windows\system32\Mystify.scr".into()));
        assert!(other.ends_with(r"another one is selected (C:\Windows\system32\Mystify.scr)."));
        assert!(state(p, false, ours).contains("still names it"));
        assert_eq!(
            state(p, false, None),
            "JETCARP's screen saver is not installed."
        );
    }

    #[test]
    fn a_scr_in_use_says_to_close_the_dialog() {
        let p = r"C:\x\JETCARP.scr";
        let denied = std::io::Error::from_raw_os_error(5);
        let m = not_removed(p, &denied);
        assert!(m.contains("Close that dialog"), "{m}");
        assert!(m.ends_with("Nothing was changed."));
        let in_use = std::io::Error::from_raw_os_error(32);
        assert!(not_removed(p, &in_use).contains("Screen Saver Settings"));
    }
}
