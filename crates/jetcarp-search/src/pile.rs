//! Large sandpiles as catalog jobs (kind `pile`), so they are started, watched, and stopped
//! like hunts and searches (`job_status`, `job_stop` over MCP).
//!
//! The pile itself already resumes from its folder: every finished doubling step is saved to
//! `steps/step-KK.pile`. The job row holds the spec and folder, and its progress (the steps
//! done, grains, topplings, the step in flight) is written after every step and every second
//! inside one, with the time left once two steps have run; a resumed job loads the folder's
//! latest step file. A stop request (the job marked `paused`, or the stop flag, which a time
//! budget sets too) stops the step in flight and saves it as a checkpoint
//! (`steps/step-KK.partial.pile`: the board part-toppled, its grains added, the step's counts so
//! far), so the next run carries on toppling it rather than starting the step again. The job
//! runner (`crate::jobs::pile`) runs a job a doubling step at a time ([`PileJob`]).
//!
//! **Watching.** While someone watches the run's live state (`crate::jobs::Viewer`: the app's
//! Sandpile window), the job shows its board every second and after each step, as a
//! [`jetcarp_core::pile::lattice::LatticeBoard`] or [`jetcarp_core::pile::patch::PatchBoard`]
//! ([`jetcarp_core::pile::BoardRef::from_any`]), the GPU copying it back for each.
//!
//! **Colours.** The export takes its palette and direction from the folder's `look.json`
//! (written when the job is made, from its spec), so whoever changes them while the pile builds
//! (the Sandpile window) gets the export in the new colours.

use jetcarp_core::pattern::now_utc;
use jetcarp_core::pile::export::ExportOptions;
use jetcarp_core::pile::{
    Ctl, Pile, PileSpec, RunOptions, StepRecord, Toppler, file, latest_step_file, save_to_dir,
    write_growth,
};
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub grains: u64,
    pub tiling: String,
    /// Doubling steps done and in all.
    pub step: u32,
    pub steps: u32,
    /// Grains in the pile after the steps done.
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub grains_done: u64,
    /// Beyond 2^53 on a pile of a billion grains or so: a decimal string in JSON then.
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub topplings: u64,
    pub sweeps: u64,
    /// Seconds of toppling (finished steps).
    pub seconds: f64,
    /// The step in flight, or stopped part way (its checkpoint): sweeps, topplings, seconds so
    /// far.
    #[serde(default)]
    pub in_step: InFlight,
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// What topples it: a GPU's name, or `cpu, N threads`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Seconds left, while a step runs and two have run before it
    /// ([`jetcarp_core::pile::eta_seconds`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_s: Option<f64>,
    /// The last finished step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_step: Option<StepRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated: String,
}

/// What the step in flight has done so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InFlight {
    pub sweeps: u64,
    #[serde(with = "jetcarp_core::json_int::uint")]
    pub topplings: u64,
    pub seconds: f64,
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
            jetcarp_core::pile::fmt_seconds(self.seconds),
            if self.in_step.seconds > 0.0 {
                format!(
                    "; step {} {} {}",
                    self.step + 1,
                    if self.status == "running" {
                        "running"
                    } else {
                        "stopped part way after"
                    },
                    jetcarp_core::pile::fmt_seconds(self.in_step.seconds)
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
        tiling: jetcarp_core::pile::tiling_label(&pile.spec.tiling),
        step: pile.done,
        steps: pile.steps,
        grains_done: pile.grains_after(pile.done),
        topplings: top,
        sweeps,
        seconds: secs,
        in_step: pile
            .in_step
            .map(|x| InFlight {
                sweeps: x.sweeps,
                topplings: x.topplings,
                seconds: x.seconds,
            })
            .unwrap_or_default(),
        folder: spec.folder.display().to_string(),
        backend: pile.records.last().map(|r| r.backend.clone()),
        last_step: pile.records.last().cloned(),
        updated: now_utc(),
        ..Default::default()
    }
}

/// Create a pile job (paused until a runner starts it). Returns the job id.
pub fn start(lib: &Library, spec: &PileJobSpec) -> Result<i64, String> {
    jetcarp_core::render::builtin_palette(&spec.palette)?;
    let pile = Pile::new(spec.pile.clone())?;
    std::fs::create_dir_all(spec.folder.join("steps"))
        .map_err(|e| format!("{}: {e}", spec.folder.display()))?;
    jetcarp_core::pile::export::write_look_file(&spec.folder, &spec.palette, spec.reverse)?;
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

/// Ask a running pile job (in any process) to stop; the step in flight is saved as a checkpoint.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let (st, ..) = spec_of(lib, job)?;
    if st == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// A pile job run in steps by the job runner (`crate::jobs::pile`): one doubling step each,
/// saved to the folder (`steps/step-KK.pile`, `growth.csv`) when it finishes. While a step runs
/// it publishes its progress every second and stops within about a second of a stop (the run's
/// stop flag, which a time budget sets too, or the job marked `paused`, checked every two
/// seconds); the stopped step is saved as a checkpoint (`steps/step-KK.partial.pile`) that the
/// next step carries on. At the end the pile is exported (`pile.png`, `preview.png`,
/// `pile.json`).
pub struct PileJob {
    job: i64,
    spec: PileJobSpec,
    pile: Pile,
    /// A step failed to save its checkpoint: the pile is reloaded from its folder before the next.
    stale: bool,
    /// What topples it in this run (set at its first step).
    backend: Option<String>,
    done: Option<PileProgress>,
}

impl PileJob {
    /// Open the run's pile job from the latest step file in its folder (a fresh pile if none).
    pub fn open(cx: &crate::jobs::RunCx<'_>) -> Result<PileJob, String> {
        let (j, job) = (cx.row, cx.job);
        if j.kind != KIND {
            return Err(format!("job {job} is a {} job, not a pile", j.kind));
        }
        let spec: PileJobSpec =
            serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job}: {e}"))?;
        let pile = load(&spec)?;
        Ok(PileJob {
            job,
            spec,
            pile,
            stale: false,
            backend: None,
            done: None,
        })
    }

    /// One doubling step on `toppler` (the CPU when `None`). `Ok(true)`: the pile is finished.
    pub fn step(
        &mut self,
        cx: &crate::jobs::RunCx<'_>,
        toppler: Option<&mut dyn Toppler>,
    ) -> Result<bool, String> {
        if self.stale {
            self.pile = load(&self.spec)?;
            self.stale = false;
        }
        if self.pile.finished() {
            return Ok(true);
        }
        let backend = self
            .backend
            .get_or_insert_with(|| match &toppler {
                Some(t) => t.name(),
                None => format!("cpu, {} threads", rayon::current_num_threads()),
            })
            .clone();
        let mut base = self.progress("running");
        base.backend = Some(backend);
        let records = self.pile.records.clone();
        let cancel = AtomicBool::new(false);
        let mut last_check = Instant::now();
        let r = {
            let mut ctl = Ctl {
                cancel: Some(&cancel),
                tick_every: 1.0,
                // A watcher draws the board: the GPU copies it back for each tick.
                needs_board: cx.viewing(),
                on_tick: Some(Box::new(|_, t, b| {
                    let mut p = base.clone();
                    p.in_step = InFlight {
                        sweeps: t.sweeps,
                        topplings: t.topplings,
                        seconds: t.seconds,
                    };
                    p.eta_s = jetcarp_core::pile::eta_seconds(&records, p.steps, t.seconds);
                    p.updated = now_utc();
                    cx.publish(serde_json::to_value(&p).unwrap_or_default());
                    if cx.viewing() {
                        cx.show(b.as_any());
                    }
                    let mut stop = cx.stopped();
                    if !stop && last_check.elapsed().as_secs_f64() >= 2.0 {
                        last_check = Instant::now();
                        stop = cx.paused_elsewhere();
                    }
                    if stop {
                        cancel.store(true, Ordering::SeqCst);
                    }
                })),
                ..Default::default()
            };
            self.pile.next_step(toppler, &mut ctl)
        };
        match r {
            Ok(_) => {
                save_to_dir(&self.pile, &self.spec.folder)?;
                write_growth(&self.pile, &self.spec.folder)?;
                if cx.viewing() {
                    cx.show(self.pile.board.as_ref().as_any());
                }
                Ok(self.pile.finished())
            }
            Err(_) if cancel.load(Ordering::SeqCst) && self.pile.in_step.is_some() => {
                // Stopped part way: the work so far is kept, and the next step carries it on.
                if save_to_dir(&self.pile, &self.spec.folder).is_err() {
                    self.stale = true;
                }
                Ok(false)
            }
            Err(_) if cancel.load(Ordering::SeqCst) => {
                self.stale = true;
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Export the finished pile (`pile.png`, `preview.png`, `pile.json`), in the colours of the
    /// folder's `look.json` (else the spec's).
    pub fn finish(&mut self) -> Result<Vec<PathBuf>, String> {
        let spec = &self.spec;
        let (palette, reverse) = jetcarp_core::pile::export::read_look_file(&spec.folder)
            .filter(|(p, _)| jetcarp_core::render::builtin_palette(p).is_ok())
            .unwrap_or_else(|| (spec.palette.clone(), spec.reverse));
        let opts = RunOptions {
            export: ExportOptions {
                px_per_cell: spec.px_per_cell,
                margin: spec.margin,
                palette,
                reverse,
                preview_side: spec.preview_side,
                png: spec.png,
            },
            save_steps: false,
            extra: {
                let mut x = match &spec.extra {
                    serde_json::Value::Object(m) => m.clone(),
                    _ => serde_json::Map::new(),
                };
                x.insert("job".into(), serde_json::json!(self.job));
                serde_json::Value::Object(x)
            },
        };
        let mut quiet = Ctl::default();
        let s =
            jetcarp_core::pile::run_to_dir(&mut self.pile, &spec.folder, None, &mut quiet, &opts)?;
        let mut p = self.progress("done");
        p.png = s.png.as_ref().map(|x| x.display().to_string());
        p.preview = s.preview.as_ref().map(|x| x.display().to_string());
        self.done = Some(p);
        Ok(s.png.into_iter().chain(s.preview).collect())
    }

    /// The progress with this status (once exported: the final one).
    pub fn progress(&self, status: &str) -> PileProgress {
        match &self.done {
            Some(p) => p.clone(),
            None => {
                let mut p = progress_of(self.job, &self.pile, &self.spec, status);
                if self.backend.is_some() {
                    p.backend = self.backend.clone();
                }
                p
            }
        }
    }
}

/// The pile as its folder has it: the latest step file, or a fresh pile.
fn load(spec: &PileJobSpec) -> Result<Pile, String> {
    match latest_step_file(&spec.folder) {
        Some(f) => file::load(&f),
        None => Pile::new(spec.pile.clone()),
    }
}
