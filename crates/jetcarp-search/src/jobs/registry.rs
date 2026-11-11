//! The jobs running in this process ([`Registry`]; [`registry`] is the process's own).

use super::{JobHandle, Outcome, RunOptions, Runner, Strategy};
use crate::dashboard::JobSummary;
use jetcarp_store::Library;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

type Key = (PathBuf, i64);

/// The handles of jobs run on threads of this process, by catalog and job id. The app's
/// dashboard, the MCP server, and the front ends that start background jobs share the
/// process's [`registry`], so each sees the jobs the others run here.
#[derive(Default)]
pub struct Registry(Mutex<HashMap<Key, JobHandle>>);

fn key(lib: &Library, job: i64) -> Key {
    let p = lib.path();
    (
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()),
        job,
    )
}

/// The registry of this process.
pub fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(Registry::default)
}

impl Registry {
    /// Start (or resume) a job on a new thread, as its kind builds it. Refused while it is
    /// running here; a run of it that has just returned is waited for (it is only letting go of
    /// the job) and its result dropped.
    pub fn start(&self, lib: Library, job: i64, o: RunOptions) -> Result<(), String> {
        self.start_with(lib, job, None, o)
    }

    /// [`start`](Self::start) with a strategy the caller built (`None`: the kind's).
    pub fn start_with(
        &self,
        lib: Library,
        job: i64,
        s: Option<Box<dyn Strategy + Send>>,
        o: RunOptions,
    ) -> Result<(), String> {
        let k = key(&lib, job);
        let old = {
            let mut m = self.0.lock().unwrap();
            if m.get(&k).is_some_and(|h| h.is_running()) {
                return Err(format!("job {job} is already running here"));
            }
            m.remove(&k)
        };
        if let Some(h) = old {
            let _ = h.wait();
        }
        let r = Runner::new(lib);
        let h = match s {
            Some(s) => r.spawn_with(job, s, o)?,
            None => r.spawn(job, o)?,
        };
        self.0.lock().unwrap().insert(k, h);
        Ok(())
    }

    /// Running here and not yet ending.
    pub fn is_running(&self, lib: &Library, job: i64) -> bool {
        self.with(lib, job, |h| h.is_running()).unwrap_or(false)
    }

    /// Has a handle (running, or ended and not yet reaped).
    pub fn contains(&self, lib: &Library, job: i64) -> bool {
        self.0.lock().unwrap().contains_key(&key(lib, job))
    }

    /// Stop a job running here at its next checkpoint; false when it is not here.
    pub fn stop(&self, lib: &Library, job: i64) -> bool {
        self.with(lib, job, |h| h.stop()).is_some()
    }

    /// Look at a job's handle.
    pub fn with<R>(&self, lib: &Library, job: i64, f: impl FnOnce(&JobHandle) -> R) -> Option<R> {
        self.0.lock().unwrap().get(&key(lib, job)).map(f)
    }

    /// A job's live progress, when it has a handle here.
    pub fn progress(&self, lib: &Library, job: i64) -> Option<Value> {
        self.with(lib, job, |h| h.progress())
    }

    /// A job's live summary, when it has a handle here.
    pub fn summary(&self, lib: &Library, job: i64) -> Option<JobSummary> {
        self.with(lib, job, |h| h.summary())
    }

    /// Take out the runs of this catalog whose threads have ended, with their results.
    pub fn reap(&self, lib: &Library) -> Vec<(i64, Result<Outcome, String>)> {
        let cat = key(lib, 0).0;
        let ended: Vec<JobHandle> = {
            let mut m = self.0.lock().unwrap();
            let ks: Vec<Key> = m
                .iter()
                .filter(|(k, h)| k.0 == cat && h.is_finished())
                .map(|(k, _)| k.clone())
                .collect();
            ks.iter().filter_map(|k| m.remove(k)).collect()
        };
        ended.into_iter().map(|h| (h.job(), h.wait())).collect()
    }

    /// Wait for a job's run to end and take its handle out (`None`: no handle here).
    pub fn wait(&self, lib: &Library, job: i64) -> Option<Result<Outcome, String>> {
        let h = self.0.lock().unwrap().remove(&key(lib, job))?;
        Some(h.wait())
    }

    /// The jobs of this catalog with a handle here.
    pub fn jobs(&self, lib: &Library) -> Vec<i64> {
        let cat = key(lib, 0).0;
        let mut v: Vec<i64> = self
            .0
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.0 == cat)
            .map(|k| k.1)
            .collect();
        v.sort();
        v
    }
}
