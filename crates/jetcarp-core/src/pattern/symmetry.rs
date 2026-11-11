//! Symmetries of lattice tilings as maps on lattice cells, for canonical forms and orientations.
//!
//! On one-tile lattices (square, hexagon, ...) a symmetry is an integer 2x2 matrix on `(i, j)`
//! that preserves the neighbour offsets: exactly the list `objects::symmetries` uses, in the same
//! order. On lattices with several tile types per unit cell a symmetry also permutes the tile
//! types and shifts each by a lattice vector: `(i, j, k) -> (M (i, j) + off[k], perm[k])`. These
//! are found geometrically: an integer matrix `M` whose real form `A M A^-1` (`A` = the lattice
//! vectors) is orthogonal, and a translation, such that every tile's polygon lands exactly on a
//! tile's polygon; the map must also carry every neighbour slot to a neighbour slot. So on 4.8.8,
//! 3.6.3.6, and the other Archimedean and k-uniform tilings a pattern found in another
//! orientation gets the same canonical form.

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
    for a in -2i32..=2 {
        for b in -2..=2 {
            for c in -2..=2 {
                for d in -2..=2 {
                    if (a * d - b * c).abs() != 1 || [a, b, c, d] == [1, 0, 0, 1] {
                        continue;
                    }
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
                        continue;
                    }
                    // Translations taking tile 0's centroid to each tile type's centroid.
                    for k0 in 0..n {
                        let rc0 = r(cents[0]);
                        let t = [cents[k0][0] - rc0[0], cents[k0][1] - rc0[1]];
                        if let Some(map) =
                            try_map(&polys, &cents, [a, b, c, d], &r, t, &to_lattice, near_int)
                            && preserves_neighbours(g, nb, &map)
                            && !out.contains(&map)
                        {
                            out.push(map);
                        }
                    }
                }
            }
        }
    }
    out
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
