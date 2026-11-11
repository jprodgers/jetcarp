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
//!    a margin) and the bounding box changed since the last look: every group of the cells outside
//!    the region that is a spaceship on its own and is flying away from everything else (beyond
//!    the region and every other such group by a gap, on an axis it moves outwards along) is
//!    counted and erased;
//! 4. the state hash is recorded; if it equals the hash `p` generations earlier (with the same
//!    population) for some `p` up to `settle_period`, the soup **settled** with period `p`;
//! 5. at `max_gens` the soup is **unsettled**.
//!
//! The census then splits what is left into objects ([`Census::census`]). Steps 1, 2, 4, and 5
//! never look at anything but the state, and step 3 only acts once something has left the
//! region, so a stepper that runs the soup inside the region alone (the GPU) and hands it over the
//! first time a cell leaves gives the same result.

use super::bits::Universe;
use super::separate::{Cache, Census, Found, Obj, OtherKind, Part};
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
    /// Ships cut (with their displacement as they flew).
    pub cut: Vec<(Arc<Found>, [i32; 2])>,
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
        let groups = c.groups(&cells);
        let gap_i = 2 * c.plan.reach_i + 2;
        let gap_j = 2 * c.plan.reach_j + 2;
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
        for (x, g) in groups.iter().enumerate() {
            let b = g.bbox;
            if inside(b, self.region) {
                continue;
            }
            let rest = union(self.region, union(pre[x], suf[x + 1]));
            // Beyond everything else on some side (cheap; most groups fail here).
            let beyond = [
                b[0] > rest[1] + gap_i,
                b[1] < rest[0] - gap_i,
                b[2] > rest[3] + gap_j,
                b[3] < rest[2] - gap_j,
            ];
            if !beyond.iter().any(|&x| x) {
                continue;
            }
            let Some(ship) = super::gun::ship_of(c, cache, &g.cells) else {
                continue;
            };
            let d = ship.d;
            let away = (d[0] > 0 && beyond[0])
                || (d[0] < 0 && beyond[1])
                || (d[1] > 0 && beyond[2])
                || (d[1] < 0 && beyond[3]);
            if away {
                self.cut.push((ship.found.clone(), d));
                erase.extend_from_slice(&g.cells);
            }
        }
        if !erase.is_empty() {
            self.u.erase(&erase);
        }
    }
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
    for (f, _) in run.cut {
        if guns.contains(&f.id) {
            continue;
        }
        out.escaped += 1;
        out.objects.push(f);
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
