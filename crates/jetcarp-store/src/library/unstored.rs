//! Objects a hunt saw but did not store as full records (the hunt's storage policy,
//! `jetcarp_search::hunt::StorePolicy`), pruning a catalog to that policy, and compact pattern
//! bodies.
//!
//! **Unstored objects.** Boiling rules leave huge numbers of one-off large still lifes and
//! oscillators; a full record each (phases, verification, rule range: about 10 KB of JSON) made a
//! real catalog grow to 9.5 GB. A hunt now keeps them as one `unstored` row per (job, rule,
//! pattern id): the class, period, cells, copies, soups, and the first find (soup, RNG state,
//! generation), about 60 bytes, no phases. When the policy later admits one (it is seen again),
//! it is stored as a full record from its first find and its row is deleted.
//!
//! **Compact bodies.** `patterns.body` is written as `Z1` + the deflated JSON (a BLOB in the
//! TEXT column; SQLite keeps either), and read either way, so older catalogs need no rewrite;
//! [`Library::compact_bodies`] rewrites old text bodies in place.

use super::{Library, e};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use jetcarp_core::pattern::PatternClass;
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Version 6: hunts' unstored objects.
pub(super) const MIGRATION_UNSTORED: &str = r#"
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
"#;

/// One unstored object of a job in a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnstoredRow {
    pub rule_id: i64,
    pub pattern_id: String,
    pub class: PatternClass,
    pub period: u64,
    pub cells: u64,
    pub count: u64,
    pub soups: u64,
    pub first_soup: u64,
    pub rng_state: u64,
    pub generation: u64,
}

/// What a hunt flush does to its unstored rows: add copies (insert or add to the counts; the
/// first find is kept) and delete the rows of objects now stored.
#[derive(Clone, Debug, Default)]
pub struct UnstoredFlush {
    pub add: Vec<UnstoredRow>,
    /// (rule id, pattern id)
    pub remove: Vec<(i64, String)>,
}

pub(super) fn flush_in(tx: &Transaction, job: i64, u: &UnstoredFlush) -> Result<(), String> {
    let mut add = tx
        .prepare_cached(
            "INSERT INTO unstored (job_id, rule_id, pattern_id, class, period, cells, count,
                soups, first_soup, rng_state, generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (job_id, rule_id, pattern_id) DO UPDATE SET
                count = count + excluded.count, soups = soups + excluded.soups",
        )
        .map_err(e)?;
    for r in &u.add {
        add.execute(params![
            job,
            r.rule_id,
            r.pattern_id,
            r.class.name(),
            r.period as i64,
            r.cells as i64,
            r.count as i64,
            r.soups as i64,
            r.first_soup as i64,
            r.rng_state as i64,
            r.generation as i64,
        ])
        .map_err(e)?;
    }
    let mut rm = tx
        .prepare_cached(
            "DELETE FROM unstored WHERE job_id = ?1 AND rule_id = ?2 AND pattern_id = ?3",
        )
        .map_err(e)?;
    for (rule, id) in &u.remove {
        rm.execute(params![job, rule, id]).map_err(e)?;
    }
    Ok(())
}

/// A body as stored: `Z1` + deflate of the JSON.
pub(super) fn pack_body(json: &str) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(
        Vec::with_capacity(json.len() / 4 + 16),
        Compression::default(),
    );
    let _ = enc.write_all(json.as_bytes());
    let mut out = b"Z1".to_vec();
    out.extend(enc.finish().unwrap_or_default());
    out
}

/// A body read back: JSON text, or `Z1` + deflate.
pub(super) fn unpack_body(v: rusqlite::types::ValueRef) -> Result<String, String> {
    match v {
        rusqlite::types::ValueRef::Text(t) => {
            String::from_utf8(t.to_vec()).map_err(|x| x.to_string())
        }
        rusqlite::types::ValueRef::Blob(b) if b.starts_with(b"Z1") => {
            let mut s = String::new();
            DeflateDecoder::new(&b[2..])
                .read_to_string(&mut s)
                .map_err(|x| format!("pattern body: {x}"))?;
            Ok(s)
        }
        _ => Err("pattern body is neither JSON nor a compressed body".into()),
    }
}

/// What [`Library::prune_patterns`] did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PruneReport {
    /// Patterns checked against the policy (still lifes and oscillators).
    pub checked: u64,
    /// Patterns deleted (and moved to `unstored` rows of the jobs that saw them).
    pub deleted: u64,
    /// Deleted by class.
    pub by_class: std::collections::BTreeMap<String, u64>,
    pub kept: u64,
    pub unstored_rows: u64,
    pub media_rows: u64,
}

impl Library {
    /// A job's unstored objects in one rule.
    pub fn unstored(&self, job: i64, rule_id: i64) -> Result<Vec<UnstoredRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT rule_id, pattern_id, class, period, cells, count, soups, first_soup,
                        rng_state, generation FROM unstored WHERE job_id = ?1 AND rule_id = ?2",
                )
                .map_err(e)?;
            let rows = st
                .query_map(params![job, rule_id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        [
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, i64>(8)?,
                            r.get::<_, i64>(9)?,
                        ],
                    ))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            rows.into_iter()
                .map(|(rule_id, pattern_id, class, n)| {
                    Ok(UnstoredRow {
                        rule_id,
                        pattern_id,
                        class: PatternClass::parse(&class)?,
                        period: n[0] as u64,
                        cells: n[1] as u64,
                        count: n[2] as u64,
                        soups: n[3] as u64,
                        first_soup: n[4] as u64,
                        rng_state: n[5] as u64,
                        generation: n[6] as u64,
                    })
                })
                .collect()
        })
    }

    /// Unstored objects of a job by class: (distinct, copies).
    pub fn unstored_counts(
        &self,
        job: i64,
    ) -> Result<std::collections::BTreeMap<String, (u64, u64)>, String> {
        self.read(Default::default(), |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT class, count(*), sum(count) FROM unstored WHERE job_id = ?1
                     GROUP BY class",
                )
                .map_err(e)?;
            st.query_map([job], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64),
                ))
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)
        })
    }

    /// Delete the still lifes and oscillators a hunt would no longer store (more than
    /// `max_cells` cells or `period x cells` above `max_volume`, unless some job saw them at
    /// least `min_seen` times), with their rules, sightings, and media rows. Each deleted
    /// pattern becomes `unstored` rows of the jobs that saw it, so their counts survive. Curated
    /// patterns (named, rated, tagged, or not `new`) are kept. Run [`Library::vacuum`] after.
    pub fn prune_patterns(
        &self,
        max_cells: u64,
        max_volume: u64,
        min_seen: u64,
    ) -> Result<PruneReport, String> {
        self.write(|tx| {
            let mut rep = PruneReport::default();
            // Candidates by their columns (no body read).
            let doomed: Vec<(String, String, i64, i64)> = {
                let mut st = tx
                    .prepare(
                        "SELECT p.id, p.class, p.period, p.cells FROM patterns p
                         WHERE p.class IN ('still_life', 'oscillator')
                           AND (p.cells > ?1 OR p.period * p.cells > ?2)
                           AND p.state = 'new' AND p.name IS NULL AND p.rating IS NULL
                           AND p.tags = '[]'
                           AND NOT EXISTS (SELECT 1 FROM sightings s WHERE s.pattern_id = p.id
                                           AND s.count >= ?3)",
                    )
                    .map_err(e)?;
                st.query_map(
                    params![max_cells as i64, max_volume as i64, min_seen as i64],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(e)?
                .collect::<Result<_, _>>()
                .map_err(e)?
            };
            rep.checked = tx
                .query_row(
                    "SELECT count(*) FROM patterns WHERE class IN ('still_life', 'oscillator')",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(e)? as u64;
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS prune_ids (id TEXT PRIMARY KEY) WITHOUT ROWID;
                 DELETE FROM prune_ids;",
            )
            .map_err(e)?;
            {
                let mut ins = tx
                    .prepare("INSERT OR IGNORE INTO prune_ids (id) VALUES (?1)")
                    .map_err(e)?;
                for (id, class, _, _) in &doomed {
                    ins.execute([id]).map_err(e)?;
                    *rep.by_class.entry(class.clone()).or_default() += 1;
                }
            }
            // Sightings become unstored rows (first find: the job's first sighting is not
            // known in detail, so soup 0).
            rep.unstored_rows = tx
                .execute(
                    "INSERT INTO unstored (job_id, rule_id, pattern_id, class, period, cells,
                        count, soups, first_soup, rng_state, generation)
                     SELECT s.job_id, s.rule_id, s.pattern_id, p.class, p.period, p.cells,
                        s.count, s.soups, 0, 0, 0
                     FROM sightings s JOIN patterns p ON p.id = s.pattern_id
                     WHERE s.pattern_id IN (SELECT id FROM prune_ids) AND s.job_id != 0
                     ON CONFLICT (job_id, rule_id, pattern_id) DO UPDATE SET
                        count = count + excluded.count, soups = soups + excluded.soups",
                    [],
                )
                .map_err(e)? as u64;
            rep.media_rows = tx
                .execute(
                    "DELETE FROM media WHERE target_kind = 'pattern'
                     AND target_id IN (SELECT id FROM prune_ids)",
                    [],
                )
                .map_err(e)? as u64;
            tx.execute(
                "DELETE FROM sightings WHERE pattern_id IN (SELECT id FROM prune_ids)",
                [],
            )
            .map_err(e)?;
            tx.execute(
                "DELETE FROM pattern_rules WHERE pattern_id IN (SELECT id FROM prune_ids)",
                [],
            )
            .map_err(e)?;
            rep.deleted = tx
                .execute(
                    "DELETE FROM patterns WHERE id IN (SELECT id FROM prune_ids)",
                    [],
                )
                .map_err(e)? as u64;
            tx.execute("DELETE FROM prune_ids", []).map_err(e)?;
            rep.kept = rep.checked - rep.deleted;
            Ok(rep)
        })
    }

    /// Rewrite text bodies as compressed bodies, in batches. Returns (rows rewritten, bytes
    /// before, bytes after).
    pub fn compact_bodies(&self) -> Result<(u64, u64, u64), String> {
        let (mut n, mut before, mut after) = (0u64, 0u64, 0u64);
        loop {
            let done = self.write(|tx| {
                let rows: Vec<(String, String)> = {
                    let mut st = tx
                        .prepare(
                            "SELECT id, body FROM patterns WHERE typeof(body) = 'text' LIMIT 20000",
                        )
                        .map_err(e)?;
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                        .map_err(e)?
                        .collect::<Result<_, _>>()
                        .map_err(e)?
                };
                let mut up = tx
                    .prepare_cached("UPDATE patterns SET body = ?2 WHERE id = ?1")
                    .map_err(e)?;
                for (id, body) in &rows {
                    let z = pack_body(body);
                    before += body.len() as u64;
                    after += z.len() as u64;
                    up.execute(params![id, z]).map_err(e)?;
                }
                n += rows.len() as u64;
                Ok(rows.is_empty())
            })?;
            if done {
                return Ok((n, before, after));
            }
        }
    }

    /// A consistent copy of the catalog in another file (`VACUUM INTO`; the file must not
    /// exist).
    pub fn backup_to(&self, path: &std::path::Path) -> Result<(), String> {
        if path.exists() {
            return Err(format!("{} exists already", path.display()));
        }
        let p = path.display().to_string();
        self.read((), |c| {
            c.execute("VACUUM INTO ?1", [p]).map(|_| ()).map_err(e)
        })
    }

    /// `VACUUM` the catalog (after a prune), then checkpoint the WAL.
    pub fn vacuum(&self) -> Result<(), String> {
        self.read((), |c| {
            c.execute_batch(
                "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .map_err(e)
        })
    }
}
