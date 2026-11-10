//! Puffers: patterns that travel like a spaceship while leaving debris behind.
//!
//! The test is exact. The pattern runs alone on the unbounded lattice (a sparse simulation in
//! unwrapped lattice coordinates, so it never meets an edge) for a few hundred generations. For a
//! period `p` and lattice vector `d`, let `L(t)` be the largest projection `i·dx + j·dy` of a live
//! cell at generation `t`, and the **front** `F_W(t)` the live cells within `W` of it. The pattern
//! is a puffer of period `p` and displacement `d` if, over the last four periods of the run,
//! `F_W(t + p)` is exactly `F_W(t)` shifted by `d` (positions, tile types, and states), for a
//! front at least as deep as one generation's reach and holding more than one cell, while the
//! population grows every period, and the front lays the debris: cut off from the rest (at an
//! empty band behind it, where there is one) and run alone, it evolves exactly as it did within
//! the pattern and keeps growing every period. (A
//! spaceship escaping from a growing mess recurs only as the spaceship, which alone does not
//! grow.) Debris may be still lifes, oscillators, or spaceships (a rake). The deepest recurring
//! front is reported; a puffer's debris is laid periodically, so it usually reaches far back
//! along the trail.

use super::{LCell, Phase};
use crate::rules::Rule;
use crate::topology::{LatticeGrid, Nb};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A small fast hasher for lattice coordinates (the default hasher dominates the sparse run).
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }
    fn write_u32(&mut self, x: u32) {
        self.0 = (self.0.rotate_left(5) ^ x as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_i32(&mut self, x: i32) {
        self.write_u32(x as u32);
    }
}

type CellMap<V> = HashMap<(i32, i32, u32), V, BuildHasherDefault<CellHasher>>;

/// A rule run sparsely on an unbounded lattice.
pub struct Sparse {
    rule: Rule,
    /// Per tile type: neighbour offsets (di, dj, tile type).
    offsets: Vec<Vec<(i32, i32, u32)>>,
    /// How far one generation reaches in the projection, per unit of |d| (lattice steps).
    pub reach: i32,
}

/// What [`detect`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct PufferFit {
    pub period: u64,
    pub d: [i32; 2],
    /// Front depth (in projection units) and its cells at the last checked generation.
    pub depth: i64,
    pub front_cells: usize,
    /// Population at the start and end of the checked window.
    pub pop_from: usize,
    pub pop_to: usize,
    /// Generations run.
    pub generations: u64,
}

impl Sparse {
    /// `None` if the rule is not one the sparse runner knows (B/S without B0, Generations,
    /// WireWorld, priority tables with a quiet vacuum).
    pub fn new(rule: &Rule, g: &LatticeGrid, nb: Nb) -> Option<Sparse> {
        match rule {
            Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } if birth & 1 == 0 => {}
            Rule::WireWorld => {}
            Rule::Priority(p) if !(0..=24).any(|d| p.ignites(d)) => {}
            _ => return None,
        }
        let offsets: Vec<Vec<(i32, i32, u32)>> = g.offsets[nb as usize]
            .iter()
            .map(|v| v.iter().map(|o| (o.di, o.dj, o.k)).collect())
            .collect();
        let reach = offsets
            .iter()
            .flatten()
            .map(|o| o.0.abs().max(o.1.abs()))
            .max()
            .unwrap_or(1)
            .max(1);
        Some(Sparse {
            rule: rule.clone(),
            offsets,
            reach,
        })
    }

    fn next(&self, own: u8, c: &[u16; 8], degree: usize) -> u8 {
        match &self.rule {
            Rule::LifeLike { birth, survive } => {
                let n = c[1] as u32;
                if own == 0 {
                    ((birth >> n) & 1) as u8
                } else {
                    ((survive >> n) & 1) as u8
                }
            }
            Rule::Generations {
                birth,
                survive,
                states,
            } => {
                let n = c[1] as u32;
                match own {
                    0 => ((birth >> n) & 1) as u8,
                    1 if (survive >> n) & 1 == 1 => 1,
                    s if s + 1 < *states => s + 1,
                    _ => 0,
                }
            }
            Rule::WireWorld => match own {
                1 => 2,
                2 => 3,
                3 if c[1] == 1 || c[1] == 2 => 1,
                3 => 3,
                _ => 0,
            },
            Rule::Priority(p) => {
                let mut c = *c;
                let live: u16 = c[1..].iter().sum();
                c[0] = degree as u16 - live;
                p.next(own, &c)
            }
            _ => 0,
        }
    }

    /// One generation.
    fn step(&self, live: &CellMap<u8>) -> CellMap<u8> {
        let mut cands: Vec<(i32, i32, u32)> = Vec::with_capacity(live.len() * 9);
        for &(i, j, k) in live.keys() {
            cands.push((i, j, k));
            for &(di, dj, k2) in &self.offsets[k as usize] {
                cands.push((i + di, j + dj, k2));
            }
        }
        cands.sort_unstable();
        cands.dedup();
        let mut out = CellMap::with_capacity_and_hasher(live.len() * 2, Default::default());
        for (i, j, k) in cands {
            let mut c = [0u16; 8];
            let offs = &self.offsets[k as usize];
            for &(di, dj, k2) in offs {
                if let Some(&s) = live.get(&(i + di, j + dj, k2)) {
                    c[s.min(7) as usize] += 1;
                }
            }
            let own = live.get(&(i, j, k)).copied().unwrap_or(0);
            let v = self.next(own, &c, offs.len());
            if v != 0 {
                out.insert((i, j, k), v);
            }
        }
        out
    }

    /// Run `gens` generations; `None` if the population passes `cap`. Phases are sorted.
    pub fn run(&self, start: &[LCell], gens: u64, cap: usize) -> Option<Vec<Phase>> {
        let mut live: CellMap<u8> = start.iter().map(|c| ((c.i, c.j, c.k), c.s)).collect();
        let mut out = vec![phase_of(&live)];
        for _ in 0..gens {
            live = self.step(&live);
            if live.len() > cap {
                return None;
            }
            out.push(phase_of(&live));
            if live.is_empty() {
                break;
            }
        }
        Some(out)
    }
}

fn phase_of(live: &CellMap<u8>) -> Phase {
    let mut v: Phase = live
        .iter()
        .map(|(&(i, j, k), &s)| LCell { i, j, k, s })
        .collect();
    v.sort_unstable();
    v
}

fn proj(c: &LCell, d: [i32; 2]) -> i64 {
    c.i as i64 * d[0] as i64 + c.j as i64 * d[1] as i64
}

/// Cells of `ph` with projection at least `lead - w`, shifted by `-shift`, sorted.
fn front(ph: &Phase, d: [i32; 2], lead: i64, w: i64, shift: [i32; 2]) -> Phase {
    let mut v: Phase = ph
        .iter()
        .filter(|c| proj(c, d) >= lead - w)
        .map(|c| LCell {
            i: c.i - shift[0],
            j: c.j - shift[1],
            ..*c
        })
        .collect();
    v.sort_unstable();
    v
}

/// Live cells above which the detector gives up (an explosion, not a puffer worth testing).
pub const TEST_CAP: usize = 3_000;

/// Periods checked at the end of the run.
pub const CHECK_PERIODS: u64 = 4;

/// Try to fit a puffer to a run (phases from generation 0): the largest front that recurs
/// shifted by `d` every `p` generations over the last [`CHECK_PERIODS`] periods while the
/// population grows. `reach` is how far one generation reaches (lattice steps).
pub fn fit(sp: &Sparse, phases: &[Phase], max_period: u64) -> Option<PufferFit> {
    let n = phases.len() as u64;
    if n < 8 {
        return None;
    }
    let last = n - 1;
    for p in 1..=max_period {
        if CHECK_PERIODS * p > last {
            break;
        }
        let at = |k: u64| &phases[(last - k * p) as usize];
        let bbox = |ph: &Phase| -> Option<[i32; 4]> {
            if ph.is_empty() {
                return None;
            }
            Some([
                ph.iter().map(|c| c.i).min()?,
                ph.iter().map(|c| c.i).max()?,
                ph.iter().map(|c| c.j).min()?,
                ph.iter().map(|c| c.j).max()?,
            ])
        };
        let boxes: Option<Vec<[i32; 4]>> = (0..=CHECK_PERIODS).map(|k| bbox(at(k))).collect();
        let Some(boxes) = boxes else { continue };
        // Candidate displacements: per axis, the side that moved (the other side is debris).
        let delta = |a: [i32; 4], b: [i32; 4]| [b[0] - a[0], b[1] - a[1], b[2] - a[2], b[3] - a[3]];
        let d0 = delta(boxes[1], boxes[0]);
        // (Debris spreading sideways moves a side too, so 0 is always a candidate.)
        let axis = |lo: i32, hi: i32| -> Vec<i32> {
            match (lo, hi) {
                (0, 0) => vec![0],
                (0, h) => vec![h, 0],
                (l, 0) => vec![l, 0],
                (l, h) => vec![l, h, 0],
            }
        };
        for di in axis(d0[0], d0[1]) {
            for dj in axis(d0[2], d0[3]) {
                if di == 0 && dj == 0 {
                    continue;
                }
                let d = [di, dj];
                if let Some(f) = check(sp, phases, last, p, d) {
                    return Some(f);
                }
            }
        }
    }
    None
}

fn check(sp: &Sparse, phases: &[Phase], last: u64, p: u64, d: [i32; 2]) -> Option<PufferFit> {
    let reach = sp.reach;
    let dd = d[0] as i64 * d[0] as i64 + d[1] as i64 * d[1] as i64;
    let at = |k: u64| &phases[(last - k * p) as usize];
    let lead = |ph: &Phase| ph.iter().map(|c| proj(c, d)).max().unwrap_or(0);
    // The lead moves by |d|^2 per period throughout the window.
    for k in 0..CHECK_PERIODS {
        if lead(at(k)) - lead(at(k + 1)) != dd {
            return None;
        }
    }
    // The population grows every period (debris accumulates).
    let (pop_from, pop_to) = (at(CHECK_PERIODS).len(), at(0).len());
    if (0..CHECK_PERIODS).any(|k| at(k).len() <= at(k + 1).len()) {
        return None;
    }
    // Widest front that recurs, as a depth in projection units.
    let unit = (d[0].abs().max(d[1].abs()) as i64).max(1);
    let min_depth = reach as i64 * unit * (d[0].abs() + d[1].abs()) as i64;
    let span = at(0)
        .iter()
        .map(|c| lead(at(0)) - proj(c, d))
        .max()
        .unwrap_or(0);
    let recurs = |w: i64| -> bool {
        (0..CHECK_PERIODS).all(|k| {
            let a = at(k + 1);
            let b = at(k);
            front(b, d, lead(b), w, d) == front(a, d, lead(a), w, [0, 0])
        })
    };
    if !recurs(min_depth) {
        return None;
    }
    // Monotone in w (a recurring front stays recurring when cut shallower): binary search.
    let (mut w, mut hi) = (min_depth, span.max(min_depth));
    while w < hi {
        let mid = w + (hi - w + 1) / 2;
        if recurs(mid) {
            w = mid;
        } else {
            hi = mid - 1;
        }
    }
    // (A front as deep as the whole pattern is fine: a puffer's regular trail recurs too. The
    // growing population rules out spaceships.)
    let fc = front(at(0), d, lead(at(0)), w, [0, 0]).len();
    if fc < 2 {
        return None;
    }
    // The front must lay the debris: cut it off (at an empty band, where there is one) and run
    // it alone; it must keep growing every period. A spaceship flying away from a growing mess
    // recurs only as the spaceship, which alone does not grow.
    let first = at(CHECK_PERIODS);
    let l0 = lead(first);
    let step = (d[0].abs() + d[1].abs()) as i64;
    let band = 3 * reach as i64 * step;
    let mut depths: Vec<i64> = first.iter().map(|c| l0 - proj(c, d)).collect();
    depths.sort_unstable();
    depths.dedup();
    let mut cut = w;
    for win in depths.windows(2).rev() {
        if win[1] <= w && win[1] - win[0] > band {
            cut = win[0];
            break;
        }
    }
    let engine = front(first, d, l0, cut, [0, 0]);
    let run = sp.run(&engine, CHECK_PERIODS * p, TEST_CAP)?;
    if run.len() as u64 != CHECK_PERIODS * p + 1 {
        return None;
    }
    // Alone, it must do exactly what it did in the pattern (within its depth of the lead), so
    // what was cut off did not drive it ...
    for k in 0..=CHECK_PERIODS {
        let a = &run[(k * p) as usize];
        let o = at(CHECK_PERIODS - k);
        if front(a, d, lead(a), cut, [0, 0]) != front(o, d, lead(o), cut, [0, 0]) {
            return None;
        }
    }
    // ... and keep growing.
    let pops: Vec<usize> = (0..=CHECK_PERIODS)
        .map(|k| run[(k * p) as usize].len())
        .collect();
    if pops.windows(2).any(|w| w[1] <= w[0]) {
        return None;
    }
    Some(PufferFit {
        period: p,
        d,
        depth: w,
        front_cells: fc,
        pop_from,
        pop_to,
        generations: last,
    })
}

/// Generations run for a puffer test with this longest period.
pub fn test_gens(max_period: u64) -> u64 {
    (CHECK_PERIODS + 2) * max_period.max(16) + 64
}

/// Run `start` alone and fit a puffer (None if it is not one, or grows past `cap` cells).
pub fn detect(sp: &Sparse, start: &[LCell], max_period: u64, cap: usize) -> Option<PufferFit> {
    let gens = test_gens(max_period);
    let phases = sp.run(start, gens, cap)?;
    if phases.last().is_none_or(|p| p.is_empty()) {
        return None;
    }
    fit(sp, &phases, max_period)
}
