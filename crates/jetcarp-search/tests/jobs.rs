//! The job runner (`jetcarp_search::jobs`): jobs made from a `JobSpec`, runs stopped and resumed
//! (on the runner and across catalog handles) ending exactly as uninterrupted ones, budgets that
//! stop a run cleanly, a job left `running` by a crashed process seen as stalled and resumed, two
//! runners never owning one job, a runner that lost its job writing nothing more, the registry,
//! and the bounded-search and collision strategies storing what their direct calls store.

use jetcarp_core::census::{self, CensusSpec, all_bs_rules};
use jetcarp_core::pattern::PatternRecord;
use jetcarp_core::topology::{SizeSpec, TilingSpec};
use jetcarp_search::ants::{self as antjob, AntSpec};
use jetcarp_search::dashboard;
use jetcarp_search::hunt::{self, HuntProgress, HuntSpec};
use jetcarp_search::jobs::{
    self, Budget, JobSpec, Outcome, RunOptions, Runner, Stopped, hunt::HuntRun, registry,
};
use jetcarp_store::{JobStatus, Library, Query};
use std::time::{Duration, Instant};

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-jobs-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

/// What a hunt left in a catalog: every pattern (id, rule, seen, first find) and the job's
/// sightings.
fn snapshot(lib: &Library, job: i64) -> (Vec<String>, Vec<(String, String, u64, u64)>) {
    let mut recs: Vec<PatternRecord> = lib.query(&Query::default()).unwrap();
    recs.sort_by(|a, b| a.id.cmp(&b.id));
    let rows = recs
        .iter()
        .map(|r| {
            format!(
                "{} {} {} seen {} soup {:?} rng {:?}",
                r.id,
                r.rule,
                r.class.name(),
                r.seen,
                r.provenance.soup,
                r.provenance.rng_state
            )
        })
        .collect();
    (rows, lib.job_sightings(job).unwrap())
}

fn hunt_spec(soups: Option<u64>) -> HuntSpec {
    HuntSpec {
        rules: vec!["B3/S23".into(), "B36/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        soups,
        ..Default::default()
    }
}

fn opts(threads: usize) -> RunOptions {
    RunOptions {
        threads,
        ..Default::default()
    }
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(120), "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn soups(out: &Outcome) -> u64 {
    out.progress_as::<HuntProgress>().soups_done
}

#[test]
fn job_specs_make_jobs_with_labels_and_budgets() {
    let lib = temp_lib("specs");
    let js = JobSpec {
        strategy: "hunt".into(),
        spec: serde_json::to_value(hunt_spec(None)).unwrap(),
        label: Some("survey · squares".into()),
        budget: Budget::items(80),
    };
    let job = jobs::create(&lib, &js).unwrap();
    let row = lib.job(job).unwrap();
    assert_eq!(
        (row.kind.as_str(), row.status, row.label()),
        ("hunt", JobStatus::Paused, Some("survey · squares"))
    );
    assert_eq!(Budget::of(&row), Budget::items(80));
    // The stored budget applies to a run that gives none.
    let out = Runner::new(lib.clone()).run(job, &opts(2)).unwrap();
    assert_eq!(
        (out.status, out.stopped, soups(&out)),
        (JobStatus::Paused, Some(Stopped::Budget), 80)
    );
    // Every kind with a strategy is listed and found by name.
    let names: Vec<&str> = jobs::kinds().iter().map(|k| k.kind()).collect();
    assert_eq!(
        names,
        [
            "hunt",
            "search",
            "ants",
            "census",
            "mover",
            "pile",
            "collide",
            "evolve",
            "explore",
            "reverify",
            "compounds"
        ]
    );
    // Every kind but explore resumes exactly; an explore search continues from its archive.
    for k in jobs::kinds() {
        assert_eq!(k.deterministic(), k.kind() != "explore", "{}", k.kind());
    }
    assert_eq!(jobs::kind("bench").label(), "Job");
    assert!(!jobs::kind("bench").runnable(&row));
    // Bad requests.
    let bad = |js: JobSpec| jobs::create(&lib, &js).unwrap_err();
    assert!(
        bad(JobSpec {
            strategy: "nonsense".into(),
            ..js.clone()
        })
        .contains("no strategy 'nonsense'")
    );
    assert!(
        bad(JobSpec {
            spec: serde_json::json!({"rules": 3}),
            ..js.clone()
        })
        .contains("hunt spec")
    );
    assert!(
        bad(JobSpec {
            strategy: "pile".into(),
            spec: serde_json::json!({}),
            budget: Budget::items(3),
            label: None,
        })
        .contains("minutes, not items")
    );
    assert!(
        bad(JobSpec {
            budget: Budget::minutes(-1.0),
            ..js.clone()
        })
        .contains("above 0")
    );
    // A job of a kind without a strategy cannot be run.
    let other = lib.create_job("bench", &serde_json::json!({})).unwrap();
    let e = Runner::new(lib.clone())
        .run(other, &RunOptions::default())
        .unwrap_err();
    assert!(e.contains("cannot be run"), "{e}");
}

#[test]
fn hunt_stopped_and_resumed_on_the_runner_equals_uninterrupted() {
    let spec = hunt_spec(Some(1200));
    let a = temp_lib("hunt-whole");
    let ja = hunt::start(&a, &spec).unwrap();
    let pa: HuntProgress = Runner::new(a.clone())
        .run(ja, &opts(2))
        .unwrap()
        .progress_as();
    assert_eq!((pa.status.as_str(), pa.soups_done), ("done", 1200));

    // Stopped on the way (at whatever batch it is in), then resumed by a new catalog handle.
    let b = temp_lib("hunt-stopped");
    let jb = hunt::start(&b, &spec).unwrap();
    let run = HuntRun::with_batch(40);
    let h = Runner::new(Library::open(b.path()))
        .spawn_with(jb, Box::new(run), opts(2))
        .unwrap();
    assert_eq!((h.job(), h.kind()), (jb, "hunt"));
    wait_for("80 soups", || {
        h.progress_as::<HuntProgress>().soups_done >= 80
    });
    let live = h.summary();
    assert!(live.here && live.status == "running" && live.title.contains("B3/S23"));
    h.stop();
    let out = h.wait().unwrap();
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked))
    );
    let half = soups(&out);
    assert!(half >= 80 && half < 1200, "{half}");
    assert_eq!(b.job_lease(jb).unwrap(), (None, None), "let go");
    let b2 = Library::open(b.path());
    let pb: HuntProgress = Runner::new(b2.clone())
        .run(jb, &opts(3))
        .unwrap()
        .progress_as();
    assert_eq!(pb.status, "done");
    assert_eq!(pa.groups, pb.groups);
    assert_eq!(pa.newest, pb.newest);
    assert_eq!(snapshot(&a, ja), snapshot(&b2, jb));
    // A finished job's lock file is gone; running it again changes nothing.
    assert!(!jobs::lock_path(&b2, jb).exists());
    let again = Runner::new(b2.clone()).run(jb, &opts(1)).unwrap();
    assert_eq!((again.status, again.stopped), (JobStatus::Done, None));
    assert_eq!(snapshot(&a, ja), snapshot(&b2, jb));
}

#[test]
fn budgets_stop_jobs_cleanly_and_runs_resume_from_there() {
    // Items: exactly that many soups per session.
    let lib = temp_lib("budget-items");
    let job = hunt::start(&lib, &hunt_spec(None)).unwrap();
    let r = Runner::new(lib.clone());
    let items = |n| RunOptions {
        threads: 2,
        budget: Budget::items(n),
        ..Default::default()
    };
    let out = r.run(job, &items(150)).unwrap();
    assert_eq!(
        (out.status, out.stopped, soups(&out)),
        (JobStatus::Paused, Some(Stopped::Budget), 150)
    );
    let out = r.run(job, &items(100)).unwrap();
    assert_eq!(soups(&out), 250);
    let s = &dashboard::summaries(&[lib.job(job).unwrap()], dashboard::now_seconds())[0];
    assert!(s.can_resume() && !s.stale);

    // Minutes: a hunt with no end stops soon after its time is up, paused at a checkpoint.
    let t0 = Instant::now();
    let out = r
        .run(
            job,
            &RunOptions {
                threads: 2,
                budget: Budget::minutes(0.05),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(t0.elapsed() < Duration::from_secs(60));
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Budget))
    );
    assert!(soups(&out) >= 250);
    let (_, saved) = hunt::status(&lib, job).unwrap();
    assert_eq!(
        saved.soups_done,
        soups(&out),
        "the checkpoint is the last batch"
    );

    // A census in rule budgets ends with the report of one run.
    let c = temp_lib("budget-census");
    let mut spec = CensusSpec {
        tiling: TilingSpec::rect("square", 16, 16, jetcarp_core::Boundary::Wrap),
        soups: 2,
        gens: 200,
        ..Default::default()
    };
    spec.rules = all_bs_rules(&[2, 3], &[2, 3], None).unwrap();
    let straight = census::run_census(&spec, &Default::default(), None).unwrap();
    let cj = jetcarp_search::census::start_job(&c, &spec).unwrap();
    let rc = Runner::new(c.clone());
    for (n, done) in [(5, 5), (6, 11)] {
        let out = rc.run(cj, &items(n)).unwrap();
        let p: jetcarp_search::census::CensusProgress = out.progress_as();
        assert_eq!((p.status.as_str(), p.rules_done), ("paused", done));
    }
    let out = Runner::new(Library::open(c.path()))
        .run(cj, &RunOptions::default())
        .unwrap();
    assert_eq!((out.status, out.stopped), (JobStatus::Done, None));
    let mut a = straight.report.clone();
    let mut b = jetcarp_search::census::job_report(&c, cj).unwrap();
    a.jetcarp_version.clear();
    b.jetcarp_version.clear();
    assert_eq!(a.to_json(), b.to_json());
}

#[test]
fn ant_searches_resumed_on_the_runner_equal_uninterrupted() {
    let spec = AntSpec {
        tilings: vec![TilingSpec::new("square"), TilingSpec::new("hex")],
        rules: vec![],
        strings: Some(jetcarp_core::ants::search::Strings {
            min: 2,
            max: 3,
            alphabet: None,
        }),
        steps: 40_000,
        ..Default::default()
    };
    let a = temp_lib("ants-whole");
    let ja = jobs::create(
        &a,
        &JobSpec {
            strategy: "ants".into(),
            spec: serde_json::to_value(&spec).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    let pa: antjob::AntProgress = Runner::new(a.clone())
        .run(ja, &RunOptions::default())
        .unwrap()
        .progress_as();
    let b = temp_lib("ants-steps");
    let jb = antjob::start(&b, &spec).unwrap();
    let mut done = 0;
    loop {
        let out = Runner::new(Library::open(b.path()))
            .run(
                jb,
                &RunOptions {
                    budget: Budget::items(3),
                    ..Default::default()
                },
            )
            .unwrap();
        let p: antjob::AntProgress = out.progress_as();
        assert!(p.runs_done > done);
        done = p.runs_done;
        if out.status == JobStatus::Done {
            assert_eq!(p.by_outcome, pa.by_outcome);
            assert_eq!(p.groups, pa.groups);
            assert_eq!(p.newest, pa.newest);
            break;
        }
        assert_eq!(out.stopped, Some(Stopped::Budget));
    }
    let rows = |l: &Library, j: i64| {
        let q = jetcarp_store::AntQuery {
            job: Some(j),
            ..Default::default()
        };
        let mut v: Vec<(String, String)> = l
            .ants(&q)
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.outcome.name().to_string()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(rows(&a, ja), rows(&b, jb));
}

#[test]
fn a_job_left_running_by_a_crash_is_stalled_and_resumable() {
    let spec = hunt_spec(Some(600));
    let a = temp_lib("crash-whole");
    let ja = hunt::start(&a, &spec).unwrap();
    Runner::new(a.clone()).run(ja, &opts(2)).unwrap();

    let b = temp_lib("crash");
    let jb = hunt::start(&b, &spec).unwrap();
    let r = Runner::new(b.clone());
    r.run(
        jb,
        &RunOptions {
            budget: Budget::items(200),
            ..Default::default()
        },
    )
    .unwrap();
    // The process "crashes": the job is left running, its owner gone, its lock let go.
    let dead = "deadhost:4242";
    b.update_job(jb, Some(JobStatus::Running), None, None)
        .unwrap();
    let old = dashboard::now_seconds() - 1000;
    assert!(b.set_job_lease(jb, Some(dead), Some(old), None).unwrap());
    let mut row = b.job(jb).unwrap();
    assert_eq!(
        (row.owner.as_deref(), row.heartbeat),
        (Some(dead), Some(old))
    );
    // The dashboard sees it as stalled once both its progress and heartbeat are old...
    row.updated = "2020-01-01T00:00:00Z".into();
    let now = dashboard::now_seconds();
    let s = &dashboard::summaries(std::slice::from_ref(&row), now)[0];
    assert!(s.stale && s.can_resume() && !s.is_running());
    // ...but not while a heartbeat is fresh (a run with long batches).
    row.heartbeat = Some(now - 5);
    let s = &dashboard::summaries(&[row], now)[0];
    assert!(!s.stale && s.is_running());
    // Nobody holds its lock, so a runner takes it over at once and finishes it.
    let out = Runner::new(Library::open(b.path()))
        .run(jb, &opts(2))
        .unwrap();
    assert_eq!((out.status, soups(&out)), (JobStatus::Done, 600));
    assert_eq!(snapshot(&a, ja), snapshot(&b, jb));
    assert_eq!(b.job_lease(jb).unwrap(), (None, None));
}

#[test]
fn two_runners_never_own_one_job() {
    let lib = temp_lib("two-runners");
    let job = hunt::start(&lib, &hunt_spec(None)).unwrap();
    let h = Runner::new(Library::open(lib.path()))
        .spawn(job, opts(2))
        .unwrap();
    wait_for("running", || {
        lib.job(job).unwrap().status == JobStatus::Running
    });
    let (owner, beat) = lib.job_lease(job).unwrap();
    assert_eq!(owner.as_deref(), Some(jobs::owner_id().as_str()));
    assert!(beat.is_some_and(|t| (dashboard::now_seconds() - t).abs() < 60));
    // A second runner, in this process or another (the lock is the same), is refused.
    let e = Runner::new(Library::open(lib.path()))
        .run(job, &opts(1))
        .unwrap_err();
    assert!(
        e.contains("already being run") && e.contains(&jobs::owner_id()),
        "{e}"
    );
    assert!(Runner::new(lib.clone()).spawn(job, opts(1)).is_err());
    h.stop();
    let out = h.wait().unwrap();
    assert_eq!(out.stopped, Some(Stopped::Asked));

    // Another process holding the lock file (as a runner there would) keeps this one off too.
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(jobs::lock_path(&lib, job))
        .unwrap();
    lock.try_lock().unwrap();
    lib.set_job_lease(
        job,
        Some("otherhost:77"),
        Some(dashboard::now_seconds()),
        None,
    )
    .unwrap();
    let e = Runner::new(lib.clone()).run(job, &opts(1)).unwrap_err();
    assert!(e.contains("otherhost:77"), "{e}");
    assert_eq!(lib.job(job).unwrap().status, JobStatus::Paused, "untouched");
    drop(lock);
    // Once it lets go (or dies), the job is free again.
    let out = Runner::new(lib.clone())
        .run(
            job,
            &RunOptions {
                budget: Budget::items(50),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.stopped, Some(Stopped::Budget));

    // The registry: one run of a job per process, and it waits for one that is ending.
    registry().start(lib.clone(), job, opts(2)).unwrap();
    assert!(registry().is_running(&lib, job));
    let e = registry().start(lib.clone(), job, opts(2)).unwrap_err();
    assert!(e.contains("already running here"), "{e}");
    let live = registry().summary(&lib, job).unwrap();
    assert!(live.here && live.kind == "hunt");
    assert!(registry().jobs(&lib).contains(&job));
    assert!(registry().stop(&lib, job));
    wait_for("stopped", || !registry().is_running(&lib, job));
    registry().start(lib.clone(), job, opts(2)).unwrap();
    // Another process stops it by marking it paused (once it runs: a paused job is left alone).
    wait_for("running again", || {
        lib.job(job).unwrap().status == JobStatus::Running
    });
    jobs::kind("hunt").request_stop(&lib, job).unwrap();
    let out = registry().wait(&lib, job).unwrap().unwrap();
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked))
    );
    assert!(!registry().contains(&lib, job));
}

#[test]
fn a_runner_that_lost_its_job_writes_nothing_more() {
    use jetcarp_core::evolve::EvolveSpec;
    use jetcarp_search::jobs::evolve::{EvolveJobSpec, EvolveRun};
    let lib = temp_lib("lost");
    let spec = EvolveSpec {
        tiling: TilingSpec::new("square"),
        pop: 24,
        gens: 400,
        radius: 12,
        horizon: 300,
        champions: 2,
        verify_factors: vec![1, 2],
        ..Default::default()
    };
    let job = jobs::evolve::start(&lib, &EvolveJobSpec::new(spec)).unwrap();
    let h = Runner::new(Library::open(lib.path()))
        .spawn_with(job, Box::new(EvolveRun::default()), opts(2))
        .unwrap();
    wait_for("a checkpoint", || {
        lib.job_checkpoint(job).unwrap().is_some()
    });
    // Someone takes the job over (this runner was thought dead): its next step is refused.
    assert!(
        lib.set_job_lease(
            job,
            Some("thief:1"),
            Some(dashboard::now_seconds()),
            Some(&jobs::owner_id())
        )
        .unwrap()
    );
    let before = lib.job(job).unwrap().progress["generation"].clone();
    let out = h.wait().unwrap();
    assert_eq!(out.stopped, Some(Stopped::LostLease));
    let row = lib.job(job).unwrap();
    assert_eq!(
        row.owner.as_deref(),
        Some("thief:1"),
        "the new owner keeps it"
    );
    assert_eq!(row.status, JobStatus::Running);
    let after = row.progress["generation"].clone();
    // At most the step in flight when it was taken had been written.
    assert!(
        after.as_u64().unwrap() <= before.as_u64().unwrap() + 1,
        "{before} then {after}"
    );
}

#[test]
fn bounded_searches_resumed_store_what_one_whole_search_stores() {
    use jetcarp_search::bounded::SearchSpec;
    let spec = SearchSpec {
        rules: vec!["B3/S23".into(), "B36/S23".into()],
        tilings: vec![TilingSpec::new("square"), TilingSpec::new("hex")],
        size: SizeSpec::Rect { w: 24, h: 24 },
        soups: 7,
        gens: 500,
        ..Default::default()
    };
    // Uninterrupted, as one session.
    let a = temp_lib("search-whole");
    let ja = jobs::search::start(&a, &spec).unwrap();
    let out = Runner::new(a.clone()).run(ja, &opts(3)).unwrap();
    assert_eq!((out.status, out.stopped), (JobStatus::Done, None));
    // Five soups a session, each from a new catalog handle (as another process would), then
    // the verification and the store.
    let b = temp_lib("search-sessions");
    let job = jobs::search::start(&b, &spec).unwrap();
    let mut sessions = 0;
    loop {
        let out = Runner::new(Library::open(b.path()))
            .run(
                job,
                &RunOptions {
                    threads: 3,
                    budget: Budget::items(5),
                    ..Default::default()
                },
            )
            .unwrap();
        sessions += 1;
        if out.status == JobStatus::Done {
            break;
        }
        assert_eq!(out.stopped, Some(Stopped::Budget));
        let done = out.progress["soups_done"].as_u64().unwrap();
        assert_eq!(done, (5 * sessions).min(28), "{}", out.progress);
        assert!(b.job_checkpoint(job).unwrap().is_some());
    }
    assert_eq!(sessions, 6);
    let (ra, rb) = (a.job(ja).unwrap(), b.job(job).unwrap());
    assert_eq!(ra.progress, rb.progress);
    assert_eq!(snapshot(&a, ja), snapshot(&b, job));
}

/// A block and a glider (records from a world), and a collision of the glider into the block
/// (as `tests/library.rs` collides them).
fn glider_into_block() -> (
    PatternRecord,
    PatternRecord,
    jetcarp_core::pattern::collide::CollideSpec,
) {
    use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, record_from_object};
    use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, Topology, World};
    let mut w = World::new(TilingSpec::rect("square", 96, 80, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = place_rows(&g, ".o./..o/ooo", 10, 10);
    cells.extend(place_rows(&g, "oo/oo", 40, 40));
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let objs = find_objects(&w, &ObjectOptions::default()).unwrap();
    let recs: Vec<PatternRecord> = objs
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| {
            record_from_object(
                &w,
                w.cas[0].id,
                o,
                Provenance::default(),
                DiscoveredBy::User,
            )
            .unwrap()
        })
        .collect();
    let named = |n: &str| recs.iter().find(|r| r.name.as_deref() == Some(n)).unwrap();
    let (block, glider) = (named("block").clone(), named("glider").clone());
    let topo = Topology::build(&TilingSpec::rect("square", 8, 8, Boundary::Dead)).unwrap();
    let d = glider.displacement.unwrap();
    let o = jetcarp_core::pattern::paste::orientations(&topo, glider.neighborhood)
        .iter()
        .position(|m| m.vector(d) == [-1, -1])
        .unwrap() as u32;
    let cs = jetcarp_core::pattern::collide::CollideSpec {
        dx: [-2, 5],
        dy: [3, 5],
        orientation_b: o,
        gens: 600,
        ..Default::default()
    };
    (block, glider, cs)
}

#[test]
fn collisions_resumed_equal_uninterrupted() {
    use jetcarp_search::jobs::collide::{self as collide_job, CollideJobSpec};
    let (block, glider, cs) = glider_into_block();
    let job_in = |name: &str| {
        let l = temp_lib(name);
        l.add(block.clone()).unwrap();
        l.add(glider.clone()).unwrap();
        let spec = CollideJobSpec {
            a: block.id.clone(),
            b: glider.id.clone(),
            collide: cs.clone(),
            store: true,
        };
        let j = collide_job::start(&l, &spec).unwrap();
        (l, j)
    };
    let (a, ja) = job_in("collide-whole");
    let whole = Runner::new(a.clone()).run(ja, &opts(2)).unwrap();
    assert_eq!(whole.status, JobStatus::Done);
    assert_eq!(whole.progress["runs"], 96);
    // Twenty placements a session, from new catalog handles, then the verification.
    let (b, jb) = job_in("collide-sessions");
    let mut sessions = 0;
    let last = loop {
        let out = Runner::new(Library::open(b.path()))
            .run(
                jb,
                &RunOptions {
                    threads: 3,
                    budget: Budget::items(20),
                    ..Default::default()
                },
            )
            .unwrap();
        sessions += 1;
        if out.status == JobStatus::Done {
            break out;
        }
        assert_eq!(out.stopped, Some(Stopped::Budget));
        assert_eq!(
            out.progress["runs_done"].as_u64(),
            Some((20 * sessions).min(96)),
            "{}",
            out.progress
        );
    };
    assert_eq!(sessions, 5);
    assert_eq!(last.progress, whole.progress);
    assert_eq!(b.job(jb).unwrap().progress, a.job(ja).unwrap().progress);
    assert_eq!(snapshot(&a, ja), snapshot(&b, jb));
}

#[test]
fn bounded_searches_and_collisions_store_what_their_calls_store() {
    use jetcarp_core::pattern::collide::collide;
    use jetcarp_search::bounded::{SearchSpec, search};
    let spec = SearchSpec {
        rules: vec!["B3/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        size: SizeSpec::Rect { w: 32, h: 32 },
        soups: 6,
        gens: 600,
        ..Default::default()
    };
    // The search itself (`jetcarp_search::bounded`): what the job must store.
    let out = search(&spec, None).unwrap();
    // As a job.
    let b = temp_lib("search-job");
    let job = jobs::create(
        &b,
        &JobSpec {
            strategy: "search".into(),
            spec: serde_json::to_value(&spec).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(b.job(job).unwrap().status, JobStatus::Paused);
    let res = Runner::new(b.clone()).run(job, &opts(2)).unwrap();
    assert_eq!(res.status, JobStatus::Done);
    // Every record of the search stored, each new to this catalog, with its copies and soups
    // as the job's sightings.
    let n = out.records.len() as u64;
    assert_eq!(res.progress["records"].as_u64(), Some(n));
    assert_eq!(res.progress["added"].as_u64(), Some(n));
    let mut stored: Vec<(String, u64, u64)> = b
        .job_sightings(job)
        .unwrap()
        .into_iter()
        .map(|s| (s.0, s.2, s.3))
        .collect();
    stored.sort();
    let mut want: Vec<(String, u64, u64)> = out
        .records
        .iter()
        .zip(&out.soups)
        .map(|(r, &soups)| (r.id.clone(), r.seen, soups as u64))
        .collect();
    want.sort();
    assert_eq!(stored, want);
    let rb = b.job(job).unwrap();
    let s = &dashboard::summaries(&[rb], dashboard::now_seconds())[0];
    assert_eq!(
        (s.kind_label.as_str(), s.title.as_str()),
        ("Soup search", "B3/S23")
    );

    // A glider into a block (as `tests/library.rs` collides them), the products stored in the
    // job.
    use jetcarp_search::jobs::collide::CollideJobSpec;
    let (block, glider, cs) = glider_into_block();
    b.add(block.clone()).unwrap();
    b.add(glider.clone()).unwrap();
    let direct = collide(&block, &glider, &cs, 0, None).unwrap();
    assert!(!direct.records.is_empty());
    let cjs = CollideJobSpec {
        a: block.id.clone(),
        b: glider.id.clone(),
        collide: cs.clone(),
        store: true,
    };
    let cj = jobs::create(
        &b,
        &JobSpec {
            strategy: "collide".into(),
            spec: serde_json::to_value(&cjs).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    let res = Runner::new(b.clone()).run(cj, &opts(2)).unwrap();
    assert_eq!(res.status, JobStatus::Done);
    assert_eq!(
        res.progress["products"].as_u64(),
        Some(direct.records.len() as u64)
    );
    assert_eq!(
        serde_json::to_value(&direct.report.counts).unwrap(),
        res.progress["outcomes"]
    );
    let mut stored: Vec<String> = b
        .job_sightings(cj)
        .unwrap()
        .into_iter()
        .map(|s| s.0)
        .collect();
    stored.sort();
    stored.dedup();
    let mut want: Vec<String> = direct.records.iter().map(|r| r.id.clone()).collect();
    want.sort();
    assert_eq!(stored, want);
    let s = &dashboard::summaries(&[b.job(cj).unwrap()], dashboard::now_seconds())[0];
    assert_eq!(s.kind_label, "Collisions");
    assert!(s.resumable);
    // A collide job from before collisions were strategies (no patterns named) cannot run.
    let old = b
        .create_job("collide", &serde_json::to_value(&cs).unwrap())
        .unwrap();
    assert!(!jobs::kind("collide").runnable(&b.job(old).unwrap()));
}

/// A pile job's growth table without its timings and allocations (step, grains, topplings,
/// sweeps, extent) and its picture's pixels.
fn pile_result(dir: &std::path::Path) -> (Vec<String>, Vec<u8>) {
    let csv = std::fs::read_to_string(dir.join("growth.csv")).unwrap();
    let rows = csv
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            [&f[..4], &f[5..7]].concat().join(",")
        })
        .collect();
    let png = std::fs::read(dir.join("pile.png")).unwrap();
    let img = jetcarp_core::media::encode::decode_png(&png).unwrap();
    (rows, img.rgba)
}

#[test]
fn pile_jobs_stopped_part_way_and_resumed_equal_uninterrupted() {
    use jetcarp_core::pile::{Centre, PileSpec};
    use jetcarp_core::topology::Nb;
    use jetcarp_search::pile::{self as pilejob, PileJobSpec, PileProgress};
    use std::sync::{Arc, Mutex};
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-jobs-test-pile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(dir.join("catalog"));
    let spec = |name: &str| PileJobSpec {
        pile: PileSpec {
            tiling: TilingSpec::new("square"),
            nb: Nb::Edge,
            grains: 300_000,
            centre: Centre::Cell,
        },
        folder: dir.join(name),
        px_per_cell: 1.0,
        margin: 2.0,
        palette: "Lava".into(),
        reverse: false,
        preview_side: 32,
        png: true,
        gpu: false,
        cpu: false,
        extra: serde_json::Value::Null,
    };
    let ja = pilejob::start(&lib, &spec("whole")).unwrap();
    let out = Runner::new(lib.clone())
        .run(ja, &RunOptions::default())
        .unwrap();
    assert_eq!(out.status, JobStatus::Done);
    let steps = out.progress_as::<PileProgress>().steps;
    // Stopped once a few doubling steps are saved (the step in flight is cancelled), then
    // resumed from the folder by a new catalog handle. A viewer of the run sees its board (each
    // finished step, and each second of a long one).
    let jb = pilejob::start(&lib, &spec("stopped")).unwrap();
    let boards = Arc::new(Mutex::new(Vec::<u64>::new()));
    let b2 = boards.clone();
    let o = RunOptions {
        view: Some(Arc::new(move |state: &dyn std::any::Any| {
            if let Some(b) = jetcarp_core::pile::BoardRef::from_any(state) {
                let cells = match b {
                    jetcarp_core::pile::BoardRef::Lattice(l) => l.cells(),
                    jetcarp_core::pile::BoardRef::Patch(p) => p.cells(),
                };
                b2.lock().unwrap().push(cells);
            }
        })),
        ..Default::default()
    };
    let h = Runner::new(lib.clone()).spawn(jb, o).unwrap();
    wait_for("four steps", || h.progress_as::<PileProgress>().step >= 4);
    h.stop();
    let p: PileProgress = h.wait().unwrap().progress_as();
    assert_eq!(p.status, "paused", "{p:?}");
    let seen = boards.lock().unwrap().clone();
    assert!(!seen.is_empty(), "the viewer saw no board");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert_eq!(p.last_step.as_ref().map(|r| r.step), Some(p.step));
    assert!(p.step >= 4 && p.step < steps, "{} of {steps}", p.step);
    let b2 = Library::open(lib.path());
    let out = Runner::new(b2).run(jb, &RunOptions::default()).unwrap();
    assert_eq!(out.status, JobStatus::Done);
    assert_eq!(out.progress_as::<PileProgress>().step, steps);
    assert_eq!(
        pile_result(&dir.join("whole")),
        pile_result(&dir.join("stopped"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
