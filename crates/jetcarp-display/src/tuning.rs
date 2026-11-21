//! *Fit to this computer*: the finest size each desktop background scene can show at within the
//! processor and GPU limits at a smooth frame rate, found in about ten seconds (#113), so the
//! person can see what fits and choose.
//!
//! A fast fit measures one scene a screen: the rotation's most demanding, by an estimate of each
//! scene's work per cell ([`cost_per_cell`]), a series' rules counted as one scene
//! ([`scene_groups`], #109). Every screen is measured at once. For each, a binary search over the
//! scene's sizes ([`Search`]) shows it at a size for about a second ([`TRIAL_SECONDS`]), measures
//! JETCARP's own load meanwhile, the processor's time, the GPU's and how long a frame's work takes
//! (not the wait to present it, #106), and goes finer or coarser: about five sizes in all
//! ([`MAX_TRIALS`]). The size found serves the whole rotation: a cheaper scene may go finer by its
//! cost ratio ([`derive`]). The rules, palettes and colouring stay as they are; only the number of
//! cells changes.
//!
//! **One set of limits for the whole desktop** (#107): the processor and GPU limits are shared
//! between the screens by their pixel area ([`area_shares`]), each screen held to its share, so
//! the screens together stay within them. Measured together, the process's time is shared between
//! the screens by how long each one's frames took ([`attribute`]).
//!
//! On a lattice the sizes are cell sizes in physical pixels ([`LADDER`], [`lattice_ladder`]); on
//! an aperiodic tiling, patch levels ([`patch_ladder`]). Each size tried becomes a card ([`Rung`])
//! with a picture of the screen, its numbers, and whether it is within the limits ([`fits`]), or
//! within the measurement's noise of one ([`NOISE`]): a fit repeated with nothing changed keeps the
//! size it recommended (#94). A pattern placed on an empty field ([`is_placed`]) keeps its own
//! size unless someone chooses.
//!
//! The background host runs the fit on the real screens. With no host running, `jetcarp host
//! tune` runs it off screen in a process of its own, which the app starts with [`TuneChild`].

use crate::host_settings::{GPU_UNMEASURED, GpuTiming, Rung, Size, TuneLimits, TunedScene, Tuning};
use crate::pace::Pacer;
use crate::profiles::Show;
use crate::rotation::Slot;
use jetcarp_core::{Command, Scene, SeedSpec};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::process::{Child, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, channel};

/// Each limit is kept with this much to spare: a size passes at 80% of the CPU and GPU limits.
pub const MARGIN: f32 = 0.8;
/// A frame's 95th percentile must be within this share of the frame's interval.
pub const FRAME_MARGIN: f32 = 0.9;
/// Generations stepped must reach this share of the scene's speed.
pub const KEEP_UP: f64 = 0.9;
/// The coarsest sensible grid has at least this many cells across its shorter side.
pub const MIN_CELLS_ACROSS: f64 = 60.0;
/// The cell sizes a lattice sweep shows, in physical pixels.
pub const LADDER: [f32; 12] = [
    1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0,
];
/// No size of a sweep has more cells than this (a world the size the MCP server allows).
pub const SWEEP_MAX_CELLS: u64 = 4_000_000;
/// Seconds each size shows for: a quarter warming up, the rest measured.
pub const TRIAL_SECONDS: f64 = 1.0;
/// The most sizes a fit tries on one screen: a binary search over the twelve sizes, from a good
/// first guess, needs about four.
pub const MAX_TRIALS: usize = 6;
/// A size this far over a limit is not followed by finer ones.
pub const FAR_OVER: f64 = 2.0;
/// A load within this share of a limit's line, either side, is within the measurement's noise:
/// the Mac's 4 px measured 7.3 to 8.3% against a line of 8.0% from one fit to the next (#94).
pub const NOISE: f64 = 0.10;
/// A cell drawn costs about as much a frame as this many neighbour reads (an estimate: about one
/// Life generation's work on squares).
pub const DRAW_READS: f64 = 8.0;

/// Of a size's `seconds` on show, how many are run before measuring (uploads, the first
/// colouring, shaders): a quarter, at most half a second. The rest is measured.
pub fn warmup(seconds: f64) -> f64 {
    (seconds * 0.25).min(0.5)
}

/// What one size measured.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Measure {
    /// JETCARP's processor time per second, as a share of the machine (0 to 1).
    pub cpu: f64,
    /// Its share of the GPU's time (None: not measured).
    pub gpu: Option<f64>,
    /// The 95th percentile of a frame's work (stepping, drawing and handing it to the GPU, not
    /// waiting to present it: #106), in seconds.
    pub p95: f64,
    /// Frames drawn a second.
    #[serde(default)]
    pub fps: f64,
    /// Generations wanted at the scene's speed, and stepped (None: as fast as it can, so no
    /// target).
    pub gens_wanted: Option<f64>,
    pub gens_done: f64,
    /// Stepped on the GPU.
    pub on_gpu: bool,
    pub gpu_timing: GpuTiming,
    /// Cells in the world.
    pub cells: u64,
}

impl Measure {
    /// The GPU share the limits judge: none from [`GpuTiming::Submission`], an upper bound that
    /// counts the work's time waiting in the driver's queue. On Windows it read about 90% for a
    /// scene the GPU spent 10% of its time on, so every size looked far over the limit (#38). Its
    /// frames are still judged by their times. Windows' own engine counters
    /// ([`GpuTiming::Counters`]) are judged, as the kernel's count is on Linux (#72).
    pub fn judged_gpu(&self) -> Option<f64> {
        self.gpu.filter(|_| !self.gpu_timing.unmeasured())
    }

    /// A GPU was there, but its share wasn't measured, so the GPU limit isn't applied: an unjudged
    /// share, or a share the system's count failed to give.
    pub fn gpu_unmeasured(&self) -> bool {
        self.gpu_timing != GpuTiming::None && self.judged_gpu().is_none()
    }
}

/// Each screen's share of the desktop's limits: its pixel area over that of every screen shown at
/// once (#107). The limits are one set for the whole desktop, so the screens together stay within
/// them.
pub fn area_shares(canvases: &[[f64; 2]]) -> Vec<f64> {
    let areas: Vec<f64> = canvases
        .iter()
        .map(|c| (c[0].max(0.0) * c[1].max(0.0)).max(0.0))
        .collect();
    let total: f64 = areas.iter().sum();
    if total <= 0.0 {
        return vec![1.0 / canvases.len().max(1) as f64; canvases.len()];
    }
    areas.iter().map(|a| a / total).collect()
}

/// The limits one scene is held to: its screen's share of the user's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SceneLimits {
    /// Share of the machine's processor time (0 to 1).
    pub cpu: f64,
    /// Share of the GPU's time (0 to 1).
    pub gpu: f64,
    /// Frames per second to keep.
    pub fps: f64,
}

impl SceneLimits {
    /// The user's limits for a screen with `share` of the desktop's ([`area_shares`]), at `fps`.
    pub fn of(l: &TuneLimits, fps: f32, share: f64) -> SceneLimits {
        let share = if share.is_finite() && share > 0.0 {
            share.min(1.0)
        } else {
            1.0
        };
        SceneLimits {
            cpu: l.cpu_percent as f64 / 100.0 * share,
            gpu: l.gpu_percent as f64 / 100.0 * share,
            fps: fps.max(1.0) as f64,
        }
    }

    /// The frame interval, in seconds.
    pub fn interval(&self) -> f64 {
        1.0 / self.fps.max(1.0)
    }
}

/// The frame rate a scene asking for `speed` generations a second is held to: the one the limits
/// ask for (else the monitor's refresh rate, or 60), at most the rate its screen will draw it at
/// ([`Pacer::pace`]). So a scene drawn at 60 frames a second is not judged as if it drew 30 (#42).
pub fn scene_fps(limits: &TuneLimits, pacer: &Pacer, speed: f32) -> f32 {
    limits.target_fps(pacer.refresh, pacer.pace(speed).fps)
}

/// Whether a size stayed within the limits (with [`MARGIN`] to spare); Err says which it passed.
pub fn fits(m: &Measure, l: &SceneLimits) -> Result<(), String> {
    let mut over = vec![];
    let margin = MARGIN as f64;
    if m.cpu > l.cpu * margin {
        over.push(format!(
            "processor {} (over {}: the {} limit, less 20%)",
            percent(m.cpu),
            percent(l.cpu * margin),
            percent(l.cpu)
        ));
    }
    if let Some(g) = m.judged_gpu()
        && g > l.gpu * margin
    {
        over.push(format!(
            "GPU {} (over {}: the {} limit, less 20%)",
            percent(g),
            percent(l.gpu * margin),
            percent(l.gpu)
        ));
    }
    if m.p95 > l.interval() * FRAME_MARGIN as f64 {
        over.push(format!(
            "frames {:.1} ms (at {} fps, {:.1} ms)",
            m.p95 * 1000.0,
            l.fps,
            l.interval() * 1000.0
        ));
    }
    // A generation of slack: a second's trial of a scene of 30 generations a second wants about
    // 22, and one short is the frame it was measured from, not a scene falling behind.
    if let Some(want) = m.gens_wanted
        && want > 0.0
        && m.gens_done + 1.0 < want * KEEP_UP
    {
        over.push(format!(
            "slow: {:.0} of {:.0} generations",
            m.gens_done, want
        ));
    }
    if over.is_empty() {
        Ok(())
    } else {
        Err(over.join(", "))
    }
}

/// How near a size came to the limits: the largest of its loads over that limit's line (the
/// processor and GPU limits less [`MARGIN`], the frame interval times [`FRAME_MARGIN`], and for
/// the speed, the generations it fell short by over the most it may, `1 - KEEP_UP`: 0 when it
/// keeps its speed). At most 1 is within the limits, as [`fits`] says.
pub fn load(m: &Measure, l: &SceneLimits) -> f64 {
    let margin = MARGIN as f64;
    let over = |x: f64, line: f64| if line > 0.0 { x / line } else { 0.0 };
    let mut q = over(m.cpu, l.cpu * margin);
    if let Some(g) = m.judged_gpu() {
        q = q.max(over(g, l.gpu * margin));
    }
    q = q.max(over(m.p95, l.interval() * FRAME_MARGIN as f64));
    if let Some(want) = m.gens_wanted
        && want > 0.0
    {
        // With a generation of slack, as `fits` allows.
        let short = ((want - m.gens_done - 1.0) / want).max(0.0);
        q = q.max(over(short, 1.0 - KEEP_UP));
    }
    q.clamp(0.0, 100.0)
}

/// About how many cells the measured scene could hold on its limits' line, from its cards (the
/// sizes tried, with their loads, [`load`]): where the loads cross 1, between the finest size
/// within the limits and the next finer over them; with nothing finer over them, the finest
/// within them scaled by its load, at most four times its cells. A load that grows linearly with
/// the cells from any base of its own (a frame's fixed cost) crosses later, so this errs low.
/// None without a card within the limits (or with cards from before 0.4.3, which have no loads).
pub fn line_cells(cards: &[Rung]) -> Option<f64> {
    let fit = cards
        .iter()
        .filter(|r| r.load > 0.0 && r.load <= 1.0)
        .max_by_key(|r| r.cells)?;
    let (c0, q0) = (fit.cells as f64, fit.load as f64);
    let over = cards
        .iter()
        .filter(|r| r.load > 1.0 && r.cells > fit.cells)
        .min_by_key(|r| r.cells);
    Some(match over {
        Some(o) => {
            let (c1, q1) = (o.cells as f64, o.load as f64);
            (c0 + (1.0 - q0) / (q1 - q0) * (c1 - c0)).clamp(c0, c1)
        }
        None => c0 / q0.max(0.25),
    })
}

/// Whether a load ([`load`]) is within the measurement's noise of its line, either side.
pub fn at_limit(load: f64) -> bool {
    (load - 1.0).abs() <= NOISE
}

/// `x` (a share) as a percentage, to a tenth below 10%.
pub fn percent(x: f64) -> String {
    let p = x * 100.0;
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// The coarsest sensible cell size, in pixels, on a canvas of `canvas` pixels: at least
/// [`MIN_CELLS_ACROSS`] cells across the shorter side, at most the largest cell size there is.
pub fn coarsest_px(canvas: [f64; 2]) -> f32 {
    let short = canvas[0].min(canvas[1]).max(1.0);
    ((short / MIN_CELLS_ACROSS).floor() as f32).clamp(1.0, *crate::profiles::CELL_PX_RANGE.end())
}

/// About how many cells a lattice has at `px` pixels a cell on `canvas` (its tiles' mean area is
/// `px` squared, as [`crate::profiles::fill_size`] builds it).
pub fn lattice_cells(canvas: [f64; 2], px: f32) -> u64 {
    (canvas[0] * canvas[1] / (px as f64 * px as f64).max(1e-6)).round() as u64
}

/// The sizes a lattice sweep shows on `canvas` (physical pixels), finest first: each size of
/// [`LADDER`] from the finest that keeps the world within `max_cells` to the coarsest sensible,
/// and the scene's own size (`design_px`, physical pixels) wherever it falls. There is no floor
/// above one pixel: on a screen with more pixels per point, the finest sizes simply hold more
/// cells, and `max_cells` cuts them off sooner.
pub fn lattice_ladder(canvas: [f64; 2], design_px: f32, max_cells: u64) -> Vec<Size> {
    let coarsest = coarsest_px(canvas);
    let mut v: Vec<Size> = LADDER
        .iter()
        .copied()
        .filter(|&px| px <= coarsest && lattice_cells(canvas, px) <= max_cells)
        .map(Size::px)
        .collect();
    if !v.iter().any(|s| s.same(&Size::px(design_px))) {
        v.push(Size::px(design_px));
    }
    v.sort_by(|a, b| a.px.total_cmp(&b.px));
    v
}

/// About how big a patch of `cells` tiles shows its tiles when fitted to `canvas`: a round patch
/// across the shorter side.
pub fn patch_px(canvas: [f64; 2], cells: u64) -> f32 {
    let short = canvas[0].min(canvas[1]);
    (short * short * std::f64::consts::FRAC_PI_4 / cells.max(1) as f64).sqrt() as f32
}

/// The sizes an aperiodic sweep shows, finest first: each level of the `family`'s patches with at
/// least [`MIN_CELLS_ACROSS`] squared tiles, at most `max_cells`, and tiles of at least a pixel
/// when fitted to `canvas`, and the scene's own level. Each size's `px` is an estimate until it
/// has been shown.
pub fn patch_ladder(
    family: &str,
    canvas: [f64; 2],
    own_level: Option<u32>,
    max_cells: u64,
) -> Vec<Size> {
    use jetcarp_core::aperiodic::{level_range, tile_count};
    let (lo, hi) = level_range(family);
    let min = (MIN_CELLS_ACROSS * MIN_CELLS_ACROSS) as u64;
    let mut v = vec![];
    for l in lo..=hi {
        let Some(n) = tile_count(family, l).map(|n| n as u64) else {
            continue;
        };
        let px = patch_px(canvas, n);
        let own = own_level == Some(l);
        if own || (n >= min && n <= max_cells && px >= 1.0) {
            v.push(Size { px, level: Some(l) });
        }
    }
    v.sort_by(|a, b| a.px.total_cmp(&b.px));
    v
}

/// About how many cells a scene on `family` has at `size` on `canvas`: a patch's tiles at its
/// level, a lattice's from its cell size.
pub fn cells_at(family: &str, canvas: [f64; 2], size: &Size) -> u64 {
    match size.level {
        Some(l) => jetcarp_core::aperiodic::tile_count(family, l).map_or(0, |n| n as u64),
        None => lattice_cells(canvas, size.px),
    }
}

/// Whether a scene is a pattern placed on an empty field, rather than a soup or a fill: no
/// automaton starts from random cells, and something is placed (cells by index, one cell, an ant,
/// a pile, a Lenia creature, pasted patterns). Such a scene keeps its own size unless someone
/// chooses another.
pub fn is_placed(scene: &Scene) -> bool {
    let soup = |s: &SeedSpec| {
        matches!(
            s,
            SeedSpec::Random { .. }
                | SeedSpec::Blob { .. }
                | SeedSpec::TopRandom { .. }
                | SeedSpec::RandomStates
                | SeedSpec::Signed { .. }
                | SeedSpec::Halves { .. }
                | SeedSpec::Gaussian { .. }
                | SeedSpec::Circuit { .. }
                | SeedSpec::Pattern { .. }
        ) || matches!(s, SeedSpec::Ants { count } if *count > 1)
    };
    let placed = |s: &SeedSpec| {
        matches!(
            s,
            SeedSpec::Cells { .. }
                | SeedSpec::Center
                | SeedSpec::TopCenter
                | SeedSpec::AntAt { .. }
                | SeedSpec::Pile { .. }
                | SeedSpec::GliderPair
                | SeedSpec::WireDemo
                | SeedSpec::Lenia { .. }
        ) || matches!(s, SeedSpec::Ants { count } if *count <= 1)
    };
    let mut seeds: Vec<&SeedSpec> = scene.cas.iter().filter_map(|c| c.seed.as_ref()).collect();
    for c in &scene.commands {
        if let Command::AddCa { ca } = c
            && let Some(s) = &ca.seed
        {
            seeds.push(s);
        }
    }
    if seeds.iter().any(|s| soup(s)) || !scene.reseed.is_empty() {
        return false;
    }
    seeds.iter().any(|s| placed(s)) || jetcarp_core::quickstart::places_cells(scene)
}

/// The scenes a fit sees in a rotation's `slots`: each slot's index, with the indexes of the
/// slots that show at its size. A series' rules (the slots of one series item, on one tiling with
/// one look) are one scene, shown by its first rule (#109): fitting the Wolfram series rule by rule
/// took 12 minutes.
pub fn scene_groups(slots: &[Slot]) -> Vec<(usize, Vec<usize>)> {
    // A series' rules are its slots from one item, each showing a scene on the series' tiling.
    let key = |s: &Slot| -> Option<String> {
        let item = s.series?;
        let Show::Scene { scene } = &s.profile.show else {
            return None;
        };
        Some(format!("{item}|{}", scene.tiling.family))
    };
    let mut groups: Vec<(usize, Vec<usize>)> = vec![];
    let mut last: Option<String> = None;
    for (i, s) in slots.iter().enumerate() {
        let k = key(s);
        match (&k, &last, groups.last_mut()) {
            (Some(a), Some(b), Some(g)) if a == b => g.1.push(i),
            _ => groups.push((i, vec![])),
        }
        last = k;
    }
    groups
}

/// About how much work a scene asks for, per cell and second, in neighbour reads: each
/// generation's reads per cell (`reads`: the tiling's neighbours, for each automaton's passes) at
/// its `speed`, and each frame's drawing at `fps` ([`DRAW_READS`]). A scene that runs as fast as
/// it can is counted at a generation a frame.
pub fn cost_per_cell(reads: f64, speed: f64, fps: f64) -> f64 {
    let fps = fps.max(1.0);
    let speed = if speed > 0.0 { speed } else { fps };
    reads.max(1.0) * speed + DRAW_READS * fps
}

/// How many times the processor's work a neighbour read on the GPU is worth, where the machine's
/// score doesn't say ([`device_cost`]): cautious, so a scene stepped on the processor is not sized
/// as cheaply as one on the GPU.
pub const GPU_SPEEDUP: f64 = 8.0;

/// [`cost_per_cell`] in the processor's terms: the stepping on the GPU (`gpu`) counted at
/// `speedup` times the processor's speed, and the drawing, always the GPU's, too.
pub fn device_cost(reads: f64, speed: f64, fps: f64, gpu: bool, speedup: f64) -> f64 {
    let fps = fps.max(1.0);
    let speed = if speed > 0.0 { speed } else { fps };
    let speedup = speedup.max(1.0);
    let step = reads.max(1.0) * speed;
    (if gpu { step / speedup } else { step }) + DRAW_READS * fps / speedup
}

/// The size a scene shows at from the measured scene's: the finest of its `sizes` (finest first)
/// whose cells (`cells`, by size), times `ratio` (its cost per cell over the measured scene's),
/// are no more than `base_cells` (the measured scene's cells at its size); else its coarsest. A
/// cheaper scene goes finer by its cost ratio, a dearer one coarser.
pub fn derive(sizes: &[Size], cells: impl Fn(&Size) -> u64, ratio: f64, base_cells: u64) -> Size {
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    };
    sizes
        .iter()
        .find(|s| cells(s) as f64 * ratio <= base_cells as f64 * (1.0 + 1e-9))
        .or(sizes.last())
        .copied()
        .unwrap_or_default()
}

/// [`derive`], keeping the size a scene was given last time (`prev`, not chosen by anyone) while it
/// stays within the noise of the allowance ([`NOISE`]): a finer size takes its place only when
/// clearly within it, so a fit repeated with nothing changed gives every scene the same size
/// (#94).
pub fn derive_kept(
    sizes: &[Size],
    cells: impl Fn(&Size) -> u64,
    ratio: f64,
    base_cells: u64,
    prev: Option<Size>,
) -> Size {
    let found = derive(sizes, &cells, ratio, base_cells);
    let Some(prev) = prev.and_then(|p| sizes.iter().find(|s| s.same(&p)).copied()) else {
        return found;
    };
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    };
    let b = base_cells as f64;
    let clearly = derive(sizes, &cells, ratio, (b * (1.0 - NOISE)) as u64);
    if cells(&clearly) > cells(&prev) {
        return clearly;
    }
    if cells(&prev) as f64 * ratio <= b * (1.0 + NOISE) {
        return prev;
    }
    found
}

/// One screen's share of time measured for every screen at once ([`attribute`]).
pub fn attribute(total: f64, work: &[f64]) -> Vec<f64> {
    let sum: f64 = work.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 {
        return vec![total / work.len().max(1) as f64; work.len()];
    }
    work.iter().map(|w| total * w.max(0.0) / sum).collect()
}

/// One size's card from what it measured.
pub fn rung_of(size: Size, m: &Measure, l: &SceneLimits, own: bool) -> Rung {
    let over = fits(m, l).err();
    let q = load(m, l);
    Rung {
        size,
        cells: m.cells,
        cpu_percent: (m.cpu * 100.0) as f32,
        // Only a measured share: an unjudged number would read as one (#72).
        gpu_percent: m.judged_gpu().map(|g| (g * 100.0) as f32),
        gpu_unmeasured: m.gpu_unmeasured(),
        p95_ms: (m.p95 * 1000.0) as f32,
        fps: m.fps as f32,
        on_gpu: m.on_gpu,
        fits: over.is_none(),
        over,
        load: q as f32,
        at_limit: at_limit(q),
        crop: None,
        own,
    }
}

/// One screen's search for the finest size of its measured scene within the limits: a binary
/// search over its sizes, from a first guess (a size chosen before, or the one recommended last
/// time, or a guess from the machine's score), about a second a size.
///
/// The size recommended last time is kept while it stays within the noise of the limits
/// ([`NOISE`]): a finer one displaces it only when clearly within them, so a fit repeated with
/// nothing changed recommends the same size (#94).
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    /// The sizes, finest first.
    pub sizes: Vec<Size>,
    /// The scene's own size.
    pub own: Size,
    /// The size tried first, by index, and whether the sizes either side of it come next (a
    /// good guess: two sizes then settle it).
    first: Option<(usize, bool)>,
    /// The size recommended last time, by index (#94).
    keep: Option<usize>,
    /// Each size's verdict, by index: within the limits (with [`Search::keep`]'s allowance) or
    /// not; None until tried.
    judged: Vec<Option<bool>>,
    /// Each size tried, in the order tried.
    pub rungs: Vec<Rung>,
    /// Each size's card in `rungs`, by index (None: not tried, or it could not be shown).
    card_of: Vec<Option<usize>>,
    /// The indexes tried, in the order tried.
    order: Vec<usize>,
    /// Why it stopped before it settled, when it did.
    pub stopped: Option<String>,
}

impl Search {
    /// A search over `sizes` (finest first), the scene's own size `own`, trying `first` first
    /// (None: the middle), keeping `keep` (the size recommended last time) while it stays within
    /// the noise.
    pub fn new(sizes: Vec<Size>, own: Size, first: Option<Size>, keep: Option<Size>) -> Search {
        let index = |s: Option<Size>| s.and_then(|s| sizes.iter().position(|x| x.same(&s)));
        let keep = index(keep);
        let first = index(first).or(keep).map(|i| (i, true));
        let n = sizes.len();
        Search {
            sizes,
            own,
            first,
            keep,
            judged: vec![None; n],
            rungs: vec![],
            card_of: vec![None; n],
            order: vec![],
            stopped: None,
        }
    }

    /// The sizes not yet settled, by index: those coarser than every size over the limits and
    /// finer than every size within them.
    fn open(&self) -> (usize, usize) {
        let n = self.sizes.len();
        let lo = self
            .judged
            .iter()
            .rposition(|j| *j == Some(false))
            .map_or(0, |i| i + 1);
        let hi = self
            .judged
            .iter()
            .enumerate()
            .filter(|(i, j)| **j == Some(true) && *i >= lo)
            .map(|(i, _)| i)
            .min()
            .unwrap_or(n);
        (lo, hi)
    }

    /// The size to try next, by index (None: done).
    pub fn next(&self) -> Option<usize> {
        if self.stopped.is_some() || self.order.len() >= MAX_TRIALS {
            return None;
        }
        let (lo, hi) = self.open();
        if lo >= hi {
            return None;
        }
        let untried = |i: usize| i < self.sizes.len() && self.judged[i].is_none();
        if let Some((f, bracket)) = self.first {
            if untried(f) {
                return Some(f);
            }
            // A good first guess: the size either side of it settles it.
            if bracket && self.order.len() == 1 {
                let side = match self.judged[f] {
                    Some(true) => f.checked_sub(1),
                    _ => Some(f + 1),
                };
                if let Some(s) = side.filter(|&s| s >= lo && s < hi && untried(s)) {
                    return Some(s);
                }
            }
        }
        let mid = lo + (hi - lo) / 2;
        if untried(mid) {
            return Some(mid);
        }
        (lo..hi).find(|&i| untried(i))
    }

    /// The size to try next (None: done).
    pub fn next_size(&self) -> Option<Size> {
        self.next().map(|i| self.sizes[i])
    }

    /// Records what the size at `index` (from [`next`](Self::next)) measured, the screen having
    /// shown its cells at `shown_px`.
    pub fn record(&mut self, index: usize, shown_px: f32, m: &Measure, l: &SceneLimits) -> &Rung {
        let asked = self.sizes[index];
        let own = asked.same(&self.own);
        let size = Size {
            px: shown_px,
            level: asked.level,
        };
        let r = rung_of(size, m, l, own);
        // The size recommended last time stays while within the noise; a finer one must be
        // clearly within the limits to displace it (#94).
        let line = match self.keep {
            Some(k) if k == index => 1.0 + NOISE,
            Some(k) if index < k => 1.0 - NOISE,
            _ => 1.0,
        };
        self.judged[index] = Some(r.load as f64 <= line);
        if r.load as f64 >= FAR_OVER {
            // Finer sizes would only crawl.
            for j in self.judged.iter_mut().take(index) {
                j.get_or_insert(false);
            }
        }
        self.card_of[index] = Some(self.rungs.len());
        self.rungs.push(r);
        self.order.push(index);
        self.rungs.last().expect("just pushed")
    }

    /// Records that the size at `index` could not be shown (its world could not be built): it
    /// and every finer size count as over.
    pub fn failed(&mut self, index: usize, why: &str) {
        for j in self.judged.iter_mut().take(index + 1) {
            *j = Some(false);
        }
        self.order.push(index);
        if self.open().0 >= self.sizes.len() {
            self.stopped = Some(why.to_string());
        }
    }

    /// Stops it where it is.
    pub fn stop(&mut self, why: &str) {
        self.stopped = Some(why.to_string());
    }

    /// Sizes tried.
    pub fn tried(&self) -> usize {
        self.order.len()
    }

    /// The most sizes it will try.
    pub fn planned(&self) -> usize {
        self.sizes.len().min(MAX_TRIALS)
    }

    /// The cards, finest first.
    pub fn cards(&self) -> Vec<Rung> {
        let mut v = self.rungs.clone();
        v.sort_by(|a, b| a.size.px.total_cmp(&b.size.px));
        v
    }

    /// The finest size found within the limits (with the allowance for the size kept), by index:
    /// the finest within them that is coarser than every size over them. None: none was.
    pub fn found(&self) -> Option<usize> {
        let (lo, _) = self.open();
        (lo..self.sizes.len()).find(|&i| self.judged[i] == Some(true))
    }

    /// The size to recommend, with its card when it was tried: the finest found within the
    /// limits; when none was, the coarsest.
    pub fn recommended(&self) -> (Size, Option<&Rung>) {
        let i = self.found().unwrap_or(self.sizes.len().saturating_sub(1));
        let card = self.card_at(i);
        (
            card.map_or(self.sizes.get(i).copied().unwrap_or_default(), |r| r.size),
            card,
        )
    }

    /// Whether a size was found within the limits.
    pub fn any_fit(&self) -> bool {
        self.found().is_some()
    }

    /// The card of the size at `index`, if it was shown.
    pub fn card_at(&self, index: usize) -> Option<&Rung> {
        self.rungs.get((*self.card_of.get(index)?)?)
    }
}

/// The size to recommend from a scene's cards: the finest within the limits, or a placed
/// pattern's own size; when none fits, the coarsest.
pub fn recommend(cards: &[Rung], placed: bool) -> Size {
    if placed && let Some(r) = cards.iter().find(|r| r.own) {
        return r.size;
    }
    cards
        .iter()
        .filter(|r| r.fits)
        .min_by(|a, b| a.size.px.total_cmp(&b.size.px))
        .or_else(|| cards.iter().max_by(|a, b| a.size.px.total_cmp(&b.size.px)))
        .map(|r| r.size)
        .unwrap_or_default()
}

/// Which size a sweep leaves a scene at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// The recommended size: the finest within the limits.
    Recommended,
    /// The finest size shown, within the limits or not.
    Finest,
    /// The scene's own size, shown in the sweep or not.
    Own,
    /// This cell size in physical pixels, shown in the sweep or not (an aperiodic patch: the
    /// nearest level).
    Px(f32),
}

impl Pick {
    /// `recommended`, `finest`, `own`, or a number of pixels.
    pub fn parse(s: &str) -> Result<Pick, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "recommended" => Ok(Pick::Recommended),
            "finest" => Ok(Pick::Finest),
            "own" => Ok(Pick::Own),
            t => t
                .trim_end_matches("px")
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|p| *p >= 1.0 && *p <= 64.0)
                .map(Pick::Px)
                .ok_or_else(|| {
                    format!("--pick is recommended, finest, own, or pixels 1 to 64, not {s:?}")
                }),
        }
    }

    pub fn word(&self) -> String {
        match self {
            Pick::Recommended => "recommended".into(),
            Pick::Finest => "finest".into(),
            Pick::Own => "own".into(),
            Pick::Px(p) => p.to_string(),
        }
    }
}

/// The nearest of `sizes` (a patch's levels) to `px`.
fn nearest_level(sizes: &[Size], px: f32) -> Option<Size> {
    sizes
        .iter()
        .filter(|s| s.level.is_some())
        .min_by(|a, b| (a.px - px).abs().total_cmp(&(b.px - px).abs()))
        .copied()
}

/// The size a scene shows at after its sweep, whether someone chose it, and a note when that needs
/// saying. With `pick` given, it decides: the scene's `own` size and a cell size in pixels are
/// placed whether or not the sweep showed them, so a sweep that stopped early (or a failed one)
/// can be undone (#39). Without, a size someone chose before is kept while it is still within the
/// limits (as when the monitors change), else the recommended one.
pub fn choose(
    cards: &[Rung],
    recommended: Size,
    own: Size,
    pick: Option<Pick>,
    before: Option<&TunedScene>,
) -> (Size, bool, Option<String>) {
    let shown: Vec<Size> = cards.iter().map(|r| r.size).collect();
    match pick {
        Some(Pick::Recommended) => return (recommended, false, None),
        Some(Pick::Finest) => {
            if let Some(r) = cards.iter().min_by(|a, b| a.size.px.total_cmp(&b.size.px)) {
                return (r.size, true, None);
            }
        }
        Some(Pick::Own) => {
            let size = cards.iter().find(|r| r.own).map_or(own, |r| r.size);
            return (size, true, None);
        }
        Some(Pick::Px(px)) => {
            if own.level.is_none() && recommended.level.is_none() {
                return (Size::px(px), true, None);
            }
            if let Some(s) = nearest_level(&shown, px) {
                return (s, true, None);
            }
        }
        None => {}
    }
    if let Some(b) = before.filter(|b| b.chosen) {
        let was = b.size();
        match cards.iter().find(|r| r.size.same(&was)) {
            Some(r) if r.fits || r.at_limit => return (r.size, true, None),
            Some(_) => {
                return (
                    recommended,
                    false,
                    Some(format!(
                        "the size chosen before, {}, is over the limits here; showing {}",
                        was.describe(),
                        recommended.describe()
                    )),
                );
            }
            None => {}
        }
    }
    (recommended, false, None)
}

/// What a fit needs to know of one scene of a screen's rotation (a series' rules are one scene).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Planned {
    /// Its label in the rotation (a series: its first rule's).
    pub label: String,
    /// The other rules of its series.
    pub also: Vec<String>,
    /// Its tiling's family.
    pub tiling: String,
    /// Its own cell size, in physical pixels.
    pub design: f32,
    /// Its own size, as the sweep tries it.
    pub own: Size,
    /// A pattern placed on an empty field: its own size unless someone chooses.
    pub placed: bool,
    /// Not rebuilt to fill the screen: its own size is the only one.
    pub fixed: bool,
    /// The sizes it can show at, finest first.
    pub sizes: Vec<Size>,
    /// Its estimated work per cell and second ([`cost_per_cell`]); None: not known.
    pub cost: Option<f64>,
    /// It runs as fast as it can, so its load hardly follows its size.
    pub unpaced: bool,
    /// It steps on the GPU (its rules run there, and the GPU setting allows it).
    pub gpu: bool,
    /// The frame rate it is held to.
    pub fps: f32,
    /// Why it shows as it does, when that needs saying.
    pub note: Option<String>,
}

impl Planned {
    /// Whether a fit can size it: it fills the screen, is no placed pattern, and has sizes.
    pub fn sizable(&self) -> bool {
        !self.fixed && !self.placed && !self.sizes.is_empty()
    }

    /// About how many cells it has at `size` on `canvas`.
    pub fn cells(&self, canvas: [f64; 2], size: &Size) -> u64 {
        cells_at(&self.tiling, canvas, size)
    }
}

/// The scene of a screen's rotation a fit measures: the most demanding of those it can size, by
/// [`Planned::cost`] (those that run as fast as they can only when nothing else can be). None
/// when none can be sized.
pub fn most_demanding(scenes: &[Planned]) -> Option<usize> {
    let pick = |paced: bool| {
        scenes
            .iter()
            .enumerate()
            .filter(|(_, p)| p.sizable() && p.unpaced != paced)
            .max_by(|(ia, a), (ib, b)| {
                a.cost
                    .unwrap_or(0.0)
                    .total_cmp(&b.cost.unwrap_or(0.0))
                    // The first of equals.
                    .then(ib.cmp(ia))
            })
            .map(|(i, _)| i)
    };
    pick(true).or_else(|| pick(false))
}

/// The measured scene's result: its cards, the recommended size, and the size chosen
/// ([`choose`]).
pub fn measured_scene(
    p: &Planned,
    search: &Search,
    gpu_timing: GpuTiming,
    pick: Option<Pick>,
    before: Option<&TunedScene>,
) -> TunedScene {
    let cards = search.cards();
    let (recommended, _) = search.recommended();
    let (mut size, chosen, note) = choose(&cards, recommended, p.own, pick, before);
    // The scene's own size is kept as the scene has it, not as the size its cells came to.
    if cards.iter().any(|r| r.own && r.size.same(&size)) || (size.same(&p.own) && chosen) {
        size = Size::px(p.design);
    }
    let mut notes: Vec<String> = p.note.iter().cloned().chain(note).collect();
    if !search.any_fit() && search.tried() > 0 {
        notes.push("no size was within the limits".into());
    }
    if let Some(s) = &search.stopped {
        notes.push(s.clone());
    }
    let mut t = TunedScene {
        scene: p.label.clone(),
        tiling: p.tiling.clone(),
        design_px: p.design,
        chosen,
        recommended,
        placed: p.placed,
        rungs: cards,
        gpu_timing,
        note: (!notes.is_empty()).then(|| notes.join("; ")),
        measured: true,
        cost: 1.0,
        also: p.also.clone(),
        gpu: p.gpu,
        ..Default::default()
    };
    t.set_size(size);
    t
}

/// Another scene's result: its size from the measured scene's, `base_cells` cells at a cost of
/// `base_cost` a cell, by their cost ratio ([`derive`]); a placed pattern, or a scene not rebuilt
/// to fill the screen, at its own size. A pick (`--pick`) or a size chosen before (kept while it
/// holds no more work than the measured scene's) decides as for the measured scene.
pub fn derived_scene(
    p: &Planned,
    canvas: [f64; 2],
    base_cells: u64,
    base_cost: Option<f64>,
    gpu_timing: GpuTiming,
    pick: Option<Pick>,
    before: Option<&TunedScene>,
) -> TunedScene {
    let ratio = match (p.cost, base_cost) {
        (Some(c), Some(b)) if b > 0.0 => c / b,
        _ => 1.0,
    };
    let cells = |s: &Size| p.cells(canvas, s);
    // A placed pattern can be given another size, but keeps its own unless someone does.
    let resizable = !p.fixed && !p.sizes.is_empty();
    // The size it was given last time, kept while within the noise (#94).
    let prev = before
        .filter(|b| !b.chosen && !b.measured)
        .map(TunedScene::size);
    let recommended = if p.sizable() && base_cells > 0 {
        derive_kept(&p.sizes, cells, ratio, base_cells, prev)
    } else {
        p.own
    };
    let mut note = p.note.clone();
    let (size, chosen) = match pick {
        Some(Pick::Own) => (p.own, true),
        Some(Pick::Px(px)) if resizable => match nearest_level(&p.sizes, px) {
            Some(l) => (l, true),
            None => (Size::px(px), true),
        },
        Some(Pick::Recommended | Pick::Finest) => (recommended, false),
        _ => match before.filter(|b| b.chosen) {
            Some(b) if cells(&b.size()) as f64 * ratio <= base_cells as f64 * (1.0 + NOISE) => {
                (b.size(), true)
            }
            Some(b) => {
                let words = format!(
                    "the size chosen before, {}, holds more work than the measured scene's size \
                     allows here; showing {}",
                    b.size().describe(),
                    recommended.describe()
                );
                note = Some(match note {
                    Some(n) => format!("{n}; {words}"),
                    None => words,
                });
                (recommended, false)
            }
            None => (recommended, false),
        },
    };
    let size = if size.same(&p.own) {
        Size::px(p.design)
    } else {
        size
    };
    let mut t = TunedScene {
        scene: p.label.clone(),
        tiling: p.tiling.clone(),
        design_px: p.design,
        chosen,
        recommended,
        placed: p.placed,
        gpu_timing,
        note,
        // None for a scene that can't be given another size: it has no sizes to choose from.
        cost: if resizable { ratio as f32 } else { 0.0 },
        also: p.also.clone(),
        gpu: p.gpu,
        ..Default::default()
    };
    t.set_size(size);
    t
}

/// The cells the other scenes' sizes come from, in the measured scene's work: where its loads
/// cross the limits' line ([`line_cells`]), so a scene of the same cost may go finer than the
/// measured scene's ladder had room for; or, once someone chose its size, its cells there.
pub fn base_cells(measured: &TunedScene, canvas: [f64; 2]) -> u64 {
    let size = measured.size();
    let at_size = measured
        .rung()
        .map(|r| r.cells)
        .filter(|&c| c > 0)
        .unwrap_or_else(|| cells_at(&measured.tiling, canvas, &size));
    if measured.chosen {
        return at_size;
    }
    line_cells(&measured.rungs).map_or(at_size, |c| c.round() as u64)
}

/// A screen's results: the measured scene's ([`measured_scene`]) and every other's
/// ([`derived_scene`]), in the rotation's order. `before` gives what was saved for each label.
#[allow(clippy::too_many_arguments)]
pub fn screen_results(
    scenes: &[Planned],
    measured: Option<usize>,
    search: Option<&Search>,
    canvas: [f64; 2],
    gpu_timing: GpuTiming,
    pick: Option<Pick>,
    before: &dyn Fn(&str) -> Option<TunedScene>,
) -> Vec<TunedScene> {
    let first = match (measured, search) {
        (Some(m), Some(s)) => Some(measured_scene(
            &scenes[m],
            s,
            gpu_timing,
            pick,
            before(&scenes[m].label).as_ref(),
        )),
        _ => None,
    };
    // The work the rest may have: the measured scene's cells on its limits' line, or at the size
    // chosen for it. A scene that steps on the other device (the processor where the measured
    // scene used the GPU, or the other way about) has only the estimate of their costs to go by,
    // so it is not sized past the measured scene's own size.
    let (line, at_size, base_cost, base_gpu) = match (&first, measured) {
        (Some(t), Some(m)) => {
            let size = t.size();
            let at_size = t
                .rung()
                .map(|r| r.cells)
                .filter(|&c| c > 0)
                .unwrap_or_else(|| cells_at(&t.tiling, canvas, &size));
            let line = base_cells(t, canvas);
            (line, at_size.min(line), scenes[m].cost, scenes[m].gpu)
        }
        _ => (0, 0, None, false),
    };
    scenes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if Some(i) == measured
                && let Some(t) = &first
            {
                return t.clone();
            }
            if first.is_some() && p.gpu != base_gpu && p.sizable() {
                let device = |gpu: bool| if gpu { "the GPU" } else { "the processor" };
                let words = format!(
                    "steps on {}, the measured scene on {}: sized by estimate, no finer than the \
                     measured scene's work",
                    device(p.gpu),
                    device(base_gpu)
                );
                let mut t = derived_scene(
                    p,
                    canvas,
                    at_size,
                    base_cost,
                    gpu_timing,
                    pick,
                    before(&p.label).as_ref(),
                );
                t.note = Some(match t.note {
                    Some(n) => format!("{n}; {words}"),
                    None => words,
                });
                return t;
            }
            let base_cells = line;
            if first.is_none() {
                // Nothing measured: each at its own size, or as picked.
                return derived_scene(
                    &Planned {
                        sizes: vec![],
                        ..p.clone()
                    },
                    canvas,
                    0,
                    None,
                    gpu_timing,
                    pick.filter(|k| matches!(k, Pick::Own | Pick::Px(_))),
                    before(&p.label).as_ref(),
                );
            }
            derived_scene(
                p,
                canvas,
                base_cells,
                base_cost,
                gpu_timing,
                pick,
                before(&p.label).as_ref(),
            )
        })
        .collect()
}

/// The sizes a saved fit's scenes show at, picked again without measuring (#77): the measured
/// scene's from its cards, the rest from it by their cost ratios. `sizes` gives each scene's
/// sizes (finest first), as a fit plans them.
pub fn repick(
    scenes: &[TunedScene],
    sizes: &dyn Fn(&TunedScene) -> Vec<Size>,
    canvas: [f64; 2],
    pick: Pick,
) -> Vec<TunedScene> {
    let measured = scenes.iter().find(|s| s.measured).map(|m| {
        let (size, chosen, _) = choose(
            &m.rungs,
            m.recommended,
            Size::px(m.design_px),
            Some(pick),
            None,
        );
        let mut t = m.clone();
        t.note = None;
        let own = m.rungs.iter().any(|r| r.own && r.size.same(&size));
        t.set_size(if own { Size::px(m.design_px) } else { size });
        t.chosen = chosen;
        t
    });
    let line = measured.as_ref().map_or(0, |m| base_cells(m, canvas));
    let (at_size, gpu) = measured.as_ref().map_or((0, false), |m| {
        let size = m.size();
        let cells = m
            .rung()
            .map(|r| r.cells)
            .filter(|&c| c > 0)
            .unwrap_or_else(|| cells_at(&m.tiling, canvas, &size));
        (cells.min(line), m.gpu)
    });
    scenes
        .iter()
        .map(|s| {
            if s.measured
                && let Some(m) = &measured
            {
                return m.clone();
            }
            let base = if s.gpu == gpu { line } else { at_size };
            let mut t = s.clone();
            t.note = None;
            let ladder = sizes(s);
            let own = Size::px(s.design_px);
            let (size, chosen) = match pick {
                Pick::Own => (own, true),
                Pick::Px(px) if !ladder.is_empty() => {
                    (nearest_level(&ladder, px).unwrap_or(Size::px(px)), true)
                }
                Pick::Px(_) => (own, true),
                Pick::Recommended | Pick::Finest if !ladder.is_empty() && !s.placed && base > 0 => {
                    let ratio = if s.cost > 0.0 { s.cost as f64 } else { 1.0 };
                    let size = derive(&ladder, |x| cells_at(&s.tiling, canvas, x), ratio, base);
                    t.recommended = size;
                    (size, false)
                }
                _ => (s.recommended, false),
            };
            t.set_size(if size.same(&own) { own } else { size });
            t.chosen = chosen;
            t
        })
        .collect()
}

/// The sizes a saved scene can be chosen at, as a fit plans them: a lattice's ladder, a patch's
/// levels; none for a scene a fit could not size (not rebuilt to fill the screen: it keeps its
/// own).
pub fn scene_sizes(t: &Tuning, s: &TunedScene) -> Vec<Size> {
    let canvas = [t.canvas[0] as f64, t.canvas[1] as f64];
    let most = if t.max_cells > 0 {
        t.max_cells
    } else {
        SWEEP_MAX_CELLS
    };
    if s.rungs.is_empty() && s.cost == 0.0 {
        return vec![];
    }
    let patch = jetcarp_core::families::family(&s.tiling)
        .is_some_and(|f| f.kind == jetcarp_core::families::FamilyKind::Aperiodic);
    if patch {
        patch_ladder(&s.tiling, canvas, s.level, most)
    } else {
        lattice_ladder(canvas, s.design_px, most)
    }
}

/// After the measured scene's size changed (a card chosen), every other scene nobody chose takes
/// its size from it again, by its cost ratio.
pub fn rederive(t: &mut Tuning) {
    let canvas = [t.canvas[0] as f64, t.canvas[1] as f64];
    let Some(m) = t.scenes.iter().find(|s| s.measured) else {
        return;
    };
    let line = base_cells(m, canvas);
    let size = m.size();
    let at_size = m
        .rung()
        .map(|r| r.cells)
        .filter(|&c| c > 0)
        .unwrap_or_else(|| cells_at(&m.tiling, canvas, &size))
        .min(line);
    let gpu = m.gpu;
    let ladders: Vec<Vec<Size>> = t.scenes.iter().map(|s| scene_sizes(t, s)).collect();
    for (s, ladder) in t.scenes.iter_mut().zip(ladders) {
        if s.measured || s.chosen || s.placed || ladder.is_empty() || s.cost <= 0.0 {
            continue;
        }
        let base = if s.gpu == gpu { line } else { at_size };
        let tiling = s.tiling.clone();
        let size = derive(
            &ladder,
            |x| cells_at(&tiling, canvas, x),
            s.cost as f64,
            base,
        );
        s.recommended = size;
        let own = Size::px(s.design_px);
        s.set_size(if size.same(&own) { own } else { size });
    }
}

/// A card's picture's file name: `s0-profile-life-3px.png`, `s1-penrose-waves-l6.png`.
pub fn crop_name(screen: usize, scene: &str, size: &Size) -> String {
    let what = match size.level {
        Some(l) => format!("l{l}"),
        None => format!(
            "{}px",
            crate::host_settings::round_px(size.px).replace('.', "_")
        ),
    };
    format!("s{screen}-{}-{what}.png", crate::profiles::slug(scene))
}

/// A card's picture's size in physical pixels on a canvas of `canvas`: about 192 by 120, with
/// each side's evenness the canvas's, so the picture's pixels fall on the screen's.
pub fn crop_size(canvas: [f64; 2]) -> [u32; 2] {
    let fit = |want: u32, side: f64| {
        let side = side.round().max(1.0) as u32;
        let w = want.min(side);
        if (side - w).is_multiple_of(2) {
            w
        } else {
            w - 1
        }
        .max(1)
    };
    [fit(192, canvas[0]), fit(120, canvas[1])]
}

/// One screen's part in a fit: its share of the desktop's limits (#107), and the scene measured.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ScreenShare {
    pub screen: usize,
    /// Its canvas, in physical pixels.
    pub canvas: [u32; 2],
    /// Its share of the limits: its pixel area over every screen's shown at once.
    pub share: f64,
    /// Its share of the processor limit, and of the GPU limit, in percent of the machine.
    pub cpu_percent: f32,
    pub gpu_percent: f32,
    /// The scene measured for it (None: none of its scenes can be sized).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    /// Scenes in its rotation (a series counted once).
    #[serde(default)]
    pub scenes: usize,
}

impl ScreenShare {
    /// For people: `screen 0 (2560x1440, 64% of the area): processor 6.4%, GPU 16%`.
    pub fn describe(&self, screens: usize) -> String {
        let area = if screens > 1 {
            format!(", {:.0}% of the area", self.share * 100.0)
        } else {
            String::new()
        };
        format!(
            "screen {} ({}x{}{area}): processor {}, GPU {}",
            self.screen,
            self.canvas[0],
            self.canvas[1],
            percent(self.cpu_percent as f64 / 100.0),
            percent(self.gpu_percent as f64 / 100.0),
        )
    }
}

/// How the limits are shared, in a sentence: `one limit for the whole desktop, shared by pixel
/// area: ...` (#107).
pub fn shares_line(limits: &TuneLimits, shares: &[ScreenShare]) -> String {
    let parts: Vec<String> = shares.iter().map(|s| s.describe(shares.len())).collect();
    if shares.len() > 1 {
        format!(
            "the {} processor and {} GPU limits are one for the whole desktop, shared between \
             the screens by their pixel area: {}",
            percent(limits.cpu_percent as f64 / 100.0),
            percent(limits.gpu_percent as f64 / 100.0),
            parts.join("; ")
        )
    } else {
        format!("limits: {}", parts.join("; "))
    }
}

/// One line of what a tuner reports, as `jetcarp host tune --json` prints it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TuneEvent {
    /// The fit has begun: this many scenes on these screens, at this frame rate (each scene is
    /// held to the rate it draws at: with `fps_max`, from `fps` to that), each screen's share of
    /// the limits and the scene measured on it (`shares`), or with `reused`, sizes placed from
    /// the last fit, nothing measured (#77).
    Start {
        screens: Vec<usize>,
        scenes: usize,
        fps: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fps_max: Option<f32>,
        gpu: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        shares: Vec<ScreenShare>,
        /// The limits shared, for the opening line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limits: Option<TuneLimits>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reused: bool,
    },
    /// A size is on show.
    Rung {
        screen: usize,
        /// The scene, counted from 1 across every screen.
        number: usize,
        of: usize,
        scene: String,
        size: Size,
        /// The size, counted from 1, and how many the scene has at most.
        rung: usize,
        rungs: usize,
    },
    /// What a size measured.
    Measured {
        screen: usize,
        scene: String,
        rung: Rung,
    },
    /// A scene is done: its cards, and the size it shows at.
    Scene { screen: usize, result: TunedScene },
    /// Everything is done (and saved, unless it was a dry run).
    Done {
        /// Where the host's settings were saved (None: they were not).
        saved: Option<String>,
        scenes: usize,
        /// How the GPU's time was measured.
        #[serde(default)]
        gpu_timing: GpuTiming,
        /// [`GPU_UNMEASURED`], when the GPU share wasn't measured ([`TuneEvent::done`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gpu_note: Option<String>,
        /// Sizes tried in all.
        #[serde(default)]
        trials: usize,
        /// How long the fit took, seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seconds: Option<f64>,
    },
    /// It stopped early.
    Failed { error: String },
}

impl TuneEvent {
    /// The fit's last line: saved to `saved` (None: not saved), `scenes` scenes, the GPU timed
    /// by `gpu_timing`, saying so plainly when its share wasn't measured (#72).
    pub fn done(saved: Option<String>, scenes: usize, gpu_timing: GpuTiming) -> TuneEvent {
        TuneEvent::Done {
            saved,
            scenes,
            gpu_timing,
            gpu_note: gpu_timing.unmeasured().then(|| GPU_UNMEASURED.to_string()),
            trials: 0,
            seconds: None,
        }
    }

    /// The same, with the sizes tried and the seconds taken.
    pub fn done_in(
        saved: Option<String>,
        scenes: usize,
        gpu_timing: GpuTiming,
        trials: usize,
        seconds: f64,
    ) -> TuneEvent {
        match TuneEvent::done(saved, scenes, gpu_timing) {
            TuneEvent::Done {
                saved,
                scenes,
                gpu_timing,
                gpu_note,
                ..
            } => TuneEvent::Done {
                saved,
                scenes,
                gpu_timing,
                gpu_note,
                trials,
                seconds: Some(seconds),
            },
            other => other,
        }
    }

    /// One line for people.
    pub fn describe(&self) -> String {
        match self {
            TuneEvent::Start {
                screens,
                scenes,
                fps,
                fps_max,
                gpu,
                shares,
                limits,
                reused,
            } => {
                let mut s = if *reused {
                    format!(
                        "placing the sizes the last fit found for {scenes} scene{} on {} \
                         screen{}: nothing has changed since (--fresh measures again)",
                        if *scenes == 1 { "" } else { "s" },
                        screens.len(),
                        if screens.len() == 1 { "" } else { "s" },
                    )
                } else {
                    format!(
                        "fitting {scenes} scene{} on {} screen{}{} at {} frames a second, {}",
                        if *scenes == 1 { "" } else { "s" },
                        screens.len(),
                        if screens.len() == 1 { "" } else { "s" },
                        if screens.len() > 1 { " at once" } else { "" },
                        match fps_max {
                            Some(m) if *m > *fps => {
                                format!("{} to {}", round_fps(*fps), round_fps(*m))
                            }
                            _ => round_fps(*fps),
                        },
                        gpu.as_deref()
                            .map_or("on the processor".to_string(), |g| format!("on {g}"))
                    )
                };
                if let (Some(l), false) = (limits, shares.is_empty()) {
                    s.push_str(&format!("; {}", shares_line(l, shares)));
                }
                for sh in shares.iter().filter(|_| !*reused) {
                    match &sh.scene {
                        Some(n) => s.push_str(&format!(
                            "; screen {} measures {n}, the most demanding of its {} scene{}",
                            sh.screen,
                            sh.scenes,
                            if sh.scenes == 1 { "" } else { "s" }
                        )),
                        None => s.push_str(&format!(
                            "; screen {}: no scene can be sized, each keeps its own",
                            sh.screen
                        )),
                    }
                }
                s
            }
            TuneEvent::Rung {
                screen,
                scene,
                size,
                rung,
                rungs,
                ..
            } => format!(
                "screen {screen}: {scene} at {} (size {rung} of up to {rungs})",
                // A patch's tiles come to whatever size fits it to the screen: its level alone,
                // until the measured line says how big they are.
                match size.level {
                    Some(l) => format!("level {l}"),
                    None => size.describe(),
                }
            ),
            TuneEvent::Measured { rung, .. } => format!("  {}", rung_line(rung)),
            TuneEvent::Scene { screen, result } => scene_line(*screen, result),
            TuneEvent::Done {
                saved,
                scenes,
                gpu_timing,
                trials,
                seconds,
                ..
            } => {
                let how = match gpu_timing {
                    GpuTiming::None => String::new(),
                    t if t.unmeasured() => format!("; {GPU_UNMEASURED}"),
                    t => format!(" (GPU time from {})", t.describe()),
                };
                let took = match seconds {
                    Some(s) => format!(
                        " in {s:.1} s, {trials} size{} tried",
                        if *trials == 1 { "" } else { "s" }
                    ),
                    None => String::new(),
                };
                match saved {
                    Some(p) => format!("fitted {scenes} scene(s){took}{how}; saved to {p}"),
                    None => format!("fitted {scenes} scene(s){took}{how}; not saved"),
                }
            }
            TuneEvent::Failed { error } => format!("stopped: {error}"),
        }
    }
}

/// A frame rate as people read it: whole, or to a hundredth (59.94).
pub fn round_fps(fps: f32) -> String {
    if (fps - fps.round()).abs() < 0.005 {
        format!("{}", fps.round())
    } else {
        format!("{fps:.2}")
    }
}

/// The frame rates a sweep's scenes are held to, slowest and fastest (the fastest only when it
/// differs), for [`TuneEvent::Start`].
pub fn fps_range(rates: impl IntoIterator<Item = f32>) -> (f32, Option<f32>) {
    let (lo, hi) = rates
        .into_iter()
        .fold((f32::INFINITY, 0.0f32), |(lo, hi), r| {
            (lo.min(r), hi.max(r))
        });
    if !lo.is_finite() {
        return (0.0, None);
    }
    (lo, (hi > lo).then_some(hi))
}

/// A card's numbers, for people.
pub fn rung_line(r: &Rung) -> String {
    let gpu = match r.gpu_percent {
        Some(g) => format!(", GPU {}", percent(g as f64 / 100.0)),
        None if r.gpu_unmeasured => ", GPU not measured".into(),
        None => String::new(),
    };
    format!(
        "{}, {} cells: processor {}{gpu}, {:.0} fps, frames {:.1} ms, on the {}{}{}{}",
        r.size.describe(),
        thousands(r.cells),
        percent(r.cpu_percent as f64 / 100.0),
        r.fps,
        r.p95_ms,
        if r.on_gpu { "GPU" } else { "processor" },
        if r.own { ", its own size" } else { "" },
        match &r.over {
            None => " — within the limits".to_string(),
            Some(o) => format!(" — over: {o}"),
        },
        if r.at_limit {
            " (at the limit: within the measurement's noise of it)"
        } else {
            ""
        }
    )
}

/// One scene's result, for people.
pub fn scene_line(screen: usize, t: &TunedScene) -> String {
    // As its cells show (a scene kept at its own size shows at its own rung's).
    let shows = t.rung().map_or(t.size(), |r| r.size);
    let what = if t.measured {
        format!(
            "measured, {} sizes; recommended {}",
            t.rungs.len(),
            t.recommended.describe()
        )
    } else if t.placed {
        "a placed pattern, at its own size unless chosen".to_string()
    } else if t.cost > 0.0 {
        format!(
            "sized from the measured scene, at {} its work a cell",
            if (t.cost - 1.0).abs() < 0.005 {
                "the same as".to_string()
            } else {
                format!("{:.2} times", t.cost)
            }
        )
    } else {
        "its own size".to_string()
    };
    let mut s = format!(
        "screen {screen}: {}{}: {what}; shows at {}{}",
        t.scene,
        if t.also.is_empty() {
            String::new()
        } else {
            format!(
                " and {} more rule{} of its series",
                t.also.len(),
                if t.also.len() == 1 { "" } else { "s" }
            )
        },
        shows.describe(),
        if t.chosen { " (chosen)" } else { "" }
    );
    if let Some(r) = t.rung() {
        s.push_str(&format!(", {} cells", thousands(r.cells)));
    }
    if let Some(n) = &t.note {
        s.push_str(&format!(" ({n})"));
    }
    s
}

/// A count with thousands separated by commas.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What to sweep for, as `jetcarp host tune` takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TuneRequest {
    pub cpu_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
    pub fps: Option<f32>,
    /// Only this screen.
    pub screen: Option<usize>,
    /// Only these screens (the host's own fit when monitors change: those whose monitor did,
    /// #110). Empty: every screen on the desktop.
    pub screens: Vec<usize>,
    /// Which size each scene is left at (None: a size chosen before while it fits, else the
    /// recommended one).
    pub pick: Option<Pick>,
    /// With `pick`: measure again even if the last fit is still current (#77).
    pub fresh: bool,
    /// Measure and report, but save nothing.
    pub dry_run: bool,
    /// Started by the host because the monitors changed.
    pub auto: bool,
}

impl TuneRequest {
    /// The command line after `jetcarp host tune`.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![];
        if let Some(x) = self.cpu_percent {
            a.extend(["--cpu-limit".into(), x.to_string()]);
        }
        if let Some(x) = self.gpu_percent {
            a.extend(["--gpu-limit".into(), x.to_string()]);
        }
        if let Some(x) = self.fps {
            a.extend(["--fps".into(), x.to_string()]);
        }
        if let Some(x) = self.screen {
            a.extend(["--screen".into(), x.to_string()]);
        }
        if let Some(p) = self.pick {
            a.extend(["--pick".into(), p.word()]);
        }
        if self.fresh {
            a.push("--fresh".into());
        }
        if self.dry_run {
            a.push("--dry-run".into());
        }
        if self.auto {
            a.push("--auto".into());
        }
        a
    }

    /// Whether screen `i` is one to fit.
    pub fn wants(&self, i: usize) -> bool {
        self.screen.is_none_or(|s| s == i) && (self.screens.is_empty() || self.screens.contains(&i))
    }
}

/// A tuner running as a child process (`jetcarp host tune --json`), its progress read from its
/// output on a thread of its own.
pub struct TuneChild {
    child: Child,
    rx: Receiver<TuneEvent>,
    /// Every event so far.
    pub events: Vec<TuneEvent>,
    exit: Option<std::process::ExitStatus>,
}

impl TuneChild {
    /// Starts `jetcarp host tune --json` with `req`'s options.
    pub fn spawn(req: &TuneRequest) -> Result<TuneChild, String> {
        let cmd = crate::autostart::jetcarp_exe()?;
        let mut c = Process::new(cmd);
        c.args(["host", "tune", "--json"])
            .args(req.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up from the app or the host.
            c.creation_flags(0x0800_0000);
        }
        let mut child = c
            .spawn()
            .map_err(|e| format!("could not start the tuner: {e}"))?;
        let out = child.stdout.take().ok_or("the tuner has no output")?;
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("jetcarp-tune-read".into())
            .spawn(move || {
                for line in std::io::BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(e) = serde_json::from_str::<TuneEvent>(&line)
                        && tx.send(e).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(TuneChild {
            child,
            rx,
            events: vec![],
            exit: None,
        })
    }

    /// New events since the last call (also kept in `events`).
    pub fn poll(&mut self) -> Vec<TuneEvent> {
        let new: Vec<TuneEvent> = self.rx.try_iter().collect();
        self.events.extend(new.iter().cloned());
        if self.exit.is_none()
            && let Ok(Some(s)) = self.child.try_wait()
        {
            self.exit = Some(s);
            // Whatever it wrote before it ended.
            let rest: Vec<TuneEvent> = self.rx.try_iter().collect();
            self.events.extend(rest.iter().cloned());
            return new.into_iter().chain(rest).collect();
        }
        new
    }

    /// Whether it has ended (after [`poll`](Self::poll) noticed).
    pub fn finished(&self) -> bool {
        self.exit.is_some()
    }

    /// How it ended: Ok once it reported `done`, else why not.
    pub fn outcome(&self) -> Option<Result<(), String>> {
        let exit = self.exit?;
        Some(
            match self
                .events
                .iter()
                .rev()
                .find(|e| matches!(e, TuneEvent::Done { .. } | TuneEvent::Failed { .. }))
            {
                Some(TuneEvent::Done { .. }) => Ok(()),
                Some(TuneEvent::Failed { error }) => Err(error.clone()),
                _ if exit.success() => Ok(()),
                _ => Err(format!("the tuner ended ({exit})")),
            },
        )
    }

    /// The latest progress line, for people.
    pub fn progress(&self) -> Option<String> {
        self.events
            .iter()
            .rev()
            .find(|e| matches!(e, TuneEvent::Rung { .. } | TuneEvent::Start { .. }))
            .map(TuneEvent::describe)
    }

    /// Stops it (the scenes swept so far stay saved).
    pub fn stop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            self.exit = self.child.wait().ok();
        }
    }
}

impl Drop for TuneChild {
    fn drop(&mut self) {
        // A tuner nobody follows any more finishes on its own and saves what it found.
        let _ = self.child.try_wait();
    }
}

#[cfg(test)]
mod tests;
