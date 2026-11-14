//! `search.*` on an explore job's run records (`searches/<uuid>/` beside the library: sealed
//! Parquet segments and the working database `search.sqlite`; `jetcarp_search::explore::runs`):
//! find runs (`search.runs`), a contact sheet of them (`search.sheet`), one run as a scene
//! (`search.run_scene`), try a configuration of one's own now (`search.try`), record a verdict on
//! runs (`search.review`, one at a time or a folder of rating files), fit a score model to those
//! verdicts (`search.refit`), recompute every score after the score changes or by a fitted model
//! (`search.rescore`), seal the newest runs into a segment (`search.seal`), and animated clips of
//! a sample for rating (`search.clips`). Each takes the job (default: the newest explore job of
//! the library) and reads its runs while it runs; rescoring and sealing wait for a stopped job.
//!
//! The reviews are the ratings the score was fitted to (the taste rounds' keep, maybe, and drop,
//! rated by Jimmie Rodgers and Emily Teich): clips go out, verdicts come back as rating files,
//! `search.refit` fits a model to them, and `search.rescore` applies one to every stored run.
//! Reviews are kept in the library (ratings of target kind `search_run`), so deleting a search's
//! folder keeps them.

use super::clip_ratings;
use super::library::catalog_err;
use crate::args;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::Features;
use jetcarp_core::explore::features::SignMeasures;
use jetcarp_core::explore::space::{Fam, sized_spec};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_search::explore::Db;
use jetcarp_search::explore::archive::{Eval, ScoreModel};
use jetcarp_search::explore::atlas;
use jetcarp_search::explore::breed::Parent;
use jetcarp_search::explore::job::{tiling_groups, try_one};
use jetcarp_search::explore::reviews;
use jetcarp_search::explore::runs::{RunFilter, Runs};
use jetcarp_search::explore::sheet::{self, RunRow};
use jetcarp_search::explore::store::{self, feat_from_blob};
use jetcarp_search::explore::{model, remeasure, runs, segments};
use jetcarp_search::jobs::{self, Lease};
use jetcarp_store::Library;
use jetcarp_store::library::JobRow;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

fn invalid(e: impl Into<String>) -> ApiError {
    ApiError::invalid(e)
}

/// An explore job of the catalog (`None`: the newest), with its working database's path.
pub fn explore_job(cx: &Ctx, job: Option<i64>) -> Result<(Library, JobRow, PathBuf), ApiError> {
    let lib = cx.library();
    let row = match job {
        Some(j) => lib.job(j).map_err(ApiError::not_found)?,
        None => lib
            .jobs()
            .map_err(catalog_err)?
            .into_iter()
            .rev()
            .find(|j| j.kind == jobs::explore::KIND_NAME)
            .ok_or_else(|| {
                ApiError::not_found("the catalog has no explore job").hint(
                    "start one: jetcarp search explore (search.start {\"strategy\": \"explore\"})",
                )
            })?,
    };
    let path = jobs::explore::db_path(&lib, &row).map_err(invalid)?;
    Ok((lib, row, path))
}

/// The job's runs for reading.
fn read_db(path: &std::path::Path) -> Result<Db, ApiError> {
    Runs::open_read(path)
        .map_err(|e| ApiError::not_found(e).hint("the job has not run yet: jobs.resume runs it"))
}

/// A stored run, as a list shows it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunLine {
    pub id: i64,
    /// 1 screened, 2 a confirmation run of `parent` on a bigger grid.
    pub stage: i64,
    pub parent: Option<i64>,
    pub tiling: String,
    pub family: String,
    pub rule: String,
    /// touch or edge.
    pub nb: String,
    /// The starting pattern, in words, and its kind.
    pub seed: String,
    pub seed_kind: String,
    pub cells: u32,
    pub gens: u32,
    /// How the candidate was made: random, mutate, seed, cover, named, confirm.
    pub origin: String,
    /// died, froze, cycle, or running.
    pub fate: String,
    /// The interest score, about 0 to 3 (higher is more interesting), to two places.
    pub score: f64,
    /// When it settled, its final period, late density and activity, movers, smoothness.
    pub settle: u32,
    pub period: u32,
    pub density: f32,
    pub activity: f32,
    pub movers: u32,
    pub smooth: f32,
    /// The Quick Start category it would go in.
    pub category: String,
    /// The measures asked for (`measures`), by name: null where a run has none (JACA's sign
    /// measures for other families, or for runs stored before them and not replayed since).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measures: Option<BTreeMap<String, Option<f64>>>,
}

/// The measures `search.runs` lists on request, in order.
pub const MEASURES: [&str; 26] = [
    "bin",
    "settle",
    "period",
    "density",
    "density_late",
    "activity_late",
    "activity_early",
    "pop_cv",
    "growth",
    "peak_density",
    "entropy",
    "bits",
    "same_nb",
    "smooth",
    "comps_per_live",
    "largest",
    "size_entropy",
    "patchiness",
    "travel",
    "movers",
    "mover_speed",
    "sign_pos",
    "sign_balance",
    "sign_sorted",
    "sign_trade",
    "sign_lost",
];

/// A stored run's measure by name (`None`: the run has none).
fn measure(r: &RunRow, f: &Features, name: &str) -> Option<f64> {
    let s = f.signs;
    let x = match name {
        "bin" => return Some(r.bin as f64),
        "settle" => return Some(f.settle as f64),
        "period" => return Some(f.period as f64),
        "movers" => return Some(f.movers as f64),
        "density" => f.density,
        "density_late" => f.density_late,
        "activity_late" => f.activity_late,
        "activity_early" => f.activity_early,
        "pop_cv" => f.pop_cv,
        "growth" => f.growth,
        "peak_density" => f.peak_density,
        "entropy" => f.entropy,
        "bits" => f.bits,
        "same_nb" => f.same_nb,
        "smooth" => f.smooth,
        "comps_per_live" => f.comps_per_live,
        "largest" => f.largest,
        "size_entropy" => f.size_entropy,
        "patchiness" => f.patchiness,
        "travel" => f.travel,
        "mover_speed" => f.mover_speed,
        "sign_pos" => s?.pos,
        "sign_balance" => s?.balance,
        "sign_sorted" => s?.sorted,
        "sign_trade" => s?.trade,
        "sign_lost" => s?.lost,
        _ => return None,
    };
    // + 0.0: no negative zero in the output.
    x.is_finite().then_some(x as f64 + 0.0)
}

/// The measures named (`all`, or names), checked.
fn measure_names(v: Option<&Value>) -> Result<Vec<String>, ApiError> {
    let names = super::search::texts(v, "measures", &[',', ' ', ';', '\t', '\n'])?;
    if names.iter().any(|n| n == "all") {
        return Ok(MEASURES.iter().map(|s| s.to_string()).collect());
    }
    for n in &names {
        if !MEASURES.contains(&n.as_str()) {
            return Err(invalid(format!("no measure '{n}'"))
                .hint(format!("measures: all, or {}", MEASURES.join(", "))));
        }
    }
    Ok(names)
}

impl RunLine {
    fn of(r: &RunRow) -> RunLine {
        let f = feat_from_blob(&r.feats);
        RunLine {
            id: r.id,
            stage: r.stage,
            parent: r.parent,
            tiling: r.cand.tiling.clone(),
            family: r.cand.fam.name().into(),
            rule: r.cand.rule.clone(),
            nb: if r.cand.nb == Nb::Edge {
                "edge"
            } else {
                "touch"
            }
            .into(),
            seed: r.cand.seed.describe(),
            seed_kind: r.seed_kind.clone(),
            cells: r.cand.cells,
            gens: r.cand.gens,
            origin: r.cand.origin.clone(),
            fate: r.fate.clone(),
            score: (r.score * 100.0).round() / 100.0,
            settle: f.settle,
            period: f.period,
            density: f.density_late,
            activity: f.activity_late,
            movers: f.movers,
            smooth: f.smooth,
            category: atlas::categorize(&r.fate, &f, r.cand.fam, r.cand.gens).into(),
            measures: None,
        }
    }

    /// The line with these measures.
    fn with(r: &RunRow, names: &[String]) -> RunLine {
        let mut l = RunLine::of(r);
        if !names.is_empty() {
            let f = feat_from_blob(&r.feats);
            l.measures = Some(
                names
                    .iter()
                    .map(|n| (n.clone(), measure(r, &f, n)))
                    .collect(),
            );
        }
        l
    }
}

/// The CSV columns of a run before its measures.
const CSV_HEAD: [&str; 15] = [
    "id",
    "stage",
    "parent",
    "family",
    "tiling",
    "nb",
    "rule",
    "seed",
    "seed_kind",
    "cells",
    "gens",
    "rng",
    "origin",
    "fate",
    "score",
];

/// A CSV field, quoted when it must be.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A run as a CSV line: [`CSV_HEAD`], then the measures (empty where it has none). The seed is
/// its JSON, so the line replays the run; the score is the stored one, in full.
fn csv_line(r: &RunRow, names: &[String]) -> String {
    let f = feat_from_blob(&r.feats);
    let mut v: Vec<String> = vec![
        r.id.to_string(),
        r.stage.to_string(),
        r.parent.map(|p| p.to_string()).unwrap_or_default(),
        r.cand.fam.name().into(),
        r.cand.tiling.clone(),
        if r.cand.nb == Nb::Edge {
            "edge"
        } else {
            "touch"
        }
        .into(),
        r.cand.rule.clone(),
        serde_json::to_string(&r.cand.seed).unwrap_or_default(),
        r.seed_kind.clone(),
        r.cand.cells.to_string(),
        r.cand.gens.to_string(),
        r.cand.rng.to_string(),
        r.cand.origin.clone(),
        r.fate.clone(),
        format!("{}", r.score as f32),
    ];
    for n in names {
        v.push(measure(r, &f, n).map_or(String::new(), |x| {
            if matches!(n.as_str(), "bin" | "settle" | "period" | "movers") {
                format!("{x}")
            } else {
                format!("{}", x as f32)
            }
        }));
    }
    v.iter().map(|x| csv_field(x)).collect::<Vec<_>>().join(",")
}

// ---------------------------------------------------------------------------------------------
// search.runs

/// Which runs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunsIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// Only this rule family (life_like, generations, ltl, jaca, cyclic, ...).
    #[serde(default)]
    pub family: Option<String>,
    /// Only this tiling: an id such as hex or k2-017.
    #[serde(default)]
    pub tiling: Option<String>,
    /// Only runs that ended this way: died, froze, cycle, running.
    #[serde(default)]
    pub fate: Option<String>,
    /// Only runs with at least this score.
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Only runs with at least this many moving objects.
    #[serde(default)]
    pub min_movers: Option<u32>,
    /// 1 screened (default), 2 confirmation runs on bigger grids.
    #[serde(default)]
    pub stage: Option<u8>,
    /// Only reviewed runs: any, keep, maybe, or drop.
    #[serde(default)]
    pub reviewed: Option<String>,
    /// These runs, instead of the filters.
    #[serde(default)]
    pub ids: Option<Vec<i64>>,
    /// score (default, best first), newest, or id.
    #[serde(default)]
    pub order: Option<String>,
    /// Most runs to return (default 40; 0: every run).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Measures to add: all, or names.
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub measures: Option<Value>,
    /// Only runs after this id, in id order.
    #[serde(default)]
    pub since: Option<i64>,
    /// Write them to this CSV file (limit 0: all).
    #[serde(default)]
    pub csv: Option<String>,
}

/// Runs found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunList {
    pub job: i64,
    /// The job's working database (its sealed segments are in `runs/` beside it).
    pub db: String,
    /// The runs (none when they went to a CSV file).
    pub runs: Vec<RunLine>,
    /// With `csv`: the file written, and the runs in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub written: Option<u64>,
    /// The measures asked for, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<String>,
}

impl Report for RunList {
    fn text(&self) -> String {
        let mut s = String::new();
        if let (Some(p), Some(n)) = (&self.csv, self.written) {
            return format!("{n} run(s) of explore job {} written to {p}\n", self.job);
        }
        if !self.measures.is_empty() {
            let _ = writeln!(
                s,
                "{:>9}  {:>5}  {:<7} {:<18} {}",
                "id",
                "score",
                "fate",
                "family",
                self.measures.join(" ")
            );
        }
        for r in &self.runs {
            match &r.measures {
                Some(m) if !self.measures.is_empty() => {
                    let vals: Vec<String> = self
                        .measures
                        .iter()
                        .map(|n| match m.get(n).copied().flatten() {
                            Some(x) if x.fract() == 0.0 && x.abs() < 1e9 => format!("{x}"),
                            Some(x) => format!("{x:.4}"),
                            None => "-".into(),
                        })
                        .collect();
                    let _ = writeln!(
                        s,
                        "{:>9}  {:>5.2}  {:<7} {:<18} {}",
                        r.id,
                        r.score,
                        r.fate,
                        r.family,
                        vals.join(" ")
                    );
                }
                _ => {
                    let _ = writeln!(
                        s,
                        "{:>9}  {:>5.2}  {:<7} {:<18} {:<16} {}",
                        r.id, r.score, r.fate, r.family, r.tiling, r.rule
                    );
                }
            }
        }
        let _ = writeln!(s, "{} run(s) of explore job {}", self.runs.len(), self.job);
        s
    }
}

impl RunsIn {
    fn filter(&self) -> Result<RunFilter, ApiError> {
        let family = match &self.family {
            Some(f) => {
                Some(store::fam_named(f).ok_or_else(|| invalid(format!("no rule family '{f}'")))?)
            }
            None => None,
        };
        Ok(RunFilter {
            ids: self.ids.clone(),
            family,
            tiling: self
                .tiling
                .as_deref()
                .map(jetcarp_core::explore::space::tiling_key),
            fate: self.fate.clone(),
            min_score: self.min_score,
            stage: self.stage,
            reviewed: self.reviewed.clone(),
            within: None,
            order: self
                .order
                .clone()
                .or_else(|| self.since.map(|_| "id".to_string())),
            after: self.since,
        })
    }

    /// The most runs to give (0: every run).
    fn most(&self) -> usize {
        match self.limit {
            Some(0) => usize::MAX,
            Some(n) => n,
            None => 40,
        }
    }
}

/// Runs read at a time when they go to a CSV file.
const CSV_CHUNK: usize = 20_000;

/// Write the runs `a` names as CSV (a header, then a line per run; `a.csv` is not read), a part
/// at a time, so every run of a large search can go out. Returns the job and the runs written.
pub fn runs_csv(cx: &Ctx, a: &RunsIn, w: &mut dyn std::io::Write) -> Result<(i64, u64), ApiError> {
    let (lib, row, path) = explore_job(cx, a.job)?;
    let c = read_db(&path)?;
    let names = measure_names(a.measures.as_ref())?;
    let mut f = a.filter()?;
    reviews::resolve(&lib, &c.uuid(), &mut f).map_err(invalid)?;
    let fetch = if a.min_movers.is_some() {
        a.most().saturating_mul(20)
    } else {
        a.most()
    };
    let ids = c.ids(&f, fetch).map_err(invalid)?;
    let io = |e: std::io::Error| ApiError::io(format!("writing the CSV: {e}"));
    let mut head: Vec<&str> = CSV_HEAD.to_vec();
    head.extend(names.iter().map(String::as_str));
    writeln!(w, "{}", head.join(",")).map_err(io)?;
    let mut n = 0u64;
    let most = a.most() as u64;
    for chunk in ids.chunks(CSV_CHUNK) {
        let mut rows = c.rows(chunk).map_err(ApiError::engine)?;
        with_replays(&path, &mut rows)?;
        for r in rows {
            if n >= most {
                break;
            }
            if a.min_movers
                .is_some_and(|m| feat_from_blob(&r.feats).movers < m)
            {
                continue;
            }
            writeln!(w, "{}", csv_line(&r, &names)).map_err(io)?;
            n += 1;
        }
    }
    w.flush().map_err(io)?;
    Ok((row.id, n))
}

/// Runs stored without sign measures take them from the search folder's cache of replays, when
/// they were replayed (`search.refit`, `search.rescore` with `replay`).
fn with_replays(db: &std::path::Path, rows: &mut [RunRow]) -> Result<(), ApiError> {
    let Some(dir) = db.parent().filter(|d| d.join(remeasure::FILE).is_file()) else {
        return Ok(());
    };
    let lacking: Vec<i64> = rows
        .iter()
        .filter(|r| r.cand.fam == Fam::Jaca && feat_from_blob(&r.feats).signs.is_none())
        .map(|r| r.id)
        .collect();
    if lacking.is_empty() {
        return Ok(());
    }
    let have = remeasure::Remeasured::open(dir)
        .and_then(|c| c.get(&lacking))
        .map_err(ApiError::engine)?;
    for r in rows.iter_mut() {
        if let Some(signs) = have.get(&r.id).and_then(|f| f.signs) {
            let mut f = feat_from_blob(&r.feats);
            if f.signs.is_none() {
                f.signs = Some(signs);
                r.feats = store::feat_blob(&f);
            }
        }
    }
    Ok(())
}

/// The runs a filter names, in order, at most `limit` (`reviewed` read from the library).
fn find(lib: &Library, c: &Db, f: &RunFilter, limit: usize) -> Result<Vec<RunRow>, ApiError> {
    let mut f = f.clone();
    reviews::resolve(lib, &c.uuid(), &mut f).map_err(invalid)?;
    let ids = c.ids(&f, limit).map_err(invalid)?;
    c.rows(&ids).map_err(ApiError::engine)
}

/// `search.runs`.
pub struct SearchRuns;
impl Op for SearchRuns {
    const NAME: &'static str = "search.runs";
    const SUMMARY: &'static str = "List an explore job's stored runs, best first, with their measures (all of them on request): filtered by family, tiling, fate, score, movers, stage, review, or id, or chosen by id; or write them to a CSV file, every run if asked.";
    const READ_ONLY: bool = false;
    type In = RunsIn;
    type Out = RunList;
    fn run(cx: &Ctx, a: RunsIn, _: &dyn Progress) -> Result<RunList, ApiError> {
        let names = measure_names(a.measures.as_ref())?;
        if let Some(p) = &a.csv {
            let f = cx.fs.writable(p)?;
            let file = std::fs::File::create(&f)
                .map_err(|e| ApiError::io(format!("{}: {e}", f.display())))?;
            let mut out = std::io::BufWriter::new(file);
            let (job, n) = runs_csv(cx, &a, &mut out)?;
            let (_, _, path) = explore_job(cx, Some(job))?;
            return Ok(RunList {
                job,
                db: path.display().to_string(),
                runs: vec![],
                csv: Some(f.display().to_string()),
                written: Some(n),
                measures: names,
            });
        }
        let (lib, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        let limit = a.most();
        // Movers are inside the features: read more, then filter.
        let fetch = if a.min_movers.is_some() {
            limit.saturating_mul(20)
        } else {
            limit
        };
        let mut rs = find(&lib, &c, &a.filter()?, fetch)?;
        with_replays(&path, &mut rs)?;
        if let Some(m) = a.min_movers {
            rs.retain(|r| feat_from_blob(&r.feats).movers >= m);
        }
        rs.truncate(limit);
        Ok(RunList {
            job: row.id,
            db: path.display().to_string(),
            runs: rs.iter().map(|r| RunLine::with(r, &names)).collect(),
            csv: None,
            written: None,
            measures: names,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.sheet

fn px_default() -> u32 {
    200
}
fn cols_default() -> u32 {
    8
}

/// Which runs, and how to draw them.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// Run ids to show.
    #[serde(default)]
    pub ids: Option<Vec<i64>>,
    /// Pixels per thumbnail (default 200).
    #[serde(default = "px_default")]
    pub px: u32,
    /// Thumbnails per row (default 8).
    #[serde(default = "cols_default")]
    pub cols: u32,
    /// Moments to show, as fractions of each run (default [0.25, 1]).
    #[serde(default)]
    pub fracs: Option<Vec<f64>>,
    /// Draw every run in this palette; several, separated by commas, compare them side by side (one row per run, at the first moment).
    #[serde(default)]
    pub palette: Option<String>,
    /// Most runs on the sheet (default 36).
    #[serde(default)]
    pub max: Option<usize>,
    /// Also write the PNG here.
    #[serde(default)]
    pub png: Option<String>,
}

/// A contact sheet of runs.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunSheet {
    pub job: i64,
    /// The runs drawn, in order.
    pub runs: Vec<i64>,
    pub width: u32,
    pub height: u32,
    /// Where the PNG was written.
    pub saved: Option<String>,
    /// The PNG.
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

impl Report for RunSheet {
    fn text(&self) -> String {
        match &self.saved {
            Some(p) => format!(
                "{} runs, {}x{}: {p}\n",
                self.runs.len(),
                self.width,
                self.height
            ),
            None => format!(
                "{} runs, {}x{} (not saved)\n",
                self.runs.len(),
                self.width,
                self.height
            ),
        }
    }
}

/// `search.sheet`.
pub struct SearchSheet;
impl Op for SearchSheet {
    const NAME: &'static str = "search.sheet";
    const SUMMARY: &'static str = "A contact sheet (PNG) of an explore job's runs at several moments, labelled with id, tiling, score, fate, rule, and measures, or one run per row in several palettes.";
    const READ_ONLY: bool = false;
    type In = SheetIn;
    type Out = RunSheet;
    fn run(cx: &Ctx, a: SheetIn, _: &dyn Progress) -> Result<RunSheet, ApiError> {
        let (lib, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        if !(16..=2000).contains(&a.px) || a.cols == 0 {
            return Err(invalid("px must be 16 to 2000 and cols at least 1"));
        }
        let max = a.max.unwrap_or(36).max(1);
        if a.ids.is_none() {
            return Err(invalid("give ids (search.runs finds them)"));
        }
        let q = RunFilter {
            ids: a.ids.clone(),
            ..Default::default()
        };
        let runs = find(&lib, &c, &q, max)?;
        if runs.is_empty() {
            return Err(ApiError::not_found("no runs to draw"));
        }
        let fracs = a.fracs.clone().unwrap_or_else(|| vec![0.25, 1.0]);
        if fracs.is_empty() || fracs.iter().any(|f| !(0.0..=1.0).contains(f)) {
            return Err(invalid("fracs are fractions of a run, 0 to 1"));
        }
        let names: Vec<&str> = a
            .palette
            .as_deref()
            .map_or(vec![], |p| p.split(',').map(str::trim).collect());
        // Checked here: a run drawn in an unknown palette would come out blank.
        for n in &names {
            jetcarp_core::render::builtin_palette(n).map_err(invalid)?;
        }
        let img = if names.len() > 1 {
            sheet::palette_sheet(&runs, &names, fracs[0], a.px)
        } else {
            sheet::sheet(&runs, &fracs, a.px, a.cols, a.palette.as_deref())
        };
        let bytes = crate::out::encode_png_rgba(&img, &[])?;
        let saved = match &a.png {
            Some(p) => {
                let f = cx.fs.writable(p)?;
                std::fs::write(&f, &bytes)
                    .map_err(|e| ApiError::io(format!("writing {}: {e}", f.display())))?;
                Some(f.display().to_string())
            }
            None => None,
        };
        Ok(RunSheet {
            job: row.id,
            runs: runs.iter().map(|r| r.id).collect(),
            width: img.width,
            height: img.height,
            saved,
            bytes,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.run_scene

/// One run.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// The run's id.
    pub run: i64,
    /// Also save the scene here (JSON).
    #[serde(default)]
    pub save: Option<String>,
}

/// A run as a scene.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunScene {
    pub job: i64,
    pub run: RunLine,
    /// The scene that replays the run (world.open, `jetcarp run`, the app).
    #[schemars(with = "Value")]
    pub scene: Value,
    pub saved: Option<String>,
}

impl Report for RunScene {
    fn text(&self) -> String {
        match &self.saved {
            Some(p) => format!("run {} saved as a scene: {p}\n", self.run.id),
            None => serde_json::to_string_pretty(&self.scene).unwrap_or_default() + "\n",
        }
    }
}

/// `search.run_scene`.
pub struct SearchRunScene;
impl Op for SearchRunScene {
    const NAME: &'static str = "search.run_scene";
    const SUMMARY: &'static str = "An explore job's stored run as scene JSON that replays it exactly (to open in a world, the app, or `jetcarp run`), optionally saved to a file.";
    const READ_ONLY: bool = false;
    type In = RunIn;
    type Out = RunScene;
    fn run(cx: &Ctx, a: RunIn, _: &dyn Progress) -> Result<RunScene, ApiError> {
        let (_, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        let r = c
            .rows(&[a.run])
            .map_err(ApiError::engine)?
            .pop()
            .ok_or_else(|| ApiError::not_found(format!("no run {} in job {}", a.run, row.id)))?;
        let (spec, _) = sized_spec(&r.cand.tiling, r.cand.cells).map_err(ApiError::engine)?;
        let mut scene = r.cand.scene(&spec);
        scene.name = format!("explore {} run {}: {}", row.id, r.id, r.cand.rule);
        let v = serde_json::to_value(&scene).map_err(|e| ApiError::engine(e.to_string()))?;
        let saved = match &a.save {
            Some(p) => {
                let f = cx.fs.writable(p)?;
                let text = serde_json::to_string_pretty(&scene)
                    .map_err(|e| ApiError::engine(e.to_string()))?;
                std::fs::write(&f, text + "\n")
                    .map_err(|e| ApiError::io(format!("writing {}: {e}", f.display())))?;
                Some(f.display().to_string())
            }
            None => None,
        };
        Ok(RunScene {
            job: row.id,
            run: RunLine::of(&r),
            scene: v,
            saved,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.try

/// A configuration to try.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TryIn {
    /// Tiling id: square, hex, trihex, spectre, k2-001, ih28, ...
    pub tiling: String,
    /// Rule notation.
    pub rule: String,
    /// The starting pattern: a seed shorthand ("random:0.3", "pattern:blobs:0.4:by_region", "pattern:symmetric:0.5") or object (default: the rule's own).
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub seed: Option<Value>,
    /// touch or edge (default: touch, or what the notation says).
    #[serde(default)]
    pub nb: Option<String>,
    /// About this many cells (default 16,000).
    #[serde(default)]
    pub cells: Option<u32>,
    /// Generations (default 1024).
    #[serde(default)]
    pub gens: Option<u32>,
    /// The random seed (default 1).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub rng: Option<u64>,
    /// Picture size in pixels (default 420).
    #[serde(default)]
    pub px: Option<u32>,
    /// Also write the picture (the run's end) here.
    #[serde(default)]
    pub png: Option<String>,
}

/// What a configuration did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Tried {
    pub family: String,
    pub fate: String,
    pub score: f64,
    pub category: String,
    pub settle: u32,
    pub period: u32,
    pub density: f32,
    pub activity: f32,
    pub movers: u32,
    pub mover_speed: f32,
    pub smooth: f32,
    pub entropy: f32,
    /// The scene that replays it.
    #[schemars(with = "Value")]
    pub scene: Value,
    pub saved: Option<String>,
    /// A picture of its end (PNG).
    #[serde(skip)]
    pub png: Vec<u8>,
}

impl Report for Tried {
    fn text(&self) -> String {
        let mut s = format!(
            "{}: {} at generation {}{}, score {:.2} ({}); density {:.3}, activity {:.4}, movers {}, smooth {:.2}\n",
            self.family,
            self.fate,
            self.settle,
            if self.period > 1 {
                format!(", period {}", self.period)
            } else {
                String::new()
            },
            self.score,
            self.category,
            self.density,
            self.activity,
            self.movers,
            self.smooth
        );
        if let Some(p) = &self.saved {
            let _ = writeln!(s, "picture: {p}");
        }
        s
    }
}

/// `search.try`.
pub struct SearchTry;
impl Op for SearchTry {
    const NAME: &'static str = "search.try";
    const SUMMARY: &'static str = "Run one configuration of your own now on the CPU, measured as the explore search measures its runs: fate, measures, score, a picture, and its scene.";
    const READ_ONLY: bool = false;
    type In = TryIn;
    type Out = Tried;
    fn run(cx: &Ctx, a: TryIn, _: &dyn Progress) -> Result<Tried, ApiError> {
        let (rule, nb0) = Rule::parse(&a.rule).map_err(invalid)?;
        let fam = Fam::of(&rule);
        let nb = match a.nb.as_deref() {
            Some(o) => args::nb_word(o)
                .ok_or_else(|| invalid(format!("nb must be touch or edge, got {o}")))?,
            None => nb0.unwrap_or(Nb::Touch),
        };
        let cells = a.cells.unwrap_or(16_000);
        let gens = a.gens.unwrap_or(1024);
        let l = &cx.limits;
        if cells as u64 > l.max_cells || gens as u64 > l.max_gens {
            return Err(ApiError::limit(format!(
                "{cells} cells for {gens} generations is over this front end's limits"
            )));
        }
        if !(100..=4_000_000).contains(&cells) || gens == 0 {
            return Err(invalid(
                "cells must be 100 to 4,000,000 and gens at least 1",
            ));
        }
        if cells as u64 * gens as u64 > l.max_work {
            return Err(ApiError::limit("that run is too long for one call"));
        }
        let key = jetcarp_core::explore::space::tiling_key(&a.tiling);
        let (spec, topo) = sized_spec(&key, cells).map_err(invalid)?;
        let seed = match &a.seed {
            Some(v) if !v.is_null() => args::parse_seed(v).map_err(invalid)?,
            _ => jetcarp_core::world::default_seed(&rule, topo.len()),
        };
        let cand = Cand {
            tiling: key,
            nb,
            fam,
            rule: rule.notation(),
            seed,
            cells,
            gens,
            steps: 1,
            rng: a.rng.unwrap_or(1),
            origin: "try".into(),
        };
        let ev = try_one(cand.clone()).map_err(ApiError::engine)?;
        let px = a.px.unwrap_or(420).clamp(32, 2000);
        let img = sheet::thumb(&cand, 1.0, px, None)
            .ok_or_else(|| ApiError::engine("could not render the run"))?;
        let png = crate::out::encode_png_rgba(&img, &[])?;
        let saved = match &a.png {
            Some(p) => {
                let f = cx.fs.writable(p)?;
                std::fs::write(&f, &png)
                    .map_err(|e| ApiError::io(format!("writing {}: {e}", f.display())))?;
                Some(f.display().to_string())
            }
            None => None,
        };
        let f = &ev.feat;
        let fate = store::fate_name(ev.fate).to_string();
        Ok(Tried {
            family: fam.name().into(),
            category: atlas::categorize(&fate, f, fam, gens).into(),
            fate,
            score: (ev.score as f64 * 1000.0).round() / 1000.0,
            settle: f.settle,
            period: f.period,
            density: f.density_late,
            activity: f.activity_late,
            movers: f.movers,
            mover_speed: f.mover_speed,
            smooth: f.smooth,
            entropy: f.entropy,
            scene: serde_json::to_value(cand.scene(&spec))
                .map_err(|e| ApiError::engine(e.to_string()))?,
            saved,
            png,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.review

/// A verdict on a run, or a folder of rating files.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// The run reviewed.
    #[serde(default)]
    pub run: Option<i64>,
    /// keep, maybe, or drop.
    #[serde(default)]
    pub verdict: Option<String>,
    /// What was seen, in a sentence.
    #[serde(default)]
    pub note: Option<String>,
    /// Who judged: a name, `user`, or `model:<name>` (default: a ratings file's own rater, else the publisher's name when one is set, as `pack.key` sets it, else `user`). Given, it overrides a ratings file's.
    #[serde(default)]
    pub rater: Option<String>,
    /// Instead of one run: a ratings file as the clip-rating page saves it (`jetcarp-ratings 1`: its verdicts become reviews, its stars stars), or a folder of rating files (`<run id>.json` holding {"verdict": "keep", "ts": ...}); ratings already recorded from it are skipped.
    #[serde(default)]
    pub from: Option<String>,
}

/// Reviews recorded.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Reviewed {
    pub job: i64,
    pub recorded: u64,
    /// With `from`: rating files read, for runs not in this search, unreadable, and already recorded.
    pub files: u64,
    pub unknown_runs: u64,
    pub unreadable: u64,
    pub already: u64,
    /// With a ratings file: the stars recorded, its rater as recorded, and its note.
    pub stars: u64,
    pub rater: Option<String>,
    pub note: Option<String>,
    /// The job's reviews now, per verdict.
    pub counts: Vec<(String, u64)>,
    /// The job's stars now.
    pub starred: u64,
}

impl Report for Reviewed {
    fn text(&self) -> String {
        let mut s = format!("recorded {} review(s)", self.recorded);
        if let Some(r) = &self.rater {
            let _ = write!(
                s,
                " and {} star(s) by {r} ({} for runs not in job {}, {} already recorded)",
                self.stars, self.unknown_runs, self.job, self.already
            );
        } else if self.files > 0 {
            let _ = write!(
                s,
                " from {} file(s) ({} for runs not in job {}, {} unreadable, {} already recorded)",
                self.files, self.unknown_runs, self.job, self.unreadable, self.already
            );
        }
        let mut c: Vec<String> = self
            .counts
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        if self.starred > 0 {
            c.push(format!("\u{2605} {}", self.starred));
        }
        let _ = writeln!(s, "; job {} has {}", self.job, c.join(", "));
        if let Some(n) = &self.note {
            let _ = writeln!(s, "note: {n}");
        }
        s
    }
}

/// `search.review`.
pub struct SearchReview;
impl Op for SearchReview {
    const NAME: &'static str = "search.review";
    const SUMMARY: &'static str = "Record a verdict (keep, maybe, drop) on an explore job's run, or the ratings in a folder of rating files; reviews choose what is published and tune the score.";
    const READ_ONLY: bool = false;
    type In = ReviewIn;
    type Out = Reviewed;
    fn run(cx: &Ctx, a: ReviewIn, _: &dyn Progress) -> Result<Reviewed, ApiError> {
        let (lib, row, path) = explore_job(cx, a.job)?;
        if !path.is_file() {
            return Err(ApiError::not_found(format!(
                "job {} has no runs yet",
                row.id
            )));
        }
        let c = read_db(&path)?;
        let rater = a
            .rater
            .clone()
            .or_else(publisher_name)
            .unwrap_or_else(|| "user".into());
        let mut out = Reviewed {
            job: row.id,
            recorded: 0,
            files: 0,
            unknown_runs: 0,
            unreadable: 0,
            already: 0,
            stars: 0,
            rater: None,
            note: None,
            counts: vec![],
            starred: 0,
        };
        match (&a.from, a.run, &a.verdict) {
            (Some(from), None, None) if clip_ratings::is_ratings_file(&cx.fs.readable(from)?)? => {
                let f = cx.fs.readable(from)?;
                let default = publisher_name().unwrap_or_else(|| "user".into());
                let r = clip_ratings::import_file(&lib, &c, &f, a.rater.as_deref(), &default)?;
                out.recorded = r.recorded;
                out.files = 1;
                out.unknown_runs = r.unknown_runs;
                out.already = r.already;
                out.stars = r.stars;
                out.rater = Some(r.rater);
                out.note = r.note;
            }
            (Some(dir), None, None) => {
                let d = cx.fs.readable(dir)?;
                let r = reviews::import_ratings(&lib, &c, &d, &rater).map_err(ApiError::io)?;
                out.recorded = r.recorded;
                out.files = r.files;
                out.unknown_runs = r.unknown_runs;
                out.unreadable = r.unreadable;
                out.already = r.already;
            }
            (None, Some(run), Some(v)) => {
                let v = v.trim().to_ascii_lowercase();
                reviews::add_review(
                    &lib,
                    &c,
                    run,
                    &v,
                    a.note.as_deref().unwrap_or(""),
                    &rater,
                    "review",
                )
                .map_err(|e| {
                    if e.starts_with("no run") {
                        ApiError::not_found(e)
                    } else {
                        invalid(e)
                    }
                })?;
                out.recorded = 1;
            }
            _ => {
                return Err(invalid(
                    "give a `run` and its `verdict`, or a folder of rating files (`from`)",
                ));
            }
        }
        out.counts = reviews::review_counts(&lib, &c.uuid()).map_err(ApiError::engine)?;
        out.starred = lib.run_stars(&c.uuid()).map_err(ApiError::engine)?.len() as u64;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// search.rescore

/// The job to rescore.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RescoreIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// Score with this fitted model (a file's path, or the model); the job keeps it. `builtin`: back to the built-in score. Default: the job's score.
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub score: Option<Value>,
    /// First replay runs stored without measures the score reads (older JACA runs), caching them; slow for a large search.
    #[serde(default)]
    pub replay: Option<bool>,
    /// Replay on the GPU, when there is one.
    #[serde(default)]
    pub gpu: Option<bool>,
}

/// A backend as replays take it.
fn gpu_of(be: &mut Option<Box<dyn Backend>>) -> Option<&mut dyn Backend> {
    match be {
        Some(b) => Some(&mut **b),
        None => None,
    }
}

/// The GPU batch stepper for replays, when asked for and there is one (a note when there is none).
fn replay_backend(cx: &Ctx, gpu: Option<bool>, p: &dyn Progress) -> Option<Box<dyn Backend>> {
    if !gpu.unwrap_or(false) {
        return None;
    }
    match cx.platform.explore_backend() {
        Ok(b) => {
            p.log(&format!("replaying on {}", b.name()));
            Some(b)
        }
        Err(e) => {
            p.log(&format!("no GPU ({e}): replaying on the processor"));
            None
        }
    }
}

/// A score model from what `score` says: a file's path, the model itself, or `builtin` (none).
pub(crate) fn load_model(cx: &Ctx, v: &Value) -> Result<Option<ScoreModel>, ApiError> {
    match v {
        Value::Null => Ok(None),
        Value::String(t) if matches!(t.trim(), "builtin" | "built-in" | "none") => Ok(None),
        Value::String(t) => {
            let p = cx.fs.readable(t)?;
            ScoreModel::load(&p).map(Some).map_err(invalid)
        }
        Value::Object(_) => ScoreModel::from_value(v).map(Some).map_err(invalid),
        x => Err(invalid(format!(
            "score is a model file's path, a model, or builtin; got {x}"
        ))),
    }
}

/// Parents runs taken from another job are at most.
const MOST_PARENTS: usize = 500;

/// The parents a new search takes from explore job `job` (`parents_from`): its JACA runs with
/// these verdicts (any rater's; default keep and maybe), keeps first, or with none recorded its
/// best screened JACA runs; runs stored before sign measures are replayed for them.
pub(crate) fn parents_from(
    cx: &Ctx,
    job: i64,
    reviewed: Option<&str>,
    p: &dyn Progress,
) -> Result<Vec<Parent>, ApiError> {
    let want = super::search::texts(
        Some(&Value::String(reviewed.unwrap_or("keep,maybe").into())),
        "parents_reviewed",
        &[',', ' ', ';'],
    )?;
    if let Some(v) = want
        .iter()
        .find(|v| !reviews::VERDICTS.contains(&v.as_str()))
    {
        return Err(invalid(format!(
            "parents_reviewed takes keep, maybe and drop, not {v}"
        )));
    }
    let (lib, row, path) = explore_job(cx, Some(job))?;
    let c = read_db(&path)?;
    let mut why: HashMap<i64, String> = HashMap::new();
    for r in lib.run_reviews(&c.uuid()).map_err(ApiError::engine)? {
        if want.contains(&r.verdict) {
            let e = why.entry(r.run).or_insert(r.verdict.clone());
            if r.verdict == "keep" {
                *e = r.verdict;
            }
        }
    }
    let mut ids: Vec<i64> = why.keys().copied().collect();
    if ids.is_empty() {
        let f = RunFilter {
            family: Some(Fam::Jaca),
            ..Default::default()
        };
        ids = c.ids(&f, MOST_PARENTS).map_err(invalid)?;
    }
    let mut evs: Vec<(i64, Eval)> = c
        .get(&ids)
        .map_err(ApiError::engine)?
        .iter()
        .filter_map(|r| Some((r.id, store::eval_of(r)?)))
        .filter(|(_, ev)| ev.cand.fam == Fam::Jaca)
        .collect();
    let rank = |id: &i64| match why.get(id).map(String::as_str) {
        Some("keep") => 0,
        Some("maybe") => 1,
        Some(_) => 2,
        None => 3,
    };
    evs.sort_by_key(|(id, _)| (rank(id), *id));
    evs.truncate(MOST_PARENTS);
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    remeasure::fill(&dir, &mut evs, true, None, &|n, total| {
        p.report(n, Some(total), &format!("replayed {n} of {total} parents"));
        !p.cancelled()
    })
    .map_err(ApiError::engine)?;
    if evs.is_empty() {
        return Err(invalid(format!(
            "explore job {} has no JACA runs to take parents from",
            row.id
        )));
    }
    Ok(evs
        .iter()
        .map(|(id, ev)| {
            let w = why.get(id).map_or("best", String::as_str);
            Parent::of(row.id, *id, w, ev)
        })
        .collect())
}

/// The score model a job's spec holds (none: the built-in score).
fn spec_model(row: &JobRow) -> Result<Option<ScoreModel>, ApiError> {
    match row.spec.get("score") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => ScoreModel::from_value(v)
            .map(Some)
            .map_err(|e| invalid(format!("job {}: {e}", row.id))),
    }
}

/// What a score is called in reports.
fn model_name(m: Option<&ScoreModel>) -> String {
    match m {
        None => "the built-in score".into(),
        Some(m) => {
            let fam = m
                .family
                .as_deref()
                .map_or(String::new(), |f| format!(" (for {f} runs)"));
            match &m.name {
                Some(n) => format!("the score model '{n}'{fam}"),
                None => format!("a fitted score model{fam}"),
            }
        }
    }
}

/// What rescoring did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Rescored {
    pub job: i64,
    /// The score used: the built-in score or a fitted model.
    pub score: String,
    /// Runs replayed now for measures they lacked, and runs that took them from the cache.
    pub replayed: u64,
    pub cached: u64,
    /// Runs rescored, and those whose score changed.
    pub runs: u64,
    pub changed: u64,
    /// The archive rebuilt from the new scores: its elites and niches.
    pub elites: u64,
    pub niches: u64,
    /// Score sidecars written (`scores-<version>/`, one per sealed segment whose scores changed;
    /// segments are never rewritten), and the rescore's version (0: none needed).
    pub sidecars: u64,
    pub version: u32,
    pub seconds: f64,
}

impl Report for Rescored {
    fn text(&self) -> String {
        let side = if self.sidecars > 0 {
            format!(
                "; {} score sidecar(s) in scores-{}/",
                self.sidecars, self.version
            )
        } else {
            String::new()
        };
        let replay = if self.replayed > 0 {
            format!(
                "; {} run(s) replayed for the measures they lacked",
                self.replayed
            )
        } else {
            String::new()
        };
        format!(
            "rescored {} runs of explore job {} by {} ({} scores changed{side}{replay}); the archive now holds {} elites in {} niches ({:.1} s)\n",
            self.runs, self.job, self.score, self.changed, self.elites, self.niches, self.seconds
        )
    }
}

/// `search.rescore`.
pub struct SearchRescore;
impl Op for SearchRescore {
    const NAME: &'static str = "search.rescore";
    const SUMMARY: &'static str = "Recompute every stored score of a stopped explore job from its stored measures (after the score changes, or by a fitted score model from search.refit, which the job then keeps) and rebuild its archive from them; reads every run.";
    const READ_ONLY: bool = false;
    type In = RescoreIn;
    type Out = Rescored;
    fn run(cx: &Ctx, a: RescoreIn, p: &dyn Progress) -> Result<Rescored, ApiError> {
        let t0 = Instant::now();
        let (lib, row, path) = explore_job(cx, a.job)?;
        if !path.is_file() {
            return Err(ApiError::not_found(format!(
                "job {} has no runs yet",
                row.id
            )));
        }
        // Holding the job keeps a runner off it meanwhile.
        let lease = Lease::acquire(&lib, row.id).map_err(|e| {
            ApiError::conflict(format!("job {} is running: {e}", row.id))
                .hint("stop it first (jobs.stop), then rescore")
        })?;
        if cx.registry().is_running(&lib, row.id) {
            return Err(ApiError::conflict(format!(
                "job {} is running here",
                row.id
            )));
        }
        let groups = tiling_groups();
        let g = |k: &str| groups.get(k).copied().unwrap_or(0);
        let (model, spec) = match &a.score {
            None => (spec_model(&row)?, None),
            Some(v) => {
                let m = load_model(cx, v)?;
                let mut spec = row.spec.clone();
                if let Some(o) = spec.as_object_mut() {
                    match &m {
                        Some(m) => {
                            o.insert(
                                "score".into(),
                                serde_json::to_value(m)
                                    .map_err(|e| ApiError::engine(e.to_string()))?,
                            );
                        }
                        None => {
                            o.remove("score");
                        }
                    }
                }
                (m, Some(spec))
            }
        };
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        let mut filled = remeasure::Filled::default();
        let r = (|| -> Result<runs::Rescored, String> {
            if a.replay.unwrap_or(false) {
                let c = Runs::open_read(&path)?;
                let mut lacking: Vec<(i64, Eval)> = c.scan(
                    segments::EVAL | segments::cols(&[segments::col::ID, segments::col::STAGE]),
                    &|_, _, _| true,
                    &|b, out: &mut Vec<(i64, Eval)>| {
                        for i in 0..b.len {
                            if b.stage(i) != 1 {
                                continue;
                            }
                            let r = b.rec(i);
                            if let Some(ev) = store::eval_of(&r).filter(remeasure::lacks) {
                                out.push((r.id, ev));
                            }
                        }
                    },
                )?;
                drop(c);
                let mut be = replay_backend(cx, a.gpu, p);
                filled =
                    remeasure::fill(&dir, &mut lacking, true, gpu_of(&mut be), &|n, total| {
                        p.report(n, Some(total), &format!("replayed {n} of {total} runs"));
                        !p.cancelled()
                    })?;
            }
            let signs: HashMap<i64, SignMeasures> = if dir.join(remeasure::FILE).is_file() {
                remeasure::Remeasured::open(&dir)?
                    .all()?
                    .into_iter()
                    .filter_map(|(id, f)| Some((id, f.signs?)))
                    .collect()
            } else {
                HashMap::new()
            };
            let mut c = Runs::open(&path)?;
            if let Some(s) = &spec {
                c.set_spec(s)?;
            }
            c.rescore_with(model.as_ref(), &signs, &g, &|n| {
                p.report(n, None, &format!("rescored {n} runs"));
                !p.cancelled()
            })
        })();
        let r = r.map_err(ApiError::engine);
        if r.is_ok()
            && let Some(s) = &spec
            && let Err(e) = lib.set_job_spec(row.id, s)
        {
            lease.release(false);
            return Err(ApiError::engine(e));
        }
        lease.release(false);
        let r = r?;
        // The saved progress shows the rebuilt archive at the job's next run; say so now.
        let mut prog = row.progress.clone();
        if prog.is_object() {
            prog["elites"] = r.elites.into();
            prog["niches"] = r.niches.into();
            let _ = lib.update_job(row.id, None, Some(&prog), None);
        }
        Ok(Rescored {
            job: row.id,
            score: model_name(model.as_ref()),
            replayed: filled.replayed,
            cached: filled.cached,
            runs: r.runs,
            changed: r.changed,
            elites: r.elites,
            niches: r.niches,
            sidecars: r.sidecars,
            version: r.version,
            seconds: t0.elapsed().as_secs_f64(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.refit

/// Whose reviews to learn, and where the model goes.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefitIn {
    /// The explore jobs whose reviews to learn (default: the newest).
    #[serde(default)]
    pub jobs: Option<Vec<i64>>,
    /// Only this rater's reviews (default: every rater's).
    #[serde(default)]
    pub rater: Option<String>,
    /// Fit to this family's runs only; the model then scores only that family.
    #[serde(default)]
    pub family: Option<String>,
    /// Write the model here (JSON), for a search's or a rescore's `score`.
    #[serde(default)]
    pub out: Option<String>,
    /// Replay reviewed runs stored without measures the fit reads (older JACA runs), caching them (default true).
    #[serde(default)]
    pub replay: Option<bool>,
    /// Replay on the GPU, when there is one.
    #[serde(default)]
    pub gpu: Option<bool>,
}

/// A term's weight.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TermWeight {
    pub term: String,
    /// Its pull per standard deviation over the runs fitted (comparable across terms; positive favours keep).
    pub per_sd: f64,
    /// Its weight in raw units, as the model holds it.
    pub weight: f64,
}

/// A fitted score model and how well it did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Refitted {
    pub jobs: Vec<i64>,
    pub raters: Vec<String>,
    pub family: Option<String>,
    /// Reviews fitted per verdict, and the runs they cover.
    pub keep: u64,
    pub maybe: u64,
    pub drop: u64,
    pub runs: u64,
    /// Reviewed runs that lacked measures: replayed now, from the cache, replays that differ from the stored measures (0 when runs replay exactly), and replays that failed.
    pub replayed: u64,
    pub cached: u64,
    pub differ: u64,
    pub failed: u64,
    /// AUC of keep against drop on runs held out of the fit (5-fold, mean of 8 shuffles; 0.5 chance, 1 perfect), on the runs fitted, and the built-in score's on the same runs.
    pub auc: f64,
    pub auc_fitted: f64,
    pub auc_builtin: f64,
    /// The strongest terms, strongest first.
    pub strongest: Vec<TermWeight>,
    /// Where the model was written.
    pub saved: Option<String>,
    /// The model.
    #[schemars(with = "Value")]
    pub model: Value,
    pub note: Option<String>,
    pub seconds: f64,
}

impl Report for Refitted {
    fn text(&self) -> String {
        let mut s = format!(
            "fitted to {} review(s) of {} run(s) of explore job(s) {} by {} ({} keep, {} maybe, {} drop{})\n",
            self.keep + self.maybe + self.drop,
            self.runs,
            self.jobs
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            self.raters.join(", "),
            self.keep,
            self.maybe,
            self.drop,
            self.family
                .as_deref()
                .map_or(String::new(), |f| format!("; {f} runs only"))
        );
        if self.replayed + self.cached > 0 {
            let _ = writeln!(
                s,
                "{} run(s) lacked measures: {} replayed now, {} from the cache ({} differ from their stored measures, {} failed)",
                self.replayed + self.cached + self.failed,
                self.replayed,
                self.cached,
                self.differ,
                self.failed
            );
        }
        let _ = writeln!(
            s,
            "held-out AUC {:.3} (keep against drop; on the runs fitted {:.3}; the built-in score {:.3})",
            self.auc, self.auc_fitted, self.auc_builtin
        );
        let _ = writeln!(s, "strongest terms (pull per standard deviation):");
        for w in self.strongest.iter().take(10) {
            let _ = writeln!(s, "  {:<24} {:+.3}", w.term, w.per_sd);
        }
        if let Some(n) = &self.note {
            let _ = writeln!(s, "note: {n}");
        }
        match &self.saved {
            Some(p) => {
                let _ = writeln!(s, "model: {p}");
            }
            None => {
                let _ = writeln!(s, "(not saved: give out to keep it)");
            }
        }
        s
    }
}

/// `search.refit`.
pub struct SearchRefit;
impl Op for SearchRefit {
    const NAME: &'static str = "search.refit";
    const SUMMARY: &'static str = "Fit a score model to the keep, maybe and drop reviews of explore jobs' runs (a ridge logistic regression on the measures the score reads), report its held-out AUC and strongest terms, and save it for search.start's or search.rescore's `score`.";
    const READ_ONLY: bool = false;
    type In = RefitIn;
    type Out = Refitted;
    fn run(cx: &Ctx, a: RefitIn, p: &dyn Progress) -> Result<Refitted, ApiError> {
        let t0 = Instant::now();
        let family = match &a.family {
            Some(f) => {
                Some(store::fam_named(f).ok_or_else(|| invalid(format!("no rule family '{f}'")))?)
            }
            None => None,
        };
        let jobs = match &a.jobs {
            Some(v) if !v.is_empty() => v.clone(),
            _ => vec![explore_job(cx, None)?.1.id],
        };
        let mut rated: Vec<model::Rated> = Vec::new();
        let mut raters: BTreeSet<String> = BTreeSet::new();
        let mut searches = Vec::new();
        let mut runs = 0u64;
        let mut filled = remeasure::Filled::default();
        let mut be = replay_backend(cx, a.gpu, p);
        for &j in &jobs {
            let (lib, _, path) = explore_job(cx, Some(j))?;
            let c = read_db(&path)?;
            searches.push(c.uuid());
            // Each rater's latest verdict on a run.
            let mut latest: HashMap<(String, i64), (String, String)> = HashMap::new();
            for r in lib.run_reviews(&c.uuid()).map_err(ApiError::engine)? {
                // Verdicts only: a rating without one (a star) says nothing of keep or drop.
                if a.rater.as_deref().is_some_and(|x| x != r.rater)
                    || !reviews::VERDICTS.contains(&r.verdict.as_str())
                {
                    continue;
                }
                let e = latest
                    .entry((r.rater.clone(), r.run))
                    .or_insert_with(|| (r.created.clone(), r.verdict.clone()));
                if r.created >= e.0 {
                    *e = (r.created, r.verdict);
                }
            }
            let mut ids: Vec<i64> = latest.keys().map(|k| k.1).collect();
            ids.sort_unstable();
            ids.dedup();
            let mut evs: Vec<(i64, Eval)> = c
                .get(&ids)
                .map_err(ApiError::engine)?
                .iter()
                .filter_map(|r| Some((r.id, store::eval_of(r)?)))
                .filter(|(_, ev)| family.is_none_or(|f| f == ev.cand.fam))
                .filter(|(_, ev)| !ev.cand.fam.is_rows())
                .collect();
            let dir = path.parent().map(PathBuf::from).unwrap_or_default();
            let f = remeasure::fill(
                &dir,
                &mut evs,
                a.replay.unwrap_or(true),
                gpu_of(&mut be),
                &|n, total| {
                    p.report(n, Some(total), &format!("replayed {n} of {total} runs"));
                    !p.cancelled()
                },
            )
            .map_err(ApiError::engine)?;
            filled.replayed += f.replayed;
            filled.cached += f.cached;
            filled.differ += f.differ;
            filled.failed += f.failed;
            runs += evs.len() as u64;
            let by_id: HashMap<i64, Eval> = evs.into_iter().collect();
            let mut keys: Vec<&(String, i64)> = latest.keys().collect();
            keys.sort();
            for k in keys {
                let Some(ev) = by_id.get(&k.1) else { continue };
                raters.insert(k.0.clone());
                rated.push(model::Rated {
                    fate: ev.fate,
                    feat: ev.feat,
                    gens: ev.cand.gens,
                    fam: ev.cand.fam,
                    verdict: latest[k].1.clone(),
                });
            }
        }
        let mut m = model::fit(&rated, &model::FitOptions { family }).map_err(|e| {
            invalid(e).hint(
                "review more runs: jobs clips makes clips to rate, jobs review records the ratings",
            )
        })?;
        let raters: Vec<String> = raters.into_iter().collect();
        m.fitted_on.jobs = jobs.clone();
        m.fitted_on.searches = searches;
        m.fitted_on.raters = raters.clone();
        m.fitted_on.runs = runs;
        m.fitted_on.replayed = filled.replayed + filled.cached;
        m.name = Some(format!(
            "{}{} on explore job {}",
            raters.join(", "),
            family.map_or(String::new(), |f| format!(", {}", f.name())),
            jobs.iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let note = (family.is_none() && m.fitted_on.families.len() == 1).then(|| {
            format!(
                "every run fitted is a {f} run, but the model scores every family; fit with family {f} to keep the built-in score for the others",
                f = m.fitted_on.families[0]
            )
        });
        let text = serde_json::to_string_pretty(&m).map_err(|e| ApiError::engine(e.to_string()))?;
        let saved = match &a.out {
            Some(o) => {
                let f = cx.fs.writable(o)?;
                std::fs::write(&f, text + "\n")
                    .map_err(|e| ApiError::io(format!("writing {}: {e}", f.display())))?;
                Some(f.display().to_string())
            }
            None => None,
        };
        let strongest = m
            .strongest()
            .into_iter()
            .map(|(term, per_sd)| {
                let weight = m
                    .features
                    .iter()
                    .position(|t| *t == term)
                    .map_or(0.0, |i| m.weights[i]);
                TermWeight {
                    term,
                    per_sd: (per_sd * 1000.0).round() / 1000.0,
                    weight,
                }
            })
            .collect();
        let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
        Ok(Refitted {
            jobs,
            raters,
            family: family.map(|f| f.name().to_string()),
            keep: m.fitted_on.keep,
            maybe: m.fitted_on.maybe,
            drop: m.fitted_on.drop,
            runs,
            replayed: filled.replayed,
            cached: filled.cached,
            differ: filled.differ,
            failed: filled.failed,
            auc: r3(m.metrics.auc),
            auc_fitted: r3(m.metrics.auc_fitted),
            auc_builtin: r3(m.metrics.auc_builtin),
            strongest,
            saved,
            model: serde_json::to_value(&m).map_err(|e| ApiError::engine(e.to_string()))?,
            note,
            seconds: (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.seal

/// The job whose newest runs to seal.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SealIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
}

/// What sealing did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Sealed {
    pub job: i64,
    /// The segments written (relative to the search folder), and their runs.
    pub segments: Vec<String>,
    pub runs: u64,
    /// The search folder.
    pub folder: String,
    pub seconds: f64,
}

impl Report for Sealed {
    fn text(&self) -> String {
        if self.segments.is_empty() {
            return format!(
                "explore job {}: every run is sealed already ({})\n",
                self.job, self.folder
            );
        }
        format!(
            "sealed {} run(s) of explore job {} into {} in {} ({:.1} s)\n",
            self.runs,
            self.job,
            self.segments.join(", "),
            self.folder,
            self.seconds
        )
    }
}

/// `search.seal`.
pub struct SearchSeal;
impl Op for SearchSeal {
    const NAME: &'static str = "search.seal";
    const SUMMARY: &'static str = "Seal a stopped explore job's newest runs (those still in its working database) into Parquet segments, so its folder holds every run in Parquet (before sharing it); a search seals every million runs by itself.";
    const READ_ONLY: bool = false;
    type In = SealIn;
    type Out = Sealed;
    fn run(cx: &Ctx, a: SealIn, _: &dyn Progress) -> Result<Sealed, ApiError> {
        let t0 = Instant::now();
        let (lib, row, path) = explore_job(cx, a.job)?;
        if !path.is_file() {
            return Err(ApiError::not_found(format!(
                "job {} has no runs yet",
                row.id
            )));
        }
        let lease = Lease::acquire(&lib, row.id).map_err(|e| {
            ApiError::conflict(format!("job {} is running: {e}", row.id))
                .hint("stop it first (jobs.stop), then seal")
        })?;
        if cx.registry().is_running(&lib, row.id) {
            return Err(ApiError::conflict(format!(
                "job {} is running here",
                row.id
            )));
        }
        let r = Runs::open(&path).and_then(|mut c| c.seal_all());
        lease.release(false);
        let done = r.map_err(ApiError::engine)?;
        Ok(Sealed {
            job: row.id,
            runs: done.iter().map(|s| s.rows).sum(),
            segments: done.into_iter().map(|s| s.file).collect(),
            folder: path
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            seconds: t0.elapsed().as_secs_f64(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// search.clips

fn sample_default() -> usize {
    96
}
fn seed_default() -> u64 {
    1
}
fn clip_px_default() -> u32 {
    150
}
fn frames_default() -> u32 {
    28
}

/// Which runs to clip, and how.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipsIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// The folder the clips (`<run id>.gif`) and `manifest.json` go in.
    pub out: String,
    /// A stratified sample of this many screened runs, half strong and half random, spread over categories and families (default 96).
    #[serde(default = "sample_default")]
    pub sample: usize,
    /// The sample's random seed (default 1).
    #[serde(
        default = "seed_default",
        serialize_with = "jetcarp_core::json_int::uint::serialize",
        deserialize_with = "jetcarp_core::json_int::uint::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub seed: u64,
    /// These runs instead of a sample.
    #[serde(default)]
    pub ids: Option<Vec<i64>>,
    /// Instead of ids: a file of one `id[ bucket]` per line (the bucket goes in the manifest).
    #[serde(default)]
    pub ids_file: Option<String>,
    /// Pixels across (default 150).
    #[serde(default = "clip_px_default")]
    pub px: u32,
    /// Frames per clip, spread over the run (default 28).
    #[serde(default = "frames_default")]
    pub frames: u32,
    /// Also write `index.html`, a page for rating the clips by eye (Keep, Maybe, Drop, a star), which works offline and saves the ratings as one file for search.review.
    #[serde(default)]
    pub page: bool,
}

/// One clip.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Clip {
    pub id: i64,
    pub file: String,
    pub category: String,
    pub family: String,
    pub bucket: String,
    pub tiling: String,
    pub rule: String,
    pub fate: String,
    pub score: f64,
    pub seed: String,
}

/// Clips written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Clips {
    pub job: i64,
    pub folder: String,
    /// The manifest (`manifest.json`, the clips' list).
    pub manifest: String,
    /// The rating page (`index.html`), with `page`.
    pub page: Option<String>,
    pub clips: Vec<Clip>,
}

impl Report for Clips {
    fn text(&self) -> String {
        let mut s = format!("{} clips in {}\n", self.clips.len(), self.folder);
        if let Some(p) = &self.page {
            let _ = writeln!(s, "rating page: {p}");
        }
        s
    }
}

/// `search.clips`.
pub struct SearchClips;
impl Op for SearchClips {
    const NAME: &'static str = "search.clips";
    const SUMMARY: &'static str = "Animated clips (GIF) of a stratified sample of an explore job's runs, or of chosen runs, with a manifest, for rating by eye; the ratings come back through search.review.";
    const READ_ONLY: bool = false;
    type In = ClipsIn;
    type Out = Clips;
    fn run(cx: &Ctx, a: ClipsIn, p: &dyn Progress) -> Result<Clips, ApiError> {
        use rayon::prelude::*;
        let (_, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        if a.frames < 2 || !(16..=1000).contains(&a.px) {
            return Err(invalid("frames must be at least 2 and px 16 to 1000"));
        }
        let mut buckets = std::collections::HashMap::new();
        let ids: Option<Vec<i64>> = match (&a.ids, &a.ids_file) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(f)) => {
                let fp = cx.fs.readable(f)?;
                let text = std::fs::read_to_string(&fp)
                    .map_err(|e| ApiError::io(format!("{}: {e}", fp.display())))?;
                let mut list = Vec::new();
                for line in text.lines() {
                    let mut parts = line.split_whitespace();
                    let Some(id) = parts.next().and_then(|x| x.parse::<i64>().ok()) else {
                        continue;
                    };
                    list.push(id);
                    buckets.insert(id, parts.next().unwrap_or("").to_string());
                }
                Some(list)
            }
            (None, None) => None,
        };
        let picks = match &ids {
            Some(list) => atlas::runs_by_id(&c, list).map_err(ApiError::engine)?,
            None => atlas::taste_sample(&c, a.sample.max(1), a.seed).map_err(ApiError::engine)?,
        };
        let dir = cx.fs.writable_dir(&a.out)?;
        let total = picks.len() as u64;
        let done = std::sync::atomic::AtomicU64::new(0);
        let mut clips: Vec<Clip> = picks
            .par_iter()
            .filter_map(|(r, cat)| {
                let bytes = atlas::clip(r, cat, a.frames, a.px).ok()?;
                let name = format!("{}.gif", r.id);
                std::fs::write(dir.join(&name), bytes).ok()?;
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                p.report(n, Some(total), &format!("clip {n} of {total}"));
                Some(Clip {
                    id: r.id,
                    file: name,
                    category: cat.to_string(),
                    family: r.cand.fam.name().into(),
                    bucket: buckets.get(&r.id).cloned().unwrap_or_default(),
                    tiling: r.cand.tiling.clone(),
                    rule: r.cand.rule.clone(),
                    fate: r.fate.clone(),
                    score: (r.score * 100.0).round() / 100.0,
                    seed: r.cand.seed.describe(),
                })
            })
            .collect();
        clips.sort_by_key(|c| c.id);
        let manifest = dir.join("manifest.json");
        let text =
            serde_json::to_string_pretty(&clips).map_err(|e| ApiError::engine(e.to_string()))?;
        std::fs::write(&manifest, &text)
            .map_err(|e| ApiError::io(format!("writing {}: {e}", manifest.display())))?;
        let page = if a.page {
            let p = clip_ratings::write_page(&dir, row.id, &c.uuid(), row.label(), &text, &clips)?;
            Some(p.display().to_string())
        } else {
            None
        };
        Ok(Clips {
            job: row.id,
            folder: dir.display().to_string(),
            manifest: manifest.display().to_string(),
            page,
            clips,
        })
    }
}

/// The pack publisher's name, the default rater (none in a build without packs).
#[cfg(feature = "pack")]
fn publisher_name() -> Option<String> {
    crate::ops::pack::publisher_name()
}

#[cfg(not(feature = "pack"))]
fn publisher_name() -> Option<String> {
    None
}
