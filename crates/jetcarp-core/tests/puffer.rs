//! Puffer detection: the front recurs shifted by a lattice vector while the debris grows.

use jetcarp_core::library::{Library, PatternClass, Provenance, record_from_object, verify};
use jetcarp_core::objects::puffer::{Sparse, detect};
use jetcarp_core::objects::{
    LCell, ObjClass, ObjectOptions, find_objects, place_rows, rows_to_cells,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Boundary, Nb, TilingSpec, Topology};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};

/// The 10-cell infinite-growth pattern of Life (Paul Callahan's, entered by hand from its
/// well-known cell coordinates): it turns into a block-laying switch engine, a puffer moving
/// c/12 diagonally with period 288 that leaves a trail of blocks.
const TEN_CELL: &str = "......o./....o.oo/....o.o./....o.../..o...../o.o.....";

fn sparse_life() -> Sparse {
    let topo = Topology::build(&TilingSpec::rect("square", 8, 8, Boundary::Dead)).unwrap();
    Sparse::new(
        &Rule::parse("B3/S23").unwrap().0,
        topo.lattice().unwrap(),
        Nb::Touch,
    )
    .unwrap()
}

fn cells(text: &str) -> Vec<LCell> {
    let mut v: Vec<LCell> = text
        .split(' ')
        .map(|t| {
            let (a, b) = t.split_once(',').unwrap();
            LCell {
                i: a.parse().unwrap(),
                j: b.parse().unwrap(),
                k: 0,
                s: 1,
            }
        })
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn block_laying_switch_engine_is_a_puffer() {
    let sp = sparse_life();
    let f = detect(&sp, &rows_to_cells(TEN_CELL), 300, 20_000).expect("a puffer");
    assert_eq!(f.period, 288);
    assert_eq!(f.d[0].abs(), 24);
    assert_eq!(f.d[1].abs(), 24);
    assert!(f.pop_to > f.pop_from);
}

#[test]
fn spaceship_escaping_from_a_mess_is_not_a_puffer() {
    // Two LWSSs and a B-heptomino (found by a small search): the B-heptomino wrecks one ship,
    // the other flies on ahead of a growing mess. Its front (the LWSS) recurs and the population
    // grows, but the front alone does not grow, so it is not a puffer.
    let sp = sparse_life();
    let start = cells(
        "1,0 4,0 0,1 0,2 4,2 0,3 1,3 2,3 3,3 0,8 1,8 2,8 3,8 0,9 1,9 2,9 4,9 -1,10 0,10 2,10 1,11 4,11",
    );
    assert!(detect(&sp, &start, 64, 20_000).is_none());
    // A growing mess whose thin leading edge happens to recur (c/2, period 2, 4 cells, from
    // the same search): cut off, that edge does not evolve as it did, so it is rejected.
    let edge = cells(
        "1,0 4,0 0,1 0,2 4,2 0,3 1,3 2,3 3,3 6,4 7,4 7,5 8,5 1,6 4,6 6,6 7,6 0,7 6,7 0,8 4,8 0,9 1,9 2,9 3,9",
    );
    assert!(detect(&sp, &edge, 64, 20_000).is_none());
    // Nor are a lone spaceship, an oscillator, or a still life.
    for rows in [".o..o/o..../o...o/oooo.", "ooo", "oo/oo", ".o./..o/ooo"] {
        assert!(
            detect(&sp, &rows_to_cells(rows), 64, 20_000).is_none(),
            "{rows}"
        );
    }
}

#[test]
fn detector_classifies_and_library_stores_a_puffer() {
    let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let id = w.cas[0].id;
    let cells: Vec<[u32; 2]> = place_rows(&g, TEN_CELL, 30, 30);
    w.apply(Command::SetCells { id, cells }).unwrap();
    let rep = find_objects(
        &w,
        &ObjectOptions {
            max_period: 300,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rep.objects.len(), 1, "{}", rep.summary_line());
    let o = &rep.objects[0];
    let ObjClass::Puffer { period, dx, dy } = o.class else {
        panic!("{:?}", o.class);
    };
    assert_eq!((period, dx.abs(), dy.abs()), (288, 24, 24));
    assert_eq!(rep.puffers(), 1);
    assert!(o.name.contains("puffer"), "{}", o.name);
    // Into the library: class puffer, its starting phase, period, displacement, speed; verified
    // by the same exact front recurrence; reloads and re-verifies.
    let rec = record_from_object(&w, id, o, Provenance::default(), Default::default()).unwrap();
    assert_eq!(rec.class, PatternClass::Puffer);
    assert_eq!(rec.period, 288);
    assert_eq!(rec.phases.len(), 1);
    assert_eq!(rec.speed.as_deref(), Some("c/12 diagonal"));
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-puffer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(&dir);
    lib.add(rec.clone()).unwrap();
    let back = lib.get(&rec.id).unwrap();
    assert!(back.verified.periods >= 4, "{:?}", back.verified);
    assert!(back.verified.field.contains("recurs"));
    assert!(verify(&back, 4).is_ok());
    // The same puffer in another orientation gets the same record.
    let mut w2 = World::new(TilingSpec::rect("square", 64, 64, Boundary::Dead), 1).unwrap();
    w2.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    let flipped: String = TEN_CELL
        .split('/')
        .map(|r| r.chars().rev().collect::<String>())
        .collect::<Vec<_>>()
        .join("/");
    let cells2 = place_rows(&g, &flipped, 20, 20);
    let id2 = w2.cas[0].id;
    w2.apply(Command::SetCells {
        id: id2,
        cells: cells2,
    })
    .unwrap();
    let rep2 = find_objects(
        &w2,
        &ObjectOptions {
            max_period: 300,
            ..Default::default()
        },
    )
    .unwrap();
    let rec2 = record_from_object(
        &w2,
        id2,
        &rep2.objects[0],
        Provenance::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(rec2.id, rec.id);
    let _ = std::fs::remove_dir_all(&dir);
}
