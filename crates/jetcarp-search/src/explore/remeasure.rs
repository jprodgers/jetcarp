//! Measures of older runs, replayed. A run stored before a measure existed (JACA's sign measures,
//! JETCARP 0.4.2) lacks it; runs replay exactly from their candidates (tiling, rule, seed, size,
//! generations and random state), so [`fill`] runs such a run again on the processor and keeps
//! what it measured in the search folder's `remeasured.sqlite` ([`FILE`]), so a refit or a rescore
//! replays each run once. The search's own records are never rewritten.

use crate::explore::archive::Eval;
use crate::explore::job::replay_on;
use crate::explore::store::{feat_blob, feat_from_blob};
use jetcarp_core::explore::features::Features;
use jetcarp_core::explore::space::{Fam, Grid};
use rayon::prelude::*;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The cache's file name in a search folder.
pub const FILE: &str = "remeasured.sqlite";

fn e(x: rusqlite::Error) -> String {
    x.to_string()
}

/// Does a run lack a measure this JETCARP takes (a JACA run without sign measures)?
pub fn lacks(ev: &Eval) -> bool {
    ev.cand.fam == Fam::Jaca && ev.feat.signs.is_none()
}

/// A search folder's cache of replayed measures: per run, the measures its replay gave (the
/// blob [`feat_blob`] writes), whether they differ from the stored ones (but for the measures the
/// run lacked), the JETCARP that replayed it, and when.
pub struct Remeasured {
    db: Connection,
}

impl Remeasured {
    /// Open (and make) the cache of the search folder `dir`.
    pub fn open(dir: &Path) -> Result<Remeasured, String> {
        let p = dir.join(FILE);
        let db = Connection::open(&p).map_err(|x| format!("{}: {x}", p.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(30))
            .map_err(e)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS measures(id INTEGER PRIMARY KEY, feats BLOB NOT NULL,
               differs INT NOT NULL, engine_version TEXT NOT NULL, ts INT NOT NULL);",
        )
        .map_err(e)?;
        Ok(Remeasured { db })
    }

    /// The cached measures of these runs (those cached), by id.
    pub fn get(&self, ids: &[i64]) -> Result<HashMap<i64, Features>, String> {
        let mut out = HashMap::new();
        let mut st = self
            .db
            .prepare_cached("SELECT feats FROM measures WHERE id = ?1")
            .map_err(e)?;
        for &id in ids {
            let b: Option<Vec<u8>> = st
                .query_row([id], |r| r.get(0))
                .map(Some)
                .or_else(|x| match x {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    x => Err(x),
                })
                .map_err(e)?;
            if let Some(b) = b {
                out.insert(id, feat_from_blob(&b));
            }
        }
        Ok(out)
    }

    /// Every cached run's measures, by id.
    pub fn all(&self) -> Result<HashMap<i64, Features>, String> {
        let mut st = self
            .db
            .prepare("SELECT id, feats FROM measures")
            .map_err(e)?;
        st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, feat_from_blob(&r.get::<_, Vec<u8>>(1)?)))
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }

    /// Keep these runs' replayed measures, with whether each differs from its stored ones.
    pub fn put(&mut self, rows: &[(i64, Features, bool)]) -> Result<(), String> {
        let tx = self.db.transaction().map_err(e)?;
        {
            let mut st = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO measures(id, feats, differs, engine_version, ts)
                     VALUES(?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(e)?;
            let ts = crate::explore::store::now();
            for (id, f, d) in rows {
                st.execute(params![id, feat_blob(f), *d as i64, jetcarp_core::VERSION, ts])
                    .map_err(e)?;
            }
        }
        tx.commit().map_err(e)
    }
}

/// What [`fill`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filled {
    /// Runs that lacked measures, those found in the cache, and those replayed now.
    pub lacking: u64,
    pub cached: u64,
    pub replayed: u64,
    /// Replayed runs whose other measures differ from the stored ones (0 when runs replay
    /// exactly), and runs that could not be replayed (they keep lacking).
    pub differ: u64,
    pub failed: u64,
}

/// Give the runs that lack measures ([`lacks`]) theirs: from the search folder `dir`'s cache, or,
/// with `replay`, replayed now on the current thread pool and cached. A run's stored measures
/// stay as they were; it gains only the ones it lacked. `progress(done, total)` hears of the
/// replays and returns false to stop them (those done so far are cached).
pub fn fill(
    dir: &Path,
    runs: &mut [(i64, Eval)],
    replay: bool,
    progress: &(dyn Fn(u64, u64) -> bool + Sync),
) -> Result<Filled, String> {
    let mut out = Filled::default();
    let want: Vec<i64> = runs
        .iter()
        .filter(|(_, ev)| lacks(ev))
        .map(|(id, _)| *id)
        .collect();
    out.lacking = want.len() as u64;
    if want.is_empty() {
        return Ok(out);
    }
    let mut cache = Remeasured::open(dir)?;
    let mut have = cache.get(&want)?;
    out.cached = have.len() as u64;
    if replay {
        // Grouped by grid, so each grid is built once.
        let mut groups: HashMap<(String, u32), Vec<(i64, Eval)>> = HashMap::new();
        for (id, ev) in runs.iter() {
            if lacks(ev) && !have.contains_key(id) {
                groups
                    .entry((ev.cand.tiling.clone(), ev.cand.cells))
                    .or_default()
                    .push((*id, ev.clone()));
            }
        }
        let total: u64 = groups.values().map(|g| g.len() as u64).sum();
        let done = AtomicU64::new(0);
        let stop = AtomicBool::new(false);
        let groups: Vec<_> = groups.into_iter().collect();
        let got: Vec<(i64, Option<(Features, bool)>)> = groups
            .par_iter()
            .flat_map_iter(|((tiling, cells), list)| {
                let grid = Grid::build(tiling, *cells).ok();
                let mut v = Vec::with_capacity(list.len());
                for (id, ev) in list {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let r = grid.as_ref().and_then(|g| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            replay_on(g, &ev.cand)
                        }))
                        .ok()?
                        .ok()
                    });
                    let r = r.map(|x| {
                        let mut mine = x.feat;
                        mine.signs = ev.feat.signs;
                        (x.feat, mine != ev.feat)
                    });
                    v.push((*id, r));
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if !progress(n, total) {
                        stop.store(true, Ordering::Relaxed);
                    }
                }
                v
            })
            .collect();
        let mut keep = Vec::new();
        for (id, r) in got {
            match r {
                Some((f, d)) => {
                    out.replayed += 1;
                    out.differ += d as u64;
                    keep.push((id, f, d));
                    have.insert(id, f);
                }
                None => out.failed += 1,
            }
        }
        cache.put(&keep)?;
        if stop.load(Ordering::Relaxed) {
            return Err(format!(
                "replaying was stopped after {} of {total} runs (those are kept for next time)",
                out.replayed + out.failed
            ));
        }
    }
    for (id, ev) in runs.iter_mut() {
        if lacks(ev)
            && let Some(f) = have.get(id)
        {
            ev.feat.signs = f.signs;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explore::job::{Search, Settings, StepLimits};
    use crate::explore::store::Saved;
    use std::time::{Duration, Instant};

    /// Runs stored without sign measures gain them by replay, the same as they were measured,
    /// and from the cache the next time.
    #[test]
    fn older_runs_gain_their_sign_measures_once() {
        let set = Settings {
            tilings: vec!["square".into(), "hex".into()],
            families: vec![Fam::Jaca],
            screen_cells: 900,
            screen_gens: 300,
            seed: 4,
            ..Settings::default()
        };
        let s = Search::new(set, Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let lim = StepLimits {
            until: Instant::now() + Duration::from_secs(20),
            end: None,
            stop: &stop,
            max_screened: Some(24),
        };
        assert!(s.step(&pool, None, &lim).is_none());
        let rows = s.take_delta().rows;
        assert!(rows.len() >= 24);
        let truth: HashMap<i64, Features> = rows.iter().map(|r| (r.id, r.ev.feat)).collect();
        let mut old: Vec<(i64, Eval)> = rows
            .iter()
            .map(|r| {
                let mut ev = r.ev.clone();
                ev.feat.signs = None;
                (r.id, ev)
            })
            .collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(format!("remeasure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut again = old.clone();
        let f = fill(&dir, &mut old, false, &|_, _| true).unwrap();
        assert_eq!(
            (f.lacking as usize, f.cached, f.replayed),
            (rows.len(), 0, 0)
        );
        assert!(old.iter().all(|(_, ev)| ev.feat.signs.is_none()));
        let f = fill(&dir, &mut old, true, &|_, _| true).unwrap();
        assert_eq!(
            (f.replayed as usize, f.differ, f.failed),
            (rows.len(), 0, 0)
        );
        for (id, ev) in &old {
            assert_eq!(ev.feat, truth[id], "run {id}");
        }
        let f = fill(&dir, &mut again, true, &|_, _| true).unwrap();
        assert_eq!((f.cached as usize, f.replayed), (rows.len(), 0));
        assert!(again.iter().all(|(id, ev)| ev.feat == truth[id]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
