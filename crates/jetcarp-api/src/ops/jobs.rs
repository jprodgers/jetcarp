//! `jobs.*`: the catalog's jobs, whatever started them (any front end, any process): list them
//! (`jobs.list`), follow one (`jobs.status`: its live progress when it runs in this process, with
//! an optional wait for its end and its finds), stop one at its next checkpoint (`jobs.stop`),
//! carry one on (`jobs.resume`), delete one (`jobs.remove`), and write a Markdown report of what
//! one job or a labelled set of jobs searched and found (`jobs.report`).
//!
//! Every kind is the job runner's (`jetcarp_core::jobs`): what is kind-specific (its summary,
//! progress line, stop, finds) comes from the kind, so one status and one stop serve hunts, ant
//! searches, censuses, movers, piles, bounded searches, collisions, evolve searches, and explore
//! searches alike. A deterministic job resumes exactly (it ends as an uninterrupted run would);
//! an explore search continues from its saved archive, and the status says which.

use super::library::catalog_err;
use super::search::{FilesIn, How, SearchStarted, line_of, run_job};
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::ants::job::AntProgress;
use jetcarp_core::dashboard::{self, JobSummary, Target, digits, duration};
use jetcarp_core::hunt::job::HuntProgress;
use jetcarp_core::jobs::explore::ExploreProgress;
use jetcarp_core::jobs::{self, status_name};
use jetcarp_core::library::store::JobRow;
use jetcarp_core::library::{JobStatus, Library, PatternClass, PatternRecord};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// A kind's name in sentences: `hunt`, `ant search`, `collision`, ...
pub fn noun(kind: &str) -> &str {
    match kind {
        "search" => "soup search",
        "ants" => "ant search",
        "collide" => "collision",
        "evolve" => "evolve search",
        "explore" => "explore search",
        k => k,
    }
}

/// What resuming a job does, in words: `resumes it exactly` or (an explore search) `continues it
/// from its archive`.
pub fn resume_words(kind: &str) -> &'static str {
    if jobs::kind(kind).deterministic() {
        "resumes it exactly"
    } else {
        "continues it from its archive (not run for run: the search is not deterministic)"
    }
}

/// A job's summary with this progress (a live one, when the job runs here).
pub(crate) fn summary_of(row: &JobRow, progress: &Value) -> JobSummary {
    let mut r = row.clone();
    r.progress = progress.clone();
    if let Some(st) = progress
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(jobs::status_of)
    {
        r.status = st;
    }
    jobs::kind(&r.kind).summary(&r)
}

/// A job's progress as people read it, by kind (what `jetcarp search` and `jetcarp jobs status`
/// print).
pub fn progress_text(kind: &str, progress: &Value, line: &str) -> String {
    let mut s = String::new();
    match kind {
        "hunt" => {
            let p: HuntProgress = jobs_typed(progress);
            let _ = writeln!(s, "{}", p.line());
            let _ = writeln!(
                s,
                "  status {}, {:.0} s run, {} settled, {} died, {} exploded, {} unsettled{}",
                p.status,
                p.elapsed_s,
                p.settled,
                p.died,
                p.exploded,
                p.unsettled,
                p.gpu
                    .as_ref()
                    .map(|g| format!(", GPU {g}"))
                    .unwrap_or_default()
            );
            for g in &p.groups {
                let classes: Vec<String> =
                    g.by_class.iter().map(|(k, v)| format!("{k} {v}")).collect();
                let _ = writeln!(
                    s,
                    "  {} on {}: {} soups, {} objects ({}), {} distinct, {} new{}",
                    g.rule,
                    g.tiling,
                    g.soups,
                    g.objects(),
                    classes.join(", "),
                    g.distinct,
                    g.new_patterns,
                    g.error
                        .as_ref()
                        .map(|e| format!(" [error: {e}]"))
                        .unwrap_or_default()
                );
            }
            if !p.newest.is_empty() {
                let _ = writeln!(s, "  newest finds:");
                for f in p.newest.iter().take(10) {
                    let _ = writeln!(
                        s,
                        "    {}  {:<34} {:<10} {} soup {}{}",
                        f.id,
                        f.label,
                        f.rule,
                        f.tiling,
                        f.soup,
                        if f.new { "  (new to the catalog)" } else { "" }
                    );
                }
            }
        }
        "ants" => {
            let p: AntProgress = jobs_typed(progress);
            let _ = writeln!(s, "{}", p.line());
            let _ = writeln!(
                s,
                "  status {}, {:.0} s run, {} ant steps",
                p.status, p.elapsed_s, p.steps
            );
            for g in &p.groups {
                let outs: Vec<String> = g
                    .by_outcome
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect();
                let _ = writeln!(
                    s,
                    "  {}: {}/{} runs, {}{}{}{}",
                    g.tiling,
                    g.runs,
                    g.rules,
                    outs.join(", "),
                    g.latest_onset
                        .as_ref()
                        .map(|(r, o)| format!("; latest highway {r} from step {o}"))
                        .unwrap_or_default(),
                    g.longest_period
                        .as_ref()
                        .map(|(r, p)| format!("; longest period {r} p{p}"))
                        .unwrap_or_default(),
                    g.error
                        .as_ref()
                        .map(|e| format!(" [error: {e}]"))
                        .unwrap_or_default()
                );
            }
            if !p.newest.is_empty() {
                let _ = writeln!(s, "  newest:");
                for f in p.newest.iter().take(10) {
                    let _ = writeln!(
                        s,
                        "    {}  {:<22} {:<10} {}{}{}",
                        f.id,
                        f.rule,
                        f.tiling,
                        f.outcome.name(),
                        f.period.map(|x| format!(" p{x}")).unwrap_or_default(),
                        f.onset
                            .map(|x| format!(" from step {x}"))
                            .unwrap_or_default()
                    );
                }
            }
        }
        "explore" => {
            let p: ExploreProgress = jobs_typed(progress);
            let _ = writeln!(s, "{line}");
            let _ = writeln!(
                s,
                "  status {}, {} run over every session, {} confirmation runs, {} candidates tried, {} GB",
                p.status,
                duration(p.elapsed_s),
                digits(p.confirmed),
                digits(p.seen),
                format_args!("{:.2}", p.rss_gb)
            );
            if let Some(i) = &p.import {
                let _ = writeln!(
                    s,
                    "  import of {}: {}",
                    i.from,
                    if i.done {
                        "done".to_string()
                    } else {
                        format!(
                            "run {} of {}",
                            digits(i.last.max(0) as u64),
                            digits(i.max.max(0) as u64)
                        )
                    }
                );
            }
            if let Some(l) = &p.loaded {
                let _ = writeln!(
                    s,
                    "  loaded {} elites, {} identities, {} coverage rows in {:.2} s (no other runs read)",
                    digits(l.elites),
                    digits(l.seen),
                    digits(l.coverage_rows),
                    l.seconds
                );
            }
            if !p.families.is_empty() {
                let _ = writeln!(
                    s,
                    "  {:<20} {:>10} {:>6} {:>6} {:>7}",
                    "family", "runs", "mean", "best", "elites"
                );
                for f in &p.families {
                    let _ = writeln!(
                        s,
                        "  {:<20} {:>10} {:>6.2} {:>6.2} {:>7}",
                        f.family,
                        digits(f.runs),
                        f.mean,
                        f.best,
                        digits(f.elites)
                    );
                }
            }
            for (k, v) in [
                ("folder", Some(p.folder.clone())),
                ("note", p.note.clone()),
                ("GPU error", p.gpu_error.clone()),
                ("error", p.error.clone()),
            ] {
                if let Some(v) = v.filter(|v| !v.is_empty()) {
                    let _ = writeln!(s, "  {k}: {v}");
                }
            }
        }
        _ => {
            let _ = writeln!(s, "{line}");
            let text = |k: &str| progress.get(k).and_then(|v| v.as_str());
            for (k, what) in [
                ("record", "library"),
                ("png", "image"),
                ("preview", "preview"),
                ("folder", "folder"),
                ("error", "error"),
            ] {
                if let Some(v) = text(k).filter(|v| !v.is_empty()) {
                    let _ = writeln!(s, "  {what}: {v}");
                }
            }
            if let Some(c) = progress.get("champions").and_then(|v| v.as_array()) {
                for x in c {
                    let _ = writeln!(
                        s,
                        "  champion {:.2}  {}",
                        x["fitness"].as_f64().unwrap_or(0.0),
                        x["rule"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
    s
}

fn jobs_typed<T: serde::de::DeserializeOwned + Default>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// A job's progress: the live one when it runs in this process (or has just ended here), else
/// the saved one. Also whether it runs here.
pub(crate) fn live_progress(cx: &Ctx, lib: &Library, row: &JobRow) -> (Value, bool) {
    if let Some((finished, p)) = cx
        .registry()
        .with(lib, row.id, |h| (h.is_finished(), h.progress()))
        && (!finished || p.get("status").and_then(|v| v.as_str()) != Some("running"))
        && !p.is_null()
    {
        return (p, !finished);
    }
    (jobs::kind(&row.kind).saved_progress(lib, row), false)
}

// ---------------------------------------------------------------------------------------------
// jobs.list

/// Which jobs to list.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIn {
    /// Only jobs of this kind: hunt, search, ants, census, mover, pile, collide, evolve, explore (strategy aliases work too).
    #[serde(default)]
    pub kind: Option<String>,
    /// Only jobs with this status: running, paused, done, failed.
    #[serde(default)]
    pub status: Option<String>,
    /// Only jobs with this label.
    #[serde(default)]
    pub label: Option<String>,
    /// Only jobs whose label starts with this.
    #[serde(default)]
    pub prefix: Option<String>,
    /// At most this many (newest first; default all).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One job in a list.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobLine {
    pub job: i64,
    pub kind: String,
    pub kind_label: String,
    /// `running`, `paused`, `done`, or `failed`.
    pub status: String,
    pub label: Option<String>,
    /// What it searches (rules and tilings, ...), after its label.
    pub title: String,
    /// Its progress as one line.
    pub line: String,
    pub done: u64,
    pub total: Option<u64>,
    pub unit: String,
    /// Running in this process.
    pub here: bool,
    /// Running, but no progress or heartbeat for five minutes: resumable elsewhere.
    pub stale: bool,
    pub created: String,
    pub updated: String,
}

/// The jobs found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobList {
    /// The catalog file.
    pub catalog: String,
    /// Newest first.
    pub jobs: Vec<JobLine>,
}

impl Report for JobList {
    fn text(&self) -> String {
        let mut s = String::new();
        for j in &self.jobs {
            let _ = writeln!(
                s,
                "{:>5}  {:<8} {:<8} {}  {}{}",
                j.job,
                j.kind,
                if j.stale { "stale" } else { &j.status },
                j.created,
                j.line,
                if j.here { "  (here)" } else { "" }
            );
        }
        let _ = writeln!(
            s,
            "{} job(s); {} running",
            self.jobs.len(),
            self.jobs
                .iter()
                .filter(|j| j.status == "running" && !j.stale)
                .count()
        );
        s
    }
}

fn kind_filter(k: &str) -> String {
    super::search::strategy_kind(k)
        .map(String::from)
        .unwrap_or_else(|_| k.to_string())
}

/// The jobs matching `a`, newest first.
pub(crate) fn list(cx: &Ctx, a: &ListIn) -> Result<JobList, ApiError> {
    let lib = cx.library();
    if let Some(s) = &a.status
        && jobs::status_of(s).is_none()
    {
        return Err(ApiError::invalid(format!(
            "`status` is running, paused, done, or failed, not {s}"
        )));
    }
    let kind = a.kind.as_deref().map(kind_filter);
    let rows = lib.jobs().map_err(catalog_err)?;
    let now = dashboard::now_seconds();
    let mut out = Vec::new();
    for r in rows.iter().rev() {
        if r.id == 0
            || r.kind == "manual"
            || kind.as_ref().is_some_and(|k| &r.kind != k)
            || a.status
                .as_ref()
                .is_some_and(|s| status_name(r.status) != s)
            || a.label
                .as_ref()
                .is_some_and(|l| r.label() != Some(l.as_str()))
            || a.prefix
                .as_ref()
                .is_some_and(|p| !r.label().is_some_and(|l| l.starts_with(p.as_str())))
        {
            continue;
        }
        if a.limit.is_some_and(|n| out.len() >= n) {
            break;
        }
        out.push(line_for(cx, &lib, r, now));
    }
    Ok(JobList {
        catalog: lib.path().display().to_string(),
        jobs: out,
    })
}

fn line_for(cx: &Ctx, lib: &Library, r: &JobRow, now: i64) -> JobLine {
    let k = jobs::kind(&r.kind);
    let (progress, here) = live_progress(cx, lib, r);
    let mut row = r.clone();
    row.progress = progress.clone();
    let mut s = dashboard::summaries(std::slice::from_ref(&row), now)
        .pop()
        .unwrap_or_else(|| summary_of(r, &progress));
    if here && let Some(st) = progress.get("status").and_then(|v| v.as_str()) {
        s.status = st.into();
        s.stale = false;
    }
    JobLine {
        job: r.id,
        kind: r.kind.clone(),
        kind_label: k.label().into(),
        status: s.status.clone(),
        label: r.label().map(String::from),
        title: s.title.clone(),
        line: line_of(k, r, &progress),
        done: s.done,
        total: s.total,
        unit: s.unit.clone(),
        here,
        stale: s.stale,
        created: r.created.clone(),
        updated: r.updated.clone(),
    }
}

/// `jobs.list`.
pub struct JobsList;
impl Op for JobsList {
    const NAME: &'static str = "jobs.list";
    const SUMMARY: &'static str =
        "List the catalog's jobs (every kind, from any front end or process), newest first.";
    const READ_ONLY: bool = true;
    type In = ListIn;
    type Out = JobList;
    fn run(cx: &Ctx, a: ListIn, _: &dyn Progress) -> Result<JobList, ApiError> {
        list(cx, &a)
    }
}

// ---------------------------------------------------------------------------------------------
// jobs.status

/// One job (live, with an optional wait and its finds), or the list.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusIn {
    /// The job (default: list the jobs, filtered as jobs.list).
    #[serde(default)]
    pub job: Option<i64>,
    /// Wait up to this many seconds for a running job to stop running before answering (default 0: answer now).
    #[serde(default)]
    pub wait_s: Option<f64>,
    /// Include this many of its newest and rarest finds (hunts, bounded searches, collisions).
    #[serde(default)]
    pub finds: Option<usize>,
    /// Listing: only jobs of this kind.
    #[serde(default)]
    pub kind: Option<String>,
    /// Listing: only jobs with this status.
    #[serde(default)]
    pub status: Option<String>,
    /// Listing: only jobs with this label.
    #[serde(default)]
    pub label: Option<String>,
    /// Listing: only jobs whose label starts with this.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Listing: at most this many.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A find to look at.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FindOut {
    /// The pattern's id.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub rule: String,
    pub rating: Option<u8>,
    pub rejected: bool,
}

/// A job's newest and other (rarest) finds.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FindsOut {
    pub newest: Vec<FindOut>,
    pub other: Vec<FindOut>,
    /// What `other` holds (`Rarest`).
    pub other_title: String,
}

/// One job.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobView {
    pub job: i64,
    pub kind: String,
    pub kind_label: String,
    /// The catalog file.
    pub catalog: String,
    pub label: Option<String>,
    /// `running`, `paused`, `done`, or `failed` (live when it runs here).
    pub status: String,
    /// Running in this process.
    pub here: bool,
    /// Running, but no progress or heartbeat for five minutes: resumable elsewhere.
    pub stale: bool,
    /// This program can carry it on.
    pub resumable: bool,
    /// Stopped and resumed, it ends exactly as an uninterrupted run; otherwise (an explore search) a resumed run continues from its saved state.
    pub deterministic: bool,
    /// What it searches.
    pub title: String,
    /// Units done and in all (none: until stopped), and what a unit is.
    pub done: u64,
    pub total: Option<u64>,
    pub unit: String,
    /// Units per second in the last session, and seconds run over every session.
    pub per_s: f64,
    pub elapsed_s: f64,
    /// Seconds left, when it runs and has an end.
    pub eta_s: Option<f64>,
    /// Counters, in order (`settled`, `1,204`).
    pub counters: Vec<(String, String)>,
    pub created: String,
    pub updated: String,
    pub finished: Option<String>,
    pub error: Option<String>,
    /// The job's spec (its kind's own JSON).
    #[schemars(with = "Value")]
    pub spec: Value,
    /// Its progress (its kind's progress JSON; live when it runs here).
    #[schemars(with = "Value")]
    pub progress: Value,
    /// The progress as one line.
    pub line: String,
    pub finds: Option<FindsOut>,
    /// Seconds waited (with `wait_s`).
    pub waited_s: Option<f64>,
}

impl Report for JobView {
    fn text(&self) -> String {
        let mut s = progress_text(&self.kind, &self.progress, &self.line);
        if self.resumable
            && !self.here
            && (matches!(self.status.as_str(), "paused" | "failed") || self.stale)
        {
            let _ = writeln!(
                s,
                "  `jetcarp jobs resume {}` {}",
                self.job,
                resume_words(&self.kind)
            );
        }
        if !matches!(self.kind.as_str(), "hunt" | "ants" | "explore") {
            let _ = writeln!(
                s,
                "  {} {}: {}, {}",
                self.kind_label,
                self.job,
                if self.stale { "stale" } else { &self.status },
                self.title
            );
            for (k, v) in &self.counters {
                let _ = writeln!(s, "  {k}: {v}");
            }
        }
        if let Some(f) = &self.finds {
            for (what, list) in [("newest", &f.newest), (f.other_title.as_str(), &f.other)] {
                if list.is_empty() {
                    continue;
                }
                let _ = writeln!(s, "  {}:", what.to_lowercase());
                for x in list {
                    let _ = writeln!(s, "    {}  {}  {}", x.id, x.title, x.subtitle);
                }
            }
        }
        s
    }
}

/// The status: one job, or the list.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StatusOut {
    One(Box<JobView>),
    List(JobList),
}

impl Report for StatusOut {
    fn text(&self) -> String {
        match self {
            StatusOut::One(v) => v.text(),
            StatusOut::List(l) => l.text(),
        }
    }
}

/// One job's view now.
pub(crate) fn view(
    cx: &Ctx,
    lib: &Library,
    job: i64,
    finds: Option<usize>,
) -> Result<JobView, ApiError> {
    let row = lib.job(job).map_err(catalog_err)?;
    let k = jobs::kind(&row.kind);
    let (progress, here) = live_progress(cx, lib, &row);
    let mut r = row.clone();
    r.progress = progress.clone();
    let mut s = dashboard::summaries(std::slice::from_ref(&r), dashboard::now_seconds())
        .pop()
        .unwrap_or_else(|| summary_of(&row, &progress));
    let status = if here {
        progress
            .get("status")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| "running".into())
    } else {
        status_name(row.status).to_string()
    };
    if here {
        s.stale = false;
    }
    let finds = match finds {
        Some(n) if n > 0 => {
            let f = k.finds(lib, &row, &[], n).map_err(catalog_err)?;
            let conv = |v: Vec<dashboard::FindItem>| -> Vec<FindOut> {
                v.into_iter()
                    .map(|x| FindOut {
                        id: match x.target {
                            Target::Pattern(id) => id,
                        },
                        title: x.title,
                        subtitle: x.subtitle,
                        rule: x.rule,
                        rating: x.rating,
                        rejected: x.rejected,
                    })
                    .collect()
            };
            Some(FindsOut {
                newest: conv(f.newest),
                other: conv(f.other),
                other_title: f.other_title,
            })
        }
        _ => None,
    };
    Ok(JobView {
        job,
        kind: row.kind.clone(),
        kind_label: k.label().into(),
        catalog: lib.path().display().to_string(),
        label: row.label().map(String::from),
        status,
        here,
        stale: s.stale,
        resumable: k.runnable(&row),
        deterministic: k.deterministic(),
        title: s.title,
        done: s.done,
        total: s.total,
        unit: s.unit,
        per_s: s.per_s,
        elapsed_s: s.elapsed_s,
        eta_s: s.eta_s,
        counters: s.counters,
        created: row.created.clone(),
        updated: row.updated.clone(),
        finished: row.finished.clone(),
        error: s.error,
        spec: row.spec.clone(),
        line: line_of(k, &row, &progress),
        progress,
        finds,
        waited_s: None,
    })
}

/// `jobs.status`.
pub struct JobsStatus;
impl Op for JobsStatus {
    const NAME: &'static str = "jobs.status";
    const SUMMARY: &'static str = "One job's progress (live when it runs here; optionally waiting for its end, with its finds), or the list of jobs.";
    const READ_ONLY: bool = true;
    type In = StatusIn;
    type Out = StatusOut;
    fn run(cx: &Ctx, a: StatusIn, p: &dyn Progress) -> Result<StatusOut, ApiError> {
        let Some(job) = a.job else {
            let l = ListIn {
                kind: a.kind,
                status: a.status,
                label: a.label,
                prefix: a.prefix,
                limit: a.limit,
            };
            return list(cx, &l).map(StatusOut::List);
        };
        let lib = cx.library();
        let t0 = Instant::now();
        let wait = a.wait_s.unwrap_or(0.0).max(0.0);
        loop {
            let mut v = view(cx, &lib, job, a.finds)?;
            let running = v.status == "running" && !v.stale;
            if wait <= 0.0 || !running || p.cancelled() || t0.elapsed().as_secs_f64() >= wait {
                if wait > 0.0 {
                    v.waited_s = Some(t0.elapsed().as_secs_f64());
                }
                return Ok(StatusOut::One(Box::new(v)));
            }
            p.report(v.done, v.total, &v.line);
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// jobs.stop

/// The job to stop.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobIn {
    /// The job id.
    pub job: i64,
}

/// A stop asked for.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct StopOut {
    pub job: i64,
    pub kind: String,
    /// It was running (here or elsewhere) and stops at its next checkpoint, resumable.
    pub stopping: bool,
    /// It runs in this process.
    pub here: bool,
    /// Its status when asked.
    pub status: String,
}

impl Report for StopOut {
    fn text(&self) -> String {
        if self.stopping {
            format!(
                "asked {} {} to stop at its next checkpoint\n",
                noun(&self.kind),
                self.job
            )
        } else {
            format!(
                "{} {} is not running ({})\n",
                noun(&self.kind),
                self.job,
                self.status
            )
        }
    }
}

/// `jobs.stop`.
pub struct JobsStop;
impl Op for JobsStop {
    const NAME: &'static str = "jobs.stop";
    const SUMMARY: &'static str =
        "Stop a job at its next checkpoint, in whatever process runs it; it stays resumable.";
    const READ_ONLY: bool = false;
    type In = JobIn;
    type Out = StopOut;
    fn run(cx: &Ctx, a: JobIn, _: &dyn Progress) -> Result<StopOut, ApiError> {
        let lib = cx.library();
        let row = lib.job(a.job).map_err(catalog_err)?;
        let here = cx.registry().is_running(&lib, a.job) && cx.registry().stop(&lib, a.job);
        if !here {
            // Another process: it sees `paused` at its next checkpoint.
            jobs::kind(&row.kind)
                .request_stop(&lib, a.job)
                .map_err(catalog_err)?;
        }
        Ok(StopOut {
            job: a.job,
            kind: row.kind.clone(),
            stopping: here || row.status == JobStatus::Running,
            here,
            status: status_name(row.status).into(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// jobs.resume

/// The job to carry on, and how.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeIn {
    /// The job id.
    pub job: i64,
    /// This run's budget; the job stays resumable past it.
    #[serde(default)]
    pub budget: Option<super::search::BudgetIn>,
    /// Worker threads (default 0: all cores).
    #[serde(default)]
    pub threads: Option<usize>,
    /// Hunts: the GPU soup batches; piles: GPU toppling; explore: the GPU batch stepper, when there is one.
    #[serde(default)]
    pub gpu: bool,
    /// Run on a thread of this process and answer at once.
    #[serde(default)]
    pub background: bool,
    /// Run in a process of its own and answer at once.
    #[serde(default)]
    pub detach: bool,
    /// Files to write when the job ends in this call.
    #[serde(default)]
    pub files: FilesIn,
}

/// `jobs.resume`.
pub struct JobsResume;
impl Op for JobsResume {
    const NAME: &'static str = "jobs.resume";
    const SUMMARY: &'static str = "Carry on a stopped, paused, or cut-off job from its last checkpoint: here, in the background, or in a process of its own.";
    const READ_ONLY: bool = false;
    type In = ResumeIn;
    type Out = SearchStarted;
    fn run(cx: &Ctx, a: ResumeIn, p: &dyn Progress) -> Result<SearchStarted, ApiError> {
        let t0 = Instant::now();
        let lib = cx.library();
        let how = How {
            threads: a.threads.unwrap_or(0),
            gpu: a.gpu,
            budget: a
                .budget
                .map(super::search::BudgetIn::budget)
                .unwrap_or_default(),
            background: a.background,
            detach: a.detach,
            files: a.files,
        };
        let mut out = run_job(cx, &lib, a.job, &how, p)?;
        out.seconds = t0.elapsed().as_secs_f64();
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// jobs.remove

/// The job to delete.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveIn {
    /// The job id.
    pub job: i64,
    /// Delete its finds too: the ant runs it stored, the patterns no other job has seen that nobody has curated, and its working folder (an explore search's runs and archive).
    #[serde(default)]
    pub delete_results: bool,
}

/// A job deleted.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Removed {
    pub job: i64,
    pub kind: String,
    /// Its sightings, deleted with it.
    pub sightings: u64,
    /// With `delete_results`: the patterns and ant runs deleted.
    pub patterns: Vec<String>,
    pub ants: Vec<String>,
    /// Its working folder (an explore search's), deleted with `delete_results`, else left.
    pub folder: Option<String>,
    pub folder_deleted: bool,
}

impl Report for Removed {
    fn text(&self) -> String {
        let mut s = format!(
            "removed {} {} ({} sightings",
            noun(&self.kind),
            self.job,
            self.sightings
        );
        if !self.patterns.is_empty() || !self.ants.is_empty() {
            let _ = write!(
                s,
                ", {} pattern(s), {} ant run(s)",
                self.patterns.len(),
                self.ants.len()
            );
        }
        s.push_str(")\n");
        if let Some(f) = &self.folder {
            if self.folder_deleted {
                let _ = writeln!(s, "deleted its folder {f}");
            } else {
                let _ = writeln!(s, "left its folder {f} (delete_results deletes it)");
            }
        }
        s
    }
}

/// `jobs.remove`.
pub struct JobsRemove;
impl Op for JobsRemove {
    const NAME: &'static str = "jobs.remove";
    const SUMMARY: &'static str = "Delete a job that is not running, with its sightings, and optionally the finds only it made.";
    const READ_ONLY: bool = false;
    type In = RemoveIn;
    type Out = Removed;
    fn run(cx: &Ctx, a: RemoveIn, _: &dyn Progress) -> Result<Removed, ApiError> {
        let lib = cx.library();
        let row = lib.job(a.job).map_err(catalog_err)?;
        let s = dashboard::summaries(std::slice::from_ref(&row), dashboard::now_seconds());
        let stale = s.first().is_some_and(|s| s.stale);
        if cx.registry().is_running(&lib, a.job) || (row.status == JobStatus::Running && !stale) {
            return Err(ApiError::conflict(format!(
                "{} {} is running; stop it first (jobs.stop)",
                noun(&row.kind),
                a.job
            )));
        }
        let _ = cx.registry().wait(&lib, a.job);
        let folder = jobs::kind(&row.kind)
            .work_dir(&lib, &row)
            .filter(|d| d.exists());
        let r = lib
            .remove_job(a.job, a.delete_results)
            .map_err(catalog_err)?;
        let _ = std::fs::remove_file(jobs::lock_path(&lib, a.job));
        let mut folder_deleted = false;
        if let Some(d) = folder.as_ref().filter(|_| a.delete_results) {
            std::fs::remove_dir_all(d)
                .map_err(|e| ApiError::io(format!("deleting {}: {e}", d.display())))?;
            folder_deleted = true;
        }
        Ok(Removed {
            job: a.job,
            kind: row.kind,
            sightings: r.sightings,
            patterns: r.patterns,
            ants: r.ants,
            folder: folder.map(|d| d.display().to_string()),
            folder_deleted,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// jobs.report

/// Which jobs to report on.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportIn {
    /// One job.
    #[serde(default)]
    pub job: Option<i64>,
    /// Instead: every job whose label starts with this (a script's labelled jobs).
    #[serde(default)]
    pub prefix: Option<String>,
    /// Rules or finds listed per row (default 5).
    #[serde(default)]
    pub top: Option<usize>,
}

/// A report in Markdown.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct JobReport {
    /// The jobs reported on.
    pub jobs: Vec<i64>,
    /// The report.
    pub markdown: String,
}

impl Report for JobReport {
    fn text(&self) -> String {
        self.markdown.clone()
    }
}

/// `jobs.report`.
pub struct JobsReport;
impl Op for JobsReport {
    const NAME: &'static str = "jobs.report";
    const SUMMARY: &'static str = "A Markdown report of what a job (or every job with a label prefix) searched, for how long, and what it found.";
    const READ_ONLY: bool = true;
    type In = ReportIn;
    type Out = JobReport;
    fn run(cx: &Ctx, a: ReportIn, _: &dyn Progress) -> Result<JobReport, ApiError> {
        let lib = cx.library();
        let rows: Vec<JobRow> = match (&a.job, &a.prefix) {
            (Some(j), _) => vec![lib.job(*j).map_err(catalog_err)?],
            (None, Some(p)) => lib
                .jobs()
                .map_err(catalog_err)?
                .into_iter()
                .filter(|j| j.id != 0 && j.label().is_some_and(|l| l.starts_with(p.as_str())))
                .collect(),
            (None, None) => return Err(ApiError::invalid("give a `job` or a label `prefix`")),
        };
        if rows.is_empty() {
            return Err(ApiError::not_found(format!(
                "no job with a label starting {:?}",
                a.prefix.as_deref().unwrap_or("")
            )));
        }
        let markdown = report(cx, &lib, &rows, a.prefix.as_deref(), a.top.unwrap_or(5))
            .map_err(catalog_err)?;
        Ok(JobReport {
            jobs: rows.iter().map(|r| r.id).collect(),
            markdown,
        })
    }
}

/// A table cell: no pipes or line breaks.
fn cell(s: &str) -> String {
    s.replace('|', "/").replace('\n', " ")
}

/// At most `n` items joined, with how many more.
fn cap(v: Vec<String>, n: usize) -> String {
    let k = v.len();
    let mut s = v.into_iter().take(n).collect::<Vec<_>>().join("; ");
    if k > n {
        let _ = write!(s, " (+{})", k - n);
    }
    s
}

/// The report (the generic parts of the old pipeline's science report: the jobs, then per kind
/// what they found).
fn report(
    cx: &Ctx,
    lib: &Library,
    rows: &[JobRow],
    prefix: Option<&str>,
    top: usize,
) -> Result<String, String> {
    let now = dashboard::now_seconds();
    let sums = dashboard::summaries(rows, now);
    let by_id: HashMap<i64, &JobSummary> = sums.iter().map(|s| (s.id, s)).collect();
    let total: f64 = sums.iter().map(|s| s.elapsed_s).sum();
    let mut o = String::new();
    match (rows, prefix) {
        ([r], None) => {
            let k = jobs::kind(&r.kind);
            let title = by_id
                .get(&r.id)
                .map(|s| s.title.clone())
                .unwrap_or_default();
            let _ = writeln!(o, "# {} {}: {}\n", k.label(), r.id, title);
        }
        (_, Some(p)) => {
            let _ = writeln!(o, "# Jobs labelled `{p}…`\n");
        }
        _ => {
            let _ = writeln!(o, "# Jobs\n");
        }
    }
    let _ = writeln!(
        o,
        "Catalog `{}`; {} job(s), {} of searching in all; JETCARP {}.\n",
        lib.path().display(),
        rows.len(),
        duration(total),
        jetcarp_core::VERSION
    );
    let _ = writeln!(
        o,
        "## Jobs\n\n| Job | Kind | Label | Status | Done | Time | Started | Updated |\n| --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for r in rows {
        let s = by_id.get(&r.id);
        let done = s
            .map(|s| match s.total {
                Some(t) => format!("{}/{} {}", digits(s.done), digits(t), s.unit),
                None => format!("{} {}", digits(s.done), s.unit),
            })
            .unwrap_or_default();
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            r.id,
            r.kind,
            cell(r.label().unwrap_or("")),
            s.map(|s| if s.stale { "stale" } else { s.status.as_str() })
                .unwrap_or(status_name(r.status)),
            cell(&done),
            duration(s.map(|s| s.elapsed_s).unwrap_or(0.0)),
            r.created,
            r.updated
        );
    }
    // What each job searched.
    let _ = writeln!(o, "\n## What was searched\n");
    for r in rows {
        let s = by_id.get(&r.id);
        let _ = writeln!(
            o,
            "- **{} {}**: {}",
            jobs::kind(&r.kind).label(),
            r.id,
            cell(&s.map(|s| s.title.clone()).unwrap_or_default())
        );
        if let Some(s) = s {
            let c: Vec<String> = s.counters.iter().map(|(k, v)| format!("{k} {v}")).collect();
            if !c.is_empty() {
                let _ = writeln!(o, "  ({})", c.join(", "));
            }
            if let Some(e) = &s.error {
                let _ = writeln!(o, "  Error: {}", cell(e));
            }
        }
    }
    fn by_kind<'a>(rows: &'a [JobRow], k: &'a str) -> impl Iterator<Item = &'a JobRow> {
        rows.iter().filter(move |r| r.kind == k)
    }
    let of = |k: &'static str| by_kind(rows, k);
    // Censuses: fate fractions per patch, and the most oscillating rules.
    if of("census").next().is_some() {
        let _ = writeln!(
            o,
            "\n## Censuses: rules by fate\n\n| Job | Tiling | Rules | Died | Frozen | Oscillating | Active | Exploded | Most oscillating rules |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
        );
        for j in of("census") {
            let rows = lib.census_rows(j.id)?;
            let n = rows.len().max(1) as f64;
            let mean = |k: &str| rows.iter().filter_map(|r| r[k].as_f64()).sum::<f64>() / n;
            let mut osc: Vec<(f64, String, Option<f64>)> = rows
                .iter()
                .filter(|r| r["exploded"].as_f64() == Some(0.0))
                .map(|r| {
                    (
                        r["oscillating"].as_f64().unwrap_or(0.0),
                        r["notation"].as_str().unwrap_or("").to_string(),
                        r["median_period"].as_f64(),
                    )
                })
                .collect();
            osc.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let tiling = j.spec["tiling"]["family"].as_str().unwrap_or("?");
            let _ = writeln!(
                o,
                "| {} | {} | {} | {:.0}% | {:.0}% | {:.0}% | {:.0}% | {:.0}% | {} |",
                j.id,
                tiling,
                rows.len(),
                100.0 * mean("died"),
                100.0 * mean("frozen"),
                100.0 * mean("oscillating"),
                100.0 * mean("active"),
                100.0 * mean("exploded"),
                osc.iter()
                    .take(top)
                    .map(|x| format!(
                        "{} ({:.0}%{})",
                        x.1,
                        100.0 * x.0,
                        x.2.map(|p| format!(", p{p}")).unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }
    // Hunts: per hunted rule, what was found.
    let mut recs: HashMap<String, Option<PatternRecord>> = HashMap::new();
    let mut find = |id: &str| -> Option<PatternRecord> {
        recs.entry(id.to_string())
            .or_insert_with(|| lib.get(id).ok())
            .clone()
    };
    if of("hunt").next().is_some() {
        let _ = writeln!(
            o,
            "\n## Hunts: per hunted rule\n\n| Job | Tiling | Rule | Soups | Distinct (unstored) | Spaceships (speed, cells, period, copies) | Guns | Puffers | Longest stored oscillators |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
        );
        for j in of("hunt") {
            let (p, _) = live_progress(cx, lib, j);
            let hp: HuntProgress = jobs_typed(&p);
            let sightings = lib.job_sightings(j.id)?;
            for g in &hp.groups {
                let bare = g.rule.trim_end_matches('V');
                let (mut ships, mut guns, mut puffers) = (vec![], vec![], vec![]);
                let mut osc: Vec<(u64, String)> = Vec::new();
                for (pid, rule, count, _) in &sightings {
                    if rule != bare && rule != &g.rule {
                        continue;
                    }
                    let Some(r) = find(pid) else { continue };
                    match r.class {
                        PatternClass::Spaceship => ships.push(format!(
                            "{} {}c p{} x{count}",
                            r.speed.clone().unwrap_or_default(),
                            r.cells,
                            r.period
                        )),
                        PatternClass::Gun => guns.push(format!("p{} x{count}", r.period)),
                        PatternClass::Puffer => puffers.push(format!(
                            "{} p{} x{count}",
                            r.speed.clone().unwrap_or_default(),
                            r.period
                        )),
                        PatternClass::Oscillator => {
                            osc.push((r.period, format!("p{} {}c", r.period, r.cells)))
                        }
                        _ => {}
                    }
                }
                osc.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                osc.dedup_by(|a, b| a.0 == b.0);
                let _ = writeln!(
                    o,
                    "| {} | {} | {} | {} | {} ({}) | {} | {} | {} | {} |",
                    j.id,
                    g.tiling,
                    g.rule,
                    g.soups,
                    g.distinct,
                    g.unstored_distinct,
                    cap(ships, 4),
                    cap(guns, 3),
                    cap(puffers, 3),
                    osc.iter()
                        .take(3)
                        .map(|x| x.1.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
    }
    // Bounded searches and collisions: their finds.
    for (kind, title) in [("search", "Soup searches"), ("collide", "Collisions")] {
        if of(kind).next().is_none() {
            continue;
        }
        let _ = writeln!(
            o,
            "\n## {title}\n\n| Job | Distinct patterns | Copies | By class | Most seen |\n| --- | --- | --- | --- | --- |"
        );
        for j in of(kind) {
            let sightings = lib.job_sightings(j.id)?;
            let mut ids: Vec<&String> = sightings.iter().map(|s| &s.0).collect();
            ids.sort();
            ids.dedup();
            let copies: u64 = sightings.iter().map(|s| s.2).sum();
            let mut by_class: std::collections::BTreeMap<String, usize> = Default::default();
            for id in &ids {
                if let Some(r) = find(id) {
                    *by_class.entry(r.class.name().to_string()).or_default() += 1;
                }
            }
            let most: Vec<String> = sightings
                .iter()
                .take(top)
                .map(|(id, rule, n, _)| {
                    let l = find(id).map(|r| r.label()).unwrap_or_else(|| id.clone());
                    format!("{l} ({rule}) x{n}")
                })
                .collect();
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} | {} |",
                j.id,
                ids.len(),
                copies,
                by_class
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                cell(&most.join("; "))
            );
        }
    }
    // Ant searches: outcomes per tiling, and the newest highways.
    if of("ants").next().is_some() {
        let _ = writeln!(
            o,
            "\n## Ant searches\n\n| Job | Tiling | Runs | Outcomes |\n| --- | --- | --- | --- |"
        );
        for j in of("ants") {
            for t in lib.ant_census(Some(j.id))? {
                let _ = writeln!(
                    o,
                    "| {} | {} | {} | {} |",
                    j.id,
                    t.tiling_name,
                    t.runs,
                    t.by_outcome
                        .iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let (p, _) = live_progress(cx, lib, j);
            let ap: AntProgress = jobs_typed(&p);
            let hw: Vec<String> = ap
                .newest
                .iter()
                .take(top)
                .map(|f| {
                    format!(
                        "{} on {}: {}{}{}",
                        f.rule,
                        f.tiling,
                        f.outcome.name(),
                        f.period.map(|x| format!(" p{x}")).unwrap_or_default(),
                        f.onset
                            .map(|x| format!(" from step {x}"))
                            .unwrap_or_default()
                    )
                })
                .collect();
            if !hw.is_empty() {
                let _ = writeln!(o, "\nJob {} newest: {}.", j.id, hw.join("; "));
            }
        }
    }
    // Evolve searches: champions and verified movers.
    if of("evolve").next().is_some() {
        let _ = writeln!(o, "\n## Evolutionary searches\n");
        for j in of("evolve") {
            let p = &j.progress;
            let champs: Vec<String> = p["champions"]
                .as_array()
                .into_iter()
                .flatten()
                .take(top)
                .map(|c| {
                    format!(
                        "`{}` ({:.2})",
                        c["rule"].as_str().unwrap_or(""),
                        c["fitness"].as_f64().unwrap_or(0.0)
                    )
                })
                .collect();
            let _ = writeln!(
                o,
                "- Job {}: {} generations, best ever {}, {} verified glider(s){}.",
                j.id,
                p["generation"].as_u64().unwrap_or(0),
                p["best_ever"]
                    .as_f64()
                    .map(|b| format!("{b:.2}"))
                    .unwrap_or_else(|| "-".into()),
                p["gliders"].as_u64().unwrap_or(0),
                if champs.is_empty() {
                    String::new()
                } else {
                    format!("; champions {}", champs.join(", "))
                }
            );
            for m in p["movers"].as_array().into_iter().flatten() {
                let _ = writeln!(o, "  - {}", cell(m["summary"].as_str().unwrap_or("")));
            }
        }
    }
    // Explore searches: what they covered, and their families.
    if of("explore").next().is_some() {
        let _ = writeln!(
            o,
            "\n## Explore searches\n\n| Job | Runs | Confirmation runs | Elites | Niches | Confirmed elites | Tilings | Families (runs, best score) |\n| --- | --- | --- | --- | --- | --- | --- | --- |"
        );
        for j in of("explore") {
            let (p, _) = live_progress(cx, lib, j);
            let ep: ExploreProgress = jobs_typed(&p);
            let fams: Vec<String> = ep
                .families
                .iter()
                .map(|f| format!("{} {} ({:.2})", f.family, digits(f.runs), f.best))
                .collect();
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} | {} | {} | {} of {} | {} |",
                j.id,
                digits(ep.screened),
                digits(ep.confirmed),
                digits(ep.elites),
                digits(ep.niches),
                digits(ep.confirmed_elites),
                digits(ep.tilings_tried),
                digits(ep.tilings),
                cell(&cap(fams, top))
            );
        }
    }
    // Movers and piles: their summaries.
    for (kind, title) in [("mover", "Movers"), ("pile", "Sandpiles")] {
        if of(kind).next().is_none() {
            continue;
        }
        let _ = writeln!(o, "\n## {title}\n");
        for j in of(kind) {
            let (p, _) = live_progress(cx, lib, j);
            let _ = writeln!(o, "- {}", cell(&line_of(jobs::kind(kind), j, &p)));
            for k in ["summary", "record", "png", "folder"] {
                if let Some(v) = p.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
                    let _ = writeln!(o, "  - {k}: {}", cell(v));
                }
            }
        }
    }
    Ok(o)
}
