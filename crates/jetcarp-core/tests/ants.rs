//! Ant and turmite search (Phase 4, catalog deliverable 11): the unbounded engine against the
//! world's turmites, exact highways (Langton's ant against its published numbers), symmetric
//! growth, a hexagon highway, the reductions, and searches as resumable, thread-independent
//! catalog jobs.

use jetcarp_core::ants::job::{AntProgress, AntSpec, Soups, rules_for, run_one, start, status};
use jetcarp_core::ants::plane::{Ant, Compiled, Field, Lattice, step};
use jetcarp_core::ants::run::{Limits, Outcome, Start};
use jetcarp_core::ants::search::{Strings, Tables, parse_rule};
use jetcarp_core::jobs::ants::AntsRun;
use jetcarp_core::jobs::{Budget, RunOptions, Runner};
use jetcarp_core::library::{AntQuery, AntRow, Library, tiling_list};
use jetcarp_core::topology::{Boundary, TilingSpec};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};

/// Run (or resume) an ant search here on the job runner: on `threads` threads (0: all), `batch`
/// runs per batch (0: adapt), stopping (paused) after `limit` runs in this session.
fn run_job(lib: &Library, job: i64, threads: usize, batch: u64, limit: Option<u64>) -> AntProgress {
    let o = RunOptions {
        threads,
        budget: Budget {
            items: limit,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut s = AntsRun::default();
    s.batch = batch;
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .unwrap()
        .progress_as()
}

fn tiling(t: &str) -> TilingSpec {
    tiling_list(t).unwrap().remove(0)
}

fn temp_lib(name: &str) -> Library {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-ants-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Library::open(d.join("catalog.sqlite"))
}

/// A world of `w` x `h` unit cells with one ant at the centre, and the centre's unsheared
/// lattice coordinates.
fn ant_world(t: &str, rule: &str, w: u32, h: u32) -> (World, i32, i32) {
    let mut spec = tiling(t);
    spec.size = Some(jetcarp_core::topology::SizeSpec::Rect { w, h });
    spec.boundary = Boundary::Dead;
    let mut world = World::new(spec, 1).unwrap();
    world
        .apply(Command::AddCa {
            ca: CaSpec::new(rule).seed(SeedSpec::Ants { count: 1 }),
        })
        .unwrap();
    let g = world.topo.lattice().unwrap().clone();
    let c = world.cas[0].agents[0].cell as usize;
    let (u, j, k) = g.decompose(c);
    assert_eq!(k, 0, "the world's ant starts on tile type 0");
    (world, u + g.row_start[j as usize], j)
}

/// The engine walks exactly as the world's turmites: colours and the ant, step by step.
#[test]
fn engine_equals_world_on_every_family() {
    let cases = [
        ("square", "Ant RL"),
        ("square", "Turmite LRRRRRLLR"),
        ("square", "Turmite 1R1,1L0/1R1,0N0"),
        ("square", "Turmite RLNU"),
        ("hex", "Turmite LNL"),
        ("hex", "Turmite RL2NUR2"),
        ("hex", "Turmite 1L21,1R0/0U0,1N1"),
        ("triangle", "Turmite RLN"),
        ("4.8.8", "Turmite RLLR"),
        ("3.4.6.4", "Turmite LRRL"),
        ("cairo", "Turmite RLR"),
        ("3.3.4.3.4", "Ant RL"),
    ];
    for (t, rule) in cases {
        let lat = Lattice::new(&tiling(t)).unwrap();
        let m = parse_rule(rule).unwrap();
        let comp = Compiled::new(&m, &lat);
        let mut f = Field::new(lat.ntypes);
        let mut a = Ant::default();
        let mut path = Vec::new();
        let mut far = 0;
        for _ in 0..4000u64 {
            step(&lat, &comp, &mut f, &mut a);
            path.push(a);
            far = far.max(a.i.abs()).max(a.j.abs());
        }
        // A world wide enough that its dead edge is never reached (rows are sheared).
        let side = (4 * far + 40) as u32;
        let (mut w, i0, j0) = ant_world(t, rule, side, side);
        let g = w.topo.lattice().unwrap().clone();
        for (s, a) in path.iter().enumerate() {
            w.apply(Command::Step { gens: 1 }).unwrap();
            let wa = w.cas[0].agents[0];
            let (u, j, k) = g.decompose(wa.cell as usize);
            let wi = u + g.row_start[j as usize] - i0;
            assert_eq!(
                (wi, j - j0, k as u8, wa.heading as u8, wa.state),
                (a.i, a.j, a.k, a.h, a.q),
                "{rule} on {t}: the ant differs after {} steps",
                s + 1
            );
        }
        let field = &w.fields[w.cas[0].field].state;
        let mine = f.nonzero();
        for &(i, j, k, c) in &mine {
            let x = g
                .resolve(i0 + i, j0 + j, k as u32)
                .expect("inside the world");
            assert_eq!(field[x], c, "{rule} on {t}: cell ({i},{j},{k})");
        }
        let live = field.iter().filter(|&&c| c != 0).count();
        assert_eq!(live, mine.len(), "{rule} on {t}: coloured cells");
    }
}

fn lim(steps: u64) -> Limits {
    Limits {
        steps,
        ..Default::default()
    }
}

/// Langton's ant: the highway, proved exactly, against the published numbers and an
/// independent check on a world.
#[test]
fn langtons_ant_highway_is_exact() {
    let r = run_one(
        &parse_rule("RL").unwrap(),
        &tiling("square"),
        &Start::Empty,
        &lim(200_000),
    )
    .unwrap();
    assert_eq!(r.outcome, Outcome::Highway);
    assert_eq!(r.period, Some(104), "the highway repeats every 104 steps");
    let d = r.d.unwrap();
    assert_eq!(
        (d[0].abs(), d[1].abs()),
        (2, 2),
        "two cells diagonally per period"
    );
    // Published: the highway starts after 9,977 steps (onset = the first periodic step).
    assert_eq!(r.onset, Some(9977));
    assert!((r.speed.unwrap() - 8f64.sqrt() / 104.0).abs() < 1e-6);
    let proof = r.run.proof.as_ref().unwrap();
    assert!(proof.cells > 100, "the proof compares the cells ahead too");

    // The same numbers from the world's own turmite: the ant's (colour, heading) sequence is
    // 104-periodic from step 9977 on and not from 9976, and after 104 steps the ant has moved
    // by d with the same heading.
    let (mut w, ..) = ant_world("square", "Ant RL", 220, 220);
    let mut seq = Vec::new();
    let mut cells = Vec::new();
    for _ in 0..12_000 {
        let a = w.cas[0].agents[0];
        let st = w.fields[w.cas[0].field].state[a.cell as usize];
        seq.push((st, a.heading));
        cells.push(a.cell);
        w.apply(Command::Step { gens: 1 }).unwrap();
    }
    let p = 104;
    assert!((9977..12_000 - p).all(|t| seq[t] == seq[t + p]));
    assert_ne!(seq[9976], seq[9976 + p]);
    let g = w.topo.lattice().unwrap().clone();
    let (u0, j0, _) = g.decompose(cells[10_000] as usize);
    let (u1, j1, _) = g.decompose(cells[10_000 + p] as usize);
    assert_eq!(((u1 - u0).abs(), (j1 - j0).abs()), (2, 2));
}

#[test]
fn symmetric_growth_is_classified() {
    for rule in ["LLRR", "RLLR", "LRRL"] {
        let r = run_one(
            &parse_rule(rule).unwrap(),
            &tiling("square"),
            &Start::Empty,
            &lim(400_000),
        )
        .unwrap();
        assert_eq!(r.outcome, Outcome::Symmetric, "{rule}");
        let s = r.run.symmetry.unwrap();
        assert!(s.count >= 10 && s.last_step >= 100_000, "{rule}: {s:?}");
        assert!(s.last_cells >= 1000, "{rule}: the symmetric fields grow");
    }
    // Langton's ant is symmetric only early on (below the cell threshold), then a highway; a
    // chaotic string never returns to a large symmetric field.
    let r = run_one(
        &parse_rule("LRRRRRLLR").unwrap(),
        &tiling("square"),
        &Start::Empty,
        &lim(400_000),
    )
    .unwrap();
    assert_eq!(r.outcome, Outcome::Chaotic);
}

/// A highway on hexagons among the short strings: `LNL` (60-degree turns only) builds one
/// after 161,093 steps with period 35.
#[test]
fn hexagon_highway() {
    let r = run_one(
        &parse_rule("LNL").unwrap(),
        &tiling("hex"),
        &Start::Empty,
        &lim(400_000),
    )
    .unwrap();
    assert_eq!(r.outcome, Outcome::Highway, "{r:?}");
    assert_eq!(r.period, Some(35));
    assert_eq!(r.onset, Some(161_093));
    let d = r.d.unwrap();
    assert_eq!(
        jetcarp_core::ants::plane::Lattice::new(&tiling("hex"))
            .map(|l| l.world(d))
            .map(|w| (w[0].hypot(w[1]) * 1e6).round())
            .unwrap(),
        (Lattice::new(&tiling("hex")).unwrap().world([2, 0])[0].abs() * 1e6).round(),
        "two cells per period"
    );
    // Hexagonal Langton's ant (RL with 60-degree turns) grows symmetrically.
    let r = run_one(
        &parse_rule("RL").unwrap(),
        &tiling("hex"),
        &Start::Empty,
        &lim(400_000),
    )
    .unwrap();
    assert_eq!(r.outcome, Outcome::Symmetric);
}

#[test]
fn periodic_orbits_and_soups() {
    // U then R: back and forth into a small loop.
    let r = run_one(
        &parse_rule("UR").unwrap(),
        &tiling("square"),
        &Start::Empty,
        &lim(100_000),
    )
    .unwrap();
    assert!(
        matches!(r.outcome, Outcome::Periodic | Outcome::Highway),
        "{r:?}"
    );
    // From a soup, Langton's ant still builds a highway (a run is a pure function of the soup).
    let soup = Start::Soup {
        w: 8,
        h: 8,
        density: 0.5,
        state: 7,
    };
    let a = run_one(
        &parse_rule("RL").unwrap(),
        &tiling("square"),
        &soup,
        &lim(500_000),
    )
    .unwrap();
    let b = run_one(
        &parse_rule("RL").unwrap(),
        &tiling("square"),
        &soup,
        &lim(500_000),
    )
    .unwrap();
    assert_eq!(a.run, b.run);
    assert_eq!(a.outcome, Outcome::Highway);
    assert_eq!(a.period, Some(104));
}

#[test]
fn notation_and_reductions() {
    for s in [
        "Ant RL",
        "Turmite LLRR",
        "Turmite RL2NU",
        "Turmite 1R1,1L0/1R1,0N0",
    ] {
        let m = parse_rule(s).unwrap();
        assert_eq!(m.notation(), s);
        assert_eq!(parse_rule(&m.notation()).unwrap(), m);
    }
    assert_eq!(parse_rule("rl").unwrap().notation(), "Ant RL");
    assert!(parse_rule("Turmite 1R1,1L0/1R1").is_err());
    assert!(
        parse_rule("Turmite 1R2,1L0/1R1,0N0").is_err(),
        "state 2 of 2"
    );

    let sq = Lattice::new(&tiling("square")).unwrap();
    let spec = AntSpec {
        strings: Some(Strings {
            min: 2,
            max: 4,
            alphabet: Some("LR".into()),
        }),
        ..Default::default()
    };
    let names: Vec<String> = rules_for(&spec, &sq)
        .unwrap()
        .iter()
        .map(|m| m.notation())
        .collect();
    // 2-4 letters over L, R: no repeats (LL, RR, LRLR, ...), one of each mirror pair.
    assert_eq!(
        names,
        [
            "Turmite LR",
            "Turmite LLR",
            "Turmite LRL",
            "Turmite LRR",
            "Turmite LLLR",
            "Turmite LLRL",
            "Turmite LLRR",
            "Turmite LRLL",
            "Turmite LRRL",
            "Turmite LRRR",
        ]
    );
    let tri = Lattice::new(&tiling("triangle")).unwrap();
    assert!(!tri.mirror, "triangles have two tile types");
    assert_eq!(
        jetcarp_core::ants::search::alphabet(&tri),
        [
            jetcarp_core::rules::Turn::L,
            jetcarp_core::rules::Turn::R,
            jetcarp_core::rules::Turn::N
        ]
    );
    // Tables: 1 state x 2 colours over L, R: every live table up to the mirror.
    let spec = AntSpec {
        strings: None,
        tables: Some(Tables {
            states: 2,
            colors: 2,
            alphabet: Some("LR".into()),
            all_max: 1_000_000,
            sample: 10,
            seed: 1,
        }),
        ..Default::default()
    };
    let t = rules_for(&spec, &sq).unwrap();
    assert!(!t.is_empty());
    assert!(t.iter().all(|m| m.states == 2 && m.colors == 2));
    let set: std::collections::HashSet<_> = t.iter().collect();
    assert!(
        t.iter()
            .all(|m| !set.contains(&m.mirror()) || m.mirror() == *m)
    );
}

fn rows(lib: &Library, job: i64) -> Vec<AntRow> {
    let mut v = lib
        .ants(&AntQuery {
            job: Some(job),
            sort: Some("rule".into()),
            ..Default::default()
        })
        .unwrap();
    for r in &mut v {
        r.created.clear();
        r.updated.clear();
        r.job = None;
    }
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

fn small_spec() -> AntSpec {
    AntSpec {
        tilings: tiling_list("square,hex").unwrap(),
        strings: Some(Strings {
            min: 2,
            max: 3,
            alphabet: None,
        }),
        steps: 60_000,
        ..Default::default()
    }
}

#[test]
fn search_is_independent_of_threads_and_batches() {
    let a = temp_lib("threads-1");
    let ja = start(&a, &small_spec()).unwrap();
    let pa = run_job(&a, ja, 1, 7, None);
    let b = temp_lib("threads-all");
    let jb = start(&b, &small_spec()).unwrap();
    let pb = run_job(&b, jb, 0, 0, None);
    assert_eq!(pa.status, "done");
    assert_eq!(pa.runs_done, pa.runs_total);
    assert_eq!(pa.by_outcome, pb.by_outcome);
    assert_eq!(pa.groups, pb.groups);
    assert_eq!(rows(&a, ja), rows(&b, jb));
    assert_eq!(rows(&a, ja).len() as u64, pa.runs_total);
    assert!(pa.by_outcome.get("highway").copied().unwrap_or(0) > 0);
    let census = a.ant_census(Some(ja)).unwrap();
    assert_eq!(census.len(), 2);
    assert_eq!(census.iter().map(|c| c.runs).sum::<u64>(), pa.runs_total);
}

#[test]
fn search_resumed_equals_uninterrupted() {
    let mut spec = small_spec();
    spec.soup = Some(Soups {
        w: 4,
        h: 4,
        density: 0.4,
        soups: 3,
        seed: 5,
    });
    let a = temp_lib("straight");
    let ja = start(&a, &spec).unwrap();
    let pa = run_job(&a, ja, 0, 0, None);

    let b = temp_lib("resumed");
    let jb = start(&b, &spec).unwrap();
    let half = pa.runs_total / 2;
    let p1 = run_job(&b, jb, 0, 5, Some(half));
    assert_eq!(p1.status, "paused");
    assert!(p1.runs_done >= half);
    // A newly opened catalog continues at the checkpoint.
    let b2 = Library::open(b.path());
    let (_, st) = status(&b2, jb).unwrap();
    assert_eq!(st.runs_done, p1.runs_done);
    let p2 = run_job(&b2, jb, 0, 0, None);
    assert_eq!(p2.status, "done");
    assert_eq!(p2.by_outcome, pa.by_outcome);
    assert_eq!(p2.groups, pa.groups);
    assert_eq!(p2.newest, pa.newest);
    assert_eq!(rows(&b2, jb), rows(&a, ja));
}

#[test]
fn stored_rows_curate_and_keep_final_results() {
    let lib = temp_lib("rows");
    let m = parse_rule("RL").unwrap();
    let short = run_one(&m, &tiling("square"), &Start::Empty, &lim(5_000)).unwrap();
    assert!(!matches!(
        short.outcome,
        Outcome::Highway | Outcome::Periodic
    ));
    lib.ants_flush(0, std::slice::from_ref(&short), None, None)
        .unwrap();
    let long = run_one(&m, &tiling("square"), &Start::Empty, &lim(100_000)).unwrap();
    assert_eq!(long.id, short.id, "one id per rule, tiling, and start");
    assert_eq!(lib.ants_flush(0, &[long], None, None).unwrap(), 1);
    // A shorter run later does not replace the proved highway.
    assert_eq!(lib.ants_flush(0, &[short], None, None).unwrap(), 0);
    let got = lib.ant("ant-").unwrap();
    assert_eq!(got.outcome, Outcome::Highway);
    assert_eq!(got.onset, Some(9977));
    let u = lib
        .update_ant(
            &got.id,
            &jetcarp_core::library::AntUpdate {
                name: Some("Langton's highway".into()),
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(u.name.as_deref(), Some("Langton's highway"));
    assert_eq!(lib.ant("Langton's highway").unwrap().id, got.id);
    assert_eq!(
        lib.ratings(Some("ant")).unwrap().len(),
        1,
        "a rating row for the ant"
    );
    assert_eq!(lib.info().unwrap().ants, 1);
}

/// Media by outcome: a highway's loop alone is seamless and moves with the ant; the build-up
/// replays the run from step 0 as a scene; a symmetric rule has only the build-up.
#[test]
fn ant_media_by_outcome() {
    use jetcarp_core::media::{self, MediaMode, MediaOptions, MediaTarget};
    let lib = temp_lib("media");
    let mut rows = Vec::new();
    for (rule, t, steps) in [
        ("RL", "square", 100_000),
        ("LNL", "hex", 300_000),
        ("LLRR", "square", 200_000),
    ] {
        rows.push(
            run_one(
                &parse_rule(rule).unwrap(),
                &tiling(t),
                &Start::Empty,
                &lim(steps),
            )
            .unwrap(),
        );
    }
    lib.ants_flush(0, &rows, None, None).unwrap();
    let o = MediaOptions::default();
    for r in &rows[..2] {
        let a = jetcarp_core::ants::media::highway_animation(r, &o).unwrap();
        assert!(a.seamless, "{}: the loop closes", r.rule);
        assert_eq!(a.frames.len() as u64, r.period.unwrap().min(240));
        assert_eq!(a.window_kind, "moving");
        let done = media::render(&lib, &MediaTarget::Ant(r.id.clone()), &o).unwrap();
        assert_eq!(
            done.mode,
            MediaMode::Alone,
            "highways render alone by default"
        );
        assert!(done.animation().is_some() && done.file("poster").is_some());
    }
    // The build-up: a scene that replays the run, with the highway leaving the tangle.
    let seed = MediaOptions {
        mode: MediaMode::Seed,
        frames: false,
        ..Default::default()
    };
    let done = media::render(&lib, &MediaTarget::Ant(rows[0].id.clone()), &seed).unwrap();
    assert_eq!(done.mode, MediaMode::Seed);
    let anim = done.animation().unwrap();
    assert!(
        anim.frames >= 100 && anim.frames <= 152,
        "{} frames",
        anim.frames
    );
    let sc = jetcarp_core::ants::media::build_scene(&rows[0]).unwrap();
    let mut w = World::from_scene(&sc).unwrap();
    let gens = jetcarp_core::ants::media::build_gens(&rows[0]);
    w.apply(Command::Step { gens }).unwrap();
    let per = sc.cas[0].steps_per_gen as u64;
    assert!(gens * per >= 9977 + 4 * 104, "runs past the onset");
    assert_eq!(w.cas[0].agents.len(), 1);
    // Unchanged inputs skip; a symmetric rule has no loop, only its build-up.
    let again = media::render(&lib, &MediaTarget::Ant(rows[0].id.clone()), &o).unwrap();
    assert!(!again.written);
    let sym = MediaTarget::Ant(rows[2].id.clone());
    let alone = MediaOptions {
        mode: MediaMode::Alone,
        ..Default::default()
    };
    assert!(media::render(&lib, &sym, &alone).is_err());
    let s = media::render(&lib, &sym, &MediaOptions::default()).unwrap();
    assert_eq!(s.mode, MediaMode::Seed);
    if let Ok(dir) = std::env::var("JETCARP_ANT_MEDIA_OUT") {
        let hex = media::render(&lib, &MediaTarget::Ant(rows[1].id.clone()), &o).unwrap();
        for d in [&done, &s, &again, &hex] {
            for r in &d.rows {
                if r.kind != "frames" {
                    let name = format!("{}-{}-{}", d.target_id, d.mode.name(), r.kind);
                    let ext = r
                        .abs_path
                        .extension()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    std::fs::copy(&r.abs_path, format!("{dir}/{name}.{ext}")).unwrap();
                }
            }
        }
    }
}

/// Ant scenes open in Prism or a chosen palette; media in a palette are rendered in memory in
/// its colours.
#[test]
fn ant_scenes_and_media_take_a_palette() {
    use jetcarp_core::ants::media::{SCENE_PALETTE, build_scene, build_scene_in};
    use jetcarp_core::media::{self, MediaOptions, MediaStyle, MediaTarget};
    let lib = temp_lib("palette");
    let r = run_one(
        &parse_rule("RL").unwrap(),
        &tiling("square"),
        &Start::Empty,
        &lim(100_000),
    )
    .unwrap();
    lib.ants_flush(0, std::slice::from_ref(&r), None, None)
        .unwrap();
    assert_eq!(
        build_scene(&r).unwrap().cas[0].palette.as_deref(),
        Some(SCENE_PALETTE)
    );
    assert_eq!(
        build_scene_in(&r, None).unwrap().cas[0].palette.as_deref(),
        Some(SCENE_PALETTE)
    );
    let sc = build_scene_in(&r, Some("Paper")).unwrap();
    assert!(sc.cas.iter().all(|c| c.palette.as_deref() == Some("Paper")));
    let e = build_scene_in(&r, Some("Nope")).unwrap_err();
    assert!(e.contains("Prism"), "{e}");
    let o = MediaOptions {
        style: MediaStyle::with_palette(Some("Glacier")).unwrap(),
        ..Default::default()
    };
    let a = media::target_animation(&lib, &MediaTarget::Ant(r.id.clone()), &o).unwrap();
    assert!(a.seamless);
    let bg = &o.style.background;
    let f = &a.frames[0];
    assert!(
        f.rgba.chunks(4).any(|x| &x[..3] == bg),
        "Glacier's background"
    );
    let house = MediaStyle::default();
    assert!(!f.rgba.chunks(4).any(|x| x[..3] == house.background));
    // Nothing was stored.
    assert!(media::stored(&lib, "ant", &r.id).unwrap().is_empty());
}
