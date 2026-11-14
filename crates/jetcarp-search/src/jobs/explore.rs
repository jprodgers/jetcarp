//! Explore searches (`jobs.kind` `explore`): the broad quality-diversity search over tilings, rule
//! families, rules, and seeds ([`crate::explore::job::Search`]) as a catalog job, stopped,
//! budgeted, and carried on like the others.
//!
//! **Its folder.** A job's working state is its own folder, `searches/<uuid>/` beside the library
//! ([`work_dir`]; the uuid is made with the job and kept in its spec): its runs, sealed into
//! Parquet segments a million at a time (`runs/`, listed in `manifest.json`;
//! [`crate::explore::runs`]), and its working database `search.sqlite` with the newest runs and
//! the search's archive, confirmed set, coverage, family totals, seen set, and counters
//! ([`crate::explore::store`]). Each step writes its runs and what they changed there in one
//! transaction, then seals a segment if a million runs are waiting; stopping seals nothing. The
//! library row holds the progress, and no checkpoint (the folder is the checkpoint). A resumed
//! job loads the saved state, never its runs, so a search of millions of runs carries on within
//! about a second. Reviews of its runs are ratings in the library
//! ([`crate::explore::reviews`]).
//!
//! **Not deterministic.** The GPU and CPU rounds share the archive as they run, so a stopped
//! search *continues* from its archive rather than resuming exactly as an uninterrupted one
//! would ([`StrategyKind::deterministic`] is false; the dashboard and `jobs.status` say so).
//! Each run it stores replays exactly from its tiling, rule, seed, and random state.
//!
//! **A step** runs about [`STEP_S`] seconds of rounds: GPU rounds on a thread of their own when
//! the run has a GPU ([`ExploreRun::with_backend`]; the job's `gpu` or the run's asks for one),
//! CPU rounds on the run's threads. A GPU that fails three steps in a row is restarted; one that
//! cannot be, or fails forty times, is dropped and the CPU takes its families. Past `max_rss_gb`
//! of memory the run stops (paused). The item budget counts screened runs. A time budget, and a
//! Quick Start build's minutes, end the run on time: the last step ends with it, runs still going
//! on the processor are given up unrecorded, and the GPU's batches are sized to end before it
//! (one that would not is given up part way; see [`Search::step`]); a run on the processor is
//! capped at [`CPU_RUN_S`](crate::explore::job::CPU_RUN_S) seconds anyway. A step under way
//! writes the job's progress every [`PUBLISH_S`] seconds, and stops when another process pauses
//! the job.
//!
//! **Its threads** ([`worker_threads`]): every core but [`FREE_CORES`] unless the run is given a
//! thread count, a third of them for the GPU's batches when there is a GPU, so the computer
//! stays usable while it searches. `jobs.status` says how many.
//!
//! **A Quick Start build** (`quickstart.build`, and the first-run setup) is an explore job with
//! an end: its spec's [`BuildSpec`] gives the minutes it searches, counted over every session
//! (so a stopped build resumes where it was), and how its atlas is picked. Once its minutes are
//! spent the job is done: its atlas is published into the library's Quick Start
//! (`Library::publish_atlas`, as `quickstart.publish` does) and its progress says what was
//! published. Any other explore job runs until it is stopped or out of budget.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, short_list};
use crate::dashboard::JobSummary;
use crate::explore::job::{FamilyLine, Search, Settings, StepLimits};
use crate::explore::runs::{Runs, SEAL_ROWS};
use crate::explore::store::{self, LoadStats};
use jetcarp_core::explore::batch::Backend;
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct ExploreKind;
pub(super) static KIND: ExploreKind = ExploreKind;

pub const KIND_NAME: &str = "explore";

/// Seconds of rounds a step runs (a round under way runs to its end).
pub const STEP_S: f64 = 2.0;

/// Seconds between the progress a step writes while it runs. A step can last minutes (a round
/// of confirmations under way runs to its end), so the job's progress (its time, its runs) is
/// written this often meanwhile, and a pause asked by another process is seen as soon.
pub const PUBLISH_S: f64 = 2.0;

/// Cores an explore search leaves for the computer's other work when it is given no thread
/// count.
pub const FREE_CORES: usize = 2;

/// The computer's cores (logical processors this process may use).
pub fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// An explore search's worker threads: the runs on the processor's, and the GPU's (preparing
/// its batches' worlds and measuring their runs; 0 without a GPU). They are `threads` in all
/// when it is given, else every core less [`FREE_CORES`] (at least one), so the computer stays
/// usable while it searches. A third of them (at least one) are the GPU's.
pub fn worker_threads(threads: usize, gpu: bool) -> (usize, usize) {
    let all = if threads == 0 {
        cores().saturating_sub(FREE_CORES).max(1)
    } else {
        threads
    };
    match (gpu, all) {
        (false, n) => (n, 0),
        // One thread: the two pools take turns with it.
        (true, n) if n < 2 => (1, 1),
        (true, n) => {
            let g = n.div_ceil(3);
            (n - g, g)
        }
    }
}

fn worker_pool(threads: usize, name: &'static str) -> Result<rayon::ThreadPool, String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .thread_name(move |i| format!("{name}-{i}"))
        .build()
        .map_err(|e| e.to_string())
}

/// An explore job: the search's settings, and how to run it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExploreSpec {
    #[serde(flatten)]
    pub search: Settings,
    /// Asked for the GPU when it was made: run the rules a GPU can run there. With neither this
    /// nor `cpu`, JETCARP's GPU setting decides at each run (a run's own request wins over both).
    #[serde(default)]
    pub gpu: bool,
    /// Asked for the CPU only when it was made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cpu: bool,
    /// Stop (paused) when the process uses more memory than this.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::explore::runs::opt_f64_or_text"
    )]
    pub max_rss_gb: Option<f64>,
    /// The search's folder, `searches/<uuid>` beside the catalog (made with the job).
    #[serde(default)]
    pub uuid: String,
    /// Runs per sealed segment (default a million; smaller only for tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal_rows: Option<u64>,
    /// A Quick Start build: search this long in all, then publish the atlas and end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildSpec>,
}

/// How long a Quick Start build searches, and how its atlas is picked (`quickstart.publish`'s
/// options).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildSpec {
    /// Minutes of searching over every session; then the atlas is published and the job ends.
    pub minutes: f64,
    /// Entries per category, family, and tiling.
    #[serde(default = "d_per_niche")]
    pub per_niche: usize,
    /// Lowest confirmed score (contrast picks are exempt).
    #[serde(default = "d_min_score")]
    pub min_score: f64,
    /// Most entries.
    #[serde(default = "d_max_entries")]
    pub max_entries: usize,
}

fn d_per_niche() -> usize {
    crate::explore::atlas::AtlasOpts::default().per_niche
}
fn d_min_score() -> f64 {
    crate::explore::atlas::AtlasOpts::default().min_score
}
fn d_max_entries() -> usize {
    crate::explore::atlas::AtlasOpts::default().max_entries
}

impl BuildSpec {
    /// A build of `minutes` with the atlas's usual picks.
    pub fn minutes(minutes: f64) -> BuildSpec {
        BuildSpec {
            minutes,
            per_niche: d_per_niche(),
            min_score: d_min_score(),
            max_entries: d_max_entries(),
        }
    }
    fn check(&self) -> Result<(), String> {
        if !(self.minutes > 0.0 && self.minutes.is_finite()) {
            return Err(format!(
                "a Quick Start build's minutes must be above 0, not {}",
                self.minutes
            ));
        }
        if self.per_niche == 0 || self.max_entries == 0 {
            return Err("per_niche and max_entries must be at least 1".into());
        }
        Ok(())
    }
}

/// What a Quick Start build published (`Library::publish_atlas`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Published {
    /// The atlas's entries now.
    pub entries: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

/// An explore job's progress (`jobs.progress`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExploreProgress {
    pub job: i64,
    pub status: String,
    /// Runs screened and confirmation runs, in all.
    pub screened: u64,
    pub confirmed: u64,
    /// Elites, the niches they fill, and the elites confirmed on bigger grids.
    pub elites: u64,
    pub niches: u64,
    pub confirmed_elites: u64,
    /// Candidates tried (the seen set).
    pub seen: u64,
    /// Tilings with screened runs, of the tilings the search draws from.
    pub tilings_tried: u64,
    pub tilings: u64,
    /// Screened runs per second this session; seconds run over every session, and this one.
    pub per_s: f64,
    pub elapsed_s: f64,
    pub session_s: f64,
    /// `cpu only`, or the GPU's name.
    pub backend: String,
    pub gpu_error: Option<String>,
    pub rss_gb: f64,
    /// Per family: screened runs, mean and best score, elites.
    pub families: Vec<FamilyLine>,
    /// What loading the saved state read this session.
    pub loaded: Option<LoadStats>,
    /// The search's folder.
    pub folder: String,
    /// Why it stopped, when it stopped itself (memory).
    pub note: Option<String>,
    /// Candidates whose run panicked (skipped).
    pub panics: u64,
    /// Runs on the processor this session cut short by their time cap (kept as runs of the
    /// generations they reached), and runs given up at a stop or the run's end (not recorded).
    pub capped: u64,
    pub given_up: u64,
    /// This session's worker threads: for runs on the processor, and for the GPU's batches; and
    /// the computer's cores.
    pub threads: u64,
    pub gpu_threads: u64,
    pub cores: u64,
    pub error: Option<String>,
    /// A Quick Start build: its minutes in all, and what it published once they were spent.
    pub build_minutes: Option<f64>,
    pub published: Option<Published>,
    pub line: String,
}

/// A new uuid (version 4) from the system's randomness (the hasher keys `RandomState` draws).
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let word = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u64(N.fetch_add(1, Ordering::Relaxed));
        h.write_u32(std::process::id());
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.finish()
    };
    let (a, b) = (word(1), word(2));
    let a = (a & !0xF000) | 0x4000;
    let b = (b & !(0xC << 60)) | (0x8 << 60);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

/// What [`rebuild`] made.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Rebuilt {
    pub runs: u64,
    pub screened: u64,
    pub confirmed: u64,
    pub elites: u64,
    pub niches: u64,
    pub seen: u64,
    pub seconds: f64,
}

/// Make the working database of an explore job whose folder holds only sealed segments (a
/// search imported from a pack): its archive, coverage, family totals, seen set and confirmed
/// elites rebuilt from the runs, offered in id order with their newest scores, so the job is a
/// stopped search like any other (its status, runs, report and atlas read it, and it can be
/// resumed). The job is left paused with its progress written. Its time spent is not known (0)
/// and its random states start again from its seed; the seen set keeps it from trying a
/// candidate again.
pub fn rebuild(lib: &Library, job: i64) -> Result<Rebuilt, String> {
    use crate::explore::job::TakenRun;
    use crate::explore::segments as seg;
    let t0 = Instant::now();
    let row = lib.job(job)?;
    let spec: ExploreSpec =
        serde_json::from_value(row.spec.clone()).map_err(|e| format!("job {job}: {e}"))?;
    let folder = work_dir(lib, &row)?;
    let path = folder.join("search.sqlite");
    if path.exists() {
        return Err(format!(
            "job {job} has a working database already ({})",
            path.display()
        ));
    }
    let mut runs = Runs::open(&path)?;
    runs.set_spec(&row.spec)?;
    let search = Search::new(spec.search.clone(), store::Saved::default())?;
    let mut n = 0u64;
    for file in runs.segment_files() {
        let name = file.file_name().map(|f| f.to_os_string());
        let rows: Vec<TakenRun> = runs.scan(
            seg::ALL,
            &|s, _, _| s.path.file_name().map(|f| f.to_os_string()) == name,
            &|b, out| {
                for i in 0..b.len {
                    let r = b.rec(i);
                    out.push(TakenRun {
                        id: r.id,
                        stage: r.stage,
                        parent: r.parent,
                        ev: store::eval_of(&r),
                        seed_kind: r.seed_kind,
                    });
                }
            },
        )?;
        n += rows.len() as u64;
        search.take_in(&rows);
    }
    let mut d = search.take_delta();
    d.meta.push(("elapsed_s".into(), "0".into()));
    runs.write(&d)?;
    let (elites, niches) = search.elites();
    let mut p = ExploreProgress {
        job,
        status: "paused".into(),
        screened: search.screened(),
        confirmed: search.confirmed(),
        elites,
        niches,
        confirmed_elites: search.confirmed_elites(),
        seen: search.seen(),
        tilings_tried: search.tilings_tried(),
        tilings: search.tiling_count() as u64,
        families: search.families(),
        folder: folder.display().to_string(),
        note: Some("its working database was rebuilt from its sealed runs".into()),
        backend: "cpu only".into(),
        ..Default::default()
    };
    p.line = line(&p);
    let v = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    lib.update_job(job, Some(JobStatus::Paused), Some(&v), None)?;
    Ok(Rebuilt {
        runs: n,
        screened: p.screened,
        confirmed: p.confirmed,
        elites,
        niches,
        seen: p.seen,
        seconds: (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0,
    })
}

/// The folder of an explore job: `searches/<uuid>` beside the catalog.
pub fn work_dir(lib: &Library, row: &JobRow) -> Result<PathBuf, String> {
    let uuid = row
        .spec
        .get("uuid")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .ok_or_else(|| format!("job {} has no search folder", row.id))?;
    Ok(lib.folder().join("searches").join(uuid))
}

/// An explore job's working database (`<work_dir>/search.sqlite`; its segments are beside it).
pub fn db_path(lib: &Library, row: &JobRow) -> Result<PathBuf, String> {
    if row.kind != KIND_NAME {
        return Err(format!("job {} is a {} job, not explore", row.id, row.kind));
    }
    Ok(work_dir(lib, row)?.join("search.sqlite"))
}

/// Store an explore job (status `paused` until a runner starts it). Returns its id.
pub fn start(lib: &Library, spec: &ExploreSpec) -> Result<i64, String> {
    spec.search.check()?;
    if let Some(m) = spec.max_rss_gb
        && !(m > 0.0 && m.is_finite())
    {
        return Err(format!("max_rss_gb must be above 0, not {m}"));
    }
    if spec.seal_rows == Some(0) {
        return Err("seal_rows must be at least 1".into());
    }
    if let Some(b) = &spec.build {
        b.check()?;
    }
    let mut spec = spec.clone();
    if spec.uuid.is_empty() {
        spec.uuid = new_uuid();
    }
    let v = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND_NAME, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

impl StrategyKind for ExploreKind {
    fn kind(&self) -> &'static str {
        KIND_NAME
    }
    fn label(&self) -> &'static str {
        "Explore"
    }
    fn deterministic(&self) -> bool {
        false
    }
    fn units(&self) -> Option<&'static str> {
        Some("runs")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: ExploreSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("explore spec: {e}"))?;
        start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(ExploreRun::default()))
    }
    fn work_dir(&self, lib: &Library, row: &JobRow) -> Option<PathBuf> {
        work_dir(lib, row).ok()
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = base_summary(row, self);
        let spec: Option<ExploreSpec> = serde_json::from_value(row.spec.clone()).ok();
        let p: ExploreProgress = super::typed(&row.progress);
        if let Some(sp) = &spec {
            s.title = title(sp);
        }
        s.unit = "runs".into();
        s.done = p.screened;
        s.total = None;
        s.per_s = p.per_s;
        s.elapsed_s = p.elapsed_s;
        let d = crate::dashboard::digits;
        if let Some(b) = spec.as_ref().and_then(|sp| sp.build.as_ref()) {
            s.kind_label = "Quick Start build".into();
            s.counters.push((
                "searched".into(),
                format!(
                    "{} of {} min",
                    minutes(p.elapsed_s.min(b.minutes * 60.0)),
                    minutes(b.minutes * 60.0)
                ),
            ));
            if row.status == JobStatus::Running {
                s.eta_s = Some((b.minutes * 60.0 - p.elapsed_s).max(0.0));
            }
            if let Some(pb) = &p.published {
                s.counters.push((
                    "published".into(),
                    format!("{} entries", d(pb.entries as u64)),
                ));
            }
        }
        for (k, v) in [
            ("elites", p.elites),
            ("niches", p.niches),
            ("confirmed elites", p.confirmed_elites),
        ] {
            s.counters.push((k.into(), d(v)));
        }
        if p.tilings > 0 {
            s.counters.push((
                "tilings tried".into(),
                format!("{} of {}", d(p.tilings_tried), d(p.tilings)),
            ));
        }
        if !p.backend.is_empty() {
            s.counters.push(("backend".into(), p.backend.clone()));
        }
        s.bars = p
            .families
            .iter()
            .take(12)
            .map(|f| (f.family.clone(), f.runs))
            .collect();
        s.bars_title = "Runs by family".into();
        s.error = p.error.clone();
        s
    }
}

/// Seconds as minutes, to a tenth (`3.5`).
fn minutes(s: f64) -> String {
    let m = (s / 6.0).round() / 10.0;
    if m.fract() == 0.0 {
        format!("{m:.0}")
    } else {
        format!("{m:.1}")
    }
}

/// What a job searches, in a few words.
pub fn title(sp: &ExploreSpec) -> String {
    let t = if sp.search.tilings.is_empty() {
        "every tiling".to_string()
    } else {
        short_list(&sp.search.tilings, 3)
    };
    let f = if sp.search.families.is_empty() {
        "every family".to_string()
    } else {
        let names: Vec<String> = sp
            .search
            .families
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        short_list(&names, 3)
    };
    format!("{t}, {f}")
}

/// The process's resident memory in GB (its working set on Windows; 0 where it cannot be read).
pub fn rss_gb() -> f64 {
    memory_stats::memory_stats().map_or(0.0, |m| m.physical_mem as f64 / (1u64 << 30) as f64)
}

/// A live explore run, with or without a GPU backend.
#[derive(Default)]
pub struct ExploreRun {
    backend: Option<Box<dyn Backend>>,
    /// Why the run has no GPU though one was asked for (shown as its backend).
    pub no_gpu: Option<String>,
    st: Option<Running>,
}

struct Running {
    spec: ExploreSpec,
    search: Search,
    runs: Runs,
    /// The threads of the runs on the processor, and of the GPU's batches (with a GPU); all the
    /// threads the run was given ([`worker_threads`]).
    pool: rayon::ThreadPool,
    gpu_pool: Option<rayon::ThreadPool>,
    threads: usize,
    folder: PathBuf,
    job: i64,
    elapsed_before: f64,
    t0: Instant,
    /// Runs screened by this session's search steps (the item budget).
    items: u64,
    loaded: LoadStats,
    gpu_fails: u32,
    gpu_error: Option<String>,
    note: Option<String>,
    rss: f64,
    rate: (Instant, u64),
    per_s: f64,
    /// A Quick Start build's atlas, once published.
    published: Option<Published>,
}

impl ExploreRun {
    /// A run whose GPU rounds go to `backend` (the GPU batch stepper).
    pub fn with_backend(backend: Box<dyn Backend>) -> ExploreRun {
        ExploreRun {
            backend: Some(backend),
            ..Default::default()
        }
    }

    fn backend_name(&self) -> String {
        backend_name(self.backend.as_deref(), self.no_gpu.as_deref())
    }
}

/// `cpu only`, with why when a GPU was asked for, or the GPU's name.
fn backend_name(backend: Option<&dyn Backend>, no_gpu: Option<&str>) -> String {
    match (backend, no_gpu) {
        (Some(b), _) => b.name(),
        (None, Some(why)) => format!("cpu only ({why})"),
        (None, None) => "cpu only".into(),
    }
}

/// `JETCARP_EXPLORE_TRACE` is set: steps and a build's publishing are timed on standard error.
fn trace() -> bool {
    std::env::var_os("JETCARP_EXPLORE_TRACE").is_some()
}

fn elapsed(r: &Running) -> f64 {
    r.elapsed_before + r.t0.elapsed().as_secs_f64()
}

/// Run one search step on a thread of its own, writing the job's progress every
/// [`PUBLISH_S`] seconds while it runs, and setting `halt` (the step's stop) as soon as the
/// run's stop flag is set or another process pauses the job. Returns the step's GPU error.
fn step_publishing(
    cx: &RunCx<'_>,
    r: &Running,
    backend: &str,
    gpu: Option<(&mut dyn Backend, &rayon::ThreadPool)>,
    lim: &StepLimits<'_>,
    halt: &AtomicBool,
) -> Option<String> {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (tx, rx) = channel::<()>();
    let (search, pool) = (&r.search, &r.pool);
    std::thread::scope(|sc| {
        let worker = sc.spawn(move || {
            let e = search.step(pool, gpu, lim);
            let _ = tx.send(());
            e
        });
        let mut last = Instant::now();
        // Until the step is done (or has panicked, which the join passes on).
        while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(Duration::from_millis(100)) {
            if cx.stopped() {
                halt.store(true, Ordering::SeqCst);
            }
            if last.elapsed().as_secs_f64() >= PUBLISH_S {
                if cx.paused_elsewhere() {
                    halt.store(true, Ordering::SeqCst);
                }
                cx.publish(progress_of(r, backend, JobStatus::Running, true));
                last = Instant::now();
            }
        }
        match worker.join() {
            Ok(e) => e,
            Err(p) => std::panic::resume_unwind(p),
        }
    })
}

/// The progress of a live run with this backend (its name) and status; `mid_step`: written
/// part way through a step, its rate measured since the last one.
fn progress_of(r: &Running, backend: &str, status: JobStatus, mid_step: bool) -> Value {
    let (elites, niches) = r.search.elites();
    // Part way through a long step, the rate since the last one that measured it.
    let (t, n) = r.rate;
    let dt = t.elapsed().as_secs_f64();
    let per_s = if mid_step && dt >= 5.0 {
        r.search.screened().saturating_sub(n) as f64 / dt
    } else {
        r.per_s
    };
    let gpu_threads = r.gpu_pool.as_ref().map_or(0, |p| p.current_num_threads());
    let mut p = ExploreProgress {
        job: r.job,
        status: super::status_name(status).into(),
        screened: r.search.screened(),
        confirmed: r.search.confirmed(),
        elites,
        niches,
        confirmed_elites: r.search.confirmed_elites(),
        seen: r.search.seen(),
        tilings_tried: r.search.tilings_tried(),
        tilings: r.search.tiling_count() as u64,
        per_s,
        elapsed_s: elapsed(r),
        session_s: r.t0.elapsed().as_secs_f64(),
        backend: backend.to_string(),
        gpu_error: r.gpu_error.clone(),
        rss_gb: r.rss,
        families: r.search.families(),
        loaded: Some(r.loaded.clone()),
        folder: r.folder.display().to_string(),
        note: r.note.clone(),
        panics: r.search.panics(),
        capped: r.search.capped(),
        given_up: r.search.given_up(),
        threads: r.pool.current_num_threads() as u64,
        gpu_threads: gpu_threads as u64,
        cores: cores() as u64,
        error: None,
        build_minutes: r.spec.build.as_ref().map(|b| b.minutes),
        published: r.published.clone(),
        line: String::new(),
    };
    p.line = line(&p);
    serde_json::to_value(p).unwrap_or_default()
}

impl Strategy for ExploreRun {
    fn restore(&mut self, cx: &RunCx<'_>, _checkpoint: Option<&[u8]>) -> Result<(), String> {
        let spec: ExploreSpec = serde_json::from_value(cx.row.spec.clone())
            .map_err(|e| format!("job {}: {e}", cx.job))?;
        let folder = work_dir(cx.lib, cx.row)?;
        let path = folder.join("search.sqlite");
        let mut runs = Runs::open(&path)?;
        crate::explore::reviews::move_old(cx.lib, &runs)?;
        runs.set_spec(&cx.row.spec)?;
        let mut saved = store::load(&mut runs.db)?;
        saved.next_id = saved.next_id.max(runs.max_id()? + 1);
        let loaded = saved.stats.clone();
        let elapsed_before = saved.meta_f64("elapsed_s");
        let search = Search::new(spec.search.clone(), saved)?;
        search.can_run(self.backend.is_some())?;
        let (cpu, gpu) = worker_threads(cx.threads, self.backend.is_some());
        let pool = worker_pool(cpu, "explore")?;
        let gpu_pool = if gpu > 0 {
            Some(worker_pool(gpu, "explore-gpu")?)
        } else {
            None
        };
        let screened = search.screened();
        self.st = Some(Running {
            spec,
            search,
            runs,
            pool,
            gpu_pool,
            threads: cx.threads,
            folder,
            job: cx.job,
            elapsed_before,
            t0: Instant::now(),
            items: 0,
            loaded,
            gpu_fails: 0,
            gpu_error: None,
            note: None,
            rss: rss_gb(),
            rate: (Instant::now(), screened),
            per_s: 0.0,
            published: None,
        });
        Ok(())
    }

    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let r = self.st.as_mut().ok_or("the search was not restored")?;
        r.rss = rss_gb();
        if let Some(cap) = r.spec.max_rss_gb
            && r.rss > cap
        {
            r.note = Some(format!(
                "memory {:.1} GB is over the cap of {cap} GB",
                r.rss
            ));
            cx.stop.store(true, Ordering::SeqCst);
            return Ok(Step::More);
        }
        // A Quick Start build ends once its minutes are spent, the last step cut to fit; so does
        // a run with a time budget. Runs still going on the processor then are given up, and the
        // GPU's batches are sized to end before it.
        let mut end = cx.deadline;
        if let Some(b) = &r.spec.build {
            let left = b.minutes * 60.0 - elapsed(r);
            if left <= 0.0 {
                return Ok(Step::Done);
            }
            let at = Instant::now() + Duration::from_secs_f64(left);
            end = Some(end.map_or(at, |e| e.min(at)));
        }
        let mut until = Instant::now() + Duration::from_secs_f64(STEP_S);
        if let Some(e) = end {
            until = until.min(e);
        }
        let before = r.search.screened();
        // The step's own stop: the run's stop flag, or a pause asked by another process, seen
        // while the step runs.
        let halt = AtomicBool::new(cx.stopped());
        let lim = StepLimits {
            until,
            end,
            stop: &halt,
            max_screened: cx.items.map(|n| before + n.saturating_sub(r.items)),
        };
        let backend = backend_name(self.backend.as_deref(), self.no_gpu.as_deref());
        let gpu = match (self.backend.as_mut(), r.gpu_pool.as_ref()) {
            (Some(b), Some(p)) => Some((&mut **b as &mut dyn Backend, p)),
            _ => None,
        };
        let err = step_publishing(cx, r, &backend, gpu, &lim, &halt);
        if trace() {
            let past = Instant::now()
                .saturating_duration_since(until)
                .as_secs_f64();
            eprintln!(
                "step ending at {:.1} s ran {past:.2} s past its deadline{}",
                elapsed(r),
                if end.is_some_and(|e| Instant::now() >= e) {
                    " (the run's end)"
                } else {
                    ""
                }
            );
        }
        r.items += r.search.screened() - before;
        match err {
            Some(e) => {
                r.gpu_fails += 1;
                r.gpu_error = Some(e);
                let drop_it = if r.gpu_fails >= 40 {
                    true
                } else if r.gpu_fails == 3 {
                    !self.backend.as_mut().is_some_and(|b| b.reset())
                } else {
                    false
                };
                if drop_it {
                    self.backend = None;
                    self.no_gpu = Some(format!(
                        "the GPU failed: {}",
                        r.gpu_error.clone().unwrap_or_default()
                    ));
                    r.search.can_run(false)?;
                    // The processor takes the GPU's families, and its threads.
                    let (cpu, _) = worker_threads(r.threads, false);
                    r.pool = worker_pool(cpu, "explore")?;
                    r.gpu_pool = None;
                }
            }
            None if self.backend.is_some() => r.gpu_fails = 0,
            None => {}
        }
        let mut d = r.search.take_delta();
        d.meta.push(("elapsed_s".into(), elapsed(r).to_string()));
        r.runs.write(&d)?;
        r.runs.seal_full(r.spec.seal_rows.unwrap_or(SEAL_ROWS))?;
        let (t, n) = r.rate;
        let dt = t.elapsed().as_secs_f64();
        if dt >= 5.0 {
            let now = r.search.screened();
            r.per_s = (now - n) as f64 / dt;
            r.rate = (Instant::now(), now);
        } else if r.per_s == 0.0 && dt > 0.0 {
            r.per_s = (r.search.screened() - n) as f64 / dt;
        }
        Ok(Step::More)
    }

    fn progress(&self, status: JobStatus) -> Value {
        let Some(r) = &self.st else {
            return serde_json::json!({"status": super::status_name(status)});
        };
        progress_of(r, &self.backend_name(), status, false)
    }

    fn items(&self) -> u64 {
        self.st.as_ref().map_or(0, |r| r.items)
    }

    /// A Quick Start build publishes its atlas into the library's Quick Start.
    fn finish(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<(), String> {
        let Some(b) = self.st.as_ref().and_then(|r| r.spec.build.clone()) else {
            return Ok(());
        };
        let mut p: ExploreProgress = super::typed(&self.progress(JobStatus::Running));
        p.line = format!("{}; publishing its atlas into the Quick Start", p.line);
        cx.publish(serde_json::to_value(&p).unwrap_or_default());
        let r = self.st.as_mut().ok_or("the search was not restored")?;
        let opts = crate::explore::atlas::AtlasOpts {
            per_niche: b.per_niche,
            min_score: b.min_score,
            max_entries: b.max_entries,
        };
        let (searched, t0) = (elapsed(r), Instant::now());
        // On the search's threads, all of them (the GPU's are idle now).
        let all = r.pool.current_num_threads()
            + r.gpu_pool.as_ref().map_or(0, |p| p.current_num_threads());
        let pool = worker_pool(all, "explore-publish")?;
        let rows = crate::explore::atlas::rows_on(&r.runs, &opts, &r.spec.uuid, Some(&pool))?;
        let w = cx.lib.publish_atlas(cx.job, &r.spec.uuid, &rows)?;
        if trace() {
            eprintln!(
                "build: searched {searched:.1} s, published {} atlas entries in {:.2} s",
                rows.len(),
                t0.elapsed().as_secs_f64()
            );
        }
        r.published = Some(Published {
            entries: rows.len(),
            added: w.added,
            updated: w.updated,
            removed: w.removed,
        });
        Ok(())
    }
}

/// The progress as one line: `explore 7: 12,345 runs (230/s), 1,204 elites in 980 niches, ...`;
/// a Quick Start build's with its minutes, and once done what it published.
pub fn line(p: &ExploreProgress) -> String {
    let d = crate::dashboard::digits;
    let head = match (p.build_minutes, &p.published) {
        (Some(_), Some(pb)) => format!(
            "Quick Start build (explore {}): {} atlas entries published ({} new, {} updated, {} removed);",
            p.job,
            d(pb.entries as u64),
            pb.added,
            pb.updated,
            pb.removed
        ),
        (Some(m), None) => format!(
            "Quick Start build (explore {}): {} of {} min,",
            p.job,
            // The last step may end a moment past the minutes (a GPU batch under way): the end.
            minutes(p.elapsed_s.min(m * 60.0)),
            minutes(m * 60.0)
        ),
        _ => format!("explore {}:", p.job),
    };
    format!(
        "{head} {} runs ({:.0}/s), {} elites in {} niches, {} confirmed, {} of {} tilings; {}",
        d(p.screened),
        p.per_s,
        d(p.elites),
        d(p.niches),
        d(p.confirmed_elites),
        d(p.tilings_tried),
        d(p.tilings),
        p.backend
    )
}
