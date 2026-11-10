//! Priority-table rules: the rule family of Edmond's glider search (arXiv:2609.22579, the
//! `evolve_table` genome of `monotile-ca`), reimplemented here.
//!
//! A rule is an ordered list of rows `own | conditions -> next`. `own` is a state or `*` (any);
//! a condition `nS>=m` holds when at least `m` neighbours are in state `S` (state 0 included, so
//! `n0` counts the quiescent neighbours that exist: cells past a dead edge or an open patch
//! boundary are absent, not quiescent). The first row whose `own` matches and whose conditions
//! all hold gives the next state; when no row matches the cell becomes 0. The cell's own state is
//! not counted among its neighbours. The rule is outer-totalistic, so it runs on any tiling.
//!
//! Notation (the `Rule` parser accepts it): `Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0;
//! 3|n1>=1->1`. A row with no conditions is written `always` (`*|always->0`). Edmond's Spectre
//! glider rule is exactly the three rows above (see [`spectre_glider`]).

use crate::topology::{Nb, Topology};
use crate::world::Field;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Most states a priority rule may use.
pub const MAX_STATES: u8 = 8;
/// Genome bounds of the search (as in `monotile-ca`): rows, conditions per row, threshold.
pub const MAX_ROWS: usize = 8;
pub const MAX_CONDS: usize = 2;
pub const MAX_MIN: u8 = 3;

/// One row: `own | conds -> next`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Row {
    /// The cell's own state, or `None` for any state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<u8>,
    /// `(state, min)`: at least `min` neighbours in `state`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conds: Vec<(u8, u8)>,
    pub next: u8,
}

impl Row {
    #[inline]
    fn fires(&self, own: u8, counts: &[u16; MAX_STATES as usize]) -> bool {
        self.own.is_none_or(|o| o == own)
            && self
                .conds
                .iter()
                .all(|&(s, m)| counts[s as usize] >= m as u16)
    }
    pub fn notation(&self) -> String {
        let own = self
            .own
            .map(|o| o.to_string())
            .unwrap_or_else(|| "*".into());
        let conds = if self.conds.is_empty() {
            "always".to_string()
        } else {
            self.conds
                .iter()
                .map(|(s, m)| format!("n{s}>={m}"))
                .collect::<Vec<_>>()
                .join("&")
        };
        format!("{own}|{conds}->{}", self.next)
    }
}

/// An ordered row list with a state count.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PriorityRule {
    pub states: u8,
    pub rows: Vec<Row>,
}

impl PriorityRule {
    /// Next state of a cell in state `own` whose neighbours have these state counts.
    #[inline]
    pub fn next(&self, own: u8, counts: &[u16; MAX_STATES as usize]) -> u8 {
        for r in &self.rows {
            if r.fires(own, counts) {
                return r.next;
            }
        }
        0
    }

    /// `Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1`.
    pub fn notation(&self) -> String {
        let rows: Vec<String> = self.rows.iter().map(Row::notation).collect();
        format!("Priority {}: {}", self.states, rows.join("; "))
    }

    /// Parse the notation. `None` if the text is not a priority rule at all.
    pub fn parse(text: &str) -> Option<Result<PriorityRule, String>> {
        let t = text.trim();
        let low = t.to_ascii_lowercase();
        let rest = low
            .strip_prefix("priority")
            .or_else(|| low.strip_prefix("pt"))?;
        if !rest.starts_with([' ', ':']) && !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        Some(Self::parse_body(rest))
    }

    fn parse_body(rest: &str) -> Result<PriorityRule, String> {
        let (k, body) = rest
            .split_once(':')
            .ok_or("priority rules look like 'Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0'")?;
        let states: u8 = k
            .trim()
            .parse()
            .map_err(|_| format!("priority rule: bad state count '{}'", k.trim()))?;
        if !(2..=MAX_STATES).contains(&states) {
            return Err(format!("priority rules have 2 to {MAX_STATES} states"));
        }
        let st = |s: &str| -> Result<u8, String> {
            let v: u8 = s
                .trim()
                .parse()
                .map_err(|_| format!("priority rule: bad state '{s}'"))?;
            if v >= states {
                return Err(format!("priority rule: state {v} is not below {states}"));
            }
            Ok(v)
        };
        let mut rows = Vec::new();
        for r in body.split(';').map(str::trim).filter(|r| !r.is_empty()) {
            let r: String = r.chars().filter(|c| !c.is_whitespace()).collect();
            let (lhs, next) = r
                .split_once("->")
                .ok_or_else(|| format!("priority row '{r}' has no '->'"))?;
            let (own, conds) = lhs
                .split_once('|')
                .ok_or_else(|| format!("priority row '{r}' has no '|'"))?;
            let own = if own == "*" { None } else { Some(st(own)?) };
            let mut cv = Vec::new();
            if conds != "always" && !conds.is_empty() {
                for c in conds.split('&') {
                    let c = c
                        .strip_prefix('n')
                        .ok_or_else(|| format!("condition '{c}' should look like n1>=2"))?;
                    let (s, m) = c
                        .split_once(">=")
                        .ok_or_else(|| format!("condition 'n{c}' should look like n1>=2"))?;
                    let m: u8 = m
                        .parse()
                        .map_err(|_| format!("condition 'n{c}': bad count"))?;
                    cv.push((st(s)?, m));
                }
            }
            rows.push(Row {
                own,
                conds: cv,
                next: st(next)?,
            });
        }
        if rows.len() > 64 {
            return Err("priority rules have at most 64 rows".into());
        }
        Ok(PriorityRule { states, rows })
    }

    /// Does an all-quiescent neighbourhood of `degree` cells turn a quiescent cell non-zero?
    pub fn ignites(&self, degree: usize) -> bool {
        let mut c = [0u16; MAX_STATES as usize];
        c[0] = degree as u16;
        self.next(0, &c) != 0
    }

    /// The same rule with rows that can never fire removed: rows after an unconditional row for
    /// the same (or any) own state, conditions that always hold (`n>=0`), duplicate conditions,
    /// and rows that give 0 (the default) when no later row could fire for the same cells. The behaviour is unchanged.
    pub fn simplified(&self) -> PriorityRule {
        let mut rows: Vec<Row> = Vec::new();
        // own states that an earlier unconditional row already decides (index = state; all).
        let mut decided = vec![false; self.states as usize];
        for r in &self.rows {
            let shadowed = match r.own {
                Some(o) => decided[o as usize],
                None => decided.iter().all(|&d| d),
            };
            if shadowed {
                continue;
            }
            let mut conds: Vec<(u8, u8)> = r.conds.iter().copied().filter(|c| c.1 > 0).collect();
            conds.sort_unstable();
            // n_s>=a & n_s>=b is n_s>=max(a, b).
            let mut merged: Vec<(u8, u8)> = Vec::new();
            for (s, m) in conds {
                match merged.last_mut() {
                    Some(l) if l.0 == s => l.1 = l.1.max(m),
                    _ => merged.push((s, m)),
                }
            }
            if merged.is_empty() {
                match r.own {
                    Some(o) => decided[o as usize] = true,
                    None => decided.iter_mut().for_each(|d| *d = true),
                }
            }
            rows.push(Row {
                own: r.own,
                conds: merged,
                next: r.next,
            });
        }
        // A row giving 0 (the default) is inert unless a later row could catch the same cells.
        let mut i = rows.len();
        while i > 0 {
            i -= 1;
            let o = rows[i].own;
            let overlap = |r: &Row| r.own.is_none() || o.is_none() || r.own == o;
            if rows[i].next == 0 && !rows[i + 1..].iter().any(overlap) {
                rows.remove(i);
            }
        }
        PriorityRule {
            states: self.states,
            rows,
        }
    }
}

/// Edmond's Spectre glider rule as a priority table (equivalent to `SpectreGlider`).
pub fn spectre_glider() -> PriorityRule {
    PriorityRule::parse("Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1")
        .unwrap()
        .unwrap()
}

/// One generation of a priority rule on a world field (dense, parallel over cells).
pub(crate) fn step_priority(topo: &Topology, nb: Nb, rule: &PriorityRule, f: &mut Field) {
    let a = &f.state;
    let k = rule.states;
    f.back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(4096)
        .for_each(|(i, (o, oa))| {
            let mut c = [0u16; MAX_STATES as usize];
            topo.for_each_neighbor(i, nb, |j| c[(a[j] % k) as usize] += 1);
            let v = rule.next(a[i] % k, &c);
            *o = v;
            *oa = (v != 0) as u16;
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_round_trip() {
        let r = spectre_glider();
        assert_eq!(r.rows.len(), 3);
        let t = r.notation();
        assert_eq!(t, "Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1");
        assert_eq!(PriorityRule::parse(&t).unwrap().unwrap(), r);
        let a = PriorityRule::parse("priority 3: *|always->0")
            .unwrap()
            .unwrap();
        assert_eq!(a.rows[0].own, None);
        assert!(a.rows[0].conds.is_empty());
        assert!(PriorityRule::parse("B3/S23").is_none());
        assert!(
            PriorityRule::parse("Priority 4: 0|n5>=1->1")
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn simplify_keeps_behaviour() {
        // Edmond's stored s33 record: rows 2, 4 and 6 are inert.
        let r = PriorityRule::parse(
            "Priority 4: 0|n0>=3&n1>=1->3; 0|always->0; 3|n3>=3->0; 0|n0>=3&n1>=1->0; 3|n1>=1->1; 0|always->0",
        )
        .unwrap()
        .unwrap();
        let s = r.simplified();
        assert_eq!(s, spectre_glider());
        // Same next state for every own state and count vector up to 7 neighbours.
        for own in 0..4u8 {
            for code in 0..4096u32 {
                let mut c = [0u16; 8];
                let mut x = code;
                for s in c.iter_mut().take(4) {
                    *s = (x % 8) as u16;
                    x /= 8;
                }
                assert_eq!(r.next(own, &c), s.next(own, &c));
            }
        }
    }
}
