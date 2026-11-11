//! Explore searches (`jobs.kind` `explore`): the broad quality-diversity search over tilings, rule
//! families, rules, and seeds ([`crate::explore::job::Search`]) as a catalog job, stopped,
//! budgeted, and carried on like the others.
//!
//! **Its folder.** A job's working state is its own database, `searches/<uuid>/search.sqlite`
//! beside the catalog ([`work_dir`]; the uuid is made with the job and kept in its spec): every
//! run it measured, the reviews of them, and the search's archive, confirmed set, coverage,
//! family totals, seen set, and counters ([`crate::explore::store`]). Each step writes its runs
//! and what they changed there in one transaction; the catalog row holds the progress, and no
//! checkpoint (the folder is the checkpoint). A resumed job loads the saved state, never every
//! run, so a search of millions of runs carries on within seconds.
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
//! database's runs and reviews into its own, in steps of a few seconds, rebuilding the archive,
//! coverage, and seen set from them once, as the old search did on every restart; with
//! `import_only` it then stops (paused), else it searches on from the imported archive.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, short_list, thread_count};
use crate::dashboard::JobSummary;
use crate::explore::batch::Backend;
use crate::explore::job::{FamilyLine, Search, Settings, StepLimits};
use crate::explore::store::{self, Import, LoadStats};
use crate::library::store::JobRow;
use crate::library::{JobStatus, Library};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    /// The search's folder, `searches/<uuid>` beside the catalog (made with the job).
    #[serde(default)]
    pub uuid: String,
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

/// An explore job's working database (`<work_dir>/search.sqlite`).
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
    db: Connection,
    pool: rayon::ThreadPool,
    folder: PathBuf,
    job: i64,
    import: Option<Import>,
    import_progress: Option<ImportProgress>,
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

    /// Copy old runs for about a step; at the end the reviews, the indexes, and (with
    /// `import_only`) a stop.
    fn import_step(&mut self, cx: &RunCx<'_>) -> Result<(), String> {
        let r = self.running()?;
        let t = Instant::now();
        r.rss = rss_gb();
        let Some(imp) = r.import.as_mut() else {
            return Ok(());
        };
        loop {
            let rows = imp.next(IMPORT_BATCH)?;
            if rows.is_empty() {
                let reviews = imp.reviews()?;
                let source = format!("import:{}", imp.path.display());
                let mut d = r.search.take_delta();
                d.meta.push(("import_done".into(), "1".into()));
                d.meta.push(("elapsed_s".into(), elapsed(r).to_string()));
                store::write_imported(&mut r.db, &[], &d, &reviews, &source)?;
                store::finish_import(&r.db)?;
                // The indexes take a while on a large import: count that time too.
                let d = store::Delta {
                    meta: vec![("elapsed_s".into(), elapsed(r).to_string())],
                    ..Default::default()
                };
                store::write(&mut r.db, &d)?;
                r.import = None;
                if let Some(p) = r.import_progress.as_mut() {
                    p.done = true;
                }
                if r.spec.import_only {
                    r.note = Some("the import is done".into());
                    cx.stop.store(true, Ordering::SeqCst);
                }
                return Ok(());
            }
            r.search.import(&rows);
            let mut d = r.search.take_delta();
            d.meta.push(("import_last".into(), imp.last.to_string()));
            d.meta
                .push(("elapsed_s".into(), elapsed_now(r.elapsed_before, r.t0)));
            store::write_imported(&mut r.db, &rows, &d, &[], "")?;
            if let Some(p) = r.import_progress.as_mut() {
                p.last = imp.last;
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

fn elapsed_now(before: f64, t0: Instant) -> String {
    (before + t0.elapsed().as_secs_f64()).to_string()
}

impl Strategy for ExploreRun {
    fn restore(&mut self, cx: &RunCx<'_>, _checkpoint: Option<&[u8]>) -> Result<(), String> {
        let spec: ExploreSpec = serde_json::from_value(cx.row.spec.clone())
            .map_err(|e| format!("job {}: {e}", cx.job))?;
        let folder = work_dir(cx.lib, cx.row)?;
        let path = folder.join("search.sqlite");
        let mut db = store::open(&path)?;
        if let Some(from) = &spec.import
            && store::meta_get(&db, "import_done")?.is_none()
        {
            store::begin_import(&db, from)?;
        }
        let saved = store::load(&mut db)?;
        let loaded = saved.stats.clone();
        let elapsed_before = saved.meta_f64("elapsed_s");
        let importing = saved.meta.get("import_done").is_some_and(|v| v == "0");
        let import_last: i64 = saved
            .meta
            .get("import_last")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
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
            db,
            pool,
            folder,
            job: cx.job,
            import,
            import_progress,
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
        store::write(&mut r.db, &d)?;
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
        return format!(
            "explore {}: importing {} (run {} of {}), {} elites in {} niches",
            p.job,
            Path::new(&i.from)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default(),
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
