//! Ratings in the catalog (table `ratings`): a user's or a model's 1-5 rating of a pattern, an
//! ant run, or a Quick Start entry.

use super::*;

/// One row of `ratings`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RatingRow {
    pub id: i64,
    pub target_kind: String,
    pub target_id: String,
    pub rater: String,
    pub rating: f64,
    pub note: String,
    pub created: String,
}

impl Library {
    /// Rate a pattern, a setting, or a Quick Start entry, 1 to 5. `rater` is `user` or
    /// `model:<name>`. A Quick Start entry made from a setting (`setting-<n>`) is rated as the
    /// setting. Returns the rating's id.
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
            "setting" => ("setting", parse_setting_id(target_id)?.to_string()),
            "quickstart" => {
                let t = target_id.trim().trim_start_matches("find:");
                match t.strip_prefix("setting-") {
                    Some(n) => ("setting", parse_setting_id(n)?.to_string()),
                    None => ("quickstart", t.to_string()),
                }
            }
            "pattern" => ("pattern", target_id.trim().to_string()),
            "ant" => ("ant", self.ant(target_id)?.id),
            x => {
                return Err(format!(
                    "target is pattern, setting, quickstart, or ant, not '{x}'"
                ));
            }
        };
        if kind == "setting" {
            self.setting(&id)?;
        }
        let now = now_utc();
        self.write(|tx| rate_in(tx, kind, &id, rater, rating, note, &now))
    }

    /// Ratings, oldest first (of one kind of target when given).
    pub fn ratings(&self, target_kind: Option<&str>) -> Result<Vec<RatingRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT id, target_kind, target_id, rater, rating, note, created FROM ratings
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
                    note: r.get(5)?,
                    created: r.get(6)?,
                })
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)
        })
    }
}

pub(super) fn rate_in(
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
