//! The symmetry groups that canonical forms use, on every periodic tiling JETCARP builds: the cell
//! maps are closed under composition, the whole group (cell maps, or isometries on unit cells
//! that hold their tile pattern more than once) is every symmetry an independent scan finds, and
//! one pattern gets one id at every position and orientation those symmetries relate.

use jetcarp_core::geom::{P, centroid};
use jetcarp_core::objects::{ObjClass, ObjectOptions, Phase, find_objects};
use jetcarp_core::pattern::canon::tiling_symmetries;
use jetcarp_core::pattern::group::{Iso, tiling_group};
use jetcarp_core::pattern::symmetry::{CellMap, compose};
use jetcarp_core::pattern::{
    self, DiscoveredBy, PatternClass, PatternRecord, Provenance, encode_cells, record_from_object,
};
use jetcarp_core::*;

/// Every periodic tiling JETCARP builds: the catalog (Archimedean and k-uniform), the other
/// families at their default shapes, and the isohedral types.
fn periodic() -> Vec<TilingSpec> {
    let mut v = vec![];
    for t in jetcarp_core::catalog::all() {
        v.push(pattern::tiling_from_key(&format!("catalog:{}", t.id)).unwrap());
    }
    v.extend(pattern::tiling_list("all-periodic isohedral").unwrap());
    v
}

/// The same map up to a lattice translation of every cell by one vector.
fn same(a: &CellMap, b: &CellMap) -> bool {
    if a.m != b.m || a.perm != b.perm {
        return false;
    }
    let d = [a.off[0][0] - b.off[0][0], a.off[0][1] - b.off[0][1]];
    a.off
        .iter()
        .zip(&b.off)
        .all(|(x, y)| [x[0] - y[0], x[1] - y[1]] == d)
}

#[test]
fn every_tilings_cell_maps_form_a_group() {
    let specs = periodic();
    assert!(specs.len() >= 1_355, "{}", specs.len());
    let mut open = vec![];
    for t in &specs {
        for nb in [Nb::Edge, Nb::Touch] {
            let s = tiling_symmetries(t, nb, true).unwrap();
            let closed = s
                .iter()
                .all(|x| s.iter().all(|y| s.iter().any(|w| same(w, &compose(x, y)))));
            let dup = s
                .iter()
                .enumerate()
                .any(|(i, x)| s.iter().skip(i + 1).any(|y| same(x, y)));
            if !closed || dup {
                open.push(format!(
                    "{} {nb:?}: closed {closed}, duplicates {dup}",
                    pattern::tiling_key(t)
                ));
            }
        }
    }
    assert!(
        open.is_empty(),
        "{} not groups: {:?}",
        open.len(),
        &open[..open.len().min(10)]
    );
}

/// An independent scan for the symmetries, from the geometry alone: every symmetry carries tile 0
/// of the cell onto some tile with its corners onto corners, so the candidates are the isometries
/// that map two consecutive corners of tile 0 onto two consecutive corners (either way round) of
/// a tile type of the cell; a candidate is kept when every tile of the cell lands exactly on a
/// tile. Returns the rotation parts found and the translations inside the cell.
fn scan(t: &TilingSpec) -> (Vec<[f64; 4]>, usize) {
    let mut spec = pattern::tiling_identity(t);
    spec.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    let topo = Topology::build(&spec).unwrap();
    let g = topo.lattice().unwrap();
    let (a1, a2) = (g.a1, g.a2);
    let det = a1[0] * a2[1] - a2[0] * a1[1];
    let to_l = |p: P| {
        [
            (p[0] * a2[1] - p[1] * a2[0]) / det,
            (a1[0] * p[1] - a1[1] * p[0]) / det,
        ]
    };
    let tol = 1e-6;
    let int = |l: [f64; 2]| (l[0] - l[0].round()).abs() < tol && (l[1] - l[1].round()).abs() < tol;
    let polys: Vec<Vec<P>> = g
        .types
        .iter()
        .map(|(m, s)| topo.shapes[*s].iter().map(|&p| m.apply(p)).collect())
        .collect();
    let cents: Vec<P> = polys.iter().map(|p| centroid(p)).collect();
    let lands = |r: [f64; 4], tr: P| -> bool {
        polys.iter().all(|poly| {
            let moved: Vec<P> = poly
                .iter()
                .map(|q| {
                    [
                        r[0] * q[0] + r[1] * q[1] + tr[0],
                        r[2] * q[0] + r[3] * q[1] + tr[1],
                    ]
                })
                .collect();
            let mc = centroid(&moved);
            polys.iter().zip(&cents).any(|(p2, c2)| {
                let l = to_l([mc[0] - c2[0], mc[1] - c2[1]]);
                if !int(l) {
                    return false;
                }
                let s = [
                    l[0].round() * a1[0] + l[1].round() * a2[0],
                    l[0].round() * a1[1] + l[1].round() * a2[1],
                ];
                moved.len() == p2.len()
                    && moved.iter().all(|q| {
                        p2.iter().any(|v| {
                            (v[0] + s[0] - q[0]).abs() < tol && (v[1] + s[1] - q[1]).abs() < tol
                        })
                    })
            })
        })
    };
    let close = |a: &[f64; 4], b: &[f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
    // Translations inside the cell: tile 0 onto each type, unturned.
    let taus: Vec<P> = (0..cents.len())
        .map(|k0| [cents[k0][0] - cents[0][0], cents[k0][1] - cents[0][1]])
        .filter(|&tr| lands([1.0, 0.0, 0.0, 1.0], tr))
        .collect();
    let translation = |v: P| taus.iter().any(|t| int(to_l([v[0] - t[0], v[1] - t[1]])));
    let mut found: Vec<[f64; 4]> = vec![];
    let p0 = &polys[0];
    let e = [p0[1][0] - p0[0][0], p0[1][1] - p0[0][1]];
    let le = (e[0] * e[0] + e[1] * e[1]).sqrt();
    for pk in &polys {
        if pk.len() != p0.len() {
            continue;
        }
        let n = pk.len();
        for s0 in 0..n {
            for rev in [false, true] {
                let (w0, w1) = if rev {
                    (pk[s0], pk[(s0 + n - 1) % n])
                } else {
                    (pk[s0], pk[(s0 + 1) % n])
                };
                let f = [w1[0] - w0[0], w1[1] - w0[1]];
                if ((f[0] * f[0] + f[1] * f[1]).sqrt() - le).abs() > tol {
                    continue;
                }
                // The rotation or reflection taking e to f.
                let (ce, se) = (e[0] / le, e[1] / le);
                let (cf, sf) = (f[0] / le, f[1] / le);
                // The rotation and the reflection taking e to f (tiles may be listed either
                // way round, so both are tried for both orders).
                let (c, s) = (cf * ce + sf * se, sf * ce - cf * se);
                let (c2, s2) = (cf * ce - sf * se, sf * ce + cf * se);
                for r in [[c, -s, s, c], [c2, s2, s2, -c2]] {
                    let rv = [
                        r[0] * p0[0][0] + r[1] * p0[0][1],
                        r[2] * p0[0][0] + r[3] * p0[0][1],
                    ];
                    let tr = [w0[0] - rv[0], w0[1] - rv[1]];
                    // A symmetry also carries the cell vectors to translations of the tiling (a
                    // motion that fits one cell's tiles need not fit the next cell's).
                    let turn = |v: P| [r[0] * v[0] + r[1] * v[1], r[2] * v[0] + r[3] * v[1]];
                    if translation(turn(a1))
                        && translation(turn(a2))
                        && lands(r, tr)
                        && !found.iter().any(|x| close(x, &r))
                    {
                        found.push(r);
                    }
                }
            }
        }
    }
    (found, taus.len())
}

#[test]
fn the_whole_symmetry_group_is_found_on_every_tiling() {
    let close = |a: &[f64; 4], b: &[f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
    let mut bad = vec![];
    let mut nonprimitive = 0;
    for t in periodic() {
        let grp = tiling_group(&t, Nb::Touch).unwrap();
        let (want, translations) = scan(&t);
        assert_eq!(grp.taus.len(), translations, "{}", pattern::tiling_key(&t));
        // The rotation parts the canonical forms use: the isometries on a cell that holds its
        // pattern more than once, else the cell maps written in world coordinates.
        let have: Vec<[f64; 4]> = if grp.primitive() {
            let det = grp.a1[0] * grp.a2[1] - grp.a2[0] * grp.a1[1];
            tiling_symmetries(&t, Nb::Touch, true)
                .unwrap()
                .iter()
                .map(|m| {
                    let w = |x: f64, y: f64| {
                        [x * grp.a1[0] + y * grp.a2[0], x * grp.a1[1] + y * grp.a2[1]]
                    };
                    let l = |p: P| {
                        [
                            (p[0] * grp.a2[1] - p[1] * grp.a2[0]) / det,
                            (grp.a1[0] * p[1] - grp.a1[1] * p[0]) / det,
                        ]
                    };
                    let im = |p: P| {
                        let q = l(p);
                        let m = &m.m;
                        w(
                            m[0] as f64 * q[0] + m[1] as f64 * q[1],
                            m[2] as f64 * q[0] + m[3] as f64 * q[1],
                        )
                    };
                    let (e1, e2) = (im([1.0, 0.0]), im([0.0, 1.0]));
                    [e1[0], e2[0], e1[1], e2[1]]
                })
                .collect()
        } else {
            nonprimitive += 1;
            grp.isos.iter().map(|g| g.r).collect()
        };
        let missing: Vec<_> = want
            .iter()
            .filter(|r| !have.iter().any(|h| close(h, r)))
            .collect();
        let extra: Vec<_> = have
            .iter()
            .filter(|h| !want.iter().any(|r| close(h, r)))
            .collect();
        // One-tile lattices use the symmetries of their neighbour graph, which may be more than
        // the geometric ones (the brick wall's graph is the hexagonal grid's).
        if !missing.is_empty() || (!extra.is_empty() && grp.ntypes > 1) {
            bad.push(format!(
                "{}: missing {:?}, extra {:?}",
                pattern::tiling_key(&t),
                missing,
                extra
            ));
        }
    }
    assert!(
        nonprimitive >= 19,
        "{nonprimitive} cells hold their pattern more than once"
    );
    assert!(bad.is_empty(), "{:?}", bad);
}

fn prov() -> Provenance {
    Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: pattern::now_utc(),
        ..Default::default()
    }
}

/// The periodic objects of a world.
fn records_of(w: &World) -> Vec<PatternRecord> {
    let r = find_objects(w, &ObjectOptions::default()).unwrap();
    r.objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .filter_map(|o| record_from_object(w, w.cas[0].id, o, prov(), DiscoveredBy::User).ok())
        .collect()
}

/// Still lifes and oscillators of a few soups.
fn soup_records(family: &str, rule: &str) -> Vec<PatternRecord> {
    // The rule given, then others, until a soup leaves still lifes or oscillators.
    for rule in [rule, "B3/S23", "B2/S34", "B3/S12", "B34/S34", "B2/S2"] {
        let mut w = World::new(TilingSpec::rect(family, 40, 40, Boundary::Wrap), 5).unwrap();
        w.apply(Command::AddCa {
            ca: CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 }),
        })
        .unwrap();
        w.apply(Command::Step { gens: 400 }).unwrap();
        let mut v: Vec<PatternRecord> = records_of(&w)
            .into_iter()
            .filter(|r| matches!(r.class, PatternClass::StillLife | PatternClass::Oscillator))
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v.dedup_by(|a, b| a.id == b.id);
        if !v.is_empty() {
            return v;
        }
    }
    vec![]
}

/// A record's cells moved by an isometry of the tiling.
fn moved(
    rec: &PatternRecord,
    grp: &jetcarp_core::pattern::group::TilingGroup,
    g: &Iso,
) -> PatternRecord {
    let mut x = rec.clone();
    x.phases = rec
        .all_phases()
        .unwrap()
        .iter()
        .map(|ph| {
            let mut v: Phase = ph.iter().map(|c| grp.apply(g, c).unwrap()).collect();
            v.sort_unstable();
            encode_cells(&v)
        })
        .collect();
    x.canonical = "not canonical".into();
    x
}

// A pattern and its translate inside the cell (the glide family's rectangles, and IH58's squares,
// sit twice in a cell) get one id; so does every image under every symmetry of the 19 tilings
// whose cell holds its pattern more than once and that have rotations no cell map can write.
#[test]
fn patterns_have_one_id_under_the_whole_group() {
    for (family, rule) in [
        ("glide", "B3/S23"),
        ("ih58", "B3/S23"),
        ("hex_flat", "B2/S34"),
        ("ih02", "B2/S34"),
        ("ih85", "B2/S34"),
        ("ih42", "B3/S23"),
    ] {
        let recs = soup_records(family, rule);
        assert!(!recs.is_empty(), "{family}: no objects");
        let grp = tiling_group(&recs[0].tiling, recs[0].neighborhood).unwrap();
        assert!(!grp.primitive(), "{family}");
        // The translation inside the cell, as a cell map (the symmetry search finds it now).
        let maps = tiling_symmetries(&recs[0].tiling, recs[0].neighborhood, true).unwrap();
        let tr = maps
            .iter()
            .find(|m| {
                m.m == [1, 0, 0, 1] && m.perm.iter().enumerate().any(|(k, &p)| p as usize != k)
            })
            .unwrap_or_else(|| panic!("{family}: no translation inside the cell"));
        let mut n = 0;
        for rec in recs.iter().take(6) {
            assert_eq!(
                pattern::pattern_id(rec).unwrap(),
                rec.id,
                "{family} {}",
                rec.label()
            );
            let mut x = rec.clone();
            x.phases = rec
                .all_phases()
                .unwrap()
                .iter()
                .map(|ph| {
                    let mut v: Phase = ph.iter().map(|c| tr.apply(c)).collect();
                    v.sort_unstable();
                    encode_cells(&v)
                })
                .collect();
            assert_eq!(
                pattern::pattern_id(&x).unwrap(),
                rec.id,
                "{family}: translate of {}",
                rec.label()
            );
            for (gi, g) in grp.isos.iter().enumerate() {
                let y = moved(rec, &grp, g);
                assert_eq!(
                    pattern::pattern_id(&y).unwrap(),
                    rec.id,
                    "{family} {} symmetry {gi}",
                    rec.label()
                );
                let mut c = y.clone();
                c.canonicalize().unwrap();
                assert_eq!(
                    (&c.phases, &c.canonical),
                    (&rec.phases, &rec.canonical),
                    "{family}"
                );
                n += 1;
            }
        }
        assert!(n >= grp.isos.len(), "{family}");
    }
}

/// Conway's glider on IH58 (unit squares, two to a 1 x 2 cell, so Life on IH58 is Life on the
/// square grid): placed in each of the 8 orientations of the square grid, at both rows of the
/// cell, it is found by the object finder (which sees it again only after it has moved by a whole
/// cell vector) and gets one id, period, and displacement.
#[test]
fn the_glider_on_ih58_has_one_id_in_every_orientation_and_row() {
    let glider: [[i32; 2]; 5] = [[1, 0], [2, 1], [0, 2], [1, 2], [2, 2]];
    let mut ids = std::collections::BTreeMap::new();
    for turn in 0..8 {
        for row in 0..2 {
            let mut w = World::new(TilingSpec::rect("ih58", 24, 12, Boundary::Wrap), 1).unwrap();
            let cells: Vec<[u32; 2]> = glider
                .iter()
                .map(|&[x, y]| {
                    let (x, y) = match turn {
                        0 => (x, y),
                        1 => (-y, x),
                        2 => (-x, -y),
                        3 => (y, -x),
                        4 => (-x, y),
                        5 => (x, -y),
                        6 => (y, x),
                        _ => (-y, -x),
                    };
                    // Unit squares: the square at column x, row y has its centre there.
                    let p = [10.3 + x as f64, 10.3 + (y + row) as f64];
                    [w.topo.locate(p).unwrap() as u32, 1]
                })
                .collect();
            w.apply(Command::AddCa {
                ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
            })
            .unwrap();
            let v = records_of(&w);
            assert_eq!(v.len(), 1, "turn {turn} row {row}");
            let r = &v[0];
            assert_eq!(r.class, PatternClass::Spaceship);
            *ids.entry((r.id.clone(), r.period, r.displacement))
                .or_insert(0) += 1;
        }
    }
    assert_eq!(ids.len(), 1, "{ids:?}");
}
