//! Large sandpiles on periodic tilings: a growing box of lattice unit cells, one byte per cell,
//! toppled in tiles with dirty flags.
//!
//! **Layout.** The box covers lattice coordinates `i0 ≤ i < i0 + W`, `j0 ≤ j < j0 + H` (unsheared,
//! so the box is a parallelogram on screen), with `nt` tiles per unit cell. Heights are stored as
//! `nt` planes of `ph` rows of `pw` bytes: the logical box plus [`PAD`] zero cells on every side,
//! so neighbour reads never need bounds checks. `W` and `H` are multiples of [`TILE`].
//!
//! **Toppling.** Synchronous sweeps (every unstable cell topples once per sweep, as Jacobi), which
//! are deterministic and give the same sequence of states on any thread count and on the GPU.
//! Sweeps run in batches of `T` per tile: a tile is copied out with a halo of `T × reach` cells,
//! swept `T` times in cache, and its interior (which is exact after `T` sweeps) written to the
//! back buffer. Only tiles with an unstable cell, and their neighbours, are processed.

use super::{Ctl, StabStats, Tick};
use crate::geom::*;
use crate::topology::{Boundary, Nb, Offset, SizeSpec, TilingSpec, Topology};
use rayon::prelude::*;
use std::sync::Arc;
use std::time::Instant;

/// Unit cells per tile side.
pub const TILE: usize = 128;
/// Zero cells around the box (at least the largest halo).
pub const PAD: usize = 8;

/// The geometry and neighbour table of a periodic tiling, for one neighbourhood.
#[derive(Clone, Debug)]
pub struct Lattice {
    pub spec: TilingSpec,
    pub nb: Nb,
    pub nt: usize,
    pub a1: P,
    pub a2: P,
    /// Per tile type: transform from shape coordinates and the shape index.
    pub types: Vec<(Affine, usize)>,
    /// Straight corner polygons per shape.
    pub shapes: Vec<Vec<P>>,
    /// Drawn outlines per shape.
    pub outlines: Vec<Vec<P>>,
    /// `offs[type]`: neighbour slots.
    pub offs: Vec<Vec<Offset>>,
    /// Degree (toppling threshold) per tile type.
    pub deg: Vec<u8>,
    /// Largest |di| or |dj| over the offsets.
    pub reach: usize,
}

impl Lattice {
    pub fn new(spec: &TilingSpec, nb: Nb) -> Result<Lattice, String> {
        let mut s = spec.clone();
        s.size = Some(SizeSpec::Rect { w: 12, h: 12 });
        s.boundary = Boundary::Dead;
        let topo = Topology::build(&s)?;
        let g = topo
            .lattice()
            .ok_or("not a periodic tiling (aperiodic patches use the patch board)")?;
        let offs = g.offsets[nb as usize].clone();
        let deg: Vec<u8> = offs
            .iter()
            .map(|o| u8::try_from(o.len()).unwrap_or(255))
            .collect();
        if deg.iter().any(|&d| d == 0 || d > 120) {
            return Err("tile degree out of range for 1-byte heights (1 to 120)".into());
        }
        let reach = offs
            .iter()
            .flatten()
            .map(|o| o.di.unsigned_abs().max(o.dj.unsigned_abs()) as usize)
            .max()
            .unwrap_or(1)
            .max(1);
        if reach > PAD {
            return Err(format!("neighbour reach {reach} is larger than {PAD}"));
        }
        Ok(Lattice {
            spec: spec.clone(),
            nb,
            nt: g.ntypes as usize,
            a1: g.a1,
            a2: g.a2,
            types: g.types.clone(),
            shapes: topo.shapes.clone(),
            outlines: topo.outlines.clone(),
            offs,
            deg,
            reach,
        })
    }
    pub fn max_degree(&self) -> u8 {
        self.deg.iter().copied().max().unwrap_or(1)
    }
    /// Transform of cell `(i, j, k)` from shape coordinates to the world.
    pub fn transform(&self, i: i64, j: i64, k: usize) -> Affine {
        let t = [
            i as f64 * self.a1[0] + j as f64 * self.a2[0],
            i as f64 * self.a1[1] + j as f64 * self.a2[1],
        ];
        Affine::trans(t[0], t[1]).mul(&self.types[k].0)
    }
    /// Straight corner polygon of a cell.
    pub fn polygon(&self, i: i64, j: i64, k: usize) -> Vec<P> {
        let m = self.transform(i, j, k);
        self.shapes[self.types[k].1]
            .iter()
            .map(|&p| m.apply(p))
            .collect()
    }
    /// Mean tile area (world units).
    pub fn cell_area(&self) -> f64 {
        cross(self.a1, self.a2).abs() / self.nt as f64
    }
    /// Sweeps per tile batch and the halo they need.
    pub fn batch(&self) -> (usize, usize) {
        let t = (PAD / self.reach).max(1);
        (t, t * self.reach)
    }
}

/// A growing box of heights on a periodic tiling.
pub struct LatticeBoard {
    pub lat: Arc<Lattice>,
    /// Lattice coordinates of logical cell (0, 0).
    pub i0: i64,
    pub j0: i64,
    /// Tiles across and down.
    pub tx: usize,
    pub ty: usize,
    /// Physical row length and rows per plane (logical size plus `2 × PAD`).
    pub pw: usize,
    pub ph: usize,
    /// Heights, `nt` planes of `ph × pw`.
    pub cur: Vec<u8>,
    nxt: Vec<u8>,
    /// Tiles holding an unstable cell.
    unstable: Vec<bool>,
    /// Tiles processed in the previous batch (their back buffer may differ from the front).
    prev: Vec<bool>,
}

struct OutPtr(*mut u8);
unsafe impl Send for OutPtr {}
unsafe impl Sync for OutPtr {}
impl OutPtr {
    fn at(&self, i: usize) -> *mut u8 {
        // SAFETY: callers stay inside the buffer.
        unsafe { self.0.add(i) }
    }
}

#[derive(Default, Clone, Copy)]
struct TileOut {
    fired: u64,
    /// Last local sweep in which an interior cell toppled (-1 = none).
    last: i32,
    unstable: bool,
}

impl LatticeBoard {
    /// A 3 × 3 tile box with lattice cell (0, 0) in the middle tile.
    pub fn new(lat: Arc<Lattice>) -> LatticeBoard {
        let off = (TILE + TILE / 2) as i64;
        LatticeBoard::with_box(lat, -off, -off, 3, 3)
    }
    pub fn with_box(lat: Arc<Lattice>, i0: i64, j0: i64, tx: usize, ty: usize) -> LatticeBoard {
        let pw = tx * TILE + 2 * PAD;
        let ph = ty * TILE + 2 * PAD;
        let n = lat.nt * pw * ph;
        LatticeBoard {
            lat,
            i0,
            j0,
            tx,
            ty,
            pw,
            ph,
            cur: vec![0; n],
            nxt: vec![0; n],
            unstable: vec![false; tx * ty],
            prev: vec![false; tx * ty],
        }
    }
    /// Logical width and height in unit cells.
    pub fn dims(&self) -> (usize, usize) {
        (self.tx * TILE, self.ty * TILE)
    }
    /// Cells in the box.
    pub fn cells(&self) -> u64 {
        (self.tx * self.ty * TILE * TILE * self.lat.nt) as u64
    }
    /// Bytes held (both buffers).
    pub fn bytes(&self) -> u64 {
        (self.cur.len() + self.nxt.len()) as u64
    }
    #[inline]
    fn phys(&self, k: usize, lj: usize, li: usize) -> usize {
        (k * self.ph + lj + PAD) * self.pw + li + PAD
    }
    /// Physical index of lattice cell `(i, j, k)`, if inside the box.
    pub fn index(&self, i: i64, j: i64, k: usize) -> Option<usize> {
        let (w, h) = self.dims();
        let (li, lj) = (i - self.i0, j - self.j0);
        (li >= 0 && lj >= 0 && (li as usize) < w && (lj as usize) < h && k < self.lat.nt)
            .then(|| self.phys(k, lj as usize, li as usize))
    }
    pub fn get(&self, i: i64, j: i64, k: usize) -> u8 {
        self.index(i, j, k).map_or(0, |x| self.cur[x])
    }
    /// A row of logical cells: plane `k`, lattice row `j`, lattice columns `i0 ..`.
    pub fn row(&self, k: usize, j: i64) -> Option<&[u8]> {
        let lj = j - self.j0;
        if lj < 0 || lj as usize >= self.dims().1 {
            return None;
        }
        let s = self.phys(k, lj as usize, 0);
        Some(&self.cur[s..s + self.dims().0])
    }
    /// Add grains to a cell (growing the box if it lies outside).
    pub fn add(&mut self, i: i64, j: i64, k: usize, n: u8) {
        while self.index(i, j, k).is_none() {
            let (w, h) = self.dims();
            let l = (i < self.i0 + TILE as i64) as usize;
            let r = (i >= self.i0 + w as i64 - TILE as i64) as usize;
            let t = (j < self.j0 + TILE as i64) as usize;
            let b = (j >= self.j0 + h as i64 - TILE as i64) as usize;
            self.grow(l.max(1), r.max(1), t.max(1), b.max(1));
        }
        let x = self.index(i, j, k).unwrap();
        self.cur[x] = self.cur[x].wrapping_add(n);
        self.nxt[x] = self.cur[x];
    }
    /// Double every height (the start of a doubling step).
    pub fn double(&mut self) {
        self.cur
            .par_chunks_mut(1 << 16)
            .for_each(|c| c.iter_mut().for_each(|h| *h = h.wrapping_mul(2)));
        self.sync();
    }
    /// After `cur` changed outside the toppler (doubling, adding, the GPU): copy it to the back
    /// buffer and recompute which tiles are unstable.
    pub fn sync(&mut self) {
        self.nxt
            .par_chunks_mut(1 << 16)
            .enumerate()
            .for_each(|(c, d)| {
                let s = c << 16;
                d.copy_from_slice(&self.cur[s..s + d.len()]);
            });
        let flags: Vec<bool> = (0..self.tx * self.ty)
            .into_par_iter()
            .map(|t| self.tile_unstable(t % self.tx, t / self.tx))
            .collect();
        self.unstable = flags;
        self.prev.iter_mut().for_each(|p| *p = false);
    }
    fn tile_unstable(&self, tx: usize, ty: usize) -> bool {
        (0..self.lat.nt).any(|k| {
            let d = self.lat.deg[k];
            (0..TILE).any(|y| {
                let s = self.phys(k, ty * TILE + y, tx * TILE);
                self.cur[s..s + TILE].iter().any(|&h| h >= d)
            })
        })
    }
    /// Any unstable cell?
    pub fn is_unstable(&self) -> bool {
        self.unstable.iter().any(|&u| u)
    }
    /// Total grains.
    pub fn grains(&self) -> u64 {
        self.cur
            .par_chunks(1 << 16)
            .map(|c| c.iter().map(|&h| h as u64).sum::<u64>())
            .sum()
    }
    /// Bounding box of nonzero cells in lattice coordinates `[imin, jmin, imax, jmax]` (inclusive).
    pub fn nonzero_box(&self) -> Option<[i64; 4]> {
        let (w, h) = self.dims();
        let rows: Vec<Option<(usize, usize, usize)>> = (0..h)
            .into_par_iter()
            .map(|lj| {
                let mut lo = usize::MAX;
                let mut hi = 0;
                for k in 0..self.lat.nt {
                    let s = self.phys(k, lj, 0);
                    let row = &self.cur[s..s + w];
                    if let Some(a) = row.iter().position(|&x| x != 0) {
                        lo = lo.min(a);
                        hi = hi.max(row.iter().rposition(|&x| x != 0).unwrap());
                    }
                }
                (lo != usize::MAX).then_some((lj, lo, hi))
            })
            .collect();
        let mut b: Option<[i64; 4]> = None;
        for (lj, lo, hi) in rows.into_iter().flatten() {
            let (i_lo, i_hi, j) = (
                self.i0 + lo as i64,
                self.i0 + hi as i64,
                self.j0 + lj as i64,
            );
            b = Some(match b {
                None => [i_lo, j, i_hi, j],
                Some(o) => [o[0].min(i_lo), o[1].min(j), o[2].max(i_hi), o[3].max(j)],
            });
        }
        b
    }

    /// Add tiles on each side (left, right, top, bottom). Both buffers end equal.
    pub fn grow(&mut self, l: usize, r: usize, t: usize, b: usize) {
        let mut nb = LatticeBoard::with_box(
            self.lat.clone(),
            self.i0 - (l * TILE) as i64,
            self.j0 - (t * TILE) as i64,
            self.tx + l + r,
            self.ty + t + b,
        );
        let (w, h) = self.dims();
        for k in 0..self.lat.nt {
            for lj in 0..h {
                let s = self.phys(k, lj, 0);
                let d = nb.phys(k, lj + t * TILE, l * TILE);
                nb.cur[d..d + w].copy_from_slice(&self.cur[s..s + w]);
            }
        }
        for ty in 0..self.ty {
            for tx in 0..self.tx {
                nb.unstable[(ty + t) * nb.tx + tx + l] = self.unstable[ty * self.tx + tx];
            }
        }
        nb.nxt.copy_from_slice(&nb.cur);
        *self = nb;
    }

    /// Grow if an unstable tile sits in the outer ring of tiles. Returns true if it grew.
    pub fn grow_if_needed(&mut self) -> bool {
        let (tx, ty) = (self.tx, self.ty);
        let u = |x: usize, y: usize| self.unstable[y * tx + x];
        let l = (0..ty).any(|y| u(0, y));
        let r = (0..ty).any(|y| u(tx - 1, y));
        let t = (0..tx).any(|x| u(x, 0));
        let b = (0..tx).any(|x| u(x, ty - 1));
        if !(l || r || t || b) {
            return false;
        }
        let gx = (tx / 4).max(1);
        let gy = (ty / 4).max(1);
        self.grow(
            l as usize * gx,
            r as usize * gx,
            t as usize * gy,
            b as usize * gy,
        );
        true
    }

    /// Grow so that every nonzero cell is at least `margin` cells from the box edge.
    pub fn ensure_margin(&mut self, margin: usize) {
        let Some(bx) = self.nonzero_box() else { return };
        let (w, h) = self.dims();
        let need = |gap: i64| -> usize {
            if gap >= margin as i64 {
                0
            } else {
                (margin as i64 - gap).max(0) as usize / TILE + 1
            }
        };
        let l = need(bx[0] - self.i0);
        let r = need(self.i0 + w as i64 - 1 - bx[2]);
        let t = need(bx[1] - self.j0);
        let b = need(self.j0 + h as i64 - 1 - bx[3]);
        if l + r + t + b > 0 {
            let gx = (self.tx / 4).max(1);
            let gy = (self.ty / 4).max(1);
            let f = |n: usize, g: usize| if n > 0 { n.max(g) } else { 0 };
            self.grow(f(l, gx), f(r, gx), f(t, gy), f(b, gy));
        }
    }

    /// Topple until stable on the CPU.
    pub fn stabilize(&mut self, ctl: &mut Ctl) -> Result<StabStats, String> {
        let (t_steps, halo) = self.lat.batch();
        let mut st = StabStats::default();
        let start = Instant::now();
        let mut last_tick = Instant::now();
        loop {
            if ctl.cancelled() {
                return Err("cancelled".into());
            }
            if !self.is_unstable() {
                break;
            }
            if self.grow_if_needed() {
                st.grows += 1;
                continue;
            }
            let (fired, sweeps) = self.batch(t_steps, halo);
            st.topplings += fired;
            st.sweeps += sweeps as u64;
            if last_tick.elapsed().as_secs_f64() >= ctl.tick_every {
                last_tick = Instant::now();
                let tick = Tick {
                    sweeps: st.sweeps,
                    topplings: st.topplings,
                    seconds: start.elapsed().as_secs_f64(),
                };
                ctl.tick(&tick, super::BoardRef::Lattice(self));
            }
        }
        Ok(st)
    }

    /// One batch of `t_steps` sweeps. Returns (topplings, sweeps that toppled something).
    fn batch(&mut self, t_steps: usize, halo: usize) -> (u64, usize) {
        let (tx, ty) = (self.tx, self.ty);
        // Tiles to process: unstable tiles and their neighbours, plus last batch's set (whose
        // back buffer may still hold older values).
        let mut active = vec![false; tx * ty];
        for y in 0..ty {
            for x in 0..tx {
                if self.unstable[y * tx + x] {
                    for yy in y.saturating_sub(1)..(y + 2).min(ty) {
                        for xx in x.saturating_sub(1)..(x + 2).min(tx) {
                            active[yy * tx + xx] = true;
                        }
                    }
                }
            }
        }
        let list: Vec<usize> = (0..tx * ty)
            .filter(|&t| active[t] || self.prev[t])
            .collect();
        let out = OutPtr(self.nxt.as_mut_ptr());
        let this = &*self;
        let lw = TILE + 2 * halo;
        let plane = lw * lw;
        let n_local = this.lat.nt * plane;
        let res: Vec<TileOut> = list
            .par_iter()
            .map_init(
                || (vec![0u8; n_local], vec![0u8; n_local]),
                |(a, b), &t| {
                    let o = this.run_tile(t % tx, t / tx, t_steps, halo, a, b);
                    // Scatter the exact interior into the back buffer.
                    let (tx0, ty0) = ((t % tx) * TILE, (t / tx) * TILE);
                    for k in 0..this.lat.nt {
                        for y in 0..TILE {
                            let s = k * plane + (y + halo) * lw + halo;
                            let d = this.phys(k, ty0 + y, tx0);
                            // SAFETY: tiles cover disjoint cells of the back buffer.
                            unsafe {
                                std::ptr::copy_nonoverlapping(a.as_ptr().add(s), out.at(d), TILE);
                            }
                        }
                    }
                    o
                },
            )
            .collect();
        std::mem::swap(&mut self.cur, &mut self.nxt);
        let mut fired = 0u64;
        let mut last = -1i32;
        let mut new_unstable = vec![false; tx * ty];
        for (&t, o) in list.iter().zip(&res) {
            fired += o.fired;
            last = last.max(o.last);
            new_unstable[t] = o.unstable;
        }
        let any = new_unstable.iter().any(|&u| u);
        self.unstable = new_unstable;
        self.prev = active;
        let sweeps = if any { t_steps } else { (last + 1) as usize };
        (fired, sweeps)
    }

    /// Copy a tile and its halo into `a`, sweep it `t_steps` times (ping-pong with `b`); the
    /// result is left in `a`.
    fn run_tile(
        &self,
        tx: usize,
        ty: usize,
        t_steps: usize,
        halo: usize,
        a: &mut Vec<u8>,
        b: &mut Vec<u8>,
    ) -> TileOut {
        let lat = &*self.lat;
        let nt = lat.nt;
        let r = lat.reach;
        let lw = TILE + 2 * halo;
        let plane = lw * lw;
        // Gather.
        for k in 0..nt {
            for y in 0..lw {
                let s =
                    (k * self.ph + ty * TILE + y + PAD - halo) * self.pw + tx * TILE + PAD - halo;
                a[k * plane + y * lw..k * plane + (y + 1) * lw]
                    .copy_from_slice(&self.cur[s..s + lw]);
            }
        }
        b.copy_from_slice(a);
        // The outer `r` cells are never swept; if one is unstable the region is never a fixed
        // point, so early exit is allowed only when they are all stable.
        let ring_unstable = (0..nt).any(|k| {
            let d = lat.deg[k];
            (0..lw).any(|y| {
                let row = &a[k * plane + y * lw..k * plane + (y + 1) * lw];
                if y < r || y >= lw - r {
                    row.iter().any(|&h| h >= d)
                } else {
                    row[..r].iter().any(|&h| h >= d) || row[lw - r..].iter().any(|&h| h >= d)
                }
            })
        });
        let mut out = sweeps(lat, a, b, lw, t_steps, halo, ring_unstable);
        out.unstable = (0..nt).any(|k| {
            let d = lat.deg[k];
            (halo..halo + TILE).any(|y| {
                let s = k * plane + y * lw + halo;
                a[s..s + TILE].iter().any(|&h| h >= d)
            })
        });
        out
    }
}

/// `t_steps` sweeps of a local tile (`a` → `b`, swapped after each), counting interior topplings.
fn sweeps(
    lat: &Lattice,
    a: &mut Vec<u8>,
    b: &mut Vec<u8>,
    lw: usize,
    t_steps: usize,
    halo: usize,
    ring_unstable: bool,
) -> TileOut {
    let nt = lat.nt;
    let r = lat.reach;
    let plane = lw * lw;
    let tile = lw - 2 * halo;
    let mut out = TileOut {
        last: -1,
        ..Default::default()
    };
    let span = lw - 2 * r;
    for s in 0..t_steps {
        let mut any = 0u8;
        let mut interior = 0u64;
        for k in 0..nt {
            let d = lat.deg[k];
            for y in r..lw - r {
                let row = k * plane + y * lw + r;
                let src = &a[row..row + span];
                let dst = &mut b[row..row + span];
                let mut f_or = 0u8;
                for (o, &v) in dst.iter_mut().zip(src) {
                    let m = 0u8.wrapping_sub((v >= d) as u8);
                    f_or |= m;
                    *o = v.wrapping_sub(m & d);
                }
                any |= f_or;
                for off in &lat.offs[k] {
                    let nrow = (off.k as usize * plane) as isize
                        + (y as isize + off.dj as isize) * lw as isize
                        + r as isize
                        + off.di as isize;
                    let ns = &a[nrow as usize..nrow as usize + span];
                    let dn = lat.deg[off.k as usize];
                    for (o, &v) in dst.iter_mut().zip(ns) {
                        *o = o.wrapping_add((v >= dn) as u8);
                    }
                }
                if f_or != 0 && y >= halo && y < halo + tile {
                    let int = &src[halo - r..halo - r + tile];
                    interior += int.iter().map(|&v| (v >= d) as u32).sum::<u32>() as u64;
                }
            }
        }
        std::mem::swap(a, b);
        if interior > 0 {
            out.fired += interior;
            out.last = s as i32;
        }
        if any == 0 && !ring_unstable {
            break;
        }
    }
    out
}
