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
//! Those neighbourhoods can hold more cells than the square grid's (a triangle has 12 touch
//! neighbours), so every count a rule names is kept, up to [`MAX_COUNT`], and the notation reads
//! back as written.
//!
//! On a lattice the neighbourhood is a list of offsets ([`reach`]), resolved like any neighbour
//! slot: on a wrapped grid smaller than the neighbourhood, two offsets can land on the same cell,
//! which then counts twice. On a patch the neighbourhood is the set of cells within `r` steps, each
//! counted once.

use crate::topology::{Nb, Offset, Storage, Topology};
use crate::world::Field;
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

/// The largest birth or survival count a rule may name. Far more than any neighbourhood holds:
/// the largest, within range 20 of one tile of the 5-uniform tiling `k5-047`, has 8,462 cells
/// (the square grid's has 1,680).
pub const MAX_COUNT: usize = 65_535;

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
    /// A two-state rule with birth on 0: kept relative to its background, as Life-like B0 rules
    /// are (`b0.rs`, [`step_ltl_b0`]), so empty space stays empty and Golly's runs are matched.
    /// A rule of more states with birth on 0 runs as written (Golly refuses those).
    pub fn b0_kept(&self) -> bool {
        self.states <= 2 && self.birth.first() == Some(&true)
    }

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
        // The cells in the square grid's neighbourhood. The tables are at least this long, and
        // longer when the rule names a larger count: other tilings' neighbourhoods can hold more
        // cells (a triangle has 12 touch neighbours), so every count written is kept.
        let square = match hood {
            Hood::Moore => (2 * r + 1) * (2 * r + 1) - 1,
            Hood::VonNeumann => 2 * r * (r + 1),
            Hood::Cross => 4 * r,
        };
        let set = |k: char, shift: usize| -> Result<Vec<bool>, String> {
            let mut counts = Vec::new();
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
                if b > MAX_COUNT {
                    return Err(format!(
                        "{k}{item}: the count {b} is too large; Larger than Life counts are at \
                         most {MAX_COUNT}"
                    ));
                }
                counts.extend((a..=b).filter(|&c| c >= shift).map(|c| c - shift));
            }
            let len = counts.iter().copied().fold(square, usize::max) + 1;
            let mut v = vec![false; len];
            for c in counts {
                v[c] = true;
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

/// A two-state rule with birth on 0 ([`LtlRule::b0_kept`]), stored relative to its background
/// (`b0.rs`): each step decodes the real states, applies the rule and stores the result against
/// the next background. On a lattice the background is one state per tile type; a slot past a
/// dead edge is in the background, past a live edge live. On a patch it is kept per cell and
/// stepped here. `tick` is the CA's 1-based step count.
pub(crate) fn step_ltl_b0(
    topo: &Topology,
    rule: &LtlRule,
    reach: &Reach,
    tick: u64,
    slot: &mut Option<std::sync::Arc<crate::b0::Background>>,
    f: &mut Field,
) {
    let plain = crate::b0::Plain::Ltl { rule, reach };
    let Some(bg) = crate::b0::ready_of(slot, topo, &plain, tick - 1) else {
        return;
    };
    let live = topo.live_edges();
    let a = &f.state;
    let age = &f.age;
    match (&bg.kind, reach, &topo.storage) {
        (crate::b0::Kind::Types(t), Reach::Lattice(per), Storage::Lattice(g)) => {
            let (p0, p1) = (t.phase(tick - 1), t.phase(tick));
            f.back
                .par_iter_mut()
                .zip(f.back_age.par_iter_mut())
                .enumerate()
                .with_min_len(1024)
                .for_each(|(i, (o, oa))| {
                    let (u, j, k) = g.decompose(i);
                    let base = u + g.row_start[j as usize];
                    let mut c = 0u32;
                    for off in &per[k as usize] {
                        let bk = t.state(p0, off.k as usize);
                        c += match g.resolve(base + off.di, j + off.dj, off.k) {
                            Some(x) => ((a[x] == 1) as u8 ^ bk) as u32,
                            None if live => 1,
                            None => bk as u32,
                        };
                    }
                    let real = (a[i] != 0) as u8 ^ t.state(p0, k as usize);
                    let v = plain.next(real, c) ^ t.state(p1, k as usize);
                    *o = v;
                    *oa = crate::step::b0_age(a[i], v, age[i]);
                });
        }
        (crate::b0::Kind::Cells(cells), Reach::Explicit(csr), _) => {
            let bgs = &cells.states;
            let mut next = vec![0u8; bgs.len()];
            f.back
                .par_iter_mut()
                .zip(f.back_age.par_iter_mut())
                .zip(next.par_iter_mut())
                .enumerate()
                .with_min_len(1024)
                .for_each(|(i, ((o, oa), nbg))| {
                    let (mut c, mut cb) = (0u32, 0u32);
                    for &x in csr.row(i) {
                        let x = x as usize;
                        c += ((a[x] == 1) as u8 ^ bgs[x]) as u32;
                        cb += bgs[x] as u32;
                    }
                    let b1 = plain.next(bgs[i], cb);
                    let v = plain.next((a[i] != 0) as u8 ^ bgs[i], c) ^ b1;
                    *nbg = b1;
                    *o = v;
                    *oa = crate::step::b0_age(a[i], v, age[i]);
                });
            *slot = Some(std::sync::Arc::new(bg.with_cells(tick, next)));
        }
        _ => return,
    }
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
