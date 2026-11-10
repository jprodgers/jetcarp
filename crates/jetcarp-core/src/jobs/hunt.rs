//! Hunts (`jobs.kind` `hunt`): soups on the plane engine, `crate::hunt::job`'s loop wrapped
//! as it is. Results and stored rows are exactly those of `hunt::job::run`.

use super::{
    RunCx, RunOptions, Runner, Strategy, StrategyKind, base_summary, eta, short_list, typed,
};
use crate::dashboard::{FindItem, Finds, HeatSpace, JobSummary, Target, digits};
use crate::hunt::job::{self as hunt, HuntOptions, HuntProgress, HuntSpec, ProgressHook};
use crate::library::store::JobRow;
use crate::library::{HeatQuery, Library, tiling_display_name};
use crate::rules::Rule;
use crate::topology::Nb;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct HuntKind;
pub(super) static KIND: HuntKind = HuntKind;

/// A hunt's summary from its spec and progress.
pub fn summary(base: JobSummary, spec: Option<&HuntSpec>, p: &HuntProgress) -> JobSummary {
    let mut s = base;
    let groups = (p.groups.len() as u64).max(
        spec.map(|x| (x.rules.len() * x.tilings.len()) as u64)
            .unwrap_or(1),
    );
    let groups = groups.max(1);
    if let Some(sp) = spec {
        let tilings: Vec<String> = sp.tilings.iter().map(tiling_display_name).collect();
        s.title = format!(
            "{} on {}, {}x{} soups at {}",
            short_list(&sp.rules, 3),
            short_list(&tilings, 2),
            sp.soup.w,
            sp.soup.h,
            sp.soup.density
        );
    }
    s.unit = "soups".into();
    s.done = p.soups_done * groups;
    s.total = p
        .soups_total
        .or(spec.and_then(|x| x.soups))
        .map(|t| t * groups);
    s.per_s = p.soups_per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters = vec![
        ("settled".into(), digits(p.settled)),
        ("died".into(), digits(p.died)),
        ("exploded".into(), digits(p.exploded)),
        ("unsettled".into(), digits(p.unsettled)),
        ("objects".into(), digits(p.objects)),
        ("distinct".into(), digits(p.distinct)),
        ("new to the catalog".into(), digits(p.new_patterns)),
        ("failed verification".into(), digits(p.failed)),
    ];
    if groups > 1 {
        s.counters
            .push(("groups (rule x tiling)".into(), digits(groups)));
    }
    if let Some(g) = &p.gpu {
        s.counters.push(("GPU".into(), g.clone()));
    }
    s.bars_title = "Object copies by class".into();
    let order = ["still life", "oscillator", "spaceship", "puffer", "gun"];
    let mut bars: Vec<(String, u64)> = order
        .iter()
        .filter_map(|c| p.by_class.get(*c).map(|n| (c.to_string(), *n)))
        .collect();
    for (k, v) in &p.by_class {
        if !order.contains(&k.as_str()) {
            bars.push((k.clone(), *v));
        }
    }
    s.bars = bars;
    s.error = p.error.clone();
    s
}

/// The neighbourhood and state count of a hunt's first rule.
fn rule_space(rule: &str, nb: Option<Nb>) -> Option<(Nb, u32)> {
    let (r, hint) = Rule::parse(rule).ok()?;
    let states = match r {
        Rule::LifeLike { .. } => 2,
        Rule::Generations { states, .. } => states as u32,
        _ => return None,
    };
    Some((nb.or(hint).unwrap_or_default(), states))
}

impl StrategyKind for HuntKind {
    fn kind(&self) -> &'static str {
        "hunt"
    }
    fn label(&self) -> &'static str {
        "Hunt"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("soups")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: HuntSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("hunt spec: {e}"))?;
        hunt::start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(HuntRun::default()))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec: Option<HuntSpec> = serde_json::from_value(row.spec.clone()).ok();
        let p: HuntProgress = typed(&row.progress);
        summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn saved_progress(&self, lib: &Library, row: &JobRow) -> Value {
        match hunt::status(lib, row.id) {
            Ok((_, p)) => serde_json::to_value(p).unwrap_or_default(),
            Err(_) => row.progress.clone(),
        }
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        hunt::request_stop(lib, job)
    }
    fn heat_spaces(&self, row: &JobRow) -> Vec<HeatSpace> {
        let Ok(spec) = serde_json::from_value::<HuntSpec>(row.spec.clone()) else {
            return vec![];
        };
        let Some((nb, states)) = spec
            .rules
            .iter()
            .find_map(|r| rule_space(r, spec.neighborhood))
        else {
            return vec![];
        };
        spec.tilings
            .iter()
            .map(|t| HeatSpace {
                tiling: t.clone(),
                tiling_name: tiling_display_name(t),
                nb,
                states,
                query: HeatQuery::Patterns { job: Some(row.id) },
            })
            .collect()
    }
    fn finds(
        &self,
        lib: &Library,
        row: &JobRow,
        rules: &[String],
        n: usize,
    ) -> Result<Finds, String> {
        let item = |f: crate::library::FindRow| FindItem {
            subtitle: format!(
                "{} · seen {} here, {} in all",
                f.rule,
                digits(f.count),
                digits(f.seen)
            ),
            title: f.label,
            rule: f.rule,
            rating: f.rating,
            rejected: f.state == "rejected",
            target: Target::Pattern(f.id),
        };
        Ok(Finds {
            newest: lib
                .job_finds(Some(row.id), rules, "new", false, n)?
                .into_iter()
                .map(item)
                .collect(),
            other: lib
                .job_finds(Some(row.id), rules, "rare", false, n)?
                .into_iter()
                .map(item)
                .collect(),
            other_title: "Rarest".into(),
        })
    }
}

/// A hunt run: `hunt::job::run` with the runner's threads, GPU, stop flag, and item budget (as
/// its session `limit`), and the caller's batch size and hooks.
#[derive(Clone, Default)]
pub struct HuntRun {
    /// Soups per batch (0: adapt).
    pub batch: u64,
    pub on_progress: Option<ProgressHook>,
    /// The caller's live progress, kept up to date too.
    pub live: Option<Arc<Mutex<HuntProgress>>>,
}

impl Strategy for HuntRun {
    fn run(&mut self, cx: &RunCx<'_>) -> Result<Value, String> {
        let live = self
            .live
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(HuntProgress::default())));
        {
            let mut l = live.lock().unwrap();
            if let Ok((_, p)) = hunt::status(cx.lib, cx.job) {
                *l = p;
            }
            l.job = cx.job;
            l.status = "running".into();
        }
        let l2 = live.clone();
        cx.live
            .follow(move || serde_json::to_value(&*l2.lock().unwrap()).unwrap_or_default());
        let opts = HuntOptions {
            threads: cx.threads,
            stop: cx.stop.clone(),
            limit: cx.items,
            gpu: cx.gpu.clone(),
            live: Some(live),
            on_progress: self.on_progress.clone(),
            batch: self.batch,
        };
        hunt::run(cx.lib, cx.job, &opts)
            .and_then(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
    }
}

/// Run (or resume) a hunt here, on this thread, under the runner (its lease and heartbeat):
/// `hunt::job::run` with these options, as before.
pub fn run(lib: &Library, job: i64, opts: HuntOptions) -> Result<HuntProgress, String> {
    let mut s = HuntRun {
        batch: opts.batch,
        on_progress: opts.on_progress.clone(),
        live: opts.live.clone(),
    };
    let o = RunOptions {
        threads: opts.threads,
        gpu: opts.gpu.clone(),
        budget: super::Budget {
            minutes: None,
            items: opts.limit,
        },
        stop: Some(opts.stop.clone()),
    };
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .map(|out| out.progress_as())
}
