//! Phase 2: rule census from the command line (engine checks), and the bit-parallel board
//! the census runs B/S and Generations rules on.

use jetcarp_core::board::Board;
use jetcarp_core::census::*;
use jetcarp_core::jobs::census::CensusRun;
use jetcarp_core::jobs::{Budget, RunOptions, Runner};
use jetcarp_core::*;
use std::sync::Arc;

/// Run (or resume) a census job here on the job runner: `chunk` rules per chunk (0: adapt),
/// stopping (paused) after `limit` rules in this session.
fn run_job(
    lib: &jetcarp_core::library::Library,
    job: i64,
    chunk: u64,
    limit: Option<u64>,
) -> CensusProgress {
    let o = RunOptions {
        budget: Budget {
            items: limit,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut s = CensusRun::default();
    s.chunk = chunk;
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .unwrap()
        .progress_as()
}

fn spec(rules: &[&str], soups: u32, gens: u64) -> CensusSpec {
    CensusSpec {
        tiling: TilingSpec::rect("square", 24, 24, Boundary::Wrap),
        rules: rules.iter().map(|s| s.to_string()).collect(),
        soups,
        gens,
        ..Default::default()
    }
}

fn pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .unwrap()
}

#[test]
fn census_is_identical_at_any_thread_count() {
    let mut s = spec(&[], 3, 300);
    s.rules = all_bs_rules(&[2, 3], &[2, 3], None).unwrap();
    s.rules.push("B2/S/C3".into());
    let one = pool(1).install(|| run_census(&s, &CensusOptions::default(), None).unwrap());
    let four = pool(4).install(|| run_census(&s, &CensusOptions::default(), None).unwrap());
    assert_eq!(one.report.to_json(), four.report.to_json());
    assert_eq!(one.report.to_csv(), four.report.to_csv());
    assert_eq!(one.report.rules.len(), 17);
    assert_eq!(one.report.rng_states, vec![1, 2, 3]);
}

#[test]
fn known_rules_classify_as_expected() {
    let s = spec(&["B3/S23", "B2/S", "B1357/S1357", "B/S"], 4, 1500);
    let out = run_census(&s, &CensusOptions::default(), None).unwrap();
    let r = |n: &str| out.report.rules.iter().find(|r| r.rule == n).unwrap();
    // Life on a small torus settles (frozen or periodic) or dies within 1,500 generations.
    let life = r("B3/S23");
    assert_eq!(life.name.as_deref(), Some("Life"));
    assert_eq!(life.active + life.exploded, 0.0, "{life:?}");
    assert!(life.median_settle.is_some());
    for run in &life.results {
        if let Some(p) = run.period {
            assert!(run.settle.unwrap() <= run.gens_run - p);
        }
    }
    // Seeds explodes or stays active.
    let seeds = r("B2/S");
    assert_eq!(seeds.active + seeds.exploded, 1.0, "{seeds:?}");
    // The replicator rule stays active.
    let rep = r("B1357/S1357");
    assert_eq!(rep.active + rep.exploded, 1.0, "{rep:?}");
    // B/S kills everything in one generation.
    let dead = r("B/S");
    assert_eq!(dead.died, 1.0);
    assert_eq!(dead.median_settle, Some(1.0));
}

#[test]
fn settle_generation_is_exact() {
    let s = spec(&["B3/S23", "B2/S34"], 4, 1500);
    let out = run_census(&s, &CensusOptions::default(), None).unwrap();
    let states = |rule: &str, rng: u64, g: u64| {
        let mut w = World::new(s.tiling.clone(), rng).unwrap();
        w.apply(Command::AddCa {
            ca: CaSpec::new(rule).seed(SeedSpec::Random { density: s.density }),
        })
        .unwrap();
        w.apply(Command::Step { gens: g }).unwrap();
        w.fields[0].state.clone()
    };
    let mut checked = 0;
    for r in &out.report.rules {
        for run in &r.results {
            let (Some(p), Some(t)) = (run.period, run.settle) else {
                continue;
            };
            assert_eq!(
                states(&r.rule, run.rng_state, t),
                states(&r.rule, run.rng_state, t + p)
            );
            if t > 0 {
                assert_ne!(
                    states(&r.rule, run.rng_state, t - 1),
                    states(&r.rule, run.rng_state, t - 1 + p)
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 4);
    // A full grid under Life dies in one generation.
    let mut s = spec(&["B3/S23"], 1, 50);
    s.density = 1.0;
    let out = run_census(&s, &CensusOptions::default(), None).unwrap();
    assert_eq!(out.report.rules[0].results[0].class, RunClass::Died);
    assert_eq!(out.report.rules[0].results[0].settle, Some(1));
}

#[test]
fn rule_sets() {
    assert_eq!(parse_digits("1-3,6").unwrap(), vec![1, 2, 3, 6]);
    let all = all_bs_rules(&[3], &[2, 3], None).unwrap();
    assert_eq!(
        all,
        vec![
            "B/S", "B/S2", "B/S3", "B/S23", "B3/S", "B3/S2", "B3/S3", "B3/S23"
        ]
    );
    let gens = all_bs_rules(&[2], &[], Some(3)).unwrap();
    assert_eq!(gens, vec!["B/S/C3", "B2/S/C3"]);
    let a = sample_bs_rules(
        &[1, 2, 3, 4, 5, 6, 7, 8],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8],
        None,
        20,
        9,
    );
    let b = sample_bs_rules(
        &[1, 2, 3, 4, 5, 6, 7, 8],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8],
        None,
        20,
        9,
    );
    assert_eq!(a, b);
    assert_eq!(a.len(), 20);
    let mut d = a.clone();
    d.sort();
    d.dedup();
    assert_eq!(d.len(), 20);
    // Never more than exist.
    assert_eq!(sample_bs_rules(&[3], &[2], None, 50, 1).len(), 4);
    assert_eq!(
        parse_rule_list("B3/S23\n# comment\nB36/S23 ; B2/S/C3\n\n"),
        vec!["B3/S23", "B36/S23", "B2/S/C3"]
    );
    let t = Topology::build(&TilingSpec::new("square")).unwrap();
    assert_eq!(max_degree(&t, Nb::Touch), 8);
    assert_eq!(max_degree(&t, Nb::Edge), 4);
}

#[test]
fn bad_rules_are_reported_not_fatal() {
    let s = spec(&["B3/S23", "nonsense"], 1, 10);
    let out = run_census(&s, &CensusOptions::default(), None).unwrap();
    assert!(out.report.rules[1].error.is_some());
    assert!(out.report.rules[0].error.is_none());
}

#[test]
fn thumbnails_and_spectre() {
    let s = CensusSpec {
        tiling: TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
        rules: vec!["B3/S23".into(), "B2/S34".into()],
        soups: 2,
        gens: 200,
        ..Default::default()
    };
    let out = run_census(
        &s,
        &CensusOptions {
            thumb_px: Some(64),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert!(out.report.dead_boundary);
    assert_eq!(out.thumbs.len(), 2);
    assert!(out.thumbs.iter().all(|t| t.is_some()));
}

#[test]
fn the_bit_parallel_board_gives_the_worlds_census() {
    let world = CensusOptions {
        world_only: true,
        ..Default::default()
    };
    for (family, b, rules) in [
        (
            "square",
            Boundary::Wrap,
            vec![
                "B3/S23",
                "B36/S23",
                "B2/S",
                "B1357/S1357",
                "B/S",
                "B3/S23/C3",
                "B2/S/C3",
            ],
        ),
        (
            "hex",
            Boundary::Dead,
            vec!["B2/S34", "B245/S3", "B24/S35/C4"],
        ),
        ("oct", Boundary::Wrap, vec!["B3/S23", "B2/S3V"]),
    ] {
        let s = CensusSpec {
            tiling: TilingSpec::rect(family, 20, 18, b),
            rules: rules.iter().map(|r| r.to_string()).collect(),
            soups: 3,
            gens: 600,
            ..Default::default()
        };
        let fast = run_census(&s, &CensusOptions::default(), None).unwrap();
        let slow = run_census(&s, &world, None).unwrap();
        assert_eq!(fast.report.to_json(), slow.report.to_json(), "{family}");
        // Something settled, oscillated, or stayed active, so the watch was exercised.
        assert!(
            fast.report
                .rules
                .iter()
                .any(|r| r.oscillating > 0.0 || r.frozen > 0.0)
        );
    }
}

#[test]
fn census_jobs_resume_to_the_same_report() {
    use jetcarp_core::library::{JobStatus, Library};
    let dir = std::env::temp_dir().join(format!("jetcarp-census-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(&dir);
    let mut s = spec(&[], 2, 300);
    s.rules = all_bs_rules(&[2, 3, 6], &[2, 3], None).unwrap();
    s.rules.push("nonsense".into());
    let straight = run_census(&s, &CensusOptions::default(), None).unwrap();
    let job = start_job(&lib, &s).unwrap();
    for limit in [5, 7] {
        let p = run_job(&Library::open(&dir), job, 3, Some(limit));
        assert_eq!(p.status, "paused");
    }
    assert_eq!(lib.job(job).unwrap().status, JobStatus::Paused);
    assert_eq!(job_report(&lib, job).unwrap().rules.len(), 12);
    let p = run_job(&Library::open(&dir), job, 0, None);
    assert_eq!((p.status.as_str(), p.rules_done), ("done", 33));
    let r = job_report(&lib, job).unwrap();
    let mut a = straight.report.clone();
    a.jetcarp_version.clear();
    let mut b = r.clone();
    b.jetcarp_version.clear();
    assert_eq!(a.to_json(), b.to_json());
    assert_eq!(job_status(&lib, job).unwrap().1.status, "done");
}

fn world_states(w: &World) -> Vec<u8> {
    w.fields[w.cas[0].field].state.clone()
}

/// Step a world and a board side by side and compare every cell at every generation.
fn same_as_world(family: &str, nb: Nb, rule: &str, size: (u32, u32), b: Boundary, gens: u64) {
    let spec = TilingSpec::rect(family, size.0, size.1, b);
    let topo = Arc::new(Topology::build(&spec).unwrap());
    let mut w = World::on_topology(topo.clone(), 7);
    let mut ca = CaSpec::new(rule);
    ca.neighborhood = Some(nb);
    ca.seed = Some(SeedSpec::Random { density: 0.4 });
    w.apply(Command::AddCa { ca }).unwrap();
    let (r, _) = jetcarp_core::rules::Rule::parse(rule).unwrap();
    let mut board = Board::for_topology(&r, &topo, nb).unwrap();
    board.load(&world_states(&w));
    assert_eq!(board.states(), world_states(&w));
    for g in 1..=gens {
        let before = world_states(&w);
        w.apply(Command::Step { gens: 1 }).unwrap();
        board.step();
        let now = world_states(&w);
        assert_eq!(
            board.states(),
            now,
            "{family} {nb:?} {rule} {size:?} {b:?}: differs at generation {g}"
        );
        let pop = now.iter().filter(|&&s| s != 0).count() as u64;
        assert_eq!(board.population(), pop);
        let changed = before.iter().zip(&now).filter(|(a, b)| a != b).count() as u64;
        assert_eq!(
            board.changed(),
            changed,
            "{family} {rule}: changed cells at {g}"
        );
    }
}

#[test]
fn board_equals_the_world_on_every_family() {
    for b in [Boundary::Wrap, Boundary::Dead] {
        for size in [(37, 23), (70, 12)] {
            same_as_world("square", Nb::Touch, "B3/S23", size, b, 60);
            same_as_world("square", Nb::Edge, "B1/S1", size, b, 30);
            same_as_world("square", Nb::Touch, "B2/S/C3", size, b, 40);
            same_as_world("square", Nb::Touch, "B34/S34/C5", size, b, 40);
            same_as_world("hex", Nb::Touch, "B2/S34", size, b, 60);
            same_as_world("triangle", Nb::Touch, "B45/S34", size, b, 40);
            same_as_world("triangle", Nb::Edge, "B1/S12", size, b, 40);
            same_as_world("oct", Nb::Touch, "B3/S23", size, b, 40);
            same_as_world("trihex", Nb::Touch, "B2/S34", size, b, 40);
            same_as_world("snubsq", Nb::Touch, "B3/S23", size, b, 30);
        }
    }
    // Narrow and tiny grids (the column wraps inside one word).
    same_as_world("square", Nb::Touch, "B3/S23", (8, 8), Boundary::Wrap, 40);
    same_as_world("hex", Nb::Touch, "B2/S34", (8, 9), Boundary::Wrap, 40);
    same_as_world("triangle", Nb::Touch, "B4/S345", (9, 8), Boundary::Wrap, 40);
    same_as_world("square", Nb::Touch, "B36/S23", (64, 20), Boundary::Wrap, 40);
    same_as_world("hex", Nb::Touch, "B2/S34", (128, 9), Boundary::Dead, 40);
}
