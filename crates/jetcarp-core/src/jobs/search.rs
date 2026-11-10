//! Bounded soup searches (`jobs.kind` `search`): `crate::library::search` on a finite grid of
//! any tiling (aperiodic patches too), in steps: a batch of soups each, gathered in run order
//! into the checkpoint, then every distinct pattern's verification in batches, then the records
//! stored into the job with its counts, as `jetcarp library search` stores a whole run
//! (`Library::store_search`). A stopped search resumes from its last batch, and ends exactly as
//! an uninterrupted one. Searches stored before they ran in steps had no checkpoint; a paused one
//! simply starts again.

use super::{
    RunCx, Sink, Step, Strategy, StrategyKind, base_summary, other, prefix_until, thread_count,
};
use crate::dashboard::JobSummary;
use crate::library::search::{Gathered, SearchSpec, SoupSearch};
use crate::library::store::JobRow;
use crate::library::{AddOutcome, JobStatus, Library, now_utc};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::time::Instant;

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
    fn units(&self) -> Option<&'static str> {
        Some("soups")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: SearchSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("search spec: {e}"))?;
        start(lib, &spec)
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        let spec: SearchSpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| format!("job {} spec: {e}", row.id))?;
        Ok(Box::new(SearchRun::new(spec)))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        other::summary(base_summary(row, self), row)
    }
}

/// A bounded search run in steps, on a pool of the runner's threads: soup runs for about two
/// seconds per step (handed out in order, so the step gathers a run-order prefix), gathered
/// into the checkpoint
/// ([`Gathered`]), then the distinct patterns verified a batch per step, then the verified
/// records stored into the job with their soups, and its counts (`added`: the records new to
/// the catalog) in the final progress, as `Library::store_search` stores a whole run.
pub struct SearchRun {
    pub spec: SearchSpec,
    search: Option<SoupSearch>,
    state: Option<Gathered>,
    pool: Option<rayon::ThreadPool>,
    /// Verifications per step (they adapt).
    checks: usize,
    /// Soup runs gathered when this session began.
    session_start: usize,
    /// The final counts: records stored, and new to the catalog.
    records: Option<usize>,
    added: usize,
}

impl SearchRun {
    pub fn new(spec: SearchSpec) -> SearchRun {
        SearchRun {
            spec,
            search: None,
            state: None,
            pool: None,
            checks: 1,
            session_start: 0,
            records: None,
            added: 0,
        }
    }
}

const NOT_OPEN: &str = "the search was not restored";

/// How long a step hands out soups.
const STEP: std::time::Duration = std::time::Duration::from_secs(2);

/// The next batch size, for about two seconds of work at `dt` seconds for `n` items.
fn adapt(n: usize, dt: f64) -> usize {
    let per = dt / n.max(1) as f64;
    ((2.0 / per.max(1e-6)) as usize).clamp(1, (n * 4).max(1))
}

impl Strategy for SearchRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        let search = SoupSearch::new(&self.spec)?;
        let state = match checkpoint {
            Some(b) => unpack(b)
                .and_then(|g| g.restored(&search))
                .map_err(|e| format!("job {} checkpoint: {e}", cx.job))?,
            None => Gathered::new(&search, now_utc()),
        };
        let threads = thread_count(cx.threads);
        self.pool = Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .map_err(|e| e.to_string())?,
        );
        self.checks = threads * 4;
        self.session_start = state.next;
        self.search = Some(search);
        self.state = Some(state);
        Ok(())
    }

    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let (Some(search), Some(state), Some(pool)) = (&self.search, &mut self.state, &self.pool)
        else {
            return Err(NOT_OPEN.into());
        };
        let total = search.runs();
        let t0 = Instant::now();
        if state.next < total {
            // Soups handed out in order for about two seconds (each one started runs to its
            // end, even after a stop), up to the item budget.
            let mut b = total;
            if let Some(n) = cx.items {
                b = b.min(self.session_start + n as usize);
            }
            let date = state.date.clone();
            let outs = prefix_until(pool, state.next..b, t0 + STEP, &cx.stop, |k| {
                search.run_one(&date, k)
            });
            state.add(search, outs);
        } else if state.verified() < state.distinct() {
            let n = self.checks.min(state.distinct() - state.verified());
            pool.install(|| state.verify(n));
            self.checks = adapt(n, t0.elapsed().as_secs_f64());
        }
        Ok(
            if state.next == total && state.verified() == state.distinct() {
                Step::Done
            } else {
                Step::More
            },
        )
    }

    fn finish(&mut self, _cx: &RunCx<'_>, sink: &mut Sink) -> Result<(), String> {
        let state = self.state.take().ok_or(NOT_OPEN)?;
        let out = state.output(&self.spec);
        self.records = Some(out.records.len());
        let soups = out.soups.iter().map(|&s| s as u64);
        sink.patterns.extend(out.records.into_iter().zip(soups));
        Ok(())
    }

    fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        if self.records.is_some() {
            self.added = outcomes
                .iter()
                .filter(|o| matches!(o, Ok(AddOutcome::Added)))
                .count();
        }
    }

    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.state.as_ref().map(pack).transpose()
    }

    fn items(&self) -> u64 {
        self.state
            .as_ref()
            .map_or(0, |g| (g.next - self.session_start) as u64)
    }

    fn progress(&self, _status: JobStatus) -> Value {
        let total = self.search.as_ref().map_or(0, |s| s.runs());
        match (&self.state, self.records) {
            (_, Some(records)) => json!({
                "soups": total, "soups_done": total, "records": records, "added": self.added,
            }),
            (Some(g), None) if g.next == total => json!({
                "soups": total, "soups_done": total,
                "distinct": g.distinct(), "verified": g.verified(),
            }),
            (Some(g), None) => json!({"soups": total, "soups_done": g.next}),
            (None, None) => json!({"soups": total, "soups_done": 0}),
        }
    }
}

/// A checkpoint: `Z1` and the gathered state's JSON, deflated (its records are most of it).
fn pack(g: &Gathered) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(g).map_err(|e| e.to_string())?;
    let mut enc = flate2::write::DeflateEncoder::new(
        Vec::with_capacity(json.len() / 4 + 16),
        flate2::Compression::fast(),
    );
    enc.write_all(&json).map_err(|e| e.to_string())?;
    let mut out = b"Z1".to_vec();
    out.extend(enc.finish().map_err(|e| e.to_string())?);
    Ok(out)
}

fn unpack(b: &[u8]) -> Result<Gathered, String> {
    let z = b
        .strip_prefix(b"Z1")
        .ok_or("not a search checkpoint (searches had none before they ran in steps)")?;
    let mut json = Vec::new();
    flate2::read::DeflateDecoder::new(z)
        .read_to_end(&mut json)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&json).map_err(|e| e.to_string())
}
