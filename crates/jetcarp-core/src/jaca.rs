//! JACA, Jimmie's Annihilating Cellular Automata: balanced ternary cells (-1, 0, +1).
//!
//! Each sign runs its own B/S rule, counting only neighbours of its own sign, so a world seeded
//! with one sign behaves exactly like the plain B/S rule. Where the signs meet they annihilate:
//! every live cell touching a cell of the opposite sign (through the CA's neighbourhood) is empty
//! in the next generation, and so is every cell within `blast` neighbourhood steps of one.
//!
//! One generation, all computed from the current state:
//! 1. Contact cells: live cells with at least one opposite-sign neighbour.
//! 2. The blast zone: contact cells, grown `blast` times through the neighbourhood.
//! 3. Cells in the blast zone are empty. Every other cell follows its sign's rule: a live cell
//!    survives on its own-sign count; an empty cell is born +1 or -1 on that sign's birth count.
//!    An empty cell that both signs would fill stays empty (the two births cancel).
//!
//! Stored states: 0 empty, 1 is +1, 2 is -1 (a 3-state field, so ages, drawing, scenes and
//! scripts work as for other multi-state rules).
//!
//! Notation (case-insensitive): `JACA B3/S23` (both signs), `JACA B3/S23 -B36/S23` (positive,
//! then negative; `+` and `-` prefixes are optional, and the second rule is the negative one),
//! `JACA B3/S23 blast 2`. A `V` suffix on a B/S part picks the edge (von Neumann) neighbourhood.

use crate::topology::{Nb, Topology};
use crate::world::Field;
use rayon::prelude::*;

pub const POS: u8 = 1;
pub const NEG: u8 = 2;
/// The largest blast radius.
pub const MAX_BLAST: u8 = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JacaRule {
    pub pos_birth: u32,
    pub pos_survive: u32,
    pub neg_birth: u32,
    pub neg_survive: u32,
    /// Extra neighbourhood steps destroyed around each annihilation (0: only the touching cells).
    pub blast: u8,
}

impl JacaRule {
    /// The same B/S rule for both signs.
    pub fn symmetric(birth: u32, survive: u32, blast: u8) -> JacaRule {
        JacaRule {
            pos_birth: birth,
            pos_survive: survive,
            neg_birth: birth,
            neg_survive: survive,
            blast,
        }
    }
    pub fn is_symmetric(&self) -> bool {
        self.pos_birth == self.neg_birth && self.pos_survive == self.neg_survive
    }
    /// The positive and negative rules as B/S strings.
    pub fn pos_bs(&self) -> String {
        bs(self.pos_birth, self.pos_survive)
    }
    pub fn neg_bs(&self) -> String {
        bs(self.neg_birth, self.neg_survive)
    }

    pub fn notation(&self) -> String {
        self.notation_with(false)
    }

    /// The notation, with `V` on each B/S part for the edge neighbourhood.
    pub fn notation_with(&self, edge: bool) -> String {
        let v = if edge { "V" } else { "" };
        let mut s = if self.is_symmetric() {
            format!("JACA {}{v}", self.pos_bs())
        } else {
            format!("JACA +{}{v} -{}{v}", self.pos_bs(), self.neg_bs())
        };
        if self.blast > 0 {
            s.push_str(&format!(" blast {}", self.blast));
        }
        s
    }

    /// Parses `JACA ...`; `None` when the text is not a JACA rule. The neighbourhood is `Some`
    /// when a B/S part carries a `V` suffix.
    pub fn parse(text: &str) -> Option<Result<(JacaRule, Option<Nb>), String>> {
        let low = text.trim().to_ascii_lowercase();
        let rest = low.strip_prefix("jaca")?;
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return None;
        }
        Some(parse_body(rest))
    }
}

fn bs(b: u32, s: u32) -> String {
    format!(
        "B{}/S{}",
        crate::rules::count_digits(b),
        crate::rules::count_digits(s)
    )
}

fn parse_body(rest: &str) -> Result<(JacaRule, Option<Nb>), String> {
    // Commas separate counts above 9 inside a B/S part (`B2,10/S3`), so they only end a token
    // at its end (`JACA B3/S23, blast 2`).
    let tokens: Vec<&str> = rest
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '|'))
        .map(|t| t.trim_matches(|c| c == ',' || c == ':'))
        .filter(|t| !t.is_empty())
        .collect();
    let mut pos: Option<(u32, u32)> = None;
    let mut neg: Option<(u32, u32)> = None;
    let mut blast: u32 = 0;
    let mut nb = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(n) = t.strip_prefix("blast").or_else(|| t.strip_prefix('x')) {
            let n = if n.is_empty() || n == "=" {
                i += 1;
                tokens
                    .get(i)
                    .copied()
                    .ok_or("blast needs a radius, like blast 2")?
            } else {
                n.trim_start_matches('=')
            };
            blast = n
                .parse()
                .map_err(|_| format!("blast radius '{n}' is not a whole number"))?;
            if blast > MAX_BLAST as u32 {
                return Err(format!("blast radius runs 0 to {MAX_BLAST}"));
            }
        } else {
            let (sign, body) = match t.as_bytes()[0] {
                b'+' => (Some(true), &t[1..]),
                b'-' => (Some(false), &t[1..]),
                _ => (None, t),
            };
            let body = if let Some(b) = body.strip_suffix('v') {
                nb = Some(Nb::Edge);
                b
            } else {
                body
            };
            let parts: Vec<&str> = body.split('/').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "'{t}' is not a B/S rule. JACA rules look like JACA B3/S23, \
                     JACA +B3/S23 -B36/S23, or JACA B3/S23 blast 2"
                ));
            }
            let b = parts[0]
                .strip_prefix('b')
                .ok_or("B/S rules start with B, like B3/S23")?;
            let s = parts[1]
                .strip_prefix('s')
                .ok_or("the second part starts with S, like B3/S23")?;
            let m = (crate::rules::count_mask(b)?, crate::rules::count_mask(s)?);
            let slot = match sign {
                Some(true) => &mut pos,
                Some(false) => &mut neg,
                None if pos.is_none() => &mut pos,
                None => &mut neg,
            };
            if slot.is_some() {
                return Err("JACA takes one rule per sign: a positive and a negative".into());
            }
            *slot = Some(m);
        }
        i += 1;
    }
    let (p, n) = match (pos, neg) {
        (Some(p), Some(n)) => (p, n),
        (Some(p), None) => (p, p),
        (None, Some(n)) => (n, n),
        (None, None) => return Err("JACA needs a B/S rule, like JACA B3/S23".into()),
    };
    if p.0 & 1 == 1 || n.0 & 1 == 1 {
        return Err("JACA does not support B0 rules".into());
    }
    Ok((
        JacaRule {
            pos_birth: p.0,
            pos_survive: p.1,
            neg_birth: n.0,
            neg_survive: n.1,
            blast: blast as u8,
        },
        nb,
    ))
}

/// What one JACA generation did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Live cells that touched the opposite sign.
    pub contacts: u64,
    /// Cells in the blast zone (contacts included).
    pub destroyed: u64,
    /// Empty cells where both signs would have been born.
    pub cancelled: u64,
}

/// Per-CA JACA state kept outside the field: a fading glow where cells were destroyed (for
/// drawing only) and running counts.
#[derive(Clone, Debug, Default)]
pub struct Heat {
    /// 0 none, 255 destroyed this generation; halves each generation.
    pub glow: Vec<u8>,
    pub last: Stats,
    pub total_contacts: u64,
    pub total_destroyed: u64,
}

/// One generation. Returns the cells destroyed this step (blast zone), as a per-cell mask.
pub fn step(topo: &Topology, nb: Nb, r: &JacaRule, f: &mut Field) -> (Stats, Vec<bool>) {
    let n = f.state.len();
    let a = &f.state;
    let live = topo.live_edges();
    // 1. Contact cells.
    let contact: Vec<bool> = (0..n)
        .into_par_iter()
        .with_min_len(crate::step::MIN_PAR)
        .map(|i| {
            let s = a[i];
            if s == 0 {
                return false;
            }
            let other = if s == POS { NEG } else { POS };
            let mut hit = false;
            topo.for_each_neighbor(i, nb, |j| hit |= a[j] == other);
            hit
        })
        .collect();
    let contacts = contact.iter().filter(|&&c| c).count() as u64;
    // 2. Grow the blast zone.
    let mut zone = contact;
    if contacts > 0 {
        for _ in 0..r.blast {
            let prev = &zone;
            let next: Vec<bool> = (0..n)
                .into_par_iter()
                .with_min_len(crate::step::MIN_PAR)
                .map(|i| {
                    if prev[i] {
                        return true;
                    }
                    let mut hit = false;
                    topo.for_each_neighbor(i, nb, |j| hit |= prev[j]);
                    hit
                })
                .collect();
            zone = next;
        }
    }
    // 3. Births and survival.
    let age = &f.age;
    let z = &zone;
    let cancelled = f
        .back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(crate::step::MIN_PAR)
        .map(|(i, (o, oa))| {
            let s = a[i];
            if z[i] {
                *o = 0;
                *oa = 0;
                return 0u64;
            }
            let (mut cp, mut cn) = (0u32, 0u32);
            topo.for_each_neighbor(i, nb, |j| match a[j] {
                POS => cp += 1,
                NEG => cn += 1,
                _ => {}
            });
            if live {
                let m = topo.missing(i, nb);
                cp += m;
                cn += m;
            }
            let mut cancel = 0;
            let v = match s {
                POS => {
                    if r.pos_survive >> cp.min(31) & 1 == 1 {
                        POS
                    } else {
                        0
                    }
                }
                NEG => {
                    if r.neg_survive >> cn.min(31) & 1 == 1 {
                        NEG
                    } else {
                        0
                    }
                }
                _ => {
                    let bp = r.pos_birth >> cp.min(31) & 1 == 1;
                    let bn = r.neg_birth >> cn.min(31) & 1 == 1;
                    match (bp, bn) {
                        (true, false) => POS,
                        (false, true) => NEG,
                        (true, true) => {
                            cancel = 1;
                            0
                        }
                        _ => 0,
                    }
                }
            };
            *o = v;
            *oa = if v == 0 {
                0
            } else if v == s {
                age[i].saturating_add(1).min(65000)
            } else {
                1
            };
            cancel
        })
        .sum::<u64>();
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
    let destroyed = zone.iter().filter(|&&c| c).count() as u64;
    (
        Stats {
            contacts,
            destroyed,
            cancelled,
        },
        zone,
    )
}

/// Fold one step's blast zone into the glow.
pub fn update_heat(heat: &mut Heat, n: usize, stats: Stats, zone: &[bool]) {
    if heat.glow.len() != n {
        heat.glow = vec![0; n];
    }
    heat.glow
        .par_iter_mut()
        .zip(zone.par_iter())
        .with_min_len(crate::step::MIN_PAR)
        .for_each(|(g, &z)| *g = if z { 255 } else { *g / 2 });
    heat.last = stats;
    heat.total_contacts += stats.contacts;
    heat.total_destroyed += stats.destroyed;
}

/// The value of a stored state: +1, 0 or -1.
pub fn value(state: u8) -> i8 {
    match state {
        POS => 1,
        NEG => -1,
        _ => 0,
    }
}

/// The stored state for a value (+1 -> 1, -1 -> 2, else 0).
pub fn state_of(value: i64) -> u8 {
    match value.signum() {
        1 => POS,
        -1 => NEG,
        _ => 0,
    }
}
