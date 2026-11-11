//! Evolutionary rule search for gliders, after James Edmond's "Gliders on Aperiodic Monotilings"
//! (arXiv:2609.22579) and its code (`monotile-ca`, MIT OR Apache-2.0). The method is
//! reimplemented here from the paper and the repository, generalised from the hat, Spectre and
//! Penrose patches to every JETCARP tiling. [`mutate`] is adapted from `monotile-ca`'s mutation
//! operator (MIT licence, Copyright (c) 2026 James Edmond; see THIRD_PARTY_NOTICES.md).
//!
//! - **Genome**: a priority table ([`priority`]): up to 8 ordered rows `own | conditions -> next`,
//!   at most 2 conditions `nS>=m` (m = 1..3) per row, first match wins, default 0.
//! - **Fitness** ([`arena`]): 13 small seeds (for 4 states) on a ball of radius 48 edge rings;
//!   each run scores `distance · P_inner / P_max`; the genome's fitness is its best seed.
//! - **Anti-gaming filters**, each of which the original search needed after a rule exploited its
//!   absence: the population cap (explosive rules score 0); raw distance instead of an escape
//!   bonus (mortal travellers that outlive a short patch); the flatness ratio `P_inner / P_max`
//!   (thin slow growers); the light-cone (causality) filter (rules igniting low-degree boundary
//!   tiles, "boundary sniffing"); classification at `radius - fan margin`; and verification
//!   outside the GA at radii r, 2r, 4r with a flat peak population (growers and loopers whose
//!   failure shows only on bigger patches). Trivial oscillators and blinker farms never travel,
//!   so they score at most their small spread and lose to travellers; the cap kills farms that
//!   spread.
//! - **GA**: population 256, elitism of the top 1/16, tournaments of 3, one-point row splice
//!   crossover on every child, two independent 50% chances of a mutation (insert, delete or swap
//!   a row, or edit one field of a row), xorshift64 with the original seed scramble. Genomes are
//!   evaluated in parallel and gathered in order, and the RNG is only used between generations,
//!   so a run is a pure function of its seed (identical at any thread count).
//!
//! Verified gliders go on to the pattern library: on lattices the detector finds the exact
//! spaceship (lattice translation); on aperiodic patches the mover tracker
//! ([`crate::pattern::movers`]) follows the glider across growing patch levels and records the
//! distance it was verified to.

pub mod arena;
pub mod priority;

use arena::{Arena, Outcome, RunStats};
use priority::{MAX_CONDS, MAX_MIN, MAX_ROWS, PriorityRule, Row};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use crate::topology::{Nb, TilingSpec};

/// Search parameters. Defaults are the published ones (radius 48, horizon 2500, population 256,
/// 800 generations, 4 states, cap 64).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolveSpec {
    /// Tiling identity (family, parameters); the size is chosen to fit the arena.
    pub tiling: TilingSpec,
    #[serde(default = "touch")]
    pub nb: Nb,
    pub states: u8,
    pub pop: usize,
    pub gens: usize,
    pub seed: u64,
    pub radius: u32,
    pub horizon: u64,
    pub pop_cap: usize,
    /// Distinct top genomes verified at the end.
    pub champions: usize,
    /// Verification radii as multiples of `radius` (the paper: 1, 2, 4; more for the record).
    pub verify_factors: Vec<u32>,
    /// Stop as soon as a genome passes verification (checked each generation on new genomes at
    /// the fitness ceiling).
    pub stop_on_glider: bool,
    /// Worker threads (None: the current rayon pool). Results do not depend on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
}

fn touch() -> Nb {
    Nb::Touch
}

impl Default for EvolveSpec {
    fn default() -> Self {
        EvolveSpec {
            tiling: TilingSpec::new("spectre"),
            nb: Nb::Touch,
            states: 4,
            pop: 256,
            gens: 800,
            seed: 1,
            radius: 48,
            horizon: 2500,
            pop_cap: 64,
            champions: 16,
            verify_factors: vec![1, 2, 4],
            stop_on_glider: false,
            threads: None,
        }
    }
}

/// One generation's numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenStats {
    pub generation: usize,
    pub best: f64,
    pub median: f64,
    pub mean: f64,
    pub best_ever: f64,
    /// Genomes this generation within 90% of the arena's boundary distance.
    pub at_ceiling: usize,
    /// Distinct behaviours (simplified rules) in the population.
    pub distinct: usize,
    /// Genome evaluations so far (each a full seed bank), and distinct ones actually run.
    pub evaluations: u64,
    pub unique_evaluations: u64,
    /// Wall time so far (not part of the deterministic result).
    pub elapsed_s: f64,
}

/// One radius of a cross-radius check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadiusCheck {
    pub radius: u32,
    pub tiles: usize,
    pub outcome: Outcome,
    pub generation: u64,
    pub max_population: usize,
}

/// Cross-radius verification of a candidate (outside the GA).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossCheck {
    pub passed: bool,
    pub reason: String,
    pub radii: Vec<RadiusCheck>,
}

/// A distinct high-scoring genome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Champion {
    /// The rule with inert rows removed (what runs).
    pub rule: String,
    /// The genome as evolved.
    pub genome: String,
    pub fitness: f64,
    /// Index into the seed bank, and the seed as (cell, state) on the arena (cell 0 = root).
    pub seed_index: usize,
    pub seed: Vec<(u32, u8)>,
    pub stats: RunStats,
    /// First GA generation it appeared.
    pub first_generation: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<CrossCheck>,
}

/// What a search produced.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolveReport {
    pub spec: EvolveSpec,
    pub jetcarp_version: String,
    /// The arena: tiles, size or level, fan margin, boundary ring.
    pub arena_tiles: usize,
    pub arena_tiling: TilingSpec,
    pub margin: u32,
    pub boundary: u32,
    pub generations: Vec<GenStats>,
    pub champions: Vec<Champion>,
    /// Verified gliders (champions that passed), best first.
    pub gliders: Vec<Champion>,
    pub stopped_early: bool,
    pub elapsed_s: f64,
}

impl EvolveReport {
    /// The per-generation CSV.
    pub fn fitness_csv(&self) -> String {
        let mut s = String::from(
            "generation,best,median,mean,best_ever,at_ceiling,distinct,evaluations,unique_evaluations,elapsed_s\n",
        );
        for g in &self.generations {
            s.push_str(&format!(
                "{},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{:.3}\n",
                g.generation,
                g.best,
                g.median,
                g.mean,
                g.best_ever,
                g.at_ceiling,
                g.distinct,
                g.evaluations,
                g.unique_evaluations,
                g.elapsed_s
            ));
        }
        s
    }
    /// Everything except timings, for determinism checks.
    pub fn fingerprint(&self) -> String {
        let mut s = String::new();
        for g in &self.generations {
            s.push_str(&format!(
                "{} {:.6} {:.6} {:.6} {} {}\n",
                g.generation, g.best, g.median, g.mean, g.at_ceiling, g.distinct
            ));
        }
        for c in &self.champions {
            s.push_str(&format!(
                "{} {} {} {:?}\n",
                c.rule,
                c.fitness,
                c.first_generation,
                c.verify.as_ref().map(|v| v.passed)
            ));
        }
        s
    }
}

/// Write a run folder: `fitness.csv` (per generation), `champions.json` (top distinct genomes
/// with their cross-radius checks), `gliders.json` (the ones that passed), and `run.json` (the
/// whole report). Returns the files written.
pub fn write_outputs(
    report: &EvolveReport,
    dir: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    let mut put = |name: &str, text: String| -> Result<(), String> {
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("writing {}: {e}", p.display()))?;
        out.push(p);
        Ok(())
    };
    put("fitness.csv", report.fitness_csv())?;
    put(
        "champions.json",
        serde_json::to_string_pretty(&report.champions).unwrap() + "\n",
    )?;
    put(
        "gliders.json",
        serde_json::to_string_pretty(&report.gliders).unwrap() + "\n",
    )?;
    put(
        "run.json",
        serde_json::to_string_pretty(report).unwrap() + "\n",
    )?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// RNG (xorshift64 13/7/17 with the original seed scramble)

#[derive(Clone, Debug)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        XorShift((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1234_5678_9ABC_DEF1) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

// ---------------------------------------------------------------------------------------------
// Genome operators

fn random_cond(rng: &mut XorShift, k: u8) -> (u8, u8) {
    let s = rng.below(k as usize) as u8;
    let m = 1 + rng.below(MAX_MIN as usize) as u8;
    (s, m)
}

fn random_own(rng: &mut XorShift, k: u8) -> Option<u8> {
    match rng.below(k as usize + 1) {
        0 => None,
        v => Some(v as u8 - 1),
    }
}

/// A random row: own (k+1 options, 0 = any), 0-2 conditions, next state.
pub fn random_row(rng: &mut XorShift, k: u8) -> Row {
    let own = random_own(rng, k);
    let n = rng.below(MAX_CONDS + 1);
    let conds = (0..n).map(|_| random_cond(rng, k)).collect();
    let next = rng.below(k as usize) as u8;
    Row { own, conds, next }
}

/// A random genome of 2 to 6 rows.
pub fn random_genome(rng: &mut XorShift, k: u8) -> PriorityRule {
    let n = 2 + rng.below(5);
    PriorityRule {
        states: k,
        rows: (0..n).map(|_| random_row(rng, k)).collect(),
    }
}

/// One mutation: insert, delete or swap rows, or edit one field of a row. Adapted from
/// `monotile-ca` (MIT licence, Copyright (c) 2026 James Edmond).
pub fn mutate(g: &mut PriorityRule, rng: &mut XorShift) {
    let k = g.states;
    let len = g.rows.len();
    let r = rng.below(8);
    if r == 0 && len < MAX_ROWS {
        let at = rng.below(len + 1);
        let row = random_row(rng, k);
        g.rows.insert(at, row);
        return;
    }
    if r == 1 && len > 1 {
        let at = rng.below(len);
        g.rows.remove(at);
        return;
    }
    if r == 2 && len > 1 {
        let at = rng.below(len - 1);
        g.rows.swap(at, at + 1);
        return;
    }
    if len == 0 {
        g.rows.push(random_row(rng, k));
        return;
    }
    let i = rng.below(len);
    let q = rng.below(4);
    let row = &mut g.rows[i];
    match q {
        0 => row.own = random_own(rng, k),
        1 => row.next = rng.below(k as usize) as u8,
        2 if !row.conds.is_empty() => {
            let c = rng.below(row.conds.len());
            row.conds[c] = random_cond(rng, k);
        }
        _ => {
            if row.conds.len() < MAX_CONDS {
                let c = random_cond(rng, k);
                row.conds.push(c);
            } else {
                let c = rng.below(row.conds.len());
                row.conds.remove(c);
            }
        }
    }
}

/// One-point row splice: `a[..i] ++ b[j..]`, at most 8 rows, never empty.
pub fn crossover(a: &PriorityRule, b: &PriorityRule, rng: &mut XorShift) -> PriorityRule {
    let i = rng.below(a.rows.len() + 1);
    let j = rng.below(b.rows.len() + 1);
    let mut rows: Vec<Row> = a.rows[..i].to_vec();
    rows.extend_from_slice(&b.rows[j..]);
    rows.truncate(MAX_ROWS);
    if rows.is_empty() {
        rows.push(random_row(rng, a.states));
    }
    PriorityRule {
        states: a.states,
        rows,
    }
}

fn tournament(fit: &[f64], rng: &mut XorShift, size: usize) -> usize {
    let mut pick = rng.below(fit.len());
    for _ in 1..size {
        let c = rng.below(fit.len());
        if fit[c] > fit[pick] {
            pick = c;
        }
    }
    pick
}

// ---------------------------------------------------------------------------------------------
// The search

/// Arenas at the verification radii, rooted on the same tile as the GA's arena.
pub struct Arenas {
    pub base: Arena,
    bigger: HashMap<u32, Arena>,
}

impl Arenas {
    pub fn new(spec: &EvolveSpec) -> Result<Arenas, String> {
        Ok(Arenas {
            base: Arena::build_with_room(
                &spec.tiling,
                spec.nb,
                spec.radius,
                spec.radius * spec.verify_factors.iter().copied().max().unwrap_or(1),
            )?,
            bigger: HashMap::new(),
        })
    }
    fn at(&mut self, radius: u32) -> Result<&Arena, String> {
        if radius == self.base.radius {
            return Ok(&self.base);
        }
        if !self.bigger.contains_key(&radius) {
            let a = self.base.grown(radius)?;
            self.bigger.insert(radius, a);
        }
        Ok(&self.bigger[&radius])
    }
}

/// Cross-radius check: the seed must reach the boundary at every radius with exactly the same
/// peak population (no cap; horizon 120 generations per ring of radius).
pub fn cross_check(
    arenas: &mut Arenas,
    rule: &PriorityRule,
    seed_index: usize,
    factors: &[u32],
) -> Result<CrossCheck, String> {
    let r0 = arenas.base.radius;
    let mut radii = Vec::new();
    let mut reason = String::new();
    let mut peak = None;
    let mut skipped = Vec::new();
    for &f in factors {
        let a = match arenas.at(r0 * f.max(1)) {
            Ok(a) => a,
            Err(_) => {
                skipped.push(r0 * f.max(1));
                continue;
            }
        };
        let seeds = a.seeds(rule.states);
        let seed = seeds
            .get(seed_index)
            .ok_or("seed index out of range")?
            .clone();
        let st = a.run(rule, &seed, a.radius as u64 * 120, None);
        radii.push(RadiusCheck {
            radius: a.radius,
            tiles: a.len(),
            outcome: st.outcome,
            generation: st.generation,
            max_population: st.max_population,
        });
        if st.outcome != Outcome::ReachedBoundary {
            reason = format!("radius {}: {}", a.radius, st.outcome.name());
            break;
        }
        if !st.causal {
            reason = format!("radius {}: broke the light cone", a.radius);
            break;
        }
        match peak {
            None => peak = Some(st.max_population),
            Some(p) if p != st.max_population => {
                reason = format!(
                    "radius {}: peak population {} (was {p}): it grows",
                    a.radius, st.max_population
                );
                break;
            }
            _ => {}
        }
    }
    if reason.is_empty() && radii.len() < 2 {
        reason = "fewer than two radii fit on this tiling's largest patch".into();
    }
    let passed = reason.is_empty();
    let mut text = if passed {
        "reached the boundary with a flat peak population at every radius".to_string()
    } else {
        reason
    };
    if !skipped.is_empty() {
        text.push_str(&format!(
            " (radius {} does not fit on the largest patch)",
            skipped
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(CrossCheck {
        passed,
        reason: text,
        radii,
    })
}

struct Eval {
    fitness: f64,
    seed: usize,
    stats: RunStats,
}

/// Run the search. `progress` sees each generation and returns false to stop.
pub fn evolve(
    spec: &EvolveSpec,
    progress: &(dyn Fn(&GenStats) -> bool + Sync),
) -> Result<EvolveReport, String> {
    check(spec)?;
    match spec.threads {
        Some(t) => rayon::ThreadPoolBuilder::new()
            .num_threads(t.max(1))
            .build()
            .map_err(|e| e.to_string())?
            .install(|| run(spec, progress)),
        None => run(spec, progress),
    }
}

fn check(spec: &EvolveSpec) -> Result<(), String> {
    if !(2..=priority::MAX_STATES).contains(&spec.states) {
        return Err(format!("states must be 2 to {}", priority::MAX_STATES));
    }
    if spec.pop < 4 {
        return Err("population must be at least 4".into());
    }
    Ok(())
}

fn run(
    spec: &EvolveSpec,
    progress: &(dyn Fn(&GenStats) -> bool + Sync),
) -> Result<EvolveReport, String> {
    let mut e = Evolution::new(spec)?;
    while e.generation(progress)? {}
    e.finish()
}

/// What a search needs to carry on after its last finished generation (see
/// [`Evolution::checkpoint`]): the bred population, the RNG, the hall of fame, the cross-radius
/// checks so far, every generation's numbers, and the gliders found.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolveCheckpoint {
    /// The next generation to evaluate.
    pub next: usize,
    pub pop: Vec<PriorityRule>,
    pub rng: u64,
    pub hof: BTreeMap<String, Champion>,
    pub checked: BTreeMap<String, CrossCheck>,
    pub generations: Vec<GenStats>,
    pub best_ever: f64,
    pub evaluations: u64,
    pub gliders: Vec<Champion>,
    pub stopped_early: bool,
    /// No generation left to run (the champions are verified next).
    pub over: bool,
    pub elapsed_s: f64,
}

/// A search in progress, one generation at a time: what [`evolve`] runs, and what an evolve job
/// (`jetcarp_search::jobs::evolve`) saves after every generation and resumes from. A resumed search
/// ends exactly as an uninterrupted one; only the memo of evaluated rules starts empty again, so
/// `unique_evaluations` (which counts it) and the timings differ.
pub struct Evolution {
    spec: EvolveSpec,
    arenas: Arenas,
    rng: XorShift,
    pop: Vec<PriorityRule>,
    cache: HashMap<PriorityRule, Arc<Eval>>,
    hof: BTreeMap<String, Champion>,
    checked: HashMap<String, CrossCheck>,
    gens: Vec<GenStats>,
    best_ever: f64,
    evaluations: u64,
    stopped_early: bool,
    gliders: Vec<Champion>,
    next: usize,
    over: bool,
    t0: Instant,
    base_elapsed: f64,
}

impl Evolution {
    /// A new search: the arenas and the first, random population. Runs on the current rayon
    /// pool (`spec.threads` is [`evolve`]'s to apply).
    pub fn new(spec: &EvolveSpec) -> Result<Evolution, String> {
        check(spec)?;
        let t0 = Instant::now();
        let arenas = Arenas::new(spec)?;
        let k = spec.states;
        let mut rng = XorShift::new(spec.seed);
        let pop: Vec<PriorityRule> = (0..spec.pop).map(|_| random_genome(&mut rng, k)).collect();
        Ok(Evolution {
            spec: spec.clone(),
            arenas,
            rng,
            pop,
            cache: HashMap::new(),
            hof: BTreeMap::new(),
            checked: HashMap::new(),
            gens: Vec::new(),
            best_ever: 0.0,
            evaluations: 0,
            stopped_early: false,
            gliders: Vec::new(),
            next: 0,
            over: spec.gens == 0,
            t0,
            base_elapsed: 0.0,
        })
    }

    /// Carry on from a checkpoint of a search with this spec.
    pub fn resume(spec: &EvolveSpec, cp: &EvolveCheckpoint) -> Result<Evolution, String> {
        check(spec)?;
        if cp.pop.len() != spec.pop {
            return Err(format!(
                "the checkpoint's population ({}) does not match the spec's ({})",
                cp.pop.len(),
                spec.pop
            ));
        }
        Ok(Evolution {
            spec: spec.clone(),
            arenas: Arenas::new(spec)?,
            rng: XorShift(cp.rng),
            pop: cp.pop.clone(),
            cache: HashMap::new(),
            hof: cp.hof.clone(),
            checked: cp.checked.clone().into_iter().collect(),
            gens: cp.generations.clone(),
            best_ever: cp.best_ever,
            evaluations: cp.evaluations,
            stopped_early: cp.stopped_early,
            gliders: cp.gliders.clone(),
            next: cp.next,
            over: cp.over,
            t0: Instant::now(),
            base_elapsed: cp.elapsed_s,
        })
    }

    /// The state after the last finished generation.
    pub fn checkpoint(&self) -> EvolveCheckpoint {
        EvolveCheckpoint {
            next: self.next,
            pop: self.pop.clone(),
            rng: self.rng.0,
            hof: self.hof.clone(),
            checked: self
                .checked
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            generations: self.gens.clone(),
            best_ever: self.best_ever,
            evaluations: self.evaluations,
            gliders: self.gliders.clone(),
            stopped_early: self.stopped_early,
            over: self.over,
            elapsed_s: self.elapsed_s(),
        }
    }

    pub fn spec(&self) -> &EvolveSpec {
        &self.spec
    }
    /// Every finished generation's numbers.
    pub fn generations(&self) -> &[GenStats] {
        &self.gens
    }
    /// No generation left: [`finish`](Self::finish) is next.
    pub fn is_over(&self) -> bool {
        self.over
    }
    /// Gliders verified so far (with `stop_on_glider`; the rest are found by `finish`).
    pub fn gliders(&self) -> &[Champion] {
        &self.gliders
    }
    /// Seconds run, over every session.
    pub fn elapsed_s(&self) -> f64 {
        self.base_elapsed + self.t0.elapsed().as_secs_f64()
    }

    /// Run the next generation: evaluate, rank, record, and breed the next population.
    /// `progress` sees the generation's numbers and returns false to stop the search there
    /// (stopped early). Returns whether there is another generation to run.
    pub fn generation(
        &mut self,
        progress: &(dyn Fn(&GenStats) -> bool + Sync),
    ) -> Result<bool, String> {
        use rayon::prelude::*;
        if self.over {
            return Ok(false);
        }
        let spec = &self.spec;
        let g = self.next;
        let k = spec.states;
        let elite = (spec.pop / 16).max(2);
        let ceiling = 0.9 * self.arenas.base.boundary as f64;
        let hof_min = 0.75 * self.arenas.base.boundary as f64;
        let pop = &self.pop;
        // Evaluate (distinct behaviours only, in parallel, gathered in order).
        let simple: Vec<PriorityRule> = pop.iter().map(|r| r.simplified()).collect();
        let mut todo: Vec<PriorityRule> = Vec::new();
        for s in &simple {
            if !self.cache.contains_key(s) && !todo.contains(s) {
                todo.push(s.clone());
            }
        }
        let base = &self.arenas.base;
        let done: Vec<Eval> = todo
            .par_iter()
            .map(|r| {
                let (fitness, seed, stats) = base.fitness(r, spec.horizon, spec.pop_cap);
                Eval {
                    fitness,
                    seed,
                    stats,
                }
            })
            .collect();
        for (r, e) in todo.into_iter().zip(done) {
            self.cache.insert(r, Arc::new(e));
        }
        self.evaluations += pop.len() as u64;
        let evals: Vec<Arc<Eval>> = simple.iter().map(|s| self.cache[s].clone()).collect();
        // Hall of fame.
        let mut fresh = Vec::new();
        for (i, e) in evals.iter().enumerate() {
            if e.fitness >= hof_min {
                let key = simple[i].notation();
                if !self.hof.contains_key(&key) {
                    let seed = base.seeds(k)[e.seed].clone();
                    self.hof.insert(
                        key.clone(),
                        Champion {
                            rule: key.clone(),
                            genome: pop[i].notation(),
                            fitness: e.fitness,
                            seed_index: e.seed,
                            seed,
                            stats: e.stats.clone(),
                            first_generation: g,
                            verify: None,
                        },
                    );
                    if e.fitness >= ceiling {
                        fresh.push((simple[i].clone(), key));
                    }
                }
            }
        }
        if self.cache.len() > 200_000 {
            self.cache.clear();
        }
        // Order by fitness (stable: ties keep their order).
        let mut idx: Vec<usize> = (0..pop.len()).collect();
        idx.sort_by(|&a, &b| evals[b].fitness.total_cmp(&evals[a].fitness));
        let pop_sorted: Vec<PriorityRule> = idx.iter().map(|&i| pop[i].clone()).collect();
        let fit: Vec<f64> = idx.iter().map(|&i| evals[i].fitness).collect();
        self.best_ever = self.best_ever.max(fit[0]);
        let mut sorted = fit.clone();
        sorted.sort_by(f64::total_cmp);
        let median = if sorted.len() % 2 == 1 {
            sorted[sorted.len() / 2]
        } else {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        };
        let mut distinct = simple.clone();
        distinct.sort_by_key(|r| r.notation());
        distinct.dedup();
        let st = GenStats {
            generation: g,
            best: fit[0],
            median,
            mean: fit.iter().sum::<f64>() / fit.len() as f64,
            best_ever: self.best_ever,
            at_ceiling: fit.iter().filter(|&&f| f >= ceiling).count(),
            distinct: distinct.len(),
            evaluations: self.evaluations,
            unique_evaluations: self.cache.len() as u64,
            elapsed_s: self.base_elapsed + self.t0.elapsed().as_secs_f64(),
        };
        let go_on = progress(&st);
        self.gens.push(st);
        if spec.stop_on_glider {
            for (rule, key) in fresh.into_iter().take(8) {
                let seed = self.hof[&key].seed_index;
                let cc = cross_check(&mut self.arenas, &rule, seed, &spec.verify_factors)?;
                let passed = cc.passed;
                self.checked.insert(key.clone(), cc.clone());
                if passed {
                    let mut c = self.hof[&key].clone();
                    c.verify = Some(cc);
                    self.gliders.push(c);
                }
            }
            if !self.gliders.is_empty() {
                self.stopped_early = g + 1 < spec.gens;
                self.over = true;
                return Ok(false);
            }
        }
        if !go_on {
            self.stopped_early = true;
            self.over = true;
            return Ok(false);
        }
        if g + 1 == spec.gens {
            self.over = true;
            return Ok(false);
        }
        // Breed.
        let rng = &mut self.rng;
        let mut next: Vec<PriorityRule> = pop_sorted[..elite.min(pop_sorted.len())].to_vec();
        while next.len() < spec.pop {
            let a = &pop_sorted[tournament(&fit, rng, 3)];
            let b = &pop_sorted[tournament(&fit, rng, 3)];
            let mut child = crossover(a, b, rng);
            if rng.below(2) == 0 {
                mutate(&mut child, rng);
            }
            if rng.below(2) == 0 {
                mutate(&mut child, rng);
            }
            next.push(child);
        }
        self.pop = next;
        self.next = g + 1;
        Ok(true)
    }

    /// Verify the champions (distinct top genomes, best first, earliest on ties) and make the
    /// report.
    pub fn finish(mut self) -> Result<EvolveReport, String> {
        let spec = &self.spec;
        let mut champs: Vec<Champion> = std::mem::take(&mut self.hof).into_values().collect();
        champs.sort_by(|a, b| {
            b.fitness
                .total_cmp(&a.fitness)
                .then(a.first_generation.cmp(&b.first_generation))
                .then(a.rule.cmp(&b.rule))
        });
        champs.truncate(spec.champions);
        for c in &mut champs {
            if let Some(cc) = self.checked.get(&c.rule) {
                c.verify = Some(cc.clone());
                continue;
            }
            let rule = PriorityRule::parse(&c.rule).unwrap()?;
            let cc = cross_check(&mut self.arenas, &rule, c.seed_index, &spec.verify_factors)?;
            c.verify = Some(cc);
        }
        let mut gliders = std::mem::take(&mut self.gliders);
        for c in &champs {
            if c.verify.as_ref().is_some_and(|v| v.passed)
                && !gliders.iter().any(|g| g.rule == c.rule)
            {
                gliders.push(c.clone());
            }
        }
        Ok(EvolveReport {
            spec: spec.clone(),
            jetcarp_version: crate::VERSION.into(),
            arena_tiles: self.arenas.base.len(),
            arena_tiling: self.arenas.base.tiling.clone(),
            margin: self.arenas.base.margin,
            boundary: self.arenas.base.boundary,
            generations: std::mem::take(&mut self.gens),
            champions: champs,
            gliders,
            stopped_early: self.stopped_early,
            elapsed_s: self.elapsed_s(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_respect_bounds() {
        let mut rng = XorShift::new(7);
        let mut g = random_genome(&mut rng, 4);
        for _ in 0..5000 {
            let h = random_genome(&mut rng, 4);
            g = crossover(&g, &h, &mut rng);
            mutate(&mut g, &mut rng);
            assert!(!g.rows.is_empty() && g.rows.len() <= MAX_ROWS);
            for r in &g.rows {
                assert!(r.conds.len() <= MAX_CONDS);
                assert!(r.next < 4 && r.own.is_none_or(|o| o < 4));
                assert!(
                    r.conds
                        .iter()
                        .all(|&(s, m)| s < 4 && (1..=MAX_MIN).contains(&m))
                );
            }
        }
    }
}
