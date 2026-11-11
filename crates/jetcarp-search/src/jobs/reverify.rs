//! Re-verification jobs (`jobs.kind` `reverify`): check stored records again with this
//! engine, after a geometry change or a change in a rule's meaning flagged them
//! (`needs_reverify`), or all of them.
//!
//! A library flags a record when its geometry pin, or a pattern's rule-semantics version, is not
//! the running engine's (see `jetcarp_store::library::schema`). This job checks each flagged
//! record again, or every record with `all`, in id order: patterns, then ant runs, then Quick
//! Start entries.
//!
//! - A **pattern** is put in canonical form on the current geometry (its pin and so perhaps its
//!   content id change), verified again exactly ([`jetcarp_core::pattern::verify`]: N periods on
//!   a dead-edged frame, or on its patch at its site), and its rule range computed again; it
//!   then carries this engine's version of its rule's family.
//! - An **ant** run is run again from its start with its limits, and must end the same way
//!   (outcome, period, displacement and onset).
//! - A **Quick Start** scene must build, and, when it records a state hash, reach it again.
//!
//! What passes is stored with the new pin, its flag cleared, and a `reverified` provenance row;
//! a record whose id changed moves to the new id and keeps the old one as an alias. What fails
//! stays flagged and is listed in the progress; with `drop` it is deleted instead (never
//! otherwise). Steps check records for about two seconds on the runner's threads; the
//! checkpoint is the kind and the last id checked, so a stopped job resumes where it was, and
//! (records being checked once each, in order) ends as an uninterrupted one.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, status_name, typed};
use crate::dashboard::JobSummary;
use jetcarp_core::ants::run::run as run_ant;
use jetcarp_core::ants::search::parse_rule;
use jetcarp_core::pattern::{self, PatternRecord, default_periods, verify};
use jetcarp_store::library::{JobRow, Reverified, ReverifyTarget};
use jetcarp_store::{AntRow, JobStatus, Library, QuickStartRow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

/// What a re-verification job checks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverifySpec {
    /// Every record, not only those flagged `needs_reverify`.
    #[serde(default)]
    pub all: bool,
    /// Delete the records that fail (else they stay, flagged, and are reported).
    #[serde(default)]
    pub drop: bool,
    /// Periods a pattern is run (default: its own default, 4 to 16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periods: Option<u64>,
    /// Kept by the job (its label).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Value>,
}

/// A record that failed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReverifyFailure {
    /// `pattern`, `ant`, or `quickstart`.
    pub kind: String,
    pub id: String,
    pub error: String,
}

/// A record that passed with a new id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rekeyed {
    pub kind: String,
    pub old: String,
    pub new: String,
}

/// A re-verification job's progress (also its checkpoint).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReverifyProgress {
    pub job: i64,
    pub status: String,
    /// The kind being checked (`pattern`, `ant`, `quickstart`), `done` at the end.
    pub phase: String,
    /// The last id checked in that kind.
    pub after: String,
    /// Records flagged when the job started (all records with `all`).
    pub total: u64,
    pub checked: u64,
    pub passed: u64,
    pub failed: u64,
    /// Passed with a new id (the geometry changed); the old id is an alias now.
    pub rekeyed: u64,
    /// Failed and deleted (`drop`).
    pub removed: u64,
    /// The first failures (at most 200).
    pub failures: Vec<ReverifyFailure>,
    /// The first records that moved (at most 200).
    pub moved: Vec<Rekeyed>,
    pub elapsed_s: f64,
    pub line: String,
}

impl ReverifyProgress {
    fn line(&self) -> String {
        format!(
            "reverify {}: {} of {} checked, {} passed{}, {} failed{}",
            self.job,
            self.checked,
            self.total,
            self.passed,
            if self.rekeyed > 0 {
                format!(" ({} with new ids)", self.rekeyed)
            } else {
                String::new()
            },
            self.failed,
            if self.removed > 0 {
                format!(" ({} removed)", self.removed)
            } else {
                String::new()
            }
        )
    }
}

const KINDS: [&str; 3] = ["pattern", "ant", "quickstart"];
const LIST: usize = 200;

pub struct ReverifyKind;
pub(super) static KIND: ReverifyKind = ReverifyKind;

/// Records a job will check: flagged ones, or all with `all`.
fn total(lib: &Library, all: bool) -> Result<u64, String> {
    if all {
        let i = lib.info()?;
        return Ok((i.patterns + i.ants + i.quickstart) as u64);
    }
    Ok(lib.flagged()?.iter().map(|x| x.1).sum())
}

impl StrategyKind for ReverifyKind {
    fn kind(&self) -> &'static str {
        "reverify"
    }
    fn label(&self) -> &'static str {
        "Re-verification"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("records")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let s: ReverifySpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("reverify spec: {e}"))?;
        let v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
        let job = lib.create_job("reverify", &v)?;
        let p = ReverifyProgress {
            job,
            status: "paused".into(),
            phase: KINDS[0].into(),
            total: total(lib, s.all)?,
            ..Default::default()
        };
        lib.update_job(
            job,
            Some(JobStatus::Paused),
            Some(&serde_json::to_value(&p).map_err(|e| e.to_string())?),
            None,
        )?;
        Ok(job)
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        let spec: ReverifySpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| format!("job {} spec: {e}", row.id))?;
        Ok(Box::new(ReverifyRun::new(spec)))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let p: ReverifyProgress = typed(&row.progress);
        let spec: ReverifySpec = typed(&row.spec);
        let mut s = base_summary(row, self);
        s.title = if spec.all {
            "every record, checked again".into()
        } else {
            "flagged records, checked again".into()
        };
        s.unit = "records".into();
        s.done = p.checked;
        s.total = Some(p.total);
        s.elapsed_s = p.elapsed_s;
        s
    }
    fn line(&self, progress: &Value) -> String {
        typed::<ReverifyProgress>(progress).line
    }
}

/// One live re-verification run.
pub struct ReverifyRun {
    spec: ReverifySpec,
    p: ReverifyProgress,
    /// Records checked in this session (the item budget).
    session: u64,
    t0: Instant,
    before: f64,
    pool: Option<rayon::ThreadPool>,
}

impl ReverifyRun {
    pub fn new(spec: ReverifySpec) -> ReverifyRun {
        ReverifyRun {
            spec,
            p: ReverifyProgress::default(),
            session: 0,
            t0: Instant::now(),
            before: 0.0,
            pool: None,
        }
    }
}

/// Check one record again (see the module notes): the record as it passes, or why it fails.
pub fn check(target: &ReverifyTarget, periods: Option<u64>) -> Result<ReverifyTarget, String> {
    match target {
        ReverifyTarget::Pattern(r) => {
            check_pattern(r, periods).map(|r| ReverifyTarget::Pattern(Box::new(r)))
        }
        ReverifyTarget::Ant(a) => check_ant(a).map(|a| ReverifyTarget::Ant(Box::new(a))),
        ReverifyTarget::Quickstart(q) => {
            check_scene(q)?;
            Ok(target.clone())
        }
    }
}

fn check_pattern(r: &PatternRecord, periods: Option<u64>) -> Result<PatternRecord, String> {
    let mut x = r.clone();
    x.canonicalize()?;
    x.verified = verify(&x, periods.unwrap_or_else(|| default_periods(x.period)))?;
    if x.range.is_some() {
        x.range = pattern::range::rule_range(&x)?;
    }
    x.needs_reverify = false;
    x.engine_version = jetcarp_core::VERSION.into();
    x.semantics = jetcarp_core::semantics::of_notation(&x.rule)
        .ok_or_else(|| format!("its rule {} no longer reads", x.rule))?
        .1
        .into();
    x.updated = pattern::now_utc();
    Ok(x)
}

fn check_ant(a: &AntRow) -> Result<AntRow, String> {
    let m = parse_rule(&a.rule)?;
    let lat = jetcarp_core::ants::plane::Lattice::new(&a.tiling)?;
    let r = run_ant(&lat, &m, &a.start, &a.limits);
    let same = r.outcome == a.outcome && r.period == a.period && r.d == a.d && r.onset == a.onset;
    if !same {
        return Err(format!(
            "it now runs to {} (period {:?}, displacement {:?}, onset {:?}); the record says {} (period {:?}, displacement {:?}, onset {:?})",
            r.outcome.name(),
            r.period,
            r.d,
            r.onset,
            a.outcome.name(),
            a.period,
            a.d,
            a.onset
        ));
    }
    let mut x = a.clone();
    x.geometry_pin = jetcarp_core::pin::geometry_pin(&a.tiling)?.pin;
    x.id = jetcarp_store::ant_id(&a.rule, &a.tiling, &a.start);
    x.needs_reverify = false;
    x.engine_version = jetcarp_core::VERSION.into();
    Ok(x)
}

fn check_scene(q: &QuickStartRow) -> Result<(), String> {
    let mut w = jetcarp_core::World::from_scene(&q.scene)?;
    if let Some(rec) = &q.scene.record {
        // Scenes recorded far ahead are only built (their hash is checked when they are opened).
        if rec.generation > 100_000 {
            return Ok(());
        }
        w.apply(jetcarp_core::Command::Step {
            gens: rec.generation.saturating_sub(w.generation),
        })?;
        let h = w.hash_hex();
        if !rec.hash.is_empty() && h != rec.hash {
            return Err(format!(
                "at generation {} its state hash is {h}, the scene recorded {}",
                rec.generation, rec.hash
            ));
        }
    }
    Ok(())
}

impl Strategy for ReverifyRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        self.p = match checkpoint {
            Some(b) => serde_json::from_slice(b)
                .map_err(|e| format!("job {}: bad checkpoint: {e}", cx.job))?,
            None => {
                let mut p: ReverifyProgress = typed(&cx.row.progress);
                if p.phase.is_empty() {
                    p.phase = KINDS[0].into();
                    p.total = total(cx.lib, self.spec.all)?;
                }
                p
            }
        };
        self.p.job = cx.job;
        self.before = self.p.elapsed_s;
        self.t0 = Instant::now();
        self.pool = Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(super::thread_count(cx.threads))
                .build()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }

    fn step(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<Step, String> {
        let Some(k) = KINDS.iter().position(|k| *k == self.p.phase) else {
            return Ok(Step::Done);
        };
        let page = cx
            .lib
            .reverify_page(KINDS[k], self.spec.all, &self.p.after, 256)?;
        if page.is_empty() {
            self.p.phase = KINDS.get(k + 1).copied().unwrap_or("done").into();
            self.p.after.clear();
            return Ok(if self.p.phase == "done" {
                Step::Done
            } else {
                Step::More
            });
        }
        let room = cx
            .items
            .map(|n| n.saturating_sub(self.session) as usize)
            .unwrap_or(usize::MAX)
            .max(1);
        let page: Vec<ReverifyTarget> = page.into_iter().take(room).collect();
        let pool = self.pool.as_ref().ok_or("the run was not restored")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let periods = self.spec.periods;
        let done = super::prefix_until(pool, 0..page.len(), deadline, &cx.stop, |i| {
            check(&page[i], periods)
        });
        let n = done.len();
        for (target, result) in page.into_iter().zip(done) {
            self.p.checked += 1;
            self.session += 1;
            match &result {
                Ok(now) => {
                    self.p.passed += 1;
                    if now.id() != target.id() {
                        self.p.rekeyed += 1;
                        if self.p.moved.len() < LIST {
                            self.p.moved.push(Rekeyed {
                                kind: target.kind().into(),
                                old: target.id().into(),
                                new: now.id().into(),
                            });
                        }
                    }
                }
                Err(e) => {
                    self.p.failed += 1;
                    if self.spec.drop {
                        self.p.removed += 1;
                    }
                    if self.p.failures.len() < LIST {
                        self.p.failures.push(ReverifyFailure {
                            kind: target.kind().into(),
                            id: target.id().into(),
                            error: e.clone(),
                        });
                    }
                }
            }
            self.p.after = target.id().to_string();
            sink.reverified.push(Reverified {
                target,
                result,
                drop: self.spec.drop,
            });
        }
        debug_assert_eq!(sink.reverified.len(), n);
        Ok(Step::More)
    }

    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        serde_json::to_vec(&self.p)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn progress(&self, status: JobStatus) -> Value {
        let mut p = self.p.clone();
        p.status = status_name(status).into();
        p.elapsed_s = self.before + self.t0.elapsed().as_secs_f64();
        p.line = p.line();
        serde_json::to_value(p).unwrap_or_default()
    }

    fn items(&self) -> u64 {
        self.session
    }
}
