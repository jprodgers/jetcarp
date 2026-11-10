//! Running a job in this process: the [`Runner`], its [`RunOptions`] and [`Outcome`], a run on
//! its own thread ([`JobHandle`]), and a run's live progress ([`Live`]).

use super::lease::HEARTBEAT_S;
use super::{Budget, Lease, RunCx, Strategy, kind, now_seconds, status_name, status_of, typed};
use crate::dashboard::JobSummary;
use crate::hunt::job::SoupBatcher;
use crate::library::store::JobRow;
use crate::library::{JobStatus, Library};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How to run a job here (none of this changes a deterministic job's results).
#[derive(Clone, Default)]
pub struct RunOptions {
    /// Worker threads (0: all cores).
    pub threads: usize,
    /// GPU soup batches, for hunts.
    pub gpu: Option<Arc<dyn SoupBatcher>>,
    /// This run's budget; empty: the one stored with the job (`spec.budget`), if any.
    pub budget: Budget,
    /// The caller's own stop flag (Ctrl-C, a script's cancel): the run stops at its next
    /// checkpoint once it is set. The runner sets it too, at the end of the time budget.
    pub stop: Option<Arc<AtomicBool>>,
}

/// Why a run stopped before the job's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stopped {
    /// The stop flag was set, or another process marked the job `paused`.
    Asked,
    /// The budget's minutes or items ran out.
    Budget,
    /// Another runner took the job over (this one was thought dead).
    LostLease,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Outcome {
    pub job: i64,
    /// The job's status now: `done`, or `paused` when it stopped early.
    pub status: JobStatus,
    /// Its progress at the end (its kind's progress JSON).
    pub progress: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

impl Outcome {
    /// The progress as its kind's own type.
    pub fn progress_as<T: serde::de::DeserializeOwned + Default>(&self) -> T {
        typed(&self.progress)
    }
}

type Source = Arc<dyn Fn() -> Value + Send + Sync>;
type Observer = Arc<dyn Fn(&Value) + Send + Sync>;

#[derive(Default)]
struct LiveState {
    value: Value,
    source: Option<Source>,
    observers: Vec<Observer>,
}

/// A run's live progress, readable from any thread: a value the run sets after each step (or
/// a source it reads from, read only when asked), and the observers told of each value set
/// (a front end's progress lines and live views).
#[derive(Clone, Default)]
pub struct Live(Arc<Mutex<LiveState>>);

impl Live {
    /// The progress now; the observers are told.
    pub fn set(&self, v: Value) {
        let obs = {
            let mut l = self.0.lock().unwrap();
            l.value = v.clone();
            l.source = None;
            l.observers.clone()
        };
        for f in obs {
            f(&v);
        }
    }
    /// Read the progress from `f` when asked, from now on.
    pub fn follow(&self, f: impl Fn() -> Value + Send + Sync + 'static) {
        self.0.lock().unwrap().source = Some(Arc::new(f));
    }
    /// Call `f` with each progress set from now on (after the step's write, on the run's
    /// thread).
    pub fn observe(&self, f: impl Fn(&Value) + Send + Sync + 'static) {
        self.0.lock().unwrap().observers.push(Arc::new(f));
    }
    pub fn get(&self) -> Value {
        let (v, src) = {
            let l = self.0.lock().unwrap();
            (l.value.clone(), l.source.clone())
        };
        match src {
            Some(f) => f(),
            None => v,
        }
    }
    /// Replace the progress without telling the observers (the run's end, as the runner
    /// reports it).
    fn replace(&self, v: Value) {
        let mut l = self.0.lock().unwrap();
        l.value = v;
        l.source = None;
    }
    /// Change one key of the progress (and stop following a source), without telling the
    /// observers.
    fn put(&self, key: &str, value: Value) {
        let mut v = self.get();
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v[key] = value;
        self.replace(v);
    }
}

/// Renews a run's heartbeat and ends its time budget, on a thread of its own.
struct Keeper {
    done: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<std::thread::JoinHandle<(bool, bool)>>,
}

impl Keeper {
    fn start(
        lib: Library,
        job: i64,
        owner: String,
        stop: Arc<AtomicBool>,
        deadline: Option<Instant>,
    ) -> Keeper {
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let d2 = done.clone();
        let thread = std::thread::Builder::new()
            .name(format!("job-{job}-keeper"))
            .spawn(move || {
                let (mut out_of_time, mut lost) = (false, false);
                let beat = Duration::from_secs(HEARTBEAT_S);
                let mut next_beat = Instant::now() + beat;
                loop {
                    let now = Instant::now();
                    if let Some(d) = deadline
                        && now >= d
                        && !out_of_time
                    {
                        out_of_time = true;
                        stop.store(true, Ordering::SeqCst);
                    }
                    if !lost && now >= next_beat {
                        next_beat = now + beat;
                        if lib.set_job_lease(job, Some(&owner), Some(now_seconds()), Some(&owner))
                            == Ok(false)
                        {
                            lost = true;
                            stop.store(true, Ordering::SeqCst);
                        }
                    }
                    let mut until = next_beat;
                    if let Some(d) = deadline.filter(|_| !out_of_time) {
                        until = until.min(d);
                    }
                    let wait = until.saturating_duration_since(Instant::now());
                    let (lock, cv) = &*d2;
                    let g = lock.lock().unwrap();
                    if *g {
                        break;
                    }
                    let (g, _) = cv.wait_timeout(g, wait).unwrap();
                    if *g {
                        break;
                    }
                }
                (out_of_time, lost)
            })
            .ok();
        Keeper { done, thread }
    }

    /// Stop it; (the time budget ran out, the lease was lost).
    fn finish(mut self) -> (bool, bool) {
        let (lock, cv) = &*self.done;
        *lock.lock().unwrap() = true;
        cv.notify_all();
        self.thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or((false, false))
    }
}

/// Runs jobs of one catalog in this process.
#[derive(Clone)]
pub struct Runner {
    lib: Library,
}

impl Runner {
    /// A runner on this catalog handle (each run's writes go through it; give a background
    /// run a handle of its own, `Library::open(path)`, so its writes never wait on the
    /// caller's reads).
    pub fn new(lib: Library) -> Runner {
        Runner { lib }
    }

    pub fn library(&self) -> &Library {
        &self.lib
    }

    /// Run (or resume) a job here, on this thread, as its kind builds it, until it is done,
    /// stopped, or out of budget.
    pub fn run(&self, job: i64, o: &RunOptions) -> Result<Outcome, String> {
        let mut s = self.build(job)?;
        self.run_with(job, &mut *s, o)
    }

    /// [`run`](Self::run) with a strategy the caller built (with its own hooks).
    pub fn run_with(
        &self,
        job: i64,
        s: &mut dyn Strategy,
        o: &RunOptions,
    ) -> Result<Outcome, String> {
        let (row, lease) = self.take(job)?;
        let mut o = o.clone();
        let stop = o.stop.get_or_insert_with(Default::default).clone();
        execute(&self.lib, row, lease, s, &o, &Live::default(), stop)
    }

    /// Run (or resume) a job on a new thread, as its kind builds it. Fails at once when the job
    /// cannot run (another runner holds it, its kind cannot run it).
    pub fn spawn(self, job: i64, o: RunOptions) -> Result<JobHandle, String> {
        let s = self.build(job)?;
        self.spawn_with(job, s, o)
    }

    /// [`spawn`](Self::spawn) with a strategy the caller built.
    pub fn spawn_with(
        self,
        job: i64,
        mut s: Box<dyn Strategy + Send>,
        mut o: RunOptions,
    ) -> Result<JobHandle, String> {
        let (row, lease) = self.take(job)?;
        let k = kind(&row.kind);
        let live = Live::default();
        let mut first = k.saved_progress(&self.lib, &row);
        if first.is_object() {
            first["status"] = "running".into();
        }
        live.set(first);
        let stop = o.stop.get_or_insert_with(Default::default).clone();
        let ending = Arc::new(AtomicBool::new(false));
        let handle_kind = row.kind.clone();
        let spec = row.spec.clone();
        let (lib, live2, stop2, ending2) =
            (self.lib.clone(), live.clone(), stop.clone(), ending.clone());
        let thread = std::thread::Builder::new()
            .name(format!("{}-{job}", row.kind))
            .spawn(move || {
                let r = execute(&lib, row, lease, &mut *s, &o, &live2, stop2);
                ending2.store(true, Ordering::SeqCst);
                r
            })
            .map_err(|e| format!("job {job}: {e}"))?;
        Ok(JobHandle {
            job,
            kind: handle_kind,
            spec,
            catalog: self.lib.path().to_path_buf(),
            stop,
            live,
            ending,
            thread: Some(thread),
        })
    }

    fn build(&self, job: i64) -> Result<Box<dyn Strategy + Send>, String> {
        let row = self.lib.job(job)?;
        let k = kind(&row.kind);
        if !k.runnable(&row) {
            return Err(format!(
                "{} jobs cannot be run or resumed here (job {job})",
                row.kind
            ));
        }
        k.build(&row)
    }

    /// The job's lease and its row as of taking it.
    fn take(&self, job: i64) -> Result<(JobRow, Lease), String> {
        let lease = Lease::acquire(&self.lib, job)?;
        let row = self.lib.job(job)?;
        Ok((row, lease))
    }
}

fn execute(
    lib: &Library,
    row: JobRow,
    lease: Lease,
    s: &mut dyn Strategy,
    o: &RunOptions,
    live: &Live,
    stop: Arc<AtomicBool>,
) -> Result<Outcome, String> {
    let job = row.id;
    let k = kind(&row.kind);
    let budget = if o.budget.is_empty() {
        Budget::of(&row)
    } else {
        o.budget
    };
    budget.check(k)?;
    let deadline = budget
        .minutes
        .map(|m| Instant::now() + Duration::from_secs_f64(m * 60.0));
    let asked_before = stop.load(Ordering::SeqCst);
    let keeper = Keeper::start(
        lib.clone(),
        job,
        lease.owner().to_string(),
        stop.clone(),
        deadline,
    );
    let r = {
        let cx = RunCx {
            lib,
            job,
            row: &row,
            threads: o.threads,
            gpu: o.gpu.clone(),
            stop: stop.clone(),
            items: budget.items,
            live: live.clone(),
            owner: lease.owner(),
        };
        s.run(&cx)
    };
    let (out_of_time, lost_beat) = keeper.finish();
    let lost = lost_beat
        || lib
            .job_lease(job)
            .is_ok_and(|(o, _)| o.as_deref() != Some(lease.owner()));
    let status = lib.job(job).map(|r| r.status).unwrap_or(row.status);
    lease.release(!lost && status == JobStatus::Done);
    let asked = asked_before || (stop.load(Ordering::SeqCst) && !out_of_time && !lost_beat);
    match r {
        Ok(progress) => {
            let stopped = if lost {
                Some(Stopped::LostLease)
            } else if out_of_time {
                Some(Stopped::Budget)
            } else if status == JobStatus::Paused {
                Some(if !asked && budget.items.is_some() {
                    Stopped::Budget
                } else {
                    Stopped::Asked
                })
            } else {
                None
            };
            live.replace(progress.clone());
            if !lost {
                live.put("status", status_name(status).into());
            }
            Ok(Outcome {
                job,
                status,
                progress: if lost { progress } else { live.get() },
                stopped,
            })
        }
        Err(_) if lost => Ok(Outcome {
            job,
            status,
            progress: live.get(),
            stopped: Some(Stopped::LostLease),
        }),
        Err(e) => {
            live.put("status", "failed".into());
            live.put("error", e.clone().into());
            Err(e)
        }
    }
}

/// A job running on a thread of this process ([`Runner::spawn`]).
pub struct JobHandle {
    job: i64,
    kind: String,
    spec: Value,
    catalog: PathBuf,
    stop: Arc<AtomicBool>,
    live: Live,
    ending: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<Outcome, String>>>,
}

impl JobHandle {
    pub fn job(&self) -> i64 {
        self.job
    }
    /// Its `jobs.kind`.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// The catalog file it runs in.
    pub fn catalog(&self) -> &Path {
        &self.catalog
    }
    /// Stop at the next checkpoint (the job is left paused there, resumable).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    /// The thread has ended.
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    /// Still at work: the run has not returned yet (once it has, the thread only lets go of
    /// the job).
    pub fn is_running(&self) -> bool {
        !self.ending.load(Ordering::SeqCst) && !self.is_finished()
    }
    /// The live progress (its kind's progress JSON).
    pub fn progress(&self) -> Value {
        self.live.get()
    }
    /// The live progress as its kind's own type (`HuntProgress`, `AntProgress`, ...).
    pub fn progress_as<T: serde::de::DeserializeOwned + Default>(&self) -> T {
        typed(&self.progress())
    }
    /// `running`, `paused`, `done`, `failed`, as the live progress says.
    pub fn status(&self) -> String {
        self.progress()
            .get("status")
            .and_then(|s| s.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                if self.is_running() {
                    "running"
                } else {
                    "paused"
                }
                .into()
            })
    }
    /// The live progress as the dashboard shows it.
    pub fn summary(&self) -> JobSummary {
        let p = self.progress();
        let status = p
            .get("status")
            .and_then(|s| s.as_str())
            .and_then(status_of)
            .unwrap_or(if self.is_running() {
                JobStatus::Running
            } else {
                JobStatus::Paused
            });
        let updated = p
            .get("updated")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        let row = JobRow {
            id: self.job,
            kind: self.kind.clone(),
            spec: self.spec.clone(),
            status,
            progress: p,
            has_checkpoint: false,
            created: String::new(),
            updated,
            finished: None,
            owner: None,
            heartbeat: None,
        };
        let mut s = kind(&self.kind).summary(&row);
        s.here = !self.is_finished();
        s.resumable = true;
        s
    }
    /// Wait for the run to end.
    pub fn wait(mut self) -> Result<Outcome, String> {
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| format!("the {} thread of job {} panicked", self.kind, self.job))?,
            None => Err(format!("job {} has no thread", self.job)),
        }
    }
}
