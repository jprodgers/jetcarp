//! Exact neighbour graphs for patches of several prototiles (hat, Penrose), built from exact
//! placements ([`crate::exact::Place`]).
//!
//! Each prototile lists its corners in shape order; an edge on which other tiles' corners can land
//! (the hat's edge of length 2) is split at its midpoint, so the patch is edge to edge and
//! [`crate::exact::pair_edges`] matches every shared stretch exactly. Neighbour lists follow the
//! order of the float builder ([`crate::patch::build_graph`]): neighbours by their contact
//! position along the tile's boundary in shape order (corner `k` is position `k`; a stretch of
//! shared edge is its midpoint), ties by tile number, reversed for tiles listed clockwise.

use crate::exact::{
    Cyclo, Iso, OPEN, PatchStats, Place, Z4, around_vertex, pair_edges, verify_patch,
};
use crate::topology::Csr;
use std::marker::PhantomData;

/// A prototile in exact coordinates.
#[derive(Clone, Debug)]
pub struct ExactShape {
    /// Corners in shape order (the order of the float prototile).
    pub corners: Vec<Z4>,
    /// Edges split at their midpoint (corner `i` to corner `i + 1`).
    pub split: Vec<bool>,
}

/// A prototile's boundary vertices with the split points added, each labelled with its original
/// edge and its position along that edge in quarters (0 = the edge's first corner, 2 = midpoint).
struct Sub {
    verts: Vec<Z4>,
    label: Vec<(u8, u8)>,
    ccw: bool,
}

/// Exact neighbour-graph builder for a patch of placed prototiles.
pub struct ExactTiles<'a, C: Cyclo> {
    subs: Vec<Sub>,
    /// Per shape, per linear part (`refl * N + rot`): boundary vertices in counterclockwise order.
    lin: Vec<Vec<Vec<Z4>>>,
    stride: usize,
    pub shape: &'a [u16],
    pub places: &'a [Place],
    _c: PhantomData<C>,
}

impl<'a, C: Cyclo> ExactTiles<'a, C> {
    pub fn new(
        shapes: &[ExactShape],
        shape: &'a [u16],
        places: &'a [Place],
    ) -> Result<Self, String> {
        let mut subs = Vec::new();
        for s in shapes {
            let m = s.corners.len();
            let mut verts = Vec::new();
            let mut label = Vec::new();
            for i in 0..m {
                verts.push(s.corners[i]);
                label.push((i as u8, 0));
                if s.split[i] {
                    let sum = crate::exact::zadd(s.corners[i], s.corners[(i + 1) % m]);
                    if sum.iter().any(|x| x % 2 != 0) {
                        return Err("a split edge's midpoint is not a lattice point".into());
                    }
                    verts.push(sum.map(|x| x / 2));
                    label.push((i as u8, 2));
                }
            }
            // Orientation of the shape order, exactly: twice the signed area's sign.
            let mut a = [0i64; 4];
            for k in 0..verts.len() {
                a = crate::exact::zadd(a, C::mul(C::conj(verts[k]), verts[(k + 1) % verts.len()]));
            }
            let ccw = C::sign_im(a) == std::cmp::Ordering::Greater;
            subs.push(Sub { verts, label, ccw });
        }
        let stride = subs.iter().map(|s| s.verts.len()).max().unwrap_or(1);
        let n = C::N as usize;
        let lin = subs
            .iter()
            .map(|s| {
                (0..2 * n)
                    .map(|v| {
                        let iso = Iso {
                            rot: (v % n) as u8,
                            refl: v >= n,
                            t: [0; 4],
                        };
                        let mut w: Vec<Z4> =
                            s.verts.iter().map(|&z| iso.apply_vec::<C>(z)).collect();
                        if s.ccw == iso.refl {
                            // Listed clockwise: reverse, keeping vertex 0 first.
                            w = (0..w.len()).map(|k| w[(w.len() - k) % w.len()]).collect();
                        }
                        w
                    })
                    .collect()
            })
            .collect();
        Ok(ExactTiles {
            subs,
            lin,
            stride,
            shape,
            places,
            _c: PhantomData,
        })
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }
    #[inline]
    fn deg(&self, t: usize) -> usize {
        self.subs[self.shape[t] as usize].verts.len()
    }
    /// Is tile `t` listed counterclockwise in shape order?
    #[inline]
    fn ccw(&self, t: usize) -> bool {
        self.subs[self.shape[t] as usize].ccw != self.places[t].refl
    }
    /// Boundary vertex `k` of tile `t`, counterclockwise from shape vertex 0.
    #[inline]
    pub fn corner(&self, t: usize, k: usize) -> Z4 {
        let p = &self.places[t];
        let v =
            self.lin[self.shape[t] as usize][p.refl as usize * C::N as usize + p.rot as usize][k];
        [
            v[0] + p.t[0] as i64,
            v[1] + p.t[1] as i64,
            v[2] + p.t[2] as i64,
            v[3] + p.t[3] as i64,
        ]
    }
    /// Counterclockwise slot of shape-order boundary vertex `v` (and of the edge from it to the
    /// next vertex in shape order, `edge = true`).
    #[inline]
    fn slot(&self, t: usize, v: usize, edge: bool) -> usize {
        let d = self.deg(t);
        match (self.ccw(t), edge) {
            (true, _) => v,
            (false, false) => (d - v) % d,
            (false, true) => (2 * d - 1 - v) % d,
        }
    }

    pub fn pairs(&self) -> Result<Vec<u32>, String> {
        pair_edges(
            self.len(),
            self.stride,
            |t| self.deg(t),
            |t, k| self.corner(t, k),
        )
    }

    /// Check exactly that the patch tiles a disc (see [`crate::exact::verify_patch`]).
    pub fn verify(&self, partner: &[u32]) -> Result<PatchStats, String> {
        verify_patch::<C>(
            self.len(),
            self.stride,
            |t| self.deg(t),
            |t, k| self.corner(t, k),
            partner,
        )
    }

    /// Touching and edge neighbours (CSR) and open-boundary flags.
    pub fn graph(&self, partner: &[u32]) -> (Csr, Csr, Vec<bool>) {
        use rayon::prelude::*;
        let n = self.len();
        let st = self.stride;
        let deg = |t: usize| self.deg(t);
        let corner = |t: usize, k: usize| self.corner(t, k);
        let open: Vec<bool> = (0..n)
            .into_par_iter()
            .map(|t| (0..self.deg(t)).any(|k| partner[t * st + k] == OPEN))
            .collect();
        let chunk = 1 << 14;
        let mut touch = Csr {
            off: Vec::with_capacity(n + 1),
            nb: Vec::new(),
        };
        let mut edge = Csr {
            off: Vec::with_capacity(n + 1),
            nb: Vec::new(),
        };
        touch.off.push(0);
        edge.off.push(0);
        let chunks: Vec<usize> = (0..n.div_ceil(chunk)).collect();
        for batch in chunks.chunks(rayon::current_num_threads().max(1) * 4) {
            type Part = (Vec<u32>, Vec<u8>, Vec<u32>, Vec<u8>);
            let parts: Vec<Part> = batch
                .par_iter()
                .map(|&c| {
                    let (mut tl, mut tn, mut el, mut en) =
                        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                    let mut around = Vec::new();
                    // (neighbour, original edge, lo, hi) for edge contacts; (neighbour, pos) else.
                    let mut edges: Vec<(u32, u8, u8, u8)> = Vec::new();
                    let mut found: Vec<(u32, u32, bool)> = Vec::new();
                    for t in c * chunk..((c + 1) * chunk).min(n) {
                        let sub = &self.subs[self.shape[t] as usize];
                        let d = sub.verts.len();
                        edges.clear();
                        found.clear();
                        // Shared stretches, in shape order: the first original edge wins.
                        for e in 0..d {
                            let p = partner[t * st + self.slot(t, e, true)];
                            if p == OPEN {
                                continue;
                            }
                            let u = p / st as u32;
                            let (i, lo) = sub.label[e];
                            let hi = if sub.label[(e + 1) % d].0 == i {
                                sub.label[(e + 1) % d].1
                            } else {
                                4
                            };
                            match edges.iter_mut().find(|x| x.0 == u) {
                                Some(x) => {
                                    if x.1 == i && x.3 == lo {
                                        x.3 = hi;
                                    }
                                }
                                None => edges.push((u, i, lo, hi)),
                            }
                        }
                        for x in &edges {
                            found.push((x.0, 4 * x.1 as u32 + (x.2 + x.3) as u32 / 2, true));
                        }
                        // Corner contacts: tiles at a boundary vertex that share no edge.
                        for v in 0..d {
                            around_vertex(
                                t,
                                self.slot(t, v, false),
                                st,
                                &deg,
                                &corner,
                                partner,
                                &mut around,
                            );
                            let (i, q) = sub.label[v];
                            let pos = 4 * i as u32 + q as u32;
                            for &x in &around {
                                if edges.iter().any(|y| y.0 == x) {
                                    continue;
                                }
                                match found.iter_mut().find(|y| y.0 == x) {
                                    Some(y) => y.1 = y.1.min(pos),
                                    None => found.push((x, pos, false)),
                                }
                            }
                        }
                        found.sort_by_key(|x| (x.1, x.0));
                        if !self.ccw(t) {
                            found.reverse();
                        }
                        for &(x, _, is_edge) in &found {
                            tl.push(x);
                            if is_edge {
                                el.push(x);
                            }
                        }
                        tn.push(found.len() as u8);
                        en.push(edges.len() as u8);
                    }
                    (tl, tn, el, en)
                })
                .collect();
            for (tl, tn, el, en) in parts {
                for (csr, list, lens) in [(&mut touch, tl, tn), (&mut edge, el, en)] {
                    csr.nb.extend_from_slice(&list);
                    for k in lens {
                        let last = *csr.off.last().unwrap();
                        csr.off.push(last + k as u32);
                    }
                }
            }
        }
        (touch, edge, open)
    }
}
