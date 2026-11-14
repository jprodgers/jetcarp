//! `quickstart.*`: the Quick Start, one table of entries from every source: the built-in
//! examples, entries from packs, the atlas an explore search publishes, and promotions. Entries
//! are listed, read, added (promoted from a pattern, an ant run, one run of a sweep folder, a
//! search run, or a scene file), hidden or shown, and removed here; `quickstart.publish` writes
//! an explore search's atlas into the table; `quickstart.check` lists the built-in examples and
//! checks that each one works.

use super::library::catalog_err;
use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Op, Progress, Report};
use crate::sweeps::{self, Manifest};
use jetcarp_core::quickstart;
use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, SizeSpec, TilingSpec};
use jetcarp_store::{AntUpdate, QsSource, QuickStartRow};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;

/// A Quick Start entry, as every front end lists it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Entry {
    /// Its handle: what the app, rotations, profiles and every command open it by (`find:<id>`,
    /// `atlas:<...>`, `qs-<hex>`, a built-in id such as `life`).
    pub id: String,
    pub title: String,
    pub description: String,
    /// The tiling's name.
    pub grid: String,
    pub rule: String,
    pub rule_family: String,
    /// `still life`, `spaceship`, a row class, an ant outcome, an atlas category, ...
    pub behaviour: String,
    /// The menu's category.
    pub category: String,
    /// `builtin`, `pack`, `atlas`, or `promotion`.
    pub source: String,
    /// A pack's manifest hash, a search's uuid, or a built-in rule's reference.
    pub source_ref: String,
    /// What it was made from: `pattern`, `ant`, `sweep_run`, `search_run`, or `scene`.
    pub target_kind: String,
    pub target_id: Option<String>,
    pub score: Option<f64>,
    /// Left out of the menu, packs and the website.
    pub hidden: bool,
    pub created: String,
}

impl Entry {
    pub fn of(q: &QuickStartRow) -> Entry {
        Entry {
            id: q.handle.clone(),
            title: q.title.clone(),
            description: q.description.clone(),
            grid: q.tiling_name.clone(),
            rule: q.rule.clone(),
            rule_family: q.rule_family.clone(),
            behaviour: q.behaviour.clone(),
            category: q.category.clone(),
            source: q.source.name().into(),
            source_ref: q.source_ref.clone(),
            target_kind: q.target_kind.clone(),
            target_id: q.target_id.clone(),
            score: q.score,
            hidden: q.hidden,
            created: q.created.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.list

/// Which entries to list.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIn {
    /// Only entries from this source: builtin, pack, atlas, promotion, or all (default: every
    /// source but the built-in examples, which `quickstart.check` lists).
    #[serde(default)]
    pub source: Option<String>,
    /// Only hidden entries (true) or only shown ones (false); default both.
    #[serde(default)]
    pub hidden: Option<bool>,
}

/// The sources a `source` argument names: one, `all`, or (none given) every one but `builtin`.
pub fn sources(source: Option<&str>) -> Result<Vec<QsSource>, ApiError> {
    match source.map(str::trim) {
        None | Some("") => Ok(vec![QsSource::Promotion, QsSource::Pack, QsSource::Atlas]),
        Some("all") => Ok(vec![]),
        Some(s) => Ok(vec![QsSource::parse(s).map_err(ApiError::invalid)?]),
    }
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
                "{:<29} {:<9} {:<10} {:<28} {:<14} {}{}",
                r.id,
                r.source,
                r.target_kind,
                r.title.chars().take(28).collect::<String>(),
                r.rule.chars().take(14).collect::<String>(),
                r.grid,
                if r.hidden { "  (hidden)" } else { "" }
            );
        }
        s
    }
}

/// `quickstart.list`.
pub struct QuickstartList;
impl Op for QuickstartList {
    const NAME: &'static str = "quickstart.list";
    const SUMMARY: &'static str = "The library's Quick Start entries, oldest first: promotions, pack entries and the atlas (or one source, the built-ins too), shown or hidden.";
    const READ_ONLY: bool = true;
    type In = ListIn;
    type Out = Listed;
    fn run(cx: &Ctx, a: ListIn, _: &dyn Progress) -> Result<Listed, ApiError> {
        let src = sources(a.source.as_deref())?;
        let lib = cx.library();
        let mut v = lib.quickstart(&src).map_err(catalog_err)?;
        if let Some(h) = a.hidden {
            v.retain(|q| q.hidden == h);
        }
        Ok(Listed {
            catalog: lib.path().display().to_string(),
            entries: v.iter().map(Entry::of).collect(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.get

/// An entry, by handle or id.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdIn {
    /// The entry's handle (`find:<id>`, `atlas:<...>`, `life`, ...) or id.
    pub id: String,
}

/// A Quick Start entry with its scene.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct EntryRow(#[schemars(with = "Value")] pub QuickStartRow);

impl Report for EntryRow {
    fn text(&self) -> String {
        format!(
            "{}  {}  {} on {} ({}{})\n",
            self.0.handle,
            self.0.title,
            self.0.rule,
            self.0.tiling_name,
            self.0.source.name(),
            if self.0.hidden { ", hidden" } else { "" }
        )
    }
}

/// `quickstart.get`.
pub struct QuickstartGet;
impl Op for QuickstartGet {
    const NAME: &'static str = "quickstart.get";
    const SUMMARY: &'static str = "One Quick Start entry, by handle or id, with its scene.";
    const READ_ONLY: bool = true;
    type In = IdIn;
    type Out = EntryRow;
    fn run(cx: &Ctx, a: IdIn, _: &dyn Progress) -> Result<EntryRow, ApiError> {
        let lib = cx.library();
        // A built-in entry reads the same before the library's file is made.
        if let Some(r) = jetcarp_store::quickstart::unmade_builtin(&lib, &a.id) {
            return Ok(EntryRow(r.clone()));
        }
        lib.get_quickstart(&a.id).map(EntryRow).map_err(catalog_err)
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.add

fn yes() -> bool {
    true
}

/// What to add to the Quick Start: one of `pattern`, `ant`, `folder` with `run`, `search_run`
/// (with `job`), or `scene`.
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
    /// A stored run of an explore search, by its id: on the large grid it was confirmed on, its
    /// card on the small grid it was found on, as the atlas shows its entries.
    #[serde(default)]
    pub search_run: Option<i64>,
    /// With `search_run`: the explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
    /// A scene file.
    #[serde(default)]
    pub scene: Option<String>,
    /// The entry's title (default: the pattern's name or label, the ant's rule, the run's rule,
    /// the scene's name).
    #[serde(default)]
    pub title: Option<String>,
    /// Patterns, ants, search runs and scenes: the description (default: generated, or the
    /// scene's).
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
            search_run: None,
            job: None,
            scene: None,
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
    /// Its handle, `find:<id>`.
    pub entry: String,
    /// The entry's id: the pattern's or ant's id, `sweep-<hash>`, `run-<search>-<run>`,
    /// `scene-<hash>`.
    pub id: String,
    /// `pattern`, `ant`, `sweep`, `search_run`, or `scene`.
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
    const SUMMARY: &'static str = "Add an entry to the app's Quick Start under \"My finds\": a library pattern, an ant run from step 0, one run of a sweep folder, a run of an explore search, or a scene file.";
    const READ_ONLY: bool = false;
    type In = AddIn;
    type Out = Added;
    fn run(cx: &Ctx, a: AddIn, _: &dyn Progress) -> Result<Added, ApiError> {
        let lib = cx.library();
        let catalog = lib.path().display().to_string();
        let given = [
            a.pattern.is_some(),
            a.ant.is_some(),
            a.folder.is_some(),
            a.search_run.is_some(),
            a.scene.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if given != 1 {
            return Err(ApiError::invalid(
                "give one of `pattern`, `ant`, `folder` (with `run`), `search_run`, or `scene`",
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
                entry: q.handle,
                id: p.id,
                source: "sweep".into(),
                title: p.title,
                description: q.description,
                rule: Some(p.rule),
                run: Some(p.index),
                catalog,
            });
        }
        if let Some(run) = a.search_run {
            let (_, job, path) = super::explore::explore_job(cx, a.job)?;
            let db = jetcarp_search::explore::open_read(&path)
                .map_err(|e| ApiError::not_found(e).hint("the job has not run yet"))?;
            let r = db
                .rows(&[run])
                .map_err(ApiError::engine)?
                .pop()
                .ok_or_else(|| {
                    ApiError::not_found(format!("no run {run} in explore job {}", job.id))
                })?;
            let mut q =
                jetcarp_search::explore::atlas::run_row(&r, &job.uuid).map_err(ApiError::engine)?;
            let short: String = job.uuid.chars().filter(|c| *c != '-').take(8).collect();
            q.id = format!("run-{short}-{run}");
            q.handle = format!("find:{}", q.id);
            q.source = QsSource::Promotion;
            q.source_ref.clear();
            if let Some(t) = &a.title {
                q.title = t.clone();
                q.scene.name = t.clone();
            }
            if let Some(d) = &a.description {
                q.description = d.clone();
                q.scene.description = d.clone();
            }
            lib.put_quickstart(&q).map_err(catalog_err)?;
            return Ok(Added {
                entry: q.handle,
                id: q.id,
                source: "search_run".into(),
                title: q.title,
                description: q.description,
                rule: Some(q.rule),
                run: Some(run as u64),
                catalog,
            });
        }
        if let Some(file) = &a.scene {
            let path = cx.fs.readable(file)?;
            let text = std::fs::read_to_string(&path)
                .map_err(|e| ApiError::io(format!("reading {}: {e}", path.display())))?;
            let mut sc = Scene::from_json(&text).kind(ErrorKind::Invalid)?;
            if let Some(t) = &a.title {
                sc.name = t.clone();
            }
            if let Some(d) = &a.description {
                sc.description = d.clone();
            }
            if sc.name.trim().is_empty() {
                sc.name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
            }
            let id = format!(
                "scene-{}",
                &jetcarp_core::sha256::sha256_hex(sc.to_json().as_bytes())[..16]
            );
            let q = QuickStartRow::for_scene(&id, "scene", sc);
            lib.put_quickstart(&q).map_err(catalog_err)?;
            return Ok(Added {
                entry: q.handle,
                id: q.id,
                source: "scene".into(),
                title: q.title,
                description: q.description,
                rule: Some(q.rule),
                run: None,
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
            let mut q = QuickStartRow::for_scene(&r.id, "ant", sc);
            q.target_id = Some(r.id.clone());
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
                entry: q.handle,
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

/// Hide an entry or show it again.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HideIn {
    /// The entry's handle or id.
    pub id: String,
    /// Left out of the menu, packs and the website (true), or shown again (false).
    pub hidden: bool,
}

/// An entry hidden or shown.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct HiddenSet {
    /// Its handle.
    pub id: String,
    pub hidden: bool,
}

impl Report for HiddenSet {
    fn text(&self) -> String {
        format!(
            "{}: {}\n",
            self.id,
            if self.hidden { "hidden" } else { "shown" }
        )
    }
}

/// `quickstart.update`.
pub struct QuickstartUpdate;
impl Op for QuickstartUpdate {
    const NAME: &'static str = "quickstart.update";
    const SUMMARY: &'static str =
        "Hide a Quick Start entry (from the menu, packs and the website), or show it again.";
    const READ_ONLY: bool = false;
    type In = HideIn;
    type Out = HiddenSet;
    fn run(cx: &Ctx, a: HideIn, _: &dyn Progress) -> Result<HiddenSet, ApiError> {
        let q = cx
            .library()
            .set_quickstart_hidden(&a.id, a.hidden)
            .map_err(catalog_err)?;
        Ok(HiddenSet {
            id: q.handle,
            hidden: q.hidden,
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
    const SUMMARY: &'static str = "Delete a Quick Start entry (the pattern or run it came from stays; a built-in comes back with the next build, so hide it instead).";
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
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
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

/// A scene from a file path, a Quick Start entry's handle (a built-in id such as `life`,
/// `find:<id>`, `atlas:<...>`), or a built-in entry's number in `jetcarp quickstart`.
pub fn scene_source(cx: &Ctx, source: &str) -> Result<Scene, ApiError> {
    if std::path::Path::new(source).exists() {
        let file = cx.fs.readable(source)?;
        let text = std::fs::read_to_string(&file)
            .map_err(|e| ApiError::io(format!("reading {}: {e}", file.display())))?;
        return Scene::from_json(&text).kind(ErrorKind::Invalid);
    }
    let lib = cx.library();
    if let Ok(sc) = jetcarp_store::quickstart::scene(&lib, source) {
        return Ok(sc);
    }
    // The number `jetcarp quickstart` lists a built-in under.
    if let Ok(k) = source.parse::<usize>()
        && let Some(q) = jetcarp_store::quickstart::builtin_rows().get(k)
    {
        return jetcarp_store::quickstart::scene(&lib, &q.handle).map_err(catalog_err);
    }
    Err(ApiError::not_found(format!(
        "'{source}' is neither a scene file nor a Quick Start entry (see `jetcarp quickstart` \
         and `jetcarp library quickstart`)"
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

/// Which explore job to publish, and how its entries are picked.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishIn {
    /// The explore job (default: the newest).
    #[serde(default)]
    pub job: Option<i64>,
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

/// An atlas published into the Quick Start.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Published {
    pub job: i64,
    /// The search's uuid (the rows' `source_ref`).
    pub search: String,
    /// The atlas's entries now.
    pub entries: usize,
    /// New entries, entries written again, and the search's earlier entries no longer chosen
    /// and deleted (hidden ones are kept).
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub catalog: String,
}

impl Report for Published {
    fn text(&self) -> String {
        format!(
            "{} atlas entries from explore job {} (search {}) in the Quick Start of {}: {} new, \
             {} updated, {} removed\n",
            self.entries,
            self.job,
            self.search,
            self.catalog,
            self.added,
            self.updated,
            self.removed
        )
    }
}

/// `quickstart.publish`.
pub struct QuickstartPublish;
impl Op for QuickstartPublish {
    const NAME: &'static str = "quickstart.publish";
    const SUMMARY: &'static str = "Publish an explore job's confirmed best runs as the atlas in the Quick Start: chosen per category, family, and tiling, each with its palette and a mature state, written into the library's Quick Start table (replacing the job's earlier atlas entries).";
    const READ_ONLY: bool = false;
    type In = PublishIn;
    type Out = Published;
    fn run(cx: &Ctx, a: PublishIn, _: &dyn Progress) -> Result<Published, ApiError> {
        if a.per_niche == 0 || a.max_entries == 0 {
            return Err(ApiError::invalid(
                "per_niche and max_entries must be at least 1",
            ));
        }
        let (lib, row, path) = super::explore::explore_job(cx, a.job)?;
        let conn = jetcarp_search::explore::open_read(&path).map_err(ApiError::not_found)?;
        let opts = jetcarp_search::explore::atlas::AtlasOpts {
            per_niche: a.per_niche,
            min_score: a.min_score,
            max_entries: a.max_entries,
        };
        let rows = jetcarp_search::explore::atlas::rows(&conn, &opts, &row.uuid)
            .map_err(ApiError::engine)?;
        let w = lib
            .publish_atlas(row.id, &row.uuid, &rows)
            .map_err(catalog_err)?;
        Ok(Published {
            job: row.id,
            search: row.uuid,
            entries: rows.len(),
            added: w.added,
            updated: w.updated,
            removed: w.removed,
            catalog: lib.path().display().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// quickstart.build

/// A Quick Start build: an explore search of so many minutes, then its atlas published into the
/// Quick Start, as one job.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildIn {
    /// Minutes the search runs in all, over every session, before its atlas is published (default 10).
    #[serde(default)]
    pub minutes: Option<f64>,
    /// The search's settings (search.start's explore spec); default: every tiling and rule family, and a seed from the clock.
    #[serde(default)]
    pub search: super::search::ExploreIn,
    /// Run the rules a GPU can run on the GPU (true) or everything on the CPU (false). Default: as JETCARP's GPU setting says (automatic: the GPU when there is a usable one).
    #[serde(default)]
    pub gpu: Option<bool>,
    /// Entries per category, family, and tiling (default 12).
    #[serde(default)]
    pub per_niche: Option<usize>,
    /// Lowest confirmed score (default 1.25; contrast picks are exempt).
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Most entries (default 12,000).
    #[serde(default)]
    pub max_entries: Option<usize>,
    /// The job's label (default "Quick Start build").
    #[serde(default)]
    pub label: Option<String>,
    /// Worker threads (default 0: every core but two, a third of them for the GPU's batches).
    #[serde(default)]
    pub threads: Option<usize>,
    /// Run on a thread of this process and answer at once.
    #[serde(default)]
    pub background: bool,
    /// Run in a process of its own (`jetcarp jobs resume JOB`) and answer at once.
    #[serde(default)]
    pub detach: bool,
}

/// The label of a Quick Start build's job.
pub const BUILD_LABEL: &str = "Quick Start build";

impl BuildIn {
    /// The explore job it makes.
    pub fn job_spec(&self) -> Result<jetcarp_search::jobs::JobSpec, ApiError> {
        use jetcarp_search::jobs::explore::BuildSpec;
        let minutes = self.minutes.unwrap_or(10.0);
        if !(minutes > 0.0 && minutes.is_finite()) {
            return Err(ApiError::invalid(format!(
                "a Quick Start build needs minutes above 0, not {minutes}"
            )));
        }
        let mut search = self.search.clone();
        if search.seed.is_none() {
            // Each build starts its random draws somewhere of its own, so two computers find
            // different things.
            search.seed = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0),
            );
        }
        let mut spec = search.spec()?;
        spec.gpu = self.gpu == Some(true);
        spec.cpu = self.gpu == Some(false);
        let mut b = BuildSpec::minutes(minutes);
        if let Some(n) = self.per_niche {
            b.per_niche = n;
        }
        if let Some(m) = self.min_score {
            b.min_score = m;
        }
        if let Some(n) = self.max_entries {
            b.max_entries = n;
        }
        spec.build = Some(b);
        Ok(jetcarp_search::jobs::JobSpec {
            strategy: "explore".into(),
            spec: serde_json::to_value(&spec).map_err(|e| ApiError::engine(e.to_string()))?,
            label: Some(
                self.label
                    .clone()
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| BUILD_LABEL.into()),
            ),
            budget: Default::default(),
        })
    }
}

/// `quickstart.build`.
pub struct QuickstartBuild;
impl Op for QuickstartBuild {
    const NAME: &'static str = "quickstart.build";
    const SUMMARY: &'static str = "Build Quick Start entries of your own: an explore search of so many minutes (on the GPU when there is one), then its best runs published into the Quick Start as its atlas, as one job (stopped, it resumes where it was).";
    const READ_ONLY: bool = false;
    type In = BuildIn;
    type Out = super::search::SearchStarted;
    fn run(
        cx: &Ctx,
        a: BuildIn,
        p: &dyn Progress,
    ) -> Result<super::search::SearchStarted, ApiError> {
        let t0 = std::time::Instant::now();
        let lib = cx.library();
        let js = a.job_spec()?;
        let job = jetcarp_search::jobs::create(&lib, &js).map_err(ApiError::invalid)?;
        let how = super::search::How {
            threads: a.threads.unwrap_or(0),
            gpu: a.gpu,
            budget: Default::default(),
            background: a.background,
            detach: a.detach,
            files: Default::default(),
        };
        let mut out = super::search::run_job(cx, &lib, job, &how, p)?;
        out.created = true;
        out.seconds = t0.elapsed().as_secs_f64();
        Ok(out)
    }
}
