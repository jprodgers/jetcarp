//! Explore searches as catalog jobs (`jetcarp_search::jobs::explore`): started, stopped (by the
//! budget and by the handle), and continued from the archive saved in their working database;
//! a resumed search reads its saved state, not its runs; the GPU loop on a backend; an old
//! explore database imported into sealed segments, its reviews into the library; rescoring
//! rebuilding the archive; reviews in the library.

use jetcarp_core::explore::batch::CpuBatch;
use jetcarp_search::explore::reviews;
use jetcarp_search::explore::runs::{RunFilter, Runs};
use jetcarp_search::explore::segments::{RunRec, col, cols};
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
fn an_old_database_is_imported_then_searched_on() {
    let dir = scratch("import");
    // An old search's database: a job's runs, as CART's explore databases hold them (its own
    // tables but `runs` and `meta` dropped, and a `reviews` table).
    let lib0 = Library::open(dir.join("old").join("library.sqlite"));
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
             CREATE TABLE reviews(run_id INT, verdict TEXT, note TEXT, ts INT);
             INSERT INTO reviews SELECT id, 'keep', 'nice', 5 FROM runs WHERE stage = 1 LIMIT 3;",
        )
        .unwrap();
    }
    let lib = Library::open(dir.join("new").join("library.sqlite"));
    // Segments of 100 runs, so the import writes several.
    let job = make(
        &lib,
        small(json!({"import": old.display().to_string(), "import_only": true, "seal_rows": 100})),
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
    // Every run went straight into segments of 100 (the last one shorter).
    let n = p.screened + p.confirmed;
    assert_eq!(c.manifest.rows(), n);
    assert_eq!(c.manifest.segments.len() as u64, n.div_ceil(100));
    assert!(
        c.manifest
            .segments
            .iter()
            .rev()
            .skip(1)
            .all(|s| s.rows == 100)
    );
    assert_eq!(count(&c.db, "SELECT COUNT(*) FROM runs"), 0);
    // The reviews went to the library.
    let rv = lib.run_reviews(&c.uuid()).unwrap();
    assert_eq!(rv.len(), 3);
    assert!(
        rv.iter()
            .all(|r| r.source.starts_with("import:") && r.verdict == "keep")
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
    let newer: Vec<i64> = c
        .scan(1 << col::ID, &|_, _, _| true, &|b, out| {
            out.extend((0..b.len).map(|i| b.id(i)).filter(|i| *i > old_max))
        })
        .unwrap();
    assert_eq!(
        newer.len() as u64,
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

/// An old database from a small search's runs (CART's layout: no archive tables, a `reviews`
/// table), with `tamper` applied to it. Returns its path and its runs as they were measured.
fn old_database(dir: &std::path::Path, tamper: &str) -> (PathBuf, Vec<RunRec>) {
    let lib0 = Library::open(dir.join("old").join("library.sqlite"));
    let j0 = make(&lib0, small(json!({})));
    Runner::new(lib0.clone())
        .run(j0, &opts(2, Budget::items(240)))
        .unwrap();
    let src = db_path(&lib0, &lib0.job(j0).unwrap()).unwrap();
    let old = dir.join("old.sqlite");
    {
        let c = Connection::open(&src).unwrap();
        c.execute("VACUUM INTO ?1", [old.display().to_string()])
            .unwrap();
    }
    let c = Connection::open(&old).unwrap();
    c.execute_batch(
        "DROP TABLE elites; DROP TABLE confirmed; DROP TABLE coverage; DROP TABLE families;
         DROP TABLE seen; DELETE FROM meta;
         CREATE TABLE reviews(run_id INT, verdict TEXT, note TEXT, ts INT);",
    )
    .unwrap();
    let measured = old_runs(&c);
    c.execute_batch(tamper).unwrap();
    (old, measured)
}

/// The runs as text, every field (so a NaN measure compares equal to itself).
fn text(r: &[RunRec]) -> Vec<String> {
    r.iter().map(|x| format!("{x:?}")).collect()
}

fn old_runs(c: &Connection) -> Vec<RunRec> {
    let mut st = c
        .prepare(&format!("SELECT {} FROM runs ORDER BY id", store::RUN_COLS))
        .unwrap();
    st.query_map([], |r| store::rec_at(r, 0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect()
}

/// Every run of an imported search, in id order.
fn imported(lib: &Library, job: i64) -> Vec<RunRec> {
    let c = db(lib, job);
    let mut ids = c
        .scan(1 << col::ID, &|_, _, _| true, &|b, out| {
            out.extend((0..b.len).map(|i| b.id(i)))
        })
        .unwrap();
    ids.sort_unstable();
    c.get(&ids).unwrap()
}

#[test]
fn an_import_re_measures_the_tilings_whose_runs_do_not_reproduce() {
    let dir = scratch("remeasure");
    // As if an older engine had measured the hex runs: their scores and settling times are off.
    let (old, measured) = old_database(
        &dir,
        "UPDATE runs SET score = score + 0.25, settle = settle + 1 WHERE tiling = 'hex';",
    );
    let tampered = old_runs(&Connection::open(&old).unwrap());
    let hex = measured.iter().filter(|r| r.tiling == "hex").count() as u64;
    assert!(hex > 20 && hex < measured.len() as u64);
    let lib = Library::open(dir.join("new").join("library.sqlite"));
    let job = make(
        &lib,
        small(json!({"import": old.display().to_string(), "import_only": true, "seal_rows": 100})),
    );
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::default()))
        .unwrap();
    let p = progress(&out.progress);
    assert_eq!(p.note.as_deref(), Some("the import is done"));
    let m = p.remeasure.clone().unwrap();
    assert_eq!((m.mode.as_str(), m.phase.as_str()), ("auto", "done"));
    assert_eq!(m.tilings, 2);
    assert_eq!(
        m.tilings_remeasured
            .iter()
            .map(|x| x.0.as_str())
            .collect::<Vec<_>>(),
        ["hex"]
    );
    assert_eq!((m.runs, m.changed), (hex, hex));
    assert!(m.tilings_remeasured[0].1.contains("measured differently"));
    // The manifest says what was checked and re-measured.
    let r = db(&lib, job).manifest.remeasure.clone().unwrap();
    assert_eq!((r.mode.as_str(), r.tilings, r.done), ("auto", 2, true));
    assert!(r.sampled > 32 && r.sampled <= 64 && r.sampled_differ > 0);
    assert_eq!(r.remeasured.len(), 1);
    assert_eq!(
        (r.remeasured[0].tiling.as_str(), r.remeasured[0].runs),
        ("hex", hex)
    );
    assert_eq!((r.runs, r.changed, r.backend.as_str()), (hex, hex, "cpu"));
    // The same ids; the square runs copied as they were, the hex runs measured again: as their
    // search first measured them, the tampering undone.
    let got = imported(&lib, job);
    assert_eq!(got.len(), measured.len());
    let (g, was, t) = (text(&got), text(&measured), text(&tampered));
    for (i, r) in got.iter().enumerate() {
        assert_eq!(r.id, measured[i].id);
        if r.tiling == "hex" {
            assert_ne!(t[i], was[i], "run {}", r.id);
            assert_eq!(g[i], was[i], "run {}", r.id);
        } else {
            assert_eq!(g[i], t[i], "run {}", r.id);
        }
    }
    // The archive was rebuilt from the measures as they are now.
    consistent(&db(&lib, job), &p);
}

#[test]
fn an_import_re_measures_one_family_when_only_its_runs_do_not_reproduce() {
    let dir = scratch("remeasure-family");
    // As if the way one rule family runs had changed: only the square tiling's Cyclic runs are
    // off (each family's first run is in the sample, so the change is found).
    let (old, measured) = old_database(
        &dir,
        "UPDATE runs SET settle = settle + 1 WHERE tiling = 'square' AND fam = 'cyclic';",
    );
    let cyclic = measured
        .iter()
        .filter(|r| r.tiling == "square" && r.fam == "cyclic")
        .count() as u64;
    assert!(cyclic > 0);
    let lib = Library::open(dir.join("new").join("library.sqlite"));
    let job = make(
        &lib,
        small(json!({"import": old.display().to_string(), "import_only": true})),
    );
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::default()))
        .unwrap();
    let m = progress(&out.progress).remeasure.unwrap();
    assert_eq!((m.runs, m.changed), (cyclic, cyclic));
    let r = db(&lib, job).manifest.remeasure.unwrap();
    assert_eq!(r.remeasured.len(), 1);
    let t = &r.remeasured[0];
    assert_eq!(
        (t.tiling.as_str(), t.family.as_deref(), t.runs),
        ("square", Some("cyclic"), cyclic)
    );
    assert!(
        t.why
            .ends_with("all cyclic: its cyclic runs are re-measured"),
        "{}",
        t.why
    );
    // Every run as first measured: the Cyclic ones run again, the rest copied.
    assert_eq!(text(&imported(&lib, job)), text(&measured));
}

#[test]
fn an_import_can_re_measure_every_run_or_none_and_on_a_batch_backend() {
    let dir = scratch("remeasure-modes");
    let (old, measured) = old_database(&dir, "UPDATE runs SET settle = settle + 1;");
    let tampered = old_runs(&Connection::open(&old).unwrap());
    let n = measured.len() as u64;
    let run = |name: &str, mode: &str, gpu: bool| {
        let lib = Library::open(dir.join(name).join("library.sqlite"));
        let job = make(
            &lib,
            small(
                json!({"import": old.display().to_string(), "import_only": true,
                         "remeasure": mode, "gpu": gpu}),
            ),
        );
        let mut r = if gpu {
            ExploreRun::with_backend(Box::new(CpuBatch))
        } else {
            ExploreRun::default()
        };
        let out = Runner::new(lib.clone())
            .run_with(job, &mut r, &opts(2, Budget::default()))
            .unwrap();
        (lib, job, progress(&out.progress))
    };
    // None: as they were.
    let (lib, job, p) = run("none", "none", false);
    assert_eq!(p.remeasure.as_ref().map(|m| m.runs), Some(0));
    assert_eq!(text(&imported(&lib, job)), text(&tampered));
    assert_eq!(
        db(&lib, job).manifest.remeasure.unwrap().mode.as_str(),
        "none"
    );
    // All: every run, on the CPU (no check has shown the backend measures as the CPU does).
    let (lib, job, p) = run("all", "all", true);
    let m = p.remeasure.unwrap();
    assert_eq!((m.runs, m.changed), (n, n));
    assert_eq!(text(&imported(&lib, job)), text(&measured));
    let r = db(&lib, job).manifest.remeasure.unwrap();
    assert_eq!((r.backend.as_str(), r.runs), ("cpu", n));
    assert_eq!(
        r.remeasured
            .iter()
            .map(|x| x.tiling.as_str())
            .collect::<Vec<_>>(),
        ["hex", "square"]
    );
    // Auto with a batch backend: it measures the sample as the CPU does, so it re-measures the
    // rules it runs.
    let (lib, job, p) = run("auto-gpu", "auto", true);
    let m = p.remeasure.unwrap();
    assert_eq!((m.runs, m.changed), (n, n));
    assert!(
        m.backend.starts_with("CPU batches (it measured all"),
        "{}",
        m.backend
    );
    assert_eq!(text(&imported(&lib, job)), text(&measured));
    // A remeasure mode needs an import.
    let e = jobs::create(
        &lib,
        &JobSpec {
            strategy: "explore".into(),
            spec: small(json!({"remeasure": "all"})),
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap_err();
    assert!(e.contains("remeasure needs an import"), "{e}");
}

/// Still lifes and oscillators of Life, as records to add to a library.
fn life_records() -> Vec<jetcarp_core::pattern::PatternRecord> {
    use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, record_from_object};
    use jetcarp_core::*;
    let prov = Provenance {
        source: "another importer".into(),
        jetcarp_version: VERSION.into(),
        date: jetcarp_core::pattern::now_utc(),
        ..Default::default()
    };
    let mut out = Vec::new();
    for rows in ["oo/oo", "ooo", ".o./o.o/.o."] {
        let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1).unwrap();
        let g = w.topo.lattice().unwrap().clone();
        let cells = place_rows(&g, rows, 20, 20);
        w.apply(Command::AddCa {
            ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
        })
        .unwrap();
        let found = find_objects(&w, &ObjectOptions::default()).unwrap();
        out.extend(
            found
                .objects
                .iter()
                .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
                .map(|o| {
                    record_from_object(&w, w.cas[0].id, o, prov.clone(), DiscoveredBy::User)
                        .unwrap()
                }),
        );
    }
    out
}

#[test]
fn an_import_and_another_importer_share_a_library() {
    let dir = scratch("two-importers");
    let (old, measured) = old_database(
        &dir,
        "UPDATE runs SET settle = settle + 1 WHERE tiling = 'square';
         INSERT INTO reviews SELECT id, 'keep', '', 5 FROM runs WHERE stage = 1 LIMIT 4;",
    );
    let recs = life_records();
    assert_eq!(recs.len(), 3);
    let path = dir.join("shared").join("library.sqlite");
    let lib = Library::open(&path);
    let job = make(
        &lib,
        small(json!({"import": old.display().to_string(), "import_only": true, "seal_rows": 50})),
    );
    // Another process's importer, as another connection to the same file, adding patterns (each
    // add its own write transaction) the whole time the import runs.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let other = {
        let (done, path, recs) = (done.clone(), path.clone(), recs.clone());
        std::thread::spawn(move || {
            let lib2 = Library::open(&path);
            let mut adds = 0;
            while !done.load(std::sync::atomic::Ordering::SeqCst) || adds < 30 {
                for r in &recs {
                    lib2.add(r.clone()).unwrap();
                    adds += 1;
                }
            }
            adds
        })
    };
    let out = Runner::new(lib.clone())
        .run(job, &opts(2, Budget::default()))
        .unwrap();
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let adds = other.join().unwrap();
    let p = progress(&out.progress);
    assert_eq!(p.note.as_deref(), Some("the import is done"));
    // Both landed: every run (the square ones measured again), the reviews, and the patterns.
    let m = p.remeasure.unwrap();
    let square = measured.iter().filter(|r| r.tiling == "square").count() as u64;
    assert_eq!((m.runs, m.changed), (square, square));
    assert_eq!(text(&imported(&lib, job)), text(&measured));
    let c = db(&lib, job);
    assert_eq!(lib.run_reviews(&c.uuid()).unwrap().len(), 4);
    assert!(
        lib.run_reviews(&c.uuid())
            .unwrap()
            .iter()
            .all(|r| r.source == "import:old.sqlite")
    );
    assert!(adds >= 30);
    let q = lib.query(&jetcarp_store::Query::default()).unwrap();
    assert_eq!(q.len(), 3);
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
