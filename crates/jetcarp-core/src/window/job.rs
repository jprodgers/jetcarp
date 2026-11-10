//! Long mover verifications as catalog jobs: a pattern followed on a sliding window
//! ([`super::track`]) in the background, with its progress in the job row and a checkpoint taken
//! right after a window is built (about every ten seconds), so a stopped or crashed run resumes
//! and ends exactly as an uninterrupted one. A job of kind `mover` has a [`MoverJobSpec`]; when it
//! ends verified it stores the mover in the catalog (with the sliding-window flag in its record).
//!
//! A run stops at its next window when its stop flag is set or another process marks the job
//! `paused` ([`request_stop`]).
//!
//! The job runner (`crate::jobs::mover`) runs [`run_job`] as it is, under its lease, in this
//! process or on a thread of it.

use super::track::{Tracker, WindowCheckpoint, WindowOpts};
use crate::library::movers::{MoverReport, Stop, TrackSpec, mover_record};
use crate::library::{DiscoveredBy, JobStatus, Library, Provenance, now_utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const KIND: &str = "mover";

/// Everything that defines a run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoverJobSpec {
    pub track: TrackSpec,
    #[serde(default)]
    pub window: WindowOpts,
    /// Store the mover in the catalog when it ends verified.
    #[serde(default = "yes")]
    pub store: bool,
    /// Who started it (`jetcarp movers`, a script, MCP), for the record's provenance.
    #[serde(default)]
    pub source: String,
}
fn yes() -> bool {
    true
}

/// Progress, written to the job row about every ten seconds and kept live in memory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MoverProgress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    pub generation: u64,
    pub max_gens: u64,
    pub rings: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_rings: Option<u32>,
    pub euclid: f64,
    pub population: usize,
    /// Supertile level (Spectre) or generation depth of the hierarchy.
    pub level: u32,
    pub tiles: usize,
    /// In this session.
    pub gens_per_s: f64,
    pub rings_per_s: f64,
    /// Over every session.
    pub elapsed_s: f64,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<MoverReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated: String,
}

impl MoverProgress {
    /// `mover 4: 12,345 rings (39,000 units) after 24,690 generations, 21,000 gens/s, level 12, 900 tiles`
    pub fn line(&self) -> String {
        format!(
            "mover {}: {} rings{} ({:.0} units) after {} of {} generations, {:.0} gens/s, {:.0} rings/s, level {}, {} tiles{}",
            self.job,
            self.rings,
            self.target_rings
                .map(|t| format!(" of {t}"))
                .unwrap_or_default(),
            self.euclid,
            self.generation,
            self.max_gens,
            self.gens_per_s,
            self.rings_per_s,
            self.level,
            self.tiles,
            match self.status.as_str() {
                "done" | "failed" => format!(" | {}", self.summary),
                _ => String::new(),
            }
        )
    }
}

/// A start that travels on a window: `states` on the tile `at` and, for a second state, one of
/// its edge neighbours; with no `at`, tiles near the patch's deepest tile are tried in order.
/// A start is taken when it runs 600 generations (or to the end) without dying, growing, or
/// repeating, and moves at least two rings.
pub fn travelling_start(
    rule: &str,
    nb: crate::topology::Nb,
    patch: &crate::topology::TilingSpec,
    states: &[u8],
    at: Option<u32>,
    opts: &WindowOpts,
) -> Result<Vec<(u32, u8)>, String> {
    use crate::topology::Nb;
    if states.is_empty() {
        return Err("give at least one state".into());
    }
    let topo = crate::library::site::patch_topology(patch)?;
    let roots: Vec<usize> = match at {
        Some(x) => vec![x as usize],
        None => {
            let (deep, _) = crate::evolve::arena::deepest(&topo);
            let c = topo.centroid(deep);
            let mut near: Vec<usize> = (0..topo.len()).collect();
            near.sort_by(|&p, &q| {
                crate::geom::dist(topo.centroid(p), c)
                    .total_cmp(&crate::geom::dist(topo.centroid(q), c))
            });
            near.truncate(12);
            near
        }
    };
    for r in roots {
        for k in 0..topo.degree(r, Nb::Edge) {
            let mut cells = vec![(r as u32, states[0])];
            if let (Some(&s), Some(u)) = (states.get(1), topo.slot(r, Nb::Edge, k)) {
                cells.push((u as u32, s));
            }
            let mut spec = TrackSpec::new(rule, nb, patch.clone(), cells.clone());
            spec.max_gens = 600;
            if let Ok(mut t) = Tracker::new(&spec, opts)
                && let Ok(Some(st)) = t.run(None, None)
                && st.verifies()
                && t.rings() >= 2
            {
                return Ok(cells);
            }
            if states.len() < 2 {
                break;
            }
        }
    }
    Err(format!(
        "no start with states {states:?} travels on a {} window under {rule}",
        patch.family
    ))
}

/// Create a run (status `paused` until [`run_job`] starts it). Fails early on a bad spec.
pub fn start(lib: &Library, spec: &MoverJobSpec) -> Result<i64, String> {
    // Builds the first window and checks the rule and the start.
    Tracker::new(&spec.track, &spec.window)?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

fn job_spec(
    lib: &Library,
    job: i64,
) -> Result<(crate::library::store::JobRow, MoverJobSpec), String> {
    let j = lib.job(job)?;
    if j.kind != KIND {
        return Err(format!("job {job} is a {} job, not a mover run", j.kind));
    }
    let spec: MoverJobSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    Ok((j, spec))
}

/// Ask a running job (in any process) to stop at its next window.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let (j, _) = job_spec(lib, job)?;
    if j.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

/// A job's spec and last saved progress.
pub fn status(lib: &Library, job: i64) -> Result<(MoverJobSpec, MoverProgress), String> {
    let (j, spec) = job_spec(lib, job)?;
    let mut p: MoverProgress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.status = status_name(j.status).into();
    p.max_gens = spec.track.max_gens;
    p.target_rings = spec.window.target_rings;
    Ok((spec, p))
}

fn progress(job: i64, t: &Tracker, status: &str, session: (Instant, u64, u32)) -> MoverProgress {
    let rep = t.report();
    let secs = session.0.elapsed().as_secs_f64().max(1e-9);
    MoverProgress {
        job,
        status: status.into(),
        generation: t.generation(),
        max_gens: t.spec.max_gens,
        rings: rep.reach.rings,
        target_rings: t.opts.target_rings,
        euclid: rep.reach.euclid,
        population: t.live().len(),
        level: t.level(),
        tiles: t.window().len(),
        gens_per_s: ((t.generation() - session.1) as f64 / secs).round(),
        rings_per_s: ((rep.reach.rings.saturating_sub(session.2)) as f64 / secs).round(),
        elapsed_s: (t.seconds() * 10.0).round() / 10.0,
        summary: rep.summary,
        record: None,
        report: None,
        error: None,
        updated: now_utc(),
    }
}

/// Run (or resume) a job until it ends or is stopped. Returns its final progress.
pub fn run_job(
    lib: &Library,
    job: i64,
    stop: Arc<AtomicBool>,
    live: Option<Arc<Mutex<MoverProgress>>>,
) -> Result<MoverProgress, String> {
    let (j, spec) = job_spec(lib, job)?;
    if j.status == JobStatus::Done {
        return status(lib, job).map(|x| x.1);
    }
    let fail = |e: String| {
        let _ = lib.update_job(
            job,
            Some(JobStatus::Failed),
            Some(&serde_json::json!({"status": "failed", "error": e, "updated": now_utc()})),
            None,
        );
        e
    };
    let mut t = match lib.job_checkpoint(job)? {
        Some(b) => {
            let cp: WindowCheckpoint =
                serde_json::from_slice(&b).map_err(|e| format!("job {job} checkpoint: {e}"))?;
            Tracker::resume(&spec.track, &spec.window, &cp).map_err(fail)?
        }
        None => Tracker::new(&spec.track, &spec.window).map_err(fail)?,
    };
    lib.update_job(job, Some(JobStatus::Running), None, None)?;
    let session = (Instant::now(), t.generation(), t.rings());
    let mut last_save = Instant::now();
    let publish = |p: &MoverProgress| {
        if let Some(l) = &live {
            *l.lock().unwrap() = p.clone();
        }
    };
    let end = loop {
        let until = Instant::now() + Duration::from_secs(2);
        match t.run(Some(until), Some(&stop)).map_err(fail)? {
            Some(s) => break Some(s),
            None => {
                let paused = stop.load(Ordering::SeqCst)
                    || lib.job(job).map(|j| j.status) == Ok(JobStatus::Paused);
                if paused {
                    // Run on to the next window so the checkpoint is current.
                    let mut n = 0;
                    while t.checkpoint().is_none() && t.stopped().is_none() && n < 100_000 {
                        t.step_once().map_err(fail)?;
                        n += 1;
                    }
                    if let Some(s) = t.stopped() {
                        break Some(s);
                    }
                    break None;
                }
                let p = progress(job, &t, "running", session);
                publish(&p);
                if last_save.elapsed() >= Duration::from_secs(10)
                    && let Some(cp) = t.checkpoint()
                {
                    last_save = Instant::now();
                    let pj = serde_json::to_value(&p).unwrap_or_default();
                    let cj = serde_json::to_vec(&cp).map_err(|e| e.to_string())?;
                    lib.update_job(job, None, Some(&pj), Some(&cj))?;
                }
            }
        }
    };
    match end {
        None => {
            let p = progress(job, &t, "paused", session);
            let pj = serde_json::to_value(&p).unwrap_or_default();
            let cj = t
                .checkpoint()
                .map(|cp| serde_json::to_vec(&cp).unwrap_or_default());
            lib.update_job(job, Some(JobStatus::Paused), Some(&pj), cj.as_deref())?;
            publish(&p);
            Ok(p)
        }
        Some(s) => {
            let rep = t.report();
            let mut p = progress(job, &t, "done", session);
            if spec.store && rep.verified && s != Stop::Stopped {
                let prov = Provenance {
                    source: if spec.source.is_empty() {
                        "mover job".into()
                    } else {
                        spec.source.clone()
                    },
                    tiling: Some(spec.track.patch.clone()),
                    jetcarp_version: crate::VERSION.into(),
                    date: now_utc(),
                    ..Default::default()
                };
                match mover_record(&spec.track, &rep, prov, DiscoveredBy::User) {
                    Ok(rec) => {
                        let id = rec.id.clone();
                        match lib.add_in_job(rec, job) {
                            Ok(_) => p.record = Some(id),
                            Err(e) => p.error = Some(e),
                        }
                    }
                    Err(e) => p.error = Some(e),
                }
            }
            p.report = Some(rep);
            let pj = serde_json::to_value(&p).unwrap_or_default();
            lib.update_job(job, Some(JobStatus::Done), Some(&pj), None)?;
            publish(&p);
            Ok(p)
        }
    }
}
