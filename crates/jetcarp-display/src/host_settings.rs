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
    /// The cell sizes *Fit to this computer* chose for this screen's rotation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuning: Option<Tuning>,
}

impl Default for ScreenSettings {
    fn default() -> Self {
        ScreenSettings {
            monitor: None,
            rotation: "A classic a day".into(),
            place: Place::Window,
            tuning: None,
        }
    }
}

/// The limits *Fit to this computer* tunes the desktop background for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct TuneLimits {
    /// JETCARP's own processor time, in percent of the whole machine (every core).
    pub cpu_percent: f32,
    /// JETCARP's own share of the GPU's time, in percent.
    pub gpu_percent: f32,
    /// Frames per second it must keep. None: the monitor's refresh rate, or 60, at most the
    /// host's own frame rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
    /// Tune again by itself when the monitors change (only once it has been tuned).
    pub auto: bool,
}

impl Default for TuneLimits {
    fn default() -> Self {
        TuneLimits {
            cpu_percent: 10.0,
            gpu_percent: 25.0,
            fps: None,
            auto: true,
        }
    }
}

impl TuneLimits {
    pub const CPU_RANGE: std::ops::RangeInclusive<f32> = 1.0..=100.0;
    pub const GPU_RANGE: std::ops::RangeInclusive<f32> = 1.0..=100.0;
    pub const FPS_RANGE: std::ops::RangeInclusive<f32> = 1.0..=240.0;

    /// In range.
    pub fn checked(mut self) -> TuneLimits {
        let fix = |x: f32, r: std::ops::RangeInclusive<f32>, d: f32| {
            if x.is_finite() {
                x.clamp(*r.start(), *r.end())
            } else {
                d
            }
        };
        let d = TuneLimits::default();
        self.cpu_percent = fix(self.cpu_percent, Self::CPU_RANGE, d.cpu_percent);
        self.gpu_percent = fix(self.gpu_percent, Self::GPU_RANGE, d.gpu_percent);
        self.fps = self
            .fps
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(*Self::FPS_RANGE.start(), *Self::FPS_RANGE.end()));
        self
    }

    /// The frame rate to keep: the one asked for, else the monitor's refresh rate (or 60), and
    /// never more than the host draws (`host_fps`).
    pub fn target_fps(&self, refresh: Option<f32>, host_fps: f32) -> f32 {
        let want = self
            .fps
            .or(refresh.filter(|r| r.is_finite() && *r >= 1.0))
            .unwrap_or(60.0);
        want.min(host_fps).max(1.0)
    }
}

/// How the GPU's time was measured.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum GpuTiming {
    /// No GPU: everything ran on the processor.
    #[default]
    None,
    /// The system's own count of JETCARP's GPU time (Linux's DRM `fdinfo`): exact, and blind to
    /// other programs' use of the GPU.
    Kernel,
    /// The GPU's own clock, from timestamp queries around the step and the drawing (an upper
    /// bound for the step, whose span includes the processor's share of it).
    Timestamps,
    /// From handing the work to the GPU until it said it was done: an upper bound, used where the
    /// GPU has no timestamp queries.
    Submission,
}

impl GpuTiming {
    pub fn describe(self) -> &'static str {
        match self {
            GpuTiming::None => "no GPU",
            GpuTiming::Kernel => "the system's count of JETCARP's GPU time",
            GpuTiming::Timestamps => "the GPU's timestamp queries",
            GpuTiming::Submission => "submission to completion (an upper bound)",
        }
    }
}

/// What *Fit to this computer* chose for one scene.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct TunedScene {
    /// The scene's label in the rotation.
    pub scene: String,
    /// The cell size the scene asks for, in physical pixels.
    pub design_px: f32,
    /// The cell size chosen, in physical pixels: never smaller than `design_px`.
    pub cell_px: f32,
    /// Cells at that size.
    pub cells: u64,
    /// JETCARP's processor time at that size, in percent of the machine.
    pub cpu_percent: f32,
    /// Its share of the GPU's time, in percent (None: not measured).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_percent: Option<f32>,
    /// The 95th percentile of the time a frame took, in milliseconds.
    pub p95_ms: f32,
    /// Generations stepped on the GPU (else on the processor).
    pub on_gpu: bool,
    pub gpu_timing: GpuTiming,
    /// It met every limit at `cell_px`.
    pub fits: bool,
    /// Why it did not, or why its size could not change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What *Fit to this computer* found for one screen: a cell size for each scene of its rotation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Tuning {
    /// The rotation it was found for. A screen given another rotation shows its scenes at their
    /// own sizes until it is tuned again.
    pub rotation: String,
    /// The canvas it was found for, in physical pixels.
    pub canvas: [u32; 2],
    /// Its pixels per point.
    pub scale: f64,
    /// The limits it was found for (each screen's share of them).
    pub limits: TuneLimits,
    /// The frame rate it kept.
    pub fps: f32,
    pub scenes: Vec<TunedScene>,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            rotation: String::new(),
            canvas: [0, 0],
            scale: 1.0,
            limits: TuneLimits::default(),
            fps: 0.0,
            scenes: vec![],
        }
    }
}

impl Tuning {
    /// The cell size, in pixels, for the scene labelled `label`, which asks for `design_px` on a
    /// canvas of `canvas` pixels: None when it was not tuned, its own size was chosen, or the
    /// scene has changed size since. On another canvas the cell count is kept (the size grows
    /// or shrinks with the canvas), never below the scene's own.
    pub fn cell_px_for(&self, label: &str, design_px: f32, canvas: [f64; 2]) -> Option<f32> {
        let t = self.scenes.iter().find(|s| s.scene == label)?;
        if (t.design_px - design_px).abs() > 1e-3 || t.cell_px <= design_px {
            return None;
        }
        let [w0, h0] = self.canvas;
        let mut px = t.cell_px;
        let (a0, a1) = (w0 as f64 * h0 as f64, canvas[0] * canvas[1]);
        if a0 > 0.0 && a1 > 0.0 && (a0 - a1).abs() > 0.5 {
            px = (px as f64 * (a1 / a0).sqrt()).round() as f32;
        }
        let px = px.clamp(design_px, *crate::profiles::CELL_PX_RANGE.end());
        (px > design_px).then_some(px)
    }

    /// Whether it was found for a canvas of this size.
    pub fn fits_canvas(&self, canvas: [f64; 2]) -> bool {
        self.canvas == [canvas[0].round() as u32, canvas[1].round() as u32]
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct HostSettings {
    pub screens: Vec<ScreenSettings>,
    /// Frames per second, at most.
    pub fps: f32,
    /// Step on the GPU as JETCARP's GPU setting says (`gpu.json`, or `JETCARP_GPU`: automatic
    /// by default). False: never, whatever that setting says.
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
    /// The limits *Fit to this computer* tunes for.
    pub tune: TuneLimits,
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
            tune: TuneLimits::default(),
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
        self.tune = self.tune.checked();
        self
    }

    /// The tuning that applies to screen `i` now: the one found for the rotation it runs.
    pub fn tuning_for(&self, i: usize, rotation: &str) -> Option<&Tuning> {
        self.screens
            .get(i)?
            .tuning
            .as_ref()
            .filter(|t| t.rotation == rotation)
    }

    /// Whether any screen has been tuned.
    pub fn tuned(&self) -> bool {
        self.screens.iter().any(|s| s.tuning.is_some())
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

    /// A host.json as JETCARP 0.4.1 wrote it.
    const OLD: &str = r#"{
  "screens": [{"monitor": 0, "rotation": "My wallpaper", "place": "wallpaper"}],
  "fps": 30.0, "gpu": true,
  "caption": {"show": false, "corner": "BottomLeft", "size": 20.0, "generation": false, "seconds": 0.0},
  "max_memory_mb": 6144, "max_cells": 64000000, "saver_rotation": null,
  "lock_screen": "live", "lock_awake_minutes": 30, "pause_on_battery": true,
  "still_picture": true, "busy_percent": 80
}"#;

    #[test]
    fn an_old_file_reads_untuned_and_a_tuned_one_round_trips() {
        let s: HostSettings = serde_json::from_str(OLD).unwrap();
        let s = s.checked();
        assert_eq!(s.screens[0].rotation, "My wallpaper");
        assert!(s.still_picture);
        assert_eq!(s.tune, TuneLimits::default());
        assert!(!s.tuned() && s.screens[0].tuning.is_none());
        // Untuned screens write no `tuning`, so a file read back by 0.4.1 looks as it did.
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("\"tuning\""), "{text}");
        let mut t = s.clone();
        t.tune = TuneLimits {
            cpu_percent: 5.0,
            gpu_percent: 40.0,
            fps: Some(60.0),
            auto: false,
        };
        t.screens[0].tuning = Some(Tuning {
            rotation: "My wallpaper".into(),
            canvas: [2560, 1440],
            scale: 1.25,
            limits: t.tune,
            fps: 30.0,
            scenes: vec![TunedScene {
                scene: "profile Life".into(),
                design_px: 3.0,
                cell_px: 5.0,
                cells: 147_456,
                cpu_percent: 2.5,
                gpu_percent: Some(4.0),
                p95_ms: 6.0,
                on_gpu: true,
                gpu_timing: GpuTiming::Timestamps,
                fits: true,
                note: None,
            }],
        });
        let back: HostSettings = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.checked(), t);
        assert!(t.tuned());
        assert!(t.tuning_for(0, "My wallpaper").is_some());
        assert!(t.tuning_for(0, "Another").is_none(), "another rotation");
        assert!(t.tuning_for(1, "My wallpaper").is_none(), "no such screen");
        // Limits out of range are brought in.
        let s: HostSettings = serde_json::from_str(
            r#"{"tune": {"cpu_percent": 0, "gpu_percent": 500, "fps": 1000}}"#,
        )
        .unwrap();
        let s = s.checked();
        assert_eq!((s.tune.cpu_percent, s.tune.gpu_percent), (1.0, 100.0));
        assert_eq!(s.tune.fps, Some(240.0));
        assert!(s.tune.auto, "unset fields keep their defaults");
    }

    #[test]
    fn a_tuned_size_applies_to_its_scene_and_keeps_the_cell_count_on_another_canvas() {
        let t = Tuning {
            rotation: "r".into(),
            canvas: [2000, 1000],
            scenes: vec![
                TunedScene {
                    scene: "a".into(),
                    design_px: 2.0,
                    cell_px: 6.0,
                    ..Default::default()
                },
                TunedScene {
                    scene: "b".into(),
                    design_px: 3.0,
                    cell_px: 3.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(t.cell_px_for("a", 2.0, [2000.0, 1000.0]), Some(6.0));
        assert_eq!(
            t.cell_px_for("b", 3.0, [2000.0, 1000.0]),
            None,
            "its own size"
        );
        assert_eq!(t.cell_px_for("c", 3.0, [2000.0, 1000.0]), None, "not tuned");
        assert_eq!(
            t.cell_px_for("a", 4.0, [2000.0, 1000.0]),
            None,
            "the scene's size changed since"
        );
        // Four times the pixels: twice the size, the same cells.
        assert_eq!(t.cell_px_for("a", 2.0, [4000.0, 2000.0]), Some(12.0));
        // A quarter of the pixels: half the size, never below the scene's own.
        assert_eq!(t.cell_px_for("a", 2.0, [1000.0, 500.0]), Some(3.0));
        assert_eq!(t.cell_px_for("a", 2.0, [500.0, 250.0]), None);
        assert!(t.fits_canvas([2000.0, 1000.0]) && !t.fits_canvas([1920.0, 1080.0]));
        let l = TuneLimits::default();
        assert_eq!(l.target_fps(Some(144.0), 30.0), 30.0, "at most the host's");
        assert_eq!(l.target_fps(None, 120.0), 60.0);
        assert_eq!(l.target_fps(Some(75.0), 120.0), 75.0);
        let l = TuneLimits {
            fps: Some(24.0),
            ..l
        };
        assert_eq!(l.target_fps(Some(144.0), 30.0), 24.0);
    }
}
