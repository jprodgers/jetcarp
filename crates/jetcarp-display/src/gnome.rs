//! Installing JETCARP's GNOME Shell extension, which turns the host's windows into the desktop and
//! lock-screen background and adds a top-bar menu. The files are built into the program.

use std::path::{Path, PathBuf};
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

/// The text of one of the extension's files as this program installs it: the metadata's
/// `version-name` is the program's version, so GNOME (and [`state`]) can tell which JETCARP's
/// extension it has loaded.
fn file_text(name: &str, text: &'static str) -> std::borrow::Cow<'static, str> {
    if name == "metadata.json" {
        text.replace("@VERSION@", env!("CARGO_PKG_VERSION")).into()
    } else {
        text.into()
    }
}

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
    /// Running, but GNOME Shell loaded an older JETCARP's extension (the version it reports, if
    /// any): the files on disk are this program's, and GNOME Shell reads them at login.
    Outdated(Option<String>),
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
            ExtensionState::Outdated(v) => format!(
                "The GNOME extension was updated to JETCARP {}'s, but GNOME is still running {}. Log out and back in once to load it.",
                env!("CARGO_PKG_VERSION"),
                v.as_deref()
                    .map_or("an older version".to_string(), |v| format!("JETCARP {v}'s"))
            ),
        }
    }
}

/// The extension's folder: `<data>/gnome-shell/extensions/<uuid>`, where `<data>` is
/// `$XDG_DATA_HOME` or `~/.local/share`. The same folder in any session, GNOME or not.
fn dir() -> Result<PathBuf, String> {
    Ok(extensions_dir()?.join(UUID))
}

/// GNOME Shell's folder of per-user extensions.
pub fn extensions_dir() -> Result<PathBuf, String> {
    Ok(dirs::data_dir()
        .ok_or("no data folder on this system")?
        .join("gnome-shell/extensions"))
}

/// Whether the extension's files are in `extensions` (GNOME Shell's folder of extensions). Read
/// from the folder itself, so it works outside a GNOME session too (from another desktop or a
/// text console), where [`state`] can only say [`ExtensionState::NoGnome`].
pub fn installed_in(extensions: &Path) -> bool {
    let d = extensions.join(UUID);
    d.join("metadata.json").exists() || d.join("extension.js").exists()
}

/// Whether the extension's files are in place for this user, whatever the session.
pub fn installed() -> bool {
    extensions_dir().is_ok_and(|d| installed_in(&d))
}

/// GNOME's numeric extension state in a `GetExtensionInfo` reply, printed by `gdbus`. An unknown
/// extension gives an empty dictionary.
fn parse_state(reply: &str) -> Option<f64> {
    let rest = reply.split("'state': <").nth(1)?;
    rest.split('>').next()?.trim().parse().ok()
}

/// The `version-name` in a `GetExtensionInfo` reply: the JETCARP version whose extension GNOME
/// Shell loaded (extensions from JETCARP 0.4.1 and earlier have none).
fn parse_version_name(reply: &str) -> Option<String> {
    let rest = reply.split("'version-name': <'").nth(1)?;
    Some(rest.split('\'').next()?.to_string())
}

/// GNOME 50's and 51's meaning for the state numbers (`ExtensionState` in `extensionUtils.js`).
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
    let reply = String::from_utf8_lossy(&out.stdout);
    match parse_state(&reply) {
        Some(1.0) => match parse_version_name(&reply) {
            Some(v) if v == env!("CARGO_PKG_VERSION") => ExtensionState::Active,
            // Files of another version on disk than GNOME loaded: one login loads them. (Files
            // left from an older JETCARP that nothing refreshed are what GNOME runs, and fine.)
            v if !stale() => ExtensionState::Outdated(v),
            _ => ExtensionState::Active,
        },
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

/// Whether the extension in `extensions` differs from this program's: one of its files is
/// missing or holds other text. False when it isn't installed.
pub fn stale_in(extensions: &Path) -> bool {
    if !installed_in(extensions) {
        return false;
    }
    let d = extensions.join(UUID);
    FILES.iter().any(|(name, text)| {
        std::fs::read_to_string(d.join(name)).map_or(true, |t| t != file_text(name, text))
    })
}

/// Whether this user's extension differs from this program's.
pub fn stale() -> bool {
    extensions_dir().is_ok_and(|d| stale_in(&d))
}

fn write_files(dir: &Path) -> Result<(), String> {
    for (name, text) in FILES {
        let path = dir.join(name);
        let parent = path.parent().expect("a folder");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, file_text(name, text).as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Brings an installed extension in `extensions` up to this program's files, so upgrading
/// JETCARP upgrades its extension too; the background host does this when it starts. It leaves
/// GNOME's list of enabled extensions alone (switching the extension off stays the user's
/// choice) and does nothing when the extension isn't installed or is already this program's.
/// GNOME Shell loads the new files at the next login. Returns whether it wrote them.
pub fn refresh_in(extensions: &Path) -> Result<bool, String> {
    if !stale_in(extensions) {
        return Ok(false);
    }
    write_files(&extensions.join(UUID))?;
    Ok(true)
}

/// [`refresh_in`] this user's extensions folder.
pub fn refresh() -> Result<bool, String> {
    refresh_in(&extensions_dir()?)
}

/// Copies the extension into place and switches it on for the next login.
pub fn install() -> Result<(), String> {
    if !is_gnome() {
        return Err("this desktop is not GNOME".into());
    }
    write_files(&dir()?)?;
    let mut list = enabled_list()?;
    if !list.iter().any(|u| u == UUID) {
        list.push(UUID.to_string());
        set_enabled_list(&list)?;
    }
    Ok(())
}

/// Switches the extension off and removes its files.
pub fn remove() -> Result<(), String> {
    remove_from(&extensions_dir()?, enabled_list, set_enabled_list)
}

/// Removes the extension's files from `extensions`, then takes it off GNOME's list of enabled
/// extensions through `list` and `set`. The files go first and decide the result: outside a
/// GNOME session the list may be out of reach (no `org.gnome.shell` schema on another desktop, or
/// no session bus to write it through), and GNOME Shell skips an enabled extension whose files
/// are gone, so a list left as it was does no harm.
pub fn remove_from(
    extensions: &Path,
    list: impl FnOnce() -> Result<Vec<String>, String>,
    set: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    let dir = extensions.join(UUID);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if let Ok(mut l) = list()
        && l.iter().any(|u| u == UUID)
    {
        l.retain(|u| u != UUID);
        let _ = set(&l);
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
    fn metadata_names_the_uuid_the_lock_screen_and_this_version() {
        let m: serde_json::Value =
            serde_json::from_str(&file_text(FILES[0].0, FILES[0].1)).unwrap();
        assert_eq!(m["uuid"], UUID);
        assert_eq!(m["session-modes"][1], "unlock-dialog");
        assert_eq!(m["version-name"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn reads_the_loaded_version() {
        let reply = "({'uuid': <'x'>, 'version-name': <'0.4.2'>, 'state': <1.0>},)";
        assert_eq!(parse_version_name(reply).as_deref(), Some("0.4.2"));
        assert_eq!(parse_version_name("({'state': <1.0>},)"), None);
    }

    /// An upgrade's host brings an older extension's files up to its own, leaves a current one
    /// and a missing one alone, and doesn't touch other extensions.
    #[test]
    fn refresh_updates_only_an_installed_older_extension() {
        let tmp = std::env::temp_dir().join(format!("jetcarp-gnome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // Not installed: nothing written.
        assert!(!stale_in(&tmp));
        assert_eq!(refresh_in(&tmp), Ok(false));
        assert!(!tmp.join(UUID).exists());
        // An older version's files (as 0.4.1 left them: other metadata, other script).
        let d = tmp.join(UUID);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("metadata.json"), r#"{"uuid": "jetcarp@jimmieprodgers.com", "shell-version": ["50"]}"#).unwrap();
        std::fs::write(d.join("extension.js"), "// old").unwrap();
        assert!(stale_in(&tmp));
        assert_eq!(refresh_in(&tmp), Ok(true));
        for (name, text) in FILES {
            assert_eq!(std::fs::read_to_string(d.join(name)).unwrap(), file_text(name, text));
        }
        // Now current: left alone.
        assert!(!stale_in(&tmp));
        assert_eq!(refresh_in(&tmp), Ok(false));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
