//! Larger than Life (range-r outer totalistic) rules.
//!
//! Notation, as on LifeWiki:
//! - HROT form: `R5,C2,S33-57,B34-45` with an optional neighbourhood `,NM` (Moore, default),
//!   `,NN` (von Neumann), or `,N+` (cross, square grid only). The centre cell is not counted.
//! - MCell form: `R5,C0,M1,S34..58,B34..45,NM`. `M1` counts the centre cell, so survival bounds
//!   drop by one when converted to the HROT form.
//!
//! `C` is the state count: `C0` and `C2` mean two states; larger values decay like Generations
//! rules (only state-1 neighbours count).
//!
//! On any tiling, Moore means "within r steps over touching neighbours" and von Neumann means
//! "within r steps over edge neighbours"; on the square grid these are the usual box and diamond.

use crate::topology::{Nb, Offset, Storage, Topology};
use crate::world::Field;
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hood {
    Moore,
    VonNeumann,
    Cross,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LtlRule {
    pub range: u8,
    pub states: u8,
    pub hood: Hood,
    /// `birth[c]`: a dead cell with `c` live neighbours is born.
    pub birth: Vec<bool>,
    pub survive: Vec<bool>,
}

fn ranges(v: &[bool]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i] {
            let s = i;
            while i + 1 < v.len() && v[i + 1] {
                i += 1;
            }
            out.push(if s == i {
                format!("{s}")
            } else {
                format!("{s}-{i}")
            });
        }
        i += 1;
    }
    out.join(",")
}

impl LtlRule {
    pub fn notation(&self) -> String {
        let hood = match self.hood {
            Hood::Moore => "",
            Hood::VonNeumann => ",NN",
            Hood::Cross => ",N+",
        };
        format!(
            "R{},C{},S{},B{}{}",
            self.range,
            self.states,
            ranges(&self.survive),
            ranges(&self.birth),
            hood
        )
    }

    /// Parse either form. Returns `None` if `text` does not look like LtL notation.
    pub fn parse(text: &str) -> Option<Result<LtlRule, String>> {
        let t: String = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_uppercase();
        if !t.starts_with('R') || !t[1..].starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        Some(Self::parse_inner(&t))
    }

    fn parse_inner(t: &str) -> Result<LtlRule, String> {
        // Split into fields: a token starting with a letter opens a field.
        let mut fields: Vec<(char, Vec<String>)> = Vec::new();
        for tok in t.split(',') {
            if tok.is_empty() {
                continue;
            }
            let c = tok.chars().next().unwrap();
            if c.is_ascii_alphabetic() {
                fields.push((c, vec![tok[1..].to_string()]));
            } else if let Some(f) = fields.last_mut() {
                f.1.push(tok.to_string());
            } else {
                return Err(format!("unexpected '{tok}'"));
            }
        }
        let get = |k: char| fields.iter().find(|f| f.0 == k).map(|f| &f.1);
        let num = |k: char| -> Result<Option<u32>, String> {
            match get(k) {
                None => Ok(None),
                Some(v) => v[0]
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{k} needs a number")),
            }
        };
        let range = num('R')?.ok_or("Larger than Life needs R")?;
        if !(1..=20).contains(&range) {
            return Err("range must be 1 to 20".into());
        }
        let states = match num('C')?.unwrap_or(2) {
            0 | 2 => 2,
            c if c <= 255 => c,
            _ => return Err("C must be at most 255".into()),
        };
        let middle = num('M')?.unwrap_or(0) == 1;
        let hood = match get('N').map(|v| v[0].as_str()) {
            None | Some("M") => Hood::Moore,
            Some("N") => Hood::VonNeumann,
            Some("+") => Hood::Cross,
            Some(o) => {
                return Err(format!(
                    "neighbourhood N{o} is not supported (use NM, NN, or N+)"
                ));
            }
        };
        let r = range as usize;
        let max = match hood {
            Hood::Moore => (2 * r + 1) * (2 * r + 1) - 1,
            Hood::VonNeumann => 2 * r * (r + 1),
            Hood::Cross => 4 * r,
        };
        let set = |k: char, shift: usize| -> Result<Vec<bool>, String> {
            let mut v = vec![false; max + 1];
            for item in get(k).ok_or_else(|| format!("missing {k}"))? {
                if item.is_empty() {
                    continue;
                }
                let (a, b) = if let Some((a, b)) = item.split_once("..") {
                    (a, b)
                } else if let Some((a, b)) = item.split_once('-') {
                    (a, b)
                } else {
                    (item.as_str(), item.as_str())
                };
                let a: usize = a.parse().map_err(|_| format!("bad count '{item}'"))?;
                let b: usize = b.parse().map_err(|_| format!("bad count '{item}'"))?;
                for c in a..=b {
                    if c >= shift && c - shift <= max {
                        v[c - shift] = true;
                    }
                }
            }
            Ok(v)
        };
        Ok(LtlRule {
            range: range as u8,
            states: states as u8,
            hood,
            birth: set('B', 0)?,
            survive: set('S', middle as usize)?,
        })
    }
}

/// Precomputed neighbourhoods for a topology.
#[derive(Debug)]
pub enum Reach {
    /// Per tile type, offsets relative to the cell's lattice position.
    Lattice(Vec<Vec<Offset>>),
    /// Per cell, neighbour lists (explicit storage).
    Explicit(crate::topology::Csr),
}

/// Neighbourhood within `r` steps. Lattices: a BFS over the offset table from one cell of each
/// type, in unbounded lattice coordinates, so wrap and edges are handled by `resolve`.
pub fn reach(topo: &Topology, hood: Hood, r: u8) -> Result<Reach, String> {
    let r = r as i32;
    match &topo.storage {
        Storage::Lattice(g) => {
            if hood == Hood::Cross {
                if topo.family.id != "square" || g.ntypes != 1 {
                    return Err("the cross neighbourhood (N+) needs the square grid".into());
                }
                let mut v = Vec::new();
                for d in 1..=r {
                    for (di, dj) in [(d, 0), (-d, 0), (0, d), (0, -d)] {
                        v.push(Offset { di, dj, k: 0 });
                    }
                }
                return Ok(Reach::Lattice(vec![v]));
            }
            let nb = if hood == Hood::Moore {
                Nb::Touch
            } else {
                Nb::Edge
            };
            let mut out = Vec::new();
            for k0 in 0..g.ntypes {
                let mut seen: HashMap<(i32, i32, u32), i32> = HashMap::new();
                let mut q = VecDeque::new();
                seen.insert((0, 0, k0), 0);
                q.push_back((0i32, 0i32, k0));
                let mut list = Vec::new();
                while let Some((i, j, k)) = q.pop_front() {
                    let d = seen[&(i, j, k)];
                    if d > 0 {
                        list.push(Offset { di: i, dj: j, k });
                    }
                    if d == r {
                        continue;
                    }
                    for o in &g.offsets[nb as usize][k as usize] {
                        let key = (i + o.di, j + o.dj, o.k);
                        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                            e.insert(d + 1);
                            q.push_back(key);
                        }
                    }
                }
                out.push(list);
            }
            Ok(Reach::Lattice(out))
        }
        Storage::Explicit(_) => {
            if hood == Hood::Cross {
                return Err("the cross neighbourhood (N+) needs the square grid".into());
            }
            let nb = if hood == Hood::Moore {
                Nb::Touch
            } else {
                Nb::Edge
            };
            let lists: Vec<Vec<u32>> = (0..topo.len())
                .into_par_iter()
                .map(|s| {
                    let mut dist: HashMap<usize, i32> = HashMap::new();
                    dist.insert(s, 0);
                    let mut q = VecDeque::from([s]);
                    let mut list = Vec::new();
                    while let Some(c) = q.pop_front() {
                        let d = dist[&c];
                        if d > 0 {
                            list.push(c as u32);
                        }
                        if d == r {
                            continue;
                        }
                        topo.for_each_neighbor(c, nb, |x| {
                            if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(x) {
                                e.insert(d + 1);
                                q.push_back(x);
                            }
                        });
                    }
                    list
                })
                .collect();
            Ok(Reach::Explicit(crate::topology::Csr::from_lists(&lists)))
        }
    }
}

pub(crate) fn step_ltl(topo: &Topology, rule: &LtlRule, reach: &Reach, f: &mut Field) {
    let live = topo.live_edges();
    let n = rule.states;
    let a = &f.state;
    let age = &f.age;
    let count = |i: usize| -> usize {
        let mut c = 0usize;
        match (reach, &topo.storage) {
            (Reach::Lattice(per), Storage::Lattice(g)) => {
                let (u, j, k) = g.decompose(i);
                let base = u + g.row_start[j as usize];
                for o in &per[k as usize] {
                    match g.resolve(base + o.di, j + o.dj, o.k) {
                        Some(x) => c += (a[x] == 1) as usize,
                        None => c += live as usize,
                    }
                }
            }
            (Reach::Explicit(csr), _) => {
                for &x in csr.row(i) {
                    c += (a[x as usize] == 1) as usize;
                }
            }
            _ => {}
        }
        c
    };
    f.back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(1024)
        .for_each(|(i, (o, oa))| {
            let s = a[i];
            let v = if s == 0 {
                let c = count(i);
                rule.birth.get(c).copied().unwrap_or(false) as u8
            } else if s == 1 {
                let c = count(i);
                if rule.survive.get(c).copied().unwrap_or(false) {
                    1
                } else if n > 2 {
                    2
                } else {
                    0
                }
            } else if s + 1 < n {
                s + 1
            } else {
                0
            };
            *o = v;
            *oa = if v == 1 {
                if s == 1 {
                    age[i].saturating_add(1).min(65000)
                } else {
                    1
                }
            } else if v != 0 {
                age[i]
            } else {
                0
            };
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notation_forms() {
        let a = LtlRule::parse("R5,C2,S33-57,B34-45").unwrap().unwrap();
        let b = LtlRule::parse("R5,C0,M1,S34..58,B34..45,NM")
            .unwrap()
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.notation(), "R5,C2,S33-57,B34-45");
        let c = LtlRule::parse("R3,C2,S2,B3,N+").unwrap().unwrap();
        assert_eq!(c.notation(), "R3,C2,S2,B3,N+");
        let d = LtlRule::parse("R1,C2,S0,B1,NN").unwrap().unwrap();
        assert_eq!(d.birth.len(), 5);
        assert!(LtlRule::parse("B3/S23").is_none());
    }
}
