//! The exploratory search's measures: fates, mover tracking, candidates and batch traces.

use jetcarp_core::explore::batch::{BatchJob, BatchWorld, neighbor_csr, run_cpu};
use jetcarp_core::explore::features::{Fate, fate_of, frames_for, measure_run, track_movers};
use jetcarp_core::explore::space::{ALL_FAMS, Fam, Grid, all_tilings, mutate_rule, sample_rule};
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_core::{CaSpec, SeedSpec, World};
use std::sync::Arc;

fn life(grid: &Grid, cells: &[(usize, usize)]) -> Vec<u8> {
    let l = grid.topo.lattice().unwrap();
    let w = l.w as usize;
    let mut v = vec![0u8; grid.topo.len()];
    for &(x, y) in cells {
        v[y * w + x] = 1;
    }
    v
}

fn run(
    grid: &Grid,
    init: Vec<u8>,
    gens: u32,
    frames: Vec<u32>,
) -> jetcarp_core::explore::batch::WorldTrace {
    let job = BatchJob {
        topo: grid.topo.clone(),
        nb: Nb::Touch,
        worlds: vec![BatchWorld {
            rule: Rule::LifeLike {
                birth: 1 << 3,
                survive: (1 << 2) | (1 << 3),
            },
            init,
        }],
        gens,
        frames_at: frames,
    };
    run_cpu(&job).unwrap().into_iter().next().unwrap()
}

#[test]
fn fates() {
    let g = Grid::build("square", 400).unwrap();
    let block = life(&g, &[(5, 5), (6, 5), (5, 6), (6, 6)]);
    let t = run(&g, block, 20, vec![]);
    assert_eq!(fate_of(&t).0, Fate::Froze);
    let blinker = life(&g, &[(5, 5), (6, 5), (7, 5)]);
    let (f, _, p) = fate_of(&run(&g, blinker, 20, vec![]));
    assert_eq!((f, p), (Fate::Cycle, 2));
    let lone = life(&g, &[(5, 5)]);
    assert_eq!(fate_of(&run(&g, lone, 8, vec![])).0, Fate::Died);
}

#[test]
fn trackers_see_gliders_not_oscillators() {
    let g = Grid::build("square", 4096).unwrap();
    let glider = life(&g, &[(10, 9), (11, 10), (9, 11), (10, 11), (11, 11)]);
    let frames = frames_for(40, 17);
    let t = run(&g, glider, 40, frames.clone());
    let refs: Vec<&[u8]> = t.frames.iter().map(|f| f.as_slice()).collect();
    let (count, speed) = track_movers(&g, &refs);
    assert_eq!(count, 1, "glider seen");
    assert!((speed - 0.35).abs() < 0.08, "c/4 diagonal, got {speed}");
    let (fate, feat) = measure_run(&g, &t, 40, 17);
    assert_ne!(fate, Fate::Died);
    assert_eq!(feat.movers, 1);
    let blinker = life(&g, &[(5, 5), (6, 5), (7, 5)]);
    let t = run(&g, blinker, 40, frames);
    let refs: Vec<&[u8]> = t.frames.iter().map(|f| f.as_slice()).collect();
    assert_eq!(track_movers(&g, &refs).0, 0, "an oscillator is not a mover");
}

#[test]
fn glider_tracks_across_the_torus_seam() {
    let g = Grid::build("square", 900).unwrap();
    let l = g.topo.lattice().unwrap();
    let (w, h) = (l.w as usize, l.h as usize);
    // Start next to the corner: the path crosses both seams.
    let (x0, y0) = (w - 3, h - 3);
    let cells: Vec<(usize, usize)> = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)]
        .iter()
        .map(|(x, y)| ((x0 + x) % w, (y0 + y) % h))
        .collect();
    let t = run(&g, life(&g, &cells), 24, frames_for(24, 17));
    let refs: Vec<&[u8]> = t.frames.iter().map(|f| f.as_slice()).collect();
    assert_eq!(track_movers(&g, &refs).0, 1);
}

#[test]
fn grid_specific_families_fit_their_native_tilings_at_search_size() {
    let mut rng = Rng::new(440);
    for (fam, key) in [
        (Fam::Wolfram, "square"),
        (Fam::Margolus, "square"),
        (Fam::Table, "square"),
        (Fam::HexWolfram, "hex"),
        (Fam::HexTri, "hex_flat"),
    ] {
        let g = Grid::build(key, 16000).unwrap();
        assert!(fam.fits(&g), "{fam:?} does not fit {key}");
        assert!(
            (0..8).any(|_| sample_rule(fam, &g, Nb::Touch, &mut rng).is_some()),
            "{fam:?} sampled no rule on {key}"
        );
    }
}

#[test]
fn every_family_samples_a_valid_rule_where_it_fits() {
    let grids: Vec<Grid> = ["square", "hex", "hex_flat", "trihex", "spectre"]
        .iter()
        .map(|k| Grid::build(k, 1000).unwrap())
        .collect();
    let mut rng = Rng::new(439);
    for fam in ALL_FAMS {
        for g in &grids {
            if !fam.fits(g) {
                continue;
            }
            for _ in 0..8 {
                let Some(r) = sample_rule(fam, g, Nb::Touch, &mut rng) else {
                    continue;
                };
                let text = r.notation();
                let (back, _) =
                    Rule::parse(&text).unwrap_or_else(|e| panic!("{fam:?}: {text}: {e}"));
                assert_eq!(Fam::of(&back), fam, "{text}");
                let mut w = World::on_topology(g.topo.clone(), 1);
                let (seed, _) = jetcarp_core::explore::seeds::seed_for(fam, g, &mut rng);
                w.add_ca(CaSpec::new(&text).seed(seed))
                    .unwrap_or_else(|e| panic!("{} on {}: {e}", text, g.key));
                if let Some(m) = mutate_rule(&r, g, Nb::Touch, &mut rng) {
                    Rule::parse(&m.notation())
                        .unwrap_or_else(|e| panic!("{fam:?}: {}: {e}", m.notation()));
                }
            }
        }
    }
}

#[test]
fn tilings_list_finds_twins() {
    let all = all_tilings();
    assert!(all.len() > 100);
    let by = |k: &str| all.iter().find(|t| t.key == k).unwrap();
    assert!(by("square").twin_of.is_none());
    // Glide rectangles are squares with the same neighbours.
    assert!(by("glide").twin_of.is_some());
}

#[test]
fn neighbour_lists_agree_with_the_topology() {
    let g = Grid::build("trihex", 3000).unwrap();
    for nb in [Nb::Touch, Nb::Edge] {
        let c = neighbor_csr(&g.topo, nb, None).unwrap();
        for i in (0..g.topo.len()).step_by(37) {
            let mut v = Vec::new();
            g.topo.for_each_neighbor(i, nb, |x| v.push(x as u32));
            assert_eq!(
                &c.nb[c.off[i] as usize..c.off[i + 1] as usize],
                v.as_slice()
            );
        }
    }
    let t: Arc<_> = g.topo.clone();
    let mut w = World::on_topology(t, 1);
    w.add_ca(CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }))
        .unwrap();
    assert!(w.fields[0].population() > 0);
}

#[test]
fn a_saved_scene_replays_the_run_the_search_measured() {
    use jetcarp_core::explore::batch::trace_world;
    use jetcarp_core::explore::cand::Cand;
    use jetcarp_core::explore::space::sized_spec;
    use jetcarp_core::seeding::{SeedFill, SeedShape, SeedStates};
    let cases = [
        ("square", "B3/S23", 400u32),
        ("hex", "B2/S34", 400),
        ("trihex", "B2/S/C5", 400),
    ];
    for (tiling, rule, gens) in cases {
        let (r, _) = Rule::parse(rule).unwrap();
        let seed = SeedSpec::Pattern {
            shape: SeedShape::Blobs {
                count: 3,
                min_radius: 0.05,
                max_radius: 0.15,
                wobble: 0.3,
                min_gap: 0.02,
            },
            fill: SeedFill {
                density: 0.4,
                states: SeedStates::One,
            },
        };
        let c = Cand {
            tiling: tiling.into(),
            nb: Nb::Touch,
            fam: Fam::of(&r),
            rule: r.notation(),
            seed,
            cells: 4000,
            gens,
            steps: 1,
            rng: 77,
            origin: "test".into(),
        };
        let grid = Grid::build(tiling, 4000).unwrap();
        let mut w = World::on_topology(grid.topo.clone(), c.rng);
        w.add_ca(c.ca()).unwrap();
        let t = trace_world(&mut w, gens, &[]).unwrap();
        let (spec, _) = sized_spec(tiling, 4000).unwrap();
        let mut replay = World::from_scene(&c.scene(&spec)).unwrap();
        let t2 = trace_world(&mut replay, gens, &[]).unwrap();
        assert_eq!(t.pop, t2.pop, "{tiling}");
        assert_eq!(t.hash, t2.hash, "{tiling}");
    }
}

#[test]
fn circuit_seeds_lay_thin_wires_with_signals_on_any_tiling() {
    use jetcarp_core::world::{CaSpec, SeedSpec, World};
    for key in ["square", "hex", "trihex", "penrose-rhomb"] {
        let (_, topo) = jetcarp_core::explore::space::sized_spec(key, 4000).unwrap();
        let n = topo.len();
        let mut w = World::on_topology(topo, 7);
        w.add_ca(CaSpec::new("WireWorld").seed(SeedSpec::Circuit {
            sites: 12,
            electrons: 30,
        }))
        .unwrap();
        let count = |s: u8| w.fields[0].state.iter().filter(|x| **x == s).count();
        let wire = count(3) + count(1) + count(2);
        assert!(
            wire > 0 && wire < n / 2,
            "{key}: {wire} of {n} cells are wire"
        );
        assert!(count(1) > 0, "{key}: no signals");
    }
}

#[test]
fn row_rules_stop_at_the_bottom_on_wrapped_grids() {
    use jetcarp_core::world::{CaSpec, World};
    let (_, topo) = jetcarp_core::explore::space::sized_spec("square", 1600).unwrap();
    let mut w = World::on_topology(topo, 1);
    w.add_ca(CaSpec::new("W30")).unwrap();
    while !w.done() {
        w.step();
    }
    let finished = w.fields[0].state.clone();
    for _ in 0..20 {
        w.step();
    }
    assert_eq!(w.fields[0].state, finished);
}
