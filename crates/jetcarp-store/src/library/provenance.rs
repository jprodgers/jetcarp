//! Provenance and id aliases: a record's history, and the ids it had before.
//!
//! **Provenance** (table `provenance`). Row 0 of a record says how it came into this library:
//! `found` (by a search, a script, or a person) or `imported` (from records made elsewhere), who
//! found it (`JETCARP_FINDER`, `JETCARP_ORCID`; empty when unset), which search (the job's kind
//! and uuid), the machine (operating system and CPU architecture only, since provenance travels
//! in packs), the engine version and geometry pin it was verified with, what found it
//! (`source`: `search`, `collide a + b`, a scene path, `import <file>`), and when. It is written
//! once, when the record is first stored, and never rewritten: a trigger refuses any change to
//! it but the record id it belongs to (a record verified again on a new geometry gets a new id,
//! and takes its history with it). Later events append rows 1, 2, ... (`reverified`: checked
//! again by `jetcarp library reverify`, with the new pin).
//!
//! A record **found elsewhere** and imported with its history ([`Origin::found_elsewhere`]) keeps
//! its discovery as row 0: `found`, by the finder the importer names, on the date it was first
//! stored where it was found (its `created`), from its own source, with the engine that found it
//! (`provenance.jetcarp_version`) and the pin it was verified with here; the machine is not
//! known. Row 1 is then `imported`: now, by this engine, from the importer's source.
//!
//! **Aliases** (table `id_aliases`). An old id of a record (`geometry-change`: the id before a
//! geometry change and re-verification), so links and notes that name it keep working: lookups
//! by id or prefix resolve aliases too.

use super::{Library, e, now_utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

/// How a record came in (its provenance row 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Origin {
    /// `found` or `imported`.
    pub kind: &'static str,
    /// What brought it in, when not the record's own `provenance.source` (an import's file).
    pub source: Option<String>,
    /// The pack it came in, when it did (step 10's packs).
    pub pack_hash: String,
    /// Found elsewhere, by this finder (a name and an ORCID iD, either may be empty): see
    /// [`Origin::found_elsewhere`].
    pub found_by: Option<(String, String)>,
}

impl Origin {
    /// Found here, by a search, a script, or a person.
    pub fn found() -> Origin {
        Origin {
            kind: "found",
            source: None,
            pack_hash: String::new(),
            found_by: None,
        }
    }
    /// Imported from records made elsewhere.
    pub fn imported(source: &str) -> Origin {
        Origin {
            kind: "imported",
            source: Some(source.to_string()),
            pack_hash: String::new(),
            found_by: None,
        }
    }
    /// Found elsewhere by `finder` (with ORCID iD `orcid`), and imported from `source`. A record
    /// stored with this origin gets two rows: row 0 `found`, by the finder, on the date it was
    /// first stored where it was found (its `created`; else its `provenance.date`), from its own
    /// source, with the engine that found it (`provenance.jetcarp_version`)
    /// and the pin it was verified with here; row 1 `imported`, now, by this engine, from
    /// `source`. It keeps its seen count, but no sighting is credited for it (the importer adds
    /// the sightings it knows of).
    pub fn found_elsewhere(finder: &str, orcid: &str, source: &str) -> Origin {
        Origin {
            kind: "found",
            source: Some(source.to_string()),
            pack_hash: String::new(),
            found_by: Some((finder.trim().to_string(), orcid.trim().to_string())),
        }
    }
}

/// One row of a record's provenance.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProvenanceRow {
    pub target_kind: String,
    pub target_id: String,
    pub seq: i64,
    /// `found`, `imported`, or `reverified`.
    pub kind: String,
    pub finder: String,
    pub orcid: String,
    pub search_kind: String,
    pub job_uuid: String,
    pub pack_hash: String,
    pub machine: String,
    pub engine_version: String,
    pub geometry_pin: String,
    pub source: String,
    pub date: String,
}

/// One old id of a record.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AliasRow {
    pub alias: String,
    pub target_kind: String,
    pub id: String,
    /// `geometry-change`.
    pub reason: String,
    pub created: String,
}

/// The reasons an alias may give.
pub const ALIAS_REASONS: [&str; 1] = ["geometry-change"];

/// The machine, as provenance records it: operating system and CPU architecture
/// (`linux-x86_64`), never the host name.
pub fn machine() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn env(var: &str) -> String {
    std::env::var(var)
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

/// The search a job is: (its kind, its uuid); nothing for job 0 (adds by hand).
fn job_of(tx: &Transaction, job: i64) -> Result<(String, String), String> {
    if job == 0 {
        return Ok(Default::default());
    }
    Ok(tx
        .prepare_cached("SELECT kind, uuid FROM jobs WHERE id = ?1")
        .map_err(e)?
        .query_row([job], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(e)?
        .unwrap_or_default())
}

/// Write a record's provenance row 0 (nothing if it has one). `found_with` is the engine that
/// found it, which row 0 names only for a record found elsewhere ([`Origin::found_elsewhere`]:
/// then its `imported` row 1 is written too).
#[allow(clippy::too_many_arguments)]
pub(super) fn first_in(
    tx: &Transaction,
    target_kind: &str,
    id: &str,
    origin: &Origin,
    job: i64,
    pin: &str,
    source: &str,
    date: &str,
    found_with: &str,
) -> Result<(), String> {
    let (search_kind, job_uuid) = job_of(tx, job)?;
    let now = now_utc();
    let date = if date.is_empty() || origin.kind == "imported" {
        now.as_str()
    } else {
        date
    };
    if let Some((finder, orcid)) = &origin.found_by {
        let n = tx
            .prepare_cached(
                "INSERT OR IGNORE INTO provenance (target_kind, target_id, seq, kind, finder,
                    orcid, search_kind, job_uuid, pack_hash, machine, engine_version,
                    geometry_pin, source, date)
                 VALUES (?1, ?2, 0, 'found', ?3, ?4, ?5, ?6, ?7, '', ?8, ?9, ?10, ?11)",
            )
            .map_err(e)?
            .execute(params![
                target_kind,
                id,
                finder,
                orcid,
                search_kind,
                job_uuid,
                origin.pack_hash,
                found_with,
                pin,
                source,
                date,
            ])
            .map_err(e)?;
        if n == 1 {
            append_in(
                tx,
                target_kind,
                id,
                "imported",
                0,
                pin,
                origin.source.as_deref().unwrap_or(""),
            )?;
        }
        return Ok(());
    }
    tx.prepare_cached(
        "INSERT OR IGNORE INTO provenance (target_kind, target_id, seq, kind, finder, orcid,
            search_kind, job_uuid, pack_hash, machine, engine_version, geometry_pin, source, date)
         VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )
    .map_err(e)?
    .execute(params![
        target_kind,
        id,
        origin.kind,
        env("JETCARP_FINDER"),
        env("JETCARP_ORCID"),
        search_kind,
        job_uuid,
        origin.pack_hash,
        machine(),
        jetcarp_core::VERSION,
        pin,
        origin.source.as_deref().unwrap_or(source),
        date,
    ])
    .map_err(e)?;
    Ok(())
}

/// Append a row to a record's provenance (`reverified`, ...), as the next `seq`.
pub(super) fn append_in(
    tx: &Transaction,
    target_kind: &str,
    id: &str,
    kind: &str,
    job: i64,
    pin: &str,
    source: &str,
) -> Result<i64, String> {
    let (search_kind, job_uuid) = job_of(tx, job)?;
    let seq: i64 = tx
        .prepare_cached(
            "SELECT coalesce(max(seq) + 1, 0) FROM provenance
             WHERE target_kind = ?1 AND target_id = ?2",
        )
        .map_err(e)?
        .query_row(params![target_kind, id], |r| r.get(0))
        .map_err(e)?;
    tx.prepare_cached(
        "INSERT INTO provenance (target_kind, target_id, seq, kind, finder, orcid, search_kind,
            job_uuid, pack_hash, machine, engine_version, geometry_pin, source, date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', ?9, ?10, ?11, ?12, ?13)",
    )
    .map_err(e)?
    .execute(params![
        target_kind,
        id,
        seq,
        kind,
        env("JETCARP_FINDER"),
        env("JETCARP_ORCID"),
        search_kind,
        job_uuid,
        machine(),
        jetcarp_core::VERSION,
        pin,
        source,
        now_utc(),
    ])
    .map_err(e)?;
    Ok(seq)
}

/// A record's id changed (`old` to `new`): its history and its aliases follow it, and `old`
/// becomes an alias of `new` with this reason. When `new` already has a history (two records
/// became one), `old`'s rows are appended to it in order.
pub(super) fn rekey_in(
    tx: &Transaction,
    target_kind: &str,
    old: &str,
    new: &str,
    reason: &str,
) -> Result<(), String> {
    let has: bool = tx
        .prepare_cached("SELECT 1 FROM provenance WHERE target_kind = ?1 AND target_id = ?2")
        .map_err(e)?
        .exists(params![target_kind, new])
        .map_err(e)?;
    if !has {
        tx.execute(
            "UPDATE provenance SET target_id = ?3 WHERE target_kind = ?1 AND target_id = ?2",
            params![target_kind, old, new],
        )
        .map_err(e)?;
    } else {
        let base: i64 = tx
            .query_row(
                "SELECT max(seq) + 1 FROM provenance WHERE target_kind = ?1 AND target_id = ?2",
                params![target_kind, new],
                |r| r.get(0),
            )
            .map_err(e)?;
        tx.execute(
            "INSERT INTO provenance (target_kind, target_id, seq, kind, finder, orcid,
                search_kind, job_uuid, pack_hash, machine, engine_version, geometry_pin, source,
                date)
             SELECT target_kind, ?3, ?4 + seq, kind, finder, orcid, search_kind, job_uuid,
                pack_hash, machine, engine_version, geometry_pin, source, date
             FROM provenance WHERE target_kind = ?1 AND target_id = ?2 ORDER BY seq",
            params![target_kind, old, new, base],
        )
        .map_err(e)?;
        tx.execute(
            "DELETE FROM provenance WHERE target_kind = ?1 AND target_id = ?2",
            params![target_kind, old],
        )
        .map_err(e)?;
    }
    tx.execute(
        "UPDATE id_aliases SET id = ?3 WHERE target_kind = ?1 AND id = ?2",
        params![target_kind, old, new],
    )
    .map_err(e)?;
    put_alias_in(tx, old, target_kind, new, reason)
}

/// Record an alias (replacing what the alias named before).
pub(super) fn put_alias_in(
    tx: &Transaction,
    alias: &str,
    target_kind: &str,
    id: &str,
    reason: &str,
) -> Result<(), String> {
    if !ALIAS_REASONS.contains(&reason) {
        return Err(format!(
            "an alias reason is {}, not '{reason}'",
            ALIAS_REASONS.join(", ")
        ));
    }
    if alias == id {
        return Ok(());
    }
    tx.prepare_cached(
        "INSERT INTO id_aliases (alias, target_kind, id, reason, created)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (alias) DO UPDATE SET target_kind = excluded.target_kind, id = excluded.id,
            reason = excluded.reason",
    )
    .map_err(e)?
    .execute(params![alias, target_kind, id, reason, now_utc()])
    .map_err(e)?;
    Ok(())
}

/// A record is deleted: its history and its aliases go with it.
pub(super) fn forget_in(tx: &Transaction, target_kind: &str, id: &str) -> Result<(), String> {
    tx.prepare_cached("DELETE FROM provenance WHERE target_kind = ?1 AND target_id = ?2")
        .map_err(e)?
        .execute(params![target_kind, id])
        .map_err(e)?;
    tx.prepare_cached("DELETE FROM id_aliases WHERE target_kind = ?1 AND id = ?2")
        .map_err(e)?
        .execute(params![target_kind, id])
        .map_err(e)?;
    Ok(())
}

/// The ids that aliases name for a lookup key: the alias itself, or (4+ characters, hex, or an
/// `ant-` id) every alias it begins (at most 6).
pub(super) fn alias_ids(
    c: &Connection,
    target_kind: &str,
    key: &str,
) -> Result<Vec<String>, String> {
    let key = key.trim();
    let hexish = |s: &str| s.len() >= 4 && s.chars().all(|c| c.is_ascii_hexdigit());
    let prefix = hexish(key) || key.strip_prefix("ant-").is_some_and(hexish);
    let mut st = c
        .prepare_cached(
            "SELECT DISTINCT id FROM id_aliases WHERE target_kind = ?1
                AND (alias = ?2 OR (?3 AND alias >= ?2 AND alias < ?2 || 'g'))
             ORDER BY id LIMIT 6",
        )
        .map_err(e)?;
    let key = if prefix {
        key.to_ascii_lowercase()
    } else {
        key.to_string()
    };
    st.query_map(params![target_kind, key, prefix], |r| r.get::<_, String>(0))
        .map_err(e)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(e)
}

impl Library {
    /// A record's provenance, row 0 first (`pattern`, `ant`, or `quickstart`).
    pub fn provenance(&self, target_kind: &str, id: &str) -> Result<Vec<ProvenanceRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT target_kind, target_id, seq, kind, finder, orcid, search_kind,
                        job_uuid, pack_hash, machine, engine_version, geometry_pin, source, date
                     FROM provenance WHERE target_kind = ?1 AND target_id = ?2 ORDER BY seq",
                )
                .map_err(e)?;
            st.query_map(params![target_kind, id], |r| {
                Ok(ProvenanceRow {
                    target_kind: r.get(0)?,
                    target_id: r.get(1)?,
                    seq: r.get(2)?,
                    kind: r.get(3)?,
                    finder: r.get(4)?,
                    orcid: r.get(5)?,
                    search_kind: r.get(6)?,
                    job_uuid: r.get(7)?,
                    pack_hash: r.get(8)?,
                    machine: r.get(9)?,
                    engine_version: r.get(10)?,
                    geometry_pin: r.get(11)?,
                    source: r.get(12)?,
                    date: r.get(13)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Record an old id of a stored record (`geometry-change`), so lookups by it find the
    /// record.
    pub fn add_alias(
        &self,
        alias: &str,
        target_kind: &str,
        id: &str,
        reason: &str,
    ) -> Result<(), String> {
        let alias = alias.trim().to_ascii_lowercase();
        if alias.is_empty() {
            return Err("empty alias".into());
        }
        let exists = match target_kind {
            "pattern" => self.contains(id)?,
            "ant" => self.ant(id).map(|a| a.id == id).unwrap_or(false),
            k => return Err(format!("aliases name a pattern or an ant, not a {k}")),
        };
        if !exists {
            return Err(format!("no {target_kind} with id {id}"));
        }
        self.write(|tx| put_alias_in(tx, &alias, target_kind, id, reason))
    }

    /// The aliases of a record (or every alias when `id` is empty), oldest first.
    pub fn aliases(&self, target_kind: &str, id: &str) -> Result<Vec<AliasRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT alias, target_kind, id, reason, created FROM id_aliases
                     WHERE target_kind = ?1 AND (?2 = '' OR id = ?2) ORDER BY created, alias",
                )
                .map_err(e)?;
            st.query_map(params![target_kind, id], |r| {
                Ok(AliasRow {
                    alias: r.get(0)?,
                    target_kind: r.get(1)?,
                    id: r.get(2)?,
                    reason: r.get(3)?,
                    created: r.get(4)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }
}
