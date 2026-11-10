//! What a job kind and one live run of a job are ([`StrategyKind`], [`Strategy`]), what a run
//! sees ([`RunCx`]), and the runner's loop for step-based strategies ([`drive`]).

use super::{Live, Sink, now_seconds, status_name};
use crate::dashboard::{Finds, HeatSpace, JobSummary};
use crate::hunt::job::SoupBatcher;
use crate::library::store::JobRow;
use crate::library::{AddOutcome, JobStatus, Library};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Everything kind-specific about a job (replaces the dashboard's old `JobKind`).
pub trait StrategyKind: Send + Sync {
    /// The `jobs.kind` value, which is also the strategy's name in a [`super::JobSpec`].
    fn kind(&self) -> &'static str;
    /// `Hunt`, `Census`, `Ant search`, ...
    fn label(&self) -> &'static str;
    /// A job stopped and resumed (in any process, at any thread count) ends exactly as an
    /// uninterrupted one. The dashboard says a non-deterministic job "continues" rather than
    /// "resumes".
    fn deterministic(&self) -> bool;
    /// What an item of the budget is (`soups`, `runs`, `rules`, `generations`); `None`: this
    /// kind takes a budget in minutes only.
    fn units(&self) -> Option<&'static str> {
        None
    }
    /// Can the runner run (and resume) this job?
    fn runnable(&self, _row: &JobRow) -> bool {
        true
    }
    /// Check a spec (this kind's JSON) and store a new job of this kind, status `paused`.
    /// Returns its id.
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String>;
    /// A live run of one of its jobs, with the kind's default hooks.
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String>;
    /// The job's saved progress as a summary (no catalog reads).
    fn summary(&self, row: &JobRow) -> JobSummary;
    /// The job's saved progress as this kind's progress JSON, normalised as its module reports
    /// it (the job id and status filled in), for the live view of a run that has not written
    /// any yet.
    fn saved_progress(&self, _lib: &Library, row: &JobRow) -> Value {
        let mut v = row.progress.clone();
        if let Value::Object(m) = &mut v {
            m.insert("job".into(), row.id.into());
            m.insert("status".into(), status_name(row.status).into());
        }
        v
    }
    /// Ask the job to stop at its next checkpoint, in whatever process runs it.
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        super::request_stop(lib, job)
    }
    /// The heat maps this job feeds (none by default).
    fn heat_spaces(&self, _row: &JobRow) -> Vec<HeatSpace> {
        vec![]
    }
    /// Its finds, limited to these rules (all when empty).
    fn finds(
        &self,
        _lib: &Library,
        _row: &JobRow,
        _rules: &[String],
        _n: usize,
    ) -> Result<Finds, String> {
        Ok(Finds::default())
    }
}

/// What a step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// There is more to do.
    More,
    /// The job's work is done ([`Strategy::finish`] is next).
    Done,
}

/// One live run of a job, built by its kind (or by a front end with its own hooks) and run by
/// the [`super::Runner`].
///
/// A step-based strategy implements [`restore`](Self::restore), [`step`](Self::step),
/// [`checkpoint`](Self::checkpoint), [`progress`](Self::progress), and maybe
/// [`finish`](Self::finish) and [`items`](Self::items); the default [`run`](Self::run) is the
/// runner's loop, [`drive`]. A coarse strategy overrides [`run`](Self::run) and does it all
/// itself (stops at [`RunCx::stop`], stays within [`RunCx::items`], writes its own rows).
pub trait Strategy {
    /// Run (or resume) the job until it is done, stopped, or out of budget. Returns its last
    /// progress (this kind's progress JSON).
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        drive(self, cx)
    }
    /// Load the job's checkpoint (`None`: start afresh).
    fn restore(&mut self, _cx: &RunCx<'_>, _checkpoint: Option<&[u8]>) -> Result<(), String> {
        Ok(())
    }
    /// One batch of work; its results go in `sink`. It should take no more than a few seconds:
    /// the runner checks for a stop between steps (a step that cannot be cut that small
    /// watches [`RunCx::stop`] itself, and may [`RunCx::publish`] its progress meanwhile).
    fn step(&mut self, _cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        Err("this strategy runs as a whole, not in steps".into())
    }
    /// The state to save after a step (`None`: keep the saved one).
    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    /// The progress to show and save, with this status.
    fn progress(&self, _status: JobStatus) -> Value {
        Value::Null
    }
    /// Units run in this session, for the item budget (see [`StrategyKind::units`]).
    fn items(&self) -> u64 {
        0
    }
    /// What became of the records of the step being written (one result per record of the
    /// sink's `patterns`, then its `verified`, in order), told inside its transaction before
    /// its progress and checkpoint are taken: a strategy whose progress counts the records new
    /// to the catalog, or that remembers which are stored, reads them here.
    fn stored(&mut self, _outcomes: &[Result<AddOutcome, String>]) {}
    /// After the last step: the finishing work (results in `sink`, written with the final
    /// progress, status `done`).
    fn finish(&mut self, _cx: &RunCx<'_>, _sink: &mut Sink) -> Result<(), String> {
        Ok(())
    }
}

/// What a run sees.
pub struct RunCx<'a> {
    /// The run's own catalog handle.
    pub lib: &'a Library,
    pub job: i64,
    /// The job as the run started.
    pub row: &'a JobRow,
    /// Worker threads (0: all cores).
    pub threads: usize,
    /// GPU soup batches (hunts), when the caller has a GPU.
    pub gpu: Option<Arc<dyn SoupBatcher>>,
    /// Set to stop at the next checkpoint: the handle's [`stop`](super::JobHandle::stop), the
    /// caller's own flag, the end of the time budget, or the lease lost.
    pub stop: Arc<AtomicBool>,
    /// Units this session may run (the item budget).
    pub items: Option<u64>,
    /// Where a run publishes its progress for the handle.
    pub live: Live,
    pub(crate) owner: &'a str,
}

impl RunCx<'_> {
    /// The lease holder, `host:pid` ([`super::owner_id`]).
    pub fn owner(&self) -> &str {
        self.owner
    }
    /// The stop flag is set.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
    /// Another process asked the job to stop (marked it `paused`).
    pub fn paused_elsewhere(&self) -> bool {
        self.lib.job(self.job).map(|j| j.status) == Ok(JobStatus::Paused)
    }
    /// Show (and save) the progress part way through a long step: the job's progress is
    /// written, only while this runner holds the job, and the live progress set.
    pub fn publish(&self, progress: Value) {
        let w = Sink::default().take(None, now_seconds());
        let _ = self
            .lib
            .job_step(self.job, self.owner, w, |_| Ok((progress.clone(), None)));
        self.live.set(progress);
    }
}

/// Write a step in one transaction: the sink's rows, then (told what became of its records)
/// the strategy's progress with this status and, when `save` is set, its checkpoint, with the
/// heartbeat. The job's status is set only when `set` is (a step's write leaves it, so a pause
/// marked by another process meanwhile is seen at the next check). Returns the progress
/// written.
fn write<S: Strategy + ?Sized>(
    cx: &RunCx<'_>,
    s: &mut S,
    sink: &mut Sink,
    status: JobStatus,
    set: bool,
    save: bool,
) -> Result<Value, String> {
    let w = sink.take(set.then_some(status), now_seconds());
    cx.lib.job_step(cx.job, cx.owner, w, |outcomes| {
        s.stored(outcomes);
        let p = s.progress(status);
        let c = if save { s.checkpoint()? } else { None };
        Ok((p, c))
    })
}

/// The runner's loop for a step-based strategy: restore from the job's checkpoint, then until
/// the strategy is done: stop (paused) when the stop flag is set, another process marked the
/// job `paused`, or the item budget has run; else run a step and write its results, progress,
/// and checkpoint in one transaction (only while this runner holds the job). After the last
/// step, [`Strategy::finish`] and the final write (status `done`). A failing step (or a write
/// that fails) marks the job `failed` with the error in its progress, as far as this runner
/// still may.
pub fn drive<S: Strategy + ?Sized>(s: &mut S, cx: &RunCx<'_>) -> Result<Value, String> {
    if cx.row.status == JobStatus::Done {
        return Ok(cx.row.progress.clone());
    }
    let fail = |e: String, last: Value| -> String {
        let mut p = match last {
            Value::Object(m) => Value::Object(m),
            _ => serde_json::json!({}),
        };
        p["status"] = "failed".into();
        p["error"] = e.clone().into();
        let w = Sink::default().take(Some(JobStatus::Failed), now_seconds());
        let _ = cx
            .lib
            .job_step(cx.job, cx.owner, w, |_| Ok((p.clone(), None)));
        cx.live.set(p);
        e
    };
    let checkpoint = cx.lib.job_checkpoint(cx.job)?;
    if let Err(e) = s.restore(cx, checkpoint.as_deref()) {
        return Err(fail(e, cx.row.progress.clone()));
    }
    let mut sink = Sink::default();
    let p = write(cx, s, &mut sink, JobStatus::Running, true, false)?;
    cx.live.set(p);
    loop {
        let out_of_items = cx.items.is_some_and(|n| s.items() >= n);
        if cx.stopped() || out_of_items || cx.paused_elsewhere() {
            let p = write(cx, s, &mut sink, JobStatus::Paused, true, false)?;
            cx.live.set(p.clone());
            return Ok(p);
        }
        let step = match s.step(cx, &mut sink) {
            Ok(x) => x,
            Err(e) => return Err(fail(e, s.progress(JobStatus::Running))),
        };
        if step == Step::Done {
            if let Err(e) = s.finish(cx, &mut sink) {
                return Err(fail(e, s.progress(JobStatus::Running)));
            }
            let p = match write(cx, s, &mut sink, JobStatus::Done, true, true) {
                Ok(p) => p,
                Err(e) => return Err(fail(e, s.progress(JobStatus::Running))),
            };
            cx.live.set(p.clone());
            return Ok(p);
        }
        let p = match write(cx, s, &mut sink, JobStatus::Running, false, true) {
            Ok(p) => p,
            Err(e) => return Err(fail(e, s.progress(JobStatus::Running))),
        };
        cx.live.set(p);
    }
}
