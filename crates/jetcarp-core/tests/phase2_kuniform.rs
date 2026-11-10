//! Phase 2 deliverable 4: the 1,248 k-uniform tilings (k ≤ 6) from Galebach's catalog, via
//! Soto Sánchez's integer encoding (`tools/gen_kuniform.py`). These checks recompute everything
//! from the decoded polygons, independently of the converter.

use jetcarp_core::catalog::{self, CatalogTiling};
use jetcarp_core::geom::*;
use jetcarp_core::{Nb, SizeSpec, TilingSpec, Topology};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::PI;

const Q: f64 = 1e6;

fn kuniform() -> Vec<&'static CatalogTiling> {
    catalog::all()
        .iter()
        .filter(|t| t.group.starts_with("k-uniform"))
        .collect()
}

/// A tiling with its vertices and polygons reduced modulo the lattice.
struct Cell {
    t: &'static CatalogTiling,
    det: f64,
    /// Vertex key -> representative position.
    verts: BTreeMap<(i64, i64), P>,
    /// Vertex key -> (bisector angle, sides) of each incident polygon, in angular order.
    stars: HashMap<(i64, i64), Vec<(f64, usize)>>,
    /// (centroid key, sides) for every polygon.
    faces: HashSet<((i64, i64), usize)>,
    vtype: HashMap<(i64, i64), Vec<usize>>,
}

impl Cell {
    fn new(t: &'static CatalogTiling) -> Cell {
        let mut c = Cell {
            t,
            det: cross(t.a1, t.a2),
            verts: BTreeMap::new(),
            stars: HashMap::new(),
            faces: HashSet::new(),
            vtype: HashMap::new(),
        };
        for p in &t.polys {
            let g = centroid(p);
            c.faces.insert((c.key(g), p.len()));
            let n = p.len();
            for &v in p {
                let k = c.key(v);
                c.verts.entry(k).or_insert(v);
                // Angle of the polygon's interior bisector at this corner.
                let b = sub(g, v);
                c.stars.entry(k).or_default().push((b[1].atan2(b[0]), n));
            }
        }
        for (k, s) in c.stars.iter_mut() {
            s.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let seq: Vec<usize> = s.iter().map(|x| x.1).collect();
            c.vtype.insert(*k, canon(&seq));
        }
        c
    }
    fn frac(&self, p: P) -> (f64, f64) {
        let (a1, a2) = (self.t.a1, self.t.a2);
        (cross(p, a2) / self.det, cross(a1, p) / self.det)
    }
    fn key(&self, p: P) -> (i64, i64) {
        let (u, v) = self.frac(p);
        let m = Q as i64;
        (
            ((u * Q).round() as i64).rem_euclid(m),
            ((v * Q).round() as i64).rem_euclid(m),
        )
    }
    fn is_lattice(&self, v: P) -> bool {
        let (u, w) = self.frac(v);
        (u - u.round()).abs() < 1e-6 && (w - w.round()).abs() < 1e-6
    }
    fn area(&self) -> f64 {
        self.det.abs()
    }
    fn density(&self) -> f64 {
        self.verts.len() as f64 / self.area()
    }
    /// Edge directions (radians) at a vertex.
    fn edge_dirs(&self, k: (i64, i64)) -> Vec<f64> {
        let mut out = Vec::new();
        for p in &self.t.polys {
            for (i, &c) in p.iter().enumerate() {
                if self.key(c) == k {
                    let d = sub(p[(i + 1) % p.len()], c);
                    out.push(d[1].atan2(d[0]));
                }
            }
        }
        out
    }
}

/// Least rotation or reflection of a cyclic sequence.
fn canon(seq: &[usize]) -> Vec<usize> {
    let n = seq.len();
    let mut best: Option<Vec<usize>> = None;
    for rev in [false, true] {
        let s: Vec<usize> = if rev {
            seq.iter().rev().copied().collect()
        } else {
            seq.to_vec()
        };
        for r in 0..n {
            let c: Vec<usize> = (0..n).map(|i| s[(i + r) % n]).collect();
            if best.as_ref().is_none_or(|b| c < *b) {
                best = Some(c);
            }
        }
    }
    best.unwrap()
}

/// Usual notation: 3.3.4.3.4 -> 3^2.4.3.4.
fn type_str(seq: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        let mut j = i;
        while j < seq.len() && seq[j] == seq[i] {
            j += 1;
        }
        parts.push(if j - i > 1 {
            format!("{}^{}", seq[i], j - i)
        } else {
            seq[i].to_string()
        });
        i = j;
    }
    parts.join(".")
}

/// Vertex types the name promises, one per vertex orbit.
fn name_types(t: &CatalogTiling) -> Vec<String> {
    let mut v: Vec<String> = if t.k == 1 {
        vec![t.name.split(' ').next().unwrap().to_string()]
    } else {
        let inner = &t.name[1..t.name.find(']').unwrap()];
        inner.split("; ").map(String::from).collect()
    };
    v.sort();
    v
}

#[derive(Clone, Copy)]
struct Iso {
    rot: f64,
    refl: bool,
    t: P,
}

impl Iso {
    fn lin(&self, p: P) -> P {
        let p = if self.refl { [p[0], -p[1]] } else { p };
        let (s, c) = self.rot.sin_cos();
        [c * p[0] - s * p[1], s * p[0] + c * p[1]]
    }
    fn apply(&self, p: P) -> P {
        add(self.lin(p), self.t)
    }
}

/// Isometries g with g(a) = b, one per class modulo b's lattice. With `first`, stop at one.
fn isometries(a: &Cell, b: &Cell, first: bool) -> Vec<Iso> {
    let mut out = Vec::new();
    if (a.density() - b.density()).abs() > 1e-9 {
        return out;
    }
    let (&k0, &v0) = a.verts.iter().next().unwrap();
    let d0 = a.edge_dirs(k0)[0];
    for (&kw, &w) in &b.verts {
        if b.vtype[&kw] != a.vtype[&k0] {
            continue;
        }
        for phi in b.edge_dirs(kw) {
            for refl in [false, true] {
                // Linear part sending direction d0 to phi.
                let rot = if refl { phi + d0 } else { phi - d0 };
                let mut g = Iso {
                    rot,
                    refl,
                    t: [0.0, 0.0],
                };
                g.t = sub(w, g.lin(v0));
                let ok = b.is_lattice(g.lin(a.t.a1))
                    && b.is_lattice(g.lin(a.t.a2))
                    && a.verts
                        .values()
                        .all(|&v| b.verts.contains_key(&b.key(g.apply(v))))
                    && a.t
                        .polys
                        .iter()
                        .all(|p| b.faces.contains(&(b.key(g.apply(centroid(p))), p.len())));
                if ok {
                    out.push(g);
                    if first {
                        return out;
                    }
                }
            }
        }
    }
    out
}

/// Vertex orbits under the full symmetry group.
fn orbits(c: &Cell) -> Vec<Vec<(i64, i64)>> {
    let keys: Vec<(i64, i64)> = c.verts.keys().copied().collect();
    let idx: HashMap<(i64, i64), usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let mut parent: Vec<usize> = (0..keys.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for g in isometries(c, c, false) {
        for (i, v) in c.verts.values().enumerate() {
            let j = idx[&c.key(g.apply(*v))];
            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
            parent[a] = b;
        }
    }
    let mut groups: BTreeMap<usize, Vec<(i64, i64)>> = BTreeMap::new();
    for (i, &k) in keys.iter().enumerate() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(k);
    }
    groups.into_values().collect()
}

#[test]
fn counts_per_k_and_ids() {
    let all = kuniform();
    let mut by_k = BTreeMap::new();
    for t in &all {
        *by_k.entry(t.k).or_insert(0) += 1;
        assert_eq!(t.group, format!("k-uniform (k={})", t.k));
        assert!(t.id.starts_with(&format!("k{}-", t.k)), "{}", t.id);
    }
    let want: BTreeMap<u32, usize> =
        [(1, 11), (2, 20), (3, 61), (4, 151), (5, 332), (6, 673)].into();
    assert_eq!(by_k, want);
    assert_eq!(all.len(), 1248);
    let ids: HashSet<&str> = catalog::all().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids.len(), catalog::all().len(), "catalog ids are unique");
    let names: HashSet<&str> = all.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names.len(), all.len(), "names are unique");
}

#[test]
fn polygons_are_regular_fill_the_cell_and_meet_edge_to_edge() {
    for t in kuniform() {
        let c = Cell::new(t);
        let mut area_sum = 0.0;
        // Directed edges keyed by midpoint; each must be matched by its reverse.
        let mut edges: HashMap<(i64, i64), Vec<P>> = HashMap::new();
        for p in &t.polys {
            let n = p.len();
            assert!([3, 4, 6, 8, 12].contains(&n), "{}: {n}-gon", t.id);
            for i in 0..n {
                let e = sub(p[(i + 1) % n], p[i]);
                assert!(
                    (len(e) - 1.0).abs() < 1e-9,
                    "{}: edge length {}",
                    t.id,
                    len(e)
                );
                let f = sub(p[(i + 2) % n], p[(i + 1) % n]);
                let turn = cross(e, f).atan2(dot(e, f));
                assert!(
                    (turn - 2.0 * PI / n as f64).abs() < 1e-9,
                    "{}: not regular",
                    t.id
                );
                let mid = scale(add(p[i], p[(i + 1) % n]), 0.5);
                edges.entry(c.key(mid)).or_default().push(e);
            }
            area_sum += area(p);
        }
        assert!(
            (area_sum - c.area()).abs() < 1e-8,
            "{}: area {area_sum} vs cell {}",
            t.id,
            c.area()
        );
        for (k, es) in &edges {
            assert!(
                es.len() == 2 && len(add(es[0], es[1])) < 1e-9,
                "{}: edge at {k:?} is not shared edge to edge",
                t.id
            );
        }
        // Corner angles close up at every vertex.
        for (k, s) in &c.stars {
            let sum: f64 = s.iter().map(|&(_, n)| PI - 2.0 * PI / n as f64).sum();
            assert!(
                (sum - 2.0 * PI).abs() < 1e-9,
                "{}: vertex {k:?} angle {sum}",
                t.id
            );
        }
    }
}

#[test]
fn k_vertex_orbits_with_the_named_vertex_types() {
    let mut checked = 0;
    for t in kuniform() {
        let c = Cell::new(t);
        let orb = orbits(&c);
        assert_eq!(
            orb.len(),
            t.k as usize,
            "{} {}: vertex orbits",
            t.id,
            t.name
        );
        let mut types: Vec<String> = orb.iter().map(|o| type_str(&c.vtype[&o[0]])).collect();
        types.sort();
        assert_eq!(types, name_types(t), "{} {}", t.id, t.name);
        checked += 1;
    }
    assert_eq!(checked, 1248);
}

#[test]
fn no_two_tilings_are_isometric() {
    let cells: Vec<Cell> = kuniform().into_iter().map(Cell::new).collect();
    // (vertex types, vertex density, polygon counts per area) -> tilings.
    type Sig = (Vec<String>, i64, Vec<(usize, i64)>);
    let mut by_sig: HashMap<Sig, Vec<usize>> = HashMap::new();
    for (i, c) in cells.iter().enumerate() {
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        for p in &c.t.polys {
            *counts.entry(p.len()).or_default() += 1;
        }
        let per_area = counts
            .into_iter()
            .map(|(n, m)| (n, (m as f64 / c.area() * 1e6).round() as i64))
            .collect();
        let sig = (
            name_types(c.t),
            (c.density() * 1e6).round() as i64,
            per_area,
        );
        by_sig.entry(sig).or_default().push(i);
    }
    let mut compared = 0;
    for group in by_sig.values() {
        for (x, &i) in group.iter().enumerate() {
            for &j in &group[x + 1..] {
                compared += 1;
                assert!(
                    isometries(&cells[i], &cells[j], true).is_empty(),
                    "{} and {} are the same tiling",
                    cells[i].t.id,
                    cells[j].t.id
                );
            }
        }
    }
    assert!(compared > 0);
}

#[test]
fn every_tiling_builds_with_measured_neighbours() {
    for t in kuniform() {
        let c = Cell::new(t);
        let spec = TilingSpec {
            params: Some(serde_json::json!({ "catalog": t.id })),
            ..TilingSpec::new("periodic")
        }
        .with_size(SizeSpec::Rect { w: 8, h: 8 });
        let top = Topology::build(&spec).unwrap_or_else(|e| panic!("{}: {e}", t.id));
        assert_eq!(top.len(), 64 * t.polys.len(), "{}", t.id);
        // Polygons at each vertex, to predict the touching-neighbour count.
        let deg: HashMap<(i64, i64), usize> = c.stars.iter().map(|(k, s)| (*k, s.len())).collect();
        for (k, p) in t.polys.iter().enumerate() {
            let n = p.len();
            let touch: usize = p.iter().map(|&v| deg[&c.key(v)] - 1).sum::<usize>() - n;
            assert_eq!(
                top.degree(k, Nb::Edge),
                n,
                "{}: polygon {k} edge neighbours",
                t.id
            );
            assert_eq!(
                top.degree(k, Nb::Touch),
                touch,
                "{}: polygon {k} touching",
                t.id
            );
        }
    }
}

#[test]
fn unit_cell_checker_accepts_every_tiling() {
    use jetcarp_core::cellcheck::{UnitCell, check};
    for t in kuniform() {
        let cell = UnitCell::from_params(&serde_json::json!({ "catalog": t.id })).unwrap();
        let rep = check(&cell, false);
        assert!(
            rep.ok(),
            "{}: {:?}",
            t.id,
            rep.problems.iter().map(|p| p.message()).collect::<Vec<_>>()
        );
        assert!((rep.cell_area - rep.tile_area).abs() < 1e-8, "{}", t.id);
    }
}
