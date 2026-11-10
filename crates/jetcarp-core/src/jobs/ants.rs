//! Ant searches (`jobs.kind` `ants`): `crate::ants::job`'s loop wrapped as it is. Results and
//! stored rows are exactly those of `ants::job::run_job`.

use super::{
    Budget, RunCx, RunOptions, Runner, Strategy, StrategyKind, base_summary, eta, short_list, typed,
};
use crate::ants::job::{self as job, AntOptions, AntProgress, AntSpec, ProgressHook};
use crate::dashboard::{JobSummary, digits};
use crate::library::store::JobRow;
use crate::library::{Library, tiling_display_name};
use serde_json::Value;
use std::sync::{Arc, Mutex};

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
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        job::request_stop(lib, job)
    }
}

/// An ant search run: `ants::job::run_job` with the runner's threads, stop flag, and item
/// budget (as its session `limit`), and the caller's batch size and hooks.
#[derive(Clone, Default)]
pub struct AntsRun {
    /// Runs per batch (0: adapt).
    pub batch: u64,
    pub on_progress: Option<ProgressHook>,
    /// The caller's live progress, kept up to date too.
    pub live: Option<Arc<Mutex<AntProgress>>>,
}

impl Strategy for AntsRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        let live = self
            .live
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(AntProgress::default())));
        {
            let mut l = live.lock().unwrap();
            if let Ok((_, p)) = job::status(cx.lib, cx.job) {
                *l = p;
            }
            l.job = cx.job;
            l.status = "running".into();
        }
        let l2 = live.clone();
        cx.live
            .follow(move || serde_json::to_value(&*l2.lock().unwrap()).unwrap_or_default());
        let opts = AntOptions {
            threads: cx.threads,
            stop: cx.stop.clone(),
            limit: cx.items,
            live: Some(live),
            on_progress: self.on_progress.clone(),
            batch: self.batch,
        };
        job::run_job(cx.lib, cx.job, &opts)
            .and_then(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
    }
}

/// Run (or resume) an ant search here, on this thread, under the runner (its lease and
/// heartbeat): `ants::job::run_job` with these options, as before.
pub fn run(lib: &Library, job: i64, opts: AntOptions) -> Result<AntProgress, String> {
    let mut s = AntsRun {
        batch: opts.batch,
        on_progress: opts.on_progress.clone(),
        live: opts.live.clone(),
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
