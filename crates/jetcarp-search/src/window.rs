//! Long mover verifications as catalog jobs: a pattern followed on a sliding window
//! ([`jetcarp_core::window::track`]) in the background, with its progress in the job row and a
//! checkpoint taken right after a window is built (every step, about every two seconds), so a
//! stopped or crashed run resumes and ends exactly as an uninterrupted one. A job of kind `mover`
//! has a [`MoverJobSpec`]; when it ends verified it stores the mover in the catalog (with the
//! sliding-window flag in its record).
//!
//! A run stops at its next window when its stop flag is set or another process marks the job
//! `paused` ([`request_stop`]).
//!
//! The job runner (`crate::jobs::mover`) runs a job in steps of a couple of seconds each
//! ([`MoverJob`]), under its lease, in this process or on a thread of it.

use jetcarp_core::library::{JobStatus, Library};
use jetcarp_core::pattern::movers::{MoverReport, Stop, TrackSpec, mover_record};
use jetcarp_core::pattern::{DiscoveredBy, PatternRecord, Provenance, now_utc};
use jetcarp_core::window::track::{Tracker, WindowCheckpoint, WindowOpts};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Who started it (`jetcarp search mover`, a script, MCP), for the record's provenance.
    #[serde(default)]
    pub source: String,
}
fn yes() -> bool {
    true
}

/// Progress, written to the job row after every step (about every two seconds) and kept live
/// in memory.
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
    nb: jetcarp_core::topology::Nb,
    patch: &jetcarp_core::topology::TilingSpec,
    states: &[u8],
    at: Option<u32>,
    opts: &WindowOpts,
) -> Result<Vec<(u32, u8)>, String> {
    use jetcarp_core::topology::Nb;
    if states.is_empty() {
        return Err("give at least one state".into());
    }
    let topo = jetcarp_core::pattern::site::patch_topology(patch)?;
    let roots: Vec<usize> = match at {
        Some(x) => vec![x as usize],
        None => {
            let (deep, _) = jetcarp_core::evolve::arena::deepest(&topo);
            let c = topo.centroid(deep);
            let mut near: Vec<usize> = (0..topo.len()).collect();
            near.sort_by(|&p, &q| {
                jetcarp_core::geom::dist(topo.centroid(p), c)
                    .total_cmp(&jetcarp_core::geom::dist(topo.centroid(q), c))
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

/// Create a run (status `paused` until a runner starts it). Fails early on a bad spec.
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
) -> Result<(jetcarp_core::library::store::JobRow, MoverJobSpec), String> {
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

/// A mover run in steps by the job runner (`crate::jobs::mover`): each step runs the tracker
/// for a slice of time (two seconds by default) and on to the next window it builds, where its
/// checkpoint is taken (once it is past the repeat check's first 4,096 generations; before
/// that a step ends at the slice and there is no checkpoint yet). A stop part way through a
/// slice runs on to the next window too, so the checkpoint written with the pause is current.
/// When the run ends verified, [`MoverJob::finish`] makes the mover's record for the sink.
pub struct MoverJob {
    job: i64,
    spec: MoverJobSpec,
    t: Tracker,
    /// Session start, and the generation and rings then.
    session: (Instant, u64, u32),
    slice: Duration,
    end: Option<Stop>,
    done: Option<MoverProgress>,
    /// The id of the record in the sink, until it is stored.
    record: Option<String>,
}

impl MoverJob {
    /// Open the run's job from its checkpoint (a window frame), if it has one, with steps of
    /// `seconds` (0: two).
    pub fn open(
        cx: &crate::jobs::RunCx<'_>,
        checkpoint: Option<&[u8]>,
        seconds: f64,
    ) -> Result<MoverJob, String> {
        let (j, job) = (cx.row, cx.job);
        if j.kind != KIND {
            return Err(format!("job {job} is a {} job, not a mover run", j.kind));
        }
        let spec: MoverJobSpec =
            serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
        let t = match checkpoint {
            Some(b) => {
                let cp: WindowCheckpoint =
                    serde_json::from_slice(b).map_err(|e| format!("job {job} checkpoint: {e}"))?;
                Tracker::resume(&spec.track, &spec.window, &cp)?
            }
            None => Tracker::new(&spec.track, &spec.window)?,
        };
        let slice = if seconds > 0.0 && seconds.is_finite() {
            Duration::from_secs_f64(seconds)
        } else {
            Duration::from_secs(2)
        };
        Ok(MoverJob {
            job,
            spec,
            session: (Instant::now(), t.generation(), t.rings()),
            t,
            slice,
            end: None,
            done: None,
            record: None,
        })
    }

    /// One slice. `Ok(true)`: the run has ended (verified or not).
    pub fn step(&mut self, stop: &AtomicBool) -> Result<bool, String> {
        let t = &mut self.t;
        let until = Instant::now() + self.slice;
        if let Some(s) = t.run(Some(until), Some(stop))? {
            self.end = Some(s);
            return Ok(true);
        }
        if stop.load(Ordering::SeqCst) {
            // Run on to the next window so the checkpoint is current.
            let mut n = 0;
            while t.checkpoint().is_none() && t.stopped().is_none() && n < 100_000 {
                t.step_once()?;
                n += 1;
            }
            if let Some(s) = t.stopped() {
                self.end = Some(s);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The run has ended: its report, and the mover's record when it ended verified and the
    /// job stores it (credited to the job when the runner writes it).
    pub fn finish(&mut self) -> Option<PatternRecord> {
        let rep = self.t.report();
        let mut p = progress(self.job, &self.t, "done", self.session);
        let mut rec = None;
        let spec = &self.spec;
        if spec.store && rep.verified && self.end != Some(Stop::Stopped) {
            let prov = Provenance {
                source: if spec.source.is_empty() {
                    "mover job".into()
                } else {
                    spec.source.clone()
                },
                tiling: Some(spec.track.patch.clone()),
                jetcarp_version: jetcarp_core::VERSION.into(),
                date: now_utc(),
                ..Default::default()
            };
            match mover_record(&spec.track, &rep, prov, DiscoveredBy::User) {
                Ok(r) => {
                    self.record = Some(r.id.clone());
                    rec = Some(r);
                }
                Err(e) => p.error = Some(e),
            }
        }
        p.report = Some(rep);
        self.done = Some(p);
        rec
    }

    /// What became of the record: stored (its id in the progress), or the error.
    pub fn stored(&mut self, outcome: Option<&Result<jetcarp_core::library::AddOutcome, String>>) {
        if let (Some(id), Some(o), Some(p)) = (self.record.take(), outcome, self.done.as_mut()) {
            match o {
                Ok(_) => p.record = Some(id),
                Err(e) => p.error = Some(e.clone()),
            }
        }
    }

    /// The progress with this status (once ended: the final one).
    pub fn progress(&self, status: &str) -> MoverProgress {
        match &self.done {
            Some(p) => p.clone(),
            None => progress(self.job, &self.t, status, self.session),
        }
    }

    /// The window frame to resume from, when the tracker is right after building one.
    pub fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        self.t
            .checkpoint()
            .map(|cp| serde_json::to_vec(&cp).map_err(|e| e.to_string()))
            .transpose()
    }
}
