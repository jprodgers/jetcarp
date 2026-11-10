//! Installing JETCARP's GNOME Shell extension, which turns the host's windows into the desktop and
//! lock-screen background and adds a top-bar menu. The files are built into the program.

use std::path::PathBuf;
use std::process::Command;

pub const UUID: &str = "jetcarp@jimmieprodgers.com";

const FILES: [(&str, &str); 3] = [
    (
        "metadata.json",
        include_str!("../gnome/jetcarp@jimmieprodgers.com/metadata.json"),
    ),
    (
        "extension.js",
        include_str!("../gnome/jetcarp@jimmieprodgers.com/extension.js"),
    ),
    (
        "icons/jetcarp-symbolic.svg",
        include_str!("../gnome/jetcarp@jimmieprodgers.com/icons/jetcarp-symbolic.svg"),
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionState {
    /// This session isn't GNOME, or GNOME Shell can't be reached.
    NoGnome,
    NotInstalled,
    /// The files are in place but GNOME Shell hasn't loaded them: it reads extensions at login.
    NeedsLogin,
    /// GNOME Shell knows it but it isn't running, with GNOME's word for why.
    Inactive(String),
    Active,
}

impl ExtensionState {
    /// One line for people.
    pub fn describe(&self) -> String {
        match self {
            ExtensionState::NoGnome => "GNOME Shell was not found in this session.".into(),
            ExtensionState::NotInstalled => "The GNOME extension is not installed.".into(),
            ExtensionState::NeedsLogin => {
                "The GNOME extension is installed. Log out and back in once to load it.".into()
            }
            ExtensionState::Inactive(why) => format!("The GNOME extension is not running: {why}."),
            ExtensionState::Active => "The GNOME extension is running.".into(),
        }
    }
}

fn dir() -> Result<PathBuf, String> {
    Ok(dirs::data_dir()
        .ok_or("no data folder on this system")?
        .join("gnome-shell/extensions")
        .join(UUID))
}

/// GNOME's numeric extension state in a `GetExtensionInfo` reply, printed by `gdbus`. An unknown
/// extension gives an empty dictionary.
fn parse_state(reply: &str) -> Option<f64> {
    let rest = reply.split("'state': <").nth(1)?;
    rest.split('>').next()?.trim().parse().ok()
}

/// GNOME 50's meaning for the state numbers (`ExtensionState` in `extensionUtils.js`).
fn state_word(n: f64) -> &'static str {
    match n as i32 {
        2 => "it is switched off",
        3 => "it failed with an error; see the journal",
        4 => "this version of GNOME is not supported",
        5 | 6 => "it is starting",
        7 => "it is stopping",
        8 => "it is starting",
        _ => "its state is unknown",
    }
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_ascii_uppercase().contains("GNOME"))
}

/// Whether the extension is installed and running in this session.
pub fn state() -> ExtensionState {
    let files = dir().is_ok_and(|d| d.join("metadata.json").exists());
    let out = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell",
            "--method",
            "org.gnome.Shell.Extensions.GetExtensionInfo",
            UUID,
        ])
        .output();
    let Ok(out) = out.ok().filter(|o| o.status.success()).ok_or(()) else {
        return ExtensionState::NoGnome;
    };
    match parse_state(&String::from_utf8_lossy(&out.stdout)) {
        Some(1.0) => ExtensionState::Active,
        Some(n) => ExtensionState::Inactive(state_word(n).into()),
        None if files => ExtensionState::NeedsLogin,
        None => ExtensionState::NotInstalled,
    }
}

/// The names inside a GVariant string array, as `gsettings get` prints it.
fn parse_list(s: &str) -> Vec<String> {
    s.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn print_list(list: &[String]) -> String {
    let items: Vec<String> = list.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", items.join(", "))
}

const SCHEMA: &str = "org.gnome.shell";
const ENABLED: &str = "enabled-extensions";

fn enabled_list() -> Result<Vec<String>, String> {
    let out = Command::new("gsettings")
        .args(["get", SCHEMA, ENABLED])
        .output()
        .map_err(|e| format!("gsettings: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_list(&String::from_utf8_lossy(&out.stdout)))
}

fn set_enabled_list(list: &[String]) -> Result<(), String> {
    let out = Command::new("gsettings")
        .args(["set", SCHEMA, ENABLED, &print_list(list)])
        .output()
        .map_err(|e| format!("gsettings: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Copies the extension into place and switches it on for the next login.
pub fn install() -> Result<(), String> {
    if !is_gnome() {
        return Err("this desktop is not GNOME".into());
    }
    let dir = dir()?;
    for (name, text) in FILES {
        let path = dir.join(name);
        let parent = path.parent().expect("a folder");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut list = enabled_list()?;
    if !list.iter().any(|u| u == UUID) {
        list.push(UUID.to_string());
        set_enabled_list(&list)?;
    }
    Ok(())
}

/// Switches the extension off and removes its files.
pub fn remove() -> Result<(), String> {
    let mut list = enabled_list()?;
    if list.iter().any(|u| u == UUID) {
        list.retain(|u| u != UUID);
        set_enabled_list(&list)?;
    }
    let dir = dir()?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gnome_replies() {
        assert_eq!(
            parse_state("({'uuid': <'x'>, 'state': <1.0>, 'path': <'/p'>},)"),
            Some(1.0)
        );
        assert_eq!(parse_state("(@a{sv} {},)"), None);
    }

    #[test]
    fn extension_lists_survive_a_round_trip() {
        assert!(parse_list("@as []").is_empty());
        let list = parse_list("['a@b', 'jetcarp@jimmieprodgers.com']");
        assert_eq!(list, ["a@b", "jetcarp@jimmieprodgers.com"]);
        assert_eq!(print_list(&list), "['a@b', 'jetcarp@jimmieprodgers.com']");
    }

    #[test]
    fn metadata_names_the_uuid_and_the_lock_screen() {
        let m: serde_json::Value = serde_json::from_str(FILES[0].1).unwrap();
        assert_eq!(m["uuid"], UUID);
        assert_eq!(m["session-modes"][1], "unlock-dialog");
    }
}
