//! Explore searches as catalog jobs (`jetcarp_search::jobs::explore`): started, stopped (by the
//! budget and by the handle), and continued from the archive saved in their working database;
//! a resumed search reads its saved state, not its runs; the GPU loop on a backend; an old
//! explore database imported; rescoring rebuilding the archive.

use jetcarp_core::explore::batch::CpuBatch;
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

fn db(lib: &Library, job: i64) -> Connection {
    store::open_read(&db_path(lib, &lib.job(job).unwrap()).unwrap()).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
        .unwrap()
        .unwrap_or(0)
}

/// The working database agrees with itself and with the progress.
fn consistent(c: &Connection, p: &ExploreProgress) {
    let screened = count(c, "SELECT COUNT(*) FROM runs WHERE stage = 1") as u64;
    assert_eq!(screened, p.screened, "screened runs");
    assert_eq!(
        count(c, "SELECT COUNT(*) FROM runs WHERE stage = 2") as u64,
        p.confirmed
    );
    assert_eq!(count(c, "SELECT SUM(n) FROM coverage") as u64, screened);
    assert_eq!(count(c, "SELECT SUM(runs) FROM families") as u64, screened);
    assert_eq!(count(c, "SELECT COUNT(*) FROM elites") as u64, p.elites);
    // Every elite is a screened run, and at most three share a niche.
    assert_eq!(
        count(
            c,
            "SELECT COUNT(*) FROM elites e JOIN runs r ON r.id = e.id WHERE r.stage = 1"
        ) as u64,
        p.elites
    );
    assert!(
        count(
            c,
            "SELECT MAX(k) FROM (SELECT COUNT(*) AS k FROM elites e JOIN runs r ON r.id = e.id
             GROUP BY r.fam, e.grp, r.bin)"
        ) <= 3
    );
    // Every confirmation run's parent is marked confirmed.
    assert_eq!(
        count(
            c,
            "SELECT COUNT(DISTINCT parent) FROM runs WHERE stage = 2 AND parent NOT IN
             (SELECT id FROM confirmed)"
        ),
        0
    );
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
    let lib = Library::open(dir.join("catalog.sqlite"));
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
    let lib = Library::open(dir.join("catalog.sqlite"));
    let job = make(&lib, small(json!({})));
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(300)))
        .unwrap();
    let p1 = progress(&out.progress);
    let c = db(&lib, job);
    let coverage = count(&c, "SELECT SUM(n) FROM coverage");
    let elites = count(&c, "SELECT COUNT(*) FROM elites");
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
        count(&c, "SELECT SUM(n) FROM coverage") as u64,
        coverage as u64 + (p2.screened - p1.screened)
    );
    assert_eq!(
        count(&c, "SELECT SUM(runs) FROM families") as u64,
        p2.screened
    );
}

#[test]
fn the_gpu_loop_runs_on_a_backend() {
    let dir = scratch("backend");
    let lib = Library::open(dir.join("catalog.sqlite"));
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
    let ids: Vec<i64> = c
        .prepare("SELECT id FROM runs WHERE stage = 1 ORDER BY id LIMIT 40")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    let rows = jetcarp_search::explore::sheet::query_runs(
        &c,
        &format!(
            "SELECT id FROM runs WHERE id IN ({})",
            ids.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
    .unwrap();
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
fn an_old_database_is_imported_then_searched_on() {
    let dir = scratch("import");
    // An old search's database: a job's runs, as CART's explore databases hold them (its own
    // tables but `runs`, `reviews`, and `meta` dropped).
    let lib0 = Library::open(dir.join("old").join("catalog.sqlite"));
    let j0 = make(&lib0, small(json!({})));
    let out = Runner::new(lib0.clone())
        .run(j0, &opts(2, Budget::items(250)))
        .unwrap();
    let p0 = progress(&out.progress);
    let src = db_path(&lib0, &lib0.job(j0).unwrap()).unwrap();
    let old = dir.join("old.sqlite");
    {
        let c = Connection::open(&src).unwrap();
        c.execute("VACUUM INTO ?1", [old.display().to_string()])
            .unwrap();
    }
    {
        let c = Connection::open(&old).unwrap();
        c.execute_batch(
            "DROP TABLE elites; DROP TABLE confirmed; DROP TABLE coverage; DROP TABLE families;
             DROP TABLE seen; DELETE FROM meta;
             CREATE TABLE r2(run_id INT, verdict TEXT, note TEXT, ts INT);
             INSERT INTO r2 SELECT id, 'keep', 'nice', 5 FROM runs WHERE stage = 1 LIMIT 3;
             DROP TABLE reviews; ALTER TABLE r2 RENAME TO reviews;",
        )
        .unwrap();
    }
    let lib = Library::open(dir.join("new").join("catalog.sqlite"));
    let job = make(
        &lib,
        small(json!({"import": old.display().to_string(), "import_only": true})),
    );
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::default()))
        .unwrap();
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked))
    );
    let p = progress(&out.progress);
    assert_eq!(p.note.as_deref(), Some("the import is done"));
    assert!(p.import.as_ref().unwrap().done);
    // The rebuilt state is the old search's (the seen set holds the runs, of every candidate
    // the old search tried).
    assert_eq!(
        (p.screened, p.confirmed, p.elites, p.niches),
        (p0.screened, p0.confirmed, p0.elites, p0.niches)
    );
    assert!(p.seen <= p0.seen && p.seen >= p.screened + p.confirmed);
    let c = db(&lib, job);
    consistent(&c, &p);
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM reviews WHERE source LIKE 'import:%'"
        ),
        3
    );
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'runs_score'"
        ),
        1
    );
    drop(c);
    // Searched on from the imported archive: new ids follow the old ones.
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(60)))
        .unwrap();
    let p2 = progress(&out.progress);
    assert!(p2.screened >= p.screened + 60);
    assert_eq!(p2.loaded.as_ref().unwrap().elites, p.elites);
    let c = db(&lib, job);
    consistent(&c, &p2);
    let old_max = count(&Connection::open(&old).unwrap(), "SELECT MAX(id) FROM runs");
    assert_eq!(
        count(
            &c,
            &format!("SELECT COUNT(*) FROM runs WHERE id > {old_max}")
        ) as u64,
        (p2.screened - p.screened) + (p2.confirmed - p.confirmed)
    );
    // A job cannot import what is not an explore database.
    let bad = dir.join("not-explore.sqlite");
    Connection::open(&bad)
        .unwrap()
        .execute_batch("CREATE TABLE x(a)")
        .unwrap();
    let e = jobs::create(
        &lib,
        &JobSpec {
            strategy: "explore".into(),
            spec: small(json!({"import": bad.display().to_string()})),
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap_err();
    assert!(e.contains("not an explore database"), "{e}");
}

#[test]
fn rescoring_rebuilds_the_archive_and_reviews_are_kept() {
    let dir = scratch("rescore");
    let lib = Library::open(dir.join("catalog.sqlite"));
    let job = make(&lib, small(json!({})));
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::items(200)))
        .unwrap();
    let p = progress(&out.progress);
    let path = db_path(&lib, &lib.job(job).unwrap()).unwrap();
    let mut c = store::open(&path).unwrap();
    // Scores knocked out of shape, then recomputed from the stored features.
    let before: Vec<(i64, f64)> = c
        .prepare("SELECT id, score FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    c.execute("UPDATE runs SET score = -1", []).unwrap();
    let groups = jetcarp_search::explore::job::tiling_groups();
    let g = |k: &str| groups.get(k).copied().unwrap_or(0);
    let r = store::rescore(&mut c, &g, &|_| true).unwrap();
    assert_eq!(
        (r.runs as usize, r.changed as usize),
        (before.len(), before.len())
    );
    assert_eq!((r.elites, r.niches), (p.elites, p.niches));
    let after: Vec<(i64, f64)> = c
        .prepare("SELECT id, score FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(after, before);
    // Again: nothing changes, nothing is written.
    let r = store::rescore(&mut c, &g, &|_| true).unwrap();
    assert_eq!((r.changed, r.elites), (0, p.elites));
    // Reviews: one at a time, and a folder of rating files (as the rating page saves them).
    let id: i64 = c
        .query_row("SELECT MIN(id) FROM runs WHERE stage = 1", [], |r| r.get(0))
        .unwrap();
    store::add_review(&c, id, "keep", "busy", "user", "review").unwrap();
    assert!(store::add_review(&c, id, "great", "", "user", "review").is_err());
    assert!(store::add_review(&c, -5, "keep", "", "user", "review").is_err());
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
    let got = store::import_ratings(&mut c, &rd, "").unwrap();
    assert_eq!(
        (got.files, got.recorded, got.unknown_runs, got.unreadable),
        (4, 2, 1, 1)
    );
    // Again: nothing new.
    let again = store::import_ratings(&mut c, &rd, "").unwrap();
    assert_eq!((again.recorded, again.already), (0, 2));
    assert_eq!(
        store::review_counts(&c).unwrap(),
        vec![("drop".into(), 1), ("keep".into(), 1), ("maybe".into(), 1)]
    );
    let ts: i64 = c
        .query_row("SELECT ts FROM reviews WHERE verdict = 'drop'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(ts, 1794168214);
}
