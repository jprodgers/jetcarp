//! A census as a catalog job (gap 6). [`start_job`] stores a census as a catalog job; the job
//! runner (`crate::jobs::census`) runs its rules in chunks, a step each ([`CensusJob`]), writing
//! each chunk's summaries (`census_rows`) with the progress and checkpoint in one transaction, so
//! an interrupted census resumes where it stopped and [`job_report`] gives the same report as an
//! uninterrupted run. The census itself is `jetcarp_core::census`.

use jetcarp_core::census::{
    CensusOptions, CensusReport, CensusSpec, RuleSummary, check_spec, report, run_rules,
};
use jetcarp_core::topology::Topology;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A census as a catalog job.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CensusProgress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    pub rules_done: u64,
    pub rules_total: u64,
    /// Rules per second in this session.
    pub per_s: f64,
    /// Seconds run, over every session.
    pub elapsed_s: f64,
    pub updated: String,
}

impl CensusProgress {
    pub fn line(&self) -> String {
        format!(
            "census {}: {}/{} rules, {:.0} rules/s, {}",
            self.job, self.rules_done, self.rules_total, self.per_s, self.status
        )
    }
}

/// Store a census as a job (status `paused` until a runner starts it).
pub fn start_job(lib: &jetcarp_core::library::Library, spec: &CensusSpec) -> Result<i64, String> {
    check_spec(spec)?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job("census", &v)?;
    lib.update_job(
        job,
        Some(jetcarp_core::library::JobStatus::Paused),
        None,
        None,
    )?;
    Ok(job)
}

fn census_job(
    lib: &jetcarp_core::library::Library,
    job: i64,
) -> Result<(jetcarp_core::library::store::JobRow, CensusSpec), String> {
    let j = lib.job(job)?;
    if j.kind != "census" {
        return Err(format!("job {job} is a {} job, not a census", j.kind));
    }
    let spec: CensusSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    Ok((j, spec))
}

/// A census job's spec and saved progress.
pub fn job_status(
    lib: &jetcarp_core::library::Library,
    job: i64,
) -> Result<(CensusSpec, CensusProgress), String> {
    use jetcarp_core::library::JobStatus;
    let (j, spec) = census_job(lib, job)?;
    let mut p: CensusProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.rules_total = spec.rules.len() as u64;
    p.status = match j.status {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
    .into();
    Ok((spec, p))
}

/// Ask a running census job (in any process) to pause at its next chunk.
pub fn request_stop(lib: &jetcarp_core::library::Library, job: i64) -> Result<(), String> {
    let (j, _) = census_job(lib, job)?;
    if j.status == jetcarp_core::library::JobStatus::Running {
        lib.update_job(
            job,
            Some(jetcarp_core::library::JobStatus::Paused),
            None,
            None,
        )?;
    }
    Ok(())
}

/// A census job run in steps by the job runner (`crate::jobs::census`): each step runs one
/// chunk of rules (adapting to about three seconds of work) and puts their summaries in the
/// sink, which the runner writes with the progress and checkpoint (the next rule index) in one
/// transaction.
pub struct CensusJob {
    spec: CensusSpec,
    topo: Arc<Topology>,
    pool: rayon::ThreadPool,
    /// The next rule index.
    next: u64,
    /// Rules per chunk, and whether it adapts.
    chunk: u64,
    adapt: bool,
    p: CensusProgress,
    session: std::time::Instant,
    session_done: u64,
    base_elapsed: f64,
}

impl CensusJob {
    /// Open the run's census job (on its threads; `chunk` rules per step, 0: adapt) from its
    /// checkpoint, if it has one (the next rule index, as it always was).
    pub fn open(
        cx: &crate::jobs::RunCx<'_>,
        checkpoint: Option<&[u8]>,
        chunk: u64,
    ) -> Result<CensusJob, String> {
        let (j, job) = (cx.row, cx.job);
        if j.kind != "census" {
            return Err(format!("job {job} is a {} job, not a census", j.kind));
        }
        let spec: CensusSpec =
            serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
        check_spec(&spec)?;
        let next: u64 = match checkpoint {
            Some(b) => serde_json::from_slice::<u64>(b).map_err(|e| format!("checkpoint: {e}"))?,
            None => 0,
        };
        let mut p: CensusProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
        p.job = job;
        p.rules_total = spec.rules.len() as u64;
        let topo = Arc::new(Topology::build(&spec.tiling)?);
        let threads = if cx.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            cx.threads
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(CensusJob {
            topo,
            pool,
            next,
            chunk: if chunk > 0 { chunk } else { 32 },
            adapt: chunk == 0,
            base_elapsed: p.elapsed_s,
            p,
            spec,
            session: std::time::Instant::now(),
            session_done: 0,
        })
    }

    /// One chunk of rules (none past the end or the item budget), its summaries put in the
    /// sink. `Ok(true)`: the census has run its last rule.
    pub fn step(
        &mut self,
        cx: &crate::jobs::RunCx<'_>,
        sink: &mut crate::jobs::Sink,
    ) -> Result<bool, String> {
        let total = self.spec.rules.len() as u64;
        if self.next >= total {
            return Ok(true);
        }
        let mut n = self.chunk.min(total - self.next);
        if let Some(l) = cx.items {
            n = n.min(l - self.session_done).max(1);
        }
        let which: Vec<usize> = (self.next as usize..(self.next + n) as usize).collect();
        let t0 = std::time::Instant::now();
        let (topo, spec) = (&self.topo, &self.spec);
        let out = self
            .pool
            .install(|| run_rules(topo, spec, &which, &CensusOptions::default(), None));
        sink.census
            .extend(which.iter().zip(&out).map(|(&i, (r, _))| {
                (
                    i,
                    r.rule.clone(),
                    serde_json::to_value(r).unwrap_or(serde_json::Value::Null),
                )
            }));
        self.next += n;
        self.session_done += n;
        let el = self.session.elapsed().as_secs_f64();
        let p = &mut self.p;
        p.rules_done = self.next;
        p.rules_total = total;
        p.per_s = self.session_done as f64 / el.max(1e-9);
        p.elapsed_s = self.base_elapsed + el;
        p.updated = jetcarp_core::pattern::now_utc();
        if self.adapt {
            let dt = t0.elapsed().as_secs_f64();
            let want = (n as f64 * 3.0 / dt.max(1e-3)).clamp(4.0, 4096.0) as u64;
            self.chunk = ((self.chunk + want) / 2).max(4);
        }
        Ok(self.next >= total)
    }

    /// The progress (as of the last chunk) with this status.
    pub fn progress(&self, status: &str) -> CensusProgress {
        CensusProgress {
            status: status.into(),
            ..self.p.clone()
        }
    }

    /// The checkpoint: the next rule index.
    pub fn checkpoint(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.next).map_err(|e| e.to_string())
    }

    /// Rules run in this session.
    pub fn rules_run(&self) -> u64 {
        self.session_done
    }
}

/// The report of a census job (all its rules, once done; the rules so far otherwise).
pub fn job_report(lib: &jetcarp_core::library::Library, job: i64) -> Result<CensusReport, String> {
    let (_, spec) = census_job(lib, job)?;
    let topo = Topology::build(&spec.tiling)?;
    let rules: Vec<RuleSummary> = lib
        .census_rows(job)?
        .into_iter()
        .map(|v| serde_json::from_value(v).map_err(|e| format!("census row: {e}")))
        .collect::<Result<_, _>>()?;
    Ok(report(&spec, &topo, rules))
}
