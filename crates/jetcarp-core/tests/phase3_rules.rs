//! Phase 3 rule families: three-parent hexagon rules (HexT, HexT3) on the flat-top hexagon grid.

use jetcarp_core::geom::P;
use jetcarp_core::rules::{HexTMode, Rule};
use jetcarp_core::topology::LatticeGrid;
use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};

fn flat_world(w: u32, h: u32, b: Boundary, rule: &str, seed: SeedSpec, rng: u64) -> World {
    let mut wd = World::new(TilingSpec::rect("hex_flat", w, h, b), rng).unwrap();
    wd.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(seed),
    })
    .unwrap();
    wd
}

/// The hexagon in column `c` (0..2w) of half-row layer `l` (0..2h); `c` and `l` have the same parity.
fn cell(g: &LatticeGrid, l: u32, c: u32) -> usize {
    g.index((c / 2) as i32, (l / 2) as i32, l % 2)
}

/// Reference implementation, independent of the engine's lattice: layers of `cols` columns where
/// layer `l` holds the columns of its parity, each cell reading (l-1, c-1), (l-2, c), (l-1, c+1).
fn reference(
    k: u32,
    table: &dyn Fn(u32, u32, u32) -> u32,
    top: &[u32],
    layers: usize,
) -> Vec<Vec<u32>> {
    let cols = top.len();
    let mut out = vec![vec![0u32; cols]; layers];
    out[0] = top.to_vec();
    for l in 1..layers {
        for c in 0..cols {
            if c % 2 != l % 2 {
                continue;
            }
            let at = |ll: isize, cc: isize| -> u32 {
                if ll < 0 || cc < 0 || cc >= cols as isize {
                    0
                } else {
                    out[ll as usize][cc as usize]
                }
            };
            let (l, ci) = (l as isize, c as isize);
            let v = table(at(l - 1, ci - 1), at(l - 2, ci), at(l - 1, ci + 1)) % k;
            out[l as usize][c] = v;
        }
    }
    out
}

#[test]
fn flat_hexagons_have_three_parents_above() {
    let t =
        jetcarp_core::Topology::build(&TilingSpec::rect("hex_flat", 12, 10, Boundary::Dead)).unwrap();
    let g = t.flat_hex_rows().expect("flat-top hexagon rows");
    assert_eq!((g.w, g.h, g.ntypes), (12, 10, 2));
    let (edge, touch) = t.degree_sets();
    assert_eq!((edge, touch), (vec![6], vec![6]));
    // For every interior hexagon, the edge neighbours above it are NW, N, NE at half, one, and
    // half a row up: exactly the parents the rule reads.
    for l in 2..2 * g.h - 1 {
        for c in 2..2 * g.w - 2 {
            if c % 2 != l % 2 {
                continue;
            }
            let i = cell(g, l, c);
            let ci = t.centroid(i);
            let mut above: Vec<(P, usize)> = Vec::new();
            t.for_each_neighbor(i, jetcarp_core::Nb::Edge, |j| {
                let cj = t.centroid(j);
                if cj[1] < ci[1] - 1e-9 {
                    above.push((cj, j));
                }
            });
            above.sort_by(|a, b| a.0[0].partial_cmp(&b.0[0]).unwrap());
            let want = [
                cell(g, l - 1, c - 1),
                cell(g, l - 2, c),
                cell(g, l - 1, c + 1),
            ];
            assert_eq!(
                above.iter().map(|x| x.1).collect::<Vec<_>>(),
                want,
                "layer {l} column {c}"
            );
            let s3 = 3f64.sqrt();
            assert!((ci[1] - above[0].0[1] - s3 / 2.0).abs() < 1e-9);
            assert!((ci[1] - above[1].0[1] - s3).abs() < 1e-9);
            assert!((ci[0] - above[1].0[0]).abs() < 1e-9);
        }
    }
}

#[test]
fn rule_numbering_matches_a_reference() {
    let (w, h) = (20u32, 12u32);
    type Case = (String, u32, Box<dyn Fn(u32, u32, u32) -> u32>);
    let cases: Vec<Case> = {
        let mut v: Vec<Case> = Vec::new();
        for n in [30u64, 90, 110, 150, 177, 255] {
            v.push((
                format!("HexT {n}"),
                2,
                Box::new(move |a, b, c| ((n >> (a * 4 + b * 2 + c)) & 1) as u32),
            ));
        }
        for n in [1u64, 8229, 7_625_597_484_986, 4_000_000_000_123] {
            v.push((
                format!("HexT3 {n}"),
                3,
                Box::new(move |a, b, c| ((n / 3u64.pow(a * 9 + b * 3 + c)) % 3) as u32),
            ));
        }
        for n in [588u64, 1000, 2186] {
            v.push((
                format!("HexT3 sum {n}"),
                3,
                Box::new(move |a, b, c| ((n / 3u64.pow(a + b + c)) % 3) as u32),
            ));
        }
        for n in [6u64, 9] {
            v.push((
                format!("HexT sum {n}"),
                2,
                Box::new(move |a, b, c| ((n >> (a + b + c)) & 1) as u32),
            ));
        }
        v
    };
    for (rule, k, f) in &cases {
        for seed in 1..4u64 {
            let mut wd = flat_world(
                w,
                h,
                Boundary::Dead,
                rule,
                SeedSpec::TopRandom { density: 0.5 },
                seed,
            );
            if *k == 3 {
                // Some state-2 cells in the top layer too.
                let g = wd.topo.flat_hex_rows().unwrap().clone();
                let cells: Vec<[u32; 2]> = (0..w)
                    .filter(|u| u % 3 == 0)
                    .map(|u| [g.index(u as i32, 0, 0) as u32, 2])
                    .collect();
                wd.apply(Command::SetCells { id: 1, cells }).unwrap();
            }
            let g = wd.topo.flat_hex_rows().unwrap().clone();
            let top: Vec<u32> = (0..2 * w)
                .map(|c| {
                    if c % 2 == 0 {
                        wd.fields[0].state[cell(&g, 0, c)] as u32
                    } else {
                        0
                    }
                })
                .collect();
            let r = wd.apply(Command::Step {
                gens: 2 * h as u64 + 5,
            });
            r.unwrap();
            let want = reference(*k, f.as_ref(), &top, 2 * h as usize);
            for l in 0..2 * h {
                for c in (0..2 * w).filter(|c| c % 2 == l % 2) {
                    assert_eq!(
                        wd.fields[0].state[cell(&g, l, c)] as u32,
                        want[l as usize][c as usize],
                        "{rule} seed {seed} layer {l} col {c}"
                    );
                }
            }
        }
    }
}

#[test]
fn symmetric_numbering_ignores_left_and_right() {
    for n in [0u64, 1, 12345, 387_420_488] {
        let (r, _) = Rule::parse(&format!("HexT3 sym {n}")).unwrap();
        let t = r.hext_table().unwrap();
        for a in 0..3 {
            for b in 0..3 {
                for c in 0..3 {
                    assert_eq!(t[a * 9 + b * 3 + c], t[c * 9 + b * 3 + a]);
                }
            }
        }
        // Entry order: pairs (0,0) (0,1) (0,2) (1,1) (1,2) (2,2), then N.
        let pairs = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)];
        for (p, &(x, y)) in pairs.iter().enumerate() {
            for nn in 0..3 {
                let d = (n / 3u64.pow((p * 3 + nn) as u32)) % 3;
                assert_eq!(t[x * 9 + nn * 3 + y] as u64, d);
            }
        }
    }
    assert_eq!(HexTMode::Sym.max(3), 3u64.pow(18) - 1);
    assert_eq!(HexTMode::Full.max(3), 7_625_597_484_986);
    assert_eq!(HexTMode::Sum.max(3), 2186);
    assert_eq!(HexTMode::Full.max(2), 255);
    assert!(Rule::parse("HexT 256").is_err());
    assert!(Rule::parse("HexT3 7625597484987").is_err());
    assert!(Rule::parse("HexT3 sum 2187").is_err());
}

#[test]
fn xor_rules_give_pascal_counts() {
    // HexT 90 = NW xor NE: layer n from one cell holds C(n, k) mod 2, so 2^popcount(n) live cells.
    let (w, h) = (80u32, 30u32);
    let mut wd = flat_world(w, h, Boundary::Dead, "HexT 90", SeedSpec::TopCenter, 1);
    wd.apply(Command::Step { gens: 2 * h as u64 }).unwrap();
    let g = wd.topo.flat_hex_rows().unwrap().clone();
    for l in 0..2 * h {
        let live = (0..2 * w)
            .filter(|&c| c % 2 == l % 2 && wd.fields[0].state[cell(&g, l, c)] == 1)
            .count();
        assert_eq!(live, 1 << l.count_ones(), "layer {l}");
    }
    // HexT3 sum 588 = (NW + N + NE) mod 3: layer n is P_n(x) = (x + 1/x) P_{n-1} + P_{n-2} mod 3,
    // P_0 = 1, P_{-1} = 0 (Fibonacci polynomials in x + 1/x). Counted per state.
    let mut wd = flat_world(
        w,
        h,
        Boundary::Dead,
        "HexT3 sum 588",
        SeedSpec::TopCenter,
        1,
    );
    wd.apply(Command::Step { gens: 2 * h as u64 }).unwrap();
    let span = 2 * (2 * h as usize) + 1;
    let mid = span / 2;
    let mut prev = vec![0u32; span];
    let mut cur = vec![0u32; span];
    cur[mid] = 1;
    let centre = w; // the seeded column: type 0, unit cell w / 2
    for l in 0..2 * h {
        if l > 0 {
            let mut next = vec![0u32; span];
            for x in 1..span - 1 {
                next[x] = (cur[x - 1] + cur[x + 1] + prev[x]) % 3;
            }
            prev = std::mem::replace(&mut cur, next);
        }
        for s in 1..3u32 {
            let want = cur
                .iter()
                .enumerate()
                .filter(|&(x, &v)| {
                    let c = centre as i64 + x as i64 - mid as i64;
                    v == s && c >= 0 && c < 2 * w as i64
                })
                .count();
            let got = (0..2 * w)
                .filter(|&c| c % 2 == l % 2 && wd.fields[0].state[cell(&g, l, c)] as u32 == s)
                .count();
            assert_eq!(got, want, "layer {l} state {s}");
        }
    }
}

#[test]
fn done_at_the_bottom_layer() {
    let (w, h) = (16u32, 10u32);
    let mut wd = flat_world(
        w,
        h,
        Boundary::Dead,
        "HexT3 sum 588",
        SeedSpec::TopCenter,
        1,
    );
    let mut gens = 0;
    while !wd.done() {
        wd.step();
        gens += 1;
        assert!(gens < 100);
    }
    assert_eq!(gens, 2 * h - 1);
    assert_eq!(wd.done_reason().as_deref(), Some("reached the bottom row"));
    // Wrap edges: one full pass, then the picture stays as it is.
    let mut wd = flat_world(w, h, Boundary::Wrap, "HexT 90", SeedSpec::TopCenter, 1);
    let mut gens = 0;
    while !wd.done() {
        wd.apply(Command::Step { gens: 1 }).unwrap();
        gens += 1;
    }
    assert_eq!(gens, 2 * h - 1);
    assert_eq!(
        wd.done_reason().as_deref(),
        Some("a full pass reached the bottom row")
    );
    let finished = wd.fields[0].state.clone();
    wd.apply(Command::Step { gens: 1 }).unwrap();
    assert!(wd.done());
    assert_eq!(wd.fields[0].state, finished);
    // Scenes replay to the same hash.
    let sc = wd.to_scene();
    let w2 = World::from_scene(&sc).unwrap();
    assert_eq!(w2.hash_hex(), wd.hash_hex());
    // The three-parent rules need the flat-top grid.
    let mut sq = World::new(TilingSpec::rect("hex", 16, 16, Boundary::Dead), 1).unwrap();
    assert!(
        sq.apply(Command::AddCa {
            ca: CaSpec::new("HexT 30")
        })
        .is_err()
    );
}

#[test]
fn notation_round_trips() {
    for s in [
        "HexT 30",
        "HexT sum 6",
        "HexT3 7625597484986",
        "HexT3 sum 777",
        "HexT3 sym 1234",
    ] {
        assert_eq!(Rule::parse(s).unwrap().0.notation(), s);
    }
    assert_eq!(
        Rule::parse("hext3  SUM 5").unwrap().0.notation(),
        "HexT3 sum 5"
    );
}

// ---------------------------------------------------------------------------------------------
// Margolus block rules

fn margolus_table(name: &str) -> [u8; 16] {
    jetcarp_core::rules::margolus_named(name).unwrap()
}

fn square(w: u32, h: u32, b: Boundary, rule: &str, seed: SeedSpec, rng: u64) -> World {
    let mut wd = World::new(TilingSpec::rect("square", w, h, b), rng).unwrap();
    wd.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(seed),
    })
    .unwrap();
    wd
}

fn live(w: &World) -> Vec<usize> {
    (0..w.topo.len())
        .filter(|&i| w.fields[0].state[i] != 0)
        .collect()
}

#[test]
fn critters_runs_backwards() {
    let t = margolus_table("critters");
    let mut inv = [0u8; 16];
    for (x, &y) in t.iter().enumerate() {
        inv[y as usize] = x as u8;
    }
    let inverse: Vec<String> = inv.iter().map(|x| x.to_string()).collect();
    for b in [Boundary::Wrap, Boundary::Dead] {
        let mut w = square(
            40,
            30,
            b,
            "Margolus Critters",
            SeedSpec::Blob {
                radius: 0.5,
                density: 0.4,
            },
            7,
        );
        let start = w.fields[0].state.clone();
        // An odd number of steps: the last forward step used phase 0, which is the phase the
        // inverse rule (step count reset by the rule change) starts with.
        w.apply(Command::Step { gens: 101 }).unwrap();
        assert_ne!(w.fields[0].state, start);
        w.apply(Command::UpdateCa {
            id: 1,
            change: jetcarp_core::CaChange {
                rule: Some(format!("Margolus {}", inverse.join(","))),
                ..Default::default()
            },
        })
        .unwrap();
        w.apply(Command::Step { gens: 101 }).unwrap();
        assert_eq!(w.fields[0].state, start, "{b:?}");
    }
}

#[test]
fn billiard_balls_are_conserved_and_fly_diagonally() {
    for b in [Boundary::Wrap, Boundary::Dead] {
        let mut w = square(
            64,
            48,
            b,
            "Margolus BBM",
            SeedSpec::Random { density: 0.2 },
            3,
        );
        let p0 = w.fields[0].population();
        let mut changed = false;
        for _ in 0..300 {
            let before = w.fields[0].state.clone();
            w.apply(Command::Step { gens: 1 }).unwrap();
            changed |= before != w.fields[0].state;
            assert_eq!(w.fields[0].population(), p0, "{b:?}");
        }
        assert!(changed);
    }
    // A lone ball in the upper left of its block moves one cell down-right per generation.
    let mut w = square(32, 32, Boundary::Wrap, "Margolus BBM", SeedSpec::Empty, 1);
    w.apply(Command::SetCells {
        id: 1,
        cells: vec![[10 * 32 + 10, 1]],
    })
    .unwrap();
    for n in 1..=8u32 {
        w.apply(Command::Step { gens: 1 }).unwrap();
        assert_eq!(live(&w), vec![((10 + n) * 32 + 10 + n) as usize]);
    }
    // Single Rotation: a lone cell goes round a 2x2 square with period 4 (Mishin 2013).
    let mut w = square(
        32,
        32,
        Boundary::Wrap,
        "Margolus SingleRotation",
        SeedSpec::Empty,
        1,
    );
    w.apply(Command::SetCells {
        id: 1,
        cells: vec![[10 * 32 + 10, 1]],
    })
    .unwrap();
    let mut path = vec![];
    for _ in 0..4 {
        w.apply(Command::Step { gens: 1 }).unwrap();
        path.push(live(&w)[0]);
    }
    assert_eq!(
        path,
        vec![10 * 32 + 11, 9 * 32 + 11, 9 * 32 + 10, 10 * 32 + 10]
    );
    // Tron flips an empty wrapped grid to full and back.
    let mut w = square(16, 12, Boundary::Wrap, "Margolus Tron", SeedSpec::Empty, 1);
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert_eq!(w.fields[0].population(), 16 * 12);
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert_eq!(w.fields[0].population(), 0);
}

#[test]
fn margolus_phase_is_part_of_the_state() {
    let mut w = square(
        24,
        20,
        Boundary::Wrap,
        "Margolus Critters",
        SeedSpec::Random { density: 0.3 },
        5,
    );
    w.apply(Command::Step { gens: 37 }).unwrap();
    let w2 = World::from_scene(&w.to_scene()).unwrap();
    assert_eq!(w2.hash_hex(), w.hash_hex());
    assert_eq!(w2.fields[0].state, w.fields[0].state);
    // A still pattern under one partition is not "done": the next partition differs.
    let mut w = square(16, 16, Boundary::Dead, "Margolus BBM", SeedSpec::Empty, 1);
    // Two balls side by side in one phase-0 block stay put in phase 0, then move in phase 1.
    w.apply(Command::SetCells {
        id: 1,
        cells: vec![[4 * 16 + 4, 1], [4 * 16 + 5, 1]],
    })
    .unwrap();
    w.set_track_changes(true);
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert!(!w.done());
    // Odd sides with wrap edges: blocks never straddle the odd seam, and nothing breaks.
    let mut w = square(
        15,
        13,
        Boundary::Wrap,
        "Margolus BBM",
        SeedSpec::Random { density: 0.2 },
        2,
    );
    let p0 = w.fields[0].population();
    w.apply(Command::Step { gens: 50 }).unwrap();
    assert_eq!(w.fields[0].population(), p0);
    // Margolus rules need the square grid.
    let mut h = World::new(TilingSpec::rect("hex", 16, 16, Boundary::Dead), 1).unwrap();
    assert!(
        h.apply(Command::AddCa {
            ca: CaSpec::new("Margolus BBM")
        })
        .is_err()
    );
}

// ---------------------------------------------------------------------------------------------
// Lenia

const ORBIUM: &str = "Lenia R=13,T=10,b=1,m=0.15,s=0.015";

fn lenia_rule(s: &str) -> jetcarp_core::lenia::LeniaRule {
    match Rule::parse(s).unwrap().0 {
        Rule::Lenia(l) => *l,
        r => panic!("{r:?}"),
    }
}

#[test]
fn lenia_kernels_sum_to_one() {
    for (fam, rule) in [
        ("square", ORBIUM),
        ("square", "Lenia R=10,T=10,b=1/2,1,m=0.26,s=0.036,kn=2,gn=2"),
        ("hex", ORBIUM),
        ("triangle", "Lenia R=6,T=5,b=1,m=0.15,s=0.015"),
        ("snubsq", "Lenia R=6,T=5,b=1,m=0.15,s=0.015"),
        ("hex_flat", ORBIUM),
    ] {
        let t = jetcarp_core::Topology::build(&TilingSpec::rect(fam, 40, 40, Boundary::Wrap)).unwrap();
        let k = jetcarp_core::lenia::kernel(&t, &lenia_rule(rule)).unwrap();
        for s in k.sums() {
            assert!((s - 1.0).abs() < 1e-5, "{fam} {rule}: {s}");
        }
    }
    // Square, R = 13: the taps are the cells strictly inside the radius, less the centre
    // (the polynomial core is 0 at r = 0).
    let t =
        jetcarp_core::Topology::build(&TilingSpec::rect("square", 40, 40, Boundary::Wrap)).unwrap();
    let k = jetcarp_core::lenia::kernel(&t, &lenia_rule(ORBIUM)).unwrap();
    let inside = (-13i32..=13)
        .flat_map(|x| (-13i32..=13).map(move |y| (x, y)))
        .filter(|&(x, y)| x * x + y * y < 169 && (x, y) != (0, 0))
        .count();
    assert_eq!(k.taps(), inside);
    // Spectre patch: per-cell lists, interior cells sum to 1.
    let t = jetcarp_core::Topology::build(
        &TilingSpec::new("spectre").with_size(jetcarp_core::SizeSpec::Level(2)),
    )
    .unwrap();
    let k = jetcarp_core::lenia::kernel(&t, &lenia_rule("Lenia R=3,T=10,b=1,m=0.15,s=0.015")).unwrap();
    let sums = k.sums();
    let ones = sums.iter().filter(|s| (*s - 1.0).abs() < 1e-5).count();
    assert!(ones * 2 > sums.len(), "{ones} of {}", sums.len());
    assert!(sums.iter().all(|&s| s <= 1.0 + 1e-5));
}

#[test]
fn lenia_notation_and_orbium_pattern() {
    let l = lenia_rule(ORBIUM);
    assert_eq!(
        (l.r, l.t, l.b.clone(), l.m, l.s, l.kn, l.gn),
        (13.0, 10.0, vec![1.0], 0.15, 0.015, 1, 1)
    );
    assert_eq!(
        Rule::parse("lenia R=13, T=10, b=[1], m=0.15, s=0.015, kn=1, gn=1")
            .unwrap()
            .0
            .notation(),
        ORBIUM
    );
    assert_eq!(
        lenia_rule("Lenia b=[1/2,1,1/3],m=0.2").b,
        vec![0.5, 1.0, 1.0 / 3.0]
    );
    assert!(Rule::parse("Lenia R=0").is_err());
    assert!(Rule::parse("Lenia x=3").is_err());
    let p = jetcarp_core::lenia::pattern("orbium").unwrap();
    assert_eq!((p.len(), p[0].len()), (20, 20));
    assert!(p.iter().flatten().all(|&v| (0.0..=1.0).contains(&v)));
    // A few values decoded by hand from Chan's string: row 0 is "7.MD6.qL".
    assert_eq!(p[0][7], 13.0 / 255.0);
    assert_eq!(p[0][8], 4.0 / 255.0);
    assert_eq!(
        p[0][15],
        ((b'q' - b'p') as f32 * 24.0 + (b'L' - b'A') as f32 + 25.0) / 255.0
    );
}

fn orbium_world(w: u32, h: u32) -> World {
    let mut wd = World::new(TilingSpec::rect("square", w, h, Boundary::Wrap), 1).unwrap();
    let ca = CaSpec::new(ORBIUM).seed(SeedSpec::Lenia {
        name: "orbium".into(),
        scale: 1.0,
    });
    wd.apply(Command::AddCa { ca }).unwrap();
    wd
}

fn centre_of_mass(w: &World) -> [f64; 2] {
    let g = w.topo.lattice().unwrap();
    let (mut sx, mut sy, mut m) = (0.0, 0.0, 0.0);
    for (i, &v) in w.fields[0].value.iter().enumerate() {
        let (u, j, _) = g.decompose(i);
        sx += u as f64 * v as f64;
        sy += j as f64 * v as f64;
        m += v as f64;
    }
    [sx / m, sy / m]
}

#[test]
fn orbium_glides_and_keeps_its_mass() {
    let mut w = orbium_world(96, 96);
    let m0 = w.fields[0].mass();
    let c0 = centre_of_mass(&w);
    for _ in 0..10 {
        w.apply(Command::Step { gens: 30 }).unwrap();
        let m = w.fields[0].mass();
        assert!((m - m0).abs() / m0 < 0.15, "mass {m} vs {m0}");
    }
    let c1 = centre_of_mass(&w);
    let moved = ((c1[0] - c0[0]).powi(2) + (c1[1] - c0[1]).powi(2)).sqrt();
    assert!(moved > 10.0 && moved < 60.0, "moved {moved}");
    // Still a compact creature: nonzero cells stay a small blob.
    let pop = w.fields[0].population();
    assert!((100..600).contains(&pop), "population {pop}");
    assert!(!w.done());
}

#[test]
fn lenia_is_deterministic_and_replays() {
    let run = |threads: usize| -> (String, Vec<f32>) {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut w =
                    World::new(TilingSpec::rect("square", 70, 50, Boundary::Dead), 9).unwrap();
                let ca = CaSpec::new("Lenia R=8,T=10,b=1,m=0.15,s=0.017").seed(SeedSpec::Blob {
                    radius: 0.6,
                    density: 0.6,
                });
                w.apply(Command::AddCa { ca }).unwrap();
                let mut ca2 = CaSpec::new(ORBIUM).seed(SeedSpec::Gaussian {
                    count: 3,
                    radius: 4.0,
                    peak: 0.8,
                });
                ca2.affects_others = Some(false);
                w.apply(Command::AddCa { ca: ca2 }).unwrap();
                w.apply(Command::Step { gens: 25 }).unwrap();
                (w.hash_hex(), w.fields[0].value.clone())
            })
    };
    let a = run(1);
    let b = run(7);
    let c = run(16);
    assert_eq!(a, b);
    assert_eq!(a, c);
    // Scenes replay to the same hash; the hash sees the exact values.
    let mut w = orbium_world(64, 64);
    w.apply(Command::Step { gens: 17 }).unwrap();
    let w2 = World::from_scene(&w.to_scene()).unwrap();
    assert_eq!(w2.hash_hex(), w.hash_hex());
    let h = w.hash_hex();
    w.fields[0].value[5] = f32::from_bits(w.fields[0].value[5].to_bits() ^ 1);
    assert_ne!(w.hash_hex(), h);
    // Lenia on hexagons and on a Spectre patch runs too.
    for spec in [
        TilingSpec::rect("hex", 60, 50, Boundary::Wrap),
        TilingSpec::new("spectre").with_size(jetcarp_core::SizeSpec::Level(3)),
    ] {
        let mut w = World::new(spec, 3).unwrap();
        let ca = CaSpec::new("Lenia R=5,T=10,b=1,m=0.15,s=0.017").seed(SeedSpec::Blob {
            radius: 0.5,
            density: 0.7,
        });
        w.apply(Command::AddCa { ca }).unwrap();
        let m0 = w.fields[0].mass();
        w.apply(Command::Step { gens: 10 }).unwrap();
        assert!(w.fields[0].mass() != m0);
    }
    // An empty Lenia field stays empty, so the world is done.
    let mut w = World::new(TilingSpec::rect("square", 32, 32, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new(ORBIUM).seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert!(w.done());
}

// The Quick Start examples of the new families, at preview size, do what they are shown for:
// Orbium is still one creature of about the same mass; the conserving block rules keep their
// particle count; every example is still changing.
#[test]
fn new_quick_start_examples_behave() {
    use jetcarp_core::quickstart::{entries, preview_cells, preview_scene};
    let all = entries();
    let pick = |rule: &str| {
        all.iter()
            .find(|e| e.rule == rule)
            .unwrap_or_else(|| panic!("no example for {rule}"))
    };
    let run = |rule: &str, gens: u64| -> (World, World) {
        let e = pick(rule);
        let sc = preview_scene(&e.scene, preview_cells(&e.scene, 6000).unwrap());
        let w0 = World::from_scene(&sc).unwrap();
        let mut w = World::from_scene(&sc).unwrap();
        w.apply(Command::Step { gens }).unwrap();
        (w0, w)
    };
    let (w0, w) = run(ORBIUM, 300);
    let (m0, m) = (w0.fields[0].mass(), w.fields[0].mass());
    assert!(
        (m - m0).abs() / m0 < 0.15 && w.fields[0].population() < 600,
        "Orbium: mass {m0} -> {m}"
    );
    for name in [
        "Billiard ball machine",
        "Single Rotation",
        "HPP gas (Margolus)",
    ] {
        let e = jetcarp_core::rules::catalog()
            .iter()
            .find(|e| e.name == name)
            .unwrap();
        let (w0, w) = run(e.notation, 300);
        assert_eq!(
            w.fields[0].population(),
            w0.fields[0].population(),
            "{name}"
        );
        assert!(w0.fields[0].population() > 0);
    }
    for rule in ["HexT 90", "HexT3 sum 588", "HexT3 sum 314"] {
        let (_, w) = run(rule, 1000);
        assert!(w.done() && w.fields[0].population() > 0, "{rule}");
    }
}

/// Lenia states are exact f32 bits, so the hash pins every value. The growth and kernel use
/// `jetcarp_core::detmath` (no platform `exp` or `hypot`), so these hashes must be the same on every
/// OS and CPU; a failure here on another platform means some maths is not portable.
#[test]
fn lenia_hashes_are_portable() {
    let run = |family: &str, w: u32, h: u32, rule: &str, seed: SeedSpec| -> String {
        let mut wd = World::new(TilingSpec::rect(family, w, h, Boundary::Wrap), 7).unwrap();
        wd.apply(Command::AddCa {
            ca: CaSpec::new(rule).seed(seed),
        })
        .unwrap();
        wd.apply(Command::Step { gens: 30 }).unwrap();
        wd.hash_hex()
    };
    let lenia = |name: &str| SeedSpec::Lenia {
        name: name.into(),
        scale: 1.0,
    };
    assert_eq!(
        run("square", 96, 96, ORBIUM, lenia("orbium")),
        "ebdff31cdf1e4130"
    );
    // Exponential kernel core and Gaussian growth (both use exp), from Gaussian bumps (exp).
    assert_eq!(
        run(
            "square",
            64,
            64,
            "Lenia R=10,T=10,b=1/2,1,m=0.26,s=0.036,kn=2,gn=2",
            SeedSpec::Gaussian {
                count: 3,
                radius: 6.0,
                peak: 1.0
            }
        ),
        "0c1a7b79a60d5999"
    );
    assert_eq!(
        run(
            "hex",
            64,
            64,
            "Lenia R=8,T=10,b=1,m=0.15,s=0.017,gn=2",
            SeedSpec::Blob {
                radius: 0.4,
                density: 0.6
            }
        ),
        "f731ec3b4dc887d7"
    );
}
