//! Phase 3: the pattern library (records, dedup, verification, search, collisions, paste,
//! promotion to Quick Start). Every test uses its own temporary catalog.

use jetcarp_core::library::collide::{CollideSpec, Outcome, collide};
use jetcarp_core::library::search::{SearchSpec, search};
use jetcarp_core::library::{
    self, AddOutcome, CurationState, DiscoveredBy, Library, PasteAt, PatternClass, PatternRecord,
    Provenance, Query, record_from_object, verify,
};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::*;

/// A fresh folder under the system temp folder.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("jetcarp-lib-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A fresh catalog (`catalog.sqlite` in a fresh folder).
fn temp_lib(name: &str) -> Library {
    Library::open(temp_dir(name))
}

fn life_world(pats: &[(&str, i32, i32)]) -> World {
    let mut w = World::new(TilingSpec::rect("square", 96, 80, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = Vec::new();
    for &(rows, u, j) in pats {
        cells.extend(place_rows(&g, rows, u, j));
    }
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    w
}

fn prov(src: &str) -> Provenance {
    Provenance {
        source: src.into(),
        jetcarp_version: VERSION.into(),
        date: library::now_utc(),
        ..Default::default()
    }
}

/// Records for every periodic object in a world.
fn records_of(w: &World, src: &str) -> Vec<PatternRecord> {
    let r = find_objects(w, &ObjectOptions::default()).unwrap();
    r.objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| record_from_object(w, w.cas[0].id, o, prov(src), DiscoveredBy::User).unwrap())
        .collect()
}

/// Standard Life objects, each twice in different orientations or phases.
const LIFE_SET: &[(&str, i32, i32)] = &[
    ("oo/oo", 2, 2),
    ("oo/oo", 10, 2),
    (".oo./o..o/.oo.", 18, 2),
    (".o/o.o/o.o/.o", 26, 2), // beehive, turned
    ("ooo", 2, 12),
    ("o/o/o", 10, 10), // blinker, other phase
    (".o./..o/ooo", 2, 30),
    ("ooo/o../.o.", 20, 30), // glider, another orientation and phase
    (".o..o/o..../o...o/oooo.", 34, 30),
    ("o..o./....o/o...o/.oooo", 34, 45), // LWSS, mirrored
];

fn by_name<'a>(v: &'a [PatternRecord], n: &str) -> &'a PatternRecord {
    v.iter()
        .find(|r| r.name.as_deref() == Some(n))
        .unwrap_or_else(|| panic!("no {n}"))
}

#[test]
fn life_objects_round_trip_with_dedup_across_orientations() {
    let lib = temp_lib("roundtrip");
    let w = life_world(LIFE_SET);
    let recs = records_of(&w, "test");
    assert_eq!(recs.len(), 10);
    for r in recs.clone() {
        lib.add(r).unwrap();
    }
    let all = lib.list().unwrap();
    assert_eq!(
        all.len(),
        5,
        "{:?}",
        all.iter().map(|r| r.label()).collect::<Vec<_>>()
    );
    for n in ["block", "beehive", "blinker", "glider", "LWSS"] {
        let r = by_name(&all, n);
        assert_eq!(r.seen, 2, "{n}");
        assert!(r.verified.periods >= 4, "{n}");
        assert_eq!(r.state, CurationState::New);
    }
    let g = by_name(&all, "glider");
    assert_eq!(
        (g.class, g.period, g.cells),
        (PatternClass::Spaceship, 4, 5)
    );
    let d = g.displacement.unwrap();
    assert_eq!((d[0].abs(), d[1].abs()), (1, 1));
    assert_eq!(g.speed.as_deref(), Some("c/4 diagonal"));
    let l = by_name(&all, "LWSS");
    let d = l.displacement.unwrap();
    assert_eq!(d[0].abs() + d[1].abs(), 2);
    assert_eq!(by_name(&all, "blinker").class, PatternClass::Oscillator);
    assert_eq!(by_name(&all, "block").class, PatternClass::StillLife);
    // Interest: spaceships above oscillators above common still lifes.
    assert!(g.score > by_name(&all, "blinker").score);
    assert!(by_name(&all, "blinker").score > by_name(&all, "block").score);
    // The catalog round-trips the record exactly, and get() takes a prefix or a name.
    let back = lib.get(&g.id).unwrap();
    assert_eq!(&back, g);
    assert_eq!(back.rules, vec!["B3/S23".to_string()]);
    let exported = lib
        .export(std::slice::from_ref(&g.id), &temp_dir("roundtrip-one"))
        .unwrap();
    let text = std::fs::read_to_string(&exported[0]).unwrap();
    assert_eq!(&PatternRecord::from_json(&text).unwrap(), g);
    assert_eq!(lib.get(&g.id[..6]).unwrap().id, g.id);
    assert_eq!(lib.get("Glider").unwrap().id, g.id);
    // Adding again merges: seen counts add up, the earliest provenance stays.
    let first = g.provenance.clone();
    let again = records_of(&life_world(LIFE_SET), "later");
    let out = lib
        .add(again.into_iter().find(|r| r.id == g.id).unwrap())
        .unwrap();
    assert_eq!(
        out,
        AddOutcome::Merged {
            seen: 3,
            new_rule: false
        }
    );
    assert_eq!(lib.get(&g.id).unwrap().provenance, first);
    // Queries.
    let ships = lib
        .query(&Query {
            class: Some(PatternClass::Spaceship),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ships.len(), 2);
    let osc = lib
        .query(&Query {
            min_period: Some(2),
            rule: Some("b3/s23".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(osc.len(), 3); // blinker, glider, LWSS
    // Curation.
    let u = lib
        .update(
            "glider",
            &library::Update {
                add_tags: vec!["classic".into()],
                notes: Some("The smallest spaceship.".into()),
                rating: Some(5),
                state: Some(CurationState::Reviewed),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        (u.rating, u.state, u.tags.clone()),
        (
            Some(5),
            CurationState::Reviewed,
            vec!["classic".to_string()]
        )
    );
    assert_eq!(
        lib.query(&Query {
            tag: Some("classic".into()),
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    // Export writes one JSON file per pattern; import re-verifies them into another catalog,
    // and a whole catalog file imports too.
    let dir = temp_dir("roundtrip-export");
    assert_eq!(lib.export(&[], &dir).unwrap().len(), 5);
    let third = temp_lib("roundtrip-import");
    let done = third.import(&dir).unwrap();
    assert_eq!(done.len(), 5);
    let fourth = temp_lib("roundtrip-import-db");
    assert_eq!(fourth.import(lib.path()).unwrap().len(), 5);
    assert_eq!(
        third.get("glider").unwrap().notes,
        "The smallest spaceship."
    );
    lib.remove("block").unwrap();
    assert!(lib.get("block").is_err());
}

#[test]
fn compound_objects_split_into_their_parts() {
    // A toad (vertical phase: two pieces that die alone) two rows from a beehive: the object
    // finder reports one 12-cell p2 object; the library stores a toad and a beehive.
    let pats = [
        ("..o./o..o/o..o/.o..", 10, 10),
        ("..o../.o.o./.o.o./..o..", 13, 14),
    ];
    let w = life_world(&pats);
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(r.objects.len(), 1, "{}", r.summary_line());
    let o = &r.objects[0];
    let whole = record_from_object(&w, 1, o, prov("t"), DiscoveredBy::User).unwrap();
    assert_eq!((whole.class, whole.cells), (PatternClass::Oscillator, 12));
    let parts =
        library::split::records_from_object(&w, 1, o, prov("t"), DiscoveredBy::User).unwrap();
    let alone = records_of(
        &life_world(&[(".ooo/ooo.", 5, 5), (".oo./o..o/.oo.", 30, 30)]),
        "t",
    );
    let mut got: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
    let mut want: Vec<&str> = alone.iter().map(|p| p.id.as_str()).collect();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want);
    // Objects that really are one (the pentadecathlon, whose phases also fall apart) stay whole.
    let w = life_world(&[("..o....o../oo.oooo.oo/..o....o..", 10, 10)]);
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let parts =
        library::split::records_from_object(&w, 1, &r.objects[0], prov("t"), DiscoveredBy::User)
            .unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].period, 15);
}

#[test]
fn verification_rejects_patterns_that_do_not_repeat() {
    let lib = temp_lib("reject");
    let recs = records_of(
        &life_world(&[(".o./..o/ooo", 10, 10), ("oo/oo", 40, 40)]),
        "test",
    );
    let glider = by_name(&recs, "glider").clone();
    assert!(verify(&glider, 16).is_ok());
    // The R-pentomino claimed as a still life.
    let mut bad = by_name(&recs, "block").clone();
    bad.phases = vec!["1,0,0,1 2,0,0,1 0,1,0,1 1,1,0,1 1,2,0,1".into()];
    let e = verify(&bad, 4).unwrap_err();
    assert!(e.contains("generation 1"), "{e}");
    assert!(lib.add(bad).is_err());
    // A glider with the wrong displacement.
    let mut bad = glider.clone();
    bad.displacement = Some([1, -1]);
    assert!(verify(&bad, 4).is_err());
    // A blinker claimed to have period 1.
    let mut bad = glider.clone();
    bad.class = PatternClass::Oscillator;
    bad.period = 2;
    bad.displacement = None;
    bad.phases = vec![
        "0,0,0,1 1,0,0,1 2,0,0,1".into(),
        "0,0,0,1 1,0,0,1 2,0,0,1".into(),
    ];
    assert!(verify(&bad, 4).is_err());
    assert!(lib.list().unwrap().is_empty());
}

fn small_search(rules: &[&str], tilings: &str, soups: u32, gens: u64) -> SearchSpec {
    SearchSpec {
        rules: rules.iter().map(|s| s.to_string()).collect(),
        tilings: library::tiling_list(tilings).unwrap(),
        size: SizeSpec::Rect { w: 40, h: 40 },
        boundary: Boundary::Wrap,
        soups,
        gens,
        seed: 7,
        ..Default::default()
    }
}

#[test]
fn search_is_identical_at_any_thread_count() {
    let spec = small_search(&["B3/S23", "B2/S34"], "square,hex,oct", 6, 600);
    let run = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| search(&spec, None).unwrap())
    };
    let (a, b) = (run(1), run(8));
    assert_eq!(a.report, b.report);
    let key = |o: &library::search::SearchOutput| {
        o.records
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    r.seen,
                    r.phases.clone(),
                    r.provenance.rng_state,
                    r.provenance.generation,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&a), key(&b));
    let life = &a.report.groups[0];
    assert_eq!(
        (life.rule.as_str(), life.tiling_spec.family.as_str()),
        ("B3/S23", "square")
    );
    // Life's commonest objects come out in their well-known order.
    let names: Vec<&str> = life.found.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(&names[..2], ["block", "blinker"], "{names:?}");
    assert!(a.records.iter().all(|r| r.verified.periods > 0));
}

#[test]
fn tiling_symmetry_groups_used_for_canonical_forms() {
    // Point groups of the wallpaper groups: square p4m 8, hexagon p6m 12, 4.8.8 p4m 8,
    // 3.6.3.6 p6m 12, 3.3.4.3.4 p4g 8, 3.3.3.3.6 p6 6 (chiral), 3.4.6.4, 3.12.12, 4.6.12 p6m 12,
    // 3.3.3.4.4 cmm 4.
    for (fam, n) in [
        ("square", 8),
        ("hex", 12),
        ("oct", 8),
        ("trihex", 12),
        ("snubsq", 8),
        ("snubhex", 6),
        ("rhombitrihex", 12),
        ("trunchex", 12),
        ("trunctrihex", 12),
        ("elongtri", 4),
    ] {
        let topo = Topology::build(&TilingSpec::rect(fam, 8, 8, Boundary::Wrap)).unwrap();
        for nb in [Nb::Touch, Nb::Edge] {
            let maps = library::canonical_symmetries(&topo, nb, true);
            assert_eq!(maps.len(), n, "{fam} {nb:?}");
        }
        assert_eq!(
            library::canonical_symmetries(&topo, Nb::Touch, false).len(),
            1
        );
    }
}

#[test]
fn spaceships_and_oscillators_on_multi_type_tilings() {
    // B36/S23 on the trihexagonal tiling (3.6.3.6: one hexagon and two triangles per unit cell)
    // has small c/4 spaceships; B2/S3 on 4.8.8 (octagon + square) has oscillators.
    let spec = small_search(&["B36/S23"], "trihex", 12, 800);
    let spec = SearchSpec {
        size: SizeSpec::Rect { w: 48, h: 48 },
        seed: 1,
        ..spec
    };
    let out = search(&spec, None).unwrap();
    let ship = out
        .records
        .iter()
        .find(|r| r.class == PatternClass::Spaceship)
        .expect("a spaceship with B36/S23 on 3.6.3.6")
        .clone();
    assert_eq!(ship.tile_types, 3);
    // Copies flying in all six directions are one record (the canonical form takes the tiling's
    // 12 symmetries, with their tile-type permutations, into account).
    let ships: Vec<_> = out.report.groups[0]
        .found
        .iter()
        .filter(|f| f.class == PatternClass::Spaceship)
        .collect();
    assert_eq!(ships.len(), 1, "{ships:?}");
    assert!(ships[0].count >= 6);
    assert_ne!(ship.displacement, Some([0, 0]));
    assert!(verify(&ship, 16).is_ok());
    // Every phase, and the phase one period later, uses the same tile types: the displacement is
    // a translation of the unit cell.
    let types = |v: &[jetcarp_core::objects::LCell]| {
        let mut k: Vec<u32> = v.iter().map(|c| c.k).collect();
        k.sort_unstable();
        k
    };
    assert_eq!(
        types(&ship.phase(0).unwrap()),
        types(&ship.phase(ship.period).unwrap())
    );
    // Run it on a world through PastePattern: after one period the same cells (same types) appear
    // shifted by the displacement.
    let mut w = World::new(TilingSpec::rect("trihex", 24, 24, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new(&ship.rule).seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::PastePattern {
        id: 1,
        pattern: library::paste::source_from_record(&ship, 0),
        at: PasteAt::Cell { x: 10, y: 10 },
        orientation: 0,
        phase: 0,
    })
    .unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let live = |w: &World| -> Vec<(i32, i32, u32)> {
        let mut v: Vec<(i32, i32, u32)> = w.fields[0]
            .state
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .map(|(x, _)| {
                let (u, j, k) = g.decompose(x);
                (u + g.row_start[j as usize], j, k)
            })
            .collect();
        v.sort_unstable();
        v
    };
    let before = live(&w);
    w.apply(Command::Step { gens: ship.period }).unwrap();
    let d = ship.displacement.unwrap();
    let mut moved: Vec<(i32, i32, u32)> = before
        .iter()
        .map(|&(i, j, k)| (i + d[0], j + d[1], k))
        .collect();
    moved.sort_unstable();
    assert_eq!(live(&w), moved);
    // The same cells on the wrong tile types do not verify.
    let mut bad = ship.clone();
    bad.phases = bad
        .phases
        .iter()
        .map(|p| {
            let v: Vec<jetcarp_core::objects::LCell> = library::decode_cells(p)
                .unwrap()
                .into_iter()
                .map(|c| jetcarp_core::objects::LCell {
                    k: (c.k + 1) % 3,
                    ..c
                })
                .collect();
            library::encode_cells(&v)
        })
        .collect();
    assert!(verify(&bad, 4).is_err());

    // An oscillator on 4.8.8.
    let spec = SearchSpec {
        seed: 1,
        ..small_search(&["B2/S3"], "oct", 6, 600)
    };
    let out = search(&spec, None).unwrap();
    let osc = out
        .records
        .iter()
        .find(|r| r.class == PatternClass::Oscillator)
        .expect("an oscillator on 4.8.8");
    assert_eq!(osc.tile_types, 2);
    assert!(verify(osc, 8).is_ok());
}

#[test]
fn collisions_glider_into_block() {
    let recs = records_of(
        &life_world(&[(".o./..o/ooo", 10, 10), ("oo/oo", 40, 40)]),
        "test",
    );
    let (block, glider) = (by_name(&recs, "block"), by_name(&recs, "glider"));
    // Turn the glider so it flies up and to the left, towards the block at the origin.
    let topo = Topology::build(&TilingSpec::rect("square", 8, 8, Boundary::Dead)).unwrap();
    let d = glider.displacement.unwrap();
    let o = library::paste::orientations(&topo, glider.neighborhood)
        .iter()
        .position(|m| m.vector(d) == [-1, -1])
        .unwrap() as u32;
    let spec = CollideSpec {
        dx: [-2, 5],
        dy: [3, 5],
        orientation_b: o,
        gens: 600,
        ..Default::default()
    };
    let out = collide(block, glider, &spec, 0, None).unwrap();
    let r = &out.report;
    println!("{:?}", r.counts);
    for x in &r.runs {
        println!(
            "{:>3},{:>3} p{} {:<13} {}",
            x.dx,
            x.dy,
            x.phase,
            x.outcome.name(),
            x.result
        );
    }
    // Outcomes at some lanes, checked against an independent simulation (a plain Python set-of-
    // cells Life with the same placements): annihilation, the four-beehive "honey farm", a 55-cell
    // debris field, the block alone, a clean miss.
    let at = |dx: i32, dy: i32| {
        r.runs
            .iter()
            .find(|x| (x.dx, x.dy, x.phase) == (dx, dy, 0))
            .unwrap()
    };
    assert_eq!(at(4, 4).outcome, Outcome::BothDie);
    assert_eq!(
        (at(-1, 4).outcome, at(-1, 4).result.as_str()),
        (Outcome::NewObjects, "beehive x 4")
    );
    assert_eq!(at(0, 4).outcome, Outcome::NewObjects);
    assert_eq!(at(0, 4).result, "block x 6, blinker (p2) x 5, pond x 2");
    assert_eq!(at(-2, 4).outcome, Outcome::ASurvives);
    assert_eq!(at(-2, 5).outcome, Outcome::BothSurvive);
    assert_eq!(at(0, 3).outcome, Outcome::Overlap);
    let has = |o: Outcome| r.runs.iter().any(|x| x.outcome == o);
    // Known outcomes of glider + block: the block destroyed, debris left, or a clean miss.
    assert!(
        has(Outcome::BothDie) || has(Outcome::NewObjects),
        "{:?}",
        r.counts
    );
    assert!(has(Outcome::BothSurvive), "{:?}", r.counts);
    assert!(
        r.runs
            .iter()
            .all(|x| x.outcome != Outcome::Unsettled && x.outcome != Outcome::Explodes)
    );
    // A glider far to the side misses: both survive.
    let far = CollideSpec {
        dx: [12, 12],
        dy: [-12, -12],
        phases: vec![0],
        orientation_b: o,
        gens: 200,
        ..Default::default()
    };
    let out2 = collide(block, glider, &far, 0, None).unwrap();
    assert_eq!(out2.report.runs[0].outcome, Outcome::BothSurvive);
    // Products are verified records.
    assert!(out.records.iter().all(|p| p.verified.periods > 0));
}

#[test]
fn paste_pattern_command_replays_and_refuses_bad_spots() {
    let recs = records_of(&life_world(&[(".o./..o/ooo", 10, 10)]), "test");
    let glider = by_name(&recs, "glider");
    let mut sc = Scene::new(TilingSpec::rect("square", 32, 32, Boundary::Dead));
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Empty)];
    sc.commands = vec![
        Command::PastePattern {
            id: 1,
            pattern: library::paste::source_from_record(glider, 0),
            at: PasteAt::Cell { x: 5, y: 5 },
            orientation: 3,
            phase: 0,
        },
        Command::Step { gens: 8 },
    ];
    let w = World::from_scene(&sc).unwrap();
    assert_eq!(w.fields[0].population(), 5);
    // Through JSON and back: the same hash.
    let w2 = World::from_scene(&Scene::from_json(&w.to_scene().to_json()).unwrap()).unwrap();
    assert_eq!(w.hash_hex(), w2.hash_hex());
    // Off a dead edge: refused with a clear message.
    let mut bad = World::new(TilingSpec::rect("square", 16, 16, Boundary::Dead), 1).unwrap();
    bad.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    let e = bad
        .apply(Command::PastePattern {
            id: 1,
            pattern: library::paste::source_from_record(glider, 0),
            at: PasteAt::Cell { x: 15, y: 2 },
            orientation: 0,
            phase: 0,
        })
        .unwrap_err();
    assert!(e.contains("does not fit"), "{e}");
    // On another tiling: refused.
    let mut hex = World::new(TilingSpec::rect("hex", 16, 16, Boundary::Wrap), 1).unwrap();
    hex.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    let e = hex
        .apply(Command::PastePattern {
            id: 1,
            pattern: library::paste::source_from_record(glider, 0),
            at: PasteAt::Center,
            orientation: 0,
            phase: 0,
        })
        .unwrap_err();
    assert!(e.contains("belongs to"), "{e}");
}

#[test]
fn promoted_patterns_appear_in_quick_start() {
    let lib = temp_lib("promote");
    for r in records_of(
        &life_world(&[(".o./..o/ooo", 10, 10), ("ooo", 40, 40)]),
        "test",
    ) {
        lib.add(r).unwrap();
    }
    let (sc, entry) = library::promote::promote(&lib, "glider", Some("My glider"), None).unwrap();
    let glider = lib.get("glider").unwrap();
    assert_eq!(entry, glider.id);
    assert_eq!(glider.state, CurationState::Promoted);
    let finds = quickstart::user_finds(&lib);
    assert_eq!(finds.len(), 1);
    let q = &finds[0];
    assert_eq!(q.id, format!("find:{}", glider.id));
    let row = lib.get_quickstart(&q.id).unwrap();
    assert_eq!(
        (row.source_kind.as_str(), row.behaviour.as_str()),
        ("pattern", "spaceship")
    );
    // A candidate waits in the queue and is not shown; removing the pattern removes its entry.
    lib.set_quickstart_status(&q.id, library::QsStatus::Candidate)
        .unwrap();
    assert!(quickstart::user_finds(&lib).is_empty());
    lib.set_quickstart_status(&q.id, library::QsStatus::Approved)
        .unwrap();
    assert_eq!(
        (q.family.as_str(), q.scene.name.as_str()),
        (quickstart::MY_FINDS, "My glider")
    );
    assert_eq!(q.scene, sc);
    // It works as a Quick Start example: alive after 300 generations at preview size, the glider
    // still a glider (wrapped edges).
    let h = quickstart::health(&q.scene, 300, quickstart::preview_cells(&q.scene, 6000)).unwrap();
    assert_eq!(h.population, 5);
    // The built-in examples are unaffected.
    assert!(
        quickstart::entries()
            .iter()
            .all(|e| e.family != quickstart::MY_FINDS)
    );
    lib.remove("glider").unwrap();
    assert!(quickstart::user_finds(&lib).is_empty());
}

#[test]
fn aperiodic_patterns_paste_only_where_the_tiles_match() {
    // An oscillator found by a soup search on a Spectre patch.
    let spec = SearchSpec {
        rules: vec!["B35/S234".into()],
        tilings: vec![TilingSpec::new("spectre")],
        size: SizeSpec::Level(4),
        boundary: Boundary::Dead,
        soups: 3,
        gens: 600,
        seed: 1,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    let rec = out
        .records
        .iter()
        .filter(|r| r.class == PatternClass::Oscillator)
        .max_by_key(|r| (r.period, r.cells))
        .expect("an oscillator on the Spectre patch")
        .clone();
    let site = rec
        .site
        .clone()
        .expect("aperiodic records carry their site");
    assert!(site.tiles.len() > rec.cells);
    assert!(verify(&rec, 4).is_ok());
    let lib = temp_lib("aperiodic");
    lib.add(rec.clone()).unwrap();
    let back = lib.get(&rec.id).unwrap();
    assert_eq!(back.site, rec.site);

    let paste_at = |w: &mut World, at: PasteAt| {
        w.apply(Command::PastePattern {
            id: 1,
            pattern: library::paste::source_from_record(&rec, 0),
            at,
            orientation: 0,
            phase: 0,
        })
    };
    let topo = std::sync::Arc::new(Topology::build(&site.patch).unwrap());
    let fresh = || {
        let mut w = World::on_topology(topo.clone(), 1);
        w.apply(Command::AddCa {
            ca: CaSpec::new(&rec.rule).seed(SeedSpec::Empty),
        })
        .unwrap();
        w
    };
    // The original site: accepted, and the pattern oscillates there with its period.
    let mut w = fresh();
    paste_at(&mut w, PasteAt::Original).unwrap();
    let start = w.fields[0].state.clone();
    assert_eq!(w.fields[0].population(), rec.cells as u64);
    w.apply(Command::Step { gens: rec.period }).unwrap();
    assert_eq!(w.fields[0].state, start);
    // Every other tile: refused unless its surroundings match; where they do, it works too.
    let (mut refused, mut matched) = (0, 0);
    for cell in (0..topo.len()).step_by(7) {
        if cell == site.anchor as usize {
            continue;
        }
        let mut w = fresh();
        match paste_at(&mut w, PasteAt::Index { cell: cell as u32 }) {
            Err(e) => {
                assert!(e.contains("do not match") || e.contains("shape"), "{e}");
                assert_eq!(
                    w.fields[0].population(),
                    0,
                    "a refused paste changes nothing"
                );
                refused += 1;
            }
            Ok(_) => {
                let s0 = w.fields[0].state.clone();
                w.apply(Command::Step { gens: rec.period }).unwrap();
                assert_eq!(
                    w.fields[0].state, s0,
                    "pasted at a matching site {cell}, it must oscillate"
                );
                matched += 1;
            }
        }
    }
    println!("refused {refused}, matched {matched}");
    assert!(refused > 100);
}

#[test]
fn paste_at_cell_uses_grid_columns_on_multi_type_tilings() {
    // On 3.6.3.6 (3 tiles per unit cell) a row of the grid has 3 * width columns, as in MCP and
    // set_cells; pasting at (x, y) puts the pattern's corner in the unit cell holding column x.
    let mut w = World::new(TilingSpec::rect("trihex", 20, 20, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    let src = library::PatternSource {
        cells: Some("0,0,0,1".into()),
        tiling: Some(TilingSpec::new("trihex")),
        ..Default::default()
    };
    w.apply(Command::PastePattern {
        id: 1,
        pattern: src,
        at: PasteAt::Cell { x: 31, y: 7 },
        orientation: 0,
        phase: 0,
    })
    .unwrap();
    let live: Vec<usize> = (0..w.topo.len())
        .filter(|&c| w.fields[0].state[c] != 0)
        .collect();
    assert_eq!(live.len(), 1);
    let g = w.topo.lattice().unwrap();
    let (u, j, k) = g.decompose(live[0]);
    // Row 7, unit cell 31 / 3 = 10 within the row, tile type 0.
    assert_eq!((u, j, k), (10, 7, 0), "cell {}", live[0]);
}
