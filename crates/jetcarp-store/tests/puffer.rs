//! A puffer found by the object detector, stored in the pattern library, read back and verified.
//! (Puffer detection itself is tested in jetcarp-core.)

use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{PatternClass, Provenance, record_from_object, verify};
use jetcarp_core::topology::{Boundary, TilingSpec};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};
use jetcarp_store::Library;

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

/// The 10-cell infinite-growth pattern of Life (Paul Callahan's, entered by hand from its
/// well-known cell coordinates): it turns into a block-laying switch engine, a puffer moving
/// c/12 diagonally with period 288 that leaves a trail of blocks.
const TEN_CELL: &str = "......o./....o.oo/....o.o./....o.../..o...../o.o.....";
