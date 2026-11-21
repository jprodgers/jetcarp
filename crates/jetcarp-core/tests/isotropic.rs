//! Isotropic non-totalistic rules in Hensel's notation (#70): the stepper against the B/S
//! steppers and against the rule's own letters, against the von Neumann rules, isotropy, edges,
//! threads, the tiling, objects, and the state hash. `jetcarp-api`'s `tests/golly.rs` checks the
//! letters themselves against Golly.

use jetcarp_core::isotropic::{IsoRule, OFFSETS, classes};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::rules::Rule;
use jetcarp_core::*;

fn world(rule: &str, w: u32, h: u32, b: Boundary, density: f64, seed: u64) -> World {
    let mut world = World::new(TilingSpec::rect("square", w, h, b), seed).unwrap();
    world
        .add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density }))
        .unwrap_or_else(|e| panic!("{rule}: {e}"));
    world
}

const BOUNDARIES: [Boundary; 3] = [Boundary::Dead, Boundary::Wrap, Boundary::Live];

/// The B/S masks and state count of a B/S or Generations rule.
fn masks(rule: &str) -> (u32, u32, u8) {
    match Rule::parse(rule).unwrap().0 {
        Rule::LifeLike { birth, survive } => (birth, survive, 2),
        Rule::Generations {
            birth,
            survive,
            states,
        } => (birth, survive, states),
        r => panic!("{r:?}"),
    }
}

/// The isotropic stepper, given a B/S rule's table (every letter of each count), steps exactly as
/// the B/S stepper: two states, Generations, and B0 with and without survival on 8, with every
/// edge.
#[test]
fn every_letter_steps_as_the_b_s_rule() {
    for rule in [
        "B3/S23",
        "B36/S23",
        "B3678/S34678",
        "B2/S",
        "B1357/S1357",
        "B2/S/C3",
        "B34/S12/C5",
        "B014/S2",
        "B0123478/S01234678",
    ] {
        for b in BOUNDARIES {
            let mut a = world(rule, 40, 30, b, 0.35, 3);
            let mut i = world(rule, 40, 30, b, 0.35, 3);
            let (birth, survive, states) = masks(rule);
            i.cas[0].rule = Rule::Isotropic(Box::new(IsoRule::from_masks(birth, survive, states)));
            i.cas[0].background = None;
            for g in 1..=60 {
                a.apply(Command::Step { gens: 1 }).unwrap();
                i.apply(Command::Step { gens: 1 }).unwrap();
                assert_eq!(
                    a.fields[0].state, i.fields[0].state,
                    "{rule} {b:?} at generation {g}"
                );
                assert_eq!(a.fields[0].age, i.fields[0].age, "{rule} {b:?} ages at {g}");
            }
        }
    }
}

/// Life written with every letter is Life: the same rule, named B3/S23, in its family.
#[test]
fn life_written_with_every_letter_is_life() {
    for text in [
        "B3aceijknqry/S2aceikn3aceijknqry",
        "B3-/S2-3",
        "b3cekainyqjr/s2ackein3jkqraceiny",
    ] {
        let (r, nb) = Rule::parse(text).unwrap();
        assert_eq!(
            r,
            Rule::LifeLike {
                birth: 1 << 3,
                survive: 1 << 2 | 1 << 3
            },
            "{text}"
        );
        assert_eq!(nb, None);
        assert_eq!(r.display_name(), "Life");
        assert_eq!(semantics::family(&r), "lifelike");
    }
}

/// The neighbourhood of square-grid cell `(x, y)` as the rule reads it, by its own geometry (not
/// the stepper's): bit k for a live neighbour at `OFFSETS[k]`; `edge` past the edge.
fn nbhd(st: &[u8], w: i32, h: i32, x: i32, y: i32, wrap: bool, edge: bool) -> u8 {
    let mut m = 0u8;
    for (k, &(dx, dy)) in OFFSETS.iter().enumerate() {
        let (mut u, mut v) = (x + dx, y + dy);
        let live = if wrap {
            u = u.rem_euclid(w);
            v = v.rem_euclid(h);
            st[(v * w + u) as usize] == 1
        } else if u < 0 || v < 0 || u >= w || v >= h {
            edge
        } else {
            st[(v * w + u) as usize] == 1
        };
        m |= (live as u8) << k;
    }
    m
}

/// Every generation, every cell's next state is what the rule's letters say for its
/// neighbourhood (`IsoRule::next`): two states and Generations, with every edge.
#[test]
fn every_cell_does_what_its_letters_say() {
    for rule in [
        "B3-cnqy/S23-a",
        "B2-a/S12",
        "B2a3-ckn/S1e2-in3/C4",
        "B1c2-k/S/C3",
    ] {
        let r = match Rule::parse(rule).unwrap().0 {
            Rule::Isotropic(r) => r,
            x => panic!("{x:?}"),
        };
        for b in BOUNDARIES {
            let (w, h) = (31, 23);
            let mut wd = world(rule, w, h, b, 0.4, 9);
            let g = wd.topo.lattice().unwrap().clone();
            assert_eq!((g.w, g.h, g.ntypes), (w, h, 1));
            for step in 0..40 {
                let st = wd.fields[0].state.clone();
                let want: Vec<u8> = (0..(w * h) as i32)
                    .map(|i| {
                        let (x, y) = (i % w as i32, i / w as i32);
                        let n = nbhd(
                            &st,
                            w as i32,
                            h as i32,
                            x,
                            y,
                            b == Boundary::Wrap,
                            b == Boundary::Live,
                        );
                        r.next(st[i as usize], n)
                    })
                    .collect();
                wd.apply(Command::Step { gens: 1 }).unwrap();
                assert_eq!(
                    wd.fields[0].state, want,
                    "{rule} {b:?} at generation {step}"
                );
            }
        }
    }
}

/// A von Neumann rule is an isotropic rule that reads only the edge neighbours (rotations and
/// reflections keep edges edges): written letter by letter, it steps as the rule with a V.
#[test]
fn von_neumann_rules_as_isotropic_rules() {
    const EDGES: u8 = 0b0101_1010; // N, W, E, S
    for vn in ["B2/S013V", "B13/S024V", "B1/S0V", "B3/S23V", "B24/S1V"] {
        let (birth, survive, _) = masks(vn.trim_end_matches('V'));
        let mut r = IsoRule {
            birth: [0; 9],
            survive: [0; 9],
            states: 2,
        };
        for x in 0..=255u8 {
            let c = classes()[x as usize];
            let (n, l) = ((c >> 4) as usize, c & 15);
            let e = (x & EDGES).count_ones();
            if birth >> e & 1 == 1 {
                r.birth[n] |= 1 << l;
            }
            if survive >> e & 1 == 1 {
                r.survive[n] |= 1 << l;
            }
        }
        let text = r.notation();
        assert!(
            matches!(Rule::parse(&text).unwrap().0, Rule::Isotropic(_)),
            "{text}"
        );
        for b in BOUNDARIES {
            let mut a = world(vn, 36, 28, b, 0.3, 5);
            let mut i = world(&text, 36, 28, b, 0.3, 5);
            for g in 1..=40 {
                a.apply(Command::Step { gens: 1 }).unwrap();
                i.apply(Command::Step { gens: 1 }).unwrap();
                assert_eq!(
                    a.fields[0].state, i.fields[0].state,
                    "{vn} as {text}, {b:?}, generation {g}"
                );
            }
        }
    }
}

/// The rules are isotropic: a start turned or reflected by any of the square's eight symmetries
/// reaches the same state turned or reflected the same way.
#[test]
fn turned_and_reflected_starts_stay_so() {
    let n = 36i32;
    let sym = |s: u8, x: i32, y: i32| -> (i32, i32) {
        let (mut x, mut y) = if s >= 4 { (n - 1 - x, y) } else { (x, y) };
        for _ in 0..s % 4 {
            (x, y) = (n - 1 - y, x);
        }
        (x, y)
    };
    for rule in [
        "B3-cnqy/S23-a",
        "B2ci3ai4c8/S02ae3eijkq4iz5ar6i7e",
        "B2a3-ckn/S1e2-in3/C4",
    ] {
        let base = world(rule, n as u32, n as u32, Boundary::Wrap, 0.4, 13);
        let start = base.fields[0].state.clone();
        let mut a = base.clone();
        a.apply(Command::Step { gens: 30 }).unwrap();
        for s in 1..8u8 {
            let mut t = base.clone();
            let mut st = vec![0u8; (n * n) as usize];
            for y in 0..n {
                for x in 0..n {
                    let (u, v) = sym(s, x, y);
                    st[(v * n + u) as usize] = start[(y * n + x) as usize];
                }
            }
            t.fields[0].state = st;
            t.apply(Command::Step { gens: 30 }).unwrap();
            for y in 0..n {
                for x in 0..n {
                    let (u, v) = sym(s, x, y);
                    assert_eq!(
                        t.fields[0].state[(v * n + u) as usize],
                        a.fields[0].state[(y * n + x) as usize],
                        "{rule}, symmetry {s}, cell ({x}, {y})"
                    );
                }
            }
        }
    }
}

/// The square grid's lattice offsets are its geometry: neighbour `(di, dj)` is the cell whose
/// centre is one step across and down, so the letters read the arrangement they name.
#[test]
fn the_square_lattice_is_its_geometry() {
    let t = Topology::build(&TilingSpec::rect("square", 8, 8, Boundary::Dead)).unwrap();
    let g = t.lattice().unwrap();
    let (a1, a2) = (g.a1, g.a2);
    // Orthonormal: a turn or reflection of the unit square.
    let dot = a1[0] * a2[0] + a1[1] * a2[1];
    let len = |v: [f64; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
    assert!(
        dot.abs() < 1e-9 && (len(a1) - len(a2)).abs() < 1e-9,
        "{a1:?} {a2:?}"
    );
    let offs: Vec<(i32, i32)> = g.offsets[Nb::Touch as usize][0]
        .iter()
        .map(|o| (o.di, o.dj))
        .collect();
    assert!(jetcarp_core::isotropic::slots(&offs).is_some(), "{offs:?}");
}

#[test]
fn the_rule_needs_the_square_grid_s_touching_neighbours() {
    let mut w = World::new(TilingSpec::rect("hex", 10, 10, Boundary::Wrap), 1).unwrap();
    let e = w.add_ca(CaSpec::new("B2-a/S12")).unwrap_err();
    assert!(e.contains("square grid"), "{e}");
    for text in ["B2-a/S12V", "B2a/S12/C3V", "B2-a/S12H"] {
        assert!(Rule::parse(text).is_err(), "{text}");
    }
    // Edge neighbours asked for are not taken: the letters name touching ones.
    let mut w = World::new(TilingSpec::rect("square", 10, 10, Boundary::Wrap), 1).unwrap();
    w.add_ca(CaSpec::new("B2-a/S12").nb(Nb::Edge)).unwrap();
    assert_eq!(w.cas[0].nb, Nb::Touch);
    // A world whose tiling changes away from the square grid is refused, as for Margolus.
    assert!(
        w.apply(Command::SetTiling {
            tiling: TilingSpec::rect("hex", 10, 10, Boundary::Wrap)
        })
        .is_err()
    );
}

#[test]
fn the_result_does_not_depend_on_the_thread_count() {
    let run = |threads: usize, rule: &str| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut w = world(rule, 160, 120, Boundary::Wrap, 0.35, 21);
                w.apply(Command::Step { gens: 50 }).unwrap();
                w.hash_hex()
            })
    };
    for rule in ["B3-cnqy/S23-a", "B2a3-ckn/S1e2-in3/C4", "B01e2a/S2-i3"] {
        assert_eq!(run(1, rule), run(7, rule), "{rule}");
    }
}

/// The state hash holds the rule: one letter apart, two worlds of the same cells differ; the same
/// rule written two ways hashes alike.
#[test]
fn the_state_hash_covers_the_rule() {
    let h = |rule: &str| world(rule, 20, 20, Boundary::Wrap, 0.3, 1).hash_hex();
    assert_ne!(h("B3-cnqy/S23-a"), h("B3-cnqy/S23-ae"));
    assert_ne!(h("B3-cnqy/S23-a"), h("B3-cnqy/S23"));
    assert_ne!(h("B2a/S12/C3"), h("B2a/S12/C4"));
    assert_eq!(h("B2-a/S12"), h("B2cekin/S21"));
}

/// B0 rules keep their cells relative to the background, so an empty world stays empty, with
/// and without survival on 8.
#[test]
fn b0_rules_keep_empty_space_empty() {
    for rule in ["B01e2a/S2-i3", "B01e3-cnqy/S23-a8"] {
        for b in [Boundary::Dead, Boundary::Wrap] {
            let mut w = World::new(TilingSpec::rect("square", 20, 20, b), 1).unwrap();
            w.add_ca(CaSpec::new(rule).seed(SeedSpec::Empty)).unwrap();
            w.apply(Command::Step { gens: 9 }).unwrap();
            assert!(w.fields[0].state.iter().all(|&s| s == 0), "{rule} {b:?}");
            assert_eq!(w.cas[0].ticks, 9);
        }
    }
}

/// The object finder isolates objects of isotropic rules and counts a turned or reflected copy
/// as the same object: Life's boat, a still life in tlife (B3/S2-i34q; its cells have two
/// neighbours in arrangements 2e and 2c, or three), in all eight orientations.
#[test]
fn objects_of_isotropic_rules() {
    let start = "oo./o.o/.o.";
    let mut pats = Vec::new();
    let rows: Vec<Vec<char>> = start.split('/').map(|r| r.chars().collect()).collect();
    let n = 3usize;
    for s in 0..8u8 {
        let mut grid = vec![vec!['.'; n]; n];
        for (y, row) in rows.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                let (mut u, mut v) = if s >= 4 { (n - 1 - x, y) } else { (x, y) };
                for _ in 0..s % 4 {
                    (u, v) = (n - 1 - v, u);
                }
                grid[v][u] = c;
            }
        }
        let text: Vec<String> = grid.iter().map(|r| r.iter().collect()).collect();
        pats.push((
            text.join("/"),
            4 + 10 * (s as i32 % 4),
            4 + 12 * (s as i32 / 4),
        ));
    }
    let mut w = World::new(TilingSpec::rect("square", 48, 32, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = Vec::new();
    for (rows, u, j) in &pats {
        cells.extend(place_rows(&g, rows, *u, *j));
    }
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S2-i34q").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let line = r.summary_line();
    assert_eq!(r.counts.len(), 1, "{line}");
    assert_eq!(r.counts[0].count, 8, "{line}");
    assert_eq!(r.counts[0].class, ObjClass::StillLife, "{line}");
}
