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
//! of memory the run stops (paused). The item budget counts screened runs.
//!
//! **Importing.** A job made with `import` (an old CART explore database) first copies that
//! database's runs into sealed segments of its own, in steps of a few seconds, rebuilding the
//! archive, coverage, and seen set from them once, as the old search did on every restart; its
//! progress is committed with each segment, so an import stopped part way goes on from its last
//! segment. Its reviews go to the library. With `import_only` it then stops (paused), else it
//! searches on from the imported archive.
//!
//! **Re-measuring.** Before it copies anything, an import checks the old runs on this engine
//! ([`crate::explore::remeasure`], `remeasure` auto): a stratified sample of every tiling's runs is
//! run again and compared exactly with what is stored, and every run of a tiling whose sample
//! differs (only one family's, when only its runs differ) is run again, keeping its id (on the
//! GPU, for the rules it runs, when the job has one and it measured the sample as the CPU did).
//! The runs to re-measure are run in a pass of their own before the copy, many at a time, and
//! their new measures kept in the working database (table `remeasured`, committed with the
//! pass's progress, dropped once the copy has stored them). The check and the tilings
//! re-measured, with why, go in the search's manifest, the progress, and the working database
//! (so a stopped import goes on with the same decision); `remeasure` all runs every run again,
//! none copies them as they are.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, short_list, thread_count};
use crate::dashboard::JobSummary;
use crate::explore::job::{FamilyLine, Search, Settings, StepLimits};
use crate::explore::remeasure::{self, Remeasure, Sampler, TilingCheck};
use crate::explore::runs::{Building, Runs, SEAL_ROWS};
use crate::explore::segments::RunRec;
use crate::explore::store::{self, Import, LoadStats};
use jetcarp_core::explore::batch::Backend;
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub struct ExploreKind;
pub(super) static KIND: ExploreKind = ExploreKind;

pub const KIND_NAME: &str = "explore";

/// Seconds of rounds a step runs (a round under way runs to its end).
pub const STEP_S: f64 = 2.0;
/// Old runs read per import batch (a step reads batches for about [`STEP_S`]).
const IMPORT_BATCH: usize = 5000;
/// Old runs re-measured at a time: many, so the GPU's batches are full and a run that takes the
/// CPU minutes (Lenia, a big sandpile, their confirmation runs) hides behind the GPU's work.
const REMEASURE_CHUNK: usize = 65_536;
/// Old runs read per step of the check's sampling, and tilings checked at a time.
const SAMPLE_READ: usize = 250_000;
const CHECK_TILINGS: usize = 320;

/// An explore job: the search's settings, and how to run it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExploreSpec {
    #[serde(flatten)]
    pub search: Settings,
    /// Run the rules a GPU can run on the GPU when there is one (a run's own `gpu` option also
    /// asks for it).
    #[serde(default)]
    pub gpu: bool,
    /// Stop (paused) when the process uses more memory than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rss_gb: Option<f64>,
    /// An old explore database (CART's) to import first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<PathBuf>,
    /// Stop (paused) once the import is done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub import_only: bool,
    /// With `import`: which old runs to run again on this engine before they are stored (auto:
    /// every run of each tiling whose checked sample does not reproduce; all; none).
    #[serde(default, skip_serializing_if = "Remeasure::is_auto")]
    pub remeasure: Remeasure,
    /// The search's folder, `searches/<uuid>` beside the catalog (made with the job).
    #[serde(default)]
    pub uuid: String,
    /// Runs per sealed segment (default a million; smaller only for tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal_rows: Option<u64>,
}

/// An import under way, as the progress shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportProgress {
    pub from: String,
    /// The last old run id copied, and the largest.
    pub last: i64,
    pub max: i64,
    pub done: bool,
}

/// An import's check and re-measuring, as the progress shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemeasureProgress {
    /// `auto`, `all`, or `none`.
    pub mode: String,
    /// `sampling` (reading the old runs for the check's sample), `checking` (running the sample
    /// again), `copying`, `done`.
    pub phase: String,
    /// Old runs read for the sample, the tilings among them, the tilings checked, the sampled
    /// runs run again, and those that measured differently.
    pub scanned: u64,
    pub tilings: u64,
    pub checked_tilings: u64,
    pub sampled: u64,
    pub differ: u64,
    /// The tilings whose every run is run again, with why.
    pub tilings_remeasured: Vec<(String, String)>,
    /// Runs to re-measure, runs re-measured so far, those whose measures changed, and runs
    /// re-measured per second.
    pub to_remeasure: u64,
    pub runs: u64,
    pub changed: u64,
    pub per_s: f64,
    /// What runs the rules a GPU can run: `cpu`, or the GPU (when it measured the sample as the
    /// CPU did).
    pub backend: String,
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
    pub import: Option<ImportProgress>,
    /// An import's check of the old runs on this engine, and its re-measuring.
    pub remeasure: Option<RemeasureProgress>,
    /// What loading the saved state read this session.
    pub loaded: Option<LoadStats>,
    /// The search's folder.
    pub folder: String,
    /// Why it stopped, when it stopped itself (memory, an import done).
    pub note: Option<String>,
    /// Candidates whose run panicked (skipped).
    pub panics: u64,
    pub error: Option<String>,
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
    if let Some(p) = &spec.import {
        Import::open(p, 0)?;
    } else if spec.import_only {
        return Err("import_only needs an import".into());
    } else if !spec.remeasure.is_auto() {
        return Err("remeasure needs an import".into());
    }
    if spec.seal_rows == Some(0) {
        return Err("seal_rows must be at least 1".into());
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
        if let Some(i) = p.import.as_ref().filter(|i| !i.done) {
            s.counters.push((
                "importing".into(),
                format!("{} of {}", d(i.last.max(0) as u64), d(i.max.max(0) as u64)),
            ));
        }
        if let Some(m) = &p.remeasure {
            if matches!(m.phase.as_str(), "sampling" | "checking") {
                s.counters.push((
                    "checking".into(),
                    format!(
                        "{} of {} tilings, {} sampled runs, {} differ",
                        d(m.checked_tilings),
                        d(m.tilings),
                        d(m.sampled),
                        d(m.differ)
                    ),
                ));
            } else if m.mode != "none" {
                let t: Vec<String> = m.tilings_remeasured.iter().map(|x| x.0.clone()).collect();
                s.counters.push((
                    "re-measured".into(),
                    format!(
                        "{} runs ({} changed) of {}",
                        d(m.runs),
                        d(m.changed),
                        if m.mode == "all" {
                            "every tiling".to_string()
                        } else if t.is_empty() {
                            "no tiling".to_string()
                        } else {
                            short_list(&t, 4)
                        }
                    ),
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
    /// An import's segment being written.
    building: Option<Building>,
    pool: rayon::ThreadPool,
    folder: PathBuf,
    job: i64,
    import: Option<Import>,
    import_progress: Option<ImportProgress>,
    /// An import's check of the old runs, while it is under way.
    check: Option<Check>,
    /// An import's re-measuring: the decision, its progress, and its counts.
    remeasure: Option<remeasure::State>,
    grids: remeasure::Grids,
    /// Which runs the GPU re-measures (the check chose; none: the CPU runs them all).
    remeasure_gpu: Option<remeasure::OnGpu>,
    remeasure_progress: Option<RemeasureProgress>,
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
}

/// An import's check under way: the sample being drawn from the old runs, then run again a few
/// tilings at a time.
struct Check {
    sampler: Sampler,
    /// The last old run read for the sample.
    last: i64,
    /// Once every old run is read: the tilings still to check (tiling, its runs, its sample).
    todo: Option<VecDeque<(String, u64, Vec<i64>)>>,
    done: Vec<TilingCheck>,
    t0: Instant,
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
        match (&self.backend, &self.no_gpu) {
            (Some(b), _) => b.name(),
            (None, Some(why)) => format!("cpu only ({why})"),
            (None, None) => "cpu only".into(),
        }
    }

    fn running(&mut self) -> Result<&mut Running, String> {
        self.st
            .as_mut()
            .ok_or_else(|| "the search was not restored".into())
    }

    /// One step of an import's check: read old runs for the sample, or run the sample of a few
    /// tilings again; once every tiling is checked, decide which are re-measured and record it.
    fn check_step(&mut self, cx: &RunCx<'_>) -> Result<(), String> {
        let ExploreRun { backend, st, .. } = self;
        let r = st.as_mut().ok_or("the search was not restored")?;
        let t = Instant::now();
        let (Some(imp), Some(ck)) = (r.import.as_ref(), r.check.as_mut()) else {
            return Ok(());
        };
        let p = r.remeasure_progress.get_or_insert_with(Default::default);
        while ck.todo.is_none() {
            let keys = imp.keys(ck.last, SAMPLE_READ)?;
            for k in &keys {
                ck.sampler.offer(k);
            }
            match keys.last() {
                Some(k) => ck.last = k.id,
                None => {
                    ck.todo = Some(ck.sampler.picks().into());
                    p.phase = "checking".into();
                }
            }
            p.scanned = ck.sampler.seen;
            p.tilings = ck.sampler.tilings() as u64;
            if t.elapsed().as_secs_f64() >= STEP_S || cx.stopped() {
                return Ok(());
            }
        }
        let todo = ck.todo.as_mut().unwrap();
        while !todo.is_empty() {
            let chunk: Vec<(String, u64, Vec<i64>)> =
                todo.drain(..CHECK_TILINGS.min(todo.len())).collect();
            let ids: Vec<i64> = chunk.iter().flat_map(|c| c.2.iter().copied()).collect();
            let mut by: BTreeMap<String, Vec<RunRec>> = BTreeMap::new();
            for rec in imp.get(&ids)? {
                by.entry(rec.tiling.clone()).or_default().push(rec);
            }
            let tilings: Vec<(String, u64, Vec<RunRec>)> = chunk
                .into_iter()
                .map(|(t, n, _)| {
                    let v = by.remove(&t).unwrap_or_default();
                    (t, n, v)
                })
                .collect();
            let gpu = backend.as_mut().map(|b| &mut **b as &mut dyn Backend);
            let grids = &r.grids;
            let checks = r
                .pool
                .install(|| remeasure::check_many(&tilings, grids, gpu))?;
            for mut c in checks {
                c.family_runs = ck.sampler.family_runs(&c.tiling);
                p.checked_tilings += 1;
                p.sampled += c.sampled;
                p.differ += c.differ;
                if c.remeasure() {
                    p.tilings_remeasured.push((c.tiling.clone(), c.why()));
                }
                ck.done.push(c);
            }
            // Only the grids of the tilings to re-measure are wanted again.
            let differ: HashSet<String> =
                p.tilings_remeasured.iter().map(|x| x.0.clone()).collect();
            r.grids.keep_only(&|t| differ.contains(t));
            if t.elapsed().as_secs_f64() >= STEP_S || cx.stopped() {
                return Ok(());
            }
        }
        // Decided: recorded in the working database and the manifest.
        let ck = r.check.take().unwrap();
        let mut state = remeasure::State::new(Remeasure::Auto);
        state.decide(&ck.done);
        // The GPU measures what it can when it measured the sample as the CPU does; else only
        // Larger than Life, which a search never runs on the CPU.
        let gpu_runs: u64 = ck.done.iter().map(|c| c.gpu_runs).sum();
        let gpu_differ: u64 = ck.done.iter().map(|c| c.gpu_differ).sum();
        state.gpu = backend.as_ref().map(|_| {
            if gpu_differ == 0 {
                remeasure::OnGpu::Shared
            } else {
                remeasure::OnGpu::LtlOnly
            }
        });
        state.record.backend = match backend {
            Some(b) if gpu_differ == 0 => format!(
                "{} (it measured all {gpu_runs} sampled runs it ran that the CPU checked as the CPU \
                 does)",
                b.name()
            ),
            Some(b) => format!(
                "{}, for Larger than Life only (it measured {gpu_differ} of {gpu_runs} sampled \
                 runs unlike the CPU); the CPU for the rest",
                b.name()
            ),
            None => "cpu".into(),
        };
        r.remeasure_gpu = state.gpu;
        state.record.check_seconds = ck.t0.elapsed().as_secs_f64();
        let mut d = store::Delta::default();
        d.meta.push((
            "remeasure_checks".into(),
            serde_json::to_string(&ck.done).map_err(|e| e.to_string())?,
        ));
        d.meta.push((
            "remeasure".into(),
            serde_json::to_string(&state).map_err(|e| e.to_string())?,
        ));
        r.runs.write(&d)?;
        r.runs.set_remeasure(&state.record)?;
        r.grids.keep_only(&|t| state.wants_tiling(t));
        let p = r.remeasure_progress.get_or_insert_with(Default::default);
        p.phase = if state.is_empty() {
            "copying"
        } else {
            "re-measuring"
        }
        .into();
        p.backend = state.record.backend.clone();
        r.remeasure = Some(state);
        Ok(())
    }

    /// One step of an import's re-measuring, before the copy: the next runs to re-measure (by
    /// tiling and family, in id order, many at a time so the GPU's batches are full and a slow
    /// run on the CPU holds up few others), run again, and kept with their new measures in the
    /// working database with the counts, in one transaction, so a stopped import goes on from
    /// the last chunk.
    fn remeasure_step(&mut self, cx: &RunCx<'_>) -> Result<(), String> {
        let ExploreRun { backend, st, .. } = self;
        let r = st.as_mut().ok_or("the search was not restored")?;
        let t = Instant::now();
        r.rss = rss_gb();
        let (Some(imp), Some(m)) = (r.import.as_ref(), r.remeasure.as_mut()) else {
            return Ok(());
        };
        let filter = m.filter_sql();
        loop {
            let t1 = Instant::now();
            let rows = imp.select(m.last, REMEASURE_CHUNK, &filter)?;
            let p = r.remeasure_progress.get_or_insert_with(Default::default);
            let Some(last) = rows.last().map(|x| x.id) else {
                m.measured = true;
                store::put_remeasured(
                    &mut r.runs.db,
                    &[],
                    &[(
                        "remeasure".into(),
                        serde_json::to_string(&*m).map_err(|e| e.to_string())?,
                    )],
                )?;
                r.runs.set_remeasure(&m.record)?;
                p.phase = "copying".into();
                return Ok(());
            };
            let refs: Vec<&RunRec> = rows.iter().collect();
            let gpu = match r.remeasure_gpu {
                Some(on) => backend.as_mut().map(|b| (&mut **b as &mut dyn Backend, on)),
                None => None,
            };
            let grids = &r.grids;
            let got = r.pool.install(|| remeasure::measure(&refs, grids, gpu))?;
            let mut counts: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
            let mut fresh = Vec::with_capacity(rows.len());
            for (row, got) in rows.iter().zip(got) {
                let e = counts.entry(row.tiling.clone()).or_default();
                e.0 += 1;
                let Some(got) = got else {
                    e.2 += 1;
                    continue;
                };
                e.1 += remeasure::diff(row, &got).any() as u64;
                let mut rec = row.clone();
                remeasure::apply(&mut rec, &got);
                fresh.push(rec);
            }
            m.last = last;
            m.record.remeasure_seconds += t1.elapsed().as_secs_f64();
            m.commit(&counts);
            store::put_remeasured(
                &mut r.runs.db,
                &fresh,
                &[
                    (
                        "remeasure".into(),
                        serde_json::to_string(&*m).map_err(|e| e.to_string())?,
                    ),
                    ("elapsed_s".into(), elapsed_now(r.elapsed_before, r.t0)),
                ],
            )?;
            r.runs.set_remeasure(&m.record)?;
            p.runs = m.record.runs;
            p.to_remeasure = m.record.to_remeasure;
            p.changed = m.record.changed;
            p.per_s = m.record.runs as f64 / m.record.remeasure_seconds.max(1e-9);
            if t.elapsed().as_secs_f64() >= STEP_S || cx.stopped() {
                return Ok(());
            }
        }
    }

    /// Copy old runs into sealed segments for about a step, with the new measures of the runs
    /// re-measured, committing the import's progress with each segment; at the end the last
    /// segment, the reviews (to the library), and (with `import_only`) a stop. A stop part way
    /// leaves the segment being written unfinished: the next session writes it again from the
    /// last segment committed.
    fn import_step(&mut self, cx: &RunCx<'_>) -> Result<(), String> {
        if self.st.as_ref().is_some_and(|r| r.check.is_some()) {
            return self.check_step(cx);
        }
        if self
            .st
            .as_ref()
            .and_then(|r| r.remeasure.as_ref())
            .is_some_and(|m| !m.is_empty() && !m.measured)
        {
            return self.remeasure_step(cx);
        }
        let r = self.running()?;
        let t = Instant::now();
        r.rss = rss_gb();
        let seal = r.spec.seal_rows.unwrap_or(SEAL_ROWS).max(1);
        let Some(imp) = r.import.as_mut() else {
            return Ok(());
        };
        loop {
            let have = r.building.as_ref().map_or(0, |b| b.rows());
            let mut rows =
                imp.next((seal - have.min(seal)).clamp(1, IMPORT_BATCH as u64) as usize)?;
            let end = rows.is_empty();
            if !end {
                if let Some(m) = r.remeasure.as_ref().filter(|m| !m.is_empty()) {
                    let (lo, hi) = (rows[0].id, rows[rows.len() - 1].id);
                    let fresh = store::remeasured_between(&r.runs.db, lo, hi)?;
                    for row in rows.iter_mut() {
                        if !m.wants(&row.rec.tiling, &row.rec.fam) {
                            continue;
                        }
                        // A run that could not run again keeps its measures.
                        let Some((fate, settle, period, score, bin, feat)) = fresh.get(&row.id)
                        else {
                            continue;
                        };
                        let x = &mut row.rec;
                        (x.fate, x.settle, x.period, x.score, x.bin, x.feat) =
                            (fate.clone(), *settle, *period, *score, *bin, *feat);
                        row.ev = store::eval_of(&row.rec);
                    }
                }
                r.search.import(&rows);
                if r.building.is_none() {
                    r.building = Some(r.runs.begin_segment()?);
                }
                let b = r.building.as_mut().unwrap();
                for row in rows {
                    b.push(row.rec)?;
                }
                if let Some(p) = r.import_progress.as_mut() {
                    p.last = imp.last;
                }
            }
            let full = r.building.as_ref().is_some_and(|b| b.rows() >= seal);
            if (full || end)
                && let Some(b) = r.building.take()
            {
                let last = b.last_id();
                r.runs.finish_segment(b, None)?;
                let mut d = r.search.take_delta();
                d.meta.push(("import_last".into(), last.to_string()));
                d.meta
                    .push(("elapsed_s".into(), elapsed_now(r.elapsed_before, r.t0)));
                r.runs.write(&d)?;
            }
            if end {
                let reviews = imp.reviews()?;
                let source = format!("import:{}", imp.file_name());
                crate::explore::reviews::import_old(cx.lib, &r.runs.uuid(), &reviews, &source)?;
                let mut d = r.search.take_delta();
                d.meta.push(("import_done".into(), "1".into()));
                d.meta
                    .push(("elapsed_s".into(), elapsed_now(r.elapsed_before, r.t0)));
                if let Some(m) = r.remeasure.as_mut() {
                    m.record.done = true;
                    d.meta.push((
                        "remeasure".into(),
                        serde_json::to_string(&*m).map_err(|e| e.to_string())?,
                    ));
                }
                r.runs.write(&d)?;
                if let Some(m) = &r.remeasure {
                    r.runs.set_remeasure(&m.record)?;
                }
                // The new measures are in the segments now.
                store::drop_remeasured(&r.runs.db)?;
                r.grids.keep_only(&|_| false);
                r.import = None;
                if let Some(p) = r.import_progress.as_mut() {
                    p.done = true;
                }
                if let Some(p) = r.remeasure_progress.as_mut() {
                    p.phase = "done".into();
                }
                if r.spec.import_only {
                    r.note = Some("the import is done".into());
                    cx.stop.store(true, Ordering::SeqCst);
                }
                return Ok(());
            }
            if t.elapsed().as_secs_f64() >= STEP_S || cx.stopped() {
                return Ok(());
            }
        }
    }
}

fn elapsed(r: &Running) -> f64 {
    r.elapsed_before + r.t0.elapsed().as_secs_f64()
}

/// The progress of a re-measuring decided earlier.
fn remeasure_progress(st: &remeasure::State) -> RemeasureProgress {
    let r = &st.record;
    RemeasureProgress {
        mode: r.mode.clone(),
        phase: if r.done {
            "done"
        } else if !st.measured && !st.is_empty() {
            "re-measuring"
        } else {
            "copying"
        }
        .into(),
        scanned: 0,
        tilings: r.tilings,
        checked_tilings: r.tilings,
        sampled: r.sampled,
        differ: r.sampled_differ,
        tilings_remeasured: st
            .tilings
            .iter()
            .map(|(t, w)| (t.clone(), w.clone()))
            .collect(),
        to_remeasure: r.to_remeasure,
        runs: r.runs,
        changed: r.changed,
        per_s: if r.remeasure_seconds > 0.0 {
            r.runs as f64 / r.remeasure_seconds
        } else {
            0.0
        },
        backend: r.backend.clone(),
    }
}

fn elapsed_now(before: f64, t0: Instant) -> String {
    (before + t0.elapsed().as_secs_f64()).to_string()
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
        if let Some(from) = &spec.import
            && store::meta_get(&runs.db, "import_done")?.is_none()
        {
            store::begin_import(&runs.db, from)?;
        }
        let mut saved = store::load(&mut runs.db)?;
        saved.next_id = saved.next_id.max(runs.max_id()? + 1);
        let loaded = saved.stats.clone();
        let elapsed_before = saved.meta_f64("elapsed_s");
        let importing = saved.meta.get("import_done").is_some_and(|v| v == "0");
        let import_last: i64 = saved
            .meta
            .get("import_last")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        // The import's re-measuring: the decision made (and its counts), else a check to make
        // (auto) or the mode's own decision (all, none).
        let decided: Option<remeasure::State> = saved
            .meta
            .get("remeasure")
            .and_then(|v| serde_json::from_str(v).ok());
        let (check, decided) = match (&spec.import, importing, decided) {
            (Some(_), _, Some(st)) => (None, Some(st)),
            (Some(_), true, None) if spec.remeasure.is_auto() => (
                Some(Check {
                    sampler: Sampler::default(),
                    last: 0,
                    todo: None,
                    done: Vec::new(),
                    t0: Instant::now(),
                }),
                None,
            ),
            (Some(_), true, None) => {
                // Without a check nothing shows that a GPU measures as the CPU does: the CPU
                // runs every run.
                let mut st = remeasure::State::new(spec.remeasure);
                st.record.backend = "cpu".into();
                st.commit(&BTreeMap::new());
                store::meta_set(
                    &runs.db,
                    "remeasure",
                    &serde_json::to_string(&st).map_err(|e| e.to_string())?,
                )?;
                runs.set_remeasure(&st.record)?;
                (None, Some(st))
            }
            _ => (None, None),
        };
        // The GPU the check chose, if this run has it.
        let remeasure_gpu = match (&decided, &self.backend) {
            (Some(st), Some(b)) if st.record.backend.starts_with(&b.name()) => st.gpu,
            _ => None,
        };
        let remeasure_progress = match (&check, &decided) {
            (Some(_), _) => Some(RemeasureProgress {
                mode: "auto".into(),
                phase: "sampling".into(),
                ..Default::default()
            }),
            (None, Some(st)) => Some(remeasure_progress(st)),
            _ => None,
        };
        let search = Search::new(spec.search.clone(), saved)?;
        search.can_run(self.backend.is_some())?;
        let (import, import_progress) = match (&spec.import, importing) {
            (Some(from), true) => {
                let imp = Import::open(from, import_last)?;
                let p = ImportProgress {
                    from: from.display().to_string(),
                    last: imp.last,
                    max: imp.max,
                    done: false,
                };
                (Some(imp), Some(p))
            }
            (Some(from), false) => (
                None,
                Some(ImportProgress {
                    from: from.display().to_string(),
                    done: true,
                    ..Default::default()
                }),
            ),
            _ => (None, None),
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count(cx.threads))
            .thread_name(move |i| format!("explore-{i}"))
            .build()
            .map_err(|e| e.to_string())?;
        let screened = search.screened();
        self.st = Some(Running {
            spec,
            search,
            runs,
            building: None,
            pool,
            folder,
            job: cx.job,
            import,
            import_progress,
            check,
            remeasure: decided,
            grids: remeasure::Grids::default(),
            remeasure_gpu,
            remeasure_progress,
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
        });
        Ok(())
    }

    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        if self.running()?.import.is_some() {
            self.import_step(cx)?;
            return Ok(Step::More);
        }
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
        let before = r.search.screened();
        let lim = StepLimits {
            until: Instant::now() + Duration::from_secs_f64(STEP_S),
            stop: &cx.stop,
            max_screened: cx.items.map(|n| before + n.saturating_sub(r.items)),
        };
        let gpu = self.backend.as_mut().map(|b| &mut **b as &mut dyn Backend);
        let err = r.search.step(&r.pool, gpu, &lim);
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
        let (elites, niches) = r.search.elites();
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
            per_s: r.per_s,
            elapsed_s: elapsed(r),
            session_s: r.t0.elapsed().as_secs_f64(),
            backend: self.backend_name(),
            gpu_error: r.gpu_error.clone(),
            rss_gb: r.rss,
            families: r.search.families(),
            import: r.import_progress.clone(),
            remeasure: r.remeasure_progress.clone(),
            loaded: Some(r.loaded.clone()),
            folder: r.folder.display().to_string(),
            note: r.note.clone(),
            panics: r.search.panics(),
            error: None,
            line: String::new(),
        };
        p.line = line(&p);
        serde_json::to_value(p).unwrap_or_default()
    }

    fn items(&self) -> u64 {
        self.st.as_ref().map_or(0, |r| r.items)
    }
}

/// The progress as one line: `explore 7: 12,345 runs (230/s), 1,204 elites in 980 niches, ...`.
pub fn line(p: &ExploreProgress) -> String {
    let d = crate::dashboard::digits;
    if let Some(i) = p.import.as_ref().filter(|i| !i.done) {
        let file = Path::new(&i.from)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(m) = &p.remeasure {
            match m.phase.as_str() {
                "sampling" => {
                    return format!(
                        "explore {}: checking {file} on this engine: reading its runs for a sample \
                         ({} runs, {} tilings so far)",
                        p.job,
                        d(m.scanned),
                        d(m.tilings)
                    );
                }
                "checking" => {
                    return format!(
                        "explore {}: checking {file} on this engine: {} of {} tilings, {} sampled \
                         runs run again, {} differ ({} tilings to re-measure)",
                        p.job,
                        d(m.checked_tilings),
                        d(m.tilings),
                        d(m.sampled),
                        d(m.differ),
                        m.tilings_remeasured.len()
                    );
                }
                "re-measuring" => {
                    let of = if m.to_remeasure > 0 {
                        format!(" of {}", d(m.to_remeasure))
                    } else {
                        String::new()
                    };
                    return format!(
                        "explore {}: re-measuring {file}'s runs on this engine: {}{of} ({:.0}/s, \
                         {} changed) on {} tilings",
                        p.job,
                        d(m.runs),
                        m.per_s,
                        d(m.changed),
                        if m.mode == "all" {
                            "all".to_string()
                        } else {
                            m.tilings_remeasured.len().to_string()
                        }
                    );
                }
                _ => {}
            }
        }
        let rm = match &p.remeasure {
            Some(m) if m.runs > 0 => format!(
                ", {} runs re-measured ({:.0}/s, {} changed)",
                d(m.runs),
                m.per_s,
                d(m.changed)
            ),
            _ => String::new(),
        };
        return format!(
            "explore {}: importing {file} (run {} of {}){rm}, {} elites in {} niches",
            p.job,
            d(i.last.max(0) as u64),
            d(i.max.max(0) as u64),
            d(p.elites),
            d(p.niches)
        );
    }
    format!(
        "explore {}: {} runs ({:.0}/s), {} elites in {} niches, {} confirmed, {} of {} tilings; {}",
        p.job,
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
