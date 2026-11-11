//! Evolutionary glider searches (`jobs.kind` `evolve`), the first strategy run in steps: one
//! generation each ([`jetcarp_core::evolve::Evolution`]), checkpointed after each, so a stopped
//! search resumes from its last generation and ends exactly as an uninterrupted one (and as
//! [`jetcarp_core::evolve::evolve`] with the same spec). At the end the champions are verified, the
//! verified gliders' records (`crate::evolve::glider_records`, the gliders `jetcarp search evolve`
//! stores) go to the catalog with the final write, credited to the job, and the run's files are
//! written to its folder, when it has one.
//!
//! The progress is what the MCP tool `evolve_start` has always written: per generation
//! `generation` (the last one run, from 0), `generations`, `best`, `best_ever`, `median`,
//! `at_ceiling`, `distinct`, `elapsed_s`, and `line`; at the end the generations run,
//! `best_ever`, the first eight `champions` (rule and fitness), the count of verified `gliders`,
//! the `movers` stored, and the `folder`.

use super::{RunCx, Sink, Step, Strategy, StrategyKind};
use super::{base_summary, eta, short_list};
use crate::dashboard::JobSummary;
use crate::evolve::{FoundMover, glider_records, not_cataloged};
use jetcarp_core::evolve::{Evolution, EvolveCheckpoint, EvolveReport, EvolveSpec, GenStats};
use jetcarp_core::library::store::JobRow;
use jetcarp_core::library::{AddOutcome, JobStatus, Library};
use jetcarp_core::pattern::tiling_display_name;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct EvolveKind;
pub(super) static KIND: EvolveKind = EvolveKind;

pub const KIND_NAME: &str = "evolve";

fn yes() -> bool {
    true
}
fn track_default() -> u64 {
    20_000
}

/// An evolve job: the search, and what to do with its result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolveJobSpec {
    #[serde(flatten)]
    pub search: EvolveSpec,
    /// Store the verified gliders in the catalog.
    #[serde(default = "yes")]
    pub store: bool,
    /// Generations the mover tracker may run on aperiodic patches.
    #[serde(default = "track_default")]
    pub track_gens: u64,
    /// Write `fitness.csv`, `champions.json`, `gliders.json`, `run.json`, and `movers.json`
    /// here at the end (`evolve::write_outputs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl EvolveJobSpec {
    pub fn new(search: EvolveSpec) -> EvolveJobSpec {
        EvolveJobSpec {
            search,
            store: true,
            track_gens: track_default(),
            folder: None,
        }
    }
}

/// Store an evolve job (status `paused` until a runner starts it).
pub fn start(lib: &Library, spec: &EvolveJobSpec) -> Result<i64, String> {
    let s = &spec.search;
    if !(2..=jetcarp_core::evolve::priority::MAX_STATES).contains(&s.states) {
        return Err(format!(
            "states must be 2 to {}",
            jetcarp_core::evolve::priority::MAX_STATES
        ));
    }
    if s.pop < 4 {
        return Err("population must be at least 4".into());
    }
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job(KIND_NAME, &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

impl StrategyKind for EvolveKind {
    fn kind(&self) -> &'static str {
        KIND_NAME
    }
    fn label(&self) -> &'static str {
        "Evolutionary search"
    }
    fn deterministic(&self) -> bool {
        true
    }
    fn units(&self) -> Option<&'static str> {
        Some("generations")
    }
    fn create(&self, lib: &Library, spec: &Value) -> Result<i64, String> {
        let spec: EvolveJobSpec =
            serde_json::from_value(spec.clone()).map_err(|e| format!("evolve spec: {e}"))?;
        start(lib, &spec)
    }
    fn build(&self, _row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Ok(Box::new(EvolveRun::default()))
    }
    /// From the progress the search writes (`generation`, `generations`, `best`, `best_ever`,
    /// `gliders`, `champions`).
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = base_summary(row, self);
        let spec: Option<EvolveSpec> = serde_json::from_value(row.spec.clone()).ok();
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
}

/// Called with each generation's numbers.
pub type GenHook = Arc<dyn Fn(&GenStats) + Send + Sync>;

/// What a finished search produced.
#[derive(Clone, Debug)]
pub struct EvolveResult {
    pub report: EvolveReport,
    /// The verified gliders' fate in the catalog.
    pub found: Vec<FoundMover>,
    /// The files written to the job's folder.
    pub files: Vec<PathBuf>,
}

/// An evolve run: a generation per step, on a pool of the runner's threads (or the spec's
/// `threads`; the results do not depend on them).
#[derive(Default)]
pub struct EvolveRun {
    /// Called with each generation's numbers.
    pub on_generation: Option<GenHook>,
    /// Where the finished search's report goes, for the caller.
    pub result: Option<Arc<Mutex<Option<EvolveResult>>>>,
    spec: Option<EvolveJobSpec>,
    evo: Option<Evolution>,
    pool: Option<rayon::ThreadPool>,
    job: i64,
    session: u64,
    done: Option<Finished>,
}

/// A finished search: its report, the verified gliders' fate, the files written, and the
/// records in the sink (glider index, id) until they are stored.
struct Finished {
    report: EvolveReport,
    found: Vec<FoundMover>,
    files: Vec<PathBuf>,
    owners: Vec<(usize, String)>,
    stored: bool,
}

impl EvolveRun {
    fn in_pool<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        match &self.pool {
            Some(p) => p.install(f),
            None => f(),
        }
    }
}

fn line_running(job: i64, g: &GenStats, total: usize) -> String {
    format!(
        "evolve {job}: generation {}/{total}, best {:.2}, best ever {:.2}, {:.0} s",
        g.generation, g.best, g.best_ever, g.elapsed_s
    )
}

impl Strategy for EvolveRun {
    fn restore(&mut self, cx: &RunCx<'_>, checkpoint: Option<&[u8]>) -> Result<(), String> {
        let spec: EvolveJobSpec = serde_json::from_value(cx.row.spec.clone())
            .map_err(|e| format!("job {}: {e}", cx.job))?;
        let threads = if cx.threads > 0 {
            Some(cx.threads)
        } else {
            spec.search.threads
        };
        self.pool = match threads {
            Some(t) => Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(t.max(1))
                    .build()
                    .map_err(|e| e.to_string())?,
            ),
            None => None,
        };
        let cp: Option<EvolveCheckpoint> = match checkpoint {
            Some(b) => Some(
                serde_json::from_slice(b).map_err(|e| format!("job {} checkpoint: {e}", cx.job))?,
            ),
            None => None,
        };
        let search = spec.search.clone();
        let evo = self.in_pool(|| match &cp {
            Some(cp) => Evolution::resume(&search, cp),
            None => Evolution::new(&search),
        })?;
        self.job = cx.job;
        self.spec = Some(spec);
        self.evo = Some(evo);
        Ok(())
    }

    fn step(&mut self, _cx: &RunCx<'_>, _sink: &mut Sink) -> Result<Step, String> {
        let mut evo = self.evo.take().ok_or("the search was not restored")?;
        let hook = self.on_generation.clone();
        let before = evo.generations().len();
        let r = self.in_pool(|| {
            evo.generation(&|g| {
                if let Some(h) = &hook {
                    h(g);
                }
                true
            })
        });
        self.session += (evo.generations().len() - before) as u64;
        self.evo = Some(evo);
        Ok(if r? { Step::More } else { Step::Done })
    }

    fn checkpoint(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.evo {
            Some(e) => serde_json::to_vec(&e.checkpoint())
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    fn progress(&self, status: JobStatus) -> Value {
        if let Some(d) = &self.done {
            return done_progress(self.job, self.spec.as_ref(), d);
        }
        let status = super::status_name(status);
        let (Some(evo), Some(spec)) = (&self.evo, &self.spec) else {
            return json!({"status": status, "generation": 0});
        };
        let total = spec.search.gens;
        match evo.generations().last() {
            Some(g) => json!({
                "status": status, "generation": g.generation, "generations": total,
                "best": g.best, "best_ever": g.best_ever, "median": g.median,
                "at_ceiling": g.at_ceiling, "distinct": g.distinct, "elapsed_s": g.elapsed_s,
                "line": line_running(self.job, g, total),
            }),
            None => json!({"status": status, "generation": 0, "generations": total}),
        }
    }

    fn items(&self) -> u64 {
        self.session
    }

    fn finish(&mut self, cx: &RunCx<'_>, sink: &mut Sink) -> Result<(), String> {
        let evo = self.evo.take().ok_or("the search was not restored")?;
        let spec = self.spec.clone().ok_or("the search was not restored")?;
        let report = self.in_pool(|| evo.finish())?;
        // The verified gliders' records go in with the final write, credited to the job; the
        // files report which are new to the catalog as it stands now (`stored` makes the
        // progress and the result say what the write did).
        let mut found = Vec::new();
        let mut owners = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (fi, (mut f, recs)) in glider_records(&report, spec.track_gens, spec.store)?
            .into_iter()
            .enumerate()
        {
            if spec.store {
                for r in recs {
                    if f.ids.contains(&r.id)
                        && seen.insert(r.id.clone())
                        && !cx.lib.contains(&r.id)?
                    {
                        f.added.push(r.id.clone());
                    }
                    owners.push((fi, r.id.clone()));
                    sink.patterns.push((r, 0));
                }
            }
            found.push(f);
        }
        let mut files = Vec::new();
        if let Some(dir) = &spec.folder {
            files = jetcarp_core::evolve::write_outputs(&report, dir)?;
            let fp = dir.join("movers.json");
            std::fs::write(
                &fp,
                serde_json::to_string_pretty(&found).map_err(|e| e.to_string())? + "\n",
            )
            .map_err(|e| format!("writing {}: {e}", fp.display()))?;
            files.push(fp);
        }
        sink.files.extend(files.iter().cloned());
        self.done = Some(Finished {
            report,
            found,
            files,
            owners,
            stored: false,
        });
        Ok(())
    }

    fn stored(&mut self, outcomes: &[Result<AddOutcome, String>]) {
        let Some(d) = self.done.as_mut().filter(|d| !d.stored) else {
            return;
        };
        d.stored = true;
        if !d.owners.is_empty() {
            for f in &mut d.found {
                f.added.clear();
            }
            for ((fi, id), o) in std::mem::take(&mut d.owners).into_iter().zip(outcomes) {
                let f = &mut d.found[fi];
                match o {
                    Ok(AddOutcome::Added) if f.ids.contains(&id) => f.added.push(id),
                    Ok(_) => {}
                    Err(e) => *f = not_cataloged(&f.rule, e),
                }
            }
        }
        if let Some(slot) = &self.result {
            *slot.lock().unwrap() = Some(EvolveResult {
                report: d.report.clone(),
                found: d.found.clone(),
                files: d.files.clone(),
            });
        }
    }
}

/// The progress of a finished search.
fn done_progress(job: i64, spec: Option<&EvolveJobSpec>, d: &Finished) -> Value {
    let (report, found) = (&d.report, &d.found);
    let folder = spec.and_then(|s| s.folder.as_ref());
    json!({
        "status": "done",
        "generation": report.generations.len(),
        "generations": spec.map(|s| s.search.gens),
        "best_ever": report.generations.last().map(|g| g.best_ever),
        "elapsed_s": report.elapsed_s,
        "champions": report.champions.iter().take(8).map(|c| json!({
            "rule": c.rule, "fitness": c.fitness,
        })).collect::<Vec<_>>(),
        "gliders": report.gliders.len(),
        "movers": found.iter().map(|m| json!({
            "kind": m.kind, "ids": m.ids, "added": m.added, "summary": m.summary,
        })).collect::<Vec<_>>(),
        "folder": folder.map(|d| d.display().to_string()).unwrap_or_default(),
        "line": format!("evolve {job}: {} generations, {} verified glider(s)",
            report.generations.len(), report.gliders.len()),
    })
}
