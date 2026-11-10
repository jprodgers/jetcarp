//! App settings: the menu bar, status bar, pane layout, and tool windows, saved as JSON at
//! `<config dir>/jetcarp/app.json` (`$JETCARP_CONFIG_DIR/app.json` when set). Screenshot runs
//! (`JETCARP_SCREENSHOT`) neither read nor write the file unless `JETCARP_CONFIG_DIR` is set, so
//! they start from the defaults and leave the user's file alone.
//!
//! A file that cannot be read, or one written by another settings version, gives the
//! defaults. Saving is debounced: at most every couple of seconds while something changes,
//! and once at exit.

use super::*;
use crate::popout::WinState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bump when the format changes; older files are ignored (no migration).
pub(super) const SETTINGS_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub(super) struct AppSettings {
    pub version: u32,
    pub menu_autohide: bool,
    pub status_bar: bool,
    pub cell_outlines: bool,
    pub panes_hidden: bool,
    /// Docked tree, pane places, popped-out pane sizes.
    pub layout: panes::Layout,
    /// Tool windows by key (`ToolWin::key`): open, popped out, size.
    pub tool_windows: BTreeMap<String, ToolWindowSettings>,
    /// Switch AI control on at launch (as `JETCARP_AI_CONTROL=1` does).
    pub ai_control_at_launch: bool,
    /// Outputs (window, full screen, span, mirror), cell size, keep-awake, screensaver. A
    /// display section that cannot be read gives the default display only.
    #[serde(deserialize_with = "lenient")]
    pub display: DisplaySettings,
    /// The Reseed pane: saved presets, the Reseed now modes, the Auto reseed preset.
    pub reseed: reseed_ui::ReseedSettings,
    /// The last wallpaper export choices.
    pub wallpaper: wallpaper_ui::WallpaperSettings,
    /// Each tool window's palette (sandpile, ants, HashLife view, library, hunts, queue).
    #[serde(deserialize_with = "lenient")]
    pub tool_palettes: crate::palette_pick::ToolPalettes,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub(super) struct ToolWindowSettings {
    pub open: bool,
    #[serde(flatten)]
    pub window: WinState,
}

pub(super) use super::display::DisplaySettings;

/// Any value, or the type's default when it does not parse.
fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            version: SETTINGS_VERSION,
            menu_autohide: false,
            status_bar: true,
            cell_outlines: true,
            panes_hidden: false,
            layout: panes::Layout::default(),
            tool_windows: BTreeMap::new(),
            ai_control_at_launch: false,
            display: DisplaySettings::default(),
            reseed: Default::default(),
            wallpaper: Default::default(),
            tool_palettes: Default::default(),
        }
    }
}

impl AppSettings {
    /// Parse a settings file; anything unreadable or of another version gives the defaults.
    pub fn from_json(text: &str) -> AppSettings {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            return AppSettings::default();
        };
        if v.get("version").and_then(|x| x.as_u64()) != Some(SETTINGS_VERSION as u64) {
            return AppSettings::default();
        }
        match serde_json::from_value::<AppSettings>(v) {
            Ok(mut s) => {
                s.layout.sanitize();
                s
            }
            Err(_) => AppSettings::default(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Where the settings live, and when they were last written.
pub(super) struct Store {
    path: Option<PathBuf>,
    last_json: String,
    last_check: f64,
}

impl Store {
    /// The settings file for this run (None: do not read or write one).
    pub fn from_env() -> Store {
        let path = match std::env::var_os("JETCARP_CONFIG_DIR") {
            Some(d) => Some(PathBuf::from(d).join("app.json")),
            None if std::env::var_os("JETCARP_SCREENSHOT").is_some() => None,
            None => dirs::config_dir().map(|d| d.join("jetcarp").join("app.json")),
        };
        Store {
            path,
            last_json: String::new(),
            last_check: 0.0,
        }
    }

    #[cfg(test)]
    pub fn at(path: &Path) -> Store {
        Store {
            path: Some(path.to_path_buf()),
            last_json: String::new(),
            last_check: 0.0,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn load(&mut self) -> AppSettings {
        let Some(p) = &self.path else {
            return AppSettings::default();
        };
        let s = std::fs::read_to_string(p)
            .map(|t| AppSettings::from_json(&t))
            .unwrap_or_default();
        self.last_json = s.to_json();
        s
    }

    /// Write if the settings changed since the last write.
    pub fn save(&mut self, s: &AppSettings) {
        let Some(p) = &self.path else { return };
        let json = s.to_json();
        if json == self.last_json {
            return;
        }
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write a sibling and rename, so a crash never leaves half a file.
        let tmp = p.with_extension("json.tmp");
        match std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, p)) {
            Ok(()) => self.last_json = json,
            Err(e) => eprintln!("settings: could not write {}: {e}", p.display()),
        }
    }

    /// Debounced save: at most every two seconds.
    pub fn save_due(&mut self, now: f64) -> bool {
        if self.path.is_none() || now - self.last_check < 2.0 {
            return false;
        }
        self.last_check = now;
        true
    }
}

impl JetcarpApp {
    /// The settings as they are now.
    pub(super) fn settings_snapshot(&self, ctx: &egui::Context) -> AppSettings {
        let reg = crate::popout::registry(ctx);
        let tool_windows = menu::ToolWin::ALL
            .iter()
            .map(|t| {
                let window = reg.0.get(t.key()).cloned().unwrap_or_default();
                let s = ToolWindowSettings {
                    open: self.tool_open(*t),
                    window,
                };
                (t.key().to_string(), s)
            })
            .filter(|(_, s)| *s != ToolWindowSettings::default())
            .collect();
        AppSettings {
            version: SETTINGS_VERSION,
            menu_autohide: self.shell.menu_autohide,
            status_bar: self.shell.status_bar,
            cell_outlines: self.shell.cell_outlines,
            panes_hidden: self.shell.panes_hidden,
            layout: self.shell.layout.clone(),
            tool_windows,
            ai_control_at_launch: self.shell.ai_at_launch,
            display: self.shell.display.clone(),
            reseed: self.reseed.snapshot(),
            wallpaper: self.wallpaper.settings.clone(),
            tool_palettes: self.tool_palettes(),
        }
    }

    /// Put loaded settings in place (at start).
    pub(super) fn apply_settings(&mut self, ctx: &egui::Context, s: AppSettings) {
        self.shell.menu_autohide = s.menu_autohide;
        self.shell.status_bar = s.status_bar;
        self.shell.cell_outlines = s.cell_outlines;
        self.shell.panes_hidden = s.panes_hidden;
        self.shell.layout = s.layout;
        self.shell.ai_at_launch = s.ai_control_at_launch;
        self.shell.display = s.display;
        self.reseed.restore(s.reseed);
        self.wallpaper.settings = s.wallpaper;
        self.set_tool_palettes(s.tool_palettes);
        let mut reg = crate::popout::Registry::default();
        for (k, t) in s.tool_windows {
            let Some(tool) = menu::ToolWin::from_key(&k) else {
                continue;
            };
            reg.0.insert(k, t.window);
            // Windows that show the current world (editors, HashLife view, palettes) start
            // closed: the world they showed is gone.
            let standalone = !matches!(
                tool,
                menu::ToolWin::TileEditor
                    | menu::ToolWin::CellEditor
                    | menu::ToolWin::HashLife
                    | menu::ToolWin::Palettes
                    | menu::ToolWin::Wallpaper
            );
            if t.open && standalone {
                self.set_tool_open(tool, true);
            }
        }
        crate::popout::set_registry(ctx, reg);
    }

    /// Save now if due (called every frame) or `force` (at exit).
    pub(super) fn save_settings(&mut self, ctx: &egui::Context, force: bool) {
        let now = ctx.input(|i| i.time);
        // While the screensaver runs the chrome is hidden on purpose: not saved.
        if !force && self.saver_active() {
            return;
        }
        if force || self.shell.store.save_due(now) {
            let s = self.settings_snapshot(ctx);
            self.shell.store.save(&s);
        }
    }

    /// Window → Reset layout: default panes, every window back in the main window.
    pub(super) fn reset_layout(&mut self, ctx: &egui::Context) {
        self.shell.layout = panes::Layout::default();
        self.shell.panes_hidden = false;
        let mut reg = crate::popout::registry(ctx);
        for w in reg.0.values_mut() {
            *w = WinState::default();
        }
        crate::popout::set_registry(ctx, reg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON as a value: the tile tree's maps serialize in hash order.
    fn value(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("jetcarp-settings-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn json_round_trip_with_the_tile_tree() {
        let mut s = AppSettings {
            menu_autohide: true,
            status_bar: false,
            ..Default::default()
        };
        s.layout.set_place("tiling", panes::Place::Floating);
        s.layout.set_place("cas", panes::Place::Popped);
        s.layout.windows.get_mut("cas").unwrap().size = Some([500.0, 420.0]);
        s.tool_windows.insert(
            "library".into(),
            ToolWindowSettings {
                open: true,
                window: WinState {
                    popped: true,
                    size: Some([900.0, 700.0]),
                    ..Default::default()
                },
            },
        );
        let json = s.to_json();
        let back = AppSettings::from_json(&json);
        assert!(back.menu_autohide);
        assert!(!back.status_bar);
        assert_eq!(back.layout.place("tiling"), panes::Place::Floating);
        assert_eq!(back.layout.place("cas"), panes::Place::Popped);
        assert_eq!(back.layout.windows["cas"].size, Some([500.0, 420.0]));
        assert_eq!(back.tool_windows["library"], s.tool_windows["library"]);
        assert_eq!(
            format!("{:?}", back.layout.tree),
            format!("{:?}", s.layout.tree)
        );
        assert_eq!(value(&back.to_json()), value(&json));
    }

    #[test]
    fn display_settings_round_trip() {
        use super::super::display::{OutputMode, OutputSpec, SaverSource};
        let mut s = AppSettings::default();
        let d = &mut s.display;
        d.mode = OutputMode::Outputs;
        d.fullscreen_monitor = Some(1);
        d.outputs = vec![
            OutputSpec {
                monitor: 2,
                pos: Some([2560, -40]),
                window: None,
            },
            OutputSpec {
                monitor: 0,
                pos: Some([0, 0]),
                window: Some([800, 600]),
            },
        ];
        d.arrangement = crate::span::Arrangement::Mirror;
        d.units = crate::span::Units::Logical;
        d.hide_preview = true;
        d.cell_px = 1.0;
        d.keep_awake = true;
        d.screensaver.source = SaverSource::Rotation;
        d.screensaver.rotation = "Wolfram series".into();
        d.screensaver.exit_any_input = true;
        let json = s.to_json();
        let back = AppSettings::from_json(&json);
        assert_eq!(back.display, s.display);
        assert_eq!(value(&back.to_json()), value(&json));
        // A broken display section resets only the display.
        let mut v = value(&json);
        v["display"] = serde_json::json!({"mode": 7, "outputs": "x"});
        v["menu_autohide"] = serde_json::json!(true);
        let back = AppSettings::from_json(&v.to_string());
        assert!(back.menu_autohide);
        assert_eq!(back.display, DisplaySettings::default());
        // Wave 1's placeholder section reads as the default display.
        let old = r#"{"version": 1, "status_bar": false, "display": {"outputs": null, "screensaver": null, "keep_awake": false}}"#;
        let back = AppSettings::from_json(old);
        assert!(!back.status_bar);
        assert_eq!(back.display, DisplaySettings::default());
    }

    #[test]
    fn garbage_unknown_or_old_files_give_defaults() {
        let d = AppSettings::default().to_json();
        for text in [
            "",
            "not json",
            "[1, 2, 3]",
            r#"{"version": 0, "menu_autohide": true}"#,
            r#"{"version": 99, "menu_autohide": true}"#,
            r#"{"version": 1, "layout": "garbage"}"#,
        ] {
            assert_eq!(
                value(&AppSettings::from_json(text).to_json()),
                value(&d),
                "{text:?}"
            );
        }
        // Unknown fields are ignored; missing ones take defaults.
        let s = AppSettings::from_json(r#"{"version": 1, "menu_autohide": true, "future": 3}"#);
        assert!(s.menu_autohide);
        assert!(s.status_bar);
    }

    #[test]
    fn store_writes_only_on_change() {
        let dir = scratch("store");
        let p = dir.join("app.json");
        let mut st = Store::at(&p);
        assert!(!p.exists());
        let s = st.load();
        assert!(!s.menu_autohide);
        st.save(&s);
        // Defaults loaded from nothing equal what was "last written": no file yet.
        assert!(!p.exists());
        let s2 = AppSettings {
            menu_autohide: true,
            ..Default::default()
        };
        st.save(&s2);
        assert!(p.exists());
        let mut st2 = Store::at(&p);
        assert!(st2.load().menu_autohide);
        std::fs::write(&p, "{ broken").unwrap();
        assert!(!Store::at(&p).load().menu_autohide);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tool_palettes_round_trip() {
        use crate::palette_pick::ToolPalettes;
        let mut s = AppSettings {
            menu_autohide: true,
            ..Default::default()
        };
        s.tool_palettes = ToolPalettes {
            sandpile: "Paper".into(),
            sandpile_reverse: true,
            ants: Some("Glacier".into()),
            hashlife: Some("Mine".into()),
            library: Some("Candy".into()),
            hunts: None,
            queue: Some("Lava".into()),
        };
        let back = AppSettings::from_json(&s.to_json());
        assert_eq!(back.tool_palettes, s.tool_palettes);
        // Files from before the field, or with a broken section, give the default palettes
        // and keep everything else.
        let mut v = value(&s.to_json());
        v.as_object_mut().unwrap().remove("tool_palettes");
        let old = AppSettings::from_json(&v.to_string());
        assert_eq!(old.tool_palettes, ToolPalettes::default());
        assert!(old.menu_autohide);
        v["tool_palettes"] = serde_json::json!(42);
        let bad = AppSettings::from_json(&v.to_string());
        assert_eq!(bad.tool_palettes, ToolPalettes::default());
        assert!(bad.menu_autohide);
        // Unknown built-in names fall back; the HashLife view may name a world's palette.
        let mut odd = s.tool_palettes.clone();
        odd.sandpile = "Gone".into();
        odd.ants = Some("Gone".into());
        let odd = odd.sanitize();
        assert_eq!(odd.sandpile, "Ember");
        assert_eq!(odd.ants, None);
        assert_eq!(odd.hashlife.as_deref(), Some("Mine"));
        assert_eq!(odd.library.as_deref(), Some("Candy"));
    }
}
