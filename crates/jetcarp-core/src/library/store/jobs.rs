//! The job runner's side of the catalog (`crate::jobs`): who runs a job, and a runner step's
//! writes in one transaction.
//!
//! **Lease columns.** `jobs.owner` (`host:pid` of the process running the job) and
//! `jobs.heartbeat` (seconds since 1970, renewed by the runner every half minute) say who runs
//! a job and that it is alive; the dashboard reads the heartbeat so a job with long batches is
//! not shown as stalled. What keeps two runners off one job is the runner's lock file
//! (`crate::jobs`); these columns make the hold visible and let a runner's step write only
//! while it still holds the job ([`Library::job_step`]).
//!
//! **Whole searches.** [`Library::store_search`] stores a bounded soup search run in one call
//! (searches made before they were jobs, tests) as a job of its own, done, its records and counts in
//! one transaction. A search run by the runner stores the same rows through its steps' writes.

use super::super::search::SearchOutput;
use super::{
    AddOutcome, AntRow, JobStatus, Library, PatternRecord, Sighting, StoredSearch, UnstoredFlush,
    add_in, ants::put_ant, census, e, name_of, now_utc, prepare, sightings_in, unstored,
};
use rayon::prelude::*;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

/// Version 7: who runs a job, and when it last said so.
pub(super) const MIGRATION_JOB_LEASE: &str = r#"
ALTER TABLE jobs ADD COLUMN owner TEXT;
ALTER TABLE jobs ADD COLUMN heartbeat INTEGER;
"#;

/// What [`Library::remove_job`] deleted.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct RemovedJob {
    pub job: i64,
    /// Sightings credited to the job.
    pub sightings: u64,
    /// With `results`: the patterns and ant runs deleted with it.
    pub patterns: Vec<String>,
    pub ants: Vec<String>,
}

/// What a runner's step writes (see [`Library::job_step`]): the rows its step put in its sink.
#[derive(Clone, Debug, Default)]
pub struct StepWrite {
    /// Pattern records, each with the soups it appeared in (credited to the job). Records not
    /// yet verified are verified first, and each gets its rule range.
    pub patterns: Vec<(PatternRecord, u64)>,
    /// Verified records stored as they are (no rule range is computed; an unverified one is
    /// refused): a hunt's finds.
    pub verified: Vec<(PatternRecord, u64)>,
    /// Sightings of patterns already stored.
    pub sightings: Vec<Sighting>,
    /// A hunt's objects counted but not stored as records (copies added, rows of objects now
    /// stored removed).
    pub unstored: UnstoredFlush,
    pub ants: Vec<AntRow>,
    /// Census rule summaries: (rule index, rule, summary).
    pub census: Vec<(usize, String, Value)>,
    pub status: Option<JobStatus>,
    /// The heartbeat to record (seconds since 1970).
    pub heartbeat: i64,
}

/// What a step's write ends with, made once its rows are in (so it can count what they did):
/// the job's progress and, when given, its new checkpoint.
pub type StepEnd = (Value, Option<Vec<u8>>);

impl Library {
    /// A job's lease: its owner (`host:pid`) and last heartbeat (seconds since 1970).
    pub fn job_lease(&self, job: i64) -> Result<(Option<String>, Option<i64>), String> {
        self.read((None, None), |c| {
            c.query_row(
                "SELECT owner, heartbeat FROM jobs WHERE id = ?1",
                [job],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(e)?
            .ok_or_else(|| format!("no job {job}"))
        })
    }

    /// Set a job's lease when its owner is `expect` (`None`: nobody), as one compare-and-set.
    /// `owner = None` clears it. Returns false (nothing written) when someone else holds it.
    pub fn set_job_lease(
        &self,
        job: i64,
        owner: Option<&str>,
        heartbeat: Option<i64>,
        expect: Option<&str>,
    ) -> Result<bool, String> {
        self.write(|tx| {
            let n = tx
                .execute(
                    "UPDATE jobs SET owner = ?2, heartbeat = ?3 WHERE id = ?1 AND owner IS ?4",
                    params![job, owner, heartbeat, expect],
                )
                .map_err(e)?;
            if n == 0
                && !tx
                    .prepare_cached("SELECT 1 FROM jobs WHERE id = ?1")
                    .map_err(e)?
                    .exists([job])
                    .map_err(e)?
            {
                return Err(format!("no job {job}"));
            }
            Ok(n > 0)
        })
    }

    /// One runner step in one transaction, only while `owner` holds the job (an error, nothing
    /// written, when it has another owner now): the rows of `w` (records, then sightings, a
    /// hunt's unstored objects, ant runs, census rows), then `end` with what became of the
    /// records (one result per record, `patterns` then `verified`, in order) to make the job's
    /// progress and checkpoint, written with the status (when given) and the heartbeat. An
    /// error from `end` writes nothing. Returns the progress written.
    pub fn job_step(
        &self,
        job: i64,
        owner: &str,
        w: StepWrite,
        end: impl FnOnce(&[Result<AddOutcome, String>]) -> Result<StepEnd, String>,
    ) -> Result<Value, String> {
        let ready: Vec<(Result<PatternRecord, String>, u64)> = w
            .patterns
            .into_par_iter()
            .map(|(r, s)| (prepare(r), s))
            .collect();
        let now = now_utc();
        self.write(|tx| {
            let held: Option<Option<String>> = tx
                .query_row("SELECT owner FROM jobs WHERE id = ?1", [job], |r| r.get(0))
                .optional()
                .map_err(e)?;
            match held {
                None => return Err(format!("no job {job}")),
                Some(o) if o.as_deref() != Some(owner) => {
                    return Err(format!(
                        "job {job} is now held by {} (this runner lost it); nothing was written",
                        o.as_deref().unwrap_or("nobody")
                    ));
                }
                Some(_) => {}
            }
            let mut out = Vec::with_capacity(ready.len() + w.verified.len());
            for (r, soups) in ready {
                out.push(match r {
                    Ok(r) => Ok(add_in(tx, r, job, soups)?),
                    Err(x) => Err(x),
                });
            }
            for (r, soups) in w.verified {
                out.push(if r.verified.periods == 0 {
                    Err(format!("{}: not verified", r.id))
                } else {
                    Ok(add_in(tx, r, job, soups)?)
                });
            }
            sightings_in(tx, job, &w.sightings, &now)?;
            unstored::flush_in(tx, job, &w.unstored)?;
            for a in &w.ants {
                put_ant(tx, a, job, &now)?;
            }
            census::rows_in(tx, job, &w.census)?;
            let (progress, checkpoint) = end(&out)?;
            tx.execute(
                "UPDATE jobs SET updated = ?2,
                    status = coalesce(?3, status),
                    progress = ?4,
                    checkpoint = coalesce(?5, checkpoint),
                    heartbeat = ?6,
                    finished = CASE WHEN ?3 IN ('done', 'failed') THEN ?2 ELSE finished END
                 WHERE id = ?1",
                params![
                    job,
                    now,
                    w.status.map(|s| name_of(&s)),
                    progress.to_string(),
                    checkpoint,
                    w.heartbeat,
                ],
            )
            .map_err(e)?;
            Ok(progress)
        })
    }

    /// Delete a job and what is its alone: its row, sightings, unstored rows, and census rows
    /// (the job's ant runs stay, with no job). With `results`, its finds go too: the ant runs it
    /// stored, and the patterns no other job has seen that nobody has curated (state `new`, not
    /// in the Quick Start), with their media and ratings. Patterns other jobs saw keep their
    /// records and `seen` counts. Job 0 (the manual job) cannot be deleted.
    pub fn remove_job(&self, id: i64, results: bool) -> Result<RemovedJob, String> {
        if id == 0 {
            return Err("job 0 holds the patterns added by hand and cannot be removed".into());
        }
        self.job(id)?;
        self.write(|tx| {
            let count = |sql: &str| -> Result<u64, String> {
                tx.query_row(sql, [id], |r| r.get::<_, i64>(0))
                    .map(|n| n as u64)
                    .map_err(e)
            };
            let mut out = RemovedJob {
                job: id,
                sightings: count("SELECT count(*) FROM sightings WHERE job_id = ?1")?,
                ..Default::default()
            };
            if results {
                let ids = |sql: &str| -> Result<Vec<String>, String> {
                    let mut st = tx.prepare(sql).map_err(e)?;
                    st.query_map([id], |r| r.get::<_, String>(0))
                        .map_err(e)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(e)
                };
                out.ants = ids("SELECT id FROM ants WHERE job_id = ?1")?;
                out.patterns = ids(
                    "SELECT p.id FROM patterns p WHERE p.state = 'new'
                     AND EXISTS (SELECT 1 FROM sightings s WHERE s.pattern_id = p.id AND s.job_id = ?1)
                     AND NOT EXISTS (SELECT 1 FROM sightings s WHERE s.pattern_id = p.id AND s.job_id != ?1)
                     AND NOT EXISTS (SELECT 1 FROM quickstart q WHERE q.source_kind = 'pattern'
                                     AND q.source_id = p.id)
                     ORDER BY p.id",
                )?;
                for (kind, table, list) in [
                    ("ant", "ants", &out.ants),
                    ("pattern", "patterns", &out.patterns),
                ] {
                    for x in list {
                        tx.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [x])
                            .map_err(e)?;
                        for t in ["media", "ratings"] {
                            tx.execute(
                                &format!("DELETE FROM {t} WHERE target_kind = ?1 AND target_id = ?2"),
                                params![kind, x],
                            )
                            .map_err(e)?;
                        }
                    }
                }
            }
            for sql in [
                "DELETE FROM sightings WHERE job_id = ?1",
                "DELETE FROM unstored WHERE job_id = ?1",
                "DELETE FROM census_rows WHERE job_id = ?1",
                "UPDATE ants SET job_id = NULL WHERE job_id = ?1",
                "DELETE FROM jobs WHERE id = ?1",
            ] {
                tx.execute(sql, [id]).map_err(e)?;
            }
            Ok(out)
        })
    }

    /// Store a soup search's verified records into an existing job (each pattern's copies and
    /// soups as its sightings), and mark the job done with its counts, in one transaction.
    pub(super) fn store_search_in(
        &self,
        job: i64,
        out: &SearchOutput,
    ) -> Result<StoredSearch, String> {
        let soups_total: u64 = out.report.groups.iter().map(|g| g.soups as u64).sum();
        let ready: Vec<Result<PatternRecord, String>> =
            out.records.par_iter().map(|r| prepare(r.clone())).collect();
        let soups = out
            .soups
            .iter()
            .map(|&s| s as u64)
            .chain(std::iter::repeat(0));
        let now = now_utc();
        self.write(|tx| {
            let mut st = StoredSearch {
                job,
                ..Default::default()
            };
            for ((r, o), soups) in out.records.iter().zip(ready).zip(soups) {
                let o = match o {
                    Ok(rec) => Ok(add_in(tx, rec, job, soups)?),
                    Err(x) => Err(x),
                };
                match o {
                    Ok(AddOutcome::Added) => st.added.push(r.id.clone()),
                    Ok(AddOutcome::Merged { new_rule, .. }) => {
                        if new_rule {
                            st.new_rules.push((r.id.clone(), r.rule.clone()));
                        }
                        st.merged.push(r.id.clone())
                    }
                    Err(x) => st.failed.push((r.id.clone(), x)),
                }
            }
            let progress = serde_json::json!({
                "soups": soups_total, "soups_done": soups_total,
                "records": out.records.len(), "added": st.added.len(),
            });
            let n = tx
                .execute(
                    "UPDATE jobs SET updated = ?2, status = 'done', progress = ?3, finished = ?2
                     WHERE id = ?1",
                    params![job, now, progress.to_string()],
                )
                .map_err(e)?;
            if n == 0 {
                return Err(format!("no job {job}"));
            }
            Ok(st)
        })
    }
}
