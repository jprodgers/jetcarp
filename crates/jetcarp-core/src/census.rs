//! Rule census: run many rules from seeded random soups on one tiling and classify each run.
//!
//! Each run is classified as died, frozen (period 1), oscillating (period p), still active at the
//! generation limit (with its activity fraction), or exploded (population above a fraction of the
//! cells). Periods come from the world's Brent watch, which compares exact states; the settle
//! generation is found exactly too (per-generation hashes pick a candidate, then a replay compares
//! the states themselves). Runs are independent pure functions of (rule, soup seed), so results are
//! identical at any thread count.
//!
//! **Engines.** B/S and Generations rules without B0 on periodic tilings with wrapped or dead
//! edges run on the bit-parallel [`Board`](crate::board::Board) (the world still makes
//! each soup), which emulates the world's Brent watch generation for generation, so every result
//! equals the world's (`tests/census.rs` compares both). Other rules and tilings run on the world.
//! A run's `hash` is the FNV hash of its final cell states ([`World::states_hash`]).
//!
//! **Jobs** (gap 6). [`start_job`] stores a census as a catalog job; the job runner
//! (`crate::jobs::census`) runs its rules in chunks, a step each ([`CensusJob`]), writing each
//! chunk's summaries (`census_rows`) with the progress and checkpoint in one transaction, so an
//! interrupted census resumes where it stopped and [`job_report`] gives the same report as an
//! uninterrupted run.

use crate::render::{Image, RenderOptions, render};
use crate::rng::{Fnv, Rng};
use crate::rules::{Rule, catalog_name};
use crate::topology::{Nb, TilingSpec, Topology};
use crate::world::{CaSpec, Command, SeedSpec, Status, World};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Everything needed to rerun a census.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CensusSpec {
    pub tiling: TilingSpec,
    /// Neighbourhood for every rule; `None` lets each rule's notation decide (`V` = edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood: Option<Nb>,
    pub rules: Vec<String>,
    /// Soups per rule. Soup `s` uses RNG state `seed + s`, so every rule sees the same soups.
    pub soups: u32,
    pub density: f64,
    /// Generation limit per run.
    pub gens: u64,
    pub seed: u64,
    /// A run whose population exceeds this fraction of the cells is stopped and called exploded.
    pub explode_fraction: f64,
    /// Generations at the end over which the activity of a still-active run is averaged.
    #[serde(default = "ten")]
    pub activity_window: u64,
}
fn ten() -> u64 {
    10
}

impl Default for CensusSpec {
    fn default() -> Self {
        CensusSpec {
            tiling: TilingSpec::new("square"),
            neighborhood: None,
            rules: vec!["B3/S23".into()],
            soups: 8,
            density: 0.37,
            gens: 2000,
            seed: 1,
            explode_fraction: 0.5,
            activity_window: 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunClass {
    Died,
    Frozen,
    Oscillating,
    Active,
    Exploded,
}

impl RunClass {
    pub fn name(&self) -> &'static str {
        match self {
            RunClass::Died => "died",
            RunClass::Frozen => "frozen",
            RunClass::Oscillating => "oscillating",
            RunClass::Active => "active",
            RunClass::Exploded => "exploded",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    pub soup: u32,
    pub rng_state: u64,
    pub class: RunClass,
    /// Exact period for frozen (1) and oscillating runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    /// First generation of the final cycle (periodic runs) or the generation the field emptied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle: Option<u64>,
    /// Generations actually run.
    pub gens_run: u64,
    pub final_population: u64,
    pub peak_population: u64,
    /// Still-active runs: mean fraction of cells changing per generation over the last window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<f64>,
    /// World state hash when the run stopped.
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleSummary {
    /// The rule as given.
    pub rule: String,
    /// Resolved notation (with `V` for edge neighbours).
    pub notation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub runs: u32,
    pub died: f64,
    pub frozen: f64,
    pub oscillating: f64,
    pub active: f64,
    pub exploded: f64,
    /// The class most runs fell into.
    pub majority: Option<RunClass>,
    /// Median period of oscillating runs.
    pub median_period: Option<f64>,
    /// Period histogram over frozen and oscillating runs.
    pub periods: BTreeMap<u64, u32>,
    /// Median settle generation over died, frozen, and oscillating runs.
    pub median_settle: Option<f64>,
    pub peak_population: u64,
    pub median_final_density: f64,
    /// Mean activity of still-active runs.
    pub mean_activity: Option<f64>,
    pub results: Vec<RunResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CensusReport {
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    pub spec: CensusSpec,
    pub tiling: String,
    pub cells: usize,
    /// True when some cells have missing neighbours (dead edges or a patch boundary).
    pub dead_boundary: bool,
    /// RNG state of each soup.
    pub rng_states: Vec<u64>,
    pub rules: Vec<RuleSummary>,
}

/// Census results plus optional thumbnails of the final state of soup 0 for each rule.
pub struct CensusOutput {
    pub report: CensusReport,
    pub thumbs: Vec<Option<Image>>,
}

#[derive(Clone, Default)]
pub struct CensusOptions {
    /// Render a thumbnail (this many pixels on its longest side) of each rule's first soup.
    pub thumb_px: Option<u32>,
    /// Run every rule on the world, never the bit-parallel board (for comparisons).
    pub world_only: bool,
}

/// Run the census on the current rayon pool. `progress(done, total)` is called as runs finish.
pub fn run_census(
    spec: &CensusSpec,
    opts: &CensusOptions,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<CensusOutput, String> {
    check_spec(spec)?;
    let topo = Arc::new(Topology::build(&spec.tiling)?);
    let all: Vec<usize> = (0..spec.rules.len()).collect();
    let (rules, thumbs): (Vec<RuleSummary>, Vec<Option<Image>>) =
        run_rules(&topo, spec, &all, opts, progress)
            .into_iter()
            .unzip();
    Ok(CensusOutput {
        report: report(spec, &topo, rules),
        thumbs,
    })
}

fn check_spec(spec: &CensusSpec) -> Result<(), String> {
    if spec.soups == 0 || spec.gens == 0 {
        return Err("a census needs at least one soup and one generation".into());
    }
    if !(0.0..=1.0).contains(&spec.density) {
        return Err("density must be between 0 and 1".into());
    }
    Ok(())
}

fn report(spec: &CensusSpec, topo: &Topology, rules: Vec<RuleSummary>) -> CensusReport {
    CensusReport {
        jetcarp_version: crate::VERSION.into(),
        spec: spec.clone(),
        tiling: topo.describe(),
        cells: topo.len(),
        dead_boundary: topo.has_boundary(),
        rng_states: (0..spec.soups)
            .map(|s| spec.seed.wrapping_add(s as u64))
            .collect(),
        rules,
    }
}

/// Summaries (and first-soup thumbnails) of some of the spec's rules, by index.
fn run_rules(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    which: &[usize],
    opts: &CensusOptions,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Vec<(RuleSummary, Option<Image>)> {
    let jobs: Vec<(usize, u32)> = which
        .iter()
        .flat_map(|&r| (0..spec.soups).map(move |s| (r, s)))
        .collect();
    let total = jobs.len();
    let done = AtomicUsize::new(0);
    let results: Vec<Result<(RunResult, Option<Image>), String>> = jobs
        .par_iter()
        .map(|&(r, s)| {
            let thumb = if s == 0 { opts.thumb_px } else { None };
            let out = run_one(topo, spec, &spec.rules[r], s, thumb, opts.world_only);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(p) = progress {
                p(d, total);
            }
            out
        })
        .collect();
    let mut out = Vec::with_capacity(which.len());
    let mut it = results.into_iter();
    for &ri in which {
        let rule = &spec.rules[ri];
        let mut runs = Vec::new();
        let mut error = None;
        let mut thumb = None;
        for _ in 0..spec.soups {
            match it.next().unwrap() {
                Ok((r, t)) => {
                    if t.is_some() {
                        thumb = t;
                    }
                    runs.push(r);
                }
                Err(e) => error = Some(e),
            }
        }
        let (notation, name) = resolve(topo, spec, rule);
        out.push((
            summarize(rule, notation, name, error, runs, topo.len()),
            thumb,
        ));
    }
    out
}

/// A census as a catalog job.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CensusProgress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    pub rules_done: u64,
    pub rules_total: u64,
    /// Rules per second in this session.
    pub per_s: f64,
    /// Seconds run, over every session.
    pub elapsed_s: f64,
    pub updated: String,
}

impl CensusProgress {
    pub fn line(&self) -> String {
        format!(
            "census {}: {}/{} rules, {:.0} rules/s, {}",
            self.job, self.rules_done, self.rules_total, self.per_s, self.status
        )
    }
}

/// Called with a census job's progress after every chunk.
pub type CensusHook = Arc<dyn Fn(&CensusProgress) + Send + Sync>;

/// Store a census as a job (status `paused` until a runner starts it).
pub fn start_job(lib: &crate::library::Library, spec: &CensusSpec) -> Result<i64, String> {
    check_spec(spec)?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job("census", &v)?;
    lib.update_job(job, Some(crate::library::JobStatus::Paused), None, None)?;
    Ok(job)
}

fn census_job(
    lib: &crate::library::Library,
    job: i64,
) -> Result<(crate::library::store::JobRow, CensusSpec), String> {
    let j = lib.job(job)?;
    if j.kind != "census" {
        return Err(format!("job {job} is a {} job, not a census", j.kind));
    }
    let spec: CensusSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    Ok((j, spec))
}

/// A census job's spec and saved progress.
pub fn job_status(
    lib: &crate::library::Library,
    job: i64,
) -> Result<(CensusSpec, CensusProgress), String> {
    use crate::library::JobStatus;
    let (j, spec) = census_job(lib, job)?;
    let mut p: CensusProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.rules_total = spec.rules.len() as u64;
    p.status = match j.status {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
    .into();
    Ok((spec, p))
}

/// Ask a running census job (in any process) to pause at its next chunk.
pub fn request_stop(lib: &crate::library::Library, job: i64) -> Result<(), String> {
    let (j, _) = census_job(lib, job)?;
    if j.status == crate::library::JobStatus::Running {
        lib.update_job(job, Some(crate::library::JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// A census job run in steps by the job runner (`crate::jobs::census`): each step runs one
/// chunk of rules (adapting to about three seconds of work) and puts their summaries in the
/// sink, which the runner writes with the progress and checkpoint (the next rule index) in one
/// transaction.
pub struct CensusJob {
    spec: CensusSpec,
    topo: Arc<Topology>,
    pool: rayon::ThreadPool,
    /// The next rule index.
    next: u64,
    /// Rules per chunk, and whether it adapts.
    chunk: u64,
    adapt: bool,
    p: CensusProgress,
    session: std::time::Instant,
    session_done: u64,
    base_elapsed: f64,
}

impl CensusJob {
    /// Open the run's census job (on its threads; `chunk` rules per step, 0: adapt) from its
    /// checkpoint, if it has one (the next rule index, as it always was).
    pub fn open(
        cx: &crate::jobs::RunCx<'_>,
        checkpoint: Option<&[u8]>,
        chunk: u64,
    ) -> Result<CensusJob, String> {
        let (j, job) = (cx.row, cx.job);
        if j.kind != "census" {
            return Err(format!("job {job} is a {} job, not a census", j.kind));
        }
        let spec: CensusSpec =
            serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
        check_spec(&spec)?;
        let next: u64 = match checkpoint {
            Some(b) => serde_json::from_slice::<u64>(b).map_err(|e| format!("checkpoint: {e}"))?,
            None => 0,
        };
        let mut p: CensusProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
        p.job = job;
        p.rules_total = spec.rules.len() as u64;
        let topo = Arc::new(Topology::build(&spec.tiling)?);
        let threads = if cx.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            cx.threads
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(CensusJob {
            topo,
            pool,
            next,
            chunk: if chunk > 0 { chunk } else { 32 },
            adapt: chunk == 0,
            base_elapsed: p.elapsed_s,
            p,
            spec,
            session: std::time::Instant::now(),
            session_done: 0,
        })
    }

    /// One chunk of rules (none past the end or the item budget), its summaries put in the
    /// sink. `Ok(true)`: the census has run its last rule.
    pub fn step(
        &mut self,
        cx: &crate::jobs::RunCx<'_>,
        sink: &mut crate::jobs::Sink,
    ) -> Result<bool, String> {
        let total = self.spec.rules.len() as u64;
        if self.next >= total {
            return Ok(true);
        }
        let mut n = self.chunk.min(total - self.next);
        if let Some(l) = cx.items {
            n = n.min(l - self.session_done).max(1);
        }
        let which: Vec<usize> = (self.next as usize..(self.next + n) as usize).collect();
        let t0 = std::time::Instant::now();
        let (topo, spec) = (&self.topo, &self.spec);
        let out = self
            .pool
            .install(|| run_rules(topo, spec, &which, &CensusOptions::default(), None));
        sink.census
            .extend(which.iter().zip(&out).map(|(&i, (r, _))| {
                (
                    i,
                    r.rule.clone(),
                    serde_json::to_value(r).unwrap_or(serde_json::Value::Null),
                )
            }));
        self.next += n;
        self.session_done += n;
        let el = self.session.elapsed().as_secs_f64();
        let p = &mut self.p;
        p.rules_done = self.next;
        p.rules_total = total;
        p.per_s = self.session_done as f64 / el.max(1e-9);
        p.elapsed_s = self.base_elapsed + el;
        p.updated = crate::library::now_utc();
        if self.adapt {
            let dt = t0.elapsed().as_secs_f64();
            let want = (n as f64 * 3.0 / dt.max(1e-3)).clamp(4.0, 4096.0) as u64;
            self.chunk = ((self.chunk + want) / 2).max(4);
        }
        Ok(self.next >= total)
    }

    /// The progress (as of the last chunk) with this status.
    pub fn progress(&self, status: &str) -> CensusProgress {
        CensusProgress {
            status: status.into(),
            ..self.p.clone()
        }
    }

    /// The checkpoint: the next rule index.
    pub fn checkpoint(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.next).map_err(|e| e.to_string())
    }

    /// Rules run in this session.
    pub fn rules_run(&self) -> u64 {
        self.session_done
    }
}

/// The report of a census job (all its rules, once done; the rules so far otherwise).
pub fn job_report(lib: &crate::library::Library, job: i64) -> Result<CensusReport, String> {
    let (_, spec) = census_job(lib, job)?;
    let topo = Topology::build(&spec.tiling)?;
    let rules: Vec<RuleSummary> = lib
        .census_rows(job)?
        .into_iter()
        .map(|v| serde_json::from_value(v).map_err(|e| format!("census row: {e}")))
        .collect::<Result<_, _>>()?;
    Ok(report(&spec, &topo, rules))
}

/// Thumbnails of the first soup of each rule at the end of its run (for a job's contact sheet).
pub fn thumbnails(spec: &CensusSpec, px: u32) -> Result<Vec<Option<Image>>, String> {
    let topo = Arc::new(Topology::build(&spec.tiling)?);
    Ok(spec
        .rules
        .par_iter()
        .map(|r| {
            run_one(&topo, spec, r, 0, Some(px), false)
                .ok()
                .and_then(|x| x.1)
        })
        .collect())
}

fn ca_spec(spec: &CensusSpec, rule: &str) -> Result<CaSpec, String> {
    let (r, _) = Rule::parse(rule)?;
    if r.is_stochastic() {
        return Err(format!(
            "{rule} is stochastic; a census needs deterministic rules"
        ));
    }
    let mut ca = CaSpec::new(rule);
    ca.neighborhood = spec.neighborhood;
    if matches!(r, Rule::LifeLike { .. } | Rule::Generations { .. }) {
        ca.seed = Some(SeedSpec::Random {
            density: spec.density,
        });
    }
    Ok(ca)
}

fn resolve(topo: &Arc<Topology>, spec: &CensusSpec, rule: &str) -> (String, Option<String>) {
    let Ok(ca) = ca_spec(spec, rule) else {
        return (rule.to_string(), None);
    };
    let mut w = World::on_topology(topo.clone(), 0);
    let mut ca = ca;
    ca.seed = Some(SeedSpec::Empty);
    match w.apply(Command::AddCa { ca }) {
        Ok(_) => {
            let c = &w.cas[0];
            (
                c.notation(),
                catalog_name(&c.rule, c.nb).map(|s| s.to_string()),
            )
        }
        Err(_) => (rule.to_string(), None),
    }
}

fn make_world(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    rule: &str,
    rng_state: u64,
) -> Result<World, String> {
    let mut w = World::on_topology(topo.clone(), rng_state);
    w.apply(Command::AddCa {
        ca: ca_spec(spec, rule)?,
    })?;
    Ok(w)
}

/// The world of soup `soup` for one rule: the census's own soups (RNG state `seed + soup`), for
/// other tools that start from the same soups (the pattern-library search).
pub fn soup_world(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    rule: &str,
    soup: u32,
) -> Result<World, String> {
    make_world(topo, spec, rule, spec.seed.wrapping_add(soup as u64))
}

/// Hash of the cell states only (no ages), used to pick settle-generation candidates.
fn states_hash(w: &World) -> u64 {
    let mut h = Fnv::default();
    for f in &w.fields {
        h.bytes(&f.state);
        h.u32s(&f.height);
    }
    h.0
}

fn same_states(a: &World, b: &World) -> bool {
    a.fields
        .iter()
        .zip(&b.fields)
        .all(|(x, y)| x.state == y.state && x.height == y.height)
        && a.cas.iter().zip(&b.cas).all(|(x, y)| x.agents == y.agents)
}

fn run_one(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    rule: &str,
    soup: u32,
    thumb: Option<u32>,
    world_only: bool,
) -> Result<(RunResult, Option<Image>), String> {
    let rng_state = spec.seed.wrapping_add(soup as u64);
    let mut w = make_world(topo, spec, rule, rng_state)?;
    if !world_only && let Some(board) = board_for(topo, &w) {
        return run_board(topo, spec, rule, soup, thumb, w, board);
    }
    let n = topo.len() as f64;
    let fi = w.cas[0].field;
    let window = spec.activity_window.clamp(1, spec.gens);
    let mut hashes = vec![states_hash(&w)];
    let mut peak = w.fields[fi].population();
    let mut changed = 0u64;
    let mut class = RunClass::Active;
    let mut period = None;
    let mut settle = None;
    let mut g = 0;
    while g < spec.gens {
        let prev = (g >= spec.gens - window).then(|| w.fields[fi].state.clone());
        w.apply(Command::Step { gens: 1 })?;
        g += 1;
        let f = &w.fields[fi];
        let pop = f.population();
        peak = peak.max(pop);
        if let Some(p) = prev {
            changed += p.iter().zip(&f.state).filter(|(a, b)| a != b).count() as u64;
        }
        hashes.push(states_hash(&w));
        if pop == 0 && w.cas.iter().all(|c| c.agents.is_empty()) {
            class = RunClass::Died;
            settle = Some(g);
            break;
        }
        if let Status::Repeats { period: p, .. } = w.status() {
            class = if p == 1 {
                RunClass::Frozen
            } else {
                RunClass::Oscillating
            };
            period = Some(p);
            settle = Some(settle_exact(topo, spec, rule, rng_state, &hashes, p)?);
            break;
        }
        if pop as f64 > spec.explode_fraction * n {
            class = RunClass::Exploded;
            break;
        }
    }
    let activity =
        (class == RunClass::Active).then(|| changed as f64 / (window as f64 * n.max(1.0)));
    let image = thumb.map(|px| {
        render(
            &w,
            &RenderOptions {
                max_side: px,
                outline_min_px: 0.0,
                ..Default::default()
            },
        )
    });
    Ok((
        RunResult {
            soup,
            rng_state,
            class,
            period,
            settle,
            gens_run: g,
            final_population: w.fields[fi].population(),
            peak_population: peak,
            activity,
            hash: format!("{:016x}", w.states_hash()),
        },
        image,
    ))
}

/// A bit-parallel board for the world's only CA, when its rule and tiling allow one.
fn board_for(topo: &Topology, w: &World) -> Option<crate::board::Board> {
    if w.cas.len() != 1 || w.cas[0].steps_per_gen != 1 {
        return None;
    }
    let c = &w.cas[0];
    crate::board::Board::for_topology(&c.rule, topo, c.nb).ok()
}

/// [`run_one`] on the bit-parallel board: the same classification, generation for generation
/// (the world's Brent watch emulated on the board's states).
fn run_board(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    rule: &str,
    soup: u32,
    thumb: Option<u32>,
    mut w: World,
    mut b: crate::board::Board,
) -> Result<(RunResult, Option<Image>), String> {
    let rng_state = spec.seed.wrapping_add(soup as u64);
    let n = topo.len() as f64;
    let fi = w.cas[0].field;
    let start = w.fields[fi].state.clone();
    b.load(&start);
    let window = spec.activity_window.clamp(1, spec.gens);
    let mut hashes = vec![(b.hash(), b.population())];
    let mut peak = b.population();
    let mut changed = 0u64;
    let mut class = RunClass::Active;
    let mut period = None;
    let mut settle = None;
    // The world's watch: a snapshot at generations 1, 2, 4, 8, ..., compared with each state.
    let (mut power, mut lam) = (1u64, 0u64);
    let mut snap: Option<(u64, u64, Vec<u8>)> = None;
    let mut g = 0;
    while g < spec.gens {
        b.step();
        g += 1;
        let pop = b.population();
        peak = peak.max(pop);
        if g > spec.gens - window {
            changed += b.changed();
        }
        hashes.push((b.hash(), pop));
        if pop == 0 {
            class = RunClass::Died;
            settle = Some(g);
            break;
        }
        lam += 1;
        let mut found = None;
        if let Some((h, p, st)) = &snap
            && *h == b.hash()
            && *p == pop
            && *st == b.states()
        {
            found = Some(lam);
        }
        if let Some(p) = found {
            class = if p == 1 {
                RunClass::Frozen
            } else {
                RunClass::Oscillating
            };
            period = Some(p);
            settle = Some(settle_board(&b, &start, &hashes, p)?);
            break;
        }
        if lam == power {
            snap = Some((b.hash(), pop, b.states()));
            power *= 2;
            lam = 0;
        }
        if pop as f64 > spec.explode_fraction * n {
            class = RunClass::Exploded;
            break;
        }
    }
    w.fields[fi].state = b.states();
    let activity =
        (class == RunClass::Active).then(|| changed as f64 / (window as f64 * n.max(1.0)));
    let image = thumb.map(|px| {
        render(
            &w,
            &RenderOptions {
                max_side: px,
                outline_min_px: 0.0,
                ..Default::default()
            },
        )
    });
    let _ = rule;
    Ok((
        RunResult {
            soup,
            rng_state,
            class,
            period,
            settle,
            gens_run: g,
            final_population: b.population(),
            peak_population: peak,
            activity,
            hash: format!("{:016x}", w.states_hash()),
        },
        image,
    ))
}

/// The first `t` with state(t) == state(t + p) on a board, checked by exact comparison.
fn settle_board(
    b: &crate::board::Board,
    start: &[u8],
    hashes: &[(u64, u64)],
    p: u64,
) -> Result<u64, String> {
    let p = p as usize;
    let mut a = b.clone();
    a.load(start);
    let mut at = 0usize;
    for t in 0..hashes.len().saturating_sub(p) {
        if hashes[t] != hashes[t + p] {
            continue;
        }
        a.run((t - at) as u64);
        at = t;
        let mut c = a.clone();
        c.run(p as u64);
        if c.states() == a.states() {
            return Ok(t as u64);
        }
    }
    Err("period found but no exact settle generation (internal error)".into())
}

/// The first generation `t` with state(t) == state(t + p), checked by exact comparison.
fn settle_exact(
    topo: &Arc<Topology>,
    spec: &CensusSpec,
    rule: &str,
    rng_state: u64,
    hashes: &[u64],
    p: u64,
) -> Result<u64, String> {
    let p = p as usize;
    let mut a = make_world(topo, spec, rule, rng_state)?;
    let mut at = 0usize;
    for t in 0..hashes.len().saturating_sub(p) {
        if hashes[t] != hashes[t + p] {
            continue;
        }
        // Different hashes prove the states differ, so the first exact match is the answer.
        a.apply(Command::Step {
            gens: (t - at) as u64,
        })?;
        at = t;
        let mut b = make_world(topo, spec, rule, rng_state)?;
        b.apply(Command::Step {
            gens: (t + p) as u64,
        })?;
        if same_states(&a, &b) {
            return Ok(t as u64);
        }
    }
    Err("period found but no exact settle generation (internal error)".into())
}

fn median(mut v: Vec<u64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 {
        v[m] as f64
    } else {
        (v[m - 1] as f64 + v[m] as f64) / 2.0
    })
}

fn summarize(
    rule: &str,
    notation: String,
    name: Option<String>,
    error: Option<String>,
    results: Vec<RunResult>,
    cells: usize,
) -> RuleSummary {
    let k = results.len().max(1) as f64;
    let frac = |c: RunClass| results.iter().filter(|r| r.class == c).count() as f64 / k;
    let mut periods = BTreeMap::new();
    for r in &results {
        if let Some(p) = r.period {
            *periods.entry(p).or_insert(0) += 1;
        }
    }
    let classes = [
        RunClass::Died,
        RunClass::Frozen,
        RunClass::Oscillating,
        RunClass::Active,
        RunClass::Exploded,
    ];
    let majority = if results.is_empty() {
        None
    } else {
        classes
            .iter()
            .copied()
            .max_by(|a, b| frac(*a).partial_cmp(&frac(*b)).unwrap())
    };
    let act: Vec<f64> = results.iter().filter_map(|r| r.activity).collect();
    RuleSummary {
        rule: rule.into(),
        notation,
        name,
        error,
        runs: results.len() as u32,
        died: frac(RunClass::Died),
        frozen: frac(RunClass::Frozen),
        oscillating: frac(RunClass::Oscillating),
        active: frac(RunClass::Active),
        exploded: frac(RunClass::Exploded),
        majority,
        median_period: median(
            results
                .iter()
                .filter(|r| r.class == RunClass::Oscillating)
                .filter_map(|r| r.period)
                .collect(),
        ),
        periods,
        median_settle: median(results.iter().filter_map(|r| r.settle).collect()),
        peak_population: results.iter().map(|r| r.peak_population).max().unwrap_or(0),
        median_final_density: median(results.iter().map(|r| r.final_population).collect())
            .unwrap_or(0.0)
            / cells.max(1) as f64,
        mean_activity: (!act.is_empty()).then(|| act.iter().sum::<f64>() / act.len() as f64),
        results,
    }
}

impl CensusReport {
    /// One row per rule. Lines starting with `#` record the version, tiling, and settings.
    pub fn to_csv(&self) -> String {
        let s = &self.spec;
        let mut out = String::new();
        out.push_str(&format!("# jetcarp_version={}\n", self.jetcarp_version));
        out.push_str(&format!("# tiling={}\n", self.tiling));
        out.push_str(&format!(
            "# tiling_spec={}\n",
            serde_json::to_string(&s.tiling).unwrap_or_default()
        ));
        out.push_str(&format!(
            "# dead_boundary={} soups={} density={} gens={} seed={} explode_fraction={} neighborhood={}\n",
            self.dead_boundary,
            s.soups,
            s.density,
            s.gens,
            s.seed,
            s.explode_fraction,
            match s.neighborhood {
                Some(Nb::Edge) => "edge",
                Some(Nb::Touch) => "touch",
                None => "per_rule",
            }
        ));
        out.push_str("rule,name,runs,died,frozen,oscillating,active,exploded,majority,median_period,periods,median_settle,peak_population,median_final_density,mean_activity,error\n");
        let opt = |v: Option<f64>| v.map(|x| format!("{x}")).unwrap_or_default();
        for r in &self.rules {
            let periods: Vec<String> = r.periods.iter().map(|(p, c)| format!("{p}:{c}")).collect();
            out.push_str(&format!(
                "{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{:.5},{},{}\n",
                csv_field(&r.notation),
                csv_field(r.name.as_deref().unwrap_or("")),
                r.runs,
                r.died,
                r.frozen,
                r.oscillating,
                r.active,
                r.exploded,
                r.majority.map(|m| m.name()).unwrap_or(""),
                opt(r.median_period),
                periods.join(" "),
                opt(r.median_settle),
                r.peak_population,
                r.median_final_density,
                r.mean_activity
                    .map(|x| format!("{x:.5}"))
                    .unwrap_or_default(),
                csv_field(r.error.as_deref().unwrap_or("")),
            ));
        }
        out
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }
    /// A short label for the contact sheet: majority class and its share.
    pub fn label(r: &RuleSummary) -> String {
        match r.majority {
            None => "ERROR".into(),
            Some(m) => {
                let share = match m {
                    RunClass::Died => r.died,
                    RunClass::Frozen => r.frozen,
                    RunClass::Oscillating => r.oscillating,
                    RunClass::Active => r.active,
                    RunClass::Exploded => r.exploded,
                };
                let p = match (m, r.median_period) {
                    (RunClass::Oscillating, Some(p)) => format!(" P{p}"),
                    _ => String::new(),
                };
                format!("{}{} {:.0}%", m.name(), p, share * 100.0)
            }
        }
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// Rule sets

/// Digits like `0-8`, `1,2,5`, or `1-3,6`.
pub fn parse_digits(s: &str) -> Result<Vec<u32>, String> {
    let mut v = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            let a: u32 = a.trim().parse().map_err(|_| format!("bad digit '{a}'"))?;
            let b: u32 = b.trim().parse().map_err(|_| format!("bad digit '{b}'"))?;
            v.extend(a..=b);
        } else {
            v.push(part.parse().map_err(|_| format!("bad digit '{part}'"))?);
        }
    }
    v.sort_unstable();
    v.dedup();
    if v.iter().any(|&d| d > 31) {
        return Err("neighbour counts above 31 are not supported".into());
    }
    Ok(v)
}

/// Largest neighbour count on the tiling for this neighbourhood.
pub fn max_degree(topo: &Topology, nb: Nb) -> usize {
    (0..topo.len())
        .map(|i| topo.degree(i, nb))
        .max()
        .unwrap_or(0)
}

fn notation(birth: u32, survive: u32, states: Option<u8>) -> String {
    match states {
        Some(s) if s > 2 => Rule::Generations {
            birth,
            survive,
            states: s,
        }
        .notation(),
        _ => Rule::LifeLike { birth, survive }.notation(),
    }
}

fn subset_mask(digits: &[u32], bits: u64) -> u32 {
    digits
        .iter()
        .enumerate()
        .filter(|(k, _)| bits >> k & 1 == 1)
        .fold(0, |m, (_, &d)| m | 1 << d)
}

/// Every B/S rule whose birth and survival digits are subsets of these (Generations with `states`).
pub fn all_bs_rules(
    birth: &[u32],
    survive: &[u32],
    states: Option<u8>,
) -> Result<Vec<String>, String> {
    let bits = birth.len() + survive.len();
    if bits > 22 {
        return Err(format!(
            "2^{bits} rules is too many; narrow the digits or use --sample"
        ));
    }
    let mut out = Vec::with_capacity(1 << bits);
    for b in 0..(1u64 << birth.len()) {
        for s in 0..(1u64 << survive.len()) {
            out.push(notation(
                subset_mask(birth, b),
                subset_mask(survive, s),
                states,
            ));
        }
    }
    Ok(out)
}

/// `n` distinct B/S rules drawn uniformly (by digit subsets) from a seeded SplitMix64 stream.
pub fn sample_bs_rules(
    birth: &[u32],
    survive: &[u32],
    states: Option<u8>,
    n: usize,
    seed: u64,
) -> Vec<String> {
    let bits = (birth.len() + survive.len()) as u32;
    let total = if bits >= 63 { u64::MAX } else { 1u64 << bits };
    let n = (n as u64).min(total) as usize;
    let mut rng = Rng::new(seed);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let x = rng.next_u64();
        let b = x & ((1u64 << birth.len()) - 1);
        let s = (x >> birth.len()) & ((1u64 << survive.len()) - 1);
        if seen.insert((b, s)) {
            out.push(notation(
                subset_mask(birth, b),
                subset_mask(survive, s),
                states,
            ));
        }
    }
    out
}

/// One rule per line (or separated by semicolons); blank lines and `#` comments are skipped.
pub fn parse_rule_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(|l| l.split(';'))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
