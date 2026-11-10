//! Sliding-window movers (`jobs.kind` `mover`): `crate::window::job`, run in steps of a couple
//! of seconds each, checkpointed at the window that ends each step. The run and the stored
//! mover are those of the job's loop before it ran in steps (and its checkpoints read as they
//! were).

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, eta, status_name, typed};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{AddOutcome, JobStatus, Library, tiling_display_name};
use crate::window::job::{self as job, MoverJobSpec, MoverProgress};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct MoverKind;
pub(super) static KIND: MoverKind = MoverKind;

/// A sliding-window mover's summary from its spec and progress.
pub fn summary(base: JobSummary, spec: Option<&MoverJobSpec>, p: &MoverProgress) -> JobSummary {
    let mut s = base;
    if let Some(sp) = spec {
        s.title = format!(
            "{} on {}, {} cells",
            sp.track.rule,
            tiling_display_name(&sp.track.patch),
            sp.track.cells.len()
        );
    }
    match p.target_rings {
        Some(t) => {
            s.unit = "rings".into();
            s.done = p.rings as u64;
            s.total = Some(t as u64);
            s.per_s = p.rings_per_s;
        }
        None => {
            s.unit = "generations".into();
            s.done = p.generation;
            s.total = Some(p.max_gens);
            s.per_s = p.gens_per_s;
        }
    }
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters.push(("rings".into(), digits(p.rings as u64)));
    s.counters.push(("generation".into(), digits(p.generation)));
    s.counters
        .push(("distance".into(), format!("{:.0} units", p.euclid)));
    s.counters.push((
        "window".into(),
        format!("level {}, {} tiles", p.level, digits(p.tiles as u64)),
    ));
    if !p.summary.is_empty() && matches!(p.status.as_str(), "done" | "failed") {
        s.counters.push(("result".into(), p.summary.clone()));
    }
    if let Some(r) = &p.record {
        s.counters.push(("record".into(), r.clone()));
    }
    s.error = p.error.clone();
    s
}

impl StrategyKind for MoverKind {
    fn kind(&self) -> &'static str {
        job::KIND
    }
    fn label(&self) -> &'static str {
        "Mover (sliding window)"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: MoverJobSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("mover spec: {e}"))?;
        job::start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(MoverRun::default()))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec = serde_json::from_value(row.spec.clone()).ok();
        let p = typed(&row.progress);
        summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn saved_progress(&self, lib: &Library, row: &JobRow) -> Value {
        match job::status(lib, row.id) {
            Ok((_, p)) => serde_json::to_value(p).unwrap_or_default(),
            Err(_) => row.progress.clone(),
        }
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        job::request_stop(lib, job)
    }
}

/// Called with a mover run's progress after every write.
pub type MoverHook = Arc<dyn Fn(&MoverProgress) + Send + Sync>;

/// A mover run in steps ([`job::MoverJob`]): a slice of the tracker each (it checkpoints at the
/// window that ends it), and at the end the verified mover stored in the job.
#[derive(Default)]
pub struct MoverRun {
    /// Seconds per step (0: two).
    pub seconds: f64,
    /// Called with the progress after every write.
    pub on_progress: Option<MoverHook>,
    /// The caller's live progress, kept up to date too.
    pub live: Option<Arc<Mutex<MoverProgress>>>,
    mover: Option<job::MoverJob>,
}

const NOT_OPEN: &str = "the mover run was not restored";

impl Strategy for MoverRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        if self.on_progress.is_some() || self.live.is_some() {
            let (hook, live) = (self.on_progress.clone(), self.live.clone());
            cx.live.observe(move |v| {
                let p: MoverProgress = typed(v);
                if let Some(l) = &live {
                    *l.lock().unwrap() = p.clone();
                }
                if let Some(h) = &hook {
                    h(&p);
                }
            });
        }
        self.mover = Some(job::MoverJob::open(cx, checkpoint, self.seconds)?);
        Ok(())
    }
    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let m = self.mover.as_mut().ok_or(NOT_OPEN)?;
        Ok(if m.step(&cx.stop)? {
            Step::Done
        } else {
            Step::More
        })
    }
    fn finish(&mut self, _cx: &RunCx<'_>, sink: &mut Sink) -> Result<(), String> {
        let m = self.mover.as_mut().ok_or(NOT_OPEN)?;
        if let Some(rec) = m.finish() {
            sink.patterns.push((rec, 0));
        }
        Ok(())
    }
    fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        if let Some(m) = &mut self.mover {
            m.stored(outcomes.first());
        }
    }
    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.mover {
            Some(m) => m.checkpoint(),
            None => Ok(None),
        }
    }
    fn progress(&self, status: JobStatus) -> Value {
        match &self.mover {
            Some(m) => serde_json::to_value(m.progress(status_name(status))).unwrap_or_default(),
            None => Value::Null,
        }
    }
}
