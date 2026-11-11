//! The search itself ([`Search`]): MAP-Elites over tiling, rule family, rule, and seed. A step
//! runs GPU rounds on a thread of its own (for the rules a [`Backend`] can run) beside CPU rounds
//! on the run's thread pool (the rest, or everything when there is no GPU), with periodic
//! confirmation of the best runs on bigger grids, until its deadline; then
//! [`take_delta`](Search::take_delta) hands over what it measured and changed, which the job
//! writes to the search's working database (`super::store`) in one transaction.
//!
//! A search starts from a saved state ([`store::Saved`]: its archive, confirmed set, coverage,
//! family totals, seen set, counters, and random states), so a stopped one carries on where it
//! was. It is not deterministic: the GPU and CPU rounds share the archive as they run, so two
//! searches with the same settings try different candidates.

use super::archive::{self, Archive, Eval};
use super::batch::{Backend, BatchJob, BatchWorld, WorldTrace, trace_world};
use super::cand::Cand;
use super::features::{Fate, Features, frames_for, measure_run};
use super::seeds::seed_for;
use super::space::{self, Fam, Grid, TilingChoice, sample_ltl, sample_rule};
use super::store::{Delta, FamStat, OldRun, RunOut, Saved};
use crate::ltl::Hood;
use crate::rng::Rng;
use crate::rules::Rule;
use crate::topology::Nb;
use crate::world::World;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use super::store::feat_from_blob;

fn d_screen_cells() -> u32 {
    16_000
}
fn d_screen_gens() -> u32 {
    1024
}
fn d_confirm_cells() -> u32 {
    64_000
}
fn d_confirm_gens() -> u32 {
    4096
}
fn d_confirm_seeds() -> u32 {
    3
}
fn d_batch() -> usize {
    1024
}

/// A search's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Cells of a screening run's grid, and its generations.
    #[serde(default = "d_screen_cells")]
    pub screen_cells: u32,
    #[serde(default = "d_screen_gens")]
    pub screen_gens: u32,
    /// Cells and generations of a confirmation run, and how many seeds confirm an elite.
    #[serde(default = "d_confirm_cells")]
    pub confirm_cells: u32,
    #[serde(default = "d_confirm_gens")]
    pub confirm_gens: u32,
    #[serde(default = "d_confirm_seeds")]
    pub confirm_seeds: u32,
    /// Most worlds in a GPU batch.
    #[serde(default = "d_batch")]
    pub batch: usize,
    /// Where the random draws start (a search carries its random states on from there).
    #[serde(default)]
    pub seed: u64,
    /// Only these tilings (keys: `square`, `ih28`, `catalog:k2-001`, ...); empty: all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tilings: Vec<String>,
    /// Only these rule families; empty: all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub families: Vec<Fam>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            screen_cells: d_screen_cells(),
            screen_gens: d_screen_gens(),
            confirm_cells: d_confirm_cells(),
            confirm_gens: d_confirm_gens(),
            confirm_seeds: d_confirm_seeds(),
            batch: d_batch(),
            seed: 0,
            tilings: vec![],
            families: vec![],
        }
    }
}

impl Settings {
    /// Can a search run with these?
    pub fn check(&self) -> Result<(), String> {
        self.check_in(&space::all_tilings())
    }

    /// [`check`](Self::check) against this tiling list.
    fn check_in(&self, all: &[TilingChoice]) -> Result<(), String> {
        if self.screen_cells < 64 || self.confirm_cells < 64 {
            return Err("grids need at least 64 cells".into());
        }
        if self.screen_gens == 0 || self.confirm_gens == 0 {
            return Err("runs need at least one generation".into());
        }
        if self.batch == 0 {
            return Err("a GPU batch needs at least one world".into());
        }
        for t in &self.tilings {
            if !all.iter().any(|x| &x.key == t) {
                return Err(format!(
                    "no tiling '{t}' in the search's list (keys: square, hex, trihex, spectre, ih28, catalog:k2-001, ...)"
                ));
            }
        }
        let (g, c, ltl) = families(&self.families, true);
        if g.is_empty() && c.is_empty() && !ltl {
            return Err("no rule family to search".into());
        }
        Ok(())
    }
}

const WINDOW: usize = 17;
/// Neighbour reads one Larger than Life batch may do (a few seconds of GPU time).
const LTL_NEIGHBOUR_UPDATES: f64 = 1.5e12;
/// Seen identities kept in a hash set before they are merged into the sorted list.
const FRESH_MAX: usize = 1 << 20;

/// GPU sampling weights.
const GPU_FAMS: &[(Fam, f64)] = &[
    (Fam::LifeLike, 30.0),
    (Fam::Generations, 25.0),
    (Fam::Jaca, 10.0),
    (Fam::Cyclic, 4.0),
    (Fam::Rps, 4.0),
    (Fam::GreenbergHastings, 4.0),
    (Fam::Hodgepodge, 4.0),
    (Fam::Priority, 8.0),
    (Fam::WireWorld, 1.0),
    (Fam::SpectreGlider, 1.0),
];

/// CPU sampling weights.
const CPU_FAMS: &[(Fam, f64)] = &[
    (Fam::Turmite, 25.0),
    (Fam::Layered, 20.0),
    (Fam::Wolfram, 8.0),
    (Fam::HexWolfram, 8.0),
    (Fam::HexTri, 8.0),
    (Fam::Margolus, 20.0),
    (Fam::Lenia, 3.0),
    (Fam::ForestFire, 3.0),
    (Fam::Sandpile, 4.0),
    (Fam::Table, 1.0),
];

/// The GPU loop's families, the CPU loop's (with a GPU, or without one: then it also takes the
/// GPU's), and whether Larger than Life rounds run (on the GPU), within `only` (empty: all).
fn families(only: &[Fam], gpu: bool) -> (Vec<(Fam, f64)>, Vec<(Fam, f64)>, bool) {
    let keep = |f: &Fam| only.is_empty() || only.contains(f);
    let g: Vec<(Fam, f64)> = GPU_FAMS.iter().copied().filter(|x| keep(&x.0)).collect();
    let mut c: Vec<(Fam, f64)> = CPU_FAMS.iter().copied().filter(|x| keep(&x.0)).collect();
    if !gpu {
        c.extend(g.iter().copied());
    }
    (g, c, keep(&Fam::Ltl))
}

fn weighted(rng: &mut Rng, w: &[(Fam, f64)]) -> Fam {
    let t: f64 = w.iter().map(|x| x.1).sum();
    let mut r = rng.next_f64() * t;
    for (f, v) in w {
        r -= v;
        if r <= 0.0 {
            return *f;
        }
    }
    w[0].0
}

/// The identities tried: a sorted list (loaded) and a set of the ones tried since.
#[derive(Default)]
struct Seen {
    base: Vec<u64>,
    fresh: HashSet<u64>,
    /// Tried since the last delta.
    new: Vec<u64>,
}

impl Seen {
    fn insert(&mut self, h: u64) -> bool {
        if self.base.binary_search(&h).is_ok() || !self.fresh.insert(h) {
            return false;
        }
        self.new.push(h);
        if self.fresh.len() > FRESH_MAX {
            let mut add: Vec<u64> = self.fresh.drain().collect();
            add.sort_unstable();
            let old = std::mem::take(&mut self.base);
            let mut out = Vec::with_capacity(old.len() + add.len());
            let (mut i, mut j) = (0, 0);
            while i < old.len() || j < add.len() {
                if j >= add.len() || (i < old.len() && old[i] < add[j]) {
                    out.push(old[i]);
                    i += 1;
                } else {
                    out.push(add[j]);
                    j += 1;
                }
            }
            self.base = out;
        }
        true
    }
    fn len(&self) -> usize {
        self.base.len() + self.fresh.len()
    }
}

/// What a step has measured and changed, until [`Search::take_delta`].
#[derive(Default)]
struct Pending {
    rows: Vec<RunOut>,
    coverage: HashMap<(String, Fam, String), u32>,
    families: HashMap<Fam, FamStat>,
    confirmed: Vec<i64>,
}

/// One family's totals, for reports.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyLine {
    pub family: String,
    /// Screened runs, their mean and best score, and the elites of the family.
    pub runs: u64,
    pub mean: f64,
    pub best: f64,
    pub elites: u64,
}

/// When a step ends.
pub struct StepLimits<'a> {
    /// The step's deadline (a round under way runs to its end).
    pub until: Instant,
    pub stop: &'a AtomicBool,
    /// Stop once this many runs have been screened in all.
    pub max_screened: Option<u64>,
}

/// A live search.
pub struct Search {
    set: Settings,
    tilings: Vec<TilingChoice>,
    tiling_total: f64,
    grids: Mutex<HashMap<(String, u32), Arc<Grid>>>,
    archive: Mutex<Archive>,
    seen: Mutex<Seen>,
    coverage: Mutex<HashMap<(usize, Fam, String), u32>>,
    family_totals: Mutex<HashMap<Fam, FamStat>>,
    named: Mutex<Vec<(usize, String)>>,
    next_id: AtomicI64,
    screened: AtomicU64,
    confirmed: AtomicU64,
    pending: Mutex<Pending>,
    rng_gpu: Mutex<Rng>,
    rng_cpu: Mutex<Rng>,
    gpu_rounds: AtomicU64,
    cpu_rounds: AtomicU64,
    panics: AtomicU64,
}

impl Search {
    /// A search with these settings, carrying on from `saved` (a default one: from the start).
    pub fn new(set: Settings, mut saved: Saved) -> Result<Search, String> {
        let mut tilings = space::all_tilings();
        set.check_in(&tilings)?;
        if !set.tilings.is_empty() {
            tilings.retain(|t| set.tilings.contains(&t.key));
        }
        let tiling_total: f64 = tilings.iter().map(|t| t.weight).sum();
        let mut named = Vec::new();
        for (i, t) in tilings.iter().enumerate() {
            if t.twin_of.is_none() {
                for e in crate::rules::catalog() {
                    named.push((i, e.notation.to_string()));
                }
            }
        }
        let index: HashMap<&str, usize> = tilings
            .iter()
            .enumerate()
            .map(|(i, t)| (t.key.as_str(), i))
            .collect();
        let mut coverage = HashMap::new();
        for (t, f, k, n) in std::mem::take(&mut saved.coverage) {
            if let Some(&i) = index.get(t.as_str()) {
                *coverage.entry((i, f, k)).or_insert(0) += n;
            }
        }
        let rng = |k: &str, x: u64| {
            Rng::new(
                saved
                    .meta
                    .get(k)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(set.seed ^ x),
            )
        };
        Ok(Search {
            rng_gpu: Mutex::new(rng("rng_gpu", 0xABCD)),
            rng_cpu: Mutex::new(rng("rng_cpu", 0x1234)),
            screened: AtomicU64::new(saved.meta_u64("screened")),
            confirmed: AtomicU64::new(saved.meta_u64("confirmed")),
            next_id: AtomicI64::new(saved.next_id.max(1)),
            set,
            tiling_total,
            tilings,
            grids: Mutex::new(HashMap::new()),
            archive: Mutex::new(saved.archive),
            seen: Mutex::new(Seen {
                base: saved.seen,
                ..Default::default()
            }),
            coverage: Mutex::new(coverage),
            family_totals: Mutex::new(saved.families),
            named: Mutex::new(named),
            pending: Mutex::new(Pending::default()),
            gpu_rounds: AtomicU64::new(0),
            cpu_rounds: AtomicU64::new(0),
            panics: AtomicU64::new(0),
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.set
    }
    /// Is there anything to run, with a GPU or without one? (Larger than Life runs on the GPU
    /// only.)
    pub fn can_run(&self, gpu: bool) -> Result<(), String> {
        let (g, c, ltl) = families(&self.set.families, gpu);
        if c.is_empty() && !(gpu && (ltl || !g.is_empty())) {
            return Err(if gpu {
                "no rule family to search".into()
            } else {
                "these families run on the GPU only (Larger than Life): use the GPU".into()
            });
        }
        Ok(())
    }
    /// Runs screened, in all.
    pub fn screened(&self) -> u64 {
        self.screened.load(Ordering::Relaxed)
    }
    /// Confirmation runs, in all.
    pub fn confirmed(&self) -> u64 {
        self.confirmed.load(Ordering::Relaxed)
    }
    /// Elites, and the niches they are in.
    pub fn elites(&self) -> (u64, u64) {
        let a = self.archive.lock().unwrap();
        (a.runs.len() as u64, a.cells.len() as u64)
    }
    /// Elites confirmed on bigger grids.
    pub fn confirmed_elites(&self) -> u64 {
        let a = self.archive.lock().unwrap();
        a.runs.keys().filter(|id| a.confirmed.contains(id)).count() as u64
    }
    /// Identities tried.
    pub fn seen(&self) -> u64 {
        self.seen.lock().unwrap().len() as u64
    }
    /// Distinct tilings with screened runs (of the search's list).
    pub fn tilings_tried(&self) -> u64 {
        let c = self.coverage.lock().unwrap();
        c.keys().map(|k| k.0).collect::<HashSet<_>>().len() as u64
    }
    /// The tilings the search draws from.
    pub fn tiling_count(&self) -> usize {
        self.tilings.len()
    }
    /// Candidates whose run panicked (skipped).
    pub fn panics(&self) -> u64 {
        self.panics.load(Ordering::Relaxed)
    }
    /// Per family: screened runs, mean and best score, elites; most runs first.
    pub fn families(&self) -> Vec<FamilyLine> {
        let mut elites: HashMap<Fam, u64> = HashMap::new();
        for e in self.archive.lock().unwrap().runs.values() {
            *elites.entry(e.cand.fam).or_insert(0) += 1;
        }
        let mut v: Vec<FamilyLine> = self
            .family_totals
            .lock()
            .unwrap()
            .iter()
            .map(|(f, s)| FamilyLine {
                family: f.name().into(),
                runs: s.runs,
                mean: s.mean(),
                best: s.best,
                elites: elites.get(f).copied().unwrap_or(0),
            })
            .collect();
        v.sort_by(|a, b| b.runs.cmp(&a.runs).then(a.family.cmp(&b.family)));
        v
    }

    /// What was measured and changed since the last call: the runs, the identities tried, the
    /// coverage and family totals added, the elites added and dropped, the elites confirmed, and
    /// the counters and random states (`meta`).
    pub fn take_delta(&self) -> Delta {
        let p = std::mem::take(&mut *self.pending.lock().unwrap());
        let elites = self.archive.lock().unwrap().take_changes();
        let seen = std::mem::take(&mut self.seen.lock().unwrap().new);
        let meta = vec![
            ("screened".to_string(), self.screened().to_string()),
            ("confirmed".to_string(), self.confirmed().to_string()),
            (
                "rng_gpu".to_string(),
                self.rng_gpu.lock().unwrap().state.to_string(),
            ),
            (
                "rng_cpu".to_string(),
                self.rng_cpu.lock().unwrap().state.to_string(),
            ),
        ];
        Delta {
            rows: p.rows,
            seen,
            coverage: p.coverage,
            families: p.families,
            elites,
            confirmed: p.confirmed,
            meta,
        }
    }

    /// Take in runs of an old database being imported: their identities, and for the screened
    /// ones coverage, family totals, and the archive; the confirmation runs mark their elites
    /// confirmed. (Their rows are copied as they are.)
    pub fn import(&self, rows: &[OldRun]) {
        let mut a = self.archive.lock().unwrap();
        let mut seen = self.seen.lock().unwrap();
        let mut cov = self.coverage.lock().unwrap();
        let mut fams = self.family_totals.lock().unwrap();
        let mut p = self.pending.lock().unwrap();
        for r in rows {
            self.next_id.fetch_max(r.id + 1, Ordering::Relaxed);
            let Some(ev) = &r.ev else { continue };
            seen.insert(ev.cand.id_hash());
            if r.stage != 1 {
                self.confirmed.fetch_add(1, Ordering::Relaxed);
                if let Some(parent) = r.parent
                    && a.confirmed.insert(parent)
                {
                    p.confirmed.push(parent);
                }
                continue;
            }
            self.screened.fetch_add(1, Ordering::Relaxed);
            let kind = r.seed_kind.clone().unwrap_or_default();
            let c = &ev.cand;
            if let Some(i) = self.tiling_index(&c.tiling) {
                *cov.entry((i, c.fam, kind.clone())).or_insert(0) += 1;
            }
            *p.coverage
                .entry((c.tiling.clone(), c.fam, kind))
                .or_insert(0) += 1;
            fams.entry(c.fam).or_default().add(ev.score as f64);
            p.families.entry(c.fam).or_default().add(ev.score as f64);
            a.offer(r.id, ev.clone(), self.group(&c.tiling));
        }
    }

    /// The group (0-7) of a tiling, for the archive's niches.
    pub fn group(&self, key: &str) -> u8 {
        group_of(&self.tilings, key)
    }

    fn tiling_index(&self, key: &str) -> Option<usize> {
        self.tilings.iter().position(|t| t.key == key)
    }

    fn grid(&self, key: &str, cells: u32) -> Result<Arc<Grid>, String> {
        if let Some(g) = self.grids.lock().unwrap().get(&(key.to_string(), cells)) {
            return Ok(g.clone());
        }
        let g = Arc::new(Grid::build(key, cells)?);
        self.grids
            .lock()
            .unwrap()
            .insert((key.to_string(), cells), g.clone());
        Ok(g)
    }

    fn pick_tiling(&self, rng: &mut Rng) -> usize {
        let mut r = rng.next_f64() * self.tiling_total;
        for (i, t) in self.tilings.iter().enumerate() {
            r -= t.weight;
            if r <= 0.0 {
                return i;
            }
        }
        0
    }

    /// A distinct tiling with the fewest screened runs of `fams`; ties broken at random.
    fn least_tried_tiling(&self, rng: &mut Rng, fams: &[(Fam, f64)]) -> usize {
        let mut per = vec![0u64; self.tilings.len()];
        for ((ti, fam, _), n) in self.coverage.lock().unwrap().iter() {
            if fams.iter().any(|(f, _)| f == fam) {
                per[*ti] += *n as u64;
            }
        }
        let len = self.tilings.len();
        let start = rng.below(len as u64) as usize;
        (0..len)
            .map(|i| (start + i) % len)
            .filter(|&i| self.tilings[i].twin_of.is_none())
            .min_by_key(|&i| per[i])
            .unwrap_or(0)
    }

    /// The family in `fams` with the fewest screened runs.
    fn least_tried_fam(&self, fams: &[Fam]) -> Fam {
        let mut per: HashMap<Fam, u64> = HashMap::new();
        for ((_, fam, _), n) in self.coverage.lock().unwrap().iter() {
            *per.entry(*fam).or_insert(0) += *n as u64;
        }
        *fams
            .iter()
            .min_by_key(|f| per.get(f).copied().unwrap_or(0))
            .expect("at least one family")
    }

    fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn publish(&self, rows: Vec<RunOut>, stage: u8) {
        let mut p = self.pending.lock().unwrap();
        if stage == 1 {
            self.screened
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
            let mut a = self.archive.lock().unwrap();
            let mut cov = self.coverage.lock().unwrap();
            let mut fams = self.family_totals.lock().unwrap();
            for r in &rows {
                let c = &r.ev.cand;
                if let Some(i) = self.tiling_index(&c.tiling) {
                    *cov.entry((i, c.fam, r.seed_kind.clone())).or_insert(0) += 1;
                }
                *p.coverage
                    .entry((c.tiling.clone(), c.fam, r.seed_kind.clone()))
                    .or_insert(0) += 1;
                fams.entry(c.fam).or_default().add(r.ev.score as f64);
                p.families.entry(c.fam).or_default().add(r.ev.score as f64);
                a.offer(r.id, r.ev.clone(), self.group(&c.tiling));
            }
        } else {
            self.confirmed
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
        }
        p.rows.extend(rows);
    }

    fn mark_confirmed(&self, ids: &[i64]) {
        let mut p = self.pending.lock().unwrap();
        let mut a = self.archive.lock().unwrap();
        for id in ids {
            if a.confirmed.insert(*id) {
                p.confirmed.push(*id);
            }
        }
    }

    /// One step: GPU rounds (with `gpu`) on a thread of their own and CPU rounds on `pool`,
    /// until the deadline, a stop, or the screened total; a round under way runs to its end.
    /// Returns the GPU's error, when a GPU round failed (the GPU stops for the rest of the step).
    pub fn step(
        &self,
        pool: &rayon::ThreadPool,
        gpu: Option<&mut dyn Backend>,
        lim: &StepLimits<'_>,
    ) -> Option<String> {
        let go = || {
            !lim.stop.load(Ordering::SeqCst)
                && Instant::now() < lim.until
                && lim.max_screened.is_none_or(|m| self.screened() < m)
        };
        let has_gpu = gpu.is_some();
        let (gpu_fams, cpu_fams, ltl) = families(&self.set.families, has_gpu);
        std::thread::scope(|sc| {
            let gpu_thread = gpu.map(|be| {
                let gpu_fams = gpu_fams.clone();
                sc.spawn(move || {
                    if gpu_fams.is_empty() && !ltl {
                        return None;
                    }
                    let mut rng = *self.rng_gpu.lock().unwrap();
                    let keep = |rng: Rng| *self.rng_gpu.lock().unwrap() = rng;
                    while go() {
                        let round = self.gpu_rounds.fetch_add(1, Ordering::Relaxed) + 1;
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            if round.is_multiple_of(8) {
                                self.gpu_confirm(be, 96).map(|_| ())
                            } else {
                                self.gpu_round(be, &mut rng, &gpu_fams, ltl)
                            }
                        }));
                        match r {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => {
                                keep(rng);
                                return Some(e);
                            }
                            Err(_) => {
                                keep(rng);
                                self.panics.fetch_add(1, Ordering::Relaxed);
                                return Some("a GPU round panicked".into());
                            }
                        }
                    }
                    keep(rng);
                    None
                })
            });
            if !cpu_fams.is_empty() {
                let mut rng = *self.rng_cpu.lock().unwrap();
                pool.install(|| {
                    while go() {
                        self.cpu_round(&mut rng, &cpu_fams);
                    }
                });
                *self.rng_cpu.lock().unwrap() = rng;
            }
            gpu_thread.and_then(|t| t.join().unwrap_or(Some("the GPU thread panicked".into())))
        })
    }

    /// Make candidates for a grid. `ltl` fixes a Larger than Life group: only those are made.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        grid: &Grid,
        nb: Nb,
        fams: &[(Fam, f64)],
        ltl: Option<(Hood, u8)>,
        n: usize,
        gens: u32,
        rng: &mut Rng,
        backend: Option<&dyn Fn(&Rule) -> bool>,
    ) -> Vec<(Cand, String)> {
        let mut out: Vec<(Cand, String)> = Vec::with_capacity(n);
        let Some(ti) = self.tiling_index(&grid.key) else {
            return out;
        };
        let mut tries = 0;
        while out.len() < n && tries < n * 12 {
            tries += 1;
            let fam = if ltl.is_some() {
                Fam::Ltl
            } else {
                weighted(rng, fams)
            };
            if !fam.fits(grid) {
                continue;
            }
            let roll = rng.next_f64();
            let (rule, kind, seed, origin): (Option<Rule>, String, crate::world::SeedSpec, &str) =
                if roll < 0.4 {
                    let r = match ltl {
                        Some(g) => sample_ltl(grid, rng, Some(g)),
                        None => sample_rule(fam, grid, nb, rng),
                    };
                    let (sd, k) = seed_for(fam, grid, rng);
                    (r, k.to_string(), sd, "random")
                } else if roll < 0.65 {
                    let el = self
                        .archive
                        .lock()
                        .unwrap()
                        .sample(rng, Some(fam))
                        .map(|(_, e)| e.clone());
                    let Some(el) = el else { continue };
                    let Some(base) = el.cand.rule() else { continue };
                    if let Some(g) = ltl
                        && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                    {
                        continue;
                    }
                    let r = space::mutate_rule(&base, grid, nb, rng);
                    (r, "mutated".to_string(), el.cand.seed.clone(), "mutate")
                } else if roll < 0.8 {
                    let el = self
                        .archive
                        .lock()
                        .unwrap()
                        .sample(rng, Some(fam))
                        .map(|(_, e)| e.clone());
                    let Some(el) = el else { continue };
                    let Some(base) = el.cand.rule() else { continue };
                    if let Some(g) = ltl
                        && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                    {
                        continue;
                    }
                    let (sd, k) = seed_for(fam, grid, rng);
                    (Some(base), k.to_string(), sd, "seed")
                } else if roll < 0.9 {
                    // The least-tried seed kind of this family on this grid.
                    let mut best: Option<(u32, crate::world::SeedSpec, &'static str)> = None;
                    let cov = self.coverage.lock().unwrap();
                    for _ in 0..6 {
                        let (sd, k) = seed_for(fam, grid, rng);
                        let c = cov.get(&(ti, fam, k.to_string())).copied().unwrap_or(0);
                        if best.as_ref().is_none_or(|b| c < b.0) {
                            best = Some((c, sd, k));
                        }
                    }
                    drop(cov);
                    let (_, sd, k) = best.unwrap();
                    let r = match ltl {
                        Some(g) => sample_ltl(grid, rng, Some(g)),
                        None => sample_rule(fam, grid, nb, rng),
                    };
                    (r, k.to_string(), sd, "cover")
                } else {
                    // A named rule on this grid.
                    let named = {
                        let mut q = self.named.lock().unwrap();
                        if q.is_empty() {
                            continue;
                        }
                        let i = rng.below(q.len() as u64) as usize;
                        q.swap_remove(i)
                    };
                    let Ok((r, _)) = Rule::parse(&named.1) else {
                        continue;
                    };
                    let f2 = Fam::of(&r);
                    // A rule this loop cannot run goes back for the other loop.
                    if ltl.is_some()
                        || backend.is_some_and(|b| !b(&r))
                        || !fams.iter().any(|(f, _)| *f == f2)
                    {
                        self.named.lock().unwrap().push(named);
                        continue;
                    }
                    if !f2.fits(grid) {
                        continue;
                    }
                    let (sd, k) = seed_for(f2, grid, rng);
                    (Some(r), k.to_string(), sd, "named")
                };
            let Some(rule) = rule else { continue };
            if backend.is_some_and(|b| !b(&rule)) {
                continue;
            }
            let fam = Fam::of(&rule);
            let (g2, steps) = run_len(fam, grid, gens);
            let cand = Cand {
                tiling: grid.key.clone(),
                nb: if fam.nb_matters() { nb } else { Nb::Touch },
                fam,
                rule: rule.notation(),
                seed,
                cells: grid.cells_target,
                gens: g2,
                steps,
                rng: rng.next_u64() >> 8,
                origin: origin.to_string(),
            };
            if !self.seen.lock().unwrap().insert(cand.id_hash()) {
                continue;
            }
            out.push((cand, kind));
        }
        out
    }

    /// Run one GPU batch of candidates on a grid.
    fn gpu_batch(
        &self,
        be: &mut dyn Backend,
        grid: &Grid,
        nb: Nb,
        cands: Vec<(Cand, String, Option<i64>)>,
        stage: u8,
    ) -> Result<(), String> {
        if cands.is_empty() {
            return Ok(());
        }
        let gens = cands[0].0.gens;
        let trace = std::env::var_os("JETCARP_EXPLORE_TRACE").is_some();
        let t0 = Instant::now();
        let inits: Vec<Option<BatchWorld>> = cands
            .par_iter()
            .map(|(c, ..)| {
                catch_unwind(AssertUnwindSafe(|| {
                    let world = prepare(grid, c).ok()?;
                    let rule = c.rule()?;
                    Some(BatchWorld {
                        rule,
                        init: super::batch::field_bytes(world.fields.first()?),
                    })
                }))
                .unwrap_or_else(|_| {
                    self.panics.fetch_add(1, Ordering::Relaxed);
                    None
                })
            })
            .collect();
        let (mut good, mut worlds) = (Vec::new(), Vec::new());
        for (c, w) in cands.into_iter().zip(inits) {
            if let Some(w) = w {
                good.push(c);
                worlds.push(w);
            }
        }
        if worlds.is_empty() {
            return Ok(());
        }
        let job = BatchJob {
            topo: grid.topo.clone(),
            nb,
            worlds,
            gens,
            frames_at: frames_for(gens, WINDOW),
        };
        let t1 = Instant::now();
        let traces = be.run(&job)?;
        if traces.len() != good.len() {
            return Err(format!(
                "the GPU returned {} traces for {} worlds",
                traces.len(),
                good.len()
            ));
        }
        let t2 = Instant::now();
        let rows: Vec<RunOut> = good
            .into_par_iter()
            .zip(traces.into_par_iter())
            .filter_map(|((c, kind, parent), t)| {
                let (fate, feat, score, bin) =
                    catch_unwind(AssertUnwindSafe(|| evaluate(grid, &c, &t)))
                        .map_err(|_| self.panics.fetch_add(1, Ordering::Relaxed))
                        .ok()?;
                Some(RunOut {
                    id: self.next_id(),
                    stage,
                    parent,
                    ev: Eval {
                        cand: c,
                        fate,
                        feat,
                        score,
                        bin,
                    },
                    seed_kind: kind,
                })
            })
            .collect();
        let t3 = Instant::now();
        let n = rows.len();
        self.publish(rows, stage);
        if trace {
            eprintln!(
                "batch stage {stage} {} worlds={n} cells={} gens={gens}: seed {:.2}s gpu {:.2}s measure {:.2}s publish {:.3}s",
                grid.key,
                grid.topo.len(),
                (t1 - t0).as_secs_f64(),
                (t2 - t1).as_secs_f64(),
                (t3 - t2).as_secs_f64(),
                t3.elapsed().as_secs_f64(),
            );
        }
        Ok(())
    }

    /// One screening batch on the GPU.
    fn gpu_round(
        &self,
        be: &mut dyn Backend,
        rng: &mut Rng,
        fams: &[(Fam, f64)],
        ltl_ok: bool,
    ) -> Result<(), String> {
        let ltl_round = ltl_ok && (fams.is_empty() || rng.below(8) == 0);
        // Some rounds go to the least-tried tiling, so every tiling is reached by the deadline.
        let ti = if ltl_round && rng.below(3) == 0 {
            self.least_tried_tiling(rng, &[(Fam::Ltl, 1.0)])
        } else if !ltl_round && rng.below(8) == 0 {
            self.least_tried_tiling(rng, fams)
        } else {
            self.pick_tiling(rng)
        };
        let key = self.tilings[ti].key.clone();
        let grid = self.grid(&key, self.set.screen_cells)?;
        let nb = if grid.has_edge && rng.below(4) == 0 {
            Nb::Edge
        } else {
            Nb::Touch
        };
        let mut max_worlds = (be.max_cells() / grid.topo.len().next_multiple_of(256))
            .min(self.set.batch)
            .max(1);
        let ltl = if ltl_round {
            let hood = if rng.below(4) == 0 {
                Hood::VonNeumann
            } else {
                Hood::Moore
            };
            // The largest range whose reach stays affordable (aperiodic reaches run slowly).
            let limit = if grid.topo.lattice().is_some() {
                180
            } else {
                60
            };
            let mut r = (2 + rng.below(5)) as u8;
            while r > 1 && grid.reach_size(hood, r) > limit {
                r -= 1;
            }
            let reach = grid.reach_size(hood, r).max(1);
            // Hold a Larger than Life batch to about the same work as a plain batch.
            let work =
                (grid.topo.len() * self.set.screen_gens as usize).max(1) as f64 * reach as f64;
            let cap = (LTL_NEIGHBOUR_UPDATES / work) as usize;
            max_worlds = max_worlds.min(cap.max(16));
            Some((hood, r))
        } else {
            None
        };
        let filter = |r: &Rule| be.supports(r);
        let cands = self.emit(
            &grid,
            nb,
            fams,
            ltl,
            max_worlds,
            self.set.screen_gens,
            rng,
            Some(&filter),
        );
        let tagged = cands.into_iter().map(|(c, k)| (c, k, None)).collect();
        self.gpu_batch(be, &grid, nb, tagged, 1)
    }

    /// Confirm the best runs of one grid/neighbourhood group on the GPU.
    fn gpu_confirm(&self, be: &mut dyn Backend, limit: usize) -> Result<usize, String> {
        let pending = self.archive.lock().unwrap().unconfirmed(4096);
        let mut first: Option<(String, Nb, Option<(Hood, u8)>)> = None;
        let mut picked: Vec<(i64, Eval)> = Vec::new();
        let key_of = |e: &Eval| -> Option<(String, Nb, Option<(Hood, u8)>)> {
            let r = e.cand.rule()?;
            if !be.supports(&r) {
                return None;
            }
            let l = if let Rule::Ltl(l) = &r {
                Some((l.hood, l.range))
            } else {
                None
            };
            Some((e.cand.tiling.clone(), e.cand.nb, l))
        };
        for (id, e) in pending {
            let Some(k) = key_of(&e) else { continue };
            match &first {
                None => first = Some(k),
                Some(f) if *f != k => continue,
                _ => {}
            }
            picked.push((id, e));
            if picked.len() >= limit {
                break;
            }
        }
        let Some((key, nb, _)) = first else {
            return Ok(0);
        };
        let grid = self.grid(&key, self.set.confirm_cells)?;
        let n = picked.len();
        let mut cands = Vec::new();
        for (id, e) in &picked {
            for c in self.confirmations(e, &grid) {
                cands.push((c, "confirm".to_string(), Some(*id)));
            }
        }
        self.gpu_batch(be, &grid, nb, cands, 2)?;
        let ids: Vec<i64> = picked.iter().map(|p| p.0).collect();
        self.mark_confirmed(&ids);
        Ok(n)
    }

    /// The confirmation runs of an elite on a bigger grid: its candidate with other seeds (each
    /// counted as tried).
    fn confirmations(&self, e: &Eval, grid: &Grid) -> Vec<Cand> {
        let cands: Vec<Cand> = (0..self.set.confirm_seeds)
            .map(|sd| {
                let mut c = e.cand.clone();
                let (g2, st) = run_len(c.fam, grid, self.set.confirm_gens);
                c.cells = self.set.confirm_cells;
                c.gens = g2;
                c.steps = st;
                c.rng = e.cand.rng.wrapping_add(sd as u64 + 1);
                c.origin = "confirm".into();
                c
            })
            .collect();
        let mut seen = self.seen.lock().unwrap();
        for c in &cands {
            seen.insert(c.id_hash());
        }
        cands
    }

    /// One CPU round: a tiling and families chosen, candidates made and run on the pool (the
    /// caller's), and every fourth round a confirmation of elites.
    fn cpu_round(&self, rng: &mut Rng, fams: &[(Fam, f64)]) {
        let round = self.cpu_rounds.fetch_add(1, Ordering::Relaxed) + 1;
        let threads = rayon::current_num_threads().max(1);
        // Families that run on any tiling, and the ones tied to a native grid (when it is one
        // the search draws from).
        let everywhere: Vec<(Fam, f64)> = fams
            .iter()
            .filter(|(f, _)| native_tiling(*f).is_none())
            .map(|(f, _)| (*f, 1.0))
            .collect();
        let native: Vec<Fam> = fams
            .iter()
            .map(|(f, _)| *f)
            .filter(|f| native_tiling(*f).is_some_and(|k| self.tiling_index(k).is_some()))
            .collect();
        // CPU rounds are few, so weighted picks alone would miss tilings. A quarter go to the
        // least-tried native-grid family, half sweep the least-tried tiling.
        let (ti, round_fams): (usize, Vec<(Fam, f64)>) = match round % 4 {
            0 if !native.is_empty() => {
                let f = self.least_tried_fam(&native);
                let key = native_tiling(f).expect("native family");
                (self.tiling_index(key).unwrap_or(0), vec![(f, 1.0)])
            }
            1 | 2 if !everywhere.is_empty() => (
                self.least_tried_tiling(rng, &everywhere),
                everywhere.clone(),
            ),
            _ => (self.pick_tiling(rng), fams.to_vec()),
        };
        let key = self.tilings[ti].key.clone();
        let Ok(grid) = self.grid(&key, self.set.screen_cells) else {
            return;
        };
        let nb = if grid.has_edge && rng.below(4) == 0 {
            Nb::Edge
        } else {
            Nb::Touch
        };
        let cands = self.emit(
            &grid,
            nb,
            &round_fams,
            None,
            threads * 8,
            self.set.screen_gens,
            rng,
            None,
        );
        let rows: Vec<RunOut> = cands
            .into_par_iter()
            .filter_map(|(c, kind)| self.cpu_eval(&grid, c, kind, None, 1))
            .collect();
        self.publish(rows, 1);
        if round.is_multiple_of(4) {
            self.cpu_confirm(fams, threads);
        }
    }

    fn cpu_eval(
        &self,
        grid: &Grid,
        c: Cand,
        kind: String,
        parent: Option<i64>,
        stage: u8,
    ) -> Option<RunOut> {
        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut world = prepare(grid, &c).ok()?;
            let window = if c.fam.is_rows() { 1 } else { WINDOW };
            let t = trace_world(&mut world, c.gens, &frames_for(c.gens, window)).ok()?;
            Some(evaluate(grid, &c, &t))
        }));
        let (fate, feat, score, bin) = match r {
            Ok(x) => x?,
            Err(_) => {
                self.panics.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        Some(RunOut {
            id: self.next_id(),
            stage,
            parent,
            ev: Eval {
                cand: c,
                fate,
                feat,
                score,
                bin,
            },
            seed_kind: kind,
        })
    }

    fn cpu_confirm(&self, fams: &[(Fam, f64)], threads: usize) {
        let pending = self.archive.lock().unwrap().unconfirmed(4096);
        let picked: Vec<(i64, Eval)> = pending
            .into_iter()
            .filter(|(_, e)| fams.iter().any(|(f, _)| *f == e.cand.fam))
            .take(threads * 2)
            .collect();
        let rows: Vec<RunOut> = picked
            .par_iter()
            .flat_map(|(id, e)| {
                let grid = self.grid(&e.cand.tiling, self.set.confirm_cells).ok()?;
                let rows: Vec<RunOut> = self
                    .confirmations(e, &grid)
                    .into_iter()
                    .filter_map(|c| self.cpu_eval(&grid, c, "confirm".into(), Some(*id), 2))
                    .collect();
                Some(rows)
            })
            .flatten()
            .collect();
        let ids: Vec<i64> = picked.iter().map(|p| p.0).collect();
        self.mark_confirmed(&ids);
        self.publish(rows, 2);
    }
}

/// The group (0-7) of a tiling in `tilings`, for the archive's niches (0 when it is not there).
pub fn group_of(tilings: &[TilingChoice], key: &str) -> u8 {
    tilings
        .iter()
        .find(|t| t.key == key)
        .and_then(|t| group_index(&t.group))
        .unwrap_or(0)
}

fn group_index(g: &str) -> Option<u8> {
    Some(match g {
        "square" => 0,
        "hexagon" => 1,
        "triangle" => 2,
        "pentagon" => 3,
        "archimedean" => 4,
        "k-uniform" => 5,
        "isohedral" => 6,
        "aperiodic" => 7,
        _ => return None,
    })
}

/// The tiling a grid-specific family is searched on. Weighted tiling picks starve these: some
/// are structural twins of another tiling and carry the twin discount.
fn native_tiling(f: Fam) -> Option<&'static str> {
    match f {
        Fam::Wolfram | Fam::Margolus | Fam::Table => Some("square"),
        Fam::HexWolfram => Some("hex"),
        Fam::HexTri => Some("hex_flat"),
        _ => None,
    }
}

/// Generations and steps for a family on a grid.
fn run_len(fam: Fam, grid: &Grid, gens: u32) -> (u32, u32) {
    let side = (grid.topo.len() as f64).sqrt() as u32;
    match fam {
        Fam::Wolfram | Fam::HexWolfram | Fam::HexTri | Fam::Layered => (side * 3, 1),
        Fam::Turmite => (gens.min(1024), 64),
        Fam::Sandpile => (gens.min(1024), 32),
        Fam::Lenia => (gens.min(400), 1),
        _ => (gens, 1),
    }
}

/// A world seeded for a candidate.
fn prepare(grid: &Grid, c: &Cand) -> Result<World, String> {
    let mut w = World::on_topology(grid.topo.clone(), c.rng);
    w.add_ca(c.ca())?;
    Ok(w)
}

fn evaluate(grid: &Grid, c: &Cand, t: &WorldTrace) -> (Fate, Features, f32, u32) {
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let (fate, f) = measure_run(grid, t, c.gens, window);
    let score = archive::score(fate, &f, c.gens, c.fam);
    let bin = archive::behaviour_bin(fate, &f, c.gens);
    (fate, f, score, bin)
}

/// Run one candidate on the CPU now and measure it.
pub fn try_one(c: Cand) -> Result<Eval, String> {
    let grid = Grid::build(&c.tiling, c.cells)?;
    let mut world = prepare(&grid, &c)?;
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let t = trace_world(&mut world, c.gens, &frames_for(c.gens, window))?;
    let (fate, feat, score, bin) = evaluate(&grid, &c, &t);
    Ok(Eval {
        cand: c,
        fate,
        feat,
        score,
        bin,
    })
}

/// The tilings' groups, by key, for [`super::store::rescore`].
pub fn tiling_groups() -> HashMap<String, u8> {
    let all = space::all_tilings();
    all.iter()
        .map(|t| (t.key.clone(), group_of(&all, &t.key)))
        .collect()
}
