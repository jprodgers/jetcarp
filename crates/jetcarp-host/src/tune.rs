//! `jetcarp host tune`: *Fit to this computer*. A sweep through the sizes each scene of the desktop
//! background can show at ([`jetcarp_display::tuning`]): each size shown for a couple of seconds,
//! measured, and pictured, then a size chosen for each scene (the recommended one unless `--pick`
//! says otherwise, or someone chose before).
//!
//! With the background host running, the host runs the sweep on the real screen and this command
//! follows it. With no host running (or `--offscreen`), the sweep runs here, off screen, at each
//! screen's size, the way the host runs a scene (the same stage, stepping and drawing, at the
//! host's frame rate); it runs in a process of its own, so the processor time it measures is its
//! own.

use crate::core::{STEP_SHARE, SharedGpu, load_rotation};
use crate::power::ProcessMeter;
use jetcarp_core::render::Image;
use jetcarp_core::{SizeSpec, World};
use jetcarp_display::host_settings::{
    GpuTiming, HostSettings, Place, Size, TuneLimits, TunedScene, Tuning, crops_dir,
};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{self, Profile};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::{SimBackend, SimStatus};
use jetcarp_display::stage::{self, Lookup, Stage};
use jetcarp_display::tuning::{
    self, Measure, Pick, RUNG_SECONDS, SWEEP_MAX_CELLS, SceneLimits, SceneSweep, TuneEvent,
    TuneRequest, crop_name, crop_size, lattice_ladder, patch_ladder, tuned_scene, warmup,
};
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::select::Mode;
use jetcarp_mcp::link::host_call;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Scenes swept per screen; the rest keep their own sizes.
pub(crate) const MAX_SCENES: usize = 40;
/// A world that takes longer than this to build is given up on.
pub(crate) const BUILD_LIMIT: f64 = 60.0;
/// How often the host's `tuning` hold is renewed. The host lets go after 30 s without.
const HOLD_EVERY: f64 = 5.0;
/// The canvas when nothing says what the screen's is.
const FALLBACK_CANVAS: [f64; 2] = [1920.0, 1080.0];
/// The offscreen target's format: plain, as the host's surfaces are.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// How often a running host's sweep is asked for news.
const FOLLOW_EVERY: Duration = Duration::from_millis(300);

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
    /// Which size each scene is left at: recommended (the finest within the limits; the default),
    /// finest (the finest shown), own (the scene's own), or a cell size in pixels. Without it, a
    /// size chosen before is kept while it is still within the limits.
    #[arg(long, value_name = "WHICH")]
    pub pick: Option<String>,
    /// Save each size's picture here as a PNG (they are kept beside the host's settings too).
    #[arg(long, value_name = "DIR")]
    pub crops: Option<PathBuf>,
    /// The screen's size in pixels, as WxH, when no host is running to say (default: the size it
    /// was tuned for before, else its monitor's).
    #[arg(long, value_parser = crate::parse_canvas)]
    pub canvas: Option<[f64; 2]>,
    /// Seconds each size shows for (the last three quarters are measured).
    #[arg(long, default_value_t = RUNG_SECONDS)]
    pub trial_seconds: f64,
    /// Measure and report, but save nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Stop the sweep a running host is making (the scenes already done stay saved).
    #[arg(long)]
    pub stop: bool,
    /// Sweep here, off screen, even with the host running.
    #[arg(long, hide = true)]
    pub offscreen: bool,
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
        // Whoever started it may have gone; the sweep carries on and saves what it finds.
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    fn warn(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "warning: {line}");
    }
}

/// One screen to sweep.
struct Job {
    screen: usize,
    rotation: String,
    canvas: [f64; 2],
    scale: f64,
    refresh: Option<f32>,
    slots: Vec<Slot>,
}

/// Runs the sweep, reporting as it goes. `json`: JSON lines on stdout.
pub fn run(args: TuneArgs, json: bool) -> Result<(), String> {
    let out = Out { json };
    let r = if args.stop {
        host_call(&json!({"action": "tune", "on": false})).map(|_| println!("stopped"))
    } else if !args.offscreen && host_call(&json!({"action": "status"})).is_ok() {
        follow_host(&args, &out)
    } else {
        tune(&args, &out)
    };
    if let (Err(e), true) = (&r, json) {
        out.event(&TuneEvent::Failed { error: e.clone() });
    }
    r
}

/// The request the options make.
fn request(args: &TuneArgs) -> Result<TuneRequest, String> {
    use jetcarp_display::host_settings::TuneLimits as L;
    limit(args.cpu_limit, L::CPU_RANGE, "the processor limit")?;
    limit(args.gpu_limit, L::GPU_RANGE, "the GPU limit")?;
    limit(args.fps, L::FPS_RANGE, "the frame rate")?;
    if !(0.5..=60.0).contains(&args.trial_seconds) {
        return Err("each size shows for 0.5 to 60 seconds".into());
    }
    Ok(TuneRequest {
        cpu_percent: args.cpu_limit,
        gpu_percent: args.gpu_limit,
        fps: args.fps,
        screen: args.screen,
        pick: args.pick.as_deref().map(Pick::parse).transpose()?,
        dry_run: args.dry_run,
        auto: args.auto,
    })
}

/// Asks the running host to sweep on the real screen, and reports its news until it is done.
fn follow_host(args: &TuneArgs, out: &Out) -> Result<(), String> {
    let req = request(args)?;
    let mut ask = json!({"action": "tune", "seconds": args.trial_seconds, "dry_run": req.dry_run});
    for (k, v) in [
        ("cpu_limit", req.cpu_percent),
        ("gpu_limit", req.gpu_percent),
        ("fps", req.fps),
    ] {
        if let Some(v) = v {
            ask[k] = json!(v);
        }
    }
    if let Some(s) = req.screen {
        ask["screen"] = json!(s);
    }
    if let Some(p) = req.pick {
        ask["pick"] = json!(p.word());
    }
    host_call(&ask)?;
    let mut seen = 0;
    let mut crops = vec![];
    loop {
        std::thread::sleep(FOLLOW_EVERY);
        let v = host_call(&json!({"action": "tune_events", "since": seen}))?;
        let events: Vec<TuneEvent> =
            serde_json::from_value(v["events"].clone()).map_err(|e| e.to_string())?;
        seen += events.len();
        let mut end = None;
        for e in events {
            if let TuneEvent::Measured { rung, .. } = &e
                && let Some(c) = &rung.crop
            {
                crops.push(c.clone());
            }
            out.event(&e);
            match e {
                TuneEvent::Done { .. } => end = Some(Ok(())),
                TuneEvent::Failed { error } => end = Some(Err(error)),
                _ => {}
            }
        }
        if let Some(r) = end {
            copy_crops(&crops, args.crops.as_deref())?;
            return r;
        }
        if v["running"] == false {
            return Err("the host's sweep ended without saying how".into());
        }
    }
}

/// Copies the named pictures from beside the host's settings into `dir`.
fn copy_crops(names: &[String], dir: Option<&Path>) -> Result<(), String> {
    let (Some(dir), Some(from)) = (dir, crops_dir()) else {
        return Ok(());
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for n in names {
        let to = dir.join(n);
        std::fs::copy(from.join(n), &to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// Only one sweep at a time: the app's, the host's and the command line's would measure each
/// other.
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
        Err(std::fs::TryLockError::WouldBlock) => Err("a sweep is already running".into()),
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

/// What a sweep needs to know of one scene: its own size, whether it is a placed pattern, and the
/// sizes to show.
pub(crate) struct ScenePlan {
    pub label: String,
    pub tiling: String,
    /// Its own cell size, in physical pixels.
    pub design: f32,
    pub placed: bool,
    pub sweep: SceneSweep,
    pub note: Option<String>,
}

/// The plan for `slot` on a canvas of `canvas` physical pixels at `scale` pixels per point, its
/// sizes holding at most `max_cells` cells.
pub(crate) fn scene_plan(
    slot: &Slot,
    lookup: &Lookup,
    canvas: [f64; 2],
    scale: f64,
    max_cells: u64,
) -> ScenePlan {
    let design = stage::cell_pixels(slot.profile.cell_px, scale);
    let (p, scene, _) = profiles::resolve(&slot.profile, &**lookup);
    let own = Size::px(design);
    let (sizes, note) = if !profiles::is_lattice(&scene) {
        // Its own size (as the scene shows it, at its own level), and each other level fitted to
        // the screen.
        let own_level = match scene.tiling.size {
            Some(SizeSpec::Level(l)) => Some(l),
            _ => Some(jetcarp_core::aperiodic::default_level(&scene.tiling.family)),
        };
        let mut v = patch_ladder(&scene.tiling.family, canvas, None, max_cells);
        v.retain(|s| s.level != own_level);
        v.push(own);
        v.sort_by(|a, b| a.px.total_cmp(&b.px));
        (v, None)
    } else if p.refill {
        (lattice_ladder(canvas, design, max_cells), None)
    } else {
        (
            vec![own],
            Some("not rebuilt to fill the screen, so its cell count is the scene's own".into()),
        )
    };
    ScenePlan {
        label: slot.label.clone(),
        tiling: scene.tiling.family.clone(),
        design,
        placed: tuning::is_placed(&scene),
        sweep: SceneSweep::new(sizes, own),
        note,
    }
}

/// A tuning that shows the scene labelled `label` (its own size `design`) at `size` on a
/// `canvas` at `scale`: what a sweep gives a stage for each size. None for the scene's own size.
pub(crate) fn showing(
    rotation: &str,
    canvas: [f64; 2],
    scale: f64,
    label: &str,
    design: f32,
    size: Size,
) -> Option<Tuning> {
    if size.level.is_none() && (size.px - design).abs() < 1e-3 {
        return None;
    }
    Some(Tuning {
        rotation: rotation.to_string(),
        canvas: [canvas[0].round() as u32, canvas[1].round() as u32],
        scale,
        scenes: vec![TunedScene {
            scene: label.to_string(),
            design_px: design,
            cell_px: size.px,
            level: size.level,
            ..Default::default()
        }],
        ..Default::default()
    })
}

/// The most cells a sweep's sizes hold: the sweep's limit, the host's, and the GPU's buffers'.
pub(crate) fn max_cells(settings: &HostSettings, gpu: Option<&SharedGpu>) -> u64 {
    let gpu = gpu.map_or(u64::MAX, |g| {
        let l = g.device.limits();
        l.max_storage_buffer_binding_size.min(l.max_buffer_size) / 4
    });
    SWEEP_MAX_CELLS.min(settings.max_cells).min(gpu)
}

/// Saves a size's picture beside the host's settings (and in `extra`), and says its file's name.
pub(crate) fn save_crop(name: &str, image: &Image, extra: Option<&Path>) -> Option<String> {
    let png = jetcarp_core::media::encode::png(image).ok()?;
    let mut saved = false;
    for dir in [crops_dir(), extra.map(Path::to_path_buf)]
        .into_iter()
        .flatten()
    {
        if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(name), &png).is_ok() {
            saved = true;
        }
    }
    saved.then(|| name.to_string())
}

/// Saves one scene's result into `host.json`, read afresh (the settings may have changed while the
/// sweep ran), for screen `screen` while it still runs `rotation`. Returns the file.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_scene(
    screen: usize,
    rotation: &str,
    canvas: [f64; 2],
    scale: f64,
    limits: TuneLimits,
    fps: f32,
    result: &TunedScene,
) -> Result<String, String> {
    let (mut fresh, _) = HostSettings::load();
    fresh.tune = TuneLimits {
        auto: fresh.tune.auto,
        ..limits
    };
    if let Some(s) = fresh.screens.get_mut(screen)
        && s.rotation == rotation
    {
        let canvas = [canvas[0].round() as u32, canvas[1].round() as u32];
        let t = s.tuning.get_or_insert_with(Tuning::default);
        if t.rotation != rotation || t.canvas != canvas {
            // Found for another rotation or screen size: start again for this one, but keep the
            // choices made, for the sweep to keep while they fit.
            t.scenes.retain(|x| x.chosen);
        }
        t.rotation = rotation.to_string();
        t.canvas = canvas;
        t.scale = scale;
        t.limits = limits;
        t.fps = fps;
        t.scenes.retain(|x| x.scene != result.scene);
        t.scenes.push(result.clone());
    }
    fresh.save()?;
    Ok(jetcarp_display::host_settings::config_dir()
        .map(|d| d.join("host.json").display().to_string())
        .unwrap_or_default())
}

/// The scene called `label` as tuned before on screen `screen`, if it was.
pub(crate) fn tuned_before(
    settings: &HostSettings,
    screen: usize,
    rotation: &str,
    label: &str,
) -> Option<TunedScene> {
    settings
        .tuning_for(screen, rotation)?
        .scenes
        .iter()
        .find(|s| s.scene == label)
        .cloned()
}

fn tune(args: &TuneArgs, out: &Out) -> Result<(), String> {
    let req = request(args)?;
    let _lock = lock()?;
    let (mut settings, warning) = HostSettings::load();
    if let Some(w) = warning {
        out.warn(&w);
    }
    if let Some(x) = req.cpu_percent {
        settings.tune.cpu_percent = x;
    }
    if let Some(x) = req.gpu_percent {
        settings.tune.gpu_percent = x;
    }
    if req.fps.is_some() {
        settings.tune.fps = req.fps;
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
    let most = max_cells(&settings, gpu.as_ref());
    let mut rig = Rig::new(gpu, &settings, jobs.len());
    let mut number = 0;
    let mut saved = None;
    for job in &jobs {
        let fps = limits.target_fps(job.refresh, settings.fps);
        let scene_limits = SceneLimits::of(&limits, fps, jobs.len());
        for (k, slot) in job.slots.iter().enumerate() {
            number += 1;
            let mut plan = scene_plan(slot, &rig.lookup, job.canvas, job.scale, most);
            let mut failed = None;
            let mut rung = 0;
            while let Some(size) = plan.sweep.next_size() {
                rung += 1;
                hold.renew();
                out.event(&TuneEvent::Rung {
                    screen: job.screen,
                    number,
                    of: total,
                    scene: slot.label.clone(),
                    size,
                    rung,
                    rungs: plan.sweep.planned(),
                });
                let tried = rig.trial(job, slot, &plan, size, args.trial_seconds, &mut || {
                    hold.renew()
                });
                let (m, shown_px, image) = match tried {
                    Ok(t) => t,
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                };
                let r = plan.sweep.record(size, shown_px, &m, &scene_limits).clone();
                let name = crop_name(job.screen, &slot.label, &r.size);
                let crop = image.and_then(|i| save_crop(&name, &i, args.crops.as_deref()));
                if let Some(last) = plan.sweep.rungs.last_mut() {
                    last.crop = crop;
                }
                out.event(&TuneEvent::Measured {
                    screen: job.screen,
                    scene: slot.label.clone(),
                    rung: plan.sweep.rungs.last().cloned().unwrap_or(r),
                });
            }
            let before = tuned_before(&settings, job.screen, &job.rotation, &slot.label);
            let mut t = tuned_scene(
                &plan.label,
                &plan.tiling,
                plan.design,
                plan.placed,
                &plan.sweep,
                rig.gpu_timing(),
                req.pick,
                before.as_ref(),
            );
            let notes: Vec<String> = [plan.note.clone(), failed, t.note.clone()]
                .into_iter()
                .flatten()
                .collect();
            t.note = (!notes.is_empty()).then(|| notes.join("; "));
            out.event(&TuneEvent::Scene {
                screen: job.screen,
                result: t.clone(),
            });
            if !req.dry_run {
                saved = Some(save_scene(
                    job.screen,
                    &job.rotation,
                    job.canvas,
                    job.scale,
                    limits,
                    fps,
                    &t,
                )?);
            }
            let _ = k;
        }
    }
    if saved.is_some() && status.is_some() {
        let _ = host_call(&json!({"action": "reload"}));
    }
    drop(hold);
    out.event(&TuneEvent::Done {
        saved,
        scenes: total,
        gpu_timing: rig.gpu_timing(),
    });
    Ok(())
}

/// The screens to sweep, each with its size and its rotation's scenes.
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
        if self.on
            && self
                .at
                .is_none_or(|t| t.elapsed().as_secs_f64() >= HOLD_EVERY)
        {
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
pub(crate) fn steps_on_gpu(profile: &Profile, sim: &SimBackend, world: &World) -> bool {
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
        if !g
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
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
                for (k, c) in data.as_chunks::<8>().0.iter().take(4).enumerate() {
                    t[k] = u64::from_le_bytes(*c);
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
            Some(g) => SimBackend::from_device_with(
                &g.adapter,
                g.device.clone(),
                g.queue.clone(),
                settings.gpu_policy(),
            ),
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

    /// Shows `slot` at `size` for `seconds`, as the host would, and measures the last three
    /// quarters of them. Returns what it measured, the size its cells showed at, and a picture of
    /// the middle of the last frame. `renew` is called while a big world builds.
    fn trial(
        &mut self,
        job: &Job,
        slot: &Slot,
        plan: &ScenePlan,
        size: Size,
        seconds: f64,
        renew: &mut dyn FnMut(),
    ) -> Result<(Measure, f32, Option<Image>), String> {
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
        stage.set_tuned(showing(
            &job.rotation,
            job.canvas,
            job.scale,
            &plan.label,
            plan.design,
            size,
        ));
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
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
        let warm_for = warmup(seconds);
        while warm.elapsed().as_secs_f64() < warm_for {
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
        while m0.elapsed().as_secs_f64() < seconds - warm_for {
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
        let image = match (&self.gpu, &draw) {
            (Some(g), Some((_, tex, _))) => {
                let size = crop_size(job.canvas);
                jetcarp_display::crop::read(
                    &g.device,
                    &g.queue,
                    tex,
                    jetcarp_display::crop::middle(canvas, size),
                    size,
                )
            }
            _ => None,
        };
        drop(draw);
        let shown_px = stage.status().cell_px;
        let frames = times.len() as f64;
        let gens_done = stage
            .world()
            .map_or(0, |w| w.generation)
            .saturating_sub(gen0) as f64;
        let speed = slot.profile.speed as f64;
        let on_gpu = matches!(self.sim.status(), SimStatus::Gpu(_));
        let gpu_timing = self.gpu_timing();
        let m = Measure {
            cpu,
            gpu: self
                .gpu
                .as_ref()
                .map(|_| (gpu_total / wall).clamp(0.0, 1.0)),
            p95: p95(&mut times),
            fps: frames / wall.max(1e-9),
            gens_wanted: (speed > 0.0).then_some(speed * wall),
            gens_done,
            on_gpu,
            gpu_timing,
            cells,
        };
        Ok((m, shown_px, image))
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
