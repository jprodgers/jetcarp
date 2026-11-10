//! John von Neumann's 29-state cellular automaton (`JvN29`).
//!
//! Implemented from the rule as stated in J. von Neumann, *Theory of Self-Reproducing Automata*,
//! edited and completed by A. W. Burks, University of Illinois Press, 1966: Sec. 2.8.1 (the list of
//! states and the transition rule (T.1)-(T.4)), Sec. 2.8.2 (verbal summary) and Sec. 2.8.3
//! (illustrations, including the worked construction trace). Cross-checked against the Wikipedia
//! article "Von Neumann cellular automaton". No code, tables, or patterns from Golly were used.
//!
//! The rule runs on the square grid with the four edge neighbours. It is direction dependent, so
//! other tilings are rejected. Cells past a dead edge read as the ground state U, whatever the
//! boundary setting (a "live" edge has no meaning here).
//!
//! # States (JETCARP numbering)
//!
//! | Number | State | Book symbol |
//! | --- | --- | --- |
//! | 0 | ground (unexcitable) | U |
//! | 1-8 | sensitized: `S`, `S0`, `S1`, `S00`, `S01`, `S10`, `S11`, `S000` | S_Σ |
//! | 9-24 | transmission `9 + 8u + 2a + e`: class u (0 ordinary, 1 special), direction a (0 east, 1 north, 2 west, 3 south), excitation e | T_uae |
//! | 25-28 | confluent `25 + 2e + e'`: e now, e' next | C_ee' |
//!
//! # Transition rule (one step, from the states at time t)
//!
//! "X points at me" means X is a neighbouring transmission cell whose output direction is this cell.
//! - **Transmission T_uae.** Killed (to U) if an excited transmission cell of the other class
//!   points at it, from any side. Otherwise excited if an excited transmission cell of the same class
//!   points at it from one of its three input sides, or an excited confluent cell lies on one of its
//!   input sides. Otherwise quiescent. (Delay 1.)
//! - **Confluent C_ee'.** Killed if an excited special cell points at it. Otherwise it becomes
//!   C_e'1 if at least one ordinary transmission cell points at it and all of those are excited,
//!   else C_e'0. Its visible excitation is e, so the delay is 2.
//! - **Ground U.** Becomes `S` (S with the empty string) if any excited transmission cell points at it.
//! - **Sensitized S_Σ.** Always changes: to S_Σ1 if an excited transmission cell points at it,
//!   else S_Σ0. The strings 0000, 0001, 001, 010, 011, 100, 101, 110, 111 are the quiescent states
//!   ordinary east, north, west, south, special east, north, west, south, and C00.
//!
//! Confluent cells never sensitize ground or feed sensitized cells; sensitized cells are never
//! killed. Kill dominates reception but not emission: a cell excited at t still delivers its
//! stimulus at t + 1 even if it is killed at the same time.

use crate::geom::P;
use crate::step::MIN_PAR;
use crate::topology::Topology;
use crate::world::Field;
use rayon::prelude::*;

/// Number of states.
pub const STATES: u8 = 29;
/// The ground (unexcitable) state.
pub const U: u8 = 0;
/// First sensitized state (S with the empty string).
pub const S_EMPTY: u8 = 1;
const T_BASE: u8 = 9;
const C_BASE: u8 = 25;

/// Directions in the book's order: a = 0 right (east), 1 up (north), 2 left (west), 3 down (south).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    E = 0,
    N = 1,
    W = 2,
    S = 3,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::E, Dir::N, Dir::W, Dir::S];
    pub fn from_index(k: usize) -> Dir {
        Dir::ALL[k & 3]
    }
    pub fn opposite(self) -> Dir {
        Dir::from_index(self as usize + 2)
    }
    /// Lattice offset `(di, dj)`. Rows grow downward on screen, so north is `dj = -1`.
    pub fn offset(self) -> (i32, i32) {
        match self {
            Dir::E => (1, 0),
            Dir::N => (0, -1),
            Dir::W => (-1, 0),
            Dir::S => (0, 1),
        }
    }
    pub fn arrow(self) -> char {
        match self {
            Dir::E => '>',
            Dir::N => '^',
            Dir::W => '<',
            Dir::S => 'v',
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Dir::E => "east",
            Dir::N => "north",
            Dir::W => "west",
            Dir::S => "south",
        }
    }
}

/// A decoded state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Ground,
    /// Index 0..8 into `S`, `S0`, `S1`, `S00`, `S01`, `S10`, `S11`, `S000`.
    Sensitized(u8),
    Transmission {
        special: bool,
        dir: Dir,
        excited: bool,
    },
    Confluent {
        now: bool,
        next: bool,
    },
}

/// The sensitized strings, in state order.
pub const SENSITIZED: [&str; 8] = ["", "0", "1", "00", "01", "10", "11", "000"];

/// Transmission state number.
pub const fn t(special: bool, dir: Dir, excited: bool) -> u8 {
    T_BASE + 8 * special as u8 + 2 * dir as u8 + excited as u8
}
/// Confluent state number C_{now,next}.
pub const fn c(now: bool, next: bool) -> u8 {
    C_BASE + 2 * now as u8 + next as u8
}
/// Sensitized state for a string of bits from the list (S.1), or `None`.
pub fn sensitized(bits: &str) -> Option<u8> {
    SENSITIZED
        .iter()
        .position(|&s| s == bits)
        .map(|k| S_EMPTY + k as u8)
}

pub fn decode(s: u8) -> Cell {
    match s {
        1..=8 => Cell::Sensitized(s - S_EMPTY),
        9..=24 => {
            let k = s - T_BASE;
            Cell::Transmission {
                special: k >= 8,
                dir: Dir::from_index(((k >> 1) & 3) as usize),
                excited: k & 1 == 1,
            }
        }
        25..=28 => {
            let k = s - C_BASE;
            Cell::Confluent {
                now: k & 2 != 0,
                next: k & 1 != 0,
            }
        }
        _ => Cell::Ground,
    }
}

pub fn encode(cell: Cell) -> u8 {
    match cell {
        Cell::Ground => U,
        Cell::Sensitized(k) => S_EMPTY + k.min(7),
        Cell::Transmission {
            special,
            dir,
            excited,
        } => t(special, dir, excited),
        Cell::Confluent { now, next } => c(now, next),
    }
}

/// What a sensitized state becomes after receiving bit 0 or 1 (book lists (S.1)-(S.3)).
const SENS_NEXT: [[u8; 2]; 8] = [
    [2, 3],                                             // S    -> S0, S1
    [4, 5],                                             // S0   -> S00, S01
    [6, 7],                                             // S1   -> S10, S11
    [8, t(false, Dir::W, false)],                       // S00  -> S000, 001
    [t(false, Dir::S, false), t(true, Dir::E, false)],  // S01  -> 010, 011
    [t(true, Dir::N, false), t(true, Dir::W, false)],   // S10  -> 100, 101
    [t(true, Dir::S, false), c(false, false)],          // S11  -> 110, 111
    [t(false, Dir::E, false), t(false, Dir::N, false)], // S000 -> 0000, 0001
];

/// The transition function. `nb[d]` is the neighbour in direction `d` (book order E, N, W, S);
/// missing neighbours are `U`.
pub fn next_state(me: u8, nb: [u8; 4]) -> u8 {
    // The neighbour in direction d, if it is a transmission cell pointing back at this cell:
    // (special, excited).
    let pointing = |d: usize| match decode(nb[d]) {
        Cell::Transmission {
            special,
            dir,
            excited,
        } if dir == Dir::from_index(d).opposite() => Some((special, excited)),
        _ => None,
    };
    let stimulated = || (0..4).any(|d| matches!(pointing(d), Some((_, true))));
    match decode(me) {
        Cell::Ground => {
            if stimulated() {
                S_EMPTY
            } else {
                U
            }
        }
        Cell::Sensitized(k) => SENS_NEXT[k as usize][stimulated() as usize],
        Cell::Transmission { special, dir, .. } => {
            if (0..4).any(|d| pointing(d) == Some((!special, true))) {
                return U;
            }
            let excited = (0..4).filter(|&d| d != dir as usize).any(|d| {
                pointing(d) == Some((special, true))
                    || matches!(decode(nb[d]), Cell::Confluent { now: true, .. })
            });
            t(special, dir, excited)
        }
        Cell::Confluent { next, .. } => {
            if (0..4).any(|d| pointing(d) == Some((true, true))) {
                return U;
            }
            let (mut any, mut all) = (false, true);
            for d in 0..4 {
                if let Some((false, e)) = pointing(d) {
                    any = true;
                    all &= e;
                }
            }
            c(next, any && all)
        }
    }
}

/// One synchronous step on the square grid, written into `f.back` (the caller commits it).
pub(crate) fn step(topo: &Topology, f: &mut Field) {
    let Some(g) = topo.rows() else { return };
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let (u, j, _) = g.decompose(i);
            let x = u + g.row_start[j as usize];
            let mut nb = [U; 4];
            for d in Dir::ALL {
                let (di, dj) = d.offset();
                if let Some(k) = g.resolve(x + di, j + dj, 0) {
                    nb[d as usize] = a[k];
                }
            }
            *o = if a[i] == U && nb == [U; 4] {
                U
            } else {
                next_state(a[i], nb)
            };
        });
}

/// Short token for a state, as used in [`picture`]: `.` ground; `S`, `S0`, ... sensitized;
/// `o>` ordinary east quiescent, `o>*` excited (`^` north, `<` west, `v` south); `s>` special;
/// `C00`, `C01`, `C10`, `C11` confluent.
pub fn token(s: u8) -> String {
    match decode(s) {
        Cell::Ground => ".".into(),
        Cell::Sensitized(k) => format!("S{}", SENSITIZED[k as usize]),
        Cell::Transmission {
            special,
            dir,
            excited,
        } => format!(
            "{}{}{}",
            if special { 's' } else { 'o' },
            dir.arrow(),
            if excited { "*" } else { "" }
        ),
        Cell::Confluent { now, next } => format!("C{}{}", now as u8, next as u8),
    }
}

/// Parse a [`token`] (also `U` for ground and `C` for C00).
pub fn parse_token(tok: &str) -> Option<u8> {
    match tok {
        "." | "U" => return Some(U),
        "C" => return Some(c(false, false)),
        _ => {}
    }
    if let Some(bits) = tok.strip_prefix('S') {
        return sensitized(bits);
    }
    if let Some(cb) = tok.strip_prefix('C') {
        let b: Vec<char> = cb.chars().collect();
        return match b.as_slice() {
            [x @ ('0' | '1'), y @ ('0' | '1')] => Some(c(*x == '1', *y == '1')),
            _ => None,
        };
    }
    let mut ch = tok.chars();
    let special = match ch.next()? {
        'o' => false,
        's' => true,
        _ => return None,
    };
    let dir = match ch.next()? {
        '>' => Dir::E,
        '^' => Dir::N,
        '<' => Dir::W,
        'v' => Dir::S,
        _ => return None,
    };
    let excited = match ch.as_str() {
        "" => false,
        "*" => true,
        _ => return None,
    };
    Some(t(special, dir, excited))
}

/// Parse a picture: one row of whitespace-separated tokens per line, top row first.
/// Returns `(x, y, state)` for every non-ground cell, with y growing downward.
pub fn picture(rows: &[&str]) -> Result<Vec<(i32, i32, u8)>, String> {
    let mut out = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        for (x, tok) in row.split_whitespace().enumerate() {
            let s = parse_token(tok)
                .ok_or_else(|| format!("row {y}, column {x}: '{tok}' is not a JvN29 state"))?;
            if s != U {
                out.push((x as i32, y as i32, s));
            }
        }
    }
    Ok(out)
}

/// Readable name for a state (for state pickers).
pub fn state_name(s: u8) -> String {
    match decode(s) {
        Cell::Ground => "Ground U".into(),
        Cell::Sensitized(0) => "Sensitized S".into(),
        Cell::Sensitized(k) => format!("Sensitized S{}", SENSITIZED[k as usize]),
        Cell::Transmission {
            special,
            dir,
            excited,
        } => format!(
            "{} {} {}{}",
            if special { "Special" } else { "Ordinary" },
            dir.arrow(),
            dir.name(),
            if excited { ", excited" } else { "" }
        ),
        Cell::Confluent { now, next } => format!(
            "Confluent C{}{}{}",
            now as u8,
            next as u8,
            match (now, next) {
                (false, false) => "",
                (false, true) => " (fires next)",
                (true, false) => ", excited",
                (true, true) => ", excited, fires next",
            }
        ),
    }
}

/// Colours per state. Ground is the empty background. Sensitized states are amber (deeper with
/// longer strings), ordinary transmission blue, special transmission red, confluent green; excited
/// forms are much brighter. The four directions differ slightly in hue, and large renders also
/// draw an arrow on each transmission cell.
pub const COLORS: [[u8; 4]; 29] = {
    let mut lut = [[0u8; 4]; 29];
    lut[0] = crate::render::BACKGROUND;
    let sens: [[u8; 3]; 8] = [
        [0xff, 0xf0, 0x9a],
        [0xf8, 0xd8, 0x6a],
        [0xf8, 0xd8, 0x6a],
        [0xee, 0xb8, 0x40],
        [0xee, 0xb8, 0x40],
        [0xee, 0xb8, 0x40],
        [0xee, 0xb8, 0x40],
        [0xe0, 0x96, 0x28],
    ];
    // Per class, per direction (E, N, W, S): quiescent, then excited.
    let quiet: [[[u8; 3]; 4]; 2] = [
        [
            [0x2f, 0x6f, 0xd6],
            [0x3a, 0x84, 0xe0],
            [0x3a, 0x5c, 0xc8],
            [0x2c, 0x7c, 0xc0],
        ],
        [
            [0xc8, 0x34, 0x4c],
            [0xd8, 0x46, 0x5c],
            [0xb8, 0x30, 0x6c],
            [0xcc, 0x44, 0x3a],
        ],
    ];
    let hot: [[u8; 3]; 2] = [[0xa8, 0xf0, 0xff], [0xff, 0xb0, 0xc8]];
    let conf: [[u8; 3]; 4] = [
        [0x2e, 0x9a, 0x52],
        [0x5c, 0xc4, 0x6c],
        [0xc8, 0xff, 0x8a],
        [0xe8, 0xff, 0xb8],
    ];
    let mut k = 0;
    while k < 8 {
        let s = sens[k];
        lut[1 + k] = [s[0], s[1], s[2], 0xff];
        k += 1;
    }
    let mut u = 0;
    while u < 2 {
        let mut d = 0;
        while d < 4 {
            let q = quiet[u][d];
            let h = hot[u];
            lut[9 + 8 * u + 2 * d] = [q[0], q[1], q[2], 0xff];
            lut[9 + 8 * u + 2 * d + 1] = [h[0], h[1], h[2], 0xff];
            d += 1;
        }
        u += 1;
    }
    let mut k = 0;
    while k < 4 {
        let s = conf[k];
        lut[25 + k] = [s[0], s[1], s[2], 0xff];
        k += 1;
    }
    lut
};

/// Arrow (a triangle in world coordinates) showing a transmission cell's output direction.
pub fn arrow_triangle(topo: &Topology, i: usize, s: u8) -> Option<[P; 3]> {
    let Cell::Transmission { dir, .. } = decode(s) else {
        return None;
    };
    let g = topo.rows()?;
    let ctr = topo.centroid(i);
    let (di, dj) = dir.offset();
    let v = [
        di as f64 * g.a1[0] + dj as f64 * g.a2[0],
        di as f64 * g.a1[1] + dj as f64 * g.a2[1],
    ];
    let w = if di != 0 { g.a2 } else { g.a1 };
    let at = |a: f64, b: f64| [ctr[0] + a * v[0] + b * w[0], ctr[1] + a * v[1] + b * w[1]];
    Some([at(0.34, 0.0), at(-0.18, 0.2), at(-0.18, -0.2)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_and_tokens_roundtrip() {
        for s in 0..STATES {
            assert_eq!(encode(decode(s)), s);
            assert_eq!(parse_token(&token(s)), Some(s), "{}", token(s));
        }
        assert_eq!(t(false, Dir::E, false), 9);
        assert_eq!(c(true, true), 28);
    }
}
