//! `jetcarp host tune`: *Fit to this computer*, in about ten seconds (#113). For each screen of
//! the desktop background, the rotation's most demanding scene is measured at a few sizes, every
//! screen at once, by a binary search ([`jetcarp_display::tuning`]); the size found serves the
//! whole rotation, each other scene going finer by its cost ratio, and each scene is left at its
//! recommended size unless `--pick` says otherwise or someone chose before.
//!
//! With the background host running, the host runs the fit on the real screens and this command
//! follows it. With no host running (or `--offscreen`), the fit runs here, off screen, at each
//! screen's size, the way the host runs a scene (the same stage, stepping and drawing, at the
//! frame rate the host would draw it at: [`jetcarp_display::pace`]); it runs in a process of its
//! own, so the processor time it measures is its own.
//!
//! `--pick` places sizes from the last fit while it is current (the same JETCARP, screens,
//! scenes, limits and GPU), measuring nothing; `--fresh` measures again (#77).

use crate::core::{STEP_SHARE, SharedGpu, load_rotation};
use crate::gpu_engine::EngineMeter;
use crate::power::ProcessMeter;
use jetcarp_core::render::Image;
use jetcarp_core::{SizeSpec, World};
use jetcarp_display::host_settings::{
    GpuTiming, HostSettings, Place, Size, TuneLimits, TunedScene, Tuning, crops_dir,
};
use jetcarp_display::pace::{FrameClock, Pacer};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{self, Profile};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::{SimBackend, SimStatus};
use jetcarp_display::stage::{self, Lookup, Stage};
use jetcarp_display::tuning::{
    self, MARGIN, Measure, Pick, Planned, SWEEP_MAX_CELLS, SceneLimits, ScreenShare, Search,
    TRIAL_SECONDS, TuneEvent, TuneRequest, area_shares, attribute, cells_at, crop_name, crop_size,
    device_cost, fps_range, lattice_ladder, patch_ladder, scene_fps, scene_groups, screen_results,
    warmup,
};
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::select::Mode;
use jetcarp_mcp::link::host_call;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A world that takes longer than this to build is given up on.
pub(crate) const BUILD_LIMIT: f64 = 60.0;
/// How often the host's `tuning` hold is renewed. The host lets go after 30 s without.
const HOLD_EVERY: f64 = 5.0;
/// The canvas when nothing says what the screen's is.
const FALLBACK_CANVAS: [f64; 2] = [1920.0, 1080.0];
/// The offscreen target's format: plain, as the host's surfaces are.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// How often a running host's fit is asked for news.
const FOLLOW_EVERY: Duration = Duration::from_millis(300);
/// Neighbour reads a Life cell makes a generation on squares: what the score's one-core rate
/// counts in.
const LIFE_READS: f64 = 8.0;

/// `jetcarp host tune` options.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct TuneArgs {
    /// JETCARP's processor time, in percent of the whole machine, every core counted, for the
    /// whole desktop: shared between the screens by their pixel area (saved; default 10).
    #[arg(long, value_name = "PERCENT")]
    pub cpu_limit: Option<f32>,
    /// JETCARP's share of the GPU's time, in percent, for the whole desktop, shared the same way
    /// (saved; default 25).
    #[arg(long, value_name = "PERCENT")]
    pub gpu_limit: Option<f32>,
    /// Frames per second it must keep (saved; default: the monitor's refresh rate, or 60, at most
    /// the rate each scene draws at).
    #[arg(long)]
    pub fps: Option<f32>,
    /// Only this screen (counted from 0); it keeps its share of the limits. Default: every screen
    /// on the desktop.
    #[arg(long)]
    pub screen: Option<usize>,
    /// Which size each scene is left at: recommended (the finest within the limits; the default),
    /// finest (the finest tried), own (the scene's own), or a cell size in pixels. Recommended
    /// gives the rest of a rotation at most twice the measured scene's cells; the others are
    /// choices, and go past that. Without it, a size chosen before is kept while it is still within
    /// the limits. With it, the last fit's sizes are used while they are current; nothing is
    /// measured.
    #[arg(long, value_name = "WHICH")]
    pub pick: Option<String>,
    /// With --pick: measure again, even though the last fit is still current.
    #[arg(long)]
    pub fresh: bool,
    /// Save each size's picture here as a PNG (they are kept beside the host's settings too).
    #[arg(long, value_name = "DIR")]
    pub crops: Option<PathBuf>,
    /// The screen's size in pixels, as WxH, when no host is running to say (default: the size it
    /// was fitted for before, else its monitor's).
    #[arg(long, value_parser = crate::parse_canvas)]
    pub canvas: Option<[f64; 2]>,
    /// Seconds each size shows for (the last three quarters are measured; a fit's first sizes warm
    /// up half a second longer).
    #[arg(long, default_value_t = TRIAL_SECONDS)]
    pub trial_seconds: f64,
    /// Measure and report, but save nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Stop the fit a running host is making (nothing it found is saved).
    #[arg(long)]
    pub stop: bool,
    /// Fit here, off screen, even with the host running.
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
        // Whoever started it may have gone; the fit carries on and saves what it finds.
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    fn warn(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "warning: {line}");
    }
}

/// One screen of the desktop: what it shows and where.
pub(crate) struct Job {
    pub screen: usize,
    pub rotation: String,
    pub canvas: [f64; 2],
    pub scale: f64,
    pub refresh: Option<f32>,
    pub slots: Vec<Slot>,
}

/// Runs the fit, reporting as it goes. `json`: JSON lines on stdout.
pub fn run(args: TuneArgs, json: bool) -> Result<(), String> {
    let out = Out { json };
    let r = jetcarp_core::fit::available().and_then(|()| {
        if args.stop {
            host_call(&json!({"action": "tune", "on": false})).map(|_| println!("stopped"))
        } else if !args.offscreen && host_call(&json!({"action": "status"})).is_ok() {
            follow_host(&args, &out)
        } else {
            tune(&args, &out)
        }
    });
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
    if args.fresh && args.pick.is_none() {
        return Err("--fresh goes with --pick (a fit without --pick always measures)".into());
    }
    Ok(TuneRequest {
        cpu_percent: args.cpu_limit,
        gpu_percent: args.gpu_limit,
        fps: args.fps,
        screen: args.screen,
        screens: vec![],
        pick: args.pick.as_deref().map(Pick::parse).transpose()?,
        fresh: args.fresh,
        dry_run: args.dry_run,
        auto: args.auto,
    })
}

/// Asks the running host to fit on the real screens, and reports its news until it is done.
fn follow_host(args: &TuneArgs, out: &Out) -> Result<(), String> {
    let req = request(args)?;
    let mut ask = json!({"action": "tune", "seconds": args.trial_seconds, "dry_run": req.dry_run,
        "fresh": req.fresh});
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
            return Err("the host's fit ended without saying how".into());
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

/// Only one fit at a time: the app's, the host's and the command line's would measure each
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
        Err(std::fs::TryLockError::WouldBlock) => Err("a fit is already running".into()),
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

/// The screens shown at once, among which the limits are shared: those on the desktop, or with
/// none there, every screen.
pub(crate) fn shown_screens(places: &[Place]) -> Vec<usize> {
    let walls: Vec<usize> = (0..places.len())
        .filter(|&i| places[i] == Place::Wallpaper)
        .collect();
    if walls.is_empty() {
        (0..places.len()).collect()
    } else {
        walls
    }
}

/// About how many neighbour reads a generation of `scene`'s automata makes per cell: a small
/// world of its tiling and rules, built to count them ([`jetcarp_gpu::neighbour_reads`]). None
/// when it can't be built.
fn reads_per_cell(scene: &jetcarp_core::Scene) -> Option<f64> {
    let mut t = scene.tiling.clone();
    t.size = Some(if profiles::is_lattice(scene) {
        SizeSpec::Rect { w: 16, h: 16 }
    } else {
        let family = &t.family;
        let (lo, hi) = jetcarp_core::aperiodic::level_range(family);
        let level = (lo..=hi)
            .find(|&l| jetcarp_core::aperiodic::tile_count(family, l).is_some_and(|n| n >= 200))
            .unwrap_or(lo);
        SizeSpec::Level(level)
    });
    let mut w = World::new(t, 1).ok()?;
    for ca in &scene.cas {
        let mut c = ca.clone();
        c.seed = None;
        w.add_ca(c).ok()?;
    }
    let n = w.topo.len().max(1) as f64;
    Some(jetcarp_gpu::neighbour_reads(&w) as f64 / n)
}

/// Generations a rule is given to change a cell of its seed ([`changes_its_seed`]).
const DRAWS_WITHIN: u32 = 4;

/// Whether `scene` changes any cell of its seed within its first few generations, on a small world
/// of its tiling (its own size where a small one can't be built): Wolfram's rule 0, from its one
/// cell, changes none.
fn changes_its_seed(scene: &jetcarp_core::Scene) -> bool {
    let mut small = scene.clone();
    if profiles::is_lattice(scene) {
        small.tiling.size = Some(SizeSpec::Rect { w: 32, h: 32 });
    }
    let Ok(mut w) = World::from_scene(&small).or_else(|_| World::from_scene(scene)) else {
        return false;
    };
    let seeded = w.states_hash();
    (0..DRAWS_WITHIN).any(|_| {
        w.step();
        w.states_hash() != seeded
    })
}

/// The rule a series is measured and pictured by: the first of its rules (`first`, then `rest`,
/// in the rotation's order) that changes cells of its seed ([`changes_its_seed`]), so its cards
/// show a picture (#346: the Wolfram series' first rule, rule 0, draws nothing from its one
/// cell); every rule of a series costs the same a cell, so what the fit measures is the same.
/// Its first rule when none does. Returns that rule's slot, and the others' in order.
fn series_shown_by(
    slots: &[Slot],
    lookup: &Lookup,
    first: usize,
    rest: Vec<usize>,
) -> (usize, Vec<usize>) {
    if rest.is_empty() {
        return (first, rest);
    }
    let all: Vec<usize> = std::iter::once(first).chain(rest).collect();
    let by = all
        .iter()
        .copied()
        .find(|&i| changes_its_seed(&profiles::resolve(&slots[i].profile, &**lookup).1))
        .unwrap_or(first);
    (by, all.into_iter().filter(|&i| i != by).collect())
}

/// A screen's scenes as a fit sees them: a series' rules as one scene (#109), measured and
/// pictured by a rule that draws ([`series_shown_by`]), each with its sizes on `canvas` at
/// `scale` (at most `max_cells` cells), its cost, and the frame rate it is held to. `reads`
/// caches the neighbour reads of each tiling and set of rules.
pub(crate) fn plan_scenes(
    slots: &[Slot],
    lookup: &Lookup,
    canvas: [f64; 2],
    scale: f64,
    max_cells: u64,
    limits: &TuneLimits,
    pacer: &Pacer,
    devices: Devices,
    reads: &mut HashMap<String, Option<f64>>,
) -> Vec<Planned> {
    scene_groups(slots)
        .into_iter()
        .map(|(first, rest)| {
            let (k, rest) = series_shown_by(slots, lookup, first, rest);
            let slot = &slots[k];
            let design = stage::cell_pixels(slot.profile.cell_px, scale);
            // A source that can't be had (a Quick Start entry missing here) shows Life, as in
            // the host: its cards say so (#45).
            let (p, scene, missing) = profiles::resolve(&slot.profile, &**lookup);
            let lattice = profiles::is_lattice(&scene);
            let (sizes, own, fixed, note) = if !lattice {
                let own_level = match scene.tiling.size {
                    Some(SizeSpec::Level(l)) => l,
                    _ => jetcarp_core::aperiodic::default_level(&scene.tiling.family),
                };
                let sizes = patch_ladder(&scene.tiling.family, canvas, Some(own_level), max_cells);
                let own = sizes
                    .iter()
                    .find(|s| s.level == Some(own_level))
                    .copied()
                    .unwrap_or(Size::px(design));
                (sizes, own, false, None)
            } else if p.refill {
                (
                    lattice_ladder(canvas, design, max_cells),
                    Size::px(design),
                    false,
                    None,
                )
            } else {
                (
                    vec![Size::px(design)],
                    Size::px(design),
                    true,
                    Some(
                        "not rebuilt to fill the screen, so its cell count is the scene's own"
                            .into(),
                    ),
                )
            };
            let key = format!(
                "{}|{:?}|{}",
                scene.tiling.family,
                scene.tiling.params,
                scene
                    .cas
                    .iter()
                    .map(|c| format!("{}/{:?}/{}", c.rule, c.neighborhood, c.steps_per_gen))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let r = *reads.entry(key).or_insert_with(|| reads_per_cell(&scene));
            let pace = pacer.pace(p.speed);
            // Its rules all run on the GPU, and the settings let it.
            let gpu = devices.gpu
                && p.gpu
                && scene.cas.iter().filter(|c| c.enabled).all(|c| {
                    jetcarp_core::rules::Rule::parse(&c.rule)
                        .is_ok_and(|(r, _)| jetcarp_gpu::supports_rule(&r))
                });
            let note = match (missing, note) {
                (Some(m), Some(n)) => Some(format!("{m}; {n}")),
                (m, n) => m.or(n),
            };
            Planned {
                label: slot.label.clone(),
                also: rest.iter().map(|&i| slots[i].label.clone()).collect(),
                tiling: scene.tiling.family.clone(),
                design,
                own,
                placed: tuning::is_placed(&scene),
                fixed,
                sizes,
                cost: r.map(|r| {
                    device_cost(r, pace.speed as f64, pace.fps as f64, gpu, devices.speedup)
                }),
                unpaced: pace.speed <= 0.0,
                gpu,
                fps: scene_fps(limits, pacer, p.speed),
                note,
            }
        })
        .collect()
}

/// Where a fit's scenes step: whether the GPU can be used at all, and how many times the
/// processor's speed it steps at ([`tuning::device_cost`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Devices {
    pub gpu: bool,
    pub speedup: f64,
}

/// What the machine's score says of how fast it is, for a fit's first guess: neighbour reads a
/// second on every core, and on the fastest GPU that gave right answers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Speeds {
    pub cpu: Option<f64>,
    pub gpu: Option<f64>,
}

impl Speeds {
    /// From the score saved in the settings folder (none: no guess).
    pub(crate) fn saved() -> Speeds {
        let file = jetcarp_display::host_settings::config_dir()
            .and_then(|d| jetcarp_core::machine::MachineFile::load(Some(&d)));
        Speeds::of(file.and_then(|f| f.score).as_ref())
    }

    /// From `score`: none from a short run, whose rates mean little, or from one taken under load
    /// (#301), which would guess sizes too small for this machine.
    pub(crate) fn of(score: Option<&jetcarp_core::machine::Score>) -> Speeds {
        let Some(score) = score.filter(|s| !s.quick && !s.under_load()) else {
            return Speeds::default();
        };
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
        let cpu = score
            .best("core-square")
            .map(|p| p.value * threads * LIFE_READS);
        use jetcarp_core::machine::bench::{SOUP_CELLS, SOUP_GENS};
        let gpu = score
            .parts
            .iter()
            .filter(|p| {
                p.id.starts_with("soups-")
                    && p.correct == Some(true)
                    && !p.device.starts_with("processor")
            })
            .map(|p| p.value * SOUP_CELLS as f64 * SOUP_GENS as f64 * LIFE_READS)
            .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))));
        Speeds { cpu, gpu }
    }

    /// How many times the processor's speed the GPU steps at, by the score; else
    /// [`tuning::GPU_SPEEDUP`].
    pub(crate) fn speedup(&self) -> f64 {
        match (self.cpu, self.gpu) {
            (Some(c), Some(g)) if c > 0.0 && g > 0.0 => (g / c).max(1.0),
            _ => tuning::GPU_SPEEDUP,
        }
    }

    /// The size to try first for a scene costing `cost` a cell held to `l`: the finest of `sizes`
    /// whose cells' work the machine's speed allows within the limits (on the GPU where it will
    /// step there). None without a score.
    pub(crate) fn guess(
        &self,
        p: &Planned,
        canvas: [f64; 2],
        l: &SceneLimits,
        on_gpu: bool,
    ) -> Option<Size> {
        let cost = p.cost?;
        let budget = match (on_gpu, self.gpu) {
            (true, Some(g)) => g * l.gpu * MARGIN as f64,
            _ => self.cpu? * l.cpu * MARGIN as f64,
        };
        let cells = budget / cost.max(1e-9);
        p.sizes
            .iter()
            .find(|s| (cells_at(&p.tiling, canvas, s) as f64) <= cells)
            .or(p.sizes.last())
            .copied()
    }
}

/// One scene a fit measures on a screen, and its search.
pub(crate) struct Measuring {
    /// Its index in the screen's scenes, and its slot in the rotation.
    pub scene: usize,
    pub slot: Slot,
    pub search: Search,
    /// Its limits: its screen's share, at its frame rate.
    pub limits: SceneLimits,
    pub fps: f32,
    /// Sizes that could not be shown, and why.
    pub failed: Vec<String>,
}

/// One screen's fit: its scenes, the ones measured (the most demanding on each device), and its
/// share of the limits.
pub(crate) struct ScreenFit {
    pub screen: usize,
    pub rotation: String,
    pub canvas: [f64; 2],
    pub scale: f64,
    pub refresh: Option<f32>,
    pub share: f64,
    pub scenes: Vec<Planned>,
    /// The scenes measured, each by its own search: on the GPU, then on the processor.
    pub measures: Vec<Measuring>,
    /// What was saved for this screen's rotation before.
    pub before: Option<Tuning>,
}

impl ScreenFit {
    /// Plans screen `screen`'s fit: its scenes, the most demanding on each device to measure
    /// ([`tuning::scenes_to_measure`]), and where each search starts (a size chosen before, else the
    /// one recommended last time, kept while it stays within the noise, #94; else the score's
    /// guess).
    pub(crate) fn plan(
        job: &Job,
        share: f64,
        lookup: &Lookup,
        settings: &HostSettings,
        limits: &TuneLimits,
        max_cells: u64,
        speeds: &Speeds,
        gpu_steps: bool,
        reads: &mut HashMap<String, Option<f64>>,
    ) -> ScreenFit {
        let pacer = settings.pacer(job.refresh);
        let devices = Devices {
            gpu: gpu_steps,
            speedup: speeds.speedup(),
        };
        let scenes = plan_scenes(
            &job.slots, lookup, job.canvas, job.scale, max_cells, limits, &pacer, devices, reads,
        );
        let before = settings.tuning_for(job.screen, &job.rotation).cloned();
        let same = before.as_ref().is_some_and(|t| {
            t.fits_monitor(job.canvas, job.scale, job.refresh)
                && t.limits.cpu_percent == limits.cpu_percent
                && t.limits.gpu_percent == limits.gpu_percent
                && t.limits.fps == limits.fps
                && (t.share - share).abs() < 1e-3
        });
        let measures = tuning::scenes_to_measure(&scenes)
            .into_iter()
            .filter_map(|m| {
                let p = &scenes[m];
                let slot = job.slots.iter().find(|s| s.label == p.label).cloned()?;
                let l = SceneLimits::of(limits, p.fps, share);
                let saved = before
                    .as_ref()
                    .and_then(|t| t.scenes.iter().find(|s| s.names(&p.label)));
                // The size recommended last time, where it was measured as now (#94).
                let keep = saved.filter(|s| s.measured && same).map(|s| s.recommended);
                let chosen = saved.filter(|s| s.chosen).map(|s| {
                    // A choice kept as the scene's own size is its own.
                    if s.size().level.is_none() && (s.size().px - p.design).abs() < 1e-3 {
                        p.own
                    } else {
                        s.size()
                    }
                });
                let first = chosen
                    .or(keep)
                    .or_else(|| speeds.guess(p, job.canvas, &l, p.gpu));
                Some(Measuring {
                    scene: m,
                    slot,
                    search: Search::new(p.sizes.clone(), p.own, first, keep),
                    limits: l,
                    fps: p.fps,
                    failed: vec![],
                })
            })
            .collect();
        ScreenFit {
            screen: job.screen,
            rotation: job.rotation.clone(),
            canvas: job.canvas,
            scale: job.scale,
            refresh: job.refresh,
            share,
            scenes,
            measures,
            before,
        }
    }

    /// The next size to show: of the first measured scene still searching (a screen shows one at
    /// a time), as (that scene, the size's index in its search).
    pub(crate) fn next_trial(&self) -> Option<(usize, usize)> {
        self.measures
            .iter()
            .enumerate()
            .find_map(|(k, m)| m.search.next().map(|i| (k, i)))
    }

    /// Its share of the limits, for the opening line.
    pub(crate) fn share_line(&self, limits: &TuneLimits) -> ScreenShare {
        let measured: Vec<String> = (0..self.measures.len()).map(|k| self.label(k)).collect();
        ScreenShare {
            screen: self.screen,
            canvas: [self.canvas[0].round() as u32, self.canvas[1].round() as u32],
            share: self.share,
            cpu_percent: limits.cpu_percent * self.share as f32,
            gpu_percent: limits.gpu_percent * self.share as f32,
            scene: measured.first().cloned(),
            measured,
            scenes: self.scenes.len(),
        }
    }

    /// The label of measured scene `k`.
    pub(crate) fn label(&self, k: usize) -> String {
        self.measures
            .get(k)
            .map_or(String::new(), |m| self.scenes[m.scene].label.clone())
    }

    /// Every scene's result.
    pub(crate) fn results(&self, gpu_timing: GpuTiming, pick: Option<Pick>) -> Vec<TunedScene> {
        let before = |label: &str| {
            self.before
                .as_ref()
                .and_then(|t| t.scenes.iter().find(|s| s.names(label)).cloned())
        };
        let measured: Vec<(usize, &Search)> =
            self.measures.iter().map(|m| (m.scene, &m.search)).collect();
        let mut out = screen_results(
            &self.scenes,
            &measured,
            self.canvas,
            gpu_timing,
            pick,
            &before,
        );
        for m in self.measures.iter().filter(|m| !m.failed.is_empty()) {
            if let Some(t) = out
                .iter_mut()
                .find(|t| t.scene == self.scenes[m.scene].label)
            {
                let notes: Vec<String> = t.note.iter().cloned().chain(m.failed.clone()).collect();
                t.note = Some(notes.join("; "));
            }
        }
        out
    }

    /// The screen's tuning, to save: every scene's result, and what it was found for.
    pub(crate) fn tuning(
        &self,
        scenes: Vec<TunedScene>,
        limits: TuneLimits,
        max_cells: u64,
        gpu: Option<String>,
    ) -> Tuning {
        Tuning {
            rotation: self.rotation.clone(),
            canvas: [self.canvas[0].round() as u32, self.canvas[1].round() as u32],
            scale: self.scale,
            limits,
            fps: self.measures.first().map_or(30.0, |m| m.fps),
            scenes,
            share: self.share,
            hz: self.refresh,
            max_cells,
            gpu,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// A screen as it is now, for whether its last fit still holds: its number, rotation, canvas,
/// pixels per point, its monitor's refresh rate, and its rotation's scenes' labels.
pub(crate) type ScreenNow = (usize, String, [f64; 2], f64, Option<f32>, Vec<String>);

/// What a trial measured, the size its cells showed at, and a picture of it; or why it could not
/// be shown.
type Measured = Result<(Measure, f32, Option<Image>), String>;

/// The saved fits `pick` can place sizes from without measuring, one for each screen to fit, if
/// every one is current (#77); None: measure.
pub(crate) fn reusable(
    fits: &[ScreenNow],
    settings: &HostSettings,
    limits: &TuneLimits,
    gpu: Option<&str>,
) -> Option<Vec<(usize, Tuning)>> {
    fits.iter()
        .map(|(screen, rotation, canvas, scale, hz, labels)| {
            let t = settings.tuning_for(*screen, rotation)?;
            t.current(
                rotation,
                labels,
                *canvas,
                *scale,
                *hz,
                limits,
                gpu,
                env!("CARGO_PKG_VERSION"),
            )
            .then(|| (*screen, t.clone()))
        })
        .collect()
}

/// A saved tuning with `pick` placed again.
pub(crate) fn repicked(t: &Tuning, pick: Pick) -> Tuning {
    let canvas = [t.canvas[0] as f64, t.canvas[1] as f64];
    let scenes = tuning::repick(&t.scenes, &|s| tuning::scene_sizes(t, s), canvas, pick);
    Tuning {
        scenes,
        ..t.clone()
    }
}

/// A tuning that shows the scene labelled `label` (its own size `design`) at `size` on a
/// `canvas` at `scale`: what a fit gives a stage for each size. None for the scene's own size.
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

/// The most cells a fit's sizes hold: the sweep's limit, the host's, and the GPU's buffers'.
pub(crate) fn max_cells(settings: &HostSettings, gpu: Option<&SharedGpu>) -> u64 {
    let gpu = gpu.map_or(u64::MAX, |g| {
        let l = g.device.limits();
        l.max_storage_buffer_binding_size.min(l.max_buffer_size) / 4
    });
    SWEEP_MAX_CELLS.min(settings.cell_limit()).min(gpu)
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

/// Saves one screen's fit into `host.json`, read afresh (the settings may have changed while the
/// fit ran), for screen `screen` while it still runs the rotation it was found for. A size chosen
/// for a scene of another rotation stays, for a fit to keep while it fits should it come back
/// (#73). Returns the file.
pub(crate) fn save_screen(
    screen: usize,
    limits: TuneLimits,
    tuning: Tuning,
) -> Result<String, String> {
    let (mut fresh, _) = HostSettings::load();
    fresh.tune = TuneLimits {
        auto: fresh.tune.auto,
        ..limits
    };
    if let Some(s) = fresh.screens.get_mut(screen)
        && s.rotation == tuning.rotation
    {
        let mut t = tuning;
        if let Some(old) = &s.tuning {
            for x in old.scenes.iter().filter(|x| x.chosen) {
                if !t.scenes.iter().any(|n| n.names(&x.scene)) {
                    t.scenes.push(x.clone());
                }
            }
        }
        s.tuning = Some(t);
    }
    fresh.save()?;
    Ok(jetcarp_display::host_settings::config_dir()
        .map(|d| d.join("host.json").display().to_string())
        .unwrap_or_default())
}

fn tune(args: &TuneArgs, out: &Out) -> Result<(), String> {
    let t0 = Instant::now();
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
    let limits = settings.tune.checked();
    if crate::power::on_battery() == Some(true) {
        if args.auto {
            return Err("on battery: not fitting until the computer is plugged in".into());
        }
        out.warn("on battery: a computer on battery may run slower than plugged in, so the sizes found may be coarser than it needs");
    }
    let status = host_call(&json!({"action": "status"})).ok();
    let jobs = plan(&settings, args, status.as_ref(), out)?;
    let shares = area_shares(&jobs.iter().map(|j| j.canvas).collect::<Vec<_>>());
    let wanted: Vec<usize> = (0..jobs.len())
        .filter(|&k| req.wants(jobs[k].screen))
        .collect();
    if wanted.is_empty() {
        return Err(match req.screen {
            Some(i) => format!("screen {i} is not shown"),
            None => "no screens to fit".into(),
        });
    }
    let gpu = SharedGpu::headless_with(wgpu::Features::TIMESTAMP_QUERY);
    if gpu.is_none() {
        out.warn("no GPU found: measuring on the processor alone");
    }
    let gpu_name = gpu.as_ref().map(|g| g.adapter.get_info().name);
    let lookup = jetcarp_display::stage::library_lookup();
    // `--pick` places the last fit's sizes while it is current (#77).
    if let (Some(pick), false) = (req.pick, req.fresh) {
        let asked: Vec<_> = wanted
            .iter()
            .map(|&k| {
                let j = &jobs[k];
                let labels = j.slots.iter().map(|s| s.label.clone()).collect();
                (
                    j.screen,
                    j.rotation.clone(),
                    j.canvas,
                    j.scale,
                    j.refresh,
                    labels,
                )
            })
            .collect();
        if let Some(saved) = reusable(&asked, &settings, &limits, gpu_name.as_deref()) {
            return place_saved(
                &saved,
                pick,
                &limits,
                &req,
                status.is_some(),
                gpu_name,
                out,
                t0,
            );
        }
    }
    let most = max_cells(&settings, gpu.as_ref());
    let speeds = Speeds::saved();
    let mut reads = HashMap::new();
    let gpu_steps = gpu.is_some() && settings.gpu_policy().mode != Mode::Off;
    let mut fits: Vec<ScreenFit> = wanted
        .iter()
        .map(|&k| {
            ScreenFit::plan(
                &jobs[k], shares[k], &lookup, &settings, &limits, most, &speeds, gpu_steps,
                &mut reads,
            )
        })
        .collect();
    let total: usize = fits.iter().map(|f| f.scenes.len()).sum();
    let (fps, fps_max) = fps_range(fits.iter().flat_map(|f| f.scenes.iter().map(|p| p.fps)));
    out.event(&TuneEvent::Start {
        screens: fits.iter().map(|f| f.screen).collect(),
        scenes: total,
        fps,
        fps_max,
        gpu: gpu_name.clone(),
        shares: fits.iter().map(|f| f.share_line(&limits)).collect(),
        limits: Some(limits),
        reused: false,
    });
    let mut hold = Hold::start(status.is_some());
    let mut rig = Rig::new(gpu, &settings, lookup);
    let mut trials = 0;
    // Every scene measured, counted across the screens, for the progress lines.
    let numbered: Vec<(usize, usize)> = fits
        .iter()
        .enumerate()
        .flat_map(|(k, f)| (0..f.measures.len()).map(move |m| (k, m)))
        .collect();
    let mut first = true;
    loop {
        // Each screen still searching shows its next size (of one measured scene at a time);
        // every screen is measured at once.
        let round: Vec<(usize, usize, usize)> = fits
            .iter()
            .enumerate()
            .filter_map(|(k, f)| f.next_trial().map(|(m, i)| (k, m, i)))
            .collect();
        if round.is_empty() {
            break;
        }
        for &(k, m, i) in &round {
            let f = &fits[k];
            let s = &f.measures[m].search;
            hold.renew();
            out.event(&TuneEvent::Rung {
                screen: f.screen,
                number: numbered.iter().position(|&x| x == (k, m)).unwrap_or(0) + 1,
                of: numbered.len(),
                scene: f.label(m),
                size: s.sizes[i],
                rung: s.tried() + 1,
                rungs: s.planned(),
            });
        }
        let specs: Vec<TrialSpec> = round
            .iter()
            .map(|&(k, m, i)| {
                let f = &fits[k];
                let ms = &f.measures[m];
                let p = &f.scenes[ms.scene];
                TrialSpec {
                    rotation: f.rotation.clone(),
                    canvas: f.canvas,
                    scale: f.scale,
                    refresh: f.refresh,
                    slot: ms.slot.clone(),
                    label: p.label.clone(),
                    design: p.design,
                    size: ms.search.sizes[i],
                }
            })
            .collect();
        let results = rig.round(&specs, args.trial_seconds, first, &mut || hold.renew());
        first = false;
        trials += round.len();
        for (&(k, m, i), r) in round.iter().zip(results) {
            let f = &mut fits[k];
            let screen = f.screen;
            let label = f.label(m);
            let ms = &mut f.measures[m];
            let l = ms.limits;
            let s = &mut ms.search;
            match r {
                Ok((meas, shown_px, image)) => {
                    let r = s.record(i, shown_px, &meas, &l).clone();
                    let name = crop_name(screen, &label, &r.size);
                    let crop = image.and_then(|i| save_crop(&name, &i, args.crops.as_deref()));
                    if let Some(last) = s.rungs.last_mut() {
                        last.crop = crop;
                    }
                    out.event(&TuneEvent::Measured {
                        screen,
                        scene: label,
                        rung: s.rungs.last().cloned().unwrap_or(r),
                    });
                }
                Err(e) => {
                    let why = format!("{}: {e}", s.sizes[i].describe());
                    s.failed(i, &why);
                    ms.failed.push(why);
                }
            }
        }
    }
    let timing = rig.gpu_timing();
    let mut saved = None;
    for f in &fits {
        let results = f.results(timing, req.pick);
        for t in &results {
            out.event(&TuneEvent::Scene {
                screen: f.screen,
                result: t.clone(),
            });
        }
        if !req.dry_run {
            let t = f.tuning(results, limits, most, gpu_name.clone());
            saved = Some(save_screen(f.screen, limits, t)?);
        }
    }
    if saved.is_some() && status.is_some() {
        let _ = host_call(&json!({"action": "reload"}));
    }
    drop(hold);
    out.event(&TuneEvent::done_in(
        saved,
        total,
        timing,
        trials,
        t0.elapsed().as_secs_f64(),
    ));
    Ok(())
}

/// `--pick` from the last fit, nothing measured (#77).
fn place_saved(
    saved: &[(usize, Tuning)],
    pick: Pick,
    limits: &TuneLimits,
    req: &TuneRequest,
    host: bool,
    gpu: Option<String>,
    out: &Out,
    t0: Instant,
) -> Result<(), String> {
    let total = saved.iter().map(|(_, t)| t.scenes.len()).sum();
    out.event(&TuneEvent::Start {
        screens: saved.iter().map(|(s, _)| *s).collect(),
        scenes: total,
        fps: saved.iter().map(|(_, t)| t.fps).fold(0.0, f32::max),
        fps_max: None,
        gpu,
        shares: vec![],
        limits: Some(*limits),
        reused: true,
    });
    let mut file = None;
    let mut timing = GpuTiming::None;
    for (screen, t) in saved {
        let t = repicked(t, pick);
        for s in &t.scenes {
            timing = s.gpu_timing;
            out.event(&TuneEvent::Scene {
                screen: *screen,
                result: s.clone(),
            });
        }
        if !req.dry_run {
            file = Some(save_screen(*screen, *limits, t)?);
        }
    }
    if file.is_some() && host {
        let _ = host_call(&json!({"action": "reload"}));
    }
    out.event(&TuneEvent::done_in(
        file,
        total,
        timing,
        0,
        t0.elapsed().as_secs_f64(),
    ));
    Ok(())
}

/// The screens shown at once (those on the desktop, or every screen), each with its size and its
/// rotation's scenes.
fn plan(
    settings: &HostSettings,
    args: &TuneArgs,
    status: Option<&Value>,
    out: &Out,
) -> Result<Vec<Job>, String> {
    let n = settings.screens.len();
    if let Some(i) = args.screen
        && i >= n
    {
        return Err(format!("there is no screen {i}"));
    }
    let places: Vec<Place> = settings.screens.iter().map(|s| s.place).collect();
    let mut which = shown_screens(&places);
    if let Some(i) = args.screen
        && !which.contains(&i)
    {
        which.push(i);
        which.sort_unstable();
    }
    let mut probed: Option<Vec<Probed>> = None;
    let mut jobs = vec![];
    for i in which {
        let s = &settings.screens[i];
        let (_, slots) = load_rotation(&s.rotation)?;
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
                t.hz,
            ),
            _ => {
                if probed.is_none() {
                    probed = Some(probe_monitors().unwrap_or_default());
                }
                match monitor_canvas(probed.as_deref().unwrap_or(&[]), s.monitor) {
                    Some(m) => m,
                    None => {
                        out.warn(&format!(
                            "screen {i}: no host running and no monitors found; fitting for {}x{} (give --canvas)",
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
        return Err("no screens to fit".into());
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

/// The host's `tuning` hold, renewed while the fit runs and let go when it ends.
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
/// world of this size, [`GpuSim::worth`]; `paced`: it runs at a set speed). A trial left on the
/// processor is held to the processor limit like any other.
pub(crate) fn steps_on_gpu(
    profile: &Profile,
    paced: bool,
    sim: &SimBackend,
    world: &World,
) -> bool {
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

/// What one trial of a round shows: a screen's measured scene at a size.
pub(crate) struct TrialSpec {
    pub rotation: String,
    pub canvas: [f64; 2],
    pub scale: f64,
    pub refresh: Option<f32>,
    pub slot: Slot,
    pub label: String,
    pub design: f32,
    pub size: Size,
}

/// A trial under way: its stage, its own simulator, target and painter, and what it measured.
struct Trial {
    stage: Stage,
    sim: SimBackend,
    draw: Option<(Painter, wgpu::Texture)>,
    timer: Option<Timer>,
    beat: FrameClock,
    fps: f32,
    speed: f32,
    canvas: [u32; 2],
    scale: f32,
    budget: Duration,
    /// Each measured frame's time: its work, and the wait for the GPU to finish it.
    frames: Vec<f64>,
    /// The processor side's time (stepping, drawing, submitting), and the GPU wait, summed.
    work: f64,
    wait: f64,
    /// GPU seconds measured for this trial alone (timestamps, or submission to completion).
    gpu: f64,
    gen0: u64,
    cells: u64,
}

/// What the trials run on: the GPU (if any), how its time is read, and the host's settings.
struct Rig {
    gpu: Option<SharedGpu>,
    timestamps: bool,
    /// Windows' GPU engine counters, on an adapter without timestamps ([`Clock::Counters`]).
    engine: Option<EngineMeter>,
    clock: Clock,
    settings: HostSettings,
    lookup: Lookup,
}

impl Rig {
    fn new(gpu: Option<SharedGpu>, settings: &HostSettings, lookup: Lookup) -> Rig {
        let timestamps = gpu.as_ref().is_some_and(|g| {
            g.device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
        });
        let clock = match &gpu {
            None => Clock::None,
            // The system's count where it moves (Linux, and macOS: #93), found out in the first
            // warm-up.
            Some(_) if crate::power::process_gpu_time().is_some() => Clock::Undecided,
            Some(_) if timestamps => Clock::Timestamps,
            // Windows' engine counters, if they count this process's work: the first warm-up
            // finds out (#72).
            Some(_) if cfg!(windows) => Clock::Undecided,
            Some(_) => Clock::Submission,
        };
        // None off Windows, and where the counters can't be read.
        let engine = match (timestamps, clock) {
            (false, Clock::Undecided) if cfg!(windows) => EngineMeter::start(),
            _ => None,
        };
        Rig {
            timestamps,
            engine,
            clock,
            gpu,
            settings: settings.clone(),
            lookup,
        }
    }

    /// How the trials measured the GPU's time.
    fn gpu_timing(&self) -> GpuTiming {
        match self.clock {
            Clock::None => GpuTiming::None,
            Clock::Kernel => GpuTiming::Kernel,
            Clock::Timestamps => GpuTiming::Timestamps,
            Clock::Counters => GpuTiming::Counters,
            Clock::Undecided | Clock::Submission => GpuTiming::Submission,
        }
    }

    fn sim(&self) -> SimBackend {
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

    /// Builds a trial's stage and waits for its world.
    fn build(&self, spec: &TrialSpec, renew: &mut dyn FnMut()) -> Result<Trial, String> {
        let mut slot = spec.slot.clone();
        slot.end.clear();
        slot.schedule = None;
        let rotation = Rotation {
            name: spec.rotation.clone(),
            default_minutes: None,
            ..Default::default()
        };
        let mut stage = Stage::new(
            &rotation,
            vec![slot.clone()],
            1,
            spec.canvas,
            self.lookup.clone(),
            self.settings.cell_limit(),
        )?;
        stage.resize(spec.canvas, spec.scale, 0.0);
        stage.set_tuned(showing(
            &spec.rotation,
            spec.canvas,
            spec.scale,
            &spec.label,
            spec.design,
            spec.size,
        ));
        // Paced as the host would pace it on this screen's monitor.
        stage.set_pacer(self.settings.pacer(spec.refresh));
        let mut sim = self.sim();
        let t0 = Instant::now();
        let budget = Duration::from_secs_f64(STEP_SHARE as f64 / self.settings.fps.max(1.0) as f64);
        let mut notes = vec![];
        loop {
            stage.tick(t0.elapsed().as_secs_f64(), &mut sim, budget);
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
            std::thread::sleep(Duration::from_millis(10));
        }
        let world = stage.world().expect("built");
        let cells = world.topo.len() as u64;
        let pace = stage.pace();
        let on = steps_on_gpu(&slot.profile, pace.speed > 0.0, &sim, world);
        stage.set_world_gpu(on);
        let canvas = [spec.canvas[0].round() as u32, spec.canvas[1].round() as u32];
        let draw = self.gpu.as_ref().map(|g| {
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
            (Painter::new(&g.device, FORMAT), tex)
        });
        Ok(Trial {
            stage,
            sim,
            draw,
            timer: self
                .gpu
                .as_ref()
                .filter(|_| self.timestamps)
                .and_then(Timer::new),
            beat: FrameClock::default(),
            fps: pace.fps,
            speed: pace.speed,
            canvas,
            scale: spec.scale as f32,
            budget: Duration::from_secs_f64(STEP_SHARE as f64 / pace.fps.max(1.0) as f64),
            frames: vec![],
            work: 0.0,
            wait: 0.0,
            gpu: 0.0,
            gen0: 0,
            cells,
        })
    }

    /// One frame of `t` at `now` (seconds since the round began): stepped and drawn, and for its
    /// measure, how long its work took and how long the GPU took to finish it.
    fn frame(&self, t: &mut Trial, now: f64, clock: Clock) -> (f64, f64, f64) {
        t.beat.tick(now, t.fps);
        let gpu = self.gpu.as_ref();
        let timer = t
            .timer
            .as_ref()
            .filter(|_| matches!(clock, Clock::Timestamps | Clock::Undecided));
        let start = Instant::now();
        if let (Some(g), Some(tm)) = (gpu, timer) {
            tm.mark(g, 0);
        }
        let s0 = Instant::now();
        t.stage.tick(now, &mut t.sim, t.budget);
        let step_wall = s0.elapsed().as_secs_f64();
        if let (Some(g), Some(tm)) = (gpu, timer) {
            tm.mark(g, 1);
        }
        let mut gpu_secs = 0.0;
        let mut wait = 0.0;
        if let (Some(g), Some((painter, tex))) = (gpu, t.draw.as_mut()) {
            painter.paint_timed(
                &g.device,
                &g.queue,
                tex,
                t.canvas,
                t.scale,
                &t.stage,
                &self.settings.caption,
                now,
                timer.map(Timer::draw_writes),
            );
            let w0 = Instant::now();
            match (timer, clock) {
                (Some(tm), _) => {
                    let (s, d) = tm.read(g);
                    gpu_secs = s + d;
                }
                // The system counts it; the frame is over once the GPU has done it.
                (None, Clock::Kernel | Clock::Counters | Clock::Undecided) => {
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
                    let stepped = matches!(t.sim.status(), SimStatus::Gpu(_));
                    gpu_secs = drawn + if stepped { step_wall } else { 0.0 };
                }
            }
            wait = w0.elapsed().as_secs_f64();
        }
        let total = start.elapsed().as_secs_f64();
        (total - wait, wait, gpu_secs)
    }

    /// Shows every trial of a round at once for `seconds`, as the host would show its screens,
    /// and measures the last three quarters: each one's frames, and the process's processor and
    /// GPU time, shared between them by how long each one's frames took (#107). Returns, for
    /// each, what it measured, the size its cells showed at, and a picture of the middle of its
    /// last frame. `renew` is called while big worlds build.
    fn round(
        &mut self,
        specs: &[TrialSpec],
        seconds: f64,
        first: bool,
        renew: &mut dyn FnMut(),
    ) -> Vec<Measured> {
        let mut trials: Vec<Result<Trial, String>> =
            specs.iter().map(|s| self.build(s, renew)).collect();
        let k0 = crate::power::process_gpu_time();
        if self.clock == Clock::Undecided
            && let Some(e) = &mut self.engine
        {
            // Windows' counters from here, to see whether they count the warm-up's work.
            let _ = e.share();
        }
        let t0 = Instant::now();
        // A fit's first round warms up longer: a cold start is slower than a warm one.
        let warm_for = tuning::round_warmup(seconds, first);
        let run = |rig: &Rig, trials: &mut [Result<Trial, String>], until: f64, clock: Clock| {
            loop {
                let now = t0.elapsed().as_secs_f64();
                if now >= until {
                    break;
                }
                // The trial due soonest; the others wait their turn, each on its own clock.
                let due = trials
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.as_ref().ok().map(|t| (i, t.beat.next().unwrap_or(now))))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let Some((i, at)) = due else {
                    break;
                };
                if at > now {
                    std::thread::sleep(Duration::from_secs_f64((at - now).min(until - now)));
                    continue;
                }
                let Ok(t) = &mut trials[i] else { continue };
                let (work, wait, gpu) = rig.frame(t, now, clock);
                t.frames.push(work + wait);
                t.work += work;
                t.wait += wait;
                t.gpu += gpu;
            }
        };
        run(self, &mut trials, warm_for, self.clock);
        if self.clock == Clock::Undecided {
            // The system's count, where it moved while the warm-up drew; else the GPU's clock;
            // else, on Windows, its engine counters where they counted the warm-up's work.
            let k1 = crate::power::process_gpu_time();
            let counted = self
                .engine
                .as_mut()
                .and_then(EngineMeter::share)
                .is_some_and(|s| s > 0.0);
            self.clock = match (k0, k1) {
                (Some(a), Some(b)) if b > a => Clock::Kernel,
                _ if self.timestamps => Clock::Timestamps,
                _ if counted => Clock::Counters,
                _ => Clock::Submission,
            };
            if self.clock != Clock::Counters {
                self.engine = None;
            }
        }
        let clock = self.clock;
        for t in trials.iter_mut().flatten() {
            t.frames.clear();
            t.work = 0.0;
            t.wait = 0.0;
            t.gpu = 0.0;
            t.gen0 = t.stage.world().map_or(0, |w| w.generation);
            let _ = t.stage.take_notes();
        }
        if clock == Clock::Counters
            && let Some(e) = &mut self.engine
        {
            // The counters' sample from here, taken before the processor meter starts, so PDH's
            // own work is not counted as JETCARP's.
            let _ = e.share();
        }
        let meter = ProcessMeter::start();
        let counted0 = crate::power::process_gpu_time();
        let m0 = Instant::now();
        run(
            self,
            &mut trials,
            warm_for + (seconds - warmup(seconds)),
            clock,
        );
        let wall = m0.elapsed().as_secs_f64().max(1e-9);
        let cpu = meter.map_or(0.0, |m| m.share());
        let process_gpu = match clock {
            Clock::Kernel => match (counted0, crate::power::process_gpu_time()) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a).as_secs_f64() / wall),
                _ => None,
            },
            // Read once the processor meter has stopped. None if they stopped answering: the
            // share is then unknown, not zero.
            Clock::Counters => self.engine.as_mut().and_then(EngineMeter::share),
            _ => None,
        };
        let works: Vec<f64> = trials
            .iter()
            .map(|t| t.as_ref().map_or(0.0, |t| t.work + t.wait))
            .collect();
        let waits: Vec<f64> = trials
            .iter()
            .map(|t| t.as_ref().map_or(0.0, |t| t.wait))
            .collect();
        let cpus = attribute(cpu, &works);
        let gpus = process_gpu.map(|g| {
            if waits.iter().sum::<f64>() > 0.0 {
                attribute(g, &waits)
            } else {
                attribute(g, &works)
            }
        });
        trials
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let mut t = t?;
                let image = match (&self.gpu, &t.draw) {
                    (Some(g), Some((_, tex))) => {
                        let size = crop_size([t.canvas[0] as f64, t.canvas[1] as f64]);
                        jetcarp_display::crop::from_frame(
                            &g.device, &g.queue, tex, &t.stage, t.canvas, size,
                        )
                    }
                    _ => None,
                };
                t.draw = None;
                let shown_px = t.stage.status().cell_px;
                let frames = t.frames.len() as f64;
                let gens_done = t
                    .stage
                    .world()
                    .map_or(0, |w| w.generation)
                    .saturating_sub(t.gen0) as f64;
                let on_gpu = matches!(t.sim.status(), SimStatus::Gpu(_));
                let gpu = match (&self.gpu, clock) {
                    (None, _) => None,
                    (Some(_), Clock::Kernel | Clock::Counters) => gpus.as_ref().map(|g| g[i]),
                    (Some(_), _) => Some(t.gpu / wall),
                };
                let speed = t.speed as f64;
                Ok((
                    Measure {
                        cpu: cpus[i],
                        gpu: gpu.map(|g| g.clamp(0.0, 1.0)),
                        p95: p95(&mut t.frames),
                        fps: frames / wall,
                        gens_wanted: (speed > 0.0).then_some(speed * wall),
                        gens_done,
                        on_gpu,
                        gpu_timing: self.gpu_timing(),
                        cells: t.cells,
                    },
                    shown_px,
                    image,
                ))
            })
            .collect()
    }
}

/// How the trials read the GPU's time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Clock {
    /// No GPU.
    None,
    /// Not known yet: the first round's warm-up finds out whether the system counts it (Linux's
    /// kernel, macOS's task accounting, or Windows' engine counters where the adapter has no
    /// timestamps).
    Undecided,
    /// The system's count of this process's GPU time ([`crate::power::process_gpu_time`]).
    Kernel,
    /// Timestamp queries around each frame's step and drawing.
    Timestamps,
    /// Windows' GPU engine counters for this process ([`EngineMeter`]).
    Counters,
    /// From handing each frame's work to the GPU until it is done.
    Submission,
}

/// The 95th percentile.
pub(crate) fn p95(times: &mut [f64]) -> f64 {
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

    /// Fast Fit's first guesses come from a score taken with the computer quiet: one taken under
    /// load would guess sizes too small for this machine, so it gives no guess, as a short run
    /// doesn't (#301).
    #[test]
    fn a_score_taken_under_load_gives_no_first_guess() {
        use jetcarp_core::machine::Score;
        use jetcarp_core::machine::quiet::{Conditions, Load};
        use jetcarp_core::machine::score::Part;
        let score = Score {
            benchmark: 1,
            score: 100.0,
            parts: vec![Part {
                id: "core-square".into(),
                value: 1e8,
                correct: Some(true),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(Speeds::of(Some(&score)).cpu.is_some());
        let quiet = Score {
            conditions: Some(Conditions::of(
                Load {
                    cpu: Some(0.02),
                    ..Default::default()
                },
                vec!["the app".into()],
                vec![],
            )),
            ..score.clone()
        };
        assert_eq!(Speeds::of(Some(&quiet)), Speeds::of(Some(&score)));
        let loaded = Score {
            conditions: Some(Conditions::of(
                Load {
                    cpu: Some(0.6),
                    ..Default::default()
                },
                vec![],
                vec![],
            )),
            ..score.clone()
        };
        assert_eq!(Speeds::of(Some(&loaded)), Speeds::default());
        let short = Score {
            quick: true,
            ..score
        };
        assert_eq!(Speeds::of(Some(&short)), Speeds::default());
        assert_eq!(Speeds::of(None), Speeds::default());
    }

    fn slot(label: &str, show: profiles::Show, series: Option<usize>) -> Slot {
        Slot {
            label: label.into(),
            profile: profiles::Profile {
                show,
                ..Default::default()
            },
            stored: false,
            end: vec![],
            hold_s: 0.0,
            schedule: None,
            n: None,
            series,
        }
    }

    /// A Quick Start entry missing here shows Life, in the fit as in the host, and its cards say
    /// so (#45).
    #[test]
    fn a_missing_quick_start_entry_is_named_in_the_plan() {
        let s = slot(
            "Quick Start rule:nope",
            profiles::Show::QuickStart { id: "nope".into() },
            None,
        );
        let lookup: Lookup = Arc::new(|_: &str| None);
        let plan = plan_scenes(
            &[s],
            &lookup,
            [1280.0, 720.0],
            1.0,
            4_000_000,
            &TuneLimits::default(),
            &Pacer::default(),
            Devices {
                gpu: true,
                speedup: tuning::GPU_SPEEDUP,
            },
            &mut HashMap::new(),
        );
        let note = plan[0].note.clone().unwrap_or_default();
        assert!(
            note.contains("no Quick Start entry nope; showing Life"),
            "{note}"
        );
    }

    /// The scenes' costs come from their tilings' neighbours and their speeds: Life on
    /// triangles (12 neighbours a cell) costs more a cell than on squares (8), and a series'
    /// rules are one scene, measured by its first that draws (#109, #346).
    #[test]
    fn scenes_cost_by_their_neighbours_and_a_series_is_one() {
        use jetcarp_core::{Boundary, CaSpec, Scene, TilingSpec};
        let scene = |family: &str, rule: &str| {
            let mut sc = Scene::new(TilingSpec::rect(family, 40, 30, Boundary::Wrap));
            sc.cas = vec![CaSpec::new(rule).seed(jetcarp_core::SeedSpec::Random { density: 0.3 })];
            profiles::Show::Scene {
                scene: Box::new(sc),
            }
        };
        let slots = vec![
            slot("B3/S23", scene("square", "B3/S23"), Some(0)),
            slot("B36/S23", scene("square", "B36/S23"), Some(0)),
            slot("triangles", scene("triangle", "B4/S345"), None),
        ];
        let lookup: Lookup = Arc::new(|_: &str| None);
        let plan = plan_scenes(
            &slots,
            &lookup,
            [1280.0, 720.0],
            1.0,
            4_000_000,
            &TuneLimits::default(),
            &Pacer::default(),
            Devices {
                gpu: true,
                speedup: tuning::GPU_SPEEDUP,
            },
            &mut HashMap::new(),
        );
        assert_eq!(plan.len(), 2, "{plan:?}");
        assert_eq!(plan[0].also, ["B36/S23"]);
        let (sq, tri) = (plan[0].cost.unwrap(), plan[1].cost.unwrap());
        assert!(tri > sq, "triangles {tri} against squares {sq}");
        assert_eq!(tuning::most_demanding(&plan), Some(1));
    }

    /// A series is measured and pictured by its first rule that changes cells of its seed (#346):
    /// the Wolfram series' rule 0 draws nothing from its one cell, so its cards were empty
    /// pictures; rule 1 draws. A series whose first rule draws is shown by it, as before.
    #[test]
    fn a_series_is_measured_and_pictured_by_a_rule_that_draws() {
        use jetcarp_core::{Boundary, CaSpec, Scene, TilingSpec};
        let scene = |rule: &str| {
            let mut sc = Scene::new(TilingSpec::rect("square", 200, 140, Boundary::Dead));
            sc.cas = vec![CaSpec::new(rule)];
            profiles::Show::Scene {
                scene: Box::new(sc),
            }
        };
        let lookup: Lookup = Arc::new(|_: &str| None);
        let plan = |slots: &[Slot]| {
            plan_scenes(
                slots,
                &lookup,
                [1280.0, 720.0],
                1.0,
                4_000_000,
                &TuneLimits::default(),
                &Pacer::default(),
                Devices {
                    gpu: true,
                    speedup: tuning::GPU_SPEEDUP,
                },
                &mut HashMap::new(),
            )
        };
        let wolfram: Vec<Slot> = ["W0", "W1", "W2", "W30"]
            .into_iter()
            .map(|r| slot(r, scene(r), Some(0)))
            .collect();
        let p = plan(&wolfram);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].label, "W1", "{p:?}");
        assert_eq!(p[0].also, ["W0", "W2", "W30"]);
        assert!(!changes_its_seed(
            &profiles::resolve(&wolfram[0].profile, &*lookup).1
        ));
        let life: Vec<Slot> = ["B3/S23", "B36/S23"]
            .into_iter()
            .map(|r| slot(r, scene(r), Some(0)))
            .collect();
        assert_eq!(plan(&life)[0].label, "B3/S23");
    }

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
        assert_eq!(
            shown_screens(&[Place::Window, Place::Wallpaper, Place::Wallpaper]),
            [1, 2]
        );
        assert_eq!(shown_screens(&[Place::Window, Place::Window]), [0, 1]);
    }
}
