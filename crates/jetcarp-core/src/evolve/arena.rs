//! The fitness arena: a ball of `radius` edge rings around a root tile, and the run classifier
//! and score of Edmond's glider search, reimplemented for any JETCARP tiling.
//!
//! - **Rings** are edge-BFS distances from the root (always edges, also when the rule counts
//!   touching neighbours). The ball is every tile with ring ≤ radius, renumbered in BFS order,
//!   so cell 0 is the root and cell 1 its first edge neighbour. Tiles outside the ball are
//!   absent (not quiescent), like the open edge of a patch.
//! - **Fan margin**: the largest ring difference across one neighbour link of the rule's
//!   neighbourhood (2 on the Spectre and hat with touching neighbours). Activity can spread at
//!   most this many rings per generation.
//! - **Boundary**: `radius - margin`; outer rings have incomplete neighbourhoods.
//! - **Seed bank** for `k` states: the root alone in each state, the ordered pairs (root, cell 1)
//!   over the non-zero states, and the root's closed edge neighbourhood in state 1 (13 seeds for
//!   `k = 4`).
//! - **Run classification**, checked each generation in this order: population above the cap
//!   (`unbounded`), a changed cell at ring ≥ boundary (`reached_boundary`), population 0 with a
//!   vacuum that stays quiet (`died`), nothing changed (`periodic`, period 1), Brent's cycle
//!   finder on exact full-state snapshots (`periodic`), otherwise `active` at the horizon.
//! - **Score** of a run: `d · P_inner / P_max`, where `d` is the farthest ring of any changed cell,
//!   `P_max` the peak population (generation 0 included) and `P_inner` the peak while
//!   `d < boundary / 2`. Zero if the population ever exceeded the cap or if the run broke the
//!   light cone (`d > seed extent + generations · margin`, which only boundary artefacts can do).
//!   A genome's fitness is its best score over the seed bank (earliest seed on ties).

use super::priority::{MAX_STATES, PriorityRule};
use crate::families::FamilyKind;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A ball of tiles with ring numbers, as a compact neighbour graph.
pub struct Arena {
    pub tiling: TilingSpec,
    pub nb: Nb,
    pub radius: u32,
    pub margin: u32,
    pub boundary: u32,
    /// The tiling the ball was cut from (its size or level is the one used).
    pub topo: Arc<Topology>,
    /// Ball index -> tile of `topo`.
    pub cell_of: Vec<u32>,
    pub ring: Vec<u32>,
    off: Vec<u32>,
    adj: Vec<u32>,
    /// Ball cells by neighbour count (for rules whose quiet vacuum ignites).
    by_degree: Vec<Vec<u32>>,
    /// The root's edge neighbours (ball indices, ascending).
    root_edge: Vec<u32>,
}

/// Is this family an aperiodic patch family?
pub fn is_aperiodic(family: &str) -> bool {
    crate::families::family(family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

impl Arena {
    /// Cut a ball of `radius` edge rings around the centre tile of `tiling` (identity: family,
    /// parameters, edges). Aperiodic patches use the smallest level that holds the ball away
    /// from the open boundary; lattices a dead-edged grid large enough for it.
    pub fn build(tiling: &TilingSpec, nb: Nb, radius: u32) -> Result<Arena, String> {
        Self::build_with_room(tiling, nb, radius, radius)
    }

    /// Like [`Arena::build`], with room to grow the ball to `room` rings later around the same
    /// root ([`Arena::grown`]). Aperiodic patches: the root is the tile deepest inside the
    /// smallest level that has `room` rings around it (else the largest level's deepest tile).
    pub fn build_with_room(
        tiling: &TilingSpec,
        nb: Nb,
        radius: u32,
        room: u32,
    ) -> Result<Arena, String> {
        if radius < 4 {
            return Err("the arena radius must be at least 4".into());
        }
        let ident = crate::pattern::tiling_identity(tiling);
        if is_aperiodic(&ident.family) {
            let (lo, _) = crate::aperiodic::level_range(&ident.family);
            let hi = crate::aperiodic::arena_root_max(&ident.family);
            let mut last = String::new();
            for level in lo.max(2)..=hi {
                let spec = ident.clone().with_size(SizeSpec::Level(level));
                let topo = crate::pattern::site::patch_topology(&spec)?;
                let (root, depth) = deepest(&topo);
                if depth <= radius || (depth <= room && level < hi) {
                    last = format!("level {level} has {depth} rings inside its edge");
                    continue;
                }
                return Self::cut(&topo, root, nb, radius);
            }
            return Err(format!(
                "no {} patch holds a ball of radius {radius} ({last})",
                ident.family
            ));
        }
        let mut side = 2 * radius + 8;
        for _ in 0..4 {
            let spec = ident
                .clone()
                .with_size(SizeSpec::Rect { w: side, h: side })
                .with_boundary(Boundary::Dead);
            let topo = Arc::new(Topology::build(&spec)?);
            if topo.lattice().is_none() {
                return Err(format!("{} is neither a lattice nor a patch", ident.family));
            }
            match Self::cut(&topo, topo.center_cell(), nb, radius) {
                Ok(a) => return Ok(a),
                Err(_) => side *= 2,
            }
        }
        Err(format!(
            "could not fit a ball of radius {radius} on {}",
            ident.family
        ))
    }

    /// The same ball with a larger radius, rooted on the same tile (aperiodic: the tile at the
    /// same place on a larger level, which must have the same corners; lattices: a tile of the
    /// same type, which is the same up to translation).
    pub fn grown(&self, radius: u32) -> Result<Arena, String> {
        let root = self.cell_of[0] as usize;
        let ident = crate::pattern::tiling_identity(&self.tiling);
        if let Some(g) = self.topo.lattice() {
            let k = root % g.ntypes as usize;
            let mut side = 2 * radius + 8;
            for _ in 0..4 {
                let spec = ident
                    .clone()
                    .with_size(SizeSpec::Rect { w: side, h: side })
                    .with_boundary(Boundary::Dead);
                let topo = Arc::new(Topology::build(&spec)?);
                let nt = topo.lattice().map(|g| g.ntypes as usize).unwrap_or(1);
                let c = topo.center_cell();
                let r = c - c % nt + k;
                match Self::cut(&topo, r, self.nb, radius) {
                    Ok(a) => return Ok(a),
                    Err(_) => side *= 2,
                }
            }
            return Err(format!("could not grow the arena to radius {radius}"));
        }
        if let Ok(a) = Self::cut(&self.topo, root, self.nb, radius) {
            return Ok(a);
        }
        let level = match self.tiling.size {
            Some(SizeSpec::Level(l)) => l,
            _ => 1,
        };
        let (_, hi) = crate::aperiodic::level_range(&ident.family);
        let mut last = String::from("the largest patch is too small");
        for l in level + 1..=hi {
            let m = crate::aperiodic::embedding(&ident.family, level, l);
            let poly: Vec<crate::geom::P> = self
                .topo
                .polygon(root)
                .iter()
                .map(|&q| m.apply(q))
                .collect();
            let p = crate::pattern::site::interior_point(&poly);
            let spec = ident.clone().with_size(SizeSpec::Level(l));
            let topo = crate::pattern::site::patch_topology(&spec)?;
            let found = match crate::aperiodic::map_tile(
                &ident.family,
                level,
                l,
                root,
                &self.topo,
                &topo,
            ) {
                Some(r) => Some(r?),
                None => topo.locate(p),
            };
            let Some(t) = found else {
                last = "the root tile is not on the larger patch".into();
                continue;
            };
            if !same_polygon(&poly, &topo.polygon(t)) {
                return Err(format!(
                    "level {l} has a different tile at the root's place: the patches do not nest"
                ));
            }
            match Self::cut(&topo, t, self.nb, radius) {
                Ok(a) => return Ok(a),
                Err(e) => last = e,
            }
        }
        Err(format!(
            "no {} patch holds a ball of radius {radius} ({last})",
            ident.family
        ))
    }

    fn cut(topo: &Arc<Topology>, root: usize, nb: Nb, radius: u32) -> Result<Arena, String> {
        let n = topo.len();
        let mut ring = vec![u32::MAX; n];
        let mut order = vec![root as u32];
        ring[root] = 0;
        let mut head = 0;
        while head < order.len() {
            let x = order[head] as usize;
            head += 1;
            let r = ring[x];
            let incomplete = topo.is_open(x) || topo.missing(x, Nb::Edge) > 0;
            if incomplete || topo.missing(x, nb) > 0 {
                return Err(format!("tile at ring {r} is on the edge of the tiling"));
            }
            if r == radius {
                continue;
            }
            topo.for_each_neighbor(x, Nb::Edge, |y| {
                if ring[y] == u32::MAX {
                    ring[y] = r + 1;
                    order.push(y as u32);
                }
            });
        }
        let mut index = vec![u32::MAX; n];
        for (k, &x) in order.iter().enumerate() {
            index[x as usize] = k as u32;
        }
        let mut off = vec![0u32];
        let mut adj = Vec::new();
        let mut margin = 0;
        let mut by_degree: Vec<Vec<u32>> = Vec::new();
        for (k, &x) in order.iter().enumerate() {
            let mut row: Vec<u32> = Vec::new();
            topo.for_each_neighbor(x as usize, nb, |y| {
                if index[y] != u32::MAX {
                    row.push(index[y]);
                    margin = margin.max(ring[x as usize].abs_diff(ring[y]));
                }
            });
            row.sort_unstable();
            let d = row.len();
            if by_degree.len() <= d {
                by_degree.resize(d + 1, vec![]);
            }
            by_degree[d].push(k as u32);
            adj.extend(row);
            off.push(adj.len() as u32);
        }
        let mut root_edge: Vec<u32> = Vec::new();
        topo.for_each_neighbor(root, Nb::Edge, |y| root_edge.push(index[y]));
        root_edge.sort_unstable();
        let margin = margin.max(1);
        if margin >= radius {
            return Err("radius too small for this neighbourhood".into());
        }
        Ok(Arena {
            tiling: topo.spec.clone(),
            nb,
            radius,
            margin,
            boundary: radius - margin,
            topo: topo.clone(),
            ring: order.iter().map(|&x| ring[x as usize]).collect(),
            cell_of: order,
            off,
            adj,
            by_degree,
            root_edge,
        })
    }

    pub fn len(&self) -> usize {
        self.cell_of.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cell_of.is_empty()
    }
    #[inline]
    fn nbrs(&self, x: u32) -> &[u32] {
        &self.adj[self.off[x as usize] as usize..self.off[x as usize + 1] as usize]
    }

    /// The seed bank for `states` states (see the module notes).
    pub fn seeds(&self, states: u8) -> Vec<Vec<(u32, u8)>> {
        let mut out = Vec::new();
        for s in 1..states {
            out.push(vec![(0, s)]);
        }
        let partner = self.first_edge_neighbour();
        for a in 1..states {
            for b in 1..states {
                out.push(vec![(0, a), (partner, b)]);
            }
        }
        let mut ball = vec![(0, 1)];
        ball.extend(self.root_edge.iter().map(|&k| (k, 1)));
        out.push(ball);
        out
    }

    /// The lowest-numbered edge neighbour of the root (cell 1 in BFS order).
    pub fn first_edge_neighbour(&self) -> u32 {
        1
    }

    /// Run one seed. `cap = None` runs without a population cap (verification) up to
    /// `hard_cap` cells.
    pub fn run(
        &self,
        rule: &PriorityRule,
        seed: &[(u32, u8)],
        horizon: u64,
        cap: Option<usize>,
    ) -> RunStats {
        let mut sim = Sim::new(self, rule);
        sim.classify(seed, horizon, cap)
    }

    /// A genome's fitness: the best score over the seed bank (earliest seed on ties).
    pub fn fitness(&self, rule: &PriorityRule, horizon: u64, cap: usize) -> (f64, usize, RunStats) {
        let mut sim = Sim::new(self, rule);
        let mut best: Option<(f64, usize, RunStats)> = None;
        for (i, s) in self.seeds(rule.states).iter().enumerate() {
            let st = sim.classify(s, horizon, Some(cap));
            if best.as_ref().is_none_or(|b| st.fitness > b.0) {
                best = Some((st.fitness, i, st));
            }
        }
        best.unwrap()
    }
}

/// The tile farthest (in edge steps) from a patch's open boundary, nearest the patch centre on
/// ties, and its distance.
pub fn deepest(topo: &Topology) -> (usize, u32) {
    let n = topo.len();
    let mut d = vec![u32::MAX; n];
    let mut q = std::collections::VecDeque::new();
    for (x, dx) in d.iter_mut().enumerate() {
        if topo.is_open(x) {
            *dx = 0;
            q.push_back(x);
        }
    }
    while let Some(x) = q.pop_front() {
        let dx = d[x];
        topo.for_each_neighbor(x, Nb::Edge, |y| {
            if d[y] == u32::MAX {
                d[y] = dx + 1;
                q.push_back(y);
            }
        });
    }
    let c = topo.centroid(topo.center_cell());
    let mut best = (topo.center_cell(), 0u32, f64::INFINITY);
    for (x, &dx) in d.iter().enumerate() {
        if dx == u32::MAX {
            continue;
        }
        let r = crate::geom::dist(topo.centroid(x), c);
        if dx > best.1 || (dx == best.1 && r < best.2) {
            best = (x, dx, r);
        }
    }
    (best.0, best.1)
}

/// Same corners (any order), to 1e-6.
pub fn same_polygon(a: &[crate::geom::P], b: &[crate::geom::P]) -> bool {
    a.len() == b.len()
        && a.iter().all(|p| {
            b.iter()
                .any(|q| (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6)
        })
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Unbounded,
    ReachedBoundary,
    Died,
    Periodic,
    Active,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Unbounded => "unbounded",
            Outcome::ReachedBoundary => "reached_boundary",
            Outcome::Died => "died",
            Outcome::Periodic => "periodic",
            Outcome::Active => "active",
        }
    }
}

/// Statistics of one run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunStats {
    pub outcome: Outcome,
    /// Period for `periodic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    /// Generation the outcome was decided (the horizon for `active`).
    pub generation: u64,
    /// Farthest ring of any cell that changed.
    pub max_changed_distance: u32,
    pub max_population: usize,
    pub max_population_inner: usize,
    pub causal: bool,
    pub fitness: f64,
}

/// Cells above which an uncapped (verification) run is stopped as unbounded.
pub const HARD_CAP: usize = 50_000;

struct Sim<'a> {
    a: &'a Arena,
    rule: &'a PriorityRule,
    k: u8,
    state: Vec<u8>,
    live: Vec<u32>,
    mark: Vec<u32>,
    stamp: u32,
    cands: Vec<u32>,
    newv: Vec<u8>,
    ignite: Vec<u32>,
    ignites: bool,
}

impl<'a> Sim<'a> {
    fn new(a: &'a Arena, rule: &'a PriorityRule) -> Sim<'a> {
        let mut ignite = Vec::new();
        for (d, cells) in a.by_degree.iter().enumerate() {
            if !cells.is_empty() && rule.ignites(d) {
                ignite.extend_from_slice(cells);
            }
        }
        Sim {
            a,
            rule,
            k: rule.states.clamp(2, MAX_STATES),
            state: vec![0; a.len()],
            live: Vec::new(),
            mark: vec![0; a.len()],
            stamp: 0,
            cands: Vec::new(),
            newv: Vec::new(),
            ignites: !ignite.is_empty(),
            ignite,
        }
    }

    fn reset(&mut self, seed: &[(u32, u8)]) {
        for &x in &self.live {
            self.state[x as usize] = 0;
        }
        self.live.clear();
        for &(x, s) in seed {
            let s = s % self.k;
            if s != 0 && (x as usize) < self.state.len() && self.state[x as usize] == 0 {
                self.state[x as usize] = s;
                self.live.push(x);
            }
        }
        self.live.sort_unstable();
    }

    /// One generation; returns (changed, max ring of a changed cell).
    fn step(&mut self) -> (bool, u32) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.mark.fill(0);
            self.stamp = 1;
        }
        let st = self.stamp;
        self.cands.clear();
        for i in 0..self.live.len() {
            let x = self.live[i];
            if self.mark[x as usize] != st {
                self.mark[x as usize] = st;
                self.cands.push(x);
            }
            for &y in self.a.nbrs(x) {
                if self.mark[y as usize] != st {
                    self.mark[y as usize] = st;
                    self.cands.push(y);
                }
            }
        }
        for &x in &self.ignite {
            if self.mark[x as usize] != st {
                self.mark[x as usize] = st;
                self.cands.push(x);
            }
        }
        self.newv.clear();
        for &x in &self.cands {
            let mut c = [0u16; MAX_STATES as usize];
            for &y in self.a.nbrs(x) {
                c[self.state[y as usize] as usize] += 1;
            }
            self.newv
                .push(self.rule.next(self.state[x as usize], &c) % self.k);
        }
        let mut changed = false;
        let mut dmax = 0;
        self.live.clear();
        for (i, &x) in self.cands.iter().enumerate() {
            let v = self.newv[i];
            if v != self.state[x as usize] {
                changed = true;
                dmax = dmax.max(self.a.ring[x as usize]);
                self.state[x as usize] = v;
            }
            if v != 0 {
                self.live.push(x);
            }
        }
        self.live.sort_unstable();
        (changed, dmax)
    }

    fn snapshot(&self) -> Vec<(u32, u8)> {
        self.live
            .iter()
            .map(|&x| (x, self.state[x as usize]))
            .collect()
    }

    fn classify(&mut self, seed: &[(u32, u8)], horizon: u64, cap: Option<usize>) -> RunStats {
        self.reset(seed);
        let a = self.a;
        let cap_v = cap.unwrap_or(HARD_CAP);
        let extent = seed
            .iter()
            .map(|&(x, _)| a.ring.get(x as usize).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let half = a.boundary / 2;
        let mut d = 0u32;
        let mut pmax = self.live.len();
        let mut pinner = self.live.len();
        let mut tortoise = self.snapshot();
        let (mut power, mut lam) = (1u64, 0u64);
        let mut outcome = Outcome::Active;
        let mut period = None;
        let mut gen_end = horizon;
        for g in 1..=horizon {
            let (changed, dm) = self.step();
            d = d.max(dm);
            let pop = self.live.len();
            pmax = pmax.max(pop);
            if d < half {
                pinner = pinner.max(pop);
            }
            if pop > cap_v {
                outcome = Outcome::Unbounded;
                gen_end = g;
                break;
            }
            if changed && dm >= a.boundary {
                outcome = Outcome::ReachedBoundary;
                gen_end = g;
                break;
            }
            if pop == 0 && !self.ignites {
                outcome = Outcome::Died;
                gen_end = g;
                break;
            }
            if !changed {
                outcome = Outcome::Periodic;
                period = Some(1);
                gen_end = g;
                break;
            }
            lam += 1;
            if tortoise.len() == pop && tortoise == self.snapshot() {
                outcome = Outcome::Periodic;
                period = Some(lam);
                gen_end = g;
                break;
            }
            if lam == power {
                tortoise = self.snapshot();
                power *= 2;
                lam = 0;
            }
        }
        let causal = d as u64 <= extent as u64 + gen_end * a.margin as u64;
        let capped = cap.is_some_and(|c| pmax > c) || outcome == Outcome::Unbounded;
        let fitness = if !causal || capped {
            0.0
        } else {
            d as f64 * (pinner as f64 / pmax.max(1) as f64)
        };
        // Leave the field clean for the next seed.
        for &x in &self.live {
            self.state[x as usize] = 0;
        }
        self.live.clear();
        RunStats {
            outcome,
            period,
            generation: gen_end,
            max_changed_distance: d,
            max_population: pmax,
            max_population_inner: pinner,
            causal,
            fitness,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolve::priority::spectre_glider;

    #[test]
    fn spectre_arena_degrees_and_margin() {
        let a = Arena::build(&TilingSpec::new("spectre"), Nb::Touch, 24).unwrap();
        assert_eq!(
            a.margin, 2,
            "fan margin on the Spectre with touching neighbours"
        );
        assert_eq!(a.boundary, 22);
        assert_eq!(a.seeds(4).len(), 13);
        // Interior touch degrees 6-7, as in the paper.
        for x in 0..a.len() as u32 {
            if a.ring[x as usize] < 20 {
                let d = a.nbrs(x).len();
                assert!((6..=7).contains(&d), "degree {d}");
            }
        }
    }

    #[test]
    fn spectre_glider_scores_full_distance() {
        let a = Arena::build(&TilingSpec::new("spectre"), Nb::Touch, 32).unwrap();
        let (f, _, st) = a.fitness(&spectre_glider(), 2500, 64);
        assert_eq!(st.outcome, Outcome::ReachedBoundary, "{st:?}");
        assert!(f >= a.boundary as f64 * 0.9, "fitness {f}, {st:?}");
    }

    #[test]
    fn square_life_like_explosion_is_capped() {
        // B2-like: any quiescent cell with one state-1 neighbour is born; explodes.
        let r = PriorityRule::parse("Priority 2: 0|n1>=1->1; 1|always->1")
            .unwrap()
            .unwrap();
        let a = Arena::build(&TilingSpec::new("square"), Nb::Touch, 16).unwrap();
        let (f, _, st) = a.fitness(&r, 500, 64);
        assert_eq!(f, 0.0);
        assert_eq!(st.outcome, Outcome::Unbounded);
    }
}
