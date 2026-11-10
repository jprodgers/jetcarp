//! Census jobs (`jobs.kind` `census`): `crate::census`'s job loop wrapped as it is. Results
//! and stored rows are exactly those of `census::run_job`.

use super::{Budget, RunCx, RunOptions, Runner, Strategy, StrategyKind, base_summary, eta, typed};
use crate::census::{self, CensusHook, CensusJobOptions, CensusProgress, CensusSpec};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{Library, tiling_display_name};
use serde_json::Value;
use std::sync::Arc;

pub struct CensusKind;
pub(super) static KIND: CensusKind = CensusKind;

/// A census's summary from its progress.
pub fn summary(mut s: JobSummary, p: &CensusProgress) -> JobSummary {
    s.unit = "rules".into();
    s.done = p.rules_done;
    s.total = Some(p.rules_total);
    s.per_s = p.per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s
}

impl StrategyKind for CensusKind {
    fn kind(&self) -> &'static str {
        "census"
    }
    fn label(&self) -> &'static str {
        "Census"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("rules")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: CensusSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("census spec: {e}"))?;
        census::start_job(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(CensusRun::default()))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec: Option<CensusSpec> = serde_json::from_value(row.spec.clone()).ok();
        let mut p: CensusProgress = typed(&row.progress);
        let mut s = base_summary(row, self);
        if let Some(sp) = &spec {
            p.rules_total = sp.rules.len() as u64;
            s.title = format!(
                "{} rules on {}, {} soups each",
                digits(sp.rules.len() as u64),
                tiling_display_name(&sp.tiling),
                sp.soups
            );
        }
        summary(s, &p)
    }
    fn saved_progress(&self, lib: &Library, row: &JobRow) -> Value {
        match census::job_status(lib, row.id) {
            Ok((_, p)) => serde_json::to_value(p).unwrap_or_default(),
            Err(_) => row.progress.clone(),
        }
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        census::request_stop(lib, job)
    }
}

/// A census run: `census::run_job` with the runner's threads, stop flag, and item budget (as
/// its session `limit`), and the caller's chunk size and hook.
#[derive(Clone, Default)]
pub struct CensusRun {
    /// Rules per chunk (0: adapt).
    pub chunk: u64,
    pub on_progress: Option<CensusHook>,
}

impl Strategy for CensusRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        let live = cx.live.clone();
        if let Ok((_, mut p)) = census::job_status(cx.lib, cx.job) {
            p.status = "running".into();
            live.set(serde_json::to_value(p).unwrap_or_default());
        }
        let hook = self.on_progress.clone();
        let opts = CensusJobOptions {
            threads: cx.threads,
            stop: cx.stop.clone(),
            limit: cx.items,
            chunk: self.chunk,
            on_progress: Some(Arc::new(move |p: &CensusProgress| {
                live.set(serde_json::to_value(p).unwrap_or_default());
                if let Some(h) = &hook {
                    h(p);
                }
            })),
        };
        census::run_job(cx.lib, cx.job, &opts)
            .and_then(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
    }
}

/// Run (or resume) a census job here, on this thread, under the runner (its lease and
/// heartbeat): `census::run_job` with these options, as before.
pub fn run(lib: &Library, job: i64, opts: CensusJobOptions) -> Result<CensusProgress, String> {
    let mut s = CensusRun {
        chunk: opts.chunk,
        on_progress: opts.on_progress.clone(),
    };
    let o = RunOptions {
        threads: opts.threads,
        gpu: None,
        budget: Budget {
            minutes: None,
            items: opts.limit,
        },
        stop: Some(opts.stop.clone()),
    };
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .map(|out| out.progress_as())
}
