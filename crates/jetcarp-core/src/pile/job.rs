//! Large sandpiles as catalog jobs (kind `pile`), so they are started, watched, and stopped
//! like hunts and searches (`job_status`, `job_stop` over MCP).
//!
//! The pile itself already resumes from its folder: every finished doubling step is saved to
//! `steps/step-KK.pile`. The job row holds the spec and folder, and its progress (the steps
//! done, grains, topplings, the step in flight) is written after every step and every few seconds
//! inside one; a resumed job loads the folder's latest step file. A stop request (the job marked
//! `paused`, or the stop flag) cancels the step in flight, which the next run repeats.

use super::export::ExportOptions;
use super::{Ctl, Pile, PileSpec, RunOptions, Toppler, file, latest_step_file, write_growth};
use crate::library::{JobStatus, Library, now_utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const KIND: &str = "pile";

/// A pile job: what to build, where, and how to export it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PileJobSpec {
    pub pile: PileSpec,
    /// The pile's folder (steps, growth table, pictures, metadata).
    pub folder: PathBuf,
    pub px_per_cell: f64,
    pub margin: f64,
    pub palette: String,
    /// Run the palette the other way (height 1 at its last stop).
    #[serde(default)]
    pub reverse: bool,
    pub preview_side: u32,
    pub png: bool,
    /// Topple on the GPU when the host has one (periodic tilings).
    #[serde(default)]
    pub gpu: bool,
    /// Extra metadata for `pile.json` (who started it, where); the job id is added.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub extra: serde_json::Value,
}

/// Progress of a pile job.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PileProgress {
    pub job: i64,
    pub status: String,
    pub grains: u64,
    pub tiling: String,
    /// Doubling steps done and in all.
    pub step: u32,
    pub steps: u32,
    /// Grains in the pile after the steps done.
    pub grains_done: u64,
    pub topplings: u64,
    pub sweeps: u64,
    /// Seconds of toppling (finished steps).
    pub seconds: f64,
    /// The step in flight: sweeps, topplings, seconds.
    #[serde(default)]
    pub in_step: [f64; 3],
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated: String,
}

impl PileProgress {
    pub fn line(&self) -> String {
        format!(
            "pile {}: {} grains on {}, step {}/{} ({} grains), {:.3e} topplings, {}{}",
            self.job,
            self.grains,
            self.tiling,
            self.step,
            self.steps,
            self.grains_done,
            self.topplings as f64,
            super::fmt_seconds(self.seconds),
            if self.in_step[2] > 0.0 {
                format!(
                    "; step {} running {}",
                    self.step + 1,
                    super::fmt_seconds(self.in_step[2])
                )
            } else {
                String::new()
            }
        )
    }
}

fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

fn progress_of(job: i64, pile: &Pile, spec: &PileJobSpec, status: &str) -> PileProgress {
    let (top, sweeps, secs) = pile.totals();
    PileProgress {
        job,
        status: status.into(),
        grains: pile.spec.grains,
        tiling: super::tiling_label(&pile.spec.tiling),
        step: pile.done,
        steps: pile.steps,
        grains_done: pile.grains_after(pile.done),
        topplings: top,
        sweeps,
        seconds: secs,
        in_step: [0.0; 3],
        folder: spec.folder.display().to_string(),
        backend: pile.records.last().map(|r| r.backend.clone()),
        updated: now_utc(),
        ..Default::default()
    }
}

/// Create a pile job (paused until [`run_job`]). Returns the job id.
pub fn start(lib: &Library, spec: &PileJobSpec) -> Result<i64, String> {
    crate::render::builtin_palette(&spec.palette)?;
    let pile = Pile::new(spec.pile.clone())?;
    std::fs::create_dir_all(spec.folder.join("steps"))
        .map_err(|e| format!("{}: {e}", spec.folder.display()))?;
    super::export::write_look_file(&spec.folder, &spec.palette, spec.reverse)?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND, &v)?;
    let p = serde_json::to_value(progress_of(job, &pile, spec, "paused")).unwrap_or_default();
    lib.update_job(job, Some(JobStatus::Paused), Some(&p), None)?;
    Ok(job)
}

fn spec_of(lib: &Library, job: i64) -> Result<(JobStatus, PileJobSpec, serde_json::Value), String> {
    let j = lib.job(job)?;
    if j.kind != KIND {
        return Err(format!("job {job} is a {} job, not a pile", j.kind));
    }
    let spec = serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job}: {e}"))?;
    Ok((j.status, spec, j.progress))
}

/// The job's spec and last saved progress.
pub fn status(lib: &Library, job: i64) -> Result<(PileJobSpec, PileProgress), String> {
    let (st, spec, p) = spec_of(lib, job)?;
    let mut p: PileProgress = serde_json::from_value(p).unwrap_or_default();
    p.job = job;
    p.status = status_name(st).into();
    Ok((spec, p))
}

/// Ask a running pile job (in any process) to stop; the step in flight is cancelled.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let (st, ..) = spec_of(lib, job)?;
    if st == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// Run (or resume) a pile job until the pile is finished and exported, or stopped.
pub fn run_job(
    lib: &Library,
    job: i64,
    stop: Arc<AtomicBool>,
    mut toppler: Option<Box<dyn Toppler>>,
    live: Option<Arc<Mutex<PileProgress>>>,
) -> Result<PileProgress, String> {
    let (st, spec, _) = spec_of(lib, job)?;
    if st == JobStatus::Done {
        return status(lib, job).map(|x| x.1);
    }
    let mut pile = match latest_step_file(&spec.folder) {
        Some(f) => file::load(&f)?,
        None => Pile::new(spec.pile.clone())?,
    };
    lib.update_job(job, Some(JobStatus::Running), None, None)?;
    let publish = |p: &PileProgress, status: Option<JobStatus>| {
        let v = serde_json::to_value(p).unwrap_or_default();
        let _ = lib.update_job(job, status, Some(&v), None);
        if let Some(l) = &live {
            *l.lock().unwrap() = p.clone();
        }
    };
    let cancel = AtomicBool::new(false);
    let mut last_check = Instant::now();
    while !pile.finished() {
        let base = progress_of(job, &pile, &spec, "running");
        publish(&base, None);
        if stop.load(Ordering::SeqCst) || lib.job(job).map(|j| j.status) == Ok(JobStatus::Paused) {
            publish(
                &progress_of(job, &pile, &spec, "paused"),
                Some(JobStatus::Paused),
            );
            return Ok(progress_of(job, &pile, &spec, "paused"));
        }
        let r = {
            let mut ctl = Ctl {
                cancel: Some(&cancel),
                tick_every: 1.0,
                on_tick: Some(Box::new(|_, t, _| {
                    let mut p = base.clone();
                    p.in_step = [t.sweeps as f64, t.topplings as f64, t.seconds];
                    p.updated = now_utc();
                    publish(&p, None);
                    if stop.load(Ordering::SeqCst)
                        || (last_check.elapsed().as_secs_f64() >= 2.0
                            && lib.job(job).map(|j| j.status) == Ok(JobStatus::Paused))
                    {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    last_check = Instant::now();
                })),
                ..Default::default()
            };
            match toppler.as_mut() {
                Some(t) => pile.next_step(Some(&mut **t), &mut ctl),
                None => pile.next_step(None, &mut ctl),
            }
        };
        match r {
            Ok(_) => {
                file::save(
                    &pile,
                    &spec
                        .folder
                        .join(format!("steps/step-{:02}.pile", pile.done)),
                )?;
                write_growth(&pile, &spec.folder)?;
            }
            Err(_) if cancel.load(Ordering::SeqCst) => {
                let p = progress_of(job, &pile, &spec, "paused");
                publish(&p, Some(JobStatus::Paused));
                return Ok(p);
            }
            Err(e) => {
                let mut p = progress_of(job, &pile, &spec, "failed");
                p.error = Some(e.clone());
                publish(&p, Some(JobStatus::Failed));
                return Err(e);
            }
        }
    }
    let opts = RunOptions {
        export: ExportOptions {
            px_per_cell: spec.px_per_cell,
            margin: spec.margin,
            palette: spec.palette.clone(),
            reverse: spec.reverse,
            preview_side: spec.preview_side,
            png: spec.png,
        },
        save_steps: false,
        extra: {
            let mut x = match &spec.extra {
                serde_json::Value::Object(m) => m.clone(),
                _ => serde_json::Map::new(),
            };
            x.insert("job".into(), serde_json::json!(job));
            serde_json::Value::Object(x)
        },
    };
    let mut quiet = Ctl::default();
    let s = super::run_to_dir(&mut pile, &spec.folder, None, &mut quiet, &opts)?;
    let mut p = progress_of(job, &pile, &spec, "done");
    p.png = s.png.map(|x| x.display().to_string());
    p.preview = s.preview.map(|x| x.display().to_string());
    publish(&p, Some(JobStatus::Done));
    Ok(p)
}
