//! Phase 2: hat and Penrose (P2 kite and dart, P3 rhomb) patches.

use jetcarp_core::geom::*;
use jetcarp_core::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const FAMILIES: [&str; 3] = ["hat", "penrose-kd", "penrose-rhomb"];
use jetcarp_core::penrose::PHI;

fn build(id: &str, level: u32) -> Topology {
    Topology::build(&TilingSpec::new(id).with_size(SizeSpec::Level(level))).unwrap()
}

/// Largest distance from the patch centre (the origin) to a tile's vertex average.
fn radius(t: &Topology) -> f64 {
    (0..t.len()).map(|i| len(t.centroid(i))).fold(0.0, f64::max)
}

/// Largest tile diameter.
fn diameter(t: &Topology) -> f64 {
    (0..t.len())
        .map(|i| {
            let b = bbox(&t.polygon(i));
            (b[2] - b[0]).hypot(b[3] - b[1])
        })
        .fold(0.0, f64::max)
}

// Tile counts match the size classes, and the UI levels list them.
#[test]
fn tile_counts() {
    for id in FAMILIES {
        assert_eq!(
            aperiodic::levels(id),
            &[
                (3, 500),
                (4, 4000),
                (5, 35000),
                (6, 280_000),
                (7, 2_240_000),
                (8, 17_920_000)
            ],
            "{id}"
        );
        assert_eq!(aperiodic::default_level(id), 3);
        for level in 1..=5 {
            let t = build(id, level);
            assert_eq!(
                Some(t.len()),
                aperiodic::tile_count(id, level),
                "{id} {level}"
            );
        }
        // Level 6 without the neighbour graph; levels 7 and 8 are built only exactly
        // (tests/exact_patches.rs), and there is no level 9.
        let p = aperiodic::patch(id, 6).unwrap();
        assert_eq!(p.cells.len(), 280_000, "{id} level 6");
        assert!(aperiodic::patch(id, 7).is_err());
        assert!(aperiodic::build(id, 9).is_err());
    }
}

// Every level is cut from the same tiling around the same centre, so smaller patches lie inside
// larger ones tile for tile (patterns near the centre behave the same at every size).
#[test]
fn levels_nest() {
    for id in FAMILIES {
        let key = |p: &[P]| {
            let c = centroid(p);
            ((c[0] * 1e3).round() as i64, (c[1] * 1e3).round() as i64)
        };
        let mut prev: Option<aperiodic::Patch> = None;
        for level in 1..=5 {
            let p = aperiodic::patch(id, level).unwrap();
            if let Some(small) = &prev {
                let big: HashMap<_, Vec<P>> =
                    p.polygons().into_iter().map(|q| (key(&q), q)).collect();
                for q in small.polygons() {
                    let r = big.get(&key(&q));
                    assert!(
                        r.is_some_and(|r| r.iter().zip(&q).all(|(a, b)| dist(*a, *b) < 1e-6)),
                        "{id}: level {} tile missing from level {level}",
                        level - 1
                    );
                }
            }
            prev = Some(p);
        }
    }
}

// Neighbour lists are symmetric, edge neighbours are touching neighbours, and nothing repeats.
#[test]
fn neighbours_symmetric() {
    for id in FAMILIES {
        let t = build(id, 4);
        for i in 0..t.len() {
            let mut touch = BTreeSet::new();
            t.for_each_neighbor(i, Nb::Touch, |j| assert!(touch.insert(j), "{id}: repeat"));
            let mut edge = BTreeSet::new();
            t.for_each_neighbor(i, Nb::Edge, |j| assert!(edge.insert(j), "{id}: repeat"));
            assert!(edge.is_subset(&touch), "{id}: edge neighbour not touching");
            assert!(!touch.contains(&i));
            for nb in [Nb::Edge, Nb::Touch] {
                t.for_each_neighbor(i, nb, |j| {
                    let mut back = false;
                    t.for_each_neighbor(j, nb, |k| back |= k == i);
                    assert!(back, "{id}: asymmetric {nb:?} neighbours {i} {j}");
                });
            }
        }
    }
}

// No two tiles overlap, and the patch has no holes: every sample point lies in at most one tile,
// and every point well inside the patch lies in exactly one.
#[test]
fn no_overlaps_or_holes() {
    for id in FAMILIES {
        let t = build(id, 4);
        let polys: Vec<Vec<P>> = (0..t.len()).map(|i| t.polygon(i)).collect();
        let total: f64 = polys.iter().map(|p| area(p)).sum();
        let (r, d) = (radius(&t), diameter(&t));
        // Bucket tiles by bounding box on a grid of cell size d.
        let cell = d;
        let k = |x: f64| ((x + r + d) / cell).floor() as i64;
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, p) in polys.iter().enumerate() {
            let b = bbox(p);
            for gx in k(b[0])..=k(b[2]) {
                for gy in k(b[1])..=k(b[3]) {
                    grid.entry((gx, gy)).or_default().push(i);
                }
            }
        }
        // Sample on a fine grid with an irrational offset, so no sample lands on an edge.
        let step = d / 23.0;
        let (mut inside, mut samples) = (0usize, 0usize);
        let mut y = -r - d + 0.012_345_67;
        while y < r + d {
            let mut x = -r - d + 0.009_876_54;
            while x < r + d {
                let c = grid
                    .get(&(k(x), k(y)))
                    .map(|v| {
                        v.iter()
                            .filter(|&&i| point_in_poly([x, y], &polys[i]))
                            .count()
                    })
                    .unwrap_or(0);
                assert!(c <= 1, "{id}: {c} tiles overlap at ({x:.3}, {y:.3})");
                if len([x, y]) < r - 2.0 * d {
                    assert_eq!(c, 1, "{id}: hole at ({x:.3}, {y:.3})");
                }
                inside += c;
                samples += 1;
                x += step;
            }
            y += step;
        }
        // The sampled area matches the tiles' total area.
        let sampled = inside as f64 * step * step;
        assert!(
            (sampled / total - 1.0).abs() < 0.01,
            "{id}: sampled {sampled:.1} vs {total:.1} from {samples} samples"
        );
    }
}

// Tiles with part of their boundary unshared lie only on the rim of the patch.
#[test]
fn open_tiles_only_on_rim() {
    for id in FAMILIES {
        for level in [3, 4] {
            let t = build(id, level);
            let (r, d) = (radius(&t), diameter(&t));
            let mut open = 0;
            for i in 0..t.len() {
                if t.is_open(i) {
                    open += 1;
                    assert!(
                        len(t.centroid(i)) > r - 2.0 * d,
                        "{id} level {level}: tile {i} open inside the patch"
                    );
                }
            }
            assert!(open > 0 && open < t.len() / 3, "{id}: {open} open tiles");
        }
    }
}

// Interior degree sets. Penrose tilings are edge to edge, so every interior rhomb, kite, and dart
// has four edge neighbours. Touching (vertex) degrees match Edmond's (monotile-ca GLIDERS.md):
// hat 6-7, P2 8-10, P3 7-11. Hats share some boundary with 4 to 6 others (counting contacts
// where a corner meets the middle of a long edge).
#[test]
fn degree_sets() {
    let want: [(&str, &[usize], &[usize]); 3] = [
        ("hat", &[4, 5, 6], &[6, 7]),
        ("penrose-kd", &[4], &[8, 9, 10]),
        ("penrose-rhomb", &[4], &[7, 8, 9, 10, 11]),
    ];
    for (id, edge, touch) in want {
        for level in [4, 5] {
            let (e, t) = build(id, level).degree_sets();
            assert_eq!(e, edge, "{id} level {level} edge degrees");
            assert_eq!(t, touch, "{id} level {level} touch degrees");
        }
    }
}

/// Distinct vertex stars well inside the patch: the cyclic sequence of (shape, corner) labels around
/// each vertex, up to rotation and reflection. `corner` maps a shape's corner index to a label.
fn vertex_stars(t: &Topology, corner: impl Fn(usize, usize) -> u8) -> BTreeSet<Vec<(usize, u8)>> {
    let r = radius(t) - 3.0 * diameter(t);
    let q = |p: P| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64);
    type Corners = Vec<(f64, (usize, u8))>;
    let mut at: HashMap<(i64, i64), (P, Corners)> = HashMap::new();
    for i in 0..t.len() {
        let poly = t.polygon(i);
        let c = centroid(&poly);
        for (k, &v) in poly.iter().enumerate() {
            let e = at.entry(q(v)).or_insert((v, vec![]));
            e.1.push((
                angle_of(sub(c, v)),
                (t.shape_of(i), corner(t.shape_of(i), k)),
            ));
        }
    }
    let mut out = BTreeSet::new();
    for (v, mut around) in at.into_values() {
        if len(v) > r {
            continue;
        }
        around.sort_by(|a, b| a.0.total_cmp(&b.0));
        let seq: Vec<(usize, u8)> = around.into_iter().map(|x| x.1).collect();
        let n = seq.len();
        let mut best: Option<Vec<(usize, u8)>> = None;
        for rev in [false, true] {
            for s in 0..n {
                let v: Vec<_> = (0..n)
                    .map(|j| {
                        let k = if rev { (s + n - j) % n } else { (s + j) % n };
                        seq[k]
                    })
                    .collect();
                if best.as_ref().is_none_or(|b| v < *b) {
                    best = Some(v);
                }
            }
        }
        out.insert(best.unwrap());
    }
    out
}

// Penrose vertex stars: P2 has exactly seven (sun, star, ace, deuce, jack, queen, king) and P3
// exactly eight (de Bruijn 1981; Grünbaum and Shephard, Tilings and Patterns, §10.5). Two of the
// P3 eight are five thick rhombs meeting at their 72° corners, told apart only by the matching
// marks, so by angles alone P3 shows seven.
#[test]
fn penrose_vertex_stars() {
    // Corners 0 and 2 lie on each shape's axis; 1 and 3 are mirror images.
    // Kites and darts: corners 0 and 2 lie on the axis, 1 and 3 are mirror images.
    let p2 = vertex_stars(
        &build("penrose-kd", 5),
        |_, k| if k == 3 { 1 } else { k as u8 },
    );
    assert_eq!(p2.len(), 7, "P2 vertex stars: {p2:?}");
    // Rhombs: label corners by angle only, then by index. Corner 0 is the apex of the half-tile
    // with positive orientation, so the index carries the Robinson-triangle marks.
    let p3 = build("penrose-rhomb", 5);
    let by_angle = vertex_stars(&p3, |_, k| (k % 2) as u8);
    assert_eq!(by_angle.len(), 7, "P3 vertex stars by angle: {by_angle:?}");
    let marked = vertex_stars(&p3, |_, k| k as u8);
    assert_eq!(marked.len(), 8, "P3 vertex stars with marks: {marked:?}");
    assert!(marked.contains(&vec![(0, 1); 5]) && marked.contains(&vec![(0, 3); 5]));
    // The P2 sun: five kite heads.
    assert!(p2.contains(&vec![(0, 0); 5]));
    // The P2 star: five dart noses.
    assert!(p2.contains(&vec![(1, 0); 5]));
}

fn shape_ratio(t: &Topology) -> f64 {
    let mut n = [0usize; 2];
    for i in 0..t.len() {
        n[t.shape_of(i)] += 1;
    }
    n[0] as f64 / n[1] as f64
}

// Kites outnumber darts, and thick rhombs thin ones, by the golden ratio.
#[test]
fn penrose_ratios_approach_phi() {
    for id in ["penrose-kd", "penrose-rhomb"] {
        let errs: Vec<f64> = [3, 4, 5]
            .iter()
            .map(|&l| (shape_ratio(&build(id, l)) - PHI).abs())
            .collect();
        assert!(errs[2] < 0.01, "{id}: ratio errors {errs:?}");
        assert!(errs[2] < errs[0], "{id}: ratio errors {errs:?}");
    }
}

// Unreflected hats outnumber reflected ones by φ⁴ ≈ 6.85 (Smith, Myers, Kaplan, Goodman-Strauss
// 2023, §6).
#[test]
fn hat_reflection_ratio() {
    let p = aperiodic::patch("hat", 5).unwrap();
    let mut n: BTreeMap<bool, usize> = BTreeMap::new();
    for (_, m) in &p.cells {
        *n.entry(m.det() < 0.0).or_default() += 1;
    }
    let r = n[&false] as f64 / n[&true] as f64;
    assert!((r - PHI.powi(4)).abs() < 0.15, "hat:antihat = {r}");
}

// Every tile is an exact copy of its prototile (rotation, translation, maybe reflection).
#[test]
fn tiles_are_congruent() {
    for id in FAMILIES {
        let p = aperiodic::patch(id, 4).unwrap();
        for (s, m) in &p.cells {
            let l = m.linear();
            let det = m.det();
            assert!((det.abs() - 1.0).abs() < 1e-9, "{id}: det {det}");
            assert!(
                (l[0] * l[0] + l[2] * l[2] - 1.0).abs() < 1e-9,
                "{id}: not a similarity"
            );
            assert!((l[0] * l[1] + l[2] * l[3]).abs() < 1e-9, "{id}: shear");
            assert!((*s as usize) < p.shapes.len());
        }
    }
}

// The editor preview and the bundled scenes build.
#[test]
fn preview_and_scenes() {
    for id in FAMILIES {
        let t = build(id, aperiodic::preview_level(id));
        assert_eq!(t.len(), 70, "{id} preview");
        let ghosts = topology::ghost_transforms(&TilingSpec::new(id), 1).unwrap();
        assert!(ghosts.len() >= 6, "{id}: {} ghosts", ghosts.len());
    }
    for f in ["hat-b2s34", "penrose-kd-sandpile", "penrose-rhomb-brain"] {
        let path = format!("{}/data/scenes/{f}.json", env!("CARGO_MANIFEST_DIR"));
        let s = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut w = World::from_scene(&s).unwrap();
        w.apply(Command::Step { gens: 10 }).unwrap();
    }
}
