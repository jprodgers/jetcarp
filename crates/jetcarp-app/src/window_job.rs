//! Long jobs started from the app's tool windows go through the operations every front end uses:
//! `search.start` and `jobs.resume` in the background, so the job runs on a thread of this
//! process with its handle in the process's job registry, and `jobs.stop`. So the dashboard,
//! `jobs.status` and `jobs.list` (and the MCP `job_status` over the app's link) see a job a
//! window started, and `jetcarp jobs list` in another process sees its catalog row.
//!
//! [`WindowJob`] does this for one job, on a worker thread of its own: it starts or resumes the
//! job, follows its live progress through the registry's handle (and shows a window that draws
//! the run its live state: a sandpile's board), passes on a stop, and tells the window when the
//! run has ended. [`CatalogThread`] runs a window's other catalog reads and writes. The UI
//! thread never touches SQLite: every operation runs on one of these threads.

use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::jobs::{JobIn, JobsResume, JobsStop, ResumeIn};
use jetcarp_api::ops::search::{SearchStart, SearchStartIn, SearchStarted};
use jetcarp_api::{ApiError, Ctx, Platform};
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::hunt::job::SoupBatcher;
use jetcarp_core::jobs::{Live, Viewer, registry};
use jetcarp_core::library::Library;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// What the app tells the operations about the machine: the GPU soup hunter for hunts, made
/// once, and a GPU batch stepper for each explore run. (Piles topple on the GPU through the
/// toppler the app registers at start.)
pub struct AppPlatform;

impl Platform for AppPlatform {
    fn explore_backend(&self) -> Result<Box<dyn Backend>, String> {
        jetcarp_gpu::batch::BatchSim::headless().map(|g| Box::new(g) as Box<dyn Backend>)
    }
    fn soup_batcher(&self) -> Result<Arc<dyn SoupBatcher>, String> {
        static HUNTER: OnceLock<Result<Arc<dyn SoupBatcher>, String>> = OnceLock::new();
        HUNTER
            .get_or_init(|| {
                jetcarp_gpu::hunt::GpuHunter::headless()
                    .map(|g| Arc::new(g) as Arc<dyn SoupBatcher>)
            })
            .clone()
    }
}

/// A context for operations on this catalog: the app's platform, and no limits (the app's
/// user is trusted, as the command line's is).
pub fn ctx(catalog: &Path) -> Ctx {
    Ctx::new()
        .with_platform(Arc::new(AppPlatform))
        .with_library(Some(catalog.to_path_buf()))
}

/// [`ctx`] on a catalog connection the caller already has (shared with it).
pub fn ctx_on(lib: Library) -> Ctx {
    Ctx::new()
        .with_platform(Arc::new(AppPlatform))
        .with_store(lib)
}

/// Called on the worker once the run has ended, with the job and its last progress, to read
/// what the run left (a pile's metadata and final board).
pub type Ended = Arc<dyn Fn(&Ctx, i64, &Value) + Send + Sync>;

/// How a window watches its job beyond the progress.
#[derive(Clone, Default)]
pub struct Hooks {
    /// Shown the run's live state (the strategy's own types, on the run's thread).
    pub view: Option<Viewer>,
    pub ended: Option<Ended>,
}

#[derive(Default)]
struct State {
    job: Option<i64>,
    live: Option<Live>,
    /// The progress when there is no live one: the start's answer, then the last.
    last: Value,
    busy: bool,
    error: Option<String>,
}

enum Ask {
    Stop,
}

/// A job a window started or resumed, followed on a worker thread of its own.
pub struct WindowJob {
    state: Arc<Mutex<State>>,
    tx: Sender<Ask>,
}

impl WindowJob {
    /// Start a job (`search.start` in the background).
    pub fn start(catalog: &Path, input: SearchStartIn, hooks: Hooks) -> WindowJob {
        let a = SearchStartIn {
            background: true,
            detach: false,
            ..input
        };
        WindowJob::spawn(catalog, hooks, move |cx| SearchStart::run(cx, a, &Quiet))
    }

    /// Carry a job on from its last checkpoint (`jobs.resume` in the background).
    pub fn resume(catalog: &Path, input: ResumeIn, hooks: Hooks) -> WindowJob {
        let a = ResumeIn {
            background: true,
            detach: false,
            ..input
        };
        WindowJob::spawn(catalog, hooks, move |cx| JobsResume::run(cx, a, &Quiet))
    }

    fn spawn(
        catalog: &Path,
        hooks: Hooks,
        call: impl FnOnce(&Ctx) -> Result<SearchStarted, ApiError> + Send + 'static,
    ) -> WindowJob {
        let state = Arc::new(Mutex::new(State {
            busy: true,
            ..Default::default()
        }));
        let (tx, rx) = channel();
        let (st, catalog) = (state.clone(), catalog.to_path_buf());
        let spawned = std::thread::Builder::new()
            .name("jetcarp-window-job".into())
            .spawn(move || follow(&catalog, &st, hooks, call, rx));
        if let Err(e) = spawned {
            let mut s = state.lock().unwrap();
            s.busy = false;
            s.error = Some(e.to_string());
        }
        WindowJob { state, tx }
    }

    /// The job, once the operation has made or found it.
    pub fn job(&self) -> Option<i64> {
        self.state.lock().unwrap().job
    }

    /// The job's progress (its kind's progress JSON): live while it runs, then its last.
    pub fn progress(&self) -> Value {
        let s = self.state.lock().unwrap();
        match &s.live {
            Some(l) => l.get(),
            None => s.last.clone(),
        }
    }

    /// `running`, `paused`, `done`, or `failed`, as the progress says (empty before it has one).
    pub fn status(&self) -> String {
        self.progress()
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// Still at work: starting, running, winding down, or reading what the run left.
    pub fn busy(&self) -> bool {
        self.state.lock().unwrap().busy
    }

    /// Why the start, the run, or a stop failed.
    pub fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }

    /// Ask the job to stop at its next checkpoint (`jobs.stop`); it stays resumable.
    pub fn stop(&self) {
        let _ = self.tx.send(Ask::Stop);
    }
}

/// The worker: run the operation, watch the run through its registry handle until its thread
/// ends, take its result, and call the window's `ended`.
fn follow(
    catalog: &Path,
    state: &Mutex<State>,
    hooks: Hooks,
    call: impl FnOnce(&Ctx) -> Result<SearchStarted, ApiError>,
    rx: Receiver<Ask>,
) {
    let cx = ctx(catalog);
    let lib = cx.library();
    let fail = |e: String| {
        let mut s = state.lock().unwrap();
        s.error = Some(e);
        s.busy = false;
    };
    let out = match call(&cx) {
        Ok(o) => o,
        Err(e) => return fail(e.to_string()),
    };
    let job = out.job;
    {
        let mut s = state.lock().unwrap();
        s.job = Some(job);
        s.last = out.progress.clone();
    }
    // A job that was done already (or was not run here) is given as it is.
    let live = (out.mode == "background")
        .then(|| registry().with(&lib, job, |h| h.live().clone()))
        .flatten();
    if let Some(l) = &live {
        if let Some(v) = hooks.view.clone() {
            l.on_view(move |s| v(s));
        }
        state.lock().unwrap().live = Some(l.clone());
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Ask::Stop) => {
                    if let Err(e) = JobsStop::run(&cx, JobIn { job }, &Quiet) {
                        state.lock().unwrap().error = Some(e.to_string());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The window let go: the job runs on, in the registry.
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if registry().with(&lib, job, |h| h.is_finished()) != Some(false) {
                break;
            }
        }
        // Take the ended run's handle (unless the dashboard took it first).
        if let Some(Err(e)) = registry().wait(&lib, job) {
            state.lock().unwrap().error = Some(e);
        }
    }
    let last = live.as_ref().map(Live::get).unwrap_or(out.progress);
    if let Some(e) = last.get("error").and_then(|v| v.as_str()) {
        let mut s = state.lock().unwrap();
        if s.error.is_none() {
            s.error = Some(e.to_string());
        }
    }
    if let Some(f) = &hooks.ended {
        f(&cx, job, &last);
    }
    let mut s = state.lock().unwrap();
    s.last = last;
    s.live = None;
    s.busy = false;
}

type Task = Box<dyn FnOnce(&Library) + Send>;

/// A thread with its own catalog connection that runs a window's catalog reads and writes, in
/// order, off the UI thread.
pub struct CatalogThread {
    tx: Sender<Task>,
}

impl CatalogThread {
    pub fn new(catalog: &Path) -> CatalogThread {
        let (tx, rx) = channel::<Task>();
        let path: PathBuf = catalog.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("jetcarp-catalog".into())
            .spawn(move || {
                let lib = Library::open(path);
                while let Ok(f) = rx.recv() {
                    f(&lib);
                }
            });
        CatalogThread { tx }
    }

    /// Run `f` on the thread, after what was asked before.
    pub fn run(&self, f: impl FnOnce(&Library) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}

/// At exit: stop every job of this catalog running in this process at its next checkpoint
/// (`jobs.stop`), so each is left paused, and wait for them, at most `wait`.
pub fn shutdown(catalog: &Path, wait: Duration) {
    let catalog = catalog.to_path_buf();
    let (ack, rx) = channel();
    let started = std::thread::Builder::new()
        .name("jetcarp-jobs-shutdown".into())
        .spawn(move || {
            let cx = ctx(&catalog);
            let lib = cx.library();
            let here = registry().jobs(&lib);
            for &job in &here {
                if registry().is_running(&lib, job) {
                    let _ = JobsStop::run(&cx, JobIn { job }, &Quiet);
                }
            }
            for job in here {
                let _ = registry().wait(&lib, job);
            }
            let _ = ack.send(());
        });
    if started.is_ok() {
        let _ = rx.recv_timeout(wait);
    }
}
