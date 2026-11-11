//! An explore search's working database (`searches/<uuid>/search.sqlite` beside the catalog):
//! every run it measured, the reviews people gave them, and the search's own state, so a stopped
//! search carries on from where it was without reading its runs again.
//!
//! | Table | What |
//! | --- | --- |
//! | `runs` | one row per run: candidate, fate, score, behaviour bin, features (80-byte blob of 20 `f32`); `stage` 1 screened, 2 confirmed on a bigger grid (`parent` the screened run) |
//! | `reviews` | a verdict on a run (`keep`, `maybe`, `drop`), with a note, who gave it, and where it came from (`review`, a ratings folder, an imported database) |
//! | `elites` | the archive: each elite run's id and tiling group (its niche is its family, that group, and its bin) |
//! | `confirmed` | elites confirmed on bigger grids |
//! | `coverage` | screened runs per tiling, family, and seed kind (where the search tries next) |
//! | `families` | screened runs per family, with their score sum and best score |
//! | `seen` | the identities of every candidate tried ([`super::cand::Cand::id_hash`]), as blobs of little-endian `u64`, one per step, merged when a search is loaded |
//! | `meta` | counters (`screened`, `confirmed`, `elapsed_s`), the random states, the format, and an import's progress |
//!
//! Each step of a search writes its runs and what they changed in all of these in one
//! transaction ([`write`]), so the database always holds a state the search reached. Loading
//! ([`load`]) reads the elites' runs, the small tables, and the seen blobs, never the whole of
//! `runs`: a search of 15 million runs loads in seconds.
//!
//! The `runs` and `reviews` tables are those of CART's explore databases (with two more review
//! columns), so the contact sheets, the atlas, and the fit scripts read either; [`import`]
//! copies an old database's runs and reviews into a new search and rebuilds its state from them
//! once.

use super::archive::{self, Archive, Eval};
use super::cand::Cand;
use super::features::{Fate, Features};
use super::space::{ALL_FAMS, Fam};
use crate::topology::Nb;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What `meta.format` says.
pub const FORMAT: &str = "jetcarp-explore 1";

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

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs(
  id INTEGER PRIMARY KEY, stage INT NOT NULL, parent INT, tiling TEXT NOT NULL, nb INT,
  fam TEXT NOT NULL, rule TEXT NOT NULL, seed TEXT NOT NULL, seed_kind TEXT, cells INT,
  gens INT, steps INT, rng INT, origin TEXT, fate TEXT, settle INT, period INT,
  score REAL, bin INT, feats BLOB, ts INT);
CREATE TABLE IF NOT EXISTS reviews(run_id INT, verdict TEXT, note TEXT, ts INT, rater TEXT,
  source TEXT);
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);
CREATE TABLE IF NOT EXISTS elites(id INTEGER PRIMARY KEY, grp INT NOT NULL);
CREATE TABLE IF NOT EXISTS confirmed(id INTEGER PRIMARY KEY);
CREATE TABLE IF NOT EXISTS coverage(tiling TEXT NOT NULL, fam TEXT NOT NULL,
  seed_kind TEXT NOT NULL, n INT NOT NULL, PRIMARY KEY(tiling, fam, seed_kind)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS families(fam TEXT PRIMARY KEY, runs INT NOT NULL,
  score_sum REAL NOT NULL, best REAL NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS seen(seq INTEGER PRIMARY KEY, data BLOB NOT NULL);
";

const INDEXES: &str = "
CREATE INDEX IF NOT EXISTS runs_score ON runs(score);
CREATE INDEX IF NOT EXISTS runs_parent ON runs(parent);
CREATE INDEX IF NOT EXISTS reviews_run ON reviews(run_id);
";

/// Open (and create) a working database for writing.
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
    // A review table from before the rater and source columns.
    for col in ["rater", "source"] {
        let has: bool = c
            .prepare("SELECT 1 FROM pragma_table_info('reviews') WHERE name = ?1")
            .map_err(e)?
            .exists([col])
            .map_err(e)?;
        if !has {
            c.execute_batch(&format!("ALTER TABLE reviews ADD COLUMN {col} TEXT"))
                .map_err(e)?;
        }
    }
    let importing = meta_get(&c, "import_done")?.is_some_and(|v| v == "0");
    if !importing {
        c.execute_batch(INDEXES).map_err(e)?;
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

pub fn meta_get(c: &Connection, k: &str) -> Result<Option<String>, String> {
    let has: bool = c
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'")
        .map_err(e)?
        .exists([])
        .map_err(e)?;
    if !has {
        return Ok(None);
    }
    c.query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| r.get(0))
        .optional()
        .map_err(e)
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
    /// Elites added (with their tiling group) or dropped.
    pub elites: Vec<(i64, Option<u8>)>,
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

fn now() -> i64 {
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

fn write_in(tx: &rusqlite::Transaction, d: &Delta) -> Result<(), String> {
    {
        let mut st = tx
            .prepare_cached(
                "INSERT INTO runs(id,stage,parent,tiling,nb,fam,rule,seed,seed_kind,cells,gens,
                 steps,rng,origin,fate,settle,period,score,bin,feats,ts)
                 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )
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
        let mut add = tx
            .prepare_cached("INSERT OR REPLACE INTO elites(id, grp) VALUES(?1, ?2)")
            .map_err(e)?;
        let mut drop = tx
            .prepare_cached("DELETE FROM elites WHERE id = ?1")
            .map_err(e)?;
        for (id, g) in &d.elites {
            match g {
                Some(g) => add.execute(params![id, g]).map_err(e)?,
                None => drop.execute([id]).map_err(e)?,
            };
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
    /// Elites read with their runs (the only rows of `runs` loading reads).
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

/// A stored run's candidate and measures, from the columns `tiling, nb, fam, rule, seed, cells,
/// gens, steps, rng, origin, fate, score, bin, feats` starting at `at`. `None` when its family
/// or seed does not read.
fn eval_at(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Option<Eval>> {
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

const EVAL_COLS: &str = "tiling,nb,fam,rule,seed,cells,gens,steps,rng,origin,fate,score,bin,feats";

/// Load a search's state: the elites (with their runs), the confirmed set, coverage, family
/// totals, the seen set, and the counters. The seen blobs are merged into one when there are many.
pub fn load(c: &mut Connection) -> Result<Saved, String> {
    let t0 = std::time::Instant::now();
    let mut s = Saved::default();
    {
        let mut st = c
            .prepare(&format!(
                "SELECT e.id, e.grp, {} FROM elites e JOIN runs r ON r.id = e.id ORDER BY e.id",
                EVAL_COLS
                    .split(',')
                    .map(|x| format!("r.{x}"))
                    .collect::<Vec<_>>()
                    .join(",")
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
// Reviews

/// A verdict on a run.
pub const VERDICTS: [&str; 3] = ["keep", "maybe", "drop"];

/// Record a verdict on a run (`keep`, `maybe`, `drop`). The run must exist.
pub fn add_review(
    c: &Connection,
    run: i64,
    verdict: &str,
    note: &str,
    rater: &str,
    source: &str,
) -> Result<(), String> {
    if !VERDICTS.contains(&verdict) {
        return Err(format!(
            "a verdict is keep, maybe, or drop, not '{verdict}'"
        ));
    }
    let has: bool = c
        .prepare_cached("SELECT 1 FROM runs WHERE id = ?1")
        .map_err(e)?
        .exists([run])
        .map_err(e)?;
    if !has {
        return Err(format!("no run {run}"));
    }
    c.execute(
        "INSERT INTO reviews(run_id, verdict, note, ts, rater, source) VALUES(?1,?2,?3,?4,?5,?6)",
        params![run, verdict, note, now(), rater, source],
    )
    .map_err(e)?;
    Ok(())
}

/// What a ratings folder held.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RatingsRead {
    pub files: u64,
    pub recorded: u64,
    /// Files whose run is not in this search.
    pub unknown_runs: u64,
    /// Files that did not read as a rating.
    pub unreadable: u64,
    /// Ratings already recorded from this folder, skipped.
    pub already: u64,
}

/// Record the ratings in a folder of rating files, as the clip-rating page saves them:
/// `<run id>.json` holding `{"verdict": "keep", "ts": <ms>}` (or that inside `data`), with an
/// optional `note`. A rating already recorded from the same folder is skipped.
pub fn import_ratings(c: &mut Connection, dir: &Path, rater: &str) -> Result<RatingsRead, String> {
    let rd = std::fs::read_dir(dir).map_err(|x| format!("{}: {x}", dir.display()))?;
    let source = format!("ratings:{}", dir.display());
    let mut out = RatingsRead::default();
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let tx = c.transaction().map_err(e)?;
    for p in files {
        let Some(run) = p
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        out.files += 1;
        let v: Option<serde_json::Value> = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let Some(v) = v else {
            out.unreadable += 1;
            continue;
        };
        let v = v.get("data").cloned().unwrap_or(v);
        let Some(verdict) = v
            .get("verdict")
            .and_then(|x| x.as_str())
            .filter(|x| VERDICTS.contains(x))
        else {
            out.unreadable += 1;
            continue;
        };
        let note = v.get("note").and_then(|x| x.as_str()).unwrap_or("");
        // Rating pages write milliseconds.
        let ts = v
            .get("ts")
            .and_then(|x| x.as_i64())
            .map(|t| if t > 100_000_000_000 { t / 1000 } else { t })
            .unwrap_or_else(now);
        let known: bool = tx
            .prepare_cached("SELECT 1 FROM runs WHERE id = ?1")
            .map_err(e)?
            .exists([run])
            .map_err(e)?;
        if !known {
            out.unknown_runs += 1;
            continue;
        }
        let dup: bool = tx
            .prepare_cached("SELECT 1 FROM reviews WHERE run_id = ?1 AND source = ?2")
            .map_err(e)?
            .exists(params![run, source])
            .map_err(e)?;
        if dup {
            out.already += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO reviews(run_id, verdict, note, ts, rater, source) VALUES(?1,?2,?3,?4,?5,?6)",
            params![run, verdict, note, ts, rater, source],
        )
        .map_err(e)?;
        out.recorded += 1;
    }
    tx.commit().map_err(e)?;
    Ok(out)
}

/// Reviews per verdict.
pub fn review_counts(c: &Connection) -> Result<Vec<(String, u64)>, String> {
    let mut st = c
        .prepare("SELECT verdict, COUNT(*) FROM reviews GROUP BY verdict ORDER BY verdict")
        .map_err(e)?;
    st.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
}

// ---------------------------------------------------------------------------------------------
// Rescoring

/// What [`rescore`] did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rescored {
    pub runs: u64,
    /// Runs whose score changed (only these are written).
    pub changed: u64,
    pub elites: u64,
    pub niches: u64,
}

/// Recompute every stored score from the stored features (after [`archive::score`] changes), then
/// rebuild the archive and the family totals from the screened runs with their new scores. Reads
/// every run: run it on a stopped search. `group_of` gives a tiling's group; `progress` is told
/// the runs done so far and whether to go on.
pub fn rescore(
    c: &mut Connection,
    group_of: &dyn Fn(&str) -> u8,
    progress: &dyn Fn(u64) -> bool,
) -> Result<Rescored, String> {
    let mut out = Rescored::default();
    let mut arch = Archive::default();
    let mut fams: HashMap<Fam, FamStat> = HashMap::new();
    let mut scores: Vec<(i64, f64)> = Vec::new();
    {
        let mut st = c
            .prepare(&format!(
                "SELECT id, stage, {EVAL_COLS} FROM runs ORDER BY id"
            ))
            .map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
            let id: i64 = r.get(0).map_err(e)?;
            let stage: i64 = r.get(1).map_err(e)?;
            let Some(mut ev) = eval_at(r, 2).map_err(e)? else {
                continue;
            };
            let old = ev.score;
            ev.score = archive::score(ev.fate, &ev.feat, ev.cand.gens, ev.cand.fam);
            if ev.score != old {
                scores.push((id, ev.score as f64));
            }
            out.runs += 1;
            if out.runs.is_multiple_of(100_000) && !progress(out.runs) {
                return Err("rescoring was stopped; nothing was changed".into());
            }
            if stage == 1 {
                fams.entry(ev.cand.fam).or_default().add(ev.score as f64);
                let g = group_of(&ev.cand.tiling);
                arch.offer(id, ev, g);
            }
        }
    }
    let changes = arch.take_changes();
    out.changed = scores.len() as u64;
    let tx = c.transaction().map_err(e)?;
    if !scores.is_empty() {
        tx.execute_batch("DROP INDEX IF EXISTS runs_score")
            .map_err(e)?;
    }
    {
        let mut st = tx
            .prepare("UPDATE runs SET score = ?1 WHERE id = ?2")
            .map_err(e)?;
        for (id, s) in &scores {
            st.execute(params![s, id]).map_err(e)?;
        }
        tx.execute("DELETE FROM elites", []).map_err(e)?;
        let mut st = tx
            .prepare("INSERT INTO elites(id, grp) VALUES(?1, ?2)")
            .map_err(e)?;
        for (id, g) in changes {
            if let Some(g) = g {
                st.execute(params![id, g]).map_err(e)?;
                out.elites += 1;
            }
        }
        tx.execute("DELETE FROM families", []).map_err(e)?;
        let mut st = tx
            .prepare("INSERT INTO families(fam, runs, score_sum, best) VALUES(?1, ?2, ?3, ?4)")
            .map_err(e)?;
        for (f, s) in &fams {
            st.execute(params![f.name(), s.runs as i64, s.score_sum, s.best])
                .map_err(e)?;
        }
    }
    tx.execute_batch("CREATE INDEX IF NOT EXISTS runs_score ON runs(score)")
        .map_err(e)?;
    tx.commit().map_err(e)?;
    out.niches = arch.cells.len() as u64;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Importing an old database

/// One row of an old database's `runs`, as it is, with its candidate and measures when they read.
pub struct OldRun {
    pub id: i64,
    pub stage: i64,
    pub parent: Option<i64>,
    pub seed_kind: Option<String>,
    pub ev: Option<Eval>,
    /// The row's columns in `runs` order, to copy.
    pub raw: Vec<rusqlite::types::Value>,
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
        let has: bool = old
            .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'runs'")
            .map_err(e)?
            .exists([])
            .map_err(e)?;
        if !has {
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
                "SELECT id,stage,parent,seed_kind,{EVAL_COLS},
                 id,stage,parent,tiling,nb,fam,rule,seed,seed_kind,cells,gens,steps,rng,origin,
                 fate,settle,period,score,bin,feats,ts
                 FROM runs WHERE id > ?1 ORDER BY id LIMIT ?2"
            ))
            .map_err(e)?;
        let mut rows = st.query(params![self.last, n as i64]).map_err(e)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(e)? {
            let raw: Vec<rusqlite::types::Value> = (18..39)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<Result<_, _>>()
                .map_err(e)?;
            out.push(OldRun {
                id: r.get(0).map_err(e)?,
                stage: r.get::<_, Option<i64>>(1).map_err(e)?.unwrap_or(1),
                parent: r.get(2).map_err(e)?,
                seed_kind: r.get(3).map_err(e)?,
                ev: eval_at(r, 4).map_err(e)?,
                raw,
            });
        }
        if let Some(l) = out.last() {
            self.last = l.id;
        }
        Ok(out)
    }

    /// The old database's reviews (`run_id, verdict, note, ts`).
    pub fn reviews(&self) -> Result<Vec<(i64, String, String, i64)>, String> {
        let has: bool = self
            .old
            .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'reviews'")
            .map_err(e)?
            .exists([])
            .map_err(e)?;
        if !has {
            return Ok(vec![]);
        }
        let mut st = self
            .old
            .prepare("SELECT run_id, verdict, COALESCE(note, ''), COALESCE(ts, 0) FROM reviews")
            .map_err(e)?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)
    }
}

/// Copy old rows into `runs` as they are (same ids), with what they changed, in one transaction.
pub fn write_imported(
    c: &mut Connection,
    rows: &[OldRun],
    delta: &Delta,
    reviews: &[(i64, String, String, i64)],
    source: &str,
) -> Result<(), String> {
    let tx = c.transaction().map_err(e)?;
    {
        let mut st = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO runs(id,stage,parent,tiling,nb,fam,rule,seed,seed_kind,
                 cells,gens,steps,rng,origin,fate,settle,period,score,bin,feats,ts)
                 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )
            .map_err(e)?;
        for r in rows {
            st.execute(rusqlite::params_from_iter(r.raw.iter()))
                .map_err(e)?;
        }
        let mut st = tx
            .prepare_cached(
                "INSERT INTO reviews(run_id, verdict, note, ts, rater, source)
                 VALUES(?1, ?2, ?3, ?4, '', ?5)",
            )
            .map_err(e)?;
        for (run, v, note, ts) in reviews {
            st.execute(params![run, v, note, ts, source]).map_err(e)?;
        }
    }
    write_in(&tx, delta)?;
    tx.commit().map_err(e)
}

/// Start importing `from` into a new search: its indexes are dropped until the import is done
/// (copying millions of rows is faster without them), and `meta` says what is being imported.
pub fn begin_import(c: &Connection, from: &Path) -> Result<(), String> {
    c.execute_batch(
        "DROP INDEX IF EXISTS runs_score; DROP INDEX IF EXISTS runs_parent;
         DROP INDEX IF EXISTS reviews_run;",
    )
    .map_err(e)?;
    for (k, v) in [
        ("import_from", from.display().to_string()),
        ("import_done", "0".into()),
        ("import_last", "0".into()),
    ] {
        c.execute(
            "INSERT OR REPLACE INTO meta(k, v) VALUES(?1, ?2)",
            params![k, v],
        )
        .map_err(e)?;
    }
    Ok(())
}

/// Build the indexes an import leaves out (after the last row is in).
pub fn finish_import(c: &Connection) -> Result<(), String> {
    c.execute_batch(INDEXES).map_err(e)
}

// ---------------------------------------------------------------------------------------------
// Finding runs

/// Which stored runs to list.
#[derive(Clone, Debug, Default)]
pub struct RunFilter {
    /// These runs, in this order (instead of the rest).
    pub ids: Option<Vec<i64>>,
    /// One `SELECT` returning run ids (instead of the rest).
    pub sql: Option<String>,
    pub family: Option<Fam>,
    pub tiling: Option<String>,
    /// died, froze, cycle, running.
    pub fate: Option<String>,
    pub min_score: Option<f64>,
    /// 1 screened (the default), 2 confirmation runs.
    pub stage: Option<u8>,
    /// Only reviewed runs: `any`, or a verdict.
    pub reviewed: Option<String>,
    /// `score` (best first, the default), `newest`, or `id`.
    pub order: Option<String>,
}

/// The ids of the runs a filter names, in order, at most `limit`.
pub fn run_ids(c: &Connection, f: &RunFilter, limit: usize) -> Result<Vec<i64>, String> {
    let collect = |sql: &str| -> Result<Vec<i64>, String> {
        let mut st = c.prepare(sql).map_err(|x| format!("sql: {x}"))?;
        st.query_map([], |r| r.get::<_, i64>(0))
            .and_then(|it| it.collect::<Result<Vec<i64>, _>>())
            .map_err(|x| format!("sql: {x}"))
    };
    if let Some(ids) = &f.ids {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let list: Vec<String> = ids.iter().map(i64::to_string).collect();
        let found: HashSet<i64> = collect(&format!(
            "SELECT id FROM runs WHERE id IN ({})",
            list.join(",")
        ))?
        .into_iter()
        .collect();
        let mut seen = HashSet::new();
        return Ok(ids
            .iter()
            .copied()
            .filter(|i| found.contains(i) && seen.insert(*i))
            .take(limit)
            .collect());
    }
    if let Some(s) = &f.sql {
        let t = s.trim().trim_end_matches(';');
        if !t.to_ascii_lowercase().starts_with("select") || t.contains(';') {
            return Err("sql must be a single SELECT that returns run ids".into());
        }
        return collect(&format!("SELECT * FROM ({t}) LIMIT {limit}"));
    }
    let mut w = vec![format!("stage = {}", f.stage.unwrap_or(1))];
    if let Some(x) = f.family {
        w.push(format!("fam = '{}'", x.name()));
    }
    if let Some(t) = &f.tiling {
        w.push(format!("tiling = '{}'", t.replace('\'', "''")));
    }
    if let Some(x) = &f.fate {
        if !["died", "froze", "cycle", "running"].contains(&x.as_str()) {
            return Err(format!("a fate is died, froze, cycle, or running, not {x}"));
        }
        w.push(format!("fate = '{x}'"));
    }
    if let Some(m) = f.min_score {
        if !m.is_finite() {
            return Err("min_score must be a number".into());
        }
        w.push(format!("score >= {m}"));
    }
    match f.reviewed.as_deref() {
        None => {}
        Some("any") => w.push("id IN (SELECT run_id FROM reviews)".into()),
        Some(v) if VERDICTS.contains(&v) => w.push(format!(
            "id IN (SELECT run_id FROM reviews WHERE verdict = '{v}')"
        )),
        Some(v) => return Err(format!("reviewed is any, keep, maybe, or drop, not {v}")),
    }
    let order = match f.order.as_deref().unwrap_or("score") {
        "score" => "score DESC, id",
        "newest" => "id DESC",
        "id" => "id",
        o => return Err(format!("order is score, newest, or id, not {o}")),
    };
    collect(&format!(
        "SELECT id FROM runs WHERE {} ORDER BY {order} LIMIT {limit}",
        w.join(" AND ")
    ))
}

/// The stored runs `ids` names, in that order.
pub fn runs_by_ids(c: &Connection, ids: &[i64]) -> Result<Vec<super::sheet::RunRow>, String> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let list: Vec<String> = ids.iter().map(i64::to_string).collect();
    let mut found = super::sheet::query_runs(
        c,
        &format!("SELECT id FROM runs WHERE id IN ({})", list.join(",")),
    )?;
    let pos: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, x)| (*x, i)).collect();
    found.sort_by_key(|r| pos.get(&r.id).copied().unwrap_or(usize::MAX));
    Ok(found)
}
