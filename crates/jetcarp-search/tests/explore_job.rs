//! Explore searches as catalog jobs (`jetcarp_search::jobs::explore`): started, stopped (by the
//! budget and by the handle), and continued from the archive saved in their working database;
//! a resumed search reads its saved state, not its runs; the GPU loop on a backend; rescoring
//! rebuilding the archive; reviews in the library.

use jetcarp_core::explore::batch::CpuBatch;
use jetcarp_search::explore::reviews;
use jetcarp_search::explore::runs::{RunFilter, Runs};
use jetcarp_search::explore::segments::{col, cols};
use jetcarp_search::explore::store;
use jetcarp_search::jobs::explore::{ExploreProgress, ExploreRun, db_path};
use jetcarp_search::jobs::{self, Budget, JobSpec, RunOptions, Runner, Stopped};
use jetcarp_store::{JobStatus, Library};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("explore-job-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small, quick search: two tilings, three families, small grids, short runs.
fn small(extra: Value) -> Value {
    let mut v = json!({
        "tilings": ["square", "hex"],
        "families": ["life_like", "generations", "cyclic"],
        "screen_cells": 400, "screen_gens": 48,
        "confirm_cells": 900, "confirm_gens": 64, "confirm_seeds": 2,
        "batch": 48, "seed": 7,
    });
    if let (Some(a), Some(b)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in b {
            a.insert(k.clone(), x.clone());
        }
    }
    v
}

fn make(lib: &Library, spec: Value) -> i64 {
    jobs::create(
        lib,
        &JobSpec {
            strategy: "explore".into(),
            spec,
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap()
}

fn opts(threads: usize, budget: Budget) -> RunOptions {
    RunOptions {
        threads,
        budget,
        ..Default::default()
    }
}

fn db(lib: &Library, job: i64) -> Runs {
    Runs::open_read(&db_path(lib, &lib.job(job).unwrap()).unwrap()).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
        .unwrap()
        .unwrap_or(0)
}

/// The runs and working database agree with themselves and with the progress.
fn consistent(r: &Runs, p: &ExploreProgress) {
    let c = &r.db;
    let screened = r.count(Some(1)).unwrap();
    assert_eq!(screened, p.screened, "screened runs");
    assert_eq!(r.count(Some(2)).unwrap(), p.confirmed);
    assert_eq!(count(c, "SELECT SUM(n) FROM coverage") as u64, screened);
    assert_eq!(count(c, "SELECT SUM(runs) FROM families") as u64, screened);
    assert_eq!(count(c, "SELECT COUNT(*) FROM elites") as u64, p.elites);
    // Every elite is a screened run, its stored candidate that run's, and at most three share a
    // niche.
    let elites: Vec<(i64, String, String)> = c
        .prepare("SELECT id, rule, tiling FROM elites")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    let ids: Vec<i64> = elites.iter().map(|e| e.0).collect();
    let got = r
        .fetch(&ids, cols(&[col::STAGE, col::RULE, col::TILING]))
        .unwrap();
    for (id, rule, tiling) in &elites {
        let run = &got[id];
        assert_eq!(
            (run.stage, &run.rule, &run.tiling),
            (1, rule, tiling),
            "elite {id}"
        );
    }
    assert!(
        count(
            c,
            "SELECT MAX(k) FROM (SELECT COUNT(*) AS k FROM elites GROUP BY fam, grp, bin)"
        ) <= 3
    );
    // Every confirmation run's parent is marked confirmed.
    let parents: Vec<i64> = r
        .scan(
            cols(&[col::STAGE, col::PARENT]),
            &|_, _, _| true,
            &|b, out| {
                out.extend(
                    (0..b.len)
                        .filter(|&i| b.stage(i) == 2)
                        .map(|i| b.i64(col::PARENT, i)),
                )
            },
        )
        .unwrap();
    let confirmed: std::collections::HashSet<i64> = c
        .prepare("SELECT id FROM confirmed")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert!(parents.iter().all(|p| confirmed.contains(p)));
    let blobs: Vec<Vec<u8>> = c
        .prepare("SELECT data FROM seen")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    let seen: usize = blobs.iter().map(|b| b.len() / 8).sum();
    assert_eq!(seen as u64, p.seen, "seen identities");
    assert!(
        p.seen >= p.screened + p.confirmed,
        "{} {}",
        p.seen,
        p.screened
    );
}

fn progress(v: &Value) -> ExploreProgress {
    serde_json::from_value(v.clone()).unwrap()
}

#[test]
fn explore_jobs_stop_and_continue_from_their_archive() {
    let dir = scratch("continue");
    let lib = Library::open(dir.join("library.sqlite"));
    let job = make(&lib, small(json!({})));
    let row = lib.job(job).unwrap();
    assert_eq!(
        (row.kind.as_str(), row.status),
        ("explore", JobStatus::Paused)
    );
    let folder = jobs::kind("explore").work_dir(&lib, &row).unwrap();
    assert!(
        folder.starts_with(dir.join("searches")),
        "{}",
        folder.display()
    );
    assert!(!jobs::kind("explore").deterministic());
    // To its item budget: about 150 screened runs (a round under way runs to its end).
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(150)))
        .unwrap();
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Budget))
    );
    let p1 = progress(&out.progress);
    assert!((150..400).contains(&p1.screened), "{}", p1.screened);
    assert!(p1.elites > 0 && p1.niches > 0 && p1.elites >= p1.niches);
    assert_eq!(p1.backend, "cpu only");
    assert_eq!(p1.loaded.as_ref().unwrap().elites, 0);
    assert!(
        p1.line.starts_with(&format!("explore {job}: ")),
        "{}",
        p1.line
    );
    assert!(folder.join("search.sqlite").is_file());
    consistent(&db(&lib, job), &p1);
    // The summary the dashboard shows.
    let s = jobs::kind("explore").summary(&lib.job(job).unwrap());
    assert_eq!(
        (s.unit.as_str(), s.done, s.total),
        ("runs", p1.screened, None)
    );
    assert!(!s.deterministic && s.resumable && s.can_resume());
    assert!(
        s.counters
            .iter()
            .any(|(k, v)| k == "elites" && !v.is_empty())
    );
    assert!(!s.bars.is_empty());
    // On a thread of its own until asked to stop.
    let h = Runner::new(Library::open(lib.path()))
        .spawn(job, opts(2, Budget::default()))
        .unwrap();
    let t0 = Instant::now();
    while progress(&h.progress()).screened < p1.screened + 100 {
        assert!(t0.elapsed() < Duration::from_secs(120), "no progress");
        std::thread::sleep(Duration::from_millis(20));
    }
    h.stop();
    let out = h.wait().unwrap();
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked))
    );
    let p2 = progress(&out.progress);
    assert!(p2.screened >= p1.screened + 100);
    // It loaded what the first run saved.
    let l = p2.loaded.clone().unwrap();
    assert_eq!((l.elites, l.runs_read), (p1.elites, p1.elites));
    assert_eq!(l.seen, p1.seen);
    assert!(p2.elites >= p1.elites && p2.niches >= p1.niches);
    assert!(p2.elapsed_s > p1.elapsed_s);
    consistent(&db(&lib, job), &p2);
    // And once more, by a time budget.
    let out = Runner::new(lib.clone())
        .run(
            job,
            &opts(
                2,
                Budget {
                    minutes: Some(0.02),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    assert_eq!(out.stopped, Some(Stopped::Budget));
    let p3 = progress(&out.progress);
    assert!(p3.screened > p2.screened);
    assert_eq!(p3.loaded.as_ref().unwrap().elites, p2.elites);
    consistent(&db(&lib, job), &p3);
}

#[test]
fn resume_reads_the_saved_state_not_every_run() {
    let dir = scratch("saved-state");
    let lib = Library::open(dir.join("library.sqlite"));
    let job = make(&lib, small(json!({})));
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(300)))
        .unwrap();
    let p1 = progress(&out.progress);
    let c = db(&lib, job);
    let coverage = count(&c.db, "SELECT SUM(n) FROM coverage");
    let elites = count(&c.db, "SELECT COUNT(*) FROM elites");
    drop(c);
    // Every run that is not an elite is deleted: a search that rebuilt its state from its runs
    // would now see a fraction of its coverage and counters.
    let path = db_path(&lib, &lib.job(job).unwrap()).unwrap();
    {
        let w = Connection::open(&path).unwrap();
        let gone = w
            .execute(
                "DELETE FROM runs WHERE id NOT IN (SELECT id FROM elites)",
                [],
            )
            .unwrap();
        assert!(gone > 50, "{gone} {elites}");
    }
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(1)))
        .unwrap();
    let p2 = progress(&out.progress);
    let l = p2.loaded.clone().unwrap();
    // It read the elites' runs and nothing else of `runs`.
    assert_eq!((l.elites as i64, l.runs_read as i64), (elites, elites));
    assert_eq!(l.seen, p1.seen);
    assert!(l.coverage_rows > 0);
    assert!(l.seconds < 5.0, "{}", l.seconds);
    // The counters, coverage, and family totals went on from the saved ones.
    assert!(p2.screened > p1.screened);
    assert!(p2.elites >= p1.elites);
    let c = db(&lib, job);
    assert_eq!(
        count(&c.db, "SELECT SUM(n) FROM coverage") as u64,
        coverage as u64 + (p2.screened - p1.screened)
    );
    assert_eq!(
        count(&c.db, "SELECT SUM(runs) FROM families") as u64,
        p2.screened
    );
}

#[test]
fn the_gpu_loop_runs_on_a_backend() {
    let dir = scratch("backend");
    let lib = Library::open(dir.join("library.sqlite"));
    let job = make(&lib, small(json!({"gpu": true})));
    let mut run = ExploreRun::with_backend(Box::new(CpuBatch));
    let out = Runner::new(lib.clone())
        .run_with(job, &mut run, &opts(2, Budget::items(400)))
        .unwrap();
    let p = progress(&out.progress);
    assert_eq!(p.backend, "CPU batches");
    assert!(p.screened >= 400 && p.gpu_error.is_none(), "{p:?}");
    let c = db(&lib, job);
    consistent(&c, &p);
    // The batch loop ran the families a GPU runs, and each run it measured replays to the same
    // measures on the CPU's own loop.
    let ids = c
        .ids(
            &RunFilter {
                order: Some("id".into()),
                ..Default::default()
            },
            40,
        )
        .unwrap();
    let rows = c.rows(&ids).unwrap();
    assert!(!rows.is_empty());
    for r in rows {
        let ev = jetcarp_search::explore::job::try_one(r.cand.clone()).unwrap();
        assert_eq!(
            (store::fate_name(ev.fate), ev.bin as i64),
            (r.fate.as_str(), r.bin),
            "run {}",
            r.id
        );
        assert_eq!(store::feat_blob(&ev.feat), r.feats, "run {}", r.id);
        assert!((ev.score as f64 - r.score).abs() < 1e-6, "run {}", r.id);
    }
}

#[test]
fn rescoring_rebuilds_the_archive_and_reviews_are_kept() {
    let dir = scratch("rescore");
    let lib = Library::open(dir.join("library.sqlite"));
    let job = make(&lib, small(json!({})));
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(200)))
        .unwrap();
    let p = progress(&out.progress);
    let path = db_path(&lib, &lib.job(job).unwrap()).unwrap();
    let mut runs = Runs::open(&path).unwrap();
    // Scores knocked out of shape, then recomputed from the stored features.
    let before: Vec<(i64, f64)> = runs
        .db
        .prepare("SELECT id, score FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    runs.db.execute("UPDATE runs SET score = -1", []).unwrap();
    let groups = jetcarp_search::explore::job::tiling_groups();
    let g = |k: &str| groups.get(k).copied().unwrap_or(0);
    let r = runs.rescore(&g, &|_| true).unwrap();
    assert_eq!(
        (r.runs as usize, r.changed as usize),
        (before.len(), before.len())
    );
    assert_eq!((r.elites, r.niches), (p.elites, p.niches));
    let after: Vec<(i64, f64)> = runs
        .db
        .prepare("SELECT id, score FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(after, before);
    // Again: nothing changes, nothing is written.
    let r = runs.rescore(&g, &|_| true).unwrap();
    assert_eq!((r.changed, r.elites, r.sidecars), (0, p.elites, 0));
    // Reviews (in the library): one at a time, and a folder of rating files (as the rating page
    // saves them).
    let id: i64 = runs
        .db
        .query_row("SELECT MIN(id) FROM runs WHERE stage = 1", [], |r| r.get(0))
        .unwrap();
    let c = &runs;
    reviews::add_review(&lib, c, id, "keep", "busy", "user", "review").unwrap();
    assert!(reviews::add_review(&lib, c, id, "great", "", "user", "review").is_err());
    assert!(reviews::add_review(&lib, c, -5, "keep", "", "user", "review").is_err());
    let rd = dir.join("ratings");
    std::fs::create_dir_all(&rd).unwrap();
    std::fs::write(
        rd.join(format!("{}.json", id + 1)),
        r#"{"ts": 1794168214909, "verdict": "drop"}"#,
    )
    .unwrap();
    std::fs::write(
        rd.join(format!("{}.json", id + 2)),
        r#"{"data": {"verdict": "maybe"}}"#,
    )
    .unwrap();
    std::fs::write(rd.join("99999999.json"), r#"{"verdict": "keep"}"#).unwrap();
    std::fs::write(rd.join("12.json"), "not json").unwrap();
    let got = reviews::import_ratings(&lib, c, &rd, "").unwrap();
    assert_eq!(
        (got.files, got.recorded, got.unknown_runs, got.unreadable),
        (4, 2, 1, 1)
    );
    // Again: nothing new.
    let again = reviews::import_ratings(&lib, c, &rd, "").unwrap();
    assert_eq!((again.recorded, again.already), (0, 2));
    assert_eq!(
        reviews::review_counts(&lib, &c.uuid()).unwrap(),
        vec![("drop".into(), 1), ("keep".into(), 1), ("maybe".into(), 1)]
    );
    let drop = lib
        .run_reviews(&c.uuid())
        .unwrap()
        .into_iter()
        .find(|r| r.verdict == "drop")
        .unwrap();
    assert_eq!(drop.created, jetcarp_core::pattern::utc_of(1794168214));
    assert_eq!(drop.run, id + 1);
    // A folder is named by its name only, never where it is on this disk.
    assert_eq!(drop.source, "ratings:ratings");
    // Another folder of the same name, with a rating of the same run made later: recorded; the
    // first folder read again: nothing new.
    let rd2 = dir.join("round2").join("ratings");
    std::fs::create_dir_all(&rd2).unwrap();
    std::fs::write(
        rd2.join(format!("{}.json", id + 1)),
        r#"{"ts": 1794200000000, "verdict": "keep"}"#,
    )
    .unwrap();
    let second = reviews::import_ratings(&lib, c, &rd2, "Emily Teich").unwrap();
    assert_eq!((second.recorded, second.already), (1, 0));
    let again = reviews::import_ratings(&lib, c, &rd, "").unwrap();
    assert_eq!((again.recorded, again.already), (0, 2));
    // Every rating's target names its run: `<search uuid>:<run id>`, and that run is this one.
    let rows = lib.ratings(Some("search_run")).unwrap();
    assert_eq!(rows.len(), 4);
    for row in &rows {
        let (search, run) = row.target_id.split_once(':').unwrap();
        assert_eq!(search, c.uuid());
        let run: i64 = run.parse().unwrap();
        let got = c.get(&[run]).unwrap();
        assert_eq!(got.len(), 1, "{}", row.target_id);
        assert_eq!(got[0].id, run);
    }
    let later = rows.iter().find(|r| r.rater == "Emily Teich").unwrap();
    assert_eq!(
        (
            later.target_id.clone(),
            later.verdict.as_deref(),
            later.rating
        ),
        (format!("{}:{}", c.uuid(), id + 1), Some("keep"), 5.0)
    );
    let _ = store::FORMAT;
}
