//! Following a find back through its soup (`hunt::soup::sightings`, which `library soup` reads):
//! every copy a soup's census counts is found, from a generation at or before the one the run took
//! it at, where its cells, moved back along its path, are there and alone.

use jetcarp_core::hunt::bits::Universe;
use jetcarp_core::hunt::separate::{Cache, Census};
use jetcarp_core::hunt::soup::{Layout, Limits, SoupSpec, run_soup, sightings, soup_state};
use jetcarp_core::objects::LCell;
use std::collections::BTreeMap;

fn limits() -> Limits {
    Limits {
        max_gens: 20_000,
        settle_period: 64,
        pop_cap: 6000,
        max_span: 4096,
        cut_every: 16,
    }
}

#[test]
fn every_copy_of_a_find_is_followed_back_to_when_it_first_shows() {
    let t = jetcarp_core::pattern::tiling_from_key("square").unwrap();
    let c = Census::new("B3/S23", &t, None, 64).unwrap();
    let layout = Layout::new(&c.topo, SoupSpec::default(), 32).unwrap();
    let mut cache = Cache::default();
    let (mut ships, mut still) = (0, 0);
    for i in 0..40 {
        let rng = soup_state(9, i);
        let out = run_soup(&c, &mut cache, &layout, limits(), i, rng);
        let mut copies: BTreeMap<String, usize> = BTreeMap::new();
        for f in &out.objects {
            *copies.entry(f.id.clone()).or_default() += 1;
        }
        for (id, n) in copies {
            let seen = sightings(&c, &mut cache, &layout, limits(), rng, &id);
            assert_eq!(seen.len(), n, "soup {i}, {id}");
            for s in &seen {
                assert!(s.at <= out.gens, "soup {i}, {id}: {s:?}");
                let Some(first) = s.first else { continue };
                assert!(first <= s.at && (s.at - first) % s.period == 0, "{s:?}");
                if s.cut {
                    ships += 1;
                    assert_ne!(s.d, [0, 0]);
                } else if s.d == [0, 0] {
                    still += 1;
                }
            }
        }
    }
    assert!(ships > 0 && still > 0, "{ships} ships, {still} still");
}

/// `cells_within` gives a universe's cells inside a box, as `cells` filtered to it does.
#[test]
fn a_universe_gives_its_cells_within_a_box() {
    let t = jetcarp_core::pattern::tiling_from_key("hex").unwrap();
    let c = Census::new("B2/S34", &t, None, 64).unwrap();
    let layout = Layout::new(&c.topo, SoupSpec::default(), 32).unwrap();
    let mut u = Universe::with_cells(c.plan.clone(), &layout.soup(soup_state(1, 2)));
    u.run(40);
    let all = u.cells();
    assert!(!all.is_empty());
    for b in [
        [-200, 200, -200, 200],
        [3, 9, 2, 6],
        [-70, 70, 5, 5],
        [64, 130, -3, 40],
        [1000, 1010, 0, 3],
    ] {
        let want: Vec<LCell> = all
            .iter()
            .copied()
            .filter(|q| q.i >= b[0] && q.i <= b[1] && q.j >= b[2] && q.j <= b[3])
            .collect();
        assert_eq!(u.cells_within(b), want, "{b:?}");
    }
}
