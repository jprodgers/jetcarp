//! Bit-parallel stepping of B/S and Generations rules on a **bounded** lattice grid, with wrapped
//! or dead edges: the hunter's engine ([`crate::hunt::bits`]) on the grids the census runs.
//!
//! The grid is stored as the world stores it: row `j` holds unit cells `u = 0 .. w` (lattice
//! column `i = u + row_start[j]`), each tile type its own bit plane, 64 cells per word, with one
//! guard word on each side of a row. A neighbour slot `(di, dj, k2)` of a cell in row `j` reads row
//! `j + dj` shifted by `du = di + row_start[j] - row_start[j + dj]`; on a wrapped grid the row index
//! wraps (shifted by the grid's `wrap_shift` across the seam) and so does the column, through
//! *ghost* bits: before every step the guard words and the unused bits past `w` are filled with
//! the cells from the other side of the row. On a dead-edged grid they stay zero, so missing
//! neighbours count as dead, as in the world. Rows past a dead top or bottom edge read an empty
//! row.
//!
//! Every step also counts the cells that changed state (the activity) and computes a state hash
//! from the non-zero 32-cell chunks of every plane at their positions, as the hunter does.
//! `tests/census.rs` checks that the board equals the world's own stepper, cell for cell, on
//! squares (both neighbourhoods), hexagons, triangles, and Archimedean tilings, with wrapped and
//! dead edges, for B/S and Generations rules.

use crate::hunt::bits::{MAX_DEGREE, Plan, chunk_hash, count, shifted_at};
use crate::objects::LCell;
use crate::rules::Rule;
use crate::topology::{LatticeGrid, Nb, Topology};
use std::sync::Arc;

/// A bounded lattice grid stepped bit-parallel.
#[derive(Clone)]
pub struct Board {
    plan: Arc<Plan>,
    np: usize,
    nt: usize,
    /// Unit cells per row and rows.
    pub w: usize,
    pub h: usize,
    /// Data words per row; words per row with the two guards.
    nw: usize,
    ww: usize,
    wrap: bool,
    row_start: Vec<i32>,
    /// Per row, for every (tile type, slot) in order: start of the source row (plane 0 of the
    /// neighbour's type) and the shift.
    slots: Vec<(usize, i32)>,
    nslots: usize,
    /// Per row: the source rows it reads (for skipping rows with nothing nearby).
    sources: Vec<Vec<usize>>,
    /// Valid bits of the last data word.
    tail: u64,
    cur: Vec<u64>,
    nxt: Vec<u64>,
    /// Rows (of `h + 1`; the last is always empty) holding any non-zero word.
    rows_any: Vec<bool>,
    pub generation: u64,
    pop: u64,
    changed: u64,
    hash: (u32, u32),
}

impl std::fmt::Debug for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Board({}x{}x{}, gen {}, pop {})",
            self.w, self.h, self.nt, self.generation, self.pop
        )
    }
}

impl Board {
    /// A board for a B/S or Generations rule (no B0) on a lattice topology with wrapped or dead
    /// edges (live edges are not supported).
    pub fn for_topology(rule: &Rule, topo: &Topology, nb: Nb) -> Result<Board, String> {
        let g = topo.lattice().ok_or(
            "bit-parallel boards need a periodic tiling (aperiodic patches have no lattice)",
        )?;
        if topo.live_edges() {
            return Err(
                "bit-parallel boards support wrapped and dead edges, not live edges".into(),
            );
        }
        let plan = Arc::new(Plan::new(rule, g, nb)?);
        Board::new(plan, g)
    }

    /// An empty board for a plan on a grid.
    pub fn new(plan: Arc<Plan>, g: &LatticeGrid) -> Result<Board, String> {
        let (w, h, nt) = (g.w as usize, g.h as usize, g.ntypes as usize);
        if w == 0 || h == 0 {
            return Err("empty grid".into());
        }
        if nt != plan.ntypes {
            return Err("the plan's tile types do not match the grid".into());
        }
        let np = plan.planes();
        let nw = w.div_ceil(64);
        let ww = nw + 2;
        let hp = h + 1;
        let mut slots = Vec::with_capacity(h * plan.offs.iter().map(|v| v.len()).sum::<usize>());
        let mut sources = Vec::with_capacity(h);
        for j in 0..h as i32 {
            let mut src = vec![j as usize];
            for offs in &plan.offs {
                for &(di, dj, k2) in offs {
                    let j2 = j + dj;
                    let (rr, du) = if g.wrap {
                        let q = j2.div_euclid(h as i32);
                        let jj = j2 - q * h as i32;
                        let mut du = di + g.row_start[j as usize]
                            - g.row_start[jj as usize]
                            - q * g.wrap_shift;
                        // Columns wrap too: take the shift nearest zero.
                        du = du.rem_euclid(w as i32);
                        if du > w as i32 / 2 {
                            du -= w as i32;
                        }
                        (jj as usize, du)
                    } else if j2 < 0 || j2 >= h as i32 {
                        (h, 0)
                    } else {
                        (
                            j2 as usize,
                            di + g.row_start[j as usize] - g.row_start[j2 as usize],
                        )
                    };
                    if du.unsigned_abs() >= 64 {
                        return Err(
                            "neighbour offsets reach too far for a bit-parallel board".into()
                        );
                    }
                    slots.push((((k2 as usize * np) * hp + rr) * ww, du));
                    src.push(rr);
                }
            }
            src.sort_unstable();
            src.dedup();
            sources.push(src);
        }
        let nslots = slots.len() / h;
        let tail = if w % 64 == 0 {
            !0
        } else {
            (1u64 << (w % 64)) - 1
        };
        let n = nt * np * hp * ww;
        Ok(Board {
            plan,
            np,
            nt,
            w,
            h,
            nw,
            ww,
            wrap: g.wrap,
            row_start: g.row_start.clone(),
            slots,
            nslots,
            sources,
            tail,
            cur: vec![0; n],
            nxt: vec![0; n],
            rows_any: vec![false; hp],
            generation: 0,
            pop: 0,
            changed: 0,
            hash: (0, 0),
        })
    }

    pub fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }
    /// Cells on the board.
    pub fn len(&self) -> usize {
        self.w * self.h * self.nt
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Non-zero cells.
    pub fn population(&self) -> u64 {
        self.pop
    }
    /// Cells whose state changed in the last step.
    pub fn changed(&self) -> u64 {
        self.changed
    }
    /// State hash (depends only on the cells).
    pub fn hash(&self) -> u64 {
        ((self.hash.0 as u64) << 32) | self.hash.1 as u64
    }

    #[inline(always)]
    fn idx(&self, k: usize, p: usize, r: usize, x: usize) -> usize {
        ((k * self.np + p) * (self.h + 1) + r) * self.ww + x
    }

    /// Load the world's cell states (index `((j * w) + u) * ntypes + k`, as `LatticeGrid::index`).
    /// States above the rule's last are clamped to it.
    pub fn load(&mut self, states: &[u8]) {
        self.cur.fill(0);
        self.nxt.fill(0);
        let maxs = self.plan.states - 1;
        for (c, &s) in states.iter().enumerate().take(self.len()) {
            if s == 0 {
                continue;
            }
            let k = c % self.nt;
            let u = (c / self.nt) % self.w;
            let j = c / self.nt / self.w;
            let s = s.min(maxs);
            let (x, bit) = (1 + u / 64, 1u64 << (u % 64));
            for p in 0..self.np {
                let on = if p == 0 {
                    s == 1
                } else {
                    s >= 2 && (s >> (p - 1)) & 1 == 1
                };
                if on {
                    let i = self.idx(k, p, j, x);
                    self.cur[i] |= bit;
                }
            }
        }
        self.generation = 0;
        self.changed = 0;
        self.refresh();
    }

    /// Population, hash, and row flags from the current planes.
    fn refresh(&mut self) {
        let mut st = Stats::default();
        for r in 0..self.h {
            let mut any = false;
            for x in 1..=self.nw {
                for k in 0..self.nt {
                    let mut occ = 0u64;
                    for p in 0..self.np {
                        let v = self.cur[self.idx(k, p, r, x)];
                        if v != 0 {
                            occ |= v;
                            st.hash((k * self.np + p) as u32, r as i32, 2 * (x as i32 - 1), v);
                        }
                    }
                    st.pop += occ.count_ones() as u64;
                    any |= occ != 0;
                }
            }
            self.rows_any[r] = any;
        }
        self.rows_any[self.h] = false;
        self.pop = st.pop;
        self.hash = (st.ha, st.hb);
    }

    /// The state of every cell, in the world's order.
    pub fn states(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.len()];
        for j in 0..self.h {
            if !self.rows_any[j] {
                continue;
            }
            for k in 0..self.nt {
                for x in 1..=self.nw {
                    let mut occ = 0u64;
                    for p in 0..self.np {
                        occ |= self.cur[self.idx(k, p, j, x)];
                    }
                    while occ != 0 {
                        let b = occ.trailing_zeros() as usize;
                        occ &= occ - 1;
                        let u = (x - 1) * 64 + b;
                        out[(j * self.w + u) * self.nt + k] = self.state_at(k, j, x, b);
                    }
                }
            }
        }
        out
    }

    #[inline]
    fn state_at(&self, k: usize, j: usize, x: usize, b: usize) -> u8 {
        if self.cur[self.idx(k, 0, j, x)] >> b & 1 == 1 {
            return 1;
        }
        let mut v = 0u8;
        for p in 1..self.np {
            v |= ((self.cur[self.idx(k, p, j, x)] >> b & 1) as u8) << (p - 1);
        }
        v
    }

    /// The non-zero cells in lattice coordinates (`i = u + row_start[j]`), sorted by row,
    /// column, and tile type (for the object census).
    pub fn cells(&self) -> Vec<LCell> {
        let mut out = Vec::with_capacity(self.pop as usize);
        for j in 0..self.h {
            if !self.rows_any[j] {
                continue;
            }
            let start = out.len();
            for k in 0..self.nt {
                for x in 1..=self.nw {
                    let mut occ = 0u64;
                    for p in 0..self.np {
                        occ |= self.cur[self.idx(k, p, j, x)];
                    }
                    while occ != 0 {
                        let b = occ.trailing_zeros() as usize;
                        occ &= occ - 1;
                        let u = (x - 1) * 64 + b;
                        out.push(LCell {
                            j: j as i32,
                            i: u as i32 + self.row_start[j],
                            k: k as u32,
                            s: self.state_at(k, j, x, b),
                        });
                    }
                }
            }
            out[start..].sort_unstable();
        }
        out
    }

    /// The live plane of tile type `k`, row `j`, as words (cell `u` is bit `u % 64` of word
    /// `u / 64`).
    pub fn live_row(&self, k: usize, j: usize) -> &[u64] {
        let a = self.idx(k, 0, j, 1);
        &self.cur[a..a + self.nw]
    }

    /// Cells `start .. start + n` (n <= 64) of a row, the column wrapping around.
    #[inline]
    fn read_wrapped(&self, row: usize, start: i64, n: usize) -> u64 {
        let w = self.w;
        let mut out = 0u64;
        let mut got = 0usize;
        while got < n {
            let u = (start + got as i64).rem_euclid(w as i64) as usize;
            let pos = 64 + u;
            let (wi, bi) = (pos / 64, pos % 64);
            let m = (n - got).min(w - u).min(64 - bi);
            let mask = if m == 64 { !0 } else { (1u64 << m) - 1 };
            out |= ((self.cur[row + wi] >> bi) & mask) << got;
            got += m;
        }
        out
    }

    /// Fill the ghost bits of every live plane row (wrapped grids).
    fn fill_ghosts(&mut self) {
        let (w, nw, h) = (self.w, self.nw, self.h);
        let b0 = w % 64;
        for k in 0..self.nt {
            for j in 0..h {
                let row = self.idx(k, 0, j, 0);
                let left = self.read_wrapped(row, -64, 64);
                // Bits past `w` in the last data word, then the right guard word.
                let tail_n = 64 * nw - w;
                let tail = if tail_n > 0 {
                    self.read_wrapped(row, 0, tail_n)
                } else {
                    0
                };
                let right = self.read_wrapped(row, (64 * nw) as i64, 64);
                self.cur[row] = left;
                if tail_n > 0 {
                    let x = row + nw;
                    self.cur[x] = (self.cur[x] & self.tail) | (tail << b0);
                }
                self.cur[row + nw + 1] = right;
            }
        }
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
        if self.pop == 0 {
            self.changed = 0;
            return;
        }
        if self.wrap {
            self.fill_ghosts();
        }
        let plan = self.plan.clone();
        let (np, nt, nw, ww, h) = (self.np, self.nt, self.nw, self.ww, self.h);
        let hp = h + 1;
        let gens = plan.states > 2;
        let dbits = plan.dbits;
        let states = plan.states as u32;
        let mut st = Stats::default();
        let mut new_rows = vec![false; hp];
        let cur = &self.cur[..];
        let nxt = &mut self.nxt[..];
        for r in 0..h {
            let near = self.sources[r].iter().any(|&s| self.rows_any[s]);
            if !near {
                for k in 0..nt {
                    for p in 0..np {
                        let a = ((k * np + p) * hp + r) * ww;
                        nxt[a + 1..a + 1 + nw].fill(0);
                    }
                }
                continue;
            }
            let slots = &self.slots[r * self.nslots..(r + 1) * self.nslots];
            let mut any = false;
            for x in 1..=nw {
                let mask = if x == nw { self.tail } else { !0 };
                let mut at = 0;
                for k in 0..nt {
                    let n = plan.offs[k].len();
                    let mut xs = [0u64; MAX_DEGREE];
                    let mut any_in = 0u64;
                    for (o, &(base, du)) in slots[at..at + n].iter().enumerate() {
                        let v = shifted_at(cur, base + x, du);
                        xs[o] = v;
                        any_in |= v;
                    }
                    at += n;
                    let ia = ((k * np) * hp + r) * ww + x;
                    let alive = cur[ia];
                    if !gens {
                        if any_in == 0 && alive == 0 {
                            nxt[ia] = 0;
                            continue;
                        }
                        let full = count(&xs, n);
                        let mut c = [0u64; CB];
                        c.copy_from_slice(&full[..CB]);
                        let bm = plan.birth_t.eval(&c);
                        let sm = plan.survive_t.eval(&c);
                        let v = ((!alive & bm) | (alive & sm)) & mask;
                        nxt[ia] = v;
                        st.changed += (v ^ (alive & mask)).count_ones() as u64;
                        if v != 0 {
                            any = true;
                            st.pop += v.count_ones() as u64;
                            st.hash((k * np) as u32, r as i32, 2 * (x as i32 - 1), v);
                        }
                    } else {
                        let full = count(&xs, n);
                        let mut c = [0u64; CB];
                        c.copy_from_slice(&full[..CB]);
                        let bm = plan.birth_t.eval(&c);
                        let sm = plan.survive_t.eval(&c);
                        let alive = alive & mask;
                        let mut v = [0u64; 8];
                        let mut old = [0u64; 8];
                        let mut nz = 0u64;
                        for p in 0..dbits {
                            let y = cur[((k * np + 1 + p) * hp + r) * ww + x];
                            v[p] = y;
                            old[p] = y;
                            nz |= y;
                        }
                        let empty = !(alive | nz);
                        let na = ((empty & bm) | (alive & sm)) & mask;
                        let dying = alive & !sm;
                        // Decaying cells count up; reaching `states` means empty.
                        let mut carry = nz;
                        for y in v.iter_mut().take(dbits) {
                            let t = *y & carry;
                            *y ^= carry;
                            carry = t;
                        }
                        let mut eq = !0u64;
                        for (p, y) in v.iter().enumerate().take(dbits) {
                            eq &= if states >> p & 1 == 1 { *y } else { !*y };
                        }
                        if states >> dbits != 0 {
                            eq = 0;
                        }
                        for y in v.iter_mut().take(dbits) {
                            *y &= !eq;
                        }
                        v[1] |= dying;
                        nxt[ia] = na;
                        let mut o = na;
                        let mut diff = na ^ alive;
                        if na != 0 {
                            st.hash((k * np) as u32, r as i32, 2 * (x as i32 - 1), na);
                        }
                        for p in 0..dbits {
                            let y = v[p] & mask;
                            nxt[((k * np + 1 + p) * hp + r) * ww + x] = y;
                            diff |= y ^ old[p];
                            if y != 0 {
                                o |= y;
                                st.hash((k * np + 1 + p) as u32, r as i32, 2 * (x as i32 - 1), y);
                            }
                        }
                        st.changed += diff.count_ones() as u64;
                        st.pop += o.count_ones() as u64;
                        any |= o != 0;
                    }
                }
            }
            new_rows[r] = any;
        }
        self.rows_any = new_rows;
        std::mem::swap(&mut self.cur, &mut self.nxt);
        self.pop = st.pop;
        self.changed = st.changed;
        self.hash = (st.ha, st.hb);
    }
}

#[derive(Default)]
struct Stats {
    pop: u64,
    changed: u64,
    ha: u32,
    hb: u32,
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
}
