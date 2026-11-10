//! Large sandpiles (`jobs.kind` `pile`): `crate::pile::job`, run a doubling step at a time
//! (its state is the folder's step files, as before), then exported. The pile and its files are
//! those of the job's loop before it ran in steps.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, drive, status_name, typed};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{JobStatus, Library, tiling_display_name};
use crate::pile::Toppler;
use crate::pile::job::{self as job, PileJobSpec, PileProgress};
use serde_json::Value;

pub struct PileKind;
pub(super) static KIND: PileKind = PileKind;

/// A pile job's summary from its spec and progress.
pub fn summary(base: JobSummary, spec: Option<&PileJobSpec>, p: &PileProgress) -> JobSummary {
    let mut s = base;
    s.title = match spec {
        Some(sp) => format!(
            "{} grains on {}",
            digits(sp.pile.grains),
            tiling_display_name(&sp.pile.tiling)
        ),
        None => format!("{} grains on {}", digits(p.grains), p.tiling),
    };
    s.unit = "doubling steps".into();
    s.done = p.step as u64;
    s.total = Some(p.steps as u64);
    s.elapsed_s = p.seconds + p.in_step[2];
    s.per_s = if p.seconds > 0.0 {
        p.step as f64 / p.seconds
    } else {
        0.0
    };
    s.eta_s = p.eta_s.filter(|_| s.status == "running");
    s.counters
        .push(("grains so far".into(), digits(p.grains_done)));
    s.counters
        .push(("topplings".into(), format!("{:.3e}", p.topplings as f64)));
    s.counters.push(("sweeps".into(), digits(p.sweeps)));
    if p.in_step[2] > 0.0 {
        s.counters.push((
            "step in flight".into(),
            format!("{:.3e} topplings, {:.0} s", p.in_step[1], p.in_step[2]),
        ));
    }
    if let Some(b) = &p.backend {
        s.counters.push(("backend".into(), b.clone()));
    }
    s.counters.push(("folder".into(), p.folder.clone()));
    s.error = p.error.clone();
    s
}

impl StrategyKind for PileKind {
    fn kind(&self) -> &'static str {
        job::KIND
    }
    fn label(&self) -> &'static str {
        "Sandpile"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: PileJobSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("pile spec: {e}"))?;
        job::start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(PileRun::default()))
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
    fn line(&self, progress: &Value) -> String {
        typed::<PileProgress>(progress).line()
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        job::request_stop(lib, job)
    }
}

/// A pile run in steps ([`job::PileJob`]), on a toppler the caller made (`None`: the CPU), on
/// this thread (a toppler need not be `Send`): a doubling step each, then the export.
#[derive(Default)]
pub struct PileRunOn {
    pub toppler: Option<Box<dyn Toppler>>,
    pile: Option<job::PileJob>,
}

impl PileRunOn {
    pub fn new(toppler: Option<Box<dyn Toppler>>) -> PileRunOn {
        PileRunOn {
            toppler,
            pile: None,
        }
    }
}

const NOT_OPEN: &str = "the pile was not restored";

impl Strategy for PileRunOn {
    fn restore(&mut self, cx: &RunCx<'_>, _checkpoint: Option<&[u8]>) -> Result<(), String> {
        self.pile = Some(job::PileJob::open(cx)?);
        Ok(())
    }
    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let p = self.pile.as_mut().ok_or(NOT_OPEN)?;
        let t = self.toppler.as_mut().map(|t| &mut **t as &mut dyn Toppler);
        Ok(if p.step(cx, t)? {
            Step::Done
        } else {
            Step::More
        })
    }
    fn finish(&mut self, _cx: &RunCx<'_>, sink: &mut Sink) -> Result<(), String> {
        let p = self.pile.as_mut().ok_or(NOT_OPEN)?;
        sink.files.extend(p.finish()?);
        Ok(())
    }
    fn progress(&self, status: JobStatus) -> Value {
        match &self.pile {
            Some(p) => serde_json::to_value(p.progress(status_name(status))).unwrap_or_default(),
            None => Value::Null,
        }
    }
}

/// A pile run as the kind builds it: toppling on the GPU when the job asks for it and the host
/// registered a GPU toppler (`crate::pile::gpu_toppler`), else on the CPU. The toppler is made
/// on the run's thread, and the run is [`PileRunOn`]'s steps.
#[derive(Clone, Default)]
pub struct PileRun;

impl Strategy for PileRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        let spec: Option<PileJobSpec> = serde_json::from_value(cx.row.spec.clone()).ok();
        let gpu = spec.is_some_and(|s| s.gpu && !crate::pile::is_patch(&s.pile.tiling));
        let t = if gpu {
            crate::pile::gpu_toppler().and_then(|r| r.ok())
        } else {
            None
        };
        drive(&mut PileRunOn::new(t), cx)
    }
}
