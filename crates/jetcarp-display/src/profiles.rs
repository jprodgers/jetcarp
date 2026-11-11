//! Screensaver profiles: a named, complete setup for the screensaver. What to show (a scene kept
//! in the profile, a scene file, or a Quick Start id), an optional palette, colouring and reseed
//! policies that replace the scene's own, the cell size and whether to rebuild the tiling to fill
//! the screen, the speed, the GPU simulation, and the minutes it gets in a playlist.
//!
//! User profiles are JSON files in `<config dir>/jetcarp/screensavers/` (`$JETCARP_CONFIG_DIR/
//! screensavers/`), one per profile, so they can be shared and edited by hand. The built-in ones
//! ([`builtins`]) are in code; editing one saves a user copy under the same name, which then
//! takes its place until it is deleted (*Revert to built-in*).
//!
//! Reseed drop counts are absolute, so a profile records the cell count its policies were set
//! for (`reseed_cells`); when the screensaver's world has another size, rain drops scale with
//! the cell count ([`scale_policies`]) as the Reseed pane's presets do. Blob radii are already
//! fractions of the grid.

use jetcarp_core::render::ColorBy;
use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};
use jetcarp_core::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The cell sizes a display offers, in physical pixels.
pub const CELL_PX_RANGE: std::ops::RangeInclusive<f32> = 1.0..=64.0;

/// Lattice dimensions that fill a canvas of `w` x `h` pixels at `cell_px` pixels per cell (the
/// square root of the average tile area), up to 32768 cells a side.
pub fn fill_size(spec: &TilingSpec, w: f64, h: f64, cell_px: f64) -> SizeSpec {
    let probe = TilingSpec {
        size: Some(SizeSpec::Cells(400)),
        boundary: Boundary::Dead,
        ..spec.clone()
    };
    let Ok(t) = Topology::build(&probe) else {
        return SizeSpec::Cells(900);
    };
    let Some(g) = t.lattice() else {
        return SizeSpec::Cells(900);
    };
    let area = (g.a1[0] * g.a2[1] - g.a1[1] * g.a2[0]).abs() / g.ntypes.max(1) as f64;
    let zoom = cell_px.max(1e-3) / area.sqrt();
    let cw = (w / (zoom * g.a1[0].abs().max(1e-9)))
        .floor()
        .clamp(8.0, 32768.0) as u32;
    let ch = (h / (zoom * g.a2[1].abs().max(1e-9)))
        .floor()
        .clamp(8.0, 32768.0) as u32;
    SizeSpec::Rect { w: cw, h: ch }
}

/// What a profile shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Show {
    /// A scene kept in the profile (a snapshot of a world's setup).
    Scene { scene: Box<Scene> },
    /// A scene file.
    File { path: String },
    /// A Quick Start id (`rule:B3/S23`, `find:...`, a built-in id).
    QuickStart { id: String },
}

impl Default for Show {
    fn default() -> Self {
        Show::QuickStart {
            id: "rule:B3/S23".into(),
        }
    }
}

impl Show {
    /// A few words on what it shows.
    pub fn describe(&self) -> String {
        match self {
            Show::Scene { scene } => {
                let rules: Vec<String> = scene.cas.iter().map(|c| c.rule.clone()).collect();
                let tiling = jetcarp_core::families::family(&scene.tiling.family)
                    .map(|f| f.name.to_string())
                    .unwrap_or_else(|| scene.tiling.family.clone());
                format!("{} on {tiling}", rules.join(" + "))
            }
            Show::File { path } => format!("scene file {path}"),
            Show::QuickStart { id } => format!("Quick Start {id}"),
        }
    }
}

/// A screensaver profile.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub show: Show,
    /// A palette for every CA (None: the scene's own).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    /// Colouring for every CA (None: the scene's own).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorBy>,
    /// Reseed policies replacing the scene's own (None: the scene's own; empty: no reseeding).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reseed: Option<Vec<AutoReseed>>,
    /// The cell count the reseed drop counts were set for (None: use them as they are).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reseed_cells: Option<u64>,
    /// Cell size in physical pixels.
    pub cell_px: f32,
    /// Rebuild periodic tilings to fill the screen at `cell_px`.
    pub refill: bool,
    /// Generations per second (0: as fast as possible).
    pub speed: f32,
    /// Step on the GPU where the rules allow it.
    pub gpu: bool,
    /// Minutes in a playlist (None: the playlist's).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes: Option<f32>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: String::new(),
            description: String::new(),
            show: Show::default(),
            palette: None,
            color_by: None,
            reseed: None,
            reseed_cells: None,
            cell_px: 3.0,
            refill: true,
            speed: 30.0,
            gpu: true,
            minutes: None,
        }
    }
}

impl Profile {
    pub fn from_json(text: &str) -> Result<Profile, String> {
        let p: Profile = serde_json::from_str(text).map_err(|e| format!("profile JSON: {e}"))?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Check the name, sizes, and policies.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a profile needs a name".into());
        }
        if !CELL_PX_RANGE.contains(&self.cell_px) {
            return Err(format!("cell_px is 1 to 64 (got {})", self.cell_px));
        }
        if !(self.speed >= 0.0 && self.speed <= 100_000.0) {
            return Err(format!("speed is 0 (max) to 100000 (got {})", self.speed));
        }
        if let Some(m) = self.minutes
            && !(0.05..=1440.0).contains(&m)
        {
            return Err(format!("minutes is 0.05 to 1440 (got {m})"));
        }
        if let Some(r) = &self.reseed {
            crate::reseed_presets::validate_all(r)?;
        }
        if let Show::Scene { scene } = &self.show {
            for p in &scene.reseed {
                p.validate()?;
            }
        }
        Ok(())
    }

    /// The minutes it gets in a playlist whose default is `default`.
    pub fn minutes_or(&self, default: f32) -> f32 {
        self.minutes.unwrap_or(default)
    }
}

// ------------------------------------------------------------------ applying a profile

/// The tiling is periodic (a lattice that can be rebuilt to fill a canvas).
pub fn is_lattice(sc: &Scene) -> bool {
    jetcarp_core::families::family(&sc.tiling.family)
        .is_some_and(|f| f.kind != jetcarp_core::families::FamilyKind::Aperiodic)
}

/// The scene a profile's source gives, before its overrides.
pub fn source_scene(p: &Profile, lookup: &dyn Fn(&str) -> Option<Scene>) -> Result<Scene, String> {
    match &p.show {
        Show::Scene { scene } => Ok((**scene).clone()),
        Show::File { path } => std::fs::read_to_string(path)
            .map_err(|e| format!("{path}: {e}"))
            .and_then(|j| Scene::from_json(&j).map_err(|e| format!("{path}: {e}"))),
        Show::QuickStart { id } => lookup(id).ok_or(format!("no Quick Start entry {id}")),
    }
}

/// What a profile will show: its scene, or Life (keeping the profile's size and speed) with a note
/// saying why when the scene cannot be read or its tiling is unknown.
pub fn resolve(
    p: &Profile,
    lookup: &dyn Fn(&str) -> Option<Scene>,
) -> (Profile, Scene, Option<String>) {
    let mut p = p.clone();
    let found = source_scene(&p, lookup).and_then(|sc| {
        if jetcarp_core::families::family(&sc.tiling.family).is_some() {
            Ok(sc)
        } else {
            Err(format!("unknown tiling family '{}'", sc.tiling.family))
        }
    });
    match found {
        Ok(sc) => (p, sc, None),
        Err(e) => {
            let life = builtins().into_iter().next().expect("Life");
            p.reseed = life.reseed.clone();
            p.reseed_cells = life.reseed_cells;
            let Show::Scene { scene } = life.show else {
                panic!("the Life profile keeps its scene");
            };
            (p, *scene, Some(format!("{e}; showing Life")))
        }
    }
}

/// The scene a profile shows, with its overrides applied to `sc` (the scene its source gave):
/// the palette and colouring on every CA (also those added and changed by the scene's
/// commands), the reseed policies (the scene's own `set_reseed` commands dropped), and with
/// `refill` a periodic tiling sized to fill `canvas` (pixels) at the cell size. The camera and
/// record are dropped: the screensaver zooms itself.
pub fn apply_profile(p: &Profile, mut sc: Scene, canvas: Option<[f64; 2]>) -> Scene {
    if let Some(pal) = &p.palette {
        for ca in &mut sc.cas {
            ca.palette = Some(pal.clone());
        }
    }
    if let Some(cb) = p.color_by {
        for ca in &mut sc.cas {
            ca.color_by = cb;
        }
    }
    for c in &mut sc.commands {
        match c {
            Command::AddCa { ca } => {
                if let Some(pal) = &p.palette {
                    ca.palette = Some(pal.clone());
                }
                if let Some(cb) = p.color_by {
                    ca.color_by = cb;
                }
            }
            Command::UpdateCa { change, .. } => {
                if p.palette.is_some() {
                    change.palette = None;
                }
                if p.color_by.is_some() {
                    change.color_by = None;
                }
            }
            _ => {}
        }
    }
    if let Some(r) = &p.reseed {
        sc.reseed = r.clone();
        sc.commands
            .retain(|c| !matches!(c, Command::SetReseed { .. }));
    }
    if p.refill
        && is_lattice(&sc)
        && let Some(c) = canvas
    {
        sc.tiling.size = Some(fill_size(&sc.tiling, c[0], c[1], p.cell_px as f64));
    }
    sc.view = None;
    sc.record = None;
    sc
}

/// Policies set for `from` cells, for a world of `to` cells: rain drop counts scale with the
/// cell count (at least one drop). Blob radii and flip rates are relative already.
pub fn scale_policies(policies: &[AutoReseed], from: u64, to: usize) -> Vec<AutoReseed> {
    let k = to as f64 / from.max(1) as f64;
    policies
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if let ReseedMode::Rain { drops, .. } = &mut p.mode {
                *drops = (*drops as f64 * k).round().clamp(1.0, 1_000_000.0) as u32;
            }
            p
        })
        .collect()
}

/// A clean scene of a world's setup, for a profile: the tiling, the CAs as they are now (rules,
/// seeds, palettes, colouring), the palettes and rule tables, the script, and the reseed
/// policies now running. The command log is dropped except the cells drawn or pasted before the
/// world first ran, so loading it starts fresh from the seeds instead of replaying every
/// generation.
pub fn capture_scene(world: &World, name: &str, description: &str) -> Scene {
    let base = world.to_scene();
    let mut sc = Scene::new(world.topo.spec.clone());
    sc.name = name.to_string();
    sc.description = description.to_string();
    sc.rng_state = base.rng_state;
    sc.tables = base.tables.clone();
    for c in &base.commands {
        if let Command::AddTable { table } = c
            && !sc.tables.iter().any(|t| t.name == table.name)
        {
            sc.tables.push(table.clone());
        }
    }
    sc.cas = world.cas.iter().map(|c| c.spec()).collect();
    sc.palettes = world.palettes.clone();
    sc.script = base.script.clone();
    sc.reseed = world.reseed_policies().to_vec();
    let ids: Vec<u32> = world.cas.iter().map(|c| c.id).collect();
    for c in &base.commands {
        let keep = match c {
            Command::Step { .. } | Command::Jump { .. } | Command::Reseed { .. } => break,
            Command::Seed { id, .. }
            | Command::Clear { id }
            | Command::SetCells { id, .. }
            | Command::PastePattern { id, .. } => ids.contains(id),
            _ => false,
        };
        if keep {
            sc.commands.push(c.clone());
        }
    }
    sc
}

// ------------------------------------------------------------------ liveliness

/// Activity below this share of cells changed per generation counts as quiet.
pub const QUIET: f64 = 0.0005;
/// Above this the screen is noise.
pub const NOISY: f64 = 0.30;

/// How lively a run was: per-generation activity and population, summed up.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    pub gens: u64,
    act_sum: f64,
    pub quiet: u64,
    pub noisy: u64,
    run: u64,
    pub longest_quiet: u64,
    pub pop_min: f64,
    pub pop_max: f64,
    pop_sum: f64,
    /// Mean activity per chunk of `chunk` generations.
    pub chunks: Vec<f64>,
    chunk: u64,
    chunk_sum: f64,
    chunk_n: u64,
}

impl Meter {
    pub fn new(chunk: u64) -> Meter {
        Meter {
            pop_min: f64::INFINITY,
            chunk: chunk.max(1),
            ..Default::default()
        }
    }

    /// One generation: the share of cells that changed and the share alive.
    pub fn sample(&mut self, activity: f64, pop: f64) {
        self.gens += 1;
        self.act_sum += activity;
        self.pop_sum += pop;
        self.pop_min = self.pop_min.min(pop);
        self.pop_max = self.pop_max.max(pop);
        if activity < QUIET {
            self.quiet += 1;
            self.run += 1;
            self.longest_quiet = self.longest_quiet.max(self.run);
        } else {
            self.run = 0;
        }
        if activity > NOISY {
            self.noisy += 1;
        }
        self.chunk_sum += activity;
        self.chunk_n += 1;
        if self.chunk_n == self.chunk {
            self.chunks.push(self.chunk_sum / self.chunk_n as f64);
            self.chunk_sum = 0.0;
            self.chunk_n = 0;
        }
    }

    pub fn mean_activity(&self) -> f64 {
        self.act_sum / self.gens.max(1) as f64
    }

    pub fn mean_pop(&self) -> f64 {
        self.pop_sum / self.gens.max(1) as f64
    }

    /// 0 (dead or noise) to 1 (lively throughout): penalises quiet and noisy generations, a
    /// long quiet stretch, and chunks whose mean activity leaves the 0.1% to 20% band.
    pub fn score(&self) -> f64 {
        if self.gens == 0 {
            return 0.0;
        }
        let g = self.gens as f64;
        let quiet = self.quiet as f64 / g;
        let noisy = self.noisy as f64 / g;
        let stall = (self.longest_quiet as f64 / 300.0).min(1.0);
        let off_band = if self.chunks.is_empty() {
            0.0
        } else {
            self.chunks
                .iter()
                .filter(|a| !(0.001..=0.20).contains(*a))
                .count() as f64
                / self.chunks.len() as f64
        };
        (1.0 - 1.5 * quiet - noisy - 0.5 * stall - 0.5 * off_band).clamp(0.0, 1.0)
    }

    /// One line: mean activity, the chunk range, population range, quiet stretch.
    pub fn summary(&self) -> String {
        let pct = crate::reseed_presets::pct;
        let (lo, hi) = self
            .chunks
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(a, b), &x| (a.min(x), b.max(x)));
        format!(
            "activity {} (chunks {}..{}), population {}..{} (mean {}), quiet {} gens (longest {}), noisy {}, score {:.2}",
            pct(self.mean_activity()),
            pct(if lo.is_finite() { lo } else { 0.0 }),
            pct(hi),
            pct(if self.pop_min.is_finite() {
                self.pop_min
            } else {
                0.0
            }),
            pct(self.pop_max),
            pct(self.mean_pop()),
            self.quiet,
            self.longest_quiet,
            self.noisy,
            self.score()
        )
    }
}

/// Step `w` for `gens` generations, measuring activity (change tracking on) and the share of
/// cells alive in the first field. Returns the meter and how often the policies fired.
pub fn measure(w: &mut World, gens: u64, chunk: u64) -> Result<(Meter, u64), String> {
    let cells = w.topo.len().max(1) as f64;
    w.set_track_changes(true);
    let mut m = Meter::new(chunk);
    for _ in 0..gens {
        w.apply(Command::Step { gens: 1 })?;
        let pop = w.fields.first().map_or(0, |f| f.population()) as f64 / cells;
        m.sample(w.last_activity().unwrap_or(0.0), pop);
    }
    let fired = w.reseed_status().iter().map(|s| s.fired).sum();
    Ok((m, fired))
}

// ------------------------------------------------------------------ tuning

/// Cells in the small copy of a world that *Tune for this rule* runs.
pub const TUNE_CELLS: f64 = 60_000.0;
/// Generations each candidate runs.
pub const TUNE_GENS: u64 = 3_000;

/// One candidate's run.
#[derive(Clone, Debug)]
pub struct TuneResult {
    pub name: String,
    pub policies: Vec<AutoReseed>,
    /// The cell count the policies were set for (the test world's).
    pub cells: u64,
    pub meter: Meter,
    pub fired: u64,
    /// Higher is better: the meter's score, nudged towards about 3% activity.
    pub rank: f64,
}

/// The policy sets *Tune for this rule* tries on a world of `cells` cells: none, the scene's
/// own, the Reseed pane's presets, and the mixes the built-in profiles use.
pub fn tune_candidates(own: &[AutoReseed], cells: usize) -> Vec<(String, Vec<AutoReseed>)> {
    let k = cells as f64 / screen_cells(3.0) as f64;
    let d = |n: f64| (n * k).round().max(1.0) as u32;
    let mut v = vec![("No reseeding".to_string(), vec![])];
    if !own.is_empty() {
        v.push(("The scene's own".to_string(), own.to_vec()));
    }
    for p in crate::reseed_presets::PRESETS {
        if let Some(pol) = crate::reseed_presets::preset_policies(p.id, cells) {
            v.push((format!("Preset: {}", p.name), pol));
        }
    }
    let new = AutoReseed::new;
    v.push((
        "Rain every 100, blobs by 1,000".into(),
        vec![
            new(
                blobs(2, 0.06, 0.16, 0.35),
                settled(0.002, Some(12), Some(1000)),
            )
            .cooldown(150),
            new(rain(d(80.0), 2.0, 8.0, 0.45), every(100)),
        ],
    ));
    v.push((
        "Light rain every 400, blobs by 2,400".into(),
        vec![
            new(
                blobs(2, 0.06, 0.16, 0.35),
                settled(0.002, Some(12), Some(2400)),
            )
            .cooldown(150),
            new(rain(d(44.0), 2.0, 8.0, 0.45), every(400)),
        ],
    ));
    v.push((
        "Blobs when settled only".into(),
        vec![new(blobs(2, 0.06, 0.16, 0.35), settled(0.002, Some(12), None)).cooldown(200)],
    ));
    v
}

/// A small copy of a profile's scene for tuning: about [`TUNE_CELLS`] cells (periodic tilings
/// rebuilt at that size, aperiodic patches at level 5 at most), the profile's palette and
/// colouring, and the scene's own policies.
pub fn tune_scene(p: &Profile, sc: Scene) -> Scene {
    let px = p.cell_px.max(1.0) as f64;
    let w = (TUNE_CELLS * 16.0 / 9.0).sqrt() * px;
    let q = Profile {
        reseed: None,
        refill: true,
        ..p.clone()
    };
    let mut sc = apply_profile(&q, sc, Some([w, w * 9.0 / 16.0]));
    if let Some(SizeSpec::Level(l)) = sc.tiling.size
        && l > 5
    {
        sc.tiling.size = Some(SizeSpec::Level(5));
    }
    sc
}

/// Run every candidate on `sc` (seeded by the scene, so the same scene gives the same ranks)
/// for `gens` generations; best first. `progress(done, of)` after each.
pub fn run_tune(
    sc: &Scene,
    gens: u64,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<TuneResult>, String> {
    let probe = World::from_scene(sc)?;
    let cells = probe.topo.len();
    let own = probe.reseed_policies().to_vec();
    drop(probe);
    let cands = tune_candidates(&own, cells);
    let n = cands.len();
    let mut out = vec![];
    for (k, (name, policies)) in cands.into_iter().enumerate() {
        let mut s = sc.clone();
        s.reseed = policies.clone();
        s.commands
            .retain(|c| !matches!(c, Command::SetReseed { .. }));
        let mut w = World::from_scene(&s)?;
        let (meter, fired) = measure(&mut w, gens, (gens / 6).max(1))?;
        let act = meter.mean_activity().max(1e-6);
        let rank = meter.score() - 0.1 * (act / 0.03).log10().abs().min(1.0);
        out.push(TuneResult {
            name,
            policies,
            cells: cells as u64,
            meter,
            fired,
            rank,
        });
        progress(k + 1, n);
    }
    out.sort_by(|a, b| b.rank.total_cmp(&a.rank));
    Ok(out)
}

// ------------------------------------------------------------------ built-in profiles

/// Cells of a 2560 x 1440 screen at `px` pixels per cell: what the built-ins' drop counts are
/// set for.
fn screen_cells(px: f32) -> u64 {
    (2560.0 * 1440.0 / (px as f64 * px as f64)).round() as u64
}

fn soup(
    name: &str,
    desc: &str,
    tiling: TilingSpec,
    rule: &str,
    seed: SeedSpec,
    palette: &str,
    rng: u64,
) -> Scene {
    let mut sc = Scene::new(tiling);
    sc.name = name.to_string();
    sc.description = desc.to_string();
    sc.rng_state = rng;
    let mut ca = CaSpec::new(rule);
    ca.seed = Some(seed);
    ca.palette = Some(palette.to_string());
    sc.cas = vec![ca];
    sc
}

fn random(density: f64) -> SeedSpec {
    SeedSpec::Random { density }
}

fn wrap(family: &str) -> TilingSpec {
    TilingSpec::rect(family, 160, 90, Boundary::Wrap)
}

fn rain(drops: u32, min_size: f64, max_size: f64, density: f64) -> ReseedMode {
    ReseedMode::Rain {
        drops,
        min_size,
        max_size,
        density,
        sign: Default::default(),
    }
}

fn blobs(count: u32, min_radius: f64, max_radius: f64, live: f64) -> ReseedMode {
    ReseedMode::Blobs {
        count,
        min_radius,
        max_radius,
        live,
        sign: Default::default(),
    }
}

/// Settled: activity below `activity` (or a repeating change count up to `cycle`) for 60
/// generations, or at the latest after `latest` generations.
fn settled(activity: f64, cycle: Option<u64>, latest: Option<u64>) -> ReseedWhen {
    ReseedWhen::Settled {
        activity,
        for_gens: 60,
        cycle,
        period: None,
        latest,
    }
}

fn every(gens: u64) -> ReseedWhen {
    ReseedWhen::Every { gens }
}

/// One built-in profile.
fn builtin(scene: Scene, desc: &str, cell_px: f32, speed: f32, reseed: Vec<AutoReseed>) -> Profile {
    let refill = is_lattice(&scene);
    Profile {
        name: scene.name.clone(),
        description: desc.to_string(),
        show: Show::Scene {
            scene: Box::new(scene),
        },
        palette: None,
        color_by: None,
        reseed: Some(reseed),
        reseed_cells: Some(screen_cells(cell_px)),
        cell_px,
        refill,
        speed,
        gpu: true,
        minutes: None,
    }
}

/// The built-in profiles, tuned at 2560 x 1440 (the `profile_survey` test measures them).
pub fn builtins() -> Vec<Profile> {
    let life = AutoReseed::new;
    vec![
        builtin(
            soup(
                "Life",
                "Conway's Game of Life from a random soup.",
                wrap("square"),
                "B3/S23",
                random(0.35),
                "Ember",
                11,
            ),
            "Rain every 100 generations keeps the ash stirring; blobs when it settles, or every 1,000 generations at the latest.",
            3.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.06, 0.16, 0.35),
                    settled(0.002, Some(12), Some(1000)),
                )
                .cooldown(150),
                life(rain(80, 2.0, 8.0, 0.45), every(100)),
            ],
        ),
        builtin(
            soup(
                "HighLife",
                "Life with birth on 6 too.",
                wrap("square"),
                "B36/S23",
                random(0.35),
                "Aurora",
                12,
            ),
            "Its ash is quieter than Life's: heavier rain every 100 generations, blobs every 800 at the latest.",
            3.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.06, 0.16, 0.35),
                    settled(0.002, Some(12), Some(800)),
                )
                .cooldown(150),
                life(rain(120, 2.0, 8.0, 0.45), every(100)),
            ],
        ),
        builtin(
            soup(
                "Day & Night",
                "B3678/S34678: live and dead behave alike, so it grows and dissolves islands.",
                wrap("square"),
                "B3678/S34678",
                random(0.5),
                "Synthwave",
                13,
            ),
            "Big half-and-half blobs when the islands freeze.",
            3.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.08, 0.2, 0.5),
                    settled(0.002, Some(12), Some(3000)),
                )
                .cooldown(200),
            ],
        ),
        builtin(
            soup(
                "Hexagonal Life",
                "B2/S34 on hexagons.",
                wrap("hex"),
                "B2/S34H",
                random(0.35),
                "Koi",
                14,
            ),
            "Its gliders and blinkers keep it busy: a light rain every 400 generations, blobs every 2,400 at the latest.",
            4.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.06, 0.16, 0.35),
                    settled(0.002, Some(12), Some(2400)),
                )
                .cooldown(150),
                life(rain(44, 2.0, 8.0, 0.45), every(400)),
            ],
        ),
        builtin(
            soup(
                "Brian's Brain",
                "Firing, refractory, ready: a rule full of gliders that never settles on a torus.",
                wrap("square"),
                "B2/S/C3",
                random(0.2),
                "Prism",
                15,
            ),
            "Runs by itself; blobs only if it ever empties.",
            3.0,
            30.0,
            vec![life(blobs(3, 0.05, 0.12, 0.2), settled(0.0005, None, None)).cooldown(200)],
        ),
        builtin(
            soup(
                "Star Wars",
                "B2/S345/C4: starships and crystal walls.",
                wrap("square"),
                "B2/S345/C4",
                random(0.35),
                "Neon",
                16,
            ),
            "Blobs when settled.",
            3.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.06, 0.16, 0.35),
                    settled(0.002, Some(12), Some(3000)),
                )
                .cooldown(200),
            ],
        ),
        builtin(
            soup(
                "Triangles",
                "Life on the 12-neighbour triangle grid.",
                wrap("triangle"),
                "B3/S23",
                random(0.37),
                "Lava",
                17,
            ),
            "Rain every 150 generations, blobs when settled or every 1,500 at the latest.",
            4.0,
            30.0,
            vec![
                life(
                    blobs(2, 0.06, 0.16, 0.37),
                    settled(0.002, Some(12), Some(1500)),
                )
                .cooldown(150),
                life(rain(44, 2.0, 8.0, 0.45), every(150)),
            ],
        ),
        builtin(
            soup(
                "Snub square Life",
                "B3/S23 on the 3.3.4.3.4 snub square tiling (Archimedean: triangles and squares, nine neighbours each). Never settles into ash.",
                wrap("snubsq"),
                "B3/S23",
                random(0.35),
                "Glacier",
                18,
            ),
            "Runs by itself; blobs only if it settles.",
            4.0,
            30.0,
            vec![life(blobs(2, 0.06, 0.16, 0.35), settled(0.002, Some(12), None)).cooldown(200)],
        ),
        builtin(
            soup(
                "Penrose waves",
                "B2/S/C4 on a round patch of Penrose rhombs: arcs of fire sweep across the aperiodic tiling (fitted to the screen, not rebuilt).",
                TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(6)),
                "B2/S/C4",
                random(0.3),
                "Deep Sea",
                19,
            ),
            "Runs by itself; rain only if it calms down.",
            2.4,
            30.0,
            vec![life(rain(40, 3.0, 10.0, 0.3), settled(0.001, None, None)).cooldown(100)],
        ),
        builtin(
            soup(
                "Cyclic spirals",
                "Cyclic CA, 16 colours: each cell advances when a neighbour holds the next colour. Grows spirals that never stop.",
                wrap("square"),
                "Cyclic 16/1",
                SeedSpec::RandomStates,
                "Prism",
                20,
            ),
            "No reseeding: the spirals keep turning.",
            3.0,
            30.0,
            vec![],
        ),
        builtin(
            soup(
                "Hodgepodge",
                "Gerhardt-Schuster hodgepodge machine: spiral waves of infection.",
                wrap("square"),
                "Hodgepodge 200/3/3/28",
                SeedSpec::RandomStates,
                "Tide",
                21,
            ),
            "No reseeding: the waves keep coming.",
            3.0,
            30.0,
            vec![],
        ),
    ]
}

// ------------------------------------------------------------------ the store

/// A file name for a profile name: lower case letters, digits, and dashes.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "profile".into() } else { s }
}

/// A profile as listed.
#[derive(Clone, Debug)]
pub struct Listed {
    pub profile: Profile,
    /// A built-in of this name exists.
    pub builtin: bool,
    /// Its file (user profiles, and built-ins with a user copy).
    pub file: Option<PathBuf>,
}

impl Listed {
    pub fn tag(&self) -> &'static str {
        match (self.builtin, self.file.is_some()) {
            (true, false) => "built-in",
            (true, true) => "built-in, edited",
            _ => "yours",
        }
    }
}

/// User profiles on disk and the built-ins.
#[derive(Default)]
pub struct ProfileStore {
    dir: Option<PathBuf>,
    loaded: bool,
    user: Vec<(PathBuf, Profile)>,
    /// Files that could not be read.
    pub errors: Vec<String>,
    builtins: Vec<Profile>,
}

impl ProfileStore {
    /// A store on `dir` (None: built-ins only, nothing saved).
    pub fn at(dir: Option<PathBuf>) -> ProfileStore {
        let mut s = ProfileStore {
            dir,
            builtins: builtins(),
            ..Default::default()
        };
        s.reload();
        s
    }

    /// The profiles folder next to the settings file.
    pub fn dir_for_settings(settings_file: Option<&Path>) -> Option<PathBuf> {
        settings_file
            .and_then(|p| p.parent())
            .map(|d| d.join("screensavers"))
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Read the folder again.
    pub fn reload(&mut self) {
        if self.builtins.is_empty() {
            self.builtins = builtins();
        }
        self.loaded = true;
        self.user.clear();
        self.errors.clear();
        let Some(dir) = &self.dir else { return };
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f)
                .map_err(|e| e.to_string())
                .and_then(|t| Profile::from_json(&t))
            {
                Ok(p) if self.user.iter().any(|(_, q)| q.name == p.name) => self.errors.push(
                    format!("{}: a second profile named {:?}", f.display(), p.name),
                ),
                Ok(p) => self.user.push((f, p)),
                Err(e) => self.errors.push(format!("{}: {e}", f.display())),
            }
        }
    }

    /// Every profile: the built-ins in order (a user copy in place of its built-in), then the
    /// user's own by name.
    pub fn list(&self) -> Vec<Listed> {
        let mut v: Vec<Listed> = self
            .builtins
            .iter()
            .map(|b| match self.user.iter().find(|(_, p)| p.name == b.name) {
                Some((f, p)) => Listed {
                    profile: p.clone(),
                    builtin: true,
                    file: Some(f.clone()),
                },
                None => Listed {
                    profile: b.clone(),
                    builtin: true,
                    file: None,
                },
            })
            .collect();
        let mut own: Vec<Listed> = self
            .user
            .iter()
            .filter(|(_, p)| !self.builtins.iter().any(|b| b.name == p.name))
            .map(|(f, p)| Listed {
                profile: p.clone(),
                builtin: false,
                file: Some(f.clone()),
            })
            .collect();
        own.sort_by_key(|l| l.profile.name.to_lowercase());
        v.extend(own);
        v
    }

    pub fn names(&self) -> Vec<String> {
        self.list().into_iter().map(|l| l.profile.name).collect()
    }

    /// The profile of this name (a user copy before the built-in). Case-insensitive if no exact
    /// match.
    pub fn get(&self, name: &str) -> Option<Profile> {
        let list = self.list();
        list.iter()
            .find(|l| l.profile.name == name)
            .or_else(|| {
                list.iter()
                    .find(|l| l.profile.name.eq_ignore_ascii_case(name.trim()))
            })
            .map(|l| l.profile.clone())
    }

    pub fn listed(&self, name: &str) -> Option<Listed> {
        self.list().into_iter().find(|l| l.profile.name == name)
    }

    pub fn builtin(&self, name: &str) -> Option<&Profile> {
        self.builtins.iter().find(|b| b.name == name)
    }

    /// Save a profile to its file (a new file for a new name). Returns the path.
    pub fn save(&mut self, p: &Profile) -> Result<PathBuf, String> {
        p.validate()?;
        let dir = self.dir.clone().ok_or(
            "no settings folder for this run (set JETCARP_CONFIG_DIR), so profiles cannot be saved",
        )?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = match self.user.iter().find(|(_, q)| q.name == p.name) {
            Some((f, _)) => f.clone(),
            None => {
                let base = slug(&p.name);
                let mut k = 1;
                loop {
                    let f = if k == 1 {
                        dir.join(format!("{base}.json"))
                    } else {
                        dir.join(format!("{base}-{k}.json"))
                    };
                    if !f.exists() {
                        break f;
                    }
                    k += 1;
                }
            }
        };
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, p.to_json())
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match self.user.iter_mut().find(|(_, q)| q.name == p.name) {
            Some(slot) => slot.1 = p.clone(),
            None => self.user.push((path.clone(), p.clone())),
        }
        Ok(path)
    }

    /// Delete a user profile's file (a built-in's user copy: back to the built-in).
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let Some(k) = self.user.iter().position(|(_, p)| p.name == name) else {
            return Err(if self.builtin(name).is_some() {
                format!("{name:?} is built in and has no changes to revert")
            } else {
                format!("no profile named {name:?}")
            });
        };
        let (f, _) = self.user.remove(k);
        std::fs::remove_file(&f).map_err(|e| format!("{}: {e}", f.display()))
    }

    /// Rename a profile: a user profile's file is replaced; a built-in is copied.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        if new.is_empty() {
            return Err("give a name".into());
        }
        if new != old && self.get(new).is_some_and(|p| p.name == new) {
            return Err(format!("a profile named {new:?} exists"));
        }
        let mut p = self.get(old).ok_or(format!("no profile named {old:?}"))?;
        p.name = new.to_string();
        let was_user = self.user.iter().any(|(_, q)| q.name == old);
        let is_builtin = self.builtin(old).is_some();
        self.save(&p)?;
        if was_user && !is_builtin {
            self.delete(old)?;
        }
        Ok(())
    }

    /// A free name like `base`, `base 2`, `base 3`.
    pub fn free_name(&self, base: &str) -> String {
        let names = self.names();
        if !names.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !names.contains(n))
            .expect("a free name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jetcarp-profiles-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn builtins_are_valid_and_build() {
        let b = builtins();
        assert!(b.len() >= 8, "{} built-ins", b.len());
        let mut names = std::collections::HashSet::new();
        for p in &b {
            p.validate().unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert!(names.insert(p.name.clone()), "{} twice", p.name);
            let Show::Scene { scene } = &p.show else {
                panic!("{}: built-ins keep their scene", p.name)
            };
            // Built at a small canvas so the test stays quick.
            let mut sc = apply_profile(p, (**scene).clone(), Some([320.0, 180.0]));
            if !is_lattice(&sc) {
                sc.tiling.size = Some(SizeSpec::Level(3));
            }
            let w = World::from_scene(&sc).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert!(!w.topo.is_empty());
            assert_eq!(w.reseed_policies(), p.reseed.as_deref().unwrap());
            // Round trip through JSON.
            let back = Profile::from_json(&p.to_json()).unwrap();
            assert_eq!(&back, p);
        }
        assert!(b.iter().any(|p| !p.refill), "an aperiodic one");
        assert!(
            b.iter()
                .any(|p| p.reseed.as_ref().is_some_and(|r| r.is_empty())),
            "one without reseeding"
        );
    }

    #[test]
    fn json_round_trip_and_defaults() {
        let p = Profile {
            name: "Mine".into(),
            show: Show::File {
                path: "/x/a.json".into(),
            },
            palette: Some("Moss".into()),
            color_by: Some(ColorBy::State),
            reseed: Some(vec![]),
            minutes: Some(2.5),
            ..Default::default()
        };
        let j = p.to_json();
        assert_eq!(Profile::from_json(&j).unwrap(), p);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["show"]["kind"], "file");
        assert_eq!(
            v["reseed"],
            serde_json::json!([]),
            "empty means no reseeding"
        );
        // A hand-written file with only a name and a Quick Start id.
        let p = Profile::from_json(
            r#"{"name": "q", "show": {"kind": "quick_start", "id": "rule:B36/S23"}}"#,
        )
        .unwrap();
        assert_eq!(p.reseed, None, "missing: the scene's own policies");
        assert_eq!(p.cell_px, 3.0);
        assert!(Profile::from_json(r#"{"name": ""}"#).is_err());
        assert!(Profile::from_json(r#"{"name": "a", "cell_px": 0}"#).is_err());
        assert!(
            Profile::from_json(
                r#"{"name": "a", "reseed": [{"mode": {"kind": "random", "flip": 3}}, "when": "settled"]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn applying_sets_palette_policies_and_size() {
        let mut sc = Scene::new(TilingSpec::rect("square", 50, 40, Boundary::Wrap));
        let mut ca = CaSpec::new("B3/S23");
        ca.palette = Some("Ember".into());
        sc.cas = vec![ca];
        sc.reseed = vec![AutoReseed::new(ReseedMode::rain(), every(10))];
        sc.commands = vec![
            Command::UpdateCa {
                id: 1,
                change: jetcarp_core::world::CaChange {
                    palette: Some("Tide".into()),
                    ..Default::default()
                },
            },
            Command::SetReseed { policies: vec![] },
        ];
        sc.view = Some(serde_json::json!({"zoom": 3}));
        let policy = AutoReseed::new(ReseedMode::blobs(), ReseedWhen::settled());
        let p = Profile {
            name: "t".into(),
            palette: Some("Moss".into()),
            color_by: Some(ColorBy::Age),
            reseed: Some(vec![policy.clone()]),
            cell_px: 4.0,
            refill: true,
            ..Default::default()
        };
        let out = apply_profile(&p, sc.clone(), Some([800.0, 400.0]));
        assert_eq!(out.cas[0].palette.as_deref(), Some("Moss"));
        assert_eq!(out.cas[0].color_by, ColorBy::Age);
        assert_eq!(out.reseed, vec![policy.clone()]);
        assert_eq!(out.tiling.size, Some(SizeSpec::Rect { w: 200, h: 100 }));
        assert!(out.view.is_none());
        let w = World::from_scene(&out).unwrap();
        assert_eq!(
            w.ca(1).unwrap().palette,
            "Moss",
            "the log's palette change is dropped"
        );
        assert_eq!(w.reseed_policies(), &[policy]);
        assert_eq!(w.topo.len(), 200 * 100);
        // No overrides and no refill: the scene's own.
        let plain = Profile {
            name: "t".into(),
            refill: false,
            ..Default::default()
        };
        let out = apply_profile(&plain, sc.clone(), Some([800.0, 400.0]));
        assert_eq!(out.tiling, sc.tiling);
        assert_eq!(out.reseed, sc.reseed);
        assert_eq!(out.commands, sc.commands);
        // An empty override: no reseeding.
        let none = Profile {
            name: "t".into(),
            reseed: Some(vec![]),
            refill: false,
            ..Default::default()
        };
        let w = World::from_scene(&apply_profile(&none, sc, None)).unwrap();
        assert!(w.reseed_policies().is_empty());
    }

    #[test]
    fn aperiodic_scenes_are_not_resized() {
        let mut sc = Scene::new(TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(3)));
        sc.cas = vec![CaSpec::new("B2/S/C3")];
        let p = Profile {
            name: "t".into(),
            ..Default::default()
        };
        let out = apply_profile(&p, sc, Some([2560.0, 1440.0]));
        assert_eq!(out.tiling.size, Some(SizeSpec::Level(3)));
    }

    #[test]
    fn rain_scales_with_the_cell_count() {
        let p = vec![
            AutoReseed::new(rain(10, 2.0, 4.0, 0.5), every(5)),
            AutoReseed::new(blobs(2, 0.1, 0.2, 0.3), ReseedWhen::settled()),
        ];
        let s = scale_policies(&p, 1000, 4000);
        assert!(matches!(s[0].mode, ReseedMode::Rain { drops: 40, .. }));
        assert_eq!(s[1], p[1]);
        let s = scale_policies(&p, 1000, 1);
        assert!(matches!(s[0].mode, ReseedMode::Rain { drops: 1, .. }));
    }

    #[test]
    fn capture_keeps_the_setup_not_the_history() {
        let mut w = World::new(TilingSpec::rect("square", 40, 30, Boundary::Wrap), 5).unwrap();
        let mut ca = CaSpec::new("B3/S23");
        ca.seed = Some(SeedSpec::Random { density: 0.3 });
        w.apply(Command::AddCa { ca }).unwrap();
        w.apply(Command::SetCells {
            id: 1,
            cells: vec![[3, 1]],
        })
        .unwrap();
        w.apply(Command::Step { gens: 50 }).unwrap();
        w.apply(Command::UpdateCa {
            id: 1,
            change: jetcarp_core::world::CaChange {
                palette: Some("Moss".into()),
                ..Default::default()
            },
        })
        .unwrap();
        let pol = vec![AutoReseed::new(ReseedMode::rain(), every(20))];
        w.apply(Command::SetReseed {
            policies: pol.clone(),
        })
        .unwrap();
        let sc = capture_scene(&w, "n", "d");
        assert_eq!(sc.name, "n");
        assert_eq!(sc.reseed, pol);
        assert_eq!(sc.cas.len(), 1);
        assert_eq!(sc.cas[0].palette.as_deref(), Some("Moss"));
        assert!(
            sc.commands
                .iter()
                .all(|c| matches!(c, Command::SetCells { .. })),
            "{:?}",
            sc.commands
        );
        let back = World::from_scene(&sc).unwrap();
        assert_eq!(back.generation, 0);
        assert_eq!(back.reseed_policies(), pol.as_slice());
        assert_eq!(back.ca(1).unwrap().palette, "Moss");
    }

    #[test]
    fn store_saves_loads_renames_and_reverts() {
        let dir = scratch("store");
        let mut st = ProfileStore::at(Some(dir.clone()));
        let n = st.list().len();
        assert_eq!(n, builtins().len());
        assert!(st.list().iter().all(|l| l.builtin && l.file.is_none()));
        // A new profile.
        let p = Profile {
            name: "My Life!".into(),
            ..Default::default()
        };
        let f = st.save(&p).unwrap();
        assert_eq!(f, dir.join("my-life.json"));
        assert_eq!(
            ProfileStore::at(Some(dir.clone())).get("My Life!"),
            Some(p.clone())
        );
        assert_eq!(st.get("my life!").unwrap().name, "My Life!");
        // Editing a built-in saves a user copy that takes its place.
        let mut life = st.get("Life").unwrap();
        life.speed = 60.0;
        st.save(&life).unwrap();
        let mut st2 = ProfileStore::at(Some(dir.clone()));
        let l = st2.listed("Life").unwrap();
        assert_eq!(l.tag(), "built-in, edited");
        assert_eq!(l.profile.speed, 60.0);
        assert_eq!(st2.list().len(), n + 1);
        st2.delete("Life").unwrap();
        assert_eq!(st2.listed("Life").unwrap().tag(), "built-in");
        assert!(st2.delete("Life").is_err(), "nothing to revert");
        // Renaming a user profile moves its file; renaming a built-in copies it.
        st2.rename("My Life!", "Other").unwrap();
        assert!(st2.get("My Life!").is_none());
        assert!(!dir.join("my-life.json").exists());
        assert!(dir.join("other.json").exists());
        st2.rename("Hodgepodge", "Hodge 2").unwrap();
        assert!(st2.get("Hodgepodge").is_some() && st2.get("Hodge 2").is_some());
        assert!(st2.rename("Other", "Life").is_err(), "taken");
        assert_eq!(st2.free_name("Other"), "Other 2");
        // A broken file is reported, not fatal.
        std::fs::write(dir.join("broken.json"), "{ nope").unwrap();
        let st3 = ProfileStore::at(Some(dir.clone()));
        assert_eq!(st3.errors.len(), 1);
        assert!(st3.get("Other").is_some());
        // Without a folder nothing is saved.
        let mut none = ProfileStore::at(None);
        assert!(none.save(&p).is_err());
        assert_eq!(slug("  Day & Night "), "day-night");
        assert_eq!(slug("!!"), "profile");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tuning_ranks_every_candidate_the_same_each_time() {
        let life = builtins().remove(0);
        let Show::Scene { scene } = &life.show else {
            unreachable!()
        };
        let mut sc = tune_scene(&life, (**scene).clone());
        assert!(sc.tiling.size.is_some());
        // Smaller still, so the test is quick.
        sc.tiling.size = Some(SizeSpec::Rect { w: 80, h: 50 });
        let mut seen = 0;
        let a = run_tune(&sc, 300, |k, n| {
            seen = k;
            assert!(k <= n);
        })
        .unwrap();
        let cands = tune_candidates(&[], 4000).len();
        assert_eq!(a.len(), cands);
        assert_eq!(seen, cands);
        assert!(a.windows(2).all(|w| w[0].rank >= w[1].rank));
        assert!(a.iter().all(|r| r.cells == 4000));
        let b = run_tune(&sc, 300, |_, _| {}).unwrap();
        let names = |v: &[TuneResult]| v.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&a), names(&b), "deterministic");
        for r in &a {
            crate::reseed_presets::validate_all(&r.policies).unwrap();
        }
        // An aperiodic patch is tuned at level 5 at most.
        let p = Profile {
            name: "p".into(),
            ..Default::default()
        };
        let mut pen = Scene::new(TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(6)));
        pen.cas = vec![CaSpec::new("B2/S/C4")];
        assert_eq!(tune_scene(&p, pen).tiling.size, Some(SizeSpec::Level(5)));
    }

    #[test]
    fn meter_scores_lively_runs_higher() {
        let mut dead = Meter::new(100);
        let mut lively = Meter::new(100);
        let mut noise = Meter::new(100);
        for g in 0..1000 {
            dead.sample(if g < 100 { 0.05 } else { 0.0 }, 0.01);
            lively.sample(0.01 + 0.005 * ((g % 7) as f64), 0.2);
            noise.sample(0.45, 0.5);
        }
        assert!(lively.score() > 0.9, "{}", lively.summary());
        assert!(dead.score() < 0.2, "{}", dead.summary());
        assert!(noise.score() < 0.2, "{}", noise.summary());
        assert_eq!(dead.longest_quiet, 900);
        assert_eq!(lively.chunks.len(), 10);
    }

    /// Runs every built-in profile at screensaver size (2560 x 1440 at its cell size) and prints
    /// how lively it stays, per 1,000-generation chunk; writes a snapshot of each to
    /// `$JETCARP_PROFILE_OUT` if set. `JETCARP_PROFILE_ONLY=Life,Triangles` picks some,
    /// `JETCARP_PROFILE_GENS` sets the length (default 12,000). For tuning the built-ins:
    /// `cargo test -p jetcarp-app --release profile_survey -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn profile_survey() {
        let out = std::env::var("JETCARP_PROFILE_OUT").ok().map(PathBuf::from);
        let only = std::env::var("JETCARP_PROFILE_ONLY").ok();
        let gens: u64 = std::env::var("JETCARP_PROFILE_GENS")
            .ok()
            .and_then(|g| g.parse().ok())
            .unwrap_or(12_000);
        // Probes: `JETCARP_PROFILE_RULE`, `JETCARP_PROFILE_DENSITY`, `JETCARP_PROFILE_RESEED` (a JSON
        // list of policies) replace the picked profiles' rule, soup density, and policies.
        let rule = std::env::var("JETCARP_PROFILE_RULE").ok();
        let density: Option<f64> = std::env::var("JETCARP_PROFILE_DENSITY")
            .ok()
            .and_then(|d| d.parse().ok());
        let reseed: Option<Vec<AutoReseed>> = std::env::var("JETCARP_PROFILE_RESEED")
            .ok()
            .map(|j| serde_json::from_str(&j).expect("JETCARP_PROFILE_RESEED: a JSON list"));
        for mut p in builtins() {
            if only
                .as_deref()
                .is_some_and(|o| !o.split(',').any(|x| x.trim() == p.name))
            {
                continue;
            }
            if let Some(r) = &reseed {
                p.reseed = Some(r.clone());
            }
            let Show::Scene { scene } = &mut p.show else {
                continue;
            };
            if let Ok(f) = std::env::var("JETCARP_PROFILE_FAMILY") {
                scene.tiling.family = f;
            }
            for ca in &mut scene.cas {
                if let Some(r) = &rule {
                    ca.rule = r.clone();
                }
                if let Some(d) = density {
                    ca.seed = Some(SeedSpec::Random { density: d });
                }
            }
            let t0 = std::time::Instant::now();
            let base = (**scene).clone();
            let sc = apply_profile(&p, base, Some([2560.0, 1440.0]));
            let mut w = World::from_scene(&sc).unwrap();
            if let Some(from) = p.reseed_cells {
                let s = scale_policies(w.reseed_policies(), from, w.topo.len());
                w.apply(Command::SetReseed { policies: s }).unwrap();
            }
            let (m, fired) = measure(&mut w, gens, 1000).unwrap();
            let chunks: Vec<String> = m
                .chunks
                .iter()
                .map(|a| crate::reseed_presets::pct(*a))
                .collect();
            println!(
                "{:20} {:>8} cells, {} gens in {:.0} s: {}; fired {fired}\n    chunks: {}",
                p.name,
                w.topo.len(),
                gens,
                t0.elapsed().as_secs_f64(),
                m.summary(),
                chunks.join(" ")
            );
            if let Some(o) = &out {
                std::fs::create_dir_all(o).unwrap();
                let img = render::render(
                    &w,
                    &jetcarp_core::render::RenderOptions {
                        max_side: 1280,
                        ..Default::default()
                    },
                );
                render::write_png(&o.join(format!("{}.png", slug(&p.name))), &img, &[]).unwrap();
            }
        }
    }
}
