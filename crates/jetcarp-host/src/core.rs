//! The host's work, whatever shows it: the stages, the link, the settings, the memory limit.

use jetcarp_display::autostart;
use jetcarp_display::host_settings::{HostSettings, LockScreen, Place, ScreenSettings};
use jetcarp_display::rotation::Rotation;
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Lookup, Stage};
use jetcarp_display::tuning::{Pick, RUNG_SECONDS, TuneEvent, TuneRequest};

mod sweep;
use jetcarp_gpu::select;
use jetcarp_mcp::link::{HOST_ENDPOINT, HOST_TOOL, Listener, Pending};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Canvas until a surface says otherwise.
const DEFAULT_CANVAS: [f64; 2] = [1280.0, 720.0];
/// Frames per second while everything is paused.
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
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let choice = select::pick(&instance, select::Purpose::Display, None).ok()?;
        SharedGpu::on_with(choice.adapter, optional)
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
        })
    }
}

pub struct Screen {
    pub stage: Stage,
    pub sim: SimBackend,
    /// The monitor this screen belongs on.
    pub monitor: Option<usize>,
    pub place: Place,
}

pub struct Core {
    pub settings: HostSettings,
    pub screens: Vec<Screen>,
    /// A one-run override of the first screen's rotation.
    rotation_override: Option<String>,
    /// None in a screen saver, which nothing controls.
    listener: Option<Listener>,
    gpu: Option<SharedGpu>,
    lookup: Lookup,
    started: Instant,
    last_memory_check: f64,
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
}

/// The tuned desktop background screens whose canvas is not the one their tuning was found for,
/// with the canvas each has now: what a tune by itself would be for. `screens` gives each running
/// screen's place, canvas, and whether it shows.
pub fn retune_wanted(
    settings: &HostSettings,
    screens: &[(Place, [f64; 2], bool)],
) -> Vec<(usize, [u32; 2])> {
    screens
        .iter()
        .enumerate()
        .filter(|(_, (place, _, shown))| *place == Place::Wallpaper && *shown)
        .filter_map(|(i, &(_, canvas, _))| {
            let s = settings.screens.get(i)?;
            let t = s.tuning.as_ref().filter(|t| t.rotation == s.rotation)?;
            (!t.fits_canvas(canvas))
                .then(|| (i, [canvas[0].round() as u32, canvas[1].round() as u32]))
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

/// Why the screens are paused, in words, for `status` (empty while they run): by hand, then each
/// hold, and when neither, that nothing in the rotation can be shown.
pub fn pause_reasons<'a>(
    by_hand: bool,
    holds: impl Iterator<Item = &'a str>,
    all_paused: bool,
) -> Vec<String> {
    if !by_hand && !all_paused {
        return vec![];
    }
    let mut why: Vec<String> = vec![];
    if by_hand {
        why.push("by hand".into());
    }
    for h in holds {
        let words = match h {
            "battery" => "on battery".to_string(),
            "busy" => "the processor is busy".into(),
            "covered" => "held by the desktop: windows cover it".into(),
            "tuning" => "tuning the desktop background for this computer".into(),
            "dark" => "held by the desktop: the screen is locked or off".into(),
            other => format!("held by the desktop ({other})"),
        };
        if !why.contains(&words) {
            why.push(words);
        }
    }
    if why.is_empty() {
        why.push("nothing in the rotation can be shown".into());
    }
    why
}

/// The start of a wallpaper window's title; GNOME's extension looks for it.
const WALLPAPER_TAG: &str = "@jetcarp-wallpaper";

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
        core.settle_still();
        Ok(core)
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
        let mut core = Core {
            settings,
            screens: vec![],
            rotation_override: rotation,
            listener,
            gpu: None,
            lookup,
            started: Instant::now(),
            last_memory_check: 0.0,
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
        };
        if let Some(w) = warning {
            core.note(w);
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
                self.settings.gpu_policy(),
            ),
            None => SimBackend::unavailable("no GPU".into()),
        }
    }

    fn new_stage(&self, name: &str, canvas: [f64; 2]) -> Result<Stage, String> {
        let (r, slots) = load_rotation(name)?;
        let mut stage = Stage::new(
            &r,
            slots,
            seed(),
            canvas,
            self.lookup.clone(),
            self.settings.max_cells,
        )?;
        stage.resize(canvas, self.scale, self.now());
        Ok(stage)
    }

    fn wanted_rotation(&self, i: usize) -> String {
        match (&self.rotation_override, i) {
            (Some(r), 0) => r.clone(),
            _ => self.settings.screens[i].rotation.clone(),
        }
    }

    /// Makes the running screens match the settings, keeping those that did not change.
    fn build_screens(&mut self) -> Result<(), String> {
        let mut old: Vec<Option<Screen>> = std::mem::take(&mut self.screens)
            .into_iter()
            .map(Some)
            .collect();
        let mut out = Vec::with_capacity(self.settings.screens.len());
        let mut first_error = None;
        for i in 0..self.settings.screens.len() {
            let want = self.wanted_rotation(i);
            let (monitor, place) = (
                self.settings.screens[i].monitor,
                self.settings.screens[i].place,
            );
            let tuned = self.settings.tuning_for(i, &want).cloned();
            let kept = old.get_mut(i).and_then(Option::take);
            if let Some(mut s) = kept.filter(|s| s.stage.rotation() == want) {
                s.monitor = monitor;
                s.place = place;
                s.stage.set_tuned(tuned);
                out.push(s);
                continue;
            }
            match self.new_stage(&want, self.canvas) {
                Ok(mut stage) => out.push(Screen {
                    stage: {
                        stage.set_tuned(tuned);
                        stage
                    },
                    sim: self.new_sim(),
                    monitor,
                    place,
                }),
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
        match &gpu {
            None => self.note("no GPU found; running on the CPU"),
            Some(g) => {
                self.note(format!(
                    "GPU: {}; GPU simulation {}",
                    jetcarp_gpu::describe(&g.adapter.get_info()),
                    self.settings.gpu_policy().describe()
                ));
            }
        }
        self.gpu = gpu;
        for i in 0..self.screens.len() {
            self.screens[i].sim = self.new_sim();
        }
    }

    /// The GPU was lost (a driver reset, sometimes after sleep): every world is built again, and
    /// stepped on the CPU until a new device is attached.
    pub fn gpu_lost(&mut self) {
        self.gpu = None;
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
        // A sweep runs its screen whatever else holds it, and pauses the others, so the host's
        // load is the swept scene's.
        let swept = self.sweep.as_ref().and_then(|s| s.screen());
        for (i, s) in self.screens.iter_mut().enumerate() {
            s.stage.set_paused(match swept {
                Some(k) => i != k,
                None => paused,
            });
        }
    }

    /// Whether something says nothing need be drawn right now.
    pub fn held(&self) -> bool {
        !self.holds.is_empty() || !self.local_holds.is_empty()
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

    /// Whether screen `s` is left unseen: a window screen of a host started at login.
    pub fn hidden(&self, s: &Screen) -> bool {
        self.login && s.place != Place::Wallpaper
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

    pub fn frame_time(&self) -> Duration {
        let fps = if self.all_paused() {
            PAUSED_FPS
        } else {
            self.settings.fps
        };
        Duration::from_secs_f32(1.0 / fps.max(1.0))
    }

    /// One frame: answer the link, watch memory, run every screen.
    pub fn tick(&mut self) {
        let requests = self.listener.as_ref().map_or(vec![], |l| l.poll());
        for p in requests {
            self.answer(p);
        }
        self.expire_holds();
        let now = self.now();
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
        if self.tuning_at.is_some_and(|t| now - t > TUNING_SILENCE) {
            self.tuning_at = None;
            self.set_local_hold("tuning", false);
            self.note("the tuner stopped answering; carrying on");
        }
        self.sweep_step();
        self.watch_retune();
        let count = self.screens.len().max(1) as f32;
        let budget = Duration::from_secs_f32(STEP_SHARE / self.settings.fps.max(1.0) / count);
        let mut notes = vec![];
        let login = self.login;
        for s in &mut self.screens {
            if login && s.place != Place::Wallpaper {
                continue;
            }
            s.stage.tick(now, &mut s.sim, budget);
            notes.extend(s.stage.take_notes());
        }
        for n in notes {
            self.note(n);
        }
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
        let Some(m) = memory_stats::memory_stats() else {
            return;
        };
        let mb = (m.physical_mem / (1 << 20)) as u64;
        if mb <= self.settings.max_memory_mb {
            return;
        }
        let biggest = self
            .screens
            .iter_mut()
            .filter(|s| s.stage.world().is_some())
            .max_by_key(|s| s.stage.world().map_or(0, |w| w.topo.len()));
        if let Some(s) = biggest {
            let limit = self.settings.max_memory_mb;
            s.stage.abandon_item(&format!(
                "the host used {mb} MB, over its limit of {limit} MB"
            ));
        }
    }

    fn status(&self) -> Value {
        let now = self.now();
        json!({
            "running": true,
            "pid": std::process::id(),
            "uptime_s": now.round(),
            "memory_mb": memory_stats::memory_stats().map(|m| m.physical_mem / (1 << 20)),
            // Paused as the user sees it: a hold passes by itself, but a rotation with nothing it
            // can show has to be resumed.
            "paused": self.user_paused || (!self.held() && self.all_paused()),
            "held": self.holds.iter().map(String::as_str).chain(self.local_holds.iter().copied()).collect::<Vec<_>>(),
            "paused_why": pause_reasons(
                self.user_paused,
                self.holds.iter().map(String::as_str).chain(self.local_holds.iter().copied()),
                self.all_paused(),
            ),
            "desktop_layout": self.desktop_layout,
            "wallpaper": self.wants_wallpaper(),
            "extension": self.shell() == Shell::Alive,
            "lock_screen": self.settings.lock_screen.word(),
            "monitors": self.monitors,
            "monitor_hz": self.monitor_hz,
            "gpu": self.gpu.as_ref().map(|g| g.adapter.get_info().name),
            "fps": self.settings.fps,
            "tune": self.settings.tune,
            "tuning": self.tuning_status(),
            "caption": self.settings.caption,
            "run_at_login": autostart::enabled(),
            "rotations": rotation_names(),
            "login": self.login,
            "screens": self.screens.iter().map(|s| json!({
                "monitor": s.monitor,
                "place": s.place,
                "hidden": self.hidden(s),
                "stage": s.stage.status(),
            })).collect::<Vec<_>>(),
            "log": self.log,
        })
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

    /// Reads the settings file again, and the shared GPU setting.
    pub fn reload(&mut self) -> Result<(), String> {
        let (settings, warning) = HostSettings::load();
        if let Some(w) = warning {
            self.note(w);
        }
        self.settings = settings;
        let built = self.build_screens();
        // The GPU choice may have changed: its own, or the shared setting it follows.
        let mode = self.settings.gpu_policy().mode;
        for s in &mut self.screens {
            s.sim.mode = mode;
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
                    .unwrap_or(RUNG_SECONDS);
                self.start_sweep(
                    TuneRequest {
                        cpu_percent: num("cpu_limit"),
                        gpu_percent: num("gpu_limit"),
                        fps: num("fps"),
                        screen,
                        pick,
                        dry_run: a.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
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
                return Ok(json!({
                    "screen": i,
                    "label": st.label,
                    "cell_px": st.cell_px,
                    "tuned": st.tuned,
                    "grid": st.grid,
                    "scene": serde_json::to_value(&scene).map_err(|e| e.to_string())?,
                }));
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

    /// Sweeps again when the monitors have changed under tuned screens.
    fn watch_retune(&mut self) {
        if self.sweep.is_some()
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
            .map(|s| (s.place, s.stage.canvas(), !self.hidden(s)))
            .collect();
        let want = retune_wanted(&self.settings, &screens);
        let now = self.now();
        if want.is_empty() || self.retune_tried.as_ref() == Some(&want) {
            self.retune_since = None;
            return;
        }
        let since = *self.retune_since.get_or_insert(now);
        let waiting = self.user_paused
            || self.local_holds.contains("battery")
            || self.local_holds.contains("busy")
            || self.tuning_at.is_some();
        if now - since < RETUNE_SETTLE || waiting {
            return;
        }
        self.retune_since = None;
        self.retune_tried = Some(want);
        let req = TuneRequest {
            auto: true,
            ..Default::default()
        };
        match self.start_sweep(req, RUNG_SECONDS) {
            Ok(()) => {
                self.note("the monitors changed; sweeping the desktop background's sizes again")
            }
            Err(e) => self.note(format!("could not sweep again for the new monitors: {e}")),
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

#[cfg(test)]
mod tests {
    use super::{pause_reasons, retune_wanted};
    use jetcarp_display::host_settings::{HostSettings, Place, ScreenSettings, Tuning};

    #[test]
    fn a_tuned_desktop_screen_of_another_size_wants_tuning_again() {
        let tuned = |rotation: &str| Tuning {
            rotation: rotation.into(),
            canvas: [2560, 1440],
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
        let wall = |w: f64, h: f64| (Place::Wallpaper, [w, h], true);
        // The size it was tuned for: nothing to do.
        let same = [wall(2560.0, 1440.0); 3];
        assert!(retune_wanted(&s, &same).is_empty());
        // A new monitor: only the screen tuned for its rotation wants it.
        let moved = [wall(1920.0, 1080.0); 3];
        assert_eq!(retune_wanted(&s, &moved), [(0, [1920, 1080])]);
        // Not while it is in a window, or not shown.
        let window = [(Place::Window, [1920.0, 1080.0], true)];
        assert!(retune_wanted(&s, &window).is_empty());
        let hidden = [(Place::Wallpaper, [1920.0, 1080.0], false)];
        assert!(retune_wanted(&s, &hidden).is_empty());
        // Never tuned: never by itself.
        s.screens.iter_mut().for_each(|x| x.tuning = None);
        assert!(retune_wanted(&s, &moved).is_empty());
    }

    #[test]
    fn status_says_why_it_is_paused() {
        let none = std::iter::empty();
        assert!(pause_reasons(false, none, false).is_empty(), "running");
        assert_eq!(pause_reasons(true, std::iter::empty(), true), ["by hand"]);
        assert_eq!(
            pause_reasons(false, ["battery", "busy"].into_iter(), true),
            ["on battery", "the processor is busy"]
        );
        assert_eq!(
            pause_reasons(true, ["covered", "dark"].into_iter(), true),
            [
                "by hand",
                "held by the desktop: windows cover it",
                "held by the desktop: the screen is locked or off"
            ]
        );
        // The host's own "covered" and GNOME's say the same once.
        assert_eq!(
            pause_reasons(false, ["covered", "covered"].into_iter(), true).len(),
            1
        );
        assert_eq!(
            pause_reasons(false, std::iter::empty(), true),
            ["nothing in the rotation can be shown"]
        );
    }
}
