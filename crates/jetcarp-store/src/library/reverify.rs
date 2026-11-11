//! Re-verification: the records to check again, and storing what checking found (the
//! `reverify` job of `jetcarp_search::jobs`, `jetcarp library reverify`).
//!
//! A record flagged `needs_reverify` (its geometry pin is not the running engine's) is checked
//! again by the job: a pattern verified again on the current geometry, an ant run again with its
//! limits and compared, a Quick Start scene built and, when it records a state hash, run to it.
//! [`reverified_in`] stores each result in the job's step transaction:
//!
//! - **Passed.** The record gets the engine's pin and version, its flag is cleared, and its
//!   provenance gains a `reverified` row with the new pin. A pattern or ant whose content id
//!   changed with the pin (a real geometry change) is moved to its new id: its rules,
//!   sightings, unstored rows, Quick Start entries, media and ratings rows, history and aliases
//!   follow it, guns that emit it name the new id, and the old id becomes an alias
//!   (`geometry-change`). If a record with the new id is already stored, the two are merged (seen
//!   counts and sightings added, the stored record's curation kept).
//! - **Failed.** The record stays (flagged if it was not), and the job reports it. Only with the
//!   job's `drop` option is a failed record deleted.

use super::{
    Library, PatternRecord, QuickStartRow, ants::put_ant, e, load, now_utc, provenance,
    put_pattern, put_pattern_rule, rule_id_in,
};
use crate::AntRow;
use rusqlite::{Transaction, params};

/// A record to check again.
#[derive(Clone, Debug)]
pub enum ReverifyTarget {
    Pattern(Box<PatternRecord>),
    Ant(Box<AntRow>),
    Quickstart(Box<QuickStartRow>),
}

impl ReverifyTarget {
    /// `pattern`, `ant`, or `quickstart`.
    pub fn kind(&self) -> &'static str {
        match self {
            ReverifyTarget::Pattern(_) => "pattern",
            ReverifyTarget::Ant(_) => "ant",
            ReverifyTarget::Quickstart(_) => "quickstart",
        }
    }
    pub fn id(&self) -> &str {
        match self {
            ReverifyTarget::Pattern(r) => &r.id,
            ReverifyTarget::Ant(a) => &a.id,
            ReverifyTarget::Quickstart(q) => &q.id,
        }
    }
}

/// What checking a record again found.
#[derive(Clone, Debug)]
pub struct Reverified {
    /// The record as it was checked (its old id, flag and pin).
    pub target: ReverifyTarget,
    /// Passed: the record as verified now (pin, version, and perhaps id updated); failed: why.
    pub result: Result<ReverifyTarget, String>,
    /// Delete the record if it failed.
    pub drop: bool,
}

/// What [`reverified_in`] did with a batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReverifyCounts {
    pub passed: u64,
    pub failed: u64,
    /// Passed with a new id (moved, the old id an alias).
    pub rekeyed: u64,
    pub removed: u64,
}

/// Store a batch of re-verification results (inside a transaction; see the module notes).
pub(super) fn reverified_in(
    tx: &Transaction,
    job: i64,
    results: &[Reverified],
) -> Result<ReverifyCounts, String> {
    let mut n = ReverifyCounts::default();
    for r in results {
        let kind = r.target.kind();
        let old = r.target.id().to_string();
        match &r.result {
            Ok(now) => {
                n.passed += 1;
                let new = now.id().to_string();
                if new != old {
                    n.rekeyed += 1;
                }
                let pin = match now {
                    ReverifyTarget::Pattern(rec) => {
                        pattern_passed(tx, &old, rec)?;
                        rec.geometry_pin.clone()
                    }
                    ReverifyTarget::Ant(a) => {
                        ant_passed(tx, job, &old, a)?;
                        a.geometry_pin.clone()
                    }
                    ReverifyTarget::Quickstart(q) => quickstart_passed(tx, q)?,
                };
                provenance::append_in(tx, kind, &new, "reverified", job, &pin, "reverify")?;
            }
            Err(_) if r.drop => {
                n.failed += 1;
                n.removed += 1;
                delete_in(tx, kind, &old)?;
            }
            Err(_) => {
                n.failed += 1;
                let table = match kind {
                    "pattern" => "patterns",
                    "ant" => "ants",
                    _ => "quickstart",
                };
                tx.execute(
                    &format!("UPDATE {table} SET needs_reverify = 1 WHERE id = ?1"),
                    [&old],
                )
                .map_err(e)?;
            }
        }
    }
    Ok(n)
}

/// A pattern passed: store it as verified now, under its (perhaps new) id.
fn pattern_passed(tx: &Transaction, old: &str, rec: &PatternRecord) -> Result<(), String> {
    let now = now_utc();
    let rid = rule_id_in(tx, &rec.rule, rec.neighborhood, rec.steps_per_gen)?;
    if rec.id == old {
        put_pattern(tx, rec)?;
        put_pattern_rule(tx, &rec.id, rid, &rec.verified, &now)?;
        return Ok(());
    }
    let new = rec.id.as_str();
    match load(tx, new)? {
        None => {
            put_pattern(tx, rec)?;
            for sql in [
                "UPDATE pattern_rules SET pattern_id = ?2 WHERE pattern_id = ?1",
                "UPDATE sightings SET pattern_id = ?2 WHERE pattern_id = ?1",
                "UPDATE unstored SET pattern_id = ?2 WHERE pattern_id = ?1",
            ] {
                tx.execute(sql, params![old, new]).map_err(e)?;
            }
        }
        Some(mut have) => {
            // Two records became one: add up what was seen; the stored one's curation stays.
            have.seen += rec.seen;
            if have.name.is_none() {
                have.name = rec.name.clone();
            }
            if have.rating.is_none() {
                have.rating = rec.rating;
            }
            for t in &rec.tags {
                if !have.tags.contains(t) {
                    have.tags.push(t.clone());
                }
            }
            if have.notes.is_empty() {
                have.notes = rec.notes.clone();
            }
            have.updated = now.clone();
            have.rescore();
            put_pattern(tx, &have)?;
            tx.execute(
                "INSERT INTO pattern_rules (pattern_id, rule_id, verified, periods,
                    engine_version, first_seen)
                 SELECT ?2, rule_id, verified, periods, engine_version, first_seen
                 FROM pattern_rules WHERE pattern_id = ?1 AND true
                 ON CONFLICT (pattern_id, rule_id) DO NOTHING",
                params![old, new],
            )
            .map_err(e)?;
            tx.execute(
                "INSERT INTO sightings (pattern_id, rule_id, job_id, count, soups, first_seen,
                    last_seen)
                 SELECT ?2, rule_id, job_id, count, soups, first_seen, last_seen
                 FROM sightings WHERE pattern_id = ?1 AND true
                 ON CONFLICT (pattern_id, rule_id, job_id) DO UPDATE SET
                    count = count + excluded.count, soups = soups + excluded.soups,
                    first_seen = min(first_seen, excluded.first_seen),
                    last_seen = max(last_seen, excluded.last_seen)",
                params![old, new],
            )
            .map_err(e)?;
            tx.execute(
                "INSERT INTO unstored (job_id, rule_id, pattern_id, class, period, cells, count,
                    soups, first_soup, rng_state, generation)
                 SELECT job_id, rule_id, ?2, class, period, cells, count, soups, first_soup,
                    rng_state, generation FROM unstored WHERE pattern_id = ?1 AND true
                 ON CONFLICT (job_id, rule_id, pattern_id) DO UPDATE SET
                    count = count + excluded.count, soups = soups + excluded.soups",
                params![old, new],
            )
            .map_err(e)?;
            for sql in [
                "DELETE FROM unstored WHERE pattern_id = ?1",
                // A Quick Start entry the stored record already has wins.
                "DELETE FROM quickstart WHERE source_kind = 'pattern' AND source_id = ?1
                 AND EXISTS (SELECT 1 FROM quickstart WHERE id = ?2)",
            ] {
                tx.execute(sql, params![old, new]).map_err(e)?;
            }
        }
    }
    put_pattern_rule(tx, new, rid, &rec.verified, &now)?;
    for sql in [
        "UPDATE quickstart SET id = ?2, source_id = ?2 WHERE id = ?1 AND source_kind = 'pattern'",
        "UPDATE quickstart SET source_id = ?2 WHERE source_kind = 'pattern' AND source_id = ?1",
        "UPDATE media SET target_id = ?2 WHERE target_kind = 'pattern' AND target_id = ?1",
        "UPDATE ratings SET target_id = ?2 WHERE target_kind = 'pattern' AND target_id = ?1",
        "UPDATE patterns SET emits = ?2 WHERE emits = ?1",
        "UPDATE provenance SET target_id = ?2 WHERE target_kind = 'quickstart' AND target_id = ?1",
    ] {
        tx.execute(sql, params![old, new]).map_err(e)?;
    }
    tx.execute("DELETE FROM patterns WHERE id = ?1", [old])
        .map_err(e)?;
    provenance::rekey_in(tx, "pattern", old, new, "geometry-change")
}

/// An ant passed: its run stored again, under its (perhaps new) id.
fn ant_passed(tx: &Transaction, job: i64, old: &str, a: &AntRow) -> Result<(), String> {
    let now = now_utc();
    if a.id == old {
        tx.execute(
            "UPDATE ants SET needs_reverify = 0, geometry_pin = ?2, engine_version = ?3,
                updated = ?4 WHERE id = ?1",
            params![a.id, a.geometry_pin, a.engine_version, now],
        )
        .map_err(e)?;
        return Ok(());
    }
    let have: bool = tx
        .prepare_cached("SELECT 1 FROM ants WHERE id = ?1")
        .map_err(e)?
        .exists([&a.id])
        .map_err(e)?;
    if !have {
        // The new row's history is the old one's, moved below; its row 0 is not written anew.
        put_ant(tx, a, a.job.unwrap_or(job), &now)?;
        tx.execute(
            "DELETE FROM provenance WHERE target_kind = 'ant' AND target_id = ?1",
            [&a.id],
        )
        .map_err(e)?;
        tx.execute(
            "UPDATE ants SET name = (SELECT name FROM ants WHERE id = ?1),
                notes = (SELECT notes FROM ants WHERE id = ?1),
                rating = (SELECT rating FROM ants WHERE id = ?1),
                state = (SELECT state FROM ants WHERE id = ?1),
                created = (SELECT created FROM ants WHERE id = ?1)
             WHERE id = ?2",
            params![old, a.id],
        )
        .map_err(e)?;
    }
    for sql in [
        "UPDATE media SET target_id = ?2 WHERE target_kind = 'ant' AND target_id = ?1",
        "UPDATE ratings SET target_id = ?2 WHERE target_kind = 'ant' AND target_id = ?1",
        "UPDATE quickstart SET source_id = ?2 WHERE source_id = ?1",
    ] {
        tx.execute(sql, params![old, a.id]).map_err(e)?;
    }
    tx.execute("DELETE FROM ants WHERE id = ?1", [old])
        .map_err(e)?;
    provenance::rekey_in(tx, "ant", old, &a.id, "geometry-change")
}

/// A Quick Start scene passed: it carries the engine's pin now. Returns the pin.
fn quickstart_passed(tx: &Transaction, q: &QuickStartRow) -> Result<String, String> {
    let (key, pin) = super::schema::pin_for(tx, &q.scene.tiling)?;
    tx.execute(
        "UPDATE quickstart SET needs_reverify = 0, pin_key = ?2, geometry_pin = ?3,
            engine_version = ?4, updated = ?5 WHERE id = ?1",
        params![q.id, key, pin, jetcarp_core::VERSION, now_utc()],
    )
    .map_err(e)?;
    Ok(pin)
}

/// Delete a record that failed (with `drop`), as removing it would.
fn delete_in(tx: &Transaction, kind: &str, id: &str) -> Result<(), String> {
    match kind {
        "pattern" => {
            tx.execute("DELETE FROM patterns WHERE id = ?1", [id])
                .map_err(e)?;
            tx.execute(
                "DELETE FROM quickstart WHERE source_kind = 'pattern' AND source_id = ?1",
                [id],
            )
            .map_err(e)?;
        }
        "ant" => {
            tx.execute("DELETE FROM ants WHERE id = ?1", [id])
                .map_err(e)?;
        }
        _ => {
            tx.execute("DELETE FROM quickstart WHERE id = ?1", [id])
                .map_err(e)?;
        }
    }
    if kind != "quickstart" {
        for t in ["media", "ratings"] {
            tx.execute(
                &format!("DELETE FROM {t} WHERE target_kind = ?1 AND target_id = ?2"),
                params![kind, id],
            )
            .map_err(e)?;
        }
    }
    provenance::forget_in(tx, kind, id)
}

impl Library {
    /// The next records to check again, after `after` by id (ordered by id): flagged ones, or
    /// every one with `all`. `kind` is `pattern`, `ant`, or `quickstart`.
    pub fn reverify_page(
        &self,
        kind: &str,
        all: bool,
        after: &str,
        limit: usize,
    ) -> Result<Vec<ReverifyTarget>, String> {
        let flag = if all { "" } else { "AND needs_reverify = 1" };
        match kind {
            "pattern" => {
                let ids: Vec<String> = self.read(vec![], |c| {
                    let mut st = c
                        .prepare(&format!(
                            "SELECT id FROM patterns WHERE id > ?1 {flag} ORDER BY id LIMIT ?2"
                        ))
                        .map_err(e)?;
                    st.query_map(params![after, limit as i64], |r| r.get(0))
                        .map_err(e)?
                        .collect::<Result<_, _>>()
                        .map_err(e)
                })?;
                ids.iter()
                    .map(|id| {
                        self.read(None, |c| load(c, id))?
                            .map(|r| ReverifyTarget::Pattern(Box::new(r)))
                            .ok_or_else(|| format!("no pattern {id}"))
                    })
                    .collect()
            }
            "ant" => {
                let ids: Vec<String> = self.read(vec![], |c| {
                    let mut st = c
                        .prepare(&format!(
                            "SELECT id FROM ants WHERE id > ?1 {flag} ORDER BY id LIMIT ?2"
                        ))
                        .map_err(e)?;
                    st.query_map(params![after, limit as i64], |r| r.get(0))
                        .map_err(e)?
                        .collect::<Result<_, _>>()
                        .map_err(e)
                })?;
                ids.iter()
                    .map(|id| Ok(ReverifyTarget::Ant(Box::new(self.ant(id)?))))
                    .collect()
            }
            "quickstart" => {
                let ids: Vec<String> = self.read(vec![], |c| {
                    let mut st = c
                        .prepare(&format!(
                            "SELECT id FROM quickstart WHERE id > ?1 {flag} ORDER BY id LIMIT ?2"
                        ))
                        .map_err(e)?;
                    st.query_map(params![after, limit as i64], |r| r.get(0))
                        .map_err(e)?
                        .collect::<Result<_, _>>()
                        .map_err(e)
                })?;
                // Rows whose scene no longer reads are skipped by the reader; report them as
                // failures by their id.
                Ok(ids
                    .iter()
                    .filter_map(|id| self.get_quickstart(id).ok())
                    .map(|q| ReverifyTarget::Quickstart(Box::new(q)))
                    .collect())
            }
            k => Err(format!("re-verify patterns, ants, or quickstart, not {k}")),
        }
    }

    /// How many records are flagged for re-verification, per kind (`pattern`, `ant`,
    /// `quickstart`).
    pub fn flagged(&self) -> Result<Vec<(String, u64)>, String> {
        self.read(vec![], |c| {
            let mut out = vec![];
            for (k, t) in super::schema::PINNED {
                let n: i64 = c
                    .query_row(
                        &format!("SELECT count(*) FROM {t} WHERE needs_reverify = 1"),
                        [],
                        |r| r.get(0),
                    )
                    .map_err(e)?;
                out.push((k.to_string(), n as u64));
            }
            Ok(out)
        })
    }

    /// Store re-verification results outside a job (job 0) in one transaction.
    pub fn reverified(&self, results: &[Reverified]) -> Result<ReverifyCounts, String> {
        self.write(|tx| reverified_in(tx, 0, results))
    }
}
