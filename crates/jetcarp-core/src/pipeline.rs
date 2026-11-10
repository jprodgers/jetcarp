//! The catalog pipeline's state: one `jobs` row of kind `pipeline` that records which step a
//! run is on, so a rerun after a crash or reboot skips the finished steps.
//!
//! The steps, in order (the orchestration lives in `jetcarp pipeline`, which runs the stage 1 and
//! stage 2 scripts and the local model; see the README's *Catalog pipeline*):
//!
//! 1. `stage1`: settings search on every grid (`scripts/stage1-all.rhai`, one labelled stage 1
//!    job per group of grids, census jobs on the aperiodic patches);
//! 2. `curate`: contact sheets of each space's best settings and the local model's shortlist
//!    (`tools/curate_settings.py`, ratings with rater `model:<name>`);
//! 3. `refit`: the settings score refit from the ratings (skipped below 20 rated settings);
//! 4. `stage2`: hunts of the rules worth hunting (`scripts/stage2-hunt.rhai`, one labelled hunt
//!    per space, soup searches on the aperiodic patches);
//! 5. `media`: media for the Quick Start's auto entries and the showcase patterns;
//! 6. `site`: the incremental website export.
//!
//! Each step is a job of its own (labelled `<pipeline label> · <step> · <space>`, so the hunt
//! dashboard tells them apart) or idempotent, so rerunning a step that was interrupted resumes
//! it. A step is `pending`, `running`, `done`, `skipped` (with a note, retried on the next run,
//! e.g. when the local model was not reachable), or `failed`.

use crate::library::{JobStatus, Library, now_utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Step names, in order.
pub const STEPS: [&str; 6] = ["stage1", "curate", "refit", "stage2", "media", "site"];

/// A step's title for people.
pub fn step_title(name: &str) -> &'static str {
    match name {
        "stage1" => "stage 1 settings search",
        "curate" => "contact sheets and model shortlist",
        "refit" => "score refit",
        "stage2" => "stage 2 hunts",
        "media" => "media",
        "site" => "website export",
        _ => "step",
    }
}

/// Everything that defines a pipeline run (`jobs.spec`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineSpec {
    /// `quick`, `standard`, `full`, or `test`: the budgets the scripts use.
    pub mode: String,
    /// The prefix of every child job's label (`pipeline 7`).
    pub label: String,
    /// Script parameters (`--set key=value`), passed to both scripts; they override the mode.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Local model for the shortlist (`qwen/qwen3.8-27b`), its server, and settings to rate.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub stub: bool,
    #[serde(default)]
    pub rate: u32,
    /// Website export folder.
    #[serde(default)]
    pub site: String,
    /// Patterns given media (best first; 0: no limit).
    #[serde(default)]
    pub media_limit: u32,
    /// Worker threads (0: all cores).
    #[serde(default)]
    pub threads: u32,
}

/// One step's state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepState {
    pub name: String,
    /// `pending`, `running`, `done`, `skipped`, `failed`.
    pub status: String,
    #[serde(default)]
    pub started: String,
    #[serde(default)]
    pub finished: String,
    /// Seconds over every session.
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub note: String,
    /// Child jobs.
    #[serde(default)]
    pub jobs: Vec<i64>,
    /// What it did (counts, model weights before and after, ...).
    #[serde(default)]
    pub summary: Value,
}

/// A pipeline's progress (`jobs.progress`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineProgress {
    pub job: i64,
    pub status: String,
    /// Index of the step running or next.
    pub step: usize,
    pub steps: Vec<StepState>,
    pub elapsed_s: f64,
    pub updated: String,
}

impl PipelineProgress {
    pub fn new(job: i64) -> PipelineProgress {
        PipelineProgress {
            job,
            status: "paused".into(),
            step: 0,
            steps: STEPS
                .iter()
                .map(|n| StepState {
                    name: n.to_string(),
                    status: "pending".into(),
                    ..Default::default()
                })
                .collect(),
            elapsed_s: 0.0,
            updated: now_utc(),
        }
    }
    /// Steps done (or skipped for good).
    pub fn done(&self) -> usize {
        self.steps.iter().filter(|s| s.status == "done").count()
    }
    /// `pipeline 7 (quick): step 4 of 6, stage 2 hunts (running)`.
    pub fn line(&self, spec: &PipelineSpec) -> String {
        let st = self.steps.get(self.step);
        format!(
            "{} ({}): step {} of {}, {} ({})",
            spec.label,
            spec.mode,
            (self.step + 1).min(self.steps.len()),
            self.steps.len(),
            st.map(|s| step_title(&s.name)).unwrap_or("finished"),
            st.map(|s| s.status.as_str()).unwrap_or(&self.status)
        )
    }
}

/// Create a pipeline job (status `paused` until it runs). Its label becomes `pipeline <id>`
/// unless the spec has one.
pub fn start(lib: &Library, spec: &PipelineSpec) -> Result<i64, String> {
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job("pipeline", &v)?;
    let mut spec = spec.clone();
    if spec.label.is_empty() {
        spec.label = format!("pipeline {job}");
    }
    lib.set_job_label(job, &spec.label)?;
    let p = PipelineProgress::new(job);
    save(lib, job, &p, JobStatus::Paused)?;
    Ok(job)
}

/// A pipeline's spec and progress.
pub fn load(lib: &Library, job: i64) -> Result<(PipelineSpec, PipelineProgress), String> {
    let j = lib.job(job)?;
    if j.kind != "pipeline" {
        return Err(format!("job {job} is a {} job, not a pipeline", j.kind));
    }
    let spec: PipelineSpec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    let mut p: PipelineProgress = serde_json::from_value(j.progress.clone())
        .ok()
        .filter(|p: &PipelineProgress| p.steps.len() == STEPS.len())
        .unwrap_or_else(|| PipelineProgress::new(job));
    p.job = job;
    p.status = status_name(j.status).into();
    Ok((spec, p))
}

/// Write a pipeline's progress and status.
pub fn save(lib: &Library, job: i64, p: &PipelineProgress, st: JobStatus) -> Result<(), String> {
    let mut p = p.clone();
    p.status = status_name(st).into();
    p.updated = now_utc();
    let v = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    lib.update_job(job, Some(st), Some(&v), None)
}

/// Ask a running pipeline (in any process) to pause: it stops its current step at that step's
/// next checkpoint.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let j = lib.job(job)?;
    if j.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// The newest pipeline that is not done.
pub fn latest_unfinished(lib: &Library) -> Result<Option<i64>, String> {
    Ok(lib
        .jobs()?
        .into_iter()
        .rev()
        .find(|j| j.kind == "pipeline" && j.status != JobStatus::Done)
        .map(|j| j.id))
}

fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}
