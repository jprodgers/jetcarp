//! Ant and turmite runs in the catalog (table `ants`, see [`crate::ants`]).
//!
//! One row per (rule, tiling, start): the run's outcome (`highway`, `periodic`, `symmetric`,
//! `bounded`, `chaotic`), the highway's period, displacement, speed, and onset, the steps run,
//! and the proof or symmetry details in `body`. The rows of one tiling are the census of
//! outcomes per turn string or table ([`Library::ant_census`]). Running the same rule again
//! replaces the result when the new run is final (a highway or orbit) or ran at least as long,
//! and keeps the curation (name, notes, rating, state).
//!
//! Ants are their own kind of record rather than a pattern class: a pattern is a set of cells
//! that recurs under a CA rule, while an ant's highway is the trail of one agent, with no rule
//! range, phases, or sightings. Media and ratings name them with `target_kind = 'ant'`.

use super::*;
use crate::ants::run::{AntRun, Outcome, Start};

/// Ants (Phase 4 wave 4): the `ants` table, and `media` and `ratings` rebuilt so their
/// `target_kind` may be `ant` (SQLite cannot change a CHECK in place).
pub(super) const MIGRATION_ANTS: &str = r#"
CREATE TABLE ants (
    id TEXT PRIMARY KEY,
    rule TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('string', 'table')),
    states INTEGER NOT NULL,
    colors INTEGER NOT NULL,
    tiling TEXT NOT NULL,
    tiling_id TEXT NOT NULL,
    tiling_name TEXT NOT NULL,
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
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);
CREATE INDEX ants_outcome ON ants (outcome, tiling_id, score DESC);
CREATE INDEX ants_tiling ON ants (tiling, outcome);
CREATE INDEX ants_rule ON ants (rule);
CREATE INDEX ants_job ON ants (job_id);

CREATE TABLE media_v5 (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'setting', 'quickstart', 'ant')),
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
    key TEXT,
    hash TEXT,
    updated TEXT,
    UNIQUE (target_kind, target_id, kind, from_seed)
);
INSERT INTO media_v5 (id, target_kind, target_id, kind, path, from_seed, render, width, height,
    frames, bytes, created, key, hash, updated)
    SELECT id, target_kind, target_id, kind, path, from_seed, render, width, height, frames,
        bytes, created, key, hash, updated FROM media;
DROP TABLE media;
ALTER TABLE media_v5 RENAME TO media;

CREATE TABLE ratings_v5 (
    id INTEGER PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('pattern', 'setting', 'quickstart', 'ant')),
    target_id TEXT NOT NULL,
    rater TEXT NOT NULL,
    rating REAL NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created TEXT NOT NULL
);
INSERT INTO ratings_v5 (id, target_kind, target_id, rater, rating, note, created)
    SELECT id, target_kind, target_id, rater, rating, note, created FROM ratings;
DROP TABLE ratings;
ALTER TABLE ratings_v5 RENAME TO ratings;
CREATE INDEX ratings_target ON ratings (target_kind, target_id);
"#;

/// One ant run as stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AntRow {
    pub id: String,
    /// `Ant RL`, `Turmite LLRR`, `Turmite 1R1,1L0/1R1,0N0`.
    pub rule: String,
    /// `string` (a turn string) or `table` (several states, or other writes).
    pub kind: String,
    pub states: u8,
    pub colors: u8,
    pub tiling: TilingSpec,
    pub tiling_name: String,
    pub start: Start,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    /// Displacement per period, lattice vector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset: Option<u64>,
    pub steps: u64,
    pub cells: u64,
    /// The whole run (proof, symmetry, reach) and the limits it ran with.
    pub run: AntRun,
    pub limits: crate::ants::run::Limits,
    pub score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<i64>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

impl AntRow {
    /// `RL square: highway p104 d(-2,2) from 9977`
    pub fn line(&self) -> String {
        let rule = self
            .rule
            .trim_start_matches("Turmite ")
            .trim_start_matches("Ant ");
        let what = match (self.outcome, self.period, self.d, self.onset) {
            (Outcome::Highway, Some(p), Some(d), Some(o)) => {
                format!("highway p{p} d({},{}) from step {o}", d[0], d[1])
            }
            (Outcome::Periodic, Some(p), _, Some(o)) => format!("periodic p{p} from step {o}"),
            (o, ..) => format!("{} at {} steps", o.name(), self.steps),
        };
        format!(
            "{} {} {}: {what}{}",
            self.id,
            rule,
            self.tiling_name,
            if self.start == Start::Empty {
                String::new()
            } else {
                format!(" ({})", self.start.label())
            }
        )
    }
}

/// The catalog id of a (rule, tiling, start).
pub fn ant_id(rule: &str, tiling: &TilingSpec, start: &Start) -> String {
    let key = format!(
        "{rule}|{}|{}",
        tiling_key(tiling),
        serde_json::to_string(start).unwrap_or_default()
    );
    format!("ant-{}", &crate::media::content_hash(key.as_bytes())[..12])
}

/// Filters for [`Library::ants`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AntQuery {
    pub outcome: Option<Outcome>,
    /// Tiling family or catalog id (`square`, `hex`, `k2-3`).
    pub tiling: Option<String>,
    /// Rule notation or turn string (`RL`, `Turmite LLRR`); its mirror image matches too.
    pub rule: Option<String>,
    pub job: Option<i64>,
    /// `onset`, `period`, `speed`, `score`, `rule` (default: score, then rule).
    pub sort: Option<String>,
    pub limit: Option<usize>,
}

/// Outcome counts of one tiling.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AntCensus {
    pub tiling: String,
    pub tiling_name: String,
    pub runs: u64,
    pub by_outcome: BTreeMap<String, u64>,
}

/// Curation of an ant record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AntUpdate {
    pub name: Option<String>,
    pub notes: Option<String>,
    pub rating: Option<i64>,
    pub state: Option<String>,
}

use std::collections::BTreeMap;

/// The interest of a run (0 to 100): highways by how late they start and how long their
/// period is, symmetric growth, and chaos by the size reached.
pub fn ant_score(r: &AntRun) -> f64 {
    let s = match r.outcome {
        Outcome::Highway => {
            let o = r.onset.unwrap_or(0) as f64;
            let p = r.period.unwrap_or(1) as f64;
            50.0 + 25.0 * (o.max(1.0).log10() / 6.0).min(1.0) + 25.0 * (p.log10() / 4.0).min(1.0)
        }
        Outcome::Symmetric => 60.0 + 20.0 * ((r.cells as f64).max(1.0).log10() / 6.0).min(1.0),
        Outcome::Chaotic => 20.0 + 20.0 * ((r.cells as f64).max(1.0).log10() / 6.0).min(1.0),
        Outcome::Periodic => 10.0,
        Outcome::Bounded => 15.0,
    };
    (s * 10.0).round() / 10.0
}

fn row_of(r: &rusqlite::Row) -> rusqlite::Result<AntRow> {
    let body: String = r.get("body")?;
    let v: Value = serde_json::from_str(&body).unwrap_or_default();
    let run: AntRun = serde_json::from_value(v["run"].clone()).map_err(|x| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(x))
    })?;
    let limits = serde_json::from_value(v["limits"].clone()).unwrap_or_default();
    let tiling: TilingSpec = serde_json::from_str(&r.get::<_, String>("tiling")?)
        .unwrap_or_else(|_| TilingSpec::new("square"));
    let start: Start = serde_json::from_str(&r.get::<_, String>("start")?).unwrap_or_default();
    let outcome: Outcome =
        serde_json::from_value(Value::String(r.get("outcome")?)).unwrap_or(Outcome::Chaotic);
    let dx: Option<i64> = r.get("dx")?;
    let dy: Option<i64> = r.get("dy")?;
    Ok(AntRow {
        id: r.get("id")?,
        rule: r.get("rule")?,
        kind: r.get("kind")?,
        states: r.get::<_, i64>("states")? as u8,
        colors: r.get::<_, i64>("colors")? as u8,
        tiling,
        tiling_name: r.get("tiling_name")?,
        start,
        outcome,
        period: r.get::<_, Option<i64>>("period")?.map(|x| x as u64),
        d: dx.zip(dy).map(|(a, b)| [a as i32, b as i32]),
        speed: r.get("speed")?,
        onset: r.get::<_, Option<i64>>("onset")?.map(|x| x as u64),
        steps: r.get::<_, i64>("steps")? as u64,
        cells: r.get::<_, i64>("cells")? as u64,
        run,
        limits,
        score: r.get("score")?,
        rating: r.get("rating")?,
        state: r.get("state")?,
        name: r.get("name")?,
        notes: r.get("notes")?,
        job: r.get("job_id")?,
        created: r.get("created")?,
        updated: r.get("updated")?,
    })
}

/// Insert or update an ant row inside a transaction (see the module docs for which result
/// wins). Returns whether the stored result changed.
fn put_ant(tx: &Transaction, a: &AntRow, job: i64, now: &str) -> Result<bool, String> {
    let old: Option<(String, i64)> = tx
        .query_row(
            "SELECT outcome, steps FROM ants WHERE id = ?1",
            [&a.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(e)?;
    if let Some((o, steps)) = &old {
        let final_old = o == "highway" || o == "periodic";
        let final_new = matches!(a.outcome, Outcome::Highway | Outcome::Periodic);
        if final_old || (!final_new && (a.steps as i64) < *steps) {
            tx.execute(
                "UPDATE ants SET job_id = coalesce(job_id, ?2) WHERE id = ?1",
                params![a.id, job],
            )
            .map_err(e)?;
            return Ok(false);
        }
    }
    let body = serde_json::json!({"run": a.run, "limits": a.limits}).to_string();
    let tid = tiling_id(&a.tiling);
    tx.execute(
        "INSERT INTO ants (id, rule, kind, states, colors, tiling, tiling_id, tiling_name, start,
            start_label, outcome, period, dx, dy, speed, onset, steps, cells, body, score, job_id,
            created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
            ?19, ?20, ?21, ?22, ?22)
         ON CONFLICT (id) DO UPDATE SET outcome = excluded.outcome, period = excluded.period,
            dx = excluded.dx, dy = excluded.dy, speed = excluded.speed, onset = excluded.onset,
            steps = excluded.steps, cells = excluded.cells, body = excluded.body,
            score = excluded.score, job_id = excluded.job_id, updated = excluded.updated",
        params![
            a.id,
            a.rule,
            a.kind,
            a.states as i64,
            a.colors as i64,
            tiling_key(&a.tiling),
            tid,
            a.tiling_name,
            serde_json::to_string(&a.start).unwrap_or_default(),
            a.start.label(),
            a.outcome.name(),
            a.period.map(|x| x as i64),
            a.d.map(|d| d[0] as i64),
            a.d.map(|d| d[1] as i64),
            a.speed,
            a.onset.map(|x| x as i64),
            a.steps as i64,
            a.cells as i64,
            body,
            a.score,
            job,
            now,
        ],
    )
    .map_err(e)?;
    Ok(true)
}

impl Library {
    /// Store a batch of ant runs of a job (job 0: manual) and, when given, the job's progress and
    /// checkpoint, in one transaction. Returns how many stored results changed.
    pub fn ants_flush(
        &self,
        job: i64,
        rows: &[AntRow],
        progress: Option<&Value>,
        checkpoint: Option<&[u8]>,
    ) -> Result<usize, String> {
        let now = now_utc();
        self.write(|tx| {
            let mut changed = 0;
            for a in rows {
                changed += put_ant(tx, a, job, &now)? as usize;
            }
            if progress.is_some() || checkpoint.is_some() {
                let n = tx
                    .execute(
                        "UPDATE jobs SET updated = ?2, progress = coalesce(?3, progress),
                            checkpoint = coalesce(?4, checkpoint) WHERE id = ?1",
                        params![job, now, progress.map(|p| p.to_string()), checkpoint],
                    )
                    .map_err(e)?;
                if n == 0 {
                    return Err(format!("no job {job}"));
                }
            }
            Ok(changed)
        })
    }

    /// One ant record by id (or unique id prefix, or name).
    pub fn ant(&self, key: &str) -> Result<AntRow, String> {
        let key = key.trim();
        let rows = self.read(Vec::new(), |c| {
            let mut st = c
                .prepare(
                    "SELECT * FROM ants WHERE id = ?1 OR id LIKE ?2 OR name = ?1 COLLATE NOCASE
                     ORDER BY id = ?1 DESC LIMIT 3",
                )
                .map_err(e)?;
            st.query_map(params![key, format!("{key}%")], row_of)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)
        })?;
        match rows.len() {
            0 => Err(format!(
                "no ant record '{key}' (ids look like ant-1a2b3c4d5e6f)"
            )),
            1 => Ok(rows.into_iter().next().unwrap()),
            _ if rows[0].id == key => Ok(rows.into_iter().next().unwrap()),
            _ => Err(format!(
                "'{key}' matches several ant records; give more of the id"
            )),
        }
    }

    /// Ant records matching a query.
    pub fn ants(&self, q: &AntQuery) -> Result<Vec<AntRow>, String> {
        let mut wh: Vec<String> = Vec::new();
        let mut args: Vec<Sql> = Vec::new();
        if let Some(o) = q.outcome {
            args.push(Sql::Text(o.name().into()));
            wh.push(format!("outcome = ?{}", args.len()));
        }
        if let Some(t) = &q.tiling {
            let key = crate::families::alias(t.trim())
                .unwrap_or(t.trim())
                .to_string();
            args.push(Sql::Text(key.clone()));
            let n = args.len();
            args.push(Sql::Text(t.trim().to_string()));
            wh.push(format!(
                "(tiling_id = ?{n} OR tiling_name = ?{} COLLATE NOCASE)",
                n + 1
            ));
        }
        if let Some(r) = &q.rule {
            // A rule and its mirror image are one class (searches store one of them), so a
            // query for either finds both.
            let m = match Rule::parse(r) {
                Ok((Rule::Turmite(m), _)) => Some(m),
                _ => match Rule::parse(&format!("Turmite {r}")) {
                    Ok((Rule::Turmite(m), _)) => Some(m),
                    _ => None,
                },
            };
            let (a, b) = match &m {
                Some(m) => (m.notation(), m.mirror().notation()),
                None => (r.trim().to_string(), r.trim().to_string()),
            };
            args.push(Sql::Text(a));
            args.push(Sql::Text(b));
            wh.push(format!("rule IN (?{}, ?{})", args.len() - 1, args.len()));
        }
        if let Some(j) = q.job {
            args.push(Sql::Integer(j));
            wh.push(format!("job_id = ?{}", args.len()));
        }
        let order = match q.sort.as_deref().unwrap_or("score") {
            "onset" => "onset DESC NULLS LAST, id",
            "period" => "period DESC NULLS LAST, id",
            "speed" => "speed DESC NULLS LAST, id",
            "rule" => "length(rule), rule, tiling_name",
            "score" => "score DESC, length(rule), rule",
            x => {
                return Err(format!(
                    "sort by onset, period, speed, score, or rule, not '{x}'"
                ));
            }
        };
        let sql = format!(
            "SELECT * FROM ants {} ORDER BY {order} LIMIT {}",
            if wh.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", wh.join(" AND "))
            },
            q.limit.unwrap_or(1_000_000)
        );
        self.read(Vec::new(), |c| {
            let mut st = c.prepare(&sql).map_err(e)?;
            st.query_map(rusqlite::params_from_iter(args.iter()), row_of)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)
        })
    }

    /// Outcome counts per tiling (of one job, or every run).
    pub fn ant_census(&self, job: Option<i64>) -> Result<Vec<AntCensus>, String> {
        let (filter, args): (&str, Vec<i64>) = match job {
            Some(j) => ("WHERE job_id = ?1", vec![j]),
            None => ("", vec![]),
        };
        let rows: Vec<(String, String, String, i64)> = self.read(Vec::new(), |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT tiling, tiling_name, outcome, count(*) FROM ants {filter}
                     GROUP BY tiling, outcome ORDER BY tiling_name, outcome"
                ))
                .map_err(e)?;
            st.query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })?;
        let mut out: Vec<AntCensus> = Vec::new();
        for (t, name, o, n) in rows {
            if out.last().is_none_or(|c| c.tiling != t) {
                out.push(AntCensus {
                    tiling: t.clone(),
                    tiling_name: name,
                    ..Default::default()
                });
            }
            let c = out.last_mut().unwrap();
            c.runs += n as u64;
            c.by_outcome.insert(o, n as u64);
        }
        Ok(out)
    }

    /// Curate an ant record.
    pub fn update_ant(&self, key: &str, u: &AntUpdate) -> Result<AntRow, String> {
        let a = self.ant(key)?;
        if let Some(r) = u.rating
            && !(1..=5).contains(&r)
        {
            return Err("ratings are 1 to 5".into());
        }
        let now = now_utc();
        self.write(|tx| {
            tx.execute(
                "UPDATE ants SET name = coalesce(?2, name), notes = coalesce(?3, notes),
                    rating = coalesce(?4, rating), state = coalesce(?5, state), updated = ?6
                 WHERE id = ?1",
                params![a.id, u.name, u.notes, u.rating, u.state, now],
            )
            .map_err(e)?;
            if let Some(r) = u.rating {
                tx.execute(
                    "INSERT INTO ratings (target_kind, target_id, rater, rating, note, created)
                     VALUES ('ant', ?1, 'user', ?2, '', ?3)",
                    params![a.id, r as f64, now],
                )
                .map_err(e)?;
            }
            Ok(())
        })?;
        self.ant(&a.id)
    }

    /// Remove ant records (by id), with their media rows and ratings.
    pub fn remove_ant(&self, key: &str) -> Result<AntRow, String> {
        let a = self.ant(key)?;
        self.write(|tx| {
            for sql in [
                "DELETE FROM ants WHERE id = ?1",
                "DELETE FROM media WHERE target_kind = 'ant' AND target_id = ?1",
                "DELETE FROM ratings WHERE target_kind = 'ant' AND target_id = ?1",
            ] {
                tx.execute(sql, [&a.id]).map_err(e)?;
            }
            Ok(())
        })?;
        Ok(a)
    }
}
