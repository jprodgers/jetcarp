//! Collisions (`jobs.kind` `collide`): `crate::library::collide` of two catalog patterns at
//! every offset and phase, in steps: a batch of placements each, gathered in placement order
//! into the checkpoint, then the products verified in batches, then (when the job stores them)
//! stored with the job, as `jetcarp library collide` stores them. A stopped collision resumes
//! from its last batch and ends exactly as an uninterrupted one. Collide jobs had no checkpoint
//! before they ran in steps, so a paused one starts again; one made before collisions were
//! strategies (its spec without `a` and `b`) cannot be run again.

use super::{
    RunCx, Sink, Step, Strategy, StrategyKind, base_summary, other, status_name, thread_count,
};
use crate::dashboard::JobSummary;
use crate::library::collide::{Collected, CollideSpec, Collision};
use crate::library::store::JobRow;
use crate::library::{AddOutcome, JobStatus, Library, PatternRecord, now_utc, prepare};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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

/// A collision run in steps, on a pool of the runner's threads: a batch of placements per step
/// (about two seconds of work), gathered in placement order into the checkpoint
/// ([`Collected`]), then the products verified a batch per step, then (when the job stores
/// them) the verified products stored in the job, with the ones new to the catalog in the final
/// progress.
pub struct CollideRun {
    pub spec: CollideJobSpec,
    collision: Option<Collision>,
    got: Option<Collected>,
    pool: Option<rayon::ThreadPool>,
    /// Placements and verifications per step (they adapt).
    runs: usize,
    checks: usize,
    /// Placements gathered when this session began.
    session_start: usize,
    /// Once finished: the products' ids, and those new to the catalog.
    products: Option<Vec<String>>,
    added: Vec<String>,
}

impl CollideRun {
    pub fn new(spec: CollideJobSpec) -> CollideRun {
        CollideRun {
            spec,
            collision: None,
            got: None,
            pool: None,
            runs: 1,
            checks: 1,
            session_start: 0,
            products: None,
            added: Vec::new(),
        }
    }
}

const NOT_OPEN: &str = "the collision was not restored";

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
        (self.runs, self.checks) = (threads * 2, threads * 4);
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
            let a = got.next;
            let mut b = (a + self.runs).min(total);
            if let Some(n) = cx.items {
                b = b.min(self.session_start + n as usize);
            }
            let Some(outs) = pool.install(|| c.run(a..b, 0, None, Some(&cx.stop))) else {
                // Cut short: dropped (the checkpoint is the last batch gathered).
                return Ok(Step::More);
            };
            got.add(outs, false)?;
            self.runs = adapt(b - a, t0.elapsed().as_secs_f64());
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
        self.products = Some(records.iter().map(|r| r.id.clone()).collect());
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
                .map(|(id, _)| id.clone())
                .collect();
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
        match &self.products {
            Some(ids) => json!({
                "status": status,
                "runs": total,
                "outcomes": got.counts,
                "products": ids.len(),
                "added": self.added.len(),
                "new": self.added,
            }),
            None if got.next == total => json!({
                "status": status, "runs": total, "runs_done": total,
                "products": got.products(), "verified": got.verified(),
            }),
            None => json!({"status": status, "runs": total, "runs_done": got.next}),
        }
    }
}
