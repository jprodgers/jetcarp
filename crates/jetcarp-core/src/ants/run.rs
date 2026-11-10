//! Run one turmite and classify what it does: a highway, a periodic orbit, symmetric growth,
//! bounded, or still chaotic at the step limit.
//!
//! **Highways, exactly.** The run keeps the symbol of every recent step (colour read, state, tile
//! type, heading; [`super::plane::symbol`]). Every `L / 4` steps it finds the smallest period `p`
//! of the last `L` symbols (the prefix function); when the window repeats at least three times it
//! tests the candidate exactly. From the time `T` of the test it runs `p` more steps, logging the
//! old colour of every cell it writes, and checks
//!
//! 1. the ant at `T + p` has the same state, tile type, and heading as at `T`, moved by a lattice
//!    vector `d`;
//! 2. for every cell `x` it wrote in those `p` steps and every `m >= 0`, the colour of
//!    `x + m·d` at time `T` equals the colour of `x + (m + 1)·d` at time `T + p` (the rays stop
//!    where they leave the field: beyond it every cell is still 0 at both times).
//!
//! Only the ant changes cells, and the colours it reads in one period are set by the colours at
//! the start of the period on the cells it visits, so (1) and (2) make the next period a copy of
//! this one moved by `d`, and the copy satisfies (2) again (the cells ahead that the copy does not
//! write keep the colours that matched). By induction the ant repeats forever: a **highway**
//! when `d != 0`, a **periodic** orbit when `d = 0`. This is a proof for the run, not a hash.
//! The **onset** is the first step of the periodic tail: the step after the last `t` with
//! `symbol(t) != symbol(t + p)` (the tail of symbols is periodic from there, and the moves with
//! it), found in the symbol history or, when that is too short, by running again from the start.
//!
//! **Symmetric growth** (one-tile lattices with reflections: squares, hexagons): whenever the ant
//! stands on its start cell, the field is tested for a reflection `x -> M x + τ` that maps the
//! coloured cells onto themselves (`τ` follows from the cells' centroid, which a symmetric field
//! fixes), cell by cell. Rules such as `LLRR` return to a mirror-symmetric field again and again
//! as they grow; a run with symmetric fields of at least [`SYM_MIN_CELLS`] coloured cells in its
//! last three quarters and no highway is classed **symmetric**.
//!
//! **Bounded**: no highway or orbit up to `max_period`, and the ant's box did not grow in the
//! second half of the run. **Chaotic**: none of these by the step limit.

use super::plane::{Ant, Compiled, Field, Lattice, step};
use crate::rng::Rng;
use crate::turmite::Mite;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Coloured cells a symmetric field needs to count towards the `symmetric` class.
pub const SYM_MIN_CELLS: u64 = 64;

/// What the ant starts on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// Every cell colour 0.
    #[default]
    Empty,
    /// A `w` x `h` block of unit cells around the start, each tile non-zero with probability
    /// `density` (a uniformly random colour from 1), from RNG state `state`.
    Soup {
        w: u32,
        h: u32,
        density: f64,
        state: u64,
    },
}

impl Start {
    pub fn label(&self) -> String {
        match self {
            Start::Empty => "empty".into(),
            Start::Soup {
                w,
                h,
                density,
                state,
            } => format!("soup {w}x{h} d{density} s{state:016x}"),
        }
    }
    /// The field at step 0.
    pub fn field(&self, lat: &Lattice, colors: u8) -> Field {
        match self {
            Start::Empty => Field::new(lat.ntypes),
            Start::Soup {
                w,
                h,
                density,
                state,
            } => {
                let (w, h) = (*w as i32, *h as i32);
                let mut f = Field::new(lat.ntypes);
                let mut r = Rng::new(*state);
                for j in -(h / 2)..h - h / 2 {
                    for i in -(w / 2)..w - w / 2 {
                        for k in 0..lat.ntypes as u8 {
                            if r.next_f64() < *density {
                                let c = 1 + r.below(colors.max(2) as u64 - 1) as u8;
                                f.set(i, j, k, c);
                            }
                        }
                    }
                }
                f
            }
        }
    }
}

/// How long and how hard to look.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Steps before a run is classed as it is (a highway test in flight may add one period).
    pub steps: u64,
    /// Longest highway or orbit period looked for.
    pub max_period: u64,
    /// Test fields for mirror symmetry when the ant is on its start cell.
    #[serde(default = "yes")]
    pub symmetry: bool,
}
fn yes() -> bool {
    true
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            steps: 1_000_000,
            max_period: 20_000,
            symmetry: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Highway,
    Periodic,
    Symmetric,
    Bounded,
    Chaotic,
}

impl Outcome {
    pub const ALL: [Outcome; 5] = [
        Outcome::Highway,
        Outcome::Periodic,
        Outcome::Symmetric,
        Outcome::Bounded,
        Outcome::Chaotic,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Highway => "highway",
            Outcome::Periodic => "periodic",
            Outcome::Symmetric => "symmetric",
            Outcome::Bounded => "bounded",
            Outcome::Chaotic => "chaotic",
        }
    }
    pub fn parse(s: &str) -> Result<Outcome, String> {
        Outcome::ALL
            .into_iter()
            .find(|o| o.name() == s.trim().to_ascii_lowercase())
            .ok_or_else(|| {
                format!("unknown outcome '{s}' (highway, periodic, symmetric, bounded, chaotic)")
            })
    }
}

/// The exact highway (or orbit) test that succeeded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proof {
    /// Step `T` the test started at.
    pub at: u64,
    pub period: u64,
    pub d: [i32; 2],
    /// Cells compared (rays included).
    pub cells: u64,
}

/// Mirror-symmetric fields seen with the ant on its start cell.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Symmetry {
    /// Symmetric fields with at least [`SYM_MIN_CELLS`] coloured cells (after one, the next
    /// test waits 1/64 of the steps run so far).
    pub count: u64,
    pub first_step: u64,
    pub last_step: u64,
    /// Coloured cells of the last one.
    pub last_cells: u64,
    /// Its reflection `[a, b, c, d]` (lattice coordinates) and translation.
    pub reflection: [i32; 4],
    pub translation: [i32; 2],
}

/// What a run found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AntRun {
    pub outcome: Outcome,
    /// Highway or orbit period, displacement per period (lattice vector), and onset step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset: Option<u64>,
    /// Highway speed: distance per step in cell sizes (the square root of a tile's area).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Steps run.
    pub steps: u64,
    /// Coloured cells at the end.
    pub cells: u64,
    /// Unit cells the ant reached: `[i0, j0, i1, j1]`, relative to its start.
    pub reach: [i32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symmetry: Option<Symmetry>,
}

struct Runner<'a> {
    lat: &'a Lattice,
    m: Compiled,
    f: Field,
    a: Ant,
    t: u64,
    ring: Vec<u32>,
    mask: u64,
    nonzero: u64,
    sum: [i64; 2],
    reach: [i32; 4],
    sym: Symmetry,
}

impl<'a> Runner<'a> {
    fn new(lat: &'a Lattice, mite: &Mite, start: &Start, ring: usize) -> Runner<'a> {
        let f = start.field(lat, mite.colors);
        let mut nonzero = 0;
        let mut sum = [0i64; 2];
        for (i, j, _, _) in f.nonzero() {
            nonzero += 1;
            sum[0] += i as i64;
            sum[1] += j as i64;
        }
        Runner {
            lat,
            m: Compiled::new(mite, lat),
            f,
            a: Ant::default(),
            t: 0,
            ring: vec![0; ring],
            mask: ring as u64 - 1,
            nonzero,
            sum,
            reach: [0; 4],
            sym: Symmetry::default(),
        }
    }

    #[inline]
    fn one(&mut self, log: Option<&mut Vec<(i32, i32, u8, u8)>>) {
        let (i, j, k) = (self.a.i, self.a.j, self.a.k);
        let (sym, old, new) = step(self.lat, &self.m, &mut self.f, &mut self.a);
        self.ring[(self.t & self.mask) as usize] = sym;
        self.t += 1;
        if (old == 0) != (new == 0) {
            if new != 0 {
                self.nonzero += 1;
                self.sum[0] += i as i64;
                self.sum[1] += j as i64;
            } else {
                self.nonzero -= 1;
                self.sum[0] -= i as i64;
                self.sum[1] -= j as i64;
            }
        }
        if let Some(l) = log {
            l.push((i, j, k, old));
        }
        let a = &self.a;
        self.reach = [
            self.reach[0].min(a.i),
            self.reach[1].min(a.j),
            self.reach[2].max(a.i),
            self.reach[3].max(a.j),
        ];
    }

    fn sym_at(&self, t: u64) -> u32 {
        self.ring[(t & self.mask) as usize]
    }

    /// Smallest period of the last `len` symbols, if it repeats at least three times.
    fn window_period(&self, len: usize, max_p: u64, pi: &mut Vec<u32>) -> Option<u64> {
        let len = len.min(self.t as usize).min(self.ring.len());
        if len < 6 {
            return None;
        }
        let base = self.t - len as u64;
        let s = |x: usize| self.sym_at(base + x as u64);
        pi.clear();
        pi.resize(len, 0);
        for q in 1..len {
            let mut k = pi[q - 1] as usize;
            let c = s(q);
            while k > 0 && s(k) != c {
                k = pi[k - 1] as usize;
            }
            if s(k) == c {
                k += 1;
            }
            pi[q] = k as u32;
        }
        let p = (len - pi[len - 1] as usize) as u64;
        (p <= max_p && 3 * p <= len as u64).then_some(p)
    }

    /// Run `p` steps and test the highway conditions exactly (module docs). Returns the
    /// displacement and the cells compared.
    fn verify(&mut self, p: u64) -> Option<([i32; 2], u64)> {
        let a0 = self.a;
        let mut log = Vec::with_capacity(p as usize);
        for _ in 0..p {
            self.one(Some(&mut log));
        }
        let a1 = self.a;
        if (a1.k, a1.h, a1.q) != (a0.k, a0.h, a0.q) {
            return None;
        }
        let d = [a1.i - a0.i, a1.j - a0.j];
        // Colours at time T of the cells written since.
        let mut before: HashMap<(i32, i32, u8), u8> = HashMap::with_capacity(log.len());
        for &(i, j, k, old) in &log {
            before.entry((i, j, k)).or_insert(old);
        }
        let then = |i: i32, j: i32, k: u8| -> u8 {
            match before.get(&(i, j, k)) {
                Some(&c) => c,
                None => self.f.get(i, j, k),
            }
        };
        let written: HashSet<(i32, i32, u8)> = before.keys().copied().collect();
        let mut cells = 0u64;
        for &(i, j, k) in &written {
            if d == [0, 0] {
                cells += 1;
                if then(i, j, k) != self.f.get(i, j, k) {
                    return None;
                }
                continue;
            }
            // A ray starting here is covered by the one from x - d.
            if written.contains(&(i - d[0], j - d[1], k)) {
                continue;
            }
            let (mut x, mut y) = (i, j);
            while self.f.contains(x, y) {
                cells += 1;
                if then(x, y, k) != self.f.get(x + d[0], y + d[1], k) {
                    return None;
                }
                x += d[0];
                y += d[1];
            }
        }
        Some((d, cells))
    }

    /// Is the field mirror symmetric now? Records it in `self.sym`.
    fn check_symmetry(&mut self) -> bool {
        let n = self.nonzero as i64;
        if n < SYM_MIN_CELLS as i64 {
            return false;
        }
        let s = self.sum;
        for &m in &self.lat.reflections {
            let ms = [
                m[0] as i64 * s[0] + m[1] as i64 * s[1],
                m[2] as i64 * s[0] + m[3] as i64 * s[1],
            ];
            let (ti, tj) = (s[0] - ms[0], s[1] - ms[1]);
            if ti % n != 0 || tj % n != 0 {
                continue;
            }
            let tau = [(ti / n) as i32, (tj / n) as i32];
            let f = &self.f;
            let ok = f.nonzero().iter().all(|&(i, j, _, c)| {
                let (mi, mj) = (m[0] * i + m[1] * j + tau[0], m[2] * i + m[3] * j + tau[1]);
                f.get(mi, mj, 0) == c
            });
            if ok {
                if self.sym.count == 0 {
                    self.sym.first_step = self.t;
                }
                self.sym.count += 1;
                self.sym.last_step = self.t;
                self.sym.last_cells = self.nonzero;
                self.sym.reflection = m;
                self.sym.translation = tau;
                return true;
            }
        }
        false
    }
}

/// Smallest power of two holding the symbol history a run needs.
fn ring_size(max_period: u64) -> (usize, usize) {
    let window = (3 * max_period).max(1024) as usize;
    let ring = (2 * window + max_period as usize + 1024).next_power_of_two();
    (window, ring)
}

/// The first step of the periodic tail of period `p` that holds from step `upto`, by running
/// again from the start (when the history was too short).
fn onset_by_rerun(lat: &Lattice, mite: &Mite, start: &Start, p: u64, upto: u64) -> u64 {
    let mut r = Runner::new(lat, mite, start, (p as usize + 1).next_power_of_two());
    let mut last_bad: Option<u64> = None;
    while r.t < upto + p {
        r.one(None);
        if r.t > p {
            let t = r.t - 1 - p;
            if t < upto && r.sym_at(t) != r.sym_at(t + p) {
                last_bad = Some(t);
            }
        }
    }
    last_bad.map(|t| t + 1).unwrap_or(0)
}

/// Run a turmite from `start` on the lattice and classify it.
pub fn run(lat: &Lattice, mite: &Mite, start: &Start, lim: &Limits) -> AntRun {
    let (window, ring) = ring_size(lim.max_period.max(1));
    let every = (window as u64 / 4).max(256);
    let mut r = Runner::new(lat, mite, start, ring);
    let sym_on = lim.symmetry && !lat.reflections.is_empty();
    let mut pi = Vec::new();
    let mut next_check = every;
    let mut next_sym = 0;
    let mut half_reach: Option<[i32; 4]> = None;
    let mut proof = None;
    while r.t < lim.steps {
        r.one(None);
        // After a symmetric field, test again only 1/64 of the run later (a field costs its
        // area to test, and symmetric rules return to the start cell very often).
        if sym_on && r.a.i == 0 && r.a.j == 0 && r.a.k == 0 && r.t >= next_sym && r.check_symmetry()
        {
            next_sym = r.t + r.t / 64;
        }
        if r.t == next_check || r.t == lim.steps {
            next_check += every;
            if half_reach.is_none() && r.t >= lim.steps / 2 {
                half_reach = Some(r.reach);
            }
            if let Some(p) = r.window_period(window, lim.max_period, &mut pi) {
                let at = r.t;
                if let Some((d, cells)) = r.verify(p) {
                    proof = Some(Proof {
                        at,
                        period: p,
                        d,
                        cells,
                    });
                    break;
                }
            }
        }
    }
    let lattice_speed = |d: [i32; 2], p: u64| {
        let w = lat.world(d);
        (crate::detmath::hypot(w[0], w[1]) / lat.cell_size()) / p as f64
    };
    let sym = (r.sym.count > 0).then(|| r.sym.clone());
    let mut out = AntRun {
        outcome: Outcome::Chaotic,
        period: None,
        d: None,
        onset: None,
        speed: None,
        steps: r.t,
        cells: r.nonzero,
        reach: r.reach,
        proof: None,
        symmetry: sym.clone(),
    };
    if let Some(pf) = proof {
        let p = pf.period;
        let now = r.t;
        // Scan back from T for the last step that breaks the period.
        let lo = now.saturating_sub(r.ring.len() as u64);
        let mut t = pf.at;
        let mut onset = None;
        while t > lo {
            let x = t - 1;
            if r.sym_at(x) != r.sym_at(x + p) {
                onset = Some(t);
                break;
            }
            t = x;
        }
        let onset = match onset {
            Some(o) => o,
            None if lo == 0 => 0,
            None => onset_by_rerun(lat, mite, start, p, pf.at),
        };
        out.outcome = if pf.d == [0, 0] {
            Outcome::Periodic
        } else {
            Outcome::Highway
        };
        out.period = Some(p);
        out.d = Some(pf.d);
        out.onset = Some(onset);
        if pf.d != [0, 0] {
            out.speed = Some((lattice_speed(pf.d, p) * 1e6).round() / 1e6);
        }
        out.proof = Some(pf);
        return out;
    }
    if let Some(s) = &sym
        && s.last_step >= lim.steps / 4
    {
        out.outcome = Outcome::Symmetric;
    } else if half_reach == Some(r.reach) {
        out.outcome = Outcome::Bounded;
    }
    out
}
