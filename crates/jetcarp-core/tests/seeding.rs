//! Pattern seeds: determinism, every shape on square, hexagon, trihexagonal, and Spectre tilings,
//! densities, symmetry, blob gaps, field kinds, and JSON.

use jetcarp_core::geom::P;
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::FieldKind;
use jetcarp_core::seeding::{self, NO_REGION, Placement};
use jetcarp_core::world::cell_size;
use jetcarp_core::*;
use std::collections::{HashMap, HashSet};

fn tilings() -> Vec<TilingSpec> {
    vec![
        TilingSpec::rect("square", 80, 60, Boundary::Dead),
        TilingSpec::rect("square", 80, 60, Boundary::Wrap),
        TilingSpec::rect("hex", 70, 60, Boundary::Dead),
        TilingSpec::rect("hex", 70, 60, Boundary::Wrap),
        TilingSpec::rect("trihex", 30, 30, Boundary::Dead),
        TilingSpec::rect("trihex", 30, 30, Boundary::Wrap),
        TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
    ]
}

fn shapes() -> Vec<SeedShape> {
    vec![
        SeedShape::All,
        SeedShape::Disc { radius: 0.3 },
        SeedShape::Blobs {
            count: 3,
            min_radius: 0.08,
            max_radius: 0.14,
            wobble: 1.0,
            min_gap: 0.04,
        },
        SeedShape::Clusters {
            count: 20,
            radius_tiles: 2.0,
        },
        SeedShape::Dots {
            spacing_tiles: 8.0,
            jitter: 0.5,
        },
        SeedShape::Ring {
            radius: 0.3,
            width: 0.1,
        },
        SeedShape::Rings {
            count: 3,
            width: 0.06,
        },
        SeedShape::Stripes {
            period: 0.2,
            duty: 0.5,
            angle: 30.0,
        },
        SeedShape::Checker { size: 0.2 },
        SeedShape::Gradient {
            angle: 90.0,
            low: 0.0,
            high: 1.0,
        },
        SeedShape::RadialGradient {
            centre: 1.0,
            edge: 0.0,
        },
        SeedShape::Noise {
            scale: 0.2,
            threshold: 0.5,
            octaves: 3,
        },
        SeedShape::Voronoi {
            sites: 12,
            keep: 0.5,
        },
        SeedShape::Symmetric {
            order: 6,
            mirror: true,
            radius: 0.35,
        },
    ]
}

fn pattern(shape: SeedShape, density: f64) -> SeedSpec {
    SeedSpec::Pattern {
        shape,
        fill: SeedFill::density(density),
    }
}

fn world(spec: TilingSpec, rule: &str, seed: SeedSpec, rng: u64) -> World {
    let mut s = Scene::new(spec);
    s.rng_state = rng;
    s.cas = vec![CaSpec::new(rule).seed(seed)];
    World::from_scene(&s).unwrap()
}

fn states(w: &World) -> Vec<u8> {
    w.fields[w.cas[0].field].state.clone()
}

// The same RNG state gives the same field, at any thread count; another RNG state differs.
#[test]
fn deterministic_and_independent_of_threads() {
    let spec = TilingSpec::rect("hex", 120, 100, Boundary::Wrap);
    for shape in shapes() {
        let seed = pattern(shape.clone(), 0.6);
        let run = |threads: usize, rng: u64| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            pool.install(|| states(&world(spec.clone(), "B3/S23", seed.clone(), rng)))
        };
        let a = run(1, 7);
        assert_eq!(a, run(1, 7), "{shape:?}");
        assert_eq!(a, run(8, 7), "{shape:?} depends on the thread count");
        assert_ne!(a, run(1, 8), "{shape:?} ignores the RNG state");
    }
}

// A seed takes a fixed number of draws from the world RNG, so later seeds line up.
#[test]
fn draws_a_fixed_number_of_rng_values() {
    for shape in shapes() {
        for spec in tilings() {
            let t = Topology::build(&spec).unwrap();
            let mut r = Rng::new(9);
            seeding::place(&t, &shape, &mut r).unwrap();
            let mut expected = Rng::new(9);
            expected.next_u64();
            assert_eq!(r, expected, "{shape:?}");
        }
    }
}

// Every shape sets some cells and leaves cells alone where it should, on every tiling.
#[test]
fn every_shape_on_every_tiling() {
    for spec in tilings() {
        let n = Topology::build(&spec).unwrap().len();
        for shape in shapes() {
            let s = states(&world(
                spec.clone(),
                "B3/S23",
                pattern(shape.clone(), 1.0),
                3,
            ));
            assert_eq!(s.len(), n);
            let on = s.iter().filter(|&&x| x != 0).count();
            assert!(on > 0, "{shape:?} on {spec:?} set nothing");
            match shape {
                SeedShape::All => assert_eq!(on, n, "{spec:?}"),
                SeedShape::Gradient { .. } | SeedShape::RadialGradient { .. } => {}
                _ => assert!(on < n, "{shape:?} on {spec:?} filled everything"),
            }
        }
    }
}

fn population(spec: &TilingSpec, seed: SeedSpec, rng: u64) -> f64 {
    let s = states(&world(spec.clone(), "B3/S23", seed, rng));
    s.iter().filter(|&&x| x != 0).count() as f64
}

/// Within a share of the expected count, or four standard deviations if that is more.
fn near(got: f64, want: f64, tol: f64, what: &str) {
    assert!(
        (got - want).abs() <= (tol * want).max(4.0 * want.max(1.0).sqrt()),
        "{what}: {got} vs expected {want}"
    );
}

// Populated counts follow the density, and the gradients follow their ramps.
#[test]
fn populations_match_density() {
    let square = TilingSpec::rect("square", 300, 200, Boundary::Wrap);
    let hex = TilingSpec::rect("hex", 260, 200, Boundary::Wrap);
    let trihex = TilingSpec::rect("trihex", 110, 110, Boundary::Wrap);
    for spec in [square.clone(), hex, trihex] {
        let t = Topology::build(&spec).unwrap();
        let n = t.len() as f64;
        for d in [0.05, 0.3, 0.7, 1.0] {
            let got = population(&spec, pattern(SeedShape::All, d), 11);
            near(got, n * d, 0.05, "all");
            // The chosen cells of each shape, filled at this density.
            for shape in [
                SeedShape::Disc { radius: 0.3 },
                SeedShape::Checker { size: 0.1 },
                SeedShape::Stripes {
                    period: 0.1,
                    duty: 0.25,
                    angle: 45.0,
                },
            ] {
                let solid = population(&spec, pattern(shape.clone(), 1.0), 11);
                let got = population(&spec, pattern(shape.clone(), d), 11);
                near(got, solid * d, 0.08, &format!("{shape:?} at {d}"));
            }
        }
    }

    let t = Topology::build(&square).unwrap();
    let n = t.len() as f64;
    // The mean of a linear ramp from 0.2 to 0.8 is 0.5.
    let g = SeedShape::Gradient {
        angle: 0.0,
        low: 0.2,
        high: 0.8,
    };
    near(
        population(&square, pattern(g, 1.0), 5),
        n * 0.5,
        0.05,
        "gradient",
    );
    let g = SeedShape::Gradient {
        angle: 0.0,
        low: 0.0,
        high: 1.0,
    };
    let w = world(square.clone(), "B3/S23", pattern(g, 1.0), 5);
    let s = states(&w);
    let side = |lo: f64, hi: f64| {
        let (mut on, mut all) = (0.0, 0.0);
        for (c, &x) in s.iter().enumerate() {
            let cx = t.centroid(c)[0];
            let (b0, b2) = (t.bounds[0], t.bounds[2]);
            let f = (cx - b0) / (b2 - b0);
            if f >= lo && f < hi {
                all += 1.0;
                on += (x != 0) as u32 as f64;
            }
        }
        on / all
    };
    assert!(side(0.0, 0.25) < 0.2, "the low end is too full");
    assert!(side(0.75, 1.0) > 0.8, "the high end is too empty");

    let r = SeedShape::RadialGradient {
        centre: 1.0,
        edge: 0.0,
    };
    let s = states(&world(square.clone(), "B3/S23", pattern(r, 1.0), 5));
    let c = [
        (t.bounds[0] + t.bounds[2]) / 2.0,
        (t.bounds[1] + t.bounds[3]) / 2.0,
    ];
    let reach = (t.bounds[2] - t.bounds[0]).min(t.bounds[3] - t.bounds[1]) / 2.0;
    let ring = |lo: f64, hi: f64| {
        let (mut on, mut all) = (0.0, 0.0);
        for (i, &x) in s.iter().enumerate() {
            let f = jetcarp_core::detmath::dist(t.centroid(i), c) / reach;
            if f >= lo && f < hi {
                all += 1.0;
                on += (x != 0) as u32 as f64;
            }
        }
        on / all
    };
    assert!(ring(0.0, 0.2) > 0.75, "the centre should be dense");
    assert!(ring(0.8, 1.0) < 0.25, "the edge should be sparse");
}

/// Rotation and reflection of a point about a centre by a group of `order` (see the seed).
fn orbit(q: P, p: P, order: u32, mirror: Option<f64>) -> Vec<P> {
    let mut out = Vec::new();
    let (x, y) = (p[0] - q[0], p[1] - q[1]);
    let mut points = vec![(x, y)];
    if let Some(axis) = mirror {
        let (s2, c2) = jetcarp_core::detmath::sin_cos(2.0 * axis);
        points.push((c2 * x + s2 * y, s2 * x - c2 * y));
    }
    for k in 0..order {
        let (s, c) =
            jetcarp_core::detmath::sin_cos(std::f64::consts::TAU * k as f64 / order as f64);
        for &(x, y) in &points {
            out.push([q[0] + c * x - s * y, q[1] + s * x + c * y]);
        }
    }
    out
}

fn check_symmetry(spec: TilingSpec, order: u32, mirror: bool) {
    let t = Topology::build(&spec).unwrap();
    let shape = SeedShape::Symmetric {
        order,
        mirror,
        radius: 0.4,
    };
    let (q, axis) = seeding::symmetry_frame(&t, order, mirror);
    let tile = cell_size(&t);
    for rng in [1, 2, 3] {
        let w = world(
            spec.clone(),
            "Cyclic 8/1",
            SeedSpec::Pattern {
                shape: shape.clone(),
                fill: SeedFill::density(0.4).states(SeedStates::Uniform),
            },
            rng,
        );
        let s = states(&w);
        let on = s.iter().filter(|&&x| x != 0).count();
        assert!(on > 8, "too few cells to mean anything: {on}");
        let (mut compared, mut mismatches) = (0, 0);
        for c in 0..t.len() {
            if s[c] == 0 {
                continue;
            }
            for p in orbit(q, t.centroid(c), order, mirror.then_some(axis)) {
                compared += 1;
                let d = t.locate(p).expect("image should be on the tiling");
                let exact = jetcarp_core::detmath::dist(t.centroid(d), p) < 1e-6 * tile;
                assert!(exact, "the tiling's own centroid was not hit at {p:?}");
                if s[d] != s[c] {
                    mismatches += 1;
                }
            }
        }
        assert!(compared > on, "compared {compared} with {on} set");
        assert_eq!(mismatches, 0, "{mismatches} of {compared} on rng {rng}");
    }
}

// Occupancy (and states) of every orbit agree, exactly, where the tiling has the symmetry.
#[test]
fn symmetric_is_exact_on_square_and_hex() {
    for boundary in [Boundary::Dead, Boundary::Wrap] {
        check_symmetry(TilingSpec::rect("square", 100, 100, boundary), 4, false);
        check_symmetry(TilingSpec::rect("square", 100, 100, boundary), 4, true);
        check_symmetry(TilingSpec::rect("square", 91, 91, boundary), 2, false);
        check_symmetry(TilingSpec::rect("square", 91, 91, boundary), 2, true);
        check_symmetry(TilingSpec::rect("hex", 110, 96, boundary), 6, false);
        check_symmetry(TilingSpec::rect("hex", 110, 96, boundary), 6, true);
        check_symmetry(TilingSpec::rect("hex", 110, 96, boundary), 3, false);
        check_symmetry(TilingSpec::rect("hex", 110, 96, boundary), 2, true);
    }
}

// The trihexagonal tiling has 6-fold rotation about a hexagon, not about a triangle: the centre
// must be found, and the seed then matches, mixed tile types and all.
#[test]
fn symmetric_finds_the_six_fold_centre_of_trihex() {
    check_symmetry(TilingSpec::rect("trihex", 50, 50, Boundary::Dead), 6, false);
    check_symmetry(TilingSpec::rect("trihex", 50, 50, Boundary::Dead), 6, true);
}

fn region_cells(p: &Placement) -> HashMap<u32, Vec<usize>> {
    let mut by: HashMap<u32, Vec<usize>> = HashMap::new();
    for (c, &r) in p.region.iter().enumerate() {
        if r != NO_REGION {
            by.entry(r).or_default().push(c);
        }
    }
    by
}

// No two cells of different blobs are closer than the gap, through wrap edges too.
#[test]
fn blobs_keep_their_gap() {
    for spec in [
        TilingSpec::rect("square", 200, 140, Boundary::Dead),
        TilingSpec::rect("square", 200, 140, Boundary::Wrap),
        TilingSpec::rect("hex", 180, 140, Boundary::Wrap),
        TilingSpec::new("spectre").with_size(SizeSpec::Level(4)),
    ] {
        let t = Topology::build(&spec).unwrap();
        let b = t.bounds;
        let gap = 0.05 * (b[2] - b[0]).min(b[3] - b[1]);
        let periods = render::torus_periods(&t);
        let d = |a: P, c: P| -> f64 {
            let Some((t1, t2)) = periods else {
                return jetcarp_core::detmath::dist(a, c);
            };
            let mut best = f64::MAX;
            for i in -1i32..=1 {
                for j in -1i32..=1 {
                    let (i, j) = (i as f64, j as f64);
                    let q = [c[0] + i * t1[0] + j * t2[0], c[1] + i * t1[1] + j * t2[1]];
                    best = best.min(jetcarp_core::detmath::dist(a, q));
                }
            }
            best
        };
        for seed in 0..10 {
            let shape = SeedShape::Blobs {
                count: 8,
                min_radius: 0.04,
                max_radius: 0.08,
                wobble: 1.0,
                min_gap: 0.05,
            };
            let p = seeding::place(&t, &shape, &mut Rng::new(seed)).unwrap();
            let by = region_cells(&p);
            assert!(by.len() >= 4, "only {} blobs placed", by.len());
            let centroids: HashMap<u32, Vec<P>> = by
                .iter()
                .map(|(&r, cells)| (r, cells.iter().map(|&c| t.centroid(c)).collect()))
                .collect();
            let regions: Vec<u32> = centroids.keys().copied().collect();
            for (i, &a) in regions.iter().enumerate() {
                for &e in &regions[i + 1..] {
                    for &x in &centroids[&a] {
                        for &y in &centroids[&e] {
                            assert!(
                                d(x, y) >= gap,
                                "cells {x:?} and {y:?} too close ({})",
                                d(x, y)
                            );
                        }
                    }
                }
            }
        }
    }
}

// Asking for too many blobs terminates with what fits, still apart.
#[test]
fn blobs_stop_when_there_is_no_room() {
    let t = Topology::build(&TilingSpec::rect("square", 60, 60, Boundary::Dead)).unwrap();
    let shape = SeedShape::Blobs {
        count: 5000,
        min_radius: 0.2,
        max_radius: 0.3,
        wobble: 1.0,
        min_gap: 0.2,
    };
    let p = seeding::place(&t, &shape, &mut Rng::new(4)).unwrap();
    let by = region_cells(&p);
    assert!((1..100).contains(&by.len()), "{} blobs", by.len());
}

// Regions: Voronoi cells are whole, keep is honoured, and by_region gives one state per region.
#[test]
fn voronoi_regions() {
    let t = Topology::build(&TilingSpec::rect("square", 120, 120, Boundary::Wrap)).unwrap();
    let shape = SeedShape::Voronoi {
        sites: 40,
        keep: 0.25,
    };
    let p = seeding::place(&t, &shape, &mut Rng::new(9)).unwrap();
    let by = region_cells(&p);
    assert_eq!(by.len(), 10, "keep 25% of 40 regions");

    let mut w = world(
        TilingSpec::rect("square", 120, 120, Boundary::Wrap),
        "Cyclic 8/1",
        SeedSpec::Empty,
        9,
    );
    let mut rng = w.rng;
    let placed = seeding::place(&t, &shape, &mut rng).unwrap();
    let by = region_cells(&placed);
    w.apply(Command::Seed {
        id: w.cas[0].id,
        seed: SeedSpec::Pattern {
            shape,
            fill: SeedFill::density(1.0).states(SeedStates::ByRegion),
        },
    })
    .unwrap();
    let f = &w.fields[w.cas[0].field];
    let mut distinct = HashSet::new();
    for cells in by.values() {
        let s = f.state[cells[0]];
        assert_ne!(s, 0);
        distinct.insert(s);
        assert!(
            cells.iter().all(|&c| f.state[c] == s),
            "a region of mixed states"
        );
    }
    assert!(distinct.len() > 1, "every region took the same state");
}

// Noise patches are connected, so by_region gives each its own state.
#[test]
fn noise_regions_are_connected() {
    let t = Topology::build(&TilingSpec::rect("hex", 100, 80, Boundary::Dead)).unwrap();
    let shape = SeedShape::Noise {
        scale: 0.1,
        threshold: 0.55,
        octaves: 2,
    };
    let p = seeding::place(&t, &shape, &mut Rng::new(2)).unwrap();
    let by = region_cells(&p);
    assert!(by.len() > 2, "{} patches", by.len());
    for (r, cells) in &by {
        let set: HashSet<usize> = cells.iter().copied().collect();
        let mut seen = HashSet::from([cells[0]]);
        let mut stack = vec![cells[0]];
        while let Some(x) = stack.pop() {
            t.for_each_neighbor(x, Nb::Edge, |y| {
                if set.contains(&y) && seen.insert(y) {
                    stack.push(y);
                }
            });
        }
        assert_eq!(seen.len(), cells.len(), "region {r} is split");
    }
}

// Each field kind, with its mapping from a fill.
#[test]
fn field_kinds() {
    let spec = TilingSpec::rect("square", 120, 90, Boundary::Dead);
    let disc = SeedShape::Disc { radius: 0.4 };

    // Multi-state: Uniform covers every nonzero state, One only state 1, ranges stay in range.
    let seed = |s: SeedStates, d: f64| SeedSpec::Pattern {
        shape: disc.clone(),
        fill: SeedFill::density(d).states(s),
    };
    let histo = |seed: SeedSpec, rule: &str| {
        let mut h: HashMap<u8, usize> = HashMap::new();
        for x in states(&world(spec.clone(), rule, seed, 3)) {
            *h.entry(x).or_default() += 1;
        }
        h
    };
    let h = histo(seed(SeedStates::Uniform, 1.0), "Cyclic 8/1");
    assert_eq!(h.len(), 8, "{h:?}");
    assert_eq!(histo(seed(SeedStates::One, 1.0), "Cyclic 8/1").len(), 2);
    let h = histo(seed(SeedStates::Range { lo: 2, hi: 3 }, 1.0), "Cyclic 8/1");
    assert!(h.keys().all(|s| matches!(s, 0 | 2 | 3)), "{h:?}");
    assert_eq!(h.len(), 3, "{h:?}");
    // A range past the field's last state is clamped to it.
    let h = histo(seed(SeedStates::Range { lo: 5, hi: 9 }, 1.0), "Cyclic 4/1");
    assert!(h.keys().all(|s| matches!(s, 0 | 3)), "{h:?}");

    // JACA: both signs from Signed, Uniform and ByRegion; a bad state kind is refused.
    let h = histo(
        seed(SeedStates::Signed { negative: 0.5 }, 1.0),
        "JACA B3/S23",
    );
    assert_eq!(h.len(), 3, "{h:?}");
    let h = histo(
        seed(SeedStates::Signed { negative: 0.0 }, 0.5),
        "JACA B3/S23",
    );
    assert_eq!(h.len(), 2, "{h:?}");
    assert!(h.contains_key(&jaca::POS));
    let h = histo(seed(SeedStates::Uniform, 0.7), "JACA B3/S23");
    assert_eq!(h.len(), 3, "{h:?}");
    let mut sign_seen = HashSet::new();
    for rng in 0..30 {
        let w = world(
            spec.clone(),
            "JACA B3/S23",
            seed(SeedStates::ByRegion, 1.0),
            rng,
        );
        let on: HashSet<u8> = states(&w).into_iter().filter(|&s| s != 0).collect();
        assert_eq!(on.len(), 1, "a disc is one region");
        sign_seen.extend(on);
    }
    assert_eq!(sign_seen.len(), 2, "both signs turn up across seeds");
    // Many regions in one world: both signs, and no gaps.
    let territories = SeedSpec::Pattern {
        shape: SeedShape::Voronoi {
            sites: 30,
            keep: 1.0,
        },
        fill: SeedFill::density(1.0).states(SeedStates::ByRegion),
    };
    let h = histo(territories, "JACA B3/S23");
    assert!(h[&jaca::POS] > 500 && h[&jaca::NEG] > 500, "{h:?}");
    assert!(!h.contains_key(&0), "{h:?}");

    // Signed needs JACA, and out-of-scope rules give a clear error.
    let refuse = |s: SeedSpec, rule: &str, needle: &str| {
        let mut w = World::new(spec.clone(), 1).unwrap();
        let mut ca = CaSpec::new(rule);
        ca.seed = Some(SeedSpec::Empty);
        w.apply(Command::AddCa { ca }).unwrap();
        let e = w
            .apply(Command::Seed {
                id: w.cas[0].id,
                seed: s,
            })
            .unwrap_err();
        assert!(e.contains(needle), "{e}");
    };
    refuse(
        seed(SeedStates::Signed { negative: 0.5 }, 1.0),
        "B3/S23",
        "JACA",
    );
    refuse(seed(SeedStates::One, 1.0), "W30", "elementary");
    refuse(seed(SeedStates::One, 1.0), "Ant RL", "turmites");

    // Continuous: values lie in (0, 1], fall towards the rim, and scale with density.
    let lenia = "Lenia R=13,T=10,b=1,m=0.15,s=0.015";
    let w = world(spec.clone(), lenia, seed(SeedStates::One, 0.8), 3);
    let f = &w.fields[w.cas[0].field];
    assert_eq!(f.kind, FieldKind::Continuous);
    let t = Topology::build(&spec).unwrap();
    let c = [
        (t.bounds[0] + t.bounds[2]) / 2.0,
        (t.bounds[1] + t.bounds[3]) / 2.0,
    ];
    let rr = 0.4 * (t.bounds[2] - t.bounds[0]).min(t.bounds[3] - t.bounds[1]);
    let (mut core, mut n_core, mut rim, mut n_rim) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..t.len() {
        let d = jetcarp_core::detmath::dist(t.centroid(i), c);
        let v = f.value[i];
        assert!((0.0..=1.0).contains(&v));
        assert!(v > 0.0 || d >= 0.95 * rr, "a hole at {d}");
        if d < 0.5 * rr {
            core += v;
            n_core += 1.0;
        } else if d > 0.8 * rr && d <= rr {
            rim += v;
            n_rim += 1.0;
        }
        if d > rr {
            assert_eq!(v, 0.0, "outside the disc");
        }
    }
    assert!((core / n_core - 0.8 * 0.875) < 0.03, "{}", core / n_core);
    assert!(
        rim / n_rim < 0.6 * (core / n_core),
        "soft edge {} vs core {}",
        rim / n_rim,
        core / n_core
    );
    let w = world(spec.clone(), lenia, seed(SeedStates::One, 0.4), 3);
    let (a, b) = (w.fields[w.cas[0].field].mass(), f.mass());
    assert!(a < 0.7 * b, "a lower density should scale the value");

    // Sandpile: mean grains on chosen cells is 2 density degree (degree 8), and they are added.
    let w = world(spec.clone(), "Sandpile", seed(SeedStates::One, 0.5), 3);
    let f = &w.fields[w.cas[0].field];
    assert_eq!(f.kind, FieldKind::Height);
    let (mut sum, mut cells) = (0.0, 0.0);
    for i in 0..t.len() {
        let d = jetcarp_core::detmath::dist(t.centroid(i), c);
        if d < 0.5 * rr && t.degree(i, Nb::Touch) > 0 {
            sum += f.height[i] as f64 / t.degree(i, w.cas[0].nb) as f64;
            cells += 1.0;
        }
    }
    let mean = sum / cells;
    assert!(mean > 0.7 && mean < 1.05, "mean grains / degree = {mean}");
    assert_eq!(
        f.height.iter().filter(|&&h| h > 0).count(),
        f.height.iter().filter(|&&h| h != 0).count()
    );
    let h = world(
        spec.clone(),
        "Sandpile",
        seed(SeedStates::Range { lo: 2, hi: 4 }, 1.0),
        3,
    );
    let f = &h.fields[h.cas[0].field];
    assert!(f.height.iter().all(|&g| g == 0 || (2..=4).contains(&g)));
    assert!(f.height.iter().any(|&g| g > 0));
    let mut again = world(spec.clone(), "Sandpile", seed(SeedStates::One, 0.25), 3);
    let before = again.fields[again.cas[0].field].population();
    let id = again.cas[0].id;
    again
        .apply(Command::Seed {
            id,
            seed: seed(SeedStates::One, 0.25),
        })
        .unwrap();
    assert!(
        again.fields[again.cas[0].field].population() > before,
        "grains add up"
    );
}

// JSON: every variant round trips and reads short forms; describe gives card-sized labels.
#[test]
fn json_round_trip_and_labels() {
    let ugly: Vec<SeedSpec> = shapes()
        .into_iter()
        .map(|s| SeedSpec::Pattern {
            shape: s,
            fill: SeedFill::density(0.123_456_789_012_345_68).states(SeedStates::Signed {
                negative: 1.0 / 3.0,
            }),
        })
        .chain([
            SeedSpec::Empty,
            SeedSpec::Random { density: 0.2 },
            SeedSpec::Pattern {
                shape: SeedShape::Blobs {
                    count: 3,
                    min_radius: 0.1,
                    max_radius: 0.1,
                    wobble: 0.7,
                    min_gap: 0.01,
                },
                fill: SeedFill::density(0.4),
            },
            SeedSpec::Pattern {
                shape: SeedShape::Voronoi {
                    sites: 7,
                    keep: 1.0 / 7.0,
                },
                fill: SeedFill::density(1.0).states(SeedStates::Range { lo: 2, hi: 5 }),
            },
            SeedSpec::Pattern {
                shape: SeedShape::All,
                fill: SeedFill::density(1.0).states(SeedStates::ByRegion),
            },
        ])
        .collect();
    for s in &ugly {
        let j = serde_json::to_string(s).unwrap();
        assert_eq!(&serde_json::from_str::<SeedSpec>(&j).unwrap(), s, "{j}");
    }

    let j = serde_json::to_string(&ugly[0]).unwrap();
    assert!(
        j.starts_with(r#"{"pattern":{"shape":{"kind":"all"}"#),
        "{j}"
    );
    let plain = serde_json::to_value(SeedSpec::Pattern {
        shape: SeedShape::Disc { radius: 0.2 },
        fill: SeedFill::default(),
    })
    .unwrap();
    assert_eq!(
        plain,
        serde_json::json!({"pattern": {"shape": {"kind": "disc", "radius": 0.2}, "fill": {}}})
    );

    // Missing parameters take defaults; a shape can be just its name.
    let s: SeedSpec = serde_json::from_str(r#"{"pattern": {"shape": "blobs"}}"#).unwrap();
    assert!(
        matches!(&s, SeedSpec::Pattern { shape: SeedShape::Blobs { count: 3, .. }, fill } if fill.density == 1.0)
    );
    let s: SeedSpec = serde_json::from_str(
        r#"{"pattern": {"shape": {"kind": "ring", "width": 0.1}, "fill": {"density": 0.5, "states": "by_region"}}}"#,
    )
    .unwrap();
    assert!(matches!(
        s,
        SeedSpec::Pattern {
            shape: SeedShape::Ring { width, .. },
            fill: SeedFill { density, states: SeedStates::ByRegion }
        } if width == 0.1 && density == 0.5
    ));

    assert_eq!(ugly[ugly.len() - 3].describe(), "3 blobs r0.1, 40% density");
    assert_eq!(
        pattern(
            SeedShape::Symmetric {
                order: 6,
                mirror: false,
                radius: 0.3
            },
            1.0
        )
        .describe(),
        "Sym order 6, disc r0.3"
    );
    assert_eq!(
        pattern(SeedShape::Disc { radius: 0.2 }, 0.005).describe(),
        "disc r0.2, 0.5% density"
    );
    for s in ugly {
        assert!(s.describe().len() <= 60, "{}", s.describe());
    }
    for s in [
        SeedSpec::Center,
        SeedSpec::Blob {
            radius: 0.3,
            density: 0.5,
        },
        SeedSpec::TopCenter,
        SeedSpec::TopRandom { density: 0.5 },
        SeedSpec::Ants { count: 3 },
        SeedSpec::Pile { grains: 100 },
        SeedSpec::RandomStates,
        SeedSpec::Signed {
            density: 0.3,
            negative: 0.5,
        },
        SeedSpec::Halves { density: 0.3 },
        SeedSpec::GliderPair,
        SeedSpec::WireDemo,
        SeedSpec::Cells {
            cells: vec![[1, 1]],
        },
        SeedSpec::Gaussian {
            count: 2,
            radius: 8.0,
            peak: 1.0,
        },
        SeedSpec::Lenia {
            name: "orbium".into(),
            scale: 2.0,
        },
    ] {
        let d = s.describe();
        assert!(!d.is_empty() && d.len() <= 40, "{d}");
    }
}

// Scenes carrying pattern seeds replay exactly, and bad parameters are refused.
#[test]
fn scenes_and_validation() {
    let mut s = Scene::new(TilingSpec::rect("hex", 90, 70, Boundary::Wrap));
    s.rng_state = 123;
    s.cas = vec![CaSpec::new("B3/S23").seed(pattern(
        SeedShape::Noise {
            scale: 0.15,
            threshold: 0.5,
            octaves: 3,
        },
        0.8,
    ))];
    let a = World::from_scene(&s).unwrap();
    let json = s.to_json();
    let b = World::from_scene(&Scene::from_json(&json).unwrap()).unwrap();
    assert_eq!(states(&a), states(&b));

    let t = Topology::build(&TilingSpec::rect("square", 10, 10, Boundary::Dead)).unwrap();
    let bad = [
        SeedShape::Disc { radius: 0.0 },
        SeedShape::Disc { radius: f64::NAN },
        SeedShape::Clusters {
            count: 0,
            radius_tiles: 2.0,
        },
        SeedShape::Voronoi {
            sites: 0,
            keep: 0.5,
        },
        SeedShape::Voronoi {
            sites: 3,
            keep: 2.0,
        },
        SeedShape::Symmetric {
            order: 0,
            mirror: false,
            radius: 0.3,
        },
        SeedShape::Symmetric {
            order: 100,
            mirror: false,
            radius: 0.3,
        },
        SeedShape::Noise {
            scale: 0.1,
            threshold: 0.5,
            octaves: 0,
        },
        SeedShape::Blobs {
            count: 1,
            min_radius: 0.3,
            max_radius: 0.1,
            wobble: 1.0,
            min_gap: 0.0,
        },
        SeedShape::Dots {
            spacing_tiles: 0.5,
            jitter: 0.5,
        },
    ];
    for shape in bad {
        assert!(
            seeding::place(&t, &shape, &mut Rng::new(1)).is_err(),
            "{shape:?} was accepted"
        );
    }
    let mut w = world(
        TilingSpec::rect("square", 10, 10, Boundary::Dead),
        "B3/S23",
        SeedSpec::Empty,
        1,
    );
    let id = w.cas[0].id;
    for fill in [
        SeedFill::density(1.5),
        SeedFill::density(0.5).states(SeedStates::Range { lo: 3, hi: 1 }),
        SeedFill::density(0.5).states(SeedStates::Range { lo: 0, hi: 2 }),
    ] {
        let seed = SeedSpec::Pattern {
            shape: SeedShape::All,
            fill,
        };
        assert!(w.apply(Command::Seed { id, seed }).is_err());
    }
}

// Adding pattern seeds leaves every saved scene and the JACA presets readable.
#[test]
fn existing_scenes_still_load() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/scenes");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).unwrap();
            Scene::from_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            count += 1;
        }
    }
    assert!(count >= 20, "only {count} scenes found");
    assert!(
        quickstart::jaca_presets().len() > 50,
        "JACA presets failed to load"
    );
}
