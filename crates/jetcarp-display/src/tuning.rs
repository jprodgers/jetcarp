//! *Fit to this computer*: a sweep through the sizes each desktop background scene can show at,
//! measured, so the person can see which work and choose one.
//!
//! For each scene of each screen on the desktop, the sweep shows the scene at each size of a
//! ladder for a couple of seconds, coarsest first, and measures JETCARP's own load: processor
//! time, GPU time, and how long frames take. The rules, palettes and colouring stay as they are;
//! only the number of cells changes. On a lattice the ladder is cell sizes in physical pixels
//! ([`LADDER`], [`lattice_ladder`]); on an aperiodic tiling it is patch levels ([`patch_ladder`]).
//! Each size becomes a card ([`Rung`]) with a picture of the screen, its numbers, and whether it is
//! within the limits ([`fits`]). The finest size within the limits is recommended ([`recommend`]);
//! a pattern placed on an empty field ([`is_placed`]) keeps its own size unless someone chooses.
//!
//! The background host runs the sweep on the real screen. With no host running, `jetcarp host
//! tune` runs it off screen in a process of its own, which the app starts with [`TuneChild`].

use crate::host_settings::{GpuTiming, Rung, Size, TuneLimits, TunedScene};
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
/// Seconds each size shows for.
pub const RUNG_SECONDS: f64 = 2.0;
/// A size this far over a limit is not followed by finer ones.
pub const FAR_OVER: f64 = 2.0;

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
    /// The 95th percentile of frame times, in seconds.
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

/// The limits one scene is held to: the user's, split among the screens shown at once.
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
    /// The user's limits for one of `screens` screens shown at once, at `fps`.
    pub fn of(l: &TuneLimits, fps: f32, screens: usize) -> SceneLimits {
        let n = screens.max(1) as f64;
        SceneLimits {
            cpu: l.cpu_percent as f64 / 100.0 / n,
            gpu: l.gpu_percent as f64 / 100.0 / n,
            fps: fps.max(1.0) as f64,
        }
    }

    /// The frame interval, in seconds.
    pub fn interval(&self) -> f64 {
        1.0 / self.fps.max(1.0)
    }
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
    if let Some(g) = m.gpu
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
    if let Some(want) = m.gens_wanted
        && want > 0.0
        && m.gens_done < want * KEEP_UP
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

/// Whether a size is so far over a limit ([`FAR_OVER`] times) that finer sizes need not be shown:
/// they would only crawl.
pub fn far_over(m: &Measure, l: &SceneLimits) -> bool {
    m.cpu > l.cpu * FAR_OVER
        || m.gpu.is_some_and(|g| g > l.gpu * FAR_OVER)
        || m.p95 > l.interval() * FAR_OVER
        || m.gens_wanted
            .is_some_and(|w| w > 0.0 && m.gens_done < w / FAR_OVER)
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

/// One size's card from what it measured.
pub fn rung_of(size: Size, m: &Measure, l: &SceneLimits, own: bool) -> Rung {
    let over = fits(m, l).err();
    Rung {
        size,
        cells: m.cells,
        cpu_percent: (m.cpu * 100.0) as f32,
        gpu_percent: m.gpu.map(|g| (g * 100.0) as f32),
        p95_ms: (m.p95 * 1000.0) as f32,
        fps: m.fps as f32,
        on_gpu: m.on_gpu,
        fits: over.is_none(),
        over,
        crop: None,
        own,
    }
}

/// One scene's sweep: which size to show next, coarsest first, and the cards so far. It stops
/// going finer once a size is far over a limit ([`far_over`]).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSweep {
    /// The sizes to show, finest first.
    pub sizes: Vec<Size>,
    /// The scene's own size.
    pub own: Size,
    /// How many have been shown (from the coarsest).
    shown: usize,
    /// Each size shown, in the order shown.
    pub rungs: Vec<Rung>,
    /// Why finer sizes were not shown, when they were not.
    pub stopped: Option<String>,
}

impl SceneSweep {
    pub fn new(sizes: Vec<Size>, own: Size) -> SceneSweep {
        SceneSweep {
            sizes,
            own,
            shown: 0,
            rungs: vec![],
            stopped: None,
        }
    }

    /// The size to show next (None: done).
    pub fn next_size(&self) -> Option<Size> {
        if self.stopped.is_some() {
            return None;
        }
        let n = self.sizes.len();
        (self.shown < n).then(|| self.sizes[n - 1 - self.shown])
    }

    /// Records what `asked` (the size [`next_size`](Self::next_size) gave) measured, the screen
    /// having shown its cells at `shown_px`.
    pub fn record(&mut self, asked: Size, shown_px: f32, m: &Measure, l: &SceneLimits) -> &Rung {
        let own = asked.same(&self.own);
        let size = Size {
            px: shown_px,
            level: asked.level,
        };
        self.rungs.push(rung_of(size, m, l, own));
        self.shown += 1;
        if far_over(m, l) && self.next_size().is_some() {
            self.stopped = Some(format!(
                "finer sizes were not shown: {} was already far over the limits",
                size.describe()
            ));
        }
        self.rungs.last().expect("just pushed")
    }

    /// How many sizes it will show at most.
    pub fn planned(&self) -> usize {
        self.sizes.len()
    }

    /// The cards, finest first.
    pub fn cards(&self) -> Vec<Rung> {
        let mut v = self.rungs.clone();
        v.sort_by(|a, b| a.size.px.total_cmp(&b.size.px));
        v
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
    /// The scene's own size.
    Own,
    /// This cell size in physical pixels (the nearest shown).
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

/// The size a scene shows at after its sweep, whether someone chose it, and a note when that needs
/// saying. With `pick` given, it decides. Without, a size someone chose before is kept while it is
/// still within the limits (as when the monitors change), else the recommended one.
pub fn choose(
    cards: &[Rung],
    recommended: Size,
    pick: Option<Pick>,
    before: Option<&TunedScene>,
) -> (Size, bool, Option<String>) {
    let nearest = |px: f32| {
        cards
            .iter()
            .filter(|r| r.size.level.is_none())
            .min_by(|a, b| (a.size.px - px).abs().total_cmp(&(b.size.px - px).abs()))
            .map(|r| r.size)
    };
    match pick {
        Some(Pick::Recommended) => return (recommended, false, None),
        Some(Pick::Finest) => {
            if let Some(r) = cards.iter().min_by(|a, b| a.size.px.total_cmp(&b.size.px)) {
                return (r.size, true, None);
            }
        }
        Some(Pick::Own) => {
            if let Some(r) = cards.iter().find(|r| r.own) {
                return (r.size, true, None);
            }
        }
        Some(Pick::Px(px)) => {
            if let Some(s) = nearest(px) {
                return (s, true, None);
            }
        }
        None => {}
    }
    if let Some(b) = before.filter(|b| b.chosen) {
        let was = b.size();
        match cards.iter().find(|r| r.size.same(&was)) {
            Some(r) if r.fits => return (r.size, true, None),
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

/// A scene's result: its cards, the recommended size, and the size chosen ([`choose`]).
#[allow(clippy::too_many_arguments)]
pub fn tuned_scene(
    label: &str,
    tiling: &str,
    design_px: f32,
    placed: bool,
    sweep: &SceneSweep,
    gpu_timing: GpuTiming,
    pick: Option<Pick>,
    before: Option<&TunedScene>,
) -> TunedScene {
    let cards = sweep.cards();
    let recommended = recommend(&cards, placed);
    let (mut size, chosen, note) = choose(&cards, recommended, pick, before);
    // The scene's own size is kept as the scene has it, not as the size its cells came to.
    if cards.iter().any(|r| r.own && r.size.same(&size)) {
        size = Size::px(design_px);
    }
    let mut notes: Vec<String> = note.into_iter().collect();
    if !cards.iter().any(|r| r.fits) && !cards.is_empty() {
        notes.push("no size was within the limits".into());
    }
    if let Some(s) = &sweep.stopped {
        notes.push(s.clone());
    }
    let mut t = TunedScene {
        scene: label.to_string(),
        tiling: tiling.to_string(),
        design_px,
        chosen,
        recommended,
        placed,
        rungs: cards,
        gpu_timing,
        note: (!notes.is_empty()).then(|| notes.join("; ")),
        ..Default::default()
    };
    t.set_size(size);
    t
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

/// One line of what a tuner reports, as `jetcarp host tune --json` prints it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TuneEvent {
    /// The sweep has begun: this many scenes on these screens, at this frame rate.
    Start {
        screens: Vec<usize>,
        scenes: usize,
        fps: f32,
        gpu: Option<String>,
    },
    /// A size is on show.
    Rung {
        screen: usize,
        /// The scene, counted from 1 across every screen.
        number: usize,
        of: usize,
        scene: String,
        size: Size,
        /// The size, counted from 1 (coarsest first), and how many the scene has at most.
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
    },
    /// It stopped early.
    Failed { error: String },
}

impl TuneEvent {
    /// One line for people.
    pub fn describe(&self) -> String {
        match self {
            TuneEvent::Start {
                screens,
                scenes,
                fps,
                gpu,
            } => format!(
                "sweeping {scenes} scene{} on {} screen{} at {fps} frames a second, {}",
                if *scenes == 1 { "" } else { "s" },
                screens.len(),
                if screens.len() == 1 { "" } else { "s" },
                gpu.as_deref()
                    .map_or("on the processor".to_string(), |g| format!("on {g}"))
            ),
            TuneEvent::Rung {
                screen,
                number,
                of,
                scene,
                size,
                rung,
                rungs,
            } => format!(
                "screen {screen}, scene {number} of {of}: {scene} at {} (size {rung} of up to {rungs})",
                size.describe()
            ),
            TuneEvent::Measured { rung, .. } => format!("  {}", rung_line(rung)),
            TuneEvent::Scene { screen, result } => scene_line(*screen, result),
            TuneEvent::Done {
                saved,
                scenes,
                gpu_timing,
            } => {
                let how = match gpu_timing {
                    GpuTiming::None => String::new(),
                    t => format!(" (GPU time from {})", t.describe()),
                };
                match saved {
                    Some(p) => format!("swept {scenes} scene(s){how}; saved to {p}"),
                    None => format!("swept {scenes} scene(s){how}; not saved"),
                }
            }
            TuneEvent::Failed { error } => format!("stopped: {error}"),
        }
    }
}

/// A card's numbers, for people.
pub fn rung_line(r: &Rung) -> String {
    let gpu = r.gpu_percent.map_or(String::new(), |g| {
        format!(", GPU {}", percent(g as f64 / 100.0))
    });
    format!(
        "{}, {} cells: processor {}{gpu}, {:.0} fps, frames {:.1} ms, on the {}{}{}",
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
        }
    )
}

/// One scene's result, for people.
pub fn scene_line(screen: usize, t: &TunedScene) -> String {
    let mut s = format!(
        "screen {screen}: {}: {} sizes; recommended {}; shows at {}{}",
        t.scene,
        t.rungs.len(),
        t.recommended.describe(),
        t.size().describe(),
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
    /// Which size each scene is left at (None: a size chosen before while it fits, else the
    /// recommended one).
    pub pick: Option<Pick>,
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
        if self.dry_run {
            a.push("--dry-run".into());
        }
        if self.auto {
            a.push("--auto".into());
        }
        a
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
mod tests {
    use super::*;

    /// A made-up machine: the load grows with the cell count, and frames with it.
    fn model(cpu_per_mcell: f64, gpu_per_mcell: f64) -> impl Fn(u64) -> Measure {
        move |cells: u64| {
            let mc = cells as f64 / 1e6;
            Measure {
                cpu: cpu_per_mcell * mc,
                gpu: Some(gpu_per_mcell * mc),
                p95: 0.002 + 0.005 * mc,
                fps: 30.0,
                gens_wanted: Some(60.0),
                gens_done: 60.0,
                on_gpu: true,
                gpu_timing: GpuTiming::Timestamps,
                cells,
            }
        }
    }

    fn limits() -> SceneLimits {
        SceneLimits::of(&TuneLimits::default(), 30.0, 1)
    }

    /// Runs a scene's sweep on a made-up machine.
    fn sweep(
        canvas: [f64; 2],
        design: f32,
        m: impl Fn(u64) -> Measure,
        l: &SceneLimits,
    ) -> SceneSweep {
        let mut s = SceneSweep::new(
            lattice_ladder(canvas, design, SWEEP_MAX_CELLS),
            Size::px(design),
        );
        let mut order = vec![];
        while let Some(size) = s.next_size() {
            order.push(size.px);
            s.record(size, size.px, &m(lattice_cells(canvas, size.px)), l);
        }
        // Coarsest first.
        assert!(order.windows(2).all(|w| w[0] > w[1]), "{order:?}");
        s
    }

    #[test]
    fn the_ladder_is_in_physical_pixels_for_any_screen() {
        let px = |v: Vec<Size>| v.into_iter().map(|s| s.px).collect::<Vec<_>>();
        // A 2560 x 1440 screen at 1x: 1 px is 3.7 million cells, within the 4 million.
        assert_eq!(
            px(lattice_ladder([2560.0, 1440.0], 3.0, SWEEP_MAX_CELLS)),
            [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0]
        );
        // The same screen at 2x (5120 x 2880 physical): 1 and 2 px would be 14.7 and 3.7 million
        // cells; 2 px still fits.
        assert_eq!(
            px(lattice_ladder([5120.0, 2880.0], 6.0, SWEEP_MAX_CELLS))[..3],
            [2.0, 3.0, 4.0]
        );
        // A 3024 x 1964 Retina laptop: 1 px is 5.9 million; and its scene's own 2.5 px joins in.
        let mac = px(lattice_ladder([3024.0, 1964.0], 2.5, SWEEP_MAX_CELLS));
        assert_eq!(mac[..3], [2.0, 2.5, 3.0]);
        assert_eq!(
            *mac.last().unwrap(),
            32.0,
            "the coarsest keeps 60 cells across"
        );
        // A small screen stops early at the coarse end; a GPU with less room at the fine end.
        assert_eq!(
            px(lattice_ladder([1366.0, 768.0], 4.0, SWEEP_MAX_CELLS)).last(),
            Some(&12.0)
        );
        assert_eq!(
            px(lattice_ladder([2560.0, 1440.0], 3.0, 500_000))[0],
            3.0,
            "at most half a million cells: 3 px is the finest"
        );
        // A scene whose own size is beyond the cut keeps it.
        assert!(px(lattice_ladder([1280.0, 720.0], 40.0, SWEEP_MAX_CELLS)).contains(&40.0));
        assert_eq!(coarsest_px([1920.0, 1080.0]), 18.0);
        assert_eq!(coarsest_px([20000.0, 10000.0]), 64.0);
    }

    #[test]
    fn a_patch_s_ladder_is_its_levels() {
        let v = patch_ladder("penrose-rhomb", [2560.0, 1440.0], Some(5), SWEEP_MAX_CELLS);
        let levels: Vec<u32> = v.iter().filter_map(|s| s.level).collect();
        // 4,000, 35,000 and 280,000 tiles; 2.24 million would be under a pixel a tile at 1440.
        assert_eq!(levels, [6, 5, 4], "finest first");
        assert!(v.windows(2).all(|w| w[0].px <= w[1].px));
        // A 4K screen has room for level 7's 1.3-pixel tiles.
        let k4 = patch_ladder("penrose-rhomb", [3840.0, 2160.0], None, SWEEP_MAX_CELLS);
        assert_eq!(k4[0].level, Some(7));
        assert!(k4[0].px > 1.0 && k4[0].px < 2.0, "{}", k4[0].px);
        // The scene's own level is always there, however small.
        let own = patch_ladder("spectre", [1280.0, 720.0], Some(2), SWEEP_MAX_CELLS);
        assert!(own.iter().any(|s| s.level == Some(2)));
    }

    #[test]
    fn the_sweep_finds_finer_sizes_where_there_is_room() {
        // 0.02 of the machine per million cells: even 1 px (3.7 million cells, 7.4%) fits under
        // the 10% limit less 20%, so the recommended size is finer than the scene's own 3 px.
        let canvas = [2560.0, 1440.0];
        let s = sweep(canvas, 3.0, model(0.02, 0.03), &limits());
        let cards = s.cards();
        assert_eq!(cards.len(), 9);
        assert!(cards.iter().all(|r| r.fits));
        assert_eq!(recommend(&cards, false), Size::px(1.0));
        assert!(cards.iter().any(|r| r.own && r.size.px == 3.0));
        // A placed pattern keeps its own size.
        assert_eq!(recommend(&cards, true), Size::px(3.0));
        // The same, deterministic.
        assert_eq!(sweep(canvas, 3.0, model(0.02, 0.03), &limits()), s);
    }

    #[test]
    fn the_sweep_coarsens_where_there_is_not_and_stops_going_finer_when_far_over() {
        // 0.4 of the machine per million cells: under 0.2 million cells fit, so 6 px (102,400)
        // is the finest within the limits and 4 px (230,400) is over.
        let canvas = [2560.0, 1440.0];
        let s = sweep(canvas, 3.0, model(0.4, 0.1), &limits());
        assert_eq!(recommend(&s.cards(), false), Size::px(6.0));
        // 2 px (921,600 cells, 37%) is more than twice the 10% limit: 1 px is never shown.
        let shown: Vec<f32> = s.rungs.iter().map(|r| r.size.px).collect();
        assert!(!shown.contains(&1.0) && shown.contains(&2.0), "{shown:?}");
        assert!(s.stopped.as_deref().unwrap().contains("far over"));
        let over = s.cards().into_iter().find(|r| r.size.px == 2.0).unwrap();
        assert!(!over.fits && over.over.as_deref().unwrap().contains("processor"));
        // Nothing fits at all: the coarsest, with a note.
        let never = |cells: u64| Measure {
            p95: 0.040,
            cells,
            ..Measure::default()
        };
        let s = sweep([1920.0, 1080.0], 2.0, never, &limits());
        let t = tuned_scene("x", "square", 2.0, false, &s, GpuTiming::None, None, None);
        // The coarsest sensible is 18 px; the coarsest shown, 16.
        assert_eq!(t.recommended, Size::px(16.0));
        assert!(t.note.as_deref().unwrap().contains("no size was within"));
    }

    #[test]
    fn a_choice_is_kept_while_it_fits_and_replaced_when_it_does_not() {
        let canvas = [2560.0, 1440.0];
        let s = sweep(canvas, 3.0, model(0.2, 0.1), &limits());
        let first = tuned_scene("a", "square", 3.0, false, &s, GpuTiming::None, None, None);
        assert_eq!((first.size(), first.chosen), (Size::px(4.0), false));
        // Someone chooses 6 px, then the monitors change and it is measured again: kept.
        let mut mine = first.clone();
        mine.set_size(Size::px(6.0));
        mine.chosen = true;
        let again = tuned_scene(
            "a",
            "square",
            3.0,
            false,
            &s,
            GpuTiming::None,
            None,
            Some(&mine),
        );
        assert_eq!(
            (again.size(), again.chosen, again.note.clone()),
            (Size::px(6.0), true, None)
        );
        // A choice over the limits on the new screen gives way to the recommended size, with a note.
        mine.set_size(Size::px(2.0));
        let again = tuned_scene(
            "a",
            "square",
            3.0,
            false,
            &s,
            GpuTiming::None,
            None,
            Some(&mine),
        );
        assert_eq!((again.size(), again.chosen), (Size::px(4.0), false));
        assert!(
            again
                .note
                .as_deref()
                .unwrap()
                .contains("chosen before, 2 px, is over")
        );
        // Asked for, a pick wins: the finest shown, the scene's own, or the nearest cell size.
        let pick = |p| {
            tuned_scene(
                "a",
                "square",
                3.0,
                false,
                &s,
                GpuTiming::None,
                Some(p),
                Some(&mine),
            )
        };
        assert_eq!(pick(Pick::Finest).size(), Size::px(1.0));
        assert_eq!(pick(Pick::Own).size(), Size::px(3.0));
        assert_eq!(pick(Pick::Px(7.0)).size(), Size::px(6.0));
        assert_eq!(
            (
                pick(Pick::Recommended).size(),
                pick(Pick::Recommended).chosen
            ),
            (Size::px(4.0), false)
        );
        assert_eq!(Pick::parse("12px").unwrap(), Pick::Px(12.0));
        assert_eq!(Pick::parse("Recommended").unwrap(), Pick::Recommended);
        assert!(Pick::parse("0.5").is_err() && Pick::parse("bigger").is_err());
    }

    #[test]
    fn placed_patterns_are_told_from_soups() {
        use jetcarp_core::{Boundary, CaSpec, TilingSpec};
        let scene = |seed: SeedSpec| {
            let mut sc = Scene::new(TilingSpec::rect("square", 40, 30, Boundary::Wrap));
            sc.cas = vec![CaSpec::new("B3/S23").seed(seed)];
            sc
        };
        assert!(!is_placed(&scene(SeedSpec::Random { density: 0.3 })));
        assert!(!is_placed(&scene(SeedSpec::RandomStates)));
        assert!(is_placed(&scene(SeedSpec::Center)));
        assert!(is_placed(&scene(SeedSpec::Cells {
            cells: vec![[1, 1], [2, 1]]
        })));
        assert!(is_placed(&scene(SeedSpec::Ants { count: 1 })));
        assert!(!is_placed(&scene(SeedSpec::Ants { count: 5 })));
        // An empty field with a pasted pattern.
        let mut pasted = scene(SeedSpec::Empty);
        pasted.commands.push(Command::SetCells {
            id: 1,
            cells: vec![[5, 1]],
        });
        assert!(is_placed(&pasted));
        // Nothing placed at all, or a placed pattern kept going by reseeding: not.
        assert!(!is_placed(&scene(SeedSpec::Empty)));
        let mut reseeded = scene(SeedSpec::Center);
        let id = crate::reseed_presets::PRESETS[0].id;
        reseeded.reseed = crate::reseed_presets::preset_policies(id, 1200).unwrap();
        assert!(!reseeded.reseed.is_empty() && !is_placed(&reseeded));
    }

    #[test]
    fn cards_name_their_pictures_and_crops_keep_the_screen_s_pixels() {
        assert_eq!(
            crop_name(0, "profile Life", &Size::px(3.0)),
            "s0-profile-life-3px.png"
        );
        assert_eq!(
            crop_name(
                1,
                "Penrose waves",
                &Size {
                    px: 2.4,
                    level: Some(6)
                }
            ),
            "s1-penrose-waves-l6.png"
        );
        assert_eq!(crop_name(0, "x", &Size::px(2.5)), "s0-x-2_5px.png");
        // Even sides on an even canvas, odd on an odd one, so the picture's pixel grid is the
        // screen's.
        assert_eq!(crop_size([2560.0, 1440.0]), [192, 120]);
        assert_eq!(crop_size([1365.0, 767.0]), [191, 119]);
        assert_eq!(crop_size([100.0, 50.0]), [100, 50]);
        assert_eq!(warmup(2.0), 0.5);
        assert_eq!(warmup(0.5), 0.125);
        let r = TuneRequest {
            cpu_percent: Some(5.0),
            pick: Some(Pick::Own),
            dry_run: true,
            ..Default::default()
        };
        assert_eq!(r.args(), ["--cpu-limit", "5", "--pick", "own", "--dry-run"]);
        let e = TuneEvent::Rung {
            screen: 0,
            number: 2,
            of: 7,
            scene: "Life".into(),
            size: Size::px(3.0),
            rung: 1,
            rungs: 9,
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(line.contains(r#""event":"rung""#), "{line}");
        assert_eq!(serde_json::from_str::<TuneEvent>(&line).unwrap(), e);
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
