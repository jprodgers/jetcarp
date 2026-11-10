//! Large sandpiles on aperiodic patches (Spectre, hat, Penrose): the exact patch's neighbour
//! graph, one byte per tile, synchronous sweeps over chunks of tiles with dirty flags. Open
//! boundary tiles never topple; a grain reaching one is an error (the patch is too small).

use super::{Ctl, StabStats, Tick};
use crate::topology::{Nb, Storage, Topology};
use rayon::prelude::*;
use std::sync::Arc;
use std::time::Instant;

/// Tiles per chunk (the unit of dirty tracking).
const CHUNK: usize = 4096;

pub struct PatchBoard {
    pub topo: Arc<Topology>,
    pub nb: Nb,
    pub level: u32,
    pub cur: Vec<u8>,
    nxt: Vec<u8>,
    /// Toppling threshold per tile (0 on the open boundary: never topples).
    pub deg: Vec<u8>,
    /// Chunks holding a neighbour of each chunk's tiles (including itself).
    adj: Vec<Vec<u32>>,
    unstable: Vec<bool>,
    prev: Vec<bool>,
    y_index: std::sync::OnceLock<Vec<(f32, u32)>>,
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

impl PatchBoard {
    pub fn new(topo: Arc<Topology>, nb: Nb, level: u32) -> Result<PatchBoard, String> {
        let Storage::Explicit(e) = &topo.storage else {
            return Err("not an aperiodic patch".into());
        };
        let n = topo.len();
        let csr = &e.csr[nb as usize];
        let mut deg = vec![0u8; n];
        for (i, d) in deg.iter_mut().enumerate() {
            if !e.open[i] {
                let k = csr.row(i).len();
                if k == 0 || k > 120 {
                    return Err("tile degree out of range for 1-byte heights".into());
                }
                *d = k as u8;
            }
        }
        let nc = n.div_ceil(CHUNK);
        let adj: Vec<Vec<u32>> = (0..nc)
            .into_par_iter()
            .map(|c| {
                let mut v: Vec<u32> = vec![c as u32];
                for i in c * CHUNK..((c + 1) * CHUNK).min(n) {
                    for &j in csr.row(i) {
                        v.push(j / CHUNK as u32);
                    }
                }
                v.sort_unstable();
                v.dedup();
                v
            })
            .collect();
        Ok(PatchBoard {
            topo,
            nb,
            level,
            cur: vec![0; n],
            nxt: vec![0; n],
            deg,
            adj,
            unstable: vec![false; nc],
            prev: vec![false; nc],
            y_index: std::sync::OnceLock::new(),
        })
    }
    /// Tiles sorted by the y of their outline's first corner (for drawing a band of rows).
    pub fn y_index(&self) -> &[(f32, u32)] {
        self.y_index.get_or_init(|| {
            let t = &self.topo;
            let mut v: Vec<(f32, u32)> = (0..t.len())
                .into_par_iter()
                .map(|i| {
                    let c = t.transform(i).apply(t.outlines[t.shape_of(i)][0]);
                    (c[1] as f32, i as u32)
                })
                .collect();
            v.par_sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            v
        })
    }
    pub fn cells(&self) -> u64 {
        self.cur.len() as u64
    }
    pub fn bytes(&self) -> u64 {
        2 * self.cur.len() as u64
    }
    pub fn add(&mut self, i: usize, n: u8) {
        self.cur[i] = self.cur[i].wrapping_add(n);
        self.nxt[i] = self.cur[i];
    }
    pub fn double(&mut self) {
        self.cur
            .par_chunks_mut(1 << 16)
            .for_each(|c| c.iter_mut().for_each(|h| *h = h.wrapping_mul(2)));
        self.sync();
    }
    pub fn sync(&mut self) {
        self.nxt.copy_from_slice(&self.cur);
        let n = self.cur.len();
        self.unstable = (0..self.unstable.len())
            .into_par_iter()
            .map(|c| (c * CHUNK..((c + 1) * CHUNK).min(n)).any(|i| self.fires(i)))
            .collect();
        self.prev.iter_mut().for_each(|p| *p = false);
    }
    #[inline]
    fn fires(&self, i: usize) -> bool {
        let d = self.deg[i];
        d != 0 && self.cur[i] >= d
    }
    pub fn is_unstable(&self) -> bool {
        self.unstable.iter().any(|&u| u)
    }
    pub fn grains(&self) -> u64 {
        self.cur
            .par_chunks(1 << 16)
            .map(|c| c.iter().map(|&h| h as u64).sum::<u64>())
            .sum()
    }
    /// Grains on the open boundary (nonzero means the patch was too small).
    pub fn on_boundary(&self) -> u64 {
        (0..self.cur.len())
            .filter(|&i| self.deg[i] == 0)
            .map(|i| self.cur[i] as u64)
            .sum()
    }

    pub fn stabilize(&mut self, ctl: &mut Ctl) -> Result<StabStats, String> {
        let topo = self.topo.clone();
        let csr = match &topo.storage {
            Storage::Explicit(e) => &e.csr[self.nb as usize],
            _ => unreachable!(),
        };
        let n = self.cur.len();
        let nc = self.unstable.len();
        let mut st = StabStats::default();
        let start = Instant::now();
        let mut last_tick = Instant::now();
        while self.is_unstable() {
            if ctl.cancelled() {
                return Err("cancelled".into());
            }
            let mut active = vec![false; nc];
            for c in 0..nc {
                if self.unstable[c] {
                    for &a in &self.adj[c] {
                        active[a as usize] = true;
                    }
                }
            }
            let list: Vec<usize> = (0..nc).filter(|&c| active[c] || self.prev[c]).collect();
            let out = OutPtr(self.nxt.as_mut_ptr());
            let (cur, deg) = (&self.cur, &self.deg);
            let res: Vec<(u64, bool, bool)> = list
                .par_iter()
                .map(|&c| {
                    let mut fired = 0u64;
                    let mut unstable = false;
                    let mut leak = false;
                    for i in c * CHUNK..((c + 1) * CHUNK).min(n) {
                        let d = deg[i];
                        let f = d != 0 && cur[i] >= d;
                        let mut h = cur[i].wrapping_sub(if f { d } else { 0 });
                        for &j in csr.row(i) {
                            let dj = deg[j as usize];
                            h = h.wrapping_add((dj != 0 && cur[j as usize] >= dj) as u8);
                        }
                        fired += f as u64;
                        if d == 0 {
                            leak |= h != 0;
                        } else {
                            unstable |= h >= d;
                        }
                        // SAFETY: chunks are disjoint.
                        unsafe { *out.at(i) = h };
                    }
                    (fired, unstable, leak)
                })
                .collect();
            std::mem::swap(&mut self.cur, &mut self.nxt);
            let mut nu = vec![false; nc];
            let mut fired = 0;
            let mut leak = false;
            for (&c, r) in list.iter().zip(&res) {
                fired += r.0;
                nu[c] = r.1;
                leak |= r.2;
            }
            self.unstable = nu;
            self.prev = active;
            st.topplings += fired;
            st.sweeps += 1;
            if leak {
                return Err(format!(
                    "the pile reached the open edge of the level-{} {} patch; use a larger level (up to 8)",
                    self.level, self.topo.spec.family
                ));
            }
            if last_tick.elapsed().as_secs_f64() >= ctl.tick_every {
                last_tick = Instant::now();
                let tick = Tick {
                    sweeps: st.sweeps,
                    topplings: st.topplings,
                    seconds: start.elapsed().as_secs_f64(),
                };
                ctl.tick(&tick, super::BoardRef::Patch(self));
            }
        }
        Ok(st)
    }
}
