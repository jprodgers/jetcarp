//! The job runner: one model for every long search, whatever its kind.
//!
//! **Jobs.** A job is a catalog `jobs` row: its kind, spec (JSON), status (`running`, `paused`,
//! `done`, `failed`), progress (JSON), and checkpoint (BLOB). [`create`] makes one from a
//! [`JobSpec`]: the strategy's name, its spec, a label, and a [`Budget`]. Everything
//! kind-specific is behind [`StrategyKind`] ([`kind`] looks one up by the `jobs.kind` column):
//! how to make a job of the kind, summarise one for the dashboard, find the heat maps it feeds
//! and its finds, ask one to stop from any process, and build a [`Strategy`], one live run of
//! it.
//!
//! **Running.** The [`Runner`] runs a job in this process. It takes the job's lease (see
//! [`Lease`]: a lock file beside the catalog, so two runners never own one job and a crashed run
//! can be resumed at once, plus the `owner` and `heartbeat` columns, renewed every half minute,
//! that show who runs it), builds the strategy (or takes one the caller built with its own
//! hooks), and runs it within the budget: after the budget's minutes, or once its items have
//! run, the job stops at its next checkpoint, paused and resumable. [`Runner::spawn`] runs it on
//! a new thread and returns a [`JobHandle`]: live progress, stop at the next checkpoint, wait.
//! Its [`Live`] tells observers each progress the run sets, and shows viewers ([`Viewer`]) what
//! a strategy draws of itself as it runs (a sandpile's board), so a front end can draw a run it
//! started through an operation. The [`Registry`] ([`registry`], one per process) keeps the
//! handles of the jobs running here.
//! A job is also stopped from any process by marking it `paused` ([`StrategyKind::request_stop`]);
//! the run sees that at its next checkpoint.
//!
//! **Strategies.** A strategy implements [`Strategy::restore`] (load the checkpoint),
//! [`Strategy::step`] (one batch, its results put in a [`Sink`]), [`Strategy::checkpoint`],
//! [`Strategy::progress`], and [`Strategy::finish`]. The runner's loop ([`drive`]) checks the
//! stop flag, the budget, and a pause from another process before each step, and after it writes
//! the sink's rows and then (told what became of the records, [`Strategy::stored`]) the progress,
//! the checkpoint, and the heartbeat in one transaction, only while the runner still holds the
//! job. Every kind runs this way, in steps of at most a few seconds, so a stop, a pause, or a
//! lost lease is seen within a step; a sandpile's doubling step, which cannot be cut smaller,
//! watches the stop flag itself and publishes its progress meanwhile.
//!
//! | `jobs.kind` | What | Module | A step | Item budget |
//! | --- | --- | --- | --- | --- |
//! | `hunt` | soups on the plane engine | `crate::hunt` | a batch of soups | soups |
//! | `search` | soups on a bounded grid | `crate::bounded` | soups, then checks | soups |
//! | `ants` | ant and turmite runs | `crate::ants` | a batch of runs | runs |
//! | `census` | rules by fate | `crate::census` | a chunk of rules | rules |
//! | `mover` | a mover on a sliding window | `crate::window` | a slice, to a window | – |
//! | `pile` | a large sandpile | `crate::pile` | a doubling step | – |
//! | `collide` | two patterns at every offset | `jetcarp_core::pattern::collide` | runs, then checks | runs |
//! | `evolve` | the evolutionary glider search | `jetcarp_core::evolve` | a generation | generations |
//! | `explore` | the broad search over tilings, families, rules, seeds | `crate::explore` | two seconds of rounds | runs |
//! | `reverify` | stored records checked again on this engine's geometry | [`reverify`] | two seconds of checks | records |
//! | `compounds` | stored records that are several independent objects, split into their parts | [`compounds`] | two seconds of checks | records |
//!
//! Batches adapt to a few seconds of work: a hunt's to about three (with the next batch already
//! running, so the slow soups at a batch's end, its merge, and its write overlap it), an ant
//! search's to about two, a census's chunks to about three. A bounded search hands out soups for
//! two seconds a step and a collision placements for one ([`prefix_until`]: in order, so a step
//! gathers a prefix, however much its items vary), then their finds are verified in batches. A
//! mover runs two seconds and on to the next window it builds, where it checkpoints. A pile's
//! doubling step is cancelled within a second of a stop and run again on resume.
//!
//! Crates above this one add kinds of their own with [`register`]: `pack_import` (a pack
//! imported into the library, in `jetcarp-pack`), which the first-run setup starts.
//!
//! Any other kind (and the `collide` jobs stored before collisions were strategies) is listed
//! and can be stopped, but not run. Every kind here but `explore` is deterministic: a job
//! stopped and resumed, in any process, ends exactly as an uninterrupted one
//! ([`StrategyKind::deterministic`]). An explore search shares its archive between its GPU and
//! CPU rounds as they run, so a stopped one *continues* from its saved archive instead.
//!
//! **Working folders.** A kind may keep a job's working state in a folder of its own beside the
//! catalog ([`StrategyKind::work_dir`]): an explore job's `searches/<uuid>/search.sqlite` holds
//! its runs and its archive, coverage, and seen set, and is its checkpoint.
//!
//! **Checkpoints.** Hunts, ant searches, censuses, and movers keep the checkpoints they had
//! before they ran in steps (the next soup, run, or rule index with the totals; a mover's window
//! frame), so jobs stopped before then resume as they were. A pile's state is its folder's step
//! files, as before. Evolve's is the generation checkpoint it has had since it became a job.
//! Bounded searches and collisions had no checkpoint (a stopped one started again); now theirs
//! is the state gathered so far (a search's deflated, `Z1` first), and a paused job from before
//! simply starts again. A checkpoint a kind cannot read fails the run with the reason, the job
//! left `failed` (there are no users with old jobs to keep).

mod lease;
mod registry;
mod runner;
mod sink;
mod strategy;

pub mod ants;
pub mod census;
pub mod collide;
pub mod compounds;
pub mod evolve;
pub mod explore;
pub mod hunt;
pub mod mover;
pub mod other;
pub mod pile;
pub mod reverify;
pub mod search;

pub use lease::{Lease, lock_path, owner_id};
pub use registry::{Registry, registry};
pub use runner::{JobHandle, Live, Observer, Outcome, RunOptions, Runner, Stopped, Viewer};
pub use sink::Sink;
pub use strategy::{RunCx, Step, Strategy, StrategyKind, drive};

use crate::dashboard::JobSummary;
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A new job: which strategy, its spec (the strategy's own JSON: a `HuntSpec` for `hunt`, an
/// `AntSpec` for `ants`, ...), a label to find it by, and the budget its runs get.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    /// The strategy's name, which is the `jobs.kind` it makes (see the module table).
    pub strategy: String,
    pub spec: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Budget::is_empty")]
    pub budget: Budget,
}

/// How long one run of a job may go on: it stops at its next checkpoint, paused and resumable,
/// after `minutes` of wall time, at the Unix time `until` (the same end for every run, however
/// often the job is resumed), or once `items` units (soups, runs, rules, generations: see the
/// module table) have run in this session; whichever comes first. Empty: until the job ends or
/// is stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
}

impl Budget {
    pub fn is_empty(&self) -> bool {
        self.minutes.is_none() && self.until.is_none() && self.items.is_none()
    }
    /// Only minutes.
    pub fn minutes(m: f64) -> Budget {
        Budget {
            minutes: Some(m),
            ..Default::default()
        }
    }
    /// Only items.
    pub fn items(n: u64) -> Budget {
        Budget {
            items: Some(n),
            ..Default::default()
        }
    }
    /// When the run must stop by its minutes or its `until`, from now.
    pub fn deadline(&self) -> Option<std::time::Instant> {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let by_minutes = self
            .minutes
            .map(|m| now + Duration::from_secs_f64(m * 60.0));
        let by_until = self.until.map(|u| {
            let t = now_seconds().max(0) as u64;
            now + Duration::from_secs(u.saturating_sub(t))
        });
        match (by_minutes, by_until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    /// Can a job of this kind run within it?
    pub fn check(&self, k: &dyn StrategyKind) -> Result<(), String> {
        if let Some(m) = self.minutes
            && !(m > 0.0 && m.is_finite())
        {
            return Err(format!("a budget's minutes must be above 0, not {m}"));
        }
        if self.items.is_some() && k.units().is_none() {
            return Err(format!(
                "{} jobs take a budget in minutes, not items",
                k.kind()
            ));
        }
        Ok(())
    }
    /// The budget stored with a job (`spec.budget`, set by [`create`]).
    pub fn of(row: &JobRow) -> Budget {
        row.spec
            .get("budget")
            .and_then(|b| serde_json::from_value(b.clone()).ok())
            .unwrap_or_default()
    }
}

static KINDS: [&dyn StrategyKind; 11] = [
    &hunt::KIND,
    &search::KIND,
    &ants::KIND,
    &census::KIND,
    &mover::KIND,
    &pile::KIND,
    &collide::KIND,
    &evolve::KIND,
    &explore::KIND,
    &reverify::KIND,
    &compounds::KIND,
];

/// Kinds that crates above this one added ([`register`]).
static EXTRA: std::sync::RwLock<Vec<&'static dyn StrategyKind>> =
    std::sync::RwLock::new(Vec::new());

/// Add a kind defined above this crate (`pack_import`, in `jetcarp-pack`), so its jobs are
/// made, listed, run and resumed like the others. A kind already known by its name is kept as
/// it is.
pub fn register(k: &'static dyn StrategyKind) {
    let mut v = EXTRA.write().unwrap_or_else(|e| e.into_inner());
    if KINDS.iter().chain(v.iter()).all(|x| x.kind() != k.kind()) {
        v.push(k);
    }
}

/// Every kind with a strategy, in the module table's order, then the kinds registered.
pub fn kinds() -> Vec<&'static dyn StrategyKind> {
    let extra = EXTRA.read().unwrap_or_else(|e| e.into_inner());
    KINDS.iter().copied().chain(extra.iter().copied()).collect()
}

/// The kind of a `jobs.kind` value (a generic one for kinds without their own).
pub fn kind(name: &str) -> &'static dyn StrategyKind {
    strategy(name).unwrap_or(&other::KIND)
}

/// The kind a strategy name makes, if there is one.
pub fn strategy(name: &str) -> Option<&'static dyn StrategyKind> {
    KINDS
        .iter()
        .copied()
        .find(|k| k.kind() == name)
        .or_else(|| {
            let extra = EXTRA.read().unwrap_or_else(|e| e.into_inner());
            extra.iter().copied().find(|k| k.kind() == name)
        })
}

/// Make a job (status `paused` until a runner starts it): the strategy checks and stores its
/// spec, then the label and the budget are stored with it. Returns the job id.
pub fn create(lib: &Library, js: &JobSpec) -> Result<i64, String> {
    let k = strategy(&js.strategy).ok_or_else(|| {
        format!(
            "no strategy '{}' (there are {})",
            js.strategy,
            kinds()
                .iter()
                .map(|k| k.kind())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    js.budget.check(k)?;
    let job = k.create(lib, &js.spec)?;
    if let Some(l) = &js.label {
        lib.set_job_label(job, l)?;
    }
    if !js.budget.is_empty() {
        let mut row = lib.job(job)?;
        row.spec["budget"] = serde_json::to_value(js.budget).map_err(|e| e.to_string())?;
        lib.set_job_spec(job, &row.spec)?;
    }
    Ok(job)
}

/// A job's spec as its kind's own type; the job must be of `kind`.
pub fn typed_spec<T: serde::de::DeserializeOwned>(
    lib: &Library,
    job: i64,
    kind: &str,
) -> Result<T, String> {
    let j = lib.job(job)?;
    if j.kind != kind {
        return Err(format!("job {job} is a {} job, not {kind}", j.kind));
    }
    serde_json::from_value(j.spec).map_err(|e| format!("job {job} spec: {e}"))
}

/// Ask a job to stop at its next checkpoint, in whatever process runs it: a running job is
/// marked `paused` (every run checks that at its checkpoints). Jobs that are not running are
/// left alone.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    if lib.job(job)?.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// Refuse a memory cap (`max_rss_gb`) that is not above 0.
pub fn check_memory_cap(cap: Option<f64>) -> Result<(), String> {
    match cap {
        Some(m) if m.is_nan() || m <= 0.0 => Err(format!("max_rss_gb must be above 0, not {m}")),
        _ => Ok(()),
    }
}

/// When the process uses more memory than a job's cap (its spec's `max_rss_gb`: hunts and
/// bounded searches check it before each step, as an explore search does), the note that says
/// so; the run then stops, paused and resumable at its last checkpoint.
pub fn over_memory(spec: &Value) -> Option<String> {
    let cap = spec.get("max_rss_gb").and_then(Value::as_f64)?;
    let rss = explore::rss_gb();
    (rss > cap).then(|| format!("memory {rss:.1} GB is over the cap of {cap} GB"))
}

/// Seconds since 1970 now.
pub(crate) fn now_seconds() -> i64 {
    crate::dashboard::now_seconds()
}

/// `running`, `paused`, `done`, `failed`.
pub fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

/// The status a name stands for.
pub fn status_of(name: &str) -> Option<JobStatus> {
    match name {
        "running" => Some(JobStatus::Running),
        "paused" => Some(JobStatus::Paused),
        "done" => Some(JobStatus::Done),
        "failed" => Some(JobStatus::Failed),
        _ => None,
    }
}

/// A summary's fields that come from the row alone.
pub(crate) fn base_summary(row: &JobRow, k: &dyn StrategyKind) -> JobSummary {
    JobSummary {
        id: row.id,
        kind: row.kind.clone(),
        kind_label: k.label().into(),
        status: status_name(row.status).into(),
        created: row.created.clone(),
        updated: row.updated.clone(),
        resumable: k.runnable(row),
        deterministic: k.deterministic(),
        imported_from: row
            .spec
            .get("imported_from")
            .and_then(|v| v.as_str())
            .filter(|_| row.kind != explore::KIND_NAME)
            .map(str::to_string),
        ..Default::default()
    }
}

/// Seconds left, when the job is running and has an end.
pub(crate) fn eta(done: u64, total: Option<u64>, per_s: f64, running: bool) -> Option<f64> {
    let t = total?;
    (running && per_s > 0.0 && t > done).then(|| (t - done) as f64 / per_s)
}

/// `B3/S23, B36/S23 (+3)`.
pub(crate) fn short_list(items: &[String], n: usize) -> String {
    let mut s = items.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if items.len() > n {
        s.push_str(&format!(" (+{})", items.len() - n));
    }
    s
}

/// A typed progress from a JSON one (the default when it does not read).
pub(crate) fn typed<T: serde::de::DeserializeOwned + Default>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// The number of worker threads `threads` (0: all cores) stands for.
pub(crate) fn thread_count(threads: usize) -> usize {
    if threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        threads
    }
}

/// Run `f` on the indices of `range` on every thread of `pool`, handing them out in order until
/// `deadline` passes or `stop` is set; every index handed out runs to its end. Returns the
/// results of those (a prefix of `range`, in order): a step of a job whose items vary a lot in
/// cost runs for about the time it is given, whatever its items.
pub(crate) fn prefix_until<T: Send>(
    pool: &rayon::ThreadPool,
    range: std::ops::Range<usize>,
    deadline: std::time::Instant,
    stop: &std::sync::atomic::AtomicBool,
    f: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(range.start);
    let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::new());
    pool.broadcast(|_| {
        // The time is checked before an index is taken, so the indices taken are a prefix.
        while !stop.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            let i = next.fetch_add(1, Ordering::SeqCst);
            if i >= range.end {
                break;
            }
            let r = f(i);
            done.lock().unwrap().push((i, r));
        }
    });
    let mut done = done.into_inner().unwrap();
    done.sort_unstable_by_key(|x| x.0);
    debug_assert!(done.iter().enumerate().all(|(k, x)| x.0 == range.start + k));
    done.into_iter().map(|x| x.1).collect()
}
