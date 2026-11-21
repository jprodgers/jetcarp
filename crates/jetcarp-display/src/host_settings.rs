//! The background host's settings: `host.json` beside the app's settings. The file is the one
//! place they live. The app and the CLI write it, then tell a running host to read it again.

use crate::caption::CaptionSettings;
pub use crate::pace::Pacing;
use crate::pace::{DEFAULT_MAX_FPS, Pacer};
pub use jetcarp_gpu::select::Own;
use jetcarp_gpu::select::Policy;
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
    /// Frames per second it must keep. None: the monitor's refresh rate, or 60, at most the rate
    /// the scene draws at.
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
    /// never more than the scene is drawn at (`draws`, its [`crate::pace::Pace`]'s).
    pub fn target_fps(&self, refresh: Option<f32>, draws: f32) -> f32 {
        let want = self
            .fps
            .or(refresh.filter(|r| r.is_finite() && *r >= 1.0))
            .unwrap_or(60.0);
        want.min(draws).max(1.0)
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
    /// GPU has no timestamp queries and the system does not count JETCARP's GPU time. It counts the
    /// time the work waits in the driver's queue (3 to 10 times Windows' own count on the Acer's
    /// Intel UHD, #72), so the share it gives is not measured, and not judged.
    Submission,
    /// Windows' GPU engine counters for JETCARP's process, the figures Task Manager shows: its 3D
    /// engines' share of their time. Exact for its own work, like [`GpuTiming::Kernel`]; used on
    /// Windows where the adapter has no timestamp queries (#72).
    Counters,
}

/// What the cards and `host tune` say when the GPU share wasn't measured ([`GpuTiming::Submission`]).
pub const GPU_UNMEASURED: &str = "GPU share not measured on this adapter (it has no GPU timestamps): the GPU limit isn't applied";

impl GpuTiming {
    pub fn describe(self) -> &'static str {
        match self {
            GpuTiming::None => "no GPU",
            GpuTiming::Kernel => "the system's count of JETCARP's GPU time",
            GpuTiming::Timestamps => "the GPU's timestamp queries",
            GpuTiming::Submission => "submission to completion (an upper bound, not judged)",
            GpuTiming::Counters => "Windows' GPU engine counters for JETCARP's process",
        }
    }

    /// Whether a GPU was there but its share wasn't measured, so the GPU limit isn't applied.
    pub fn unmeasured(self) -> bool {
        self == GpuTiming::Submission
    }
}

/// A size a scene can show at: a cell size in physical pixels on a lattice, a patch level on an
/// aperiodic tiling (whose tiles are then as big as fitting the patch to the screen makes them).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Size {
    /// Physical pixels per cell: what a lattice is built at, and what a patch's tiles show at.
    pub px: f32,
    /// An aperiodic patch's level (None: the scene's own, or a lattice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
}

impl Size {
    pub fn px(px: f32) -> Size {
        Size { px, level: None }
    }

    /// `3 px`, `level 6 (2.4 px)`.
    pub fn describe(&self) -> String {
        match self.level {
            Some(l) => format!("level {l} ({} px)", round_px(self.px)),
            None => format!("{} px", round_px(self.px)),
        }
    }

    /// The same size (a level, or a cell size within a hundredth of a pixel).
    pub fn same(&self, o: &Size) -> bool {
        match (self.level, o.level) {
            (Some(a), Some(b)) => a == b,
            (None, None) => (self.px - o.px).abs() < 0.01,
            _ => false,
        }
    }
}

/// A pixel size as people read it: whole, or to a tenth.
pub fn round_px(px: f32) -> String {
    if (px - px.round()).abs() < 0.05 {
        format!("{}", px.round())
    } else {
        format!("{px:.1}")
    }
}

/// What one size of a sweep measured: one card in the pane.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Rung {
    /// The size, with the cell size the screen showed (for a patch, its tiles' size).
    pub size: Size,
    /// Cells in the world.
    pub cells: u64,
    /// JETCARP's processor time, in percent of the machine.
    pub cpu_percent: f32,
    /// Its share of the GPU's time, in percent (None: not measured).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_percent: Option<f32>,
    /// A GPU was there, but its share wasn't measured (no GPU timestamps, and no count of
    /// JETCARP's GPU time from the system), so the GPU limit wasn't applied to this size: the card
    /// shows the share as unknown ([`GPU_UNMEASURED`], #72).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gpu_unmeasured: bool,
    /// The 95th percentile of the time a frame took, in milliseconds.
    pub p95_ms: f32,
    /// Frames drawn a second.
    pub fps: f32,
    /// Stepped on the GPU (else on the processor).
    pub on_gpu: bool,
    /// Within every limit, with their margins.
    pub fits: bool,
    /// Which limits it passed, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub over: Option<String>,
    /// The picture of the screen at this size: a PNG in the tuning folder ([`crops_dir`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<String>,
    /// The scene's own size.
    pub own: bool,
}

/// What *Fit to this computer* found and chose for one scene.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct TunedScene {
    /// The scene's label in the rotation.
    pub scene: String,
    /// The scene's tiling family.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tiling: String,
    /// The cell size the scene asks for, in physical pixels.
    pub design_px: f32,
    /// The size it shows at: a cell size in physical pixels (a lattice), or with `level`, the
    /// size a patch of that level's tiles show at.
    pub cell_px: f32,
    /// An aperiodic patch's level (None: the scene's own, or a lattice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    /// Someone chose this size (else it is the recommended one).
    pub chosen: bool,
    /// The finest size within the limits (a placed pattern: its own size).
    pub recommended: Size,
    /// A pattern placed on an empty field, which keeps its own size unless someone chooses another.
    pub placed: bool,
    /// Every size the sweep showed, finest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rungs: Vec<Rung>,
    pub gpu_timing: GpuTiming,
    /// Why it shows at this size, when that needs saying.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl TunedScene {
    /// The size it shows at.
    pub fn size(&self) -> Size {
        Size {
            px: self.cell_px,
            level: self.level,
        }
    }

    /// Shows at `size`.
    pub fn set_size(&mut self, size: Size) {
        self.cell_px = size.px;
        self.level = size.level;
    }

    /// The rung at the size it shows at. A scene left at its own size is kept at the size it
    /// asks for, not the size its cells came to (a patch's tiles fit it to the screen), so that is
    /// its own rung.
    pub fn rung(&self) -> Option<&Rung> {
        let s = self.size();
        self.rungs.iter().find(|r| r.size.same(&s)).or_else(|| {
            (s.level.is_none() && (s.px - self.design_px).abs() < 0.01)
                .then(|| self.rungs.iter().find(|r| r.own))
                .flatten()
        })
    }
}

/// The folder the sweep's pictures go in: `tune` beside the host's settings.
pub fn crops_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("tune"))
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
    /// The frame rate the scene saved last was held to (each scene is held to the rate it draws
    /// at).
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
    /// The size for the scene labelled `label`, which asks for `design_px` on a canvas of
    /// `canvas` pixels: None when it was not tuned, its own size was chosen, or the scene has
    /// changed size since. On another canvas a lattice keeps its cell count (its cells grow or
    /// shrink with the canvas), and a patch its level.
    pub fn size_for(&self, label: &str, design_px: f32, canvas: [f64; 2]) -> Option<Size> {
        let t = self.scenes.iter().find(|s| s.scene == label)?;
        if (t.design_px - design_px).abs() > 1e-3 {
            return None;
        }
        if let Some(level) = t.level {
            return Some(Size {
                px: t.cell_px,
                level: Some(level),
            });
        }
        let [w0, h0] = self.canvas;
        let mut px = t.cell_px;
        let (a0, a1) = (w0 as f64 * h0 as f64, canvas[0] * canvas[1]);
        if a0 > 0.0 && a1 > 0.0 && (a0 - a1).abs() > 0.5 && px > 0.0 {
            px = ((px as f64 * (a1 / a0).sqrt()).round() as f32).max(1.0);
        }
        let px = px.min(*crate::profiles::CELL_PX_RANGE.end());
        (px > 0.0 && (px - design_px).abs() > 1e-3).then_some(Size::px(px))
    }

    /// Whether it was found for a canvas of this size.
    pub fn fits_canvas(&self, canvas: [f64; 2]) -> bool {
        self.canvas == [canvas[0].round() as u32, canvas[1].round() as u32]
    }

    /// Whether the scene labelled `label` is one of `labels`, the scenes of the rotation its screen
    /// shows now (None: not known, so every scene is). A size chosen while another rotation was
    /// shown stays saved, for the sweep to keep while it fits should that scene come back, but it
    /// is not one of this rotation's scenes, so it gets no row in the cards (#73).
    pub fn shows(label: &str, labels: Option<&[String]>) -> bool {
        labels.is_none_or(|l| l.iter().any(|x| x == label))
    }

    /// Its scenes that are among `labels` ([`Tuning::shows`]), in the order they were saved.
    pub fn scenes_shown<'a>(
        &'a self,
        labels: Option<&'a [String]>,
    ) -> impl Iterator<Item = &'a TunedScene> {
        self.scenes
            .iter()
            .filter(move |s| Tuning::shows(&s.scene, labels))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct HostSettings {
    pub screens: Vec<ScreenSettings>,
    /// Frames per second: what every screen draws at least, and with [`Pacing::LowPower`], what
    /// it draws at and the most generations a second a scene runs at.
    pub fps: f32,
    /// How a screen's frame rate follows its scene's speed. Files from before 0.4.2 show every
    /// generation.
    pub pacing: Pacing,
    /// The most frames a second a screen showing every generation draws at (default
    /// [`DEFAULT_MAX_FPS`]); `null`: up to its monitor's refresh rate, however high.
    pub max_fps: Option<f32>,
    /// The desktop background's own GPU choice: follow JETCARP's shared setting (`gpu.json`,
    /// automatic by default), or on, or off whatever it says. `JETCARP_GPU` comes first. Older
    /// files' `true` reads as follow and `false` as off.
    #[serde(deserialize_with = "Own::deserialize_lenient")]
    pub gpu: Own,
    pub caption: CaptionSettings,
    /// Abandon an item when the host passes this much memory. Since 0.4.3 the limit is the
    /// machine's `host-memory` ([`HostSettings::memory_limit_mb`]); a value here other than the
    /// old default (a file edited by hand) still wins.
    pub max_memory_mb: u64,
    /// Refuse to build a world estimated to have more cells than this. Since 0.4.3 the limit is
    /// the machine's `host-cells` ([`HostSettings::cell_limit`]); a value here other than the old
    /// default still wins.
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
            pacing: Pacing::default(),
            max_fps: Some(DEFAULT_MAX_FPS),
            gpu: Own::Follow,
            caption: CaptionSettings::default(),
            max_memory_mb: OLD_MAX_MEMORY_MB,
            max_cells: OLD_MAX_CELLS,
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

/// `max_memory_mb` and `max_cells` as every `host.json` had them before 0.4.3: a file with these
/// leaves the limits to the machine.
pub const OLD_MAX_MEMORY_MB: u64 = 6144;
pub const OLD_MAX_CELLS: u64 = 64_000_000;

impl HostSettings {
    /// The most cells one of the host's worlds may have: this machine's `host-cells` limit
    /// (`jetcarp limits`, Tools → This computer…), unless this file holds a value of its own, and
    /// never more than a Life world in what the rest of JETCARP leaves of the pool
    /// ([`jetcarp_core::machine::pool`]).
    pub fn cell_limit(&self) -> u64 {
        let own = if self.max_cells != OLD_MAX_CELLS {
            self.max_cells
        } else {
            jetcarp_core::machine::current().host_cells()
        };
        match pool_left() {
            Some(left) => own.min(left / jetcarp_core::machine::limits::LIFE_CELL_BYTES),
            None => own,
        }
    }

    /// The memory the host may use before it gives up its largest world, megabytes: this
    /// machine's `host-memory` limit (all of the pool by default), unless this file holds a value
    /// of its own, and never more than the rest of JETCARP leaves of the pool. So the desktop
    /// background yields when the app or a command needs the memory.
    pub fn memory_limit_mb(&self) -> u64 {
        let own = if self.max_memory_mb != OLD_MAX_MEMORY_MB {
            self.max_memory_mb
        } else {
            jetcarp_core::machine::current().host_memory_mb()
        };
        match pool_left() {
            Some(left) => own.min(left >> 20),
            None => own,
        }
    }
}

/// What the other JETCARP programs leave of the pool for this one (none when it does not record
/// its memory: tests, and code using the crate as a library).
fn pool_left() -> Option<u64> {
    let l = jetcarp_core::machine::current();
    jetcarp_core::machine::pool::Pool::now(&l).map(|p| p.left_for_this_program())
}

/// Who the host's own GPU choice belongs to, in reports.
pub const OWNER: &str = "the desktop background";

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

    /// The tuning the host applies to screen `i` now: [`HostSettings::tuning_for`]'s while *Fit
    /// to this computer* is in this release. With it switched off ([`jetcarp_core::fit::FIT`],
    /// 0.4.2, #112) none: each scene shows at its own size, and what an earlier Fit saved stays in
    /// the file, untouched, for 0.4.3.
    pub fn tuning_applied(&self, i: usize, rotation: &str) -> Option<&Tuning> {
        self.tuning_for(i, rotation)
            .filter(|_| jetcarp_core::fit::FIT)
    }

    /// The GPU policy the desktop background follows: `JETCARP_GPU`, else its own choice, else the
    /// shared setting.
    pub fn gpu_policy(&self) -> Policy {
        Policy::load_for(self.gpu, OWNER)
    }

    /// What a screen on a monitor refreshing `refresh` times a second (None: not known) is paced
    /// by.
    pub fn pacer(&self, refresh: Option<f32>) -> Pacer {
        Pacer {
            pacing: self.pacing,
            fps: self.fps,
            refresh,
            max_fps: self.max_fps,
        }
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

    /// The host's cells and memory come from the machine's limits (the reference machine's in
    /// tests: the 64 million cells every host.json had before, and all of JETCARP's 8 GB pool),
    /// unless host.json holds its own.
    #[test]
    fn the_host_limits_come_from_the_machine_unless_set_here() {
        let s = HostSettings::default();
        assert_eq!((s.cell_limit(), s.memory_limit_mb()), (64_000_000, 8192));
        let own = HostSettings {
            max_cells: 1_000_000,
            max_memory_mb: 2048,
            ..HostSettings::default()
        };
        assert_eq!((own.cell_limit(), own.memory_limit_mb()), (1_000_000, 2048));
    }

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
        assert_eq!(s.pacing, Pacing::EveryGeneration, "0.4.1 showed no choice");
        assert_eq!(
            s.gpu,
            Own::Follow,
            "0.4.1's true follows the shared setting"
        );
        let off: HostSettings =
            serde_json::from_str(&OLD.replace(r#""gpu": true"#, r#""gpu": false"#)).unwrap();
        assert_eq!(off.gpu, Own::Off, "and false is off");
        let on: HostSettings = serde_json::from_str(r#"{"gpu": "on"}"#).unwrap();
        assert_eq!(on.gpu, Own::On);
        assert!(
            serde_json::to_string(&on)
                .unwrap()
                .contains(r#""gpu":"on""#)
        );
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
                tiling: "square".into(),
                design_px: 3.0,
                cell_px: 2.0,
                level: None,
                chosen: true,
                recommended: Size::px(4.0),
                placed: false,
                rungs: vec![Rung {
                    size: Size::px(2.0),
                    cells: 921_600,
                    cpu_percent: 2.5,
                    gpu_percent: Some(4.0),
                    gpu_unmeasured: false,
                    p95_ms: 6.0,
                    fps: 30.0,
                    on_gpu: true,
                    fits: false,
                    over: Some("GPU".into()),
                    crop: Some("s0-life-2px.png".into()),
                    own: false,
                }],
                gpu_timing: GpuTiming::Kernel,
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
        // A tuning saved before the sweep (one size a scene, its numbers beside it) still reads,
        // and its size still applies.
        let old: Tuning = serde_json::from_str(
            r#"{"rotation": "r", "canvas": [2560, 1440], "scenes": [{"scene": "a",
                "design_px": 3.0, "cell_px": 5.0, "cells": 147456, "cpu_percent": 2.5,
                "gpu_percent": 4.0, "p95_ms": 6.0, "on_gpu": true, "gpu_timing": "kernel",
                "fits": true}]}"#,
        )
        .unwrap();
        assert_eq!(
            old.size_for("a", 3.0, [2560.0, 1440.0]),
            Some(Size::px(5.0))
        );
        assert!(!old.scenes[0].chosen && old.scenes[0].rungs.is_empty());
    }

    /// With *Fit to this computer* switched off (0.4.2, #112), the sizes a release candidate's Fit
    /// saved are read and written back as they were, for 0.4.3, but not applied: each scene shows
    /// at its own size. (With Fit back on, `tuning_applied` is `tuning_for`.)
    #[test]
    fn a_saved_fit_is_kept_but_not_applied_while_fit_is_switched_off() {
        let text = r#"{"screens": [{"rotation": "r", "place": "wallpaper", "tuning": {
            "rotation": "r", "canvas": [2560, 1440], "scale": 1.0,
            "scenes": [{"scene": "a", "design_px": 3.0, "cell_px": 5.0, "chosen": true,
                        "recommended": {"px": 5.0}}]}}],
            "tune": {"cpu_percent": 5.0, "gpu_percent": 40.0, "auto": true}}"#;
        let s: HostSettings = serde_json::from_str(text).unwrap();
        let s = s.checked();
        assert!(s.tuning_for(0, "r").is_some(), "read as saved");
        assert_eq!(
            s.tuning_applied(0, "r").is_some(),
            jetcarp_core::fit::FIT,
            "applied only while Fit is in this release"
        );
        // Written back whole, limits and all.
        let back: HostSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.clone().checked(), s);
        assert_eq!(back.screens[0].tuning, s.screens[0].tuning);
        assert_eq!(back.tune.cpu_percent, 5.0);
    }

    /// A size chosen under another rotation stays saved, but only the scenes of the rotation shown
    /// now get rows in the cards: the Acer's hat scene, fitted by itself for a one-scene rotation,
    /// showed under "Until it settles" (#73).
    #[test]
    fn only_the_rotation_shown_has_rows_in_the_cards() {
        let scene = |name: &str, chosen: bool| TunedScene {
            scene: name.into(),
            chosen,
            ..Default::default()
        };
        let t = Tuning {
            rotation: "Until it settles".into(),
            scenes: vec![
                scene("JACA war on the hat tiling", true),
                scene("Quick Start rule:B3/S23", false),
                scene("Quick Start rule:B36/S23", false),
            ],
            ..Default::default()
        };
        let labels: Vec<String> = [
            "Quick Start rule:B3/S23",
            "Quick Start rule:B36/S23",
            "Quick Start rule:B368/S245",
        ]
        .map(String::from)
        .to_vec();
        let shown: Vec<&str> = t
            .scenes_shown(Some(&labels))
            .map(|s| s.scene.as_str())
            .collect();
        assert_eq!(
            shown,
            ["Quick Start rule:B3/S23", "Quick Start rule:B36/S23"]
        );
        assert_eq!(t.scenes.len(), 3, "the other rotation's choice stays saved");
        // A rotation that can't be read now: every scene, as before.
        assert_eq!(t.scenes_shown(None).count(), 3);
        assert!(!Tuning::shows("JACA war on the hat tiling", Some(&labels)));
    }

    /// Every generation is shown unless the file says low power (#42); the pacing is written by
    /// name, and a screen's pacer carries it with the frame rate and its monitor's refresh.
    #[test]
    fn the_most_frames_a_second_default_to_60_and_null_means_the_refresh() {
        let s: HostSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            s.max_fps,
            Some(DEFAULT_MAX_FPS),
            "files from before have it"
        );
        let full: HostSettings = serde_json::from_str(r#"{"max_fps": null}"#).unwrap();
        assert_eq!(full.max_fps, None);
        assert_eq!(full.pacer(Some(144.0)).pace(144.0).fps, 144.0);
        assert_eq!(s.pacer(Some(144.0)).pace(144.0).fps, 60.0);
        let text = serde_json::to_string(&full).unwrap();
        assert!(text.contains(r#""max_fps":null"#), "{text}");
    }

    #[test]
    fn the_pacing_defaults_to_every_generation_and_is_read_by_name() {
        let s: HostSettings = serde_json::from_str(r#"{"fps": 24}"#).unwrap();
        assert_eq!(s.pacing, Pacing::EveryGeneration);
        assert_eq!(HostSettings::default().pacing, Pacing::EveryGeneration);
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains(r#""pacing":"every_generation""#), "{text}");
        let low: HostSettings = serde_json::from_str(r#"{"pacing": "low_power"}"#).unwrap();
        assert_eq!(low.pacing, Pacing::LowPower);
        let back: HostSettings =
            serde_json::from_str(&serde_json::to_string(&low).unwrap()).unwrap();
        assert_eq!(back, low);
        let p = s.pacer(Some(144.0));
        assert_eq!(
            (p.pacing, p.fps, p.refresh),
            (Pacing::EveryGeneration, 24.0, Some(144.0))
        );
        assert_eq!(p.pace(60.0).fps, 60.0);
        assert_eq!(low.pacer(Some(144.0)).pace(60.0).fps, 30.0);
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
        let px = |label: &str, design: f32, w: f64, h: f64| {
            t.size_for(label, design, [w, h]).map(|s| s.px)
        };
        assert_eq!(px("a", 2.0, 2000.0, 1000.0), Some(6.0));
        assert_eq!(px("b", 3.0, 2000.0, 1000.0), None, "its own size");
        assert_eq!(px("c", 3.0, 2000.0, 1000.0), None, "not tuned");
        assert_eq!(
            px("a", 4.0, 2000.0, 1000.0),
            None,
            "the scene's size changed since"
        );
        // Four times the pixels: twice the size, the same cells.
        assert_eq!(px("a", 2.0, 4000.0, 2000.0), Some(12.0));
        // A quarter of the pixels: half the size, but never below a pixel.
        assert_eq!(px("a", 2.0, 1000.0, 500.0), Some(3.0));
        assert_eq!(px("a", 2.0, 500.0, 250.0), None, "back to its own 2 px");
        // Finer than its own, and a patch's level, apply too.
        let mut t = t.clone();
        t.scenes[1].cell_px = 1.0;
        assert_eq!(t.size_for("b", 3.0, [2000.0, 1000.0]), Some(Size::px(1.0)));
        t.scenes.push(TunedScene {
            scene: "p".into(),
            design_px: 2.4,
            cell_px: 1.7,
            level: Some(6),
            ..Default::default()
        });
        let patch = t.size_for("p", 2.4, [500.0, 250.0]).unwrap();
        assert_eq!(
            patch.level,
            Some(6),
            "a patch keeps its level on any canvas"
        );
        assert_eq!(patch.describe(), "level 6 (1.7 px)");
        assert!(Size::px(3.0).same(&Size::px(3.004)) && !Size::px(3.0).same(&patch));
        assert!(t.fits_canvas([2000.0, 1000.0]) && !t.fits_canvas([1920.0, 1080.0]));
        let l = TuneLimits::default();
        assert_eq!(
            l.target_fps(Some(144.0), 30.0),
            30.0,
            "at most the rate the scene draws at"
        );
        assert_eq!(l.target_fps(None, 120.0), 60.0);
        assert_eq!(l.target_fps(Some(75.0), 120.0), 75.0);
        let l = TuneLimits {
            fps: Some(24.0),
            ..l
        };
        assert_eq!(l.target_fps(Some(144.0), 30.0), 24.0);
    }
}
