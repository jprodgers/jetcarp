//! Collisions (`jobs.kind` `collide`): `jetcarp_core::pattern::collide` of two catalog patterns at
//! every offset and phase, in steps: a batch of placements each, gathered in placement order
//! into the checkpoint, then the products verified in batches, then (when the job stores them)
//! stored with the job, as `Library::add_many` stores a whole collision. A stopped collision resumes
//! from its last batch and ends exactly as an uninterrupted one. Collide jobs had no checkpoint
//! before they ran in steps, so a paused one starts again; one made before collisions were
//! strategies (its spec without `a` and `b`) cannot be run again.
//!
//! The progress names the patterns and the rule (`a`, `b`, `rule`) and counts the runs; at the
//! end it has the runs per outcome, the products (`found`: the first [`FOUND`] with their ids,
//! labels, runs, and whether they are new to the catalog), and how many were added.

use super::{
    RunCx, Sink, Step, Strategy, StrategyKind, base_summary, other, prefix_until, status_name,
    thread_count,
};
use crate::dashboard::{JobSummary, digits};
use jetcarp_core::pattern::collide::{Collected, CollideOutput, CollideSpec, Collision};
use jetcarp_core::pattern::{PatternRecord, now_utc};
use jetcarp_store::library::JobRow;
use jetcarp_store::{AddOutcome, JobStatus, Library, prepare};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    fn units(&self) -> Option<&'static str> {
        Some("runs")
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
        Ok(Box::new(CollideRun::new(spec)))
    }
    /// Runs done of the placements, the products and those new to the catalog, and the runs
    /// per outcome as bars. A job made before collisions were strategies gets the generic one.
    fn summary(&self, row: &JobRow) -> JobSummary {
        if !self.runnable(row) {
            return other::summary(base_summary(row, self), row);
        }
        let mut s = base_summary(row, self);
        let p = &row.progress;
        let text = |k: &str| p.get(k).and_then(|v| v.as_str());
        let num = |k: &str| p.get(k).and_then(|v| v.as_u64());
        let (a, b) = match (text("a"), text("b")) {
            (Some(a), Some(b)) => (a.to_string(), b.to_string()),
            _ => {
                let id = |k: &str| {
                    let v = row.spec.get(k).and_then(|v| v.as_str()).unwrap_or("?");
                    v[..v.len().min(10)].to_string()
                };
                (id("a"), id("b"))
            }
        };
        s.title = match text("rule") {
            Some(r) => format!("{b} into {a} in {r}"),
            None => format!("{b} into {a}"),
        };
        s.unit = "runs".into();
        s.total = num("runs");
        s.done = num("runs_done")
            .or(num("runs").filter(|_| p.get("outcomes").is_some()))
            .unwrap_or(0);
        if let Some(n) = num("products") {
            s.counters.push(("products".into(), digits(n)));
        }
        if let Some(n) = num("verified") {
            s.counters.push(("verified".into(), digits(n)));
        }
        if let Some(n) = num("added") {
            s.counters.push(("new to the catalog".into(), digits(n)));
        }
        if let Some(o) = p.get("outcomes").and_then(|v| v.as_object()) {
            s.bars_title = "Runs by outcome".into();
            s.bars = o
                .iter()
                .map(|(k, v)| (k.replace('_', " "), v.as_u64().unwrap_or(0)))
                .collect();
        }
        s.error = text("error").map(String::from);
        s
    }
}

/// A collision run in steps, on a pool of the runner's threads: placements handed out for a
/// second per step (handed out in order, so the step gathers a placement-order prefix: some
/// placements run far longer than others), gathered into the checkpoint
/// ([`Collected`]), then the products verified a batch per step, then (when the job stores
/// them) the verified products stored in the job, with the ones new to the catalog in the final
/// progress.
pub struct CollideRun {
    pub spec: CollideJobSpec,
    /// Keep every run (in the checkpoint too) and, at `thumb_px` above 0, a thumbnail of each
    /// run's end (this session's only), for the caller's report, table, and contact sheet.
    pub keep: bool,
    pub thumb_px: u32,
    /// Where the finished collision's output goes (its report, products, and thumbnails), with
    /// the products new to the catalog.
    pub result: Option<Arc<Mutex<Option<(CollideOutput, Vec<String>)>>>>,
    collision: Option<Collision>,
    got: Option<Collected>,
    pool: Option<rayon::ThreadPool>,
    /// Verifications per step (they adapt).
    checks: usize,
    /// Placements gathered when this session began.
    session_start: usize,
    /// Once finished: the products (id, label, runs), and the ids new to the catalog.
    products: Option<Vec<(String, String, u64)>>,
    added: Vec<String>,
}

/// Most products listed in a finished collision's progress (`found`).
pub const FOUND: usize = 100;

impl CollideRun {
    pub fn new(spec: CollideJobSpec) -> CollideRun {
        CollideRun {
            spec,
            keep: false,
            thumb_px: 0,
            result: None,
            collision: None,
            got: None,
            pool: None,
            checks: 1,
            session_start: 0,
            products: None,
            added: Vec::new(),
        }
    }
}

const NOT_OPEN: &str = "the collision was not restored";

/// How long a step hands out placements (a placement that explodes or never settles can run a
/// few seconds by itself, so the step ends that much later).
const STEP: std::time::Duration = std::time::Duration::from_secs(1);

/// The next batch size, for about two seconds of work at `dt` seconds for `n` items.
fn adapt(n: usize, dt: f64) -> usize {
    let per = dt / n.max(1) as f64;
    ((2.0 / per.max(1e-6)) as usize).clamp(1, (n * 4).max(1))
}

impl Strategy for CollideRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        let saved: Option<Collected> = match checkpoint {
            Some(b) => Some(
                serde_json::from_slice(b).map_err(|e| format!("job {} checkpoint: {e}", cx.job))?,
            ),
            None => None,
        };
        let a = cx.lib.get(&self.spec.a)?;
        let b = cx.lib.get(&self.spec.b)?;
        let date = saved.as_ref().map_or_else(now_utc, |g| g.date.clone());
        let c = Collision::new(&a, &b, &self.spec.collide, date)?;
        let got = match saved {
            Some(g) => g
                .restored(&c)
                .map_err(|e| format!("job {} checkpoint: {e}", cx.job))?,
            None => Collected::new(&c),
        };
        let threads = thread_count(cx.threads);
        self.pool = Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .map_err(|e| e.to_string())?,
        );
        self.checks = threads * 4;
        self.session_start = got.next;
        self.collision = Some(c);
        self.got = Some(got);
        Ok(())
    }

    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let (Some(c), Some(got), Some(pool)) = (&self.collision, &mut self.got, &self.pool) else {
            return Err(NOT_OPEN.into());
        };
        let total = c.placements();
        let t0 = Instant::now();
        if got.next < total {
            // Placements handed out in order for a second (each one started runs to its end,
            // even after a stop), up to the item budget.
            let mut b = total;
            if let Some(n) = cx.items {
                b = b.min(self.session_start + n as usize);
            }
            let px = self.thumb_px;
            let outs = prefix_until(pool, got.next..b, t0 + STEP, &cx.stop, |k| c.run_one(k, px));
            got.add(outs, self.keep)?;
        } else if got.verified() < got.products() {
            let n = self.checks.min(got.products() - got.verified());
            pool.install(|| got.verify(n));
            self.checks = adapt(n, t0.elapsed().as_secs_f64());
        }
        Ok(if got.next == total && got.verified() == got.products() {
            Step::Done
        } else {
            Step::More
        })
    }

    fn finish(&mut self, _cx: &RunCx<'_>, sink: &mut Sink) -> Result<(), String> {
        let got = self.got.as_ref().ok_or(NOT_OPEN)?;
        let records = got.records();
        self.products = Some(
            records
                .iter()
                .map(|r| (r.id.clone(), r.label(), r.seen))
                .collect(),
        );
        if self.spec.store && !records.is_empty() {
            // Made ready to store here, so a product that cannot be stored fails the job before
            // anything is written.
            let ready: Vec<PatternRecord> = self.pool.as_ref().ok_or(NOT_OPEN)?.install(|| {
                records
                    .into_par_iter()
                    .map(prepare)
                    .collect::<Result<_, _>>()
            })?;
            sink.patterns.extend(ready.into_iter().map(|r| (r, 0)));
        }
        Ok(())
    }

    fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        if let Some(ids) = &self.products {
            self.added = ids
                .iter()
                .zip(outcomes)
                .filter(|(_, o)| matches!(o, Ok(AddOutcome::Added)))
                .map(|(p, _)| p.0.clone())
                .collect();
            if let (Some(slot), Some(got), Some(c)) = (&self.result, &self.got, &self.collision) {
                *slot.lock().unwrap() = Some((got.clone().output(c), self.added.clone()));
            }
        }
    }

    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.got
            .as_ref()
            .map(|g| serde_json::to_vec(g).map_err(|e| e.to_string()))
            .transpose()
    }

    fn items(&self) -> u64 {
        self.got
            .as_ref()
            .map_or(0, |g| (g.next - self.session_start) as u64)
    }

    fn progress(&self, status: JobStatus) -> Value {
        let status = status_name(status);
        let total = self.collision.as_ref().map_or(0, |c| c.placements());
        let Some(got) = &self.got else {
            return json!({"status": status, "runs": total, "runs_done": 0});
        };
        let mut v = match &self.products {
            Some(ps) => json!({
                "status": status,
                "runs": total,
                "outcomes": got.counts,
                "products": ps.len(),
                "added": self.added.len(),
                "new": self.added,
                "found": ps.iter().take(FOUND).map(|(id, label, runs)| json!({
                    "id": id, "label": label, "runs": runs, "new": self.added.contains(id),
                })).collect::<Vec<_>>(),
            }),
            None if got.next == total => json!({
                "status": status, "runs": total, "runs_done": total,
                "products": got.products(), "verified": got.verified(),
            }),
            None => json!({"status": status, "runs": total, "runs_done": got.next}),
        };
        if let Some(c) = &self.collision {
            v["a"] = c.a().label().into();
            v["b"] = c.b().label().into();
            v["rule"] = c.a().rule.clone().into();
        }
        v
    }
}
