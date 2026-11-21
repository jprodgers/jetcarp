//! One soup: its cells, the run until it settles (with escaping ships cut out), and its census.
//!
//! **Soups.** Soup `i` of a hunt with seed `s` uses the RNG state [`soup_state`]`(s, i)`; each
//! cell of the soup's shape (in row, column, tile type order) is live with probability
//! `density`. Shapes: `rect` (`w` by `h` unit cells, rows sheared like JETCARP's grids so the soup
//! is a screen rectangle on hexagons) and `disc` (the tiles whose centres lie in the disc inscribed
//! in that rectangle).
//!
//! **The run** (all on the unbounded plane, [`Universe`]). After each generation `t`:
//!
//! 1. no live cells: the soup **died** (it may have sent ships away first);
//! 2. more than `pop_cap` live cells, or a bounding box wider or taller than `max_span`: it
//!    **exploded** (no census);
//! 3. every `cut_every` generations, if anything lies outside the **region** (the soup's box plus
//!    a margin) and the bounding box changed since the last look: the cells outside the region
//!    are split into groups, and every group that is a spaceship on its own (or several flying
//!    together) and that nothing else can reach is counted, as the ships it is made of, and
//!    erased. Nothing can reach it when it lies beyond the region by a gap on an axis it moves
//!    outwards along, and beyond each other group in the same way. Far from the region (by more
//!    than half its longer side), and while at most `FEW_OUT` groups lie out beyond it, groups
//!    that no gap separates are also taken together (two gliders on parallel lanes a few cells
//!    apart), and a group that is not beyond another in that way is still clear of it when the
//!    other is a far ship that cannot close the gap: one with the same velocity (gliders flying
//!    the same way, however far apart), or one slower along an axis this one is beyond it on;
//! 4. the state hash is recorded; if it equals the hash `p` generations earlier (with the same
//!    population) for some `p` up to `settle_period`, the soup **settled** with period `p`;
//! 5. at `max_gens` the soup is **unsettled**.
//!
//! The census then splits what is left into objects ([`Census::census`]). Steps 1, 2, 4, and 5
//! never look at anything but the state, and step 3 only acts once something has left the
//! region, so a stepper that runs the soup inside the region alone (the GPU) and hands it over the
//! first time a cell leaves gives the same result.

use super::bits::Universe;
use super::separate::{Cache, Census, Found, Group, Obj, OtherKind, Part};
use crate::objects::LCell;
use crate::rng::Rng;
use crate::topology::Topology;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoupShape {
    #[default]
    Rect,
    Disc,
}

impl SoupShape {
    pub fn parse(s: &str) -> Result<SoupShape, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rect" | "square" | "box" => Ok(SoupShape::Rect),
            "disc" | "disk" | "circle" => Ok(SoupShape::Disc),
            _ => Err(format!("unknown soup shape '{s}' (rect, disc)")),
        }
    }
}

/// Size, density, and shape of every soup.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoupSpec {
    pub w: u32,
    pub h: u32,
    pub density: f64,
    #[serde(default)]
    pub shape: SoupShape,
}

impl Default for SoupSpec {
    fn default() -> Self {
        SoupSpec {
            w: 16,
            h: 16,
            density: 0.5,
            shape: SoupShape::Rect,
        }
    }
}

/// The RNG state of soup `index` in a hunt with this seed.
pub fn soup_state(seed: u64, index: u64) -> u64 {
    let mut r = Rng::new(seed ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    r.next_u64() ^ index
}

/// Where a soup's cells go, and the region it may fill before ships are cut.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Candidate cells in fill order.
    pub cells: Vec<(i32, i32, u32)>,
    /// `[i_min, i_max, j_min, j_max]`: the soup's box plus the margin.
    pub region: [i32; 4],
    pub spec: SoupSpec,
}

impl Layout {
    pub fn new(topo: &Topology, spec: SoupSpec, margin: u32) -> Result<Layout, String> {
        let g = topo.lattice().ok_or("soups need a periodic tiling")?;
        if spec.w == 0 || spec.h == 0 || spec.w > 4096 || spec.h > 4096 {
            return Err("soup size must be 1 to 4096 unit cells on a side".into());
        }
        if !(0.0..=1.0).contains(&spec.density) {
            return Err("density must be between 0 and 1".into());
        }
        let shear = g.a2[0] / g.a1[0];
        let rs = |j: i32| -> i32 { -((j as f64 * shear + 1e-9).floor() as i32) };
        // Tile centres (for discs).
        let mut cells = Vec::new();
        let (w, h) = (spec.w as i32, spec.h as i32);
        let tc: Vec<[f64; 2]> = g
            .types
            .iter()
            .map(|(m, sh)| m.apply(crate::geom::centroid(&topo.shapes[*sh])))
            .collect();
        let centre = |i: i32, j: i32, k: u32| -> [f64; 2] {
            let c = tc[k as usize];
            [
                i as f64 * g.a1[0] + j as f64 * g.a2[0] + c[0],
                i as f64 * g.a1[1] + j as f64 * g.a2[1] + c[1],
            ]
        };
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for j in 0..h {
            for u in 0..w {
                for k in 0..g.ntypes {
                    let p = centre(rs(j) + u, j, k);
                    for a in 0..2 {
                        lo[a] = lo[a].min(p[a]);
                        hi[a] = hi[a].max(p[a]);
                    }
                }
            }
        }
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let r = ((hi[0] - lo[0]).min(hi[1] - lo[1]) / 2.0).max(0.5) + 1e-9;
        for j in 0..h {
            for u in 0..w {
                let i = rs(j) + u;
                for k in 0..g.ntypes {
                    if spec.shape == SoupShape::Disc {
                        let p = centre(i, j, k);
                        if crate::detmath::hypot(p[0] - mid[0], p[1] - mid[1]) > r {
                            continue;
                        }
                    }
                    cells.push((i, j, k));
                }
            }
        }
        if cells.is_empty() {
            return Err("the soup shape holds no cells".into());
        }
        let m = margin as i32;
        let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
        for &(i, j, _) in &cells {
            b = [b[0].min(i), b[1].max(i), b[2].min(j), b[3].max(j)];
        }
        Ok(Layout {
            cells,
            region: [b[0] - m, b[1] + m, b[2] - m, b[3] + m],
            spec,
        })
    }

    /// The live cells of the soup with this RNG state.
    pub fn soup(&self, rng_state: u64) -> Vec<LCell> {
        let mut r = Rng::new(rng_state);
        let mut v = Vec::new();
        for &(i, j, k) in &self.cells {
            if r.next_f64() < self.spec.density {
                v.push(LCell { i, j, k, s: 1 });
            }
        }
        v.sort_unstable();
        v
    }
}

/// When a soup stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_gens: u64,
    /// Longest whole-universe period that counts as settled.
    pub settle_period: u64,
    pub pop_cap: u64,
    pub max_span: i32,
    pub cut_every: u64,
}

/// How a soup ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// Periodic (after escaping ships were cut).
    Settled,
    /// Nothing left (after escaping ships were cut).
    Died,
    /// Too many cells, or spread too far: no census.
    Exploded,
    /// Not settled by the generation limit (censused as it was).
    Unsettled,
}

impl End {
    pub fn name(self) -> &'static str {
        match self {
            End::Settled => "settled",
            End::Died => "died",
            End::Exploded => "exploded",
            End::Unsettled => "unsettled",
        }
    }
}

/// The recorded history of a run: state hash and population per generation, the last
/// `settle_period + 1` of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ring {
    pub len: usize,
    pub hashes: Vec<u64>,
    pub pops: Vec<u64>,
}

impl Ring {
    pub fn new(settle_period: u64) -> Ring {
        let len = settle_period as usize + 1;
        Ring {
            len,
            hashes: vec![0; len],
            pops: vec![u64::MAX; len],
        }
    }
    pub fn put(&mut self, t: u64, hash: u64, pop: u64) {
        let x = (t % self.len as u64) as usize;
        self.hashes[x] = hash;
        self.pops[x] = pop;
    }
    /// The smallest `p` with the same hash and population `p` generations before `t`.
    pub fn period(&self, t: u64, hash: u64, pop: u64) -> Option<u64> {
        let n = self.len;
        let mut x = (t % n as u64) as usize;
        for p in 1..(n as u64).min(t + 1) {
            // x = (t - p) mod n
            x = if x == 0 { n - 1 } else { x - 1 };
            if self.hashes[x] == hash && self.pops[x] == pop {
                return Some(p);
            }
        }
        None
    }
}

/// A soup in progress.
pub struct SoupRun {
    pub u: Universe,
    pub ring: Ring,
    region: [i32; 4],
    limits: Limits,
    last_look: Option<[i32; 4]>,
    /// Ships cut (with their displacement as they flew, and the independent ships they are made
    /// of when several flew together as one group; empty when one).
    pub cut: Vec<(Arc<Found>, [i32; 2], Vec<Arc<Found>>)>,
    /// When set, every ship cut is also kept here with its cells ([`CutShip`]), for
    /// [`sightings`]; a hunt leaves it unset.
    pub cut_cells: Option<Vec<CutShip>>,
    pub end: Option<End>,
    pub period: u64,
}

fn inside(b: [i32; 4], r: [i32; 4]) -> bool {
    b[0] >= r[0] && b[1] <= r[1] && b[2] >= r[2] && b[3] <= r[3]
}

impl SoupRun {
    /// Start a soup (generation 0 recorded).
    pub fn new(u: Universe, region: [i32; 4], limits: Limits) -> SoupRun {
        let mut ring = Ring::new(limits.settle_period);
        ring.put(u.generation, u.hash(), u.population());
        let mut r = SoupRun {
            u,
            ring,
            region,
            limits,
            last_look: None,
            cut: vec![],
            cut_cells: None,
            end: None,
            period: 0,
        };
        if r.u.is_empty() {
            r.end = Some(End::Died);
        }
        r
    }

    /// Continue a soup handed over at generation `u.generation`, before that generation was
    /// processed (`ring` holds the generations before it).
    pub fn resume(u: Universe, ring: Ring, region: [i32; 4], limits: Limits) -> SoupRun {
        SoupRun {
            u,
            ring,
            region,
            limits,
            last_look: None,
            cut: vec![],
            cut_cells: None,
            end: None,
            period: 0,
        }
    }

    /// Process the state at the current generation (see the module notes).
    pub fn post(&mut self, c: &Census, cache: &mut Cache) {
        let t = self.u.generation;
        if self.u.is_empty() {
            self.end = Some(End::Died);
            return;
        }
        let bb = self.u.bbox().unwrap();
        if self.u.population() > self.limits.pop_cap
            || bb[1] - bb[0] > self.limits.max_span
            || bb[3] - bb[2] > self.limits.max_span
        {
            self.end = Some(End::Exploded);
            return;
        }
        if t.is_multiple_of(self.limits.cut_every)
            && !inside(bb, self.region)
            && self.last_look != Some(bb)
        {
            self.cut_ships(c, cache);
            self.last_look = self.u.bbox();
            if self.u.is_empty() {
                self.end = Some(End::Died);
                return;
            }
        }
        let (h, p) = (self.u.hash(), self.u.population());
        if let Some(per) = self.ring.period(t, h, p) {
            self.end = Some(End::Settled);
            self.period = per;
            return;
        }
        self.ring.put(t, h, p);
        if t >= self.limits.max_gens {
            self.end = Some(End::Unsettled);
        }
    }

    /// Run to the end.
    pub fn run(&mut self, c: &Census, cache: &mut Cache) {
        while self.end.is_none() {
            self.u.step();
            self.post(c, cache);
        }
    }

    fn cut_ships(&mut self, c: &Census, cache: &mut Cache) {
        // Only cells outside the region can be cut; the region stands for everything inside it
        // (a conservative "rest of the soup").
        let cells = self.u.cells_outside(self.region);
        let gap_i = 2 * c.plan.reach_i + 2;
        let gap_j = 2 * c.plan.reach_j + 2;
        let gap = [gap_i, gap_j];
        let region = self.region;
        // Out: beyond the region by the gap on some side (a ship flying away, perhaps). Far: out
        // by more than half the region's longer side again, well clear of anything the soup is
        // still doing next to the region.
        let reach = (region[1] - region[0]).max(region[3] - region[2]) / 2;
        let far_gap = [gap_i + reach, gap_j + reach];
        let is_out = |b: [i32; 4]| beyond(b, region, gap).iter().any(|&s| s);
        let is_far = |b: [i32; 4]| beyond(b, region, far_gap).iter().any(|&s| s);
        let groups = c.groups(&cells);
        // Few groups out there (ships flying off, not a soup spreading far): then far out,
        // groups that no gap separates are taken together (two gliders on parallel lanes a few
        // cells apart are never a gap from each other, and are cut as one group of two ships),
        // and ships that cannot reach each other are told apart by their velocities (below).
        let few = groups.iter().filter(|g| is_out(g.bbox)).count() <= FEW_OUT;
        // With the cells of the groups each was taken together from (none when it is one).
        let (groups, parts): (Vec<Group>, Vec<Vec<Vec<LCell>>>) = if few {
            flocks(groups, gap_i, gap_j, &is_far).into_iter().unzip()
        } else {
            let n = groups.len();
            (groups, vec![Vec::new(); n])
        };
        // A group taken together from several is run as one only when each of them is a ship
        // (a ship passing debris is not, and would be run for nothing).
        let ship_at = |cache: &mut Cache, x: usize| {
            if parts[x]
                .iter()
                .all(|p| super::gun::ship_of(c, cache, p).is_some())
            {
                super::gun::ship_of(c, cache, &groups[x].cells)
            } else {
                None
            }
        };
        let mut erase: Vec<LCell> = Vec::new();
        // The bounding box of every group but one, from prefix and suffix unions.
        let n = groups.len();
        let union = |a: [i32; 4], b: [i32; 4]| {
            [
                a[0].min(b[0]),
                a[1].max(b[1]),
                a[2].min(b[2]),
                a[3].max(b[3]),
            ]
        };
        let empty = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
        let mut pre = vec![empty; n + 1];
        let mut suf = vec![empty; n + 1];
        for x in 0..n {
            pre[x + 1] = union(pre[x], groups[x].bbox);
            suf[n - 1 - x] = union(suf[n - x], groups[n - 1 - x].bbox);
        }
        let far: Vec<bool> = groups.iter().map(|g| few && is_far(g.bbox)).collect();
        // Each far group's velocity when it is a ship (period, displacement), found when needed.
        let mut vel: Vec<Option<Option<(u64, [i32; 2])>>> = vec![None; n];
        for (x, g) in groups.iter().enumerate() {
            let b = g.bbox;
            // Beyond the region on some side (cheap; most groups fail here).
            let off = beyond(b, region, gap);
            if !off.iter().any(|&s| s) {
                continue;
            }
            // Beyond everything else on some side, as a ship must be to be cut, unless it is
            // far out, where the velocity test below may clear it.
            let ahead = beyond(b, union(pre[x], suf[x + 1]), gap);
            if !far[x] && (!parts[x].is_empty() || !ahead.iter().any(|&s| s)) {
                continue;
            }
            let Some(ship) = ship_at(cache, x) else {
                continue;
            };
            let (p, d) = (ship.period, ship.d);
            vel[x] = Some(Some((p, d)));
            if !away(d, off) {
                continue;
            }
            // Nothing else can reach it: it is beyond everything else on a side it moves out
            // along (the common case, from the unions), or, far out, beyond each other group
            // so, or that group is a far ship that cannot close the gap. A ship with the same
            // velocity never can (every two far groups are a gap apart), so gliders flying the
            // same way on lanes far apart are cut too; nor can one slower along an axis this one
            // is beyond it on.
            let clear = away(d, ahead)
                || far[x]
                    && (0..n).filter(|&y| y != x).all(|y| {
                        let side = beyond(b, groups[y].bbox, gap);
                        if away(d, side) {
                            return true;
                        }
                        if !far[y] {
                            return false;
                        }
                        let v = *vel[y]
                            .get_or_insert_with(|| ship_at(cache, y).map(|s| (s.period, s.d)));
                        let Some((q, e)) = v else {
                            return false;
                        };
                        // Velocities d / p and e / q, compared along each axis and side.
                        let (vg, vh) = (
                            [d[0] as i64 * q as i64, d[1] as i64 * q as i64],
                            [e[0] as i64 * p as i64, e[1] as i64 * p as i64],
                        );
                        vg == vh
                            || (0..4).any(|k| {
                                let (a, s) = (k / 2, if k % 2 == 0 { 1 } else { -1 });
                                side[k] && s * vg[a] > 0 && s * vg[a] >= s * vh[a]
                            })
                    });
            if clear && !bound(c, &groups, x) {
                if let Some(v) = &mut self.cut_cells {
                    v.push(CutShip {
                        generation: self.u.generation,
                        found: ship.found.clone(),
                        period: p,
                        d,
                        cells: g.cells.clone(),
                        parts: parts[x].clone(),
                    });
                }
                self.cut.push((ship.found.clone(), d, ship.parts.clone()));
                erase.extend_from_slice(&g.cells);
            }
        }
        if !erase.is_empty() {
            self.u.erase(&erase);
        }
    }
}

/// Groups out beyond the region above which a look cuts only a group beyond everything else (a
/// soup spreading far, with many groups out there: taking them together, and running each to
/// learn its velocity, would cost much and cut little).
const FEW_OUT: usize = 128;

/// On which sides box `b` lies beyond box `o` by more than the gap: `[+i, -i, +j, -j]`.
fn beyond(b: [i32; 4], o: [i32; 4], gap: [i32; 2]) -> [bool; 4] {
    [
        b[0] > o[1] + gap[0],
        b[1] < o[0] - gap[0],
        b[2] > o[3] + gap[1],
        b[3] < o[2] - gap[1],
    ]
}

/// Does displacement `d` carry a box outward on a side where it is beyond (`[+i, -i, +j, -j]`)?
fn away(d: [i32; 2], side: [bool; 4]) -> bool {
    (d[0] > 0 && side[0]) || (d[0] < 0 && side[1]) || (d[1] > 0 && side[2]) || (d[1] < 0 && side[3])
}

/// Far groups (`far` of their boxes) merged until every two are separated by more than the gap
/// (`gap_i`, `gap_j`) on some axis, so each one left is either beyond another by the gap or not
/// apart from it at all; the other groups are left alone. Each merged group's cells are sorted and
/// its touching parts are not kept (cutting needs only the cells and the box); with it come the
/// cells of the groups it was merged from (none for a group left alone). Groups stay in order of
/// their first cell.
fn flocks(
    groups: Vec<Group>,
    gap_i: i32,
    gap_j: i32,
    far: &dyn Fn([i32; 4]) -> bool,
) -> Vec<(Group, Vec<Vec<LCell>>)> {
    let mut groups: Vec<(Group, Vec<Vec<LCell>>)> =
        groups.into_iter().map(|g| (g, Vec::new())).collect();
    loop {
        let n = groups.len();
        let mut at: Vec<usize> = (0..n).collect();
        let mut any = false;
        // Sweep in order of the boxes' least i: a later box that starts more than the gap past
        // this one's end, and every box after it, is apart from it along i.
        let mut order: Vec<usize> = (0..n).filter(|&x| far(groups[x].0.bbox)).collect();
        order.sort_by_key(|&x| (groups[x].0.bbox[0], x));
        for (k, &x) in order.iter().enumerate() {
            let a = groups[x].0.bbox;
            for &y in &order[k + 1..] {
                let b = groups[y].0.bbox;
                if b[0] > a[1] + gap_i {
                    break;
                }
                if b[2] > a[3] + gap_j || a[2] > b[3] + gap_j {
                    continue;
                }
                let (rx, ry) = (root(&mut at, x), root(&mut at, y));
                if rx != ry {
                    at[rx.max(ry)] = rx.min(ry);
                    any = true;
                }
            }
        }
        if !any {
            return groups;
        }
        // Merge each set into its first group (a merged box may now meet another: go again).
        let mut out: Vec<(Group, Vec<Vec<LCell>>)> = Vec::new();
        let mut slot: Vec<Option<usize>> = vec![None; n];
        for (x, (g, from)) in groups.into_iter().enumerate() {
            let r = root(&mut at, x);
            match slot[r] {
                None => {
                    slot[r] = Some(out.len());
                    out.push((g, from));
                }
                Some(s) => {
                    let (o, o_from) = &mut out[s];
                    if o_from.is_empty() {
                        o_from.push(o.cells.clone());
                    }
                    if from.is_empty() {
                        o_from.push(g.cells.clone());
                    } else {
                        o_from.extend(from);
                    }
                    o.cells.extend(g.cells);
                    o.cells.sort_unstable();
                    o.subs.clear();
                    o.bbox = [
                        o.bbox[0].min(g.bbox[0]),
                        o.bbox[1].max(g.bbox[1]),
                        o.bbox[2].min(g.bbox[2]),
                        o.bbox[3].max(g.bbox[3]),
                    ];
                }
            }
        }
        groups = out;
    }
}

fn root(at: &mut [usize], mut x: usize) -> usize {
    while at[x] != x {
        at[x] = at[at[x]];
        x = at[x];
    }
    x
}

/// Is group `x` (a ship) one half of a bound object: with a group near it (within
/// `piece_distance` neighbour steps past the cutting gap) a spaceship, which runs otherwise
/// than the two of them alone? Then the ship is not cut yet; the whole object will be, once it
/// is one group.
fn bound(c: &Census, groups: &[Group], x: usize) -> bool {
    use super::separate::Run;
    use crate::objects::compound::{Runner, horizon, union_is};
    let r = c.plan.reach_i.max(c.plan.reach_j);
    let reach = 2 * r + 2 + c.piece_distance as i32 * r;
    let b = groups[x].bbox;
    groups.iter().enumerate().any(|(y, h)| {
        let o = h.bbox;
        let near = y != x
            && o[0] - b[1] <= reach
            && b[0] - o[1] <= reach
            && o[2] - b[3] <= reach
            && b[2] - o[3] <= reach;
        if !near {
            return false;
        }
        let mut both = groups[x].cells.clone();
        both.extend(h.cells.iter().copied());
        both.sort_unstable();
        let Run::Periodic { period, d, .. } = c.run_alone(&both, c.max_period) else {
            return false;
        };
        if d == [0, 0] {
            return false;
        }
        let l = horizon(period);
        match (
            c.run(&groups[x].cells, l),
            c.run(&h.cells, l),
            c.run(&both, l),
        ) {
            (Some(a), Some(other), Some(j)) => {
                (0..=l as usize).any(|t| !union_is(&a[t], &other[t], &j[t]))
            }
            _ => false,
        }
    })
}

/// What one soup gave.
#[derive(Clone, Debug)]
pub struct SoupOut {
    pub index: u64,
    pub rng_state: u64,
    pub end: End,
    /// Settled: the period of the whole universe.
    pub period: u64,
    pub gens: u64,
    /// Every object copy: the census of what was left, plus the ships cut on the way.
    pub objects: Vec<Arc<Found>>,
    /// Objects that are not patterns, by kind.
    pub other: [u32; 3],
    /// Ships cut (also in `objects`).
    pub escaped: u32,
}

/// The census of a finished run.
pub fn finish(c: &Census, cache: &mut Cache, run: SoupRun, index: u64, rng_state: u64) -> SoupOut {
    let end = run.end.unwrap_or(End::Unsettled);
    let mut out = SoupOut {
        index,
        rng_state,
        end,
        period: run.period,
        gens: run.u.generation,
        objects: vec![],
        other: [0; 3],
        escaped: 0,
    };
    let mut found: Vec<Arc<Found>> = Vec::new();
    if end != End::Exploded {
        let parts: Vec<Part> = c.census(cache, &run.u.cells());
        for p in parts {
            match p.obj {
                Obj::Pattern(f) => found.push(f),
                Obj::Other(k) => {
                    out.other[match k {
                        OtherKind::Dies => 0,
                        OtherKind::Grows => 1,
                        OtherKind::Unsettled => 2,
                    }] += 1
                }
            }
        }
    }
    // Ships a gun sends out are its stream, not separate finds (whether in flight or cut).
    let guns: Vec<String> = found
        .iter()
        .filter_map(|f| f.record.gun.as_ref().map(|g| g.ship.clone()))
        .collect();
    out.objects = found
        .into_iter()
        .filter(|f| !guns.contains(&f.id))
        .collect();
    for (f, _, parts) in run.cut {
        if guns.contains(&f.id) {
            continue;
        }
        // Ships that flew together as one group are counted as the ships they are.
        if parts.is_empty() {
            out.escaped += 1;
            out.objects.push(f);
        } else {
            out.escaped += parts.len() as u32;
            out.objects.extend(parts);
        }
    }
    out
}

/// A ship cut from a soup, where it was ([`SoupRun::cut_cells`]).
#[derive(Clone, Debug)]
pub struct CutShip {
    /// The generation it was cut at, its find, period and displacement per period.
    pub generation: u64,
    pub found: Arc<Found>,
    pub period: u64,
    pub d: [i32; 2],
    /// Its cells then, and when it was several ships flying together, each one's.
    pub cells: Vec<LCell>,
    pub parts: Vec<Vec<LCell>>,
}

/// A copy of a find in a soup, as [`sightings`] followed it back from where the run took it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sighting {
    /// The generation the run took it at: as it flew off (`cut`, a spaceship cut out), or at the
    /// soup's end, in the census of what was left.
    pub at: u64,
    pub cut: bool,
    /// The first generation from which it is there, whole and apart from everything else, until
    /// `at`, to within its period. `None` when it could not be told apart: taken with other
    /// objects in one group, it has no cells of its own.
    pub first: Option<u64>,
    /// Its cells at `at` (none when it could not be told apart), its period, and its
    /// displacement per period (zero when it stays put).
    pub cells: Vec<LCell>,
    pub period: u64,
    pub d: [i32; 2],
}

/// The find in these cells alone, when they are one object and it is the find `id`: its period
/// and displacement.
fn alone_as(c: &Census, cache: &mut Cache, cells: &[LCell], id: &str) -> Option<(u64, [i32; 2])> {
    let parts = c.census(cache, cells);
    match parts.as_slice() {
        [p] if matches!(&p.obj, Obj::Pattern(f) if f.id == id) => Some((p.period, p.d)),
        _ => None,
    }
}

/// Every copy of the find `id` in the soup with this RNG state, and the generation each first
/// shows ([`Sighting`]): the soup is run as [`run_soup`] runs it, every copy taken (cut as it
/// flew off, or in the census of what was left) noted with its cells, and the soup is run again,
/// checking at each period before that whether those cells, moved back along its path, are all
/// there with nothing else within twice the rule's reach of them, so that they run as if alone.
pub fn sightings(
    c: &Census,
    cache: &mut Cache,
    layout: &Layout,
    limits: Limits,
    rng_state: u64,
    id: &str,
) -> Vec<Sighting> {
    let start = layout.soup(rng_state);
    let mut run = SoupRun::new(
        Universe::with_cells(c.plan.clone(), &start),
        layout.region,
        limits,
    );
    run.cut_cells = Some(Vec::new());
    run.run(c, cache);
    let mut out = Vec::new();
    for s in run.cut_cells.take().unwrap_or_default() {
        if s.parts.is_empty() {
            if s.found.id == id {
                out.push(Sighting {
                    at: s.generation,
                    cut: true,
                    first: None,
                    cells: s.cells,
                    period: s.period.max(1),
                    d: s.d,
                });
            }
            continue;
        }
        // Several ships flying together, at one speed: each one that is the find.
        for part in s.parts {
            if alone_as(c, cache, &part, id).is_some() {
                out.push(Sighting {
                    at: s.generation,
                    cut: true,
                    first: None,
                    cells: part,
                    period: s.period.max(1),
                    d: s.d,
                });
            }
        }
    }
    let end = run.u.generation;
    if run.end != Some(End::Exploded) {
        let left = run.u.cells();
        for g in c.groups(&left) {
            let parts = c.census(cache, &g.cells);
            let mine = |p: &&Part| matches!(&p.obj, Obj::Pattern(f) if f.id == id);
            match parts.as_slice() {
                // The find alone: followed back from its cells.
                [p] if mine(&p) => out.push(Sighting {
                    at: end,
                    cut: false,
                    first: None,
                    cells: g.cells,
                    period: p.period.max(1),
                    d: p.d,
                }),
                // Copies among other objects in one group: counted, not followed.
                _ => {
                    for p in parts.iter().filter(mine) {
                        out.push(Sighting {
                            at: end,
                            cut: false,
                            first: None,
                            cells: vec![],
                            period: p.period.max(1),
                            d: p.d,
                        });
                    }
                }
            }
        }
    }
    if out.iter().all(|s| s.cells.is_empty()) {
        return out;
    }
    // Run it again: at each period before a copy was taken, is it there, alone?
    let margin = [2 * c.plan.reach_i, 2 * c.plan.reach_j];
    let last = out.iter().map(|s| s.at).max().unwrap_or(0);
    let mut streak: Vec<Option<u64>> = vec![None; out.len()];
    let mut again = SoupRun::new(
        Universe::with_cells(c.plan.clone(), &start),
        layout.region,
        limits,
    );
    let look = |u: &Universe, streak: &mut Vec<Option<u64>>| {
        let t = u.generation;
        for (x, s) in out.iter().enumerate() {
            if s.cells.is_empty() || t > s.at || (s.at - t) % s.period != 0 {
                continue;
            }
            let k = ((s.at - t) / s.period) as i32;
            let mut want: Vec<LCell> = s
                .cells
                .iter()
                .map(|q| LCell {
                    i: q.i - k * s.d[0],
                    j: q.j - k * s.d[1],
                    ..*q
                })
                .collect();
            want.sort_unstable();
            let b = want
                .iter()
                .fold([i32::MAX, i32::MIN, i32::MAX, i32::MIN], |b, q| {
                    [b[0].min(q.i), b[1].max(q.i), b[2].min(q.j), b[3].max(q.j)]
                });
            let mut have = u.cells_within([
                b[0] - margin[0],
                b[1] + margin[0],
                b[2] - margin[1],
                b[3] + margin[1],
            ]);
            have.sort_unstable();
            streak[x] = if have == want {
                streak[x].or(Some(t))
            } else {
                None
            };
        }
    };
    look(&again.u, &mut streak);
    while again.end.is_none() && again.u.generation < last {
        again.u.step();
        look(&again.u, &mut streak);
        again.post(c, cache);
    }
    for (s, first) in out.iter_mut().zip(streak) {
        s.first = first;
    }
    out
}

/// Run one soup from its cells to its census.
pub fn run_soup(
    c: &Census,
    cache: &mut Cache,
    layout: &Layout,
    limits: Limits,
    index: u64,
    rng_state: u64,
) -> SoupOut {
    let cells = layout.soup(rng_state);
    let u = Universe::with_cells(c.plan.clone(), &cells);
    let mut run = SoupRun::new(u, layout.region, limits);
    run.run(c, cache);
    finish(c, cache, run, index, rng_state)
}

/// The start of a soup run inside its region (what the GPU computes): see the module notes.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefix {
    /// Live cells at `generation`.
    pub cells: Vec<LCell>,
    pub generation: u64,
    /// The history before `generation`.
    pub ring: Ring,
    /// Set when the soup ended inside the region (with the settled period, else 0).
    pub end: Option<(End, u64)>,
    /// Not ended: true when a cell has left the region at `generation`, which is not processed
    /// yet; false when the stepper simply stopped there (the generation is in `ring`).
    pub pending: bool,
}

/// A stepper that runs the start of many soups at once (the GPU). See the module notes for the
/// rules a prefix must follow so the results equal the CPU's.
pub trait SoupBatcher: Send + Sync {
    fn name(&self) -> String;
    /// Can it run this group? (Rule, tiling, and region size.)
    fn supports(&self, census: &Census, region: [i32; 4]) -> bool;
    /// Run soups (generation 0 cells) inside `region` until each ends or a cell leaves the
    /// region; one result per soup, in order.
    fn prefix(
        &self,
        census: &Census,
        region: [i32; 4],
        limits: Limits,
        soups: &[Vec<LCell>],
    ) -> Result<Vec<Prefix>, String>;
}

/// The CPU reference for a soup prefix: run inside the region until the soup ends or a cell
/// leaves it.
pub fn prefix_cpu(c: &Census, region: [i32; 4], limits: Limits, cells: &[LCell]) -> Prefix {
    let mut u = Universe::with_cells(c.plan.clone(), cells);
    let mut ring = Ring::new(limits.settle_period);
    ring.put(0, u.hash(), u.population());
    let done = |u: &Universe, ring: Ring, end: End, p: u64| Prefix {
        cells: u.cells(),
        generation: u.generation,
        ring,
        end: Some((end, p)),
        pending: false,
    };
    if u.is_empty() {
        return done(&u, ring, End::Died, 0);
    }
    loop {
        u.step();
        let t = u.generation;
        if u.is_empty() {
            return done(&u, ring, End::Died, 0);
        }
        if u.population() > limits.pop_cap {
            return done(&u, ring, End::Exploded, 0);
        }
        if !inside(u.bbox().unwrap(), region) {
            return Prefix {
                cells: u.cells(),
                generation: t,
                ring,
                end: None,
                pending: true,
            };
        }
        let (h, p) = (u.hash(), u.population());
        if let Some(per) = ring.period(t, h, p) {
            return done(&u, ring, End::Settled, per);
        }
        ring.put(t, h, p);
        if t >= limits.max_gens {
            return done(&u, ring, End::Unsettled, 0);
        }
    }
}

/// Finish a soup from its prefix (the same result as [`run_soup`]).
pub fn continue_soup(
    c: &Census,
    cache: &mut Cache,
    layout: &Layout,
    limits: Limits,
    index: u64,
    rng_state: u64,
    prefix: Prefix,
) -> SoupOut {
    let mut u = Universe::with_cells(c.plan.clone(), &prefix.cells);
    u.generation = prefix.generation;
    let mut run = SoupRun::resume(u, prefix.ring, layout.region, limits);
    match prefix.end {
        Some((end, p)) => {
            run.end = Some(end);
            run.period = p;
        }
        None => {
            if prefix.pending {
                run.post(c, cache);
            }
            run.run(c, cache);
        }
    }
    finish(c, cache, run, index, rng_state)
}
