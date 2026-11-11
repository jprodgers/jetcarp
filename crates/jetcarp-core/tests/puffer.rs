//! Puffer detection: the front recurs shifted by a lattice vector while the debris grows.

use jetcarp_core::objects::puffer::{Sparse, detect};
use jetcarp_core::objects::{LCell, rows_to_cells};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Boundary, Nb, TilingSpec, Topology};

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
