//! `jetcarp host tune`: *Fit to this computer*. Runs each scene of the desktop background's
//! rotations offscreen, at its screen's size, the way the host runs it (the same stage, stepping
//! and drawing, at the host's frame rate), measures JETCARP's own load, and keeps for each scene
//! the smallest cell size that stays within the user's limits ([`tuning::search`]). The rules,
//! palettes and colouring stay as they are; only the number of cells changes.
//!
//! It runs in a process of its own, so the processor time it measures is its own. A running host
//! pauses meanwhile (the `tuning` hold, renewed every few seconds, so a tuner that crashes lets
//! go by itself) and reads the sizes once they are saved.

use crate::core::{STEP_SHARE, SharedGpu, load_rotation};
use crate::power::ProcessMeter;
use jetcarp_core::World;
use jetcarp_display::host_settings::{GpuTiming, HostSettings, Place, TunedScene, Tuning};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{self, Profile};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::{SimBackend, SimStatus};
use jetcarp_display::stage::{self, Lookup, Stage};
use jetcarp_display::tuning::{
    Measure, SceneLimits, TRIAL_SECONDS, TuneEvent, candidates, coarsest_px, fits, search,
};
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::select::Mode;
use jetcarp_mcp::link::host_call;
use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Scenes tuned per screen; the rest keep their own sizes.
const MAX_SCENES: usize = 40;
/// Seconds run before measuring: uploads, first colouring, shaders.
const WARMUP: f64 = 1.0;
/// A world that takes longer than this to build is given up on.
const BUILD_LIMIT: f64 = 60.0;
/// How often the host's `tuning` hold is renewed. The host lets go after 30 s without.
const HOLD_EVERY: f64 = 5.0;
/// The canvas when nothing says what the screen's is.
const FALLBACK_CANVAS: [f64; 2] = [1920.0, 1080.0];
/// The offscreen target's format: plain, as the host's surfaces are.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// `jetcarp host tune` options.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct TuneArgs {
    /// JETCARP's processor time, in percent of the whole machine, every core counted (saved;
    /// default 10).
    #[arg(long, value_name = "PERCENT")]
    pub cpu_limit: Option<f32>,
    /// JETCARP's share of the GPU's time, in percent (saved; default 25).
    #[arg(long, value_name = "PERCENT")]
    pub gpu_limit: Option<f32>,
    /// Frames per second it must keep (saved; default: the monitor's refresh rate, or 60, at most
    /// the host's frame rate).
    #[arg(long)]
    pub fps: Option<f32>,
    /// Only this screen (counted from 0). Default: every screen on the desktop.
    #[arg(long)]
    pub screen: Option<usize>,
    /// The screen's size in pixels, as WxH, when no host is running to say (default: the size it
    /// was tuned for before, else its monitor's).
    #[arg(long, value_parser = crate::parse_canvas)]
    pub canvas: Option<[f64; 2]>,
    /// Seconds each trial measures.
    #[arg(long, default_value_t = TRIAL_SECONDS)]
    pub trial_seconds: f64,
    /// Measure and report, but save nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Started by the host because the monitors changed.
    #[arg(long, hide = true)]
    pub auto: bool,
}

/// Where the tuner reports: lines for people, or JSON lines (`--json`).
struct Out {
    json: bool,
}

impl Out {
    fn event(&self, e: &TuneEvent) {
        let line = if self.json {
            serde_json::to_string(e).unwrap_or_default()
        } else {
            e.describe()
        };
        // Whoever started it may have gone; the tune carries on and saves what it finds.
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    fn warn(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "warning: {line}");
    }
}

/// One screen to tune.
struct Job {
    screen: usize,
    rotation: String,
    canvas: [f64; 2],
    scale: f64,
    refresh: Option<f32>,
    slots: Vec<Slot>,
}

/// Runs the tune, reporting as it goes. `json`: JSON lines on stdout.
pub fn run(args: TuneArgs, json: bool) -> Result<(), String> {
    let out = Out { json };
    let r = tune(&args, &out);
    if let (Err(e), true) = (&r, json) {
        out.event(&TuneEvent::Failed { error: e.clone() });
    }
    r
}

/// Only one tune at a time: the app's, the host's and the command line's would measure each other.
fn lock() -> Result<std::fs::File, String> {
    let path = jetcarp_mcp::link::runtime_file("tune.lock").ok_or("no runtime folder")?;
    let dir = path.parent().expect("a folder");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => Err("a tune is already running".into()),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

fn limit(x: Option<f32>, range: std::ops::RangeInclusive<f32>, what: &str) -> Result<(), String> {
    match x {
        Some(v) if !range.contains(&v) => Err(format!(
            "{what} is {} to {} (got {v})",
            range.start(),
            range.end()
        )),
        _ => Ok(()),
    }
}

fn tune(args: &TuneArgs, out: &Out) -> Result<(), String> {
    use jetcarp_display::host_settings::TuneLimits as L;
    limit(args.cpu_limit, L::CPU_RANGE, "the processor limit")?;
    limit(args.gpu_limit, L::GPU_RANGE, "the GPU limit")?;
    limit(args.fps, L::FPS_RANGE, "the frame rate")?;
    if !(0.5..=60.0).contains(&args.trial_seconds) {
        return Err("trials are 0.5 to 60 seconds".into());
    }
    let _lock = lock()?;
    let (mut settings, warning) = HostSettings::load();
    if let Some(w) = warning {
        out.warn(&w);
    }
    if let Some(x) = args.cpu_limit {
        settings.tune.cpu_percent = x;
    }
    if let Some(x) = args.gpu_limit {
        settings.tune.gpu_percent = x;
    }
    if args.fps.is_some() {
        settings.tune.fps = args.fps;
    }
    if crate::power::on_battery() == Some(true) {
        if args.auto {
            return Err("on battery: not tuning until the computer is plugged in".into());
        }
        out.warn("on battery: a computer on battery may run slower than plugged in, so the sizes found may be coarser than it needs");
    }
    let status = host_call(&json!({"action": "status"})).ok();
    let jobs = plan(&settings, args, status.as_ref(), out)?;
    let gpu = SharedGpu::headless_with(wgpu::Features::TIMESTAMP_QUERY);
    if gpu.is_none() {
        out.warn("no GPU found: measuring on the processor alone");
    }
    let total: usize = jobs.iter().map(|j| j.slots.len()).sum();
    let limits = settings.tune;
    out.event(&TuneEvent::Start {
        screens: jobs.iter().map(|j| j.screen).collect(),
        scenes: total,
        fps: limits.target_fps(jobs.first().and_then(|j| j.refresh), settings.fps),
        gpu: gpu.as_ref().map(|g| g.adapter.get_info().name),
    });
    let mut hold = Hold::start(status.is_some());
    let mut rig = Rig::new(gpu, &settings, jobs.len());
    let mut found: Vec<(usize, Tuning)> = vec![];
    let mut number = 0;
    for job in &jobs {
        let fps = limits.target_fps(job.refresh, settings.fps);
        let scene_limits = SceneLimits::of(&limits, fps, jobs.len());
        let mut scenes = vec![];
        for slot in &job.slots {
            number += 1;
            let design = stage::cell_pixels(slot.profile.cell_px, job.scale);
            let fixed = !rig.resizable(&slot.profile);
            let sizes = if fixed {
                vec![design]
            } else {
                candidates(design, coarsest_px(job.canvas, design))
            };
            let result = search(&sizes, &scene_limits, |px| {
                hold.renew();
                out.event(&TuneEvent::Trial {
                    screen: job.screen,
                    number,
                    of: total,
                    scene: slot.label.clone(),
                    cell_px: px,
                });
                let m = rig.trial(job, slot, px, design, args.trial_seconds, &mut || {
                    hold.renew()
                })?;
                out.event(&TuneEvent::Measured {
                    screen: job.screen,
                    scene: slot.label.clone(),
                    cell_px: px,
                    measure: m,
                    over: fits(&m, &scene_limits).err(),
                });
                Ok(m)
            });
            let t = match result {
                Ok(f) => {
                    let mut t = f.tuned(&slot.label, design);
                    if fixed {
                        let why = "its size is set by the scene (an aperiodic patch, or not rebuilt to fill the screen), so it runs as it is";
                        t.note = Some(match t.note {
                            Some(n) => format!("{n}; {why}"),
                            None => why.to_string(),
                        });
                    }
                    t
                }
                // A scene that cannot be shown is left at its own size.
                Err(e) => TunedScene {
                    scene: slot.label.clone(),
                    design_px: design,
                    cell_px: design,
                    note: Some(e),
                    ..Default::default()
                },
            };
            out.event(&TuneEvent::Scene {
                screen: job.screen,
                result: t.clone(),
            });
            scenes.push(t);
        }
        found.push((
            job.screen,
            Tuning {
                rotation: job.rotation.clone(),
                canvas: [job.canvas[0].round() as u32, job.canvas[1].round() as u32],
                scale: job.scale,
                limits,
                fps,
                scenes,
            },
        ));
    }
    let saved = if args.dry_run {
        None
    } else {
        // Read again: the settings may have changed while it ran.
        let (mut fresh, _) = HostSettings::load();
        fresh.tune = jetcarp_display::host_settings::TuneLimits {
            auto: fresh.tune.auto,
            ..limits
        };
        for (i, t) in found {
            if let Some(s) = fresh.screens.get_mut(i)
                && s.rotation == t.rotation
            {
                s.tuning = Some(t);
            }
        }
        fresh.save()?;
        if status.is_some() {
            let _ = host_call(&json!({"action": "reload"}));
        }
        Some(
            jetcarp_display::host_settings::config_dir()
                .map(|d| d.join("host.json").display().to_string())
                .unwrap_or_default(),
        )
    };
    drop(hold);
    out.event(&TuneEvent::Done {
        saved,
        scenes: total,
        gpu_timing: rig.gpu_timing(),
    });
    Ok(())
}

/// The screens to tune, each with its size and its rotation's scenes.
fn plan(
    settings: &HostSettings,
    args: &TuneArgs,
    status: Option<&Value>,
    out: &Out,
) -> Result<Vec<Job>, String> {
    let n = settings.screens.len();
    let which: Vec<usize> = match args.screen {
        Some(i) if i < n => vec![i],
        Some(i) => return Err(format!("there is no screen {i}")),
        None => {
            let walls: Vec<usize> = (0..n)
                .filter(|&i| settings.screens[i].place == Place::Wallpaper)
                .collect();
            if walls.is_empty() {
                (0..n).collect()
            } else {
                walls
            }
        }
    };
    let mut probed: Option<Vec<Probed>> = None;
    let mut jobs = vec![];
    for i in which {
        let s = &settings.screens[i];
        let (_, mut slots) = load_rotation(&s.rotation)?;
        if slots.len() > MAX_SCENES {
            out.warn(&format!(
                "screen {i}: {} has {} scenes; tuning the first {MAX_SCENES}, the rest keep their own sizes",
                s.rotation,
                slots.len()
            ));
            slots.truncate(MAX_SCENES);
        }
        let from_host = status.and_then(|v| {
            let st = &v["screens"][i]["stage"];
            let c = st["canvas"].as_array()?;
            let canvas = [c.first()?.as_f64()?, c.get(1)?.as_f64()?];
            let scale = st["scale"].as_f64().unwrap_or(1.0);
            let hz = s
                .monitor
                .or(Some(0))
                .and_then(|m| v["monitor_hz"][m].as_f64())
                .map(|h| h as f32);
            Some((canvas, scale, hz))
        });
        let (canvas, scale, refresh) = match (from_host, args.canvas, &s.tuning) {
            (Some(h), _, _) => h,
            (None, Some(c), _) => (c, 1.0, None),
            (None, None, Some(t)) if t.canvas[0] > 0 && t.canvas[1] > 0 => (
                [t.canvas[0] as f64, t.canvas[1] as f64],
                t.scale.max(0.1),
                None,
            ),
            _ => {
                if probed.is_none() {
                    probed = Some(probe_monitors().unwrap_or_default());
                }
                match monitor_canvas(probed.as_deref().unwrap_or(&[]), s.monitor) {
                    Some(m) => m,
                    None => {
                        out.warn(&format!(
                            "screen {i}: no host running and no monitors found; tuning for {}x{} (give --canvas)",
                            FALLBACK_CANVAS[0], FALLBACK_CANVAS[1]
                        ));
                        (FALLBACK_CANVAS, 1.0, None)
                    }
                }
            }
        };
        jobs.push(Job {
            screen: i,
            rotation: s.rotation.clone(),
            canvas,
            scale,
            refresh,
            slots,
        });
    }
    if jobs.is_empty() {
        return Err("no screens to tune".into());
    }
    Ok(jobs)
}

/// A monitor, as the tuner finds it with no host running.
#[derive(Clone, Debug, PartialEq)]
struct Probed {
    pos: (i32, i32),
    size: (u32, u32),
    scale: f64,
    hz: Option<f32>,
}

/// The monitors, from the display server (empty with no display).
fn probe_monitors() -> Result<Vec<Probed>, String> {
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::WindowId;
    struct P(Vec<Probed>);
    impl ApplicationHandler for P {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            self.0 = el
                .available_monitors()
                .map(|m| Probed {
                    pos: (m.position().x, m.position().y),
                    size: (m.size().width, m.size().height),
                    scale: m.scale_factor(),
                    hz: m.refresh_rate_millihertz().map(|h| h as f32 / 1000.0),
                })
                .collect();
            el.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    let mut p = P(vec![]);
    el.run_app(&mut p).map_err(|e| e.to_string())?;
    Ok(p.0)
}

/// The canvas a screen on `monitor` gets: that monitor's size, or with none the primary's (the one
/// at 0, 0), as a desktop background across every monitor is drawn at.
fn monitor_canvas(mons: &[Probed], monitor: Option<usize>) -> Option<([f64; 2], f64, Option<f32>)> {
    let m = match monitor {
        Some(i) => mons.get(i),
        None => mons.iter().find(|m| m.pos == (0, 0)).or(mons.first()),
    }?;
    Some(([m.size.0 as f64, m.size.1 as f64], m.scale, m.hz))
}

/// The host's `tuning` hold, renewed while the tune runs and let go when it ends.
struct Hold {
    on: bool,
    at: Option<Instant>,
}

impl Hold {
    fn start(host: bool) -> Hold {
        let mut h = Hold { on: host, at: None };
        h.renew();
        h
    }

    fn renew(&mut self) {
        if self.on && self.at.is_none_or(|t| t.elapsed().as_secs_f64() >= HOLD_EVERY) {
            self.at = Some(Instant::now());
            let _ = host_call(&json!({"action": "tuning", "on": true}));
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.on {
            let _ = host_call(&json!({"action": "tuning", "on": false}));
        }
    }
}

/// Whether a trial's world steps on the GPU, as the host would step it: the scene allows it, the
/// host's GPU can run it, and the GPU policy says so (automatic: when the GPU is worth it for a
/// world of this size, [`GpuSim::worth`]). A trial left on the processor is held to the
/// processor limit like any other.
fn steps_on_gpu(profile: &Profile, sim: &SimBackend, world: &World) -> bool {
    let paced = profile.speed > 0.0;
    profile.gpu
        && sim.available()
        && match sim.mode {
            Mode::Off => false,
            Mode::On => true,
            Mode::Auto => GpuSim::worth(world, paced),
        }
}

/// GPU timestamps around a frame's step and its drawing.
struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Nanoseconds per tick.
    period: f64,
}

impl Timer {
    fn new(g: &SharedGpu) -> Option<Timer> {
        if !g.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let set = g.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("jetcarp-tune"),
            ty: wgpu::QueryType::Timestamp,
            count: 4,
        });
        let buf = |usage| {
            g.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("jetcarp-tune"),
                size: 32,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Timer {
            set,
            resolve: buf(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buf(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            period: g.queue.get_timestamp_period() as f64,
        })
    }

    /// Writes timestamp `index` once the GPU gets here (an empty pass of its own).
    fn mark(&self, g: &SharedGpu, index: u32) {
        let mut enc = g.device.create_command_encoder(&Default::default());
        drop(enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("jetcarp-tune-mark"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.set,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        }));
        g.queue.submit([enc.finish()]);
    }

    fn draw_writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2),
            end_of_pass_write_index: Some(3),
        }
    }

    /// Waits for the frame, and gives the step's and the drawing's GPU seconds.
    fn read(&self, g: &SharedGpu) -> (f64, f64) {
        let mut enc = g.device.create_command_encoder(&Default::default());
        enc.resolve_query_set(&self.set, 0..4, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 32);
        g.queue.submit([enc.finish()]);
        self.read.map_async(wgpu::MapMode::Read, .., |_| {});
        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
        let t: [u64; 4] = match self.read.get_mapped_range(..) {
            Ok(data) => {
                let mut t = [0u64; 4];
                for (k, c) in data.chunks_exact(8).take(4).enumerate() {
                    t[k] = u64::from_le_bytes(c.try_into().unwrap_or([0; 8]));
                }
                t
            }
            Err(_) => [0; 4],
        };
        self.read.unmap();
        let secs = |a: u64, b: u64| b.saturating_sub(a) as f64 * self.period * 1e-9;
        (secs(t[0], t[1]), secs(t[2], t[3]))
    }
}

/// What the trials run on: the GPU (if any), its timer, and the host's settings.
struct Rig {
    gpu: Option<SharedGpu>,
    timer: Option<Timer>,
    clock: Clock,
    sim: SimBackend,
    settings: HostSettings,
    lookup: Lookup,
    /// Screens shown at once: each gets this share of a frame's stepping time.
    screens: usize,
}

impl Rig {
    fn new(gpu: Option<SharedGpu>, settings: &HostSettings, screens: usize) -> Rig {
        let sim = match &gpu {
            Some(g) if settings.gpu => {
                SimBackend::from_device(&g.adapter, g.device.clone(), g.queue.clone())
            }
            Some(_) => SimBackend::unavailable("GPU stepping is off in the host's settings".into()),
            None => SimBackend::unavailable("no GPU".into()),
        };
        let timer = gpu.as_ref().and_then(Timer::new);
        let clock = match (&gpu, &timer) {
            (None, _) => Clock::None,
            _ if cfg!(target_os = "linux") => Clock::Undecided,
            (Some(_), Some(_)) => Clock::Timestamps,
            (Some(_), None) => Clock::Submission,
        };
        Rig {
            timer,
            clock,
            gpu,
            sim,
            settings: settings.clone(),
            lookup: jetcarp_display::stage::library_lookup(),
            screens,
        }
    }

    /// How the trials measured the GPU's time.
    fn gpu_timing(&self) -> GpuTiming {
        match self.clock {
            Clock::None => GpuTiming::None,
            Clock::Kernel => GpuTiming::Kernel,
            Clock::Timestamps => GpuTiming::Timestamps,
            Clock::Undecided | Clock::Submission => GpuTiming::Submission,
        }
    }

    /// Whether the scene's cell count follows its cell size: a periodic tiling rebuilt to fill
    /// the screen.
    fn resizable(&self, p: &Profile) -> bool {
        let (p, scene, _) = profiles::resolve(p, &*self.lookup);
        p.refill && profiles::is_lattice(&scene)
    }

    /// Runs `slot` at `px` pixels a cell (its own size is `design`) for `seconds`, as the host
    /// would, and measures it. `renew` is called while a big world builds.
    fn trial(
        &mut self,
        job: &Job,
        slot: &Slot,
        px: f32,
        design: f32,
        seconds: f64,
        renew: &mut dyn FnMut(),
    ) -> Result<Measure, String> {
        let mut slot = slot.clone();
        slot.end.clear();
        slot.schedule = None;
        let rotation = Rotation {
            name: job.rotation.clone(),
            default_minutes: None,
            ..Default::default()
        };
        let mut stage = Stage::new(
            &rotation,
            vec![slot.clone()],
            1,
            job.canvas,
            self.lookup.clone(),
            self.settings.max_cells,
        )?;
        stage.resize(job.canvas, job.scale, 0.0);
        if px > design {
            stage.set_tuned(Some(Tuning {
                rotation: job.rotation.clone(),
                canvas: [job.canvas[0].round() as u32, job.canvas[1].round() as u32],
                scale: job.scale,
                scenes: vec![TunedScene {
                    scene: slot.label.clone(),
                    design_px: design,
                    cell_px: px,
                    ..Default::default()
                }],
                ..Default::default()
            }));
        }
        let fps = self.settings.fps.max(1.0) as f64;
        let interval = Duration::from_secs_f64(1.0 / fps);
        let budget = Duration::from_secs_f64(STEP_SHARE as f64 / fps / self.screens.max(1) as f64);
        let t0 = Instant::now();
        // Built on the stage's own thread, as in the host.
        let mut notes = vec![];
        loop {
            stage.tick(t0.elapsed().as_secs_f64(), &mut self.sim, budget);
            notes.extend(stage.take_notes());
            if stage.world().is_some() {
                break;
            }
            if stage.paused() {
                return Err(if notes.is_empty() {
                    "could not be built".into()
                } else {
                    notes.join("; ")
                });
            }
            if t0.elapsed().as_secs_f64() > BUILD_LIMIT {
                return Err(format!("took over {BUILD_LIMIT} s to build"));
            }
            renew();
            std::thread::sleep(Duration::from_millis(20));
        }
        let world = stage.world().expect("built");
        let cells = world.topo.len() as u64;
        let on = steps_on_gpu(&slot.profile, &self.sim, world);
        stage.set_world_gpu(on);
        let canvas = [job.canvas[0].round() as u32, job.canvas[1].round() as u32];
        let mut draw = self.gpu.as_ref().map(|g| {
            let tex = g.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("jetcarp-tune-target"),
                size: wgpu::Extent3d {
                    width: canvas[0].max(1),
                    height: canvas[1].max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            (Painter::new(&g.device, FORMAT), tex, view)
        });
        let mut frame = |stage: &mut Stage, sim: &mut SimBackend, clock: Clock| -> (f64, f64) {
            let start = Instant::now();
            let now = t0.elapsed().as_secs_f64();
            let gpu = self.gpu.as_ref();
            let timer = self
                .timer
                .as_ref()
                .filter(|_| matches!(clock, Clock::Timestamps | Clock::Undecided));
            if let (Some(g), Some(t)) = (gpu, timer) {
                t.mark(g, 0);
            }
            let s0 = Instant::now();
            stage.tick(now, sim, budget);
            let step_wall = s0.elapsed().as_secs_f64();
            if let (Some(g), Some(t)) = (gpu, timer) {
                t.mark(g, 1);
            }
            let mut gpu_secs = 0.0;
            if let (Some(g), Some((painter, _, view))) = (gpu, draw.as_mut()) {
                painter.paint_timed(
                    &g.device,
                    &g.queue,
                    view,
                    canvas,
                    job.scale as f32,
                    stage,
                    &self.settings.caption,
                    now,
                    timer.map(Timer::draw_writes),
                );
                match (timer, clock) {
                    (Some(t), _) => {
                        let (s, d) = t.read(g);
                        gpu_secs = s + d;
                    }
                    // The system counts it; the frame is over once the GPU has done it.
                    (None, Clock::Kernel) => {
                        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
                    }
                    (None, _) => {
                        // Submission to completion: an upper bound. The step waits for its own
                        // work, so its share is its time while on the GPU.
                        let submitted = Instant::now();
                        let done = Arc::new(Mutex::new(None));
                        let d = done.clone();
                        g.queue.on_submitted_work_done(move || {
                            if let Ok(mut x) = d.lock() {
                                *x = Some(Instant::now());
                            }
                        });
                        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
                        let drawn = done
                            .lock()
                            .ok()
                            .and_then(|x| *x)
                            .map_or(0.0, |t| t.duration_since(submitted).as_secs_f64());
                        let stepped = matches!(sim.status(), SimStatus::Gpu(_));
                        gpu_secs = drawn + if stepped { step_wall } else { 0.0 };
                    }
                }
            }
            let took = start.elapsed();
            if let Some(rest) = interval.checked_sub(took) {
                std::thread::sleep(rest);
            }
            (took.as_secs_f64(), gpu_secs)
        };
        let k0 = crate::power::process_gpu_time();
        let warm = Instant::now();
        while warm.elapsed().as_secs_f64() < WARMUP {
            frame(&mut stage, &mut self.sim, self.clock);
        }
        if self.clock == Clock::Undecided {
            // The system's count, where it moved while the warm-up drew; else the GPU's clock.
            let k1 = crate::power::process_gpu_time();
            self.clock = match (k0, k1) {
                (Some(a), Some(b)) if b > a => Clock::Kernel,
                _ if self.timer.is_some() => Clock::Timestamps,
                _ => Clock::Submission,
            };
        }
        let clock = self.clock;
        notes.extend(stage.take_notes());
        let gen0 = stage.world().map_or(0, |w| w.generation);
        let meter = ProcessMeter::start();
        let counted0 = crate::power::process_gpu_time();
        let m0 = Instant::now();
        let (mut times, mut gpu_total) = (vec![], 0.0);
        while m0.elapsed().as_secs_f64() < seconds {
            let (t, g) = frame(&mut stage, &mut self.sim, clock);
            times.push(t);
            gpu_total += g;
        }
        let wall = m0.elapsed().as_secs_f64();
        let cpu = meter.map_or(0.0, |m| m.share());
        if clock == Clock::Kernel
            && let (Some(a), Some(b)) = (counted0, crate::power::process_gpu_time())
        {
            gpu_total = b.saturating_sub(a).as_secs_f64();
        }
        drop(draw);
        let gens_done = stage.world().map_or(0, |w| w.generation).saturating_sub(gen0) as f64;
        let speed = slot.profile.speed as f64;
        let on_gpu = matches!(self.sim.status(), SimStatus::Gpu(_));
        let gpu_timing = self.gpu_timing();
        Ok(Measure {
            cpu,
            gpu: self.gpu.as_ref().map(|_| (gpu_total / wall).clamp(0.0, 1.0)),
            p95: p95(&mut times),
            gens_wanted: (speed > 0.0).then_some(speed * wall),
            gens_done,
            on_gpu,
            gpu_timing,
            cells,
        })
    }
}

/// How the trials read the GPU's time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Clock {
    /// No GPU.
    None,
    /// Not known yet: the first trial's warm-up finds out whether the system counts it.
    Undecided,
    /// The system's count of this process's GPU time ([`crate::power::process_gpu_time`]).
    Kernel,
    /// Timestamp queries around each frame's step and drawing.
    Timestamps,
    /// From handing each frame's work to the GPU until it is done.
    Submission,
}

/// The 95th percentile.
fn p95(times: &mut [f64]) -> f64 {
    if times.is_empty() {
        return 0.0;
    }
    times.sort_by(f64::total_cmp);
    let k = ((times.len() as f64 * 0.95).ceil() as usize).clamp(1, times.len()) - 1;
    times[k]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_95th_percentile_and_monitor_canvases() {
        let mut t: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(p95(&mut t), 95.0);
        assert_eq!(p95(&mut [3.0]), 3.0);
        assert_eq!(p95(&mut []), 0.0);
        let mons = [
            Probed {
                pos: (-1920, 0),
                size: (1920, 1080),
                scale: 1.0,
                hz: Some(60.0),
            },
            Probed {
                pos: (0, 0),
                size: (2560, 1440),
                scale: 1.25,
                hz: Some(144.0),
            },
        ];
        assert_eq!(
            monitor_canvas(&mons, Some(0)),
            Some(([1920.0, 1080.0], 1.0, Some(60.0)))
        );
        assert_eq!(
            monitor_canvas(&mons, None),
            Some(([2560.0, 1440.0], 1.25, Some(144.0))),
            "every monitor: the primary's size"
        );
        assert_eq!(monitor_canvas(&mons, Some(5)), None);
        assert_eq!(monitor_canvas(&[], None), None);
    }
}
