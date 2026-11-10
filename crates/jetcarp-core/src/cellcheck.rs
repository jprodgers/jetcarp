//! Unit cells of the `periodic` family and checks on them: two lattice vectors and any polygons in
//! one cell. [`check`] reports overlaps between tiles (including translates), gaps, degenerate and
//! self-intersecting polygons, edges with nothing on the other side, and the measured edge and
//! touching neighbours of every tile, so mixed-tile and non-edge-to-edge cells can be edited with
//! live feedback. The app's unit-cell editor draws this report; it decides nothing itself.

use crate::families::{self, FamilyKind};
use crate::geom::*;
use crate::topology::{Nb, SizeSpec, TilingSpec, Topology, contact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Contact tolerance used by the tiling builder (`topology::lattice_def`).
pub const CONTACT_EPS: f64 = 1e-4;
/// Largest lattice shift searched in each direction.
const MAX_SHIFT: i32 = 8;
/// Samples per lattice direction when looking for where a gap is.
const GAP_SAMPLES: usize = 48;

/// One unit cell: lattice vectors and the polygons repeated by them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitCell {
    pub a1: P,
    pub a2: P,
    pub polys: Vec<Vec<P>>,
}

impl UnitCell {
    /// From `periodic` family parameters (`{a1, a2, polys}` or `{catalog: id}`).
    pub fn from_params(p: &Value) -> Result<UnitCell, String> {
        let (a1, a2, polys) = crate::catalog::from_params(p)?;
        Ok(UnitCell { a1, a2, polys })
    }

    /// Inline `periodic` parameters, rounded to 1e-9 so copied JSON stays readable.
    pub fn params(&self) -> Value {
        let r = |x: f64| {
            let v = (x * 1e9).round() / 1e9;
            if v == 0.0 { 0.0 } else { v }
        };
        let p = |q: &P| json!([r(q[0]), r(q[1])]);
        json!({
            "a1": p(&self.a1),
            "a2": p(&self.a2),
            "polys": self.polys.iter().map(|poly| poly.iter().map(p).collect::<Vec<_>>()).collect::<Vec<_>>(),
        })
    }

    /// The `periodic` tiling spec for this cell (no size or boundary set).
    pub fn spec(&self) -> TilingSpec {
        TilingSpec {
            params: Some(self.params()),
            ..TilingSpec::new("periodic")
        }
    }

    /// Unit cell of any lattice tiling. Static families keep their own coordinates; reshapeable
    /// families are built and their tile types read back (straight corners, edge shapes dropped).
    pub fn from_spec(spec: &TilingSpec) -> Result<UnitCell, String> {
        let fam = families::family(&spec.family)
            .ok_or_else(|| format!("unknown tiling family '{}'", spec.family))?;
        let params = spec
            .params
            .clone()
            .unwrap_or_else(|| families::init_params(fam.id));
        match fam.kind {
            FamilyKind::Aperiodic => Err(format!("{} is aperiodic: it has no unit cell", fam.name)),
            FamilyKind::Static => {
                let (a1, a2, polys) = if fam.id == "periodic" {
                    crate::catalog::from_params(&params)?
                } else {
                    families::static_cell(fam.id, &params).ok_or("no unit cell for this family")?
                };
                Ok(UnitCell { a1, a2, polys })
            }
            FamilyKind::Reshapeable => {
                let small = TilingSpec {
                    params: Some(params),
                    edges: None,
                    size: Some(SizeSpec::Rect { w: 8, h: 8 }),
                    ..TilingSpec::new(fam.id)
                };
                let t = Topology::build(&small)?;
                let g = t.lattice().ok_or("not a lattice tiling")?;
                let polys = g
                    .types
                    .iter()
                    .map(|(m, s)| t.shapes[*s].iter().map(|&p| m.apply(p)).collect())
                    .collect();
                Ok(UnitCell {
                    a1: g.a1,
                    a2: g.a2,
                    polys,
                })
            }
        }
    }

    /// Translation by `di·a1 + dj·a2`.
    pub fn shift(&self, s: [i32; 2]) -> P {
        add(scale(self.a1, s[0] as f64), scale(self.a2, s[1] as f64))
    }

    /// Area of the lattice parallelogram.
    pub fn area(&self) -> f64 {
        cross(self.a1, self.a2).abs()
    }

    /// Lattice coordinates of a point (`p = u·a1 + v·a2`).
    pub fn to_lattice(&self, p: P) -> [f64; 2] {
        let det = cross(self.a1, self.a2);
        [cross(p, self.a2) / det, cross(self.a1, p) / det]
    }

    /// Polygon `k` moved by lattice shift `s`.
    pub fn placed(&self, k: usize, s: [i32; 2]) -> Vec<P> {
        let t = self.shift(s);
        self.polys[k].iter().map(|&p| add(p, t)).collect()
    }

    /// Bounding box of polygon `k` in lattice coordinates.
    fn lattice_box(&self, k: usize) -> [f64; 4] {
        let pts: Vec<P> = self.polys[k].iter().map(|&p| self.to_lattice(p)).collect();
        bbox(&pts)
    }

    /// Lattice shifts `s` (excluding none) for which polygon `b + s` might meet polygon `a`.
    fn shifts_between(&self, a: usize, b: usize, pad: f64) -> Vec<[i32; 2]> {
        let (ba, bb) = (self.lattice_box(a), self.lattice_box(b));
        // A world distance d moves lattice coordinates by at most d·|a2|/det and d·|a1|/det.
        let det = self.area().max(1e-300);
        let (pu, pv) = (pad * len(self.a2) / det, pad * len(self.a1) / det);
        let lo_i = ((ba[0] - bb[2] - pu).floor() as i32).max(-MAX_SHIFT);
        let hi_i = ((ba[2] - bb[0] + pu).ceil() as i32).min(MAX_SHIFT);
        let lo_j = ((ba[1] - bb[3] - pv).floor() as i32).max(-MAX_SHIFT);
        let hi_j = ((ba[3] - bb[1] + pv).ceil() as i32).min(MAX_SHIFT);
        let mut out = Vec::new();
        for i in lo_i..=hi_i {
            for j in lo_j..=hi_j {
                out.push([i, j]);
            }
        }
        out
    }
}

/// A neighbouring tile: polygon index and the lattice shift of its copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact {
    pub poly: usize,
    pub shift: [i32; 2],
}

/// Measured facts about one polygon of the cell.
#[derive(Clone, Debug, Default)]
pub struct TileInfo {
    pub corners: usize,
    pub area: f64,
    /// Tiles sharing a stretch of edge.
    pub edge: Vec<Contact>,
    /// Tiles sharing an edge or only a point (includes `edge`).
    pub touch: Vec<Contact>,
}

#[derive(Clone, Debug)]
pub enum Problem {
    /// Lattice vectors are parallel or zero.
    Lattice,
    TooFewCorners {
        poly: usize,
    },
    /// Two consecutive corners coincide.
    ZeroEdge {
        poly: usize,
        at: P,
    },
    /// The polygon has (almost) no area.
    NoArea {
        poly: usize,
    },
    SelfIntersect {
        poly: usize,
        at: P,
    },
    /// Polygon `b` moved by `shift` overlaps polygon `a`. `pieces` are the overlap regions.
    Overlap {
        a: usize,
        b: usize,
        shift: [i32; 2],
        area: f64,
        pieces: Vec<Vec<P>>,
    },
    /// Part of the plane is not covered. `spots` are sample points inside the gap (base cell).
    Gap {
        area: f64,
        spots: Vec<P>,
    },
    /// Tiles add up to more than the cell area.
    Excess {
        area: f64,
    },
    /// A stretch of edge with no tile on the other side: a gap or an overlap runs along it.
    Unmatched {
        poly: usize,
        from: P,
        to: P,
    },
    /// The tiling builder needs every tile to share an edge with some tile.
    NoEdgeNeighbour {
        poly: usize,
    },
    /// The tiling builder refused the cell.
    Build(String),
}

impl Problem {
    /// Errors stop the cell from being a valid tiling; warnings are informational.
    pub fn is_error(&self) -> bool {
        !matches!(self, Problem::Unmatched { .. })
    }
    /// Polygons involved (base-cell indices).
    pub fn polys(&self) -> Vec<usize> {
        match *self {
            Problem::TooFewCorners { poly }
            | Problem::ZeroEdge { poly, .. }
            | Problem::NoArea { poly }
            | Problem::SelfIntersect { poly, .. }
            | Problem::Unmatched { poly, .. }
            | Problem::NoEdgeNeighbour { poly } => vec![poly],
            Problem::Overlap { a, b, .. } => vec![a, b],
            _ => vec![],
        }
    }
    /// A point to mark on the canvas.
    pub fn at(&self) -> Option<P> {
        match self {
            Problem::ZeroEdge { at, .. } | Problem::SelfIntersect { at, .. } => Some(*at),
            Problem::Overlap { pieces, .. } => pieces.first().map(|p| centroid(p)),
            Problem::Gap { spots, .. } => spots.first().copied(),
            Problem::Unmatched { from, to, .. } => Some(scale(add(*from, *to), 0.5)),
            _ => None,
        }
    }
    pub fn message(&self) -> String {
        let t = |k: usize| format!("tile {}", k + 1);
        match self {
            Problem::Lattice => {
                "Lattice vectors are parallel or zero: the cell has no area.".into()
            }
            Problem::TooFewCorners { poly } => {
                format!("{} has fewer than 3 corners.", capital(&t(*poly)))
            }
            Problem::ZeroEdge { poly, .. } => {
                format!("{} has two corners in the same place.", capital(&t(*poly)))
            }
            Problem::NoArea { poly } => format!("{} has no area.", capital(&t(*poly))),
            Problem::SelfIntersect { poly, .. } => {
                format!("{} crosses itself.", capital(&t(*poly)))
            }
            Problem::Overlap {
                a, b, shift, area, ..
            } => {
                let other = if shift == &[0, 0] {
                    t(*b)
                } else if a == b {
                    format!("its own copy shifted by ({}, {})", shift[0], shift[1])
                } else {
                    format!("{} shifted by ({}, {})", t(*b), shift[0], shift[1])
                };
                format!("{} overlaps {} (area {:.4}).", capital(&t(*a)), other, area)
            }
            Problem::Gap { area, spots } => {
                if spots.is_empty() {
                    format!(
                        "Gap: tiles leave {area:.4} of each cell uncovered (too thin to locate; see open edges)."
                    )
                } else {
                    format!("Gap: tiles leave {area:.4} of each cell uncovered (marked).")
                }
            }
            Problem::Excess { area } => {
                format!("Tiles cover {area:.4} more than the cell area: something overlaps.")
            }
            Problem::Unmatched { poly, .. } => {
                format!("An edge of {} has nothing on the other side.", t(*poly))
            }
            Problem::NoEdgeNeighbour { poly } => {
                format!("{} shares no edge with any tile.", capital(&t(*poly)))
            }
            Problem::Build(e) => format!("The tiling builder refused this cell: {e}"),
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Everything [`check`] found.
#[derive(Clone, Debug, Default)]
pub struct CellReport {
    pub cell_area: f64,
    pub tile_area: f64,
    pub tiles: Vec<TileInfo>,
    pub problems: Vec<Problem>,
}

impl CellReport {
    /// No errors: the cell tiles the plane and the builder accepts it.
    pub fn ok(&self) -> bool {
        !self.problems.iter().any(|p| p.is_error())
    }
    /// Distinct (edge, touching) neighbour counts over the tiles.
    pub fn degree_sets(&self) -> (Vec<usize>, Vec<usize>) {
        let mut e: Vec<usize> = self.tiles.iter().map(|t| t.edge.len()).collect();
        let mut t: Vec<usize> = self.tiles.iter().map(|t| t.touch.len()).collect();
        e.sort_unstable();
        e.dedup();
        t.sort_unstable();
        t.dedup();
        (e, t)
    }
}

/// Check a unit cell. `build` also asks the tiling builder to accept it (what *Apply* does).
pub fn check(cell: &UnitCell, build: bool) -> CellReport {
    let mut rep = CellReport {
        cell_area: cell.area(),
        tile_area: cell
            .polys
            .iter()
            .map(|p| if p.len() >= 3 { area(p) } else { 0.0 })
            .sum(),
        tiles: cell
            .polys
            .iter()
            .map(|p| TileInfo {
                corners: p.len(),
                area: if p.len() >= 3 { area(p) } else { 0.0 },
                ..Default::default()
            })
            .collect(),
        problems: Vec::new(),
    };
    let scale_len = rep.cell_area.sqrt().max(len(cell.a1)).max(1e-9);
    let longest = len(cell.a1).max(len(cell.a2));
    if !rep.cell_area.is_finite()
        || rep.cell_area <= 1e-9 * longest * longest
        || rep.cell_area < 1e-12
    {
        rep.problems.push(Problem::Lattice);
        return rep;
    }
    let tol = 1e-7 * scale_len;
    // Shape checks.
    let mut simple = vec![true; cell.polys.len()];
    for (k, poly) in cell.polys.iter().enumerate() {
        if poly.len() < 3 {
            rep.problems.push(Problem::TooFewCorners { poly: k });
            simple[k] = false;
            continue;
        }
        if let Some(i) = (0..poly.len()).find(|&i| dist(poly[i], poly[(i + 1) % poly.len()]) < tol)
        {
            rep.problems.push(Problem::ZeroEdge {
                poly: k,
                at: poly[i],
            });
            simple[k] = false;
            continue;
        }
        // All corners on one line (a bow-tie with zero net area is caught as a crossing below).
        let fan: f64 = (1..poly.len() - 1)
            .map(|i| cross(sub(poly[i], poly[0]), sub(poly[i + 1], poly[0])).abs() / 2.0)
            .sum();
        if fan < 1e-9 * scale_len * scale_len {
            rep.problems.push(Problem::NoArea { poly: k });
            simple[k] = false;
            continue;
        }
        if let Some(at) = self_crossing(poly, tol) {
            rep.problems.push(Problem::SelfIntersect { poly: k, at });
            simple[k] = false;
        }
    }
    // Overlaps, from exact clipping of triangulations.
    let tris: Vec<Vec<[P; 3]>> = cell
        .polys
        .iter()
        .enumerate()
        .map(|(k, p)| {
            if !simple[k] {
                return vec![];
            }
            triangulate(p)
                .into_iter()
                .map(|t| ccw_tri([p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]))
                .filter(|t| signed_area2(t) > 0.0)
                .collect()
        })
        .collect();
    let mut overlap_total = 0.0;
    let n = cell.polys.len();
    for a in 0..n {
        for b in a..n {
            if !simple[a] || !simple[b] {
                continue;
            }
            let ba = bbox(&cell.polys[a]);
            for s in cell.shifts_between(a, b, 0.0) {
                if a == b && (s[0] < 0 || (s[0] == 0 && s[1] <= 0)) {
                    continue; // self at no shift, and each ± pair once
                }
                let t = cell.shift(s);
                let bb = bbox(&cell.polys[b]);
                if ba[0] >= bb[2] + t[0]
                    || bb[0] + t[0] >= ba[2]
                    || ba[1] >= bb[3] + t[1]
                    || bb[1] + t[1] >= ba[3]
                {
                    continue;
                }
                let mut pieces = Vec::new();
                let mut ar = 0.0;
                for ta in &tris[a] {
                    let bta = bbox(ta);
                    for tb in &tris[b] {
                        let tb = [add(tb[0], t), add(tb[1], t), add(tb[2], t)];
                        let btb = bbox(&tb);
                        if bta[0] >= btb[2]
                            || btb[0] >= bta[2]
                            || bta[1] >= btb[3]
                            || btb[1] >= bta[3]
                        {
                            continue;
                        }
                        let piece = clip_convex(ta, &tb);
                        if piece.len() >= 3 {
                            let pa = area(&piece);
                            if pa > 1e-10 * scale_len * scale_len {
                                ar += pa;
                                pieces.push(piece);
                            }
                        }
                    }
                }
                let small = rep.tiles[a].area.min(rep.tiles[b].area);
                if ar > 1e-6 * small && ar > 1e-9 * scale_len * scale_len {
                    overlap_total += ar;
                    rep.problems.push(Problem::Overlap {
                        a,
                        b,
                        shift: s,
                        area: ar,
                        pieces,
                    });
                }
            }
        }
    }
    let all_simple = simple.iter().all(|&s| s);
    // Area balance: the union of one cell's tiles must equal the cell.
    if all_simple && n > 0 {
        let covered = rep.tile_area - overlap_total;
        let deficit = rep.cell_area - covered;
        if deficit > 1e-6 * rep.cell_area {
            let spots = gap_spots(cell);
            rep.problems.push(Problem::Gap {
                area: deficit,
                spots,
            });
        }
        if overlap_total == 0.0 && rep.tile_area > rep.cell_area * (1.0 + 1e-6) {
            rep.problems.push(Problem::Excess {
                area: rep.tile_area - rep.cell_area,
            });
        }
    }
    // Contact, measured exactly as the tiling builder does.
    for k in 0..n {
        if cell.polys[k].len() < 3 {
            continue;
        }
        let pk = &cell.polys[k];
        let bk = bbox(pk);
        let mut touch: Vec<(f64, Contact)> = Vec::new();
        let mut edge: Vec<(f64, Contact)> = Vec::new();
        for k2 in 0..n {
            if cell.polys[k2].len() < 3 {
                continue;
            }
            for s in cell.shifts_between(k, k2, CONTACT_EPS * 2.0) {
                if k2 == k && s == [0, 0] {
                    continue;
                }
                let q = cell.placed(k2, s);
                let bq = bbox(&q);
                let e = CONTACT_EPS;
                if bk[0] > bq[2] + e || bq[0] > bk[2] + e || bk[1] > bq[3] + e || bq[1] > bk[3] + e
                {
                    continue;
                }
                let (c, pos) = contact(pk, &q, CONTACT_EPS);
                let o = Contact { poly: k2, shift: s };
                if c >= 1 {
                    touch.push((pos, o));
                }
                if c == 2 {
                    edge.push((pos, o));
                }
            }
        }
        let flip = signed_area2(pk) < 0.0;
        let order = |mut list: Vec<(f64, Contact)>| -> Vec<Contact> {
            list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            if flip {
                list.reverse();
            }
            list.into_iter().map(|x| x.1).collect()
        };
        rep.tiles[k].touch = order(touch);
        rep.tiles[k].edge = order(edge);
        if rep.tiles[k].edge.is_empty() {
            rep.problems.push(Problem::NoEdgeNeighbour { poly: k });
        }
    }
    // Edge stretches with nothing on the other side.
    if all_simple {
        for k in 0..n {
            let others: Vec<Vec<P>> = rep.tiles[k]
                .edge
                .iter()
                .map(|c| cell.placed(c.poly, c.shift))
                .collect();
            for (from, to) in unmatched(&cell.polys[k], &others, CONTACT_EPS) {
                rep.problems.push(Problem::Unmatched { poly: k, from, to });
            }
        }
    }
    if build && rep.ok() {
        let spec = TilingSpec {
            size: Some(SizeSpec::Rect { w: 8, h: 8 }),
            ..cell.spec()
        };
        match Topology::build(&spec) {
            Ok(t) => {
                // The builder's own counts must agree with ours.
                let g = t.lattice().expect("periodic tilings are lattices");
                for k in 0..n.min(g.ntypes as usize) {
                    let (e, tc) = (
                        g.offsets[Nb::Edge as usize][k].len(),
                        g.offsets[Nb::Touch as usize][k].len(),
                    );
                    if e != rep.tiles[k].edge.len() || tc != rep.tiles[k].touch.len() {
                        rep.problems.push(Problem::Build(format!(
                            "tile {} measures {e} edge / {tc} touching neighbours in the builder but {} / {} here",
                            k + 1,
                            rep.tiles[k].edge.len(),
                            rep.tiles[k].touch.len()
                        )));
                    }
                }
            }
            Err(e) => rep.problems.push(Problem::Build(e)),
        }
    }
    rep
}

fn ccw_tri(t: [P; 3]) -> [P; 3] {
    if signed_area2(&t) < 0.0 {
        [t[0], t[2], t[1]]
    } else {
        t
    }
}

/// Sutherland–Hodgman: `subject` (convex) clipped to the convex `clip` (positive orientation).
fn clip_convex(subject: &[P], clip: &[P]) -> Vec<P> {
    let mut out = subject.to_vec();
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let side = |p: P| cross(sub(b, a), sub(p, a));
        let inp = std::mem::take(&mut out);
        for j in 0..inp.len() {
            let (p, q) = (inp[j], inp[(j + 1) % inp.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push(add(p, scale(sub(q, p), t)));
            }
        }
    }
    out
}

/// First place where a polygon's boundary meets itself, if any (crossings, touching, folds back).
pub fn self_crossing(poly: &[P], tol: f64) -> Option<P> {
    let m = poly.len();
    let seg = |i: usize| (poly[i], poly[(i + 1) % m]);
    for i in 0..m {
        let (a, b) = seg(i);
        // Adjacent edge folding back over this one.
        let c = poly[(i + 2) % m];
        let (u, v) = (sub(b, a), sub(c, b));
        if (cross(u, v) / (len(u) * len(v)).max(1e-300)).abs() < 1e-9 && dot(u, v) < 0.0 {
            return Some(b);
        }
        for j in i + 2..m {
            if i == 0 && j == m - 1 {
                continue;
            }
            let (c, e) = seg(j);
            if let Some(p) = seg_meet(a, b, c, e, tol) {
                return Some(p);
            }
        }
    }
    None
}

/// Where two segments meet (crossing, or an end lying on the other), within `tol`.
fn seg_meet(a: P, b: P, c: P, e: P, tol: f64) -> Option<P> {
    let on = |p: P, s0: P, s1: P| -> bool {
        let d = sub(s1, s0);
        let l = len(d);
        if l < tol {
            return dist(p, s0) < tol;
        }
        let off = cross(d, sub(p, s0)) / l;
        let t = dot(d, sub(p, s0)) / l;
        off.abs() < tol && t > -tol && t < l + tol
    };
    for (p, s0, s1) in [(c, a, b), (e, a, b), (a, c, e), (b, c, e)] {
        if on(p, s0, s1) {
            return Some(p);
        }
    }
    let (d1, d2) = (cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(e, a)));
    let (d3, d4) = (cross(sub(e, c), sub(a, c)), cross(sub(e, c), sub(b, c)));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        let t = d3 / (d3 - d4);
        return Some(add(a, scale(sub(b, a), t)));
    }
    None
}

/// Stretches of `p`'s boundary not shared with an edge of any polygon in `others`.
fn unmatched(p: &[P], others: &[Vec<P>], eps: f64) -> Vec<(P, P)> {
    let mut out = Vec::new();
    let m = p.len();
    for i in 0..m {
        let (a, b) = (p[i], p[(i + 1) % m]);
        let l = dist(a, b);
        if l < eps {
            continue;
        }
        let d = scale(sub(b, a), 1.0 / l);
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for q in others {
            for j in 0..q.len() {
                let (c, e) = (q[j], q[(j + 1) % q.len()]);
                let off = |x: P| cross(d, sub(x, a));
                if off(c).abs() > eps || off(e).abs() > eps {
                    continue;
                }
                let (t1, t2) = (dot(d, sub(c, a)), dot(d, sub(e, a)));
                let (lo, hi) = (t1.min(t2).max(0.0), t1.max(t2).min(l));
                if hi > lo {
                    spans.push((lo, hi));
                }
            }
        }
        spans.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let mut reach = 0.0;
        let at = |t: f64| add(a, scale(d, t));
        for (lo, hi) in spans {
            if lo > reach + eps * 10.0 {
                out.push((at(reach), at(lo)));
            }
            reach = f64::max(reach, hi);
        }
        if reach < l - eps * 10.0 {
            out.push((at(reach), b));
        }
    }
    out
}

/// Sample points of the base parallelogram that no tile covers, one per connected patch of
/// uncovered samples (the most interior sample of each), largest patches first, at most 8.
fn gap_spots(cell: &UnitCell) -> Vec<P> {
    let n = GAP_SAMPLES;
    let boxes: Vec<[f64; 4]> = (0..cell.polys.len()).map(|k| cell.lattice_box(k)).collect();
    let covered = |fu: f64, fv: f64| -> bool {
        let p = add(scale(cell.a1, fu), scale(cell.a2, fv));
        for (k, b) in boxes.iter().enumerate() {
            for di in (fu - b[2]).ceil() as i32..=(fu - b[0]).floor() as i32 {
                for dj in (fv - b[3]).ceil() as i32..=(fv - b[1]).floor() as i32 {
                    if point_in_poly(sub(p, cell.shift([di, dj])), &cell.polys[k]) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let mut open = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            open[j * n + i] = !covered((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64);
        }
    }
    // Connected patches on the torus of samples.
    let mut comp = vec![usize::MAX; n * n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for s in 0..n * n {
        if !open[s] || comp[s] != usize::MAX {
            continue;
        }
        let g = groups.len();
        let mut stack = vec![s];
        comp[s] = g;
        let mut members = Vec::new();
        while let Some(x) = stack.pop() {
            members.push(x);
            let (i, j) = (x % n, x / n);
            for (di, dj) in [(1, 0), (n - 1, 0), (0, 1), (0, n - 1)] {
                let y = ((j + dj) % n) * n + (i + di) % n;
                if open[y] && comp[y] == usize::MAX {
                    comp[y] = g;
                    stack.push(y);
                }
            }
        }
        groups.push(members);
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let depth = |x: usize| -> usize {
        let (i, j) = (x % n, x / n);
        let mut d = 0;
        while d < n / 2 {
            let r = d + 1;
            let ok = [(r, 0), (n - r, 0), (0, r), (0, n - r)]
                .iter()
                .all(|&(di, dj)| open[((j + dj) % n) * n + (i + di) % n]);
            if !ok {
                break;
            }
            d += 1;
        }
        d
    };
    groups
        .iter()
        .take(8)
        .map(|g| {
            let best = *g
                .iter()
                .max_by_key(|&&x| (depth(x), std::cmp::Reverse(x)))
                .unwrap();
            let (i, j) = (best % n, best / n);
            add(
                scale(cell.a1, (i as f64 + 0.5) / n as f64),
                scale(cell.a2, (j as f64 + 0.5) / n as f64),
            )
        })
        .collect()
}

/// Regular `n`-gon with side `side` whose first edge runs from `p` towards `q`, lying to the left
/// of that edge (positive orientation).
pub fn regular_polygon(p: P, q: P, n: usize, side: f64) -> Vec<P> {
    let n = n.max(3);
    let mut dir = sub(q, p);
    let l = len(dir).max(1e-300);
    dir = scale(dir, side / l);
    let turn = Affine::rot(std::f64::consts::TAU / n as f64);
    let mut out = Vec::with_capacity(n);
    let mut at = p;
    for _ in 0..n {
        out.push(at);
        at = add(at, dir);
        dir = turn.apply_vec(dir);
    }
    out
}

/// Regular `n`-gon with side `side`, centred on `c`, its first corner at angle `deg`.
pub fn regular_polygon_at(c: P, n: usize, side: f64, deg: f64) -> Vec<P> {
    let n = n.max(3);
    let r = side / (2.0 * crate::detmath::sin(std::f64::consts::PI / n as f64));
    (0..n)
        .map(|i| {
            let a = deg.to_radians() + std::f64::consts::TAU * i as f64 / n as f64;
            let (s, co) = crate::detmath::sin_cos(a);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<P> {
        vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }
    fn cell(a1: P, a2: P, polys: Vec<Vec<P>>) -> UnitCell {
        UnitCell { a1, a2, polys }
    }
    fn degrees(r: &CellReport) -> Vec<(usize, usize)> {
        r.tiles
            .iter()
            .map(|t| (t.edge.len(), t.touch.len()))
            .collect()
    }

    #[test]
    fn unit_square_is_valid() {
        let c = cell([1.0, 0.0], [0.0, 1.0], vec![rect(0.0, 0.0, 1.0, 1.0)]);
        let r = check(&c, true);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(degrees(&r), vec![(4, 8)]);
    }

    #[test]
    fn catches_overlap_with_translate() {
        // 1×1 squares repeated every 0.8: each overlaps its own copy.
        let c = cell([0.8, 0.0], [0.0, 1.0], vec![rect(0.0, 0.0, 1.0, 1.0)]);
        let r = check(&c, true);
        assert!(!r.ok());
        let ov: Vec<_> = r
            .problems
            .iter()
            .filter_map(|p| match p {
                Problem::Overlap {
                    a, b, shift, area, ..
                } => Some((*a, *b, *shift, *area)),
                _ => None,
            })
            .collect();
        assert_eq!(ov.len(), 1, "{:?}", r.problems);
        assert_eq!(ov[0].2, [1, 0]);
        assert!((ov[0].3 - 0.2).abs() < 1e-9);
    }

    #[test]
    fn catches_overlap_inside_cell() {
        let c = cell(
            [2.0, 0.0],
            [0.0, 1.0],
            vec![rect(0.0, 0.0, 1.2, 1.0), rect(1.0, 0.0, 1.0, 1.0)],
        );
        let r = check(&c, false);
        assert!(r.problems.iter().any(|p| matches!(
            p,
            Problem::Overlap {
                a: 0,
                b: 1,
                shift: [0, 0],
                ..
            }
        )));
        // And with the partner wrapped round: tile 1 moved left by a1 overlaps from the other side.
        let c2 = cell(
            [2.0, 0.0],
            [0.0, 1.0],
            vec![rect(0.0, 0.0, 1.0, 1.0), rect(0.9 - 2.0, 0.0, 1.0, 1.0)],
        );
        let r2 = check(&c2, false);
        assert!(!r2.ok());
        assert!(
            r2.problems
                .iter()
                .any(|p| matches!(p, Problem::Overlap { .. }))
        );
    }

    #[test]
    fn catches_gap_and_locates_it() {
        let c = cell([1.0, 0.0], [0.0, 1.0], vec![rect(0.0, 0.0, 0.8, 0.8)]);
        let r = check(&c, true);
        let (area, spots) = r
            .problems
            .iter()
            .find_map(|p| match p {
                Problem::Gap { area, spots } => Some((*area, spots.clone())),
                _ => None,
            })
            .expect("gap");
        assert!((area - 0.36).abs() < 1e-9);
        assert!(!spots.is_empty());
        for s in &spots {
            assert!(s[0] > 0.8 || s[1] > 0.8, "spot {s:?} is inside the tile");
        }
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::Unmatched { .. }))
        );
    }

    #[test]
    fn catches_thin_gap_by_open_edges() {
        // Two bricks with a sliver between them.
        let c = cell(
            [2.0, 0.0],
            [0.0, 1.0],
            vec![rect(0.0, 0.0, 0.999, 1.0), rect(1.0, 0.0, 1.0, 1.0)],
        );
        let r = check(&c, false);
        assert!(r.problems.iter().any(|p| matches!(p, Problem::Gap { .. })));
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::Unmatched { poly: 0, .. }))
        );
    }

    #[test]
    fn catches_self_intersection_and_degenerate() {
        let bow = vec![[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        let r = check(&cell([1.0, 0.0], [0.0, 1.0], vec![bow]), false);
        let at = r
            .problems
            .iter()
            .find_map(|p| match p {
                Problem::SelfIntersect { poly: 0, at } => Some(*at),
                _ => None,
            })
            .expect("self-intersection");
        assert!(dist(at, [0.5, 0.5]) < 1e-9);
        // Corner touching its own edge (pinched).
        let pinch = vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 0.0], [0.0, 2.0]];
        let r = check(&cell([3.0, 0.0], [0.0, 3.0], vec![pinch]), false);
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::SelfIntersect { .. }))
        );
        // Spike folding back on itself.
        let spike = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [1.0, 0.5], [0.0, 1.0]];
        let r = check(&cell([1.0, 0.0], [0.0, 1.0], vec![spike]), false);
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::SelfIntersect { .. }))
        );
        // Repeated corner, flat polygon, too few corners, parallel lattice.
        let rep = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let flat = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let r = check(
            &cell(
                [1.0, 0.0],
                [0.0, 1.0],
                vec![rep, flat, vec![[0.0, 0.0], [1.0, 1.0]]],
            ),
            false,
        );
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::ZeroEdge { poly: 0, .. }))
        );
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::NoArea { poly: 1 }))
        );
        assert!(
            r.problems
                .iter()
                .any(|p| matches!(p, Problem::TooFewCorners { poly: 2 }))
        );
        let r = check(
            &cell([1.0, 0.0], [2.0, 0.0], vec![rect(0.0, 0.0, 1.0, 1.0)]),
            false,
        );
        assert!(matches!(r.problems[..], [Problem::Lattice]));
    }

    #[test]
    fn bricks_offset_by_a_third() {
        // 3×1 bricks, each row shifted by 1: not edge to edge.
        let c = cell([3.0, 0.0], [1.0, 1.0], vec![rect(0.0, 0.0, 3.0, 1.0)]);
        let r = check(&c, true);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        // Left, right, two above, two below; corners land mid-edge so nothing touches only at a point.
        assert_eq!(degrees(&r), vec![(6, 6)]);
        let t = Topology::build(&TilingSpec {
            size: Some(SizeSpec::Rect { w: 10, h: 10 }),
            ..c.spec()
        })
        .unwrap();
        assert_eq!(t.degree_sets(), (vec![6], vec![6]));
        // Stacked squares (no offset) give the Moore neighbourhood instead.
        let sq = check(
            &cell([1.0, 0.0], [0.0, 1.0], vec![rect(0.0, 0.0, 1.0, 1.0)]),
            true,
        );
        assert_eq!(degrees(&sq), vec![(4, 8)]);
    }

    #[test]
    fn squares_of_two_sizes() {
        // Pythagorean tiling with sides 2 and 1.
        let c = cell(
            [2.0, 1.0],
            [-1.0, 2.0],
            vec![rect(0.0, 0.0, 2.0, 2.0), rect(2.0, 0.0, 1.0, 1.0)],
        );
        let r = check(&c, true);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(degrees(&r), vec![(8, 8), (4, 4)]);
        assert_eq!(r.degree_sets(), (vec![4, 8], vec![4, 8]));
        // Each small square's edge neighbours are all large squares.
        assert!(r.tiles[1].edge.iter().all(|c| c.poly == 0));
    }

    #[test]
    fn every_lattice_family_converts_and_checks_clean() {
        for f in families::FAMILIES {
            if f.kind == FamilyKind::Aperiodic {
                continue;
            }
            let c = UnitCell::from_spec(&TilingSpec::new(f.id))
                .unwrap_or_else(|e| panic!("{}: {e}", f.id));
            let r = check(&c, true);
            assert!(
                r.problems.is_empty(),
                "{}: {:?}",
                f.id,
                r.problems.iter().map(|p| p.message()).collect::<Vec<_>>()
            );
            // Round trip through inline parameters builds the same neighbour counts.
            let orig =
                Topology::build(&TilingSpec::new(f.id).with_size(SizeSpec::Rect { w: 10, h: 10 }))
                    .unwrap();
            let back = Topology::build(&TilingSpec {
                size: Some(SizeSpec::Rect { w: 10, h: 10 }),
                ..UnitCell::from_params(&c.params()).unwrap().spec()
            })
            .unwrap();
            assert_eq!(orig.degree_sets(), back.degree_sets(), "{}", f.id);
        }
        for t in crate::catalog::all() {
            let c = UnitCell::from_params(&json!({"catalog": t.id})).unwrap();
            assert!(check(&c, true).ok(), "{}", t.id);
        }
    }

    #[test]
    fn stamped_polygons_tile() {
        // Two triangles stamped on a square's edges, as the editor's stamp tool does.
        let sq = rect(0.0, 0.0, 1.0, 1.0);
        let tri = regular_polygon(sq[3], sq[2], 3, 1.0);
        assert!(signed_area2(&tri) > 0.0);
        assert!(dist(tri[1], sq[2]) < 1e-12);
        let hex = regular_polygon_at([0.0, 0.0], 6, 1.0, 0.0);
        assert!((dist(hex[0], hex[1]) - 1.0).abs() < 1e-12);
        // Elongated triangular tiling: square plus two triangles over it.
        let h = S3 / 2.0;
        let c = cell(
            [1.0, 0.0],
            [0.5, 1.0 + h],
            vec![
                sq.clone(),
                regular_polygon(sq[3], sq[2], 3, 1.0),
                vec![[1.0, 1.0], [1.5, 1.0 + h], [0.5, 1.0 + h]],
            ],
        );
        let r = check(&c, true);
        assert!(
            r.problems.is_empty(),
            "{:?}",
            r.problems.iter().map(|p| p.message()).collect::<Vec<_>>()
        );
    }
}
