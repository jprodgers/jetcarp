//! Hunts as catalog jobs: batches of soups, merged in a fixed order, flushed to the catalog with
//! a checkpoint, resumable after a crash or reboot.
//!
//! A hunt is a `jobs` row of kind `hunt` whose `spec` is a [`HuntSpec`]. Soup `i` of every
//! (rule, tiling) group uses the RNG state `soup_state(seed, i)`. Soups run in batches of
//! consecutive indices (the batch size adapts to keep each near a second, and changes nothing
//! else); a batch's results are merged per group in soup order, so every count is independent of
//! the thread count and the batching, and each new pattern's record comes from the lowest soup
//! index that found it. A pattern already verified in the rule only gets a sighting; any other is
//! verified (`pattern::verify`) and stored. Then one transaction writes the records, the
//! sightings, the progress (JSON, a [`HuntProgress`]), and the checkpoint (the next soup index,
//! the totals, the ids that failed verification, and the newest finds), so a crash loses at most
//! the batch in flight and a resumed hunt ends with exactly the totals of an uninterrupted one.
//!
//! A hunt stops at the next batch when its stop flag is set (Ctrl-C in `jetcarp search hunt`), when another
//! process marks the job `paused` ([`request_stop`]), or when a session limit is reached; the batch
//! in flight is dropped and the job is left `paused` at its last checkpoint.
//!
//! The job runner (`crate::jobs::hunt`) runs a hunt in steps, one batch each ([`Hunt`]), under
//! its lease, in this process or on a thread of it; it writes each batch's rows with the
//! progress and checkpoint.

use crate::jobs::{RunCx, Sink};
use jetcarp_core::hunt::separate::{Cache, Census, Found};
use jetcarp_core::hunt::soup::{
    End, Layout, Limits, SoupBatcher, SoupOut, SoupSpec, run_soup, soup_state,
};
use jetcarp_core::pattern::{
    PatternClass, PatternRecord, Provenance, default_periods, now_utc, tiling_display_name,
    tiling_key, verify,
};
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};
use jetcarp_store::{AddOutcome, JobStatus, Library, Sighting, UnstoredFlush, UnstoredRow};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Everything that defines a hunt's results (threads, GPU, and batching do not).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HuntSpec {
    pub rules: Vec<String>,
    pub tilings: Vec<TilingSpec>,
    /// Neighbourhood for every rule; `None` lets the notation decide (`V` = edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood: Option<Nb>,
    /// Soups per rule and tiling; `None` runs until stopped.
    #[serde(default)]
    pub soups: Option<u64>,
    #[serde(default)]
    pub soup: SoupSpec,
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub seed: u64,
    /// A soup not settled by then is censused as it is (`unsettled`).
    pub max_gens: u64,
    /// Longest period looked for (objects, and the whole universe settling).
    pub max_period: u64,
    /// Live cells above which a soup counts as exploded (0: automatic, at least 2,000 and 8 per
    /// soup cell). A soup whose live cells spread over more than 16 times the soup's longer side
    /// (at least 512, at most 4,096 unit cells) also counts as exploded.
    #[serde(default)]
    pub pop_cap: u64,
    /// Empty margin around the soup (unit cells) that may fill before ships are cut (0:
    /// automatic, twice the soup's longer side and at least 16).
    #[serde(default)]
    pub margin: u32,
    /// Which new objects get full records (the rest are counted as `unstored` rows).
    #[serde(default)]
    pub store: StorePolicy,
    /// Stop (paused, resumable) when the process uses more memory than this many GB, as an
    /// explore search does ([`crate::jobs::over_memory`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rss_gb: Option<f64>,
}

/// Which new objects a hunt stores as full catalog records. Spaceships, guns, puffers, and
/// other classes always are. Still lifes and oscillators are when they are small (at most
/// `max_cells` cells and `period x cells` at most `max_volume`), or once this hunt has seen
/// them `min_seen` times in a rule (0: never). The others are kept as `unstored` rows (class,
/// period, cells, copies, first find; no phases): boiling rules leave huge numbers of large
/// one-off still lifes and oscillators, about 10 KB of record each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorePolicy {
    /// [`StorePolicy::all`]'s are `u64::MAX`, a decimal string in JSON.
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub max_cells: u64,
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub max_volume: u64,
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub min_seen: u64,
}

impl Default for StorePolicy {
    fn default() -> Self {
        StorePolicy {
            max_cells: 64,
            max_volume: 4000,
            min_seen: 2,
        }
    }
}

impl StorePolicy {
    /// Store everything (the policy before Phase 4 wave 4).
    pub fn all() -> StorePolicy {
        StorePolicy {
            max_cells: u64::MAX,
            max_volume: u64::MAX,
            min_seen: 1,
        }
    }
    /// Stored at first sight?
    pub fn admits(&self, class: PatternClass, period: u64, cells: u64) -> bool {
        !matches!(class, PatternClass::StillLife | PatternClass::Oscillator)
            || (cells <= self.max_cells && period.saturating_mul(cells) <= self.max_volume)
    }
}

impl Default for HuntSpec {
    fn default() -> Self {
        HuntSpec {
            rules: vec!["B3/S23".into()],
            tilings: vec![TilingSpec::new("square")],
            neighborhood: None,
            soups: Some(10_000),
            soup: SoupSpec::default(),
            seed: 1,
            max_gens: 20_000,
            max_period: 64,
            pop_cap: 0,
            margin: 0,
            store: StorePolicy::default(),
            max_rss_gb: None,
        }
    }
}

impl HuntSpec {
    pub fn check(&self) -> Result<(), String> {
        if self.rules.is_empty() || self.tilings.is_empty() {
            return Err("a hunt needs at least one rule and one tiling".into());
        }
        if self.soups == Some(0) {
            return Err("a hunt needs at least one soup".into());
        }
        if self.max_gens == 0 {
            return Err("max_gens must be at least 1".into());
        }
        if !(0.0..=1.0).contains(&self.soup.density) {
            return Err("density must be between 0 and 1".into());
        }
        crate::jobs::check_memory_cap(self.max_rss_gb)
    }
    /// The soup margin in use.
    pub fn margin(&self) -> u32 {
        if self.margin > 0 {
            self.margin
        } else {
            (2 * self.soup.w.max(self.soup.h)).max(16)
        }
    }
    /// The run limits for a group whose soups have `cells` candidate cells.
    pub fn limits(&self, cells: usize) -> Limits {
        Limits {
            max_gens: self.max_gens,
            settle_period: self.max_period.clamp(1, 1024),
            pop_cap: if self.pop_cap > 0 {
                self.pop_cap
            } else {
                (8 * cells as u64).max(2000)
            },
            max_span: (16 * self.soup.w.max(self.soup.h) as i32).clamp(512, 4096),
            cut_every: 16,
        }
    }
}

/// Counts for one (rule, tiling) group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupTotals {
    pub rule: String,
    pub tiling: String,
    pub tiling_key: String,
    pub soups: u64,
    pub settled: u64,
    pub died: u64,
    pub exploded: u64,
    pub unsettled: u64,
    pub generations: u64,
    /// Object copies (patterns), by class name.
    pub by_class: BTreeMap<String, u64>,
    /// Objects that are not patterns, by kind.
    pub other: BTreeMap<String, u64>,
    /// Ships cut out as they flew away.
    pub escaped: u64,
    /// Distinct patterns this hunt has seen in the group.
    pub distinct: u64,
    /// Patterns stored in the catalog for the first time.
    pub new_patterns: u64,
    /// Patterns that failed verification (counted once each, never stored).
    pub failed: u64,
    /// Copies of objects kept as `unstored` rows (by class), and how many such objects.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unstored: BTreeMap<String, u64>,
    #[serde(default)]
    pub unstored_distinct: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl GroupTotals {
    pub fn objects(&self) -> u64 {
        self.by_class.values().sum()
    }
}

/// A pattern this hunt saw for the first time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Find {
    pub id: String,
    pub label: String,
    pub class: String,
    pub period: u64,
    pub rule: String,
    pub tiling: String,
    pub soup: u64,
    /// The (rule, tiling) group, in spec order (tilings outer).
    pub group: usize,
    /// New to the catalog (not just to this hunt).
    pub new: bool,
    pub score: f64,
}

/// Progress of a hunt, written to the job's `progress` at every flush and kept live in memory
/// for a dashboard (`crate::jobs::JobHandle::progress`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HuntProgress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    /// Soups done per group (the next soup index).
    pub soups_done: u64,
    /// Soups per group, if the hunt has an end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soups_total: Option<u64>,
    /// Soups (over all groups) per second in this session.
    pub soups_per_s: f64,
    /// Seconds run, over every session.
    pub elapsed_s: f64,
    pub settled: u64,
    pub died: u64,
    pub exploded: u64,
    pub unsettled: u64,
    pub objects: u64,
    /// Object copies by class, over all groups.
    pub by_class: BTreeMap<String, u64>,
    pub distinct: u64,
    pub new_patterns: u64,
    pub failed: u64,
    /// Newest first-time finds (most recent first).
    pub newest: Vec<Find>,
    pub groups: Vec<GroupTotals>,
    pub threads: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why the run stopped when nobody asked it to (its memory cap).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub updated: String,
}

impl HuntProgress {
    /// `job 3: 12,288 soups (4,100/s), 41,233 objects, 312 distinct, 2 new | spaceship 1,840 ...`
    pub fn line(&self) -> String {
        let total = self
            .soups_total
            .map(|t| format!("/{}", group_digits(t)))
            .unwrap_or_default();
        // Soups count per rule-and-tiling group: "100/100 soups in each of 1,200 groups".
        let groups = if self.groups.len() > 1 {
            format!(
                " soups in each of {} groups",
                group_digits(self.groups.len() as u64)
            )
        } else {
            " soups".to_string()
        };
        let classes: Vec<String> = ["spaceship", "oscillator", "still life", "puffer", "gun"]
            .iter()
            .filter_map(|c| {
                self.by_class
                    .get(*c)
                    .map(|n| format!("{} {}", short_class(c), group_digits(*n)))
            })
            .collect();
        format!(
            "hunt {}: {}{}{}, {:.0} soups/s, {} objects ({}), {} distinct, {} new{}",
            self.job,
            group_digits(self.soups_done),
            total,
            groups,
            self.soups_per_s,
            group_digits(self.objects),
            classes.join(", "),
            group_digits(self.distinct),
            group_digits(self.new_patterns),
            self.newest
                .first()
                .map(|f| format!(" | newest: {} ({})", f.label, f.rule))
                .unwrap_or_default()
        )
    }
}

fn short_class(c: &str) -> &str {
    match c {
        "spaceship" => "ships",
        "oscillator" => "osc",
        "still life" => "still",
        "puffer" => "puffers",
        "gun" => "guns",
        x => x,
    }
}

fn group_digits(n: u64) -> String {
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
    /// Per group: ids that failed verification (never retried).
    pub bad: Vec<Vec<String>>,
    pub newest: Vec<Find>,
    pub elapsed_s: f64,
}

/// Create a hunt job (status `paused` until a runner starts it). Returns the job id.
pub fn start(lib: &Library, spec: &HuntSpec) -> Result<i64, String> {
    spec.check()?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job("hunt", &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

/// Ask a running hunt (in any process) to stop at its next batch.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let j = lib.job(job)?;
    if j.kind != "hunt" {
        return Err(format!("job {job} is a {} job, not a hunt", j.kind));
    }
    if j.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// A hunt's spec and last saved progress, from the catalog.
pub fn status(lib: &Library, job: i64) -> Result<(HuntSpec, HuntProgress), String> {
    let j = lib.job(job)?;
    if j.kind != "hunt" {
        return Err(format!("job {job} is a {} job, not a hunt", j.kind));
    }
    let spec: HuntSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    let mut p: HuntProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.status = name_of_status(j.status).into();
    if p.soups_total.is_none() {
        p.soups_total = spec.soups;
    }
    Ok((spec, p))
}

fn name_of_status(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

/// What the workers need for one group (shared, read-only).
struct Runner {
    census: Arc<Census>,
    layout: Layout,
    limits: Limits,
}

struct Group {
    run: Arc<Runner>,
    rule_id: i64,
    tiling: TilingSpec,
    /// Patterns already verified in this rule (fast path: sightings only).
    known: HashSet<String>,
    /// Patterns this hunt has seen in this group.
    seen: HashSet<String>,
    bad: HashSet<String>,
    /// Objects kept as `unstored` rows (not stored yet), by id.
    pending: HashMap<String, UnstoredRow>,
}

/// A pool of per-thread caches, one per group, reused across batches; each cache holds at
/// most `.2` bytes (estimated).
struct CachePool(Mutex<Vec<Vec<Cache>>>, usize, usize);

/// Bytes all of a hunt's caches may hold together (`JETCARP_HUNT_CACHE_MB`, default 1,536 MB).
fn cache_budget() -> usize {
    std::env::var("JETCARP_HUNT_CACHE_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1536)
        << 20
}

impl CachePool {
    fn take(&self) -> Vec<Cache> {
        self.0
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| (0..self.1).map(|_| Cache::with_budget(self.2)).collect())
    }
    fn give(&self, c: Vec<Cache>) {
        self.0.lock().unwrap().push(c);
    }
}

struct Lease<'a>(&'a CachePool, Option<Vec<Cache>>);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.1.take() {
            self.0.give(c);
        }
    }
}

/// A batch of soups running on a thread of its own.
struct Flight {
    a: u64,
    b: u64,
    t0: Instant,
    handle: std::thread::JoinHandle<Option<Vec<Vec<SoupOut>>>>,
}

/// A hunt run in steps by the job runner (`crate::jobs::hunt`): each step merges one batch of
/// soups (all groups, consecutive indices) into the totals and puts its records, sightings, and
/// unstored objects in the sink, which the runner writes with the progress and checkpoint in
/// one transaction. Two batches are in flight: the next one runs while the current one's
/// slowest soups finish, while it is merged and written, and (with a GPU) while its prefixes
/// overlap the CPU work. A stop sets the run's stop flag, which every soup checks: the batches
/// in flight are dropped (the job resumes from its last checkpoint).
pub struct Hunt {
    job: i64,
    spec: Arc<HuntSpec>,
    cp: Checkpoint,
    groups: Vec<Option<Group>>,
    runners: Arc<Vec<Option<Arc<Runner>>>>,
    pool: Arc<rayon::ThreadPool>,
    caches: Arc<CachePool>,
    threads: usize,
    gpu: Option<Arc<dyn SoupBatcher>>,
    gpu_name: Option<String>,
    /// Soups per batch, and whether it adapts (to about three seconds a batch).
    batch: u64,
    adapt: bool,
    session: Instant,
    session_start: u64,
    base_elapsed: f64,
    ngroups: u64,
    date: String,
    /// The first soup index not launched yet.
    next: u64,
    inflight: VecDeque<Flight>,
    /// Set when the hunt is dropped, so the batches in flight end at once.
    cancel: Arc<AtomicBool>,
    /// (group, id) per record of the batch being written.
    owners: Vec<(usize, String)>,
}

impl Hunt {
    /// Open the run's hunt job (on its threads and GPU; `batch` soups per batch, 0: adapt)
    /// from its checkpoint, if it has one (its format is unchanged since hunts became jobs).
    pub fn open(cx: &RunCx<'_>, checkpoint: Option<&[u8]>, batch: u64) -> Result<Hunt, String> {
        let (lib, row, job, gpu) = (cx.lib, cx.row, cx.job, cx.gpu.clone());
        if row.kind != "hunt" {
            return Err(format!("job {job} is a {} job, not a hunt", row.kind));
        }
        let spec: HuntSpec =
            serde_json::from_value(row.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
        spec.check()?;
        let mut cp: Checkpoint = match checkpoint {
            Some(b) => {
                serde_json::from_slice(b).map_err(|e| format!("job {job} checkpoint: {e}"))?
            }
            None => Checkpoint::default(),
        };
        // Groups: every tiling x rule.
        let mut groups: Vec<Option<Group>> = Vec::new();
        let fresh = cp.groups.is_empty();
        if !fresh && cp.groups.len() != spec.tilings.len() * spec.rules.len() {
            return Err(format!("job {job}: the checkpoint does not match its spec"));
        }
        for (ti, t) in spec.tilings.iter().enumerate() {
            for (ri, r) in spec.rules.iter().enumerate() {
                let gi = ti * spec.rules.len() + ri;
                let made = (|| -> Result<Group, String> {
                    let census = Census::new(r, t, spec.neighborhood, spec.max_period)?;
                    let layout = Layout::new(&census.topo, spec.soup, spec.margin())?;
                    let limits = spec.limits(layout.cells.len());
                    let rule_id = lib.rule_id(&census.notation, census.nb, 1)?;
                    let known: HashSet<String> = lib.rule_patterns(rule_id)?.into_iter().collect();
                    let pending: HashMap<String, UnstoredRow> = if fresh {
                        HashMap::new()
                    } else {
                        lib.unstored(job, rule_id)?
                            .into_iter()
                            .map(|r| (r.pattern_id.clone(), r))
                            .collect()
                    };
                    let mut seen: HashSet<String> = if fresh {
                        HashSet::new()
                    } else {
                        lib.job_sightings(job)?
                            .into_iter()
                            .filter(|x| x.1 == census.notation)
                            .map(|x| x.0)
                            .collect()
                    };
                    seen.extend(pending.keys().cloned());
                    let mut tiling = jetcarp_core::pattern::tiling_identity(t);
                    tiling.size = Some(SizeSpec::Rect {
                        w: spec.soup.w,
                        h: spec.soup.h,
                    });
                    Ok(Group {
                        run: Arc::new(Runner {
                            census: Arc::new(census),
                            layout,
                            limits,
                        }),
                        bad: cp
                            .bad
                            .get(gi)
                            .map(|v| v.iter().cloned().collect())
                            .unwrap_or_default(),
                        rule_id,
                        tiling,
                        known,
                        seen,
                        pending,
                    })
                })();
                if fresh {
                    cp.groups.push(GroupTotals {
                        rule: r.clone(),
                        tiling: tiling_display_name(t),
                        tiling_key: tiling_key(t),
                        ..Default::default()
                    });
                    cp.bad.push(vec![]);
                }
                match made {
                    Ok(g) => {
                        if fresh {
                            cp.groups[gi].rule = g.run.census.notation.clone();
                        }
                        groups.push(Some(g));
                    }
                    Err(e) => {
                        cp.groups[gi].error = Some(e);
                        groups.push(None);
                    }
                }
            }
        }
        if groups.iter().all(|g| g.is_none()) {
            let msg = cp
                .groups
                .iter()
                .filter_map(|g| g.error.clone())
                .next()
                .unwrap_or_default();
            return Err(format!("hunt {job}: no group can run: {msg}"));
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
        // Two batches in flight, each with a cache per thread and group.
        let per_cache =
            (cache_budget() / (2 * threads * groups.len()).max(1)).clamp(1 << 20, 64 << 20);
        let caches = CachePool(Mutex::new(Vec::new()), groups.len(), per_cache);
        let runners: Vec<Option<Arc<Runner>>> = groups
            .iter()
            .map(|g| g.as_ref().map(|g| g.run.clone()))
            .collect();
        let ngroups = runners.iter().flatten().count() as u64;
        Ok(Hunt {
            job,
            spec: Arc::new(spec),
            session_start: cp.next,
            base_elapsed: cp.elapsed_s,
            next: cp.next,
            cp,
            groups,
            runners: Arc::new(runners),
            pool: Arc::new(pool),
            caches: Arc::new(caches),
            threads,
            gpu_name: gpu.as_ref().map(|g| g.name()),
            gpu,
            batch: if batch > 0 {
                batch
            } else {
                (threads as u64 * 8).max(64)
            },
            adapt: batch == 0,
            session: Instant::now(),
            ngroups,
            date: now_utc(),
            inflight: VecDeque::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            owners: Vec::new(),
        })
    }

    /// One step: launch batches while fewer than two are in flight (none past the end, past
    /// the run's item budget, or once it is stopped or paused elsewhere), then merge the
    /// oldest into the totals and put its rows in the sink. `Ok(true)`: the hunt has run its
    /// last soup. A batch cut short by the stop flag is dropped with every later one, and
    /// nothing is merged.
    pub fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<bool, String> {
        let (total, limit, stop) = (self.spec.soups, cx.items, &cx.stop);
        while self.inflight.len() < 2 {
            if total.is_some_and(|t| self.next >= t) {
                break;
            }
            if cx.stopped()
                || limit.is_some_and(|l| self.next >= self.session_start + l)
                || cx.paused_elsewhere()
            {
                break;
            }
            let a = self.next;
            let mut b = a + self.batch;
            if let Some(t) = total {
                b = b.min(t);
            }
            if let Some(l) = limit {
                b = b.min(self.session_start + l);
            }
            self.launch(a, b, stop.clone())?;
            self.next = b;
        }
        let Some(f) = self.inflight.pop_front() else {
            return Ok(total.is_some_and(|t| self.cp.next >= t));
        };
        let (a, b) = (f.a, f.b);
        let outs = f
            .handle
            .join()
            .map_err(|_| format!("hunt {}: the batch of soups {a}..{b} panicked", self.job))?;
        let Some(outs) = outs else {
            // Stopped part way: drop this batch and every later one.
            for f in self.inflight.drain(..) {
                let _ = f.handle.join();
            }
            self.next = self.cp.next;
            return Ok(false);
        };
        let dt = f.t0.elapsed().as_secs_f64();
        let (groups, cp, spec, date) = (&mut self.groups, &mut self.cp, &self.spec, &self.date);
        let flush = self.pool.install(|| merge(groups, cp, outs, spec, date));
        self.cp.next = b;
        self.cp.elapsed_s = self.base_elapsed + self.session.elapsed().as_secs_f64();
        sink.verified.extend(flush.records);
        sink.sightings.extend(flush.sightings);
        sink.unstored.add.extend(flush.unstored.add);
        sink.unstored.remove.extend(flush.unstored.remove);
        self.owners = flush.owners;
        if self.adapt {
            // Aim for a few seconds per batch (two run at once).
            let per = dt / (b - a).max(1) as f64;
            let want = (3.0 / per.max(1e-6)) as u64;
            self.batch = want.clamp(16, (self.batch * 4).max(16)).min(1 << 20);
        }
        Ok(total.is_some_and(|t| self.cp.next >= t))
    }

    fn launch(&mut self, a: u64, b: u64, stop: Arc<AtomicBool>) -> Result<(), String> {
        let (runners, spec, pool, caches) = (
            self.runners.clone(),
            self.spec.clone(),
            self.pool.clone(),
            self.caches.clone(),
        );
        let (gpu, cancel) = (self.gpu.clone(), self.cancel.clone());
        let handle = std::thread::Builder::new()
            .name(format!("hunt-{}-soups", self.job))
            .spawn(move || {
                run_batch(
                    &runners,
                    a,
                    b,
                    &spec,
                    gpu.as_ref(),
                    [&*stop, &*cancel],
                    &caches,
                    &pool,
                )
            })
            .map_err(|e| format!("hunt {}: {e}", self.job))?;
        self.inflight.push_back(Flight {
            a,
            b,
            t0: Instant::now(),
            handle,
        });
        Ok(())
    }

    /// What became of the last batch's records: the stored ones are known now (sightings
    /// only from here on), a failure is the group's error.
    pub fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        for ((gi, id), r) in std::mem::take(&mut self.owners).into_iter().zip(outcomes) {
            match r {
                Err(e) => {
                    self.cp.groups[gi].error.get_or_insert(e.clone());
                }
                Ok(_) => {
                    if let Some(g) = self.groups[gi].as_mut() {
                        g.known.insert(id);
                    }
                }
            }
        }
    }

    /// The progress with this status (`running`, `paused`, `done`, `failed`).
    pub fn progress(&self, status: &str) -> HuntProgress {
        let secs = self.session.elapsed().as_secs_f64();
        let rate = ((self.cp.next - self.session_start) * self.ngroups) as f64 / secs.max(1e-9);
        make_progress(
            self.job,
            &self.spec,
            &self.cp,
            self.base_elapsed + secs,
            (self.threads, &self.gpu_name),
            rate,
            status,
        )
    }

    /// The checkpoint after the last batch merged.
    pub fn checkpoint(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.cp).map_err(|e| e.to_string())
    }

    /// Soup indices merged in this session.
    pub fn soups_run(&self) -> u64 {
        self.cp.next - self.session_start
    }
}

impl Drop for Hunt {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        for f in self.inflight.drain(..) {
            let _ = f.handle.join();
        }
    }
}

fn make_progress(
    job: i64,
    spec: &HuntSpec,
    cp: &Checkpoint,
    elapsed_s: f64,
    (threads, gpu): (usize, &Option<String>),
    rate: f64,
    status: &str,
) -> HuntProgress {
    let mut p = HuntProgress {
        job,
        status: status.into(),
        soups_done: cp.next,
        soups_total: spec.soups,
        soups_per_s: (rate * 10.0).round() / 10.0,
        elapsed_s: (elapsed_s * 10.0).round() / 10.0,
        newest: cp.newest.clone(),
        groups: cp.groups.clone(),
        threads,
        gpu: gpu.clone(),
        updated: now_utc(),
        ..Default::default()
    };
    for g in &cp.groups {
        p.settled += g.settled;
        p.died += g.died;
        p.exploded += g.exploded;
        p.unsettled += g.unsettled;
        p.objects += g.objects();
        for (k, v) in &g.by_class {
            *p.by_class.entry(k.clone()).or_default() += v;
        }
        p.distinct += g.distinct;
        p.new_patterns += g.new_patterns;
        p.failed += g.failed;
        if p.error.is_none() {
            p.error = g.error.clone();
        }
    }
    p
}

/// Run soups `a..b` of every group; `None` if stopped part way (either flag set).
#[allow(clippy::too_many_arguments)]
fn run_batch(
    groups: &[Option<Arc<Runner>>],
    a: u64,
    b: u64,
    spec: &HuntSpec,
    gpu: Option<&Arc<dyn SoupBatcher>>,
    stops: [&AtomicBool; 2],
    caches: &CachePool,
    pool: &rayon::ThreadPool,
) -> Option<Vec<Vec<SoupOut>>> {
    // The GPU is called from this batch's own thread, never from inside the pool: it takes a
    // lock, and a pool worker blocked on it could be the one its holder is waiting for.
    let mut out = Vec::with_capacity(groups.len());
    for (gi, g) in groups.iter().enumerate() {
        let Some(g) = g else {
            out.push(vec![]);
            continue;
        };
        let prefixes = gpu.and_then(|gpu| {
            if !gpu.supports(&g.census, g.layout.region) {
                return None;
            }
            let soups: Vec<Vec<jetcarp_core::objects::LCell>> = (a..b)
                .into_par_iter()
                .map(|i| g.layout.soup(soup_state(spec.seed, i)))
                .collect();
            gpu.prefix(&g.census, g.layout.region, g.limits, &soups)
                .ok()
        });
        let res: Vec<Option<SoupOut>> = pool.install(|| {
            (a..b)
                .into_par_iter()
                .map_init(
                    || {
                        let c = caches.take();
                        Lease(caches, Some(c))
                    },
                    |lease, i| {
                        if stops.iter().any(|s| s.load(Ordering::Relaxed)) {
                            return None;
                        }
                        let cache = &mut lease.1.as_mut().unwrap()[gi];
                        let state = soup_state(spec.seed, i);
                        Some(match &prefixes {
                            Some(p) => jetcarp_core::hunt::soup::continue_soup(
                                &g.census,
                                cache,
                                &g.layout,
                                g.limits,
                                i,
                                state,
                                p[(i - a) as usize].clone(),
                            ),
                            None => run_soup(&g.census, cache, &g.layout, g.limits, i, state),
                        })
                    },
                )
                .collect()
        });
        let res: Option<Vec<SoupOut>> = res.into_iter().collect();
        out.push(res?);
    }
    Some(out)
}

struct Flush {
    records: Vec<(PatternRecord, u64)>,
    sightings: Vec<Sighting>,
    unstored: UnstoredFlush,
    /// (group, id) per record.
    owners: Vec<(usize, String)>,
}

/// Merge a batch into the totals (per group, in soup order) and prepare the flush.
fn merge(
    groups: &mut [Option<Group>],
    cp: &mut Checkpoint,
    outs: Vec<Vec<SoupOut>>,
    spec: &HuntSpec,
    date: &str,
) -> Flush {
    let mut flush = Flush {
        records: vec![],
        sightings: vec![],
        unstored: UnstoredFlush::default(),
        owners: vec![],
    };
    let mut finds: Vec<Find> = Vec::new();
    let mut staged: Vec<(u64, usize, String, PatternRecord, u64)> = Vec::new();
    for (gi, outs) in outs.into_iter().enumerate() {
        let Some(g) = groups[gi].as_mut() else {
            continue;
        };
        let tot = &mut cp.groups[gi];
        // id -> (copies, soups, first soup, its found, rng state, generation)
        type First = (u64, u64, u64, Arc<Found>, u64, u64);
        let mut per: HashMap<String, First> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for o in &outs {
            tot.soups += 1;
            tot.generations += o.gens;
            match o.end {
                End::Settled => tot.settled += 1,
                End::Died => tot.died += 1,
                End::Exploded => tot.exploded += 1,
                End::Unsettled => tot.unsettled += 1,
            }
            tot.escaped += o.escaped as u64;
            for (k, n) in ["other (dies)", "other (grows)", "other (unsettled)"]
                .iter()
                .zip(o.other)
            {
                if n > 0 {
                    *tot.other.entry(k.to_string()).or_default() += n as u64;
                }
            }
            let mut in_soup: HashSet<&str> = HashSet::new();
            for f in &o.objects {
                if g.bad.contains(&f.id) {
                    continue;
                }
                *tot.by_class.entry(f.class.name().to_string()).or_default() += 1;
                let e = per.entry(f.id.clone()).or_insert_with(|| {
                    order.push(f.id.clone());
                    (0, 0, o.index, f.clone(), o.rng_state, o.gens)
                });
                e.0 += 1;
                if in_soup.insert(&f.id) {
                    e.1 += 1;
                }
            }
        }
        // Guns bring their ship along (stored with no copies if new).
        let mut ships: Vec<(Arc<Found>, u64, u64, u64)> = Vec::new();
        for id in &order {
            let e = &per[id];
            if e.3.class == PatternClass::Gun
                && let Some(gi2) = &e.3.record.gun
                && !per.contains_key(&gi2.ship)
                && !g.known.contains(&gi2.ship)
            {
                // The ship's record: rebuilt from the gun's stream phases.
                if let Some(rec) = ship_record(&g.run.census, gi2) {
                    ships.push((rec, e.2, e.4, e.5));
                }
            }
        }
        // New patterns: verify (in parallel) and store; known ones: sightings.
        let mut fresh: Vec<(String, PatternRecord, u64)> = Vec::new();
        let policy = spec.store;
        for id in &order {
            let (mut count, mut soups, mut first, f, mut rng, mut gens) = per[id].clone();
            let first_time = g.seen.insert(id.clone());
            if first_time {
                tot.distinct += 1;
            }
            // The storage policy: a large still life or oscillator waits as an unstored row
            // until it has been seen `min_seen` times; then it is stored from its first find.
            let known = g.known.contains(id);
            let mut promoted = false;
            if !known && !policy.admits(f.class, f.period, f.record.cells as u64) {
                let prev = g.pending.get(id).cloned();
                let total = prev.as_ref().map_or(0, |p| p.count) + count;
                let class = f.class.name().to_string();
                if policy.min_seen > 0 && total >= policy.min_seen {
                    if let Some(p) = prev {
                        g.pending.remove(id);
                        flush.unstored.remove.push((g.rule_id, id.clone()));
                        tot.unstored_distinct = tot.unstored_distinct.saturating_sub(1);
                        if let Some(n) = tot.unstored.get_mut(&class) {
                            *n = n.saturating_sub(p.count);
                        }
                        count += p.count;
                        soups += p.soups;
                        first = p.first_soup;
                        rng = p.rng_state;
                        gens = p.generation;
                    }
                    promoted = true;
                } else {
                    let row = UnstoredRow {
                        rule_id: g.rule_id,
                        pattern_id: id.clone(),
                        class: f.class,
                        period: f.period,
                        cells: f.record.cells as u64,
                        count,
                        soups,
                        first_soup: first,
                        rng_state: rng,
                        generation: gens,
                    };
                    match g.pending.get_mut(id) {
                        Some(p) => {
                            p.count += count;
                            p.soups += soups;
                        }
                        None => {
                            g.pending.insert(id.clone(), row.clone());
                            tot.unstored_distinct += 1;
                        }
                    }
                    *tot.unstored.entry(class).or_default() += count;
                    flush.unstored.add.push(row);
                    continue;
                }
            }
            if first_time || promoted {
                finds.push(Find {
                    id: id.clone(),
                    label: f.label.clone(),
                    class: f.class.name().into(),
                    period: f.period,
                    rule: g.run.census.notation.clone(),
                    tiling: tot.tiling.clone(),
                    soup: first,
                    group: gi,
                    new: !g.known.contains(id),
                    score: f.record.score,
                });
            }
            if g.known.contains(id) {
                flush.sightings.push(Sighting {
                    pattern_id: id.clone(),
                    rule_id: g.rule_id,
                    count,
                    soups,
                });
            } else {
                let rec = provenance(&f.record, g, spec, first, rng, gens, count, date);
                fresh.push((id.clone(), rec, soups));
            }
        }
        for (f, first, rng, gens) in ships {
            let rec = provenance(&f.record, g, spec, first, rng, gens, 0, date);
            fresh.push((f.id.clone(), rec, 0));
        }
        let checked: Vec<(String, Result<PatternRecord, String>, u64)> = fresh
            .into_par_iter()
            .map(|(id, mut rec, soups)| {
                let r = verify(&rec, default_periods(rec.period)).map(|v| {
                    rec.verified = v;
                    rec
                });
                (id, r, soups)
            })
            .collect();
        for (id, r, soups) in checked {
            match r {
                Ok(rec) => {
                    if !g.known.contains(&id) {
                        tot.new_patterns += 1;
                    }
                    staged.push((rec.provenance.soup.unwrap_or(0), gi, id, rec, soups));
                }
                Err(_) => {
                    tot.failed += 1;
                    // Its copies were counted above; take them back out.
                    if let Some(e) = per.get(&id)
                        && let Some(n) = tot.by_class.get_mut(e.3.class.name())
                    {
                        *n -= e.0;
                    }
                    g.bad.insert(id.clone());
                    cp.bad[gi].push(id);
                }
            }
        }
    }
    // Stored in order of first find (soup, then group), so a pattern found new by several
    // groups in one batch keeps the provenance and default rule of the lowest, as it would with
    // any other batching.
    staged.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    for (_, gi, id, rec, soups) in staged {
        flush.owners.push((gi, id));
        flush.records.push((rec, soups));
    }
    // Newest first (by soup, then group, then id), at most 32.
    finds.append(&mut cp.newest);
    finds.sort_by(|a, b| (b.soup, b.group, &b.id).cmp(&(a.soup, a.group, &a.id)));
    finds.dedup_by(|a, b| a.id == b.id && a.group == b.group);
    finds.truncate(32);
    cp.newest = finds;
    flush
}

#[allow(clippy::too_many_arguments)]
fn provenance(
    rec: &PatternRecord,
    g: &Group,
    spec: &HuntSpec,
    soup: u64,
    rng: u64,
    gens: u64,
    count: u64,
    date: &str,
) -> PatternRecord {
    let mut r = rec.clone();
    r.provenance = Provenance {
        source: "hunt".into(),
        tiling: Some(g.tiling.clone()),
        rng_state: Some(rng),
        soup: Some(soup),
        density: Some(spec.soup.density),
        generation: Some(gens),
        shape: Some(spec.soup.shape),
        seed: Some(spec.seed),
        jetcarp_version: jetcarp_core::VERSION.into(),
        date: date.into(),
    };
    r.seen = count;
    r.rescore();
    r
}

/// A gun's ship as a record (from the stream phases it stores).
fn ship_record(c: &Census, info: &jetcarp_core::pattern::GunInfo) -> Option<Arc<Found>> {
    let start = jetcarp_core::pattern::decode_cells(info.ship_phases.first()?).ok()?;
    match c.run_alone(&start, c.max_period.max(info.ship_period)) {
        jetcarp_core::hunt::separate::Run::Periodic { period, d, phases } if d != [0, 0] => {
            let rec = c.record(PatternClass::Spaceship, period, d, &phases).ok()?;
            let mut cache = Cache::default();
            Some(c.intern(&mut cache, rec))
        }
        _ => None,
    }
}

#[cfg(test)]
mod progress_line_tests {
    use super::*;

    /// Issue #8: a hunt over many rule-and-tiling groups says its soups are per group.
    #[test]
    fn the_progress_line_counts_soups_per_group() {
        let mut p = HuntProgress {
            job: 21,
            soups_done: 100,
            soups_total: Some(100),
            distinct: 82_445,
            new_patterns: 75_657,
            ..Default::default()
        };
        p.groups = vec![Default::default(); 1200];
        let line = p.line();
        assert!(
            line.starts_with("hunt 21: 100/100 soups in each of 1,200 groups, "),
            "{line}"
        );
        assert!(line.contains("82,445 distinct, 75,657 new"), "{line}");
        p.groups.truncate(1);
        assert!(
            p.line().starts_with("hunt 21: 100/100 soups, "),
            "{}",
            p.line()
        );
    }
}
