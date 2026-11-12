//! What packs (`jetcarp_pack`) read from a library and write to it.
//!
//! **Publishing** reads every pattern in id order with the rules it was verified in, its
//! sightings (with the job's uuid and kind, never a local path) and its provenance
//! ([`Library::pack_patterns`]), and the rules, Quick Start entries with their pins, ant runs,
//! media rows and ratings.
//!
//! **Importing** stores records through [`Library::add_many_each`] with each record's origin
//! ([`Origin::with_history`]), then adds what else a pack says about them: the further rules
//! they were verified in, sightings merged per job (a job made for each search a pack names,
//! by its uuid), ratings, aliases, ant runs, Quick Start entries and media. Two tables record
//! what was imported, made the first time a pack is imported:
//!
//! | Table | One row per |
//! | --- | --- |
//! | `pack_imports` | import of a pack version: its manifest hash, feed, publisher, how its signature checked, when, and a report (JSON: counts, and the sample verified) |
//! | `pack_retractions` | record a publisher retracted: target, publisher, reason, date, the manifest that said so |
//! | `pack_revoked` | publisher key a pack's key statements revoked: kept for good, so a store serving an older statement cannot undo it |

use super::*;
use jetcarp_core::pattern::verify::Verification;
use std::collections::HashSet;

/// A rule a pattern was verified in (`pattern_rules`).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleVerification {
    pub notation: String,
    pub verified: Verification,
    pub periods: u64,
    pub engine_version: String,
    pub first_seen: String,
}

/// Copies of a pattern a job saw (`sightings`), with the job's uuid and kind.
#[derive(Clone, Debug, PartialEq)]
pub struct JobSighting {
    pub rule: String,
    pub job_uuid: String,
    pub job_kind: String,
    pub count: u64,
    pub soups: u64,
    pub first_seen: String,
    pub last_seen: String,
}

/// A pattern with what a pack says about it.
#[derive(Clone, Debug)]
pub struct PackPattern {
    pub rec: PatternRecord,
    pub rules: Vec<RuleVerification>,
    pub sightings: Vec<JobSighting>,
    pub provenance: Vec<ProvenanceRow>,
}

/// A row of `rules`.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleInfo {
    pub notation: String,
    pub neighborhood: String,
    pub steps_per_gen: u32,
    pub family: String,
    pub states: i64,
    pub birth: Option<i64>,
    pub survive: Option<i64>,
}

/// A Quick Start entry with its pin.
#[derive(Clone, Debug)]
pub struct PackQuickstart {
    pub row: QuickStartRow,
    pub pin_key: String,
    pub pin: String,
    pub provenance: Vec<ProvenanceRow>,
}

/// An ant run with its pin and history.
#[derive(Clone, Debug)]
pub struct PackAnt {
    pub row: AntRow,
    pub pin_key: String,
    pub provenance: Vec<ProvenanceRow>,
}

/// A media row (one file; frame lists are not packed).
#[derive(Clone, Debug, PartialEq)]
pub struct PackMedia {
    pub target_kind: String,
    pub target_id: String,
    pub kind: String,
    pub from_seed: bool,
    pub hash: String,
    pub ext: String,
    pub render: String,
    pub width: i64,
    pub height: i64,
    pub frames: i64,
    pub bytes: i64,
    pub key: String,
    pub created: String,
}

/// Sightings to merge into a job (the pack's count for the pattern, rule and search).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSighting {
    pub pattern_id: String,
    pub rule: String,
    pub job: i64,
    pub count: u64,
    pub soups: u64,
    pub first_seen: String,
    pub last_seen: String,
}

/// A retraction as the library keeps it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RetractionRow {
    pub target_kind: String,
    pub target_id: String,
    pub publisher: String,
    pub reason: String,
    pub date: String,
    pub manifest: String,
}

impl RetractionRow {
    /// `retracted by <publisher> on <date>: <reason>`, the publisher by name (and key) when the
    /// library knows the name ([`Library::publisher_names`]), else by key; the date's day only.
    pub fn describe(&self, names: &HashMap<String, String>) -> String {
        let who = match names.get(&self.publisher) {
            Some(n) if !n.is_empty() => format!("{n} ({})", self.publisher),
            _ => self.publisher.clone(),
        };
        let day = self.date.get(..10).unwrap_or(&self.date);
        if self.reason.trim().is_empty() {
            format!("retracted by {who} on {day}")
        } else {
            format!("retracted by {who} on {day}: {}", self.reason.trim())
        }
    }
}

/// An import as the library keeps it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PackImportRow {
    pub manifest: String,
    pub feed: String,
    pub publisher: String,
    /// `verified` (by the publisher's key), `unsigned`, ...
    pub signed: String,
    /// The store's or export's folder name.
    pub source: String,
    pub imported: String,
    pub engine_version: String,
    /// Counts and the verification sample (JSON).
    pub report: Value,
}

const PACK_TABLES: &str = "
CREATE TABLE IF NOT EXISTS pack_imports (
    id INTEGER PRIMARY KEY,
    manifest TEXT NOT NULL,
    feed TEXT NOT NULL,
    publisher TEXT NOT NULL,
    signed TEXT NOT NULL,
    source TEXT NOT NULL,
    imported TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    report TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pack_retractions (
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    publisher TEXT NOT NULL,
    reason TEXT NOT NULL,
    date TEXT NOT NULL,
    manifest TEXT NOT NULL,
    PRIMARY KEY (target_kind, target_id, publisher)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS pack_revoked (
    fingerprint TEXT PRIMARY KEY,
    seen TEXT NOT NULL
) WITHOUT ROWID;
";

fn has_table(c: &Connection, name: &str) -> Result<bool, String> {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(e)
}

fn provenance_of(c: &Connection, kind: &str, id: &str) -> Result<Vec<ProvenanceRow>, String> {
    let mut st = c
        .prepare_cached(
            "SELECT target_kind, target_id, seq, kind, finder, orcid, search_kind, job_uuid,
                pack_hash, machine, engine_version, geometry_pin, source, date
             FROM provenance WHERE target_kind = ?1 AND target_id = ?2 ORDER BY seq",
        )
        .map_err(e)?;
    st.query_map(params![kind, id], |r| {
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
}

impl Library {
    /// Every pattern (or those in `ids`) in id order, each with its rules, sightings and
    /// provenance; `f` is called for each, outside the library's lock. Returns how many.
    pub fn pack_patterns(
        &self,
        ids: Option<&HashSet<String>>,
        mut f: impl FnMut(PackPattern) -> Result<(), String>,
    ) -> Result<u64, String> {
        let mut last = String::new();
        let mut n = 0u64;
        loop {
            let (page, next): (Vec<PackPattern>, Option<String>) =
                self.read((vec![], None), |c| {
                    let mut st = c
                    .prepare_cached(&format!(
                        "SELECT {PCOLS} FROM patterns p WHERE p.id > ?1 ORDER BY p.id LIMIT 2000"
                    ))
                    .map_err(e)?;
                    let recs = st
                        .query_map([&last], record_from_row)
                        .map_err(e)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(e)?;
                    let mut rules = c
                        .prepare_cached(
                            "SELECT r.notation, pr.verified, pr.periods, pr.engine_version,
                            pr.first_seen
                         FROM pattern_rules pr JOIN rules r ON r.id = pr.rule_id
                         WHERE pr.pattern_id = ?1 ORDER BY pr.first_seen, r.id",
                        )
                        .map_err(e)?;
                    let mut seen = c
                        .prepare_cached(
                            "SELECT r.notation, j.uuid, j.kind, s.count, s.soups, s.first_seen,
                            s.last_seen
                         FROM sightings s JOIN rules r ON r.id = s.rule_id
                            JOIN jobs j ON j.id = s.job_id
                         WHERE s.pattern_id = ?1 ORDER BY r.notation, j.uuid",
                        )
                        .map_err(e)?;
                    let mut out = Vec::with_capacity(recs.len());
                    let mut next = None;
                    for rec in recs {
                        let rec = rec?;
                        next = Some(rec.id.clone());
                        if ids.is_some_and(|s| !s.contains(&rec.id)) {
                            continue;
                        }
                        let rv = rules
                            .query_map([&rec.id], |r| {
                                Ok((
                                    r.get::<_, String>(0)?,
                                    r.get::<_, String>(1)?,
                                    r.get::<_, i64>(2)?,
                                    r.get::<_, String>(3)?,
                                    r.get::<_, String>(4)?,
                                ))
                            })
                            .map_err(e)?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(e)?
                            .into_iter()
                            .map(|(notation, v, periods, ev, first)| RuleVerification {
                                notation,
                                verified: serde_json::from_str(&v).unwrap_or_default(),
                                periods: periods as u64,
                                engine_version: ev,
                                first_seen: first,
                            })
                            .collect();
                        let sv = seen
                            .query_map([&rec.id], |r| {
                                Ok(JobSighting {
                                    rule: r.get(0)?,
                                    job_uuid: r.get(1)?,
                                    job_kind: r.get(2)?,
                                    count: r.get::<_, i64>(3)? as u64,
                                    soups: r.get::<_, i64>(4)? as u64,
                                    first_seen: r.get(5)?,
                                    last_seen: r.get(6)?,
                                })
                            })
                            .map_err(e)?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(e)?;
                        let provenance = provenance_of(c, "pattern", &rec.id)?;
                        out.push(PackPattern {
                            rec,
                            rules: rv,
                            sightings: sv,
                            provenance,
                        });
                    }
                    Ok((out, next))
                })?;
            let Some(next) = next else { break };
            last = next;
            for p in page {
                n += 1;
                f(p)?;
            }
        }
        Ok(n)
    }

    /// Every rule as run.
    pub fn pack_rules(&self) -> Result<Vec<RuleInfo>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT notation, neighborhood, steps_per_gen, family, states, birth,
                        survive FROM rules ORDER BY notation, neighborhood, steps_per_gen",
                )
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(RuleInfo {
                    notation: r.get(0)?,
                    neighborhood: r.get(1)?,
                    steps_per_gen: r.get::<_, i64>(2)? as u32,
                    family: r.get(3)?,
                    states: r.get(4)?,
                    birth: r.get(5)?,
                    survive: r.get(6)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// The Quick Start entries a pack publishes, with their pins and history: every one not
    /// hidden, promotions, pack and atlas rows, but not the built-ins (every JETCARP has them).
    pub fn pack_quickstart(&self) -> Result<Vec<PackQuickstart>, String> {
        let rows = self
            .quickstart(&[QsSource::Promotion, QsSource::Pack, QsSource::Atlas])?
            .into_iter()
            .filter(|q| !q.hidden)
            .collect();
        self.pack_quickstart_of(rows)
    }

    /// Quick Start entries of these sources (every one when empty, hidden ones too), with their
    /// pins and history.
    pub fn pack_quickstart_with(
        &self,
        sources: &[QsSource],
    ) -> Result<Vec<PackQuickstart>, String> {
        self.pack_quickstart_of(self.quickstart(sources)?)
    }

    fn pack_quickstart_of(&self, rows: Vec<QuickStartRow>) -> Result<Vec<PackQuickstart>, String> {
        self.read(vec![], |c| {
            let mut out = vec![];
            for row in rows {
                let (pin_key, pin): (String, String) = c
                    .query_row(
                        "SELECT pin_key, geometry_pin FROM quickstart WHERE id = ?1",
                        [&row.id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(e)?;
                let provenance = provenance_of(c, "quickstart", &row.id)?;
                out.push(PackQuickstart {
                    row,
                    pin_key,
                    pin,
                    provenance,
                });
            }
            Ok(out)
        })
    }

    /// Every ant run, with its pin key and history.
    pub fn pack_ants(&self) -> Result<Vec<PackAnt>, String> {
        let rows = self.ants(&AntQuery {
            sort: Some("rule".into()),
            ..Default::default()
        })?;
        self.read(vec![], |c| {
            let mut out = vec![];
            for row in rows {
                let pin_key: String = c
                    .query_row("SELECT pin_key FROM ants WHERE id = ?1", [&row.id], |r| {
                        r.get(0)
                    })
                    .map_err(e)?;
                let provenance = provenance_of(c, "ant", &row.id)?;
                out.push(PackAnt {
                    row,
                    pin_key,
                    provenance,
                });
            }
            Ok(out)
        })
    }

    /// Every media row that names one file (not `frames`).
    pub fn pack_media(&self) -> Result<Vec<PackMedia>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT target_kind, target_id, kind, from_seed, hash, ext, render,
                        coalesce(width, 0), coalesce(height, 0), coalesce(frames, 0),
                        coalesce(bytes, 0), coalesce(key, ''), created
                     FROM media WHERE kind != 'frames'
                     ORDER BY target_kind, target_id, kind, from_seed",
                )
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(PackMedia {
                    target_kind: r.get(0)?,
                    target_id: r.get(1)?,
                    kind: r.get(2)?,
                    from_seed: r.get::<_, i64>(3)? != 0,
                    hash: r.get(4)?,
                    ext: r.get(5)?,
                    render: r.get(6)?,
                    width: r.get(7)?,
                    height: r.get(8)?,
                    frames: r.get(9)?,
                    bytes: r.get(10)?,
                    key: r.get(11)?,
                    created: r.get(12)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Importing

    /// The job of a search a pack names, by its uuid: the library's own when it has one, else a
    /// finished job of that kind made now (its spec says which pack it came from). Sightings
    /// imported for that search are credited to it.
    pub fn job_for_uuid(&self, uuid: &str, kind: &str, pack: &str) -> Result<i64, String> {
        self.job_for_uuid_with(uuid, kind, pack, None)
    }

    /// [`job_for_uuid`](Self::job_for_uuid) with the job's spec when it is made (an imported
    /// search's own settings, so it can be resumed): a made job is paused rather than done.
    pub fn job_for_uuid_with(
        &self,
        uuid: &str,
        kind: &str,
        pack: &str,
        spec: Option<&Value>,
    ) -> Result<i64, String> {
        let uuid = uuid.trim();
        if uuid.is_empty() || uuid.len() > 64 {
            return Err(format!("bad search uuid '{uuid}'"));
        }
        let kind = if kind.is_empty() { "imported" } else { kind };
        let now = now_utc();
        self.write(|tx| {
            if let Some(id) = tx
                .query_row("SELECT id FROM jobs WHERE uuid = ?1", [uuid], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()
                .map_err(e)?
            {
                return Ok(id);
            }
            let mut v = spec
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| serde_json::json!({}));
            v["uuid"] = Value::String(uuid.to_string());
            v["imported_from"] = Value::String(pack.to_string());
            let status = if spec.is_some() { "paused" } else { "done" };
            tx.execute(
                "INSERT INTO jobs (uuid, kind, spec, status, progress, engine_version, created,
                    updated, finished)
                 VALUES (?1, ?2, ?3, ?4, '{}', ?5, ?6, ?6, ?6)",
                params![
                    uuid,
                    kind,
                    v.to_string(),
                    status,
                    jetcarp_core::VERSION,
                    now
                ],
            )
            .map_err(e)?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// Merge sightings: each (pattern, rule, job) keeps the larger count and soups, the earlier
    /// first and the later last sighting, so importing the same pack again changes nothing.
    /// Each pattern's seen count grows by what its sightings grew by, and by `extra` (copies a
    /// pack counts without naming a search); then it is rescored. Returns the patterns changed.
    pub fn merge_sightings(
        &self,
        rows: &[ImportSighting],
        extra: &[(String, u64)],
    ) -> Result<u64, String> {
        self.write(|tx| {
            let mut grew: HashMap<String, u64> = HashMap::new();
            for s in rows {
                let (nb, spg): (String, i64) = tx
                    .prepare_cached(
                        "SELECT neighborhood, steps_per_gen FROM patterns WHERE id = ?1",
                    )
                    .map_err(e)?
                    .query_row([&s.pattern_id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(|x| format!("sighting of unknown pattern {}: {x}", s.pattern_id))?;
                let rid = rule_id_in(tx, &s.rule, parse_name(&nb)?, spg as u32)?;
                let old: Option<i64> = tx
                    .prepare_cached(
                        "SELECT count FROM sightings WHERE pattern_id = ?1 AND rule_id = ?2
                            AND job_id = ?3",
                    )
                    .map_err(e)?
                    .query_row(params![s.pattern_id, rid, s.job], |r| r.get(0))
                    .optional()
                    .map_err(e)?;
                tx.prepare_cached(
                    "INSERT INTO sightings (pattern_id, rule_id, job_id, count, soups,
                        first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (pattern_id, rule_id, job_id) DO UPDATE SET
                        count = max(count, excluded.count), soups = max(soups, excluded.soups),
                        first_seen = min(first_seen, excluded.first_seen),
                        last_seen = max(last_seen, excluded.last_seen)",
                )
                .map_err(e)?
                .execute(params![
                    s.pattern_id,
                    rid,
                    s.job,
                    s.count as i64,
                    s.soups as i64,
                    s.first_seen,
                    s.last_seen
                ])
                .map_err(e)?;
                let d = (s.count as i64 - old.unwrap_or(0)).max(0) as u64;
                *grew.entry(s.pattern_id.clone()).or_default() += d;
            }
            for (id, n) in extra {
                *grew.entry(id.clone()).or_default() += n;
            }
            let mut ids: Vec<(String, u64)> = grew.into_iter().filter(|x| x.1 > 0).collect();
            ids.sort();
            let mut get = tx
                .prepare_cached(
                    "SELECT class, period, cells, speed, seen FROM patterns WHERE id = ?1",
                )
                .map_err(e)?;
            let mut set = tx
                .prepare_cached("UPDATE patterns SET seen = ?2, score = ?3 WHERE id = ?1")
                .map_err(e)?;
            for (id, n) in &ids {
                let (class, period, cells, speed, seen): (String, i64, i64, Option<String>, i64) =
                    get.query_row([id], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                    })
                    .map_err(e)?;
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
            Ok(ids.len() as u64)
        })
    }

    /// Rules patterns were verified in, as a pack says (each kept if it ran longer than what
    /// the library has): (pattern id, rule notation, verification, first seen).
    pub fn add_rule_verifications(
        &self,
        rows: &[(String, String, Verification, String)],
    ) -> Result<(), String> {
        self.write(|tx| {
            for (id, notation, v, first) in rows {
                let (nb, spg): (String, i64) = tx
                    .prepare_cached(
                        "SELECT neighborhood, steps_per_gen FROM patterns WHERE id = ?1",
                    )
                    .map_err(e)?
                    .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(|x| format!("verification of unknown pattern {id}: {x}"))?;
                let rid = rule_id_in(tx, notation, parse_name(&nb)?, spg as u32)?;
                put_pattern_rule(tx, id, rid, v, first)?;
            }
            Ok(())
        })
    }

    /// Ratings from a pack, each unless the library has the same one (target, rater, rating,
    /// verdict, note, source and time). Returns (added, already there).
    pub fn import_ratings(&self, rows: &[RatingRow]) -> Result<(u64, u64), String> {
        self.write(|tx| {
            let (mut added, mut had) = (0u64, 0u64);
            for r in rows {
                let exists = tx
                    .prepare_cached(
                        "SELECT 1 FROM ratings WHERE target_kind = ?1 AND target_id = ?2
                            AND rater = ?3 AND rating = ?4 AND verdict IS ?5 AND note = ?6
                            AND source = ?7 AND created = ?8",
                    )
                    .map_err(e)?
                    .exists(params![
                        r.target_kind,
                        r.target_id,
                        r.rater,
                        r.rating,
                        r.verdict,
                        r.note,
                        r.source,
                        r.created
                    ])
                    .map_err(e)?;
                if exists {
                    had += 1;
                    continue;
                }
                tx.prepare_cached(
                    "INSERT INTO ratings (target_kind, target_id, rater, rating, verdict, note,
                        source, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )
                .map_err(e)?
                .execute(params![
                    r.target_kind,
                    r.target_id,
                    r.rater,
                    r.rating,
                    r.verdict,
                    r.note,
                    r.source,
                    r.created
                ])
                .map_err(e)?;
                added += 1;
            }
            Ok((added, had))
        })
    }

    /// Old ids of records a pack lists (aliases whose record is not here are skipped).
    pub fn import_aliases(&self, rows: &[AliasRow]) -> Result<u64, String> {
        self.write(|tx| {
            let mut n = 0;
            for a in rows {
                let table = match a.target_kind.as_str() {
                    "pattern" => "patterns",
                    "ant" => "ants",
                    _ => continue,
                };
                let here: bool = tx
                    .prepare_cached(&format!("SELECT 1 FROM {table} WHERE id = ?1"))
                    .map_err(e)?
                    .exists([&a.id])
                    .map_err(e)?;
                if here {
                    provenance::put_alias_in(tx, &a.alias, &a.target_kind, &a.id, &a.reason)?;
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Ant runs from a pack, each with its history (written first, so the run's own `found`
    /// row is not made).
    pub fn import_ants(&self, rows: &[(AntRow, Origin)]) -> Result<u64, String> {
        let now = now_utc();
        self.write(|tx| {
            let mut n = 0;
            for (a, o) in rows {
                let had: bool = tx
                    .prepare_cached("SELECT 1 FROM ants WHERE id = ?1")
                    .map_err(e)?
                    .exists([&a.id])
                    .map_err(e)?;
                if !had {
                    provenance::first_in(tx, "ant", &a.id, o, 0, &a.geometry_pin, "", "", "")?;
                }
                n += ants::put_ant(tx, a, 0, &now)? as u64;
                if !had && (a.name.is_some() || !a.notes.is_empty() || a.rating.is_some()) {
                    tx.execute(
                        "UPDATE ants SET name = ?2, notes = ?3, rating = ?4, state = ?5
                         WHERE id = ?1",
                        params![a.id, a.name, a.notes, a.rating, a.state],
                    )
                    .map_err(e)?;
                }
            }
            Ok(n)
        })
    }

    /// Quick Start entries from a pack (an entry the library has is left as it is). Returns how
    /// many were added.
    pub fn import_quickstart(
        &self,
        rows: &[(QuickStartRow, Origin, String)],
    ) -> Result<u64, String> {
        let mut n = 0;
        for (q, o, pin) in rows {
            if self.get_quickstart(&q.id).is_ok() {
                continue;
            }
            self.write(|tx| provenance::first_in(tx, "quickstart", &q.id, o, 0, pin, "", "", ""))?;
            self.put_quickstart(q)?;
            n += 1;
        }
        Ok(n)
    }

    /// Media rows whose files are in the library's media folder already (a row the library has
    /// for the same target, kind and mode is kept).
    pub fn import_media(&self, rows: &[PackMedia]) -> Result<u64, String> {
        self.write(|tx| {
            let mut n = 0;
            for r in rows {
                n += tx
                    .prepare_cached(
                        "INSERT OR IGNORE INTO media (target_kind, target_id, kind, ext,
                            from_seed, render, width, height, frames, bytes, key, hash, created,
                            updated)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                    )
                    .map_err(e)?
                    .execute(params![
                        r.target_kind,
                        r.target_id,
                        r.kind,
                        r.ext,
                        r.from_seed as i64,
                        r.render,
                        r.width,
                        r.height,
                        r.frames,
                        r.bytes,
                        r.key,
                        r.hash,
                        r.created
                    ])
                    .map_err(e)? as u64;
            }
            Ok(n)
        })
    }

    /// Flag records as `needs_reverify` (a pack made them on another geometry or meaning).
    pub fn flag_reverify(&self, kind: &str, ids: &[String]) -> Result<u64, String> {
        let table = match kind {
            "pattern" => "patterns",
            "ant" => "ants",
            "quickstart" => "quickstart",
            k => return Err(format!("no records of kind {k}")),
        };
        self.write(|tx| {
            let mut n = 0;
            for id in ids {
                n += tx
                    .prepare_cached(&format!(
                        "UPDATE {table} SET needs_reverify = 1 WHERE id = ?1 AND needs_reverify = 0"
                    ))
                    .map_err(e)?
                    .execute([id])
                    .map_err(e)? as u64;
            }
            Ok(n)
        })
    }

    /// Record retractions (a retraction already recorded for the target and publisher is
    /// replaced). Returns how many name records the library has.
    pub fn record_retractions(&self, rows: &[RetractionRow]) -> Result<u64, String> {
        self.write(|tx| {
            tx.execute_batch(PACK_TABLES).map_err(e)?;
            let mut n = 0;
            for r in rows {
                tx.prepare_cached(
                    "INSERT INTO pack_retractions (target_kind, target_id, publisher, reason,
                        date, manifest) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (target_kind, target_id, publisher) DO UPDATE SET
                        reason = excluded.reason, date = excluded.date,
                        manifest = excluded.manifest",
                )
                .map_err(e)?
                .execute(params![
                    r.target_kind,
                    r.target_id,
                    r.publisher,
                    r.reason,
                    r.date,
                    r.manifest
                ])
                .map_err(e)?;
                let table = match r.target_kind.as_str() {
                    "pattern" => "patterns",
                    "ant" => "ants",
                    "quickstart" => "quickstart",
                    _ => continue,
                };
                n += tx
                    .prepare_cached(&format!("SELECT 1 FROM {table} WHERE id = ?1"))
                    .map_err(e)?
                    .exists([&r.target_id])
                    .map_err(e)? as u64;
            }
            Ok(n)
        })
    }

    /// The names of the publishers whose packs the library imported, by key, as their imports
    /// recorded them (an import before names were recorded gives none).
    pub fn publisher_names(&self) -> Result<HashMap<String, String>, String> {
        self.read(HashMap::new(), |c| {
            if !has_table(c, "pack_imports")? {
                return Ok(HashMap::new());
            }
            let mut st = c
                .prepare("SELECT publisher, report FROM pack_imports ORDER BY id")
                .map_err(e)?;
            let rows = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(e)?;
            let mut out = HashMap::new();
            for row in rows {
                let (p, report) = row.map_err(e)?;
                if let Some(n) = serde_json::from_str::<Value>(&report)
                    .ok()
                    .and_then(|v| v["publisher_name"].as_str().map(str::to_string))
                    .filter(|n| !n.is_empty())
                {
                    out.insert(p, n);
                }
            }
            Ok(out)
        })
    }

    /// The retractions recorded for records of one kind (`pattern`, `ant`, `quickstart`), by id.
    pub fn retractions_by_id(
        &self,
        kind: &str,
    ) -> Result<HashMap<String, Vec<RetractionRow>>, String> {
        let mut out: HashMap<String, Vec<RetractionRow>> = HashMap::new();
        for r in self.retractions()? {
            if r.target_kind == kind {
                out.entry(r.target_id.clone()).or_default().push(r);
            }
        }
        Ok(out)
    }

    /// Retractions recorded from packs.
    pub fn retractions(&self) -> Result<Vec<RetractionRow>, String> {
        self.read(vec![], |c| {
            if !has_table(c, "pack_retractions")? {
                return Ok(vec![]);
            }
            let mut st = c
                .prepare(
                    "SELECT target_kind, target_id, publisher, reason, date, manifest
                     FROM pack_retractions ORDER BY target_kind, target_id, publisher",
                )
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(RetractionRow {
                    target_kind: r.get(0)?,
                    target_id: r.get(1)?,
                    publisher: r.get(2)?,
                    reason: r.get(3)?,
                    date: r.get(4)?,
                    manifest: r.get(5)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Record an import and what it found.
    pub fn record_pack_import(&self, r: &PackImportRow) -> Result<(), String> {
        self.write(|tx| {
            tx.execute_batch(PACK_TABLES).map_err(e)?;
            tx.execute(
                "INSERT INTO pack_imports (manifest, feed, publisher, signed, source, imported,
                    engine_version, report) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    r.manifest,
                    r.feed,
                    r.publisher,
                    r.signed,
                    r.source,
                    r.imported,
                    r.engine_version,
                    r.report.to_string()
                ],
            )
            .map_err(e)?;
            Ok(())
        })
    }

    /// The imports recorded, oldest first.
    pub fn pack_imports(&self) -> Result<Vec<PackImportRow>, String> {
        self.read(vec![], |c| {
            if !has_table(c, "pack_imports")? {
                return Ok(vec![]);
            }
            let mut st = c
                .prepare(
                    "SELECT manifest, feed, publisher, signed, source, imported, engine_version,
                        report FROM pack_imports ORDER BY id",
                )
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(PackImportRow {
                    manifest: r.get(0)?,
                    feed: r.get(1)?,
                    publisher: r.get(2)?,
                    signed: r.get(3)?,
                    source: r.get(4)?,
                    imported: r.get(5)?,
                    engine_version: r.get(6)?,
                    report: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Keep keys as revoked for good.
    pub fn record_revoked(&self, fingerprints: &[String]) -> Result<(), String> {
        if fingerprints.is_empty() {
            return Ok(());
        }
        let now = now_utc();
        self.write(|tx| {
            tx.execute_batch(PACK_TABLES).map_err(e)?;
            for fp in fingerprints {
                tx.execute(
                    "INSERT OR IGNORE INTO pack_revoked (fingerprint, seen) VALUES (?1, ?2)",
                    params![fp, now],
                )
                .map_err(e)?;
            }
            Ok(())
        })
    }

    /// The keys packs have revoked.
    pub fn revoked_keys(&self) -> Result<HashSet<String>, String> {
        self.read(HashSet::new(), |c| {
            if !has_table(c, "pack_revoked")? {
                return Ok(HashSet::new());
            }
            let mut st = c
                .prepare("SELECT fingerprint FROM pack_revoked")
                .map_err(e)?;
            st.query_map([], |r| r.get::<_, String>(0))
                .map_err(e)?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(e)
        })
    }

    /// The sightings of a pattern with each job's uuid (what a round trip compares).
    pub fn sightings_by_uuid(&self, id: &str) -> Result<Vec<JobSighting>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare_cached(
                    "SELECT r.notation, j.uuid, j.kind, s.count, s.soups, s.first_seen,
                        s.last_seen
                     FROM sightings s JOIN rules r ON r.id = s.rule_id
                        JOIN jobs j ON j.id = s.job_id
                     WHERE s.pattern_id = ?1 ORDER BY r.notation, j.uuid",
                )
                .map_err(e)?;
            st.query_map([id], |r| {
                Ok(JobSighting {
                    rule: r.get(0)?,
                    job_uuid: r.get(1)?,
                    job_kind: r.get(2)?,
                    count: r.get::<_, i64>(3)? as u64,
                    soups: r.get::<_, i64>(4)? as u64,
                    first_seen: r.get(5)?,
                    last_seen: r.get(6)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }
}
