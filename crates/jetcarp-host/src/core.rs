//! The host's work, whatever shows it: the stages, the link, the settings, the memory limit.

use jetcarp_display::autostart;
use jetcarp_display::host_settings::{HostSettings, LockScreen, Place, ScreenSettings};
use jetcarp_display::rotation::Rotation;
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Lookup, Stage};
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
const STEP_SHARE: f32 = 0.6;
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
    /// A device with no surface (None when the machine has no GPU).
    pub fn headless() -> Option<SharedGpu> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).ok()?;
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
    /// Set when the screens were rebuilt, so surfaces match them again.
    pub screens_changed: bool,
    /// Asked to quit; the host ends once [`Core::finished`] says so.
    pub quit: bool,
    quit_at: Option<f64>,
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
            screens_changed: false,
            quit: false,
            quit_at: None,
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

    fn new_sim(&self) -> SimBackend {
        match &self.gpu {
            Some(g) if self.settings.gpu => {
                SimBackend::from_device(&g.adapter, g.device.clone(), g.queue.clone())
            }
            Some(_) => SimBackend::unavailable("GPU stepping is off in the host's settings".into()),
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
            let kept = old.get_mut(i).and_then(Option::take);
            if let Some(mut s) = kept.filter(|s| s.stage.rotation() == want) {
                s.monitor = monitor;
                s.place = place;
                out.push(s);
                continue;
            }
            match self.new_stage(&want, self.canvas) {
                Ok(stage) => out.push(Screen {
                    stage,
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
        if gpu.is_none() {
            self.note("no GPU found; running on the CPU");
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
        for s in &mut self.screens {
            s.stage.set_paused(paused);
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
        autostart::set(on, &autostart::host_command()?)
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
    /// the link closes (its endpoint file goes), the screens' worlds and GPU buffers are dropped,
    /// and the GPU is waited on until it is idle. The device itself is left for the process's end.
    pub fn shutdown(&mut self) {
        self.listener = None;
        self.screens.clear();
        if let Some(g) = &self.gpu {
            jetcarp_display::exit::wait_idle(&g.device);
        }
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
        let count = self.screens.len().max(1) as f32;
        let budget = Duration::from_secs_f32(STEP_SHARE / self.settings.fps.max(1.0) / count);
        let mut notes = vec![];
        for s in &mut self.screens {
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
            "desktop_layout": self.desktop_layout,
            "wallpaper": self.wants_wallpaper(),
            "extension": self.shell() == Shell::Alive,
            "lock_screen": self.settings.lock_screen.word(),
            "monitors": self.monitors,
            "gpu": self.gpu.as_ref().map(|g| g.adapter.get_info().name),
            "fps": self.settings.fps,
            "caption": self.settings.caption,
            "run_at_login": autostart::enabled(),
            "rotations": rotation_names(),
            "screens": self.screens.iter().map(|s| json!({
                "monitor": s.monitor,
                "place": s.place,
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

    /// Reads the settings file again.
    pub fn reload(&mut self) -> Result<(), String> {
        let (settings, warning) = HostSettings::load();
        if let Some(w) = warning {
            self.note(w);
        }
        self.settings = settings;
        let built = self.build_screens();
        self.settle_still();
        built
    }

    /// Whether a still of the scene should stand in for the live wallpaper.
    pub fn wants_still(&self) -> bool {
        jetcarp_display::desktop_picture::wanted(&self.settings)
    }

    /// Leaves a still of the first wallpaper screen's scene as the desktop picture.
    pub fn leave_still(&mut self) -> Result<std::path::PathBuf, String> {
        let screen = self
            .screens
            .iter()
            .filter(|s| s.place == Place::Wallpaper)
            .find(|s| s.stage.world().is_some())
            .ok_or("no scene to leave on the desktop")?;
        let image = screen
            .stage
            .still()
            .ok_or("no scene to leave on the desktop")?;
        let png = jetcarp_core::media::encode::png(&image)?;
        jetcarp_display::desktop_picture::show(&png)
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

    /// While the live wallpaper can't be shown, keeps a still of each new item on the desktop.
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
