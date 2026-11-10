//! The catalog: one SQLite file holding every pattern, the rules they work in, sightings, jobs,
//! census results, the Quick Start entries, ratings, ant runs, and media.
//!
//! **File.** `JETCARP_LIBRARY` if set (a folder, or a `.sqlite` file), else
//! `<data>/catalog.sqlite`. A folder means `<folder>/catalog.sqlite`. The file is
//! opened in WAL mode with a 30 s busy timeout, so the app, the CLI, scripts, and MCP servers can
//! read while a long hunt writes. Writes go through `BEGIN IMMEDIATE` transactions; batch them
//! ([`Library::add_many`], [`Library::add_sightings`]) for speed.
//!
//! **Schema versions.** `PRAGMA user_version` counts the migrations applied; [`MIGRATIONS`] is the
//! ordered list. Opening a catalog applies the missing ones (each in its own transaction). A later
//! change adds a migration at the end (`ALTER TABLE ... ADD COLUMN`, `CREATE TABLE ...`); never
//! edit one that has shipped. A catalog newer than this build is refused.
//!
//! | Table | One row per | Key columns |
//! | --- | --- | --- |
//! | `rules` | rule as run (notation, neighbourhood, steps per generation) | `id`, `notation`, `neighborhood`, `steps_per_gen`, `family` (`lifelike`, `generations`, `wireworld`, `table`, `priority`, ...), `states`, `birth`/`survive` bitmasks (B/S and Generations) |
//! | `patterns` | rule-free pattern ([`super::range::identity`]) | `id`, `model`, `tiling` (identity JSON), `tiling_id` (family or catalog id), `neighborhood`, `class`, `period`, `dx`/`dy`, `cells`, `canonical`, `rule` (first rule verified in, used to run it), the rule range (`range_bits`, `birth_min`, `birth_max`, `survive_min`, `survive_max`, `states_min`, `states_max`; NULL without a range), `seen` (sum of sightings), `score`, curation (`name`, `label`, `tags` JSON, `notes`, `rating`, `state`, `discovered_by`), `body` (JSON: phases, bbox, site, verification, mover, first-find provenance) |
//! | `pattern_rules` | (pattern, rule) it was verified in | `verified` (JSON), `periods`, `first_seen` |
//! | `sightings` | (pattern, rule, job) | `count` (copies), `soups` (soups it appeared in), `first_seen`, `last_seen`; job 0 is "manual" (adds outside a job) |
//! | `jobs` | search or hunt | `kind`, `spec` (JSON), `status` (`running`, `paused`, `done`, `failed`), `progress` (JSON), `checkpoint` (BLOB) |
//! | `census_rows` | rule of a census job | `job_id`, `idx`, `rule`, `summary` (JSON) |
//! | `quickstart` | Quick Start entry from any source | `id` (pattern id, `sweep-<hash>`), `source_kind` (`pattern`, `sweep`, `scene`), `source_id`, `title`, `description`, `grid`, `tiling`, `rule`, `rule_family`, `behaviour`, `scene` (JSON), `score`, `status` (`auto`, `candidate`, `approved`, `rejected`) |
//! | `quickstart_usage` | Quick Start entry id as the app knows it | `opens`, `first_opened`, `last_opened` (local; not exported) |
//! | `ratings` | rating of a pattern, an ant run, or a Quick Start entry | `target_kind`, `target_id`, `rater` (`user` or `model:<name>`), `rating` (1-5), `note` |
//! | `ants` | ant or turmite run: rule, tiling, start ([`ants`]) | `id` (`ant-<hash>`), `rule`, `kind` (`string`, `table`), `states`, `colors`, `tiling`, `tiling_id`, `start`, `outcome` (`highway`, `periodic`, `symmetric`, `bounded`, `chaotic`), `period`, `dx`/`dy`, `speed`, `onset`, `steps`, `cells`, `body` (JSON: the run with its proof, and the limits), `score`, curation, `job_id` |
//! | `media` | picture or animation ([`crate::media`]) | `target_kind`, `target_id`, `kind` (`frames`, `gif`, `webp`, `poster`, `thumb`), `path` (relative to the catalog's folder), `from_seed`, `render` (JSON), `width`, `height`, `frames`, `bytes`, `key` (render key; another key means stale), `hash` (content hash) |
//!
//! The tables `settings` and `score_models` of the old stage 1 settings search (migration 3) are
//! still created, but nothing reads or writes them; the schema v1 of the data layer drops them.
//!
//! "Every pattern that works in rule R" is [`Query::works_in`](super::Query): the range test on
//! the bitmasks (`birth_min & ~B = 0`, `B & ~birth_max = 0`, the same for S, and the state count
//! between `states_min` and `states_max`) or an explicit `pattern_rules` row.

use super::movers::MoverRecord;
use super::range::{self, RuleRange, bs_parts, rule_family};
use super::site::Site;
use super::verify::{Verification, default_periods, verify};
use super::{
    CurationState, PatternClass, PatternRecord, Provenance, Query, Update, now_utc, tiling_key,
};
use crate::rules::Rule;
use crate::topology::{Nb, TilingSpec};
use crate::world::Scene;
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
mod dashboard;
pub use ants::{AntCensus, AntQuery, AntRow, AntUpdate, ant_id, ant_score};
mod ratings;
mod unstored;
pub use dashboard::{FindRow, HeatQuery, HeatRow, UsageRow};
pub use ratings::RatingRow;
pub use unstored::{PruneReport, UnstoredFlush, UnstoredRow};

/// The catalog's file name inside a folder.
pub const FILE: &str = "catalog.sqlite";

/// Schema migrations, in order. `PRAGMA user_version` = how many have been applied.
pub const MIGRATIONS: &[&str] = &[
    MIGRATION_1,
    MIGRATION_2,
    MIGRATION_3,
    MIGRATION_MEDIA,
    ants::MIGRATION_ANTS,
    unstored::MIGRATION_UNSTORED,
];

/// Version 3 (Phase 4 stage 1): versions of the settings score (`score_models`), census jobs'
/// per-rule results (`census_rows`), and `settings` rebuilt with the setting itself (`spec`,
/// JSON), the score model, the search pass, the behaviour, and a `stored` status for settings
/// below the candidate threshold (SQLite cannot change a CHECK in place).
const MIGRATION_3: &str = r#"
CREATE TABLE score_models (
    id INTEGER PRIMARY KEY,
    target TEXT NOT NULL DEFAULT 'setting',
    source TEXT NOT NULL,
    model TEXT NOT NULL,
    ratings INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    created TEXT NOT NULL
);
CREATE INDEX score_models_active ON score_models (target, active, id);

CREATE TABLE settings_v3 (
    id INTEGER PRIMARY KEY,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    tiling TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    density REAL NOT NULL,
    soup_size TEXT NOT NULL,
    grid_size TEXT NOT NULL,
    boundary TEXT NOT NULL,
    generations INTEGER NOT NULL,
    soups INTEGER NOT NULL DEFAULT 1,
    seed INTEGER NOT NULL DEFAULT 1,
    spec TEXT NOT NULL DEFAULT '{}',
    descriptors TEXT NOT NULL DEFAULT '{}',
    score REAL,
    qs_status TEXT NOT NULL DEFAULT 'stored'
        CHECK (qs_status IN ('stored', 'auto', 'candidate', 'approved', 'rejected')),
    model_id INTEGER REFERENCES score_models (id) ON DELETE SET NULL,
    pass TEXT NOT NULL DEFAULT '',
    behaviour TEXT NOT NULL DEFAULT '',
    job_id INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    UNIQUE (rule_id, tiling, density, soup_size, grid_size, boundary, generations)
);
INSERT INTO settings_v3 (id, rule_id, tiling, tiling_name, density, soup_size, grid_size,
    boundary, generations, soups, seed, descriptors, score, qs_status, job_id, created, updated)
    SELECT id, rule_id, tiling, tiling_name, density, soup_size, grid_size, boundary,
        generations, soups, seed, descriptors, score, qs_status, job_id, created, updated
    FROM settings;
DROP TABLE settings;
ALTER TABLE settings_v3 RENAME TO settings;
CREATE INDEX settings_score ON settings (score DESC);
CREATE INDEX settings_status ON settings (qs_status, score DESC);
CREATE INDEX settings_tiling ON settings (tiling);
CREATE INDEX settings_job ON settings (job_id, score DESC);
CREATE INDEX settings_rule ON settings (rule_id);

CREATE TABLE census_rows (
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    rule TEXT NOT NULL,
    summary TEXT NOT NULL,
    PRIMARY KEY (job_id, idx)
) WITHOUT ROWID;
"#;

/// Version 2 (Phase 4 hunter): guns record the spaceship they emit (`emits`, its pattern id),
/// so "every gun that emits this ship" is one indexed query.
const MIGRATION_2: &str = r#"
ALTER TABLE patterns ADD COLUMN emits TEXT;
CREATE INDEX patterns_emits ON patterns (emits) WHERE emits IS NOT NULL;
"#;

/// Media (Phase 4 wave 3, `crate::media`): each file's render key (a hash of the inputs, the
/// options, and the house style; a different key means the file is stale) and its content hash
/// (FNV-1a 64; for a frame folder, of every frame's hash), which the website export compares.
const MIGRATION_MEDIA: &str = r#"
ALTER TABLE media ADD COLUMN key TEXT;
ALTER TABLE media ADD COLUMN hash TEXT;
ALTER TABLE media ADD COLUMN updated TEXT;
"#;

const MIGRATION_1: &str = r#"
CREATE TABLE rules (
    id INTEGER PRIMARY KEY,
    notation TEXT NOT NULL,
    neighborhood TEXT NOT NULL,
    steps_per_gen INTEGER NOT NULL DEFAULT 1,
    family TEXT NOT NULL,
    states INTEGER NOT NULL,
    birth INTEGER,
    survive INTEGER,
    created TEXT NOT NULL,
    UNIQUE (notation, neighborhood, steps_per_gen)
);
CREATE INDEX rules_family ON rules (family);

CREATE TABLE patterns (
    id TEXT PRIMARY KEY,
    model TEXT NOT NULL,
    tiling TEXT NOT NULL,
    tiling_id TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    tile_types INTEGER NOT NULL,
    neighborhood TEXT NOT NULL,
    steps_per_gen INTEGER NOT NULL,
    class TEXT NOT NULL,
    period INTEGER NOT NULL,
    dx INTEGER,
    dy INTEGER,
    speed TEXT,
    cells INTEGER NOT NULL,
    min_cells INTEGER NOT NULL,
    max_cells INTEGER NOT NULL,
    canonical TEXT NOT NULL,
    rule TEXT NOT NULL,
    range_bits INTEGER,
    birth_min INTEGER,
    birth_max INTEGER,
    survive_min INTEGER,
    survive_max INTEGER,
    states_min INTEGER,
    states_max INTEGER,
    seen INTEGER NOT NULL DEFAULT 0,
    score REAL NOT NULL DEFAULT 0,
    rating INTEGER,
    state TEXT NOT NULL DEFAULT 'new',
    name TEXT,
    label TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    notes TEXT NOT NULL DEFAULT '',
    discovered_by TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    body TEXT NOT NULL
);
CREATE INDEX patterns_class ON patterns (class, score DESC);
CREATE INDEX patterns_tiling ON patterns (tiling_id, class);
CREATE INDEX patterns_score ON patterns (score DESC);
CREATE INDEX patterns_seen ON patterns (seen DESC);
CREATE INDEX patterns_state ON patterns (state);
CREATE INDEX patterns_range ON patterns (model, tiling, neighborhood);
CREATE INDEX patterns_name ON patterns (name COLLATE NOCASE);
CREATE INDEX patterns_created ON patterns (created);

CREATE TABLE pattern_rules (
    pattern_id TEXT NOT NULL REFERENCES patterns (id) ON DELETE CASCADE,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    verified TEXT NOT NULL,
    periods INTEGER NOT NULL,
    first_seen TEXT NOT NULL,
    PRIMARY KEY (pattern_id, rule_id)
) WITHOUT ROWID;
CREATE INDEX pattern_rules_rule ON pattern_rules (rule_id);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    spec TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'paused', 'done', 'failed')),
    progress TEXT NOT NULL DEFAULT '{}',
    checkpoint BLOB,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    finished TEXT
);
CREATE INDEX jobs_status ON jobs (status);
INSERT INTO jobs (id, kind, spec, status, created, updated)
    VALUES (0, 'manual', '{}', 'running', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

CREATE TABLE sightings (
    pattern_id TEXT NOT NULL REFERENCES patterns (id) ON DELETE CASCADE,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    count INTEGER NOT NULL,
    soups INTEGER NOT NULL DEFAULT 0,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    PRIMARY KEY (pattern_id, rule_id, job_id)
) WITHOUT ROWID;
CREATE INDEX sightings_rule ON sightings (rule_id);
CREATE INDEX sightings_job ON sightings (job_id);

CREATE TABLE settings (
    id INTEGER PRIMARY KEY,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    tiling TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    density REAL NOT NULL,
    soup_size TEXT NOT NULL,
    grid_size TEXT NOT NULL,
    boundary TEXT NOT NULL,
    generations INTEGER NOT NULL,
    soups INTEGER NOT NULL DEFAULT 1,
    seed INTEGER NOT NULL DEFAULT 1,
    descriptors TEXT NOT NULL DEFAULT '{}',
    score REAL,
    qs_status TEXT NOT NULL DEFAULT 'candidate'
        CHECK (qs_status IN ('auto', 'candidate', 'approved', 'rejected')),
    job_id INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    UNIQUE (rule_id, tiling, density, soup_size, grid_size, boundary, generations)
);
CREATE INDEX settings_score ON settings (score DESC);
CREATE INDEX settings_status ON settings (qs_status, score DESC);
CREATE INDEX settings_tiling ON settings (tiling);

CREATE TABLE quickstart (
    id TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('pattern', 'setting', 'sweep', 'scene')),
    source_id TEXT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    grid TEXT NOT NULL,
    tiling TEXT,
    rule TEXT NOT NULL,
    rule_family TEXT NOT NULL,
    behaviour TEXT NOT NULL DEFAULT '',
    scene TEXT NOT NULL,
    score REAL,
    status TEXT NOT NULL DEFAULT 'approved'
        CHECK (status IN ('auto', 'candidate', 'approved', 'rejected')),
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);
CREATE INDEX quickstart_status ON quickstart (status, score DESC);
CREATE INDEX quickstart_source ON quickstart (source_kind, source_id);

CREATE TABLE quickstart_usage (
    entry_id TEXT PRIMARY KEY,
    opens INTEGER NOT NULL DEFAULT 0,
    first_opened TEXT,
    last_opened TEXT
);

CREATE TABLE ratings (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'setting', 'quickstart')),
    target_id TEXT NOT NULL,
    rater TEXT NOT NULL,
    rating REAL NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created TEXT NOT NULL
);
CREATE INDEX ratings_target ON ratings (target_kind, target_id);

CREATE TABLE media (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'setting', 'quickstart')),
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    from_seed INTEGER NOT NULL DEFAULT 0,
    render TEXT NOT NULL DEFAULT '{}',
    width INTEGER,
    height INTEGER,
    frames INTEGER,
    bytes INTEGER,
    created TEXT NOT NULL,
    UNIQUE (target_kind, target_id, kind, from_seed)
);
"#;

fn e(x: rusqlite::Error) -> String {
    format!("catalog: {x}")
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
        .map_err(|x| format!("catalog: bad value '{s}': {x}"))
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

/// Status of a Quick Start entry: `auto` and `approved` are shown in the menu, `candidate` and
/// `rejected` hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QsStatus {
    /// Shown, picked by a search.
    Auto,
    /// Hidden until approved.
    Candidate,
    /// Shown (approved by the user, or added by hand).
    #[default]
    Approved,
    /// Hidden by the user.
    Rejected,
}

impl QsStatus {
    pub fn name(self) -> String {
        name_of(&self)
    }
    pub fn parse(s: &str) -> Result<QsStatus, String> {
        parse_name(&s.trim().to_ascii_lowercase())
            .map_err(|_| format!("unknown status '{s}' (auto, candidate, approved, rejected)"))
    }
    /// Shown in the Quick Start menu.
    pub fn shown(self) -> bool {
        matches!(self, QsStatus::Auto | QsStatus::Approved)
    }
}

/// One Quick Start entry (table `quickstart`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickStartRow {
    /// Pattern id, `sweep-<hash>`, or a scene's id. The app shows it as `find:<id>`.
    pub id: String,
    /// `pattern`, `sweep`, or `scene`.
    pub source_kind: String,
    pub source_id: Option<String>,
    pub title: String,
    pub description: String,
    /// Tiling display name.
    pub grid: String,
    /// Main rule notation and its family (`lifelike`, `generations`, ...).
    pub rule: String,
    pub rule_family: String,
    /// `still life`, `oscillator`, `spaceship`, a row class, ... (free text for now).
    pub behaviour: String,
    pub scene: Scene,
    pub score: Option<f64>,
    pub status: QsStatus,
    pub created: String,
    pub updated: String,
}

impl QuickStartRow {
    /// A row for a scene, with the grid and rule read from it.
    pub fn for_scene(id: &str, source_kind: &str, scene: Scene) -> QuickStartRow {
        let (rule, family) = scene
            .cas
            .first()
            .map(|c| {
                let fam = Rule::parse(&c.rule)
                    .map(|(r, _)| rule_family(&r).to_string())
                    .unwrap_or_default();
                (c.rule.clone(), fam)
            })
            .unwrap_or_default();
        let now = now_utc();
        QuickStartRow {
            id: id.to_string(),
            source_kind: source_kind.to_string(),
            source_id: None,
            title: scene.name.clone(),
            description: scene.description.clone(),
            grid: super::tiling_display_name(&scene.tiling),
            rule,
            rule_family: family,
            behaviour: String::new(),
            scene,
            score: None,
            status: QsStatus::Approved,
            created: now.clone(),
            updated: now,
        }
    }
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

/// What [`Library::store_search`] did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct StoredSearch {
    pub job: i64,
    /// Patterns stored for the first time.
    pub added: Vec<String>,
    /// Patterns already stored (their seen counts grew).
    pub merged: Vec<String>,
    /// (pattern, rule) pairs verified for the first time on a pattern already stored.
    pub new_rules: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
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
    gun: Option<super::GunInfo>,
    provenance: Provenance,
}

const PCOLS: &str = "p.id, p.name, p.rule, p.neighborhood, p.steps_per_gen, p.tiling_name, \
    p.tile_types, p.class, p.period, p.dx, p.dy, p.speed, p.cells, p.min_cells, p.max_cells, \
    p.canonical, p.seen, p.discovered_by, p.tags, p.notes, p.score, p.rating, p.state, \
    p.created, p.updated, p.body, p.range_bits, p.birth_min, p.birth_max, p.survive_min, \
    p.survive_max, p.states_min, p.states_max, \
    (SELECT group_concat(r.notation, char(10) ORDER BY pr.first_seen, r.id) \
       FROM pattern_rules pr JOIN rules r ON r.id = pr.rule_id WHERE pr.pattern_id = p.id)";

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
            unstored::unpack_body(body).map_err(|x| format!("catalog: pattern {id}: {x}"))?;
        let b: Body =
            serde_json::from_str(&text).map_err(|x| format!("catalog: pattern {id}: {x}"))?;
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
        })
    })();
    Ok(parts)
}

fn tiling_id(t: &TilingSpec) -> String {
    t.params
        .as_ref()
        .and_then(|p| p.get("catalog"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| t.family.clone())
}

/// Write every column of a pattern row (insert or replace the values, keeping the row so its
/// `pattern_rules` and `sightings` stay).
fn put_pattern(tx: &Transaction, r: &PatternRecord) -> Result<(), String> {
    let rule = Rule::parse(&r.rule)?.0;
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
            "INSERT INTO patterns (id, model, tiling, tiling_id, tiling_name, tile_types,
                neighborhood, steps_per_gen, class, period, dx, dy, speed, cells, min_cells,
                max_cells, canonical, rule, range_bits, birth_min, birth_max, survive_min,
                survive_max, states_min, states_max, seen, score, rating, state, name, label,
                tags, notes, discovered_by, created, updated, body, emits)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33,
                ?34, ?35, ?36, ?37, ?38)
             ON CONFLICT (id) DO UPDATE SET model = excluded.model, tiling = excluded.tiling,
                tiling_id = excluded.tiling_id, tiling_name = excluded.tiling_name,
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
                body = excluded.body, emits = excluded.emits",
        )
        .map_err(e)?;
    st.execute(params![
        r.id,
        range::cell_model(&rule),
        tiling_key(&r.tiling),
        tiling_id(&r.tiling),
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
    ])
    .map_err(e)?;
    Ok(())
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
        crate::rules::FieldKind::Binary => 2,
        crate::rules::FieldKind::Multi(n) => n as i64,
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
        let score = super::interest_score(
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
    tx.prepare_cached(
        "INSERT INTO pattern_rules (pattern_id, rule_id, verified, periods, first_seen)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (pattern_id, rule_id) DO UPDATE SET
            verified = CASE WHEN excluded.periods > periods THEN excluded.verified ELSE verified END,
            periods = max(periods, excluded.periods)",
    )
    .map_err(e)?
    .execute(params![
        id,
        rid,
        serde_json::to_string(&verified).unwrap_or_default(),
        verified.periods as i64,
        now
    ])
    .map_err(e)?;
    if count > 0 {
        put_sighting(tx, &id, rid, job, count, soups, &now)?;
    }
    Ok(out)
}

/// Verify (if not yet verified) and compute the rule range (if missing).
fn prepare(mut rec: PatternRecord) -> Result<PatternRecord, String> {
    if rec.verified.periods == 0 {
        rec.verified = verify(&rec, default_periods(rec.period))?;
    }
    if rec.range.is_none() {
        rec.range = range::rule_range(&rec)?;
    }
    Ok(rec)
}

fn open_db(path: &Path) -> Result<Connection, String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|x| format!("creating {}: {x}", d.display()))?;
    }
    let mut c = Connection::open(path).map_err(|x| format!("{}: {x}", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(e)?;
    c.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))
        .map_err(e)?;
    c.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")
        .map_err(e)?;
    migrate(&mut c).map_err(|x| format!("{}: {x}", path.display()))?;
    Ok(c)
}

/// Apply the migrations this catalog lacks.
fn migrate(c: &mut Connection) -> Result<(), String> {
    let have: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(e)?;
    if have as usize > MIGRATIONS.len() {
        return Err(format!(
            "the catalog has schema version {have}, newer than this JETCARP ({}); update JETCARP",
            MIGRATIONS.len()
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(have as usize) {
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(e)?;
        // Another process may have migrated while we waited for the lock.
        let now: i64 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(e)?;
        if now as usize > i {
            continue;
        }
        tx.execute_batch(sql).map_err(e)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .map_err(e)?;
        tx.commit().map_err(e)?;
    }
    Ok(())
}

type Guard<'a> = MutexGuard<'a, Option<Connection>>;

impl Library {
    /// `JETCARP_LIBRARY` (a folder, or a `.sqlite` file), else `<data>/catalog.sqlite`
    /// ([`crate::paths::data_dir`]).
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("JETCARP_LIBRARY").filter(|p| !p.is_empty()) {
            return Library::file_for(PathBuf::from(p));
        }
        crate::paths::data_dir().join(FILE)
    }

    /// The catalog file for a path: itself if it names a `.sqlite`/`.db` file, else
    /// `<path>/catalog.sqlite`.
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

    /// A catalog at `path`: a `.sqlite` file, or a folder holding `catalog.sqlite`. Nothing is
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
            *g = Some(open_db(&self.path)?);
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
        self.write(|tx| add_in(tx, rec, job, 0))
    }

    /// Add many records in one transaction (verification and ranges computed in parallel first).
    /// One result per record, in order.
    pub fn add_many(
        &self,
        recs: Vec<PatternRecord>,
        job: Option<i64>,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        self.add_batch(recs.into_iter().map(|r| (r, 0)).collect(), job.unwrap_or(0))
    }

    fn add_batch(
        &self,
        recs: Vec<(PatternRecord, u64)>,
        job: i64,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        let ready: Vec<(Result<PatternRecord, String>, u64)> =
            recs.into_par_iter().map(|(r, s)| (prepare(r), s)).collect();
        self.write(|tx| {
            let mut out = Vec::with_capacity(ready.len());
            for (r, soups) in ready {
                out.push(match r {
                    Ok(r) => Ok(add_in(tx, r, job, soups)?),
                    Err(x) => Err(x),
                });
            }
            Ok(out)
        })
    }

    /// Store a soup search's verified records as one job (`kind` `search`, the spec, progress
    /// done), with each pattern's copies and soups as sightings.
    pub fn store_search(&self, out: &super::search::SearchOutput) -> Result<StoredSearch, String> {
        let spec = serde_json::to_value(&out.report.spec).unwrap_or(Value::Null);
        let job = self.create_job("search", &spec)?;
        let soups_total: u64 = out.report.groups.iter().map(|g| g.soups as u64).sum();
        let items: Vec<(PatternRecord, u64)> = out
            .records
            .iter()
            .cloned()
            .zip(
                out.soups
                    .iter()
                    .map(|&s| s as u64)
                    .chain(std::iter::repeat(0)),
            )
            .collect();
        let res = self.add_batch(items, job)?;
        let mut st = StoredSearch {
            job,
            ..Default::default()
        };
        for (r, o) in out.records.iter().zip(res) {
            match o {
                Ok(AddOutcome::Added) => st.added.push(r.id.clone()),
                Ok(AddOutcome::Merged { new_rule, .. }) => {
                    if new_rule {
                        st.new_rules.push((r.id.clone(), r.rule.clone()));
                    }
                    st.merged.push(r.id.clone())
                }
                Err(x) => st.failed.push((r.id.clone(), x)),
            }
        }
        self.update_job(
            job,
            Some(JobStatus::Done),
            Some(&serde_json::json!({
                "soups": soups_total, "soups_done": soups_total,
                "records": out.records.len(), "added": st.added.len(),
            })),
            None,
        )?;
        Ok(st)
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

    /// A record by id, unique id prefix (4+ characters), or exact name (case-insensitive).
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

    /// Delete a pattern with its rules, sightings, and Quick Start entry.
    pub fn remove(&self, key: &str) -> Result<PatternRecord, String> {
        let r = self.get(key)?;
        self.write(|tx| {
            tx.execute("DELETE FROM patterns WHERE id = ?1", [&r.id])
                .map_err(e)?;
            tx.execute(
                "DELETE FROM quickstart WHERE source_kind = 'pattern' AND source_id = ?1",
                [&r.id],
            )
            .map_err(e)?;
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

    /// One hunter flush, in one transaction so a crash never leaves counts without their
    /// checkpoint: store verified records (with `seen` copies in `soups` soups, credited to the
    /// job), credit sightings of patterns already stored, add and remove the job's unstored
    /// objects, and save the job's progress and checkpoint. Records must already be verified (see [`Library::add_many`] otherwise).
    pub fn hunt_flush(
        &self,
        job: i64,
        records: Vec<(PatternRecord, u64)>,
        sightings: &[Sighting],
        unstored: &UnstoredFlush,
        progress: &Value,
        checkpoint: &[u8],
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        let now = now_utc();
        self.write(|tx| {
            let mut out = Vec::with_capacity(records.len());
            for (r, soups) in records {
                out.push(if r.verified.periods == 0 {
                    Err(format!("{}: not verified", r.id))
                } else {
                    Ok(add_in(tx, r, job, soups)?)
                });
            }
            sightings_in(tx, job, sightings, &now)?;
            unstored::flush_in(tx, job, unstored)?;
            let n = tx
                .execute(
                    "UPDATE jobs SET updated = ?2, progress = ?3, checkpoint = ?4 WHERE id = ?1",
                    params![job, now, progress.to_string(), checkpoint],
                )
                .map_err(e)?;
            if n == 0 {
                return Err(format!("no job {job}"));
            }
            Ok(out)
        })
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

    /// A new job row (status `running`). Returns its id.
    pub fn create_job(&self, kind: &str, spec: &Value) -> Result<i64, String> {
        let now = now_utc();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO jobs (kind, spec, status, created, updated)
                 VALUES (?1, ?2, 'running', ?3, ?3)",
                params![kind, spec.to_string(), now],
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
                        updated, finished FROM jobs {filter} ORDER BY id"
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
    // Quick Start

    /// Insert or replace a Quick Start entry (keeping its creation date).
    pub fn put_quickstart(&self, q: &QuickStartRow) -> Result<(), String> {
        self.write(|tx| quickstart_in(tx, q))
    }

    /// Quick Start entries with one of these statuses (all when empty), oldest first.
    pub fn quickstart(&self, statuses: &[QsStatus]) -> Result<Vec<QuickStartRow>, String> {
        self.quickstart_where(statuses, None)
    }
    /// Quick Start entries with one of these statuses (all when empty), and one id when given.
    fn quickstart_where(
        &self,
        statuses: &[QsStatus],
        id: Option<&str>,
    ) -> Result<Vec<QuickStartRow>, String> {
        let mut filter = if statuses.is_empty() {
            String::new()
        } else {
            format!(
                "WHERE status IN ({})",
                statuses
                    .iter()
                    .map(|s| format!("'{}'", s.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if id.is_some() {
            filter.push_str(if filter.is_empty() {
                "WHERE id = ?1"
            } else {
                " AND id = ?1"
            });
        }
        self.read(vec![], |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT id, source_kind, source_id, title, description, grid, rule,
                        rule_family, behaviour, scene, score, status, created, updated
                     FROM quickstart {filter} ORDER BY created, id"
                ))
                .map_err(e)?;
            let rows = st
                .query_map(rusqlite::params_from_iter(id.iter()), |r| {
                    Ok((
                        (
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                        ),
                        (
                            r.get::<_, String>(7)?,
                            r.get::<_, String>(8)?,
                            r.get::<_, String>(9)?,
                            r.get::<_, Option<f64>>(10)?,
                            r.get::<_, String>(11)?,
                            r.get::<_, String>(12)?,
                            r.get::<_, String>(13)?,
                        ),
                    ))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            // Skip rows whose scene no longer loads rather than failing the whole list.
            Ok(rows
                .into_iter()
                .filter_map(|(a, b)| {
                    Some(QuickStartRow {
                        id: a.0,
                        source_kind: a.1,
                        source_id: a.2,
                        title: a.3,
                        description: a.4,
                        grid: a.5,
                        rule: a.6,
                        rule_family: b.0,
                        behaviour: b.1,
                        scene: Scene::from_json(&b.2).ok()?,
                        score: b.3,
                        status: parse_name(&b.4).ok()?,
                        created: b.5,
                        updated: b.6,
                    })
                })
                .collect())
        })
    }

    /// One Quick Start entry by id (with or without the `find:` prefix).
    pub fn get_quickstart(&self, id: &str) -> Result<QuickStartRow, String> {
        let id = id.trim().trim_start_matches("find:");
        self.quickstart_where(&[], Some(id))?
            .pop()
            .ok_or_else(|| format!("no Quick Start entry '{id}'"))
    }

    /// Set a Quick Start entry's status.
    pub fn set_quickstart_status(&self, id: &str, status: QsStatus) -> Result<(), String> {
        let id = id.trim().trim_start_matches("find:");
        self.write(|tx| {
            let n = tx
                .execute(
                    "UPDATE quickstart SET status = ?2, updated = ?3 WHERE id = ?1",
                    params![id, status.name(), now_utc()],
                )
                .map_err(e)?;
            if n == 0 {
                return Err(format!("no Quick Start entry '{id}'"));
            }
            Ok(())
        })
    }

    /// Delete a Quick Start entry.
    pub fn remove_quickstart(&self, id: &str) -> Result<(), String> {
        let id = id.trim().trim_start_matches("find:");
        self.write(|tx| {
            tx.execute("DELETE FROM quickstart WHERE id = ?1", [id])
                .map_err(e)?;
            Ok(())
        })
    }

    /// Changes whenever a Quick Start entry is added, changed, or removed (for the app's
    /// refresh check).
    pub fn quickstart_stamp(&self) -> String {
        self.read(String::new(), |c| {
            c.query_row(
                "SELECT count(*) || '|' || coalesce(max(updated), '') FROM quickstart",
                [],
                |r| r.get::<_, String>(0),
            )
            .map_err(e)
        })
        .unwrap_or_default()
    }

    /// Count an opening of a Quick Start entry (`find:<id>`, `rule:<notation>`, a bundled name).
    pub fn note_quickstart_open(&self, entry_id: &str) -> Result<(), String> {
        let now = now_utc();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO quickstart_usage (entry_id, opens, first_opened, last_opened)
                 VALUES (?1, 1, ?2, ?2)
                 ON CONFLICT (entry_id) DO UPDATE SET opens = opens + 1, last_opened = ?2",
                params![entry_id, now],
            )
            .map_err(e)?;
            Ok(())
        })
    }

    // -----------------------------------------------------------------------------------------
    // Export, import, info

    /// Write records as JSON files, one per pattern (`<dir>/<id>.json`, with its rules and rule
    /// range), all patterns when `keys` is empty. Returns the files written.
    pub fn export(&self, keys: &[String], dir: &Path) -> Result<Vec<PathBuf>, String> {
        let recs = if keys.is_empty() {
            self.list()?
        } else {
            keys.iter()
                .map(|k| self.get(k))
                .collect::<Result<Vec<_>, _>>()?
        };
        std::fs::create_dir_all(dir).map_err(|x| format!("creating {}: {x}", dir.display()))?;
        recs.iter()
            .map(|r| {
                let p = dir.join(format!("{}.json", r.id));
                std::fs::write(&p, r.to_json() + "\n")
                    .map_err(|x| format!("writing {}: {x}", p.display()))?;
                Ok(p)
            })
            .collect()
    }

    /// Merge patterns from JSON record files (a file, or a folder of them; a Phase 3 library
    /// folder with `patterns/` and `quickstart/` works too) or from another catalog file. Ids
    /// are recomputed and every record is verified again in each of its rules. Phase 3
    /// Quick Start scenes come along as `quickstart` rows.
    pub fn import(&self, src: &Path) -> Result<Vec<(String, AddOutcome)>, String> {
        let recs: Vec<PatternRecord> = if src.is_file()
            && src
                .extension()
                .is_some_and(|x| x == "sqlite" || x == "sqlite3" || x == "db")
        {
            Library::open(src).list()?
        } else {
            let files: Vec<PathBuf> = if src.is_file() {
                vec![src.to_path_buf()]
            } else {
                let dir = if src.join("patterns").is_dir() {
                    src.join("patterns")
                } else {
                    src.to_path_buf()
                };
                json_files(&dir)?
            };
            files
                .iter()
                .map(|f| {
                    let text = std::fs::read_to_string(f)
                        .map_err(|x| format!("reading {}: {x}", f.display()))?;
                    PatternRecord::from_json(&text).map_err(|x| format!("{}: {x}", f.display()))
                })
                .collect::<Result<_, _>>()?
        };
        // One record per rule it claims, each verified in that rule.
        let mut items = Vec::new();
        let mut old_ids = Vec::new();
        for r in recs {
            let mut rules = vec![r.rule.clone()];
            for x in &r.rules {
                if !rules.contains(x) {
                    rules.push(x.clone());
                }
            }
            for (k, rule) in rules.into_iter().enumerate() {
                let mut x = r.clone();
                x.rule = rule;
                x.rules = vec![];
                x.range = None;
                x.seen = if k == 0 { r.seen } else { 0 };
                x.id = range::identity(&x)?;
                old_ids.push((r.id.clone(), x.id.clone()));
                items.push(x);
            }
        }
        let checked: Vec<Result<PatternRecord, String>> = items
            .into_par_iter()
            .map(|mut r| {
                r.verified = verify(
                    &r,
                    default_periods(r.period).max(r.verified.periods.min(64)),
                )
                .map_err(|x| format!("{} in {}: {x}", r.label(), r.rule))?;
                Ok(r)
            })
            .collect();
        let checked: Vec<PatternRecord> = checked.into_iter().collect::<Result<_, _>>()?;
        let ids: Vec<String> = checked.iter().map(|r| r.id.clone()).collect();
        let res = self.add_many(checked, None)?;
        let mut out = Vec::new();
        for (id, r) in ids.into_iter().zip(res) {
            out.push((id, r?));
        }
        // Phase 3 promoted scenes.
        let qs = src.join("quickstart");
        if src.is_dir() && qs.is_dir() {
            let map: HashMap<String, String> = old_ids.into_iter().collect();
            for f in json_files(&qs)? {
                let Ok(sc) = Scene::from_json(&std::fs::read_to_string(&f).unwrap_or_default())
                else {
                    continue;
                };
                let stem = f
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let row = match map.get(&stem) {
                    Some(new) => {
                        let mut q = QuickStartRow::for_scene(new, "pattern", sc);
                        q.source_id = Some(new.clone());
                        q
                    }
                    None if stem.starts_with("sweep-") => {
                        QuickStartRow::for_scene(&stem, "sweep", sc)
                    }
                    None => QuickStartRow::for_scene(&stem, "scene", sc),
                };
                self.put_quickstart(&row)?;
            }
        }
        Ok(out)
    }

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
            })
        })
    }
}

/// Insert or replace a Quick Start entry (keeping its creation date), inside a transaction.
pub(super) fn quickstart_in(tx: &Transaction, q: &QuickStartRow) -> Result<(), String> {
    tx.execute(
        "INSERT INTO quickstart (id, source_kind, source_id, title, description, grid,
                tiling, rule, rule_family, behaviour, scene, score, status, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (id) DO UPDATE SET source_kind = excluded.source_kind,
                source_id = excluded.source_id, title = excluded.title,
                description = excluded.description, grid = excluded.grid,
                tiling = excluded.tiling, rule = excluded.rule,
                rule_family = excluded.rule_family, behaviour = excluded.behaviour,
                scene = excluded.scene, score = excluded.score, status = excluded.status,
                updated = excluded.updated",
        params![
            q.id,
            q.source_kind,
            q.source_id,
            q.title,
            q.description,
            q.grid,
            tiling_key(&q.scene.tiling),
            q.rule,
            q.rule_family,
            q.behaviour,
            q.scene.to_json(),
            q.score,
            q.status.name(),
            q.created,
            now_utc(),
        ],
    )
    .map_err(e)?;
    Ok(())
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|x| format!("reading {}: {x}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    v.sort();
    Ok(v)
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
        if crate::families::family(&t).is_some() || crate::catalog::get(&qc).is_some() {
            let a = arg(&mut v, Sql::Text(qc));
            w.push(format!("p.tiling_id = {a} COLLATE NOCASE"));
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
