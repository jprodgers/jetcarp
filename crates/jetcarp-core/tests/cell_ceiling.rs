//! The largest grid the code can number ([`jetcarp_core::topology::MAX_CELLS`]): every cell's
//! index fits in the 32-bit integers that commands, seeds, ants and layers number cells with.
//! A lattice's topology holds nothing per cell, so a grid of exactly the ceiling is built here
//! and its last cells looked at, without making any field.

use jetcarp_core::topology::MAX_CELLS;
use jetcarp_core::{Boundary, Nb, SizeSpec, TilingSpec, Topology};

fn grid(family: &str, w: u32, h: u32, wrap: bool) -> TilingSpec {
    let mut t = TilingSpec::new(family);
    t.size = Some(SizeSpec::Rect { w, h });
    t.boundary = if wrap { Boundary::Wrap } else { Boundary::Dead };
    t
}

#[test]
fn the_ceiling_is_what_32_bits_can_number() {
    assert_eq!(MAX_CELLS, u32::MAX as u64);
}

/// 65,535 x 65,537 is exactly 2^32 - 1 cells: built, and every index of its last cells and of
/// their neighbours fits in 32 bits.
#[test]
fn a_grid_of_exactly_the_ceiling_builds() {
    for wrap in [false, true] {
        let t = Topology::build(&grid("square", 65_535, 65_537, wrap)).unwrap();
        assert_eq!(t.len() as u64, MAX_CELLS);
        let g = t.lattice().unwrap();
        let last = t.len() - 1;
        assert_eq!(g.decompose(last), (65_534, 65_536, 0));
        assert_eq!(g.index(65_534, 65_536, 0), last);
        let mut n = vec![];
        t.for_each_neighbor(last, Nb::Touch, |j| n.push(j));
        assert_eq!(n.len(), if wrap { 8 } else { 3 }, "wrap {wrap}");
        assert!(n.iter().all(|&j| j < t.len() && u32::try_from(j).is_ok()));
        if wrap {
            // The last cell's neighbours wrap round to the first row and column.
            assert!(n.contains(&0));
        }
        assert!(u32::try_from(t.center_cell()).is_ok());
    }
    assert_eq!(
        Topology::cell_count(&grid("square", 65_535, 65_537, false)).unwrap(),
        Some(MAX_CELLS)
    );
}

/// One more cell is refused, saying how many and why.
#[test]
fn one_more_is_refused() {
    let e = Topology::build(&grid("square", 65_536, 65_536, false)).unwrap_err();
    assert_eq!(
        e,
        "a 65536 x 65536 grid is 4294967296 cells, more than the 4294967295 JETCARP can number"
    );
    // Several tiles in each unit cell count: trihex has three.
    let e = Topology::build(&grid("trihex", 37_838, 37_838, false)).unwrap_err();
    assert!(e.contains("is 4295142732 cells"), "{e}");
    let t = Topology::build(&grid("trihex", 37_837, 37_837, true)).unwrap();
    assert_eq!(t.len() as u64, 37_837u64 * 37_837 * 3);
    let last = t.len() - 1;
    let mut n = vec![];
    t.for_each_neighbor(last, Nb::Touch, |j| n.push(j));
    assert!(!n.is_empty() && n.iter().all(|&j| u32::try_from(j).is_ok()));
}
