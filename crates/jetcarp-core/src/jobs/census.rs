//! Census jobs (`jobs.kind` `census`): `crate::census`, run in steps of one chunk of rules
//! each. The rules' summaries and stored rows are those of the census's loop before it ran in
//! steps (and its checkpoints read as they were).

use super::{
    RunCx, Sink, Step, Strategy, StrategyKind, base_summary, eta,
    status_name, typed,
};
use crate::census::{self, CensusHook, CensusProgress, CensusSpec};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{JobStatus, Library, tiling_display_name};
use serde_json::Value;

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
    fn line(&self, progress: &Value) -> String {
        typed::<CensusProgress>(progress).line()
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        census::request_stop(lib, job)
    }
}

/// A census run in steps ([`census::CensusJob`]): one chunk of rules per step on the runner's
/// threads, stopping exactly at the item budget (rules in this session), with the caller's chunk
/// size and hook.
#[derive(Default)]
pub struct CensusRun {
    /// Rules per chunk (0: adapt).
    pub chunk: u64,
    /// Called with the progress after every write.
    pub on_progress: Option<CensusHook>,
    census: Option<census::CensusJob>,
}

const NOT_OPEN: &str = "the census was not restored";

impl Strategy for CensusRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        if let Some(h) = self.on_progress.clone() {
            cx.live.observe(move |v| h(&typed(v)));
        }
        self.census = Some(census::CensusJob::open(cx, checkpoint, self.chunk)?);
        Ok(())
    }
    fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<Step, String> {
        let c = self.census.as_mut().ok_or(NOT_OPEN)?;
        Ok(if c.step(cx, sink)? {
            Step::Done
        } else {
            Step::More
        })
    }
    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.census.as_ref().map(|c| c.checkpoint()).transpose()
    }
    fn progress(&self, status: JobStatus) -> Value {
        match &self.census {
            Some(c) => serde_json::to_value(c.progress(status_name(status))).unwrap_or_default(),
            None => Value::Null,
        }
    }
    fn items(&self) -> u64 {
        self.census.as_ref().map_or(0, |c| c.rules_run())
    }
}

