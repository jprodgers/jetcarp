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
//! **Searches as jobs.** [`Library::store_search_in`] stores a bounded soup search's records
//! into a job that already exists (the runner's `search` strategy), with its status and
//! progress in the same transaction; [`Library::store_search`] makes the job first.

use super::super::search::SearchOutput;
use super::{
    AddOutcome, AntRow, JobStatus, Library, PatternRecord, Sighting, StoredSearch, add_in,
    ants::put_ant, e, name_of, now_utc, prepare, sightings_in,
};
use rayon::prelude::*;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

/// Version 7: who runs a job, and when it last said so.
pub(super) const MIGRATION_JOB_LEASE: &str = r#"
ALTER TABLE jobs ADD COLUMN owner TEXT;
ALTER TABLE jobs ADD COLUMN heartbeat INTEGER;
"#;

/// What a runner's step writes (see [`Library::job_step`]).
#[derive(Clone, Debug, Default)]
pub struct StepWrite<'a> {
    /// Pattern records, each with the soups it appeared in (credited to the job). Records not
    /// yet verified are verified first.
    pub patterns: Vec<(PatternRecord, u64)>,
    /// Sightings of patterns already stored.
    pub sightings: &'a [Sighting],
    pub ants: &'a [AntRow],
    pub progress: Option<&'a Value>,
    pub checkpoint: Option<&'a [u8]>,
    pub status: Option<JobStatus>,
    /// The heartbeat to record (seconds since 1970).
    pub heartbeat: i64,
}

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

    /// One runner step in one transaction: the records, sightings, and ant runs it found, then
    /// the job's progress, checkpoint, status (each only when given), and heartbeat, all only
    /// while `owner` holds the job. Returns one result per record, in order; an error (nothing
    /// written) when the job has another owner now.
    pub fn job_step(
        &self,
        job: i64,
        owner: &str,
        w: StepWrite<'_>,
    ) -> Result<Vec<Result<AddOutcome, String>>, String> {
        let ready: Vec<(Result<PatternRecord, String>, u64)> = w
            .patterns
            .into_par_iter()
            .map(|(r, s)| (prepare(r), s))
            .collect();
        let now = now_utc();
        self.write(|tx| {
            let n = tx
                .execute(
                    "UPDATE jobs SET updated = ?2,
                        status = coalesce(?3, status),
                        progress = coalesce(?4, progress),
                        checkpoint = coalesce(?5, checkpoint),
                        heartbeat = ?6,
                        finished = CASE WHEN ?3 IN ('done', 'failed') THEN ?2 ELSE finished END
                     WHERE id = ?1 AND owner IS ?7",
                    params![
                        job,
                        now,
                        w.status.map(|s| name_of(&s)),
                        w.progress.map(|p| p.to_string()),
                        w.checkpoint,
                        w.heartbeat,
                        owner
                    ],
                )
                .map_err(e)?;
            if n == 0 {
                let held: Option<Option<String>> = tx
                    .query_row("SELECT owner FROM jobs WHERE id = ?1", [job], |r| r.get(0))
                    .optional()
                    .map_err(e)?;
                return Err(match held {
                    None => format!("no job {job}"),
                    Some(o) => format!(
                        "job {job} is now held by {} (this runner lost it); nothing was written",
                        o.as_deref().unwrap_or("nobody")
                    ),
                });
            }
            let mut out = Vec::with_capacity(ready.len());
            for (r, soups) in ready {
                out.push(match r {
                    Ok(r) => Ok(add_in(tx, r, job, soups)?),
                    Err(x) => Err(x),
                });
            }
            sightings_in(tx, job, w.sightings, &now)?;
            for a in w.ants {
                put_ant(tx, a, job, &now)?;
            }
            Ok(out)
        })
    }

    /// Store a soup search's verified records into an existing job (each pattern's copies and
    /// soups as its sightings), and mark the job done with its counts, in one transaction.
    pub fn store_search_in(&self, job: i64, out: &SearchOutput) -> Result<StoredSearch, String> {
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
