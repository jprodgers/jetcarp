//! Ratings in the library (table `ratings`): a user's or a model's 1-5 rating of a pattern, an
//! ant run, or a Quick Start entry, and the verdicts (keep, maybe, drop) people give an explore
//! search's runs.
//!
//! **Search runs.** A verdict on run `R` of the explore search with uuid `U` is a row with
//! `target_kind = 'search_run'` and `target_id = 'U:R'`, its `verdict`, the rating that stands for
//! it (keep 5, maybe 3, drop 1), its rater (a name, `user`, or `model:<name>`), note, and
//! `source` (`review` or `ratings:<folder>`). They live in the library, not in the search's folder, so the
//! folder can be deleted or shared without them.

use super::*;

/// One row of `ratings`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RatingRow {
    pub id: i64,
    pub target_kind: String,
    pub target_id: String,
    pub rater: String,
    pub rating: f64,
    /// keep, maybe, or drop (search runs).
    pub verdict: Option<String>,
    pub note: String,
    /// Where it came from (search runs: `review` or `ratings:<folder>`).
    pub source: String,
    pub created: String,
}

/// The verdicts on a search's runs, worst last.
pub const VERDICTS: [&str; 3] = ["keep", "maybe", "drop"];

/// The rating that stands for a verdict: keep 5, maybe 3, drop 1.
pub fn verdict_rating(verdict: &str) -> Option<f64> {
    match verdict {
        "keep" => Some(5.0),
        "maybe" => Some(3.0),
        "drop" => Some(1.0),
        _ => None,
    }
}

/// The target id of run `run` of search `search`: `<uuid>:<run id>`.
pub fn search_run_target(search: &str, run: i64) -> String {
    format!("{search}:{run}")
}

/// A verdict on a run of an explore search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunReview {
    /// The search's uuid.
    pub search: String,
    pub run: i64,
    /// keep, maybe, or drop.
    pub verdict: String,
    pub note: String,
    pub rater: String,
    pub source: String,
    /// When (UTC, `2026-11-11T12:00:00Z`).
    pub created: String,
}

/// Which reviews [`Library::add_run_reviews`] skips as already recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dedupe {
    /// None: every one is recorded.
    None,
    /// One with a review of the same run from the same source (a ratings folder read again).
    SameSource,
    /// One that is recorded already, every field the same (a ratings folder read again).
    Exact,
}

impl Library {
    /// Rate a pattern, an ant run, or a Quick Start entry (`find:` optional), 1 to 5. `rater` is
    /// `user` or `model:<name>`. Returns the rating's id.
    pub fn rate(
        &self,
        target_kind: &str,
        target_id: &str,
        rater: &str,
        rating: f64,
        note: &str,
    ) -> Result<i64, String> {
        if !(1.0..=5.0).contains(&rating) {
            return Err("ratings are 1 to 5".into());
        }
        if rater != "user" && !rater.starts_with("model:") {
            return Err(format!("rater is user or model:<name>, not '{rater}'"));
        }
        let (kind, id) = match target_kind {
            "quickstart" => (
                "quickstart",
                target_id.trim().trim_start_matches("find:").to_string(),
            ),
            "pattern" => ("pattern", target_id.trim().to_string()),
            "ant" => ("ant", self.ant(target_id)?.id),
            x => return Err(format!("target is pattern, quickstart, or ant, not '{x}'")),
        };
        let now = now_utc();
        self.write(|tx| rate_in(tx, kind, &id, rater, rating, note, &now))
    }

    /// Ratings, oldest first (of one kind of target when given).
    pub fn ratings(&self, target_kind: Option<&str>) -> Result<Vec<RatingRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT id, target_kind, target_id, rater, rating, verdict, note, source,
                        created FROM ratings
                     WHERE ?1 IS NULL OR target_kind = ?1 ORDER BY id",
                )
                .map_err(e)?;
            st.query_map([target_kind], |r| {
                Ok(RatingRow {
                    id: r.get(0)?,
                    target_kind: r.get(1)?,
                    target_id: r.get(2)?,
                    rater: r.get(3)?,
                    rating: r.get(4)?,
                    verdict: r.get(5)?,
                    note: r.get(6)?,
                    source: r.get(7)?,
                    created: r.get(8)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }

    /// Record verdicts on search runs in one transaction, skipping those `dedupe` names.
    /// Returns (recorded, skipped).
    pub fn add_run_reviews(
        &self,
        reviews: &[RunReview],
        dedupe: Dedupe,
    ) -> Result<(u64, u64), String> {
        for r in reviews {
            if verdict_rating(&r.verdict).is_none() {
                return Err(format!(
                    "a verdict is keep, maybe, or drop, not '{}'",
                    r.verdict
                ));
            }
        }
        self.write(|tx| {
            let (mut done, mut skipped) = (0, 0);
            for r in reviews {
                let target = search_run_target(&r.search, r.run);
                let dup = match dedupe {
                    Dedupe::None => false,
                    Dedupe::SameSource => tx
                        .prepare_cached(
                            "SELECT 1 FROM ratings WHERE target_kind = 'search_run'
                             AND target_id = ?1 AND source = ?2",
                        )
                        .map_err(e)?
                        .exists(params![target, r.source])
                        .map_err(e)?,
                    Dedupe::Exact => tx
                        .prepare_cached(
                            "SELECT 1 FROM ratings WHERE target_kind = 'search_run'
                             AND target_id = ?1 AND source = ?2 AND verdict = ?3 AND note = ?4
                             AND rater = ?5 AND created = ?6",
                        )
                        .map_err(e)?
                        .exists(params![
                            target, r.source, r.verdict, r.note, r.rater, r.created
                        ])
                        .map_err(e)?,
                };
                if dup {
                    skipped += 1;
                    continue;
                }
                tx.prepare_cached(
                    "INSERT INTO ratings (target_kind, target_id, rater, rating, verdict, note,
                        source, created)
                     VALUES ('search_run', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(e)?
                .execute(params![
                    target,
                    r.rater,
                    verdict_rating(&r.verdict),
                    r.verdict,
                    r.note,
                    r.source,
                    r.created
                ])
                .map_err(e)?;
                done += 1;
            }
            Ok((done, skipped))
        })
    }

    /// Every verdict on the runs of search `search`, oldest first.
    pub fn run_reviews(&self, search: &str) -> Result<Vec<RunReview>, String> {
        // Target ids `<uuid>:<run>`: the range from `<uuid>:` to `<uuid>;` (`;` follows `:`).
        let lo = format!("{search}:");
        let hi = format!("{search};");
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT target_id, verdict, note, rater, source, created FROM ratings
                     WHERE target_kind = 'search_run' AND target_id >= ?1 AND target_id < ?2
                       AND verdict IS NOT NULL
                     ORDER BY id",
                )
                .map_err(e)?;
            let rows = st
                .query_map(params![lo, hi], |r| {
                    let t: String = r.get(0)?;
                    Ok(RunReview {
                        search: search.to_string(),
                        run: t
                            .rsplit(':')
                            .next()
                            .and_then(|x| x.parse().ok())
                            .unwrap_or(0),
                        verdict: r.get(1)?,
                        note: r.get(2)?,
                        rater: r.get(3)?,
                        source: r.get(4)?,
                        created: r.get(5)?,
                    })
                })
                .map_err(e)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(e)
        })
    }

    /// Verdicts on the runs of search `search`, per verdict (in name order).
    pub fn run_review_counts(&self, search: &str) -> Result<Vec<(String, u64)>, String> {
        let mut n: std::collections::BTreeMap<String, u64> = Default::default();
        for r in self.run_reviews(search)? {
            *n.entry(r.verdict).or_default() += 1;
        }
        Ok(n.into_iter().collect())
    }

    /// The runs of search `search` with a verdict (this one, or any).
    pub fn reviewed_runs(&self, search: &str, verdict: Option<&str>) -> Result<Vec<i64>, String> {
        let mut v: Vec<i64> = self
            .run_reviews(search)?
            .into_iter()
            .filter(|r| verdict.is_none_or(|x| r.verdict == x))
            .map(|r| r.run)
            .collect();
        v.sort_unstable();
        v.dedup();
        Ok(v)
    }
}

fn rate_in(
    tx: &Transaction,
    kind: &str,
    id: &str,
    rater: &str,
    rating: f64,
    note: &str,
    now: &str,
) -> Result<i64, String> {
    tx.execute(
        "INSERT INTO ratings (target_kind, target_id, rater, rating, note, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kind, id, rater, rating, note, now],
    )
    .map_err(e)?;
    Ok(tx.last_insert_rowid())
}
