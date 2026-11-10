//! Ant searches as catalog jobs: every rule of the spec on every tiling, from an empty field or
//! seeded soups, in batches, flushed to the catalog with a checkpoint, resumable after a crash
//! or reboot.
//!
//! A search is a `jobs` row of kind `ants` whose `spec` is an [`AntSpec`]. Its runs are numbered:
//! tilings outer, then the tiling's rules (explicit rules, then turn strings, then tables, each in
//! [`super::search`] order), then soups. Run `u` is a pure function of the spec and `u` (soup `s`
//! uses the RNG state `soup_state(seed, s)`), so the rows and totals do not depend on threads or
//! batches. A batch of consecutive runs is computed in parallel and merged in run order; one
//! transaction then writes its rows, the progress (JSON, an [`AntProgress`]), and the checkpoint
//! (the next run, the totals, the newest highways), so a crash loses at most one batch and a
//! resumed search ends exactly as an uninterrupted one.
//!
//! A search stops at the next batch when its stop flag is set, when another process marks the
//! job `paused` ([`request_stop`]), or when a session limit is reached.
//!
//! The job runner (`crate::jobs::ants`) runs a search in steps, one batch each
//! ([`AntSearch`]), under its lease; it writes each batch's rows with the progress and
//! checkpoint.

use super::plane::Lattice;
use super::run::{Limits, Outcome, Start, run};
use super::search::{Strings, Tables, strings, tables};
use crate::jobs::{RunCx, Sink};
use crate::library::{
    AntRow, JobStatus, Library, ant_id, ant_score, now_utc, tiling_display_name, tiling_key,
};
use crate::rules::Rule;
use crate::topology::TilingSpec;
use crate::turmite::Mite;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

/// Seeded soups to start from instead of an empty field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Soups {
    /// Size in unit cells.
    pub w: u32,
    pub h: u32,
    pub density: f64,
    /// Soups per rule and tiling.
    pub soups: u64,
    pub seed: u64,
}

/// Everything that defines a search's results (threads and batching do not).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AntSpec {
    pub tilings: Vec<TilingSpec>,
    /// Explicit rules (`Ant RL`, `Turmite LLRR`, `RL2N`, `Turmite 1R1,1L0/1R1,0N0`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strings: Option<Strings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<Tables>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soup: Option<Soups>,
    pub steps: u64,
    pub max_period: u64,
    #[serde(default = "yes")]
    pub symmetry: bool,
}
fn yes() -> bool {
    true
}

impl Default for AntSpec {
    fn default() -> Self {
        AntSpec {
            tilings: vec![TilingSpec::new("square")],
            rules: vec![],
            strings: Some(Strings {
                min: 2,
                max: 6,
                alphabet: None,
            }),
            tables: None,
            soup: None,
            steps: 1_000_000,
            max_period: 20_000,
            symmetry: true,
        }
    }
}

impl AntSpec {
    pub fn check(&self) -> Result<(), String> {
        if self.tilings.is_empty() {
            return Err("an ant search needs at least one tiling".into());
        }
        if self.rules.is_empty() && self.strings.is_none() && self.tables.is_none() {
            return Err("an ant search needs rules, strings, or tables".into());
        }
        if self.steps == 0 || self.max_period == 0 {
            return Err("steps and max_period must be at least 1".into());
        }
        if self.max_period > 1_000_000 {
            return Err("max_period is at most 1,000,000".into());
        }
        if let Some(s) = &self.soup {
            if s.soups == 0 || s.w == 0 || s.h == 0 {
                return Err("soups need a size and a count of at least 1".into());
            }
            if !(0.0..=1.0).contains(&s.density) {
                return Err("density must be between 0 and 1".into());
            }
        }
        Ok(())
    }
    pub fn limits(&self) -> Limits {
        Limits {
            steps: self.steps,
            max_period: self.max_period,
            symmetry: self.symmetry,
        }
    }
    fn soups(&self) -> u64 {
        self.soup.as_ref().map(|s| s.soups).unwrap_or(1)
    }
    fn start(&self, s: u64) -> Start {
        match &self.soup {
            None => Start::Empty,
            Some(p) => Start::Soup {
                w: p.w,
                h: p.h,
                density: p.density,
                state: crate::hunt::soup::soup_state(p.seed, s),
            },
        }
    }
}

/// The rules of a spec on one tiling, in run order (duplicates once).
pub fn rules_for(spec: &AntSpec, lat: &Lattice) -> Result<Vec<Mite>, String> {
    let mut out: Vec<Mite> = Vec::new();
    for r in &spec.rules {
        out.push(parse_rule(r)?);
    }
    if let Some(s) = &spec.strings {
        out.extend(strings(lat, s, spec.soup.is_none())?);
    }
    if let Some(t) = &spec.tables {
        out.extend(tables(lat, t)?);
    }
    let mut seen = HashSet::new();
    out.retain(|m| seen.insert(m.clone()));
    Ok(out)
}

/// A rule notation or a bare turn string or table (`RL2N`, `1R1,1L0/1R1,0N0`).
pub fn parse_rule(r: &str) -> Result<Mite, String> {
    let t = r.trim();
    let low = t.to_ascii_lowercase();
    let text = if low.starts_with("ant") || low.starts_with("turmite") {
        t.to_string()
    } else {
        format!("Turmite {t}")
    };
    match Rule::parse(&text) {
        Ok((Rule::Turmite(m), _)) => Ok(m),
        Ok(_) => Err(format!("'{r}' is not a turmite")),
        Err(e) => Err(format!("'{r}': {e}")),
    }
}

/// One stored run, made from a rule, tiling, start, and its result.
pub fn row(m: &Mite, lat: &Lattice, start: &Start, lim: &Limits, r: super::run::AntRun) -> AntRow {
    let rule = m.notation();
    AntRow {
        id: ant_id(&rule, &lat.tiling, start),
        kind: if m.turns().is_some() {
            "string"
        } else {
            "table"
        }
        .into(),
        states: m.states,
        colors: m.colors,
        tiling: lat.tiling.clone(),
        tiling_name: tiling_display_name(&lat.tiling),
        start: start.clone(),
        outcome: r.outcome,
        period: r.period,
        d: r.d,
        speed: r.speed,
        onset: r.onset,
        steps: r.steps,
        cells: r.cells,
        score: ant_score(&r),
        run: r,
        limits: *lim,
        rating: None,
        state: "new".into(),
        name: None,
        notes: String::new(),
        job: None,
        created: String::new(),
        updated: String::new(),
        rule,
    }
}

/// Run one rule on one tiling from one start (no catalog).
pub fn run_one(
    m: &Mite,
    tiling: &TilingSpec,
    start: &Start,
    lim: &Limits,
) -> Result<AntRow, String> {
    let lat = Lattice::new(tiling)?;
    let r = run(&lat, m, start, lim);
    Ok(row(m, &lat, start, lim, r))
}

/// Counts for one tiling.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupTotals {
    pub tiling: String,
    pub tiling_key: String,
    /// Rules on this tiling (after reduction).
    pub rules: u64,
    /// Runs done.
    pub runs: u64,
    pub steps: u64,
    pub by_outcome: BTreeMap<String, u64>,
    /// The highway that starts latest, and the longest highway period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_onset: Option<(String, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longest_period: Option<(String, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A highway (or orbit, or symmetric rule) this search found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AntFind {
    pub id: String,
    pub rule: String,
    pub tiling: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset: Option<u64>,
    /// Run number in the search.
    pub run: u64,
    pub score: f64,
}

/// Progress of a search, written to the job's `progress` at every flush and kept live in
/// memory for a dashboard (the job runner's `crate::jobs::JobHandle::progress`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AntProgress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    pub runs_done: u64,
    pub runs_total: u64,
    /// Runs and ant steps per second in this session.
    pub runs_per_s: f64,
    pub steps_per_s: f64,
    /// Seconds run, over every session.
    pub elapsed_s: f64,
    pub steps: u64,
    pub by_outcome: BTreeMap<String, u64>,
    /// Newest highways and symmetric rules (most recent first).
    pub newest: Vec<AntFind>,
    pub groups: Vec<GroupTotals>,
    pub threads: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated: String,
}

impl AntProgress {
    /// `ants 7: 1,200/2,047 runs (310/s), highway 41, symmetric 9, chaotic 1,150 | newest: ...`
    pub fn line(&self) -> String {
        let outs: Vec<String> = Outcome::ALL
            .iter()
            .filter_map(|o| {
                self.by_outcome
                    .get(o.name())
                    .map(|n| format!("{} {}", o.name(), digits(*n)))
            })
            .collect();
        format!(
            "ants {}: {}/{} runs, {:.0} runs/s, {:.2e} steps/s, {}{}",
            self.job,
            digits(self.runs_done),
            digits(self.runs_total),
            self.runs_per_s,
            self.steps_per_s,
            outs.join(", "),
            self.newest
                .first()
                .map(|f| format!(
                    " | newest: {} {} {}{}",
                    f.rule,
                    f.tiling,
                    f.outcome.name(),
                    f.period.map(|p| format!(" p{p}")).unwrap_or_default()
                ))
                .unwrap_or_default()
        )
    }
}

fn digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// What survives a crash: written with every flush.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub next: u64,
    pub groups: Vec<GroupTotals>,
    pub newest: Vec<AntFind>,
    pub elapsed_s: f64,
}

pub const KIND: &str = "ants";

/// Create a search job (status `paused` until a runner starts it). Returns the job id.
pub fn start(lib: &Library, spec: &AntSpec) -> Result<i64, String> {
    spec.check()?;
    // Fail early on bad rules or tilings.
    for t in &spec.tilings {
        rules_for(spec, &Lattice::new(t)?)?;
    }
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

fn job_spec(lib: &Library, job: i64) -> Result<(crate::library::store::JobRow, AntSpec), String> {
    let j = lib.job(job)?;
    if j.kind != KIND {
        return Err(format!("job {job} is a {} job, not an ant search", j.kind));
    }
    let spec: AntSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    Ok((j, spec))
}

/// Ask a running search (in any process) to stop at its next batch.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let (j, _) = job_spec(lib, job)?;
    if j.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

/// A search's spec and last saved progress, from the catalog.
pub fn status(lib: &Library, job: i64) -> Result<(AntSpec, AntProgress), String> {
    let (j, spec) = job_spec(lib, job)?;
    let mut p: AntProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.status = status_name(j.status).into();
    Ok((spec, p))
}

struct Group {
    lat: Lattice,
    rules: Vec<Mite>,
    /// First run number of the group.
    first: u64,
}

/// An ant search run in steps by the job runner (`crate::jobs::ants`): each step runs one
/// batch of consecutive runs in parallel (about two seconds of work), merges it into the totals
/// in run order, and puts its rows in the sink, which the runner writes with the progress and
/// checkpoint in one transaction.
pub struct AntSearch {
    job: i64,
    spec: AntSpec,
    cp: Checkpoint,
    groups: Vec<Option<Group>>,
    /// Runs in all, and per rule.
    total: u64,
    soups: u64,
    lim: Limits,
    pool: rayon::ThreadPool,
    threads: usize,
    /// Runs per batch, and whether it adapts.
    batch: u64,
    adapt: bool,
    session: Instant,
    session_start: u64,
    session_steps: u64,
    base_elapsed: f64,
}

impl AntSearch {
    /// Open the run's ant search (on its threads; `batch` runs per batch, 0: adapt) from its
    /// checkpoint, if it has one (its format is unchanged since searches became jobs).
    pub fn open(
        cx: &RunCx<'_>,
        checkpoint: Option<&[u8]>,
        batch: u64,
    ) -> Result<AntSearch, String> {
        let (j, job) = (cx.row, cx.job);
        if j.kind != KIND {
            return Err(format!("job {job} is a {} job, not an ant search", j.kind));
        }
        let spec: AntSpec =
            serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
        spec.check()?;
        let mut cp: Checkpoint = match checkpoint {
            Some(b) => {
                serde_json::from_slice(b).map_err(|e| format!("job {job} checkpoint: {e}"))?
            }
            None => Checkpoint::default(),
        };
        let fresh = cp.groups.is_empty();
        if !fresh && cp.groups.len() != spec.tilings.len() {
            return Err(format!("job {job}: the checkpoint does not match its spec"));
        }
        let soups = spec.soups();
        let mut groups: Vec<Option<Group>> = Vec::new();
        let mut first = 0u64;
        for (ti, t) in spec.tilings.iter().enumerate() {
            let made = Lattice::new(t).and_then(|lat| {
                let rules = rules_for(&spec, &lat)?;
                Ok((lat, rules))
            });
            if fresh {
                cp.groups.push(GroupTotals {
                    tiling: tiling_display_name(t),
                    tiling_key: tiling_key(t),
                    ..Default::default()
                });
            }
            match made {
                Ok((lat, rules)) => {
                    cp.groups[ti].rules = rules.len() as u64;
                    let n = rules.len() as u64 * soups;
                    groups.push(Some(Group { lat, rules, first }));
                    first += n;
                }
                Err(e) => {
                    cp.groups[ti].error = Some(e);
                    groups.push(None);
                }
            }
        }
        if groups.iter().all(|g| g.is_none()) {
            let msg = cp
                .groups
                .iter()
                .find_map(|g| g.error.clone())
                .unwrap_or_default();
            return Err(format!("ant search {job}: no tiling can run: {msg}"));
        }
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
        Ok(AntSearch {
            job,
            lim: spec.limits(),
            spec,
            session_start: cp.next,
            base_elapsed: cp.elapsed_s,
            cp,
            groups,
            total: first,
            soups,
            pool,
            threads,
            batch: if batch > 0 {
                batch
            } else {
                (threads as u64 * 4).max(16)
            },
            adapt: batch == 0,
            session: Instant::now(),
            session_steps: 0,
        })
    }

    /// The group, rule, and soup of run `u`.
    fn locate(&self, u: u64) -> Option<(usize, usize, u64)> {
        for (gi, g) in self.groups.iter().enumerate() {
            let Some(g) = g else { continue };
            let n = g.rules.len() as u64 * self.soups;
            if u >= g.first && u < g.first + n {
                let k = u - g.first;
                return Some((gi, (k / self.soups) as usize, k % self.soups));
            }
        }
        None
    }

    /// One batch of runs (none past the end or the item budget), merged in run order, its
    /// rows put in the sink. `Ok(true)`: the search has run its last run.
    pub fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<bool, String> {
        if self.cp.next >= self.total {
            return Ok(true);
        }
        let a = self.cp.next;
        let mut b = (a + self.batch).min(self.total);
        if let Some(l) = cx.items {
            b = b.min(self.session_start + l);
        }
        let t0 = Instant::now();
        let (spec, lim, groups) = (&self.spec, &self.lim, &self.groups);
        let rows: Vec<(usize, AntRow)> = self.pool.install(|| {
            (a..b)
                .collect::<Vec<u64>>()
                .into_par_iter()
                .with_max_len(1)
                .filter_map(|u| {
                    let (gi, ri, s) = self.locate(u)?;
                    let g = groups[gi].as_ref()?;
                    let m = &g.rules[ri];
                    let start = spec.start(s);
                    let r = run(&g.lat, m, &start, lim);
                    Some((gi, row(m, &g.lat, &start, lim, r)))
                })
                .collect()
        });
        // Merge in run order.
        let cp = &mut self.cp;
        for (k, (gi, r)) in rows.iter().enumerate() {
            let g = &mut cp.groups[*gi];
            g.runs += 1;
            g.steps += r.steps;
            self.session_steps += r.steps;
            *g.by_outcome.entry(r.outcome.name().into()).or_default() += 1;
            if r.outcome == Outcome::Highway {
                let o = r.onset.unwrap_or(0);
                if g.latest_onset.as_ref().is_none_or(|x| o > x.1) {
                    g.latest_onset = Some((r.rule.clone(), o));
                }
                let p = r.period.unwrap_or(0);
                if g.longest_period.as_ref().is_none_or(|x| p > x.1) {
                    g.longest_period = Some((r.rule.clone(), p));
                }
            }
            if matches!(r.outcome, Outcome::Highway | Outcome::Symmetric) {
                cp.newest.insert(
                    0,
                    AntFind {
                        id: r.id.clone(),
                        rule: r.rule.clone(),
                        tiling: r.tiling_name.clone(),
                        outcome: r.outcome,
                        period: r.period,
                        d: r.d,
                        onset: r.onset,
                        run: a + k as u64,
                        score: r.score,
                    },
                );
                cp.newest.truncate(20);
            }
        }
        cp.next = b;
        cp.elapsed_s = self.base_elapsed + self.session.elapsed().as_secs_f64();
        sink.ants.extend(rows.into_iter().map(|x| x.1));
        if self.adapt {
            let per = t0.elapsed().as_secs_f64() / (b - a).max(1) as f64;
            let want = (2.0 / per.max(1e-6)) as u64;
            self.batch = want.clamp(16, (self.batch * 4).max(16)).min(100_000);
        }
        Ok(self.cp.next >= self.total)
    }

    /// The progress with this status (`running`, `paused`, `done`, `failed`).
    pub fn progress(&self, status: &str) -> AntProgress {
        let cp = &self.cp;
        let secs = self.session.elapsed().as_secs_f64().max(1e-9);
        let elapsed = self.base_elapsed + self.session.elapsed().as_secs_f64();
        let mut p = AntProgress {
            job: self.job,
            status: status.into(),
            runs_done: cp.next,
            runs_total: self.total,
            runs_per_s: (((cp.next - self.session_start) as f64 / secs) * 10.0).round() / 10.0,
            steps_per_s: (self.session_steps as f64 / secs).round(),
            elapsed_s: (elapsed * 10.0).round() / 10.0,
            newest: cp.newest.clone(),
            groups: cp.groups.clone(),
            threads: self.threads,
            updated: now_utc(),
            ..Default::default()
        };
        for g in &cp.groups {
            p.steps += g.steps;
            for (k, v) in &g.by_outcome {
                *p.by_outcome.entry(k.clone()).or_default() += v;
            }
            if p.error.is_none() {
                p.error = g.error.clone();
            }
        }
        p
    }

    /// The checkpoint after the last batch.
    pub fn checkpoint(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.cp).map_err(|e| e.to_string())
    }

    /// Runs merged in this session.
    pub fn runs_run(&self) -> u64 {
        self.cp.next - self.session_start
    }
}
