//! The hunt dashboard's logic (the app's *Hunts…* window draws it): every catalog job as a
//! [`JobSummary`], pause, resume, and stop across processes, the rule-space heat map, and the
//! finds to show, animate, and rate.
//!
//! **Job kinds.** Everything kind-specific sits behind [`JobKind`]: how a job's saved progress
//! reads ([`JobKind::summary`]), how to ask it to stop from any process
//! ([`JobKind::request_stop`]), how to run or resume it in this process ([`JobKind::resume`],
//! returning a [`RunningJob`] with live progress), which heat maps it feeds
//! ([`JobKind::heat_spaces`]), and its finds ([`JobKind::finds`]). [`kind`] looks one up by the
//! `jobs.kind` column: `hunt` (`crate::hunt::job`), `census` (`crate::census`), `ants`
//! (`crate::ants::job`), `pile` (`crate::pile::job`), `mover`
//! (`crate::window::job`), `evolve` (summary and stop; a search has no checkpoint to resume),
//! and a generic fallback for the rest (`search`, `collide`: listed, paused by setting the
//! status, never resumed). A new kind of job is one more `impl JobKind` and a
//! line in [`kind`].
//!
//! **Heat map** ([`heat_map`]). Life-like rules on one tiling and neighbourhood, birth mask
//! across (B1..B8, B0 is never searched) and survival mask down (S0..S8), each axis in Gray-code
//! order: neighbouring columns (rows) differ in exactly one birth (survival) count, so the
//! rule-space neighbours of a rule (one count added or removed) are mostly beside it and
//! interesting rules show up as clusters rather than scattered dots. Squares with Moore
//! neighbours make 256 x 512 cells, hexagons 64 x 128, von Neumann squares 16 x 32. Tilings with
//! more than 8 neighbours fold the higher counts onto the cell of their first eight (the map
//! says so). A cell's value is the sum over its rules (finds, spaceships).

use crate::census;
use crate::hunt::job::{self as hunt, HuntOptions, HuntProgress, HuntSpec, SoupBatcher};
use crate::library::store::JobRow;
use crate::library::{
    CurationState, HeatQuery, HeatRow, JobStatus, Library, QsStatus, Update, tiling_display_name,
    tiling_key,
};
use crate::rules::Rule;
use crate::topology::{Nb, SizeSpec, TilingSpec, Topology};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A running job that has not written its progress for this long is shown as stalled (its
/// process probably ended without marking it), and may be resumed here.
pub const STALE_S: i64 = 300;

/// One job as the dashboard shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct JobSummary {
    pub id: i64,
    pub kind: String,
    /// `Hunt`, `Census`, `Ant search`, ...
    pub kind_label: String,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    /// Running, but no progress written for [`STALE_S`] seconds.
    pub stale: bool,
    /// Running in this process.
    pub here: bool,
    /// Rules and tilings, or spaces.
    pub title: String,
    /// Units done and in all (`None`: runs until stopped), and what a unit is.
    pub done: u64,
    pub total: Option<u64>,
    pub unit: String,
    /// Units per second in the last session.
    pub per_s: f64,
    pub elapsed_s: f64,
    /// Seconds left, when the job is running and has an end.
    pub eta_s: Option<f64>,
    /// Counters to show (`settled 1,204`), in order.
    pub counters: Vec<(String, String)>,
    /// Bars to show (finds per class, a score histogram), in order.
    pub bars: Vec<(String, u64)>,
    /// What the bars are.
    pub bars_title: String,
    pub created: String,
    pub updated: String,
    pub error: Option<String>,
    /// The kind can run it here (pause, then resume).
    pub resumable: bool,
}

impl JobSummary {
    /// The fraction done, if the job has an end.
    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|&t| t > 0)
            .map(|t| (self.done as f64 / t as f64).clamp(0.0, 1.0))
    }
    /// Can be resumed in this process now.
    pub fn can_resume(&self) -> bool {
        self.resumable
            && !self.here
            && (matches!(self.status.as_str(), "paused" | "failed") || self.stale)
    }
    /// Running somewhere (here or in another process) and not stalled.
    pub fn is_running(&self) -> bool {
        self.status == "running" && !self.stale
    }
}

/// How to run a job here (none of this changes its results).
#[derive(Clone, Default)]
pub struct RunOptions {
    /// Worker threads (0: all cores).
    pub threads: usize,
    /// GPU batches for hunts.
    pub gpu: Option<Arc<dyn SoupBatcher>>,
}

/// A job running on a background thread of this process.
pub trait RunningJob: Send {
    fn job(&self) -> i64;
    /// Live progress (updated after every batch), as a summary.
    fn summary(&self) -> JobSummary;
    /// Stop at the next batch (the job is left paused at its checkpoint).
    fn stop(&self);
    fn is_finished(&self) -> bool;
    /// Wait for the end.
    fn join(self: Box<Self>) -> Result<(), String>;
}

/// A heat map a job feeds: one tiling, neighbourhood, and state count.
#[derive(Clone, Debug, PartialEq)]
pub struct HeatSpace {
    pub tiling: TilingSpec,
    pub tiling_name: String,
    pub nb: Nb,
    pub states: u32,
    pub query: HeatQuery,
}

impl HeatSpace {
    pub fn label(&self) -> String {
        format!(
            "{} ({}){}",
            self.tiling_name,
            match self.nb {
                Nb::Touch => "touch",
                Nb::Edge => "edge",
            },
            if self.states > 2 {
                format!(" C{}", self.states)
            } else {
                String::new()
            }
        )
    }
}

/// What a find is, for rating and rejecting.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Pattern(String),
}

/// A find to show (and animate, rate, reject).
#[derive(Clone, Debug, PartialEq)]
pub struct FindItem {
    pub target: Target,
    pub title: String,
    pub subtitle: String,
    /// The rule it was found in.
    pub rule: String,
    /// The user's rating.
    pub rating: Option<u8>,
    pub rejected: bool,
}

/// Newest and rarest finds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Finds {
    pub newest: Vec<FindItem>,
    pub other: Vec<FindItem>,
    /// `Rarest`.
    pub other_title: String,
}

/// Everything kind-specific about a job.
pub trait JobKind: Send + Sync {
    /// The `jobs.kind` value.
    fn kind(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// The job's saved progress as a summary (no catalog reads).
    fn summary(&self, row: &JobRow) -> JobSummary;
    /// Ask the job to stop at its next batch, in whatever process runs it.
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String>;
    /// Can [`resume`](Self::resume) run it?
    fn resumable(&self) -> bool {
        false
    }
    /// Run (or resume) the job on a background thread of this process.
    fn resume(
        &self,
        _lib: Library,
        job: i64,
        _o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        Err(format!(
            "{} jobs cannot be resumed from the app (job {job})",
            self.kind()
        ))
    }
    /// The heat maps this job feeds (none by default).
    fn heat_spaces(&self, _row: &JobRow) -> Vec<HeatSpace> {
        vec![]
    }
    /// Its finds, limited to these rules (all when empty).
    fn finds(
        &self,
        _lib: &Library,
        _row: &JobRow,
        _rules: &[String],
        _n: usize,
    ) -> Result<Finds, String> {
        Ok(Finds::default())
    }
}

static HUNT: HuntKind = HuntKind;
static CENSUS: CensusKind = CensusKind;
static OTHER: OtherKind = OtherKind;
static ANTS: AntsKind = AntsKind;
static PILE: PileKind = PileKind;
static MOVER: MoverKind = MoverKind;
static EVOLVE: EvolveKind = EvolveKind;

/// The kind of a `jobs.kind` value (a generic one for kinds without their own).
pub fn kind(name: &str) -> &'static dyn JobKind {
    match name {
        "hunt" => &HUNT,
        "census" => &CENSUS,
        crate::ants::job::KIND => &ANTS,
        crate::pile::job::KIND => &PILE,
        crate::window::job::KIND => &MOVER,
        "evolve" => &EVOLVE,
        _ => &OTHER,
    }
}

/// Every job but the "manual" job 0, newest first, with `stale` set for running jobs whose
/// progress is older than [`STALE_S`] at `now` (seconds since 1970).
pub fn summaries(rows: &[JobRow], now: i64) -> Vec<JobSummary> {
    let mut v: Vec<JobSummary> = rows
        .iter()
        .filter(|r| r.id != 0 && r.kind != "manual")
        .map(|r| {
            let mut s = kind(&r.kind).summary(r);
            if let Some(l) = r.label() {
                s.title = if s.title.is_empty() {
                    l.to_string()
                } else {
                    format!("{l}: {}", s.title)
                };
            }
            s.stale = r.status == JobStatus::Running
                && utc_seconds(&r.updated).is_some_and(|t| now - t > STALE_S);
            s
        })
        .collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.id));
    v
}

/// Seconds since 1970 now.
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` (as `library::now_utc` writes) as seconds since 1970.
pub fn utc_seconds(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> { s.get(a..a + n)?.parse().ok() };
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hh, mm, ss) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// `1,234,567`.
pub fn digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `3 d 4 h`, `12 min 5 s`, `40 s`.
pub fn duration(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    let (d, h, m, sec) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min {sec} s")
    } else {
        format!("{sec} s")
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

fn base_summary(row: &JobRow, k: &dyn JobKind) -> JobSummary {
    JobSummary {
        id: row.id,
        kind: row.kind.clone(),
        kind_label: k.label().into(),
        status: status_name(row.status).into(),
        created: row.created.clone(),
        updated: row.updated.clone(),
        resumable: k.resumable(),
        ..Default::default()
    }
}

fn eta(done: u64, total: Option<u64>, per_s: f64, running: bool) -> Option<f64> {
    let t = total?;
    (running && per_s > 0.0 && t > done).then(|| (t - done) as f64 / per_s)
}

/// `B3/S23, B36/S23 (+3)`.
fn short_list(items: &[String], n: usize) -> String {
    let mut s = items.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if items.len() > n {
        s.push_str(&format!(" (+{})", items.len() - n));
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Hunts

struct HuntKind;

/// A hunt's summary from its spec and progress.
pub fn hunt_summary(base: JobSummary, spec: Option<&HuntSpec>, p: &HuntProgress) -> JobSummary {
    let mut s = base;
    let groups = (p.groups.len() as u64).max(
        spec.map(|x| (x.rules.len() * x.tilings.len()) as u64)
            .unwrap_or(1),
    );
    let groups = groups.max(1);
    if let Some(sp) = spec {
        let tilings: Vec<String> = sp.tilings.iter().map(tiling_display_name).collect();
        s.title = format!(
            "{} on {}, {}x{} soups at {}",
            short_list(&sp.rules, 3),
            short_list(&tilings, 2),
            sp.soup.w,
            sp.soup.h,
            sp.soup.density
        );
    }
    s.unit = "soups".into();
    s.done = p.soups_done * groups;
    s.total = p
        .soups_total
        .or(spec.and_then(|x| x.soups))
        .map(|t| t * groups);
    s.per_s = p.soups_per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters = vec![
        ("settled".into(), digits(p.settled)),
        ("died".into(), digits(p.died)),
        ("exploded".into(), digits(p.exploded)),
        ("unsettled".into(), digits(p.unsettled)),
        ("objects".into(), digits(p.objects)),
        ("distinct".into(), digits(p.distinct)),
        ("new to the catalog".into(), digits(p.new_patterns)),
        ("failed verification".into(), digits(p.failed)),
    ];
    if groups > 1 {
        s.counters
            .push(("groups (rule x tiling)".into(), digits(groups)));
    }
    if let Some(g) = &p.gpu {
        s.counters.push(("GPU".into(), g.clone()));
    }
    s.bars_title = "Object copies by class".into();
    let order = ["still life", "oscillator", "spaceship", "puffer", "gun"];
    let mut bars: Vec<(String, u64)> = order
        .iter()
        .filter_map(|c| p.by_class.get(*c).map(|n| (c.to_string(), *n)))
        .collect();
    for (k, v) in &p.by_class {
        if !order.contains(&k.as_str()) {
            bars.push((k.clone(), *v));
        }
    }
    s.bars = bars;
    s.error = p.error.clone();
    s
}

/// The neighbourhood and state count of a hunt's first rule.
fn rule_space(rule: &str, nb: Option<Nb>) -> Option<(Nb, u32)> {
    let (r, hint) = Rule::parse(rule).ok()?;
    let states = match r {
        Rule::LifeLike { .. } => 2,
        Rule::Generations { states, .. } => states as u32,
        _ => return None,
    };
    Some((nb.or(hint).unwrap_or_default(), states))
}

struct HuntRun(hunt::HuntHandle);

impl RunningJob for HuntRun {
    fn job(&self) -> i64 {
        self.0.job
    }
    fn summary(&self) -> JobSummary {
        let p = self.0.progress();
        let base = JobSummary {
            id: self.0.job,
            kind: "hunt".into(),
            kind_label: HUNT.label().into(),
            status: p.status.clone(),
            here: true,
            resumable: true,
            updated: p.updated.clone(),
            ..Default::default()
        };
        hunt_summary(base, None, &p)
    }
    fn stop(&self) {
        self.0.stop();
    }
    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
    fn join(self: Box<Self>) -> Result<(), String> {
        self.0.join().map(|_| ())
    }
}

impl JobKind for HuntKind {
    fn kind(&self) -> &'static str {
        "hunt"
    }
    fn label(&self) -> &'static str {
        "Hunt"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec: Option<HuntSpec> = serde_json::from_value(row.spec.clone()).ok();
        let p: HuntProgress = serde_json::from_value(row.progress.clone()).unwrap_or_default();
        hunt_summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        hunt::request_stop(lib, job)
    }
    fn resumable(&self) -> bool {
        true
    }
    fn resume(
        &self,
        lib: Library,
        job: i64,
        o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        let opts = HuntOptions {
            threads: o.threads,
            gpu: o.gpu.clone(),
            ..Default::default()
        };
        Ok(Box::new(HuntRun(hunt::HuntHandle::spawn(lib, job, opts))))
    }
    fn heat_spaces(&self, row: &JobRow) -> Vec<HeatSpace> {
        let Ok(spec) = serde_json::from_value::<HuntSpec>(row.spec.clone()) else {
            return vec![];
        };
        let Some((nb, states)) = spec
            .rules
            .iter()
            .find_map(|r| rule_space(r, spec.neighborhood))
        else {
            return vec![];
        };
        spec.tilings
            .iter()
            .map(|t| HeatSpace {
                tiling: t.clone(),
                tiling_name: tiling_display_name(t),
                nb,
                states,
                query: HeatQuery::Patterns { job: Some(row.id) },
            })
            .collect()
    }
    fn finds(
        &self,
        lib: &Library,
        row: &JobRow,
        rules: &[String],
        n: usize,
    ) -> Result<Finds, String> {
        let item = |f: crate::library::FindRow| FindItem {
            subtitle: format!(
                "{} · seen {} here, {} in all",
                f.rule,
                digits(f.count),
                digits(f.seen)
            ),
            title: f.label,
            rule: f.rule,
            rating: f.rating,
            rejected: f.state == "rejected",
            target: Target::Pattern(f.id),
        };
        Ok(Finds {
            newest: lib
                .job_finds(Some(row.id), rules, "new", false, n)?
                .into_iter()
                .map(item)
                .collect(),
            other: lib
                .job_finds(Some(row.id), rules, "rare", false, n)?
                .into_iter()
                .map(item)
                .collect(),
            other_title: "Rarest".into(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Census jobs

struct CensusKind;

struct CensusRun {
    job: i64,
    stop: Arc<AtomicBool>,
    live: Arc<std::sync::Mutex<census::CensusProgress>>,
    thread: Option<std::thread::JoinHandle<Result<census::CensusProgress, String>>>,
}

fn census_summary(mut s: JobSummary, p: &census::CensusProgress) -> JobSummary {
    s.unit = "rules".into();
    s.done = p.rules_done;
    s.total = Some(p.rules_total);
    s.per_s = p.per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s
}

impl RunningJob for CensusRun {
    fn job(&self) -> i64 {
        self.job
    }
    fn summary(&self) -> JobSummary {
        let p = self.live.lock().unwrap().clone();
        let base = JobSummary {
            id: self.job,
            kind: "census".into(),
            kind_label: CENSUS.label().into(),
            status: if self.is_finished() {
                "paused".into()
            } else {
                "running".into()
            },
            here: true,
            resumable: true,
            updated: p.updated.clone(),
            ..Default::default()
        };
        census_summary(base, &p)
    }
    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    fn join(mut self: Box<Self>) -> Result<(), String> {
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| "the census thread panicked".to_string())?
                .map(|_| ()),
            None => Err("the census thread could not start".into()),
        }
    }
}

impl JobKind for CensusKind {
    fn kind(&self) -> &'static str {
        "census"
    }
    fn label(&self) -> &'static str {
        "Census"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec: Option<census::CensusSpec> = serde_json::from_value(row.spec.clone()).ok();
        let mut p: census::CensusProgress =
            serde_json::from_value(row.progress.clone()).unwrap_or_default();
        let mut s = base_summary(row, self);
        if let Some(sp) = &spec {
            p.rules_total = sp.rules.len() as u64;
            s.title = format!(
                "{} rules on {}, {} soups each",
                digits(sp.rules.len() as u64),
                tiling_display_name(&sp.tiling),
                sp.soups
            );
        }
        census_summary(s, &p)
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        census::request_stop(lib, job)
    }
    fn resumable(&self) -> bool {
        true
    }
    fn resume(
        &self,
        lib: Library,
        job: i64,
        o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let live = Arc::new(std::sync::Mutex::new(
            census::job_status(&lib, job)
                .map(|x| x.1)
                .unwrap_or_default(),
        ));
        let l2 = live.clone();
        let opts = census::CensusJobOptions {
            threads: o.threads,
            stop: stop.clone(),
            on_progress: Some(Arc::new(move |p: &census::CensusProgress| {
                *l2.lock().unwrap() = p.clone();
            })),
            ..Default::default()
        };
        let thread = std::thread::Builder::new()
            .name(format!("census-{job}"))
            .spawn(move || census::run_job(&lib, job, &opts))
            .ok();
        Ok(Box::new(CensusRun {
            job,
            stop,
            live,
            thread,
        }))
    }
}

// ---------------------------------------------------------------------------------------------
// Ant searches, sandpiles, sliding-window movers, evolutionary searches

struct AntsKind;
struct PileKind;
struct MoverKind;
struct EvolveKind;

/// An ant search's summary from its spec and progress.
pub fn ants_summary(
    base: JobSummary,
    spec: Option<&crate::ants::job::AntSpec>,
    p: &crate::ants::job::AntProgress,
) -> JobSummary {
    let mut s = base;
    if let Some(sp) = spec {
        let tilings: Vec<String> = sp.tilings.iter().map(tiling_display_name).collect();
        let what = if !sp.rules.is_empty() {
            short_list(&sp.rules, 2)
        } else {
            let mut v = vec![];
            if let Some(st) = &sp.strings {
                v.push(format!("turn strings of {} to {}", st.min, st.max));
            }
            if sp.tables.is_some() {
                v.push("tables".into());
            }
            v.join(" and ")
        };
        s.title = format!(
            "{what} on {}{}",
            short_list(&tilings, 3),
            if sp.soup.is_some() { ", soups" } else { "" }
        );
    }
    s.unit = "runs".into();
    s.done = p.runs_done;
    s.total = (p.runs_total > 0).then_some(p.runs_total);
    s.per_s = p.runs_per_s;
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters.push(("ant steps".into(), digits(p.steps)));
    if p.steps_per_s > 0.0 {
        s.counters
            .push(("steps/s".into(), format!("{:.2e}", p.steps_per_s)));
    }
    if let Some(f) = p.newest.first() {
        s.counters.push((
            "newest".into(),
            format!(
                "{} on {} ({}{})",
                f.rule,
                f.tiling,
                f.outcome.name(),
                f.period.map(|x| format!(", p{x}")).unwrap_or_default()
            ),
        ));
    }
    s.bars_title = "Outcomes".into();
    s.bars = p.by_outcome.iter().map(|(k, v)| (k.clone(), *v)).collect();
    s.error = p.error.clone();
    s
}

/// A pile job's summary from its spec and progress.
pub fn pile_summary(
    base: JobSummary,
    spec: Option<&crate::pile::job::PileJobSpec>,
    p: &crate::pile::job::PileProgress,
) -> JobSummary {
    let mut s = base;
    s.title = match spec {
        Some(sp) => format!(
            "{} grains on {}",
            digits(sp.pile.grains),
            tiling_display_name(&sp.pile.tiling)
        ),
        None => format!("{} grains on {}", digits(p.grains), p.tiling),
    };
    s.unit = "doubling steps".into();
    s.done = p.step as u64;
    s.total = Some(p.steps as u64);
    s.elapsed_s = p.seconds + p.in_step[2];
    s.per_s = if p.seconds > 0.0 {
        p.step as f64 / p.seconds
    } else {
        0.0
    };
    s.counters
        .push(("grains so far".into(), digits(p.grains_done)));
    s.counters
        .push(("topplings".into(), format!("{:.3e}", p.topplings as f64)));
    s.counters.push(("sweeps".into(), digits(p.sweeps)));
    if p.in_step[2] > 0.0 {
        s.counters.push((
            "step in flight".into(),
            format!("{:.3e} topplings, {:.0} s", p.in_step[1], p.in_step[2]),
        ));
    }
    if let Some(b) = &p.backend {
        s.counters.push(("backend".into(), b.clone()));
    }
    s.counters.push(("folder".into(), p.folder.clone()));
    s.error = p.error.clone();
    s
}

/// A sliding-window mover's summary from its spec and progress.
pub fn mover_summary(
    base: JobSummary,
    spec: Option<&crate::window::job::MoverJobSpec>,
    p: &crate::window::job::MoverProgress,
) -> JobSummary {
    let mut s = base;
    if let Some(sp) = spec {
        s.title = format!(
            "{} on {}, {} cells",
            sp.track.rule,
            tiling_display_name(&sp.track.patch),
            sp.track.cells.len()
        );
    }
    match p.target_rings {
        Some(t) => {
            s.unit = "rings".into();
            s.done = p.rings as u64;
            s.total = Some(t as u64);
            s.per_s = p.rings_per_s;
        }
        None => {
            s.unit = "generations".into();
            s.done = p.generation;
            s.total = Some(p.max_gens);
            s.per_s = p.gens_per_s;
        }
    }
    s.elapsed_s = p.elapsed_s;
    s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
    s.counters.push(("rings".into(), digits(p.rings as u64)));
    s.counters.push(("generation".into(), digits(p.generation)));
    s.counters
        .push(("distance".into(), format!("{:.0} units", p.euclid)));
    s.counters.push((
        "window".into(),
        format!("level {}, {} tiles", p.level, digits(p.tiles as u64)),
    ));
    if !p.summary.is_empty() && matches!(p.status.as_str(), "done" | "failed") {
        s.counters.push(("result".into(), p.summary.clone()));
    }
    if let Some(r) = &p.record {
        s.counters.push(("record".into(), r.clone()));
    }
    s.error = p.error.clone();
    s
}

fn job_status_of(lib: &Library, job: i64) -> Result<JobStatus, String> {
    Ok(lib.job(job)?.status)
}

struct AntsRun {
    h: crate::ants::job::AntHandle,
    spec: Option<crate::ants::job::AntSpec>,
}
impl RunningJob for AntsRun {
    fn job(&self) -> i64 {
        self.h.job
    }
    fn summary(&self) -> JobSummary {
        let p = self.h.progress();
        let base = JobSummary {
            id: self.h.job,
            kind: crate::ants::job::KIND.into(),
            kind_label: ANTS.label().into(),
            status: if self.h.is_finished() && p.status == "running" {
                "paused".into()
            } else {
                p.status.clone()
            },
            here: !self.h.is_finished(),
            resumable: true,
            updated: p.updated.clone(),
            ..Default::default()
        };
        ants_summary(base, self.spec.as_ref(), &p)
    }
    fn stop(&self) {
        self.h.stop();
    }
    fn is_finished(&self) -> bool {
        self.h.is_finished()
    }
    fn join(self: Box<Self>) -> Result<(), String> {
        self.h.join().map(|_| ())
    }
}

impl JobKind for AntsKind {
    fn kind(&self) -> &'static str {
        crate::ants::job::KIND
    }
    fn label(&self) -> &'static str {
        "Ant search"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec = serde_json::from_value(row.spec.clone()).ok();
        let p = serde_json::from_value(row.progress.clone()).unwrap_or_default();
        ants_summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        crate::ants::job::request_stop(lib, job)
    }
    fn resumable(&self) -> bool {
        true
    }
    fn resume(
        &self,
        lib: Library,
        job: i64,
        o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        let (spec, _) = crate::ants::job::status(&lib, job)?;
        let opts = crate::ants::job::AntOptions {
            threads: o.threads,
            ..Default::default()
        };
        let h = crate::ants::job::AntHandle::spawn(lib, job, opts);
        Ok(Box::new(AntsRun {
            h,
            spec: Some(spec),
        }))
    }
}

struct PileRun {
    job: i64,
    spec: Option<crate::pile::job::PileJobSpec>,
    stop: Arc<AtomicBool>,
    live: Arc<std::sync::Mutex<crate::pile::job::PileProgress>>,
    thread: Option<std::thread::JoinHandle<Result<crate::pile::job::PileProgress, String>>>,
}
impl RunningJob for PileRun {
    fn job(&self) -> i64 {
        self.job
    }
    fn summary(&self) -> JobSummary {
        let p = self.live.lock().unwrap().clone();
        let fin = self.is_finished();
        let base = JobSummary {
            id: self.job,
            kind: crate::pile::job::KIND.into(),
            kind_label: PILE.label().into(),
            status: if fin && p.status == "running" {
                "paused".into()
            } else {
                p.status.clone()
            },
            here: !fin,
            resumable: true,
            updated: p.updated.clone(),
            ..Default::default()
        };
        pile_summary(base, self.spec.as_ref(), &p)
    }
    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    fn join(mut self: Box<Self>) -> Result<(), String> {
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| "the pile thread panicked".to_string())?
                .map(|_| ()),
            None => Err("the pile thread could not start".into()),
        }
    }
}

impl JobKind for PileKind {
    fn kind(&self) -> &'static str {
        crate::pile::job::KIND
    }
    fn label(&self) -> &'static str {
        "Sandpile"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec = serde_json::from_value(row.spec.clone()).ok();
        let p = serde_json::from_value(row.progress.clone()).unwrap_or_default();
        pile_summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        crate::pile::job::request_stop(lib, job)
    }
    fn resumable(&self) -> bool {
        true
    }
    /// Topples on the GPU when the job asks for it and the host registered a GPU toppler
    /// (`crate::pile::gpu_toppler`), else on the CPU.
    fn resume(
        &self,
        lib: Library,
        job: i64,
        _o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        let (spec, p) = crate::pile::job::status(&lib, job)?;
        let stop = Arc::new(AtomicBool::new(false));
        let live = Arc::new(std::sync::Mutex::new(p));
        let (s2, l2) = (stop.clone(), live.clone());
        let gpu = spec.gpu && !crate::pile::is_patch(&spec.pile.tiling);
        let thread = std::thread::Builder::new()
            .name(format!("pile-{job}"))
            .spawn(move || {
                let t = if gpu {
                    crate::pile::gpu_toppler().and_then(|r| r.ok())
                } else {
                    None
                };
                crate::pile::job::run_job(&lib, job, s2, t, Some(l2))
            })
            .ok();
        Ok(Box::new(PileRun {
            job,
            spec: Some(spec),
            stop,
            live,
            thread,
        }))
    }
}

struct MoverRun {
    h: crate::window::job::MoverHandle,
    spec: Option<crate::window::job::MoverJobSpec>,
}
impl RunningJob for MoverRun {
    fn job(&self) -> i64 {
        self.h.job
    }
    fn summary(&self) -> JobSummary {
        let p = self.h.progress();
        let fin = self.h.is_finished();
        let base = JobSummary {
            id: self.h.job,
            kind: crate::window::job::KIND.into(),
            kind_label: MOVER.label().into(),
            status: if fin && p.status == "running" {
                "paused".into()
            } else {
                p.status.clone()
            },
            here: !fin,
            resumable: true,
            updated: p.updated.clone(),
            ..Default::default()
        };
        mover_summary(base, self.spec.as_ref(), &p)
    }
    fn stop(&self) {
        self.h.stop();
    }
    fn is_finished(&self) -> bool {
        self.h.is_finished()
    }
    fn join(self: Box<Self>) -> Result<(), String> {
        self.h.join().map(|_| ())
    }
}

impl JobKind for MoverKind {
    fn kind(&self) -> &'static str {
        crate::window::job::KIND
    }
    fn label(&self) -> &'static str {
        "Mover (sliding window)"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let spec = serde_json::from_value(row.spec.clone()).ok();
        let p = serde_json::from_value(row.progress.clone()).unwrap_or_default();
        mover_summary(base_summary(row, self), spec.as_ref(), &p)
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        crate::window::job::request_stop(lib, job)
    }
    fn resumable(&self) -> bool {
        true
    }
    fn resume(
        &self,
        lib: Library,
        job: i64,
        _o: &RunOptions,
    ) -> Result<Box<dyn RunningJob>, String> {
        let (spec, _) = crate::window::job::status(&lib, job)?;
        let h = crate::window::job::MoverHandle::spawn(lib, job);
        Ok(Box::new(MoverRun {
            h,
            spec: Some(spec),
        }))
    }
}

impl JobKind for EvolveKind {
    fn kind(&self) -> &'static str {
        "evolve"
    }
    fn label(&self) -> &'static str {
        "Evolutionary search"
    }
    /// From the progress the running search writes (`generation`, `generations`, `best`,
    /// `best_ever`, `gliders`, `champions`).
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = base_summary(row, self);
        let spec: Option<crate::evolve::EvolveSpec> = serde_json::from_value(row.spec.clone()).ok();
        let p = &row.progress;
        let num = |k: &str| p.get(k).and_then(|v| v.as_f64());
        if let Some(sp) = &spec {
            s.title = format!(
                "{} states on {}, population {}",
                sp.states,
                tiling_display_name(&sp.tiling),
                sp.pop
            );
        }
        s.unit = "generations".into();
        s.done = num("generation").unwrap_or(0.0) as u64;
        s.total = num("generations")
            .map(|x| x as u64)
            .or(spec.as_ref().map(|x| x.gens as u64));
        s.elapsed_s = num("elapsed_s").unwrap_or(0.0);
        if s.elapsed_s > 0.0 {
            s.per_s = s.done as f64 / s.elapsed_s;
        }
        s.eta_s = eta(s.done, s.total, s.per_s, s.status == "running");
        for (k, label) in [
            ("best", "best"),
            ("best_ever", "best ever"),
            ("median", "median"),
        ] {
            if let Some(v) = num(k) {
                s.counters.push((label.into(), format!("{v:.2}")));
            }
        }
        if let Some(g) = num("gliders") {
            s.counters
                .push(("verified gliders".into(), format!("{g:.0}")));
        }
        if let Some(c) = p.get("champions").and_then(|v| v.as_array()) {
            let rules: Vec<String> = c
                .iter()
                .filter_map(|x| x.get("rule").and_then(|r| r.as_str()).map(String::from))
                .collect();
            if !rules.is_empty() {
                s.counters.push(("champions".into(), short_list(&rules, 2)));
            }
        }
        s.error = p.get("error").and_then(|v| v.as_str()).map(String::from);
        s
    }
    /// Sets the status to paused; the search checks it every two seconds and stops. (A search
    /// has no checkpoint: `evolve_start {"resume": job}` runs it again from its seed.)
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        if job_status_of(lib, job)? == JobStatus::Running {
            lib.update_job(job, Some(JobStatus::Paused), None, None)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Other kinds

struct OtherKind;

impl JobKind for OtherKind {
    fn kind(&self) -> &'static str {
        "other"
    }
    fn label(&self) -> &'static str {
        "Job"
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = base_summary(row, self);
        s.kind_label = match row.kind.as_str() {
            "search" => "Soup search".into(),
            "collide" => "Collisions".into(),
            k => k.to_string(),
        };
        s.title = row
            .spec
            .get("rules")
            .and_then(|r| r.as_array())
            .map(|a| {
                let v: Vec<String> = a
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
                short_list(&v, 3)
            })
            .or_else(|| {
                row.spec
                    .get("rule")
                    .and_then(|r| r.as_str())
                    .map(String::from)
            })
            .unwrap_or_default();
        s.unit = "items".into();
        if let Some(obj) = row.progress.as_object() {
            for (k, v) in obj.iter().take(8) {
                if v.is_number() || v.is_string() {
                    s.counters.push((k.replace('_', " "), v.to_string()));
                }
            }
        }
        s
    }
    fn request_stop(&self, lib: &Library, job: i64) -> Result<(), String> {
        let j = lib.job(job)?;
        if j.status == JobStatus::Running {
            lib.update_job(job, Some(JobStatus::Paused), None, None)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Actions on finds

/// Rate a find as the user (1 to 5): a `ratings` row with rater `user`, and for a pattern its
/// own `rating` too.
pub fn rate(lib: &Library, t: &Target, stars: u8) -> Result<(), String> {
    match t {
        Target::Pattern(id) => lib.rate_pattern(id, stars, "").map(|_| ()),
    }
}

/// Reject a find: a pattern's curation state becomes `rejected` (and its Quick Start entry, if
/// it has one, is hidden).
pub fn reject(lib: &Library, t: &Target) -> Result<(), String> {
    match t {
        Target::Pattern(id) => {
            let r = lib.update(
                id,
                &Update {
                    state: Some(CurationState::Rejected),
                    ..Default::default()
                },
            )?;
            if lib.get_quickstart(&r.id).is_ok() {
                lib.set_quickstart_status(&r.id, QsStatus::Rejected)?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Heat map

/// What colours the heat map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HeatColour {
    /// Distinct patterns found.
    #[default]
    Finds,
    /// Distinct spaceships found.
    Ships,
}

impl HeatColour {
    pub const ALL: [(HeatColour, &'static str); 2] = [
        (HeatColour::Finds, "finds"),
        (HeatColour::Ships, "spaceships"),
    ];
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|x| x.0 == self)
            .map(|x| x.1)
            .unwrap_or("")
    }
    /// The value of a row.
    pub fn value(self, r: &HeatRow) -> f64 {
        match self {
            HeatColour::Finds => r.finds as f64,
            HeatColour::Ships => r.ships as f64,
        }
    }
}

/// The binary-reflected Gray code of `n`.
pub fn gray(n: u32) -> u32 {
    n ^ (n >> 1)
}

/// The `n` whose Gray code is `g`.
pub fn gray_inverse(g: u32) -> u32 {
    let mut n = g;
    let mut s = 1;
    while s < 32 {
        n ^= n >> s;
        s <<= 1;
    }
    n
}

/// Birth and survival bits shown for a neighbour count (`degree`): B1..B8 and S0..S8 at most.
pub fn heat_bits(degree: u32) -> (u32, u32) {
    (degree.clamp(1, 8), (degree + 1).clamp(1, 9))
}

/// The cell `(x, y)` of a rule's birth and survival masks (bit `k` = count `k`).
pub fn heat_cell(birth: u32, survive: u32, degree: u32) -> (usize, usize) {
    let (bb, sb) = heat_bits(degree);
    let b = (birth >> 1) & ((1 << bb) - 1);
    let s = survive & ((1 << sb) - 1);
    (gray_inverse(b) as usize, gray_inverse(s) as usize)
}

/// The birth and survival masks of cell `(x, y)` (the shown bits only).
pub fn heat_masks(x: usize, y: usize, degree: u32) -> (u32, u32) {
    let (bb, sb) = heat_bits(degree);
    let b = gray(x as u32) & ((1 << bb) - 1);
    let s = gray(y as u32) & ((1 << sb) - 1);
    (b << 1, s)
}

/// A binned heat map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeatMap {
    pub degree: u32,
    /// Columns (2^birth bits) and rows (2^survival bits).
    pub w: usize,
    pub h: usize,
    /// Row-major, `w * h`.
    pub values: Vec<f64>,
    /// Rows binned into each cell (indices into the rows given), only for cells with any.
    pub rules: std::collections::HashMap<usize, Vec<usize>>,
    pub max: f64,
    /// Counts above 8 were folded onto the first eight.
    pub folded: bool,
    pub colour: HeatColour,
}

impl HeatMap {
    pub fn at(&self, x: usize, y: usize) -> f64 {
        if x < self.w && y < self.h {
            self.values[y * self.w + x]
        } else {
            0.0
        }
    }
    pub fn rules_at(&self, x: usize, y: usize) -> &[usize] {
        self.rules
            .get(&(y * self.w + x))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Bin rules into the B x S map (see the module notes).
pub fn heat_map(rows: &[HeatRow], degree: u32, colour: HeatColour) -> HeatMap {
    let (bb, sb) = heat_bits(degree);
    let (w, h) = (1usize << bb, 1usize << sb);
    let mut m = HeatMap {
        degree,
        w,
        h,
        values: vec![0.0; w * h],
        folded: degree > 8,
        colour,
        ..Default::default()
    };
    for (i, r) in rows.iter().enumerate() {
        let (x, y) = heat_cell(r.birth, r.survive, degree);
        let k = y * w + x;
        m.values[k] += colour.value(r);
        m.rules.entry(k).or_default().push(i);
    }
    m.max = m.values.iter().cloned().fold(0.0, f64::max);
    m
}

/// The largest neighbour count on a tiling (from a small copy of it); falls back to the
/// highest count used by `rows`.
pub fn tiling_degree(t: &TilingSpec, nb: Nb, rows: &[HeatRow]) -> u32 {
    let mut small = t.clone();
    small.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    let built = Topology::build(&small)
        .map(|topo| census::max_degree(&topo, nb) as u32)
        .unwrap_or(0);
    let used = rows
        .iter()
        .map(|r| 32 - (r.birth | r.survive).leading_zeros())
        .max()
        .unwrap_or(0)
        .saturating_sub(1);
    built.max(used).max(1)
}

/// Read and bin one heat map.
pub fn load_heat(
    lib: &Library,
    space: &HeatSpace,
    colour: HeatColour,
) -> Result<(HeatMap, Vec<HeatRow>), String> {
    let rows = lib.heat_rows(
        &space.query,
        &tiling_key(&space.tiling),
        space.nb,
        space.states,
    )?;
    let degree = tiling_degree(&space.tiling, space.nb, &rows);
    Ok((heat_map(&rows, degree, colour), rows))
}

/// Finds per class over all jobs, for the job list's footer.
pub fn class_totals(rows: &[JobSummary]) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for r in rows.iter().filter(|r| r.kind == "hunt") {
        for (k, v) in &r.bars {
            *m.entry(k.clone()).or_default() += v;
        }
    }
    m
}
