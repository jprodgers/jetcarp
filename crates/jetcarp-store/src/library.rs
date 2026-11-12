//! The library: one SQLite file holding every pattern, the rules they work in, sightings, jobs,
//! census results, the Quick Start entries, ratings, ant runs, media, and each record's
//! provenance and geometry pin.
//!
//! **File.** `JETCARP_LIBRARY` if set (a folder, or a `.sqlite` file), else
//! `<data>/library.sqlite`. A folder means `<folder>/library.sqlite`. The file is
//! opened in WAL mode with a 30 s busy timeout, so the app, the CLI, scripts, and MCP servers can
//! read while a long hunt writes. Writes go through `BEGIN IMMEDIATE` transactions; batch them
//! ([`Library::add_many`], [`Library::add_sightings`]) for speed.
//!
//! **Schema.** Version 3 ([`schema`]): `PRAGMA user_version` = 3 and the application id `JETC`.
//! A library of schema 1 or 2 (before the 0.4 release) is brought up to 3 when it is opened. A library
//! from a newer JETCARP is refused, and so is a catalog from before schema 1 (`catalog.sqlite`,
//! made by a build before 0.4): there is no migration from those.
//!
//! **Ids.** Patterns and ants have content ids: 32 hex digits of SHA-256 over the record's
//! canonical identity and its tiling's geometry pin (`jetcarp_core::pattern::range`; ants
//! [`ant_id`]). Lookups ([`Library::get`], [`Library::ant`]) take a whole id, a unique prefix
//! (4+ digits; the first 8 are the short id), a name, or an alias from `id_aliases`.
//!
//! | Table | One row per | Key columns |
//! | --- | --- | --- |
//! | `meta` | library fact | `key`, `value`: `schema`, `library_uuid`, `created`, `created_by`, `builtin_version` (the build whose built-in Quick Start entries the library has) |
//! | `engine_pins` | pin key in use | `pin_key` (the tiling's identity key: its identity JSON, without an aperiodic family's level), the running engine's `pin`, `polygon_hash`, `neighbour_hash`, `engine_version` (the engine that computed it), `checked` |
//! | `rules` | rule as run (notation, neighbourhood, steps per generation) | `id`, `notation`, `neighborhood`, `steps_per_gen`, `family` (`lifelike`, `generations`, `wireworld`, `table`, `priority`, ...), `states`, `birth`/`survive` bitmasks (B/S and Generations) |
//! | `patterns` | rule-free pattern | `id` (content id), `model`, `tiling` (identity JSON), `tiling_key` (family or catalog id), `tiling_name`, `pin_key`, `geometry_pin`, `neighborhood`, `class`, `period`, `dx`/`dy`, `cells`, `canonical`, `rule` (first rule verified in, used to run it), the rule range (`range_bits`, `birth_min`, `birth_max`, `survive_min`, `survive_max`, `states_min`, `states_max`; NULL without a range), `seen` (sum of sightings), `score`, curation (`name`, `label`, `tags` JSON, `notes`, `rating`, `state`, `discovered_by`), `emits` (a gun's ship), `needs_reverify`, `engine_version`, `body` (JSON: phases, bbox, site, verification, mover, first-find provenance), `family` (its rule's family, as rule-semantics versions name them: `lifelike`, `generations`, `cyclic`, ...) and `semantics` (the version of that family's meaning it was verified under; [`jetcarp_core::semantics`]) |
//! | `pattern_rules` | (pattern, rule) it was verified in | `verified` (JSON), `periods`, `engine_version`, `first_seen` |
//! | `sightings` | (pattern, rule, job) | `count` (copies), `soups` (soups it appeared in), `first_seen`, `last_seen`; job 0 is "manual" (adds outside a job) |
//! | `unstored` | (job, rule, pattern) a hunt counted but did not store ([`unstored`]) | class, period, cells, copies, soups, first find |
//! | `jobs` | search or hunt | `uuid` (the search id, for folders and provenance), `kind`, `spec` (JSON, with its `label`), `status` (`running`, `paused`, `done`, `failed`), `progress` (JSON), `checkpoint` (BLOB), `owner` (`host:pid` running it) and `heartbeat` (seconds since 1970; [`jobs`]), `engine_version` |
//! | `census_rows` | rule of a census job | `job_id`, `idx`, `rule`, `summary` (JSON); read by the census results, the job report, and Rhai's `lib.census_rows` |
//! | `provenance` | event in a record's history | (`target_kind`, `target_id`, `seq`); `kind` (`found`, `imported`, `reverified`), `finder` and `orcid` (`JETCARP_FINDER`, `JETCARP_ORCID`), `search_kind` and `job_uuid`, `pack_hash`, `machine` (OS and CPU architecture), `engine_version`, `geometry_pin`, `source`, `date`. Row 0 is written once and never rewritten (a trigger refuses it); later rows append ([`provenance`]) |
//! | `id_aliases` | old id of a record | `alias`, `target_kind`, `id`, `reason` (`geometry-change`) |
//! | `quickstart` | Quick Start entry from any source ([`quickstart`]) | `id`, `handle` (what it is opened by), `source` (`builtin`, `pack`, `atlas`, `promotion`), `source_ref`, `target_kind` (`pattern`, `ant`, `sweep_run`, `search_run`, `scene`), `target_id`, `title`, `description`, the menu's `category`, `grid_group` and `family`, `tiling_key`, `tiling_name`, `rule`, `rule_family`, `behaviour`, `scene` (JSON), `preview` (JSON), `mature_gen`, `mature`, `score`, `rank`, `featured`, `hidden`, `pin_key`, `geometry_pin`, `needs_reverify`, `engine_version` |
//! | `quickstart_usage` | Quick Start entry the app opened, by its handle | `opens`, `first_opened`, `last_opened` (local; not exported) |
//! | `ratings` | rating of a pattern, an ant run, a Quick Start entry, or a search run | `target_kind`, `target_id` (a search run: `<search uuid>:<run id>`), `rater` (`user` or `model:<name>`; a search run's reviewer may be any name), `rating` (1-5), `verdict` (a search run's keep, maybe or drop; rating 5, 3, 1), `note`, `source` |
//! | `ants` | ant or turmite run: rule, tiling, start ([`ants`]) | `id` (`ant-` and a content id), `rule`, `kind` (`string`, `table`), `states`, `colors`, `tiling`, `tiling_key`, `pin_key`, `geometry_pin`, `start`, `outcome` (`highway`, `periodic`, `symmetric`, `bounded`, `chaotic`), `period`, `dx`/`dy`, `speed`, `onset`, `steps`, `cells`, `body` (JSON: the run with its proof, and the limits), `score`, curation, `job_id`, `needs_reverify`, `engine_version` |
//! | `media` | picture or animation ([`crate::media`]) | `target_kind`, `target_id`, `kind` (`frames`, `gif`, `webp`, `poster`, `thumb`), `from_seed`, `hash` (SHA-256 of the file; for `frames` of its frame list) and `ext` (the file is `media/<first two hex digits>/<hash>.<ext>` beside the library), `render` (JSON), `width`, `height`, `frames`, `bytes`, `key` (render key; another key means stale) |
//!
//! Gone since the catalog: the stage 1 tables `settings` and `score_models`, and the `setting`
//! targets of media and ratings.
//!
//! **Geometry pins and rule semantics.** Every pattern, ant run and Quick Start row stores the
//! pin of the tiling it was verified on, and every pattern the semantics version of its rule's
//! family. Opening a library flags (`needs_reverify`) the records whose pin or version is not the
//! running engine's ([`schema`]); [`Library::reverified`] stores what checking them again found.
//!
//! "Every pattern that works in rule R" is [`Query::works_in`](Query): the range test on
//! the bitmasks (`birth_min & ~B = 0`, `B & ~birth_max = 0`, the same for S, and the state count
//! between `states_min` and `states_max`) or an explicit `pattern_rules` row.

use jetcarp_core::pattern::movers::MoverRecord;
use jetcarp_core::pattern::range::{self, RuleRange, bs_parts, rule_family};
use jetcarp_core::pattern::site::Site;
use jetcarp_core::pattern::verify::{Verification, default_periods, verify};
use jetcarp_core::pattern::{
    CurationState, PatternClass, PatternRecord, Provenance, now_utc, tiling_key,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::semantics;
use jetcarp_core::topology::{Nb, TilingSpec};
use rayon::prelude::*;
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

mod ants;
mod census;
mod compounds;
mod dashboard;
mod jobs;
pub mod pack;
pub mod provenance;
mod query;
pub mod quickstart;
pub mod schema;
pub use ants::{
    AntCensus, AntQuery, AntRow, AntUpdate, ant_id, ant_identity, ant_score, start_text,
};
mod ratings;
mod reverify;
mod unstored;
pub use compounds::{COMPOUND_TAG, CompoundCounts, CompoundSplit};
pub use dashboard::{FindRow, HeatQuery, HeatRow, UsageRow};
pub use jobs::{RemovedJob, StepEnd, StepWrite};
pub use provenance::{AliasRow, History, Origin, ProvenanceRow};
pub use query::{Query, Update, sort_records};
pub use quickstart::{AtlasWrite, BuiltinSync, MenuRow, QsSource, QuickStartRow, TARGET_KINDS};
pub use ratings::{Dedupe, RatingRow, RunReview, VERDICTS, search_run_target, verdict_rating};
pub use reverify::{Reverified, ReverifyCounts, ReverifyTarget};
pub use schema::{PinCheck, SCHEMA_VERSION, new_uuid};
pub use unstored::{PruneReport, UnstoredFlush, UnstoredRow};

/// The library's file name inside a folder.
pub const FILE: &str = "library.sqlite";

fn e(x: rusqlite::Error) -> String {
    format!("library: {x}")
}

/// A serde unit enum as its name (`still_life`, `touch`).
fn name_of<T: Serialize>(t: &T) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn parse_name<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(s.to_string()))
        .map_err(|x| format!("library: bad value '{s}': {x}"))
}

// ---------------------------------------------------------------------------------------------
// Public types

/// What [`Library::add`] did.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AddOutcome {
    /// A new pattern was stored.
    Added,
    /// The pattern was already stored; its seen count is now `seen`. `new_rule`: it had not been
    /// verified in this rule before.
    Merged { seen: u64, new_rule: bool },
}

/// Job status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Paused,
    Done,
    Failed,
}

/// One row of `jobs` (without the checkpoint blob; see [`Library::job_checkpoint`]).
#[derive(Clone, Debug, Serialize)]
pub struct JobRow {
    pub id: i64,
    pub kind: String,
    pub spec: Value,
    pub status: JobStatus,
    pub progress: Value,
    pub has_checkpoint: bool,
    pub created: String,
    pub updated: String,
    pub finished: Option<String>,
    /// Who runs it (`host:pid`), while a runner holds it (`jetcarp_search::jobs`).
    #[serde(skip)]
    pub owner: Option<String>,
    /// The runner's last heartbeat, seconds since 1970.
    #[serde(skip)]
    pub heartbeat: Option<i64>,
    /// The job's uuid (the search id: folders and provenance name it).
    #[serde(skip)]
    pub uuid: String,
}

impl JobRow {
    /// The job's label (`spec.label`, set with [`Library::set_job_label`]), if any: scripts
    /// label their jobs (`survey · Hexagon (touch)`) so they can be found again and told apart.
    pub fn label(&self) -> Option<&str> {
        self.spec.get("label").and_then(|l| l.as_str())
    }
}

/// Copies of a stored pattern seen by a job (for [`Library::add_sightings`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Sighting {
    pub pattern_id: String,
    /// From [`Library::rule_id`].
    pub rule_id: i64,
    pub count: u64,
    /// Soups it appeared in (0 if unknown).
    pub soups: u64,
}

/// One row of `sightings`, with the rule's notation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SightingRow {
    pub rule: String,
    pub job: i64,
    pub count: u64,
    pub soups: u64,
    pub first_seen: String,
    pub last_seen: String,
}

/// Row counts and the schema version (`jetcarp library info`).
#[derive(Clone, Debug, Default, Serialize)]
pub struct CatalogInfo {
    pub path: String,
    pub schema_version: i64,
    pub patterns: i64,
    pub rules: i64,
    pub pattern_rules: i64,
    pub sightings: i64,
    pub seen: i64,
    pub jobs: i64,
    pub quickstart: i64,
    pub ratings: i64,
    pub media: i64,
    pub ants: i64,
    /// Records flagged for re-verification (their geometry pin, or a pattern's rule-semantics
    /// version, is not the engine's): patterns, ants, and Quick Start entries.
    pub needs_reverify: i64,
}

// ---------------------------------------------------------------------------------------------
// The store

/// The pattern catalog (one SQLite file). Cheap to clone; clones share one connection, which is
/// opened on first use. Reading a catalog that does not exist yet gives empty results without
/// creating the file; the first write creates it.
#[derive(Clone, Debug)]
pub struct Library {
    path: PathBuf,
    conn: Arc<Mutex<Option<Connection>>>,
}

/// The pattern's large or rarely queried fields, stored as JSON in `patterns.body`.
#[derive(Serialize, Deserialize)]
struct Body {
    format: u32,
    tiling: TilingSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    displacement_xy: Option<[f64; 2]>,
    bbox: [i32; 4],
    phases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    site: Option<Site>,
    verified: Verification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mover: Option<MoverRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gun: Option<jetcarp_core::pattern::GunInfo>,
    provenance: Provenance,
}

const PCOLS: &str = "p.id, p.name, p.rule, p.neighborhood, p.steps_per_gen, p.tiling_name, \
    p.tile_types, p.class, p.period, p.dx, p.dy, p.speed, p.cells, p.min_cells, p.max_cells, \
    p.canonical, p.seen, p.discovered_by, p.tags, p.notes, p.score, p.rating, p.state, \
    p.created, p.updated, p.body, p.range_bits, p.birth_min, p.birth_max, p.survive_min, \
    p.survive_max, p.states_min, p.states_max, \
    (SELECT group_concat(r.notation, char(10) ORDER BY pr.first_seen, r.id) \
       FROM pattern_rules pr JOIN rules r ON r.id = pr.rule_id WHERE pr.pattern_id = p.id), \
    p.geometry_pin, p.engine_version, p.needs_reverify, p.semantics";

fn record_from_row(row: &rusqlite::Row) -> rusqlite::Result<Result<PatternRecord, String>> {
    let g_i = |i: usize| -> rusqlite::Result<i64> { row.get::<_, i64>(i) };
    let body = row.get_ref(25)?;
    let tags: String = row.get(18)?;
    let rules: Option<String> = row.get(33)?;
    let bits: Option<i64> = row.get(26)?;
    let range = match bits {
        Some(b) => Some(RuleRange::new(
            b as u32,
            [g_i(27)? as u32, g_i(28)? as u32],
            [g_i(29)? as u32, g_i(30)? as u32],
            [g_i(31)? as u8, g_i(32)? as u8],
        )),
        None => None,
    };
    let dx: Option<i64> = row.get(9)?;
    let dy: Option<i64> = row.get(10)?;
    let id: String = row.get(0)?;
    let nb: String = row.get(3)?;
    let class: String = row.get(7)?;
    let by: String = row.get(17)?;
    let state: String = row.get(22)?;
    let rating: Option<i64> = row.get(21)?;
    let parts = (|| -> Result<PatternRecord, String> {
        let text =
            unstored::unpack_body(body).map_err(|x| format!("library: pattern {id}: {x}"))?;
        let b: Body =
            serde_json::from_str(&text).map_err(|x| format!("library: pattern {id}: {x}"))?;
        Ok(PatternRecord {
            format: b.format,
            id: id.clone(),
            name: row.get(1).map_err(e)?,
            rule: row.get(2).map_err(e)?,
            neighborhood: parse_name::<Nb>(&nb)?,
            steps_per_gen: g_i(4).map_err(e)? as u32,
            tiling: b.tiling,
            tiling_name: row.get(5).map_err(e)?,
            tile_types: g_i(6).map_err(e)? as u32,
            class: parse_name(&class)?,
            period: g_i(8).map_err(e)? as u64,
            displacement: dx.zip(dy).map(|(x, y)| [x as i32, y as i32]),
            displacement_xy: b.displacement_xy,
            speed: row.get(11).map_err(e)?,
            cells: g_i(12).map_err(e)? as usize,
            min_cells: g_i(13).map_err(e)? as usize,
            max_cells: g_i(14).map_err(e)? as usize,
            bbox: b.bbox,
            canonical: row.get(15).map_err(e)?,
            phases: b.phases,
            site: b.site,
            verified: b.verified,
            mover: b.mover,
            gun: b.gun,
            seen: g_i(16).map_err(e)? as u64,
            provenance: b.provenance,
            discovered_by: parse_name(&by)?,
            tags: serde_json::from_str(&tags).unwrap_or_default(),
            notes: row.get(19).map_err(e)?,
            score: row.get(20).map_err(e)?,
            rating: rating.map(|r| r as u8),
            state: parse_name::<CurationState>(&state)?,
            created: row.get(23).map_err(e)?,
            updated: row.get(24).map_err(e)?,
            rules: rules
                .map(|s| s.lines().map(String::from).collect())
                .unwrap_or_default(),
            range,
            geometry_pin: row.get(34).map_err(e)?,
            engine_version: row.get(35).map_err(e)?,
            needs_reverify: row.get::<_, i64>(36).map_err(e)? != 0,
            semantics: row.get(37).map_err(e)?,
        })
    })();
    Ok(parts)
}

/// The short key of a tiling (its family, or its catalog id: `square`, `k3-019`), the
/// `tiling_key` columns.
pub fn short_tiling_key(t: &TilingSpec) -> String {
    t.params
        .as_ref()
        .and_then(|p| p.get("catalog"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| t.family.clone())
}

/// Write every column of a pattern row (insert or replace the values, keeping the row so its
/// `pattern_rules` and `sightings` stay). The pin key is the record's pin's; the engine's pin for
/// it is put in `engine_pins` when missing.
fn put_pattern(tx: &Transaction, r: &PatternRecord) -> Result<(), String> {
    let rule = Rule::parse(&r.rule)?.0;
    let sem = if r.semantics.is_empty() {
        semantics::FIRST
    } else {
        r.semantics.as_str()
    };
    let (pin_key, _) = schema::pin_for(tx, record_pin_spec(r))?;
    let body = serde_json::to_string(&Body {
        format: r.format,
        tiling: r.tiling.clone(),
        displacement_xy: r.displacement_xy,
        bbox: r.bbox,
        phases: r.phases.clone(),
        site: r.site.clone(),
        verified: r.verified.clone(),
        mover: r.mover.clone(),
        gun: r.gun.clone(),
        provenance: r.provenance.clone(),
    })
    .map_err(|x| x.to_string())?;
    // Compressed (a body is mostly phases and verification JSON; about a quarter the size).
    let body = unstored::pack_body(&body);
    let rg = r.range.as_ref();
    let mut st = tx
        .prepare_cached(
            "INSERT INTO patterns (id, model, tiling, tiling_key, tiling_name, tile_types,
                neighborhood, steps_per_gen, class, period, dx, dy, speed, cells, min_cells,
                max_cells, canonical, rule, range_bits, birth_min, birth_max, survive_min,
                survive_max, states_min, states_max, seen, score, rating, state, name, label,
                tags, notes, discovered_by, created, updated, body, emits, pin_key,
                geometry_pin, needs_reverify, engine_version, family, semantics)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33,
                ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44)
             ON CONFLICT (id) DO UPDATE SET model = excluded.model, tiling = excluded.tiling,
                tiling_key = excluded.tiling_key, tiling_name = excluded.tiling_name,
                tile_types = excluded.tile_types, neighborhood = excluded.neighborhood,
                steps_per_gen = excluded.steps_per_gen, class = excluded.class,
                period = excluded.period, dx = excluded.dx, dy = excluded.dy,
                speed = excluded.speed, cells = excluded.cells, min_cells = excluded.min_cells,
                max_cells = excluded.max_cells, canonical = excluded.canonical,
                rule = excluded.rule, range_bits = excluded.range_bits,
                birth_min = excluded.birth_min, birth_max = excluded.birth_max,
                survive_min = excluded.survive_min, survive_max = excluded.survive_max,
                states_min = excluded.states_min, states_max = excluded.states_max,
                seen = excluded.seen, score = excluded.score, rating = excluded.rating,
                state = excluded.state, name = excluded.name, label = excluded.label,
                tags = excluded.tags, notes = excluded.notes,
                discovered_by = excluded.discovered_by, updated = excluded.updated,
                body = excluded.body, emits = excluded.emits, pin_key = excluded.pin_key,
                geometry_pin = excluded.geometry_pin, needs_reverify = excluded.needs_reverify,
                engine_version = excluded.engine_version, family = excluded.family,
                semantics = excluded.semantics",
        )
        .map_err(e)?;
    st.execute(params![
        r.id,
        range::cell_model(&rule),
        tiling_key(&r.tiling),
        short_tiling_key(&r.tiling),
        r.tiling_name,
        r.tile_types as i64,
        name_of(&r.neighborhood),
        r.steps_per_gen as i64,
        name_of(&r.class),
        r.period as i64,
        r.displacement.map(|d| d[0] as i64),
        r.displacement.map(|d| d[1] as i64),
        r.speed,
        r.cells as i64,
        r.min_cells as i64,
        r.max_cells as i64,
        r.canonical,
        r.rule,
        rg.map(|x| x.bits as i64),
        rg.map(|x| x.birth_min as i64),
        rg.map(|x| x.birth_max as i64),
        rg.map(|x| x.survive_min as i64),
        rg.map(|x| x.survive_max as i64),
        rg.map(|x| x.states_min as i64),
        rg.map(|x| x.states_max as i64),
        r.seen as i64,
        r.score,
        r.rating.map(|x| x as i64),
        name_of(&r.state),
        r.name,
        r.label(),
        serde_json::to_string(&r.tags).unwrap_or_else(|_| "[]".into()),
        r.notes,
        name_of(&r.discovered_by),
        r.created,
        r.updated,
        body,
        r.gun.as_ref().map(|g| g.ship.clone()),
        pin_key,
        r.geometry_pin,
        r.needs_reverify as i64,
        r.engine_version,
        semantics::family(&rule),
        sem,
    ])
    .map_err(e)?;
    Ok(())
}

/// The tiling a record's pin is the pin of: its site's patch on aperiodic tilings, else its
/// tiling.
fn record_pin_spec(r: &PatternRecord) -> &TilingSpec {
    r.site.as_ref().map(|s| &s.patch).unwrap_or(&r.tiling)
}

fn load(c: &Connection, id: &str) -> Result<Option<PatternRecord>, String> {
    let mut st = c
        .prepare_cached(&format!("SELECT {PCOLS} FROM patterns p WHERE p.id = ?1"))
        .map_err(e)?;
    st.query_row([id], record_from_row)
        .optional()
        .map_err(e)?
        .transpose()
}

fn rule_id_in(c: &Connection, notation: &str, nb: Nb, spg: u32) -> Result<i64, String> {
    let rule = Rule::parse(notation)?.0;
    let notation = rule.notation();
    let nbs = name_of(&nb);
    let mut find = c
        .prepare_cached(
            "SELECT id FROM rules WHERE notation = ?1 AND neighborhood = ?2 AND steps_per_gen = ?3",
        )
        .map_err(e)?;
    if let Some(id) = find
        .query_row(params![notation, nbs, spg as i64], |r| r.get::<_, i64>(0))
        .optional()
        .map_err(e)?
    {
        return Ok(id);
    }
    let states = match rule.field_kind() {
        jetcarp_core::rules::FieldKind::Binary => 2,
        jetcarp_core::rules::FieldKind::Multi(n) => n as i64,
        _ => 0,
    };
    let bs = bs_parts(&rule);
    c.prepare_cached(
        "INSERT INTO rules (notation, neighborhood, steps_per_gen, family, states, birth, survive,
            created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .map_err(e)?
    .execute(params![
        notation,
        nbs,
        spg as i64,
        rule_family(&rule),
        states,
        bs.map(|b| b.0 as i64),
        bs.map(|b| b.1 as i64),
        now_utc(),
    ])
    .map_err(e)?;
    Ok(c.last_insert_rowid())
}

fn put_sighting(
    c: &Connection,
    pattern: &str,
    rule: i64,
    job: i64,
    count: u64,
    soups: u64,
    now: &str,
) -> Result<(), String> {
    c.prepare_cached(
        "INSERT INTO sightings (pattern_id, rule_id, job_id, count, soups, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT (pattern_id, rule_id, job_id) DO UPDATE SET count = count + excluded.count,
            soups = soups + excluded.soups, last_seen = excluded.last_seen",
    )
    .map_err(e)?
    .execute(params![pattern, rule, job, count as i64, soups as i64, now])
    .map_err(e)?;
    Ok(())
}

/// Credit sightings of stored patterns to a job and update their `seen` and score once each.
fn sightings_in(tx: &Transaction, job: i64, s: &[Sighting], now: &str) -> Result<(), String> {
    let mut per: HashMap<&str, u64> = HashMap::new();
    for x in s {
        put_sighting(tx, &x.pattern_id, x.rule_id, job, x.count, x.soups, now)?;
        *per.entry(x.pattern_id.as_str()).or_default() += x.count;
    }
    let mut get = tx
        .prepare_cached("SELECT class, period, cells, speed, seen FROM patterns WHERE id = ?1")
        .map_err(e)?;
    let mut set = tx
        .prepare_cached("UPDATE patterns SET seen = ?2, score = ?3 WHERE id = ?1")
        .map_err(e)?;
    let mut ids: Vec<(&str, u64)> = per.into_iter().collect();
    ids.sort_unstable();
    for (id, n) in ids {
        let (class, period, cells, speed, seen): (String, i64, i64, Option<String>, i64) = get
            .query_row([id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(|x| format!("catalog: sighting of unknown pattern {id}: {x}"))?;
        let seen = seen as u64 + n;
        let score = jetcarp_core::pattern::interest_score(
            parse_name::<PatternClass>(&class)?,
            period as u64,
            cells as usize,
            seen,
            speed.as_deref(),
        );
        set.execute(params![id, seen as i64, score]).map_err(e)?;
    }
    Ok(())
}

/// Store or merge one verified record (inside a transaction).
fn add_in(
    tx: &Transaction,
    mut rec: PatternRecord,
    job: i64,
    soups: u64,
    origin: &Origin,
) -> Result<AddOutcome, String> {
    let now = now_utc();
    let rid = rule_id_in(tx, &rec.rule, rec.neighborhood, rec.steps_per_gen)?;
    let had_rule: bool = tx
        .prepare_cached("SELECT 1 FROM pattern_rules WHERE pattern_id = ?1 AND rule_id = ?2")
        .map_err(e)?
        .exists(params![rec.id, rid])
        .map_err(e)?;
    let verified = rec.verified.clone();
    let id = rec.id.clone();
    let count = rec.seen;
    let out = match load(tx, &rec.id)? {
        None => {
            rec.rules = vec![];
            rec.rescore();
            put_pattern(tx, &rec)?;
            provenance::first_in(
                tx,
                "pattern",
                &rec.id,
                origin,
                job,
                &rec.geometry_pin,
                &rec.provenance.source,
                // A record found elsewhere: the moment of the find, the earliest of its own date
                // and when it was stored there.
                if origin.found_by.is_some() {
                    found_date(&rec)
                } else {
                    &rec.provenance.date
                },
                &rec.provenance.jetcarp_version,
            )?;
            AddOutcome::Added
        }
        Some(mut old) => {
            old.seen += rec.seen;
            if rec.provenance.date < old.provenance.date && !rec.provenance.date.is_empty() {
                old.provenance = rec.provenance;
            }
            let farther = match (&rec.mover, &old.mover) {
                (Some(a), Some(b)) => a.distance_rings > b.distance_rings,
                (Some(_), None) => true,
                _ => false,
            };
            if farther {
                old.mover = rec.mover;
                old.verified = rec.verified;
            } else if rec.verified.periods > old.verified.periods {
                old.verified = rec.verified;
            }
            if old.name.is_none() {
                old.name = rec.name;
            }
            if old.range.is_none() {
                old.range = rec.range;
            }
            if old.gun.is_none() {
                old.gun = rec.gun;
            }
            old.updated = now.clone();
            old.rescore();
            put_pattern(tx, &old)?;
            AddOutcome::Merged {
                seen: old.seen,
                new_rule: !had_rule,
            }
        }
    };
    put_pattern_rule(tx, &id, rid, &verified, &now)?;
    // A record found elsewhere brings its seen count; no sighting is made up for it here.
    if count > 0 && origin.found_by.is_none() {
        put_sighting(tx, &id, rid, job, count, soups, &now)?;
    }
    Ok(out)
}

/// When a record was found: the earliest of its own provenance date and its `created` (dates
/// are UTC `YYYY-MM-DDTHH:MM:SSZ`, so they compare as text); either when the other is empty.
fn found_date(rec: &PatternRecord) -> &str {
    match (rec.provenance.date.as_str(), rec.created.as_str()) {
        ("", c) => c,
        (d, "") => d,
        (d, c) => d.min(c),
    }
}

/// A `pattern_rules` row: a rule a pattern was verified in (the longer verification kept).
fn put_pattern_rule(
    tx: &Transaction,
    id: &str,
    rid: i64,
    verified: &Verification,
    now: &str,
) -> Result<(), String> {
    tx.prepare_cached(
        "INSERT INTO pattern_rules (pattern_id, rule_id, verified, periods, engine_version,
            first_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (pattern_id, rule_id) DO UPDATE SET
            verified = CASE WHEN excluded.periods > periods THEN excluded.verified ELSE verified END,
            engine_version = CASE WHEN excluded.periods > periods THEN excluded.engine_version
                ELSE engine_version END,
            periods = max(periods, excluded.periods)",
    )
    .map_err(e)?
    .execute(params![
        id,
        rid,
        serde_json::to_string(verified).unwrap_or_default(),
        verified.periods as i64,
        if verified.jetcarp_version.is_empty() {
            jetcarp_core::VERSION
        } else {
            verified.jetcarp_version.as_str()
        },
        now
    ])
    .map_err(e)?;
    Ok(())
}

/// A record as the library stores it: in canonical form with this engine's geometry pin and its
/// content id ([`PatternRecord::canonicalize`]), verified (if not yet verified, or verified on
/// another geometry or another meaning of its rule: a stored pin, or a rule-semantics version,
/// that is not this engine's; a record without a version has the first), with this engine's
/// version of its rule's family, and given its rule range (if missing). Adding a record does
/// this first; a job that must know a record can be stored before it writes (its write is then
/// only the store) does it itself.
pub fn prepare(mut rec: PatternRecord) -> Result<PatternRecord, String> {
    let had_pin = std::mem::take(&mut rec.geometry_pin);
    rec.canonicalize()?;
    let engine = semantics::rule_version(&Rule::parse(&rec.rule)?.0);
    let had = match rec.semantics.as_str() {
        "" => semantics::FIRST,
        v => v,
    };
    if rec.verified.periods == 0
        || rec.needs_reverify
        || (!had_pin.is_empty() && had_pin != rec.geometry_pin)
        || had != engine
    {
        rec.verified = verify(&rec, default_periods(rec.period))?;
        rec.engine_version = jetcarp_core::VERSION.into();
    }
    rec.semantics = engine.into();
    rec.needs_reverify = false;
    if rec.engine_version.is_empty() {
        rec.engine_version = jetcarp_core::VERSION.into();
    }
    if rec.range.is_none() {
        rec.range = range::rule_range(&rec)?;
    }
    Ok(rec)
}

type Guard<'a> = MutexGuard<'a, Option<Connection>>;

impl Library {
    /// `JETCARP_LIBRARY` (a folder, or a `.sqlite` file), else `<data>/library.sqlite`
    /// ([`jetcarp_core::paths::data_dir`]).
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("JETCARP_LIBRARY").filter(|p| !p.is_empty()) {
            return Library::file_for(PathBuf::from(p));
        }
        jetcarp_core::paths::data_dir().join(FILE)
    }

    /// The library file for a path: itself if it names a `.sqlite`/`.db` file, else
    /// `<path>/library.sqlite`.
    pub fn file_for(p: PathBuf) -> PathBuf {
        let is_file = p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x, "sqlite" | "sqlite3" | "db"))
            || p.is_file();
        if is_file { p } else { p.join(FILE) }
    }

    pub fn open_default() -> Library {
        Library::open(Library::default_path())
    }

    /// A library at `path`: a `.sqlite` file, or a folder holding `library.sqlite`. Nothing is
    /// created until the first write.
    pub fn open(path: impl Into<PathBuf>) -> Library {
        Library {
            path: Library::file_for(path.into()),
            conn: Arc::new(Mutex::new(None)),
        }
    }

    /// The catalog file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The folder holding the catalog (media paths are relative to it).
    pub fn folder(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    fn guard(&self, create: bool) -> Result<Option<Guard<'_>>, String> {
        let mut g = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            if !create && !self.path.exists() {
                return Ok(None);
            }
            *g = Some(schema::open_db(&self.path)?);
        }
        Ok(Some(g))
    }

    /// Run a read; `empty` when the catalog does not exist yet.
    pub(crate) fn read<R>(
        &self,
        empty: R,
        f: impl FnOnce(&Connection) -> Result<R, String>,
    ) -> Result<R, String> {
        match self.guard(false)? {
            None => Ok(empty),
            Some(g) => f(g.as_ref().unwrap()),
        }
    }

    /// Run a write in one `BEGIN IMMEDIATE` transaction.
    pub(crate) fn write<R>(
        &self,
        f: impl FnOnce(&Transaction) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut g = self.guard(true)?.unwrap();
        let c = g.as_mut().unwrap();
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(e)?;
        let r = f(&tx)?;
        tx.commit().map_err(e)?;
        Ok(r)
    }

    // -----------------------------------------------------------------------------------------
    // Patterns

    /// Add a record (verifying it first unless it carries a verification, and computing its rule
    /// range), or merge it into the stored pattern: seen counts add up, the earliest provenance
    /// and the curation fields are kept, a longer verification replaces a shorter one, and the
    /// record's rule joins the pattern's rules. Counts as a manual sighting (job 0).
    pub fn add(&self, rec: PatternRecord) -> Result<AddOutcome, String> {
        self.add_in_job(rec, 0)
    }

    /// [`add`](Self::add) with the sighting credited to a job.
    pub fn add_in_job(&self, rec: PatternRecord, job: i64) -> Result<AddOutcome, String> {
        let rec = prepare(rec)?;
        self.write(|tx| add_in(tx, rec, job, 0, &Origin::found()))
    }

    /// Add many records in one transaction (verification and ranges computed in parallel first).
    /// One result per record, in order.
    pub fn add_many(
        &self,
        recs: Vec<PatternRecord>,
        job: Option<i64>,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        self.add_batch(
            recs.into_iter().map(|r| (r, 0)).collect(),
            job.unwrap_or(0),
            &Origin::found(),
        )
    }

    /// Add many records that came in another way ([`Origin`]): imported from elsewhere, or found
    /// elsewhere and imported with their history ([`Origin::found_elsewhere`]: provenance rows
    /// 0 and 1, the seen count kept, no sighting made up). Otherwise as
    /// [`add_many`](Self::add_many), with `job` the job that found them (0: none).
    pub fn add_many_as(
        &self,
        recs: Vec<PatternRecord>,
        job: i64,
        origin: &Origin,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        self.add_batch(recs.into_iter().map(|r| (r, 0)).collect(), job, origin)
    }

    /// [`add_many_as`](Self::add_many_as) with an origin per record (a pack's records each
    /// bring their own finder and history: [`Origin::with_history`]). With `prepare` false the
    /// records are stored as they are, without canonical form or verification, keeping their id
    /// and pin (an importer's records made on another geometry or meaning of their rule, stored
    /// flagged `needs_reverify` until `library reverify` checks them).
    pub fn add_many_each(
        &self,
        recs: Vec<(PatternRecord, Origin)>,
        job: i64,
        prepare_first: bool,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        let ready: Vec<(Result<PatternRecord, String>, Origin)> = recs
            .into_par_iter()
            .map(|(r, o)| (if prepare_first { prepare(r) } else { Ok(r) }, o))
            .collect();
        self.write(|tx| {
            let mut out = Vec::with_capacity(ready.len());
            for (r, origin) in ready {
                out.push(match r {
                    Ok(r) => Ok(add_in(tx, r, job, 0, &origin)?),
                    Err(x) => Err(x),
                });
            }
            Ok(out)
        })
    }

    fn add_batch(
        &self,
        recs: Vec<(PatternRecord, u64)>,
        job: i64,
        origin: &Origin,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        let ready: Vec<(Result<PatternRecord, String>, u64)> =
            recs.into_par_iter().map(|(r, s)| (prepare(r), s)).collect();
        self.write(|tx| {
            let mut out = Vec::with_capacity(ready.len());
            for (r, soups) in ready {
                out.push(match r {
                    Ok(r) => Ok(add_in(tx, r, job, soups, origin)?),
                    Err(x) => Err(x),
                });
            }
            Ok(out)
        })
    }

    /// Is a pattern with this exact id stored?
    pub fn contains(&self, id: &str) -> Result<bool, String> {
        self.read(false, |c| {
            c.prepare_cached("SELECT 1 FROM patterns WHERE id = ?1")
                .map_err(e)?
                .exists([id])
                .map_err(e)
        })
    }

    /// A record by id, unique id prefix (4+ characters; the first 8 are the short id), exact
    /// name (case-insensitive), or an old id or prefix kept in `id_aliases`. A prefix that
    /// matches both a record's id and an alias of another record is ambiguous.
    pub fn get(&self, key: &str) -> Result<PatternRecord, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("empty pattern id".into());
        }
        let found: Vec<PatternRecord> = self.read(vec![], |c| {
            if let Some(r) = load(c, key)? {
                return Ok(vec![r]);
            }
            let run = |sql: &str, arg: String| -> Result<Vec<PatternRecord>, String> {
                let mut st = c
                    .prepare(&format!(
                        "SELECT {PCOLS} FROM patterns p WHERE {sql} LIMIT 6"
                    ))
                    .map_err(e)?;
                let v = st
                    .query_map([arg], record_from_row)
                    .map_err(e)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(e)?;
                v.into_iter().collect()
            };
            let mut v = vec![];
            if key.len() >= 4 && key.chars().all(|c| c.is_ascii_hexdigit()) {
                v = run(
                    "p.id >= ?1 AND p.id < ?1 || 'g' ORDER BY p.id",
                    key.to_ascii_lowercase(),
                )?;
            }
            for id in provenance::alias_ids(c, "pattern", key)? {
                if !v.iter().any(|r| r.id == id)
                    && let Some(r) = load(c, &id)?
                {
                    v.push(r);
                }
            }
            if v.is_empty() {
                v = run("p.name = ?1 COLLATE NOCASE ORDER BY p.id", key.to_string())?;
            }
            Ok(v)
        })?;
        match found.len() {
            1 => Ok(found.into_iter().next().unwrap()),
            0 => Err(format!("no pattern '{key}' in {}", self.path.display())),
            n => Err(format!(
                "'{key}' matches {}{n} patterns ({}); give more of the id",
                if n > 5 { "more than " } else { "" },
                found
                    .iter()
                    .take(5)
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// Every record, highest score first.
    pub fn list(&self) -> Result<Vec<PatternRecord>, String> {
        self.query(&Query::default())
    }

    /// Records matching a query, in its order (default: highest score first).
    pub fn query(&self, q: &Query) -> Result<Vec<PatternRecord>, String> {
        let (wh, vals) = where_clause(q)?;
        let sql = format!(
            "SELECT {PCOLS} FROM patterns p {wh} ORDER BY {} LIMIT {} OFFSET {}",
            order_by(q.order.as_deref())?,
            q.limit.map(|n| n as i64).unwrap_or(-1),
            q.offset.unwrap_or(0)
        );
        self.read(vec![], |c| {
            let mut st = c.prepare(&sql).map_err(e)?;
            let v = st
                .query_map(rusqlite::params_from_iter(vals.iter()), record_from_row)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            v.into_iter().collect()
        })
    }

    /// How many records match a query (ignoring its limit and offset).
    pub fn count(&self, q: &Query) -> Result<u64, String> {
        let (wh, vals) = where_clause(q)?;
        self.read(0, |c| {
            c.query_row(
                &format!("SELECT count(*) FROM patterns p {wh}"),
                rusqlite::params_from_iter(vals.iter()),
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(e)
        })
    }

    /// Change curation fields. Returns the updated record.
    pub fn update(&self, key: &str, u: &Update) -> Result<PatternRecord, String> {
        let mut r = self.get(key)?;
        u.apply(&mut r)?;
        self.write(|tx| put_pattern(tx, &r))?;
        Ok(r)
    }

    /// Delete a pattern with its rules, sightings, Quick Start entry, and media rows (the media
    /// files stay until `jetcarp media gc`, as another record may share them).
    pub fn remove(&self, key: &str) -> Result<PatternRecord, String> {
        let r = self.get(key)?;
        self.write(|tx| {
            tx.execute("DELETE FROM patterns WHERE id = ?1", [&r.id])
                .map_err(e)?;
            tx.execute(
                "DELETE FROM quickstart WHERE target_kind = 'pattern' AND target_id = ?1",
                [&r.id],
            )
            .map_err(e)?;
            tx.execute(
                "DELETE FROM media WHERE target_kind = 'pattern' AND target_id = ?1",
                [&r.id],
            )
            .map_err(e)?;
            provenance::forget_in(tx, "pattern", &r.id)?;
            Ok(())
        })?;
        Ok(r)
    }

    /// Sightings of a pattern, by rule and job.
    pub fn sightings(&self, id: &str) -> Result<Vec<SightingRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT r.notation, s.job_id, s.count, s.soups, s.first_seen, s.last_seen
                     FROM sightings s JOIN rules r ON r.id = s.rule_id
                     WHERE s.pattern_id = ?1 ORDER BY s.first_seen, s.job_id, r.id",
                )
                .map_err(e)?;
            st.query_map([id], |r| {
                Ok(SightingRow {
                    rule: r.get(0)?,
                    job: r.get(1)?,
                    count: r.get::<_, i64>(2)? as u64,
                    soups: r.get::<_, i64>(3)? as u64,
                    first_seen: r.get(4)?,
                    last_seen: r.get(5)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Rules and sightings

    /// The `rules` row id for a rule as run (created if new).
    pub fn rule_id(&self, notation: &str, nb: Nb, steps_per_gen: u32) -> Result<i64, String> {
        self.write(|tx| rule_id_in(tx, notation, nb, steps_per_gen))
    }

    /// Credit copies of already stored patterns to a job, in one transaction: the fast path for
    /// hunters that recognise a pattern without verifying it again. Each pattern's `seen` and
    /// score are updated once per call.
    pub fn add_sightings(&self, job: i64, s: &[Sighting]) -> Result<(), String> {
        let now = now_utc();
        self.write(|tx| sightings_in(tx, job, s, &now))
    }

    /// Ids of the patterns verified in a rule (`pattern_rules`).
    pub fn rule_patterns(&self, rule_id: i64) -> Result<Vec<String>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached("SELECT pattern_id FROM pattern_rules WHERE rule_id = ?1")
                .map_err(e)?;
            st.query_map([rule_id], |r| r.get::<_, String>(0))
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)
        })
    }

    /// Sightings credited to a job: (pattern id, rule notation, count, soups), most copies first.
    pub fn job_sightings(&self, job: i64) -> Result<Vec<(String, String, u64, u64)>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT s.pattern_id, r.notation, s.count, s.soups FROM sightings s
                     JOIN rules r ON r.id = s.rule_id WHERE s.job_id = ?1
                     ORDER BY s.count DESC, s.pattern_id, r.notation",
                )
                .map_err(e)?;
            st.query_map([job], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, i64>(3)? as u64,
                ))
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Jobs

    /// A new job row (status `running`) with a new uuid (the spec's `uuid` when it has one: an
    /// explore search's folder name). Returns its id.
    pub fn create_job(&self, kind: &str, spec: &Value) -> Result<i64, String> {
        let now = now_utc();
        let uuid = spec
            .get("uuid")
            .and_then(|u| u.as_str())
            .filter(|u| !u.is_empty())
            .map(String::from)
            .unwrap_or_else(new_uuid);
        self.write(|tx| {
            tx.execute(
                "INSERT INTO jobs (uuid, kind, spec, status, engine_version, created, updated)
                 VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?5)",
                params![uuid, kind, spec.to_string(), jetcarp_core::VERSION, now],
            )
            .map_err(e)?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// Label a job: `spec.label` is set to `label` (the spec structs ignore the extra key, so
    /// any kind of job can carry one). Scripts find their jobs again by label, which makes a
    /// script of labelled jobs resumable, and the dashboard shows it.
    pub fn set_job_label(&self, id: i64, label: &str) -> Result<(), String> {
        let mut j = self.job(id)?;
        if !j.spec.is_object() {
            j.spec = Value::Object(Default::default());
        }
        j.spec["label"] = Value::String(label.to_string());
        let spec = j.spec.to_string();
        self.write(|tx| {
            tx.execute("UPDATE jobs SET spec = ?2 WHERE id = ?1", params![id, spec])
                .map_err(e)?;
            Ok(())
        })
    }

    /// Replace a job's spec (new budgets on resume). The label is kept.
    pub fn set_job_spec(&self, id: i64, spec: &Value) -> Result<(), String> {
        let mut spec = spec.clone();
        if let Some(l) = self.job(id)?.label() {
            spec["label"] = Value::String(l.to_string());
        }
        let text = spec.to_string();
        self.write(|tx| {
            tx.execute("UPDATE jobs SET spec = ?2 WHERE id = ?1", params![id, text])
                .map_err(e)?;
            Ok(())
        })
    }

    /// Jobs with this label, oldest first.
    pub fn jobs_labelled(&self, label: &str) -> Result<Vec<JobRow>, String> {
        Ok(self
            .jobs()?
            .into_iter()
            .filter(|j| j.label() == Some(label))
            .collect())
    }

    /// Update a job's status, progress, and checkpoint (each only when given).
    pub fn update_job(
        &self,
        id: i64,
        status: Option<JobStatus>,
        progress: Option<&Value>,
        checkpoint: Option<&[u8]>,
    ) -> Result<(), String> {
        let now = now_utc();
        self.write(|tx| {
            let n = tx
                .execute(
                    "UPDATE jobs SET updated = ?2,
                        status = coalesce(?3, status),
                        progress = coalesce(?4, progress),
                        checkpoint = coalesce(?5, checkpoint),
                        finished = CASE WHEN ?3 IN ('done', 'failed') THEN ?2 ELSE finished END
                     WHERE id = ?1",
                    params![
                        id,
                        now,
                        status.map(|s| name_of(&s)),
                        progress.map(|p| p.to_string()),
                        checkpoint
                    ],
                )
                .map_err(e)?;
            if n == 0 {
                return Err(format!("no job {id}"));
            }
            Ok(())
        })
    }

    fn job_rows(&self, filter: &str, arg: Option<i64>) -> Result<Vec<JobRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT id, kind, spec, status, progress, checkpoint IS NOT NULL, created,
                        updated, finished, owner, heartbeat, uuid FROM jobs {filter} ORDER BY id"
                ))
                .map_err(e)?;
            let rows = st
                .query_map(rusqlite::params_from_iter(arg.iter()), |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, bool>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, Option<i64>>(10)?,
                        r.get::<_, String>(11)?,
                    ))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            rows.into_iter()
                .map(|x| {
                    Ok(JobRow {
                        id: x.0,
                        kind: x.1,
                        spec: serde_json::from_str(&x.2).unwrap_or(Value::Null),
                        status: parse_name(&x.3)?,
                        progress: serde_json::from_str(&x.4).unwrap_or(Value::Null),
                        has_checkpoint: x.5,
                        created: x.6,
                        updated: x.7,
                        finished: x.8,
                        owner: x.9,
                        heartbeat: x.10,
                        uuid: x.11,
                    })
                })
                .collect()
        })
    }

    /// One job.
    pub fn job(&self, id: i64) -> Result<JobRow, String> {
        self.job_rows("WHERE id = ?1", Some(id))?
            .pop()
            .ok_or_else(|| format!("no job {id}"))
    }

    /// Every job, oldest first (job 0 is "manual").
    pub fn jobs(&self) -> Result<Vec<JobRow>, String> {
        self.job_rows("", None)
    }

    /// A job's checkpoint blob.
    pub fn job_checkpoint(&self, id: i64) -> Result<Option<Vec<u8>>, String> {
        self.read(None, |c| {
            c.query_row("SELECT checkpoint FROM jobs WHERE id = ?1", [id], |r| {
                r.get::<_, Option<Vec<u8>>>(0)
            })
            .optional()
            .map(Option::flatten)
            .map_err(e)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Info

    /// Row counts and the schema version.
    pub fn info(&self) -> Result<CatalogInfo, String> {
        let path = self.path.display().to_string();
        let empty = CatalogInfo {
            path: path.clone(),
            ..Default::default()
        };
        self.read(empty, |c| {
            let n = |t: &str| -> Result<i64, String> {
                c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                    .map_err(e)
            };
            Ok(CatalogInfo {
                path,
                schema_version: c
                    .query_row("PRAGMA user_version", [], |r| r.get(0))
                    .map_err(e)?,
                patterns: n("patterns")?,
                rules: n("rules")?,
                pattern_rules: n("pattern_rules")?,
                sightings: n("sightings")?,
                seen: c
                    .query_row("SELECT coalesce(sum(count), 0) FROM sightings", [], |r| {
                        r.get(0)
                    })
                    .map_err(e)?,
                jobs: n("jobs")?,
                quickstart: n("quickstart")?,
                ratings: n("ratings")?,
                media: n("media")?,
                ants: n("ants")?,
                needs_reverify: c
                    .query_row(
                        "SELECT (SELECT count(*) FROM patterns WHERE needs_reverify = 1)
                            + (SELECT count(*) FROM ants WHERE needs_reverify = 1)
                            + (SELECT count(*) FROM quickstart WHERE needs_reverify = 1)",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(e)?,
            })
        })
    }
}

fn order_by(o: Option<&str>) -> Result<&'static str, String> {
    Ok(match o.unwrap_or("score").trim() {
        "" | "score" => "p.score DESC, p.rating DESC, p.id",
        "seen" | "common" => "p.seen DESC, p.score DESC, p.id",
        "rare" | "rarity" => "p.seen ASC, p.score DESC, p.id",
        "new" | "newest" | "created" => "p.created DESC, p.id",
        "updated" => "p.updated DESC, p.id",
        "cells" => "p.cells DESC, p.id",
        "period" => "p.period DESC, p.score DESC, p.id",
        "rating" => "p.rating DESC, p.score DESC, p.id",
        "id" => "p.id",
        x => {
            return Err(format!(
                "unknown order '{x}' (score, seen, rare, newest, updated, cells, period, rating, id)"
            ));
        }
    })
}

/// The WHERE clause and its parameters for a query.
fn where_clause(q: &Query) -> Result<(String, Vec<Sql>), String> {
    let mut w: Vec<String> = vec![];
    let mut v: Vec<Sql> = vec![];
    let arg = |v: &mut Vec<Sql>, x: Sql| -> String {
        v.push(x);
        format!("?{}", v.len())
    };
    let norm = |s: &str| s.to_ascii_lowercase().replace(' ', "");
    if let Some(r) = &q.rule {
        let a = arg(&mut v, Sql::Text(norm(r)));
        w.push(format!(
            "EXISTS (SELECT 1 FROM pattern_rules pr JOIN rules r ON r.id = pr.rule_id
                WHERE pr.pattern_id = p.id
                AND instr(lower(replace(r.notation, ' ', '')), {a}) > 0)"
        ));
    }
    if let Some(r) = &q.works_in {
        let (rule, nb_hint) = Rule::parse(r)?;
        let notation = rule.notation();
        let n = arg(&mut v, Sql::Text(notation));
        let exact = format!(
            "EXISTS (SELECT 1 FROM pattern_rules pr JOIN rules r ON r.id = pr.rule_id
                WHERE pr.pattern_id = p.id AND r.notation = {n})"
        );
        match bs_parts(&rule) {
            Some((b, s, c)) => {
                let nb = q.neighborhood.or(nb_hint).unwrap_or(Nb::Touch);
                let nb = arg(&mut v, Sql::Text(name_of(&nb)));
                let b = arg(&mut v, Sql::Integer(b as i64));
                let s = arg(&mut v, Sql::Integer(s as i64));
                let c = arg(&mut v, Sql::Integer(c as i64));
                w.push(format!(
                    "p.neighborhood = {nb} AND ((p.range_bits IS NOT NULL
                        AND (p.birth_min & ~{b}) = 0
                        AND (({b} & ((1 << p.range_bits) - 1)) & ~p.birth_max) = 0
                        AND (p.survive_min & ~{s}) = 0
                        AND (({s} & ((1 << p.range_bits) - 1)) & ~p.survive_max) = 0
                        AND {c} BETWEEN p.states_min AND p.states_max) OR {exact})"
                ));
            }
            None => w.push(exact),
        }
    } else if let Some(nb) = q.neighborhood {
        let a = arg(&mut v, Sql::Text(name_of(&nb)));
        w.push(format!("p.neighborhood = {a}"));
    }
    if let Some(t) = &q.tiling {
        let t = t.trim().to_ascii_lowercase();
        let qc = t.trim_start_matches("catalog:").to_string();
        if jetcarp_core::families::family(&t).is_some() || jetcarp_core::catalog::get(&qc).is_some()
        {
            let a = arg(&mut v, Sql::Text(qc));
            w.push(format!("p.tiling_key = {a} COLLATE NOCASE"));
        } else {
            let a = arg(&mut v, Sql::Text(t));
            w.push(format!("instr(lower(p.tiling_name), {a}) > 0"));
        }
    }
    if let Some(c) = q.class {
        let a = arg(&mut v, Sql::Text(name_of(&c)));
        w.push(format!("p.class = {a}"));
    }
    if let Some(x) = q.min_period {
        let a = arg(&mut v, Sql::Integer(x as i64));
        w.push(format!("p.period >= {a}"));
    }
    if let Some(x) = q.max_period {
        let a = arg(&mut v, Sql::Integer(x as i64));
        w.push(format!("p.period <= {a}"));
    }
    if let Some(x) = q.min_score {
        let a = arg(&mut v, Sql::Real(x));
        w.push(format!("p.score >= {a}"));
    }
    if let Some(x) = q.state {
        let a = arg(&mut v, Sql::Text(name_of(&x)));
        w.push(format!("p.state = {a}"));
    }
    if let Some(x) = q.discovered_by {
        let a = arg(&mut v, Sql::Text(name_of(&x)));
        w.push(format!("p.discovered_by = {a}"));
    }
    if let Some(t) = &q.tag {
        let a = arg(&mut v, Sql::Text(t.trim().to_ascii_lowercase()));
        w.push(format!(
            "EXISTS (SELECT 1 FROM json_each(p.tags) WHERE lower(json_each.value) = {a})"
        ));
    }
    if let Some(t) = &q.text {
        let a = arg(&mut v, Sql::Text(t.to_ascii_lowercase()));
        w.push(format!("instr(lower(p.label), {a}) > 0"));
    }
    Ok(if w.is_empty() {
        (String::new(), v)
    } else {
        (format!("WHERE {}", w.join(" AND ")), v)
    })
}

impl Update {
    /// Apply the changes to a record (and stamp `updated`, rescore).
    pub fn apply(&self, r: &mut PatternRecord) -> Result<(), String> {
        let u = self;
        if let Some(n) = &u.name {
            r.name = (!n.trim().is_empty()).then(|| n.trim().to_string());
        }
        if let Some(t) = &u.tags {
            r.tags = t
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        for t in &u.add_tags {
            let t = t.trim();
            if !t.is_empty() && !r.tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                r.tags.push(t.to_string());
            }
        }
        r.tags.retain(|x| {
            !u.remove_tags
                .iter()
                .any(|t| t.trim().eq_ignore_ascii_case(x))
        });
        if let Some(n) = &u.notes {
            r.notes = n.trim().to_string();
        }
        if let Some(n) = &u.append_notes {
            let n = n.trim();
            if !n.is_empty() {
                if !r.notes.is_empty() {
                    r.notes.push_str("\n\n");
                }
                r.notes.push_str(n);
            }
        }
        if let Some(x) = u.rating {
            if x > 5 {
                return Err("rating is 1 to 5 (0 clears it)".into());
            }
            r.rating = (x > 0).then_some(x);
        }
        if let Some(s) = u.state {
            r.state = s;
        }
        if let Some(d) = u.discovered_by {
            r.discovered_by = d;
        }
        r.updated = now_utc();
        r.rescore();
        Ok(())
    }
}
