//! Hunts (`jobs.kind` `hunt`): soups on the plane engine (`crate::hunt::job`), run in steps of
//! one batch of soups each. The soups, their merge, and the stored rows are those of the hunt's
//! loop before it ran in steps (and its checkpoints read as they were).

use super::{
    RunCx, RunOptions, Runner, Sink, Step, Strategy, StrategyKind, base_summary, eta, short_list,
    status_name, typed,
};
use crate::dashboard::{FindItem, Finds, HeatSpace, JobSummary, Target, digits};
use crate::hunt::job::{self as hunt, HuntOptions, HuntProgress, HuntSpec, ProgressHook};
use crate::library::store::JobRow;
use crate::library::{AddOutcome, HeatQuery, JobStatus, Library, tiling_display_name};
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

/// A hunt run in steps ([`hunt::Hunt`]): one batch of soups merged and written per step, the
/// next batch running meanwhile, on the runner's threads and GPU, stopping exactly at the item
/// budget (soups per group in this session), with the caller's batch size and hooks.
#[derive(Default)]
pub struct HuntRun {
    /// Soups per batch (0: adapt).
    pub batch: u64,
    /// Called with the progress after every write.
    pub on_progress: Option<ProgressHook>,
    /// The caller's live progress, kept up to date too.
    pub live: Option<Arc<Mutex<HuntProgress>>>,
    hunt: Option<hunt::Hunt>,
}

impl HuntRun {
    /// A run with this many soups per batch (0: adapt).
    pub fn with_batch(batch: u64) -> HuntRun {
        HuntRun {
            batch,
            ..Default::default()
        }
    }
}

const NOT_OPEN: &str = "the hunt was not restored";

impl Strategy for HuntRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        if self.on_progress.is_some() || self.live.is_some() {
            let (hook, live) = (self.on_progress.clone(), self.live.clone());
            cx.live.observe(move |v| {
                let p: HuntProgress = typed(v);
                if let Some(l) = &live {
                    *l.lock().unwrap() = p.clone();
                }
                if let Some(h) = &hook {
                    h(&p);
                }
            });
        }
        self.hunt = Some(hunt::Hunt::open(cx, checkpoint, self.batch)?);
        Ok(())
    }
    fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<Step, String> {
        let h = self.hunt.as_mut().ok_or(NOT_OPEN)?;
        Ok(if h.step(cx, sink)? {
            Step::Done
        } else {
            Step::More
        })
    }
    fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        if let Some(h) = &mut self.hunt {
            h.stored(outcomes);
        }
    }
    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.hunt.as_ref().map(|h| h.checkpoint()).transpose()
    }
    fn progress(&self, status: JobStatus) -> Value {
        match &self.hunt {
            Some(h) => serde_json::to_value(h.progress(status_name(status))).unwrap_or_default(),
            None => Value::Null,
        }
    }
    fn items(&self) -> u64 {
        self.hunt.as_ref().map_or(0, |h| h.soups_run())
    }
}

/// Run (or resume) a hunt here, on this thread, under the runner (its lease and heartbeat),
/// with these options: its threads, GPU, stop flag, session limit (the item budget), batch
/// size, and hooks.
pub fn run(lib: &Library, job: i64, opts: HuntOptions) -> Result<HuntProgress, String> {
    let mut s = HuntRun {
        batch: opts.batch,
        on_progress: opts.on_progress.clone(),
        live: opts.live.clone(),
        hunt: None,
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
