//! Sliding-window movers (`jobs.kind` `mover`): `crate::window::job`'s loop wrapped as it is.
//! Results and stored rows are exactly those of `window::job::run_job`.

use super::{RunCx, Strategy, StrategyKind, base_summary, eta, typed};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{Library, tiling_display_name};
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

/// A mover run: `window::job::run_job` with the runner's stop flag (it checkpoints at the next
/// window).
#[derive(Clone, Default)]
pub struct MoverRun {
    /// The caller's live progress, kept up to date too.
    pub live: Option<Arc<Mutex<MoverProgress>>>,
}

impl Strategy for MoverRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        let live = self
            .live
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(MoverProgress::default())));
        {
            let mut l = live.lock().unwrap();
            *l = job::status(cx.lib, cx.job).map(|x| x.1).unwrap_or_default();
            l.job = cx.job;
            l.status = "running".into();
        }
        let l2 = live.clone();
        cx.live
            .follow(move || serde_json::to_value(&*l2.lock().unwrap()).unwrap_or_default());
        job::run_job(cx.lib, cx.job, cx.stop.clone(), Some(live))
            .and_then(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
    }
}
