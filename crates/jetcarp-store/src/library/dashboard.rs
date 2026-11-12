//! Catalog reads for the hunt dashboard and the Quick Start menu (`jetcarp_search::dashboard`,
//! `crate::quickstart::menu`): Quick Start usage, the latest user rating of every target, the rule-space
//! heat map's rows, and a job's finds. All of them are plain reads (WAL lets them run while a
//! hunt writes).

use super::*;

/// One row of `quickstart_usage`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageRow {
    /// The entry's handle (`find:<id>`, `atlas:<...>`, `rule:<notation>`, a bundled name).
    pub handle: String,
    pub opens: u64,
    pub first_opened: Option<String>,
    pub last_opened: Option<String>,
}

/// One rule of the heat map: its B/S bitmasks and what was found in it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct HeatRow {
    pub birth: u32,
    pub survive: u32,
    pub notation: String,
    /// Distinct patterns seen.
    pub finds: u64,
    /// Distinct spaceships seen (hunts).
    pub ships: u64,
    /// Object copies seen (hunts).
    pub copies: u64,
}

/// What the heat map counts.
#[derive(Clone, Debug, PartialEq)]
pub enum HeatQuery {
    /// Patterns sighted (by one job, or every job), per rule.
    Patterns { job: Option<i64> },
}

/// A pattern a job saw, for the dashboard's lists.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FindRow {
    pub id: String,
    pub label: String,
    pub class: String,
    pub period: u64,
    /// A rule it was sighted in (the first by notation).
    pub rule: String,
    pub tiling: String,
    /// Copies in this job, and in the whole catalog.
    pub count: u64,
    pub seen: u64,
    pub score: f64,
    pub rating: Option<u8>,
    pub state: String,
    pub first_seen: String,
}

impl Library {
    /// Every row of `quickstart_usage` (most recently opened first).
    pub fn quickstart_usage(&self) -> Result<Vec<UsageRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT handle, opens, first_opened, last_opened FROM quickstart_usage
                     ORDER BY last_opened DESC, handle",
                )
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(UsageRow {
                    handle: r.get(0)?,
                    opens: r.get::<_, i64>(1)? as u64,
                    first_opened: r.get(2)?,
                    last_opened: r.get(3)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// The latest user rating of every rated target: `(target_kind, target_id) -> rating`.
    pub fn user_ratings(&self) -> Result<HashMap<(String, String), f64>, String> {
        self.read(HashMap::new(), |c| {
            let mut st = c
                .prepare(
                    "SELECT target_kind, target_id, rating FROM ratings WHERE rater = 'user'
                     ORDER BY id",
                )
                .map_err(e)?;
            let rows = st
                .query_map([], |r| {
                    Ok((
                        (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                        r.get::<_, f64>(2)?,
                    ))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            // Later rows win.
            Ok(rows.into_iter().collect())
        })
    }

    /// B/S rules with what was found in them on one tiling (identity key, [`tiling_key`]) and
    /// neighbourhood, for rules with `states` states (2: B/S; more: Generations).
    pub fn heat_rows(
        &self,
        q: &HeatQuery,
        tiling: &str,
        nb: Nb,
        states: u32,
    ) -> Result<Vec<HeatRow>, String> {
        let nb = name_of(&nb);
        self.read(vec![], |c| {
            let (sql, job) = match q {
                HeatQuery::Patterns { job } => (
                    "SELECT r.birth, r.survive, r.notation, count(DISTINCT s.pattern_id),
                        count(DISTINCT CASE WHEN p.class = 'spaceship' THEN s.pattern_id END),
                        coalesce(sum(s.count), 0)
                     FROM sightings s JOIN rules r ON r.id = s.rule_id
                        JOIN patterns p ON p.id = s.pattern_id
                     WHERE (?1 IS NULL OR s.job_id = ?1) AND p.tiling = ?2
                        AND r.neighborhood = ?3 AND r.states = ?4 AND r.birth IS NOT NULL
                     GROUP BY s.rule_id",
                    *job,
                ),
            };
            let mut st = c.prepare(sql).map_err(e)?;
            st.query_map(params![job, tiling, nb, states as i64], |r| {
                Ok(HeatRow {
                    birth: r.get::<_, i64>(0)? as u32,
                    survive: r.get::<_, i64>(1)? as u32,
                    notation: r.get(2)?,
                    finds: r.get::<_, i64>(3)? as u64,
                    ships: r.get::<_, i64>(4)? as u64,
                    copies: r.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Patterns a job sighted (every job when `None`), one row per pattern (with the first of
    /// its rules by notation, and its copies over them):
    /// `order` is `rare` (fewest sightings in the catalog first), `new` (latest first sighting
    /// first), or `score`. `rules` limits it to these notations (empty: all). Rejected patterns
    /// are left out unless `rejected`.
    pub fn job_finds(
        &self,
        job: Option<i64>,
        rules: &[String],
        order: &str,
        rejected: bool,
        limit: usize,
    ) -> Result<Vec<FindRow>, String> {
        let order = match order {
            "rare" => "p.seen ASC, p.score DESC, p.id",
            "new" | "newest" => "min(s.first_seen) DESC, p.score DESC, p.id",
            "score" => "p.score DESC, p.id",
            x => return Err(format!("unknown order '{x}' (rare, new, score)")),
        };
        let rule_filter = if rules.is_empty() {
            String::new()
        } else {
            format!(
                " AND r.notation IN ({})",
                (0..rules.len())
                    .map(|i| format!("?{}", i + 3))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let state_filter = if rejected {
            ""
        } else {
            " AND p.state != 'rejected'"
        };
        let mut args: Vec<Sql> = vec![
            job.map(Sql::Integer).unwrap_or(Sql::Null),
            Sql::Integer(limit.max(1) as i64),
        ];
        args.extend(rules.iter().map(|r| Sql::Text(r.clone())));
        self.read(vec![], |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT p.id, p.label, p.class, p.period, min(r.notation), p.tiling_name,
                        sum(s.count), p.seen, p.score, p.rating, p.state, min(s.first_seen)
                     FROM sightings s JOIN rules r ON r.id = s.rule_id
                        JOIN patterns p ON p.id = s.pattern_id
                     WHERE (?1 IS NULL OR s.job_id = ?1){rule_filter}{state_filter}
                     GROUP BY p.id ORDER BY {order} LIMIT ?2"
                ))
                .map_err(e)?;
            st.query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(FindRow {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    class: r.get::<_, String>(2)?.replace('_', " "),
                    period: r.get::<_, i64>(3)? as u64,
                    rule: r.get(4)?,
                    tiling: r.get(5)?,
                    count: r.get::<_, i64>(6)? as u64,
                    seen: r.get::<_, i64>(7)? as u64,
                    score: r.get(8)?,
                    rating: r.get::<_, Option<i64>>(9)?.map(|x| x as u8),
                    state: r.get(10)?,
                    first_seen: r.get(11)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Rate a pattern as the user (1 to 5): a `ratings` row (source `user`) and the pattern's
    /// own `rating`, which its score reads.
    pub fn rate_pattern(&self, id: &str, stars: u8, note: &str) -> Result<PatternRecord, String> {
        if !(1..=5).contains(&stars) {
            return Err("ratings are 1 to 5".into());
        }
        let rec = self.get(id)?;
        self.rate("pattern", &rec.id, "user", stars as f64, note)?;
        self.update(
            &rec.id,
            &Update {
                rating: Some(stars),
                ..Default::default()
            },
        )
    }
}
