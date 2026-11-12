//! The library's schema (version 3), opening a library file, and what is checked on opening.
//!
//! **Schema.** One script, [`SCHEMA`], makes a new library. `PRAGMA user_version` is the schema
//! version ([`SCHEMA_VERSION`], 3) and `PRAGMA application_id` marks the file as a JETCARP
//! library ([`APPLICATION_ID`], `JETC`), so a library from a newer JETCARP is refused with a
//! clear message, and so is a catalog from before schema 1 (made by a build before 0.4: no
//! application id, user versions 1 to 7), which is not migrated: JETCARP 0.4 starts a fresh
//! library.
//!
//! **Migrations.** A library of an older schema is brought up to date when it is opened, in one
//! transaction: schema 1 to 2 ([`migrate_1_2`]), 2 to 3 ([`migrate_2_3`]: one Quick Start
//! table for every source), and a library made before rule-semantics versions gains them
//! ([`add_semantics`]; schema 2 gained them before the 0.4.0 release, so its number stayed 2).
//! A schema change adds a migration here; never edit [`SCHEMA`] once it has shipped.
//!
//! **Geometry pins on opening.** Every pattern, ant run and Quick Start row stores the geometry
//! pin of the tiling it was verified on (`geometry_pin`) and the key of that pin (`pin_key`;
//! [`jetcarp_core::pin`]). `engine_pins` caches the running engine's pin for every key the
//! library uses, with the engine that computed it ([`jetcarp_core::pin::engine_geometry`]).
//! Opening a library computes again the pins an older (or newer) engine cached, then flags every
//! record whose pin differs from the engine's (`needs_reverify = 1`). Flagged records are kept
//! and listed, never deleted or failed silently; `jetcarp library reverify` checks them again.
//!
//! **Rule-semantics versions on opening.** Every pattern also stores its rule's family
//! (`family`) and the version of that family's meaning it was verified under (`semantics`;
//! [`jetcarp_core::semantics`]). Opening a library flags, in the same way, every pattern whose
//! version is not the running engine's for its family, or whose family the engine does not have.
//!
//! **Built-in Quick Start entries on opening.** Then, when `meta.builtin_version` is not this
//! build's ([`crate::quickstart::builtin_version`]: its engine and a hash of its built-in
//! entries), the built-in rows of the `quickstart` table are written again from the build's
//! entries ([`super::quickstart`]).

use super::{Library, e};
use jetcarp_core::pattern::now_utc;
use jetcarp_core::pin::{GeometryPin, engine_geometry, geometry_pin, pin_key, pin_spec};
use jetcarp_core::semantics;
use jetcarp_core::topology::TilingSpec;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashMap;

/// The schema version a library made by this build has (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = 3;

/// `PRAGMA application_id` of a JETCARP library: `JETC` in ASCII.
pub const APPLICATION_ID: i64 = 0x4A45_5443;

/// Schema 3's `quickstart` table: one row per Quick Start entry, from every source
/// ([`super::quickstart`]). A macro, so [`SCHEMA`] and [`migrate_2_3`] use one text.
macro_rules! quickstart_3 {
    () => {
        r#"
CREATE TABLE quickstart (
    id TEXT PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL CHECK (source IN ('builtin', 'pack', 'atlas', 'promotion')),
    source_ref TEXT NOT NULL DEFAULT '',
    target_kind TEXT NOT NULL
        CHECK (target_kind IN ('pattern', 'ant', 'sweep_run', 'search_run', 'scene')),
    target_id TEXT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT '',
    grid_group TEXT NOT NULL DEFAULT '',
    family TEXT NOT NULL DEFAULT '',
    tiling_key TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    rule TEXT NOT NULL,
    rule_family TEXT NOT NULL,
    behaviour TEXT NOT NULL DEFAULT '',
    scene TEXT NOT NULL,
    preview TEXT,
    mature_gen INTEGER NOT NULL DEFAULT 0,
    mature BLOB,
    score REAL,
    rank INTEGER NOT NULL DEFAULT 0,
    featured INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    pin_key TEXT NOT NULL,
    geometry_pin TEXT NOT NULL,
    needs_reverify INTEGER NOT NULL DEFAULT 0,
    engine_version TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);
CREATE INDEX quickstart_source ON quickstart (source, source_ref);
CREATE INDEX quickstart_target ON quickstart (target_kind, target_id);
CREATE INDEX quickstart_pin ON quickstart (pin_key, geometry_pin);
"#
    };
}

/// Schema 3's `quickstart` table.
pub const QUICKSTART_3: &str = quickstart_3!();

/// Schema 2's `media` table (content-addressed files: a hash and an extension, not a path).
const MEDIA_2: &str = r#"
CREATE TABLE media (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'quickstart', 'ant')),
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    from_seed INTEGER NOT NULL DEFAULT 0,
    hash TEXT NOT NULL,
    ext TEXT NOT NULL,
    render TEXT NOT NULL DEFAULT '{}',
    width INTEGER,
    height INTEGER,
    frames INTEGER,
    bytes INTEGER,
    key TEXT,
    created TEXT NOT NULL,
    updated TEXT,
    UNIQUE (target_kind, target_id, kind, from_seed)
);
CREATE INDEX media_hash ON media (hash);
"#;

/// Schema 1 (before the 0.4 release) to 2: ratings gain `verdict` and `source` (verdicts on
/// search runs); media files are content-addressed, so `media` names a file by its hash and
/// extension instead of a path. Media are always made again from their records, so the old
/// `media` rows go; their files, in the old per-record folders, are left alone (`media gc`
/// touches only `media/<two hex digits>/`). Each change is made only where it is missing (a
/// development build may have made part of it).
pub fn migrate_1_2(tx: &Transaction) -> Result<(), String> {
    let has = |table: &str, col: &str| has_column(tx, table, col);
    if !has("ratings", "verdict")? {
        tx.execute_batch(
            "ALTER TABLE ratings ADD COLUMN verdict TEXT
                CHECK (verdict IS NULL OR verdict IN ('keep', 'maybe', 'drop'));",
        )
        .map_err(e)?;
    }
    if !has("ratings", "source")? {
        tx.execute_batch("ALTER TABLE ratings ADD COLUMN source TEXT NOT NULL DEFAULT '';")
            .map_err(e)?;
    }
    if !has("media", "ext")? {
        tx.execute_batch(&format!("DROP TABLE IF EXISTS media; {MEDIA_2}"))
            .map_err(e)?;
    }
    tx.execute_batch("UPDATE meta SET value = '2' WHERE key = 'schema'; PRAGMA user_version = 2;")
        .map_err(e)
}

/// Schema 2 (before the 0.4 release) to 3: one `quickstart` table for every source, built-in,
/// pack, atlas and promotion, with handles and `hidden` instead of the statuses
/// ([`super::quickstart::migrate_rows_in`] keeps the rows it had, as promotions or pack rows),
/// and `quickstart_usage` counting entries by `handle`. The built-ins are written afterwards,
/// as on every opening. Each change is made only where it is missing.
pub fn migrate_2_3(tx: &Transaction) -> Result<(), String> {
    if has_column(tx, "quickstart", "status")? {
        super::quickstart::migrate_rows_in(tx)?;
    }
    if has_column(tx, "quickstart_usage", "entry_id")? {
        tx.execute_batch("ALTER TABLE quickstart_usage RENAME COLUMN entry_id TO handle;")
            .map_err(e)?;
    }
    tx.execute_batch(
        "UPDATE meta SET value = '3' WHERE key = 'schema';
         UPDATE meta SET value = '' WHERE key = 'builtin_version';
         PRAGMA user_version = 3;",
    )
    .map_err(e)
}

/// Does `table` have the column `col`?
fn has_column(c: &Connection, table: &str, col: &str) -> Result<bool, String> {
    c.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")
        .map_err(e)?
        .exists(params![table, col])
        .map_err(e)
}

/// Rule-semantics versions, for a library made before them (schema 1 or 2): `patterns` gains
/// `family`, its rule's family ([`semantics::family`], worked out once per distinct rule; empty
/// for a rule this engine cannot read, which is then flagged), and `semantics`, the version of
/// the family's meaning the record was verified under: `1` ([`semantics::FIRST`]), the version
/// every family had before versions existed.
pub fn add_semantics(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch(
        "ALTER TABLE patterns ADD COLUMN family TEXT NOT NULL DEFAULT '';
         ALTER TABLE patterns ADD COLUMN semantics TEXT NOT NULL DEFAULT '1';
         CREATE TEMP TABLE rule_families (rule TEXT PRIMARY KEY, family TEXT NOT NULL);",
    )
    .map_err(e)?;
    let rules: Vec<String> = {
        let mut st = tx
            .prepare("SELECT DISTINCT rule FROM patterns")
            .map_err(e)?;
        st.query_map([], |r| r.get(0))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?
    };
    {
        let mut put = tx
            .prepare("INSERT INTO temp.rule_families (rule, family) VALUES (?1, ?2)")
            .map_err(e)?;
        for r in &rules {
            let family = semantics::of_notation(r).map_or("", |(f, _)| f);
            put.execute(params![r, family]).map_err(e)?;
        }
    }
    tx.execute_batch(
        "UPDATE patterns SET family = coalesce(
             (SELECT f.family FROM temp.rule_families f WHERE f.rule = patterns.rule), '');
         DROP TABLE temp.rule_families;
         CREATE INDEX patterns_semantics ON patterns (family, semantics);",
    )
    .map_err(e)
}

/// The tables of schema 3.
pub const SCHEMA: &str = concat!(
    r#"
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE engine_pins (
    pin_key TEXT PRIMARY KEY,
    pin TEXT NOT NULL,
    polygon_hash TEXT NOT NULL,
    neighbour_hash TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    checked TEXT NOT NULL
) WITHOUT ROWID;

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
    tiling_key TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    pin_key TEXT NOT NULL,
    geometry_pin TEXT NOT NULL,
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
    emits TEXT,
    needs_reverify INTEGER NOT NULL DEFAULT 0,
    engine_version TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    body TEXT NOT NULL,
    family TEXT NOT NULL DEFAULT '',
    semantics TEXT NOT NULL DEFAULT '1'
);
CREATE INDEX patterns_class ON patterns (class, score DESC);
CREATE INDEX patterns_tiling ON patterns (tiling_key, class);
CREATE INDEX patterns_score ON patterns (score DESC);
CREATE INDEX patterns_seen ON patterns (seen DESC);
CREATE INDEX patterns_state ON patterns (state);
CREATE INDEX patterns_range ON patterns (model, tiling, neighborhood);
CREATE INDEX patterns_name ON patterns (name COLLATE NOCASE);
CREATE INDEX patterns_created ON patterns (created);
CREATE INDEX patterns_emits ON patterns (emits) WHERE emits IS NOT NULL;
CREATE INDEX patterns_pin ON patterns (pin_key, geometry_pin);
CREATE INDEX patterns_reverify ON patterns (id) WHERE needs_reverify = 1;
CREATE INDEX patterns_semantics ON patterns (family, semantics);

CREATE TABLE pattern_rules (
    pattern_id TEXT NOT NULL REFERENCES patterns (id) ON DELETE CASCADE,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    verified TEXT NOT NULL,
    periods INTEGER NOT NULL,
    engine_version TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    PRIMARY KEY (pattern_id, rule_id)
) WITHOUT ROWID;
CREATE INDEX pattern_rules_rule ON pattern_rules (rule_id);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    spec TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'paused', 'done', 'failed')),
    progress TEXT NOT NULL DEFAULT '{}',
    checkpoint BLOB,
    owner TEXT,
    heartbeat INTEGER,
    engine_version TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    finished TEXT
);
CREATE INDEX jobs_status ON jobs (status);

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

CREATE TABLE unstored (
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    rule_id INTEGER NOT NULL REFERENCES rules (id),
    pattern_id TEXT NOT NULL,
    class TEXT NOT NULL,
    period INTEGER NOT NULL,
    cells INTEGER NOT NULL,
    count INTEGER NOT NULL,
    soups INTEGER NOT NULL,
    first_soup INTEGER NOT NULL,
    rng_state INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (job_id, rule_id, pattern_id)
) WITHOUT ROWID;

CREATE TABLE census_rows (
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    rule TEXT NOT NULL,
    summary TEXT NOT NULL,
    PRIMARY KEY (job_id, idx)
) WITHOUT ROWID;

CREATE TABLE provenance (
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'ant', 'quickstart')),
    target_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('found', 'imported', 'reverified')),
    finder TEXT NOT NULL DEFAULT '',
    orcid TEXT NOT NULL DEFAULT '',
    search_kind TEXT NOT NULL DEFAULT '',
    job_uuid TEXT NOT NULL DEFAULT '',
    pack_hash TEXT NOT NULL DEFAULT '',
    machine TEXT NOT NULL DEFAULT '',
    engine_version TEXT NOT NULL,
    geometry_pin TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    PRIMARY KEY (target_kind, target_id, seq)
) WITHOUT ROWID;
CREATE TRIGGER provenance_row0_is_fixed BEFORE UPDATE ON provenance
    WHEN OLD.seq = 0 AND (NEW.seq, NEW.kind, NEW.finder, NEW.orcid, NEW.search_kind,
        NEW.job_uuid, NEW.pack_hash, NEW.machine, NEW.engine_version, NEW.geometry_pin,
        NEW.source, NEW.date) IS NOT (OLD.seq, OLD.kind, OLD.finder, OLD.orcid, OLD.search_kind,
        OLD.job_uuid, OLD.pack_hash, OLD.machine, OLD.engine_version, OLD.geometry_pin,
        OLD.source, OLD.date)
BEGIN
    SELECT RAISE(ABORT, 'a record''s first provenance row is never rewritten');
END;

CREATE TABLE id_aliases (
    alias TEXT PRIMARY KEY,
    target_kind TEXT NOT NULL DEFAULT 'pattern' CHECK (target_kind IN ('pattern', 'ant')),
    id TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('geometry-change')),
    created TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX id_aliases_id ON id_aliases (target_kind, id);

CREATE TABLE ants (
    id TEXT PRIMARY KEY,
    rule TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('string', 'table')),
    states INTEGER NOT NULL,
    colors INTEGER NOT NULL,
    tiling TEXT NOT NULL,
    tiling_key TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
    pin_key TEXT NOT NULL,
    geometry_pin TEXT NOT NULL,
    start TEXT NOT NULL,
    start_label TEXT NOT NULL,
    outcome TEXT NOT NULL
        CHECK (outcome IN ('highway', 'periodic', 'symmetric', 'bounded', 'chaotic')),
    period INTEGER,
    dx INTEGER,
    dy INTEGER,
    speed REAL,
    onset INTEGER,
    steps INTEGER NOT NULL,
    cells INTEGER NOT NULL,
    body TEXT NOT NULL,
    score REAL NOT NULL DEFAULT 0,
    rating INTEGER,
    state TEXT NOT NULL DEFAULT 'new',
    name TEXT,
    notes TEXT NOT NULL DEFAULT '',
    job_id INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    needs_reverify INTEGER NOT NULL DEFAULT 0,
    engine_version TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);
CREATE INDEX ants_outcome ON ants (outcome, tiling_key, score DESC);
CREATE INDEX ants_tiling ON ants (tiling, outcome);
CREATE INDEX ants_rule ON ants (rule);
CREATE INDEX ants_job ON ants (job_id);
CREATE INDEX ants_pin ON ants (pin_key, geometry_pin);

"#,
    quickstart_3!(),
    r#"
CREATE TABLE quickstart_usage (
    handle TEXT PRIMARY KEY,
    opens INTEGER NOT NULL DEFAULT 0,
    first_opened TEXT,
    last_opened TEXT
);

CREATE TABLE ratings (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'ant', 'quickstart', 'search_run')),
    target_id TEXT NOT NULL,
    rater TEXT NOT NULL,
    rating REAL NOT NULL,
    verdict TEXT CHECK (verdict IS NULL OR verdict IN ('keep', 'maybe', 'drop')),
    note TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT '',
    created TEXT NOT NULL
);
CREATE INDEX ratings_target ON ratings (target_kind, target_id);

CREATE TABLE media (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'quickstart', 'ant')),
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    from_seed INTEGER NOT NULL DEFAULT 0,
    hash TEXT NOT NULL,
    ext TEXT NOT NULL,
    render TEXT NOT NULL DEFAULT '{}',
    width INTEGER,
    height INTEGER,
    frames INTEGER,
    bytes INTEGER,
    key TEXT,
    created TEXT NOT NULL,
    updated TEXT,
    UNIQUE (target_kind, target_id, kind, from_seed)
);
CREATE INDEX media_hash ON media (hash);
"#
);

/// Is this a library of this schema (`Ok(true)`), a new empty file (`Ok(false)`), or neither
/// (an error saying why it is refused)?
fn check(c: &Connection) -> Result<bool, String> {
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(e)?;
    let app: i64 = c
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(e)?;
    let tables: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |r| r.get(0),
        )
        .map_err(e)?;
    if app == APPLICATION_ID {
        if version > SCHEMA_VERSION {
            return Err(format!(
                "the library has schema version {version}, newer than this JETCARP {} \
                 (schema {SCHEMA_VERSION}); update JETCARP to open it",
                jetcarp_core::VERSION
            ));
        }
        if version < 1 {
            return Err(format!("the library has no schema version ({version})"));
        }
        return Ok(true);
    }
    if tables > 0 || version != 0 {
        return Err(format!(
            "this is not a JETCARP library of schema {SCHEMA_VERSION} (it has schema version \
             {version} and no JETCARP mark): a catalog from a build before 0.4, which this \
             version does not open or migrate. Give JETCARP another library file (the default \
             is <data>/{})",
            super::FILE
        ));
    }
    Ok(false)
}

/// Bring a library of an older schema up to this one, in one transaction (and give one made
/// before rule-semantics versions its versions).
fn migrate(c: &mut Connection) -> Result<(), String> {
    let version = |c: &Connection| -> Result<i64, String> {
        c.query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(e)
    };
    let old = version(c)?;
    if old >= SCHEMA_VERSION && has_column(c, "patterns", "semantics")? {
        return Ok(());
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|x| {
            format!(
                "the library (schema {old}) needs updating to this JETCARP's schema \
                 {SCHEMA_VERSION} with rule-semantics versions, which needs writing to it: {x}"
            )
        })?;
    if version(&tx)? == 1 {
        migrate_1_2(&tx)?;
    }
    if !has_column(&tx, "patterns", "semantics")? {
        add_semantics(&tx)?;
    }
    if version(&tx)? == 2 {
        migrate_2_3(&tx)?;
    }
    tx.commit().map_err(e)
}

/// Make the tables of a new library, or check an existing one's version (and bring an older
/// schema up to date).
fn create_or_check(c: &mut Connection) -> Result<(), String> {
    // An existing library is only read here (so a read-only file opens); a new one is made in a
    // write transaction, its state read again inside it in case another process made it first.
    if check(c)? {
        return migrate(c);
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    if check(&tx)? {
        return Ok(());
    }
    tx.execute_batch(SCHEMA).map_err(e)?;
    let now = now_utc();
    for (k, v) in [
        ("schema", SCHEMA_VERSION.to_string()),
        ("library_uuid", new_uuid()),
        ("created", now.clone()),
        ("created_by", jetcarp_core::VERSION.to_string()),
        ("builtin_version", String::new()),
    ] {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![k, v],
        )
        .map_err(e)?;
    }
    // Job 0 holds what is added by hand.
    tx.execute(
        "INSERT INTO jobs (id, uuid, kind, spec, status, engine_version, created, updated)
         VALUES (0, ?1, 'manual', '{}', 'running', ?2, ?3, ?3)",
        params![new_uuid(), jetcarp_core::VERSION, now],
    )
    .map_err(e)?;
    tx.execute_batch(&format!(
        "PRAGMA user_version = {SCHEMA_VERSION}; PRAGMA application_id = {APPLICATION_ID};"
    ))
    .map_err(e)?;
    tx.commit().map_err(e)
}

/// Open (creating if new) a library file: WAL, a 30 s busy timeout, foreign keys, the schema
/// made or checked, then the geometry pins checked ([`check_pins`]).
pub(super) fn open_db(path: &std::path::Path) -> Result<Connection, String> {
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
    create_or_check(&mut c).map_err(|x| format!("{}: {x}", path.display()))?;
    if let Err(x) = check_pins(&mut c) {
        // A library that cannot be written (a read-only file) still opens for reading; its
        // stale records cannot be flagged in it, and `library reverify` will say why.
        if !c.is_readonly(rusqlite::MAIN_DB).unwrap_or(false) {
            return Err(format!("{}: {x}", path.display()));
        }
    }
    if let Err(x) = sync_builtins(&mut c) {
        // A read-only library keeps the built-ins it has; a failure to write them leaves the
        // library usable, with the reason said.
        if !c.is_readonly(rusqlite::MAIN_DB).unwrap_or(false) {
            eprintln!(
                "{}: writing the built-in Quick Start entries: {x}",
                path.display()
            );
        }
    }
    Ok(c)
}

/// Write the build's built-in Quick Start entries if the library has another build's (read
/// first, so an up-to-date library is not written to).
fn sync_builtins(c: &mut Connection) -> Result<(), String> {
    let have: Option<String> = c
        .query_row(
            "SELECT value FROM meta WHERE key = 'builtin_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(e)?;
    if have.as_deref() == Some(crate::quickstart::builtin_version()) {
        return Ok(());
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    super::quickstart::sync_builtins_in(&tx)?;
    tx.commit().map_err(e)
}

/// The engine pin of a tiling this engine cannot build.
pub const NO_PIN: &str = "none";

/// The tables that hold records with geometry pins.
pub(super) const PINNED: [(&str, &str); 3] = [
    ("pattern", "patterns"),
    ("ant", "ants"),
    ("quickstart", "quickstart"),
];

/// What opening a library found about its pins and rule-semantics versions.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct PinCheck {
    /// Keys whose engine pin was computed (again) by this engine.
    pub computed: usize,
    /// Keys whose engine pin changed from the one cached.
    pub changed: Vec<String>,
    /// Rule families with patterns newly flagged because their semantics version is not the
    /// engine's (or the engine has no such family).
    pub semantics: Vec<String>,
    /// Records newly flagged `needs_reverify`, per table.
    pub flagged: HashMap<String, u64>,
}

/// Bring `engine_pins` up to this engine (every key the records use, computed again when an
/// older or newer engine cached it), then flag the records whose stored pin differs from the
/// engine's. Writes only when something changed.
pub(super) fn check_pins(c: &mut Connection) -> Result<PinCheck, String> {
    let engine = engine_geometry();
    let mut out = PinCheck::default();
    let cached: HashMap<String, (String, String)> = {
        let mut st = c
            .prepare("SELECT pin_key, pin, engine_version FROM engine_pins")
            .map_err(e)?;
        st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?
    };
    let mut keys: Vec<String> = Vec::new();
    for (_, t) in PINNED {
        let mut st = c
            .prepare(&format!("SELECT DISTINCT pin_key FROM {t}"))
            .map_err(e)?;
        for k in st.query_map([], |r| r.get::<_, String>(0)).map_err(e)? {
            keys.push(k.map_err(e)?);
        }
    }
    keys.extend(cached.keys().cloned());
    keys.sort();
    keys.dedup();
    let mut fresh: Vec<GeometryPin> = Vec::new();
    for k in &keys {
        match cached.get(k) {
            Some((_, v)) if v == engine => {}
            // A tiling this engine cannot build (a family removed or renamed) has no pin: every
            // record on it is flagged, and re-verifying it reports why.
            _ => fresh.push(engine_pin_of_key(k).unwrap_or_else(|_| GeometryPin {
                key: k.clone(),
                pin: NO_PIN.into(),
                polygon_hash: String::new(),
                neighbour_hash: String::new(),
            })),
        }
    }
    let mut pins: HashMap<String, String> = cached.into_iter().map(|(k, (p, _))| (k, p)).collect();
    for p in &fresh {
        if pins.get(&p.key).is_some_and(|old| old != &p.pin) {
            out.changed.push(p.key.clone());
        }
        pins.insert(p.key.clone(), p.pin.clone());
    }
    out.computed = fresh.len();
    // Which records differ (two index ranges per key, so only mismatches are read).
    let mut stale: Vec<(&str, String, String)> = Vec::new();
    for (_, t) in PINNED {
        let mut st = c
            .prepare_cached(&format!(
                "SELECT count(*) FROM {t} WHERE ((pin_key = ?1 AND geometry_pin < ?2)
                    OR (pin_key = ?1 AND geometry_pin > ?2)) AND needs_reverify = 0"
            ))
            .map_err(e)?;
        for (k, p) in &pins {
            let n: i64 = st.query_row(params![k, p], |r| r.get(0)).map_err(e)?;
            if n > 0 {
                stale.push((t, k.clone(), p.clone()));
            }
        }
    }
    if fresh.is_empty() && stale.is_empty() {
        check_semantics(c, &mut out)?;
        return Ok(out);
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    let now = now_utc();
    for p in &fresh {
        put_engine_pin(&tx, p, &now)?;
    }
    for (t, k, p) in &stale {
        let n = tx
            .execute(
                &format!(
                    "UPDATE {t} SET needs_reverify = 1 WHERE ((pin_key = ?1 AND geometry_pin < ?2)
                        OR (pin_key = ?1 AND geometry_pin > ?2)) AND needs_reverify = 0"
                ),
                params![k, p],
            )
            .map_err(e)?;
        *out.flagged.entry(t.to_string()).or_default() += n as u64;
    }
    tx.commit().map_err(e)?;
    check_semantics(c, &mut out)?;
    Ok(out)
}

/// Flag the patterns whose rule-semantics version is not the running engine's for their family
/// (or whose family the engine does not have), as [`check_pins`] flags changed pins. Two index
/// ranges per family, so only mismatches are read; writes only when something is flagged.
fn check_semantics(c: &mut Connection, out: &mut PinCheck) -> Result<(), String> {
    let families: Vec<String> = {
        let mut st = c
            .prepare("SELECT DISTINCT family FROM patterns")
            .map_err(e)?;
        st.query_map([], |r| r.get(0))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?
    };
    // A family the engine does not have gets no version, which every stored one differs from.
    let engine = |f: &str| semantics::version(f).unwrap_or("");
    const STALE: &str = "((family = ?1 AND semantics < ?2) OR (family = ?1 AND semantics > ?2))
        AND needs_reverify = 0";
    let mut stale: Vec<&String> = vec![];
    {
        let mut st = c
            .prepare_cached(&format!("SELECT count(*) FROM patterns WHERE {STALE}"))
            .map_err(e)?;
        for f in &families {
            let n: i64 = st
                .query_row(params![f, engine(f)], |r| r.get(0))
                .map_err(e)?;
            if n > 0 {
                stale.push(f);
            }
        }
    }
    if stale.is_empty() {
        return Ok(());
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    for f in stale {
        let n = tx
            .execute(
                &format!("UPDATE patterns SET needs_reverify = 1 WHERE {STALE}"),
                params![f, engine(f)],
            )
            .map_err(e)?;
        *out.flagged.entry("patterns".to_string()).or_default() += n as u64;
        out.semantics.push(f.clone());
    }
    tx.commit().map_err(e)
}

/// The running engine's pin for a pin key (the JSON of the spec it is the pin of).
pub(super) fn engine_pin_of_key(key: &str) -> Result<GeometryPin, String> {
    let spec: TilingSpec =
        serde_json::from_str(key).map_err(|x| format!("bad pin key {key}: {x}"))?;
    let mut p = geometry_pin(&spec)?;
    // A key written by another build may spell the same spec differently.
    p.key = key.to_string();
    Ok(p)
}

/// Store an engine pin.
pub(super) fn put_engine_pin(tx: &Transaction, p: &GeometryPin, now: &str) -> Result<(), String> {
    tx.prepare_cached(
        "INSERT INTO engine_pins (pin_key, pin, polygon_hash, neighbour_hash, engine_version,
            checked) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (pin_key) DO UPDATE SET pin = excluded.pin,
            polygon_hash = excluded.polygon_hash, neighbour_hash = excluded.neighbour_hash,
            engine_version = excluded.engine_version, checked = excluded.checked",
    )
    .map_err(e)?
    .execute(params![
        p.key,
        p.pin,
        p.polygon_hash,
        p.neighbour_hash,
        engine_geometry(),
        now
    ])
    .map_err(e)?;
    Ok(())
}

/// The pin key and the running engine's pin for a tiling (a periodic tiling's unit cell; an
/// aperiodic patch), making sure `engine_pins` has it.
pub(super) fn pin_for(tx: &Transaction, t: &TilingSpec) -> Result<(String, String), String> {
    let key = pin_key(t);
    let have: Option<(String, String)> = tx
        .prepare_cached("SELECT pin, engine_version FROM engine_pins WHERE pin_key = ?1")
        .map_err(e)?
        .query_row([&key], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(e)?;
    if let Some((pin, v)) = have
        && v == engine_geometry()
    {
        return Ok((key, pin));
    }
    let p = geometry_pin(&pin_spec(t))?;
    put_engine_pin(tx, &p, &now_utc())?;
    Ok((key, p.pin))
}

/// A new uuid (version 4) from the system's randomness (the hasher keys `RandomState` draws):
/// library and job uuids.
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let word = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u64(N.fetch_add(1, Ordering::Relaxed));
        h.write_u32(std::process::id());
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.finish()
    };
    let (a, b) = (word(1), word(2));
    let a = (a & !0xF000) | 0x4000;
    let b = (b & !(0xC << 60)) | (0x8 << 60);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

impl Library {
    /// A `meta` value (`schema`, `library_uuid`, `created`, `created_by`, `builtin_version`).
    pub fn meta(&self, key: &str) -> Result<Option<String>, String> {
        self.read(None, |c| {
            c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .optional()
                .map_err(e)
        })
    }

    /// Check the geometry pins again now (as opening the library does): engine pins cached by
    /// another engine are computed again and records whose pin differs are flagged.
    pub fn check_pins(&self) -> Result<PinCheck, String> {
        match self.guard(false)? {
            None => Ok(PinCheck::default()),
            Some(mut g) => check_pins(g.as_mut().unwrap()),
        }
    }
}
