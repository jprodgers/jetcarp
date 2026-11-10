//! Stage 1 (Phase 4): the bounded bit-parallel board against the world, settings evaluation,
//! the score and its refits, and the adaptive search as a resumable job.

use jetcarp_core::library::{JobStatus, Library, NewSetting, QsStatus, SettingStatus, SettingsQuery};
use jetcarp_core::stage1::board::Board;
use jetcarp_core::stage1::eval::{Descriptors, GridKit, Setting, evaluate, soup_states};
use jetcarp_core::stage1::job::{self, Base, Neighbours, Refine, Space, Stage1Options, Stage1Spec};
use jetcarp_core::stage1::score::{self, ScoreModel};
use jetcarp_core::topology::{Boundary, Nb, TilingSpec, Topology};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};
use std::sync::Arc;

fn temp_lib(name: &str) -> Library {
    let dir = std::env::temp_dir().join(format!("jetcarp-stage1-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

/// A small search: hexagon rules with births 2-3 and survivals 2-4 (32 rules), refined and
/// expanded.
fn small_spec() -> Stage1Spec {
    let mut hex = Space::new(TilingSpec::new("hex"), Nb::Touch);
    hex.birth = Some(vec![2, 3]);
    hex.survive = Some(vec![2, 3, 4]);
    let mut sq = Space::new(TilingSpec::new("square"), Nb::Touch);
    sq.rules = vec![
        "B3/S23".into(),
        "B36/S23".into(),
        "B2/S".into(),
        "B3/S12".into(),
    ];
    Stage1Spec {
        spaces: vec![hex, sq],
        base: Base {
            density: 0.35,
            soup: 1.0,
            grid: [32, 32],
            boundary: Boundary::Wrap,
            gens: 300,
            soups: 2,
            seed: 5,
        },
        refine: Refine {
            top: 3,
            densities: vec![0.2],
            soups: vec![0.5],
            grids: vec![[40, 36]],
            gens: vec![450],
        },
        neighbours: Neighbours { top: 2, rounds: 1 },
    }
}

/// Everything a search left in a catalog that must not depend on threads, batches, or resumes.
fn snapshot(lib: &Library, j: i64) -> Snap {
    let rows = lib
        .settings(&SettingsQuery {
            job: Some(j),
            order: Some("id".into()),
            ..Default::default()
        })
        .unwrap();
    let s = rows
        .iter()
        .map(|r| {
            format!(
                "{} {} {:?} {} {} {}",
                r.id,
                r.setting.label(),
                r.score,
                r.status.name(),
                r.pass,
                serde_json::to_string(&r.descriptors).unwrap()
            )
        })
        .collect();
    let mut q: Vec<String> = lib
        .quickstart(&[])
        .unwrap()
        .iter()
        .map(|q| {
            format!(
                "{} {} {:?} {:?} {}",
                q.id, q.title, q.score, q.status, q.behaviour
            )
        })
        .collect();
    q.sort();
    let (_, p) = job::status(lib, j).unwrap();
    let prog = format!(
        "{} {:?} {:?} {:?} {:?}",
        p.evaluated,
        p.statuses,
        p.histogram,
        p.best.iter().map(|b| (b.id, b.score)).collect::<Vec<_>>(),
        p.spaces
    );
    (s, q, prog)
}

type Snap = (Vec<String>, Vec<String>, String);

/// Compare snapshots, reporting the first difference.
fn same(a: &Snap, b: &Snap) {
    for (k, (x, y)) in [(&a.0, &b.0), (&a.1, &b.1)].into_iter().enumerate() {
        for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
            assert_eq!(p, q, "part {k}, row {i}");
        }
        assert_eq!(x.len(), y.len(), "part {k}: lengths");
    }
    assert_eq!(a.2, b.2, "progress");
}

fn run_search(name: &str, threads: usize, batch: u64) -> (Library, i64) {
    let lib = temp_lib(name);
    lib.set_thresholds(45.0, 35.0).unwrap();
    let j = job::start(&lib, &small_spec()).unwrap();
    let p = job::run(
        &lib,
        j,
        &Stage1Options {
            threads,
            batch,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.status, "done");
    (lib, j)
}

#[test]
fn search_is_independent_of_threads_and_batches() {
    let (a, ja) = run_search("t1", 1, 5);
    let (b, jb) = run_search("tall", 0, 0);
    let (c, jc) = run_search("t3", 3, 17);
    let sa = snapshot(&a, ja);
    same(&sa, &snapshot(&b, jb));
    same(&sa, &snapshot(&c, jc));
    // Every pass ran: 36 coarse settings, refinements, and neighbours.
    let rows = a
        .settings(&SettingsQuery {
            job: Some(ja),
            ..Default::default()
        })
        .unwrap();
    for pass in ["coarse", "refine", "neighbours-1"] {
        assert!(rows.iter().any(|r| r.pass == pass), "no {pass} settings");
    }
    assert_eq!(rows.iter().filter(|r| r.pass == "coarse").count(), 36);
    // Shown settings have Quick Start entries whose scene reproduces soup 0.
    let auto: Vec<_> = rows
        .iter()
        .filter(|r| r.status == SettingStatus::Auto)
        .collect();
    assert!(!auto.is_empty(), "no auto settings");
    for r in auto.iter().take(3) {
        let q = a.get_quickstart(&r.entry_id()).unwrap();
        assert_eq!(q.status, QsStatus::Auto);
        assert_eq!(q.source_kind, "setting");
        assert!(!q.title.is_empty() && !q.behaviour.is_empty());
        let w = World::from_scene(&q.scene).unwrap();
        let kit = GridKit::new(&r.setting).unwrap();
        assert_eq!(
            world_states(&w),
            soup_states(&kit.topo, &r.setting, 0, r.setting.density).unwrap()
        );
        // The stored descriptors are the setting's own (evaluated again; novelty is the
        // search's).
        let (mut d, _) = evaluate(&r.setting, &kit).unwrap();
        d.novelty = r.descriptors.novelty;
        assert_eq!(d, r.descriptors);
    }
    let cands = rows
        .iter()
        .filter(|r| r.status == SettingStatus::Candidate)
        .count();
    let queue = a.quickstart(&[QsStatus::Candidate]).unwrap().len();
    assert_eq!(cands, queue);
}

#[test]
fn search_resumed_equals_uninterrupted() {
    let (a, ja) = run_search("straight", 0, 7);
    let lib = temp_lib("resumed");
    lib.set_thresholds(45.0, 35.0).unwrap();
    let j = job::start(&lib, &small_spec()).unwrap();
    // Three sessions, each in a newly opened catalog, stopping mid-pass.
    for limit in [20, 15] {
        let p = job::run(
            &Library::open(lib.path()),
            j,
            &Stage1Options {
                batch: 7,
                limit: Some(limit),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.status, "paused");
        assert_eq!(lib.job(j).unwrap().status, JobStatus::Paused);
    }
    let p = job::run(
        &Library::open(lib.path()),
        j,
        &Stage1Options {
            batch: 4,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.status, "done");
    same(&snapshot(&a, ja), &snapshot(&lib, j));
}

fn synthetic(i: usize) -> (Setting, Descriptors) {
    let s = Setting {
        rule: format!("B{}/S{}", 1 + i % 6, i / 6 % 7),
        tiling: TilingSpec::new("hex"),
        density: 0.3 + 0.01 * (i / 42) as f64,
        ..Default::default()
    };
    let d = Descriptors {
        runs: 4,
        transient: (i % 5) as f64 / 4.0,
        activity: vec![0.01; 8],
        activity_mean: 0.003 * (1 + i % 7) as f64,
        activity_end: 0.002 * (1 + i % 3) as f64,
        final_density: 0.1,
        compress: 0.4,
        entropy: 0.5,
        compress_mid: 0.5,
        entropy_mid: 0.5,
        ship_kinds: (i % 4) as u32,
        ships: (i % 4) as f64,
        oscillator_kinds: (i % 3) as u32,
        distinct: 3 + (i % 5) as u32,
        novelty: Some(0.1),
        behaviour: "mixed".into(),
        ..Default::default()
    };
    (s, d)
}

#[test]
fn ratings_that_like_ships_raise_the_ships_weight() {
    let lib = temp_lib("refit");
    let hand = lib.score_model().unwrap();
    let rows: Vec<NewSetting> = (0..40)
        .map(|i| {
            let (s, d) = synthetic(i);
            let f = score::features(&d, false);
            NewSetting {
                setting: s,
                score: hand.score(&f),
                status: SettingStatus::Stored,
                descriptors: d,
                model_id: None,
                pass: "test".into(),
            }
        })
        .collect();
    let fl = lib.stage1_flush(None, &rows, None).unwrap();
    // Too few ratings: refused.
    for (k, id) in fl.ids.iter().take(10).enumerate() {
        lib.rate("setting", &id.to_string(), "user", 1.0 + (k % 5) as f64, "")
            .unwrap();
    }
    let err = lib.refit_score(2.0).unwrap_err();
    assert!(err.contains("at least 20"), "{err}");
    // Ratings that follow the ship kinds only (and a model's, at half weight).
    for (i, id) in fl.ids.iter().enumerate() {
        let r = 1.0 + (i % 4) as f64 * 4.0 / 3.0;
        lib.rate("setting", &id.to_string(), "user", r, "").unwrap();
        lib.rate("quickstart", &format!("setting-{id}"), "model:stub", r, "")
            .unwrap();
    }
    let (m, rep) = lib.refit_score(2.0).unwrap();
    assert_eq!(m.source, "refit");
    assert!(
        m.weights["ships"] > hand.weights["ships"] + 5.0,
        "ships weight {} (hand {})",
        m.weights["ships"],
        hand.weights["ships"]
    );
    assert!(m.rmse_after.unwrap() < m.rmse_before.unwrap());
    assert_eq!(rep.settings, 40);
    // Versions are kept; the refit is active and every setting was rescored with it.
    let models = lib.score_models().unwrap();
    assert_eq!(models.iter().filter(|x| x.1).count(), 1);
    assert_eq!(lib.score_model().unwrap().id, m.id);
    let all = lib.settings(&SettingsQuery::default()).unwrap();
    assert!(all.iter().all(|r| r.model_id == Some(m.id)));
    let best = &all[0];
    assert!(best.descriptors.ship_kinds == 3, "{:?}", best.descriptors);
    // The same ratings give the same fit.
    let again = score::refit(
        &hand,
        &lib.rating_samples().unwrap().0,
        lib.rating_samples().unwrap().1,
        2.0,
    )
    .unwrap();
    let again2 = score::refit(
        &hand,
        &lib.rating_samples().unwrap().0,
        lib.rating_samples().unwrap().1,
        2.0,
    )
    .unwrap();
    assert_eq!(again.weights, again2.weights);
}

#[test]
fn approval_queue_and_thresholds() {
    let lib = temp_lib("review");
    let hand = ScoreModel::hand();
    let rows: Vec<NewSetting> = (0..12)
        .map(|i| {
            let (s, d) = synthetic(i);
            let score = 30.0 + 5.0 * i as f64;
            NewSetting {
                setting: s,
                descriptors: d,
                score,
                status: SettingStatus::for_score(&hand, score),
                model_id: None,
                pass: "test".into(),
            }
        })
        .collect();
    let fl = lib.stage1_flush(None, &rows, None).unwrap();
    let count = |st: QsStatus| lib.quickstart(&[st]).unwrap().len();
    // 30..85 in steps of 5: auto from 70 (4), candidates 55..65 (3).
    assert_eq!(count(QsStatus::Auto), 4);
    assert_eq!(count(QsStatus::Candidate), 3);
    let cand = fl.ids[6];
    let r = lib
        .review_setting(
            &format!("setting-{cand}"),
            SettingStatus::Approved,
            None,
            "nice",
        )
        .unwrap();
    assert_eq!(r.status, SettingStatus::Approved);
    assert_eq!(
        lib.get_quickstart(&format!("setting-{cand}"))
            .unwrap()
            .status,
        QsStatus::Approved
    );
    let ratings = lib.ratings(Some("setting")).unwrap();
    assert_eq!(ratings.len(), 1);
    assert_eq!(
        (ratings[0].rating, ratings[0].rater.as_str()),
        (4.0, "user")
    );
    let rej = fl.ids[5];
    lib.review_setting(&rej.to_string(), SettingStatus::Rejected, None, "")
        .unwrap();
    assert_eq!(lib.ratings(Some("setting")).unwrap().len(), 1);
    // A rescore with new thresholds moves the undecided ones only.
    let (_, rep) = lib.set_thresholds(99.0, 98.0).unwrap();
    assert_eq!(rep.statuses.get("approved"), Some(&1));
    assert_eq!(rep.statuses.get("rejected"), Some(&1));
    assert_eq!(count(QsStatus::Auto), 0);
    assert_eq!(count(QsStatus::Candidate), 0);
    assert_eq!(count(QsStatus::Approved), 1);
    assert!(lib.set_thresholds(50.0, 60.0).is_err());
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

#[test]
fn soups_match_the_settings_scene() {
    let s = Setting {
        tiling: TilingSpec::new("hex"),
        rule: "B2/S34".into(),
        grid: [40, 30],
        soup: 0.5,
        ..Default::default()
    };
    let kit = GridKit::new(&s).unwrap();
    for soup in [0, 3] {
        let w = World::from_scene(&s.scene(soup).unwrap()).unwrap();
        assert_eq!(
            soup_states(&kit.topo, &s, soup, s.density).unwrap(),
            world_states(&w)
        );
    }
    let (d, runs) = evaluate(&s, &kit).unwrap();
    assert_eq!(runs.len(), 6);
    assert_eq!(d.runs, 4);
}

/// Settings per second (release, `--ignored --nocapture`): random B/S rules on 64 x 64 wrapped
/// grids, 4 soups plus 2 probes, 1,000 generations.
#[test]
#[ignore]
fn stage1_speed() {
    use rayon::prelude::*;
    let threads: usize = std::env::var("STAGE1_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    {
        let s = Setting::default();
        let kit = GridKit::new(&s).unwrap();
        let (r, nb) = s.rule().unwrap();
        let mut b = Board::for_topology(&r, &kit.topo, nb).unwrap();
        let st = soup_states(&kit.topo, &s, 0, 0.35).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            b.load(&st);
            b.run(1000);
        }
        println!(
            "board: {:.2} us per generation (64x64 Life)",
            t.elapsed().as_secs_f64() * 1e6 / 20_000.0
        );
        let t = std::time::Instant::now();
        for i in 0..200 {
            let _ = soup_states(&kit.topo, &s, i, 0.35).unwrap();
        }
        println!("soup: {:.1} us", t.elapsed().as_secs_f64() * 1e6 / 200.0);
        let t = std::time::Instant::now();
        for _ in 0..200 {
            let _ = jetcarp_core::stage1::looks::frame_looks(&st, 64, 1, 2);
        }
        println!("looks: {:.1} us", t.elapsed().as_secs_f64() * 1e6 / 200.0);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = evaluate(&s, &kit).unwrap();
        }
        println!(
            "evaluate Life: {:.2} ms",
            t.elapsed().as_secs_f64() * 1e3 / 20.0
        );
    }
    for (family, nb, n) in [
        ("square", Nb::Touch, 2000usize),
        ("square", Nb::Edge, 512),
        ("hex", Nb::Touch, 2000),
        ("oct", Nb::Touch, 1000),
        ("triangle", Nb::Touch, 1000),
    ] {
        let base = Setting {
            tiling: TilingSpec::new(family),
            nb: Some(nb),
            ..Default::default()
        };
        let kit = GridKit::new(&base).unwrap();
        let deg = jetcarp_core::census::max_degree(&kit.topo, nb) as u32;
        let birth: Vec<u32> = (1..=deg).collect();
        let surv: Vec<u32> = (0..=deg).collect();
        let rules = jetcarp_core::census::sample_bs_rules(&birth, &surv, None, n, 3);
        let t = std::time::Instant::now();
        let out: Vec<_> = pool.install(|| {
            rules
                .par_iter()
                .map(|r| {
                    let s = Setting {
                        rule: r.clone(),
                        ..base.clone()
                    };
                    evaluate(&s, &kit).map(|x| x.0.behaviour)
                })
                .collect()
        });
        let dt = t.elapsed().as_secs_f64();
        let ok = out.iter().filter(|x| x.is_ok()).count();
        println!(
            "{family} {nb:?}: {} rules in {dt:.2} s = {:.0} rules/s on {threads} threads ({ok} ok)",
            rules.len(),
            rules.len() as f64 / dt
        );
    }
}
