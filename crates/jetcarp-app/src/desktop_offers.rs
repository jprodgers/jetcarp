//! The first-run setup's desktop offers, for the Desktop art and Everything profiles: a live
//! wallpaper (the background host showing scenes behind your windows), starting it at login, and
//! the screen saver. Each does what the matching `jetcarp host` command does (`host wallpaper on`
//! with `host start`, `host login on`, `host saver install`), so the setup wizard and `jetcarp
//! setup` share them. Nothing here runs unless someone accepts an offer: the setup operations
//! themselves never touch the desktop.
//!
//! One more, for every profile and not in [`Offer::ALL`]: JETCARP among the system's apps
//! ([`Offer::Launcher`], `jetcarp launcher install`), ticked on setup's summary when it isn't there
//! yet (#105).

use jetcarp_display::host_settings::{HostSettings, Place};
use jetcarp_display::{autostart, gnome, launcher, saver};
use jetcarp_mcp::link::host_call;
use serde_json::json;

/// Something setup offers to turn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Offer {
    /// Show scenes as the desktop background (starts the background host now).
    Wallpaper,
    /// Start the background host when you log in.
    Login,
    /// Add JETCARP to the system's screen savers (Windows and macOS).
    Saver,
    /// Add JETCARP, with its icon, to the system's apps: the Start menu, Launchpad, the apps menu.
    Launcher,
}

impl Offer {
    /// The Desktop page's offers (Desktop art, Everything).
    pub const ALL: [Offer; 3] = [Offer::Wallpaper, Offer::Login, Offer::Saver];

    /// What it does, as a choice.
    pub fn title(self) -> &'static str {
        match self {
            Offer::Wallpaper => "Show a live wallpaper on the desktop",
            Offer::Login => "Start the live wallpaper when you log in",
            Offer::Saver => "Use JETCARP as the screen saver",
            Offer::Launcher if cfg!(windows) => "Add JETCARP to the Start menu",
            Offer::Launcher if cfg!(target_os = "macos") => "Add JETCARP to Launchpad",
            Offer::Launcher => "Add JETCARP to the apps menu",
        }
    }

    /// More about it, in a line.
    pub fn detail(self) -> String {
        match self {
            Offer::Wallpaper => {
                let mut s = "JETCARP's background host runs scenes behind your windows, pausing on battery and while the processor is busy.".to_string();
                if cfg!(target_os = "linux") {
                    s.push_str(" On GNOME this needs JETCARP's GNOME Shell extension, which is installed for you; log out and back in once to load it.");
                }
                s
            }
            Offer::Login => "So the wallpaper comes back after a restart.".into(),
            Offer::Saver if cfg!(windows) => {
                "Adds JETCARP to Windows' screen savers and opens Screen Saver Settings, where you choose it.".into()
            }
            Offer::Saver if cfg!(target_os = "macos") => {
                "Adds JETCARP to macOS's screen savers and opens their settings, where you choose it.".into()
            }
            Offer::Saver => {
                "On Windows and macOS, adds JETCARP to the system's screen savers.".into()
            }
            Offer::Launcher if cfg!(target_os = "macos") => {
                "Makes JETCARP.app in your Applications folder, with JETCARP's icon, so Launchpad, Spotlight and the Dock open it.".into()
            }
            Offer::Launcher => "So JETCARP opens from its icon, like your other apps.".into(),
        }
    }

    /// The commands that do the same, for people who would rather run them themselves.
    pub fn commands(self) -> &'static str {
        match self {
            Offer::Wallpaper => "jetcarp host wallpaper on && jetcarp host start",
            Offer::Login => "jetcarp host login on",
            Offer::Saver => "jetcarp host saver install",
            Offer::Launcher => "jetcarp launcher install",
        }
    }

    /// It can be offered on this system; else why not.
    pub fn available(self) -> Result<(), String> {
        match self {
            Offer::Saver if !saver::AVAILABLE => Err(
                "this system has no screen saver list; on GNOME the wallpaper can run on the lock screen instead (jetcarp host lock screensaver)".into(),
            ),
            _ => Ok(()),
        }
    }

    /// Do it. Returns what was done, in a line.
    pub fn accept(self) -> Result<String, String> {
        self.available()?;
        match self {
            Offer::Wallpaper => wallpaper_on(),
            Offer::Login => {
                let cmd = autostart::login_command()?;
                autostart::set(true, &cmd)?;
                refresh_login_item();
                Ok("the background host starts at login".into())
            }
            Offer::Saver => {
                let (path, warnings) = saver::install()?;
                let mut s = format!("the screen saver is in {}", path.display());
                for w in warnings {
                    s.push_str(&format!(" (warning: {w})"));
                }
                Ok(s)
            }
            Offer::Launcher => launcher::install().map(|i| i.said),
        }
    }
}

/// On macOS, registers the login item with launchd when it was written by another version of
/// JETCARP or by none (`autostart::refresh`), on a thread of its own, since launchctl can take
/// seconds: after an upgrade launchd refuses at login a program that has changed since it was
/// registered (#99). Says what it did on standard error. Nothing to do on other systems.
pub fn refresh_login_item() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let t = std::thread::spawn(|| match autostart::refresh() {
        Ok(Some(note)) => eprintln!("JETCARP: {note}"),
        Ok(None) => {}
        Err(e) => {
            eprintln!("JETCARP: the login item could not be registered with launchd again: {e}")
        }
    });
    REFRESHING.lock().unwrap_or_else(|e| e.into_inner()).push(t);
}

/// The registrations [`refresh_login_item`] started.
static REFRESHING: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

/// Waits for the registrations still under way, so that ending the app doesn't cut one short
/// between launchd letting the old host go and starting this program.
pub fn finish_login_item() {
    let threads = std::mem::take(&mut *REFRESHING.lock().unwrap_or_else(|e| e.into_inner()));
    for t in threads {
        let _ = t.join();
    }
}

/// `jetcarp host wallpaper on`, then `jetcarp host start` (or a reload when the host runs).
/// Refused, as they are, until the photosensitive seizure warning has been accepted (#96).
fn wallpaper_on() -> Result<String, String> {
    jetcarp_core::photosensitivity::require("show the desktop background")?;
    let mut s = HostSettings::load().0;
    s.screens
        .iter_mut()
        .for_each(|x| x.place = Place::Wallpaper);
    s.save()?;
    let mut note = String::new();
    if cfg!(target_os = "linux") {
        match gnome::state() {
            gnome::ExtensionState::NoGnome => {
                note =
                    " (GNOME Shell was not found in this session, so the wallpaper waits for it)"
                        .into();
            }
            gnome::ExtensionState::Active => {}
            _ => {
                gnome::install()?;
                if let Ok(cmd) = autostart::host_command() {
                    autostart::record_host_program(&cmd)?;
                }
                note = format!(" ({})", gnome::state().describe());
            }
        }
    }
    if host_call(&json!({"action": "reload"})).is_ok() {
        return Ok(format!("the background host shows the wallpaper now{note}"));
    }
    autostart::host_command()?.spawn_detached()?;
    Ok(format!(
        "the background host started with the wallpaper on{note}"
    ))
}
