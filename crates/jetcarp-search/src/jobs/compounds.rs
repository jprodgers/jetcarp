//! Compound records (`jobs.kind` `compounds`): find the stored patterns that are several
//! independent objects (two gliders flying together, a pulsar with blinkers beside it, two blocks
//! a cell apart), and with `apply` store their parts in their place.
//!
//! Every pattern is examined in id order ([`jetcarp_core::pattern::split`]: each part runs alone
//! as an object of the record's period and velocity, and the record's phases are exactly the
//! union of the parts' runs). For each compound the job builds its parts' records and verifies
//! them in the compound's rule ([`jetcarp_store::library::prepare`]), and decides:
//!
//! - **remove**: nobody curated it (no name, rating, notes, tags, curation state, Quick Start
//!   entry or rating row) and every part runs in every rule the compound runs in (each part's rule
//!   range holds the compound's, or, without ranges, each part works in each of its rules);
//! - **keep**: otherwise, tagged `compound` with a note naming its parts, for a person to decide.
//!   A record already tagged `compound` was handled by an earlier pass and is left alone.
//!
//! **Without `apply`** (the default) nothing in the library changes: the job lists what it would
//! do. **With `apply`** each compound's parts are stored (or merged), credited with its seen
//! count and sightings as many times as it holds them, given its history, and the compound is
//! removed or kept tagged ([`jetcarp_store::library::CompoundSplit`]).
//!
//! **Report.** Either way, every compound found is written to a report, one JSON object per line
//! (JSON Lines): its id, label, class, tiling, rule, period, cells, seen count, the action and
//! why, its parts (id, label, class, period, cells, copies, and whether already stored), and the
//! whole record as it was, with its sightings and history, so nothing removed is lost. The report
//! is `reports/compounds-<job>.jsonl` beside the library unless the spec names a file. Steps
//! examine records for about two seconds on the runner's threads; the checkpoint is the last id
//! examined and the report's length, so a stopped job resumes where it was (cutting the report
//! back to that length) and ends as an uninterrupted one.

use super::{RunCx, Sink, Step, Strategy, StrategyKind, base_summary, status_name, typed};
use crate::dashboard::JobSummary;
use jetcarp_core::pattern::split::{part_records, split_compound};
use jetcarp_core::pattern::{CurationState, PatternRecord, RuleRange};
use jetcarp_store::library::{COMPOUND_TAG, CompoundSplit, JobRow, prepare, short_tiling_key};
use jetcarp_store::{JobStatus, Library};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What a compounds job does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundsSpec {
    /// Change the library: store the parts, and remove (or tag) the compounds. Without it the
    /// job only lists them.
    #[serde(default)]
    pub apply: bool,
    /// The report file (default: `reports/compounds-<job>.jsonl` beside the library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// Kept by the job (its label).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Value>,
}

/// One part of a compound, as the report lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartRow {
    pub id: String,
    pub label: String,
    pub class: String,
    pub period: u64,
    pub cells: usize,
    /// Copies of it in the compound.
    pub copies: u64,
    /// Already in the library before the compound was examined.
    pub stored: bool,
}

/// One compound, as the report and the progress list it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompoundRow {
    pub id: String,
    pub label: String,
    pub class: String,
    /// The tiling's key (family or catalog id).
    pub tiling: String,
    pub rule: String,
    pub period: u64,
    pub cells: usize,
    pub seen: u64,
    /// `remove`, `keep` (tagged `compound`), with `apply`; `would remove`, `would keep`
    /// without; `failed` when a part did not verify.
    pub action: String,
    pub why: String,
    pub parts: Vec<PartRow>,
}

/// A compounds job's progress (also its checkpoint).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompoundsProgress {
    pub job: i64,
    pub status: String,
    pub apply: bool,
    /// The last id examined (`done` at the end is in `status`).
    pub after: String,
    pub finished: bool,
    /// Patterns in the library when the job started.
    pub total: u64,
    pub checked: u64,
    pub compounds: u64,
    /// Removed (or, without `apply`, would be).
    pub removed: u64,
    /// Kept, tagged `compound` (or would be).
    pub kept: u64,
    /// Left alone: tagged `compound` by an earlier pass.
    pub already: u64,
    /// Compounds whose parts did not verify (left as they are).
    pub failed: u64,
    /// Parts new to the library, and parts already in it (or that an earlier compound added).
    pub parts_new: u64,
    pub parts_stored: u64,
    /// Compounds by class, and by tiling key.
    pub by_class: BTreeMap<String, u64>,
    pub by_tiling: BTreeMap<String, u64>,
    /// The first compounds found (at most 200).
    pub examples: Vec<CompoundRow>,
    /// The report file, and its length after the last step.
    pub report: String,
    pub report_bytes: u64,
    pub elapsed_s: f64,
    pub line: String,
}

impl CompoundsProgress {
    fn line(&self) -> String {
        let (r, k) = if self.apply {
            ("removed", "kept tagged")
        } else {
            ("would remove", "would keep")
        };
        // Parts the pass stores after its place are examined too, so `checked` can pass `total`.
        let examined = if self.checked > self.total {
            format!(
                "{} examined ({} at the start, then parts it stored)",
                self.checked, self.total
            )
        } else {
            format!("{} of {} examined", self.checked, self.total)
        };
        format!(
            "compounds {}: {examined}, {} compounds ({r} {}, {k} {}{}{}), {} parts ({} new){}",
            self.job,
            self.compounds,
            self.removed,
            self.kept,
            if self.already > 0 {
                format!(", {} already tagged", self.already)
            } else {
                String::new()
            },
            if self.failed > 0 {
                format!(", {} failed", self.failed)
            } else {
                String::new()
            },
            self.parts_new + self.parts_stored,
            self.parts_new,
            if self.apply { "" } else { " (dry run)" }
        )
    }
}

const LIST: usize = 200;
const PAGE: usize = 256;

pub struct CompoundsKind;
pub(super) static KIND: CompoundsKind = CompoundsKind;

/// The report file of a job.
pub fn report_path(lib: &Library, job: i64, spec: &CompoundsSpec) -> PathBuf {
    match &spec.report {
        Some(p) => PathBuf::from(p),
        None => lib
            .folder()
            .join("reports")
            .join(format!("compounds-{job}.jsonl")),
    }
}

impl StrategyKind for CompoundsKind {
    fn kind(&self) -> &'static str {
        "compounds"
    }
    fn label(&self) -> &'static str {
        "Compounds"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("records")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let s: CompoundsSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("compounds spec: {e}"))?;
        let v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
        let job = lib.create_job("compounds", &v)?;
        let p = CompoundsProgress {
            job,
            status: "paused".into(),
            apply: s.apply,
            total: lib.info()?.patterns as u64,
            report: report_path(lib, job, &s).display().to_string(),
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
        let spec: CompoundsSpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| format!("job {} spec: {e}", row.id))?;
        Ok(Box::new(CompoundsRun::new(spec)))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let p: CompoundsProgress = typed(&row.progress);
        let mut s = base_summary(row, self);
        s.title = if p.apply {
            "compound records split into their parts".into()
        } else {
            "compound records listed (dry run)".into()
        };
        s.unit = "records".into();
        s.done = p.checked;
        s.total = Some(p.total.max(p.checked));
        s.elapsed_s = p.elapsed_s;
        s
    }
    fn line(&self, progress: &Value) -> String {
        typed::<CompoundsProgress>(progress).line
    }
}

/// One live compounds run.
pub struct CompoundsRun {
    spec: CompoundsSpec,
    p: CompoundsProgress,
    /// Records examined in this session (the item budget).
    session: u64,
    t0: Instant,
    before: f64,
    pool: Option<rayon::ThreadPool>,
    report: PathBuf,
}

impl CompoundsRun {
    pub fn new(spec: CompoundsSpec) -> CompoundsRun {
        CompoundsRun {
            spec,
            p: CompoundsProgress::default(),
            session: 0,
            t0: Instant::now(),
            before: 0.0,
            pool: None,
            report: PathBuf::new(),
        }
    }
}

/// What examining one record found: its parts (prepared, with copies) and whether every part
/// runs in every rule the record runs in.
pub struct Examined {
    pub parts: Vec<(PatternRecord, u64)>,
    pub covers: bool,
}

/// Is `rec` a compound? Its parts, ready to store, when it is (see the module notes).
pub fn examine(rec: &PatternRecord) -> Result<Option<Examined>, String> {
    let Some(parts) = split_compound(rec)? else {
        return Ok(None);
    };
    let recs = part_records(rec, &parts, rec.provenance.clone(), rec.discovered_by)?;
    let mut grouped: Vec<(PatternRecord, u64)> = Vec::new();
    for r in recs {
        let r = prepare(r).map_err(|e| format!("a part did not verify: {e}"))?;
        match grouped.iter_mut().find(|g| g.0.id == r.id) {
            Some(g) => g.1 += 1,
            None => grouped.push((r, 1)),
        }
    }
    let covers = grouped.iter().all(|(p, _)| covers(p, rec));
    Ok(Some(Examined {
        parts: grouped,
        covers,
    }))
}

/// Does `part` run in every rule `whole` runs in?
fn covers(part: &PatternRecord, whole: &PatternRecord) -> bool {
    let rules_ok = std::iter::once(&whole.rule)
        .chain(&whole.rules)
        .all(|r| part.works_in(r));
    match (&part.range, &whole.range) {
        (_, None) => rules_ok,
        (None, Some(_)) => false,
        (Some(p), Some(w)) => rules_ok && range_holds(p, w),
    }
}

/// Does range `p` hold every rule of range `w`?
fn range_holds(p: &RuleRange, w: &RuleRange) -> bool {
    p.bits == w.bits
        && p.birth_min & !w.birth_min == 0
        && w.birth_max & !p.birth_max == 0
        && p.survive_min & !w.survive_min == 0
        && w.survive_max & !p.survive_max == 0
        && p.states_min <= w.states_min
        && w.states_max <= p.states_max
}

fn curated(lib: &Library, rec: &PatternRecord) -> Result<bool, String> {
    Ok(rec.name.is_some()
        || rec.rating.is_some()
        || !rec.notes.is_empty()
        || !rec.tags.is_empty()
        || rec.state != CurationState::New
        || lib.has_curation_rows(&rec.id)?)
}

impl Strategy for CompoundsRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        self.p = match checkpoint {
            Some(b) => serde_json::from_slice(b)
                .map_err(|e| format!("job {}: bad checkpoint: {e}", cx.job))?,
            None => {
                let mut p: CompoundsProgress = typed(&cx.row.progress);
                p.apply = self.spec.apply;
                if p.total == 0 {
                    p.total = cx.lib.info()?.patterns as u64;
                }
                p
            }
        };
        self.p.job = cx.job;
        self.report = report_path(cx.lib, cx.job, &self.spec);
        self.p.report = self.report.display().to_string();
        // The report holds what the checkpoint counts: cut back what a lost step wrote.
        if let Some(dir) = self.report.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        }
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&self.report)
            .map_err(|e| format!("cannot open {}: {e}", self.report.display()))?;
        f.set_len(self.p.report_bytes)
            .map_err(|e| format!("cannot cut {} back: {e}", self.report.display()))?;
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
        if self.p.finished {
            return Ok(Step::Done);
        }
        let room = cx
            .items
            .map(|n| n.saturating_sub(self.session) as usize)
            .unwrap_or(usize::MAX)
            .clamp(1, PAGE);
        let page = cx.lib.pattern_page(&self.p.after, room)?;
        if page.is_empty() {
            self.p.finished = true;
            return Ok(Step::Done);
        }
        let pool = self.pool.as_ref().ok_or("the run was not restored")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let done = super::prefix_until(pool, 0..page.len(), deadline, &cx.stop, |i| {
            if page[i].tags.iter().any(|t| t == COMPOUND_TAG) {
                return Ok(None);
            }
            examine(&page[i])
        });
        let mut lines = String::new();
        let mut new_here: HashSet<String> = HashSet::new();
        for (rec, result) in page.iter().zip(done) {
            self.p.checked += 1;
            self.session += 1;
            self.p.after = rec.id.clone();
            let tagged = rec.tags.iter().any(|t| t == COMPOUND_TAG);
            let (action, why, ex) = match result {
                Ok(None) if tagged => {
                    self.p.already += 1;
                    continue;
                }
                Ok(None) => continue,
                Ok(Some(ex)) => {
                    let cur = curated(cx.lib, rec)?;
                    let remove = !cur && ex.covers;
                    let why = if remove {
                        "independent parts, uncurated; each part runs in every rule it runs in"
                            .to_string()
                    } else if cur {
                        "curated (name, rating, notes, tags, state, Quick Start entry or ratings): kept for a person to decide".to_string()
                    } else {
                        "a part does not run in every rule of its rule range (there it may be one object): kept".to_string()
                    };
                    let action = match (self.spec.apply, remove) {
                        (true, true) => "remove",
                        (true, false) => "keep",
                        (false, true) => "would remove",
                        (false, false) => "would keep",
                    };
                    if remove {
                        self.p.removed += 1;
                    } else {
                        self.p.kept += 1;
                    }
                    (action.to_string(), why, Some(ex))
                }
                Err(e) => {
                    self.p.failed += 1;
                    ("failed".to_string(), e, None)
                }
            };
            self.p.compounds += 1;
            *self
                .p
                .by_class
                .entry(rec.class.key().to_string())
                .or_default() += 1;
            let tiling = short_tiling_key(&rec.tiling);
            *self.p.by_tiling.entry(tiling.clone()).or_default() += 1;
            let mut parts = Vec::new();
            if let Some(ex) = &ex {
                for (p, copies) in &ex.parts {
                    let stored = new_here.contains(&p.id) || cx.lib.contains(&p.id)?;
                    // A stored part goes by its stored name.
                    let label = match stored && !new_here.contains(&p.id) {
                        true => cx.lib.get(&p.id).map(|r| r.label()).unwrap_or(p.label()),
                        false => p.label(),
                    };
                    if stored {
                        self.p.parts_stored += 1;
                    } else {
                        self.p.parts_new += 1;
                        new_here.insert(p.id.clone());
                    }
                    parts.push(PartRow {
                        id: p.id.clone(),
                        label,
                        class: p.class.key().into(),
                        period: p.period,
                        cells: p.cells,
                        copies: *copies,
                        stored,
                    });
                }
            }
            let row = CompoundRow {
                id: rec.id.clone(),
                label: rec.label(),
                class: rec.class.key().into(),
                tiling,
                rule: rec.rule.clone(),
                period: rec.period,
                cells: rec.cells,
                seen: rec.seen,
                action: action.clone(),
                why,
                parts,
            };
            // The whole record as it was, with its sightings and history.
            let mut line = serde_json::to_value(&row).map_err(|e| e.to_string())?;
            line["record"] = serde_json::to_value(rec).map_err(|e| e.to_string())?;
            line["sightings"] =
                serde_json::to_value(cx.lib.sightings(&rec.id)?).map_err(|e| e.to_string())?;
            line["provenance"] = serde_json::to_value(cx.lib.provenance("pattern", &rec.id)?)
                .map_err(|e| e.to_string())?;
            lines.push_str(&line.to_string());
            lines.push('\n');
            if self.spec.apply
                && let Some(ex) = ex
            {
                let note = format!(
                    "Compound: {} (jetcarp library compounds, job {}).",
                    row.parts
                        .iter()
                        .map(|p| format!(
                            "{} x{} ({})",
                            &p.id[..8.min(p.id.len())],
                            p.copies,
                            p.label
                        ))
                        .collect::<Vec<_>>()
                        .join(" + "),
                    self.p.job
                );
                sink.compounds.push(CompoundSplit {
                    compound: rec.id.clone(),
                    parts: ex.parts,
                    remove: action == "remove",
                    note,
                });
            }
            if self.p.examples.len() < LIST {
                self.p.examples.push(row);
            }
        }
        if !lines.is_empty() {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&self.report)
                .map_err(|e| format!("cannot write {}: {e}", self.report.display()))?;
            f.write_all(lines.as_bytes())
                .and_then(|_| f.sync_data())
                .map_err(|e| format!("cannot write {}: {e}", self.report.display()))?;
            self.p.report_bytes += lines.len() as u64;
        }
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
