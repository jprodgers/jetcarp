//! The rules an ant search runs: turn strings and turmite tables, enumerated in a fixed order
//! and reduced by the obvious equivalences.
//!
//! **Alphabets** (the turns that differ on the tiling): squares and other tilings whose cells
//! have 4 or more edges `L R N U`; hexagons `L R N U L2 R2` (60 and 120 degrees each way, none,
//! and back); triangles `L R N` (on a degree-3 cell `U` is `R`, and after entering a triangle
//! "straight on" is one of its two other edges, so `L`, `R`, and `N` are its three exits). A
//! mix of degrees uses `L R N U`; `alphabet` overrides any of these.
//!
//! **Turn strings** of each length, in alphabet order, keeping one of each class:
//!
//! - a string that repeats a shorter one (`RLRL`) moves exactly as the shorter one (the turn
//!   depends only on the colour modulo its period), so it is skipped;
//! - a string with no `L`, `R`, `L2`, or `R2` on a tiling of even degrees walks straight or
//!   back and forth, so it is skipped;
//! - on an empty field of squares or hexagons, a string starting with `N` walks straight over
//!   fresh cells forever, so it is skipped (with soups it is kept);
//! - on a lattice where mirror images behave as mirror images ([`Lattice::mirror`]: squares,
//!   hexagons) a string and its mirror (`L` and `R` swapped) are one class, and the smaller in
//!   alphabet order is kept.
//!
//! **Tables** with `states` states and `colors` colours: every table (or a seeded sample when
//! there are more than `all_max`), keeping one of each class under the mirror and under the
//! relabellings that keep state 0 and colour 0 (the start state and the empty colour), and only
//! tables whose every state is reachable from state 0 and whose every colour is written.

use super::plane::Lattice;
use crate::rng::Rng;
use crate::rules::{Rule, Turn};
use crate::turmite::{Entry, Mite};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A rule notation or a bare turn string or table (`RL2N`, `1R1,1L0/1R1,0N0`).
pub fn parse_rule(r: &str) -> Result<Mite, String> {
    let t = r.trim();
    let low = t.to_ascii_lowercase();
    let text = if low.starts_with("ant") || low.starts_with("turmite") {
        t.to_string()
    } else {
        format!("Turmite {t}")
    };
    match Rule::parse(&text) {
        Ok((Rule::Turmite(m), _)) => Ok(m),
        Ok(_) => Err(format!("'{r}' is not a turmite")),
        Err(e) => Err(format!("'{r}': {e}")),
    }
}

/// Turn strings of these lengths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Strings {
    pub min: usize,
    pub max: usize,
    /// Turn letters to use (default: the tiling's alphabet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alphabet: Option<String>,
}

/// Turmite tables of this size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tables {
    pub states: u8,
    pub colors: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alphabet: Option<String>,
    /// Enumerate every table up to this many (before reduction); sample above it.
    #[serde(default = "all_max")]
    pub all_max: u64,
    /// Tables kept when sampling.
    #[serde(default = "sample")]
    pub sample: u64,
    #[serde(default = "one")]
    pub seed: u64,
}
fn all_max() -> u64 {
    1_000_000
}
fn sample() -> u64 {
    10_000
}
fn one() -> u64 {
    1
}

/// The tiling's alphabet (module docs).
pub fn alphabet(lat: &Lattice) -> Vec<Turn> {
    let d = lat.degrees();
    if d == [6] {
        vec![Turn::L, Turn::R, Turn::N, Turn::U, Turn::L2, Turn::R2]
    } else if d == [3] {
        vec![Turn::L, Turn::R, Turn::N]
    } else {
        vec![Turn::L, Turn::R, Turn::N, Turn::U]
    }
}

fn parse_alphabet(lat: &Lattice, a: &Option<String>) -> Result<Vec<Turn>, String> {
    match a {
        None => Ok(alphabet(lat)),
        Some(s) => {
            let mut t = crate::turmite::parse_turns(s)?;
            let mut seen = HashSet::new();
            t.retain(|x| seen.insert(*x));
            if t.len() < 2 {
                return Err("an alphabet needs at least two turns".into());
            }
            Ok(t)
        }
    }
}

fn rank(alpha: &[Turn], t: Turn) -> usize {
    alpha.iter().position(|&x| x == t).unwrap_or(usize::MAX)
}

/// Does the string repeat a shorter one?
fn repeats(s: &[Turn]) -> bool {
    let n = s.len();
    (1..n).any(|p| n.is_multiple_of(p) && (p..n).all(|i| s[i] == s[i - p]))
}

/// Every turn string of the given lengths, reduced (module docs), in order. `empty`: the runs
/// start on an empty field, where on squares and hexagons a string starting with `N` walks
/// straight over fresh cells forever, so those are skipped too.
pub fn strings(lat: &Lattice, sp: &Strings, empty: bool) -> Result<Vec<Mite>, String> {
    let alpha = parse_alphabet(lat, &sp.alphabet)?;
    if sp.min < 2 || sp.max < sp.min || sp.max > crate::turmite::MAX_COLORS {
        return Err(format!(
            "turn strings have 2 to {} letters (min {} max {})",
            crate::turmite::MAX_COLORS,
            sp.min,
            sp.max
        ));
    }
    let even = lat.degrees().iter().all(|d| d % 2 == 0);
    let straight = empty && even && lat.ntypes == 1;
    let mirror_ok = lat.mirror && alpha.iter().all(|t| alpha.contains(&t.mirror()));
    let mut out = Vec::new();
    for len in sp.min..=sp.max {
        let total = (alpha.len() as u64)
            .checked_pow(len as u32)
            .unwrap_or(u64::MAX);
        if total > 20_000_000 {
            return Err(format!(
                "{total} turn strings of length {len}: too many (use shorter strings or a smaller alphabet)"
            ));
        }
        let mut digits = vec![0usize; len];
        for _ in 0..total {
            let s: Vec<Turn> = digits.iter().map(|&d| alpha[d]).collect();
            let keep = !repeats(&s)
                && !(straight && s[0] == Turn::N)
                && !(even && s.iter().all(|t| matches!(t, Turn::N | Turn::U)))
                && !(mirror_ok && {
                    let m: Vec<usize> = s.iter().map(|t| rank(&alpha, t.mirror())).collect();
                    m < digits
                });
            if keep {
                out.push(Mite::from_turns(&s));
            }
            // Next string (last letter fastest).
            for d in digits.iter_mut().rev() {
                *d += 1;
                if *d < alpha.len() {
                    break;
                }
                *d = 0;
            }
        }
    }
    Ok(out)
}

fn perms(n: usize) -> Vec<Vec<usize>> {
    // Permutations of 1..n keeping 0 fixed.
    let mut out = vec![];
    let mut rest: Vec<usize> = (1..n).collect();
    fn go(k: usize, v: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if k == v.len() {
            let mut p = vec![0];
            p.extend_from_slice(v);
            out.push(p);
            return;
        }
        for i in k..v.len() {
            v.swap(k, i);
            go(k + 1, v, out);
            v.swap(k, i);
        }
    }
    go(0, &mut rest, &mut out);
    out
}

/// A table as digits (write, turn rank, next) per entry, for ordering.
fn code(m: &Mite, alpha: &[Turn]) -> Vec<usize> {
    m.table
        .iter()
        .flat_map(|e| [e.write as usize, rank(alpha, e.turn), e.next as usize])
        .collect()
}

/// The table with states relabelled by `sp` and colours by `cp` (new label of old `x` is
/// `p[x]`).
fn relabel(m: &Mite, sp: &[usize], cp: &[usize]) -> Mite {
    let (n, k) = (m.states as usize, m.colors as usize);
    let mut t = vec![m.table[0]; n * k];
    for q in 0..n {
        for c in 0..k {
            let e = m.table[q * k + c];
            t[sp[q] * k + cp[c]] = Entry {
                write: cp[e.write as usize] as u8,
                turn: e.turn,
                next: sp[e.next as usize] as u8,
            };
        }
    }
    Mite {
        table: t,
        ..m.clone()
    }
}

/// Every state reachable from 0 and every colour written by a reachable state.
fn live(m: &Mite) -> bool {
    let (n, k) = (m.states as usize, m.colors as usize);
    let mut seen = vec![false; n];
    seen[0] = true;
    let mut stack = vec![0usize];
    let mut written = vec![false; k];
    written[0] = true;
    while let Some(q) = stack.pop() {
        for c in 0..k {
            let e = m.table[q * k + c];
            written[e.write as usize] = true;
            if !seen[e.next as usize] {
                seen[e.next as usize] = true;
                stack.push(e.next as usize);
            }
        }
    }
    seen.iter().all(|&x| x) && written.iter().all(|&x| x)
}

/// Every table of the given size (or a sample), reduced (module docs), in order.
pub fn tables(lat: &Lattice, sp: &Tables) -> Result<Vec<Mite>, String> {
    let alpha = parse_alphabet(lat, &sp.alphabet)?;
    let (n, k) = (sp.states as usize, sp.colors as usize);
    if !(1..=4).contains(&n) || !(2..=4).contains(&k) {
        return Err("searched tables have 1 to 4 states and 2 to 4 colours".into());
    }
    let per = (k * alpha.len() * n) as u64;
    let entries = (n * k) as u32;
    let total = per.checked_pow(entries);
    let mirror_ok = lat.mirror && alpha.iter().all(|t| alpha.contains(&t.mirror()));
    let sps = perms(n);
    let cps = perms(k);
    let make = |mut idx: u64| -> Mite {
        let mut table = Vec::with_capacity(n * k);
        for _ in 0..n * k {
            let mut x = (idx % per) as usize;
            idx /= per;
            let next = x % n;
            x /= n;
            let turn = alpha[x % alpha.len()];
            x /= alpha.len();
            table.push(Entry {
                write: x as u8,
                turn,
                next: next as u8,
            });
        }
        Mite {
            states: n as u8,
            colors: k as u8,
            table,
        }
    };
    let canonical = |m: &Mite| -> Option<Mite> {
        if !live(m) {
            return None;
        }
        let mine = code(m, &alpha);
        let mut best = mine.clone();
        let mut best_m = m.clone();
        for s in &sps {
            for c in &cps {
                let r = relabel(m, s, c);
                for x in [Some(r.clone()), mirror_ok.then(|| r.mirror())]
                    .into_iter()
                    .flatten()
                {
                    let cx = code(&x, &alpha);
                    if cx < best {
                        best = cx;
                        best_m = x;
                    }
                }
            }
        }
        Some(best_m)
    };
    let mut out = Vec::new();
    match total {
        Some(t) if t <= sp.all_max => {
            for idx in 0..t {
                let m = make(idx);
                if let Some(c) = canonical(&m)
                    && c == m
                {
                    out.push(m);
                }
            }
        }
        _ => {
            let mut r = Rng::new(sp.seed);
            let mut seen = HashSet::new();
            let mut tries = 0u64;
            while (out.len() as u64) < sp.sample && tries < sp.sample * 1000 {
                tries += 1;
                let mut table = Vec::with_capacity(n * k);
                for _ in 0..n * k {
                    let x = r.below(per) as usize;
                    let next = x % n;
                    let rest = x / n;
                    table.push(Entry {
                        write: (rest / alpha.len()) as u8,
                        turn: alpha[rest % alpha.len()],
                        next: next as u8,
                    });
                }
                let m = Mite {
                    states: n as u8,
                    colors: k as u8,
                    table,
                };
                if let Some(c) = canonical(&m)
                    && seen.insert(c.clone())
                {
                    out.push(c);
                }
            }
        }
    }
    Ok(out)
}
