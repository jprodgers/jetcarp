//! Collisions (`jobs.kind` `collide`): `crate::library::collide` of two catalog patterns at
//! every offset and phase, as one whole run, its verified products then stored with the job (as
//! `jetcarp library collide` stores them). Short, so there is no checkpoint; a collide job made
//! before collisions were strategies (its spec without `a` and `b`) cannot be run again.

use super::{RunCx, Strategy, StrategyKind, base_summary, other, thread_count};
use crate::dashboard::JobSummary;
use crate::library::collide::{CollideSpec, collide};
use crate::library::store::JobRow;
use crate::library::{AddOutcome, JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CollideKind;
pub(super) static KIND: CollideKind = CollideKind;

pub const KIND_NAME: &str = "collide";

fn yes() -> bool {
    true
}

/// A collision job: patterns A and B (catalog ids) and how to collide them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollideJobSpec {
    pub a: String,
    pub b: String,
    #[serde(flatten)]
    pub collide: CollideSpec,
    /// Store the verified products in the catalog.
    #[serde(default = "yes")]
    pub store: bool,
}

/// Store a collision as a job (status `paused` until a runner starts it). Both patterns must be
/// in the catalog.
pub fn start(lib: &Library, spec: &CollideJobSpec) -> Result<i64, String> {
    lib.get(&spec.a)?;
    lib.get(&spec.b)?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND_NAME, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

impl StrategyKind for CollideKind {
    fn kind(&self) -> &'static str {
        KIND_NAME
    }
    fn label(&self) -> &'static str {
        "Collisions"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn runnable(&self, row: &JobRow) -> bool {
        row.spec.get("a").is_some() && row.spec.get("b").is_some()
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: CollideJobSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("collide spec: {e}"))?;
        start(lib, &spec)
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        let spec: CollideJobSpec = serde_json::from_value(row.spec.clone()).map_err(|e| {
            format!(
                "job {}: {e} (collide jobs made before collisions were strategies cannot run again)",
                row.id
            )
        })?;
        Ok(Box::new(CollideRun { spec }))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = other::summary(base_summary(row, self), row);
        if let (Some(a), Some(b)) = (
            row.spec.get("a").and_then(|v| v.as_str()),
            row.spec.get("b").and_then(|v| v.as_str()),
        ) && s.title.is_empty()
        {
            s.title = format!("{a} x {b}");
        }
        s
    }
}

/// A collision run, on a pool of the runner's threads.
pub struct CollideRun {
    pub spec: CollideJobSpec,
}

impl Strategy for CollideRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        if cx.row.status == JobStatus::Done {
            return Ok(cx.row.progress.clone());
        }
        let fail = |e: String| {
            let p = json!({"status": "failed", "error": e});
            let _ = cx
                .lib
                .update_job(cx.job, Some(JobStatus::Failed), Some(&p), None);
            e
        };
        let a = cx.lib.get(&self.spec.a).map_err(fail)?;
        let b = cx.lib.get(&self.spec.b).map_err(fail)?;
        cx.lib
            .update_job(cx.job, Some(JobStatus::Running), None, None)?;
        let (done, total) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (d2, t2) = (done.clone(), total.clone());
        cx.live.follow(move || {
            json!({"status": "running", "runs": t2.load(Ordering::Relaxed),
                   "runs_done": d2.load(Ordering::Relaxed)})
        });
        let report = |d: usize, t: usize| {
            done.fetch_max(d, Ordering::Relaxed);
            total.store(t, Ordering::Relaxed);
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count(cx.threads))
            .build()
            .map_err(|e| e.to_string())?;
        let out = pool
            .install(|| collide(&a, &b, &self.spec.collide, 0, Some(&report)))
            .map_err(fail)?;
        let mut added = Vec::new();
        if self.spec.store && !out.records.is_empty() {
            let res = cx
                .lib
                .add_many(out.records.clone(), Some(cx.job))
                .map_err(fail)?;
            for (rec, o) in out.records.iter().zip(res) {
                if o.map_err(fail)? == AddOutcome::Added {
                    added.push(rec.id.clone());
                }
            }
        }
        let p = json!({
            "status": "done",
            "runs": out.report.runs.len(),
            "outcomes": out.report.counts,
            "products": out.records.len(),
            "added": added.len(),
            "new": added,
        });
        cx.lib
            .update_job(cx.job, Some(JobStatus::Done), Some(&p), None)?;
        Ok(p)
    }
}
