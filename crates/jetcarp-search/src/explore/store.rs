//! An explore search's working database (`searches/<uuid>/search.sqlite` beside the library):
//! the runs not yet sealed into Parquet segments ([`crate::explore::runs`]), and the search's
//! own state, so a stopped search carries on from where it was without reading its runs again.
//!
//! | Table | What |
//! | --- | --- |
//! | `runs` | the runs not yet sealed, one row each: candidate, fate, score, behaviour bin, features (80-byte blob of 20 `f32`); `stage` 1 screened, 2 confirmed on a bigger grid (`parent` the screened run) |
//! | `elites` | the archive: each elite run's id, tiling group (its niche is its family, that group, and its bin), and the candidate and measures the search needs of it (the columns of `runs` it reads), so loading never reads the runs |
//! | `confirmed` | elites confirmed on bigger grids |
//! | `coverage` | screened runs per tiling, family, and seed kind (where the search tries next) |
//! | `families` | screened runs per family, with their score sum and best score |
//! | `seen` | the identities of every candidate tried ([`jetcarp_core::explore::cand::Cand::id_hash`]), as blobs of little-endian `u64`, one per step, merged when a search is loaded |
//! | `meta` | counters (`screened`, `confirmed`, `elapsed_s`), the random states, the format, and an import's progress |
//!
//! Each step of a search writes its runs and what they changed in all of these in one
//! transaction ([`write`]), so the database always holds a state the search reached. Loading
//! ([`load`]) reads the elites, the small tables, and the seen blobs, never `runs`: a search of
//! 15 million runs loads in about a second.
//!
//! Reviews of runs are not here: they are ratings in the library (target kind `search_run`; see
//! [`crate::explore::reviews`]), so a search folder can be deleted or shared without them.
//!
//! The `runs` table has the columns of CART's explore databases; [`Import`] reads such a
//! database for a new search, which writes its runs straight into sealed segments.

use crate::explore::archive::{Archive, Eval};
use crate::explore::segments::RunRec;
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::{Fate, Features};
use jetcarp_core::explore::space::{ALL_FAMS, Fam};
use jetcarp_core::topology::Nb;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What `meta.format` says. Format 1 (before sealed segments) kept every run in `runs`, the
/// reviews in `reviews`, and only ids in `elites`; [`open`] brings such a database up to 2 and
/// [`crate::explore::runs::Runs::open`] seals its runs.
pub const FORMAT: &str = "jetcarp-explore 2";

/// Seen blobs past this many are merged into one when a search is loaded.
const MERGE_SEEN: i64 = 64;

fn e(x: rusqlite::Error) -> String {
    x.to_string()
}

/// A family by its name ([`Fam::name`], or its serde name: `wire_world` too).
pub fn fam_named(n: &str) -> Option<Fam> {
    let n = n.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    ALL_FAMS
        .iter()
        .copied()
        .find(|f| f.name() == n || f.name() == n.replace('_', ""))
}

pub fn fate_name(f: Fate) -> &'static str {
    match f {
        Fate::Died => "died",
        Fate::Froze => "froze",
        Fate::Cycle => "cycle",
        Fate::Running => "running",
    }
}

pub fn fate_parse(s: &str) -> Fate {
    match s {
        "died" => Fate::Died,
        "froze" => Fate::Froze,
        "cycle" => Fate::Cycle,
        _ => Fate::Running,
    }
}

/// The features as stored: 20 little-endian `f32`.
pub fn feat_blob(f: &Features) -> Vec<u8> {
    let v = [
        f.settle as f32,
        f.period as f32,
        f.density,
        f.density_late,
        f.activity_late,
        f.activity_early,
        f.pop_cv,
        f.growth,
        f.peak_density,
        f.entropy,
        f.bits,
        f.same_nb,
        f.smooth,
        f.comps_per_live,
        f.largest,
        f.size_entropy,
        f.patchiness,
        f.travel,
        f.movers as f32,
        f.mover_speed,
    ];
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Inverse of [`feat_blob`].
pub fn feat_from_blob(b: &[u8]) -> Features {
    let g = |i: usize| -> f32 {
        b.get(i * 4..i * 4 + 4)
            .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
            .unwrap_or(0.0)
    };
    Features {
        settle: g(0) as u32,
        period: g(1) as u32,
        density: g(2),
        density_late: g(3),
        activity_late: g(4),
        activity_early: g(5),
        pop_cv: g(6),
        growth: g(7),
        peak_density: g(8),
        entropy: g(9),
        bits: g(10),
        same_nb: g(11),
        smooth: g(12),
        comps_per_live: g(13),
        largest: g(14),
        size_entropy: g(15),
        patchiness: g(16),
        travel: g(17),
        movers: g(18) as u32,
        mover_speed: g(19),
    }
}

/// The columns of `runs`, in order.
pub const RUN_COLS: &str = "id,stage,parent,tiling,nb,fam,rule,seed,seed_kind,cells,gens,steps,\
                            rng,origin,fate,settle,period,score,bin,feats,ts";

/// The columns of `elites` after its id and group: what [`eval_at`] reads.
pub const EVAL_COLS: &str =
    "tiling,nb,fam,rule,seed,cells,gens,steps,rng,origin,fate,score,bin,feats";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs(
  id INTEGER PRIMARY KEY, stage INT NOT NULL, parent INT, tiling TEXT NOT NULL, nb INT,
  fam TEXT NOT NULL, rule TEXT NOT NULL, seed TEXT NOT NULL, seed_kind TEXT, cells INT,
  gens INT, steps INT, rng INT, origin TEXT, fate TEXT, settle INT, period INT,
  score REAL, bin INT, feats BLOB, ts INT);
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);
CREATE TABLE IF NOT EXISTS elites(id INTEGER PRIMARY KEY, grp INT NOT NULL, tiling TEXT,
  nb INT, fam TEXT, rule TEXT, seed TEXT, cells INT, gens INT, steps INT, rng INT, origin TEXT,
  fate TEXT, score REAL, bin INT, feats BLOB);
CREATE TABLE IF NOT EXISTS confirmed(id INTEGER PRIMARY KEY);
CREATE TABLE IF NOT EXISTS coverage(tiling TEXT NOT NULL, fam TEXT NOT NULL,
  seed_kind TEXT NOT NULL, n INT NOT NULL, PRIMARY KEY(tiling, fam, seed_kind)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS families(fam TEXT PRIMARY KEY, runs INT NOT NULL,
  score_sum REAL NOT NULL, best REAL NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS seen(seq INTEGER PRIMARY KEY, data BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS runs_score ON runs(score);
DROP INDEX IF EXISTS runs_parent;
";

/// Open (and create) a working database for writing. A format 1 database's elites gain their
/// candidates and measures from its runs (its runs are sealed by
/// [`crate::explore::runs::Runs::open`], its reviews moved by [`crate::explore::reviews::move_old`]).
pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|x| format!("{}: {x}", d.display()))?;
    }
    let c = Connection::open(path).map_err(|x| format!("{}: {x}", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(e)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    )
    .map_err(e)?;
    c.execute_batch(SCHEMA).map_err(e)?;
    // Format 1: elites held only ids.
    let has_tiling: bool = c
        .prepare("SELECT 1 FROM pragma_table_info('elites') WHERE name = 'tiling'")
        .map_err(e)?
        .exists([])
        .map_err(e)?;
    if !has_tiling {
        let mut sql = String::from("BEGIN;");
        for (n, t) in EVAL_COLS.split(',').zip([
            "TEXT", "INT", "TEXT", "TEXT", "TEXT", "INT", "INT", "INT", "INT", "TEXT", "TEXT",
            "REAL", "INT", "BLOB",
        ]) {
            sql += &format!("ALTER TABLE elites ADD COLUMN {n} {t};");
        }
        sql += &format!(
            "UPDATE elites SET ({EVAL_COLS}) = (SELECT {EVAL_COLS} FROM runs r WHERE r.id = elites.id);
             COMMIT;"
        );
        c.execute_batch(&sql).map_err(e)?;
    }
    c.execute(
        "INSERT OR IGNORE INTO meta(k, v) VALUES('format', ?1)",
        [FORMAT],
    )
    .map_err(e)?;
    Ok(c)
}

/// Open a working database (or an old CART explore database) for reading only.
pub fn open_read(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!("no search database at {}", path.display()));
    }
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|x| format!("{}: {x}", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(e)?;
    Ok(c)
}

/// Does the database have this table?
pub fn has_table(c: &Connection, t: &str) -> Result<bool, String> {
    c.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .map_err(e)?
        .exists([t])
        .map_err(e)
}

pub fn meta_get(c: &Connection, k: &str) -> Result<Option<String>, String> {
    if !has_table(c, "meta")? {
        return Ok(None);
    }
    c.query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| r.get(0))
        .optional()
        .map_err(e)
}

pub fn meta_set(c: &Connection, k: &str, v: &str) -> Result<(), String> {
    c.execute(
        "INSERT OR REPLACE INTO meta(k, v) VALUES(?1, ?2)",
        params![k, v],
    )
    .map_err(e)?;
    Ok(())
}

/// Screened runs of a family, with their score sum and best score.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FamStat {
    pub runs: u64,
    pub score_sum: f64,
    pub best: f64,
}

impl FamStat {
    pub fn add(&mut self, score: f64) {
        if self.runs == 0 || score > self.best {
            self.best = score;
        }
        self.runs += 1;
        self.score_sum += score;
    }
    pub fn merge(&mut self, o: &FamStat) {
        if o.runs == 0 {
            return;
        }
        if self.runs == 0 || o.best > self.best {
            self.best = o.best;
        }
        self.runs += o.runs;
        self.score_sum += o.score_sum;
    }
    pub fn mean(&self) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            self.score_sum / self.runs as f64
        }
    }
}

/// A run to store.
#[derive(Clone, Debug)]
pub struct RunOut {
    pub id: i64,
    pub stage: u8,
    pub parent: Option<i64>,
    pub ev: Eval,
    pub seed_kind: String,
}

/// What a step changed: written by [`write`] in one transaction.
#[derive(Debug, Default)]
pub struct Delta {
    pub rows: Vec<RunOut>,
    /// Identities newly tried.
    pub seen: Vec<u64>,
    /// Screened runs to add per (tiling, family, seed kind).
    pub coverage: HashMap<(String, Fam, String), u32>,
    pub families: HashMap<Fam, FamStat>,
    /// Elites added (with their tiling group and run) or dropped.
    pub elites: Vec<(i64, Option<(u8, Eval)>)>,
    pub confirmed: Vec<i64>,
    pub meta: Vec<(String, String)>,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
            && self.seen.is_empty()
            && self.coverage.is_empty()
            && self.families.is_empty()
            && self.elites.is_empty()
            && self.confirmed.is_empty()
            && self.meta.is_empty()
    }
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Write a step's changes in one transaction.
pub fn write(c: &mut Connection, d: &Delta) -> Result<(), String> {
    if d.is_empty() {
        return Ok(());
    }
    let tx = c.transaction().map_err(e)?;
    write_in(&tx, d)?;
    tx.commit().map_err(e)
}

/// Insert or replace an elite with its run's candidate and measures.
pub(crate) fn put_elite(tx: &Connection, id: i64, grp: u8, ev: &Eval) -> Result<(), String> {
    let c = &ev.cand;
    tx.prepare_cached(&format!(
        "INSERT OR REPLACE INTO elites(id, grp, {EVAL_COLS})
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    ))
    .map_err(e)?
    .execute(params![
        id,
        grp,
        c.tiling,
        c.nb as u8,
        c.fam.name(),
        c.rule,
        serde_json::to_string(&c.seed).unwrap_or_default(),
        c.cells,
        c.gens,
        c.steps,
        c.rng as i64,
        c.origin,
        fate_name(ev.fate),
        ev.score as f64,
        ev.bin,
        feat_blob(&ev.feat),
    ])
    .map_err(e)?;
    Ok(())
}

fn write_in(tx: &rusqlite::Transaction, d: &Delta) -> Result<(), String> {
    {
        let mut st = tx
            .prepare_cached(&format!(
                "INSERT INTO runs({RUN_COLS}) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
            ))
            .map_err(e)?;
        let t = now();
        for b in &d.rows {
            let ev = &b.ev;
            let c = &ev.cand;
            let seed = serde_json::to_string(&c.seed).unwrap_or_default();
            st.execute(params![
                b.id,
                b.stage,
                b.parent,
                c.tiling,
                c.nb as u8,
                c.fam.name(),
                c.rule,
                seed,
                b.seed_kind,
                c.cells,
                c.gens,
                c.steps,
                c.rng as i64,
                c.origin,
                fate_name(ev.fate),
                ev.feat.settle,
                ev.feat.period,
                ev.score as f64,
                ev.bin,
                feat_blob(&ev.feat),
                t
            ])
            .map_err(e)?;
        }
        if !d.seen.is_empty() {
            let blob: Vec<u8> = d.seen.iter().flat_map(|h| h.to_le_bytes()).collect();
            tx.execute("INSERT INTO seen(data) VALUES(?1)", [blob])
                .map_err(e)?;
        }
        let mut st = tx
            .prepare_cached(
                "INSERT INTO coverage(tiling, fam, seed_kind, n) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(tiling, fam, seed_kind) DO UPDATE SET n = n + excluded.n",
            )
            .map_err(e)?;
        for ((t, f, k), n) in &d.coverage {
            st.execute(params![t, f.name(), k, n]).map_err(e)?;
        }
        let mut st = tx
            .prepare_cached(
                "INSERT INTO families(fam, runs, score_sum, best) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(fam) DO UPDATE SET runs = runs + excluded.runs,
                 score_sum = score_sum + excluded.score_sum, best = max(best, excluded.best)",
            )
            .map_err(e)?;
        for (f, s) in &d.families {
            st.execute(params![f.name(), s.runs as i64, s.score_sum, s.best])
                .map_err(e)?;
        }
        let mut drop = tx
            .prepare_cached("DELETE FROM elites WHERE id = ?1")
            .map_err(e)?;
        for (id, g) in &d.elites {
            match g {
                Some((g, ev)) => put_elite(tx, *id, *g, ev)?,
                None => {
                    drop.execute([id]).map_err(e)?;
                }
            }
        }
        let mut st = tx
            .prepare_cached("INSERT OR IGNORE INTO confirmed(id) VALUES(?1)")
            .map_err(e)?;
        for id in &d.confirmed {
            st.execute([id]).map_err(e)?;
        }
        let mut st = tx
            .prepare_cached("INSERT OR REPLACE INTO meta(k, v) VALUES(?1, ?2)")
            .map_err(e)?;
        for (k, v) in &d.meta {
            st.execute(params![k, v]).map_err(e)?;
        }
    }
    Ok(())
}

/// What loading read, and how long it took.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadStats {
    pub seconds: f64,
    /// Elites read with their runs' candidates and measures (from `elites`).
    pub elites: u64,
    pub runs_read: u64,
    /// Identities in the seen set, and the blobs they were in.
    pub seen: u64,
    pub seen_blobs: u64,
    pub coverage_rows: u64,
    pub confirmed: u64,
}

/// A search's saved state.
#[derive(Default)]
pub struct Saved {
    pub archive: Archive,
    /// Screened runs per (tiling, family, seed kind).
    pub coverage: Vec<(String, Fam, String, u32)>,
    pub families: HashMap<Fam, FamStat>,
    /// Every identity tried, sorted.
    pub seen: Vec<u64>,
    pub meta: HashMap<String, String>,
    /// The next run id.
    pub next_id: i64,
    pub stats: LoadStats,
}

impl Saved {
    pub fn meta_u64(&self, k: &str) -> u64 {
        self.meta.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
    }
    pub fn meta_f64(&self, k: &str) -> f64 {
        self.meta.get(k).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }
}

/// A stored run's candidate and measures, from the columns of [`EVAL_COLS`] starting at `at`.
/// `None` when its family or seed does not read.
pub fn eval_at(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Option<Eval>> {
    let fam: String = r.get(at + 2)?;
    let seed: String = r.get(at + 4)?;
    let Some(fam) = fam_named(&fam) else {
        return Ok(None);
    };
    let Ok(seed) = serde_json::from_str(&seed) else {
        return Ok(None);
    };
    let cand = Cand {
        tiling: r.get(at)?,
        nb: if r.get::<_, Option<i64>>(at + 1)?.unwrap_or(0) == 0 {
            Nb::Touch
        } else {
            Nb::Edge
        },
        fam,
        rule: r.get(at + 3)?,
        seed,
        cells: r.get::<_, Option<i64>>(at + 5)?.unwrap_or(0) as u32,
        gens: r.get::<_, Option<i64>>(at + 6)?.unwrap_or(0) as u32,
        steps: r.get::<_, Option<i64>>(at + 7)?.unwrap_or(1) as u32,
        rng: r.get::<_, Option<i64>>(at + 8)?.unwrap_or(0) as u64,
        origin: r.get::<_, Option<String>>(at + 9)?.unwrap_or_default(),
    };
    Ok(Some(Eval {
        cand,
        fate: fate_parse(&r.get::<_, Option<String>>(at + 10)?.unwrap_or_default()),
        score: r.get::<_, Option<f64>>(at + 11)?.unwrap_or(0.0) as f32,
        bin: r.get::<_, Option<i64>>(at + 12)?.unwrap_or(0) as u32,
        feat: feat_from_blob(&r.get::<_, Option<Vec<u8>>>(at + 13)?.unwrap_or_default()),
    }))
}

/// A run's candidate and measures. `None` when its family or seed does not read.
pub fn eval_of(r: &RunRec) -> Option<Eval> {
    let fam = fam_named(&r.fam)?;
    let seed = serde_json::from_str(&r.seed).ok()?;
    Some(Eval {
        cand: Cand {
            tiling: r.tiling.clone(),
            nb: if r.nb == 0 { Nb::Touch } else { Nb::Edge },
            fam,
            rule: r.rule.clone(),
            seed,
            cells: r.cells as u32,
            gens: r.gens as u32,
            steps: r.steps as u32,
            rng: r.rng as u64,
            origin: r.origin.clone(),
        },
        fate: fate_parse(&r.fate),
        score: r.score as f32,
        bin: r.bin as u32,
        feat: r.feat,
    })
}

/// A row of `runs` (the columns of [`RUN_COLS`] from `at`) as a run. NULLs read as the search
/// reads them (no random state 0, one step per generation, ...).
pub fn rec_at(r: &rusqlite::Row, at: usize) -> rusqlite::Result<RunRec> {
    let oi = |k: usize| -> rusqlite::Result<Option<i64>> { r.get::<_, Option<i64>>(at + k) };
    let os = |k: usize| -> rusqlite::Result<String> {
        Ok(r.get::<_, Option<String>>(at + k)?.unwrap_or_default())
    };
    let settle = oi(15)?.unwrap_or(0);
    let period = oi(16)?.unwrap_or(0);
    let mut feat = feat_from_blob(&r.get::<_, Option<Vec<u8>>>(at + 19)?.unwrap_or_default());
    feat.settle = settle as u32;
    feat.period = period as u32;
    Ok(RunRec {
        id: r.get(at)?,
        stage: oi(1)?.unwrap_or(1),
        parent: oi(2)?,
        tiling: os(3)?,
        nb: oi(4)?.unwrap_or(0),
        fam: os(5)?,
        rule: os(6)?,
        seed: os(7)?,
        seed_kind: os(8)?,
        cells: oi(9)?.unwrap_or(0),
        gens: oi(10)?.unwrap_or(0),
        steps: oi(11)?.unwrap_or(1),
        rng: oi(12)?.unwrap_or(0),
        origin: os(13)?,
        fate: os(14)?,
        settle,
        period,
        score: r.get::<_, Option<f64>>(at + 17)?.unwrap_or(0.0),
        bin: oi(18)?.unwrap_or(0),
        feat,
        ts: oi(20)?.unwrap_or(0),
    })
}

/// Load a search's state: the elites (with their runs' candidates and measures), the confirmed
/// set, coverage, family totals, the seen set, and the counters. The seen blobs are merged into
/// one when there are many. `next_id` here counts the unsealed runs only (the caller knows the
/// sealed ones).
pub fn load(c: &mut Connection) -> Result<Saved, String> {
    let t0 = std::time::Instant::now();
    let mut s = Saved::default();
    {
        let mut st = c
            .prepare(&format!(
                "SELECT id, grp, {EVAL_COLS} FROM elites ORDER BY id"
            ))
            .map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
            s.stats.runs_read += 1;
            let id: i64 = r.get(0).map_err(e)?;
            let grp: i64 = r.get(1).map_err(e)?;
            if let Some(ev) = eval_at(r, 2).map_err(e)? {
                s.archive.insert(id, ev, grp as u8);
                s.stats.elites += 1;
            }
        }
        let mut st = c.prepare("SELECT id FROM confirmed").map_err(e)?;
        s.archive.confirmed = st
            .query_map([], |r| r.get(0))
            .map_err(e)?
            .collect::<Result<HashSet<i64>, _>>()
            .map_err(e)?;
        s.stats.confirmed = s.archive.confirmed.len() as u64;
        let mut st = c
            .prepare("SELECT tiling, fam, seed_kind, n FROM coverage")
            .map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
            s.stats.coverage_rows += 1;
            let f: String = r.get(1).map_err(e)?;
            if let Some(f) = fam_named(&f) {
                s.coverage.push((
                    r.get(0).map_err(e)?,
                    f,
                    r.get(2).map_err(e)?,
                    r.get::<_, i64>(3).map_err(e)? as u32,
                ));
            }
        }
        let mut st = c
            .prepare("SELECT fam, runs, score_sum, best FROM families")
            .map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
            let f: String = r.get(0).map_err(e)?;
            if let Some(f) = fam_named(&f) {
                s.families.insert(
                    f,
                    FamStat {
                        runs: r.get::<_, i64>(1).map_err(e)? as u64,
                        score_sum: r.get(2).map_err(e)?,
                        best: r.get(3).map_err(e)?,
                    },
                );
            }
        }
        let mut st = c.prepare("SELECT k, v FROM meta").map_err(e)?;
        s.meta = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?
            .collect::<Result<HashMap<String, String>, _>>()
            .map_err(e)?;
        let mut st = c.prepare("SELECT data FROM seen ORDER BY seq").map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
            s.stats.seen_blobs += 1;
            let b: Vec<u8> = r.get(0).map_err(e)?;
            s.seen.extend(
                b.chunks_exact(8)
                    .map(|x| u64::from_le_bytes(x.try_into().unwrap())),
            );
        }
        s.next_id = c
            .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM runs", [], |r| {
                r.get(0)
            })
            .map_err(e)?;
    }
    s.seen.sort_unstable();
    s.seen.dedup();
    s.stats.seen = s.seen.len() as u64;
    if s.stats.seen_blobs as i64 > MERGE_SEEN {
        let blob: Vec<u8> = s.seen.iter().flat_map(|h| h.to_le_bytes()).collect();
        let tx = c.transaction().map_err(e)?;
        tx.execute("DELETE FROM seen", []).map_err(e)?;
        tx.execute("INSERT INTO seen(data) VALUES(?1)", [blob])
            .map_err(e)?;
        tx.commit().map_err(e)?;
    }
    s.stats.seconds = t0.elapsed().as_secs_f64();
    Ok(s)
}

// ---------------------------------------------------------------------------------------------
// Reviews of a format 1 database

/// A review as an old database holds it: run, verdict, note, time (seconds since 1970), rater,
/// source.
pub type OldReview = (i64, String, String, i64, String, String);

/// The reviews of a database from before reviews moved to the library (format 1, or CART's),
/// if it still has its `reviews` table.
pub fn old_reviews(c: &Connection) -> Result<Vec<OldReview>, String> {
    if !has_table(c, "reviews")? {
        return Ok(vec![]);
    }
    let cols: Vec<String> = c
        .prepare("SELECT name FROM pragma_table_info('reviews')")
        .map_err(e)?
        .query_map([], |r| r.get(0))
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)?;
    let opt = |n: &str| {
        if cols.iter().any(|c| c == n) {
            format!("COALESCE({n}, '')")
        } else {
            "''".into()
        }
    };
    let mut st = c
        .prepare(&format!(
            "SELECT run_id, verdict, COALESCE(note, ''), COALESCE(ts, 0), {}, {} FROM reviews
             ORDER BY rowid",
            opt("rater"),
            opt("source")
        ))
        .map_err(e)?;
    st.query_map([], |r| {
        Ok((
            r.get(0)?,
            r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
        ))
    })
    .map_err(e)?
    .collect::<Result<_, _>>()
    .map_err(e)
}

/// Drop a format 1 database's `reviews` table (once its reviews are in the library).
pub fn drop_old_reviews(c: &Connection) -> Result<(), String> {
    c.execute_batch("DROP INDEX IF EXISTS reviews_run; DROP TABLE IF EXISTS reviews;")
        .map_err(e)
}

// ---------------------------------------------------------------------------------------------
// Importing an old database

/// One row of an old database's `runs`, with its candidate and measures when they read.
pub struct OldRun {
    pub id: i64,
    pub stage: i64,
    pub parent: Option<i64>,
    pub seed_kind: Option<String>,
    pub ev: Option<Eval>,
    /// The row, to store as it is.
    pub rec: RunRec,
}

/// An old explore database being imported, read in id order.
pub struct Import {
    pub path: PathBuf,
    old: Connection,
    /// The last id read.
    pub last: i64,
    /// The largest id in the old database.
    pub max: i64,
}

impl Import {
    /// Open an old database (read only), to go on after run `last`.
    pub fn open(path: &Path, last: i64) -> Result<Import, String> {
        let old = open_read(path)?;
        if !has_table(&old, "runs")? {
            return Err(format!(
                "{} is not an explore database (it has no runs table)",
                path.display()
            ));
        }
        let max = old
            .query_row("SELECT COALESCE(MAX(id), 0) FROM runs", [], |r| r.get(0))
            .map_err(e)?;
        Ok(Import {
            path: path.to_path_buf(),
            old,
            last,
            max,
        })
    }

    /// The next `n` runs (none when the import is done).
    pub fn next(&mut self, n: usize) -> Result<Vec<OldRun>, String> {
        let mut st = self
            .old
            .prepare_cached(&format!(
                "SELECT {RUN_COLS} FROM runs WHERE id > ?1 ORDER BY id LIMIT ?2"
            ))
            .map_err(e)?;
        let mut rows = st.query(params![self.last, n as i64]).map_err(e)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(e)? {
            let rec = rec_at(r, 0).map_err(e)?;
            out.push(OldRun {
                id: rec.id,
                stage: rec.stage,
                parent: rec.parent,
                seed_kind: r.get(8).map_err(e)?,
                ev: eval_of(&rec),
                rec,
            });
        }
        if let Some(l) = out.last() {
            self.last = l.id;
        }
        Ok(out)
    }

    /// The old database's reviews.
    pub fn reviews(&self) -> Result<Vec<OldReview>, String> {
        old_reviews(&self.old)
    }

    /// The file's name (without its folder), as provenance names it.
    pub fn file_name(&self) -> String {
        file_name(&self.path)
    }

    /// Up to `n` runs after run `after`, in id order, by what a sample is drawn on: id, stage,
    /// tiling, family, seed kind. Does not move the import on.
    pub fn keys(&self, after: i64, n: usize) -> Result<Vec<RunKey>, String> {
        let mut st = self
            .old
            .prepare_cached(
                "SELECT id, stage, tiling, fam, seed_kind FROM runs WHERE id > ?1 ORDER BY id
                 LIMIT ?2",
            )
            .map_err(e)?;
        st.query_map(params![after, n as i64], |r| {
            Ok(RunKey {
                id: r.get(0)?,
                stage: r.get::<_, Option<i64>>(1)?.unwrap_or(1),
                tiling: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                fam: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                seed_kind: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }

    /// Up to `n` runs after run `after` that `filter` (an SQL condition on the columns of `runs`)
    /// selects, in id order. Does not move the import on.
    pub fn select(&self, after: i64, n: usize, filter: &str) -> Result<Vec<RunRec>, String> {
        let mut st = self
            .old
            .prepare_cached(&format!(
                "SELECT {RUN_COLS} FROM runs WHERE id > ?1 AND ({filter}) ORDER BY id LIMIT ?2"
            ))
            .map_err(e)?;
        let mut rows = st.query(params![after, n as i64]).map_err(e)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(e)? {
            out.push(rec_at(r, 0).map_err(e)?);
        }
        Ok(out)
    }

    /// The runs at these ids, in id order (ids that are not runs left out).
    pub fn get(&self, ids: &[i64]) -> Result<Vec<RunRec>, String> {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let list: Vec<String> = chunk.iter().map(i64::to_string).collect();
            let mut st = self
                .old
                .prepare(&format!(
                    "SELECT {RUN_COLS} FROM runs WHERE id IN ({}) ORDER BY id",
                    list.join(",")
                ))
                .map_err(e)?;
            let mut rows = st.query([]).map_err(e)?;
            while let Some(r) = rows.next().map_err(e)? {
                out.push(rec_at(r, 0).map_err(e)?);
            }
        }
        Ok(out)
    }
}

/// What a run's sample stratum is drawn on ([`Import::keys`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunKey {
    pub id: i64,
    pub stage: i64,
    pub tiling: String,
    pub fam: String,
    pub seed_kind: String,
}

/// A path's file name, as provenance names a file (never the folders it was in on this disk).
pub fn file_name(p: &Path) -> String {
    let name = |p: &Path| p.file_name().map(|f| f.to_string_lossy().into_owned());
    name(p)
        .or_else(|| std::fs::canonicalize(p).ok().and_then(|c| name(&c)))
        .unwrap_or_else(|| p.display().to_string())
}

/// Start importing `from` into a new search: `meta` says what is being imported (by its file
/// name: the working database travels with a shared search folder).
pub fn begin_import(c: &Connection, from: &Path) -> Result<(), String> {
    for (k, v) in [
        ("import_from", file_name(from)),
        ("import_done", "0".into()),
        ("import_last", "0".into()),
    ] {
        meta_set(c, k, &v)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// An import's re-measured runs, kept until the import copies them

/// New measures of an old run: fate, settle, period, score, bin, and the features.
pub type NewMeasures = (String, i64, i64, f64, i64, Features);

/// Keep re-measured runs' new measures (table `remeasured`, made when first needed) and `meta`,
/// in one transaction.
pub fn put_remeasured(
    c: &mut Connection,
    recs: &[RunRec],
    meta: &[(String, String)],
) -> Result<(), String> {
    let tx = c.transaction().map_err(e)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS remeasured(id INTEGER PRIMARY KEY, fate TEXT, settle INT,
           period INT, score REAL, bin INT, feats BLOB);",
    )
    .map_err(e)?;
    {
        let mut st = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO remeasured(id, fate, settle, period, score, bin, feats)
                 VALUES(?,?,?,?,?,?,?)",
            )
            .map_err(e)?;
        for r in recs {
            st.execute(params![
                r.id,
                r.fate,
                r.settle,
                r.period,
                r.score,
                r.bin,
                feat_blob(&r.feat)
            ])
            .map_err(e)?;
        }
        let mut st = tx
            .prepare_cached("INSERT OR REPLACE INTO meta(k, v) VALUES(?1, ?2)")
            .map_err(e)?;
        for (k, v) in meta {
            st.execute(params![k, v]).map_err(e)?;
        }
    }
    tx.commit().map_err(e)
}

/// The new measures kept for the runs from `lo` to `hi`, by id.
pub fn remeasured_between(
    c: &Connection,
    lo: i64,
    hi: i64,
) -> Result<HashMap<i64, NewMeasures>, String> {
    if !has_table(c, "remeasured")? {
        return Ok(HashMap::new());
    }
    let mut st = c
        .prepare_cached(
            "SELECT id, fate, settle, period, score, bin, feats FROM remeasured
             WHERE id BETWEEN ?1 AND ?2",
        )
        .map_err(e)?;
    st.query_map(params![lo, hi], |r| {
        let mut f = feat_from_blob(&r.get::<_, Vec<u8>>(6)?);
        let (settle, period): (i64, i64) = (r.get(2)?, r.get(3)?);
        f.settle = settle as u32;
        f.period = period as u32;
        Ok((
            r.get::<_, i64>(0)?,
            (r.get(1)?, settle, period, r.get(4)?, r.get(5)?, f),
        ))
    })
    .map_err(e)?
    .collect::<Result<_, _>>()
    .map_err(e)
}

/// Drop the re-measured runs once the import has copied them.
pub fn drop_remeasured(c: &Connection) -> Result<(), String> {
    c.execute_batch("DROP TABLE IF EXISTS remeasured;")
        .map_err(e)
}

/// Rebuild `elites` from an archive (every elite, with its run).
pub(crate) fn put_archive(tx: &Connection, a: &Archive) -> Result<u64, String> {
    tx.execute("DELETE FROM elites", []).map_err(e)?;
    let mut n = 0;
    for list in a.cells.values() {
        for (id, _) in list {
            if let (Some(ev), Some(g)) = (a.runs.get(id), a.group(*id)) {
                put_elite(tx, *id, g, ev)?;
                n += 1;
            }
        }
    }
    Ok(n)
}
