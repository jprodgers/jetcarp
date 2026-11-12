//! Compound objects: a periodic object split into the independent objects it is made of.
//!
//! Two gliders flying side by side, a pulsar with four blinkers beside it, and two blocks a cell
//! apart are each several objects that happen to lie close together. They never influence each
//! other, so a catalogue stores their parts, not the arrangement. The pulsar's four quadrants, by
//! contrast, die alone and live only together, so the pulsar is one object; so is a pattern whose
//! parts only stabilise each other, and so is a pair of ships whose sparks touch and keep them
//! bound. This module tells the cases apart by running the parts.
//!
//! **The test.** An object is one period of phases (`P` generations) and its displacement `d`
//! per period (zero for still lifes and oscillators). Split one phase into its **atoms**, the
//! adjacency components under the rule's neighbourhood. A set of atoms is a **part** when it runs
//! alone as an object of the same period and velocity (its own period divides `P`; it moves by
//! `d` every `P` generations). Two disjoint sets `X`, `Y` covering the object are **independent**
//! when both are parts and, at every generation of a run of `L = 2P + 8` generations, the joint
//! evolution (the object's own phases) equals the union of their solo runs. Exact comparison:
//! positions, tile types and states. Since both parts repeat every `P` generations, equality over
//! one full period means equality forever; the extra generations are for safety. Ships compare in
//! absolute coordinates, which is their moving frame too.
//!
//! **Finding the parts** (one phase at a time, the phases with the most atoms first):
//!
//! 1. **Peel single atoms.** Each atom that is a part alone is split off when the rest, run
//!    alone, is a part too and the two are independent (a blinker beside a pulsar, a block beside
//!    a block, one glider of a pair).
//! 2. **Cluster the rest.** Two atoms interact when running them together differs, at any
//!    generation of the run, from the union of their solo runs. Pairs whose solo runs never come
//!    within two neighbour steps of each other (the rule's reach) cannot interact and are not
//!    run. The interacting pairs are joined (union-find): the pulsar's quadrants become one
//!    cluster.
//! 3. **Sparks.** A cluster that is not a part alone (a spark that dies, a ship missing a spark it
//!    has already shed) belongs to the object it came from: it joins the nearest cluster with
//!    which it is a part, or failing that the nearest cluster.
//! 4. **Peel clusters.** A cluster that is a part, independent of the rest, is split off.
//!
//! Each split is checked by the independence test itself, so it can only miss a split (and leave
//! a compound whole), never split one object. Each part found is then split again in its own
//! phases, so the result is the finest the steps reach.
//!
//! **What stays one object.** A tag-along (a ship that runs unperturbed while pulling a passenger
//! that cannot run alone): its passenger is never a part, so nothing splits. A ship that only
//! *contains* a phase of a smaller ship: the smaller one is perturbed, so the two are not
//! independent. A bound pair: its members interact. A pattern whose parts only stabilise each
//! other: the parts are not parts alone.

use super::{LCell, Phase};
use std::collections::{HashMap, HashSet};

/// How an object's cells are run alone: a stepper on the object's tiling and rule.
pub trait Runner {
    /// Run `cells` alone for `gens` generations: `gens + 1` phases, the first the cells as given
    /// (sorted), empty ones after it dies. `None` when the run cannot be completed exactly (it
    /// grew past the runner's limit, or left the room it has).
    fn run(&self, cells: &[LCell], gens: u64) -> Option<Vec<Phase>>;
    /// Push the cells touching `c` (lattice `(i, j, k)`, or `(id, 0, 0)` on explicit patches).
    fn touching(&self, c: &LCell, out: &mut Vec<(i32, i32, u32)>);
    /// Could a cell of `a` and a cell of `b` share a neighbour, so that one's next generation
    /// depends on the other? `true` is always safe.
    fn close(&self, a: &[LCell], b: &[LCell]) -> bool;
    /// A distance between two non-empty cell sets, for "nearest first".
    fn distance(&self, a: &[LCell], b: &[LCell]) -> f64;
}

/// One independent part of a compound: its own period and displacement per period, and one
/// period of phases, phase 0 at the whole object's phase 0 (in the whole's coordinates).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompoundPart {
    pub period: u64,
    pub d: [i32; 2],
    pub phases: Vec<Phase>,
}

/// Limits on the work one object may take (larger objects are left whole).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Atoms in a phase.
    pub atoms: usize,
    /// Live cells in a phase.
    pub cells: usize,
    /// Phases tried per object (those with the most atoms first).
    pub phases: usize,
    /// Longest run (generations).
    pub gens: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            atoms: 64,
            cells: 4096,
            phases: 64,
            gens: 4096,
        }
    }
}

/// Generations every independence test runs: two periods and eight more.
pub fn horizon(period: u64) -> u64 {
    2 * period.max(1) + 8
}

/// `ph` moved by `n` displacements `d`.
pub fn shift(ph: &[LCell], d: [i32; 2], n: i32) -> Phase {
    if d == [0, 0] || n == 0 {
        return ph.to_vec();
    }
    ph.iter()
        .map(|c| LCell {
            i: c.i + d[0] * n,
            j: c.j + d[1] * n,
            ..*c
        })
        .collect()
}

/// The parts of a compound object (`phases`: one period as run, phase 0 first; `d`: its
/// displacement per period), or `None` when it is one object (see the module notes). Parts come
/// in order of their first cell in phase 0.
pub fn split(r: &dyn Runner, phases: &[Phase], d: [i32; 2]) -> Option<Vec<CompoundPart>> {
    split_with(r, phases, d, Limits::default())
}

/// [`split`] with explicit limits.
pub fn split_with(
    r: &dyn Runner,
    phases: &[Phase],
    d: [i32; 2],
    lim: Limits,
) -> Option<Vec<CompoundPart>> {
    let mut parts = split_rec(r, phases, d, lim, 0)?;
    if parts.len() < 2 {
        return None;
    }
    parts.sort_by(|a, b| a.phases[0].first().cmp(&b.phases[0].first()));
    Some(parts)
}

/// The touching components of a phase (each sorted), in order of their first cell.
pub fn atoms(r: &dyn Runner, cells: &[LCell]) -> Vec<Phase> {
    let at: HashMap<(i32, i32, u32), usize> = cells
        .iter()
        .enumerate()
        .map(|(n, c)| ((c.i, c.j, c.k), n))
        .collect();
    let mut comp = vec![usize::MAX; cells.len()];
    let mut out: Vec<Phase> = Vec::new();
    let mut nb = Vec::new();
    for s in 0..cells.len() {
        if comp[s] != usize::MAX {
            continue;
        }
        let id = out.len();
        comp[s] = id;
        let mut stack = vec![s];
        let mut members = Vec::new();
        while let Some(x) = stack.pop() {
            members.push(cells[x]);
            nb.clear();
            r.touching(&cells[x], &mut nb);
            for key in &nb {
                if let Some(&y) = at.get(key)
                    && comp[y] == usize::MAX
                {
                    comp[y] = id;
                    stack.push(y);
                }
            }
        }
        members.sort_unstable();
        out.push(members);
    }
    out.sort_by(|a, b| a[0].cmp(&b[0]));
    out
}

fn split_rec(
    r: &dyn Runner,
    phases: &[Phase],
    d: [i32; 2],
    lim: Limits,
    depth: u32,
) -> Option<Vec<CompoundPart>> {
    let p = phases.len() as u64;
    if p == 0
        || phases
            .iter()
            .any(|ph| ph.is_empty() || ph.len() > lim.cells)
    {
        return None;
    }
    let l = horizon(p);
    if l > lim.gens {
        return None;
    }
    let whole = || CompoundPart {
        period: p,
        d,
        phases: phases.to_vec(),
    };
    // Phases with the most atoms first.
    let mut order: Vec<(usize, usize)> = phases
        .iter()
        .enumerate()
        .map(|(t, ph)| (atoms(r, ph).len(), t))
        .filter(|&(n, _)| n >= 2 && n <= lim.atoms)
        .collect();
    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    order.truncate(lim.phases);
    let cx = Cx { r, p, d, l };
    for (_, t) in order {
        let joint: Vec<Phase> = (0..=l)
            .map(|g| {
                let q = t as u64 + g;
                shift(&phases[(q % p) as usize], d, (q / p) as i32)
            })
            .collect();
        let Some(sets) = cx.split_at(&phases[t], &joint) else {
            continue;
        };
        let parts: Option<Vec<CompoundPart>> =
            sets.iter().map(|s| cx.part_from(s, t as u64)).collect();
        let Some(parts) = parts else {
            continue;
        };
        let mut out = Vec::new();
        for pt in parts {
            match (depth < 6).then(|| split_rec(r, &pt.phases, pt.d, lim, depth + 1)) {
                Some(Some(sub)) if sub.len() > 1 => out.extend(sub),
                _ => out.push(pt),
            }
        }
        return Some(out);
    }
    Some(vec![whole()])
}

struct Cx<'a> {
    r: &'a dyn Runner,
    p: u64,
    d: [i32; 2],
    l: u64,
}

/// A solo run and whether it is a part.
#[derive(Clone)]
struct Solo {
    run: Option<Vec<Phase>>,
    part: bool,
}

/// Is `a ∪ b` exactly `want` (all sorted; `a` and `b` may share cells)?
pub fn union_is(a: &[LCell], b: &[LCell], want: &[LCell]) -> bool {
    let (mut x, mut y, mut k) = (0, 0, 0);
    while x < a.len() || y < b.len() {
        let c = match (a.get(x), b.get(y)) {
            (Some(p), Some(q)) if p == q => {
                x += 1;
                y += 1;
                p
            }
            (Some(p), Some(q)) if p < q => {
                x += 1;
                p
            }
            (Some(p), None) => {
                x += 1;
                p
            }
            (_, Some(q)) => {
                y += 1;
                q
            }
            (None, None) => unreachable!(),
        };
        if want.get(k) != Some(c) {
            return false;
        }
        k += 1;
    }
    k == want.len()
}

fn cells_of(atoms: &[Phase], set: &[usize]) -> Phase {
    let mut v: Phase = set.iter().flat_map(|&a| atoms[a].iter().copied()).collect();
    v.sort_unstable();
    v
}

impl Cx<'_> {
    fn solo(&self, cells: &[LCell]) -> Solo {
        let run = self.r.run(cells, self.l);
        let part = run.as_ref().is_some_and(|v| self.is_part(v));
        Solo { run, part }
    }

    /// Does this run repeat with the whole's period and velocity?
    fn is_part(&self, run: &[Phase]) -> bool {
        let p = self.p as usize;
        run.len() > p && !run[0].is_empty() && run[p] == shift(&run[0], self.d, 1)
    }

    /// Do two solo runs together make `joint` at every generation?
    fn independent(&self, a: &Solo, b: &Solo, joint: &[Phase]) -> bool {
        match (&a.run, &b.run) {
            (Some(x), Some(y)) => (0..joint.len()).all(|g| union_is(&x[g], &y[g], &joint[g])),
            _ => false,
        }
    }

    /// The atom sets of the parts at this phase, or `None` when it does not come apart here.
    fn split_at(&self, start: &Phase, joint: &[Phase]) -> Option<Vec<Vec<LCell>>> {
        let atoms = atoms(self.r, start);
        let n = atoms.len();
        if n < 2 {
            return None;
        }
        let solo: Vec<Solo> = atoms.iter().map(|a| self.solo(a)).collect();
        let mut parts: Vec<Vec<usize>> = Vec::new();
        let mut rest: Vec<usize> = (0..n).collect();
        let mut rest_run = Solo {
            run: Some(joint.to_vec()),
            part: true,
        };
        // 1. Single atoms that are parts, independent of the rest.
        for a in 0..n {
            if !solo[a].part || rest.len() < 2 {
                continue;
            }
            let y: Vec<usize> = rest.iter().copied().filter(|&x| x != a).collect();
            let yr = self.solo(&cells_of(&atoms, &y));
            if yr.part && self.independent(&solo[a], &yr, rest_run.run.as_ref().unwrap()) {
                parts.push(vec![a]);
                rest = y;
                rest_run = yr;
            }
        }
        // 2-4. Clusters of the rest.
        if rest.len() >= 2 {
            let mut cl = self.clusters(&atoms, &solo, &rest);
            let mut runs: HashMap<Vec<usize>, Solo> = HashMap::new();
            let mut solo_of = |set: &Vec<usize>, cx: &Self| -> Solo {
                if set.len() == 1 {
                    return solo[set[0]].clone();
                }
                runs.entry(set.clone())
                    .or_insert_with(|| cx.solo(&cells_of(&atoms, set)))
                    .clone()
            };
            let mut guard = 0;
            while cl.len() >= 2 && guard < 4 * n + 8 {
                guard += 1;
                let sr: Vec<Solo> = cl.iter().map(|c| solo_of(c, self)).collect();
                // 3. A cluster that is not a part joins the nearest one it makes a part with.
                if let Some(i) = (0..cl.len()).find(|&i| !sr[i].part) {
                    let ci = cells_of(&atoms, &cl[i]);
                    let mut near: Vec<(f64, usize)> = (0..cl.len())
                        .filter(|&j| j != i)
                        .map(|j| (self.r.distance(&ci, &cells_of(&atoms, &cl[j])), j))
                        .collect();
                    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    let mut to = near[0].1;
                    for &(_, j) in &near {
                        let mut u = cl[i].clone();
                        u.extend(&cl[j]);
                        u.sort_unstable();
                        if solo_of(&u, self).part {
                            to = j;
                            break;
                        }
                    }
                    let moved = cl[i].clone();
                    cl[to].extend(moved);
                    cl[to].sort_unstable();
                    cl.remove(i);
                    continue;
                }
                // 4. A cluster independent of the rest is split off.
                let mut peeled = false;
                for i in 0..cl.len() {
                    let y: Vec<usize> = rest
                        .iter()
                        .copied()
                        .filter(|x| !cl[i].contains(x))
                        .collect();
                    let yr = solo_of(&y, self);
                    if yr.part && self.independent(&sr[i], &yr, rest_run.run.as_ref().unwrap()) {
                        parts.push(cl[i].clone());
                        rest = y;
                        rest_run = yr;
                        cl.remove(i);
                        peeled = true;
                        break;
                    }
                }
                if !peeled {
                    break;
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        parts.push(rest);
        Some(parts.iter().map(|s| cells_of(&atoms, s)).collect())
    }

    /// Clusters of `set` (atom indices) joined by pairwise interaction.
    fn clusters(&self, atoms: &[Phase], solo: &[Solo], set: &[usize]) -> Vec<Vec<usize>> {
        let n = set.len();
        let mut uf: Vec<usize> = (0..n).collect();
        fn find(uf: &mut [usize], mut x: usize) -> usize {
            while uf[x] != x {
                uf[x] = uf[uf[x]];
                x = uf[x];
            }
            x
        }
        for x in 0..n {
            for y in x + 1..n {
                if find(&mut uf, x) == find(&mut uf, y) {
                    continue;
                }
                let (a, b) = (set[x], set[y]);
                let meet = match (&solo[a].run, &solo[b].run) {
                    (Some(ra), Some(rb)) => (0..ra.len()).any(|g| {
                        !ra[g].is_empty() && !rb[g].is_empty() && self.r.close(&ra[g], &rb[g])
                    }),
                    _ => true,
                };
                if !meet {
                    continue;
                }
                let joint = self.r.run(&cells_of(atoms, &[a, b]), self.l);
                let apart = match &joint {
                    Some(j) => self.independent(&solo[a], &solo[b], j),
                    None => false,
                };
                if !apart {
                    let (rx, ry) = (find(&mut uf, x), find(&mut uf, y));
                    uf[rx.max(ry)] = rx.min(ry);
                }
            }
        }
        let mut at: HashMap<usize, usize> = HashMap::new();
        let mut out: Vec<Vec<usize>> = Vec::new();
        for x in 0..n {
            let root = find(&mut uf, x);
            let c = *at.entry(root).or_insert_with(|| {
                out.push(vec![]);
                out.len() - 1
            });
            out[c].push(set[x]);
        }
        out
    }

    /// A part from its cells at phase `t`: its phases from the whole's phase 0, and its own
    /// (smallest) period.
    fn part_from(&self, cells: &[LCell], t: u64) -> Option<CompoundPart> {
        let p = self.p;
        let run = self.r.run(cells, p)?;
        if !self.is_part(&run) {
            return None;
        }
        // Phase u of the part (u < p): generation u - t of the run, or a period later moved back.
        let at = |u: u64| -> Phase {
            if u >= t {
                run[(u - t) as usize].clone()
            } else {
                shift(&run[(u + p - t) as usize], self.d, -1)
            }
        };
        let full: Vec<Phase> = (0..p).map(at).collect();
        let q = (1..=p)
            .find(|&q| {
                if !p.is_multiple_of(q) {
                    return false;
                }
                let (dx, dy) = (self.d[0] as i64 * q as i64, self.d[1] as i64 * q as i64);
                if dx % p as i64 != 0 || dy % p as i64 != 0 {
                    return false;
                }
                let dq = [(dx / p as i64) as i32, (dy / p as i64) as i32];
                let next = if q == p {
                    shift(&full[0], self.d, 1)
                } else {
                    full[q as usize].clone()
                };
                next == shift(&full[0], dq, 1)
            })
            .unwrap_or(p);
        let dq = [
            (self.d[0] as i64 * q as i64 / p as i64) as i32,
            (self.d[1] as i64 * q as i64 / p as i64) as i32,
        ];
        Some(CompoundPart {
            period: q,
            d: dq,
            phases: full[..q as usize].to_vec(),
        })
    }
}

/// A [`Runner`] helper for lattice tilings: closeness and distance from bounding boxes, given
/// the largest `|di|` and `|dj|` of a neighbour offset.
pub fn lattice_close(a: &[LCell], b: &[LCell], reach: (i32, i32)) -> bool {
    let (ba, bb) = (bbox(a), bbox(b));
    let (ri, rj) = (2 * reach.0, 2 * reach.1);
    ba[0] - bb[1] <= ri && bb[0] - ba[1] <= ri && ba[2] - bb[3] <= rj && bb[2] - ba[3] <= rj
}

/// The larger axis gap between two bounding boxes (0 when they overlap).
pub fn lattice_distance(a: &[LCell], b: &[LCell]) -> f64 {
    let (ba, bb) = (bbox(a), bbox(b));
    let gi = (ba[0] - bb[1]).max(bb[0] - ba[1]).max(0);
    let gj = (ba[2] - bb[3]).max(bb[2] - ba[3]).max(0);
    gi.max(gj) as f64
}

fn bbox(v: &[LCell]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
    for c in v {
        b[0] = b[0].min(c.i);
        b[1] = b[1].max(c.i);
        b[2] = b[2].min(c.j);
        b[3] = b[3].max(c.j);
    }
    b
}

/// A [`Runner`] helper for explicit patches (cells are ids in `i`): could `a` and `b` share a
/// neighbour? `nb(x, out)` pushes the neighbours of cell `x`.
pub fn explicit_close(a: &[LCell], b: &[LCell], nb: &dyn Fn(usize, &mut Vec<usize>)) -> bool {
    let mut near: HashSet<usize> = HashSet::new();
    let mut buf = Vec::new();
    for c in a {
        near.insert(c.i as usize);
        buf.clear();
        nb(c.i as usize, &mut buf);
        near.extend(buf.iter().copied());
    }
    b.iter().any(|c| {
        near.contains(&(c.i as usize)) || {
            buf.clear();
            nb(c.i as usize, &mut buf);
            buf.iter().any(|y| near.contains(y))
        }
    })
}

/// Neighbour steps from `a` to `b` on an explicit patch (searched up to 32 steps).
pub fn explicit_distance(a: &[LCell], b: &[LCell], nb: &dyn Fn(usize, &mut Vec<usize>)) -> f64 {
    let target: HashSet<usize> = b.iter().map(|c| c.i as usize).collect();
    let mut seen: HashSet<usize> = a.iter().map(|c| c.i as usize).collect();
    if seen.iter().any(|x| target.contains(x)) {
        return 0.0;
    }
    let mut frontier: Vec<usize> = seen.iter().copied().collect();
    let mut buf = Vec::new();
    for step in 1..=32 {
        let mut next = Vec::new();
        for &x in &frontier {
            buf.clear();
            nb(x, &mut buf);
            for &y in &buf {
                if seen.insert(y) {
                    if target.contains(&y) {
                        return step as f64;
                    }
                    next.push(y);
                }
            }
        }
        frontier = next;
    }
    1e9
}
