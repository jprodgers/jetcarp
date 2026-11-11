//! `quickstart.*`: the Quick Start. The catalog's own entries ("My finds", with today's
//! statuses: auto, candidate, approved, rejected) are listed, read, added (from a pattern, an ant
//! run, or one run of a sweep folder), shown or hidden, and removed here; `quickstart.check`
//! lists the built-in examples and checks that each one works.

use super::library::catalog_err;
use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Op, Progress, Report};
use crate::sweeps::{self, Manifest};
use jetcarp_core::quickstart;
use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, SizeSpec, TilingSpec};
use jetcarp_store::{AntUpdate, QsStatus, QuickStartRow};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;

/// A Quick Start entry of the catalog, as every front end lists it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Entry {
    /// `find:<id>`.
    pub id: String,
    pub title: String,
    pub description: String,
    /// The tiling's name.
    pub grid: String,
    pub rule: String,
    pub rule_family: String,
    /// `still life`, `spaceship`, a row class, an ant outcome, ...
    pub behaviour: String,
    /// `pattern`, `sweep`, or `scene`.
    pub source_kind: String,
    pub source_id: Option<String>,
    pub score: Option<f64>,
    /// auto, candidate, approved, or rejected.
    #[schemars(with = "String")]
    pub status: QsStatus,
    pub created: String,
}

impl Entry {
    pub fn of(q: &QuickStartRow) -> Entry {
        Entry {
            id: format!("find:{}", q.id),
            title: q.title.clone(),
            description: q.description.clone(),
            grid: q.grid.clone(),
            rule: q.rule.clone(),
            rule_family: q.rule_family.clone(),
            behaviour: q.behaviour.clone(),
            source_kind: q.source_kind.clone(),
            source_id: q.source_id.clone(),
            score: q.score,
            status: q.status,
            created: q.created.clone(),
        }
    }
}

fn status(s: &str) -> Result<QsStatus, ApiError> {
    QsStatus::parse(s).map_err(ApiError::invalid)
}

// ---------------------------------------------------------------------------------------------
// quickstart.list

/// Which entries to list.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIn {
    /// Only entries with this status: auto, candidate, approved, rejected (default all).
    #[serde(default)]
    pub status: Option<String>,
}

/// The catalog's Quick Start entries, oldest first.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Listed {
    pub catalog: String,
    pub entries: Vec<Entry>,
}

impl Report for Listed {
    fn text(&self) -> String {
        let mut s = format!(
            "catalog: {} ({} entries)\n",
            self.catalog,
            self.entries.len()
        );
        for r in &self.entries {
            let _ = writeln!(
                s,
                "{:<29} {:<9} {:<8} {:<28} {:<14} {}",
                r.id,
                r.status.name(),
                r.source_kind,
                r.title.chars().take(28).collect::<String>(),
                r.rule.chars().take(14).collect::<String>(),
                r.grid
            );
        }
        s
    }
}

/// `quickstart.list`.
pub struct QuickstartList;
impl Op for QuickstartList {
    const NAME: &'static str = "quickstart.list";
    const SUMMARY: &'static str =
        "The catalog's Quick Start entries (\"My finds\"), oldest first, optionally of one status.";
    const READ_ONLY: bool = true;
    type In = ListIn;
    type Out = Listed;
    fn run(cx: &Ctx, a: ListIn, _: &dyn Progress) -> Result<Listed, ApiError> {
        let st = match &a.status {
            Some(s) => vec![status(s)?],
            None => vec![],
        };
        let lib = cx.library();
        let v = lib.quickstart(&st).map_err(catalog_err)?;
        Ok(Listed {
            catalog: lib.path().display().to_string(),
            entries: v.iter().map(Entry::of).collect(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.get

/// An entry, by id.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdIn {
    /// The entry's id, with or without `find:`.
    pub id: String,
}

/// A Quick Start entry with its scene.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct EntryRow(#[schemars(with = "Value")] pub QuickStartRow);

impl Report for EntryRow {
    fn text(&self) -> String {
        format!(
            "find:{}  {}  {} on {} ({})\n",
            self.0.id,
            self.0.title,
            self.0.rule,
            self.0.grid,
            self.0.status.name()
        )
    }
}

/// `quickstart.get`.
pub struct QuickstartGet;
impl Op for QuickstartGet {
    const NAME: &'static str = "quickstart.get";
    const SUMMARY: &'static str = "One Quick Start entry of the catalog, with its scene.";
    const READ_ONLY: bool = true;
    type In = IdIn;
    type Out = EntryRow;
    fn run(cx: &Ctx, a: IdIn, _: &dyn Progress) -> Result<EntryRow, ApiError> {
        cx.library()
            .get_quickstart(&a.id)
            .map(EntryRow)
            .map_err(catalog_err)
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.add

fn yes() -> bool {
    true
}

/// What to add to the Quick Start: one of `pattern`, `ant`, or `folder` with `run`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddIn {
    /// A library pattern (id, unique prefix, or name): a scene with it centred on a suitable
    /// wrapped grid; the pattern's state becomes promoted.
    #[serde(default)]
    pub pattern: Option<String>,
    /// An ant run (`ant-...`): its build-up from step 0; the run's state becomes promoted.
    #[serde(default)]
    pub ant: Option<String>,
    /// A sweep output folder (with `run`): that run's tiling, rule, and start, so it replays as
    /// in the sweep.
    #[serde(default)]
    pub folder: Option<String>,
    /// With `folder`: the run's rule text (`HexT3 sum 588`), manifest index (`#12`), or series
    /// number (`n=588`).
    #[serde(default)]
    pub run: Option<String>,
    /// The entry's title (default: the pattern's name or label, the ant's rule, the run's rule).
    #[serde(default)]
    pub title: Option<String>,
    /// Patterns and ants: the description (default: generated).
    #[serde(default)]
    pub description: Option<String>,
    /// Sweep runs: a note put before the generated description (why it is interesting).
    #[serde(default)]
    pub note: Option<String>,
    /// Ants: colour the scene with this built-in palette.
    #[serde(default)]
    pub palette: Option<String>,
    /// Sweep runs: also star the run in the folder's favourites.json (default true).
    #[serde(default = "yes")]
    pub favourite: bool,
}

/// Nothing chosen yet; sweep runs are starred (as when the field is left out of JSON).
impl Default for AddIn {
    fn default() -> AddIn {
        AddIn {
            pattern: None,
            ant: None,
            folder: None,
            run: None,
            title: None,
            description: None,
            note: None,
            palette: None,
            favourite: true,
        }
    }
}

/// The entry added (or replaced: adding the same thing again replaces its entry).
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Added {
    /// `find:<id>`.
    pub entry: String,
    /// The entry's id: the pattern's or ant's id, or `sweep-<hash>`.
    pub id: String,
    /// `pattern`, `ant`, or `sweep`.
    pub source: String,
    pub title: String,
    pub description: String,
    /// Sweep runs: the run's rule and manifest index.
    pub rule: Option<String>,
    pub run: Option<u64>,
    pub catalog: String,
}

impl Report for Added {
    fn text(&self) -> String {
        match self.source.as_str() {
            "ant" => format!("Quick Start entry {} ({})\n", self.id, self.title),
            "sweep" => format!(
                "promoted: {} as \"{}\" (run #{})\nentry:    {} in {}\nIt appears in the app's Quick Start under \"My finds\".\n",
                self.rule.as_deref().unwrap_or(""),
                self.title,
                self.run.unwrap_or(0),
                self.entry,
                self.catalog
            ),
            _ => format!(
                "promoted: {}\nentry:    {} in {}\nIt appears in the app's Quick Start under \"My finds\".\n",
                self.title, self.entry, self.catalog
            ),
        }
    }
}

/// `quickstart.add`.
pub struct QuickstartAdd;
impl Op for QuickstartAdd {
    const NAME: &'static str = "quickstart.add";
    const SUMMARY: &'static str = "Add an entry to the app's Quick Start under \"My finds\": a library pattern, an ant run from step 0, or one run of a sweep folder.";
    const READ_ONLY: bool = false;
    type In = AddIn;
    type Out = Added;
    fn run(cx: &Ctx, a: AddIn, _: &dyn Progress) -> Result<Added, ApiError> {
        let lib = cx.library();
        let catalog = lib.path().display().to_string();
        let given = [a.pattern.is_some(), a.ant.is_some(), a.folder.is_some()]
            .iter()
            .filter(|x| **x)
            .count();
        if given != 1 {
            return Err(ApiError::invalid(
                "give one of `pattern`, `ant`, or `folder` (with `run`)",
            ));
        }
        if let Some(folder) = &a.folder {
            let run = a.run.as_deref().ok_or_else(|| {
                ApiError::invalid(
                    "give `run`: the rule text, \"#index\", or \"n=number\" of a row in the folder's manifest",
                )
            })?;
            let dir = cx.fs.readable(folder)?;
            let (title, note) = (a.title.as_deref(), a.note.as_deref());
            let p = if a.favourite {
                sweeps::promote_folder_run(&dir, run, &lib, title, note)
            } else {
                promote_run_only(&dir, run, &lib, title, note)
            }
            .map_err(|e| {
                let kind = if e.starts_with("no run") || e.contains("has no manifest") {
                    ErrorKind::NotFound
                } else {
                    ErrorKind::Engine
                };
                ApiError::new(kind, e)
            })?;
            let q = lib.get_quickstart(&p.id).map_err(catalog_err)?;
            return Ok(Added {
                entry: format!("find:{}", p.id),
                id: p.id,
                source: "sweep".into(),
                title: p.title,
                description: q.description,
                rule: Some(p.rule),
                run: Some(p.index),
                catalog,
            });
        }
        if let Some(id) = &a.ant {
            let r = lib.ant(id).map_err(catalog_err)?;
            let mut sc = jetcarp_store::media::ants::build_scene_in(&r, a.palette.as_deref())
                .kind(ErrorKind::Invalid)?;
            if let Some(t) = &a.title {
                sc.name = t.clone();
            }
            if let Some(d) = &a.description {
                sc.description = d.clone();
            }
            let mut q = QuickStartRow::for_scene(&r.id, "scene", sc);
            q.behaviour = r.outcome.name().into();
            q.score = Some(r.score);
            lib.put_quickstart(&q).map_err(catalog_err)?;
            lib.update_ant(
                &r.id,
                &AntUpdate {
                    state: Some("promoted".into()),
                    ..Default::default()
                },
            )
            .map_err(catalog_err)?;
            return Ok(Added {
                entry: format!("find:{}", q.id),
                id: q.id,
                source: "ant".into(),
                title: q.title,
                description: q.scene.description,
                rule: None,
                run: None,
                catalog,
            });
        }
        let key = a.pattern.as_deref().unwrap_or_default();
        let (sc, id) = jetcarp_store::quickstart::promote(
            &lib,
            key,
            a.title.as_deref(),
            a.description.as_deref(),
        )
        .map_err(catalog_err)?;
        Ok(Added {
            entry: format!("find:{id}"),
            id,
            source: "pattern".into(),
            title: sc.name,
            description: sc.description,
            rule: None,
            run: None,
            catalog,
        })
    }
}

/// A sweep run to the Quick Start without touching the folder's favourites.
fn promote_run_only(
    folder: &std::path::Path,
    run: &str,
    lib: &jetcarp_store::Library,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<sweeps::Promoted, String> {
    let m = Manifest::load(folder)?;
    let row = m.find(run).ok_or_else(|| {
        format!(
            "no run '{run}' in {} (give the rule text, #index, or n=number from manifest.json)",
            folder.display()
        )
    })?;
    let id = sweeps::promote_run_with(&m, row, lib, title, note)?;
    let r = &m.rows[row];
    let title = lib.get_quickstart(&id).map(|q| q.title).unwrap_or_default();
    Ok(sweeps::Promoted {
        id,
        rule: sweeps::text(r, "rule").to_string(),
        index: r.get("index").and_then(|i| i.as_u64()).unwrap_or(0),
        title,
    })
}

// ---------------------------------------------------------------------------------------------
// quickstart.update / quickstart.remove

/// A new status for an entry.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusIn {
    /// The entry's id, with or without `find:`.
    pub id: String,
    /// auto or approved (shown in the menu), candidate or rejected (hidden).
    pub status: String,
}

/// An entry's new status.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct StatusSet {
    /// `find:<id>`.
    pub id: String,
    #[schemars(with = "String")]
    pub status: QsStatus,
    /// Shown in the Quick Start menu.
    pub shown: bool,
    /// The id and status as given (the command line echoes them).
    #[serde(skip)]
    pub given: (String, String),
}

impl Report for StatusSet {
    fn text(&self) -> String {
        format!("{}: {}\n", self.given.0, self.given.1)
    }
}

/// `quickstart.update`.
pub struct QuickstartUpdate;
impl Op for QuickstartUpdate {
    const NAME: &'static str = "quickstart.update";
    const SUMMARY: &'static str =
        "Show or hide a Quick Start entry: set its status (auto, candidate, approved, rejected).";
    const READ_ONLY: bool = false;
    type In = StatusIn;
    type Out = StatusSet;
    fn run(cx: &Ctx, a: StatusIn, _: &dyn Progress) -> Result<StatusSet, ApiError> {
        let st = status(&a.status)?;
        let qid = a.id.trim().trim_start_matches("find:").to_string();
        cx.library()
            .set_quickstart_status(&qid, st)
            .map_err(catalog_err)?;
        Ok(StatusSet {
            id: format!("find:{qid}"),
            status: st,
            shown: st.shown(),
            given: (a.id, a.status),
        })
    }
}

/// An entry removed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct EntryRemoved {
    /// The id, as given.
    pub id: String,
}

impl Report for EntryRemoved {
    fn text(&self) -> String {
        format!("removed {}\n", self.id)
    }
}

/// `quickstart.remove`.
pub struct QuickstartRemove;
impl Op for QuickstartRemove {
    const NAME: &'static str = "quickstart.remove";
    const SUMMARY: &'static str =
        "Delete a Quick Start entry of the catalog (the pattern or run it came from stays).";
    const READ_ONLY: bool = false;
    type In = IdIn;
    type Out = EntryRemoved;
    fn run(cx: &Ctx, a: IdIn, _: &dyn Progress) -> Result<EntryRemoved, ApiError> {
        cx.library().remove_quickstart(&a.id).map_err(catalog_err)?;
        Ok(EntryRemoved { id: a.id })
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.check

fn gens_default() -> u64 {
    300
}
fn thumb_default() -> u32 {
    160
}
fn cols_default() -> u32 {
    10
}

/// Which built-in examples to list, and what to do with them.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckIn {
    /// Only entries whose name, rule, family, or tiling contains this text.
    #[serde(default)]
    pub filter: Option<String>,
    /// Run each example (at preview size) and report whether it is alive and changing.
    #[serde(default)]
    pub check: bool,
    /// Try a range of starting patterns for each example and report how each does.
    #[serde(default)]
    pub survey: bool,
    /// For each tiling family, the candidate rules that stay lively (to choose the tiling
    /// showcases).
    #[serde(default)]
    pub survey_tilings: bool,
    /// Generations for the checks and the sheet (default 300).
    #[serde(default = "gens_default")]
    pub gens: u64,
    /// Write a numbered contact sheet of the examples (numbers match the list) here.
    #[serde(default)]
    pub sheet: Option<String>,
    /// Sheet thumbnail size (default 160).
    #[serde(default = "thumb_default")]
    pub thumb: u32,
    /// Sheet columns (default 10).
    #[serde(default = "cols_default")]
    pub cols: u32,
}

/// How an example did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct HealthOut {
    pub works: bool,
    pub generation: u64,
    pub cells: usize,
    pub population: u64,
    /// Fraction of cells that changed in the last generation.
    pub activity: f64,
    pub status: String,
}

impl HealthOut {
    fn of(h: &quickstart::Health) -> HealthOut {
        HealthOut {
            works: h.works(),
            generation: h.generation,
            cells: h.cells,
            population: h.population,
            activity: h.activity,
            status: h.status.to_string(),
        }
    }
    fn pop(&self) -> f64 {
        self.population as f64 / self.cells as f64
    }
}

/// A built-in example.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Example {
    /// Its number in the full list.
    pub index: usize,
    pub id: String,
    pub name: String,
    pub rule: String,
    pub family: String,
    pub tiling: String,
    /// With `check`: how it did (none: it could not be loaded).
    pub health: Option<HealthOut>,
    /// With `survey`: each starting pattern tried.
    pub survey: Vec<SurveyRow>,
}

/// One starting pattern tried on an example.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SurveyRow {
    pub seed: String,
    pub health: Option<HealthOut>,
    pub error: Option<String>,
}

/// One tiling family's lively candidate rules.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TilingSurvey {
    pub family: String,
    /// `rule (pop P act A)` for each lively candidate.
    pub lively: Vec<String>,
}

/// The examples, checked or surveyed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Checked {
    pub examples: Vec<Example>,
    /// With `check`: how many did not work.
    pub not_working: Option<usize>,
    pub tilings: Vec<TilingSurvey>,
    /// The sheet written, as given, and its size.
    pub sheet: Option<String>,
    pub sheet_size: Option<(u32, u32)>,
    #[serde(skip)]
    mode: u8,
}

impl Report for Checked {
    fn text(&self) -> String {
        let mut s = String::new();
        if self.mode == 2 {
            for t in &self.tilings {
                let _ = writeln!(s, "{:12} {}", t.family, t.lively.join(" | "));
            }
            return s;
        }
        if self.mode == 1 {
            for e in &self.examples {
                let _ = writeln!(s, "{:3} {} {} {}", e.index, e.id, e.name, e.rule);
                for r in &e.survey {
                    match (&r.health, &r.error) {
                        (Some(h), _) => {
                            let _ = writeln!(
                                s,
                                "      {:44} {} pop {:.3} act {:.3} {}",
                                r.seed,
                                if h.works { "ok  " } else { "FAIL" },
                                h.pop(),
                                h.activity,
                                h.status
                            );
                        }
                        (None, Some(err)) => {
                            let _ = writeln!(s, "      {} error {err}", r.seed);
                        }
                        _ => {}
                    }
                }
            }
            return s;
        }
        let check = self.not_working.is_some();
        // The id is what `jetcarp wallpaper`, `media render --quickstart` and scenes' sources
        // take (the number works too); the column fits the ids shown, up to 32 characters.
        let idw = self
            .examples
            .iter()
            .map(|e| e.id.chars().count())
            .max()
            .unwrap_or(0)
            .min(32);
        for e in &self.examples {
            let tail = match &e.health {
                Some(h) => format!(
                    "  {} pop {:.3} act {:.3} {}",
                    if h.works { "ok  " } else { "FAIL" },
                    h.pop(),
                    h.activity,
                    h.status
                ),
                None if check => "  FAIL could not load".to_string(),
                None => String::new(),
            };
            let _ = writeln!(
                s,
                "{:3} {:idw$} {:34} {:24} {:20} {:24}{}",
                e.index, e.id, e.name, e.rule, e.family, e.tiling, tail
            );
        }
        if let Some(bad) = self.not_working {
            let _ = writeln!(s, "{} examples, {} not working", self.examples.len(), bad);
        }
        if let (Some(p), Some((w, h))) = (&self.sheet, self.sheet_size) {
            let _ = writeln!(s, "wrote {p} ({w}x{h})");
        }
        s
    }
}

/// `quickstart.check`.
pub struct QuickstartCheck;
impl Op for QuickstartCheck {
    const NAME: &'static str = "quickstart.check";
    const SUMMARY: &'static str = "List the built-in Quick Start examples; check that each works, survey starting patterns or tilings, or draw a contact sheet.";
    const READ_ONLY: bool = false;
    type In = CheckIn;
    type Out = Checked;
    fn run(cx: &Ctx, a: CheckIn, _: &dyn Progress) -> Result<Checked, ApiError> {
        use rayon::prelude::*;
        if a.survey_tilings {
            return Ok(Checked {
                examples: Vec::new(),
                not_working: None,
                tilings: survey_tilings(a.gens),
                sheet: None,
                sheet_size: None,
                mode: 2,
            });
        }
        let all = quickstart::entries();
        let f = a.filter.as_ref().map(|s| s.to_lowercase());
        let picked: Vec<(usize, &quickstart::QuickStart)> = all
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                f.as_ref().is_none_or(|f| {
                    [&e.scene.name, &e.rule, &e.family, &e.tiling]
                        .iter()
                        .any(|x| x.to_lowercase().contains(f.as_str()))
                })
            })
            .collect();
        let example = |k: usize, e: &quickstart::QuickStart| Example {
            index: k,
            id: e.id.clone(),
            name: e.scene.name.clone(),
            rule: e.rule.clone(),
            family: e.family.clone(),
            tiling: e.tiling.clone(),
            health: None,
            survey: Vec::new(),
        };
        if a.survey {
            let mut seeds: Vec<SeedSpec> =
                [0.005, 0.01, 0.03, 0.1, 0.2, 0.3, 0.4, 0.45, 0.5, 0.55, 0.6]
                    .iter()
                    .map(|&d| SeedSpec::Random { density: d })
                    .collect();
            for (r, d) in [(0.08, 0.5), (0.2, 0.4), (0.5, 0.3), (0.3, 0.55), (0.5, 0.5)] {
                seeds.push(SeedSpec::Blob {
                    radius: r,
                    density: d,
                });
            }
            let examples = picked
                .iter()
                .map(|(k, e)| {
                    let mut x = example(*k, e);
                    x.survey = seeds
                        .par_iter()
                        .map(|sd| {
                            let mut sc = e.scene.clone();
                            sc.cas[0].seed = Some(sd.clone());
                            let cells = quickstart::preview_cells(&e.scene, 6000);
                            match quickstart::health(&sc, a.gens, cells) {
                                Ok(h) => SurveyRow {
                                    seed: format!("{sd:?}"),
                                    health: Some(HealthOut::of(&h)),
                                    error: None,
                                },
                                Err(err) => SurveyRow {
                                    seed: format!("{sd:?}"),
                                    health: None,
                                    error: Some(err),
                                },
                            }
                        })
                        .collect();
                    x
                })
                .collect();
            return Ok(Checked {
                examples,
                not_working: None,
                tilings: Vec::new(),
                sheet: None,
                sheet_size: None,
                mode: 1,
            });
        }
        let health: Vec<Option<HealthOut>> = if a.check {
            picked
                .par_iter()
                .map(|(_, e)| {
                    let cells = quickstart::preview_cells(&e.scene, 6000);
                    quickstart::health(&e.scene, a.gens, cells)
                        .ok()
                        .map(|h| HealthOut::of(&h))
                })
                .collect()
        } else {
            vec![None; picked.len()]
        };
        let not_working = a.check.then(|| {
            health
                .iter()
                .filter(|h| h.as_ref().is_some_and(|h| !h.works))
                .count()
        });
        let mut sheet_size = None;
        if let Some(p) = &a.sheet {
            // Numbers on the sheet are list positions, so render in runs of consecutive indices.
            let scenes: Vec<&Scene> = picked.iter().map(|(_, e)| &e.scene).collect();
            let first = picked.first().map(|x| x.0 as u32).unwrap_or(0);
            let contiguous = picked.windows(2).all(|w| w[1].0 == w[0].0 + 1);
            let img = quickstart::contact_sheet(
                &scenes,
                if contiguous { first } else { 0 },
                a.gens,
                a.thumb,
                a.cols,
            );
            let file = cx.fs.writable(p)?;
            jetcarp_core::render::write_png(&file, &img, &[]).map_err(ApiError::io)?;
            sheet_size = Some((img.width, img.height));
        }
        let examples = picked
            .iter()
            .zip(health)
            .map(|((k, e), h)| Example {
                health: h,
                ..example(*k, e)
            })
            .collect();
        Ok(Checked {
            examples,
            not_working,
            tilings: Vec::new(),
            sheet: a.sheet,
            sheet_size,
            mode: 0,
        })
    }
}

/// For each tiling family (and a few isohedral types), the first candidate rules that stay
/// lively.
fn survey_tilings(gens: u64) -> Vec<TilingSurvey> {
    use jetcarp_core::quickstart::{health, preview_scene};
    use rayon::prelude::*;
    const CANDIDATES: &[&str] = &[
        "B3/S23",
        "B36/S23",
        "B3/S23V",
        "B2/S34",
        "B2/S34V",
        "B34/S34",
        "B2/S23V",
        "B35/S234",
        "B2/S/C3",
        "B2/S/C3V",
        "B23/S34/C5",
        "B2/S3/C4V",
    ];
    let mut fams: Vec<String> = jetcarp_core::families::FAMILIES
        .iter()
        .filter(|f| f.id != "periodic")
        .map(|f| f.id.to_string())
        .collect();
    fams.extend(
        [
            "ih01", "ih07", "ih21", "ih28", "ih41", "ih55", "ih74", "ih81", "ih90",
        ]
        .map(String::from),
    );
    fams.par_iter()
        .map(|fam| {
            let size = if jetcarp_core::families::family(fam)
                .is_some_and(|f| f.kind == jetcarp_core::families::FamilyKind::Aperiodic)
            {
                SizeSpec::Level(jetcarp_core::aperiodic::default_level(fam))
            } else {
                SizeSpec::Cells(6000)
            };
            let mut lively = Vec::new();
            for r in CANDIDATES {
                let mut sc = Scene::new(
                    TilingSpec::new(fam)
                        .with_size(size)
                        .with_boundary(Boundary::Wrap),
                );
                sc.cas = vec![CaSpec::new(r).seed(SeedSpec::Random { density: 0.35 })];
                if let Ok(h) = health(&preview_scene(&sc, 6000), gens, None) {
                    let pop = h.population as f64 / h.cells as f64;
                    if h.works() && h.activity >= 0.01 && (0.03..0.7).contains(&pop) {
                        lively.push(format!("{r} (pop {pop:.2} act {:.2})", h.activity));
                    }
                }
            }
            TilingSurvey {
                family: fam.clone(),
                lively,
            }
        })
        .collect()
}

/// A scene from a file path, a built-in Quick Start id (or its number in `jetcarp quickstart`),
/// or a catalog entry (`find:...`).
pub fn scene_source(cx: &Ctx, source: &str) -> Result<Scene, ApiError> {
    if std::path::Path::new(source).exists() {
        let file = cx.fs.readable(source)?;
        let text = std::fs::read_to_string(&file)
            .map_err(|e| ApiError::io(format!("reading {}: {e}", file.display())))?;
        return Scene::from_json(&text).kind(ErrorKind::Invalid);
    }
    let all = quickstart::entries();
    if let Some(e) = all.iter().find(|e| e.id == source) {
        return Ok(e.scene.clone());
    }
    // The number `jetcarp quickstart` lists it under.
    if let Ok(k) = source.parse::<usize>()
        && let Some(e) = all.get(k)
    {
        return Ok(e.scene.clone());
    }
    if source.starts_with("find:") {
        let lib = cx.library();
        let row = lib.get_quickstart(source).map_err(catalog_err)?;
        return Ok(jetcarp_store::quickstart::find_entry(&lib, row).scene);
    }
    Err(ApiError::not_found(format!(
        "'{source}' is neither a scene file nor a Quick Start id (see `jetcarp quickstart`)"
    )))
}

// ---------------------------------------------------------------------------------------------
// quickstart.publish

fn per_niche_default() -> usize {
    12
}
fn min_score_default() -> f64 {
    1.25
}
fn max_entries_default() -> usize {
    12_000
}

/// What to publish from an explore job, and where.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// The atlas file to write (default: where the Quick Start menu reads it, `$JETCARP_ATLAS` when set, else `<data>/explore/atlas.sqlite`); it is replaced.
    #[serde(default)]
    pub out: Option<String>,
    /// Entries per category, family, and tiling (default 12).
    #[serde(default = "per_niche_default")]
    pub per_niche: usize,
    /// Lowest confirmed score, the worst of an elite's confirmation runs (default 1.25; contrast picks are exempt).
    #[serde(default = "min_score_default")]
    pub min_score: f64,
    /// Most entries (default 12,000).
    #[serde(default = "max_entries_default")]
    pub max_entries: usize,
}

/// An atlas published.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Published {
    pub job: i64,
    pub entries: usize,
    /// The atlas file.
    pub file: String,
    /// The Quick Start menu reads this file now.
    pub in_quick_start: bool,
}

impl Report for Published {
    fn text(&self) -> String {
        format!(
            "{} entries from explore job {} in {}{}\n",
            self.entries,
            self.job,
            self.file,
            if self.in_quick_start {
                " (the Quick Start menu reads it)"
            } else {
                ""
            }
        )
    }
}

/// `quickstart.publish`.
pub struct QuickstartPublish;
impl Op for QuickstartPublish {
    const NAME: &'static str = "quickstart.publish";
    const SUMMARY: &'static str = "Publish an explore job's confirmed best runs as the atlas the Quick Start menu offers: chosen per category, family, and tiling, each with its palette and a mature state.";
    const READ_ONLY: bool = false;
    type In = PublishIn;
    type Out = Published;
    fn run(cx: &Ctx, a: PublishIn, _: &dyn Progress) -> Result<Published, ApiError> {
        use jetcarp_store::atlas;
        let (_, row, path) = super::explore::explore_job(cx, a.job)?;
        let conn = jetcarp_search::explore::store::open_read(&path).map_err(ApiError::not_found)?;
        if a.per_niche == 0 || a.max_entries == 0 {
            return Err(ApiError::invalid(
                "per_niche and max_entries must be at least 1",
            ));
        }
        let out = match &a.out {
            Some(o) => cx.fs.writable(o)?,
            None => {
                let env = std::env::var_os("JETCARP_ATLAS").filter(|v| !v.is_empty());
                let p = match env {
                    Some(v) => std::path::PathBuf::from(v),
                    None => cx.paths().data.join("explore").join("atlas.sqlite"),
                };
                cx.fs.writable(&p.display().to_string())?
            }
        };
        let opts = jetcarp_search::explore::atlas::AtlasOpts {
            per_niche: a.per_niche,
            min_score: a.min_score,
            max_entries: a.max_entries,
        };
        // Built beside it and moved over it, so a reader never sees half an atlas.
        let tmp = out.with_extension("sqlite.part");
        let n =
            jetcarp_search::explore::atlas::build(&conn, &tmp, &opts).map_err(ApiError::engine)?;
        std::fs::rename(&tmp, &out).map_err(|e| ApiError::io(format!("{}: {e}", out.display())))?;
        let in_qs = atlas::locate()
            .is_some_and(|p| std::fs::canonicalize(&p).ok() == std::fs::canonicalize(&out).ok());
        Ok(Published {
            job: row.id,
            entries: n,
            file: out.display().to_string(),
            in_quick_start: in_qs,
        })
    }
}
