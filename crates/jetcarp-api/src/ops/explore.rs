//! `search.*` on an explore job's run records (its working database,
//! `searches/<uuid>/search.sqlite` beside the catalog): find runs (`search.runs`), a contact
//! sheet of them (`search.sheet`), one run as a scene (`search.run_scene`), try a configuration
//! of one's own now (`search.try`), record a verdict on runs (`search.review`, one at a time or
//! a folder of rating files), recompute every score after the score changes (`search.rescore`),
//! and animated clips of a sample for rating (`search.clips`). Each takes the job (default: the
//! newest explore job of the catalog) and reads its database while it runs; rescoring waits for
//! a stopped job.
//!
//! The reviews are the ratings the score was fitted to (the taste rounds' keep, maybe, and drop,
//! most of them Emily Teich's): clips go out, verdicts come back as rating files, and a refit
//! score is applied to every stored run by `search.rescore`.

use super::library::catalog_err;
use crate::args;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::job::{tiling_groups, try_one};
use jetcarp_core::explore::sheet::{self, RunRow};
use jetcarp_core::explore::space::{Fam, sized_spec};
use jetcarp_core::explore::store::{self, RunFilter, feat_from_blob, review_counts};
use jetcarp_core::explore::{Db, atlas};
use jetcarp_core::jobs::{self, Lease};
use jetcarp_core::library::Library;
use jetcarp_core::library::store::JobRow;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

/// The job's database for reading.
fn read_db(path: &std::path::Path) -> Result<Db, ApiError> {
    store::open_read(path)
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
        }
    }
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
    /// Only this tiling key.
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
    /// Instead of the filters: one SELECT returning run ids (read only; columns id, stage, parent, tiling, nb, fam, rule, seed, seed_kind, cells, gens, steps, rng, origin, fate, settle, period, score, bin, ts; tables runs, reviews, elites, confirmed).
    #[serde(default)]
    pub sql: Option<String>,
    /// score (default, best first), newest, or id.
    #[serde(default)]
    pub order: Option<String>,
    /// Most runs to return (default 40).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Runs found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Runs {
    pub job: i64,
    /// The job's database.
    pub db: String,
    pub runs: Vec<RunLine>,
}

impl Report for Runs {
    fn text(&self) -> String {
        let mut s = String::new();
        for r in &self.runs {
            let _ = writeln!(
                s,
                "{:>9}  {:>5.2}  {:<7} {:<18} {:<16} {}",
                r.id, r.score, r.fate, r.family, r.tiling, r.rule
            );
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
            sql: self.sql.clone(),
            family,
            tiling: self.tiling.clone(),
            fate: self.fate.clone(),
            min_score: self.min_score,
            stage: self.stage,
            reviewed: self.reviewed.clone(),
            order: self.order.clone(),
        })
    }
}

/// The runs a filter names, in order, at most `limit`.
fn find(c: &Db, f: &RunFilter, limit: usize) -> Result<Vec<RunRow>, ApiError> {
    let ids = store::run_ids(c, f, limit).map_err(invalid)?;
    store::runs_by_ids(c, &ids).map_err(ApiError::engine)
}

/// `search.runs`.
pub struct SearchRuns;
impl Op for SearchRuns {
    const NAME: &'static str = "search.runs";
    const SUMMARY: &'static str = "List an explore job's stored runs, best first, with their measures: filtered by family, tiling, fate, score, movers, stage, or review, or chosen by id or SQL.";
    const READ_ONLY: bool = true;
    type In = RunsIn;
    type Out = Runs;
    fn run(cx: &Ctx, a: RunsIn, _: &dyn Progress) -> Result<Runs, ApiError> {
        let (_, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        let limit = a.limit.unwrap_or(40).max(1);
        // Movers are inside the features: read more, then filter.
        let fetch = if a.min_movers.is_some() {
            limit.saturating_mul(20)
        } else {
            limit
        };
        let mut rs = find(&c, &a.filter()?, fetch)?;
        if let Some(m) = a.min_movers {
            rs.retain(|r| feat_from_blob(&r.feats).movers >= m);
        }
        rs.truncate(limit);
        Ok(Runs {
            job: row.id,
            db: path.display().to_string(),
            runs: rs.iter().map(RunLine::of).collect(),
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
    /// Instead of ids: one SELECT returning run ids (as search.runs takes it).
    #[serde(default)]
    pub sql: Option<String>,
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
        let (_, row, path) = explore_job(cx, a.job)?;
        let c = read_db(&path)?;
        if !(16..=2000).contains(&a.px) || a.cols == 0 {
            return Err(invalid("px must be 16 to 2000 and cols at least 1"));
        }
        let max = a.max.unwrap_or(36).max(1);
        if a.ids.is_none() && a.sql.is_none() {
            return Err(invalid("give ids or sql"));
        }
        let q = RunFilter {
            ids: a.ids.clone(),
            sql: a.sql.clone(),
            ..Default::default()
        };
        let runs = find(&c, &q, max)?;
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
        let r = store::runs_by_ids(&c, &[a.run])
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
    /// Tiling key: square, hex, trihex, spectre, catalog:k2-001, ih28, ...
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
    #[serde(default)]
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
        let (spec, topo) = sized_spec(&a.tiling, cells).map_err(invalid)?;
        let seed = match &a.seed {
            Some(v) if !v.is_null() => args::parse_seed(v).map_err(invalid)?,
            _ => jetcarp_core::world::default_seed(&rule, topo.len()),
        };
        let cand = Cand {
            tiling: a.tiling.clone(),
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
    /// Who judged: a name, `user` (default), or `model:<name>`.
    #[serde(default)]
    pub rater: Option<String>,
    /// Instead of one run: a folder of rating files as the clip-rating page saves them (`<run id>.json` holding {"verdict": "keep", "ts": ...}); ratings already recorded from it are skipped.
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
    /// The job's reviews now, per verdict.
    pub counts: Vec<(String, u64)>,
}

impl Report for Reviewed {
    fn text(&self) -> String {
        let mut s = format!("recorded {} review(s)", self.recorded);
        if self.files > 0 {
            let _ = write!(
                s,
                " from {} file(s) ({} for runs not in job {}, {} unreadable, {} already recorded)",
                self.files, self.unknown_runs, self.job, self.unreadable, self.already
            );
        }
        let c: Vec<String> = self
            .counts
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        let _ = writeln!(s, "; job {} has {}", self.job, c.join(", "));
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
        let (_, row, path) = explore_job(cx, a.job)?;
        if !path.is_file() {
            return Err(ApiError::not_found(format!(
                "job {} has no runs yet",
                row.id
            )));
        }
        let mut c = store::open(&path).map_err(ApiError::engine)?;
        let rater = a.rater.clone().unwrap_or_else(|| "user".into());
        let mut out = Reviewed {
            job: row.id,
            recorded: 0,
            files: 0,
            unknown_runs: 0,
            unreadable: 0,
            already: 0,
            counts: vec![],
        };
        match (&a.from, a.run, &a.verdict) {
            (Some(dir), None, None) => {
                let d = cx.fs.readable(dir)?;
                let r = store::import_ratings(&mut c, &d, &rater).map_err(ApiError::io)?;
                out.recorded = r.recorded;
                out.files = r.files;
                out.unknown_runs = r.unknown_runs;
                out.unreadable = r.unreadable;
                out.already = r.already;
            }
            (None, Some(run), Some(v)) => {
                let v = v.trim().to_ascii_lowercase();
                store::add_review(
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
        out.counts = review_counts(&c).map_err(ApiError::engine)?;
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
}

/// What rescoring did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Rescored {
    pub job: i64,
    /// Runs rescored, and those whose score changed (only these were written).
    pub runs: u64,
    pub changed: u64,
    /// The archive rebuilt from the new scores: its elites and niches.
    pub elites: u64,
    pub niches: u64,
    pub seconds: f64,
}

impl Report for Rescored {
    fn text(&self) -> String {
        format!(
            "rescored {} runs of explore job {} ({} scores changed); the archive now holds {} elites in {} niches ({:.1} s)\n",
            self.runs, self.job, self.changed, self.elites, self.niches, self.seconds
        )
    }
}

/// `search.rescore`.
pub struct SearchRescore;
impl Op for SearchRescore {
    const NAME: &'static str = "search.rescore";
    const SUMMARY: &'static str = "Recompute every stored score of a stopped explore job from its stored measures (after the score changes) and rebuild its archive from them; reads every run.";
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
        let mut c = store::open(&path).map_err(ApiError::engine)?;
        let groups = tiling_groups();
        let g = |k: &str| groups.get(k).copied().unwrap_or(0);
        let r = store::rescore(&mut c, &g, &|n| {
            p.report(n, None, &format!("rescored {n} runs"));
            !p.cancelled()
        })
        .map_err(ApiError::engine);
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
            runs: r.runs,
            changed: r.changed,
            elites: r.elites,
            niches: r.niches,
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
    #[serde(default = "seed_default")]
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
    pub clips: Vec<Clip>,
}

impl Report for Clips {
    fn text(&self) -> String {
        format!("{} clips in {}\n", self.clips.len(), self.folder)
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
        std::fs::write(&manifest, text)
            .map_err(|e| ApiError::io(format!("writing {}: {e}", manifest.display())))?;
        Ok(Clips {
            job: row.id,
            folder: dir.display().to_string(),
            manifest: manifest.display().to_string(),
            clips,
        })
    }
}
