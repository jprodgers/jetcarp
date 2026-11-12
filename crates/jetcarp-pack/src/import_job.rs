//! Pack imports as catalog jobs (`jobs.kind` `pack_import`): [`crate::import::import`] run by the
//! job runner, so a long import (the first-run setup's starter pack, or a large pack chosen
//! then) shows in the dashboard and in `jetcarp jobs`, and can be resumed.
//!
//! A job imports one version of one pack: a store's feed (its newest version, or the manifest
//! the spec names) or an export. It runs as one step, which cannot be cut smaller: a stop asked
//! for during the import is seen when it ends. An import changes nothing when it is run again
//! (records merge by id, sightings take the larger count), so a job cut off part way, by a crash
//! or a quit, is simply run again from the start when it is resumed. Its progress names the
//! stage the import is in while it runs, and once it is done holds the import's report, as
//! `pack.import` gives it.
//!
//! The kind lives here, above `jetcarp-search`, which runs it: [`install`](crate::install)
//! registers it with the runner ([`jetcarp_search::jobs::register`]).

use crate::import::{self, Imported, VerifyMode};
use jetcarp_search::dashboard::JobSummary;
use jetcarp_search::jobs::{RunCx, Sink, Step, Strategy, StrategyKind, status_name};
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// The `jobs.kind` of a pack import.
pub const KIND_NAME: &str = "pack_import";

pub struct PackImportKind;
/// The kind, for [`jetcarp_search::jobs::register`].
pub static KIND: PackImportKind = PackImportKind;

/// What a pack import job imports, and how.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PackImportSpec {
    /// The store's or export's folder (absolute).
    pub path: String,
    /// The feed, when the store has several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<String>,
    /// A version other than the feed's newest (`sha256:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    /// What to call it (the pack's title), for the dashboard.
    #[serde(default)]
    pub title: String,
    /// Import a version its publisher did not sign.
    #[serde(default)]
    pub allow_unsigned: bool,
    /// `sample` (default), `full` or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
    /// How many new patterns `sample` verifies again (default 3000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<u64>,
    /// Most seconds verification may take (default 600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_seconds: Option<f64>,
    /// Kept by the job runner (a budget given when it was made).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Value>,
}

impl PackImportSpec {
    /// The import's options.
    pub fn options(&self) -> Result<import::Options, String> {
        let verify = match self.verify.as_deref().unwrap_or("sample") {
            "sample" => VerifyMode::Sample(self.sample.unwrap_or(3000)),
            "full" => VerifyMode::Full,
            "none" => VerifyMode::None,
            x => return Err(format!("verify '{x}': sample, full or none")),
        };
        Ok(import::Options {
            path: PathBuf::from(&self.path),
            feed: self.feed.clone(),
            manifest: self.manifest.clone(),
            allow_unsigned: self.allow_unsigned,
            strict: false,
            verify,
            seed: None,
            verify_seconds: self.verify_seconds.unwrap_or(600.0),
            dry_run: false,
        })
    }

    /// The pack in a few words: its title, else its feed, else its folder's name.
    pub fn name(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        if let Some(f) = &self.feed {
            return f.clone();
        }
        PathBuf::from(&self.path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }
}

/// A pack import's progress (`jobs.progress`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PackImportProgress {
    pub job: i64,
    pub status: String,
    /// The pack, in a few words ([`PackImportSpec::name`]).
    pub name: String,
    /// What the import is doing (its own log lines), while it runs.
    pub stage: String,
    /// Seconds the import took.
    pub elapsed_s: f64,
    /// The import's report once it is done (what `pack.import` gives).
    pub report: Option<Value>,
    pub error: Option<String>,
    pub line: String,
}

/// The progress as one line.
pub fn line(p: &PackImportProgress) -> String {
    let name = &p.name;
    if let Some(r) = &p.report {
        let n = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0);
        let q = n("quickstart");
        return format!(
            "pack import {}: {name} imported: {} patterns new, {} merged, {q} Quick Start {}, {} ant runs",
            p.job,
            n("patterns_added"),
            n("patterns_merged"),
            if q == 1 { "entry" } else { "entries" },
            n("ants")
        );
    }
    match p.status.as_str() {
        "running" if !p.stage.is_empty() => {
            format!("pack import {}: {name}: {}", p.job, p.stage)
        }
        "failed" => format!(
            "pack import {}: {name} failed: {}",
            p.job,
            p.error.as_deref().unwrap_or("")
        ),
        s => format!("pack import {}: {name}, {s}", p.job),
    }
}

impl StrategyKind for PackImportKind {
    fn kind(&self) -> &'static str {
        KIND_NAME
    }
    fn label(&self) -> &'static str {
        "Pack import"
    }
    /// Importing again gives the same library.
    fn deterministic(&self) -> bool {
        true
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let s: PackImportSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("pack_import spec: {e}"))?;
        if !PathBuf::from(&s.path).is_absolute() {
            return Err(format!("pack_import: {} is not an absolute path", s.path));
        }
        s.options()?;
        let v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
        let job = lib.create_job(KIND_NAME, &v)?;
        let mut p = PackImportProgress {
            job,
            status: "paused".into(),
            name: s.name(),
            ..Default::default()
        };
        p.line = line(&p);
        let pv = serde_json::to_value(&p).map_err(|e| e.to_string())?;
        lib.update_job(job, Some(JobStatus::Paused), Some(&pv), None)?;
        Ok(job)
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        let spec: PackImportSpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| format!("job {} spec: {e}", row.id))?;
        Ok(Box::new(PackImportRun::new(row.id, spec)))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec: PackImportSpec = serde_json::from_value(row.spec.clone()).unwrap_or_default();
        let p: PackImportProgress =
            serde_json::from_value(row.progress.clone()).unwrap_or_default();
        let mut s = JobSummary {
            id: row.id,
            kind: row.kind.clone(),
            kind_label: self.label().into(),
            status: status_name(row.status).into(),
            created: row.created.clone(),
            updated: row.updated.clone(),
            resumable: true,
            deterministic: true,
            title: spec.name(),
            unit: "packs".into(),
            done: u64::from(row.status == JobStatus::Done),
            total: Some(1),
            elapsed_s: p.elapsed_s,
            error: p.error.clone(),
            ..Default::default()
        };
        if let Some(r) = &p.report {
            for (k, label) in [
                ("patterns_added", "patterns new"),
                ("patterns_merged", "patterns merged"),
                ("quickstart", "Quick Start entries"),
                ("ants", "ant runs"),
            ] {
                let n = r.get(k).and_then(Value::as_u64).unwrap_or(0);
                s.counters
                    .push((label.into(), jetcarp_search::dashboard::digits(n)));
            }
        } else if !p.stage.is_empty() {
            s.counters.push(("stage".into(), p.stage.clone()));
        }
        s
    }
    fn line(&self, progress: &Value) -> String {
        let mut p: PackImportProgress =
            serde_json::from_value(progress.clone()).unwrap_or_default();
        if p.status == "failed" || p.line.is_empty() {
            p.line = line(&p);
        }
        p.line
    }
}

/// One run of a pack import.
pub struct PackImportRun {
    job: i64,
    spec: PackImportSpec,
    stage: String,
    elapsed_s: f64,
    report: Option<Imported>,
}

impl PackImportRun {
    pub fn new(job: i64, spec: PackImportSpec) -> PackImportRun {
        PackImportRun {
            job,
            spec,
            stage: String::new(),
            elapsed_s: 0.0,
            report: None,
        }
    }

    /// The report, once the import is done.
    pub fn report(&self) -> Option<&Imported> {
        self.report.as_ref()
    }

    fn progress_with(&self, status: &str, stage: &str) -> PackImportProgress {
        let mut p = PackImportProgress {
            job: self.job,
            status: status.into(),
            name: self.spec.name(),
            stage: stage.into(),
            elapsed_s: self.elapsed_s,
            report: self
                .report
                .as_ref()
                .and_then(|r| serde_json::to_value(r).ok()),
            error: None,
            line: String::new(),
        };
        p.line = line(&p);
        p
    }
}

impl Strategy for PackImportRun {
    fn step(&mut self, cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let opts = self.spec.options()?;
        if !opts.path.exists() {
            return Err(format!("{} does not exist", opts.path.display()));
        }
        let t0 = Instant::now();
        // The import's own lines, as its stage: shown at once, saved at most once a second.
        let last: Mutex<Option<Instant>> = Mutex::new(None);
        let log = |l: &str| {
            let mut g = last.lock().unwrap_or_else(|e| e.into_inner());
            if g.is_some_and(|t| t.elapsed().as_secs_f64() < 1.0) {
                return;
            }
            *g = Some(Instant::now());
            let p = self.progress_with("running", l);
            cx.publish(serde_json::to_value(&p).unwrap_or_default());
        };
        let r = import::import(cx.lib, &opts, &log);
        self.elapsed_s = t0.elapsed().as_secs_f64();
        let r = r?;
        self.stage = String::new();
        self.report = Some(r);
        Ok(Step::Done)
    }

    fn progress(&self, status: JobStatus) -> Value {
        serde_json::to_value(self.progress_with(status_name(status), &self.stage))
            .unwrap_or_default()
    }
}
