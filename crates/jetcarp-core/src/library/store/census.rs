//! Census jobs' per-rule results in the catalog (table `census_rows`, see [`crate::census`]).

use super::*;

/// Store census rule summaries of a job: (rule index, rule, summary).
pub(super) fn rows_in(
    tx: &Transaction,
    job: i64,
    rows: &[(usize, String, Value)],
) -> Result<(), String> {
    for (idx, rule, summary) in rows {
        tx.prepare_cached(
            "INSERT OR REPLACE INTO census_rows (job_id, idx, rule, summary)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(e)?
        .execute(params![job, *idx as i64, rule, summary.to_string()])
        .map_err(e)?;
    }
    Ok(())
}

impl Library {
    /// Store one census rule summary (a census job's result row).
    pub fn put_census_rows(
        &self,
        job: i64,
        rows: &[(usize, String, Value)],
        progress: &Value,
        checkpoint: &[u8],
    ) -> Result<(), String> {
        let now = now_utc();
        self.write(|tx| {
            rows_in(tx, job, rows)?;
            let n = tx
                .execute(
                    "UPDATE jobs SET updated = ?2, progress = ?3, checkpoint = ?4 WHERE id = ?1",
                    params![job, now, progress.to_string(), checkpoint],
                )
                .map_err(e)?;
            if n == 0 {
                return Err(format!("no job {job}"));
            }
            Ok(())
        })
    }

    /// A census job's rule summaries, in rule order.
    pub fn census_rows(&self, job: i64) -> Result<Vec<Value>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare("SELECT summary FROM census_rows WHERE job_id = ?1 ORDER BY idx")
                .map_err(e)?;
            st.query_map([job], |r| r.get::<_, String>(0))
                .map_err(e)?
                .map(|r| {
                    r.map_err(e).and_then(|s| {
                        serde_json::from_str(&s).map_err(|x| format!("census row: {x}"))
                    })
                })
                .collect()
        })
    }
}
