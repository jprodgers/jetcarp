//! Measures of older runs, replayed. A run stored before a measure existed (JACA's sign measures,
//! JETCARP 0.4.2) lacks it; runs replay exactly from their candidates (tiling, rule, seed, size,
//! generations and random state), so [`fill`] runs such a run again on the processor and keeps
//! what it measured in the search folder's `remeasured.sqlite` ([`FILE`]), so a refit or a rescore
//! replays each run once. The search's own records are never rewritten.

use crate::explore::archive::Eval;
use crate::explore::job::{WINDOW, batch_world, evaluate_with, replay_on};
use jetcarp_core::explore::batch::{Backend, BatchJob, BatchWorld};
use jetcarp_core::explore::features::{frames_for, frames_with_trade};
use jetcarp_core::topology::Nb;
use crate::explore::store::{feat_blob, feat_from_blob};
use jetcarp_core::explore::features::Features;
use jetcarp_core::explore::space::{Fam, Grid};
use rayon::prelude::*;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Runs, by id.
type Runs = Vec<(i64, Eval)>;
/// Replays by run: the measures, and whether they differ from the stored ones (None: failed).
type Replayed = Vec<(i64, Option<(Features, bool)>)>;
/// What runs share to go in one GPU batch: tiling, cells, neighbourhood, generations.
type Batched = (String, u32, Nb, u32);

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
/// with `replay`, replayed now and cached: in batches on `gpu` when one is given (for the rules
/// it runs; a GPU batch gives exactly the trace the processor does), the rest on the current
/// thread pool. A run's stored measures stay as they were; it gains only the ones it lacked.
/// `progress(done, total)` hears of the replays and returns false to stop them (those done so far
/// are cached).
pub fn fill(
    dir: &Path,
    runs: &mut [(i64, Eval)],
    replay: bool,
    gpu: Option<&mut dyn Backend>,
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
    // A cached replay without sign measures (a cache from a build before them) does not count.
    have.retain(|_, f| f.signs.is_some());
    out.cached = have.len() as u64;
    if replay {
        let mut todo: Vec<(i64, Eval)> = runs
            .iter()
            .filter(|(id, ev)| lacks(ev) && !have.contains_key(id))
            .cloned()
            .collect();
        // By grid, so a part's runs share their grids.
        todo.sort_by(|a, b| {
            let (x, y) = (&a.1.cand, &b.1.cand);
            (&x.tiling, x.cells, x.nb as u8, x.gens, a.0).cmp(&(&y.tiling, y.cells, y.nb as u8, y.gens, b.0))
        });
        let total = todo.len() as u64;
        let done = AtomicU64::new(0);
        let stop = AtomicBool::new(false);
        let tick = || {
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if !progress(n, total) {
                stop.store(true, Ordering::Relaxed);
            }
        };
        let mut gpu = gpu;
        // A part at a time, each cached as it ends, so a stop keeps what was done.
        for part in todo.chunks(PART) {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let (on_gpu, on_cpu): (Runs, Runs) = match &gpu {
                Some(be) => part
                    .iter()
                    .cloned()
                    .partition(|(_, ev)| ev.cand.rule().is_some_and(|r| be.supports(&r))),
                None => (vec![], part.to_vec()),
            };
            let mut got: Replayed = Vec::new();
            if let Some(be) = gpu.as_deref_mut() {
                got.extend(replay_gpu(be, on_gpu, &stop, &tick)?);
            }
            got.extend(replay_cpu(on_cpu, &stop, &tick));
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
        }
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

/// Runs replayed and cached at a time.
const PART: usize = 8192;

/// Replay runs on the current thread pool, grouped by grid so each grid is built once.
fn replay_cpu(
    runs: Vec<(i64, Eval)>,
    stop: &AtomicBool,
    tick: &(dyn Fn() + Sync),
) -> Replayed {
    let mut groups: HashMap<(String, u32), Vec<(i64, Eval)>> = HashMap::new();
    for (id, ev) in runs {
        groups
            .entry((ev.cand.tiling.clone(), ev.cand.cells))
            .or_default()
            .push((id, ev));
    }
    let groups: Vec<_> = groups.into_iter().collect();
    groups
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
                v.push((*id, r.map(|x| compare(x.feat, ev))));
                tick();
            }
            v
        })
        .collect()
}

/// A replay's measures, and whether they differ from the run's stored ones (but for those it
/// lacked).
fn compare(replayed: Features, ev: &Eval) -> (Features, bool) {
    let mut mine = replayed;
    mine.signs = ev.feat.signs;
    (replayed, mine != ev.feat)
}

/// Worlds in one GPU batch of replays at most.
const GPU_BATCH: usize = 1024;

/// Replay runs in GPU batches, grouped by grid, neighbourhood and generations.
fn replay_gpu(
    be: &mut dyn Backend,
    runs: Vec<(i64, Eval)>,
    stop: &AtomicBool,
    tick: &(dyn Fn() + Sync),
) -> Result<Replayed, String> {
    let mut groups: HashMap<Batched, Runs> = HashMap::new();
    for (id, ev) in runs {
        let c = &ev.cand;
        groups
            .entry((c.tiling.clone(), c.cells, c.nb, c.gens))
            .or_default()
            .push((id, ev));
    }
    let mut keys: Vec<_> = groups.keys().cloned().collect();
    keys.sort_by(|a, b| (&a.0, a.1, a.2 as u8, a.3).cmp(&(&b.0, b.1, b.2 as u8, b.3)));
    let mut out = Vec::new();
    for k in keys {
        let list = groups.remove(&k).unwrap_or_default();
        let (tiling, cells, nb, gens) = k;
        let Ok(grid) = Grid::build(&tiling, cells) else {
            out.extend(list.iter().map(|(id, _)| (*id, None)));
            continue;
        };
        let per = (be.max_cells() / grid.topo.len().next_multiple_of(256).max(1)).clamp(1, GPU_BATCH);
        for part in list.chunks(per) {
            if stop.load(Ordering::Relaxed) {
                return Ok(out);
            }
            let worlds: Vec<Option<BatchWorld>> =
                part.par_iter().map(|(_, ev)| batch_world(&grid, &ev.cand)).collect();
            let (mut ok, mut bw) = (Vec::new(), Vec::new());
            for ((id, ev), w) in part.iter().zip(worlds) {
                match w {
                    Some(w) => {
                        ok.push((*id, ev));
                        bw.push(w);
                    }
                    None => out.push((*id, None)),
                }
            }
            if bw.is_empty() {
                continue;
            }
            let jaca = ok.iter().any(|(_, ev)| ev.cand.fam == Fam::Jaca);
            let job = BatchJob {
                topo: grid.topo.clone(),
                nb,
                worlds: bw,
                gens,
                frames_at: if jaca {
                    frames_with_trade(gens, WINDOW)
                } else {
                    frames_for(gens, WINDOW)
                },
            };
            let traces = be.run(&job)?;
            let got: Replayed = ok
                .par_iter()
                .zip(traces.par_iter())
                .map(|((id, ev), t)| {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        evaluate_with(&grid, &ev.cand, t, None)
                    }))
                    .ok()
                    .map(|(_, f, _, _)| compare(f, ev));
                    tick();
                    (*id, r)
                })
                .collect();
            out.extend(got);
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
        let f = fill(&dir, &mut old, false, None, &|_, _| true).unwrap();
        assert_eq!(
            (f.lacking as usize, f.cached, f.replayed),
            (rows.len(), 0, 0)
        );
        assert!(old.iter().all(|(_, ev)| ev.feat.signs.is_none()));
        let f = fill(&dir, &mut old, true, None, &|_, _| true).unwrap();
        assert_eq!(
            (f.replayed as usize, f.differ, f.failed),
            (rows.len(), 0, 0)
        );
        for (id, ev) in &old {
            assert_eq!(ev.feat, truth[id], "run {id}");
        }
        let mut batched = again.clone();
        let f = fill(&dir, &mut again, true, None, &|_, _| true).unwrap();
        assert_eq!((f.cached as usize, f.replayed), (rows.len(), 0));
        assert!(again.iter().all(|(id, ev)| ev.feat == truth[id]));
        let _ = std::fs::remove_dir_all(&dir);
        // In batches, as on a GPU (the processor's batch backend: the same traces).
        std::fs::create_dir_all(&dir).unwrap();
        let mut be = jetcarp_core::explore::batch::CpuBatch;
        let f = fill(&dir, &mut batched, true, Some(&mut be), &|_, _| true).unwrap();
        assert_eq!((f.replayed as usize, f.differ, f.failed), (rows.len(), 0, 0));
        for (id, ev) in &batched {
            assert_eq!(ev.feat, truth[id], "run {id} in a batch");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
