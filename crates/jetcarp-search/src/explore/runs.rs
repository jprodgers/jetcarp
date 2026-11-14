//! An explore search's run records, wherever they are: the sealed Parquet segments
//! ([`crate::explore::segments`]) its `manifest.json` lists, and the runs not yet sealed in its
//! working database ([`crate::explore::store`]). Every reader goes through [`Runs`], so it sees
//! one set of runs with one id sequence.
//!
//! ```text
//! <searches>/<uuid>/search.sqlite              working state and the runs not yet sealed
//! <searches>/<uuid>/runs/part-000001.parquet   sealed runs (immutable, zstd)
//! <searches>/<uuid>/scores-1/part-000001.parquet  a rescore's scores for that segment
//! <searches>/<uuid>/manifest.json              the spec, versions, segments, and score sets
//! ```
//!
//! **Sealing.** A search's newest runs go to the working database; once [`SEAL_ROWS`] of them
//! are there, the oldest [`SEAL_ROWS`] are sealed into the next segment ([`Runs::seal_full`]).
//! Stopping a search seals nothing: its newest runs (fewer than [`SEAL_ROWS`]) stay in the working
//! database, where the next session adds to them, so every segment but the last holds
//! [`SEAL_ROWS`] runs and a search stopped and continued a thousand times still has one segment
//! per [`SEAL_ROWS`] runs. [`Runs::seal_all`] seals the rest on request (before sharing a folder;
//! `jetcarp jobs seal`).
//!
//! **Crash safety.** A segment is written to `part-NNNNNN.parquet.tmp`, synced, opened again and
//! checked (rows and ids), renamed, added to the manifest (written to a temporary file, synced,
//! renamed over the old one), and only then are its runs deleted from the working database.
//! Opening a search for writing ([`Runs::open`]) recovers from a crash at any of those points:
//! temporary files are removed, a segment file the manifest does not list is removed when the
//! working database still holds its runs, and runs that a listed segment holds are deleted from
//! the working database. Doing it twice changes nothing.
//!
//! **Readers** open a read transaction on the working database before they read the manifest,
//! and skip working-database runs with ids the manifest's segments cover, so a seal under way in
//! another process never shows them a run twice or not at all.
//!
//! **Rescoring** never rewrites a segment: it writes a score sidecar per changed segment
//! (`scores-<n>/`, `n` counting rescores), and readers take each segment's scores from the newest
//! sidecar that has it.

use crate::explore::archive::{self, Archive, Eval, ScoreModel};
use crate::explore::segments::{
    self as seg, ALL, Batch, Cols, RunRec, Segment, SegmentWriter, Written, col, cols,
};
use crate::explore::sheet::RunRow;
use crate::explore::store::{self, FamStat, put_archive};
use jetcarp_core::explore::features::SignMeasures;
use jetcarp_core::explore::space::Fam;
use rayon::prelude::*;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The manifest's file name in a search folder.
pub const MANIFEST: &str = "manifest.json";
/// What the manifest's `format` says.
pub const MANIFEST_FORMAT: &str = "jetcarp-search-manifest 1";
/// Runs per sealed segment.
pub const SEAL_ROWS: u64 = 1_000_000;

fn e(x: rusqlite::Error) -> String {
    x.to_string()
}

fn io(p: &Path, x: std::io::Error) -> String {
    format!("{}: {x}", p.display())
}

/// A sealed segment, as the manifest lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SegmentEntry {
    /// Relative to the search folder: `runs/part-000001.parquet`.
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
    pub rows: u64,
    /// Its first and last run ids.
    pub first_id: i64,
    pub last_id: i64,
    /// When it was sealed (UTC), and by which JETCARP.
    pub sealed: String,
    pub engine_version: String,
}

/// A segment's scores from one rescore.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreFile {
    /// The segment it scores (`runs/part-000001.parquet`).
    pub segment: String,
    /// Relative to the search folder: `scores-1/part-000001.parquet`.
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
    pub rows: u64,
}

/// What one rescore wrote.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreSet {
    /// 1 for the first rescore, then 2, ...
    pub version: u32,
    pub created: String,
    pub engine_version: String,
    /// One per segment whose scores changed.
    pub files: Vec<ScoreFile>,
}

/// A search folder's `manifest.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub format: String,
    /// The search's uuid (its folder's name).
    pub search: String,
    /// The job kind: `explore`.
    pub kind: String,
    /// The JETCARP that last wrote this manifest.
    pub engine_version: String,
    /// The segments' and sidecars' schemas ([`seg::RUN_SCHEMA`], [`seg::SCORE_SCHEMA`]): the
    /// newest segment's (each segment names its own in its metadata; a search sealed before
    /// schema 2 and continued after holds both).
    pub run_schema: String,
    pub score_schema: String,
    pub created: String,
    pub updated: String,
    /// The job's spec (its search settings).
    pub spec: Value,
    /// In id order: each one's runs come after the one before's.
    pub segments: Vec<SegmentEntry>,
    pub scores: Vec<ScoreSet>,
    /// Only in the search imported from CART: how its runs were checked on the engine of the
    /// import, and which were run again ([`RemeasureRecord`]). Read and kept; nothing writes it
    /// now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remeasure: Option<RemeasureRecord>,
}

/// What the one-time import of CART's search checked on the engine of the import and re-measured:
/// its manifest's `remeasure`, kept as provenance. The importer was removed after the import (it
/// is in the history up to commit `f3492e8`); this JETCARP reads the record and keeps it, and
/// never writes a new one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemeasureRecord {
    /// `auto`, `all`, or `none`.
    pub mode: String,
    /// The JETCARP that measured, and when the check was made (UTC).
    pub engine_version: String,
    pub checked: String,
    /// The check: runs sampled per tiling at most, tilings, sampled runs, and the sampled runs
    /// that differ (auto only).
    pub sample_per_tiling: u64,
    pub tilings: u64,
    pub sampled: u64,
    pub sampled_differ: u64,
    /// What ran the runs the GPU can run: `cpu`, or the GPU's name.
    pub backend: String,
    /// The tilings re-measured, by name.
    pub remeasured: Vec<Remeasured>,
    /// Runs to re-measure (as the check counted them; 0 with `all`), runs re-measured in all,
    /// and those whose measures changed.
    pub to_remeasure: u64,
    pub runs: u64,
    pub changed: u64,
    /// Seconds the check took, and the re-measuring (read as a number, or as a decimal string,
    /// as a pack's search index writes them).
    #[serde(deserialize_with = "f64_or_text")]
    pub check_seconds: f64,
    #[serde(deserialize_with = "f64_or_text")]
    pub remeasure_seconds: f64,
    /// Whether every run had been copied.
    pub done: bool,
}

/// A number read from JSON as a number or as a string holding one (a pack's documents write
/// decimals as strings).
pub fn f64_or_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    use serde::de::Error;
    match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom("not a number")),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
            .ok_or_else(|| D::Error::custom(format!("'{s}' is not a number"))),
        Value::Null => Ok(0.0),
        other => Err(D::Error::custom(format!("{other} is not a number"))),
    }
}

/// [`f64_or_text`] for an optional number.
pub fn opt_f64_or_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => f64_or_text(v).map(Some).map_err(serde::de::Error::custom),
    }
}

/// A tiling the import re-measured, as the manifest lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remeasured {
    pub tiling: String,
    /// The one rule family whose runs were re-measured (none: every run of the tiling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub why: String,
    /// Runs run again, those whose measures changed, and those that could not run (kept as
    /// they were).
    pub runs: u64,
    pub changed: u64,
    pub failed: u64,
}

#[cfg(unix)]
fn sync_dir(d: &Path) -> Result<(), String> {
    std::fs::File::open(d)
        .and_then(|f| f.sync_all())
        .map_err(|x| io(d, x))
}

#[cfg(not(unix))]
fn sync_dir(_d: &Path) -> Result<(), String> {
    Ok(())
}

impl Manifest {
    fn new(search: &str) -> Manifest {
        let now = jetcarp_core::pattern::now_utc();
        Manifest {
            format: MANIFEST_FORMAT.into(),
            search: search.into(),
            kind: "explore".into(),
            engine_version: jetcarp_core::VERSION.into(),
            run_schema: seg::RUN_SCHEMA.into(),
            score_schema: seg::SCORE_SCHEMA.into(),
            created: now.clone(),
            updated: now,
            spec: Value::Null,
            segments: Vec::new(),
            scores: Vec::new(),
            remeasure: None,
        }
    }

    /// The manifest of a search folder (`None` when it has none).
    pub fn read(dir: &Path) -> Result<Option<Manifest>, String> {
        let p = dir.join(MANIFEST);
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(x) if x.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(x) => return Err(io(&p, x)),
        };
        let m: Manifest =
            serde_json::from_str(&text).map_err(|x| format!("{}: {x}", p.display()))?;
        if m.format != MANIFEST_FORMAT {
            return Err(format!(
                "{}: a manifest of format '{}', not '{MANIFEST_FORMAT}' (update JETCARP to read it)",
                p.display(),
                m.format
            ));
        }
        Ok(Some(m))
    }

    /// Write it: to a temporary file, synced, renamed over the old one.
    fn write(&mut self, dir: &Path) -> Result<(), String> {
        self.updated = jetcarp_core::pattern::now_utc();
        self.engine_version = jetcarp_core::VERSION.into();
        let p = dir.join(MANIFEST);
        let tmp = dir.join(format!("{MANIFEST}.tmp"));
        let text = serde_json::to_string_pretty(self).map_err(|x| x.to_string())? + "\n";
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).map_err(|x| io(&tmp, x))?;
            f.write_all(text.as_bytes()).map_err(|x| io(&tmp, x))?;
            f.sync_all().map_err(|x| io(&tmp, x))?;
        }
        std::fs::rename(&tmp, &p).map_err(|x| io(&p, x))?;
        sync_dir(dir)
    }

    /// The last sealed run id (0 with no segments).
    pub fn last_id(&self) -> i64 {
        self.segments.last().map_or(0, |s| s.last_id)
    }

    /// Sealed runs.
    pub fn rows(&self) -> u64 {
        self.segments.iter().map(|s| s.rows).sum()
    }

    /// The segment that would hold run `id`.
    pub fn segment_of(&self, id: i64) -> Option<usize> {
        let k = self.segments.partition_point(|s| s.last_id < id);
        (k < self.segments.len() && self.segments[k].first_id <= id).then_some(k)
    }

    /// The newest sidecar of segment `k`.
    pub fn scores_of(&self, k: usize) -> Option<&ScoreFile> {
        let name = &self.segments.get(k)?.file;
        self.scores
            .iter()
            .rev()
            .find_map(|s| s.files.iter().find(|f| &f.segment == name))
    }
}

/// Where to stop sealing, to test recovery: [`Runs::seal_stopping`] returns an error at that
/// point as if the process had died there, leaving its files as they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealStop {
    /// The temporary file has row groups but no footer.
    BeforeFooter,
    /// The temporary file is whole but not synced.
    BeforeSync,
    /// The temporary file is synced, not yet checked.
    BeforeVerify,
    /// Checked, not yet renamed.
    BeforeRename,
    /// Renamed, not yet in the manifest.
    BeforeManifest,
    /// In the manifest, its runs not yet deleted from the working database.
    BeforeDelete,
}

/// A segment being written from the working database's oldest runs.
///
/// The file is encoded on a thread of its own, so reading the runs and encoding them overlap.
struct Building {
    tx: Option<std::sync::mpsc::SyncSender<Vec<RunRec>>>,
    worker: Option<std::thread::JoinHandle<Result<SegmentWriter, String>>>,
    buf: Vec<RunRec>,
    rows: u64,
    /// Relative to the search folder.
    name: String,
    tmp: PathBuf,
    last: i64,
}

/// A segment's names once its writer is done.
struct Named {
    name: String,
    tmp: PathBuf,
}

/// Runs handed to a segment's writer thread at a time.
const HAND: usize = 16_384;

impl Building {
    fn new(w: SegmentWriter, name: String, tmp: PathBuf) -> Building {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<RunRec>>(4);
        let worker = std::thread::Builder::new()
            .name("seal".into())
            .spawn(move || {
                let mut w = w;
                for chunk in rx {
                    for r in chunk {
                        w.push(r)?;
                    }
                }
                Ok(w)
            })
            .ok();
        Building {
            tx: Some(tx),
            worker,
            buf: Vec::with_capacity(HAND),
            rows: 0,
            name,
            tmp,
            last: 0,
        }
    }

    fn push(&mut self, r: RunRec) -> Result<(), String> {
        if self.rows > 0 && r.id <= self.last {
            return Err(format!(
                "run {} comes after run {} in a segment",
                r.id, self.last
            ));
        }
        self.last = r.id;
        self.rows += 1;
        self.buf.push(r);
        if self.buf.len() >= HAND {
            self.hand()?;
        }
        Ok(())
    }

    fn hand(&mut self) -> Result<(), String> {
        let chunk = std::mem::replace(&mut self.buf, Vec::with_capacity(HAND));
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(chunk).is_ok());
        if sent {
            return Ok(());
        }
        // The writer stopped: its error says why.
        self.tx = None;
        match self.worker.take().map(|h| h.join()) {
            Some(Ok(Err(e))) => Err(e),
            _ => Err("the segment writer stopped".into()),
        }
    }

    /// The writer, once every run is written to it.
    fn writer(mut self) -> Result<SegmentWriter, String> {
        if !self.buf.is_empty() {
            self.hand()?;
        }
        self.tx = None;
        match self.worker.take().map(|h| h.join()) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err("the segment writer panicked".into()),
            None => Err("the segment writer did not start".into()),
        }
    }

    fn rows(&self) -> u64 {
        self.rows
    }
}

impl Drop for Building {
    /// A segment left unfinished (a seal that failed part way): its writer finishes what it was
    /// handed, and the temporary file stays for recovery to remove.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

/// Which stored runs to list.
#[derive(Clone, Debug, Default)]
pub struct RunFilter {
    /// These runs, in this order (instead of the rest).
    pub ids: Option<Vec<i64>>,
    pub family: Option<Fam>,
    pub tiling: Option<String>,
    /// died, froze, cycle, running.
    pub fate: Option<String>,
    pub min_score: Option<f64>,
    /// 1 screened (the default), 2 confirmation runs.
    pub stage: Option<u8>,
    /// Only reviewed runs: `any`, or a verdict. Reviews are in the library: the caller turns
    /// this into [`within`](Self::within) ([`crate::explore::reviews::resolve`]).
    pub reviewed: Option<String>,
    /// Only these runs, with the other filters and the order.
    pub within: Option<Vec<i64>>,
    /// `score` (best first, the default), `newest`, or `id`.
    pub order: Option<String>,
    /// Only runs with a larger id than this (for exports in parts).
    pub after: Option<i64>,
}

/// What [`Runs::rescore`] did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rescored {
    pub runs: u64,
    /// Runs whose score changed.
    pub changed: u64,
    pub elites: u64,
    pub niches: u64,
    /// Score sidecars written (one per sealed segment with a changed score), and the rescore's
    /// version (0 when none were needed).
    pub sidecars: u64,
    pub version: u32,
}

/// An explore search's runs: its sealed segments and its working database.
pub struct Runs {
    /// The search folder.
    pub dir: PathBuf,
    /// The working database.
    pub db: Connection,
    pub manifest: Manifest,
    files: Files,
    unsealed: u64,
    writable: bool,
    /// How new segments are cut and compressed (the defaults; tests make them small).
    pub tuning: seg::Tuning,
}

fn uuid_of(dir: &Path) -> String {
    dir.file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_tmp(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "tmp")
}

fn segment_name(k: usize) -> String {
    format!("runs/part-{k:06}.parquet")
}

impl Runs {
    /// Open a search's runs for writing (making the working database and manifest if new),
    /// recover from a crash, and seal the runs of a database from before sealed segments.
    pub fn open(db_path: &Path) -> Result<Runs, String> {
        let dir = db_path
            .parent()
            .ok_or_else(|| format!("{} has no folder", db_path.display()))?
            .to_path_buf();
        let db = store::open(db_path)?;
        let format = store::meta_get(&db, "format")?.unwrap_or_default();
        let had = Manifest::read(&dir)?;
        let new_manifest = had.is_none();
        let mut r = Runs {
            manifest: had.unwrap_or_else(|| Manifest::new(&uuid_of(&dir))),
            dir,
            db,
            files: Mutex::new(HashMap::new()),
            unsealed: 0,
            writable: true,
            tuning: seg::Tuning::default(),
        };
        r.recover()?;
        if new_manifest {
            r.manifest.write(&r.dir)?;
        }
        if format != store::FORMAT {
            // Before sealed segments: every run is in `runs`. Sealed without the score index
            // (deleting millions of rows is faster without it), then the file shrunk to what is
            // left.
            r.db.execute_batch("DROP INDEX IF EXISTS runs_score")
                .map_err(e)?;
            r.seal_all()?;
            store::meta_set(&r.db, "format", store::FORMAT)?;
            r.db.execute_batch("CREATE INDEX IF NOT EXISTS runs_score ON runs(score); VACUUM;")
                .map_err(e)?;
        }
        Ok(r)
    }

    /// Open a search's runs for reading (or an old database without a manifest, whose runs are
    /// all in it).
    pub fn open_read(db_path: &Path) -> Result<Runs, String> {
        let dir = db_path
            .parent()
            .ok_or_else(|| format!("{} has no folder", db_path.display()))?
            .to_path_buf();
        let db = store::open_read(db_path)?;
        // The working database's state first (a read transaction holds it), then the manifest:
        // a seal finishing meanwhile shows its runs in both, never in neither.
        db.execute_batch("BEGIN").map_err(e)?;
        store::has_table(&db, "runs")?;
        let manifest = Manifest::read(&dir)?.unwrap_or_else(|| Manifest::new(&uuid_of(&dir)));
        Ok(Runs {
            dir,
            db,
            manifest,
            files: Mutex::new(HashMap::new()),
            unsealed: 0,
            writable: false,
            tuning: seg::Tuning::default(),
        })
    }

    /// The search's uuid (its folder's name).
    pub fn uuid(&self) -> String {
        uuid_of(&self.dir)
    }

    /// Record the job's spec in the manifest (written when it changed).
    pub fn set_spec(&mut self, spec: &Value) -> Result<(), String> {
        if &self.manifest.spec != spec {
            self.manifest.spec = spec.clone();
            self.manifest.write(&self.dir)?;
        }
        Ok(())
    }

    fn need_write(&self) -> Result<(), String> {
        if self.writable {
            Ok(())
        } else {
            Err("the search's runs are open for reading only".into())
        }
    }

    // -----------------------------------------------------------------------------------------
    // Recovery

    fn recover(&mut self) -> Result<(), String> {
        // Temporary files: never complete.
        let _ = std::fs::remove_file(self.dir.join(format!("{MANIFEST}.tmp")));
        let mut changed = false;
        let runs_dir = self.dir.join("runs");
        let mut listed: HashSet<String> = self
            .manifest
            .segments
            .iter()
            .map(|s| s.file.clone())
            .collect();
        let mut found: Vec<PathBuf> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&runs_dir) {
            for d in rd.flatten() {
                found.push(d.path());
            }
        }
        found.sort();
        for p in found {
            let name = p
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = format!("runs/{name}");
            if is_tmp(&p) {
                std::fs::remove_file(&p).map_err(|x| io(&p, x))?;
                continue;
            }
            if !name.ends_with(".parquet") || listed.contains(&rel) {
                continue;
            }
            // A whole segment the manifest does not list: renamed before a crash.
            let s = Segment::open(&p);
            let range = s.as_ref().ok().and_then(|s| {
                let n = s.row_groups().checked_sub(1)?;
                Some((
                    s.int_range(0, col::ID)?.0,
                    s.int_range(n, col::ID)?.1,
                    s.rows(),
                ))
            });
            let Some((first, last, rows)) = range else {
                std::fs::remove_file(&p).map_err(|x| io(&p, x))?;
                continue;
            };
            let held: i64 = self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM runs WHERE id BETWEEN ?1 AND ?2",
                    params![first, last],
                    |r| r.get(0),
                )
                .map_err(e)?;
            if held as u64 == rows {
                std::fs::remove_file(&p).map_err(|x| io(&p, x))?;
            } else if first > self.manifest.last_id() {
                // Its runs are nowhere else: keep it.
                let entry = SegmentEntry {
                    file: rel.clone(),
                    sha256: seg::file_sha256(&p)?,
                    bytes: std::fs::metadata(&p).map_err(|x| io(&p, x))?.len(),
                    rows,
                    first_id: first,
                    last_id: last,
                    sealed: jetcarp_core::pattern::now_utc(),
                    engine_version: jetcarp_core::VERSION.into(),
                };
                self.manifest.segments.push(entry);
                listed.insert(rel);
                changed = true;
            }
        }
        // Sidecars the manifest does not list (a rescore that did not finish).
        let sides: HashSet<String> = self
            .manifest
            .scores
            .iter()
            .flat_map(|s| s.files.iter().map(|f| f.file.clone()))
            .collect();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for d in rd.flatten() {
                let name = d.file_name().to_string_lossy().into_owned();
                if !name.starts_with("scores-") || !d.path().is_dir() {
                    continue;
                }
                for f in std::fs::read_dir(d.path()).into_iter().flatten().flatten() {
                    let rel = format!("{name}/{}", f.file_name().to_string_lossy());
                    if is_tmp(&f.path()) || !sides.contains(&rel) {
                        std::fs::remove_file(f.path()).map_err(|x| io(&f.path(), x))?;
                    }
                }
                let _ = std::fs::remove_dir(d.path());
            }
        }
        if changed {
            self.manifest.write(&self.dir)?;
        }
        // Runs a listed segment holds.
        let last = self.manifest.last_id();
        if last > 0 {
            self.db
                .execute("DELETE FROM runs WHERE id <= ?1", [last])
                .map_err(e)?;
        }
        self.unsealed = self
            .db
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get::<_, i64>(0))
            .map_err(e)? as u64;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Writing and sealing

    /// Write a step's changes ([`store::write`]).
    pub fn write(&mut self, d: &store::Delta) -> Result<(), String> {
        self.need_write()?;
        store::write(&mut self.db, d)?;
        self.unsealed += d.rows.len() as u64;
        Ok(())
    }

    /// Runs in the working database (not yet sealed).
    pub fn unsealed(&self) -> u64 {
        self.unsealed
    }

    /// Seal while at least `every` runs are unsealed, `every` at a time.
    pub fn seal_full(&mut self, every: u64) -> Result<Vec<SegmentEntry>, String> {
        let mut out = Vec::new();
        while self.unsealed >= every.max(1) {
            match self.seal_stopping(every.max(1), None)? {
                Some(s) => out.push(s),
                None => break,
            }
        }
        Ok(out)
    }

    /// Seal every unsealed run, [`SEAL_ROWS`] at a time (the last segment may be shorter).
    pub fn seal_all(&mut self) -> Result<Vec<SegmentEntry>, String> {
        let mut out = Vec::new();
        while let Some(s) = self.seal_stopping(SEAL_ROWS, None)? {
            out.push(s);
        }
        Ok(out)
    }

    /// Seal the oldest `max` unsealed runs into the next segment (`None`: none were unsealed),
    /// or stop at `stop` as if the process died there.
    pub fn seal_stopping(
        &mut self,
        max: u64,
        stop: Option<SealStop>,
    ) -> Result<Option<SegmentEntry>, String> {
        self.need_write()?;
        let after = self.manifest.last_id();
        let mut b = self.begin_segment()?;
        {
            let mut st = self
                .db
                .prepare(&format!(
                    "SELECT {} FROM runs WHERE id > ?1 ORDER BY id LIMIT ?2",
                    store::RUN_COLS
                ))
                .map_err(e)?;
            let mut rows = st.query(params![after, max as i64]).map_err(e)?;
            while let Some(r) = rows.next().map_err(e)? {
                b.push(store::rec_at(r, 0).map_err(e)?)?;
            }
        }
        if b.rows() == 0 {
            let tmp = b.tmp.clone();
            drop(b.writer());
            let _ = std::fs::remove_file(&tmp);
            return Ok(None);
        }
        let entry = self.finish_segment(b, stop)?;
        if stop == Some(SealStop::BeforeDelete) {
            return Err("stopped before the sealed runs were deleted (a test)".into());
        }
        let n = self
            .db
            .execute(
                "DELETE FROM runs WHERE id BETWEEN ?1 AND ?2",
                params![entry.first_id, entry.last_id],
            )
            .map_err(e)?;
        self.unsealed = self.unsealed.saturating_sub(n as u64);
        Ok(Some(entry))
    }

    /// Start the next segment, to be filled with runs and ended by
    /// [`finish_segment`](Self::finish_segment).
    fn begin_segment(&self) -> Result<Building, String> {
        self.need_write()?;
        let name = segment_name(self.manifest.segments.len() + 1);
        let tmp = self.dir.join(format!("{name}.tmp"));
        let kv = vec![
            ("jetcarp.search".to_string(), self.uuid()),
            ("jetcarp.segment".to_string(), name.clone()),
            (
                "jetcarp.engine_version".to_string(),
                jetcarp_core::VERSION.to_string(),
            ),
        ];
        let w = SegmentWriter::create_tuned(&tmp, &kv, &self.tuning)?;
        Ok(Building::new(w, name, tmp))
    }

    /// Finish a segment: footer, sync, check, rename, manifest.
    fn finish_segment(
        &mut self,
        b: Building,
        stop: Option<SealStop>,
    ) -> Result<SegmentEntry, String> {
        let ws = match stop {
            Some(SealStop::BeforeFooter) => Some(seg::WriteStop::BeforeFooter),
            Some(SealStop::BeforeSync) => Some(seg::WriteStop::BeforeSync),
            _ => None,
        };
        let (name, tmp) = (b.name.clone(), b.tmp.clone());
        let w: Written = b.writer()?.finish_or_stop(ws)?;
        let b = Named { name, tmp };
        if w.first_id <= self.manifest.last_id() {
            return Err(format!(
                "segment {} starts at run {}, not after run {}",
                b.name,
                w.first_id,
                self.manifest.last_id()
            ));
        }
        if stop == Some(SealStop::BeforeVerify) {
            return Err("stopped before the check (a test)".into());
        }
        let s = Segment::open(&b.tmp)?;
        let n = s.row_groups();
        let ok = s.rows() == w.rows
            && n > 0
            && s.int_range(0, col::ID).map(|r| r.0) == Some(w.first_id)
            && s.int_range(n - 1, col::ID).map(|r| r.1) == Some(w.last_id);
        drop(s);
        if !ok {
            let _ = std::fs::remove_file(&b.tmp);
            return Err(format!(
                "{} did not read back as written ({} runs, {} to {})",
                b.tmp.display(),
                w.rows,
                w.first_id,
                w.last_id
            ));
        }
        let sha256 = seg::file_sha256(&b.tmp)?;
        let bytes = std::fs::metadata(&b.tmp).map_err(|x| io(&b.tmp, x))?.len();
        if stop == Some(SealStop::BeforeRename) {
            return Err("stopped before the rename (a test)".into());
        }
        let fin = self.dir.join(&b.name);
        std::fs::rename(&b.tmp, &fin).map_err(|x| io(&fin, x))?;
        sync_dir(&self.dir.join("runs"))?;
        if stop == Some(SealStop::BeforeManifest) {
            return Err("stopped before the manifest (a test)".into());
        }
        let entry = SegmentEntry {
            file: b.name,
            sha256,
            bytes,
            rows: w.rows,
            first_id: w.first_id,
            last_id: w.last_id,
            sealed: jetcarp_core::pattern::now_utc(),
            engine_version: jetcarp_core::VERSION.into(),
        };
        self.manifest.segments.push(entry.clone());
        // Its newest segment's schema (older segments keep theirs: readers take both).
        self.manifest.run_schema = seg::RUN_SCHEMA.into();
        self.manifest.write(&self.dir)?;
        Ok(entry)
    }

    // -----------------------------------------------------------------------------------------
    // Reading

    /// Segment `k` and its newest sidecar.
    fn segment(&self, k: usize) -> Result<(Arc<Segment>, Option<Arc<Segment>>), String> {
        open_segment(&self.files, &self.dir, &self.manifest, k)
    }

    fn segments(&self) -> Result<Vec<(Arc<Segment>, Option<Arc<Segment>>)>, String> {
        let (files, dir, m) = (&self.files, &self.dir, &self.manifest);
        (0..m.segments.len())
            .into_par_iter()
            .map(|k| open_segment(files, dir, m, k))
            .collect()
    }

    /// The sealed segments' files, in id order.
    pub fn segment_files(&self) -> Vec<PathBuf> {
        self.manifest
            .segments
            .iter()
            .map(|s| self.dir.join(&s.file))
            .collect()
    }

    /// The largest run id (0 with none).
    pub fn max_id(&self) -> Result<i64, String> {
        let m: i64 = self
            .db
            .query_row("SELECT COALESCE(MAX(id), 0) FROM runs", [], |r| r.get(0))
            .map_err(e)?;
        Ok(m.max(self.manifest.last_id()))
    }

    /// The unsealed runs, a batch at a time.
    fn unsealed_batches(
        &self,
        f: &mut dyn FnMut(&Batch) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut st = self
            .db
            .prepare(&format!(
                "SELECT {} FROM runs WHERE id > ?1 ORDER BY id",
                store::RUN_COLS
            ))
            .map_err(e)?;
        let mut rows = st.query([self.manifest.last_id()]).map_err(e)?;
        let mut buf = Vec::with_capacity(4096);
        while let Some(r) = rows.next().map_err(e)? {
            buf.push(store::rec_at(r, 0).map_err(e)?);
            if buf.len() >= 16_384 {
                f(&Batch::of(&buf))?;
                buf.clear();
            }
        }
        if !buf.is_empty() {
            f(&Batch::of(&buf))?;
        }
        Ok(())
    }

    /// Every run through `f` (columns `want` at least), a batch at a time: the sealed row groups
    /// `keep` passes (given a segment, its sidecar, and a row group) in parallel, then the
    /// unsealed runs. What `f` pushes comes back in id order.
    pub fn scan<T: Send>(
        &self,
        want: Cols,
        keep: &(dyn Fn(&Segment, Option<&Segment>, usize) -> bool + Sync),
        f: &(dyn Fn(&Batch, &mut Vec<T>) + Sync),
    ) -> Result<Vec<T>, String> {
        let segs = self.segments()?;
        let tasks: Vec<(usize, usize)> = segs
            .iter()
            .enumerate()
            .flat_map(|(k, (s, side))| {
                (0..s.row_groups())
                    .filter(|rg| keep(s, side.as_deref(), *rg))
                    .map(move |rg| (k, rg))
                    .collect::<Vec<_>>()
            })
            .collect();
        let parts: Vec<Result<Vec<T>, String>> = tasks
            .par_iter()
            .map(|&(k, rg)| {
                let (s, side) = &segs[k];
                let mut out = Vec::new();
                s.read(rg, want, None, side.as_deref(), &mut |_, b| {
                    f(b, &mut out);
                    Ok(())
                })?;
                Ok(out)
            })
            .collect();
        let mut all = Vec::new();
        for p in parts {
            all.extend(p?);
        }
        self.unsealed_batches(&mut |b| {
            f(b, &mut all);
            Ok(())
        })?;
        Ok(all)
    }

    /// Runs (columns `want`; the rest at their defaults) at these ids, sealed or not, keyed by
    /// id. Ids that are not runs are left out.
    pub fn fetch(&self, ids: &[i64], want: Cols) -> Result<HashMap<i64, RunRec>, String> {
        let last = self.manifest.last_id();
        let mut by_seg: HashMap<usize, Vec<i64>> = HashMap::new();
        let mut loose: Vec<i64> = Vec::new();
        for &id in ids {
            if id <= last {
                if let Some(k) = self.manifest.segment_of(id) {
                    by_seg.entry(k).or_default().push(id);
                }
            } else {
                loose.push(id);
            }
        }
        // (segment, row group, ids in it)
        let mut tasks: Vec<(usize, usize, Vec<i64>)> = Vec::new();
        let mut segs: HashMap<usize, (Arc<Segment>, Option<Arc<Segment>>)> = HashMap::new();
        for (k, mut v) in by_seg {
            v.sort_unstable();
            v.dedup();
            let (s, side) = self.segment(k)?;
            let ranges: Vec<(i64, i64)> = (0..s.row_groups())
                .map(|rg| s.int_range(rg, col::ID).unwrap_or((i64::MAX, i64::MIN)))
                .collect();
            let mut per: HashMap<usize, Vec<i64>> = HashMap::new();
            for id in v {
                let rg = ranges.partition_point(|r| r.1 < id);
                if rg < ranges.len() && ranges[rg].0 <= id {
                    per.entry(rg).or_default().push(id);
                }
            }
            for (rg, v) in per {
                tasks.push((k, rg, v));
            }
            segs.insert(k, (s, side));
        }
        let found: Vec<Result<Vec<RunRec>, String>> = tasks
            .par_iter()
            .map(|(k, rg, v)| {
                let (s, side) = &segs[k];
                // Where each id is: its page of the id column (from the page index), read.
                let pages = s
                    .page_int_range(*rg, col::ID)
                    .unwrap_or_else(|| vec![(0, usize::MAX, i64::MIN, i64::MAX)]);
                let mut pos: Vec<usize> = Vec::new();
                let mut by_page: HashMap<usize, Vec<i64>> = HashMap::new();
                for id in v {
                    let p = pages.partition_point(|x| x.3 < *id);
                    if p < pages.len() && pages[p].2 <= *id {
                        by_page.entry(p).or_default().push(*id);
                    }
                }
                let mut ps: Vec<usize> = by_page.keys().copied().collect();
                ps.sort_unstable();
                for p in ps {
                    let (a, b, _, _) = pages[p];
                    let mut idcol: Vec<i64> = Vec::new();
                    s.read(*rg, 1 << col::ID, Some(&[(a, b)]), None, &mut |_, x| {
                        idcol.extend((0..x.len).map(|i| x.id(i)));
                        Ok(())
                    })?;
                    for id in &by_page[&p] {
                        if let Ok(i) = idcol.binary_search(id) {
                            pos.push(a + i);
                        }
                    }
                }
                pos.sort_unstable();
                let ranges: Vec<(usize, usize)> = pos.iter().map(|&p| (p, p + 1)).collect();
                let mut out = Vec::new();
                if ranges.is_empty() {
                    return Ok(out);
                }
                s.read(
                    *rg,
                    want | (1 << col::ID),
                    Some(&ranges),
                    side.as_deref(),
                    &mut |_, b| {
                        out.extend((0..b.len).map(|i| b.rec(i)));
                        Ok(())
                    },
                )?;
                Ok(out)
            })
            .collect();
        let mut out = HashMap::new();
        for f in found {
            for r in f? {
                out.insert(r.id, r);
            }
        }
        loose.sort_unstable();
        loose.dedup();
        for chunk in loose.chunks(500) {
            let list: Vec<String> = chunk.iter().map(i64::to_string).collect();
            let mut st = self
                .db
                .prepare(&format!(
                    "SELECT {} FROM runs WHERE id IN ({})",
                    store::RUN_COLS,
                    list.join(",")
                ))
                .map_err(e)?;
            let mut rows = st.query([]).map_err(e)?;
            while let Some(r) = rows.next().map_err(e)? {
                let rec = store::rec_at(r, 0).map_err(e)?;
                out.insert(rec.id, rec);
            }
        }
        Ok(out)
    }

    /// The runs at these ids (every column), in that order; ids that are not runs are left out.
    pub fn get(&self, ids: &[i64]) -> Result<Vec<RunRec>, String> {
        let mut m = self.fetch(ids, ALL)?;
        let mut seen = HashSet::new();
        Ok(ids
            .iter()
            .filter(|i| seen.insert(**i))
            .filter_map(|i| m.remove(i))
            .collect())
    }

    /// The runs at these ids as rows, in that order (missing ids left out).
    pub fn rows(&self, ids: &[i64]) -> Result<Vec<RunRow>, String> {
        Ok(self.get(ids)?.iter().map(RunRow::of).collect())
    }

    /// Is there a run with this id?
    pub fn has(&self, id: i64) -> Result<bool, String> {
        Ok(!self.fetch(&[id], 1 << col::ID)?.is_empty())
    }

    /// Runs, of one stage or all.
    pub fn count(&self, stage: Option<i64>) -> Result<u64, String> {
        let Some(st) = stage else {
            let n: i64 = self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM runs WHERE id > ?1",
                    [self.manifest.last_id()],
                    |r| r.get(0),
                )
                .map_err(e)?;
            return Ok(self.manifest.rows() + n as u64);
        };
        let parts = self.scan(
            1 << col::STAGE,
            &|_, _, _| true,
            &|b, out: &mut Vec<u64>| {
                out.push((0..b.len).filter(|&i| b.stage(i) == st).count() as u64);
            },
        )?;
        Ok(parts.iter().sum())
    }

    /// The ids of the runs a filter names, in order, at most `limit`.
    pub fn ids(&self, f: &RunFilter, limit: usize) -> Result<Vec<i64>, String> {
        if let Some(ids) = &f.ids {
            if ids.is_empty() {
                return Ok(vec![]);
            }
            let found = self.fetch(ids, 1 << col::ID)?;
            let mut seen = HashSet::new();
            return Ok(ids
                .iter()
                .copied()
                .filter(|i| found.contains_key(i) && seen.insert(*i))
                .take(limit)
                .collect());
        }
        if let Some(x) = &f.fate
            && !["died", "froze", "cycle", "running"].contains(&x.as_str())
        {
            return Err(format!("a fate is died, froze, cycle, or running, not {x}"));
        }
        if let Some(m) = f.min_score
            && !m.is_finite()
        {
            return Err("min_score must be a number".into());
        }
        match f.reviewed.as_deref() {
            None => {}
            Some(v) if v == "any" || crate::explore::reviews::VERDICTS.contains(&v) => {
                if f.within.is_none() {
                    return Err("reviews are in the library: resolve `reviewed` first".into());
                }
            }
            Some(v) => return Err(format!("reviewed is any, keep, maybe, or drop, not {v}")),
        }
        let order = f.order.as_deref().unwrap_or("score");
        if !["score", "newest", "id"].contains(&order) {
            return Err(format!("order is score, newest, or id, not {order}"));
        }
        if limit == 0 {
            return Ok(vec![]);
        }
        let stage = f.stage.unwrap_or(1) as i64;
        let fam = f.family.map(|x| x.name().as_bytes().to_vec());
        let tiling = f.tiling.as_ref().map(|t| t.as_bytes().to_vec());
        let fate = f.fate.as_ref().map(|t| t.as_bytes().to_vec());
        let min = f.min_score;
        let after = f.after.unwrap_or(i64::MIN);
        let within: Option<Vec<i64>> = f.within.as_ref().map(|w| {
            let mut v = w.clone();
            v.sort_unstable();
            v.dedup();
            v
        });
        let within_set: Option<HashSet<i64>> = within.as_ref().map(|v| v.iter().copied().collect());
        let mut want = cols(&[col::ID, col::STAGE, col::SCORE]);
        if fam.is_some() {
            want |= 1 << col::FAM;
        }
        if tiling.is_some() {
            want |= 1 << col::TILING;
        }
        if fate.is_some() {
            want |= 1 << col::FATE;
        }
        let pass = |b: &Batch, i: usize| -> bool {
            b.stage(i) == stage
                && fam.as_deref().is_none_or(|x| b.bytes(col::FAM, i) == x)
                && tiling
                    .as_deref()
                    .is_none_or(|x| b.bytes(col::TILING, i) == x)
                && fate.as_deref().is_none_or(|x| b.bytes(col::FATE, i) == x)
                && min.is_none_or(|m| b.score(i) >= m)
                && b.id(i) > after
                && within_set.as_ref().is_none_or(|w| w.contains(&b.id(i)))
        };
        let keep = |s: &Segment, side: Option<&Segment>, rg: usize| -> bool {
            if let Some((lo, hi)) = s.int_range(rg, col::STAGE)
                && (stage < lo || stage > hi)
            {
                return false;
            }
            if let Some(m) = min {
                let top = side
                    .and_then(|x| x.float_max(rg, col::SCORE))
                    .or_else(|| s.float_max(rg, col::SCORE));
                if top.is_some_and(|t| (t as f64) < m) {
                    return false;
                }
            }
            if let Some(w) = &within
                && let Some((lo, hi)) = s.int_range(rg, col::ID)
            {
                let k = w.partition_point(|x| *x < lo);
                if k >= w.len() || w[k] > hi {
                    return false;
                }
            }
            if let Some((_, hi)) = s.int_range(rg, col::ID)
                && hi <= after
            {
                return false;
            }
            true
        };
        // The unsealed runs, by SQL (with the score index).
        let mut w = vec![
            format!("id > {}", self.manifest.last_id().max(after)),
            format!("stage = {stage}"),
        ];
        if let Some(x) = f.family {
            w.push(format!("fam = '{}'", x.name()));
        }
        if let Some(t) = &f.tiling {
            w.push(format!("tiling = '{}'", t.replace('\'', "''")));
        }
        if let Some(x) = &f.fate {
            w.push(format!("fate = '{x}'"));
        }
        if let Some(m) = min {
            w.push(format!("score >= {m}"));
        }
        if let Some(v) = &within {
            let list: Vec<String> = v.iter().map(i64::to_string).collect();
            w.push(format!("id IN ({})", list.join(",")));
        }
        let sql_order = match order {
            "score" => "score DESC, id",
            "newest" => "id DESC",
            _ => "id",
        };
        let loose: Vec<(i64, f64)> = {
            let mut st = self
                .db
                .prepare(&format!(
                    "SELECT id, COALESCE(score, 0) FROM runs WHERE {} ORDER BY {sql_order} LIMIT {}",
                    w.join(" AND "),
                    // SQLite's LIMIT is a signed 64-bit integer (`usize::MAX`: every run).
                    limit.min(i64::MAX as usize)
                ))
                .map_err(|x| format!("sql: {x}"))?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(e)?
                .collect::<Result<_, _>>()
                .map_err(e)?
        };
        if self.manifest.segments.is_empty() {
            return Ok(loose.into_iter().map(|x| x.0).collect());
        }
        match order {
            "score" => {
                // Every page of the score column (in the row groups kept), best page first: once
                // `limit` runs are found, a page whose best score is below the last of them has
                // nothing to add, nor has any page after it.
                let segs = self.segments()?;
                let mut pages: Vec<(f32, usize, usize, usize, usize)> = Vec::new();
                for (k, (s, side)) in segs.iter().enumerate() {
                    for rg in 0..s.row_groups() {
                        if !keep(s, side.as_deref(), rg) {
                            continue;
                        }
                        let src = side.as_deref().unwrap_or(s);
                        match src.page_float_max(rg, col::SCORE) {
                            Some(ps) => {
                                pages.extend(ps.into_iter().map(|(a, b, m)| (m, k, rg, a, b)))
                            }
                            None => pages.push((f32::INFINITY, k, rg, 0, usize::MAX)),
                        }
                    }
                }
                pages.sort_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then((a.1, a.2, a.3).cmp(&(b.1, b.2, b.3)))
                });
                let mut best: Vec<(f64, i64)> = loose.iter().map(|(i, s)| (*s, *i)).collect();
                top_n(&mut best, limit);
                let mut at = 0;
                let mut batch = rayon::current_num_threads().max(4);
                while at < pages.len() {
                    if best.len() >= limit && (pages[at].0 as f64) < best[limit - 1].0 {
                        break;
                    }
                    let end = (at + batch).min(pages.len());
                    let got: Vec<Result<Vec<(f64, i64)>, String>> = pages[at..end]
                        .par_iter()
                        .map(|&(_, k, rg, a, b)| {
                            let (s, side) = &segs[k];
                            let mut v: Vec<(f64, i64)> = Vec::new();
                            s.read(rg, want, Some(&[(a, b)]), side.as_deref(), &mut |_, x| {
                                v.extend(
                                    (0..x.len)
                                        .filter(|&i| pass(x, i))
                                        .map(|i| (x.score(i), x.id(i))),
                                );
                                Ok(())
                            })?;
                            top_n(&mut v, limit);
                            Ok(v)
                        })
                        .collect();
                    for g in got {
                        best.extend(g?);
                    }
                    top_n(&mut best, limit);
                    at = end;
                    batch = (batch * 2).min(4096);
                }
                Ok(best.into_iter().map(|x| x.1).collect())
            }
            _ => {
                // In id order (or newest first), a row group at a time until there are enough.
                let newest = order == "newest";
                let mut out: Vec<i64> = Vec::new();
                if newest {
                    out.extend(loose.iter().map(|x| x.0));
                }
                let n = self.manifest.segments.len();
                let ks: Vec<usize> = if newest {
                    (0..n).rev().collect()
                } else {
                    (0..n).collect()
                };
                'segs: for k in ks {
                    if out.len() >= limit {
                        break;
                    }
                    let (s, side) = self.segment(k)?;
                    let rgs: Vec<usize> = if newest {
                        (0..s.row_groups()).rev().collect()
                    } else {
                        (0..s.row_groups()).collect()
                    };
                    for rg in rgs {
                        if !keep(&s, side.as_deref(), rg) {
                            continue;
                        }
                        let mut v: Vec<i64> = Vec::new();
                        s.read(rg, want, None, side.as_deref(), &mut |_, b| {
                            v.extend((0..b.len).filter(|&i| pass(b, i)).map(|i| b.id(i)));
                            Ok(())
                        })?;
                        if newest {
                            v.reverse();
                        }
                        out.extend(v);
                        if out.len() >= limit {
                            break 'segs;
                        }
                    }
                }
                if !newest {
                    out.extend(loose.iter().map(|x| x.0));
                }
                out.truncate(limit);
                Ok(out)
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Rescoring

    /// Recompute every score from the stored features (after [`archive::score`] changes), then
    /// rebuild the archive and the family totals from the screened runs with their new scores.
    /// Sealed segments are not rewritten: each one with a changed score gets a sidecar in
    /// `scores-<n>/`, listed in the manifest as rescore `n`; unsealed runs are updated in place.
    /// Reads every run: run it on a stopped search, and again if it is interrupted. `group_of`
    /// gives a tiling's group; `progress` is told the runs done so far and whether to go on.
    pub fn rescore(
        &mut self,
        group_of: &dyn Fn(&str) -> u8,
        progress: &dyn Fn(u64) -> bool,
    ) -> Result<Rescored, String> {
        self.rescore_with(None, &HashMap::new(), group_of, progress)
    }

    /// [`rescore`](Self::rescore) by a fitted score model where one applies (`None`: the built-in
    /// score), with sign measures for runs stored without them (`signs`, by id: replayed and
    /// cached, [`crate::explore::remeasure`]). The archive's niches take each run's behaviour bin
    /// from its measures again (the stored bins stay as they are).
    pub fn rescore_with(
        &mut self,
        model: Option<&ScoreModel>,
        signs: &HashMap<i64, SignMeasures>,
        group_of: &dyn Fn(&str) -> u8,
        progress: &dyn Fn(u64) -> bool,
    ) -> Result<Rescored, String> {
        self.need_write()?;
        let rescore = |id: i64, ev: &mut Eval| -> f32 {
            if ev.feat.signs.is_none()
                && let Some(s) = signs.get(&id)
            {
                ev.feat.signs = Some(*s);
            }
            ev.bin = archive::behaviour_bin(ev.fate, &ev.feat, ev.cand.gens);
            archive::score_with(model, ev.fate, &ev.feat, ev.cand.gens, ev.cand.fam)
        };
        let mut out = Rescored::default();
        let mut arch = Archive::default();
        let mut fams: HashMap<Fam, FamStat> = HashMap::new();
        let stopped =
            || Err::<Rescored, String>("rescoring was stopped; nothing was changed".into());
        let mut offer = |id: i64, stage: i64, ev: Eval, out: &mut Rescored| -> bool {
            out.runs += 1;
            if out.runs.is_multiple_of(100_000) && !progress(out.runs) {
                return false;
            }
            if stage == 1 {
                fams.entry(ev.cand.fam).or_default().add(ev.score as f64);
                let g = group_of(&ev.cand.tiling);
                arch.offer(id, ev, g);
            }
            true
        };
        // Sealed: each segment's new scores, in its row order.
        let want = seg::EVAL | cols(&[col::ID, col::STAGE]);
        let mut fresh: Vec<Option<Vec<RunRec>>> = Vec::new();
        for k in 0..self.manifest.segments.len() {
            let (s, side) = self.segment(k)?;
            let parts: Vec<Result<Vec<RunRec>, String>> = (0..s.row_groups())
                .into_par_iter()
                .map(|rg| {
                    let mut v = Vec::new();
                    s.read(rg, want, None, side.as_deref(), &mut |_, b| {
                        v.extend((0..b.len).map(|i| b.rec(i)));
                        Ok(())
                    })?;
                    Ok(v)
                })
                .collect();
            let mut recs: Vec<RunRec> = Vec::with_capacity(s.rows() as usize);
            for p in parts {
                recs.extend(p?);
            }
            let mut changed = false;
            for r in recs.iter_mut() {
                let Some(mut ev) = store::eval_of(r) else {
                    continue;
                };
                let new = rescore(r.id, &mut ev);
                if new != ev.score {
                    changed = true;
                    out.changed += 1;
                    r.score = new as f64;
                }
                ev.score = new;
                if !offer(r.id, r.stage, ev, &mut out) {
                    return stopped();
                }
            }
            fresh.push(changed.then_some(recs));
        }
        // Unsealed: in place.
        let mut loose: Vec<(i64, f64)> = Vec::new();
        {
            let mut st = self
                .db
                .prepare(&format!(
                    "SELECT {} FROM runs WHERE id > ?1 ORDER BY id",
                    store::RUN_COLS
                ))
                .map_err(e)?;
            let mut rows = st.query([self.manifest.last_id()]).map_err(e)?;
            while let Some(r) = rows.next().map_err(e)? {
                let rec = store::rec_at(r, 0).map_err(e)?;
                let Some(mut ev) = store::eval_of(&rec) else {
                    continue;
                };
                let new = rescore(rec.id, &mut ev);
                if new != ev.score {
                    loose.push((rec.id, new as f64));
                    out.changed += 1;
                }
                ev.score = new;
                if !offer(rec.id, rec.stage, ev, &mut out) {
                    return stopped();
                }
            }
        }
        // The sidecars, before anything else changes.
        let version = self
            .manifest
            .scores
            .iter()
            .map(|s| s.version)
            .max()
            .unwrap_or(0)
            + 1;
        let mut files = Vec::new();
        for (k, recs) in fresh.iter().enumerate() {
            let Some(recs) = recs else { continue };
            let entry = &self.manifest.segments[k];
            let (s, _) = self.segment(k)?;
            let base = Path::new(&entry.file)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = format!("scores-{version}/{base}");
            let fin = self.dir.join(&rel);
            let tmp = self.dir.join(format!("{rel}.tmp"));
            let kv = vec![
                ("jetcarp.search".to_string(), self.uuid()),
                ("jetcarp.segment".to_string(), entry.file.clone()),
                ("jetcarp.scores_version".to_string(), version.to_string()),
                (
                    "jetcarp.engine_version".to_string(),
                    jetcarp_core::VERSION.to_string(),
                ),
            ];
            let mut w = SegmentWriter::create_scores(&tmp, &kv, s.group_rows())?;
            for r in recs {
                w.push(RunRec {
                    id: r.id,
                    score: r.score,
                    ..Default::default()
                })?;
            }
            let written = w.finish()?;
            if written.rows != entry.rows {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!(
                    "{rel}: {} scores for {} runs",
                    written.rows, entry.rows
                ));
            }
            let sha256 = seg::file_sha256(&tmp)?;
            let bytes = std::fs::metadata(&tmp).map_err(|x| io(&tmp, x))?.len();
            std::fs::rename(&tmp, &fin).map_err(|x| io(&fin, x))?;
            if let Some(d) = fin.parent() {
                sync_dir(d)?;
            }
            files.push(ScoreFile {
                segment: entry.file.clone(),
                file: rel,
                sha256,
                bytes,
                rows: written.rows,
            });
        }
        // The working database: unsealed scores, the archive, the family totals.
        let tx = self.db.transaction().map_err(e)?;
        {
            let mut st = tx
                .prepare("UPDATE runs SET score = ?1 WHERE id = ?2")
                .map_err(e)?;
            for (id, s) in &loose {
                st.execute(params![s, id]).map_err(e)?;
            }
            out.elites = put_archive(&tx, &arch)?;
            tx.execute("DELETE FROM families", []).map_err(e)?;
            let mut st = tx
                .prepare("INSERT INTO families(fam, runs, score_sum, best) VALUES(?1, ?2, ?3, ?4)")
                .map_err(e)?;
            for (f, s) in &fams {
                st.execute(params![f.name(), s.runs as i64, s.score_sum, s.best])
                    .map_err(e)?;
            }
        }
        tx.commit().map_err(e)?;
        out.niches = arch.cells.len() as u64;
        if !files.is_empty() {
            out.sidecars = files.len() as u64;
            out.version = version;
            self.manifest.scores.push(ScoreSet {
                version,
                created: jetcarp_core::pattern::now_utc(),
                engine_version: jetcarp_core::VERSION.into(),
                files,
            });
            self.manifest.write(&self.dir)?;
            self.files.lock().unwrap().clear();
        }
        Ok(out)
    }
}

type Files = Mutex<HashMap<String, Arc<Segment>>>;

fn open_file(files: &Files, dir: &Path, rel: &str) -> Result<Arc<Segment>, String> {
    if let Some(s) = files.lock().unwrap().get(rel) {
        return Ok(s.clone());
    }
    let s = Arc::new(Segment::open(&dir.join(rel))?);
    files.lock().unwrap().insert(rel.to_string(), s.clone());
    Ok(s)
}

fn open_segment(
    files: &Files,
    dir: &Path,
    m: &Manifest,
    k: usize,
) -> Result<(Arc<Segment>, Option<Arc<Segment>>), String> {
    let s = open_file(files, dir, &m.segments[k].file)?;
    let side = match m.scores_of(k) {
        Some(f) => Some(open_file(files, dir, &f.file)?),
        None => None,
    };
    Ok((s, side))
}

/// Keep the `n` best of (score, id): score high first, then id low first.
pub fn top_n(v: &mut Vec<(f64, i64)>, n: usize) {
    let cmp = |a: &(f64, i64), b: &(f64, i64)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1));
    if v.len() > n {
        v.select_nth_unstable_by(n, cmp);
        v.truncate(n);
    }
    v.sort_by(cmp);
}
