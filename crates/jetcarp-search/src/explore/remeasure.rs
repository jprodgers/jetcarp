//! Re-measuring imported runs on this engine. An old explore database (CART's) stores each run's
//! measures as the engine of its day computed them; where something has changed since (a tiling's
//! geometry, the way a rule runs), its runs no longer replay to those measures. An import
//! ([`crate::jobs::explore`]) checks, before it copies anything, a sample of every tiling's runs,
//! and re-runs every run of a tiling whose sample does not reproduce, storing the new measures
//! under the same run ids.
//!
//! **The check** ([`Remeasure::Auto`]): a deterministic, stratified sample of up to
//! [`SAMPLE_PER_TILING`] runs per tiling ([`Sampler`]: each rule family's first run, the oldest,
//! then the families in turn, and within a family its kinds of starting pattern; the
//! confirmation runs are a stratum of their own), each run again at its recorded settings and
//! compared exactly with what is stored: fate, settling generation, period, every feature,
//! score, and behaviour bin ([`diff`]). A tiling whose differing sampled runs span two or more
//! families is re-measured in full (a change to the tiling shows in every family); when they are
//! all of one family, the way that rule runs has changed, and only that family's runs on the
//! tiling are ([`TilingCheck::only_family`]). The others are copied as they are.
//! [`Remeasure::All`] re-measures every run, [`Remeasure::None`] none.
//!
//! **The GPU.** The check runs its sample on the CPU but Larger than Life, which a search runs
//! on a GPU only (on the CPU a run takes seconds), and runs each tiling's largest batch of
//! GPU-runnable sampled runs on the GPU as well ([`check_many`]). If the GPU measured them all
//! as the CPU did, the re-measuring sends it every rule it runs ([`OnGpu::Every`]); else only
//! Larger than Life ([`OnGpu::LtlOnly`]).
//!
//! **Where it is recorded:** the search's manifest (`remeasure`, a [`Record`]: which tilings were
//! re-measured and why, how many runs changed), the job's progress and report, and the working
//! database (the decision and the whole check, per tiling, so a stopped import goes on with them).
//! Runs keep their schema: a re-measured run is any run of a tiling the manifest lists.

use crate::explore::job::{Measured, batch_key, measure_batch, measure_on};
use crate::explore::segments::RunRec;
use crate::explore::store::{self, eval_of, fate_name, feat_blob};
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::explore::space::Grid;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::sync::{Arc, Mutex};

/// Runs re-run per tiling to check it.
pub const SAMPLE_PER_TILING: usize = 32;

/// Which imported runs are run again on this engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Remeasure {
    /// Check a sample of every tiling; re-measure every run of a tiling whose sample differs.
    #[default]
    Auto,
    /// Every run.
    All,
    /// None: copy the runs as they are.
    None,
}

impl Remeasure {
    pub fn name(self) -> &'static str {
        match self {
            Remeasure::Auto => "auto",
            Remeasure::All => "all",
            Remeasure::None => "none",
        }
    }

    pub fn parse(s: &str) -> Result<Remeasure, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Remeasure::Auto),
            "all" => Ok(Remeasure::All),
            "none" => Ok(Remeasure::None),
            x => Err(format!("remeasure is auto, all, or none, not '{x}'")),
        }
    }

    pub fn is_auto(&self) -> bool {
        *self == Remeasure::Auto
    }
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One tiling's runs, as the sample sees them.
#[derive(Default)]
struct Pool {
    runs: u64,
    /// Per family (`confirm` for every confirmation run), per seed kind: the runs with the
    /// lowest hashes of their ids, at most [`SAMPLE_PER_TILING`] (a max-heap).
    strata: BTreeMap<String, BTreeMap<String, BinaryHeap<(u64, i64)>>>,
    /// Per family: its first run (the lowest id: the oldest).
    first: BTreeMap<String, i64>,
}

/// Draws the check's sample while the runs go by, in any order: the same runs give the same
/// sample.
#[derive(Default)]
pub struct Sampler {
    tilings: BTreeMap<String, Pool>,
    /// Runs offered.
    pub seen: u64,
}

impl Sampler {
    pub fn offer(&mut self, k: &store::RunKey) {
        self.seen += 1;
        let p = self.tilings.entry(k.tiling.clone()).or_default();
        p.runs += 1;
        let (fam, kind) = if k.stage == 1 {
            (k.fam.as_str(), k.seed_kind.as_str())
        } else {
            ("confirm", "confirm")
        };
        let first = p.first.entry(fam.to_string()).or_insert(k.id);
        *first = (*first).min(k.id);
        let heap = p
            .strata
            .entry(fam.to_string())
            .or_default()
            .entry(kind.to_string())
            .or_default();
        let h = (splitmix(k.id as u64 ^ 0x5A4D_504C_4521), k.id);
        if heap.len() < SAMPLE_PER_TILING {
            heap.push(h);
        } else if heap.peek().is_some_and(|top| h < *top) {
            heap.pop();
            heap.push(h);
        }
    }

    /// Tilings drawn from so far.
    pub fn tilings(&self) -> usize {
        self.tilings.len()
    }

    /// Per tiling: its runs and its sample (ids in order). The sample holds each family's first
    /// run (the engine that made a database may have changed while it ran, and the oldest runs
    /// are the likeliest not to reproduce), then takes the families in turn, each family its seed
    /// kinds in turn, each kind its runs lowest hash first, until it has [`SAMPLE_PER_TILING`]
    /// runs or the tiling has no more.
    pub fn picks(&self) -> Vec<(String, u64, Vec<i64>)> {
        let mut out = Vec::new();
        for (tiling, p) in &self.tilings {
            let mut ids: Vec<i64> = p.first.values().copied().collect();
            // Each family's runs, its kinds interleaved.
            let fams: Vec<Vec<i64>> = p
                .strata
                .values()
                .map(|kinds| {
                    let lists: Vec<Vec<i64>> = kinds
                        .values()
                        .map(|h| {
                            let mut v = h.clone().into_vec();
                            v.sort_unstable();
                            v.into_iter().map(|x| x.1).collect()
                        })
                        .collect();
                    interleave(&lists)
                })
                .collect();
            ids.truncate(SAMPLE_PER_TILING);
            for id in interleave(&fams) {
                if ids.len() >= SAMPLE_PER_TILING {
                    break;
                }
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids.sort_unstable();
            out.push((tiling.clone(), p.runs, ids));
        }
        out
    }
}

/// The lists' items in turn: the first of each, then the second of each, ...
fn interleave(lists: &[Vec<i64>]) -> Vec<i64> {
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..longest {
        for l in lists {
            if let Some(x) = l.get(i) {
                out.push(*x);
            }
        }
    }
    out
}

/// What differs between a stored run and its run again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub fate: bool,
    pub settle: bool,
    pub period: bool,
    /// Any of the 18 features from `density` to `mover_speed` (compared bit for bit).
    pub features: bool,
    pub score: bool,
    pub bin: bool,
}

impl Diff {
    pub fn any(&self) -> bool {
        self.fate || self.settle || self.period || self.features || self.score || self.bin
    }
}

/// Compare a stored run with its measures now, exactly.
pub fn diff(r: &RunRec, m: &Measured) -> Diff {
    let (fate, f, score, bin) = m;
    Diff {
        fate: r.fate != fate_name(*fate),
        settle: r.settle != f.settle as i64,
        period: r.period != f.period as i64,
        features: feat_blob(&r.feat)[8..] != feat_blob(f)[8..],
        score: (r.score as f32).to_bits() != score.to_bits(),
        bin: r.bin != *bin as i64,
    }
}

/// A stored run with new measures (its id, candidate, and time kept).
pub fn apply(r: &mut RunRec, m: &Measured) {
    let (fate, f, score, bin) = m;
    r.fate = fate_name(*fate).to_string();
    r.settle = f.settle as i64;
    r.period = f.period as i64;
    r.feat = *f;
    r.score = *score as f64;
    r.bin = *bin as i64;
}

/// One tiling's check.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TilingCheck {
    pub tiling: String,
    /// The tiling's runs in the old database, and the ones run again.
    pub runs: u64,
    pub sampled: u64,
    /// Sampled runs whose measures differ, and per measure how many.
    pub differ: u64,
    pub fate: u64,
    pub settle: u64,
    pub period: u64,
    pub features: u64,
    pub score: u64,
    pub bin: u64,
    /// Sampled runs that could not run again (their seed does not read, or they panicked).
    pub failed: u64,
    /// The first sampled run that differs.
    pub example: Option<i64>,
    /// Sampled runs that differ, per rule family.
    pub families: BTreeMap<String, u64>,
    /// Sampled runs the GPU ran too, and how many of those it measured unlike the CPU.
    pub gpu_runs: u64,
    pub gpu_differ: u64,
}

impl TilingCheck {
    /// Is the tiling (or one of its families) to be run again?
    pub fn remeasure(&self) -> bool {
        self.differ > 0
    }

    /// The one family whose runs are run again, when only that family's sampled runs differ:
    /// how a rule runs has changed, not the tiling (whose change would show in every family).
    /// `None`: every run of the tiling is run again.
    pub fn only_family(&self) -> Option<&str> {
        match self.families.len() {
            1 => self.families.keys().next().map(String::as_str),
            _ => None,
        }
    }

    /// Why (or why not), in a sentence.
    pub fn why(&self) -> String {
        if self.differ == 0 {
            return format!(
                "all {} sampled runs measured as stored",
                self.sampled - self.failed
            );
        }
        let parts: Vec<String> = [
            ("fate", self.fate),
            ("settle", self.settle),
            ("period", self.period),
            ("features", self.features),
            ("score", self.score),
            ("bin", self.bin),
        ]
        .iter()
        .filter(|x| x.1 > 0)
        .map(|(k, n)| format!("{k} {n}"))
        .collect();
        let which = match self.only_family() {
            Some(f) => format!(", all {f}: its {f} runs are re-measured"),
            None => format!(
                ", in {} families: every run is re-measured",
                self.families.len()
            ),
        };
        format!(
            "{} of {} sampled runs measured differently on this engine ({}){which}",
            self.differ,
            self.sampled - self.failed,
            parts.join(", ")
        )
    }

    fn add(&mut self, d: &Diff, id: i64, fam: &str) {
        if d.any() {
            self.differ += 1;
            *self.families.entry(fam.to_string()).or_default() += 1;
            if self.example.is_none() {
                self.example = Some(id);
            }
        }
        for (n, x) in [
            (&mut self.fate, d.fate),
            (&mut self.settle, d.settle),
            (&mut self.period, d.period),
            (&mut self.features, d.features),
            (&mut self.score, d.score),
            (&mut self.bin, d.bin),
        ] {
            *n += x as u64;
        }
    }
}

/// A tiling the import re-measured, as the manifest lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remeasured {
    pub tiling: String,
    /// The one rule family whose runs were re-measured (none: every run of the tiling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub why: String,
    /// Runs run again, those whose measures changed, and those that could not run (kept as
    /// they were).
    pub runs: u64,
    pub changed: u64,
    pub failed: u64,
}

/// What an import checked and re-measured: the manifest's `remeasure`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    /// `auto`, `all`, or `none`.
    pub mode: String,
    /// The JETCARP that measured, and when the check was made (UTC).
    pub engine_version: String,
    pub checked: String,
    /// The check: runs sampled per tiling at most, tilings, sampled runs, and the sampled runs
    /// that differ (auto only).
    pub sample_per_tiling: u64,
    pub tilings: u64,
    pub sampled: u64,
    pub sampled_differ: u64,
    /// What ran the runs the GPU can run: `cpu`, or the GPU's name.
    pub backend: String,
    /// The tilings re-measured, by name.
    pub remeasured: Vec<Remeasured>,
    /// Runs re-measured in all, and those whose measures changed.
    pub runs: u64,
    pub changed: u64,
    /// Seconds the check took, and the re-measuring during the copy.
    pub check_seconds: f64,
    pub remeasure_seconds: f64,
    /// Whether every run has been copied.
    pub done: bool,
}

/// The decision and its counts as the working database keeps them (meta `remeasure`), so a
/// stopped import goes on with the same tilings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub record: Record,
    /// The tilings to re-measure, and why (auto; `all` re-measures every one).
    pub tilings: BTreeMap<String, String>,
    /// Of those, the ones re-measured for one rule family only, and that family.
    pub families: BTreeMap<String, String>,
    /// Per tiling: runs re-measured, changed, and failed, in the segments committed.
    pub counts: BTreeMap<String, (u64, u64, u64)>,
    /// Which runs the GPU named in the record's `backend` measures (none: the CPU runs all).
    pub gpu: Option<OnGpu>,
}

impl State {
    pub fn new(mode: Remeasure) -> State {
        State {
            record: Record {
                mode: mode.name().into(),
                engine_version: jetcarp_core::VERSION.into(),
                checked: jetcarp_core::pattern::now_utc(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Is this run (of this tiling and family) to be run again?
    pub fn wants(&self, tiling: &str, fam: &str) -> bool {
        self.record.mode == "all"
            || (self.tilings.contains_key(tiling)
                && self.families.get(tiling).is_none_or(|f| f == fam))
    }

    /// Are any of this tiling's runs to be run again?
    pub fn wants_tiling(&self, tiling: &str) -> bool {
        self.record.mode == "all" || self.tilings.contains_key(tiling)
    }

    pub fn is_empty(&self) -> bool {
        self.record.mode != "all" && self.tilings.is_empty()
    }

    /// Add a segment's counts.
    pub fn commit(&mut self, add: &BTreeMap<String, (u64, u64, u64)>) {
        for (t, (n, c, f)) in add {
            let e = self.counts.entry(t.clone()).or_default();
            e.0 += n;
            e.1 += c;
            e.2 += f;
        }
        self.record.runs = self.counts.values().map(|x| x.0).sum();
        self.record.changed = self.counts.values().map(|x| x.1).sum();
        self.record.remeasured = self
            .counts
            .iter()
            .map(|(t, (n, c, f))| Remeasured {
                tiling: t.clone(),
                family: self.families.get(t).cloned(),
                why: self
                    .tilings
                    .get(t)
                    .cloned()
                    .unwrap_or_else(|| "every run was re-measured (remeasure all)".into()),
                runs: *n,
                changed: *c,
                failed: *f,
            })
            .collect();
        // Tilings chosen that have had no runs yet.
        for (t, why) in &self.tilings {
            if !self.counts.contains_key(t) {
                self.record.remeasured.push(Remeasured {
                    tiling: t.clone(),
                    family: self.families.get(t).cloned(),
                    why: why.clone(),
                    ..Default::default()
                });
            }
        }
        self.record
            .remeasured
            .sort_by(|a, b| a.tiling.cmp(&b.tiling));
    }

    /// Decide from a finished check.
    pub fn decide(&mut self, checks: &[TilingCheck]) {
        let r = &mut self.record;
        r.sample_per_tiling = SAMPLE_PER_TILING as u64;
        r.tilings = checks.len() as u64;
        r.sampled = checks.iter().map(|c| c.sampled).sum();
        r.sampled_differ = checks.iter().map(|c| c.differ).sum();
        self.tilings = checks
            .iter()
            .filter(|c| c.remeasure())
            .map(|c| (c.tiling.clone(), c.why()))
            .collect();
        self.families = checks
            .iter()
            .filter_map(|c| Some((c.tiling.clone(), c.only_family()?.to_string())))
            .collect();
        self.commit(&BTreeMap::new());
    }
}

/// The grids runs are measured on, by tiling and size (built once each).
#[derive(Clone, Default)]
pub struct Grids(Arc<Mutex<HashMap<(String, u32), Arc<Grid>>>>);

impl Grids {
    pub fn get(&self, tiling: &str, cells: u32) -> Option<Arc<Grid>> {
        let k = (tiling.to_string(), cells);
        if let Some(g) = self.0.lock().unwrap().get(&k) {
            return Some(g.clone());
        }
        let g = Arc::new(Grid::build(tiling, cells).ok()?);
        Some(self.0.lock().unwrap().entry(k).or_insert(g).clone())
    }

    /// Let go of every grid but these tilings'.
    pub fn keep_only(&self, keep: &dyn Fn(&str) -> bool) {
        self.0.lock().unwrap().retain(|k, _| keep(&k.0));
    }
}

/// Which runs a GPU measures, given one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnGpu {
    /// Every run of a rule it can run, as a search with a GPU runs them.
    Every,
    /// Larger than Life only (a search runs those on a GPU only; on the CPU they take seconds).
    LtlOnly,
}

fn is_ltl(c: &jetcarp_core::explore::cand::Cand) -> bool {
    c.fam == jetcarp_core::explore::space::Fam::Ltl
}

/// Measure runs again: on `gpu` the ones it can run (when given; every one, or Larger than Life
/// only), the rest on the CPU (on the caller's rayon pool), at the same time. `None` for a run
/// that does not run.
pub fn measure(
    runs: &[&RunRec],
    grids: &Grids,
    gpu: Option<(&mut dyn Backend, OnGpu)>,
) -> Result<Vec<Option<Measured>>, String> {
    let evals: Vec<Option<crate::explore::archive::Eval>> =
        runs.iter().map(|r| eval_of(r)).collect();
    let mut out: Vec<Option<Measured>> = vec![None; runs.len()];
    let (gpu, which) = match gpu {
        Some((be, w)) => (Some(be), w),
        None => (None, OnGpu::Every),
    };
    // The GPU's share: by tiling, size, and batch key.
    let mut groups: BTreeMap<(String, u32, String), Vec<usize>> = BTreeMap::new();
    let mut cpu: Vec<usize> = Vec::new();
    for (i, ev) in evals.iter().enumerate() {
        let Some(ev) = ev else { continue };
        let key = gpu
            .as_deref()
            .filter(|_| which == OnGpu::Every || is_ltl(&ev.cand))
            .and_then(|be| batch_key(be, &ev.cand));
        match key {
            Some(k) => groups
                .entry((ev.cand.tiling.clone(), ev.cand.cells, format!("{k:?}")))
                .or_default()
                .push(i),
            None => cpu.push(i),
        }
    }
    let trace = std::env::var_os("JETCARP_EXPLORE_TRACE").is_some();
    let cpu_part = || -> Vec<(usize, Option<Measured>)> {
        let t = std::time::Instant::now();
        let v: Vec<(usize, Option<Measured>)> = cpu
            .par_iter()
            .map(|&i| {
                let c = &evals[i].as_ref().unwrap().cand;
                let m = grids
                    .get(&c.tiling, c.cells)
                    .and_then(|g| measure_on(&g, c));
                (i, m)
            })
            .collect();
        if trace {
            eprintln!(
                "remeasure cpu: {} runs in {:.2} s",
                v.len(),
                t.elapsed().as_secs_f64()
            );
        }
        v
    };
    let gpu_part = |be: &mut dyn Backend| -> Result<Vec<(usize, Option<Measured>)>, String> {
        let t0 = std::time::Instant::now();
        let mut got = Vec::new();
        for ((tiling, cells, key), idx) in &groups {
            let t = std::time::Instant::now();
            let Some(g) = grids.get(tiling, *cells) else {
                got.extend(idx.iter().map(|&i| (i, None)));
                continue;
            };
            let cands: Vec<&jetcarp_core::explore::cand::Cand> = idx
                .iter()
                .map(|&i| &evals[i].as_ref().unwrap().cand)
                .collect();
            let m = measure_batch(be, &g, &cands)?;
            got.extend(idx.iter().copied().zip(m));
            if trace {
                eprintln!(
                    "remeasure gpu {tiling} {cells} {key}: {} runs in {:.2} s",
                    idx.len(),
                    t.elapsed().as_secs_f64()
                );
            }
        }
        if trace {
            eprintln!(
                "remeasure gpu: {} runs in {:.2} s",
                got.len(),
                t0.elapsed().as_secs_f64()
            );
        }
        Ok(got)
    };
    let (a, b) = match gpu {
        Some(be) if !groups.is_empty() => {
            let (a, b) = std::thread::scope(|s| {
                let h = s.spawn(|| gpu_part(be));
                let a = cpu_part();
                (a, h.join())
            });
            (a, b.map_err(|_| "the GPU thread panicked".to_string())??)
        }
        _ => (cpu_part(), vec![]),
    };
    for (i, m) in a.into_iter().chain(b) {
        out[i] = m;
    }
    Ok(out)
}

/// Check tilings' samples (each: the tiling, its runs in the old database, its sampled runs):
/// each run again on the CPU (on the caller's rayon pool), but Larger than Life on the GPU when
/// there is one (a search runs it on a GPU only; on the CPU a run takes seconds). Given a GPU, the
/// largest batch of each tiling's sampled runs it can run (but Larger than Life) runs on the GPU
/// too, to see whether it measures as the CPU does (`gpu_runs`, `gpu_differ`).
pub fn check_many(
    tilings: &[(String, u64, Vec<RunRec>)],
    grids: &Grids,
    gpu: Option<&mut dyn Backend>,
) -> Result<Vec<TilingCheck>, String> {
    let mut gpu = gpu;
    let all: Vec<(usize, &RunRec)> = tilings
        .iter()
        .enumerate()
        .flat_map(|(k, t)| t.2.iter().map(move |r| (k, r)))
        .collect();
    let refs: Vec<&RunRec> = all.iter().map(|x| x.1).collect();
    // The grids, built in parallel first.
    let sizes: std::collections::BTreeSet<(String, u32)> = refs
        .iter()
        .map(|r| (r.tiling.clone(), r.cells as u32))
        .collect();
    sizes.par_iter().for_each(|(t, c)| {
        grids.get(t, *c);
    });
    let mut on_gpu: Vec<usize> = Vec::new();
    if let Some(be) = &gpu {
        let mut by: BTreeMap<usize, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
        for (i, (k, r)) in all.iter().enumerate() {
            let Some(ev) = eval_of(r) else { continue };
            if is_ltl(&ev.cand) {
                continue;
            }
            if let Some(key) = batch_key(&**be, &ev.cand) {
                by.entry(*k)
                    .or_default()
                    .entry(format!("{} {key:?}", ev.cand.cells))
                    .or_default()
                    .push(i);
            }
        }
        for groups in by.into_values() {
            if let Some(g) = groups.into_values().max_by_key(|v| v.len()) {
                on_gpu.extend(g);
            }
        }
    }
    let now = measure(
        &refs,
        grids,
        gpu.as_mut()
            .map(|be| (&mut **be as &mut dyn Backend, OnGpu::LtlOnly)),
    )?;
    let mut out: Vec<TilingCheck> = tilings
        .iter()
        .map(|(t, n, s)| TilingCheck {
            tiling: t.clone(),
            runs: *n,
            sampled: s.len() as u64,
            ..Default::default()
        })
        .collect();
    for ((k, r), m) in all.iter().zip(&now) {
        match m {
            Some(m) => out[*k].add(&diff(r, m), r.id, &r.fam),
            None => out[*k].failed += 1,
        }
    }
    if let Some(be) = gpu.filter(|_| !on_gpu.is_empty()) {
        let sub: Vec<&RunRec> = on_gpu.iter().map(|&i| all[i].1).collect();
        let on = measure(&sub, grids, Some((be, OnGpu::Every)))?;
        for (&i, m) in on_gpu.iter().zip(&on) {
            let same = match (m, &now[i]) {
                (Some(a), Some(b)) => {
                    let mut r = all[i].1.clone();
                    apply(&mut r, b);
                    !diff(&r, a).any()
                }
                (None, None) => true,
                _ => false,
            };
            let c = &mut out[all[i].0];
            c.gpu_runs += 1;
            c.gpu_differ += !same as u64;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: i64, stage: i64, tiling: &str, fam: &str, kind: &str) -> store::RunKey {
        store::RunKey {
            id,
            stage,
            tiling: tiling.into(),
            fam: fam.into(),
            seed_kind: kind.into(),
        }
    }

    #[test]
    fn the_sample_spreads_over_families_and_seed_kinds_and_does_not_depend_on_order() {
        let mut keys = Vec::new();
        for id in 1..=5000 {
            let fam = ["life_like", "generations", "cyclic"][(id % 3) as usize];
            let kind = ["soup", "blobs", "mutated", "point"][(id % 4) as usize];
            let stage = if id % 50 == 0 { 2 } else { 1 };
            let tiling = if id % 7 == 0 { "hex" } else { "square" };
            keys.push(key(id, stage, tiling, fam, kind));
        }
        keys.push(key(9000, 1, "tiny", "life_like", "soup"));
        let mut a = Sampler::default();
        keys.iter().for_each(|k| a.offer(k));
        let mut b = Sampler::default();
        keys.iter().rev().for_each(|k| b.offer(k));
        assert_eq!(a.picks(), b.picks());
        let picks = a.picks();
        assert_eq!(
            picks.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
            ["hex", "square", "tiny"]
        );
        let tiny = &picks[2];
        assert_eq!((tiny.1, tiny.2.clone()), (1, vec![9000]));
        let sq = &picks[1];
        assert_eq!(sq.2.len(), SAMPLE_PER_TILING);
        // Each family's oldest run is there, and the oldest confirmation run.
        for id in [1, 2, 3, 50] {
            assert!(sq.2.contains(&id), "{id} in {:?}", sq.2);
        }
        let by: HashMap<i64, &store::RunKey> = keys.iter().map(|k| (k.id, k)).collect();
        let fams: std::collections::HashSet<&str> =
            sq.2.iter()
                .map(|i| {
                    let k = by[i];
                    if k.stage == 2 { "confirm" } else { &k.fam }
                })
                .collect();
        assert_eq!(fams.len(), 4, "three families and the confirmation runs");
        let kinds: std::collections::HashSet<(&str, &str)> =
            sq.2.iter()
                .map(|i| (by[i].fam.as_str(), by[i].seed_kind.as_str()))
                .collect();
        assert!(kinds.len() >= 12, "{kinds:?}");
    }

    #[test]
    fn a_decision_lists_the_tilings_that_differ() {
        let mut s = State::new(Remeasure::Auto);
        let ok = TilingCheck {
            tiling: "square".into(),
            runs: 100,
            sampled: 32,
            ..Default::default()
        };
        let d = Diff {
            fate: true,
            score: true,
            ..Default::default()
        };
        // Two families differ: the tiling's every run.
        let mut bad = ok.clone();
        bad.tiling = "penrose-rhomb".into();
        bad.add(&d, 7, "life_like");
        bad.add(&d, 9, "cyclic");
        // One family differs: that family's runs only.
        let mut one = ok.clone();
        one.tiling = "hex".into();
        one.add(&d, 11, "hex_wolfram");
        one.add(&d, 12, "hex_wolfram");
        s.decide(&[ok, bad, one]);
        assert!(s.wants("penrose-rhomb", "life_like") && s.wants("penrose-rhomb", "ltl"));
        assert!(!s.wants("square", "life_like") && !s.wants_tiling("square"));
        assert!(s.wants("hex", "hex_wolfram") && !s.wants("hex", "life_like"));
        assert!(s.wants_tiling("hex"));
        let r = &s.record.remeasured;
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].tiling.as_str(), r[0].family.as_deref()),
            ("hex", Some("hex_wolfram"))
        );
        assert_eq!(
            r[0].why,
            "2 of 32 sampled runs measured differently on this engine (fate 2, score 2), all \
             hex_wolfram: its hex_wolfram runs are re-measured"
        );
        assert_eq!(
            (r[1].tiling.as_str(), r[1].family.as_deref()),
            ("penrose-rhomb", None)
        );
        assert_eq!(
            r[1].why,
            "2 of 32 sampled runs measured differently on this engine (fate 2, score 2), in 2 \
             families: every run is re-measured"
        );
        let mut add = BTreeMap::new();
        add.insert("penrose-rhomb".to_string(), (10, 9, 0));
        s.commit(&add);
        s.commit(&add);
        assert_eq!((s.record.runs, s.record.changed), (20, 18));
        assert_eq!(s.record.remeasured[1].runs, 20);
        assert_eq!(Remeasure::parse("ALL"), Ok(Remeasure::All));
        assert!(Remeasure::parse("some").is_err());
    }
}
