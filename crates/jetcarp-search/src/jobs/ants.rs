//! Ant searches (`jobs.kind` `ants`): `crate::ants`, run in steps of one batch of runs
//! each. The runs, their merge, and the stored rows are those of the search's loop before it ran
//! in steps (and its checkpoints read as they were).

use super::{
    RunCx, Sink, Step, Strategy, StrategyKind, base_summary, eta, short_list, status_name, typed,
};
use crate::ants::{self as job, AntProgress, AntSpec};
use crate::dashboard::{JobSummary, digits};
use jetcarp_core::pattern::tiling_display_name;
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use serde_json::Value;

pub struct AntsKind;
pub(super) static KIND: AntsKind = AntsKind;

/// An ant search's summary from its spec and progress.
pub fn summary(base: JobSummary, spec: Option<&AntSpec>, p: &AntProgress) -> JobSummary {
    let mut s = base;
    if let Some(sp) = spec {
        let tilings: Vec<String> = sp.tilings.iter().map(tiling_display_name).collect();
        let what = if !sp.rules.is_empty() {
            short_list(&sp.rules, 2)
        } else {
            let mut v = vec![];
            if let Some(st) = &sp.strings {
                v.push(format!("turn strings of {} to {}", st.min, st.max));
            }
            if sp.tables.is_some() {
                v.push("tables".into());
            }
            v.join(" and ")
        };
        s.title = format!(
            "{what} on {}{}",
            short_list(&tilings, 3),
            if sp.soup.is_some() { ", soups" } else { "" }
        );
    }
    s.unit = "runs".into();
    s.done = p.runs_done;
    s.total = (p.runs_total > 0).then_some(p.runs_total);
    s.per_s = p.runs_per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters.push(("ant steps".into(), digits(p.steps)));
    if p.steps_per_s > 0.0 {
        s.counters
            .push(("steps/s".into(), format!("{:.2e}", p.steps_per_s)));
    }
    if let Some(f) = p.newest.first() {
        s.counters.push((
            "newest".into(),
            format!(
                "{} on {} ({}{})",
                f.rule,
                f.tiling,
                f.outcome.name(),
                f.period.map(|x| format!(", p{x}")).unwrap_or_default()
            ),
        ));
    }
    s.bars_title = "Outcomes".into();
    s.bars = p.by_outcome.iter().map(|(k, v)| (k.clone(), *v)).collect();
    s.error = p.error.clone();
    s
}

impl StrategyKind for AntsKind {
    fn kind(&self) -> &'static str {
        job::KIND
    }
    fn label(&self) -> &'static str {
        "Ant search"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("runs")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: AntSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("ant search spec: {e}"))?;
        job::start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(AntsRun::default()))
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
        typed::<AntProgress>(progress).line()
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        job::request_stop(lib, job)
    }
}

/// An ant search run in steps ([`job::AntSearch`]): one batch of runs per step on the runner's
/// threads, stopping exactly at the item budget (runs in this session), with the caller's
/// batch size.
#[derive(Default)]
pub struct AntsRun {
    /// Runs per batch (0: adapt).
    pub batch: u64,
    search: Option<job::AntSearch>,
}

const NOT_OPEN: &str = "the ant search was not restored";

impl Strategy for AntsRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        self.search = Some(job::AntSearch::open(cx, checkpoint, self.batch)?);
        Ok(())
    }
    fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<Step, String> {
        let s = self.search.as_mut().ok_or(NOT_OPEN)?;
        Ok(if s.step(cx, sink)? {
            Step::Done
        } else {
            Step::More
        })
    }
    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.search.as_ref().map(|s| s.checkpoint()).transpose()
    }
    fn progress(&self, status: JobStatus) -> Value {
        match &self.search {
            Some(s) => serde_json::to_value(s.progress(status_name(status))).unwrap_or_default(),
            None => Value::Null,
        }
    }
    fn items(&self) -> u64 {
        self.search.as_ref().map_or(0, |s| s.runs_run())
    }
}
