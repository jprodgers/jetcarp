//! One stage 1 setting run from seeded soups and measured into a descriptor vector.
//!
//! A **setting** is a rule (B/S or Generations without B0), a periodic tiling and neighbourhood,
//! a soup density and size, a grid size and boundary (wrapped or dead edges), a generation count,
//! a number of soups, and a seed. Soup `s` is exactly what the world makes from the setting's
//! scene ([`Setting::scene`]) with RNG state `soup_state(seed, s)`: a random fill of the whole
//! grid (`soup` = 1) or a centred random disc (`soup` < 1, the world's `Blob` with that radius).
//! Two **probe** soups at 0.6 and 1.5 times the density (clamped to 0.95) test whether the
//! behaviour holds up across densities; they use soup indices `soups` and `soups + 1`.
//!
//! Each soup runs on a bit-parallel [`Board`] until it dies, repeats exactly (the state hash
//! and population equal those of an earlier generation; the settle generation is the first
//! `t` with hash(t) = hash(t + p)), or reaches the generation limit. Recorded per soup: the
//! outcome, settle generation and period, the fraction of cells changing in each eighth of the
//! run (activity), the density at 16 points, the looks of the frames at 1/8, 1/4, 1/2 and the end
//! ([`crate::looks`]), and an object census of the final frame (the hunter's
//! [`Census`], when the frame holds at most a fifth of the cells and 4,000 live cells), plus the
//! spaceships of the frame halfway (ships often meet debris before the end). A run
//! that repeats keeps its last cycle's activity and population for the rest of the curves.
//!
//! [`Descriptors`] summarise all soups. Everything is a pure function of the setting, so results
//! are the same at any thread count.

use crate::board::Board;
use crate::looks::{FrameLooks, frame_looks};
use crate::hunt::separate::{Cache, Census, FastMap, Obj, normalize};
use crate::hunt::soup::soup_state;
use crate::library::{CellMap, PatternClass, canonical_symmetries, tiling_display_name};
use crate::rules::Rule;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use crate::world::{CaSpec, Command, Scene, SeedSpec, World};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Activity windows per run.
pub const WINDOWS: usize = 8;
/// Population samples per run.
pub const POP_SAMPLES: usize = 16;
/// Frames measured: at these fractions of the run.
pub const FRAMES: [f64; 4] = [0.125, 0.25, 0.5, 1.0];
/// A run whose final density is at least this "filled" the grid.
pub const FILLED: f64 = 0.7;
/// A run that repeats before this fraction of its generations "froze early".
pub const EARLY: f64 = 0.05;
/// Longest object period the census looks for.
pub const OBJECT_PERIOD: u64 = 64;

/// One stage 1 setting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    /// Rule notation (`B3/S23`, `B2/S/C3`; a `V` suffix means edge neighbours).
    pub rule: String,
    /// Tiling family and parameters (size and boundary come from `grid` and `boundary`).
    pub tiling: TilingSpec,
    /// Neighbourhood; `None` lets the notation decide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nb: Option<Nb>,
    pub density: f64,
    /// Soup size: 1 fills the grid; less is a centred disc of this radius (a fraction of half
    /// the grid's shorter side).
    pub soup: f64,
    /// Grid size in unit cells.
    pub grid: [u32; 2],
    /// `wrap` or `dead`.
    pub boundary: Boundary,
    pub gens: u64,
    pub soups: u32,
    pub seed: u64,
}

impl Default for Setting {
    fn default() -> Self {
        Setting {
            rule: "B3/S23".into(),
            tiling: TilingSpec::new("square"),
            nb: None,
            density: 0.35,
            soup: 1.0,
            grid: [64, 64],
            boundary: Boundary::Wrap,
            gens: 1000,
            soups: 4,
            seed: 1,
        }
    }
}

impl Setting {
    /// The tiling with this setting's size and boundary.
    pub fn grid_tiling(&self) -> TilingSpec {
        let mut t = crate::library::tiling_identity(&self.tiling);
        t.size = Some(SizeSpec::Rect {
            w: self.grid[0],
            h: self.grid[1],
        });
        t.boundary = self.boundary;
        t
    }

    /// The parsed rule and the neighbourhood in use.
    pub fn rule(&self) -> Result<(Rule, Nb), String> {
        let (r, hint) = Rule::parse(&self.rule)?;
        Ok((r, self.nb.or(hint).unwrap_or_default()))
    }

    /// The CA notation as the world reports it (`B2/S34V` for edge neighbours).
    pub fn notation(&self) -> Result<String, String> {
        let (r, nb) = self.rule()?;
        Ok(crate::hunt::separate::ca_notation(&r, nb))
    }

    /// The seed of the world's CA for a density.
    pub fn seed_spec(&self, density: f64) -> SeedSpec {
        if self.soup >= 1.0 {
            SeedSpec::Random { density }
        } else {
            SeedSpec::Blob {
                radius: self.soup,
                density,
            }
        }
    }

    /// The densities of the probe soups.
    pub fn probe_densities(&self) -> [f64; 2] {
        [
            (self.density * 0.6).clamp(0.01, 0.95),
            (self.density * 1.5).clamp(0.01, 0.95),
        ]
    }

    /// A scene that reproduces soup `soup` of this setting exactly.
    pub fn scene(&self, soup: u32) -> Result<Scene, String> {
        let (_, nb) = self.rule()?;
        let mut s = Scene::new(self.grid_tiling());
        s.rng_state = soup_state(self.seed, soup as u64);
        let mut ca = CaSpec::new(&self.rule);
        ca.neighborhood = Some(nb);
        ca.seed = Some(self.seed_spec(self.density));
        s.cas = vec![ca];
        Ok(s)
    }

    /// A short key for logs and the unique index (`density`, soup, grid, boundary, gens).
    pub fn label(&self) -> String {
        format!(
            "{} on {} {}x{} {}, density {}, soup {}, {} gens",
            self.rule,
            tiling_display_name(&self.tiling),
            self.grid[0],
            self.grid[1],
            boundary_name(self.boundary),
            self.density,
            self.soup,
            self.gens
        )
    }

    pub fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.density) {
            return Err("density must be between 0 and 1".into());
        }
        if self.soup.is_nan() || self.soup <= 0.0 {
            return Err("soup size must be above 0".into());
        }
        if self.gens == 0 || self.soups == 0 {
            return Err("a setting needs at least one generation and one soup".into());
        }
        if self.grid[0] < 8 || self.grid[1] < 8 || self.grid[0] > 4096 || self.grid[1] > 4096 {
            return Err("grid sides must be 8 to 4,096 unit cells".into());
        }
        if self.boundary == Boundary::Live {
            return Err("stage 1 runs wrapped or dead edges, not live edges".into());
        }
        Ok(())
    }
}

pub fn boundary_name(b: Boundary) -> &'static str {
    match b {
        Boundary::Wrap => "wrap",
        Boundary::Dead => "dead",
        Boundary::Live => "live",
    }
}

/// How one soup's run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Died,
    /// Repeats exactly (period 1 is frozen).
    Settled,
    /// Still changing at the generation limit.
    Active,
}

/// One soup's run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoupRun {
    pub soup: u32,
    pub density: f64,
    pub outcome: Outcome,
    /// First generation of the final cycle, or the generation it died.
    pub settle: Option<u64>,
    pub period: Option<u64>,
    pub start_density: f64,
    pub final_density: f64,
    pub peak_density: f64,
    /// Mean fraction of cells changing per generation in each eighth of the run.
    pub activity: Vec<f64>,
    /// Density at 16 evenly spaced generations (the last is the end).
    pub population: Vec<f64>,
    pub frames: Vec<FrameLooks>,
    /// Object census of the final frame (`None` when skipped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<ObjectCensus>,
}

/// Objects in a final frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectCensus {
    /// Copies by class (`still life`, `oscillator`, `spaceship`, `other`).
    pub by_class: BTreeMap<String, u64>,
    /// Distinct pattern ids by class.
    pub ids: BTreeMap<String, BTreeSet<String>>,
    pub longest_period: u64,
    /// Distinct patterns with a symmetry of the tiling other than the identity.
    pub symmetric: BTreeSet<String>,
}

/// Everything measured about a setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Descriptors {
    /// Soups at the setting's density (probes are counted separately).
    pub runs: u32,
    // Lives
    pub died: f64,
    pub filled: f64,
    pub froze_early: f64,
    pub settled: f64,
    pub active: f64,
    /// Mean over soups of log(1 + settle) / log(1 + gens) (1 for runs still active).
    pub transient: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_settle: Option<f64>,
    pub longest_run_period: u64,
    // Looks
    /// Mean activity per window over soups.
    pub activity: Vec<f64>,
    pub activity_mean: f64,
    pub activity_end: f64,
    /// Mean density curve over soups.
    pub population: Vec<f64>,
    pub final_density: f64,
    /// Final frame: compressed size against noise, block entropy (means over soups).
    pub compress: f64,
    pub entropy: f64,
    /// Middle frames (1/8 to 1/2 of the run), means.
    pub compress_mid: f64,
    pub entropy_mid: f64,
    /// Final population over starting population (growth from small soups).
    pub growth: f64,
    // Structure
    /// Object copies per censused soup.
    pub objects: f64,
    pub distinct: u32,
    pub ship_kinds: u32,
    pub ships: f64,
    pub oscillator_kinds: u32,
    pub still_kinds: u32,
    pub longest_period: u64,
    pub symmetric: u32,
    pub censused: u32,
    // Holds up
    /// Mean distance between soups' summaries (0 = identical behaviour).
    pub spread: f64,
    /// Distance between the main soups and the density probes.
    pub density_spread: f64,
    // Is new (filled in by the search, against the Quick Start and stored settings)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub novelty: Option<f64>,
    /// Main behaviour (`spaceships`, `chaotic`, `oscillators`, `growth`, ...) and every tag.
    pub behaviour: String,
    pub tags: Vec<String>,
    /// Ids of a few ships and oscillators (most copies first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}

/// Shared, read-only pieces for many settings on one grid (topology, census symmetries).
pub struct GridKit {
    pub topo: Arc<Topology>,
    pub small: Arc<Topology>,
    syms: [Vec<CellMap>; 2],
}

impl GridKit {
    pub fn new(s: &Setting) -> Result<GridKit, String> {
        let topo = Arc::new(Topology::build(&s.grid_tiling())?);
        if topo.lattice().is_none() {
            return Err(format!(
                "{}: stage 1 needs a periodic tiling",
                tiling_display_name(&s.tiling)
            ));
        }
        let small = Arc::new(crate::hunt::separate::small_topology(&s.tiling)?);
        let syms = [
            canonical_symmetries(&small, Nb::Touch, true),
            canonical_symmetries(&small, Nb::Edge, true),
        ];
        Ok(GridKit { topo, small, syms })
    }
}

/// The world's starting state for soup `index` at `density`.
pub fn soup_states(
    topo: &Arc<Topology>,
    s: &Setting,
    index: u32,
    density: f64,
) -> Result<Vec<u8>, String> {
    let (_, nb) = s.rule()?;
    let mut w = World::on_topology(topo.clone(), soup_state(s.seed, index as u64));
    let mut ca = CaSpec::new(&s.rule);
    ca.neighborhood = Some(nb);
    ca.seed = Some(s.seed_spec(density));
    w.apply(Command::AddCa { ca })?;
    let f = w.cas[0].field;
    Ok(std::mem::take(&mut w.fields[f].state))
}

/// Evaluate a setting: every soup and both probes.
pub fn evaluate(s: &Setting, kit: &GridKit) -> Result<(Descriptors, Vec<SoupRun>), String> {
    s.check()?;
    let (rule, nb) = s.rule()?;
    let mut board = Board::for_topology(&rule, &kit.topo, nb)?;
    let mut census = Census::from_parts(
        rule.clone(),
        nb,
        kit.small.clone(),
        kit.syms[nb as usize].clone(),
        OBJECT_PERIOD,
    )?;
    census.growth_tests = false;
    census.ranges = false;
    let mut cache = Cache::default();
    let nstates = match rule {
        Rule::Generations { states, .. } => states as usize,
        _ => 2,
    };
    let mut runs = Vec::with_capacity(s.soups as usize + 2);
    let mut jobs: Vec<(u32, f64)> = (0..s.soups).map(|i| (i, s.density)).collect();
    for (k, d) in s.probe_densities().into_iter().enumerate() {
        jobs.push((s.soups + k as u32, d));
    }
    for (i, d) in jobs {
        let start = soup_states(&kit.topo, s, i, d)?;
        runs.push(run_soup(
            &mut board, &census, &mut cache, &start, s.gens, i, d, nstates,
        ));
    }
    let desc = summarize(s, &census, &runs);
    Ok((desc, runs))
}

#[allow(clippy::too_many_arguments)]
fn run_soup(
    board: &mut Board,
    census: &Census,
    cache: &mut Cache,
    start: &[u8],
    gens: u64,
    soup: u32,
    density: f64,
    nstates: usize,
) -> SoupRun {
    board.load(start);
    let n = board.len().max(1) as f64;
    let (w, nt) = (board.w, board.len() / (board.w * board.h));
    let start_density = board.population() as f64 / n;
    let mut hashes: Vec<u64> = Vec::with_capacity(gens as usize + 1);
    let mut changed: Vec<u64> = Vec::with_capacity(gens as usize + 1);
    let mut pops: Vec<u64> = Vec::with_capacity(gens as usize + 1);
    let mut seen: FastMap<u64, (u64, u64)> = FastMap::default();
    hashes.push(board.hash());
    changed.push(0);
    pops.push(board.population());
    seen.insert(board.hash(), (0, board.population()));
    // The frame halfway is censused too (ships often meet debris before the end).
    let half = (gens / 2).max(1);
    let mut mid_census: Option<ObjectCensus> = None;
    let frame_at: Vec<u64> = FRAMES
        .iter()
        .map(|f| ((gens as f64 * f).round() as u64).clamp(1, gens))
        .collect();
    let mut frames: Vec<Option<FrameLooks>> = vec![None; FRAMES.len()];
    let mut outcome = Outcome::Active;
    let mut settle = None;
    let mut period = None;
    let mut peak = board.population();
    let mut t = 0;
    while t < gens {
        board.step();
        t += 1;
        let (h, pop) = (board.hash(), board.population());
        hashes.push(h);
        changed.push(board.changed());
        pops.push(pop);
        peak = peak.max(pop);
        for (k, &g) in frame_at.iter().enumerate() {
            if g == t && frames[k].is_none() {
                frames[k] = Some(frame_looks(&board.states(), w, nt, nstates));
            }
        }
        if t == half && censusable(pop, n) {
            mid_census = Some(object_census(census, cache, &board.cells()));
        }
        if pop == 0 {
            outcome = Outcome::Died;
            settle = Some(t);
            break;
        }
        if let Some(&(t0, p0)) = seen.get(&h)
            && p0 == pop
        {
            let p = t - t0;
            outcome = Outcome::Settled;
            period = Some(p);
            let p = p as usize;
            settle = (0..hashes.len() - p)
                .find(|&a| hashes[a] == hashes[a + p] && pops[a] == pops[a + p])
                .map(|a| a as u64);
            break;
        }
        seen.insert(h, (t, pop));
    }
    // Frames not reached: the state now (a phase of the final cycle, or empty).
    if frames.iter().any(|f| f.is_none()) {
        let now = frame_looks(&board.states(), w, nt, nstates);
        for f in frames.iter_mut().filter(|f| f.is_none()) {
            *f = Some(now);
        }
    }
    // Curves: a run that stopped early keeps its last cycle's values.
    let (cyc_changed, cyc_pop) = match (outcome, period) {
        (Outcome::Settled, Some(p)) => {
            let k = (p as usize).min(changed.len() - 1).max(1);
            let c = &changed[changed.len() - k..];
            let q = &pops[pops.len() - k..];
            (
                c.iter().sum::<u64>() as f64 / k as f64,
                q.iter().sum::<u64>() as f64 / k as f64,
            )
        }
        _ => (0.0, 0.0),
    };
    let ran = t as usize;
    let changed_at = |g: usize| -> f64 {
        if g <= ran {
            changed[g] as f64
        } else {
            cyc_changed
        }
    };
    let pop_at = |g: usize| -> f64 { if g <= ran { pops[g] as f64 } else { cyc_pop } };
    let activity: Vec<f64> = (0..WINDOWS)
        .map(|k| {
            let a = (gens as usize * k / WINDOWS) + 1;
            let b = (gens as usize * (k + 1) / WINDOWS).max(a);
            let sum: f64 = (a..=b).map(changed_at).sum();
            sum / ((b - a + 1) as f64 * n)
        })
        .collect();
    let population: Vec<f64> = (1..=POP_SAMPLES)
        .map(|k| pop_at(gens as usize * k / POP_SAMPLES) / n)
        .collect();
    let final_density = board.population() as f64 / n;
    // Object census of the final frame.
    let pop = board.population();
    let mut objects = censusable(pop, n).then(|| object_census(census, cache, &board.cells()));
    // Ships seen halfway count too (their ids, and copies if the end shows none).
    if let Some(m) = mid_census
        && let Some(ship_ids) = m.ids.get("spaceship")
    {
        let o = objects.get_or_insert_with(ObjectCensus::default);
        let ids = o.ids.entry("spaceship".into()).or_default();
        ids.extend(ship_ids.iter().cloned());
        let mid_ships = m.by_class.get("spaceship").copied().unwrap_or(0);
        let n = o.by_class.entry("spaceship".into()).or_default();
        *n = (*n).max(mid_ships);
    }
    SoupRun {
        soup,
        density,
        outcome,
        settle,
        period,
        start_density,
        final_density,
        peak_density: peak as f64 / n,
        activity,
        population,
        frames: frames.into_iter().map(|f| f.unwrap_or_default()).collect(),
        objects,
    }
}

/// The census runs on frames with at most a fifth of the cells live and at most 4,000 cells.
fn censusable(pop: u64, cells: f64) -> bool {
    pop > 0 && pop as f64 <= 0.2 * cells && pop <= 4000
}

fn object_census(c: &Census, cache: &mut Cache, cells: &[crate::objects::LCell]) -> ObjectCensus {
    let mut out = ObjectCensus::default();
    let mut sym_checked: FastMap<String, bool> = FastMap::default();
    for p in c.census(cache, cells) {
        match &p.obj {
            Obj::Pattern(f) => {
                let class = match f.class {
                    PatternClass::StillLife => "still life",
                    PatternClass::Oscillator => "oscillator",
                    PatternClass::Spaceship => "spaceship",
                    _ => "other",
                };
                *out.by_class.entry(class.into()).or_default() += 1;
                out.ids
                    .entry(class.into())
                    .or_default()
                    .insert(f.id.clone());
                out.longest_period = out.longest_period.max(f.period);
                let sym = *sym_checked
                    .entry(f.id.clone())
                    .or_insert_with(|| symmetric(c, &f.record.phases));
                if sym {
                    out.symmetric.insert(f.id.clone());
                }
            }
            Obj::Other(_) => *out.by_class.entry("other".into()).or_default() += 1,
        }
    }
    out
}

/// Does the first phase have a symmetry of the tiling other than the identity?
fn symmetric(c: &Census, phases: &[String]) -> bool {
    let Some(cells) = phases
        .first()
        .and_then(|p| crate::library::decode_cells(p).ok())
    else {
        return false;
    };
    let (base, _) = normalize(&cells);
    c.syms.iter().skip(1).any(|m| {
        let mut moved: Vec<_> = base.iter().map(|x| m.apply(x)).collect();
        moved.sort_unstable();
        normalize(&moved).0 == base
    })
}

/// Per-soup summary vector for spreads: outcome (died, settled, active, filled), final
/// density, end activity (scaled), final compress and entropy.
fn soup_vector(r: &SoupRun) -> [f64; 8] {
    let last = r.frames.last().copied().unwrap_or_default();
    [
        (r.outcome == Outcome::Died) as u8 as f64,
        (r.outcome == Outcome::Settled) as u8 as f64,
        (r.outcome == Outcome::Active) as u8 as f64,
        (r.final_density >= FILLED) as u8 as f64,
        r.final_density.min(1.0),
        (r.activity.last().copied().unwrap_or(0.0) * 10.0).min(1.0),
        last.compress_vs_noise.min(1.0),
        last.block_entropy.unwrap_or(0.0),
    ]
}

fn mean_distance(a: &[[f64; 8]], b: &[[f64; 8]], same: bool) -> f64 {
    let mut sum = 0.0;
    let mut n = 0usize;
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            if same && j <= i {
                continue;
            }
            sum += x.iter().zip(y).map(|(p, q)| (p - q).abs()).sum::<f64>() / 8.0;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { sum / n as f64 }
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for x in v {
        s += x;
        n += 1;
    }
    if n == 0 { 0.0 } else { s / n as f64 }
}

fn summarize(s: &Setting, _c: &Census, all: &[SoupRun]) -> Descriptors {
    let main: Vec<&SoupRun> = all.iter().filter(|r| r.soup < s.soups).collect();
    let probes: Vec<&SoupRun> = all.iter().filter(|r| r.soup >= s.soups).collect();
    let k = main.len().max(1) as f64;
    let frac = |f: &dyn Fn(&SoupRun) -> bool| main.iter().filter(|r| f(r)).count() as f64 / k;
    let g = s.gens as f64;
    let mut settles: Vec<u64> = main.iter().filter_map(|r| r.settle).collect();
    settles.sort_unstable();
    let median_settle = (!settles.is_empty()).then(|| {
        let m = settles.len() / 2;
        if settles.len() % 2 == 1 {
            settles[m] as f64
        } else {
            (settles[m - 1] + settles[m]) as f64 / 2.0
        }
    });
    let curve = |f: &dyn Fn(&SoupRun) -> &Vec<f64>, n: usize| -> Vec<f64> {
        (0..n)
            .map(|i| mean(main.iter().map(|r| f(r).get(i).copied().unwrap_or(0.0))))
            .collect()
    };
    let activity = curve(&|r| &r.activity, WINDOWS);
    let population = curve(&|r| &r.population, POP_SAMPLES);
    let last = |r: &SoupRun| r.frames.last().copied().unwrap_or_default();
    let mids = |r: &SoupRun| r.frames[..r.frames.len().saturating_sub(1)].to_vec();
    // Objects over every main soup that was censused.
    let mut by_class: BTreeMap<String, u64> = BTreeMap::new();
    let mut ids: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut symmetric: BTreeSet<String> = BTreeSet::new();
    let mut longest_period = 0;
    let mut censused = 0u32;
    for r in &main {
        if let Some(o) = &r.objects {
            censused += 1;
            for (c, n) in &o.by_class {
                *by_class.entry(c.clone()).or_default() += n;
            }
            for (c, set) in &o.ids {
                for id in set {
                    *ids.entry(c.clone())
                        .or_default()
                        .entry(id.clone())
                        .or_default() += 1;
                }
            }
            symmetric.extend(o.symmetric.iter().cloned());
            longest_period = longest_period.max(o.longest_period);
        }
    }
    let kinds = |c: &str| ids.get(c).map_or(0, |m| m.len() as u32);
    let distinct = ids
        .iter()
        .filter(|(c, _)| c.as_str() != "other")
        .map(|(_, m)| m.len() as u32)
        .sum();
    let per_censused = censused.max(1) as f64;
    let mv: Vec<[f64; 8]> = main.iter().map(|r| soup_vector(r)).collect();
    let pv: Vec<[f64; 8]> = probes.iter().map(|r| soup_vector(r)).collect();
    let start = mean(main.iter().map(|r| r.start_density));
    let fin = mean(main.iter().map(|r| r.final_density));
    let mut examples: Vec<(u64, String)> = Vec::new();
    for c in ["spaceship", "oscillator"] {
        if let Some(m) = ids.get(c) {
            examples.extend(m.iter().map(|(id, n)| (*n, id.clone())));
        }
    }
    examples.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut d = Descriptors {
        runs: main.len() as u32,
        died: frac(&|r| r.outcome == Outcome::Died),
        filled: frac(&|r| r.final_density >= FILLED),
        froze_early: frac(&|r| {
            r.outcome == Outcome::Settled && r.settle.is_some_and(|t| (t as f64) < EARLY * g)
        }),
        settled: frac(&|r| r.outcome == Outcome::Settled),
        active: frac(&|r| r.outcome == Outcome::Active),
        transient: mean(main.iter().map(|r| match r.settle {
            Some(t) => (1.0 + t as f64).ln() / (1.0 + g).ln(),
            None => 1.0,
        })),
        median_settle,
        longest_run_period: main.iter().filter_map(|r| r.period).max().unwrap_or(0),
        activity_mean: mean(activity.iter().copied()),
        activity_end: activity.last().copied().unwrap_or(0.0),
        activity,
        final_density: fin,
        population,
        compress: mean(main.iter().map(|r| last(r).compress_vs_noise)),
        entropy: mean(main.iter().map(|r| last(r).block_entropy.unwrap_or(0.0))),
        compress_mid: mean(
            main.iter()
                .flat_map(|r| mids(r).into_iter().map(|f| f.compress_vs_noise)),
        ),
        entropy_mid: mean(
            main.iter()
                .flat_map(|r| mids(r).into_iter().map(|f| f.block_entropy.unwrap_or(0.0))),
        ),
        growth: if start > 0.0 { fin / start } else { 0.0 },
        objects: by_class.values().sum::<u64>() as f64 / per_censused,
        distinct,
        ship_kinds: kinds("spaceship"),
        ships: by_class.get("spaceship").copied().unwrap_or(0) as f64 / per_censused,
        oscillator_kinds: kinds("oscillator"),
        still_kinds: kinds("still life"),
        longest_period,
        symmetric: symmetric.len() as u32,
        censused,
        spread: mean_distance(&mv, &mv, true),
        density_spread: mean_distance(&mv, &pv, false),
        novelty: None,
        behaviour: String::new(),
        tags: vec![],
        examples: examples.into_iter().take(6).map(|x| x.1).collect(),
    };
    d.tags = behaviour_tags(s, &d);
    d.behaviour = d.tags.first().cloned().unwrap_or_else(|| "mixed".into());
    d
}

/// Behaviour tags, the main one first: `spaceships`, `growth`, `chaotic`, `oscillators`,
/// `long transients`, `still lifes`, `fills`, `dies out`, `frozen`, `active`.
pub fn behaviour_tags(s: &Setting, d: &Descriptors) -> Vec<String> {
    let mut t: Vec<&str> = Vec::new();
    if d.died >= 0.5 {
        t.push("dies out");
    }
    if d.filled >= 0.5 {
        t.push("fills");
    }
    if d.ship_kinds > 0 {
        t.push("spaceships");
    }
    if s.soup < 1.0 && d.growth >= 3.0 {
        t.push("growth");
    }
    if d.active >= 0.5 && d.entropy >= 0.6 {
        t.push("chaotic");
    }
    if d.oscillator_kinds >= 2 || d.longest_period >= 3 {
        t.push("oscillators");
    }
    if d.settled >= 0.5 && d.transient >= 0.6 {
        t.push("long transients");
    }
    if d.still_kinds >= 3 && d.oscillator_kinds == 0 && d.ship_kinds == 0 {
        t.push("still lifes");
    }
    if d.froze_early >= 0.5 && d.died < 0.5 {
        t.push("frozen");
    }
    if d.active >= 0.5 && !t.contains(&"chaotic") {
        t.push("active");
    }
    // The most telling first: structure over texture over failure.
    let order = [
        "spaceships",
        "growth",
        "oscillators",
        "chaotic",
        "long transients",
        "active",
        "still lifes",
        "frozen",
        "fills",
        "dies out",
    ];
    let mut out: Vec<String> = order
        .iter()
        .filter(|o| t.contains(o))
        .map(|o| o.to_string())
        .collect();
    out.dedup();
    out
}
