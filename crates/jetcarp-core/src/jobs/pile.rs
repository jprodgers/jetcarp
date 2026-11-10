//! Large sandpiles (`jobs.kind` `pile`): `crate::pile::job`'s loop wrapped as it is. Results
//! and files are exactly those of `pile::job::run_job`.

use super::{RunCx, RunOptions, Runner, Strategy, StrategyKind, base_summary, typed};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{Library, tiling_display_name};
use crate::pile::Toppler;
use crate::pile::job::{self as job, PileJobSpec, PileProgress};
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

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
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        job::request_stop(lib, job)
    }
}

fn run_pile(
    cx: &RunCx<'_>,
    toppler: Option<Box<dyn Toppler>>,
    live: Option<Arc<Mutex<PileProgress>>>,
) -> Result<Value, String> {
    let live = match live {
        Some(l) => l,
        None => Arc::new(Mutex::new(
            job::status(cx.lib, cx.job).map(|x| x.1).unwrap_or_default(),
        )),
    };
    let l2 = live.clone();
    cx.live
        .follow(move || serde_json::to_value(&*l2.lock().unwrap()).unwrap_or_default());
    job::run_job(cx.lib, cx.job, cx.stop.clone(), toppler, Some(live))
        .and_then(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
}

/// A pile run: `pile::job::run_job` with the runner's stop flag (it cancels the step in
/// flight), toppling on the GPU when the job asks for it and the host registered a GPU toppler
/// (`crate::pile::gpu_toppler`), else on the CPU.
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
        run_pile(cx, t, None)
    }
}

/// A pile run on a toppler the caller made (on this thread: a toppler need not be `Send`).
pub struct PileRunOn {
    pub toppler: Option<Box<dyn Toppler>>,
    pub live: Option<Arc<Mutex<PileProgress>>>,
}

impl Strategy for PileRunOn {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        run_pile(cx, self.toppler.take(), self.live.clone())
    }
}

/// Run (or resume) a pile job here, on this thread, under the runner (its lease and
/// heartbeat): `pile::job::run_job` with these arguments, as before.
pub fn run(
    lib: &Library,
    job: i64,
    stop: Arc<AtomicBool>,
    toppler: Option<Box<dyn Toppler>>,
    live: Option<Arc<Mutex<PileProgress>>>,
) -> Result<PileProgress, String> {
    let mut s = PileRunOn { toppler, live };
    let o = RunOptions {
        stop: Some(stop),
        ..Default::default()
    };
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .map(|out| out.progress_as())
}
