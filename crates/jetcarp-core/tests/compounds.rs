//! Compounds: objects that are several independent objects come apart into them (the object
//! finder, the hunter's census, and records), and objects that are one stay one.

use jetcarp_core::hunt::separate::{Cache, Census, Obj};
use jetcarp_core::objects::compound;
use jetcarp_core::objects::{
    LCell, ObjClass, ObjectOptions, ObjectReport, Phase, find_objects, place_rows,
};
use jetcarp_core::pattern::{decode_cells, split};
use jetcarp_core::*;

/// A world on a wrapped grid of `family` with `cells` (`i,j,k,s` tuples, as records write them)
/// pasted at the centre.
fn pasted(family: &str, size: u32, rule: &str, cells: &str) -> World {
    let scene = serde_json::json!({
        "jetcarp_version": VERSION, "format": 1, "name": "t",
        "tiling": {"family": family, "size": {"rect": {"w": size, "h": size}}, "boundary": "wrap"},
        "rng_state": 1,
        "cas": [{"rule": rule, "neighborhood": "touch", "enabled": true, "steps_per_gen": 1,
                 "seed": "empty"}],
        "commands": [{"op": "paste_pattern", "id": 1,
                      "pattern": {"cells": cells, "tiling": {"family": family, "boundary": "dead"}},
                      "at": "center"}]
    });
    World::from_scene(&serde_json::from_value(scene).unwrap()).unwrap()
}

/// Life on a wrapped square grid with patterns (rows, column, row).
fn life(pats: &[(&str, i32, i32)]) -> World {
    let mut w = World::new(TilingSpec::rect("square", 96, 96, Boundary::Wrap), 1).unwrap();
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

fn objects(w: &World) -> ObjectReport {
    find_objects(w, &ObjectOptions::default()).unwrap()
}

/// The object finder without splitting compounds: what it reported before.
fn whole(w: &World) -> ObjectReport {
    let opts = ObjectOptions {
        separate: false,
        ..Default::default()
    };
    find_objects(w, &opts).unwrap()
}

fn count(r: &ObjectReport, name: &str) -> usize {
    r.counts
        .iter()
        .filter(|c| c.name == name)
        .map(|c| c.count)
        .sum()
}

fn step(w: &mut World) {
    w.apply(Command::Step { gens: 1 }).unwrap();
}

/// A world's live cells in lattice coordinates (for the hunter's census).
fn lattice_cells(w: &World) -> Vec<LCell> {
    let g = w.topo.lattice().unwrap();
    let mut v: Vec<LCell> = w.fields[w.cas[0].field]
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != 0)
        .map(|(x, &s)| {
            let (u, j, k) = g.decompose(x);
            LCell {
                i: u + g.row_start[j as usize],
                j,
                k,
                s,
            }
        })
        .collect();
    v.sort_unstable();
    v
}

/// The names of what the hunter's census finds in a world (sorted).
fn census(c: &Census, w: &World) -> Vec<String> {
    let mut cache = Cache::default();
    let mut v: Vec<String> = c
        .census(&mut cache, &lattice_cells(w))
        .iter()
        .map(|p| match &p.obj {
            Obj::Pattern(f) => f.label.clone(),
            Obj::Other(k) => k.name().to_string(),
        })
        .collect();
    v.sort();
    v
}

/// Two gliders side by side, from a 20,000-soup Life hunt that stored them as one "c/4
/// diagonal spaceship 10c" (its four phases as the record wrote them).
const GLIDER_PAIR: [&str; 4] = [
    "0,0,0,1 1,0,0,1 2,0,0,1 0,1,0,1 1,2,0,1 4,3,0,1 5,3,0,1 6,3,0,1 4,4,0,1 5,5,0,1",
    "1,-1,0,1 0,0,0,1 1,0,0,1 0,1,0,1 2,1,0,1 5,2,0,1 4,3,0,1 5,3,0,1 4,4,0,1 6,4,0,1",
    "0,-1,0,1 1,-1,0,1 0,0,0,1 2,0,0,1 0,1,0,1 4,2,0,1 5,2,0,1 4,3,0,1 6,3,0,1 4,4,0,1",
    "0,-1,0,1 1,-1,0,1 -1,0,0,1 0,0,0,1 1,1,0,1 4,2,0,1 5,2,0,1 3,3,0,1 4,3,0,1 5,4,0,1",
];

#[test]
fn a_glider_pair_is_two_gliders() {
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    for (t, cells) in GLIDER_PAIR.iter().enumerate() {
        let mut w = pasted("square", 64, "B3/S23", cells);
        for _ in 0..4 {
            let r = objects(&w);
            assert_eq!(count(&r, "glider"), 2, "phase {t}: {}", r.summary_line());
            assert_eq!(r.objects.len(), 2, "phase {t}: {}", r.summary_line());
            assert_eq!(census(&c, &w), ["glider", "glider"], "phase {t}");
            step(&mut w);
        }
    }
    // Without splitting, in the phases where they lie within two steps of each other, it is the
    // one 10-cell ship the hunt stored.
    let one = GLIDER_PAIR.iter().any(|cells| {
        let r = whole(&pasted("square", 64, "B3/S23", cells));
        r.objects.len() == 1 && matches!(r.objects[0].class, ObjClass::Spaceship { period: 4, .. })
    });
    assert!(one);
    // The split itself, on the record's phases: two gliders moving as the pair does.
    let phases: Vec<Phase> = GLIDER_PAIR
        .iter()
        .map(|p| decode_cells(p).unwrap())
        .collect();
    let parts = compound::split(&c, &phases, [-1, -1]).expect("a compound");
    assert_eq!(parts.len(), 2);
    for p in &parts {
        assert_eq!((p.period, p.d, p.phases[0].len()), (4, [-1, -1], 5));
    }
}

/// A pulsar with four blinkers around it (phase 0 of a "p6 oscillator 60c" the same hunt
/// stored twice, in two arrangements).
const PULSAR_BLINKERS: &str = "2,0,0,1 3,0,0,1 4,0,0,1 8,0,0,1 9,0,0,1 10,0,0,1 0,2,0,1 5,2,0,1 \
    7,2,0,1 12,2,0,1 0,3,0,1 5,3,0,1 7,3,0,1 12,3,0,1 0,4,0,1 5,4,0,1 7,4,0,1 12,4,0,1 2,5,0,1 \
    3,5,0,1 4,5,0,1 8,5,0,1 9,5,0,1 10,5,0,1 2,7,0,1 3,7,0,1 4,7,0,1 8,7,0,1 9,7,0,1 10,7,0,1 \
    0,8,0,1 5,8,0,1 7,8,0,1 12,8,0,1 0,9,0,1 5,9,0,1 7,9,0,1 12,9,0,1 0,10,0,1 5,10,0,1 7,10,0,1 \
    12,10,0,1 15,11,0,1 2,12,0,1 3,12,0,1 4,12,0,1 8,12,0,1 9,12,0,1 10,12,0,1 15,12,0,1 \
    15,13,0,1 11,15,0,1 12,15,0,1 13,15,0,1 17,15,0,1 18,15,0,1 19,15,0,1 15,17,0,1 15,18,0,1 \
    15,19,0,1";

#[test]
fn a_pulsar_with_blinkers_is_five_objects() {
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let mut w = pasted("square", 64, "B3/S23", PULSAR_BLINKERS);
    for g in 0..6 {
        let r = objects(&w);
        assert_eq!(
            (count(&r, "pulsar"), count(&r, "blinker"), r.objects.len()),
            (1, 4, 5),
            "generation {g}: {}",
            r.summary_line()
        );
        let mut want = vec!["blinker"; 4];
        want.push("pulsar");
        assert_eq!(census(&c, &w), want, "generation {g}");
        step(&mut w);
    }
    // Without splitting, in a phase where the blinkers lie within two steps of the pulsar, it
    // is the one 60-cell p6 oscillator the hunt stored.
    let mut w = pasted("square", 64, "B3/S23", PULSAR_BLINKERS);
    let mut one = false;
    for _ in 0..6 {
        let r = whole(&w);
        one |= r.objects.len() == 1 && r.objects[0].class == ObjClass::Oscillator { period: 6 };
        step(&mut w);
    }
    assert!(one);
}

const PULSAR: &str = "..ooo...ooo../............./o....o.o....o/o....o.o....o/o....o.o....o/\
    ..ooo...ooo../............./..ooo...ooo../o....o.o....o/o....o.o....o/o....o.o....o/\
    ............./..ooo...ooo..";

#[test]
fn a_pulsar_is_one_object() {
    // Its quadrants are four atoms that die alone and live only together.
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let mut w = life(&[(PULSAR, 20, 20)]);
    let mut phases = Vec::new();
    for g in 0..3 {
        let r = objects(&w);
        assert_eq!(
            (count(&r, "pulsar"), r.objects.len()),
            (1, 1),
            "generation {g}: {}",
            r.summary_line()
        );
        assert_eq!(census(&c, &w), ["pulsar"]);
        phases.push(lattice_cells(&w));
        step(&mut w);
    }
    assert!(phases.iter().any(|p| compound::atoms(&c, p).len() == 4));
    assert!(compound::split(&c, &phases, [0, 0]).is_none());
}

#[test]
fn ships_with_sparks_stay_one_object() {
    // The middle- and heavyweight ships shed sparks that die alone: they stay one ship at every
    // phase, and so does the pentadecathlon, whose phases fall apart into pieces.
    for (name, rows, p) in [
        ("MWSS", "...o../.o...o/o...../o....o/ooooo.", 4),
        ("HWSS", "...oo../.o....o/o....../o.....o/oooooo.", 4),
        ("pentadecathlon", "..o....o../oo.oooo.oo/..o....o..", 15),
    ] {
        let mut w = life(&[(rows, 40, 40)]);
        for g in 0..p {
            let r = objects(&w);
            assert_eq!(
                (count(&r, name), r.objects.len()),
                (1, 1),
                "{name} at generation {g}: {}",
                r.summary_line()
            );
            step(&mut w);
        }
    }
}

#[test]
fn a_pseudo_still_life_comes_apart() {
    // Two blocks a cell apart (the bi-block), and two blocks beside an aircraft carrier.
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let w = life(&[("oo.oo/oo.oo", 10, 10)]);
    let r = objects(&w);
    assert_eq!(
        (count(&r, "block"), r.objects.len()),
        (2, 2),
        "{}",
        r.summary_line()
    );
    assert_eq!(census(&c, &w), ["block", "block"]);
    assert_eq!(whole(&w).objects.len(), 1);
    let w = life(&[("oo.oo/oo.oo//......oo/.......o/.....o/.....oo", 10, 10)]);
    let r = objects(&w);
    assert_eq!(
        (count(&r, "block"), count(&r, "aircraft carrier")),
        (2, 1),
        "{}",
        r.summary_line()
    );
    assert_eq!(census(&c, &w), ["aircraft carrier", "block", "block"]);
    // An object whose parts only hold each other up stays one: the aircraft carrier is two
    // atoms, each of which becomes a block alone.
    let w = life(&[("oo../o..o/..oo", 10, 10)]);
    let r = objects(&w);
    assert_eq!(
        (count(&r, "aircraft carrier"), r.objects.len()),
        (1, 1),
        "{}",
        r.summary_line()
    );
    assert_eq!(compound::atoms(&c, &lattice_cells(&w)).len(), 2);
}

/// A bound pair: two of Guttesen's snub-square gliders (Life, 3.3.4.3.4) that interact and fly
/// together with period 16 (`2ba58828` of the curation; one phase, as its scene pastes it).
const SNUB_BOUND_PAIR: &str = "0,0,1,1 0,0,2,1 0,0,4,1 0,0,5,1 1,0,2,1 3,0,1,1 3,0,2,1 3,0,4,1 \
    3,0,5,1 4,0,2,1 0,1,0,1 0,1,1,1 0,1,3,1 0,1,4,1 1,1,1,1 1,1,2,1 3,1,0,1 3,1,3,1 1,2,0,1 \
    1,2,1,1 2,2,0,1 2,2,1,1 2,2,2,1 2,2,4,1 4,2,1,1 4,2,4,1 5,2,1,1 5,2,2,1 2,3,0,1 2,3,1,1 \
    2,3,4,1 5,3,0,1 5,3,3,1";

/// Another such pair, from a 20,000-soup Life hunt on 3.3.4.3.4: in some phases its halves are a
/// glider, a glider and a piece that dies alone, the second glider too far from the piece for the
/// piece to be merged with both.
const SNUB_BOUND_PAIR_2: &str = "0,0,1,1 0,0,2,1 0,0,4,1 0,0,5,1 1,0,2,1 0,1,0,1 0,1,1,1 0,1,3,1 \
    0,1,4,1 1,1,1,1 1,1,2,1 4,1,1,1 4,1,2,1 4,1,4,1 4,1,5,1 5,1,2,1 1,2,0,1 4,2,0,1 4,2,1,1 \
    4,2,3,1 5,2,1,1 3,3,1,1 3,3,4,1 4,3,0,1 4,3,1,1 4,3,2,1 4,3,3,1 1,6,1,1 1,6,2,1 1,6,4,1 \
    2,6,2,1 1,7,4,1 2,7,0,1";

#[test]
fn a_bound_pair_stays_one_ship() {
    let c = Census::new("B3/S23", &TilingSpec::new("snubsq"), None, 64).unwrap();
    for cells in [SNUB_BOUND_PAIR, SNUB_BOUND_PAIR_2] {
        let mut w = pasted("snubsq", 32, "B3/S23", cells);
        let mut phases = Vec::new();
        for g in 0..16 {
            let r = objects(&w);
            assert_eq!(r.objects.len(), 1, "generation {g}: {}", r.summary_line());
            assert!(
                matches!(r.objects[0].class, ObjClass::Spaceship { period: 16, .. }),
                "generation {g}: {}",
                r.summary_line()
            );
            let found = census(&c, &w);
            assert_eq!(found.len(), 1, "generation {g}: {found:?}");
            phases.push(lattice_cells(&w));
            step(&mut w);
        }
        // At some phase it is several atoms, and one of them is a glider alone; still they
        // interact.
        assert!(phases.iter().any(|p| compound::atoms(&c, p).len() >= 2));
        let d = {
            let (a, b) = (&phases[0], &lattice_cells(&w));
            [b[0].i - a[0].i, b[0].j - a[0].j]
        };
        assert!(compound::split(&c, &phases, d).is_none());
    }
}

/// A tag-along: a 4.8.8 ship (B27/S25, `5aec548b` of the curation) whose core, `69c9912a`,
/// runs inside it unperturbed while towing a passenger that cannot run alone.
const OCT_TAG_ALONG: &str = "0,0,0,1 0,0,1,1 1,0,0,1 0,1,1,1 0,2,0,1 0,3,1,1 1,3,1,1 0,4,0,1 \
    0,4,1,1 1,5,0,1";
const OCT_CORE: &str = "0,0,0,1 0,0,1,1 1,0,0,1 0,1,1,1 0,2,0,1";

#[test]
fn a_tag_along_stays_one_ship() {
    let c = Census::new("B27/S25", &TilingSpec::new("oct"), None, 64).unwrap();
    let mut w = pasted("oct", 32, "B27/S25", OCT_TAG_ALONG);
    let mut phases = Vec::new();
    for g in 0..4 {
        let r = objects(&w);
        assert_eq!(r.objects.len(), 1, "generation {g}: {}", r.summary_line());
        assert!(
            matches!(r.objects[0].class, ObjClass::Spaceship { period: 4, .. }),
            "{}",
            r.summary_line()
        );
        assert_eq!(census(&c, &w).len(), 1);
        phases.push(lattice_cells(&w));
        step(&mut w);
    }
    let d = {
        let (a, b) = (&phases[0], &lattice_cells(&w));
        [b[0].i - a[0].i, b[0].j - a[0].j]
    };
    assert!(compound::split(&c, &phases, d).is_none());
    // Its core is a ship alone.
    let r = objects(&pasted("oct", 32, "B27/S25", OCT_CORE));
    assert!(matches!(
        r.objects[0].class,
        ObjClass::Spaceship { period: 2, .. }
    ));
}

#[test]
fn two_oscillators_on_hexagons_come_apart() {
    // B345/S346 on the hexagon grid: a 4-cell and a 6-cell p2 oscillator four cells apart, which
    // the finder took for one 10-cell oscillator in this phase.
    let rows = |r: &str, di: i32, dj: i32| -> Vec<String> {
        r.split('/')
            .enumerate()
            .flat_map(|(j, l)| {
                l.chars()
                    .enumerate()
                    .filter(|(_, ch)| *ch == 'o')
                    .map(move |(i, _)| format!("{},{},0,1", i as i32 + di, j as i32 + dj))
            })
            .collect()
    };
    let mut cells = rows(".o/.oo/o", 0, 0);
    cells.extend(rows(".o.o/.oo/o.o", -4, 2));
    let cells = cells.join(" ");
    let c = Census::new("B345/S346", &TilingSpec::new("hex"), None, 64).unwrap();
    let mut w = pasted("hex", 40, "B345/S346", &cells);
    assert_eq!(whole(&w).objects.len(), 1);
    for g in 0..2 {
        let r = objects(&w);
        let sizes: Vec<usize> = {
            let mut v: Vec<usize> = r.objects.iter().map(|o| o.phases[0].len()).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(sizes.len(), 2, "generation {g}: {}", r.summary_line());
        assert!(
            r.objects
                .iter()
                .all(|o| matches!(o.class, ObjClass::Oscillator { period: 2 })),
            "{}",
            r.summary_line()
        );
        assert_eq!(census(&c, &w).len(), 2);
        step(&mut w);
    }
}

#[test]
fn compound_records_split_into_part_records() {
    // The glider pair as a record (from a phase where it is one group): its parts are two
    // copies of the glider's record.
    let (w, r) = GLIDER_PAIR
        .iter()
        .map(|cells| {
            let w = pasted("square", 64, "B3/S23", cells);
            let r = whole(&w);
            (w, r)
        })
        .find(|(_, r)| r.objects.len() == 1)
        .unwrap();
    let prov = jetcarp_core::pattern::Provenance::default();
    let rec = jetcarp_core::pattern::record_from_object(
        &w,
        w.cas[0].id,
        &r.objects[0],
        prov.clone(),
        jetcarp_core::pattern::DiscoveredBy::User,
    )
    .unwrap();
    let parts = split::split_compound(&rec).unwrap().expect("a compound");
    let recs = split::part_records(
        &rec,
        &parts,
        prov,
        jetcarp_core::pattern::DiscoveredBy::User,
    )
    .unwrap();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].id, recs[1].id);
    let glider = objects(&life(&[(".o./..o/ooo", 10, 10)]));
    assert_eq!(glider.objects[0].name, "glider");
    assert_eq!(recs[0].cells, 5);
    assert_eq!(recs[0].period, 4);
    // A runner on the record's own tiling agrees.
    let runner = split::record_runner(&rec).unwrap();
    let run = runner.run(&rec.phase(0).unwrap(), 4).unwrap();
    assert_eq!(run.len(), 5);
}
