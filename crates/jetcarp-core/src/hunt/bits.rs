//! Bit-parallel stepping of B/S and Generations rules on any periodic tiling, on an unbounded
//! plane.
//!
//! Cells live in unwrapped lattice coordinates `(i, j, k)` (lattice column, row, tile type), as
//! in `objects`. In those coordinates every periodic tiling is translation invariant: a tile of
//! type `k` at `(i, j)` has the neighbours `(i + di, j + dj, k2)` listed in the tiling's offset
//! table. So one engine covers squares (Moore and von Neumann), hexagons, triangles, and every
//! Archimedean, k-uniform, and isohedral tiling: each tile type is a bit plane (64 cells of a row
//! per word), each neighbour slot is that plane shifted by `(di, dj)`, and the neighbour count is
//! summed bit-sliced (one bit plane per binary digit of the count). The rule is then a boolean
//! function of the count bits.
//!
//! Generations rules add decay planes: a cell in state `s >= 2` stores `s` in binary over the
//! planes, state 1 is the live plane, and only state-1 neighbours are counted.
//!
//! The universe grows as the pattern grows, so nothing is lost at an edge. Each step computes
//! only the live bounding box plus one generation's reach, and skips rows with nothing nearby.
//!
//! **State hash.** Every step also computes a hash of the whole state that depends only on the
//! live cells (not on the universe's size or origin): the wrapping sums, in two 32-bit lanes, of
//! [`chunk_hash`] over every non-zero 32-cell chunk of every plane at its absolute position. The
//! GPU hunter computes the same sums, so both detect periodicity at the same generation.

use crate::objects::LCell;
use crate::rules::Rule;
use crate::topology::{LatticeGrid, Nb};
use std::sync::Arc;

/// Largest neighbour count the bit-sliced counter holds (5 count bits).
pub const MAX_DEGREE: usize = 31;

/// One count set (birth or survival) as a boolean function of the count bits: an OR of
/// equalities, or the complement of one (whichever has fewer terms).
#[derive(Clone, Debug)]
pub(crate) struct Terms {
    invert: bool,
    ns: Vec<u32>,
}

impl Terms {
    fn new(mask: u32, max_degree: usize) -> Terms {
        let all: Vec<u32> = (0..=max_degree as u32).collect();
        let ins: Vec<u32> = all.iter().copied().filter(|n| mask >> n & 1 == 1).collect();
        let outs: Vec<u32> = all.iter().copied().filter(|n| mask >> n & 1 == 0).collect();
        if outs.len() < ins.len() {
            Terms {
                invert: true,
                ns: outs,
            }
        } else {
            Terms {
                invert: false,
                ns: ins,
            }
        }
    }
    #[inline(always)]
    pub(crate) fn eval<const CB: usize>(&self, c: &[u64; CB]) -> u64 {
        let mut acc = 0u64;
        for &n in &self.ns {
            let mut t = !0u64;
            for (b, &cb) in c.iter().enumerate() {
                t &= if n >> b & 1 == 1 { cb } else { !cb };
            }
            acc |= t;
        }
        if self.invert { !acc } else { acc }
    }
}

/// A rule and a tiling's offset table, ready for bit-parallel stepping.
#[derive(Clone, Debug)]
pub struct Plan {
    pub ntypes: usize,
    /// `offs[k]` = neighbour offsets `(di, dj, k2)` of a tile of type `k`.
    pub offs: Vec<Vec<(i32, i32, u32)>>,
    /// Largest `|di|` and `|dj|` of an offset.
    pub reach_i: i32,
    pub reach_j: i32,
    pub birth: u32,
    pub survive: u32,
    /// 2 for B/S rules.
    pub states: u8,
    pub max_degree: usize,
    /// Count bits (enough for `max_degree`).
    pub cbits: usize,
    /// Decay planes (0 for B/S rules).
    pub dbits: usize,
    pub(crate) birth_t: Terms,
    pub(crate) survive_t: Terms,
}

/// The `(birth, survive, states)` of a B/S or Generations rule without B0.
pub fn bs_rule(rule: &Rule) -> Result<(u32, u32, u8), String> {
    let (b, s, c) = match *rule {
        Rule::LifeLike { birth, survive } => (birth, survive, 2),
        Rule::Generations {
            birth,
            survive,
            states,
        } => (birth, survive, states),
        _ => {
            return Err(format!(
                "the hunter runs B/S and Generations rules, not {}",
                rule.notation()
            ));
        }
    };
    if b & 1 == 1 {
        return Err("B0 rules have no empty background, so soups cannot settle".into());
    }
    if c < 2 {
        return Err("a Generations rule needs at least 2 states".into());
    }
    Ok((b, s, c))
}

impl Plan {
    /// The plan for a rule on a lattice tiling with a neighbourhood.
    pub fn new(rule: &Rule, g: &LatticeGrid, nb: Nb) -> Result<Plan, String> {
        let offs: Vec<Vec<(i32, i32, u32)>> = g.offsets[nb as usize]
            .iter()
            .map(|v| v.iter().map(|o| (o.di, o.dj, o.k)).collect())
            .collect();
        let (b, s, c) = bs_rule(rule)?;
        Plan::from_offsets(offs, b, s, c)
    }

    /// The plan for explicit offsets (tests and tools).
    pub fn from_offsets(
        offs: Vec<Vec<(i32, i32, u32)>>,
        birth: u32,
        survive: u32,
        states: u8,
    ) -> Result<Plan, String> {
        let ntypes = offs.len();
        if ntypes == 0 {
            return Err("no tile types".into());
        }
        let max_degree = offs.iter().map(|v| v.len()).max().unwrap_or(0);
        if max_degree > MAX_DEGREE {
            return Err(format!(
                "tiles with {max_degree} neighbours are more than the bit-parallel stepper counts ({MAX_DEGREE})"
            ));
        }
        let reach_i = offs.iter().flatten().map(|o| o.0.abs()).max().unwrap_or(0);
        let reach_j = offs.iter().flatten().map(|o| o.1.abs()).max().unwrap_or(0);
        if reach_i >= 64 {
            return Err("neighbour offsets reach too far for the bit-parallel stepper".into());
        }
        let cbits = (usize::BITS - max_degree.leading_zeros()).max(1) as usize;
        let dbits = if states > 2 {
            (u8::BITS - (states - 1).leading_zeros()) as usize
        } else {
            0
        };
        Ok(Plan {
            ntypes,
            birth_t: Terms::new(birth, max_degree),
            survive_t: Terms::new(survive, max_degree),
            offs,
            reach_i,
            reach_j,
            birth,
            survive,
            states,
            max_degree,
            cbits,
            dbits,
        })
    }

    /// Bit planes per tile type (the live plane plus the decay planes).
    pub fn planes(&self) -> usize {
        1 + self.dbits
    }
}

/// A 32-bit integer mixer (an avalanche finalizer), used by the state hash.
#[inline(always)]
pub fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// The hash terms of one non-zero 32-cell chunk: plane `kp = k * planes + p`, absolute row `j`,
/// absolute chunk `c` (cells `32c .. 32c + 31` of the row), bits `v`. The state hash is the
/// wrapping sum of each lane over every non-zero chunk.
#[inline(always)]
pub fn chunk_hash(kp: u32, j: i32, c: i32, v: u32) -> (u32, u32) {
    let pos = (j as u32).wrapping_mul(0x9e37_79b1)
        ^ (c as u32).wrapping_mul(0x85eb_ca77)
        ^ kp.wrapping_mul(0xc2b2_ae3d);
    let a = (v ^ pos).wrapping_mul(0x27d4_eb2f);
    let b = (v.rotate_left(16) ^ pos ^ 0x632b_e5ab).wrapping_mul(0x1656_67b1);
    (a ^ (a >> 15), b ^ (b >> 13))
}

/// A pattern on the unbounded lattice, stepped bit-parallel.
#[derive(Clone)]
pub struct Universe {
    plan: Arc<Plan>,
    np: usize,
    /// Words per row (including one empty guard word on each side) and rows.
    ww: usize,
    h: usize,
    /// Absolute lattice coordinates of bit 0 of word 0, row 0 (`i0` is a multiple of 64).
    i0: i32,
    j0: i32,
    cur: Vec<u64>,
    nxt: Vec<u64>,
    /// Region written into `nxt` by the step before last (rows, words; inclusive), to clear.
    nxt_dirty: Option<[usize; 4]>,
    cur_dirty: Option<[usize; 4]>,
    /// Rows holding any non-zero word (any type or plane).
    rows_any: Vec<bool>,
    pub generation: u64,
    pop: u64,
    /// Bounding box of the non-zero cells, absolute `[i_min, i_max, j_min, j_max]`.
    bbox: Option<[i32; 4]>,
    hash: (u32, u32),
}

impl std::fmt::Debug for Universe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Universe(gen {}, pop {}, bbox {:?}, {}x{} words)",
            self.generation, self.pop, self.bbox, self.ww, self.h
        )
    }
}

#[inline(always)]
pub(crate) fn fa(a: u64, b: u64, c: u64) -> (u64, u64) {
    let t = a ^ b;
    (t ^ c, (a & b) | (t & c))
}

#[inline(always)]
pub(crate) fn ha(a: u64, b: u64) -> (u64, u64) {
    (a ^ b, a & b)
}

/// Bit-sliced count of `n` input words (bit `b` of the result = binary digit `b` of the count
/// at each position): full-adder trees for the common degrees, ripple adds otherwise.
#[inline(always)]
pub(crate) fn count(x: &[u64; MAX_DEGREE], n: usize) -> [u64; 5] {
    match n {
        3 => {
            let (b0, b1) = fa(x[0], x[1], x[2]);
            [b0, b1, 0, 0, 0]
        }
        4 => {
            let (s, c1) = fa(x[0], x[1], x[2]);
            let (b0, c2) = ha(s, x[3]);
            let (b1, b2) = ha(c1, c2);
            [b0, b1, b2, 0, 0]
        }
        6 => {
            let (s1, c1) = fa(x[0], x[1], x[2]);
            let (s2, c2) = fa(x[3], x[4], x[5]);
            let (b0, d) = ha(s1, s2);
            let (b1, b2) = fa(c1, c2, d);
            [b0, b1, b2, 0, 0]
        }
        8 => {
            let (s1, c1) = fa(x[0], x[1], x[2]);
            let (s2, c2) = fa(x[3], x[4], x[5]);
            let (s3, c3) = ha(x[6], x[7]);
            let (b0, d1) = fa(s1, s2, s3);
            let (e1, f1) = fa(c1, c2, c3);
            let (b1, f2) = ha(e1, d1);
            let (b2, b3) = ha(f1, f2);
            [b0, b1, b2, b3, 0]
        }
        _ => {
            let mut c = [0u64; 5];
            for &v in &x[..n] {
                let mut carry = v;
                for cb in c.iter_mut() {
                    let t = *cb & carry;
                    *cb ^= carry;
                    carry = t;
                }
            }
            c
        }
    }
}

/// The word at `x` of `v`, shifted so bit `b` holds cell `b + di` of its row.
#[inline(always)]
pub(crate) fn shifted_at(v: &[u64], x: usize, di: i32) -> u64 {
    if di == 0 {
        v[x]
    } else if di > 0 {
        let s = di as u32;
        (v[x] >> s) | (v[x + 1] << (64 - s))
    } else {
        let s = (-di) as u32;
        (v[x] << s) | (v[x - 1] >> (64 - s))
    }
}

impl Universe {
    /// An empty universe.
    pub fn new(plan: Arc<Plan>) -> Universe {
        let np = plan.planes();
        let mut u = Universe {
            plan,
            np,
            ww: 0,
            h: 0,
            i0: 0,
            j0: 0,
            cur: vec![],
            nxt: vec![],
            nxt_dirty: None,
            cur_dirty: None,
            rows_any: vec![],
            generation: 0,
            pop: 0,
            bbox: None,
            hash: (0, 0),
        };
        u.alloc(-64, 0, 3, 8);
        u
    }

    /// A universe holding `cells` (state 0 entries are ignored).
    pub fn with_cells(plan: Arc<Plan>, cells: &[LCell]) -> Universe {
        let mut u = Universe::new(plan);
        u.set_cells(cells);
        u
    }

    pub fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    fn alloc(&mut self, i0: i32, j0: i32, ww: usize, h: usize) {
        let n = self.plan.ntypes * self.np * h * ww;
        self.i0 = i0;
        self.j0 = j0;
        self.ww = ww;
        self.h = h;
        self.cur = vec![0; n];
        self.nxt = vec![0; n];
        self.rows_any = vec![false; h];
        self.nxt_dirty = None;
        self.cur_dirty = None;
    }

    #[inline(always)]
    fn idx(&self, k: usize, p: usize, r: usize, w: usize) -> usize {
        ((k * self.np + p) * self.h + r) * self.ww + w
    }

    /// Live (non-zero) cells.
    pub fn population(&self) -> u64 {
        self.pop
    }
    /// Bounding box of the non-zero cells, `[i_min, i_max, j_min, j_max]`.
    pub fn bbox(&self) -> Option<[i32; 4]> {
        self.bbox
    }
    /// The state hash (see the module notes).
    pub fn hash(&self) -> u64 {
        ((self.hash.0 as u64) << 32) | self.hash.1 as u64
    }
    pub fn is_empty(&self) -> bool {
        self.pop == 0
    }

    /// Make room so that `need` (absolute `[i_min, i_max, j_min, j_max]`) plus two generations'
    /// reach lies inside the universe, away from the guard words.
    fn ensure(&mut self, need: [i32; 4]) {
        let (ri, rj) = (self.plan.reach_i, self.plan.reach_j);
        let lo_i = need[0] - 2 * ri;
        let hi_i = need[1] + 2 * ri;
        let lo_j = need[2] - 2 * rj;
        let hi_j = need[3] + 2 * rj;
        let in_i = lo_i >= self.i0 + 64 && hi_i < self.i0 + 64 * (self.ww as i32 - 1);
        let in_j = lo_j >= self.j0 && hi_j < self.j0 + self.h as i32;
        if in_i && in_j {
            return;
        }
        // Grow the side that is short, with slack: a quarter of the needed span (at least 16
        // rows or 64 columns).
        let pad_j = ((hi_j - lo_j + 1) / 4).max(16);
        let pad_i = ((hi_i - lo_i + 1) / 4).max(64);
        let cur_j1 = self.j0 + self.h as i32 - 1;
        let nj0 = if lo_j < self.j0 {
            lo_j - pad_j
        } else {
            self.j0
        };
        let nj1 = if hi_j > cur_j1 { hi_j + pad_j } else { cur_j1 };
        let cur_i1 = self.i0 + 64 * self.ww as i32 - 1;
        let ni0 = if lo_i < self.i0 + 64 {
            (lo_i - pad_i).div_euclid(64) * 64 - 64
        } else {
            self.i0
        };
        let ni1 = if hi_i >= self.i0 + 64 * (self.ww as i32 - 1) {
            (hi_i + pad_i).div_euclid(64) * 64 + 127
        } else {
            cur_i1
        };
        let nww = ((ni1 - ni0 + 1) / 64) as usize;
        let nh = (nj1 - nj0 + 1) as usize;
        let old = std::mem::take(&mut self.cur);
        let (oi0, oj0, oww, oh) = (self.i0, self.j0, self.ww, self.h);
        let old_rows = std::mem::take(&mut self.rows_any);
        self.alloc(ni0, nj0, nww, nh);
        let dw = ((oi0 - ni0) / 64) as usize;
        let dr = (oj0 - nj0) as usize;
        for k in 0..self.plan.ntypes {
            for p in 0..self.np {
                for (r, _) in old_rows.iter().enumerate().filter(|x| *x.1) {
                    let src = ((k * self.np + p) * oh + r) * oww;
                    let dst = self.idx(k, p, r + dr, dw);
                    self.cur[dst..dst + oww].copy_from_slice(&old[src..src + oww]);
                }
            }
        }
        self.rows_any[dr..dr + oh].copy_from_slice(&old_rows[..oh]);
        self.cur_dirty = Some([dr, dr + oh - 1, dw, dw + oww - 1]);
    }

    /// Set cells (state 0 clears a cell). Cells may lie anywhere; the universe grows.
    pub fn set_cells(&mut self, cells: &[LCell]) {
        if cells.is_empty() {
            return;
        }
        let mut need = self
            .bbox
            .unwrap_or([i32::MAX, i32::MIN, i32::MAX, i32::MIN]);
        for c in cells {
            need[0] = need[0].min(c.i);
            need[1] = need[1].max(c.i);
            need[2] = need[2].min(c.j);
            need[3] = need[3].max(c.j);
        }
        self.ensure(need);
        let maxs = self.plan.states - 1;
        for c in cells {
            let (r, b) = ((c.j - self.j0) as usize, (c.i - self.i0) as usize);
            let (w, bit) = (b / 64, 1u64 << (b % 64));
            let k = c.k as usize;
            if k >= self.plan.ntypes {
                continue;
            }
            let s = c.s.min(maxs);
            for p in 0..self.np {
                let on = if p == 0 {
                    s == 1
                } else {
                    s >= 2 && (s >> (p - 1)) & 1 == 1
                };
                let x = self.idx(k, p, r, w);
                if on {
                    self.cur[x] |= bit;
                } else {
                    self.cur[x] &= !bit;
                }
            }
            let d = self.cur_dirty.get_or_insert([r, r, w, w]);
            *d = [d[0].min(r), d[1].max(r), d[2].min(w), d[3].max(w)];
        }
        self.refresh();
    }

    /// Clear cells (any state).
    pub fn erase(&mut self, cells: &[LCell]) {
        let z: Vec<LCell> = cells.iter().map(|c| LCell { s: 0, ..*c }).collect();
        self.set_cells(&z);
    }

    /// Recompute population, bounding box, hash, and row flags from the current planes.
    fn refresh(&mut self) {
        let Some(d) = self.cur_dirty else {
            self.pop = 0;
            self.bbox = None;
            self.hash = (0, 0);
            return;
        };
        let mut st = Stats::default();
        for r in d[0]..=d[1] {
            let mut any = false;
            for w in d[2]..=d[3] {
                let mut occ = 0u64;
                for k in 0..self.plan.ntypes {
                    let mut ok = 0u64;
                    for p in 0..self.np {
                        let v = self.cur[self.idx(k, p, r, w)];
                        if v != 0 {
                            ok |= v;
                            st.hash(
                                (k * self.np + p) as u32,
                                self.j0 + r as i32,
                                self.i0 / 32 + 2 * w as i32,
                                v,
                            );
                        }
                    }
                    st.pop += ok.count_ones() as u64;
                    occ |= ok;
                }
                if occ != 0 {
                    any = true;
                    st.occ(occ, w, r);
                }
            }
            self.rows_any[r] = any;
        }
        self.finish(st);
    }

    fn finish(&mut self, st: Stats) {
        self.pop = st.pop;
        self.hash = (st.ha, st.hb);
        self.bbox = (st.pop > 0).then(|| {
            [
                self.i0 + st.bmin as i32,
                self.i0 + st.bmax as i32,
                self.j0 + st.rmin as i32,
                self.j0 + st.rmax as i32,
            ]
        });
    }

    /// The non-zero cells, sorted (by row, column, tile type).
    pub fn cells(&self) -> Vec<LCell> {
        let mut out = Vec::with_capacity(self.pop as usize);
        let Some(bb) = self.bbox else { return out };
        let r0 = (bb[2] - self.j0) as usize;
        let r1 = (bb[3] - self.j0) as usize;
        let w0 = ((bb[0] - self.i0) / 64) as usize;
        let w1 = ((bb[1] - self.i0) / 64) as usize;
        for r in r0..=r1 {
            if !self.rows_any[r] {
                continue;
            }
            let j = self.j0 + r as i32;
            let start = out.len();
            for k in 0..self.plan.ntypes {
                for w in w0..=w1 {
                    let mut occ = 0u64;
                    for p in 0..self.np {
                        occ |= self.cur[self.idx(k, p, r, w)];
                    }
                    while occ != 0 {
                        let b = occ.trailing_zeros() as usize;
                        occ &= occ - 1;
                        let alive = self.cur[self.idx(k, 0, r, w)] >> b & 1 == 1;
                        let s = if alive {
                            1
                        } else {
                            let mut v = 0u8;
                            for p in 1..self.np {
                                v |= ((self.cur[self.idx(k, p, r, w)] >> b & 1) as u8) << (p - 1);
                            }
                            v
                        };
                        out.push(LCell {
                            j,
                            i: self.i0 + (w * 64 + b) as i32,
                            k: k as u32,
                            s,
                        });
                    }
                }
            }
            if self.plan.ntypes > 1 {
                out[start..].sort_unstable();
            }
        }
        out
    }

    /// The non-zero cells outside a region (`[i_min, i_max, j_min, j_max]`, absolute), sorted.
    pub fn cells_outside(&self, region: [i32; 4]) -> Vec<LCell> {
        let mut out = Vec::new();
        let Some(bb) = self.bbox else { return out };
        let r0 = (bb[2] - self.j0) as usize;
        let r1 = (bb[3] - self.j0) as usize;
        let w0 = ((bb[0] - self.i0) / 64) as usize;
        let w1 = ((bb[1] - self.i0) / 64) as usize;
        // Per word: the bits inside the region's columns.
        let masks: Vec<u64> = (w0..=w1)
            .map(|w| {
                let lo = self.i0 + 64 * w as i32;
                let a = (region[0] - lo).clamp(0, 64);
                let b = (region[1] - lo + 1).clamp(0, 64);
                if b <= a {
                    0
                } else {
                    let hi = if b == 64 { !0u64 } else { (1u64 << b) - 1 };
                    hi & !((1u64 << a) - 1)
                }
            })
            .collect();
        for r in r0..=r1 {
            if !self.rows_any[r] {
                continue;
            }
            let j = self.j0 + r as i32;
            let in_rows = j >= region[2] && j <= region[3];
            let start = out.len();
            for k in 0..self.plan.ntypes {
                for w in w0..=w1 {
                    let mut occ = 0u64;
                    for p in 0..self.np {
                        occ |= self.cur[self.idx(k, p, r, w)];
                    }
                    if in_rows {
                        occ &= !masks[w - w0];
                    }
                    while occ != 0 {
                        let b = occ.trailing_zeros() as usize;
                        occ &= occ - 1;
                        let alive = self.cur[self.idx(k, 0, r, w)] >> b & 1 == 1;
                        let s = if alive {
                            1
                        } else {
                            let mut v = 0u8;
                            for p in 1..self.np {
                                v |= ((self.cur[self.idx(k, p, r, w)] >> b & 1) as u8) << (p - 1);
                            }
                            v
                        };
                        out.push(LCell {
                            j,
                            i: self.i0 + (w * 64 + b) as i32,
                            k: k as u32,
                            s,
                        });
                    }
                }
            }
            if self.plan.ntypes > 1 {
                out[start..].sort_unstable();
            }
        }
        out
    }

    /// One generation.
    pub fn step(&mut self) {
        match self.plan.cbits {
            1 => self.step_cb::<1>(),
            2 => self.step_cb::<2>(),
            3 => self.step_cb::<3>(),
            4 => self.step_cb::<4>(),
            _ => self.step_cb::<5>(),
        }
    }

    /// `n` generations.
    pub fn run(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    fn step_cb<const CB: usize>(&mut self) {
        self.generation += 1;
        let Some(bb) = self.bbox else { return };
        self.ensure(bb);
        let plan = self.plan.clone();
        let (ri, rj) = (plan.reach_i, plan.reach_j);
        let r_lo = (bb[2] - rj - self.j0) as usize;
        let r_hi = (bb[3] + rj - self.j0) as usize;
        let w_lo = ((bb[0] - ri - self.i0) / 64) as usize;
        let w_hi = ((bb[1] + ri - self.i0) / 64) as usize;
        // Clear what the step before last wrote into the back buffer.
        if let Some(d) = self.nxt_dirty.take() {
            for k in 0..plan.ntypes {
                for p in 0..self.np {
                    for r in d[0]..=d[1] {
                        let x = self.idx(k, p, r, 0);
                        self.nxt[x + d[2]..=x + d[3]].fill(0);
                    }
                }
            }
        }
        let np = self.np;
        let (h, ww) = (self.h, self.ww);
        let rows_any = std::mem::take(&mut self.rows_any);
        let mut new_rows = vec![false; h];
        let mut st = Stats::default();
        let gens = plan.states > 2;
        let dbits = plan.dbits;
        let states = plan.states as u32;
        let rj = rj as usize;
        let mut bases: Vec<(usize, i32)> =
            Vec::with_capacity(plan.offs.iter().map(|v| v.len()).sum());
        #[allow(clippy::needless_range_loop)]
        for r in r_lo..=r_hi {
            // Nothing within reach: the row stays empty.
            let a = r.saturating_sub(rj);
            let b = (r + rj).min(h - 1);
            if !rows_any[a..=b].iter().any(|&x| x) {
                continue;
            }
            let mut any = false;
            // Where each neighbour slot's source row starts, per tile type.
            bases.clear();
            for offs in &plan.offs {
                for &(di, dj, k2) in offs {
                    let rr = (r as i32 + dj) as usize;
                    bases.push((((k2 as usize * np) * h + rr) * ww, di));
                }
            }
            let cur = &self.cur[..];
            for w in w_lo..=w_hi {
                let mut occ = 0u64;
                let mut at = 0;
                for k in 0..plan.ntypes {
                    let offs = &plan.offs[k];
                    let n = offs.len();
                    let mut xs = [0u64; MAX_DEGREE];
                    let mut any_in = 0u64;
                    for (o, &(base, di)) in bases[at..at + n].iter().enumerate() {
                        let x = shifted_at(cur, base + w, di);
                        xs[o] = x;
                        any_in |= x;
                    }
                    at += n;
                    let ia = ((k * np) * h + r) * ww + w;
                    let alive = self.cur[ia];
                    // No live neighbours and nothing here: stays empty (no B0).
                    if any_in == 0 && alive == 0 && !gens {
                        continue;
                    }
                    let full = count(&xs, offs.len());
                    let mut c = [0u64; CB];
                    c.copy_from_slice(&full[..CB]);
                    let bm = plan.birth_t.eval(&c);
                    let sm = plan.survive_t.eval(&c);
                    if !gens {
                        let v = (!alive & bm) | (alive & sm);
                        self.nxt[ia] = v;
                        if v != 0 {
                            occ |= v;
                            st.pop += v.count_ones() as u64;
                            st.hash(
                                (k * np) as u32,
                                self.j0 + r as i32,
                                self.i0 / 32 + 2 * w as i32,
                                v,
                            );
                        }
                    } else {
                        let mut v = [0u64; 8];
                        let mut nz = 0u64;
                        for (p, x) in v.iter_mut().enumerate().take(dbits) {
                            *x = self.cur[((k * np + 1 + p) * h + r) * ww + w];
                            nz |= *x;
                        }
                        let empty = !(alive | nz);
                        let na = (empty & bm) | (alive & sm);
                        let dying = alive & !sm;
                        // Decaying cells count up; reaching `states` means empty.
                        let mut carry = nz;
                        for x in v.iter_mut().take(dbits) {
                            let t = *x & carry;
                            *x ^= carry;
                            carry = t;
                        }
                        let mut eq = !0u64;
                        for (p, x) in v.iter().enumerate().take(dbits) {
                            eq &= if states >> p & 1 == 1 { *x } else { !*x };
                        }
                        if states >> dbits != 0 {
                            eq = 0;
                        }
                        for x in v.iter_mut().take(dbits) {
                            *x &= !eq;
                        }
                        v[1] |= dying;
                        self.nxt[ia] = na;
                        let mut o = na;
                        if na != 0 {
                            st.hash(
                                (k * np) as u32,
                                self.j0 + r as i32,
                                self.i0 / 32 + 2 * w as i32,
                                na,
                            );
                        }
                        for (p, &x) in v.iter().enumerate().take(dbits) {
                            self.nxt[((k * np + 1 + p) * h + r) * ww + w] = x;
                            if x != 0 {
                                o |= x;
                                st.hash(
                                    (k * np + 1 + p) as u32,
                                    self.j0 + r as i32,
                                    self.i0 / 32 + 2 * w as i32,
                                    x,
                                );
                            }
                        }
                        st.pop += o.count_ones() as u64;
                        occ |= o;
                    }
                }
                if occ != 0 {
                    any = true;
                    st.occ(occ, w, r);
                }
            }
            new_rows[r] = any;
        }
        self.rows_any = new_rows;
        std::mem::swap(&mut self.cur, &mut self.nxt);
        self.nxt_dirty = self.cur_dirty.take();
        self.cur_dirty = Some([r_lo, r_hi, w_lo, w_hi]);
        self.finish(st);
    }
}

#[derive(Default)]
struct Stats {
    any: bool,
    pop: u64,
    ha: u32,
    hb: u32,
    rmin: usize,
    rmax: usize,
    bmin: usize,
    bmax: usize,
}

impl Stats {
    #[inline(always)]
    fn hash(&mut self, kp: u32, j: i32, c0: i32, v: u64) {
        let lo = v as u32;
        let hi = (v >> 32) as u32;
        if lo != 0 {
            let (a, b) = chunk_hash(kp, j, c0, lo);
            self.ha = self.ha.wrapping_add(a);
            self.hb = self.hb.wrapping_add(b);
        }
        if hi != 0 {
            let (a, b) = chunk_hash(kp, j, c0 + 1, hi);
            self.ha = self.ha.wrapping_add(a);
            self.hb = self.hb.wrapping_add(b);
        }
    }
    #[inline(always)]
    fn occ(&mut self, occ: u64, w: usize, r: usize) {
        let lo = w * 64 + occ.trailing_zeros() as usize;
        let hi = w * 64 + 63 - occ.leading_zeros() as usize;
        if !self.any {
            self.any = true;
            self.rmin = r;
            self.bmin = lo;
            self.bmax = hi;
        }
        self.rmax = r;
        self.bmin = self.bmin.min(lo);
        self.bmax = self.bmax.max(hi);
    }
}
