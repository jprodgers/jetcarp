//! Exact geometry for large aperiodic patches: the exact constructions agree with the float ones
//! at the validated levels, and each patch is verified exactly to be a tiling of a disc.

use jetcarp_core::exact::{self, Cyclo, Place, Z12};
use jetcarp_core::geom::*;
use jetcarp_core::spectre;

fn places(level: u32) -> Vec<Place> {
    spectre::spectre_exact(level)
        .iter()
        .map(|i| i.compact().unwrap())
        .collect()
}

// The exact Spectre construction places every tile where the float one does, numbered alike,
// and the exact neighbour graph equals the float graph list for list (levels 1 to 5; level 6 in
// the release-only test below).
#[test]
fn spectre_exact_equals_float() {
    for level in 1..=5 {
        check_spectre_level(level);
    }
}

fn check_spectre_level(level: u32) {
    let t0 = std::time::Instant::now();
    let fl = spectre::spectre_transforms(level);
    let ex = spectre::spectre_exact(level);
    assert_eq!(fl.len(), ex.len(), "level {level}");
    assert_eq!(ex.len() as u128, spectre::tile_count(level));
    for (a, b) in fl.iter().zip(&ex) {
        let m = b.to_affine::<Z12>();
        for k in 0..6 {
            assert!(
                (a.0[k] - m.0[k]).abs() < 1e-6,
                "level {level}: {a:?} vs {m:?}"
            );
        }
    }
    let g = spectre::spectre_graph(&fl);
    let pl: Vec<Place> = ex.iter().map(|i| i.compact().unwrap()).collect();
    let x = spectre::spectre_graph_exact(&pl).unwrap();
    assert_eq!(g.open, x.open, "level {level}: open");
    for (name, lists, csr) in [("touch", &g.touch, &x.touch), ("edge", &g.edge, &x.edge)] {
        for (t, l) in lists.iter().enumerate() {
            let row = &csr.1[csr.0[t] as usize..csr.0[t + 1] as usize];
            assert_eq!(&l[..], row, "level {level}: {name} neighbours of tile {t}");
        }
    }
    eprintln!(
        "spectre level {level}: {} tiles equal in {:?}",
        ex.len(),
        t0.elapsed()
    );
}

#[test]
#[ignore = "slow in debug builds; run with --release --ignored"]
fn spectre_exact_equals_float_level6() {
    check_spectre_level(6);
}

// Tile counts follow the substitution matrix; levels embed exactly (tile i of level L is tile
// offset + i of level L + 1, placed by the exact level embedding).
#[test]
fn spectre_counts_and_embeddings() {
    let want = [9u128, 71, 559, 4401, 34649, 272791];
    for (l, w) in (1..=6).zip(want) {
        assert_eq!(spectre::tile_count(l), w);
    }
    // The count grows by 4 + √15 per level (the Spectre's inflation squared).
    let r = spectre::tile_count(12) as f64 / spectre::tile_count(11) as f64;
    assert!((r - (4.0 + 15f64.sqrt())).abs() < 1e-6, "{r}");
    for level in 1..=4 {
        let small = spectre::spectre_exact(level);
        let big = spectre::spectre_exact(level + 1);
        let off = spectre::level_offset(level);
        let m = spectre::level_embedding_exact(level);
        for (i, s) in small.iter().enumerate() {
            assert_eq!(m.mul::<Z12>(s), big[off + i], "level {level} tile {i}");
        }
        // And it agrees with the float embedding.
        let f = spectre::level_embedding(level);
        let e = m.to_affine::<Z12>();
        for k in 0..6 {
            assert!((f.0[k] - e.0[k]).abs() < 1e-9);
        }
    }
}

// Each patch is exactly a tiling of a disc: edges matched exactly, corner angles at interior
// vertices add up to a full turn, one simple boundary loop, V - E + F = 1, and the area inside
// the boundary equals the summed tile areas (n copies of the prototile).
#[test]
fn spectre_patches_are_discs() {
    for level in 1..=5 {
        let pl = places(level);
        let st = spectre_stats(&pl);
        assert_eq!(st.euler, 1);
        assert_eq!(st.tiles, pl.len());
        // Every tile has the prototile's area.
        let proto: Vec<_> = spectre::spectre_exact_corners().to_vec();
        let mut a = [0i64; 4];
        for k in 0..14 {
            let x = Z12::mul(Z12::conj(proto[k]), proto[(k + 1) % 14]);
            a = exact::zadd(a, x);
        }
        let mut one = Z12::im_parts(a);
        if exact::sign_surd(one.0, one.1, 3) == std::cmp::Ordering::Less {
            one = (-one.0, -one.1);
        }
        let n = pl.len() as i128;
        assert_eq!(st.tile_area2, (one.0 * n, one.1 * n), "level {level}");
        eprintln!("level {level}: {st:?}");
    }
}

fn spectre_stats(pl: &[Place]) -> exact::PatchStats {
    spectre::verify_exact(pl).unwrap()
}

#[test]
fn spectre_angles_exact() {
    // The prototile's float corners are the exact ones.
    let c = spectre::spectre_exact_corners();
    for (k, z) in c.iter().enumerate() {
        assert!(dist(Z12::to_f64(*z), spectre::SPECTRE[k]) < 1e-12);
    }
}

// The topology built from the exact patch has the float build's centre tile, open tiles, and
// neighbour lists (so scenes seeded at the centre replay identically).
#[test]
fn spectre_topology_matches_float_build() {
    use jetcarp_core::*;
    for level in 1..=5 {
        let t =
            Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(level))).unwrap();
        let fl = spectre::spectre_transforms(level);
        let g = spectre::spectre_graph(&fl);
        let cs: Vec<P> = g.polys.iter().map(|p| centroid(p)).collect();
        let c0 = centroid(&cs);
        let center = (0..cs.len())
            .min_by(|&a, &b| dist(cs[a], c0).partial_cmp(&dist(cs[b], c0)).unwrap())
            .unwrap();
        assert_eq!(t.center_cell(), center, "level {level}");
        assert_eq!(t.len(), fl.len());
        for i in 0..t.len() {
            assert_eq!(t.is_open(i), g.open[i]);
            let mut row = Vec::new();
            t.for_each_neighbor(i, Nb::Touch, |j| row.push(j as u32));
            assert_eq!(row, g.touch[i]);
            let p = t.polygon(i);
            for (a, b) in p.iter().zip(&g.polys[i]) {
                assert!(dist(*a, *b) < 1e-9);
            }
        }
    }
}

/// Exact size-class patch vs the float one at the same size: (tiles only in the exact one,
/// tiles compared, neighbour lists differing as sets, differing only in order), over tiles in
/// both whose touching neighbours are all in both.
fn compare_size_class(id: &str, level: u32) -> (usize, usize, usize, usize) {
    use jetcarp_core::aperiodic;
    use std::collections::HashMap;
    let fl = aperiodic::build(id, level).unwrap();
    let n = aperiodic::SIZE_CLASSES[level as usize - 1];
    let ex = jetcarp_core::sizeclass::build(id, n).unwrap();
    assert_eq!(ex.len(), n);
    let key = |p: &aperiodic::BuiltPatch, i: usize| {
        let m = p.placements.get(i);
        let c = centroid(
            &p.shapes[p.shape[i] as usize]
                .iter()
                .map(|&q| m.apply(q))
                .collect::<Vec<_>>(),
        );
        (
            p.shape[i],
            (c[0] * 1e5).round() as i64,
            (c[1] * 1e5).round() as i64,
        )
    };
    let fmap: HashMap<_, usize> = (0..fl.len()).map(|i| (key(&fl, i), i)).collect();
    let to_f: Vec<Option<usize>> = (0..ex.len())
        .map(|i| fmap.get(&key(&ex, i)).copied())
        .collect();
    // The same tile has the same transform.
    for (i, f) in to_f.iter().enumerate() {
        if let Some(f) = f {
            let (a, b) = (ex.placements.get(i), fl.placements.get(*f));
            for k in 0..6 {
                assert!((a.0[k] - b.0[k]).abs() < 1e-6, "{id}: tile {i} transform");
            }
        }
    }
    let missing = to_f.iter().filter(|x| x.is_none()).count();
    let mut in_ex = vec![false; fl.len()];
    for f in to_f.iter().flatten() {
        in_ex[*f] = true;
    }
    let row = |c: &jetcarp_core::topology::Csr, t: usize| {
        c.nb[c.off[t] as usize..c.off[t + 1] as usize].to_vec()
    };
    let (mut checked, mut sets, mut orders) = (0, 0, 0);
    for i in 0..ex.len() {
        let Some(f) = to_f[i] else { continue };
        if row(&ex.touch, i)
            .iter()
            .any(|&j| to_f[j as usize].is_none())
            || row(&fl.touch, f).iter().any(|&j| !in_ex[j as usize])
        {
            continue;
        }
        checked += 1;
        assert_eq!(ex.open[i], fl.open[f], "{id}: open {i}");
        for (ec, fc) in [(&ex.touch, &fl.touch), (&ex.edge, &fl.edge)] {
            let mapped: Vec<u32> = row(ec, i)
                .iter()
                .map(|&j| to_f[j as usize].unwrap() as u32)
                .collect();
            let want = row(fc, f);
            if mapped != want {
                let (mut a, mut b) = (mapped.clone(), want.clone());
                a.sort_unstable();
                b.sort_unstable();
                if a != b {
                    sets += 1;
                } else {
                    orders += 1;
                    // Only tiles meeting the tile at one corner may swap: they tie on contact
                    // position (the float builder orders them by rounding noise). Edge
                    // neighbours keep their order, and so does every run of corner neighbours
                    // between two edge neighbours, as a set.
                    let edge_f = row(&fl.edge, f);
                    let runs = |l: &[u32]| -> Vec<Vec<u32>> {
                        let mut out = vec![vec![]];
                        for &x in l {
                            if edge_f.contains(&x) {
                                out.push(vec![x]);
                                out.push(vec![]);
                            } else {
                                out.last_mut().unwrap().push(x);
                            }
                        }
                        for r in &mut out {
                            r.sort_unstable();
                        }
                        out
                    };
                    assert_eq!(runs(&mapped), runs(&want), "{id}: tile {i} order");
                }
            }
        }
    }
    (missing, checked, sets, orders)
}

// The exact hat and Penrose size classes equal the float ones at the validated levels: the same
// tiles at the same places (up to ties in distance at the rim, which the float cut breaks by
// rounding), the same open tiles, the same neighbour sets; the hat's lists in the same order,
// Penrose lists up to the order of tiles meeting at one corner (tied in contact position).
#[test]
fn size_classes_exact_vs_float() {
    for level in 3..=5 {
        check_size_class_level(level);
    }
}

fn check_size_class_level(level: u32) {
    for id in ["hat", "penrose-kd", "penrose-rhomb"] {
        let n = jetcarp_core::aperiodic::SIZE_CLASSES[level as usize - 1];
        let (only, checked, sets, orders) = compare_size_class(id, level);
        eprintln!(
            "{id} level {level}: {only} rim tiles differ, {checked} compared, {orders} lists reorder ties"
        );
        // Tiles at the same distance come in groups of up to ten (the five-fold centre).
        assert!(only <= 10, "{id} level {level}: {only} tiles differ");
        assert!(checked * 10 > n * 9);
        assert_eq!(sets, 0, "{id} level {level}");
        if id == "hat" {
            assert_eq!(orders, 0, "{id} level {level}");
        }
    }
}

#[test]
#[ignore = "slow in debug builds; run with --release --ignored"]
fn size_classes_exact_vs_float_level6() {
    check_size_class_level(6);
}

// Level 7 of every aperiodic family (2.1 to 2.2 million tiles): exact tile counts, verified exactly
// as a disc tiling, the published interior degree sets, and nesting with level 6.
#[test]
fn level_7_patches() {
    use jetcarp_core::*;
    let pl = places(7);
    assert_eq!(pl.len(), 2_147_679);
    let st = spectre::verify_exact(&pl).unwrap();
    assert_eq!(st.euler, 1);
    let t = Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(7))).unwrap();
    assert_eq!(t.degree_sets(), (vec![4, 5, 6, 7], vec![6, 7]));
    let want: [(&str, &[usize], &[usize]); 3] = [
        ("hat", &[4, 5, 6], &[6, 7]),
        ("penrose-kd", &[4], &[8, 9, 10]),
        ("penrose-rhomb", &[4], &[7, 8, 9, 10, 11]),
    ];
    for (id, edge, touch) in want {
        let p = jetcarp_core::aperiodic::build(id, 7).unwrap();
        assert_eq!(p.len(), 2_240_000);
        let st = jetcarp_core::sizeclass::verify(&p, id).unwrap();
        assert_eq!(st.euler, 1, "{id}");
        eprintln!("{id} level 7: {st:?}");
        let t = Topology::build(&TilingSpec::new(id).with_size(SizeSpec::Level(7))).unwrap();
        let (e, tt) = t.degree_sets();
        assert_eq!(e, edge, "{id} edge degrees");
        assert_eq!(tt, touch, "{id} touch degrees");
        // Every level-6 tile is a level-7 tile at the same place.
        let six = Topology::build(&TilingSpec::new(id).with_size(SizeSpec::Level(6))).unwrap();
        let key = |t: &Topology, i: usize| {
            let c = t.centroid(i);
            (
                t.shape_of(i),
                (c[0] * 1e4).round() as i64,
                (c[1] * 1e4).round() as i64,
            )
        };
        let big: std::collections::HashSet<_> = (0..t.len()).map(|i| key(&t, i)).collect();
        assert!(
            (0..six.len()).all(|i| big.contains(&key(&six, i))),
            "{id}: levels nest"
        );
        // Shape ratios: kites to darts and thick to thin rhombs approach φ; hats to antihats φ⁴.
        let mut n = [0usize; 2];
        for i in 0..t.len() {
            let k = if id == "hat" {
                (t.transform(i).det() < 0.0) as usize
            } else {
                t.shape_of(i)
            };
            n[k] += 1;
        }
        let r = n[0] as f64 / n[1] as f64;
        let phi = jetcarp_core::penrose::PHI;
        let want = if id == "hat" { phi.powi(4) } else { phi };
        assert!((r - want).abs() < 0.02 * want, "{id}: ratio {r}");
    }
}
