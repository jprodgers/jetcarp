//! Compounds: stored records that are several independent objects, split into their parts (the
//! `compounds` job of `jetcarp_search::jobs`, `jetcarp library compounds --apply`).
//!
//! The job finds the compounds ([`jetcarp_core::pattern::split`]) and prepares their parts;
//! [`split_in`] stores what it found, in the job's step transaction, for each compound:
//!
//! - **Parts.** Each part is stored, or merged into the stored pattern with the same id. A new
//!   part takes the compound's history as its own (its provenance rows, copied: it was found
//!   when and by whom the compound was), then every part gains a `reverified` row whose source,
//!   `compound-split: ...`, names the compound and how many copies of the part it held.
//! - **Counts.** A compound holding `n` copies of a part was `n` sightings of it: each of the
//!   compound's sightings (rule, job, copies, soups, dates) is credited to the part `n` times
//!   over, and so is the rest of its seen count (a record imported with its seen count but no
//!   sightings), in every rule the part runs in. The part gains the compound's rules where it
//!   runs (inside its rule range).
//! - **The compound** is deleted, with its sightings, rules, media and history (the history
//!   lives on in its parts), unless `remove` is false: then it stays, tagged `compound`, with a
//!   note naming its parts, and its history gains a `reverified` row saying so. The job keeps
//!   compounds that someone curated (a name, a rating, notes, tags, a curation state, a Quick
//!   Start entry or a rating row) and those whose parts do not run in every rule the compound
//!   runs in (in such a rule the parts may be one object after all).

use super::{
    Library, Origin, PatternRecord, add_in, e, load, now_utc, provenance, put_pattern,
    put_pattern_rule,
};
use rusqlite::{Transaction, params};

/// One compound found, ready to store (see the module notes).
#[derive(Clone, Debug)]
pub struct CompoundSplit {
    /// The compound's id.
    pub compound: String,
    /// Its parts, each ready to store (canonical form, verified in the compound's rule, with its
    /// rule range), with the copies of it the compound holds.
    pub parts: Vec<(PatternRecord, u64)>,
    /// Delete the compound (else keep it, tagged `compound`, with `note` added to its notes).
    pub remove: bool,
    /// What the compound is made of, for its notes and its parts' history.
    pub note: String,
}

/// What [`split_in`] did with a batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompoundCounts {
    pub compounds: u64,
    pub removed: u64,
    pub kept: u64,
    pub parts_added: u64,
    pub parts_merged: u64,
}

/// The tag a kept compound gets.
pub const COMPOUND_TAG: &str = "compound";

/// Store a batch of split compounds (inside a transaction; see the module notes).
pub(super) fn split_in(
    tx: &Transaction,
    job: i64,
    splits: &[CompoundSplit],
) -> Result<CompoundCounts, String> {
    let mut n = CompoundCounts::default();
    let now = now_utc();
    for s in splits {
        let Some(mut comp) = load(tx, &s.compound)? else {
            continue;
        };
        n.compounds += 1;
        let rules: Vec<(i64, String)> = {
            let mut st = tx
                .prepare_cached(
                    "SELECT pr.rule_id, r.notation FROM pattern_rules pr
                     JOIN rules r ON r.id = pr.rule_id WHERE pr.pattern_id = ?1 ORDER BY pr.rule_id",
                )
                .map_err(e)?;
            st.query_map([&s.compound], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(e)?
                .collect::<Result<_, _>>()
                .map_err(e)?
        };
        type Row = (i64, i64, i64, i64, String, String);
        let sightings: Vec<Row> = {
            let mut st = tx
                .prepare_cached(
                    "SELECT rule_id, job_id, count, soups, first_seen, last_seen FROM sightings
                     WHERE pattern_id = ?1 ORDER BY rule_id, job_id",
                )
                .map_err(e)?;
            st.query_map([&s.compound], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?
        };
        let sighted: u64 = sightings.iter().map(|x| x.2.max(0) as u64).sum();
        let unsighted = comp.seen.saturating_sub(sighted);
        for (part, copies) in &s.parts {
            let had = load(tx, &part.id)?.is_some();
            if !had {
                // The part was found when and by whom the compound was: its history first.
                tx.execute(
                    "INSERT OR IGNORE INTO provenance (target_kind, target_id, seq, kind, finder,
                        orcid, search_kind, job_uuid, pack_hash, machine, engine_version,
                        geometry_pin, source, date)
                     SELECT target_kind, ?2, seq, kind, finder, orcid, search_kind, job_uuid,
                        pack_hash, machine, engine_version, ?3, source, date
                     FROM provenance WHERE target_kind = 'pattern' AND target_id = ?1
                     ORDER BY seq",
                    params![s.compound, part.id, part.geometry_pin],
                )
                .map_err(e)?;
            }
            let mut p = part.clone();
            p.seen = 0;
            add_in(tx, p, job, 0, &Origin::found())?;
            if had {
                n.parts_merged += 1;
            } else {
                n.parts_added += 1;
            }
            // The compound's rules and sightings, where the part runs.
            let mut add = unsighted * copies;
            for (rid, notation) in &rules {
                if part.works_in(notation) {
                    put_pattern_rule(tx, &part.id, *rid, &part.verified, &now)?;
                }
            }
            for (rid, sjob, count, soups, first, last) in &sightings {
                let Some((_, notation)) = rules.iter().find(|r| r.0 == *rid) else {
                    continue;
                };
                if !part.works_in(notation) {
                    continue;
                }
                let c = count.max(&0) * *copies as i64;
                add += c as u64;
                tx.prepare_cached(
                    "INSERT INTO sightings (pattern_id, rule_id, job_id, count, soups, first_seen,
                        last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (pattern_id, rule_id, job_id) DO UPDATE SET
                        count = count + excluded.count, soups = soups + excluded.soups,
                        first_seen = min(first_seen, excluded.first_seen),
                        last_seen = max(last_seen, excluded.last_seen)",
                )
                .map_err(e)?
                .execute(params![part.id, rid, sjob, c, soups, first, last])
                .map_err(e)?;
            }
            if let Some(mut r) = load(tx, &part.id)? {
                r.seen += add;
                r.updated = now.clone();
                r.rescore();
                put_pattern(tx, &r)?;
            }
            provenance::append_in(
                tx,
                "pattern",
                &part.id,
                "reverified",
                job,
                &part.geometry_pin,
                &format!(
                    "compound-split: {copies} cop{} in compound {} ({}; seen {}){}",
                    if *copies == 1 { "y" } else { "ies" },
                    s.compound,
                    comp.label(),
                    comp.seen,
                    if s.remove {
                        ", which was removed"
                    } else {
                        ", which was kept, tagged compound"
                    }
                ),
            )?;
        }
        if s.remove {
            n.removed += 1;
            tx.execute("DELETE FROM patterns WHERE id = ?1", [&s.compound])
                .map_err(e)?;
            for sql in [
                "DELETE FROM quickstart WHERE source_kind = 'pattern' AND source_id = ?1",
                "DELETE FROM media WHERE target_kind = 'pattern' AND target_id = ?1",
            ] {
                tx.execute(sql, [&s.compound]).map_err(e)?;
            }
            provenance::forget_in(tx, "pattern", &s.compound)?;
        } else {
            n.kept += 1;
            if !comp.tags.iter().any(|t| t == COMPOUND_TAG) {
                comp.tags.push(COMPOUND_TAG.into());
            }
            if !comp.notes.contains(&s.note) {
                if !comp.notes.is_empty() {
                    comp.notes.push('\n');
                }
                comp.notes.push_str(&s.note);
            }
            comp.updated = now.clone();
            put_pattern(tx, &comp)?;
            provenance::append_in(
                tx,
                "pattern",
                &s.compound,
                "reverified",
                job,
                &comp.geometry_pin,
                &format!("compound-split: kept, tagged compound; {}", s.note),
            )?;
        }
    }
    Ok(n)
}

impl Library {
    /// The next patterns after `after` by id (ordered by id), at most `limit`.
    pub fn pattern_page(&self, after: &str, limit: usize) -> Result<Vec<PatternRecord>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(&format!(
                    "SELECT {} FROM patterns p WHERE p.id > ?1 ORDER BY p.id LIMIT ?2",
                    super::PCOLS
                ))
                .map_err(e)?;
            let v = st
                .query_map(params![after, limit as i64], super::record_from_row)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            v.into_iter().collect()
        })
    }

    /// Store split compounds outside a job (job 0) in one transaction.
    pub fn split_compounds(&self, splits: &[CompoundSplit]) -> Result<CompoundCounts, String> {
        self.write(|tx| split_in(tx, 0, splits))
    }

    /// Has anyone curated this pattern beyond its record (a Quick Start entry, or a rating row)?
    pub fn has_curation_rows(&self, id: &str) -> Result<bool, String> {
        self.read(false, |c| {
            let q: bool = c
                .prepare_cached(
                    "SELECT 1 FROM quickstart WHERE source_kind = 'pattern' AND source_id = ?1",
                )
                .map_err(e)?
                .exists([id])
                .map_err(e)?;
            let r: bool = c
                .prepare_cached(
                    "SELECT 1 FROM ratings WHERE target_kind = 'pattern' AND target_id = ?1",
                )
                .map_err(e)?
                .exists([id])
                .map_err(e)?;
            Ok(q || r)
        })
    }
}
