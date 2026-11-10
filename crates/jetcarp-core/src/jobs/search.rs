//! Bounded soup searches (`jobs.kind` `search`): `crate::library::search` on a finite grid of
//! any tiling (aperiodic patches too), as one whole run: the soups, then every distinct
//! pattern's verification, then the records stored into the job with its counts
//! (`Library::store_search_in`), as `jetcarp library search` stores them. There is no
//! checkpoint: a stopped search stores nothing and stays `paused`, and running it again
//! repeats it (with the same results; it is deterministic).

use super::{RunCx, Strategy, StrategyKind, base_summary, other, thread_count};
use crate::dashboard::JobSummary;
use crate::library::search::{SearchSpec, search_cancellable};
use crate::library::store::JobRow;
use crate::library::{JobStatus, Library};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SearchKind;
pub(super) static KIND: SearchKind = SearchKind;

pub const KIND_NAME: &str = "search";

/// Store a search as a job (status `paused` until a runner starts it). Fails early on a bad
/// spec.
pub fn start(lib: &Library, spec: &SearchSpec) -> Result<i64, String> {
    if spec.soups == 0 || spec.gens == 0 {
        return Err("a search needs at least one soup and one generation".into());
    }
    if spec.rules.is_empty() || spec.tilings.is_empty() {
        return Err("a search needs at least one rule and one tiling".into());
    }
    if !(0.0..=1.0).contains(&spec.density) {
        return Err("density must be between 0 and 1".into());
    }
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND_NAME, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

impl StrategyKind for SearchKind {
    fn kind(&self) -> &'static str {
        KIND_NAME
    }
    fn label(&self) -> &'static str {
        "Soup search"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: SearchSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("search spec: {e}"))?;
        start(lib, &spec)
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        let spec: SearchSpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| format!("job {} spec: {e}", row.id))?;
        Ok(Box::new(SearchRun { spec }))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        other::summary(base_summary(row, self), row)
    }
}

/// A bounded search run, on a pool of the runner's threads.
pub struct SearchRun {
    pub spec: SearchSpec,
}

impl Strategy for SearchRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        if cx.row.status == JobStatus::Done {
            return Ok(cx.row.progress.clone());
        }
        let total =
            (self.spec.rules.len() * self.spec.tilings.len()) as u64 * self.spec.soups as u64;
        let done = Arc::new(AtomicUsize::new(0));
        let d2 = done.clone();
        cx.live.follow(move || {
            json!({"status": "running", "soups": total,
                   "soups_done": d2.load(Ordering::Relaxed)})
        });
        cx.lib.update_job(
            cx.job,
            Some(JobStatus::Running),
            Some(&json!({"soups": total, "soups_done": 0})),
            None,
        )?;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count(cx.threads))
            .build()
            .map_err(|e| e.to_string())?;
        let report = |d: usize, _t: usize| {
            done.fetch_max(d, Ordering::Relaxed);
        };
        let spec = &self.spec;
        let out = pool.install(|| search_cancellable(spec, Some(&report), Some(&cx.stop)));
        match out {
            Ok(out) => {
                let st = cx.lib.store_search_in(cx.job, &out)?;
                Ok(json!({
                    "status": "done", "soups": total, "soups_done": total,
                    "records": out.records.len(), "added": st.added.len(),
                }))
            }
            Err(_) if cx.stopped() => {
                let p = json!({"status": "paused", "soups": total, "soups_done": 0});
                cx.lib
                    .update_job(cx.job, Some(JobStatus::Paused), Some(&p), None)?;
                Ok(p)
            }
            Err(e) => {
                let p = json!({"status": "failed", "error": e, "soups": total});
                let _ = cx
                    .lib
                    .update_job(cx.job, Some(JobStatus::Failed), Some(&p), None);
                Err(e)
            }
        }
    }
}
