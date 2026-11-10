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
    if !path.exists() {
        return "JETCARP's screen saver is not installed.".into();
    }
    let chosen =
        current_saver().is_some_and(|v| v.eq_ignore_ascii_case(&path.display().to_string()));
    if chosen {
        "JETCARP is the selected screen saver.".into()
    } else {
        format!(
            "JETCARP's screen saver is at {}, but another one is selected.",
            path.display()
        )
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

/// Deletes the file and stops Windows from running it.
pub fn remove() -> Result<(), String> {
    if !cfg!(windows) {
        return Err("`.scr` screen savers exist only on Windows".into());
    }
    let path = scr_path()?;
    if current_saver().is_some_and(|v| v.eq_ignore_ascii_case(&path.display().to_string())) {
        let out = Command::new("reg")
            .args(["delete", KEY, "/v", VALUE, "/f"])
            .output()
            .map_err(|e| format!("reg: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
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
}
