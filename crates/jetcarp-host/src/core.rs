//! The host's work, whatever shows it: the stages, the link, the settings, the memory limit.

use jetcarp_core::photosensitivity::GuardPlace;
use jetcarp_display::autostart;
use jetcarp_display::host_settings::{HostSettings, LockScreen, Place, ScreenSettings};
use jetcarp_display::pace::FrameClock;
use jetcarp_display::rotation::Rotation;
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Lookup, Stage};
use jetcarp_display::tuning::{Pick, TRIAL_SECONDS, TuneEvent, TuneRequest};

mod sweep;
use jetcarp_gpu::select;
use jetcarp_mcp::link::{HOST_ENDPOINT, HOST_TOOL, Listener, Pending};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Canvas until a surface says otherwise.
const DEFAULT_CANVAS: [f64; 2] = [1280.0, 720.0];
/// Frames per second of a paused screen, and how often the host looks at its link and its
/// monitors at least.
const PAUSED_FPS: f32 = 5.0;
/// Share of a frame spent stepping; the rest is for drawing.
pub(crate) const STEP_SHARE: f32 = 0.6;
const MEMORY_CHECK: f64 = 2.0;
/// How often the battery and the processor load are looked at.
const POWER_CHECK: f64 = 5.0;
/// Log lines kept for `status`.
const LOG_LINES: usize = 40;
/// How long a starting host waits for GNOME's extension before showing plain windows.
const EXTENSION_WAIT: f64 = 20.0;
/// Silence from the extension for this long means it has gone.
const EXTENSION_SILENCE: f64 = 10.0;
/// Longest a quitting host waits for its answers to be written back.
const QUIT_GRACE: f64 = 1.0;
/// A tuner that has not renewed its hold for this long has gone: the screens carry on.
const TUNING_SILENCE: f64 = 30.0;
/// The monitors must stay as they are this long before the host tunes again for them.
const RETUNE_SETTLE: f64 = 15.0;
/// The hold that keeps every screen still until the photosensitive seizure warning has been
/// accepted (#96).
pub(crate) const WARNING_HOLD: &str = "warning";
/// How often a host held by the warning looks for an answer.
const WARNING_CHECK: f64 = 2.0;
/// How often the host reads the strobe guard's setting again.
const GUARD_CHECK: f64 = 2.0;
/// The hold that keeps every screen still while JETCARP measures this computer (#301).
pub(crate) const MEASURING_HOLD: &str = "measuring";
/// How often the host looks whether JETCARP is measuring.
const MEASURING_CHECK: f64 = 0.5;
/// What a host held by the warning prints on standard error as it starts.
const WARNING_SAID: &str = "nothing runs until the photosensitive seizure warning has been \
                            accepted: open the JETCARP app, or run `jetcarp setup`";

/// Whether wallpapers can be shown, as far as the host knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shell {
    /// Just started: GNOME's extension may still say hello.
    Waiting,
    Alive,
    Gone,
}

/// The GPU device the stages and surfaces share.
#[derive(Clone)]
pub struct SharedGpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The adapter asked for, when it was not found and this one was chosen instead.
    pub fallback: Option<String>,
}

pub use jetcarp_display::painter::device_descriptor;

impl SharedGpu {
    /// A device with no surface, on the adapter the GPU policy chooses for a display (None when
    /// the machine has no GPU).
    pub fn headless() -> Option<SharedGpu> {
        SharedGpu::headless_with(wgpu::Features::empty())
    }

    /// The same, with those of `optional` the adapter has (the tuner's timestamp queries).
    pub fn headless_with(optional: wgpu::Features) -> Option<SharedGpu> {
        let instance = select::new_instance();
        let choice = select::pick(&instance, select::Purpose::Display, None).ok()?;
        let mut g = SharedGpu::on_with(choice.adapter, optional)?;
        g.fallback = choice.fallback;
        Some(g)
    }

    /// A device on this adapter, with those of `optional` it has.
    fn on_with(adapter: wgpu::Adapter, optional: wgpu::Features) -> Option<SharedGpu> {
        let mut desc = device_descriptor(&adapter);
        desc.required_features |= adapter.features() & optional;
        let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
        Some(SharedGpu {
            adapter,
            device,
            queue,
            fallback: None,
        })
    }
}

pub struct Screen {
    pub stage: Stage,
    pub sim: SimBackend,
    /// The monitor this screen belongs on.
    pub monitor: Option<usize>,
    pub place: Place,
    /// The rotation and its items as the stage was built from them ([`made_from`]): a reload
    /// keeps the stage only while they are the same, so a rotation or profile saved since shows.
    made_from: String,
    /// When its next frame is due: each screen draws at its own rate (its stage's pace, or
    /// [`PAUSED_FPS`] while paused).
    clock: FrameClock,
    /// The last [`Core::tick`] stepped it: its surfaces draw this frame.
    pub due: bool,
    /// Frames it has drawn.
    frames: u64,
    /// No surface shows it: its monitor isn't connected, and another screen is on the one it
    /// would move to. It neither steps nor draws until its monitor comes back (#130).
    pub unseen: bool,
    /// This frame's work so far, seconds: its stepping, and its surfaces' drawing as the window
    /// side reports it ([`Core::painted`]); not the wait to present (#106).
    work: f64,
}

/// A rotation and its expanded items, as text to compare.
fn made_from(r: &Rotation, slots: &[jetcarp_display::rotation::Slot]) -> String {
    format!(
        "{}\n{}",
        r.to_json(),
        serde_json::to_string(slots).unwrap_or_default()
    )
}

impl Screen {
    fn new(
        stage: Stage,
        sim: SimBackend,
        monitor: Option<usize>,
        place: Place,
        made_from: String,
    ) -> Screen {
        Screen {
            stage,
            sim,
            monitor,
            place,
            made_from,
            unseen: false,
            clock: FrameClock::default(),
            due: false,
            frames: 0,
            work: 0.0,
        }
    }

    /// The frame rate it draws at now.
    fn rate(&self) -> f32 {
        if self.stage.paused() {
            PAUSED_FPS
        } else {
            self.stage.pace().fps
        }
    }
}

pub struct Core {
    pub settings: HostSettings,
    pub screens: Vec<Screen>,
    /// A one-run override of the first screen's rotation.
    rotation_override: Option<String>,
    /// None in a screen saver, which nothing controls.
    listener: Option<Listener>,
    gpu: Option<SharedGpu>,
    /// Whether the GPU has been looked for yet, found, not found, or lost, so `status` tells a
    /// host that hasn't opened it yet (started at login, nothing shown) from one with none (#141).
    gpu_state: GpuState,
    /// The desktop background's GPU policy as last read (its own choice, the shared setting, or
    /// `JETCARP_GPU`), read again with the settings, so `status` says what each screen steps on
    /// now and why (#75).
    policy: select::Policy,
    /// The log line naming the GPU and the policy, taken back when the policy changes.
    gpu_note: Option<String>,
    lookup: Lookup,
    started: Instant,
    last_memory_check: f64,
    /// When the host last looked whether JETCARP is measuring this computer.
    measuring_at: f64,
    /// The host itself, not a screen saver the system started: only it follows the pause rules.
    watch_power: bool,
    /// The host itself: only it touches the desktop's own picture.
    is_host: bool,
    /// The live wallpaper is wanted but can't be shown (told by the window side).
    pub wallpaper_failing: bool,
    /// The item a still was last left for, while the live wallpaper failed.
    still_for: Option<(String, usize)>,
    still_at: f64,
    power_at: f64,
    cpu: crate::power::CpuMeter,
    busy: crate::power::BusyRule,
    canvas: [f64; 2],
    /// Screen 0's pixels per point, for stages made before the window says.
    scale: f64,
    log: VecDeque<String>,
    /// Paused from the menu, the CLI, or the app.
    user_paused: bool,
    /// Why nothing need be drawn right now, as reported by GNOME's extension.
    holds: BTreeSet<String>,
    /// The same, found out by the host itself.
    local_holds: BTreeSet<&'static str>,
    /// How the desktop's windows are arranged, where the host has to care (Windows).
    pub desktop_layout: Option<&'static str>,
    /// When the extension was last heard from.
    extension_seen: Option<f64>,
    /// Names of the monitors, in the OS's order.
    pub monitors: Vec<String>,
    /// Their bounds, in physical pixels of the desktop (empty when headless).
    pub monitor_rects: Vec<jetcarp_display::workarea::Rect>,
    /// Set when the screens were rebuilt, so surfaces match them again.
    pub screens_changed: bool,
    /// Started at login: only the screens placed on the desktop show and run; the others wait,
    /// unseen, so a sign-in opens no windows.
    pub login: bool,
    /// Asked to quit; the host ends once [`Core::finished`] says so.
    pub quit: bool,
    quit_at: Option<f64>,
    /// Each monitor's refresh rate, in the OS's order (None: not known).
    pub monitor_hz: Vec<Option<f32>>,
    /// When a tuner last renewed its `tuning` hold.
    tuning_at: Option<f64>,
    /// A sweep this host is making on the real screen (for `tune`, or because the monitors
    /// changed).
    sweep: Option<sweep::HostSweep>,
    /// What the last sweep reported, for `tune_events`.
    sweep_events: Vec<TuneEvent>,
    /// How the last sweep ended, for `status`.
    sweep_last: Option<Value>,
    /// Since when the tuned screens' sizes have differed from their tuning.
    retune_since: Option<f64>,
    /// The sizes the host last tuned again for by itself, so it tries each arrangement once.
    retune_tried: Option<Vec<(usize, [u32; 2])>>,
    /// When the host last looked for an answer to the photosensitive seizure warning.
    warning_at: f64,
    /// The strobe guard's setting for the desktop background and the screen saver (#97), and
    /// when it was last read.
    desktop_guard: bool,
    /// The saved answer is *at risk*: the guard's stricter stripe rule.
    desktop_at_risk: bool,
    guard_at: f64,
    /// The caption's font, as the app's font setting names it, read again with the strobe
    /// guard's setting (every [`GUARD_CHECK`] seconds), so a font chosen in the app shows
    /// without a restart.
    caption_font: jetcarp_display::caption::FontWatch,
    /// The macOS screen saver, built again when what it bakes changes (#287).
    saver: jetcarp_display::saver_update::Keeper,
}

/// A running screen's monitor, as a fit by itself compares it with the one it was fitted for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub place: Place,
    /// Its canvas, in physical pixels.
    pub canvas: [f64; 2],
    /// Pixels per point.
    pub scale: f64,
    /// Its monitor's refresh rate (None: not known).
    pub hz: Option<f32>,
    /// It shows.
    pub shown: bool,
}

/// The fitted desktop background screens whose monitor is not the one their fit was found for (in
/// size, pixels per point or refresh rate), with the canvas each has now: the screens a fit by
/// itself sweeps; the others keep their fit (#110).
pub fn retune_wanted(settings: &HostSettings, screens: &[Monitor]) -> Vec<(usize, [u32; 2])> {
    screens
        .iter()
        .enumerate()
        .filter(|(_, m)| m.place == Place::Wallpaper && m.shown)
        .filter_map(|(i, m)| {
            let s = settings.screens.get(i)?;
            let t = s.tuning.as_ref().filter(|t| t.rotation == s.rotation)?;
            (!t.fits_monitor(m.canvas, m.scale, m.hz))
                .then(|| (i, [m.canvas[0].round() as u32, m.canvas[1].round() as u32]))
        })
        .collect()
}

/// The wallpaper screen monitor `i` of `count` showed: the one placed on it, else one across every
/// monitor, else (on the primary) one whose monitor has gone, as the live wallpaper does.
fn still_screen<'a>(
    walls: &[&'a Screen],
    i: usize,
    count: usize,
    primary: usize,
) -> Option<&'a Screen> {
    walls
        .iter()
        .find(|s| s.monitor == Some(i))
        .or_else(|| walls.iter().find(|s| s.monitor.is_none()))
        .or_else(|| {
            (i == primary)
                .then(|| walls.iter().find(|s| s.monitor.is_some_and(|m| m >= count)))
                .flatten()
        })
        .copied()
}

/// Why the screens are paused, in words, for `status`'s `paused_why`: by hand, or, with nothing
/// holding them, that nothing in the rotation can be shown. Both have to be resumed. Empty while
/// they run or are only held, so it agrees with `paused` (#76); a hold is in `held_why`.
pub fn pause_reasons(by_hand: bool, held: bool, all_paused: bool) -> Vec<String> {
    if by_hand {
        vec!["by hand".into()]
    } else if !held && all_paused {
        vec!["nothing in the rotation can be shown".into()]
    } else {
        vec![]
    }
}

/// What holds the screens, in words, for `status`'s `held_why` (empty when nothing does). A hold
/// passes by itself: the screens carry on once the battery, the busy processor, the covered or
/// dark screen, or the tuner, has gone.
pub fn hold_reasons<'a>(holds: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut why: Vec<String> = vec![];
    for h in holds {
        let words = match h {
            "battery" => "on battery".to_string(),
            "busy" => "the processor is busy".into(),
            "covered" => "windows cover the desktop".into(),
            "tuning" => "tuning the desktop background for this computer".into(),
            MEASURING_HOLD => "JETCARP is measuring this computer".into(),
            "dark" => "the screen is locked or off".into(),
            WARNING_HOLD => jetcarp_core::photosensitivity::HOST_HELD.into(),
            other => format!("the desktop asked ({other})"),
        };
        if !why.contains(&words) {
            why.push(words);
        }
    }
    why
}

/// The start of a wallpaper window's title; GNOME's extension looks for it.
const WALLPAPER_TAG: &str = "@jetcarp-wallpaper";

// Give the memory allocator's free pages back to the system, on Linux and macOS: what the host
// frees (a sweep's trial grids of up to millions of cells, a scene let go) would otherwise stay
// counted as its memory (#69, #111). Called after a fit, and before the memory limit lets a scene
// go, so memory the host no longer holds is never counted against it.
pub(crate) use jetcarp_core::machine::pool::release_free_memory;

fn gnome_session() -> bool {
    cfg!(all(unix, not(target_os = "macos")))
        && std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|d| d.to_ascii_uppercase().contains("GNOME"))
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// The rotation called `name`, ready to run. An unknown name is an error, not a silent default.
pub fn load_rotation(
    name: &str,
) -> Result<(Rotation, Vec<jetcarp_display::rotation::Slot>), String> {
    let (r, slots, warnings) = jetcarp_display::rotation::load_stored(name)?;
    for w in warnings {
        say!("jetcarp-host: {name}: {w}");
    }
    Ok((r, slots))
}

/// Every rotation that can be chosen.
pub fn rotation_names() -> Vec<String> {
    jetcarp_display::rotation::stored_names()
}

impl Core {
    pub fn new(rotation: Option<String>, canvas: Option<[f64; 2]>) -> Result<Core, String> {
        let (settings, warning) = HostSettings::load();
        let listener = Listener::start_as(0, HOST_ENDPOINT, Arc::new(|| {}))
            .map_err(|e| format!("could not open the control link: {e}"))?;
        let mut core = Core::build(settings, warning, rotation, canvas, Some(listener))?;
        // The host's own tests turn the rules off, so a busy test machine doesn't pause them.
        core.watch_power = std::env::var_os("JETCARP_HOST_NO_PAUSE_RULES").is_none();
        core.is_host = true;
        core.check_warning();
        core.settle_still();
        // The desktop background holds still while JETCARP measures this computer (#301).
        jetcarp_core::machine::quiet::honour();
        Ok(core)
    }

    /// The photosensitive seizure warning (#96): until it has been accepted, every screen is held
    /// at its first frame, so nothing animates, and the status says why. A held host looks again
    /// every few seconds ([`WARNING_CHECK`]), so an answer given in the app or with `jetcarp
    /// setup` starts the scenes.
    fn check_warning(&mut self) {
        let held = self.local_holds.contains(WARNING_HOLD);
        let accepted = jetcarp_core::photosensitivity::accepted();
        if held && accepted {
            self.note("the photosensitive seizure warning has been accepted; the scenes run");
            self.set_local_hold(WARNING_HOLD, false);
        } else if !held && !accepted {
            // The status says it as a hold (`held_why`); the log on standard error says it too.
            say!("jetcarp-host: {WARNING_SAID}");
            self.set_local_hold(WARNING_HOLD, true);
        }
    }

    /// The screen saver: one scene, no link, and nothing written back to the settings.
    pub fn for_saver() -> Result<Core, String> {
        let (mut settings, warning) = HostSettings::load();
        let rotation = settings
            .saver_rotation
            .clone()
            .unwrap_or_else(|| settings.screens[0].rotation.clone());
        let rotation = if load_rotation(&rotation).is_ok() {
            rotation
        } else {
            ScreenSettings::default().rotation
        };
        settings.screens = vec![ScreenSettings {
            rotation,
            ..Default::default()
        }];
        // So does the screen saver (#301).
        jetcarp_core::machine::quiet::honour();
        Core::build(settings, warning, None, None, None)
    }

    fn build(
        settings: HostSettings,
        warning: Option<String>,
        rotation: Option<String>,
        canvas: Option<[f64; 2]>,
        listener: Option<Listener>,
    ) -> Result<Core, String> {
        let lookup = jetcarp_display::stage::library_lookup();
        let policy = settings.gpu_policy();
        let (caption_font, font_note) =
            jetcarp_display::caption::FontWatch::new(jetcarp_core::paths::config_dir());
        // The caption's last resort, as the app's: the installed fonts' character maps, read on a
        // thread of their own so no frame waits. Tests and screenshot runs read none of this
        // machine's fonts, so they look the same everywhere.
        let font_dirs = if cfg!(test) || std::env::var_os("JETCARP_SCREENSHOT").is_some() {
            vec![]
        } else {
            jetcarp_display::fonts::system_dirs()
        };
        let caption_font =
            caption_font.with_last_resort(jetcarp_display::caption::LastResort::start(font_dirs));
        let mut core = Core {
            settings,
            screens: vec![],
            rotation_override: rotation,
            listener,
            gpu: None,
            gpu_state: GpuState::NotOpened,
            policy,
            gpu_note: None,
            lookup,
            started: Instant::now(),
            last_memory_check: 0.0,
            measuring_at: f64::NEG_INFINITY,
            watch_power: false,
            is_host: false,
            wallpaper_failing: false,
            still_for: None,
            still_at: f64::NEG_INFINITY,
            power_at: f64::NEG_INFINITY,
            cpu: Default::default(),
            busy: Default::default(),
            canvas: canvas.unwrap_or(DEFAULT_CANVAS),
            scale: 1.0,
            log: VecDeque::new(),
            user_paused: false,
            holds: BTreeSet::new(),
            local_holds: BTreeSet::new(),
            desktop_layout: None,
            extension_seen: None,
            monitors: vec![],
            monitor_rects: vec![],
            screens_changed: false,
            login: false,
            quit: false,
            quit_at: None,
            monitor_hz: vec![],
            tuning_at: None,
            sweep: None,
            sweep_events: vec![],
            sweep_last: None,
            retune_since: None,
            retune_tried: None,
            warning_at: 0.0,
            desktop_guard: jetcarp_core::photosensitivity::guard_on(GuardPlace::Desktop),
            desktop_at_risk: jetcarp_core::photosensitivity::at_risk(),
            guard_at: 0.0,
            caption_font,
            saver: Default::default(),
        };
        if let Some(w) = warning {
            core.note(w);
        }
        if let Some(n) = font_note {
            core.note(n);
        }
        core.build_screens()?;
        Ok(core)
    }

    pub fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        say!("jetcarp-host: {line}");
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// Takes back a note that is no longer true.
    pub fn retract(&mut self, line: &str) {
        self.log.retain(|l| l != line);
    }

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// A simulator on the shared device, stepping as the desktop background's GPU choice says.
    fn new_sim(&self) -> SimBackend {
        match &self.gpu {
            Some(g) => SimBackend::from_device_with(
                &g.adapter,
                g.device.clone(),
                g.queue.clone(),
                self.policy.clone(),
            ),
            None => SimBackend::unavailable(self.gpu_state.sim_reason().into()),
        }
    }

    /// A stage running the rotation called `name`, as it is now, and what it is made from.
    fn new_stage(&self, name: &str, canvas: [f64; 2]) -> Result<(Stage, String), String> {
        let (r, slots) = load_rotation(name)?;
        let from = made_from(&r, &slots);
        let mut stage = Stage::new(
            &r,
            slots,
            seed(),
            canvas,
            self.lookup.clone(),
            self.settings.cell_limit(),
        )?;
        stage.resize(canvas, self.scale, self.now());
        Ok((stage, from))
    }

    fn wanted_rotation(&self, i: usize) -> String {
        match (&self.rotation_override, i) {
            (Some(r), 0) => r.clone(),
            _ => self.settings.screens[i].rotation.clone(),
        }
    }

    /// Makes the running screens match the settings, keeping those that did not change: the same
    /// rotation, with the same items and profiles (one saved since is shown afresh).
    fn build_screens(&mut self) -> Result<(), String> {
        let mut old: Vec<Option<Screen>> = std::mem::take(&mut self.screens)
            .into_iter()
            .map(Some)
            .collect();
        let mut out = Vec::with_capacity(self.settings.screens.len());
        let mut first_error = None;
        // A screen a fit is measuring shows a trial's size; its own sizes come back with its own
        // stage when the fit ends.
        let fitting: Vec<usize> = self.sweep.as_ref().map_or(vec![], |s| s.fitting());
        for i in 0..self.settings.screens.len() {
            let want = self.wanted_rotation(i);
            let (monitor, place) = (
                self.settings.screens[i].monitor,
                self.settings.screens[i].place,
            );
            // None while Fit is switched off (#112): each scene at its own size.
            let tuned = self.settings.tuning_applied(i, &want).cloned();
            let kept = old.get_mut(i).and_then(Option::take);
            // One that can no longer be read keeps going as it is.
            let unchanged = |s: &Screen| {
                s.stage.rotation() == want
                    && jetcarp_display::rotation::load_stored(&want)
                        .ok()
                        .is_none_or(|(r, slots, _)| made_from(&r, &slots) == s.made_from)
            };
            if let Some(mut s) = kept.filter(unchanged) {
                s.monitor = monitor;
                s.place = place;
                if !fitting.contains(&i) {
                    s.stage.set_tuned(tuned);
                }
                out.push(s);
                continue;
            }
            match self.new_stage(&want, self.canvas) {
                Ok((mut stage, made_from)) => {
                    stage.set_tuned(tuned);
                    out.push(Screen::new(
                        stage,
                        self.new_sim(),
                        monitor,
                        place,
                        made_from,
                    ));
                }
                Err(e) => {
                    self.note(format!("screen {}: {e}", i + 1));
                    first_error.get_or_insert(e);
                }
            }
        }
        self.screens = out;
        self.screens_changed = true;
        self.apply_pause();
        match (self.screens.is_empty(), first_error) {
            (true, Some(e)) => Err(e),
            _ => Ok(()),
        }
    }

    /// The GPU is ready: stages step on it from now on.
    pub fn attach_gpu(&mut self, gpu: Option<SharedGpu>) {
        if gpu.is_none() {
            self.note("no GPU found; running on the CPU");
        }
        self.gpu_state = if gpu.is_some() {
            GpuState::Found
        } else {
            GpuState::NoneFound
        };
        self.gpu = gpu;
        self.note_gpu();
        for i in 0..self.screens.len() {
            self.screens[i].sim = self.new_sim();
        }
    }

    /// Notes the GPU and the policy it steps by, in place of the line noted before: the policy
    /// changes when its settings do, and the start-up line would go on saying the old one (#75).
    fn note_gpu(&mut self) {
        let Some(g) = &self.gpu else { return };
        let fallback = g.fallback.as_ref().map(|f| format!(" ({f})"));
        let line = format!(
            "GPU: {}{}; GPU simulation {}",
            jetcarp_gpu::describe(&g.adapter.get_info()),
            fallback.unwrap_or_default(),
            self.policy.describe()
        );
        if let Some(old) = self.gpu_note.take() {
            self.retract(&old);
        }
        self.gpu_note = Some(line.clone());
        self.note(line);
    }

    /// The GPU was lost (a driver reset, sometimes after sleep): every world is built again, and
    /// stepped on the CPU until a new device is attached.
    pub fn gpu_lost(&mut self) {
        self.gpu = None;
        self.gpu_state = GpuState::Lost;
        for s in &mut self.screens {
            s.stage.rebuild();
        }
        for i in 0..self.screens.len() {
            self.screens[i].sim = self.new_sim();
        }
    }

    pub fn all_paused(&self) -> bool {
        self.screens.iter().all(|s| s.stage.paused())
    }

    fn apply_pause(&mut self) {
        let paused = self.user_paused || self.held();
        // A fit runs the screens it measures whatever else holds them, and pauses the others, so
        // the host's load is the measured scenes'.
        let on_show = self.sweep.as_ref().map(|s| s.screens());
        for (i, s) in self.screens.iter_mut().enumerate() {
            s.stage.set_paused(match &on_show {
                Some(on) => !on.contains(&i),
                None => paused,
            });
        }
    }

    /// Whether something says nothing need be drawn right now.
    pub fn held(&self) -> bool {
        !self.holds.is_empty() || !self.local_holds.is_empty()
    }

    /// The holds' names: GNOME's extension's, then the host's own.
    fn hold_names(&self) -> impl Iterator<Item = &str> {
        self.holds
            .iter()
            .map(String::as_str)
            .chain(self.local_holds.iter().copied())
    }

    /// Something the host noticed for itself means nothing need be drawn, or no longer does.
    pub fn set_local_hold(&mut self, name: &'static str, on: bool) {
        let changed = if on {
            self.local_holds.insert(name)
        } else {
            self.local_holds.remove(name)
        };
        if changed {
            self.apply_pause();
        }
    }

    pub fn wants_wallpaper(&self) -> bool {
        self.screens.iter().any(|s| s.place == Place::Wallpaper)
    }

    /// Whether wallpapers can be shown: on GNOME, whether the extension is there.
    pub fn shell(&self) -> Shell {
        // Windows and macOS need no helper: the host places its own windows under the icons.
        if cfg!(any(windows, target_os = "macos")) {
            return Shell::Alive;
        }
        let now = self.now();
        match self.extension_seen {
            Some(t) if now - t < EXTENSION_SILENCE => Shell::Alive,
            Some(_) => Shell::Gone,
            None if gnome_session() && now < EXTENSION_WAIT => Shell::Waiting,
            None => Shell::Gone,
        }
    }

    /// The title of a wallpaper's window, which tells the extension what to do with it.
    pub fn wallpaper_title(&self, i: usize) -> String {
        let all = self.screens.get(i).is_none_or(|s| s.monitor.is_none());
        let lock = self.settings.lock_screen;
        let awake = if lock == LockScreen::Screensaver {
            self.settings.lock_awake_minutes
        } else {
            0
        };
        format!(
            "{WALLPAPER_TAG}{}",
            json!({"all": all, "lock": lock.word(), "awake": awake})
        )
    }

    /// The extension's heartbeat, and what it says need not be drawn.
    fn heard_extension(&mut self, holds: &Value) {
        self.extension_seen = Some(self.now());
        let now_held: BTreeSet<String> = holds
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, on)| **on == Value::Bool(true))
            .map(|(k, _)| k.clone())
            .collect();
        if now_held != self.holds {
            self.holds = now_held;
            self.apply_pause();
        }
    }

    /// Forgets holds nobody is left to release.
    fn expire_holds(&mut self) {
        if !self.holds.is_empty() && self.shell() != Shell::Alive {
            self.holds.clear();
            self.apply_pause();
        }
    }

    pub fn open_app(&self) -> Result<(), String> {
        autostart::app_command().and_then(|c| c.spawn_detached())
    }

    pub fn set_login(&self, on: bool) -> Result<(), String> {
        autostart::set(on, &autostart::login_command()?)
    }

    /// Whether screen `s` is left unseen: a window screen of a host started at login, or a
    /// screen whose monitor isn't connected and has nowhere else to show (#130).
    pub fn hidden(&self, s: &Screen) -> bool {
        (self.login && s.place != Place::Wallpaper) || s.unseen
    }

    /// Shows every screen as a wallpaper, or back in a window (saved).
    pub fn set_wallpaper(&mut self, on: bool) -> Result<(), String> {
        let place = if on { Place::Wallpaper } else { Place::Window };
        for s in &mut self.settings.screens {
            s.place = place;
        }
        self.settings.save()?;
        let built = self.build_screens();
        self.settle_still();
        built
    }

    /// Asked to quit, every answer (including the one to "quit") has gone back, and the callers
    /// have hung up (or `QUIT_GRACE` has passed). Waiting for the callers to close first matters
    /// on Windows: a process that exits with a connection still open resets it, and the caller
    /// can lose an answer it has not read yet.
    pub fn finished(&mut self) -> bool {
        if !self.quit {
            return false;
        }
        let now = self.now();
        let since = *self.quit_at.get_or_insert(now);
        let settled = self
            .listener
            .as_ref()
            .is_none_or(|l| !l.busy() && l.connection_count() == 0);
        settled || now - since >= QUIT_GRACE
    }

    /// Puts the host's own things in order before the process ends ([`jetcarp_display::exit`]):
    /// the link closes, so its endpoint file goes. Nothing on the GPU is touched: the screens'
    /// buffers, the device and the instance all end with the process (dropping them, or waiting
    /// on the device, crashed Windows' software renderer on CI with 0xC0000005).
    pub fn shutdown(&mut self) {
        self.listener = None;
    }

    /// When the next frame is due: the soonest of the screens' (each draws at its own rate), and
    /// at least [`PAUSED_FPS`] times a second, so the link is answered while nothing draws.
    pub fn next_frame(&self) -> Instant {
        let now = self.now();
        let soonest = self
            .screens
            .iter()
            .filter(|s| !self.hidden(s))
            .map(|s| s.clock.next().unwrap_or(now))
            .fold(now + 1.0 / PAUSED_FPS as f64, f64::min);
        self.started + Duration::from_secs_f64(soonest.max(0.0))
    }

    /// The refresh rate of the monitor screen `i` shows on (the first monitor's for every
    /// monitor).
    fn screen_hz(&self, i: usize) -> Option<f32> {
        let m = self.screens.get(i).and_then(|s| s.monitor).unwrap_or(0);
        self.monitor_hz.get(m).copied().flatten()
    }

    /// One frame: answer the link, watch memory, and run each screen whose frame is due. Each
    /// screen draws at its own rate: as fast as its scene steps, between the frame rate setting
    /// and its monitor's refresh rate, or in low power at the frame rate (see
    /// [`jetcarp_display::pace`]). A screen's frames keep to its rate on their own clock, and a
    /// screen that is not due is neither stepped nor drawn, so one screen's 60 frames a second
    /// do not make another's 30 draw 60.
    pub fn tick(&mut self) {
        // A signal asked the host to stop: it stops as `jetcarp host stop` asks it to (#278).
        if crate::signalled() {
            self.quit = true;
        }
        let requests = self.listener.as_ref().map_or(vec![], |l| l.poll());
        for p in requests {
            self.answer(p);
        }
        self.expire_holds();
        let now = self.now();
        // While JETCARP measures this computer its screens hold still, so their load does not
        // lower the score; they carry on as they were after (#301).
        if now - self.measuring_at >= MEASURING_CHECK {
            self.measuring_at = now;
            self.set_local_hold(MEASURING_HOLD, jetcarp_core::machine::quiet::held());
        }
        if now - self.last_memory_check >= MEMORY_CHECK {
            self.last_memory_check = now;
            self.check_memory();
        }
        if self.watch_power && now - self.power_at >= POWER_CHECK {
            self.power_at = now;
            self.check_power();
        }
        if self.is_host && now - self.still_at >= POWER_CHECK {
            self.still_at = now;
            self.refresh_still();
        }
        if now - self.guard_at >= GUARD_CHECK {
            self.guard_at = now;
            let on = jetcarp_core::photosensitivity::guard_on(GuardPlace::Desktop);
            let strict = jetcarp_core::photosensitivity::at_risk();
            if strict != self.desktop_at_risk {
                self.desktop_at_risk = strict;
                self.note(if strict {
                    "the strobe guard is stricter for someone at risk: patterns of similar parts \
                     are veiled too"
                } else {
                    "the strobe guard's stripe rule is the usual one"
                });
            }
            if let Some(n) = self.caption_font.check() {
                self.note(n);
            }
            // The guard's setting and the warning's answer above, the rotation, the caption and
            // its font: whatever the Mac's sandboxed screen saver has baked in, it gets again.
            match self.saver.poll() {
                Some(Ok(n)) => self.note(n),
                Some(Err(e)) => self.note(format!(
                    "the screen saver couldn't be brought up to date ({e}); it shows what it did \
                     before"
                )),
                None => {}
            }
            if on != self.desktop_guard {
                self.desktop_guard = on;
                self.note(if on {
                    "the strobe guard is on for the desktop background and the screen saver"
                } else {
                    "the strobe guard was turned off for the desktop background and the screen saver"
                });
            }
        }
        if self.local_holds.contains(WARNING_HOLD) && now - self.warning_at >= WARNING_CHECK {
            self.warning_at = now;
            self.check_warning();
        }
        if self.tuning_at.is_some_and(|t| now - t > TUNING_SILENCE) {
            self.tuning_at = None;
            self.set_local_hold("tuning", false);
            self.note("the tuner stopped answering; carrying on");
        }
        self.sweep_step();
        self.watch_retune();
        let count = self.screens.len().max(1) as f32;
        let pacers: Vec<_> = (0..self.screens.len())
            .map(|i| self.settings.pacer(self.screen_hz(i)))
            .collect();
        let mut notes = vec![];
        let login = self.login;
        for (s, pacer) in self.screens.iter_mut().zip(pacers) {
            s.due = false;
            // Nobody can see it: neither stepped nor drawn (#130).
            if (login && s.place != Place::Wallpaper) || s.unseen {
                continue;
            }
            s.stage.set_pacer(pacer);
            let rate = s.rate();
            if !s.clock.tick(now, rate) {
                continue;
            }
            s.due = true;
            s.frames += 1;
            // Each screen's share of the time its frame leaves for stepping.
            let budget = Duration::from_secs_f32(STEP_SHARE / rate / count);
            let t0 = Instant::now();
            s.stage.tick(now, &mut s.sim, budget);
            s.work += t0.elapsed().as_secs_f64();
            notes.extend(s.stage.take_notes());
        }
        for n in notes {
            self.note(n);
        }
    }

    /// The fonts the caption is drawn in: the app's font setting's.
    pub fn caption_font(&self) -> &std::sync::Arc<jetcarp_display::caption::CaptionFont> {
        self.caption_font.font()
    }

    /// Whether the strobe guard is on for the desktop background and the screen saver.
    pub fn desktop_guard_on(&self) -> bool {
        self.desktop_guard
    }

    /// Whether the saved answer is *at risk*, so the guard uses its stricter stripe rule.
    pub fn desktop_guard_at_risk(&self) -> bool {
        self.desktop_at_risk
    }

    /// The pause rules: on battery, and while the processor is busy with something else.
    fn check_power(&mut self) {
        let battery = self.settings.pause_on_battery && crate::power::on_battery() == Some(true);
        self.set_local_hold("battery", battery);
        let Some(load) = self.cpu.sample() else {
            return;
        };
        let busy = self.busy.feed(load * 100.0, self.settings.busy_percent);
        self.set_local_hold("busy", busy);
    }

    /// Over the limit: give up on the biggest world.
    fn check_memory(&mut self) {
        let Some(b) = crate::power::memory_bytes() else {
            return;
        };
        let mut mb = b / (1 << 20);
        if mb <= self.settings.memory_limit_mb() {
            return;
        }
        // Over the limit: first give back what the allocator only keeps, and look again (#69).
        release_free_memory();
        if let Some(b) = crate::power::memory_bytes() {
            mb = b / (1 << 20);
        }
        if mb <= self.settings.memory_limit_mb() {
            return;
        }
        let biggest = self
            .screens
            .iter_mut()
            .filter(|s| s.stage.world().is_some())
            .max_by_key(|s| s.stage.world().map_or(0, |w| w.topo.len()));
        if let Some(s) = biggest {
            let limit = self.settings.memory_limit_mb();
            s.stage.abandon_item(&format!(
                "the host used {mb} MB, over its limit of {limit} MB"
            ));
        }
    }

    fn status(&self) -> Value {
        let now = self.now();
        let mut v = json!({
            "running": true,
            "pid": std::process::id(),
            "uptime_s": now.round(),
            "memory_mb": crate::power::memory_bytes().map(|b| b / (1 << 20)),
            // Paused as the user sees it: by hand, or a rotation with nothing it can show, both of
            // which have to be resumed. A hold (`held`, the holds' names, and `held_why`, the
            // same in words) passes by itself, so it is not a pause: each pair agrees (#76).
            "paused": self.user_paused || (!self.held() && self.all_paused()),
            "paused_why": pause_reasons(self.user_paused, self.held(), self.all_paused()),
            "held": self.hold_names().collect::<Vec<_>>(),
            "held_why": hold_reasons(self.hold_names()),
            "desktop_layout": self.desktop_layout,
            "wallpaper": self.wants_wallpaper(),
            "extension": self.shell() == Shell::Alive,
            "lock_screen": self.settings.lock_screen.word(),
            "monitors": self.monitors,
            "monitor_hz": self.monitor_hz,
            "gpu": self.gpu.as_ref().map(|g| g.adapter.get_info().name),
            "gpu_state": self.gpu_state.word(),
            "fps": self.settings.fps,
            "pacing": self.settings.pacing,
            "max_fps": self.settings.max_fps,
            "caption": self.settings.caption,
            "run_at_login": autostart::enabled(),
            // On unless turned off in the app's Settings → Safety; each screen's stage says what
            // it is doing there.
            "strobe_guard": json!({
                "on": self.desktop_guard,
                "for": GuardPlace::Desktop.words(),
                "at_risk": self.desktop_at_risk,
            }),
            "rotations": rotation_names(),
            "login": self.login,
            "screens": self.screens.iter().map(|s| json!({
                "monitor": s.monitor,
                "place": s.place,
                "hidden": self.hidden(s),
                "waiting_for_monitor": s.unseen,
                "frames": s.frames,
                "stage": s.stage.status(),
                // What its world steps on now, and why, as the settings say since they last
                // changed (#75).
                "simulation": s.sim.report(&self.policy.origin()),
            })).collect::<Vec<_>>(),
            "log": self.log,
        });
        // Nothing of fitting while Fit is switched off (#112).
        if jetcarp_core::fit::FIT {
            v["tune"] = json!(self.settings.tune);
            v["tuning"] = self.tuning_status();
        }
        v
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.user_paused = paused;
        self.apply_pause();
    }

    /// Moves every screen, or just one, by `by` items.
    pub fn skip(&mut self, by: i64, screen: Option<usize>) -> Result<(), String> {
        for (i, s) in self.screens.iter_mut().enumerate() {
            if screen.is_none_or(|k| k == i) {
                s.stage.skip(by)?;
            }
        }
        Ok(())
    }

    /// Runs rotation `name` on a screen (the first by default), and remembers the choice.
    pub fn set_rotation(&mut self, name: &str, screen: Option<usize>) -> Result<(), String> {
        let i = screen.unwrap_or(0);
        if i >= self.settings.screens.len() {
            return Err(format!("there is no screen {i}"));
        }
        load_rotation(name)?;
        self.settings.screens[i].rotation = name.to_string();
        if i == 0 {
            self.rotation_override = None;
        }
        self.settings.save()?;
        self.build_screens()
    }

    /// Reads the settings file again, the shared GPU setting, and this machine's limits (its
    /// cells and memory may have been changed in Tools → This computer… or `jetcarp limits`).
    pub fn reload(&mut self) -> Result<(), String> {
        jetcarp_core::machine::reload();
        let (settings, warning) = HostSettings::load();
        if let Some(w) = warning {
            self.note(w);
        }
        self.settings = settings;
        // The GPU choice may have changed: its own, or the shared setting it follows. Read before
        // the screens are built, so a new screen's simulator steps by it too.
        let policy = self.settings.gpu_policy();
        let changed = policy != self.policy;
        self.policy = policy;
        if changed {
            self.note_gpu();
        }
        let built = self.build_screens();
        let mode = self.policy.mode;
        for s in &mut self.screens {
            s.sim.mode = mode;
        }
        // The pacing at once, so the status a settings command answers with shows it (not only
        // from the next frame).
        let pacers: Vec<_> = (0..self.screens.len())
            .map(|i| self.settings.pacer(self.screen_hz(i)))
            .collect();
        for (s, pacer) in self.screens.iter_mut().zip(pacers) {
            s.stage.set_pacer(pacer);
        }
        self.settle_still();
        built
    }

    /// Whether a still of the scene should stand in for the live wallpaper.
    pub fn wants_still(&self) -> bool {
        jetcarp_display::desktop_picture::wanted(&self.settings)
    }

    /// Leaves a still of what each monitor showed as its desktop picture (where the system has
    /// one picture for every monitor, the primary monitor's), or of the first wallpaper screen's
    /// scene when the monitors are not known.
    pub fn leave_still(&mut self) -> Result<Vec<std::path::PathBuf>, String> {
        use jetcarp_display::desktop_picture::{self, Still};
        let none = "no scene to leave on the desktop";
        let walls: Vec<&Screen> = self
            .screens
            .iter()
            .filter(|s| s.place == Place::Wallpaper && s.stage.world().is_some())
            .collect();
        let first = walls.first().ok_or(none)?;
        let rects = &self.monitor_rects;
        // The primary monitor sits at 0, 0.
        let primary = rects.iter().position(|r| (r.x, r.y) == (0, 0)).unwrap_or(0);
        let mut stills = vec![];
        for (i, &r) in rects.iter().enumerate() {
            let Some(s) = still_screen(&walls, i, rects.len(), primary) else {
                continue;
            };
            let Some(image) = s.stage.still_at([r.w, r.h]) else {
                continue;
            };
            let still = Still {
                monitor: Some(r),
                png: still_png(s, &image)?,
            };
            // The primary monitor's first, for a system with one picture.
            if i == primary {
                stills.insert(0, still);
            } else {
                stills.push(still);
            }
        }
        if stills.is_empty() {
            let image = first.stage.still().ok_or(none)?;
            stills.push(Still {
                monitor: None,
                png: still_png(first, &image)?,
            });
        }
        desktop_picture::show_each(&stills)
    }

    /// Puts the user's desktop picture back once no still is wanted.
    fn settle_still(&mut self) {
        if !self.is_host {
            return;
        }
        match jetcarp_display::desktop_picture::settle(&self.settings) {
            Ok(jetcarp_display::desktop_picture::Restored::PutBack) => {
                self.note("put the desktop's own picture back")
            }
            Ok(_) => {}
            Err(e) => self.note(format!(
                "the desktop's own picture could not be put back: {e}"
            )),
        }
    }

    /// While the live wallpaper can't be shown, keeps stills of each new item on the desktop.
    fn refresh_still(&mut self) {
        if !self.is_host || !self.wallpaper_failing || !self.wants_still() {
            self.still_for = None;
            return;
        }
        let Some(s) = self.screens.iter().find(|s| s.place == Place::Wallpaper) else {
            return;
        };
        if s.stage.world().is_none() {
            return;
        }
        let st = s.stage.status();
        let key = Some((st.label, st.number));
        if key == self.still_for {
            return;
        }
        self.still_for = key;
        if let Err(e) = self.leave_still() {
            self.note(format!("no still picture: {e}"));
        }
    }

    fn act(&mut self, a: &Value) -> Result<Value, String> {
        let screen = a.get("screen").and_then(Value::as_u64).map(|s| s as usize);
        match a.get("action").and_then(Value::as_str).unwrap_or("status") {
            // Fit to this computer is switched off (#112): its sweeps, the news of one and a
            // tuner's hold are refused, however they are asked for.
            "tune" | "tune_stop" | "tune_events" | "tuning" if !jetcarp_core::fit::FIT => {
                return Err(jetcarp_core::fit::HELD_BACK.into());
            }
            "status" => {}
            "next" => self.skip(1, screen)?,
            "previous" => self.skip(-1, screen)?,
            "pause" => self.set_paused(true),
            "resume" => self.set_paused(false),
            "rotation" => {
                let name = a
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("`rotation` needs `name`")?;
                self.set_rotation(name, screen)?;
            }
            "reload" => self.reload()?,
            "extension" => {
                self.heard_extension(&a["holds"]);
                return Ok(json!({"ok": true}));
            }
            "wallpaper" => {
                let on = a.get("on").and_then(Value::as_bool).unwrap_or(true);
                self.set_wallpaper(on)?;
            }
            "open_app" => self.open_app()?,
            "tuning" => {
                let on = a.get("on").and_then(Value::as_bool).unwrap_or(true);
                self.tuning_at = on.then(|| self.now());
                self.set_local_hold("tuning", on);
                return Ok(json!({"ok": true}));
            }
            "tune" if a.get("on").and_then(Value::as_bool) == Some(false) => {
                self.stop_sweep("stopped")?
            }
            "tune" => {
                let num = |k: &str| a.get(k).and_then(Value::as_f64).map(|x| x as f32);
                let pick = match a.get("pick") {
                    Some(Value::String(t)) => Some(Pick::parse(t)?),
                    Some(Value::Number(n)) => Some(Pick::parse(&n.to_string())?),
                    _ => None,
                };
                let seconds = a
                    .get("seconds")
                    .and_then(Value::as_f64)
                    .unwrap_or(TRIAL_SECONDS);
                let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
                self.start_sweep(
                    TuneRequest {
                        cpu_percent: num("cpu_limit"),
                        gpu_percent: num("gpu_limit"),
                        fps: num("fps"),
                        screen,
                        screens: vec![],
                        pick,
                        fresh: flag("fresh"),
                        dry_run: flag("dry_run"),
                        auto: false,
                    },
                    seconds,
                )?;
            }
            "tune_stop" => self.stop_sweep("stopped")?,
            "tune_events" => {
                let since = a.get("since").and_then(Value::as_u64).unwrap_or(0) as usize;
                return Ok(json!({
                    "events": self.sweep_events.get(since..).unwrap_or(&[]),
                    "running": self.sweep.is_some(),
                }));
            }
            "scene" => {
                let i = screen.unwrap_or(0);
                let s = self
                    .screens
                    .get(i)
                    .ok_or_else(|| format!("there is no screen {i}"))?;
                let scene = s.stage.scene().ok_or("nothing is showing yet")?;
                let st = s.stage.status();
                let mut v = json!({
                    "screen": i,
                    "label": st.label,
                    "cell_px": st.cell_px,
                    "grid": st.grid,
                    "scene": serde_json::to_value(&scene).map_err(|e| e.to_string())?,
                });
                if jetcarp_core::fit::FIT {
                    v["tuned"] = json!(st.tuned);
                }
                return Ok(v);
            }
            "login" => {
                let on = a.get("on").and_then(Value::as_bool).unwrap_or(true);
                self.set_login(on)?;
            }
            "quit" => {
                self.quit = true;
            }
            other => return Err(format!("unknown action {other:?}")),
        }
        Ok(self.status())
    }

    fn answer(&mut self, p: Pending) {
        let id = p.message.get("id").cloned();
        let method = p
            .message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("");
        let params = p.message.get("params").cloned().unwrap_or(Value::Null);
        let result = if method != "tools/call" || params["name"] != HOST_TOOL {
            Err(format!("the host only answers {HOST_TOOL}"))
        } else {
            self.act(&params["arguments"])
        };
        let (text, is_error) = match result {
            // A generation after a jump beyond 2^53 - 1 is a decimal string.
            Ok(v) => (jetcarp_core::json_int::safe(v).to_string(), false),
            Err(e) => (e, true),
        };
        let reply = id.map(|id| {
            json!({"jsonrpc": "2.0", "id": id, "result": {
                "content": [{"type": "text", "text": text}],
                "isError": is_error,
            }})
        });
        p.reply(reply);
    }

    /// Sweeps again when the monitors have changed under tuned screens (never while Fit is
    /// switched off, #112).
    fn watch_retune(&mut self) {
        if !jetcarp_core::fit::FIT
            || self.sweep.is_some()
            || !self.is_host
            || !self.settings.tune.auto
            || self.shell() != Shell::Alive
        {
            self.retune_since = None;
            return;
        }
        let screens: Vec<_> = self
            .screens
            .iter()
            .enumerate()
            .map(|(i, s)| Monitor {
                place: s.place,
                canvas: s.stage.canvas(),
                scale: s.stage.status().scale,
                hz: self.screen_hz(i),
                shown: !self.hidden(s),
            })
            .collect();
        let want = retune_wanted(&self.settings, &screens);
        let now = self.now();
        if want.is_empty() || self.retune_tried.as_ref() == Some(&want) {
            self.retune_since = None;
            return;
        }
        let since = *self.retune_since.get_or_insert(now);
        let waiting = self.user_paused
            || self.local_holds.contains(WARNING_HOLD)
            || self.local_holds.contains("battery")
            || self.local_holds.contains("busy")
            || self.local_holds.contains(MEASURING_HOLD)
            || self.tuning_at.is_some();
        if now - since < RETUNE_SETTLE || waiting {
            return;
        }
        self.retune_since = None;
        // Only the screens whose monitor changed; the others keep their fit (#110).
        let req = TuneRequest {
            auto: true,
            screens: want.iter().map(|w| w.0).collect(),
            ..Default::default()
        };
        let which: Vec<String> = want.iter().map(|w| w.0.to_string()).collect();
        self.retune_tried = Some(want);
        match self.start_sweep(req, TRIAL_SECONDS) {
            Ok(()) => self.note(format!(
                "the monitor changed under screen {}; fitting it again",
                which.join(", ")
            )),
            Err(e) => self.note(format!("could not fit again for the new monitors: {e}")),
        }
    }

    /// What `status` says of tuning: a sweep here (or a tuner elsewhere holding the host), and how
    /// the last one ended.
    fn tuning_status(&self) -> Value {
        let mut v = json!({
            "running": self.sweep.is_some() || self.tuning_at.is_some(),
            "here": self.sweep.is_some(),
            "auto": self.sweep.as_ref().is_some_and(|s| s.auto()),
        });
        if let Some(sw) = &self.sweep {
            v["screens"] = json!(sw.fitting());
            v["sweep"] = sw.progress();
            v["progress"] = json!(
                self.sweep_events
                    .iter()
                    .rev()
                    .find(|e| matches!(e, TuneEvent::Rung { .. }))
                    .map(TuneEvent::describe)
            );
        }
        if let Some(last) = &self.sweep_last {
            v["last"] = last.clone();
        }
        v
    }

    /// A screen's surfaces changed size (physical pixels) or scale (pixels per point).
    pub fn resize_screen(&mut self, i: usize, canvas: [f64; 2], scale: f64) {
        let now = self.now();
        if i == 0 {
            self.canvas = canvas;
            self.scale = scale;
        }
        if let Some(s) = self.screens.get_mut(i) {
            s.stage.resize(canvas, scale, now);
        }
    }
}

/// A still picture's PNG, with the scene it shows (its grid's size included) inside, as every
/// picture JETCARP writes has, so the desktop's picture can be opened and run again.
fn still_png(s: &Screen, image: &jetcarp_core::render::Image) -> Result<Vec<u8>, String> {
    let meta = s
        .stage
        .world()
        .map(jetcarp_core::render::png_metadata)
        .unwrap_or_default();
    jetcarp_core::media::encode::png_with_text(image, &meta)
}

/// Where the host is with its GPU (#141).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuState {
    /// Not looked for yet: the GPU is opened with the first window shown, so a host started at
    /// login with nothing to show yet has none open.
    NotOpened,
    Found,
    /// Looked for, and none could be used.
    NoneFound,
    /// Lost (a driver reset, sometimes after sleep), until a new device is attached.
    Lost,
}

impl GpuState {
    /// As `status` gives it: `not_opened`, `found`, `none_found` or `lost`.
    pub fn word(self) -> &'static str {
        match self {
            GpuState::NotOpened => "not_opened",
            GpuState::Found => "found",
            GpuState::NoneFound => "none_found",
            GpuState::Lost => "lost",
        }
    }

    /// Why a screen steps on the processor when there is no GPU to step on.
    pub fn sim_reason(self) -> &'static str {
        match self {
            GpuState::NotOpened => "the GPU is opened when the scene is first shown",
            GpuState::Lost => "the GPU was lost, until it is back",
            GpuState::Found | GpuState::NoneFound => "no GPU",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuState, Monitor, hold_reasons, made_from, pause_reasons, retune_wanted};

    /// A screen of a host that hasn't opened its GPU yet says so, not "no GPU" (#141).
    #[test]
    fn a_screen_says_why_it_has_no_gpu() {
        assert_eq!(
            GpuState::NotOpened.sim_reason(),
            "the GPU is opened when the scene is first shown"
        );
        assert_eq!(
            GpuState::Lost.sim_reason(),
            "the GPU was lost, until it is back"
        );
        assert_eq!(GpuState::NoneFound.sim_reason(), "no GPU");
        assert_eq!(GpuState::NotOpened.word(), "not_opened");
    }
    use jetcarp_display::host_settings::{HostSettings, Place, ScreenSettings, Tuning};
    use jetcarp_display::{profiles, rotation};

    /// A reload keeps a screen's stage only while its rotation is made from the same things, so
    /// a profile saved again (*Show just this scene* pressed again) or an edited rotation shows.
    #[test]
    fn a_rotation_or_profile_saved_since_is_built_again() {
        let mut builtins = profiles::builtins();
        let r = rotation::Rotation::just("Mine", "Life");
        let made = |r: &rotation::Rotation, ps: &[profiles::Profile]| {
            let (slots, _) = r.expand(&rotation::default_base(), &|n| {
                ps.iter().find(|p| p.name == n).cloned()
            });
            made_from(r, &slots)
        };
        let before = made(&r, &builtins);
        assert_eq!(made(&r, &builtins), before, "the same twice");
        builtins[0].palette = Some("Aurora".into());
        assert_eq!(builtins[0].name, "Life");
        assert_ne!(made(&r, &builtins), before, "its profile changed");
        let mut edited = r.clone();
        edited.default_minutes = Some(3.0);
        assert_ne!(
            made(&edited, &profiles::builtins()),
            before,
            "the rotation changed"
        );
    }

    #[test]
    fn a_tuned_desktop_screen_of_another_size_wants_tuning_again() {
        let tuned = |rotation: &str| Tuning {
            rotation: rotation.into(),
            canvas: [2560, 1440],
            hz: Some(60.0),
            ..Default::default()
        };
        let screen = |rotation: &str, t: Option<Tuning>| ScreenSettings {
            rotation: rotation.into(),
            place: Place::Wallpaper,
            tuning: t,
            ..Default::default()
        };
        let mut s = HostSettings {
            screens: vec![
                screen("a", Some(tuned("a"))),
                screen("b", Some(tuned("old"))),
                screen("c", None),
            ],
            ..Default::default()
        };
        let mon = |place, w: f64, h: f64, shown| Monitor {
            place,
            canvas: [w, h],
            scale: 1.0,
            hz: Some(60.0),
            shown,
        };
        let wall = |w: f64, h: f64| mon(Place::Wallpaper, w, h, true);
        // The size it was tuned for: nothing to do.
        let same = [wall(2560.0, 1440.0); 3];
        assert!(retune_wanted(&s, &same).is_empty());
        // A new monitor: only the screen tuned for its rotation wants it.
        let moved = [wall(1920.0, 1080.0); 3];
        assert_eq!(retune_wanted(&s, &moved), [(0, [1920, 1080])]);
        // Not while it is in a window, or not shown.
        let window = [mon(Place::Window, 1920.0, 1080.0, true)];
        assert!(retune_wanted(&s, &window).is_empty());
        let hidden = [mon(Place::Wallpaper, 1920.0, 1080.0, false)];
        assert!(retune_wanted(&s, &hidden).is_empty());
        // Never tuned: never by itself.
        s.screens.iter_mut().for_each(|x| x.tuning = None);
        assert!(retune_wanted(&s, &moved).is_empty());
    }

    /// An automatic fit sweeps only the screens whose monitor changed, in size, pixels per point
    /// or refresh rate; a screen whose monitor didn't change keeps its fit (#110). The Mac's built-in
    /// screen waited behind its 40 Wolfram rules when only the other monitor's resolution changed.
    #[test]
    fn only_the_screens_whose_monitor_changed_are_fitted_again() {
        let fitted = |rotation: &str, canvas: [u32; 2], scale: f64, hz: f32| ScreenSettings {
            rotation: rotation.into(),
            place: Place::Wallpaper,
            tuning: Some(Tuning {
                rotation: rotation.into(),
                canvas,
                scale,
                hz: Some(hz),
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = HostSettings {
            screens: vec![
                fitted("retina", [3024, 1964], 2.0, 120.0),
                fitted("aoc", [2560, 1440], 1.0, 60.0),
            ],
            ..Default::default()
        };
        let mon = |w: f64, h: f64, scale: f64, hz: f32| Monitor {
            place: Place::Wallpaper,
            canvas: [w, h],
            scale,
            hz: Some(hz),
            shown: true,
        };
        let retina = mon(3024.0, 1964.0, 2.0, 120.0);
        // The AOC's resolution changed: only screen 1.
        assert_eq!(
            retune_wanted(&s, &[retina, mon(1920.0, 1080.0, 1.0, 60.0)]),
            [(1, [1920, 1080])]
        );
        // Its refresh rate, or its scale: only screen 1 again.
        assert_eq!(
            retune_wanted(&s, &[retina, mon(2560.0, 1440.0, 1.0, 144.0)]),
            [(1, [2560, 1440])]
        );
        assert_eq!(
            retune_wanted(&s, &[retina, mon(2560.0, 1440.0, 1.25, 60.0)]),
            [(1, [2560, 1440])]
        );
        // Nothing changed: nothing to fit; a rate not known is not a change.
        let unknown = Monitor {
            hz: None,
            ..mon(2560.0, 1440.0, 1.0, 60.0)
        };
        assert!(retune_wanted(&s, &[retina, unknown]).is_empty());
    }

    /// A pause (by hand, or nothing to show) and a hold are told apart, so `paused` and
    /// `paused_why` agree, and `held` and `held_why` do (#76): a busy processor is a hold, not a
    /// pause.
    #[test]
    fn status_says_why_it_is_paused_and_what_holds_it() {
        assert!(pause_reasons(false, false, false).is_empty(), "running");
        assert_eq!(pause_reasons(true, false, true), ["by hand"]);
        assert_eq!(pause_reasons(true, true, true), ["by hand"], "and held");
        assert!(
            pause_reasons(false, true, true).is_empty(),
            "only held: not paused"
        );
        assert_eq!(
            pause_reasons(false, false, true),
            ["nothing in the rotation can be shown"]
        );
        assert!(hold_reasons(std::iter::empty()).is_empty());
        assert_eq!(
            hold_reasons(["battery", "busy"].into_iter()),
            ["on battery", "the processor is busy"]
        );
        assert_eq!(
            hold_reasons(["covered", "dark", "tuning"].into_iter()),
            [
                "windows cover the desktop",
                "the screen is locked or off",
                "tuning the desktop background for this computer"
            ]
        );
        // The host's own "covered" and GNOME's say the same once.
        assert_eq!(hold_reasons(["covered", "covered"].into_iter()).len(), 1);
    }
}
