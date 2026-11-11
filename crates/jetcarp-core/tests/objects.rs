//! Phase 2: still-life, oscillator, and spaceship detection.

use jetcarp_core::objects::{ObjClass, ObjectOptions, ObjectReport, find_objects, place_rows};
use jetcarp_core::*;

fn torus(w: u32, h: u32) -> TilingSpec {
    TilingSpec::rect("square", w, h, Boundary::Wrap)
}

/// A world with one CA and the given patterns (rows, column, row) placed on it.
fn world_with(spec: TilingSpec, rule: &str, pats: &[(&str, i32, i32)]) -> World {
    let mut w = World::new(spec, 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = Vec::new();
    for &(rows, u, j) in pats {
        cells.extend(place_rows(&g, rows, u, j));
    }
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    w
}

fn count(r: &ObjectReport, name: &str) -> usize {
    r.counts
        .iter()
        .filter(|c| c.name == name)
        .map(|c| c.count)
        .sum()
}

fn class_of(r: &ObjectReport, name: &str) -> ObjClass {
    r.counts
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no {name} in {}", r.summary_line()))
        .class
        .clone()
}

#[test]
fn known_life_objects() {
    let pulsar = "..ooo...ooo../............./o....o.o....o/o....o.o....o/o....o.o....o/..ooo...ooo../\
                  ............./..ooo...ooo../o....o.o....o/o....o.o....o/o....o.o....o/............./\
                  ..ooo...ooo..";
    // Rotated or reflected copies must count with the originals.
    let pats = [
        ("oo/oo", 2, 2),
        ("oo/oo", 10, 2),
        (".oo./o..o/.oo.", 18, 2),
        (".o/o.o/o.o/.o", 26, 2), // beehive, rotated
        (".oo./o..o/.o.o/..o.", 34, 2),
        ("oo./o.o/.o.", 42, 2),
        (".o./o.o/oo.", 50, 2), // boat, rotated
        ("ooo", 2, 12),
        ("o/o/o", 10, 10),
        (".ooo/ooo.", 18, 12),
        ("oo../oo../..oo/..oo", 28, 11),
        (pulsar, 40, 12),
        ("..o....o../oo.oooo.oo/..o....o..", 2, 32),
        (".o./..o/ooo", 2, 45),
        ("ooo/o../.o.", 20, 45), // glider, another orientation and phase
        (".o..o/o..../o...o/oooo.", 34, 45),
    ];
    let w = world_with(torus(80, 64), "B3/S23", &pats);
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let line = r.summary_line();
    println!("{line}");
    assert_eq!(count(&r, "block"), 2, "{line}");
    assert_eq!(count(&r, "beehive"), 2, "{line}");
    assert_eq!(count(&r, "loaf"), 1, "{line}");
    assert_eq!(count(&r, "boat"), 2, "{line}");
    assert_eq!(count(&r, "blinker"), 2, "{line}");
    assert_eq!(count(&r, "toad"), 1, "{line}");
    assert_eq!(count(&r, "beacon"), 1, "{line}");
    assert_eq!(count(&r, "pulsar"), 1, "{line}");
    assert_eq!(count(&r, "pentadecathlon"), 1, "{line}");
    assert_eq!(count(&r, "glider"), 2, "{line}");
    assert_eq!(count(&r, "LWSS"), 1, "{line}");
    assert_eq!(r.objects.len(), 16, "{line}");
    assert_eq!(class_of(&r, "block"), ObjClass::StillLife);
    assert_eq!(class_of(&r, "blinker"), ObjClass::Oscillator { period: 2 });
    assert_eq!(class_of(&r, "toad"), ObjClass::Oscillator { period: 2 });
    assert_eq!(class_of(&r, "beacon"), ObjClass::Oscillator { period: 2 });
    assert_eq!(class_of(&r, "pulsar"), ObjClass::Oscillator { period: 3 });
    assert_eq!(
        class_of(&r, "pentadecathlon"),
        ObjClass::Oscillator { period: 15 }
    );
    match class_of(&r, "glider") {
        ObjClass::Spaceship { period: 4, dx, dy } => assert!(dx.abs() == 1 && dy.abs() == 1),
        c => panic!("glider: {c:?}"),
    }
    let g = class_of(&r, "glider");
    assert_eq!(g.speed_in(r.lattice).unwrap(), "c/4 diagonal");
    match class_of(&r, "LWSS") {
        ObjClass::Spaceship { period: 4, dx, dy } => {
            assert_eq!((dx.abs(), dy.abs()), (2, 0))
        }
        c => panic!("LWSS: {c:?}"),
    }
    assert_eq!(
        class_of(&r, "LWSS").speed_in(r.lattice).unwrap(),
        "c/2 orthogonal"
    );
    assert!(line.contains("glider (c/4) x 2"), "{line}");
    assert!(line.contains("blinker (p2) x 2"), "{line}");
    // Every live cell belongs to exactly one object.
    let total: usize = r.objects.iter().map(|o| o.size).sum();
    assert_eq!(total as u64, r.live_cells);
}

#[test]
fn objects_after_some_generations_and_other_phases() {
    // The same objects found after running (other phases, gliders moved across the seam).
    let pats = [
        (".o./..o/ooo", 2, 2),
        ("ooo", 30, 30),
        ("..o....o../oo.oooo.oo/..o....o..", 10, 40),
        ("oo/oo", 50, 10),
    ];
    let mut w = world_with(torus(64, 64), "B3/S23", &pats);
    w.apply(Command::Step { gens: 7 }).unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(count(&r, "glider"), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "blinker"), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "pentadecathlon"), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "block"), 1, "{}", r.summary_line());
}

#[test]
fn blinker_next_to_block_is_separated() {
    // Two cells apart: grouped by distance, then split because they do not interact.
    let w = world_with(
        torus(32, 32),
        "B3/S23",
        &[("oo..../oo..../....../...ooo", 4, 4)],
    );
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(count(&r, "block"), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "blinker"), 1, "{}", r.summary_line());
    // Bi-block: two blocks one column apart are two blocks.
    let w = world_with(torus(32, 32), "B3/S23", &[("oo.oo/oo.oo", 4, 4)]);
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(count(&r, "block"), 2, "{}", r.summary_line());
    // A blinker that would touch a block: one interacting object, not a block plus a blinker.
    let w = world_with(
        torus(32, 32),
        "B3/S23",
        &[("oo../oo../...o/...o/...o", 4, 4)],
    );
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(r.objects.len(), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "blinker"), 0, "{}", r.summary_line());
}

#[test]
fn life_soup_census_is_plausible() {
    let mut w = World::new(torus(96, 96), 7).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.37 }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 2000 }).unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let line = r.summary_line();
    println!("{} objects: {line}", r.objects.len());
    let common = count(&r, "block") + count(&r, "blinker") + count(&r, "beehive");
    assert!(r.objects.len() >= 20, "{line}");
    assert!(
        common * 2 >= r.objects.len(),
        "blocks, blinkers, and beehives should dominate: {line}"
    );
    assert!(count(&r, "block") >= count(&r, "loaf"), "{line}");
    let total: usize = r.objects.iter().map(|o| o.size).sum();
    assert_eq!(total as u64, r.live_cells);
    // Identical at any thread count.
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| find_objects(&w, &ObjectOptions::default()).unwrap());
    assert_eq!(one.to_json(), r.to_json());
}

#[test]
fn highlife_replicator_is_other() {
    let w = world_with(
        torus(96, 96),
        "B36/S23",
        &[("..ooo/.o..o/o...o/o..o./ooo..", 45, 45)],
    );
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(r.objects.len(), 1, "{}", r.summary_line());
    assert!(
        matches!(&r.objects[0].class, ObjClass::Other { reason } if reason == "grows"),
        "{:?}",
        r.objects[0].class
    );
    // Life objects are still named under HighLife when they behave the same.
    let w = world_with(
        torus(32, 32),
        "B36/S23",
        &[("oo/oo", 4, 4), ("ooo", 14, 14)],
    );
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert_eq!(count(&r, "block"), 1, "{}", r.summary_line());
    assert_eq!(count(&r, "blinker"), 1, "{}", r.summary_line());
}

#[test]
fn other_tilings_run_without_panics() {
    for (family, rule) in [
        ("hex", "B2/S34"),
        ("oct", "B3/S23"),
        ("trihex", "B3/S23"),
        ("cairo", "B3/S23"),
    ] {
        for b in [Boundary::Wrap, Boundary::Dead] {
            let spec = TilingSpec::new(family)
                .with_size(SizeSpec::Cells(2500))
                .with_boundary(b);
            let mut w = World::new(spec, 3).unwrap();
            w.apply(Command::AddCa {
                ca: CaSpec::new(rule).seed(SeedSpec::Random { density: 0.3 }),
            })
            .unwrap();
            w.apply(Command::Step { gens: 300 }).unwrap();
            let r = find_objects(&w, &ObjectOptions::default()).unwrap();
            let total: usize = r.objects.iter().map(|o| o.size).sum();
            assert_eq!(total as u64, r.live_cells, "{family}");
            println!(
                "{family} {b:?} {rule}: {} objects, symmetries {}: {}",
                r.objects.len(),
                r.symmetries,
                r.summary_line()
            );
        }
    }
    // Spectre (explicit storage): runs, canonical forms are invariants.
    let mut w = World::new(TilingSpec::new("spectre").with_size(SizeSpec::Level(3)), 2).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 200 }).unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    assert!(r.objects.iter().all(|o| !o.canonical_exact));
    println!("spectre: {}", r.summary_line());
}

#[test]
fn hex_symmetry_group_has_twelve_elements() {
    let w = world_with(torus(16, 16), "B3/S23", &[]);
    assert_eq!(
        find_objects(&w, &ObjectOptions::default())
            .unwrap()
            .symmetries,
        8
    );
    let mut w = World::new(TilingSpec::rect("hex", 16, 16, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B2/S34").seed(SeedSpec::Empty),
    })
    .unwrap();
    assert_eq!(
        find_objects(&w, &ObjectOptions::default())
            .unwrap()
            .symmetries,
        12
    );
}

const PULSAR: &str = "..ooo...ooo../............./o....o.o....o/o....o.o....o/o....o.o....o/\
                      ..ooo...ooo../............./..ooo...ooo../o....o.o....o/o....o.o....o/\
                      o....o.o....o/............./..ooo...ooo..";

#[test]
fn every_phase_is_recognised() {
    let pats = [
        ("..o....o../oo.oooo.oo/..o....o..", 10, 10),
        (PULSAR, 40, 30),
        (".o./..o/ooo", 10, 40),
        (".o..o/o..../o...o/oooo.", 30, 50),
        ("...o../.o...o/o...../o....o/ooooo.", 50, 5),
        ("...oo../.o....o/o....../o.....o/oooooo.", 10, 60),
    ];
    let mut w = world_with(torus(80, 80), "B3/S23", &pats);
    for t in 0..15 {
        let r = find_objects(&w, &ObjectOptions::default()).unwrap();
        let line = r.summary_line();
        for name in ["pentadecathlon", "pulsar", "glider", "LWSS", "MWSS", "HWSS"] {
            assert_eq!(count(&r, name), 1, "generation {t}: {line}");
        }
        w.apply(Command::Step { gens: 1 }).unwrap();
    }
}

/// Speed directions are named in each lattice's own terms: on a hexagonal lattice JETCARP's
/// basis vectors are 60° apart, so `(1, -1)` is a shortest step (orthogonal) and `(1, 1)` lies
/// halfway between two (diagonal), where the square grid's words said the opposite.
#[test]
fn speed_directions_follow_the_lattice() {
    use jetcarp_core::objects::{LatticeShape, direction, reduce_basis};
    let basis = |id: &str| {
        let topo = Topology::build(&TilingSpec::rect(id, 8, 8, Boundary::Wrap)).unwrap();
        let g = topo.lattice().unwrap();
        (g.a1, g.a2)
    };
    let (a1, a2) = basis("square");
    assert_eq!(reduce_basis(a1, a2).0, LatticeShape::Square);
    for (d, w) in [
        ([1, 0], "orthogonal"),
        ([0, -2], "orthogonal"),
        ([1, 1], "diagonal"),
        ([-3, 3], "diagonal"),
        ([2, 1], "oblique"),
    ] {
        assert_eq!(direction(d, a1, a2), Some(w), "square {d:?}");
    }
    for id in ["hex", "trihex", "triangle", "oct"] {
        let (a1, a2) = basis(id);
        if id == "oct" {
            assert_eq!(reduce_basis(a1, a2).0, LatticeShape::Square, "{id}");
            continue;
        }
        assert_eq!(reduce_basis(a1, a2).0, LatticeShape::Hexagonal, "{id}");
        for (d, w) in [
            ([1, 0], "orthogonal"),
            ([0, 1], "orthogonal"),
            ([1, -1], "orthogonal"),
            ([-2, 2], "orthogonal"),
            ([1, 1], "diagonal"),
            ([-3, -3], "diagonal"),
            ([2, -1], "diagonal"),
            ([-1, 2], "diagonal"),
            ([2, 1], "oblique"),
        ] {
            assert_eq!(direction(d, a1, a2), Some(w), "{id} {d:?}");
        }
        let ship = ObjClass::Spaceship {
            period: 12,
            dx: -3,
            dy: 3,
        };
        assert_eq!(ship.speed_in(Some((a1, a2))).unwrap(), "c/4 orthogonal");
        assert_eq!(ship.tag().unwrap(), "c/4");
    }
    // A rectangle's axes are its only directions with a name, and so are a centred lattice's
    // (a rhombus, or rows offset by half a step, as on the elongated triangular tiling); an
    // oblique lattice has none.
    let (r1, r2) = ([2.0, 0.0], [0.0, 1.0]);
    assert!(matches!(
        reduce_basis(r1, r2).0,
        LatticeShape::Rectangular { .. }
    ));
    assert_eq!(direction([3, 0], r1, r2), Some("orthogonal"));
    assert_eq!(direction([1, 1], r1, r2), Some("oblique"));
    let (h1, h2) = ([1.0, 0.0], [0.5, 1.9]);
    assert!(matches!(
        reduce_basis(h1, h2).0,
        LatticeShape::Rectangular { .. }
    ));
    assert_eq!(direction([2, 0], h1, h2), Some("orthogonal"));
    assert_eq!(
        direction([-1, 2], h1, h2),
        Some("orthogonal"),
        "straight up"
    );
    assert_eq!(direction([0, 1], h1, h2), Some("oblique"));
    let (m1, m2) = ([1.0, 0.0], [0.4, 0.916_515_138_991_168]);
    assert!(matches!(
        reduce_basis(m1, m2).0,
        LatticeShape::Rectangular { .. }
    ));
    assert_eq!(direction([1, 1], m1, m2), Some("orthogonal"));
    assert_eq!(direction([1, -1], m1, m2), Some("orthogonal"));
    assert_eq!(direction([1, 0], m1, m2), Some("oblique"));
    let (e1, e2) = basis("elongtri");
    assert!(matches!(
        reduce_basis(e1, e2).0,
        LatticeShape::Rectangular { .. }
    ));
    assert_eq!(
        direction([2, 0], e1, e2),
        Some("orthogonal"),
        "along the rows"
    );
    let (o1, o2) = ([1.0, 0.0], [0.3, 1.1]);
    assert_eq!(reduce_basis(o1, o2).0, LatticeShape::Other);
    assert_eq!(direction([1, 0], o1, o2), None);
    let ship = ObjClass::Spaceship {
        period: 4,
        dx: 1,
        dy: 0,
    };
    assert_eq!(ship.speed_in(Some((o1, o2))).unwrap(), "c/4");
    // A basis given in another form names the same directions: the square grid's, sheared.
    assert_eq!(
        direction([1, -1], [1.0, 0.0], [1.0, 1.0]),
        Some("orthogonal")
    );
    assert_eq!(direction([0, 1], [1.0, 0.0], [1.0, 1.0]), Some("diagonal"));
}
