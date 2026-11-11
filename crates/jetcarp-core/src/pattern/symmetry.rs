//! Symmetries of lattice tilings as maps on lattice cells, for canonical forms and orientations.
//!
//! On one-tile lattices (square, hexagon, ...) a symmetry is an integer 2x2 matrix on `(i, j)`
//! that preserves the neighbour offsets: exactly the list `objects::symmetries` uses, in the same
//! order (the neighbour graph's symmetries, which may be more than the geometric ones: the brick
//! wall's graph is the hexagonal grid's). On lattices with several tile types per unit cell a
//! symmetry also permutes the tile types and shifts each by a lattice vector:
//! `(i, j, k) -> (M (i, j) + off[k], perm[k])`. These are found geometrically: an integer matrix
//! `M` whose real form `A M A^-1` (`A` = the lattice vectors) is orthogonal, and a translation,
//! such that every tile's polygon lands exactly on a tile's polygon; the map must also carry every
//! neighbour slot to a neighbour slot. So on 4.8.8, 3.6.3.6, and the other Archimedean and
//! k-uniform tilings a pattern found in another orientation gets the same canonical form.
//!
//! **What is found, and why it is all of it.** Every symmetry whose rotation part maps the cell's
//! lattice onto itself, modulo lattice translations:
//!
//! - its matrix: the rotation part maps `a1` to a lattice vector of length `|a1|` and `a2` to one
//!   of length `|a2|` with the angle between them kept, and by Cramer's rule a lattice vector `v`
//!   has coordinates `|x| <= |v| |a2| / |det|`, `|y| <= |a1| |v| / |det|`, so
//!   [`lattice_isometries`] lists every candidate (no fixed range);
//! - its translation: after a lattice translation (which canonical forms ignore) a symmetry
//!   carries tile 0 to a tile type of cell (0, 0), and each of those is tried; the identity matrix
//!   included, which gives the translations inside a cell that holds its tile pattern more than
//!   once (the glide family's rectangles, IH58's squares);
//! - and the set is then closed under composition, a safety net (a composition of symmetries is
//!   one).
//!
//! Tests check, on every periodic tiling JETCARP builds (the 1,254 of the catalog, the other
//! families at their default shapes, the 81 isohedral types; both neighbourhoods), that the maps
//! are closed under composition modulo lattice translations, and that they are every symmetry an
//! independent scan finds from the tiles' corners. A symmetry whose rotation part does *not* map
//! the cell's lattice onto itself cannot be a cell map at all; that happens only when the cell
//! holds its tile pattern more than once (the quarter turn of IH58's squares, the sixth turns of
//! flat-top hexagons in their rectangle), and canonical forms then use the whole group as
//! isometries ([`super::group`]).
//!
//! The first search, matrices with entries from -2 to 2, runs first and keeps its order: a
//! scene's `PastePattern` names an orientation by its index in this list. What the later
//! searches add comes after it.

use crate::geom::{P, centroid};
use crate::objects::LCell;
use crate::topology::{Nb, Topology};

/// `(i, j, k) -> (M (i, j) + off[k], perm[k])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellMap {
    /// Row-major `[a, b, c, d]`: `i' = a i + b j`, `j' = c i + d j`.
    pub m: [i32; 4],
    pub perm: Vec<u32>,
    pub off: Vec<[i32; 2]>,
}

impl CellMap {
    pub fn identity(ntypes: u32) -> CellMap {
        CellMap {
            m: [1, 0, 0, 1],
            perm: (0..ntypes).collect(),
            off: vec![[0, 0]; ntypes as usize],
        }
    }
    pub fn apply(&self, c: &LCell) -> LCell {
        let m = &self.m;
        let k = c.k as usize;
        let (o, p) = match (self.off.get(k), self.perm.get(k)) {
            (Some(o), Some(&p)) => (*o, p),
            _ => ([0, 0], c.k),
        };
        LCell {
            i: m[0] * c.i + m[1] * c.j + o[0],
            j: m[2] * c.i + m[3] * c.j + o[1],
            k: p,
            s: c.s,
        }
    }
    /// The map on lattice vectors (displacements).
    pub fn vector(&self, d: [i32; 2]) -> [i32; 2] {
        let m = &self.m;
        [m[0] * d[0] + m[1] * d[1], m[2] * d[0] + m[3] * d[1]]
    }
}

/// The symmetries of the tiling (identity first). With `isotropic` false (rule tables), only the
/// identity.
pub fn cell_maps(topo: &Topology, nb: Nb, isotropic: bool) -> Vec<CellMap> {
    let Some(g) = topo.lattice() else {
        return vec![CellMap::identity(1)];
    };
    if !isotropic {
        return vec![CellMap::identity(g.ntypes)];
    }
    if g.ntypes == 1 {
        return crate::objects::symmetries(g, nb)
            .into_iter()
            .map(|m| CellMap {
                m,
                perm: vec![0],
                off: vec![[0, 0]],
            })
            .collect();
    }
    let n = g.ntypes as usize;
    let (a1, a2) = (g.a1, g.a2);
    let det = a1[0] * a2[1] - a2[0] * a1[1];
    // World point -> lattice coordinates.
    let to_lattice = |p: P| -> [f64; 2] {
        [
            (p[0] * a2[1] - p[1] * a2[0]) / det,
            (a1[0] * p[1] - a1[1] * p[0]) / det,
        ]
    };
    let polys: Vec<Vec<P>> = g
        .types
        .iter()
        .map(|(m, s)| topo.shapes[*s].iter().map(|&p| m.apply(p)).collect())
        .collect();
    let cents: Vec<P> = polys.iter().map(|p| centroid(p)).collect();
    let near_int = |x: f64| (x - x.round()).abs() < 1e-6;
    let mut out = vec![CellMap::identity(g.ntypes)];
    // Every map with this linear part: tile 0 carried to each tile type of cell (0, 0) in turn
    // (any symmetry does that after a lattice translation, which canonical forms ignore).
    let with_matrix = |[a, b, c, d]: [i32; 4], out: &mut Vec<CellMap>| {
        // Real linear map R = A M A^-1 (columns of A are a1, a2).
        let r = |p: P| -> P {
            let l = to_lattice(p);
            let (li, lj) = (
                a as f64 * l[0] + b as f64 * l[1],
                c as f64 * l[0] + d as f64 * l[1],
            );
            [li * a1[0] + lj * a2[0], li * a1[1] + lj * a2[1]]
        };
        let (e1, e2) = (r([1.0, 0.0]), r([0.0, 1.0]));
        let orth = (e1[0] * e1[0] + e1[1] * e1[1] - 1.0).abs() < 1e-6
            && (e2[0] * e2[0] + e2[1] * e2[1] - 1.0).abs() < 1e-6
            && (e1[0] * e2[0] + e1[1] * e2[1]).abs() < 1e-6;
        if !orth {
            return;
        }
        for k0 in 0..n {
            let rc0 = r(cents[0]);
            let t = [cents[k0][0] - rc0[0], cents[k0][1] - rc0[1]];
            if let Some(map) = try_map(&polys, &cents, [a, b, c, d], &r, t, &to_lattice, near_int)
                && preserves_neighbours(g, nb, &map)
                && !out.contains(&map)
            {
                out.push(map);
            }
        }
    };
    // The first search, kept first and in its order: scenes name orientations by their index.
    for a in -2i32..=2 {
        for b in -2..=2 {
            for c in -2..=2 {
                for d in -2..=2 {
                    if (a * d - b * c).abs() != 1 || [a, b, c, d] == [1, 0, 0, 1] {
                        continue;
                    }
                    with_matrix([a, b, c, d], &mut out);
                }
            }
        }
    }
    // Every linear part a symmetry can have (see the module notes), the identity included: its
    // maps are the translations inside a cell that holds the pattern of tiles more than once.
    for m in lattice_isometries(a1, a2) {
        with_matrix(m, &mut out);
    }
    close(&mut out);
    out
}

/// The integer matrices `M` (row-major, on lattice coordinates) whose real form `A M A^-1` is an
/// isometry, for lattice vectors `a1`, `a2`: the linear part of every symmetry that maps the
/// lattice onto itself. Such an isometry maps `a1` to a lattice vector `x a1 + y a2` of length
/// `|a1|` (and `a2` to one of length `|a2|`, with the angle kept), and by Cramer's rule a lattice
/// vector `v` has `|x| <= |v| |a2| / |det|` and `|y| <= |a1| |v| / |det|`, so the search below is
/// complete.
pub fn lattice_isometries(a1: P, a2: P) -> Vec<[i32; 4]> {
    let det = (a1[0] * a2[1] - a2[0] * a1[1]).abs();
    let norm = |v: P| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let (l1, l2) = (norm(a1), norm(a2));
    let tol = 1e-6 * (1.0 + l1.max(l2));
    let at = |x: i32, y: i32| -> P {
        [
            x as f64 * a1[0] + y as f64 * a2[0],
            x as f64 * a1[1] + y as f64 * a2[1],
        ]
    };
    // Lattice vectors of length `len`.
    let shell = |len: f64| -> Vec<[i32; 2]> {
        let bx = (len * l2 / det + 1e-9).ceil() as i32;
        let by = (len * l1 / det + 1e-9).ceil() as i32;
        let mut v = Vec::new();
        for x in -bx..=bx {
            for y in -by..=by {
                if (norm(at(x, y)) - len).abs() < tol {
                    v.push([x, y]);
                }
            }
        }
        v
    };
    let dot = a1[0] * a2[0] + a1[1] * a2[1];
    let mut out = Vec::new();
    for c1 in shell(l1) {
        for c2 in shell(l2) {
            // Columns: the images of (1, 0) and (0, 1).
            let m = [c1[0], c2[0], c1[1], c2[1]];
            if (m[0] * m[3] - m[1] * m[2]).abs() != 1 {
                continue;
            }
            let (v1, v2) = (at(c1[0], c1[1]), at(c2[0], c2[1]));
            if (v1[0] * v2[0] + v1[1] * v2[1] - dot).abs() < tol * (1.0 + l1 + l2) {
                out.push(m);
            }
        }
    }
    out
}

/// `a` after `b`.
pub fn compose(a: &CellMap, b: &CellMap) -> CellMap {
    let n = b.perm.len();
    let mut perm = vec![0; n];
    let mut off = vec![[0, 0]; n];
    for k in 0..n {
        let k2 = b.perm[k] as usize;
        let mo = a.vector(b.off[k]);
        off[k] = [mo[0] + a.off[k2][0], mo[1] + a.off[k2][1]];
        perm[k] = a.perm[k2];
    }
    let (m, q) = (&a.m, &b.m);
    CellMap {
        m: [
            m[0] * q[0] + m[1] * q[2],
            m[0] * q[1] + m[1] * q[3],
            m[2] * q[0] + m[3] * q[2],
            m[2] * q[1] + m[3] * q[3],
        ],
        perm,
        off,
    }
}

/// The map moved by a whole lattice vector so that tile 0 lands in cell (0, 0), as the search
/// writes every map (canonical forms ignore lattice translations, so this is the same
/// symmetry).
fn normal(mut m: CellMap) -> CellMap {
    let o = m.off[0];
    for x in &mut m.off {
        x[0] -= o[0];
        x[1] -= o[1];
    }
    m
}

/// Add every composition of maps in the set until it is a group (modulo lattice translations).
/// A composition of automorphisms of the tiling is one, so this only adds true symmetries; the
/// search should already have found them all, and this is its safety net.
fn close(out: &mut Vec<CellMap>) {
    let mut i = 0;
    while i < out.len() {
        let mut j = 0;
        while j < out.len() {
            let z = normal(compose(&out[i], &out[j]));
            if !out.contains(&z) {
                out.push(z);
            }
            j += 1;
        }
        i += 1;
    }
}

fn try_map(
    polys: &[Vec<P>],
    cents: &[P],
    m: [i32; 4],
    r: &dyn Fn(P) -> P,
    t: P,
    to_lattice: &dyn Fn(P) -> [f64; 2],
    near_int: impl Fn(f64) -> bool,
) -> Option<CellMap> {
    let n = cents.len();
    let mut perm = vec![u32::MAX; n];
    let mut off = vec![[0, 0]; n];
    for k in 0..n {
        let rc = r(cents[k]);
        let p = [rc[0] + t[0], rc[1] + t[1]];
        let mut hit = None;
        for (k2, c2) in cents.iter().enumerate() {
            let l = to_lattice([p[0] - c2[0], p[1] - c2[1]]);
            if near_int(l[0]) && near_int(l[1]) {
                hit = Some((k2, [l[0].round() as i32, l[1].round() as i32]));
                break;
            }
        }
        let (k2, o) = hit?;
        // The polygon must land on that tile's polygon (same corners), up to the lattice shift o.
        let shift = [p[0] - cents[k2][0], p[1] - cents[k2][1]];
        let moved: Vec<P> = polys[k]
            .iter()
            .map(|&q| {
                let rq = r(q);
                [rq[0] + t[0] - shift[0], rq[1] + t[1] - shift[1]]
            })
            .collect();
        let same = moved.len() == polys[k2].len()
            && moved.iter().all(|q| {
                polys[k2]
                    .iter()
                    .any(|v| (v[0] - q[0]).abs() < 1e-6 && (v[1] - q[1]).abs() < 1e-6)
            });
        if !same || perm.contains(&(k2 as u32)) {
            return None;
        }
        perm[k] = k2 as u32;
        off[k] = o;
    }
    Some(CellMap { m, perm, off })
}

fn preserves_neighbours(g: &crate::topology::LatticeGrid, nb: Nb, map: &CellMap) -> bool {
    let offs = &g.offsets[nb as usize];
    (0..g.ntypes).all(|k| {
        let a = map.apply(&LCell {
            i: 0,
            j: 0,
            k,
            s: 1,
        });
        offs[k as usize].iter().all(|o| {
            let b = map.apply(&LCell {
                i: o.di,
                j: o.dj,
                k: o.k,
                s: 1,
            });
            let (di, dj) = (b.i - a.i, b.j - a.j);
            offs[a.k as usize]
                .iter()
                .any(|q| q.di == di && q.dj == dj && q.k == b.k)
        })
    })
}
