//! The background host's settings: `host.json` beside the app's settings. The file is the one
//! place they live. The app and the CLI write it, then tell a running host to read it again.

use crate::caption::CaptionSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a screen's scene appears.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    #[default]
    Window,
    /// Behind everything, as the desktop background.
    Wallpaper,
}

/// What a wallpaper does on the lock screen (GNOME).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum LockScreen {
    /// GNOME's own background.
    Off,
    /// The scene, blurred behind the clock. GNOME fades the screen to black as usual.
    #[default]
    Live,
    /// The scene stays sharp and lit until someone reaches for the unlock prompt.
    Screensaver,
}

impl LockScreen {
    pub const ALL: [LockScreen; 3] = [LockScreen::Off, LockScreen::Live, LockScreen::Screensaver];

    pub fn label(self) -> &'static str {
        match self {
            LockScreen::Off => "GNOME's background",
            LockScreen::Live => "Live behind the clock",
            LockScreen::Screensaver => "Screensaver (stays lit)",
        }
    }

    /// The word used in the extension's window tag and on the command line.
    pub fn word(self) -> &'static str {
        match self {
            LockScreen::Off => "off",
            LockScreen::Live => "live",
            LockScreen::Screensaver => "screensaver",
        }
    }
}

/// One running scene.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct ScreenSettings {
    /// Which monitor (index in the OS's list). None: a normal window, or for a wallpaper, every
    /// monitor no other screen claims.
    pub monitor: Option<usize>,
    pub rotation: String,
    pub place: Place,
}

impl Default for ScreenSettings {
    fn default() -> Self {
        ScreenSettings {
            monitor: None,
            rotation: "A classic a day".into(),
            place: Place::Window,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct HostSettings {
    pub screens: Vec<ScreenSettings>,
    /// Frames per second, at most.
    pub fps: f32,
    /// Step on the GPU where the rules allow it.
    pub gpu: bool,
    pub caption: CaptionSettings,
    /// Abandon an item when the host passes this much memory.
    pub max_memory_mb: u64,
    /// Refuse to build a world estimated to have more cells than this.
    pub max_cells: u64,
    /// The Windows screen saver's rotation. None: the first screen's.
    pub saver_rotation: Option<String>,
    pub lock_screen: LockScreen,
    /// In screensaver mode, let the screen go dark after this many minutes locked (0: never).
    pub lock_awake_minutes: u32,
    /// Pause while the computer runs on battery.
    pub pause_on_battery: bool,
    /// Leave a still of the scene as the desktop picture when the host stops, or while the live
    /// wallpaper can't be shown. The user's own picture comes back when this or the wallpaper is
    /// turned off.
    pub still_picture: bool,
    /// Pause while the whole processor stays this busy (percent), as during research runs, and
    /// carry on once it is well below. 0: never.
    pub busy_percent: u8,
}

impl Default for HostSettings {
    fn default() -> Self {
        HostSettings {
            screens: vec![ScreenSettings::default()],
            fps: 30.0,
            gpu: true,
            caption: CaptionSettings::default(),
            max_memory_mb: 6144,
            max_cells: 64_000_000,
            saver_rotation: None,
            lock_screen: LockScreen::default(),
            lock_awake_minutes: 30,
            pause_on_battery: true,
            still_picture: false,
            busy_percent: 80,
        }
    }
}

pub const MAX_AWAKE_MINUTES: u32 = 24 * 60;

/// JETCARP's settings folder (`JETCARP_CONFIG_DIR` overrides it, as it does for the app).
pub fn config_dir() -> Option<PathBuf> {
    jetcarp_core::paths::config_dir()
}

/// The file the stores of profiles and rotations are placed beside.
pub fn app_settings_file() -> Option<PathBuf> {
    Some(config_dir()?.join("app.json"))
}

fn file() -> Result<PathBuf, String> {
    Ok(config_dir()
        .ok_or("no configuration folder on this system")?
        .join("host.json"))
}

impl HostSettings {
    /// Reads the settings. A file that cannot be understood gives the defaults and says why.
    pub fn load() -> (HostSettings, Option<String>) {
        let Ok(path) = file() else {
            return (HostSettings::default(), None);
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (HostSettings::default(), None);
        };
        match serde_json::from_str::<HostSettings>(&text) {
            Ok(s) => (s.checked(), None),
            Err(e) => (
                HostSettings::default(),
                Some(format!("{}: {e}; using the defaults", path.display())),
            ),
        }
    }

    /// In range, and with something to run.
    fn checked(mut self) -> HostSettings {
        if self.screens.is_empty() {
            self.screens.push(ScreenSettings::default());
        }
        if !self.fps.is_finite() || self.fps < 1.0 {
            self.fps = 1.0;
        }
        self.fps = self.fps.min(240.0);
        self.lock_awake_minutes = self.lock_awake_minutes.min(MAX_AWAKE_MINUTES);
        self
    }

    pub fn save(&self) -> Result<(), String> {
        let path = file()?;
        let dir = path.parent().expect("a folder");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Written whole and moved, so a host reading it never sees half a file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files_fill_in_and_bad_values_are_fixed() {
        let s: HostSettings = serde_json::from_str(r#"{"fps": 0}"#).unwrap();
        let s = s.checked();
        assert_eq!(s.fps, 1.0);
        assert_eq!(s.screens.len(), 1);
        assert_eq!(s.screens[0].rotation, "A classic a day");
        let s: HostSettings = serde_json::from_str(r#"{"fps": 9000, "screens": []}"#).unwrap();
        let s = s.checked();
        assert_eq!(s.fps, 240.0);
        assert_eq!(s.screens.len(), 1);
    }

    #[test]
    fn places_and_the_lock_screen_are_read_by_name() {
        let s: HostSettings = serde_json::from_str(
            r#"{"lock_screen": "screensaver", "lock_awake_minutes": 999999,
                "screens": [{"rotation": "x", "place": "wallpaper", "monitor": 1}]}"#,
        )
        .unwrap();
        let s = s.checked();
        assert_eq!(s.lock_screen, LockScreen::Screensaver);
        assert_eq!(s.lock_awake_minutes, MAX_AWAKE_MINUTES);
        assert_eq!(s.screens[0].place, Place::Wallpaper);
        assert_eq!(s.screens[0].monitor, Some(1));
        // An older file means a window, and the live lock screen.
        let s: HostSettings = serde_json::from_str(r#"{"screens": [{"rotation": "x"}]}"#).unwrap();
        assert_eq!(s.screens[0].place, Place::Window);
        assert_eq!(s.lock_screen, LockScreen::Live);
    }
}
