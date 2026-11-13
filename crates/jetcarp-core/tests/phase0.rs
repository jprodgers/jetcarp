//! Phase 0 automated checks from PLAN.md. These must pass.

use jetcarp_core::rules::FieldKind;
use jetcarp_core::world::*;
use jetcarp_core::*;
use std::collections::BTreeSet;

fn square(w: u32, h: u32, b: Boundary) -> TilingSpec {
    TilingSpec::rect("square", w, h, b)
}

// 1. Spectre patches contain 559, 4,401, and 34,649 tiles, with interior degrees of 4–7 by edge
//    and 6–7 by touch.
#[test]
fn spectre_patch_counts_and_degrees() {
    for (level, count) in [(3, 559), (4, 4401), (5, 34649)] {
        let t =
            Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(level))).unwrap();
        assert_eq!(t.len(), count, "level {level}");
        let (edge, touch) = t.degree_sets();
        assert_eq!(edge, vec![4, 5, 6, 7], "edge degrees at level {level}");
        assert_eq!(touch, vec![6, 7], "touch degrees at level {level}");
        // Neighbour relation is symmetric.
        for i in 0..t.len() {
            for nb in [Nb::Edge, Nb::Touch] {
                t.for_each_neighbor(i, nb, |j| {
                    let mut back = false;
                    t.for_each_neighbor(j, nb, |k| back |= k == i);
                    assert!(back, "asymmetric {nb:?} neighbours {i} {j}");
                });
            }
        }
    }
}

// 2. A Life glider returns to its shape after 4 generations, shifted one cell diagonally.
#[test]
fn life_glider_moves_diagonally() {
    for b in [Boundary::Dead, Boundary::Wrap] {
        let mut w = World::new(square(24, 24, b), 7).unwrap();
        let g = w.topo.lattice().unwrap().clone();
        let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        let cells: Vec<[u32; 2]> = glider
            .iter()
            .map(|&(x, y)| [g.index(x + 5, y + 5, 0) as u32, 1])
            .collect();
        w.apply(Command::AddCa {
            ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
        })
        .unwrap();
        w.apply(Command::Step { gens: 4 }).unwrap();
        let live: BTreeSet<(i32, i32)> = (0..w.topo.len())
            .filter(|&i| w.fields[0].state[i] == 1)
            .map(|i| {
                let (u, j, _) = g.decompose(i);
                (u, j)
            })
            .collect();
        let want: BTreeSet<(i32, i32)> = glider.iter().map(|&(x, y)| (x + 6, y + 6)).collect();
        assert_eq!(live, want, "{b:?}");
    }
}

/// Reference rows from an independent Python implementation, as decimal numbers of the trimmed row.
/// Rule 30's values match OEIS A110240.
const RULE30: [u64; 13] = [
    1, 7, 25, 111, 401, 1783, 6409, 28479, 102849, 456263, 1641433, 7287855, 26332369,
];
/// Rule 110 from one cell: 1, 11, 111, 1101, 11111, 110001, 1110011, 11010111, ...
const RULE110: [u64; 16] = [
    1, 3, 7, 13, 31, 49, 115, 215, 509, 775, 1805, 3359, 7985, 12659, 29655, 54909,
];

fn wolfram_rows(rule: u8, n: usize) -> Vec<(u64, i32, i32)> {
    let mut w = World::new(square(64, 40, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new(&format!("W{rule}")),
    })
    .unwrap();
    w.apply(Command::Step { gens: n as u64 }).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    (0..n)
        .map(|j| {
            let row: Vec<u8> = (0..g.w as i32)
                .map(|u| w.fields[0].state[g.index(u, j as i32, 0)])
                .collect();
            let lo = row.iter().position(|&v| v != 0).unwrap() as i32;
            let hi = row.iter().rposition(|&v| v != 0).unwrap() as i32;
            let v = row[lo as usize..=hi as usize]
                .iter()
                .fold(0u64, |a, &b| a * 2 + b as u64);
            (v, lo, hi)
        })
        .collect()
}

// 3. Rule 30 and rule 110 match reference rows.
#[test]
fn wolfram_rule30_and_110_rows() {
    let c = 32;
    for (k, (v, lo, hi)) in wolfram_rows(30, RULE30.len()).into_iter().enumerate() {
        assert_eq!(v, RULE30[k], "rule 30 row {k}");
        assert_eq!(
            (lo, hi),
            (c - k as i32, c + k as i32),
            "rule 30 row {k} extent"
        );
    }
    for (k, (v, lo, hi)) in wolfram_rows(110, RULE110.len()).into_iter().enumerate() {
        assert_eq!(v, RULE110[k], "rule 110 row {k}");
        assert_eq!(hi, c, "rule 110 grows to the left");
        assert_eq!(
            lo,
            c - k as i32,
            "rule 110 row {k} starts {k} cells left of the seed"
        );
    }
}

// 4. Langton's ant forms its highway, repeating every 104 steps.
#[test]
fn langtons_ant_highway() {
    let mut w = World::new(square(160, 160, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("Ant RL").seed(SeedSpec::Ants { count: 1 }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 11_000 }).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let pos = |w: &World| {
        let a = w.cas[0].agents[0];
        let (u, j, _) = g.decompose(a.cell as usize);
        (u, j, a.heading)
    };
    let (u0, j0, h0) = pos(&w);
    let snap0 = w.fields[0].state.clone();
    w.apply(Command::Step { gens: 104 }).unwrap();
    let (u1, j1, h1) = pos(&w);
    let (du, dj) = (u1 - u0, j1 - j0);
    assert_eq!(
        (du.abs(), dj.abs()),
        (2, 2),
        "highway moves 2 cells diagonally per 104 steps"
    );
    assert_eq!(h0, h1, "same heading after one period");
    // The local pattern around the ant repeats, shifted by the same offset.
    for y in -6..=6 {
        for x in -6..=6 {
            let a = snap0[g.index(u0 + x, j0 + y, 0)];
            let b = w.fields[0].state[g.index(u1 + x, j1 + y, 0)];
            assert_eq!(a, b, "cell ({x},{y}) around the ant");
        }
    }
}

fn total_grains(w: &World) -> u64 {
    w.fields
        .iter()
        .filter(|f| f.kind == FieldKind::Height)
        .map(|f| f.population() + f.lost)
        .sum()
}

// 5a. Sandpile grains are conserved (grains on the grid plus grains lost off the edge).
#[test]
fn sandpile_conserves_grains() {
    for spec in [
        square(40, 40, Boundary::Dead),
        square(40, 40, Boundary::Wrap),
        TilingSpec::rect("hex", 30, 30, Boundary::Dead),
        TilingSpec::new("cairo").with_size(SizeSpec::Cells(900)),
        TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
    ] {
        let mut w = World::new(spec.clone(), 3).unwrap();
        let mut ca = CaSpec::new("Sandpile").seed(SeedSpec::Pile { grains: 5000 });
        ca.drop_per_gen = 7;
        w.apply(Command::AddCa { ca }).unwrap();
        let mut expect = 5000;
        assert_eq!(total_grains(&w), expect);
        for _ in 0..300 {
            w.apply(Command::Step { gens: 1 }).unwrap();
            expect += 7;
            assert_eq!(total_grains(&w), expect, "{}", spec.family);
        }
        if spec.boundary == Boundary::Wrap {
            assert_eq!(w.fields[0].lost, 0);
        }
    }
}

/// Reference stabilisation: topple one cell at a time, in an order chosen by a seeded shuffle.
fn stabilise_sequential(t: &Topology, nb: Nb, h: &mut [u32], seed: u64) -> u64 {
    let mut rng = jetcarp_core::rng::Rng::new(seed);
    let mut lost = 0u64;
    let explicit = matches!(t.storage, Storage::Explicit(_));
    let sink = |i: usize| explicit && t.is_open(i);
    for (i, hi) in h.iter_mut().enumerate() {
        if sink(i) {
            lost += *hi as u64;
            *hi = 0;
        }
    }
    loop {
        let mut unstable: Vec<usize> = (0..h.len())
            .filter(|&i| !sink(i) && h[i] as usize >= t.degree(i, nb))
            .collect();
        if unstable.is_empty() {
            return lost;
        }
        for k in (1..unstable.len()).rev() {
            unstable.swap(k, rng.below(k as u64 + 1) as usize);
        }
        for i in unstable {
            let d = t.degree(i, nb);
            if (h[i] as usize) < d {
                continue;
            }
            h[i] -= d as u32;
            let mut present = 0;
            t.for_each_neighbor(i, nb, |j| {
                present += 1;
                if sink(j) { lost += 1 } else { h[j] += 1 }
            });
            lost += (d - present) as u64; // slots past a dead edge
        }
    }
}

// 5b. Toppling order doesn't change the final state.
#[test]
fn sandpile_order_independent() {
    for (spec, nb) in [
        (square(30, 30, Boundary::Dead), Nb::Edge),
        (square(30, 30, Boundary::Dead), Nb::Touch),
        (
            TilingSpec::new("cairo").with_size(SizeSpec::Cells(600)),
            Nb::Edge,
        ),
        (
            TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
            Nb::Edge,
        ),
    ] {
        let mut w = World::new(spec.clone(), 11).unwrap();
        let mut ca = CaSpec::new("Sandpile")
            .seed(SeedSpec::Random { density: 1.0 })
            .nb(nb);
        ca.steps_per_gen = 1;
        w.apply(Command::AddCa { ca }).unwrap();
        // Add a big pile on top of the random stable configuration.
        let c = w.topo.center_cell() as u32;
        w.apply(Command::SetCells {
            id: 1,
            cells: vec![[c, 3000], [c / 2, 999]],
        })
        .unwrap();
        let start = w.fields[0].height.clone();
        // Engine: synchronous multi-toppling until stable.
        let mut guard = 0;
        loop {
            let before = w.fields[0].height.clone();
            w.apply(Command::Step { gens: 1 }).unwrap();
            if w.fields[0].height == before {
                break;
            }
            guard += 1;
            assert!(guard < 100_000);
        }
        for seed in [1, 2, 3] {
            let mut h = start.clone();
            let lost = stabilise_sequential(&w.topo, nb, &mut h, seed);
            assert_eq!(h, w.fields[0].height, "{} {nb:?} order {seed}", spec.family);
            assert_eq!(lost, w.fields[0].lost, "{} lost grains", spec.family);
        }
    }
}

fn busy_scene() -> Scene {
    let mut s = Scene::new(square(120, 90, Boundary::Wrap));
    s.rng_state = 42;
    s.cas = vec![
        CaSpec::new("W30"),
        CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 }),
        CaSpec::new("Ant RL")
            .seed(SeedSpec::Ants { count: 5 })
            .steps(100),
        CaSpec::new("B2/S/C3").seed(SeedSpec::Random { density: 0.2 }),
        CaSpec::new("Sandpile")
            .seed(SeedSpec::Pile { grains: 20000 })
            .steps(5),
    ];
    s
}

// 6. The same scene gives the same hash on 1 thread and on all threads.
#[test]
fn same_hash_any_thread_count() {
    let run = |threads: usize, scene: &Scene| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut w = World::from_scene(scene).unwrap();
            w.apply(Command::Step { gens: 60 }).unwrap();
            w.state_hash()
        })
    };
    let all = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(2);
    let mut scenes = vec![busy_scene()];
    let mut sp = Scene::new(TilingSpec::new("spectre").with_size(SizeSpec::Level(4)));
    sp.cas = vec![
        CaSpec::new("B2/S34").seed(SeedSpec::Random { density: 0.3 }),
        CaSpec::new("Sandpile"),
    ];
    scenes.push(sp);
    for s in &scenes {
        let a = run(1, s);
        let b = run(all, s);
        assert_eq!(a, b, "1 thread vs {all} threads");
    }
    // And a saved-and-reloaded scene replays to the same hash.
    let mut w = World::from_scene(&busy_scene()).unwrap();
    w.apply(Command::Step { gens: 25 }).unwrap();
    w.apply(Command::SetCells {
        id: 2,
        cells: vec![[100, 1], [101, 1], [102, 1]],
    })
    .unwrap();
    w.apply(Command::Step { gens: 10 }).unwrap();
    let saved = w.to_scene();
    let json = saved.to_json();
    let w2 = World::from_scene(&Scene::from_json(&json).unwrap()).unwrap();
    assert_eq!(w.state_hash(), w2.state_hash());
    assert_eq!(saved.record.as_ref().unwrap().hash, w2.hash_hex());
}

// 7. The Spectre glider rule produces a traveling pattern.
#[test]
fn spectre_glider_travels() {
    let t = std::sync::Arc::new(
        Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(5))).unwrap(),
    );
    let n = t.len();
    let cs: Vec<_> = (0..n).map(|i| t.centroid(i)).collect();
    let c0 = t.centroid(t.center_cell());
    let mut near: Vec<usize> = (0..n).filter(|&i| t.is_interior(i)).collect();
    near.sort_by(|&a, &b| {
        geom::dist(cs[a], c0)
            .partial_cmp(&geom::dist(cs[b], c0))
            .unwrap()
    });
    let mut travelled = 0;
    let mut tested = 0;
    'outer: for &a in near.iter().take(12) {
        for k in 0..t.degree(a, Nb::Edge) {
            let b = t.slot(a, Nb::Edge, k).unwrap();
            tested += 1;
            let mut w =
                World::new(TilingSpec::new("spectre").with_size(SizeSpec::Level(5)), 1).unwrap();
            w.apply(Command::AddCa {
                ca: CaSpec::new("SpectreGlider").seed(SeedSpec::Cells {
                    cells: vec![[a as u32, 1], [b as u32, 3]],
                }),
            })
            .unwrap();
            let (mut max_d, mut peak, mut pop) = (0.0f64, 0, 0);
            for _ in 0..400 {
                w.step();
                let f = &w.fields[0];
                pop = 0;
                let mut at_edge = false;
                for i in 0..n {
                    if f.state[i] != 0 {
                        pop += 1;
                        max_d = max_d.max(geom::dist(cs[i], cs[a]));
                        at_edge |= t.is_open(i);
                    }
                }
                peak = peak.max(pop);
                if pop == 0 || at_edge {
                    break;
                }
            }
            if pop > 0 && peak <= 40 && max_d > 150.0 {
                travelled += 1;
                break 'outer;
            }
        }
    }
    assert!(travelled > 0, "no glider travelled in {tested} seeds");
}

// Extra: every prototype tiling builds with the expected neighbour counts.
#[test]
fn periodic_tiling_degrees() {
    let expect: &[(&str, &[usize], &[usize])] = &[
        ("lizard", &[6], &[6]),
        ("hex", &[6], &[6]),
        ("brick", &[6], &[6]),
        ("square", &[4], &[8]),
        ("quad", &[4], &[8]),
        ("triangle", &[3], &[12]),
        ("glide", &[4], &[8]),
        ("fourfold", &[4], &[8]),
        ("cairo", &[5], &[7]),
        ("oct", &[4, 8], &[4, 8]),
        ("trihex", &[3, 6], &[6, 12]),
        ("herring", &[6], &[6]),
        ("pyth", &[4, 8], &[4, 8]),
    ];
    for &(id, e, t) in expect {
        for b in [Boundary::Dead, Boundary::Wrap] {
            let top = Topology::build(&TilingSpec::new(id).with_boundary(b)).unwrap();
            let (de, dt) = top.degree_sets();
            assert_eq!((de.as_slice(), dt.as_slice()), (e, t), "{id} {b:?}");
            for i in 0..top.len() {
                top.for_each_neighbor(i, Nb::Touch, |j| {
                    let mut back = false;
                    top.for_each_neighbor(j, Nb::Touch, |k| back |= k == i);
                    assert!(back, "{id} {b:?}: asymmetric {i} {j}");
                });
            }
        }
    }
}

// Extra: prototype tile JSON loads unchanged.
#[test]
fn prototype_tile_json_loads() {
    let json = r#"{
 "family": "cairo",
 "symmetry": "p4",
 "params": {"v0": [0, 0], "v1": [1, 0], "v2": [1.25, 1.1]},
 "edges": [
  {"mode": "S", "curved": true, "nodes": [{"t": 0.3, "d": 0.12}, {"t": 0.7, "d": -0.12}]},
  {"mode": "J", "curved": true, "nodes": [{"t": 0.4, "d": 0.2}]},
  {"mode": "I", "curved": false, "nodes": []}
 ]
}"#;
    let spec: TilingSpec = serde_json::from_str(json).unwrap();
    let t = Topology::build(&spec).unwrap();
    assert_eq!(t.degree_sets(), (vec![5], vec![7]));
    assert!(t.outlines[0].len() > 5, "curved outline is flattened");
    let mut w = World::new(spec, 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B2/S34"),
    })
    .unwrap();
    w.apply(Command::Step { gens: 10 }).unwrap();
}

// Extra: exact period detection.
#[test]
fn blinker_period_two() {
    let mut w = World::new(square(16, 16, Boundary::Dead), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let cells = (0..3).map(|x| [g.index(6 + x, 7, 0) as u32, 1]).collect();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 10 }).unwrap();
    assert!(
        matches!(w.status(), Status::Repeats { period: 2, .. }),
        "{}",
        w.status()
    );
}

// Extra: shared vs private fields and the Affects-others toggle.
#[test]
fn fields_shared_and_private() {
    let mut w = World::new(square(20, 20, Boundary::Wrap), 1).unwrap();
    let life = w
        .apply(Command::AddCa {
            ca: CaSpec::new("B3/S23"),
        })
        .unwrap()
        .new_id
        .unwrap();
    let ant = w
        .apply(Command::AddCa {
            ca: CaSpec::new("Ant RL"),
        })
        .unwrap()
        .new_id
        .unwrap();
    let bb = w
        .apply(Command::AddCa {
            ca: CaSpec::new("B2/S/C3"),
        })
        .unwrap()
        .new_id
        .unwrap();
    let sp = w
        .apply(Command::AddCa {
            ca: CaSpec::new("Sandpile"),
        })
        .unwrap()
        .new_id
        .unwrap();
    assert_eq!(
        w.ca(life).unwrap().field,
        w.ca(ant).unwrap().field,
        "binary CAs share a field"
    );
    assert_ne!(w.ca(bb).unwrap().field, w.ca(life).unwrap().field);
    assert_ne!(w.ca(sp).unwrap().field, w.ca(life).unwrap().field);
    assert_eq!(w.fields.len(), 3);
    w.apply(Command::UpdateCa {
        id: ant,
        change: CaChange {
            affects_others: Some(false),
            ..Default::default()
        },
    })
    .unwrap();
    assert_ne!(
        w.ca(ant).unwrap().field,
        w.ca(life).unwrap().field,
        "private ant field"
    );
    assert_eq!(w.fields.len(), 4);
    w.apply(Command::UpdateCa {
        id: ant,
        change: CaChange {
            affects_others: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(w.ca(ant).unwrap().field, w.ca(life).unwrap().field);
    assert_eq!(w.fields.len(), 3);
    w.apply(Command::RemoveCa { id: bb }).unwrap();
    assert_eq!(w.fields.len(), 2);
    assert_eq!(
        w.ca(sp).unwrap().rule.field_kind(),
        w.fields[w.ca(sp).unwrap().field].kind
    );
}

// Extra: every bundled scene loads, runs, renders, and replays to the same hash after saving.
#[test]
fn bundled_scenes_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/scenes");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut w = World::from_scene(&scene).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        w.apply(Command::Step { gens: 5 }).unwrap();
        let img = render::render(
            &w,
            &render::RenderOptions {
                max_side: 200,
                ..Default::default()
            },
        );
        assert!(img.width > 0 && img.height > 0);
        let again = World::from_scene(&w.to_scene()).unwrap();
        assert_eq!(again.state_hash(), w.state_hash(), "{}", path.display());
        count += 1;
    }
    assert!(count >= 10, "found {count} scenes");
}

// Extra: manual test 1 through commands only. Life added onto Rule 30's field starts from its cells.
#[test]
fn rule30_then_life_then_ants() {
    let mut w = World::new(square(125, 100, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("W30"),
    })
    .unwrap();
    w.apply(Command::Step { gens: 20 }).unwrap();
    let before = w.fields[0].population();
    let life = w
        .apply(Command::AddCa {
            ca: CaSpec::new("B3/S23"),
        })
        .unwrap()
        .new_id
        .unwrap();
    assert_eq!(w.fields[0].population(), before, "no random cells added");
    assert_eq!(w.ca(life).unwrap().seed, SeedSpec::Empty);
    let ant = w
        .apply(Command::AddCa {
            ca: CaSpec::new("Ant RL"),
        })
        .unwrap()
        .new_id
        .unwrap();
    w.apply(Command::UpdateCa {
        id: ant,
        change: CaChange {
            steps_per_gen: Some(100),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(w.fields.len(), 1, "all three share one binary field");
    w.apply(Command::Step { gens: 30 }).unwrap();
    let again = World::from_scene(&w.to_scene()).unwrap();
    assert_eq!(again.state_hash(), w.state_hash());
}

// A picture JETCARP writes carries its scene, which reads back and replays to the same state.
#[test]
fn a_png_carries_its_scene() {
    let mut w = World::new(square(40, 30, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23"),
    })
    .unwrap();
    w.apply(Command::Step { gens: 12 }).unwrap();
    let img = render::render(
        &w,
        &render::RenderOptions {
            max_side: 120,
            ..Default::default()
        },
    );
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("png-scene");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("life.png");
    render::write_png(&path, &img, &render::png_metadata(&w)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let json = render::png_scene(&bytes).unwrap().expect("a scene chunk");
    let again = World::from_scene(&Scene::from_json(&json).unwrap()).unwrap();
    assert_eq!(again.state_hash(), w.state_hash());
    // A PNG without one says so; bytes that aren't a PNG are an error.
    render::write_png(&path, &img, &[]).unwrap();
    assert_eq!(
        render::png_scene(&std::fs::read(&path).unwrap()).unwrap(),
        None
    );
    assert!(render::png_scene(b"not a png").is_err());
}
