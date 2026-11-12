//! Object separation and classification for the hunter, with caches.
//!
//! The final state of a soup is split into **groups**: live cells within two neighbour steps of
//! each other (they share a neighbour), with their touching parts. Each group is translated to
//! the origin and looked up in a per-thread cache keyed by its exact cells; only groups never seen
//! before are run alone (bit-parallel, up to the longest period) and classified, as
//! `objects::find_objects` does: still life, oscillator, spaceship, a group of non-interacting
//! parts (each periodic, and their joint run is exactly the union), puffer, gun, or other (dies,
//! grows, unsettled). A periodic group is then split into the independent objects it is made of
//! ([`crate::objects::compound`]: two gliders side by side are two gliders, a pulsar beside
//! blinkers is a pulsar and blinkers). Pieces that die alone are retried together when they lie a
//! few steps apart (some oscillator phases fall apart), and failing that with their nearest
//! periodic neighbours (the far half of a bound pair). A periodic part's canonical evolution
//! gives its rule-free catalog id ([`crate::pattern::pattern_id`]), and a second cache keyed by
//! that id holds one record per distinct pattern, so a pattern's record (canonical form, rule
//! range) is built once per thread. The caches only save work: every result is a pure function of
//! the cells.

use super::bits::{Plan, Universe};
use crate::objects::compound::{self, Runner};
use crate::objects::{LCell, Phase, puffer, translated};
use crate::pattern::{
    CellMap, DiscoveredBy, PatternClass, PatternRecord, Provenance, RecordParts, build_record_with,
    canonical_symmetries,
};
use crate::rules::Rule;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

/// A small fast hasher for cells and cell lists.
#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u8(&mut self, x: u8) {
        self.write_u64(x as u64);
    }
    fn write_u32(&mut self, x: u32) {
        self.write_u64(x as u64);
    }
    fn write_i32(&mut self, x: i32) {
        self.write_u64(x as u32 as u64);
    }
    fn write_usize(&mut self, x: usize) {
        self.write_u64(x as u64);
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

/// Why a group is not a stored pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OtherKind {
    Dies,
    Grows,
    Unsettled,
}

impl OtherKind {
    pub fn name(self) -> &'static str {
        match self {
            OtherKind::Dies => "other (dies)",
            OtherKind::Grows => "other (grows)",
            OtherKind::Unsettled => "other (unsettled)",
        }
    }
}

/// A distinct pattern as the hunter knows it: its rule-free id and an unverified record.
#[derive(Debug)]
pub struct Found {
    pub id: String,
    pub class: PatternClass,
    pub period: u64,
    pub label: String,
    /// The record (canonical form, rule range, name), not verified, with an empty provenance.
    pub record: PatternRecord,
}

/// What an object is.
#[derive(Clone, Debug)]
pub enum Obj {
    Pattern(Arc<Found>),
    Other(OtherKind),
}

/// One object of a group, as run in place: its period and displacement in this orientation.
#[derive(Clone, Debug)]
pub struct Part {
    pub obj: Obj,
    pub period: u64,
    pub d: [i32; 2],
}

impl Part {
    /// A spaceship (moving periodic pattern).
    pub fn is_ship(&self) -> bool {
        matches!(&self.obj, Obj::Pattern(f) if f.class == PatternClass::Spaceship)
    }
}

/// How a group ran alone.
pub enum Run {
    Periodic {
        period: u64,
        d: [i32; 2],
        phases: Vec<Phase>,
    },
    Died,
    Grows,
    Unsettled,
}

/// Live cells within two neighbour steps of each other, with their touching parts.
#[derive(Clone, Debug)]
pub struct Group {
    /// Absolute cells, sorted.
    pub cells: Vec<LCell>,
    /// Touching parts (indices into `cells`).
    pub subs: Vec<Vec<u32>>,
    /// `[i_min, i_max, j_min, j_max]`.
    pub bbox: [i32; 4],
}

/// A group that is a spaceship on its own (gun streams).
#[derive(Clone)]
pub struct ShipEntry {
    pub found: Arc<Found>,
    pub period: u64,
    /// Displacement per period as run (this orientation).
    pub d: [i32; 2],
    /// One period of phases, starting with the group's cells at the origin.
    pub phases: Vec<Phase>,
    /// The independent ships it is made of, when it is several flying together
    /// ([`compound`]); empty when it is one ship.
    pub parts: Vec<Arc<Found>>,
}

/// Per-thread caches (pure accelerators; clearing them changes no result).
///
/// Bounded twice: by entries per map, and by an estimate of the bytes they hold (`budget`;
/// every map is cleared when it is passed). Boiling rules make huge numbers of large one-off
/// objects, whose records (phases of up to 64 periods) used to fill tens of gigabytes across
/// threads and rule groups.
#[derive(Default)]
pub struct Cache {
    groups: FastMap<Vec<LCell>, (Arc<[Part]>, Option<Arc<Env>>)>,
    pub(crate) ships: FastMap<Vec<LCell>, Option<ShipEntry>>,
    merged: FastMap<Vec<LCell>, Option<Merged>>,
    /// Two periodic groups together (their cells at the origin): do they interact?
    pairs: FastMap<Vec<LCell>, bool>,
    /// Growth clusters: the object and which of the cluster's groups (by position) it uses.
    growth: FastMap<Vec<LCell>, Option<(Part, Vec<usize>)>>,
    ids: FastMap<String, Arc<Found>>,
    pub hits: u64,
    pub misses: u64,
    /// Estimated bytes held, and the most allowed (0: [`Cache::DEFAULT_BUDGET`]).
    pub bytes: usize,
    pub budget: usize,
    /// Times every map was cleared for the byte budget.
    pub flushes: u64,
}

/// Groups run together as one: their objects, and where their cells are when periodic.
type Merged = (Arc<[Part]>, Option<Arc<Env>>);

/// Where a periodic group's cells are over one period: its period, displacement per period, and
/// each phase's bounding box (`[i_min, i_max, j_min, j_max]`, at the group's own origin).
#[derive(Clone, Debug)]
pub struct Env {
    pub period: u64,
    pub d: [i32; 2],
    pub boxes: Vec<[i32; 4]>,
}

impl Env {
    fn of(period: u64, d: [i32; 2], phases: &[Phase]) -> Env {
        Env {
            period,
            d,
            boxes: phases.iter().map(|p| bbox_of(p)).collect(),
        }
    }
    /// The bounding box at generation `g`, the group's origin at `o`.
    fn at(&self, g: u64, o: [i32; 2]) -> [i32; 4] {
        let b = self.boxes[(g % self.period) as usize];
        let n = (g / self.period) as i32;
        let (di, dj) = (o[0] + self.d[0] * n, o[1] + self.d[1] * n);
        [b[0] + di, b[1] + di, b[2] + dj, b[3] + dj]
    }
}

/// Estimated bytes of a key of `n` cells.
pub(crate) fn key_bytes(n: usize) -> usize {
    n * std::mem::size_of::<LCell>() + 64
}

/// Estimated bytes of a found pattern's record.
fn found_bytes(f: &Found) -> usize {
    f.record.phases.iter().map(|p| p.len()).sum::<usize>()
        + f.record.canonical.len()
        + f.label.len()
        + 1024
}

impl Cache {
    /// Entries kept before the group caches are cleared.
    pub const LIMIT: usize = 400_000;
    /// Bytes a cache may hold by default.
    pub const DEFAULT_BUDGET: usize = 64 << 20;

    /// A cache that holds at most about `budget` bytes.
    pub fn with_budget(budget: usize) -> Cache {
        Cache {
            budget,
            ..Default::default()
        }
    }

    pub(crate) fn add_bytes(&mut self, n: usize) {
        self.bytes += n;
    }

    pub(crate) fn trim(&mut self) {
        let budget = if self.budget == 0 {
            Self::DEFAULT_BUDGET
        } else {
            self.budget
        };
        if self.bytes > budget {
            self.groups.clear();
            self.merged.clear();
            self.pairs.clear();
            self.growth.clear();
            self.ids.clear();
            self.ships.clear();
            self.bytes = 0;
            self.flushes += 1;
            return;
        }
        if self.groups.len() > Self::LIMIT {
            self.groups.clear();
        }
        if self.merged.len() > Self::LIMIT / 4 {
            self.merged.clear();
        }
        if self.pairs.len() > Self::LIMIT / 4 {
            self.pairs.clear();
        }
        if self.growth.len() > Self::LIMIT / 16 {
            self.growth.clear();
        }
        if self.ids.len() > Self::LIMIT {
            self.ids.clear();
        }
        if self.ships.len() > Self::LIMIT / 4 {
            self.ships.clear();
        }
    }
}

/// The shared, read-only half of the census for one rule on one tiling.
pub struct Census {
    pub plan: Arc<Plan>,
    /// A small grid of the tiling (for records and symmetries).
    pub topo: Arc<Topology>,
    pub rule: Rule,
    /// The CA's notation (`B3/S23`, `B2/S34V` for edge neighbours).
    pub notation: String,
    pub nb: Nb,
    pub max_period: u64,
    /// Neighbour offsets within one and two steps, per tile type.
    n1: Vec<Vec<(i32, i32, u32)>>,
    n2: Vec<Vec<(i32, i32, u32)>>,
    /// Well-known Life names by canonical form (square grid, rules where they behave as in Life).
    names: HashMap<String, String>,
    sparse: Option<puffer::Sparse>,
    /// The tiling's symmetries as cell maps (canonical forms).
    pub syms: Vec<CellMap>,
    /// Pieces that die alone are retried together within this many steps (0 = never).
    pub piece_distance: u32,
    /// Test growing groups for guns (and puffers).
    pub growth_tests: bool,
    /// Compute each new pattern's rule range (off: ids and classes only).
    pub ranges: bool,
}

fn two_steps(offs: &[Vec<(i32, i32, u32)>]) -> Vec<Vec<(i32, i32, u32)>> {
    offs.iter()
        .enumerate()
        .map(|(k, v)| {
            let mut out: Vec<(i32, i32, u32)> = Vec::new();
            for &(a, b, k1) in v {
                out.push((a, b, k1));
                for &(c, d, k2) in &offs[k1 as usize] {
                    out.push((a + c, b + d, k2));
                }
            }
            out.retain(|&o| o != (0, 0, k as u32));
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect()
}

/// The CA notation for a rule and neighbourhood, as the world reports it.
pub fn ca_notation(rule: &Rule, nb: Nb) -> String {
    match rule {
        Rule::LifeLike { .. } | Rule::Generations { .. } if nb == Nb::Edge => {
            format!("{}V", rule.notation())
        }
        _ => rule.notation(),
    }
}

/// A small dead-edged grid of the tiling (records and symmetries only need its shape).
pub fn small_topology(tiling: &TilingSpec) -> Result<Topology, String> {
    let mut spec = crate::pattern::tiling_identity(tiling);
    spec.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    spec.boundary = Boundary::Dead;
    Topology::build(&spec)
}

impl Census {
    /// The census for `rule` (B/S or Generations without B0) on a periodic tiling.
    pub fn new(
        rule_text: &str,
        tiling: &TilingSpec,
        nb: Option<Nb>,
        max_period: u64,
    ) -> Result<Census, String> {
        let (rule, hint) = Rule::parse(rule_text)?;
        let nb = nb.or(hint).unwrap_or_default();
        let topo = Arc::new(small_topology(tiling)?);
        let syms = canonical_symmetries(&topo, nb, true);
        let mut c = Census::from_parts(rule, nb, topo, syms, max_period)?;
        c.add_names();
        Ok(c)
    }

    /// A census on a small topology and symmetries built once for many rules, without the
    /// well-known Life names.
    pub fn from_parts(
        rule: Rule,
        nb: Nb,
        topo: Arc<Topology>,
        syms: Vec<CellMap>,
        max_period: u64,
    ) -> Result<Census, String> {
        let g = topo
            .lattice()
            .ok_or("the hunter needs a periodic tiling (aperiodic patches have no lattice)")?;
        let plan = Arc::new(Plan::new(&rule, g, nb)?);
        let n1 = plan.offs.clone();
        let n2 = two_steps(&n1);
        Ok(Census {
            sparse: puffer::Sparse::new(&rule, g, nb),
            notation: ca_notation(&rule, nb),
            plan,
            topo,
            rule,
            nb,
            max_period: max_period.clamp(1, 10_000),
            n1,
            n2,
            names: HashMap::new(),
            syms,
            piece_distance: 8,
            growth_tests: true,
            ranges: true,
        })
    }

    /// Name well-known Life patterns (square grid, touching neighbours).
    fn add_names(&mut self) {
        let (topo, nb) = (self.topo.clone(), self.nb);
        let c = &*self;
        if topo.family.id == "square" && nb == Nb::Touch {
            let mut names = HashMap::new();
            for &(name, rows, period, ship) in crate::objects::KNOWN {
                let cells = crate::objects::rows_to_cells(rows);
                if let Run::Periodic {
                    period: p,
                    d,
                    phases,
                } = c.run_alone(&cells, c.max_period)
                {
                    let moves = d != [0, 0];
                    if p == period && moves == ship {
                        let class = class_of(p, d);
                        if let Ok(rec) = c.record(class, p, d, &phases) {
                            names.insert(rec.canonical, name.to_string());
                        }
                    }
                }
            }
            self.names = names;
        }
    }

    /// Run cells alone for up to `gens` generations, stopping at the first exact repeat
    /// (possibly translated). Phases start with the cells as given.
    pub fn run_alone(&self, cells: &[LCell], gens: u64) -> Run {
        let mut start: Phase = cells.to_vec();
        start.sort_unstable();
        if start.is_empty() {
            return Run::Died;
        }
        let mut u = Universe::with_cells(self.plan.clone(), &start);
        let cap = 4 * start.len() + 256;
        let mut phases = vec![start];
        for t in 1..=gens {
            u.step();
            if u.is_empty() {
                return Run::Died;
            }
            if u.population() as usize > cap {
                return Run::Grows;
            }
            let live = u.cells();
            if let Some((di, dj)) = translated(&phases[0], &live) {
                return Run::Periodic {
                    period: t,
                    d: [di, dj],
                    phases,
                };
            }
            phases.push(live);
        }
        let (a, b) = (phases[0].len(), phases.last().unwrap().len());
        if b > 2 * a + 8 {
            Run::Grows
        } else {
            Run::Unsettled
        }
    }

    /// A record (unverified, empty provenance) for one period of phases.
    pub fn record(
        &self,
        class: PatternClass,
        period: u64,
        d: [i32; 2],
        phases: &[Phase],
    ) -> Result<PatternRecord, String> {
        let what = RecordParts {
            rule: self.notation.clone(),
            nb: self.nb,
            steps_per_gen: 1,
            isotropic: true,
            class,
            period,
            d,
            name: None,
        };
        let mut rec = build_record_with(
            &self.topo,
            Some(&self.syms),
            &what,
            phases,
            Provenance::default(),
            DiscoveredBy::Search,
        )?;
        if let Some(n) = self.names.get(&rec.canonical) {
            rec.name = Some(n.clone());
        }
        Ok(rec)
    }

    pub(crate) fn found(
        &self,
        cache: &mut Cache,
        class: PatternClass,
        period: u64,
        d: [i32; 2],
        phases: &[Phase],
    ) -> Option<Arc<Found>> {
        let rec = self.record(class, period, d, phases).ok()?;
        Some(self.intern(cache, rec))
    }

    /// The cached [`Found`] for a record's id (the first record built for that id wins; its
    /// rule range is computed then).
    pub fn intern(&self, cache: &mut Cache, mut rec: PatternRecord) -> Arc<Found> {
        if let Some(f) = cache.ids.get(&rec.id) {
            return f.clone();
        }
        if rec.range.is_none() && self.ranges {
            rec.range = crate::pattern::range::rule_range(&rec).ok().flatten();
        }
        let f = Arc::new(Found {
            id: rec.id.clone(),
            class: rec.class,
            period: rec.period,
            label: rec.label(),
            record: rec,
        });
        cache.add_bytes(found_bytes(&f) + f.id.len());
        cache.ids.insert(f.id.clone(), f.clone());
        f
    }

    /// Split live cells into groups, in order of their first cell.
    pub fn groups(&self, cells: &[LCell]) -> Vec<Group> {
        let n = cells.len();
        if n == 0 {
            return vec![];
        }
        let mut at: FastMap<(i32, i32, u32), u32> =
            FastMap::with_capacity_and_hasher(n * 2, Default::default());
        for (x, c) in cells.iter().enumerate() {
            at.insert((c.i, c.j, c.k), x as u32);
        }
        let mut near = Uf::new(n);
        let mut touch = Uf::new(n);
        for (x, c) in cells.iter().enumerate() {
            for &(di, dj, k) in &self.n2[c.k as usize] {
                if let Some(&y) = at.get(&(c.i + di, c.j + dj, k)) {
                    near.union(x, y as usize);
                }
            }
            for &(di, dj, k) in &self.n1[c.k as usize] {
                if let Some(&y) = at.get(&(c.i + di, c.j + dj, k)) {
                    touch.union(x, y as usize);
                }
            }
        }
        let mut gid: FastMap<usize, usize> = FastMap::default();
        let mut members: Vec<Vec<usize>> = Vec::new();
        for x in 0..n {
            let r = near.find(x);
            let g = *gid.entry(r).or_insert_with(|| {
                members.push(vec![]);
                members.len() - 1
            });
            members[g].push(x);
        }
        members
            .into_iter()
            .map(|m| {
                let mut subs_of: FastMap<usize, usize> = FastMap::default();
                let mut subs: Vec<Vec<u32>> = Vec::new();
                for (local, &x) in m.iter().enumerate() {
                    let r = touch.find(x);
                    let s = *subs_of.entry(r).or_insert_with(|| {
                        subs.push(vec![]);
                        subs.len() - 1
                    });
                    subs[s].push(local as u32);
                }
                let gc: Vec<LCell> = m.iter().map(|&x| cells[x]).collect();
                Group {
                    bbox: bbox_of(&gc),
                    cells: gc,
                    subs,
                }
            })
            .collect()
    }

    /// The objects of one group (cached by its cells, translated to the origin).
    pub fn analyse(&self, cache: &mut Cache, g: &Group) -> Arc<[Part]> {
        self.analyse_env(cache, g).0
    }

    /// [`Census::analyse`], with where the group's cells are over one period when they are all
    /// periodic (at the group's origin, its minimum `i` and `j`).
    pub fn analyse_env(&self, cache: &mut Cache, g: &Group) -> (Arc<[Part]>, Option<Arc<Env>>) {
        let (key, _) = normalize(&g.cells);
        if let Some(p) = cache.groups.get(&key) {
            cache.hits += 1;
            return p.clone();
        }
        cache.misses += 1;
        let subs: Vec<Vec<LCell>> = g
            .subs
            .iter()
            .map(|s| {
                let mut v: Vec<LCell> = s.iter().map(|&m| key[m as usize]).collect();
                v.sort_unstable();
                v
            })
            .collect();
        let (parts, env) = self.analyse_cells(cache, &key, &subs);
        let parts: Arc<[Part]> = parts.into();
        let env = env.map(Arc::new);
        cache.trim();
        cache.add_bytes(
            key_bytes(key.len())
                + parts.len() * 48
                + env.as_ref().map_or(0, |e| e.boxes.len() * 16),
        );
        cache.groups.insert(key, (parts.clone(), env.clone()));
        (parts, env)
    }

    /// The independent objects of one period of a periodic run ([`compound`]); the run as one
    /// object when it does not come apart.
    pub fn periodic_parts(
        &self,
        cache: &mut Cache,
        period: u64,
        d: [i32; 2],
        phases: &[Phase],
    ) -> Vec<Part> {
        match compound::split(self, phases, d) {
            Some(ps) => ps
                .into_iter()
                .map(|cp| self.part(cache, cp.period, cp.d, &cp.phases))
                .collect(),
            None => vec![self.part(cache, period, d, phases)],
        }
    }

    fn analyse_cells(
        &self,
        cache: &mut Cache,
        cells: &[LCell],
        subs: &[Vec<LCell>],
    ) -> (Vec<Part>, Option<Env>) {
        if subs.len() > 1 {
            // Parts that provably do not interact: each periodic and stationary, and the joint
            // run is exactly the union of the parts' runs over a common period.
            let mut runs = Vec::new();
            let mut l = 1u64;
            let mut ok = true;
            for s in subs {
                match self.run_alone(s, self.max_period) {
                    Run::Periodic { period, d, phases } if d == [0, 0] => {
                        l = l / gcd(l, period) * period;
                        runs.push((period, d, phases));
                        if l > self.max_period {
                            ok = false;
                            break;
                        }
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && self.joint_is_union(cells, &runs, l) {
                // Where the whole group is: the union of its parts, phase by phase.
                let boxes = (0..l as usize)
                    .map(|t| {
                        let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
                        for (_, _, ph) in &runs {
                            let x = bbox_of(&ph[t % ph.len()]);
                            b = [
                                b[0].min(x[0]),
                                b[1].max(x[1]),
                                b[2].min(x[2]),
                                b[3].max(x[3]),
                            ];
                        }
                        b
                    })
                    .collect();
                let env = Env {
                    period: l,
                    d: [0, 0],
                    boxes,
                };
                let parts = runs
                    .into_iter()
                    .flat_map(|(period, d, phases)| self.periodic_parts(cache, period, d, &phases))
                    .collect();
                return (parts, Some(env));
            }
        }
        self.whole_env(cache, cells)
    }

    fn joint_is_union(
        &self,
        cells: &[LCell],
        runs: &[(u64, [i32; 2], Vec<Phase>)],
        l: u64,
    ) -> bool {
        let mut u = Universe::with_cells(self.plan.clone(), cells);
        for t in 1..=l {
            u.step();
            let mut want: Phase = runs
                .iter()
                .flat_map(|(_, _, ph)| ph[(t as usize) % ph.len()].iter().copied())
                .collect();
            want.sort_unstable();
            if u.cells() != want {
                return false;
            }
        }
        true
    }

    fn part(&self, cache: &mut Cache, period: u64, d: [i32; 2], phases: &[Phase]) -> Part {
        let obj = match self.found(cache, class_of(period, d), period, d, phases) {
            Some(f) => Obj::Pattern(f),
            None => Obj::Other(OtherKind::Unsettled),
        };
        Part { obj, period, d }
    }

    /// The objects of cells run as one group (its independent parts if it is periodic, else one
    /// puffer, gun, or other), with where the cells are over one period when they are periodic.
    fn whole_env(&self, cache: &mut Cache, cells: &[LCell]) -> (Vec<Part>, Option<Env>) {
        match self.run_alone(cells, self.max_period) {
            Run::Periodic { period, d, phases } => {
                let env = Env::of(period, d, &phases);
                (self.periodic_parts(cache, period, d, &phases), Some(env))
            }
            Run::Died => (
                vec![Part {
                    obj: Obj::Other(OtherKind::Dies),
                    period: 0,
                    d: [0, 0],
                }],
                None,
            ),
            r => {
                if self.growth_tests
                    && let Some(p) = self.growth(cache, cells)
                {
                    return (vec![p], None);
                }
                (
                    vec![Part {
                        obj: Obj::Other(if matches!(r, Run::Grows) {
                            OtherKind::Grows
                        } else {
                            OtherKind::Unsettled
                        }),
                        period: 0,
                        d: [0, 0],
                    }],
                    None,
                )
            }
        }
    }

    /// Puffer and gun tests for a group that neither settles nor dies.
    fn growth(&self, cache: &mut Cache, cells: &[LCell]) -> Option<Part> {
        let sp = self.sparse.as_ref()?;
        if let Some(f) = puffer::detect(sp, cells, self.max_period, puffer::TEST_CAP) {
            let rec = self
                .record(PatternClass::Puffer, f.period, f.d, &[cells.to_vec()])
                .ok()?;
            return Some(Part {
                obj: Obj::Pattern(self.intern(cache, rec)),
                period: f.period,
                d: f.d,
            });
        }
        if let Some(gun) = super::gun::detect(self, cache, cells) {
            return Some(Part {
                period: gun.period,
                d: [0, 0],
                obj: Obj::Pattern(gun.found),
            });
        }
        None
    }

    /// Every object in a set of live cells: each group's parts, with pieces that die alone
    /// retried together when they lie close.
    pub fn census(&self, cache: &mut Cache, cells: &[LCell]) -> Vec<Part> {
        let groups = self.groups(cells);
        let n = groups.len();
        let (parts, envs): (Vec<Arc<[Part]>>, Vec<Option<Arc<Env>>>) =
            groups.iter().map(|g| self.analyse_env(cache, g)).unzip();
        let mut absorbed = vec![false; n];
        let mut at_head: FastMap<usize, Arc<[Part]>> = FastMap::default();
        // Periodic merges of step 1: their groups, cells, and where they are.
        let mut pieces: Vec<(Vec<usize>, Vec<LCell>, Arc<Env>)> = Vec::new();
        let is_other = |g: usize| parts[g].len() == 1 && matches!(parts[g][0].obj, Obj::Other(_));
        // 1. Pieces that die alone, retried together, and failing that with their nearest
        //    periodic neighbours (the far half of a bound pair).
        let loose: Vec<usize> = (0..n).filter(|&g| is_other(g)).collect();
        if !loose.is_empty() && self.piece_distance > 0 {
            let periodic: Vec<bool> = (0..n)
                .map(|g| {
                    parts[g].iter().all(|p| {
                        matches!(&p.obj, Obj::Pattern(f) if matches!(
                            f.class,
                            PatternClass::StillLife | PatternClass::Oscillator | PatternClass::Spaceship
                        ))
                    })
                })
                .collect();
            let mut taken = vec![false; n];
            for (c, cand) in self.close_clusters(&groups, &loose, &periodic) {
                let size: usize = c.iter().map(|&g| groups[g].cells.len()).sum();
                let mut tries: Vec<Vec<usize>> = Vec::new();
                if c.len() > 1 {
                    tries.push(c.clone());
                }
                if size <= LOOSE_CELLS {
                    for &p in &cand {
                        tries.push(c.iter().copied().chain([p]).collect());
                    }
                    if cand.len() > 1 {
                        tries.push(c.iter().copied().chain(cand.iter().copied()).collect());
                    }
                }
                for mut set in tries {
                    set.sort_unstable();
                    if set.iter().any(|&g| taken[g]) {
                        continue;
                    }
                    let cells = cells_of(&groups, &set);
                    if let Some((p, env)) = self.merge_run(cache, &cells) {
                        for &g in &set {
                            absorbed[g] = true;
                            taken[g] = true;
                        }
                        at_head.insert(set[0], p);
                        if let Some(e) = env {
                            pieces.push((set, cells, e));
                        }
                        break;
                    }
                }
            }
        }
        // 2. Growing groups with everything near them (a gun's shuttles need its blocks).
        if self.growth_tests {
            let gap = 4 * self.plan.reach_i.max(self.plan.reach_j) + 8;
            let near = |a: [i32; 4], b: [i32; 4]| {
                a[0] <= b[1] + gap && b[0] <= a[1] + gap && a[2] <= b[3] + gap && b[2] <= a[3] + gap
            };
            for s in 0..n {
                let growing = matches!(
                    &parts[s][..],
                    [Part {
                        obj: Obj::Other(OtherKind::Grows | OtherKind::Unsettled),
                        ..
                    }]
                );
                if absorbed[s] || !growing {
                    continue;
                }
                let mut members = vec![s];
                let mut bb = groups[s].bbox;
                loop {
                    let mut added = false;
                    for g in 0..n {
                        if !absorbed[g] && !members.contains(&g) && near(groups[g].bbox, bb) {
                            members.push(g);
                            let o = groups[g].bbox;
                            bb = [
                                bb[0].min(o[0]),
                                bb[1].max(o[1]),
                                bb[2].min(o[2]),
                                bb[3].max(o[3]),
                            ];
                            added = true;
                        }
                    }
                    if !added || members.len() > 64 {
                        break;
                    }
                }
                if members.len() < 2 || members.len() > 64 {
                    continue;
                }
                members.sort_unstable();
                if let Some((p, used)) = self.growth_cluster(cache, &groups, &parts, &members) {
                    for &g in &used {
                        absorbed[g] = true;
                    }
                    at_head.insert(used[0], Arc::from(vec![p]));
                }
            }
        }
        // 3. Periodic objects (groups, and the merges of step 1) that interact with a neighbour
        //    (the halves of a bound pair, apart in this phase), run together: one object when
        //    together they are periodic.
        let mut units: Vec<(Vec<usize>, Vec<LCell>, Arc<Env>)> = (0..n)
            .filter(|&g| !absorbed[g])
            .filter_map(|g| {
                envs[g]
                    .clone()
                    .map(|e| (vec![g], groups[g].cells.clone(), e))
            })
            .collect();
        units.extend(pieces);
        for set in self.interacting(cache, &units) {
            let mut members: Vec<usize> = set.iter().flat_map(|&u| units[u].0.clone()).collect();
            members.sort_unstable();
            let cells = cells_of(&groups, &members);
            if let Some((p, _)) = self.merge_run(cache, &cells) {
                for &u in &set {
                    at_head.remove(&units[u].0[0]);
                }
                for &g in &members {
                    absorbed[g] = true;
                }
                at_head.insert(members[0], p);
            }
        }
        let mut out = Vec::new();
        for (g, p) in parts.iter().enumerate() {
            if let Some(m) = at_head.get(&g) {
                out.extend(m.iter().cloned());
            }
            if !absorbed[g] {
                out.extend(p.iter().cloned());
            }
        }
        out
    }

    /// Groups run together as one (`cells` sorted, absolute): their objects and where their
    /// cells are, when every object is a pattern (cached by the cells at the origin).
    fn merge_run(&self, cache: &mut Cache, cells: &[LCell]) -> Option<Merged> {
        let (key, _) = normalize(cells);
        if let Some(x) = cache.merged.get(&key) {
            return x.clone();
        }
        let (ps, env) = self.whole_env(cache, &key);
        let v = ps
            .iter()
            .all(|p| !matches!(p.obj, Obj::Other(_)))
            .then(|| (Arc::from(ps), env.map(Arc::new)));
        cache.add_bytes(key_bytes(key.len()) + 48);
        cache.merged.insert(key, v.clone());
        v
    }

    /// Sets of periodic objects (each its groups, its cells sorted, and where it is at its own
    /// origin, its minimum `i` and `j`) that interact, as indices in order: two interact when
    /// running them together differs, at some generation of two common periods and eight more,
    /// from the union of their runs. Pairs whose envelopes never come within two neighbour steps
    /// of each other cannot interact and are not run; the answer for a pair's cells is cached.
    fn interacting(
        &self,
        cache: &mut Cache,
        units: &[(Vec<usize>, Vec<LCell>, Arc<Env>)],
    ) -> Vec<Vec<usize>> {
        let n = units.len();
        if n < 2 {
            return vec![];
        }
        let live: Vec<usize> = (0..n).collect();
        let (ri, rj) = (2 * self.plan.reach_i, 2 * self.plan.reach_j);
        let origins: Vec<[i32; 2]> = units
            .iter()
            .map(|u| {
                let b = bbox_of(&u.1);
                [b[0], b[2]]
            })
            .collect();
        let origin = |g: usize| origins[g];
        let h = compound::horizon(self.max_period);
        // Every place an object is over the longest test.
        let all: Vec<[i32; 4]> = (0..n)
            .map(|g| {
                let e = &units[g].2;
                let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
                for t in [0, h] {
                    for k in 0..e.period {
                        let x = e.at(t / e.period * e.period + k, origin(g));
                        b = [
                            b[0].min(x[0]),
                            b[1].max(x[1]),
                            b[2].min(x[2]),
                            b[3].max(x[3]),
                        ];
                    }
                }
                b
            })
            .collect();
        let near = |a: [i32; 4], b: [i32; 4]| {
            a[0] - b[1] <= ri && b[0] - a[1] <= ri && a[2] - b[3] <= rj && b[2] - a[3] <= rj
        };
        let mut order = live.clone();
        order.sort_by_key(|&g| (all[g][0], g));
        let mut uf = Uf::new(n);
        let mut any = false;
        for (x, &a) in order.iter().enumerate() {
            for &b in &order[x + 1..] {
                if all[b][0] - all[a][1] > ri {
                    break;
                }
                if !near(all[a], all[b]) {
                    continue;
                }
                let (ea, eb) = (&units[a].2, &units[b].2);
                let l = ea.period / gcd(ea.period, eb.period) * eb.period;
                if l > self.max_period {
                    continue;
                }
                let lh = compound::horizon(l);
                let (oa, ob) = (origin(a), origin(b));
                if !(0..=lh).any(|t| near(ea.at(t, oa), eb.at(t, ob))) {
                    continue;
                }
                let mut cells: Vec<LCell> = units[a].1.clone();
                cells.extend(units[b].1.iter().copied());
                cells.sort_unstable();
                let (key, _) = normalize(&cells);
                let inter = match cache.pairs.get(&key) {
                    Some(&x) => x,
                    None => {
                        let x = match (
                            self.run(&units[a].1, lh),
                            self.run(&units[b].1, lh),
                            self.run(&cells, lh),
                        ) {
                            (Some(ra), Some(rb), Some(j)) => (0..=lh as usize)
                                .any(|t| !compound::union_is(&ra[t], &rb[t], &j[t])),
                            _ => true,
                        };
                        cache.add_bytes(key_bytes(key.len()) + 8);
                        cache.pairs.insert(key, x);
                        x
                    }
                };
                if inter {
                    uf.union(a, b);
                    any = true;
                }
            }
        }
        if !any {
            return vec![];
        }
        let mut at: FastMap<usize, usize> = FastMap::default();
        let mut out: Vec<Vec<usize>> = Vec::new();
        for &g in &live {
            let r = uf.find(g);
            let c = *at.entry(r).or_insert_with(|| {
                out.push(vec![]);
                out.len() - 1
            });
            out[c].push(g);
        }
        out.retain(|s| s.len() > 1);
        for s in &mut out {
            s.sort_unstable();
        }
        out.sort();
        out
    }

    /// Puffer and gun tests on a cluster of groups; a gun is then trimmed of the periodic groups
    /// it does not need. Returns the object and the groups it is made of.
    fn growth_cluster(
        &self,
        cache: &mut Cache,
        groups: &[Group],
        parts: &[Arc<[Part]>],
        members: &[usize],
    ) -> Option<(Part, Vec<usize>)> {
        let cells_of = |m: &[usize]| -> Vec<LCell> {
            let mut v: Vec<LCell> = m
                .iter()
                .flat_map(|&g| groups[g].cells.iter().copied())
                .collect();
            v.sort_unstable();
            normalize(&v).0
        };
        let key = cells_of(members);
        if key.len() > puffer::TEST_CAP {
            return None;
        }
        let got = match cache.growth.get(&key) {
            Some(x) => x.clone(),
            None => {
                let mut res: Option<(Part, Vec<usize>)> = None;
                if let Some(p) = self.growth(cache, &key) {
                    let mut used: Vec<usize> = (0..members.len()).collect();
                    if let Obj::Pattern(f) = &p.obj
                        && f.class == PatternClass::Gun
                    {
                        let ship = f.record.gun.as_ref().map(|g| g.ship.clone());
                        let mut best = p.clone();
                        for x in 0..members.len() {
                            let periodic = parts[members[x]]
                                .iter()
                                .all(|q| matches!(q.obj, Obj::Pattern(_)));
                            if !periodic || used.len() < 2 {
                                continue;
                            }
                            let trial: Vec<usize> =
                                used.iter().copied().filter(|&y| y != x).collect();
                            let m: Vec<usize> = trial.iter().map(|&y| members[y]).collect();
                            if let Some(g) = super::gun::detect(self, cache, &cells_of(&m))
                                && g.period == p.period
                                && Some(&g.ship.id) == ship.as_ref()
                            {
                                used = trial;
                                best = Part {
                                    obj: Obj::Pattern(g.found),
                                    period: g.period,
                                    d: [0, 0],
                                };
                            }
                        }
                        res = Some((best, used));
                    } else {
                        res = Some((p, used));
                    }
                }
                cache.add_bytes(key_bytes(key.len()) + 128);
                cache.growth.insert(key, res.clone());
                res
            }
        };
        got.map(|(p, used)| (p, used.into_iter().map(|x| members[x]).collect()))
    }

    /// Loose groups within `piece_distance` neighbour steps of each other, clustered (each
    /// cluster in group order), each with the nearest periodic groups within that distance (at
    /// most three, nearest first).
    fn close_clusters(
        &self,
        groups: &[Group],
        loose: &[usize],
        periodic: &[bool],
    ) -> Vec<(Vec<usize>, Vec<usize>)> {
        // Owners: loose groups by their index in `loose`, periodic ones by group.
        let mut owner: FastMap<(i32, i32, u32), Result<usize, usize>> = FastMap::default();
        for (x, &g) in loose.iter().enumerate() {
            for c in &groups[g].cells {
                owner.insert((c.i, c.j, c.k), Ok(x));
            }
        }
        for (g, grp) in groups.iter().enumerate().filter(|(g, _)| periodic[*g]) {
            for c in &grp.cells {
                owner.insert((c.i, c.j, c.k), Err(g));
            }
        }
        let mut uf = Uf::new(loose.len());
        let mut near: Vec<Vec<(u32, usize)>> = vec![Vec::new(); loose.len()];
        for (x, &g) in loose.iter().enumerate() {
            let mut seen: FastMap<(i32, i32, u32), ()> = groups[g]
                .cells
                .iter()
                .map(|c| ((c.i, c.j, c.k), ()))
                .collect();
            let mut frontier: Vec<(i32, i32, u32)> = seen.keys().copied().collect();
            frontier.sort_unstable();
            for step in 1..=self.piece_distance {
                let mut next = Vec::new();
                for &(i, j, k) in &frontier {
                    for &(di, dj, k2) in &self.n1[k as usize] {
                        let y = (i + di, j + dj, k2);
                        if seen.insert(y, ()).is_none() {
                            next.push(y);
                            match owner.get(&y) {
                                Some(Ok(o)) => uf.union(x, *o),
                                Some(Err(p)) if !near[x].iter().any(|n| n.1 == *p) => {
                                    near[x].push((step, *p))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                frontier = next;
            }
        }
        let mut at: FastMap<usize, usize> = FastMap::default();
        let mut out: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
        let mut cand: Vec<Vec<(u32, usize)>> = Vec::new();
        for (x, &g) in loose.iter().enumerate() {
            let r = uf.find(x);
            let c = *at.entry(r).or_insert_with(|| {
                out.push((vec![], vec![]));
                cand.push(vec![]);
                out.len() - 1
            });
            out[c].0.push(g);
            cand[c].extend(near[x].iter().copied());
        }
        for (o, mut c) in out.iter_mut().zip(cand) {
            c.sort_unstable();
            let mut seen = Vec::new();
            for (_, p) in c {
                if !seen.contains(&p) && seen.len() < 3 {
                    seen.push(p);
                }
            }
            o.1 = seen;
        }
        out
    }
}

/// Live cells in a loose cluster above which it is not tried with its periodic neighbours.
const LOOSE_CELLS: usize = 256;

/// A [`Runner`] on the unbounded lattice with the bit-parallel stepper (B/S and Generations
/// rules), for splitting compounds.
pub struct BitRunner(pub Arc<Plan>);

impl Runner for BitRunner {
    fn run(&self, cells: &[LCell], gens: u64) -> Option<Vec<Phase>> {
        let mut start: Phase = cells.to_vec();
        start.sort_unstable();
        let cap = 4 * start.len() + 256;
        let mut u = Universe::with_cells(self.0.clone(), &start);
        let mut out = Vec::with_capacity(gens as usize + 1);
        out.push(start);
        for _ in 0..gens {
            if u.is_empty() {
                out.push(Vec::new());
                continue;
            }
            u.step();
            if u.population() as usize > cap {
                return None;
            }
            out.push(u.cells());
        }
        Some(out)
    }
    fn touching(&self, c: &LCell, out: &mut Vec<(i32, i32, u32)>) {
        for &(di, dj, k) in &self.0.offs[c.k as usize] {
            out.push((c.i + di, c.j + dj, k));
        }
    }
    fn close(&self, a: &[LCell], b: &[LCell]) -> bool {
        compound::lattice_close(a, b, (self.0.reach_i, self.0.reach_j))
    }
    fn distance(&self, a: &[LCell], b: &[LCell]) -> f64 {
        compound::lattice_distance(a, b)
    }
}

impl Runner for Census {
    fn run(&self, cells: &[LCell], gens: u64) -> Option<Vec<Phase>> {
        BitRunner(self.plan.clone()).run(cells, gens)
    }
    fn touching(&self, c: &LCell, out: &mut Vec<(i32, i32, u32)>) {
        for &(di, dj, k) in &self.n1[c.k as usize] {
            out.push((c.i + di, c.j + dj, k));
        }
    }
    fn close(&self, a: &[LCell], b: &[LCell]) -> bool {
        compound::lattice_close(a, b, (self.plan.reach_i, self.plan.reach_j))
    }
    fn distance(&self, a: &[LCell], b: &[LCell]) -> f64 {
        compound::lattice_distance(a, b)
    }
}

/// The cells of some groups together, sorted.
fn cells_of(groups: &[Group], set: &[usize]) -> Vec<LCell> {
    let mut v: Vec<LCell> = set
        .iter()
        .flat_map(|&g| groups[g].cells.iter().copied())
        .collect();
    v.sort_unstable();
    v
}

/// Still life, oscillator, or spaceship from a period and displacement.
pub fn class_of(period: u64, d: [i32; 2]) -> PatternClass {
    if d != [0, 0] {
        PatternClass::Spaceship
    } else if period == 1 {
        PatternClass::StillLife
    } else {
        PatternClass::Oscillator
    }
}

/// Cells translated so the minimum `i` and `j` are 0 (sorted), and the shift taken off.
pub fn normalize(cells: &[LCell]) -> (Vec<LCell>, [i32; 2]) {
    let mi = cells.iter().map(|c| c.i).min().unwrap_or(0);
    let mj = cells.iter().map(|c| c.j).min().unwrap_or(0);
    let mut v: Vec<LCell> = cells
        .iter()
        .map(|c| LCell {
            i: c.i - mi,
            j: c.j - mj,
            ..*c
        })
        .collect();
    v.sort_unstable();
    (v, [mi, mj])
}

/// `[i_min, i_max, j_min, j_max]` of a non-empty cell list.
pub fn bbox_of(cells: &[LCell]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
    for c in cells {
        b[0] = b[0].min(c.i);
        b[1] = b[1].max(c.i);
        b[2] = b[2].min(c.j);
        b[3] = b[3].max(c.j);
    }
    b
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

struct Uf(Vec<u32>);
impl Uf {
    fn new(n: usize) -> Uf {
        Uf((0..n as u32).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] as usize != x {
            let p = self.0[x] as usize;
            self.0[x] = self.0[p];
            x = p;
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            self.0[hi] = lo as u32;
        }
    }
}
