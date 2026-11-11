//! The evolutionary glider search as a catalog job: small searches that find gliders and store
//! them, and a job stopped and resumed that ends as an uninterrupted one. (The search itself is
//! tested in jetcarp-core.)

use jetcarp_core::evolve::{EvolveSpec, evolve};
use jetcarp_core::pattern::PatternClass;
use jetcarp_core::topology::TilingSpec;
use jetcarp_search::evolve::{FoundMover, glider_records};
use jetcarp_store::Library;

/// Run (or resume) an evolve job here on the job runner, `on_generation` seeing each generation;
/// with the search's result when it finished.
fn run_evolve(
    lib: &Library,
    job: i64,
    on_generation: Option<jetcarp_search::jobs::evolve::GenHook>,
    o: &jetcarp_search::jobs::RunOptions,
) -> (
    jetcarp_search::jobs::Outcome,
    Option<jetcarp_search::jobs::evolve::EvolveResult>,
) {
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut s = jetcarp_search::jobs::evolve::EvolveRun::default();
    s.on_generation = on_generation;
    s.result = Some(slot.clone());
    let out = jetcarp_search::jobs::Runner::new(lib.clone())
        .run_with(job, &mut s, o)
        .unwrap();
    let res = slot.lock().unwrap().take();
    (out, res)
}

#[test]
fn square_search_finds_an_exact_spaceship() {
    let lib_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-evolve-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&lib_dir);
    let spec = EvolveSpec {
        tiling: TilingSpec::new("square"),
        pop: 256,
        gens: 60,
        seed: 1,
        champions: 4,
        stop_on_glider: true,
        ..Default::default()
    };
    let rep = evolve(&spec, &|_| true).unwrap();
    assert!(!rep.gliders.is_empty(), "no glider in 60 generations");
    // Classified, nothing stored: an exact spaceship.
    let found: Vec<FoundMover> = glider_records(&rep, 1000, false)
        .unwrap()
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert!(found.iter().any(|f| f.kind == "spaceship"), "{found:?}");

    // As a job: the same search, its gliders stored with the final write and credited to the
    // job (not to manual finds, job 0), the progress saying which were new.
    use jetcarp_search::jobs::RunOptions;
    use jetcarp_search::jobs::evolve::{self as job, EvolveJobSpec};
    let jlib = Library::open(lib_dir.join("job"));
    let js = EvolveJobSpec {
        track_gens: 1000,
        ..EvolveJobSpec::new(spec.clone())
    };
    let j = job::start(&jlib, &js).unwrap();
    let (out, res) = run_evolve(&jlib, j, None, &RunOptions::default());
    let res = res.unwrap();
    assert_eq!(res.report.fingerprint(), rep.fingerprint());
    let ids = |found: &[FoundMover]| {
        let mut v: Vec<String> = found.iter().flat_map(|f| f.ids.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    assert_eq!(ids(&res.found), ids(&found));
    let mut credited: Vec<String> = jlib
        .job_sightings(j)
        .unwrap()
        .into_iter()
        .map(|s| s.0)
        .collect();
    credited.sort();
    credited.dedup();
    assert_eq!(credited, ids(&found));
    assert!(jlib.job_sightings(0).unwrap().is_empty());
    let added: usize = out.progress["movers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["added"].as_array().unwrap().len())
        .sum();
    assert_eq!(added, credited.len(), "{}", out.progress);
    let ships: Vec<_> = jlib
        .list()
        .unwrap()
        .into_iter()
        .filter(|r| r.class == PatternClass::Spaceship)
        .collect();
    assert!(!ships.is_empty());
    for s in &ships {
        assert!(s.rule.starts_with("Priority"));
        assert!(s.verified.periods >= 4);
    }
    let _ = std::fs::remove_dir_all(&lib_dir);
}

#[test]
fn hat_search_finds_a_mover() {
    // The published hat result, in a small budget: a glider rule on the hat whose glider the
    // tracker verifies across patch levels.
    let lib_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-evolve-hat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&lib_dir);
    let lib = Library::open(&lib_dir);
    let spec = EvolveSpec {
        tiling: TilingSpec::new("hat"),
        pop: 256,
        gens: 80,
        seed: 21,
        champions: 4,
        stop_on_glider: true,
        ..Default::default()
    };
    let rep = evolve(&spec, &|_| true).unwrap();
    assert!(!rep.gliders.is_empty(), "no glider on the hat");
    // Classified and made ready to store, then stored.
    let found = glider_records(&rep, 5000, true).unwrap();
    let (m, recs) = found
        .iter()
        .find(|f| f.0.kind == "mover")
        .unwrap_or_else(|| panic!("{:?}", found.iter().map(|f| &f.0).collect::<Vec<_>>()));
    let t = m.tracker.as_ref().unwrap();
    assert!(t.verified && t.reach.rings >= 100, "{}", t.summary);
    for r in recs {
        lib.add(r.clone()).unwrap();
    }
    let rec = lib.get(&m.ids[0]).unwrap();
    assert_eq!(rec.class, PatternClass::Mover);
    assert!(rec.mover.as_ref().unwrap().distance_rings >= 100);
    let _ = std::fs::remove_dir_all(&lib_dir);
}

#[test]
fn evolve_job_resumed_equals_uninterrupted() {
    use jetcarp_search::jobs::evolve::{self as job, EvolveJobSpec};
    use jetcarp_search::jobs::{Budget, RunOptions, Stopped};
    use jetcarp_store::JobStatus;
    use std::sync::{Arc, Mutex};
    let spec = small("square", 48, 10, 5);
    let whole = evolve(&spec, &|_| true).unwrap();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-evolve-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(dir.join("catalog"));
    let js = EvolveJobSpec {
        folder: Some(dir.join("run")),
        ..EvolveJobSpec::new(spec.clone())
    };
    let j = job::start(&lib, &js).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let hook: job::GenHook = Arc::new(move |g| s2.lock().unwrap().push(g.generation));
    // Three generations, then four more from a new catalog handle (as another process would),
    // then the rest.
    for (n, upto) in [(3, 2), (4, 6)] {
        let (out, res) = run_evolve(
            &Library::open(lib.path()),
            j,
            Some(hook.clone()),
            &RunOptions {
                budget: Budget::items(n),
                ..Default::default()
            },
        );
        assert!(res.is_none());
        assert_eq!(
            (out.status, out.stopped),
            (JobStatus::Paused, Some(Stopped::Budget))
        );
        assert_eq!(out.progress["generation"].as_u64(), Some(upto));
        assert!(lib.job_checkpoint(j).unwrap().is_some());
        let s = &jetcarp_search::dashboard::summaries(
            &[lib.job(j).unwrap()],
            jetcarp_search::dashboard::now_seconds(),
        )[0];
        assert!(s.can_resume() && s.done == upto && s.total == Some(10));
    }
    let (out, res) = run_evolve(&lib, j, Some(hook), &RunOptions::default());
    assert_eq!((out.status, out.stopped), (JobStatus::Done, None));
    assert_eq!(*seen.lock().unwrap(), (0..10).collect::<Vec<usize>>());
    let res = res.unwrap();
    assert_eq!(res.report.fingerprint(), whole.fingerprint());
    assert_eq!(res.report.gliders.len(), whole.gliders.len());
    assert_eq!(out.progress["generation"].as_u64(), Some(10));
    assert_eq!(
        out.progress["gliders"].as_u64(),
        Some(whole.gliders.len() as u64)
    );
    let names: Vec<String> = res
        .files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "fitness.csv",
            "champions.json",
            "gliders.json",
            "run.json",
            "movers.json"
        ]
    );
    assert!(res.files.iter().all(|f| f.exists()));
    let _ = std::fs::remove_dir_all(&dir);
}

fn small(tiling: &str, pop: usize, gens: usize, seed: u64) -> EvolveSpec {
    EvolveSpec {
        tiling: TilingSpec::new(tiling),
        pop,
        gens,
        seed,
        radius: 24,
        horizon: 1200,
        champions: 4,
        verify_factors: vec![1, 2],
        ..Default::default()
    }
}
