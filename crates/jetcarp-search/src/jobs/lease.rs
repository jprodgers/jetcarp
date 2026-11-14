//! A runner's hold on a job ([`Lease`]).
//!
//! **The lock.** A job's lock file is `<catalog>-jobs/<id>.lock` beside the catalog file (as
//! SQLite keeps `<catalog>-wal` there). A runner holds an exclusive lock on it
//! (`File::try_lock`) for as long as it runs the job: a second runner, in this process or
//! another, cannot take it, and when a process dies the system lets go of its locks, so a job
//! left `running` by a crash can be resumed at once (no waiting for its heartbeat to go stale).
//! The lock files stay after a job pauses (deleting a lock file that someone else may be
//! opening is a race); a finished job's is removed.
//!
//! **The columns.** While it holds the lock, the runner writes `jobs.owner` (`host:pid`) and
//! `jobs.heartbeat` (seconds since 1970), renews the heartbeat every [`HEARTBEAT_S`] seconds,
//! and clears both when it lets go. They tell other processes who runs the job and that it is
//! alive (the dashboard reads the heartbeat: a job with long batches is not shown as stalled),
//! and a step-based run writes only while the owner is still its own
//! (`Library::job_step`).

use jetcarp_store::Library;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;

/// Seconds between heartbeats.
pub const HEARTBEAT_S: u64 = 30;

/// This process as a job owner: `host:pid`.
pub fn owner_id() -> String {
    format!(
        "{}:{}",
        jetcarp_core::paths::host_name(),
        std::process::id()
    )
}

/// A job's lock file: `<catalog file>-jobs/<id>.lock`.
pub fn lock_path(lib: &Library, job: i64) -> PathBuf {
    let p = lib.path();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "catalog".into());
    p.with_file_name(format!("{name}-jobs"))
        .join(format!("{job}.lock"))
}

/// Whether a runner, in this process or another, holds `job`'s lock now (no lock file: no);
/// `None` when it cannot be told (a file system without locks). The test takes a shared lock
/// for a moment, so a runner taking the job at that very moment would be refused: ask only
/// when the heartbeat says the runner may be gone (`jobs.wait` does, to tell a crashed run from
/// a live one).
pub fn lock_held(lib: &Library, job: i64) -> Option<bool> {
    let file = match File::open(lock_path(lib, job)) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    match file.try_lock_shared() {
        Ok(()) => Some(false),
        Err(TryLockError::WouldBlock) => Some(true),
        Err(TryLockError::Error(_)) => None,
    }
}

/// A runner's hold on one job: the lock on its lock file, and the `owner` and `heartbeat`
/// columns. Let go (the lock released, the columns cleared) when dropped.
#[derive(Debug)]
pub struct Lease {
    lib: Library,
    job: i64,
    owner: String,
    path: PathBuf,
    file: Option<File>,
}

impl Lease {
    /// Take a job: fails, naming the holder, when another runner (here or in another process)
    /// holds it.
    pub fn acquire(lib: &Library, job: i64) -> Result<Lease, String> {
        // The job must exist (checked before anything is created beside the catalog).
        let row = lib.job(job)?;
        let (held_by, beat) = (row.owner, row.heartbeat);
        let path = lock_path(lib, job);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let who = match (held_by, beat) {
                    (Some(o), Some(t)) => format!(
                        " (by {o}, last heartbeat {} s ago)",
                        (super::now_seconds() - t).max(0)
                    ),
                    (Some(o), None) => format!(" (by {o})"),
                    _ => String::new(),
                };
                return Err(format!(
                    "job {job} is already being run{who}; stop it there first"
                ));
            }
            // A file system without locks: the columns alone say who runs it.
            Err(TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Unsupported => {}
            Err(TryLockError::Error(e)) => {
                return Err(format!("{}: {e}", path.display()));
            }
        }
        let owner = owner_id();
        let lease = Lease {
            lib: lib.clone(),
            job,
            owner,
            path,
            file: Some(file),
        };
        // We hold the lock, so whoever the columns name is gone (a crash, or a process that
        // did not let go): take the job over.
        let (now_held, _) = lib.job_lease(job)?;
        lib.set_job_lease(
            job,
            Some(&lease.owner),
            Some(super::now_seconds()),
            now_held.as_deref(),
        )?;
        Ok(lease)
    }

    /// `host:pid`.
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn job(&self) -> i64 {
        self.job
    }

    /// Renew the heartbeat. False when the columns name another owner now.
    pub fn renew(&self) -> Result<bool, String> {
        self.lib.set_job_lease(
            self.job,
            Some(&self.owner),
            Some(super::now_seconds()),
            Some(&self.owner),
        )
    }

    /// Let go; `finished`: the job is done, so its lock file goes too.
    pub fn release(mut self, finished: bool) {
        self.let_go(finished);
    }

    fn let_go(&mut self, finished: bool) {
        let Some(file) = self.file.take() else {
            return;
        };
        let _ = self
            .lib
            .set_job_lease(self.job, None, None, Some(&self.owner));
        if finished {
            // Still locked: nobody else can be running it.
            let _ = std::fs::remove_file(&self.path);
        }
        drop(file);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.let_go(false);
    }
}
