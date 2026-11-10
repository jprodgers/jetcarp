//! Following a mover on a sliding window: the pattern runs on the tiles around its live cells
//! ([`super::Hier::build`]), and when a live cell comes within the edge margin of the window's
//! open tiles, the window is rebuilt around the live cells (tiles ahead added, tiles behind
//! dropped) and the states carried over by exact placement.
//!
//! Semantics are those of the whole infinite tiling: cells outside the window are dead, and
//! before every step each live cell is at least the edge margin (3 neighbour steps, more for
//! several steps per generation) from every open tile, so every cell that can change, and every
//! neighbour it reads, is in the window with its full neighbourhood. The rule must keep a dead
//! neighbourhood dead (checked on the first window; B0 rules are refused).
//!
//! Distances: rings and neighbour steps are breadth-first distances from the starting tiles
//! through the tiles the windows held (each new tile takes its distance from the tiles already
//! labelled, and labels only ever decrease), an upper bound on the distance through the whole
//! tiling and equal to it on the fixed-patch comparisons; world units are exact up to floats.

use super::{Disc, Frame, Hier, Key, WindowPatch};
use crate::exact::{FxMap, Place};
use crate::geom::{P, dist};
use crate::library::movers::{
    LevelRun, MoverReport, Reach, Stop, TrackSpec, WindowStats, groups_around,
};
use crate::library::now_utc;
use crate::rules::FieldKind;
use crate::topology::{Nb, SizeSpec, Topology};
use crate::world::{CaInstance, CaSpec, Command, Field, SeedSpec, World};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

/// How the window follows the pattern (none of this changes the run's cells, only its cost
/// and, through the corridor, at most the ring count).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowOpts {
    /// World units of tiles kept around the live cells.
    #[serde(default = "default_margin")]
    pub margin: f64,
    /// Spectre: follow the fixed patches' Δ chain up to this level (so the window holds the
    /// tiles of the level patches there), then choose ancestors that keep the window deep.
    #[serde(default = "default_fixed")]
    pub fixed_levels: u32,
    /// Stop (verified) when the nearest live tile is this many edge rings from the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_rings: Option<u32>,
}

fn default_margin() -> f64 {
    24.0
}
fn default_fixed() -> u32 {
    8
}

impl Default for WindowOpts {
    fn default() -> Self {
        WindowOpts {
            margin: default_margin(),
            fixed_levels: default_fixed(),
            target_rings: None,
        }
    }
}

/// Generations whose states are kept to recognise an exact repeat in place (an oscillator).
pub const REPEAT_GENS: u64 = 4096;

/// What a resumed run needs, saved right after a window is built (so the resumed run builds
/// the same window and continues exactly as an uninterrupted one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowCheckpoint {
    pub frame: Frame,
    pub discs: Vec<Disc>,
    pub live: Vec<(Key, u8)>,
    /// Ring and step labels of the window's tiles.
    pub labels: Vec<(Key, u32, u32)>,
    pub generation: u64,
    pub min_population: usize,
    pub max_population: usize,
    pub recentres: u64,
    pub max_tiles: usize,
    pub seconds: f64,
    /// Starting centroid (world units).
    pub c0: P,
}

/// Live cells by placement and state, sorted.
type Snapshot = Vec<(Key, u8)>;

/// A mover running on a sliding window.
pub struct Tracker {
    pub spec: TrackSpec,
    pub opts: WindowOpts,
    hier: Hier,
    win: WindowPatch,
    /// The rule (compiled once) and the window's cells.
    ca: CaInstance,
    kind: FieldKind,
    field: Field,
    /// Windows are small: one thread runs them without waking a pool.
    pool: Arc<rayon::ThreadPool>,
    /// Live cells (window tiles) and states, in tile order.
    live: Vec<(u32, u8)>,
    ring: Vec<u32>,
    step: Vec<u32>,
    open_dist: Vec<u32>,
    margin_steps: u32,
    generation: u64,
    pmin: usize,
    pmax: usize,
    c0: P,
    recentres: u64,
    max_tiles: usize,
    seconds_before: f64,
    session: Instant,
    history: FxMap<u64, Vec<(u64, Snapshot)>>,
    stop: Option<Stop>,
    period: Option<u64>,
    just_built: bool,
    /// Seconds spent building windows in this session.
    build_secs: f64,
    /// Why the window could not be built further (the run stops at `Edge`).
    failure: Option<String>,
}

/// BFS distances from `sources` over `nb`.
fn bfs(topo: &Topology, nb: Nb, sources: impl Iterator<Item = usize>) -> Vec<u32> {
    let mut d = vec![u32::MAX; topo.len()];
    let mut q = VecDeque::new();
    for s in sources {
        if d[s] == u32::MAX {
            d[s] = 0;
            q.push_back(s);
        }
    }
    while let Some(x) = q.pop_front() {
        let dx = d[x];
        topo.for_each_neighbor(x, nb, |y| {
            if d[y] == u32::MAX {
                d[y] = dx + 1;
                q.push_back(y);
            }
        });
    }
    d
}

/// Lower every label to the shortest path from the labelled tiles (unit steps over `nb`).
fn relax(topo: &Topology, nb: Nb, labels: &mut [u32]) {
    let mut src: Vec<u32> = (0..labels.len() as u32)
        .filter(|&i| labels[i as usize] != u32::MAX)
        .collect();
    src.sort_by_key(|&i| (labels[i as usize], i));
    let mut q: VecDeque<(u32, u32)> = VecDeque::new();
    let mut si = 0;
    loop {
        let from_src = match (q.front(), src.get(si)) {
            (None, None) => break,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (Some(&(d, _)), Some(&s)) => labels[s as usize] < d,
        };
        let (d, x) = if from_src {
            let s = src[si];
            si += 1;
            (labels[s as usize], s)
        } else {
            q.pop_front().unwrap()
        };
        if d > labels[x as usize] {
            continue;
        }
        topo.for_each_neighbor(x as usize, nb, |y| {
            if d + 1 < labels[y] {
                labels[y] = d + 1;
                q.push_back((d + 1, y as u32));
            }
        });
    }
}

/// Discs around groups of live cells (exact-coordinate float centres): cells closer than twice
/// the margin share a disc.
fn discs_for(centres: &[P], margin: f64) -> Vec<Disc> {
    let n = centres.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for i in 0..n {
        for j in i + 1..n {
            if dist(centres[i], centres[j]) <= 2.0 * margin {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut groups: Vec<(usize, Vec<P>)> = Vec::new();
    for (i, &c) in centres.iter().enumerate() {
        let r = find(&mut parent, i);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some(g) => g.1.push(c),
            None => groups.push((r, vec![c])),
        }
    }
    groups
        .into_iter()
        .map(|(_, ps)| {
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for p in &ps {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let r = ps.iter().map(|&p| dist(p, c)).fold(0.0, f64::max);
            Disc { c, r: r + margin }
        })
        .collect()
}

fn check_rule(spec: &TrackSpec) -> Result<(), String> {
    use crate::rules::Rule;
    let (rule, _) = Rule::parse(&spec.rule)?;
    match rule {
        Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } if birth & 1 == 1 => {
            Err("B0 rules turn the whole plane on, so they cannot run on a window".into())
        }
        Rule::Turmite(_)
        | Rule::Sandpile
        | Rule::Lenia(_)
        | Rule::Ltl(_)
        | Rule::Layered { .. } => Err(format!(
            "{} cannot run on a sliding window (only local rules whose dead cells stay dead)",
            spec.rule
        )),
        _ => Ok(()),
    }
}

impl Tracker {
    /// Start a run from the spec's cells on its patch.
    pub fn new(spec: &TrackSpec, opts: &WindowOpts) -> Result<Tracker, String> {
        let pool = one_thread()?;
        pool.clone().install(|| Tracker::new_in(pool, spec, opts))
    }

    fn new_in(
        pool: Arc<rayon::ThreadPool>,
        spec: &TrackSpec,
        opts: &WindowOpts,
    ) -> Result<Tracker, String> {
        let family = spec.patch.family.clone();
        check_rule(spec)?;
        if spec.cells.is_empty() {
            return Err("no starting cells".into());
        }
        if !(opts.margin.is_finite() && opts.margin >= 8.0) {
            return Err("the window margin must be at least 8 units".into());
        }
        let ident = crate::library::tiling_identity(&spec.patch);
        let l0 = match spec.patch.size {
            Some(SizeSpec::Level(l)) => l,
            _ => crate::aperiodic::default_level(&family),
        };
        let topo0 = crate::library::site::patch_topology(&ident.with_size(SizeSpec::Level(l0)))?;
        let mut cells0 = spec.cells.clone();
        cells0.sort_unstable();
        cells0.dedup_by_key(|c| c.0);
        if let Some(&(x, _)) = cells0.iter().find(|c| c.0 as usize >= topo0.len()) {
            return Err(format!("no tile {x} on level {l0}"));
        }
        let mut hier = Hier::new(&family, l0, opts.fixed_levels)?;
        let world_c: Vec<P> = cells0
            .iter()
            .map(|&(x, _)| topo0.centroid(x as usize))
            .collect();
        let n = world_c.len() as f64;
        let c0 = world_c
            .iter()
            .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
        let exact_c: Vec<P> = world_c.iter().map(|&p| hier.from_world(p)).collect();
        let discs = discs_for(&exact_c, opts.margin);
        let win = hier.build(&discs)?;
        // The start tiles on the window: same shape, same vertex average.
        let mut live = Vec::new();
        for (&(x, s), &c) in cells0.iter().zip(&exact_c) {
            let shape = topo0.shape_of(x as usize) as u16;
            let t = (0..win.len())
                .find(|&i| win.keys[i].0 == shape && dist(win.centres[i], c) < 1e-6)
                .ok_or_else(|| format!("start tile {x} has no tile on the window"))?;
            let a = topo0.polygon(x as usize);
            let b = win.topo.polygon(t);
            if !crate::evolve::arena::same_polygon(&a, &b) {
                return Err(format!(
                    "start tile {x} does not match the window's tile (the tilings differ)"
                ));
            }
            live.push((t as u32, s));
        }
        live.sort_unstable();
        let mut t = Tracker::assemble(pool, spec, opts, hier, win, live, c0)?;
        let starts: Vec<usize> = t.live.iter().map(|c| c.0 as usize).collect();
        t.ring = bfs(&t.win.topo, Nb::Edge, starts.iter().copied());
        t.step = bfs(&t.win.topo, spec.nb, starts.iter().copied());
        // A dead window must stay dead.
        {
            let mut w = World::on_topology(t.win.topo.clone(), 1);
            let ca = CaSpec::new(&spec.rule)
                .nb(spec.nb)
                .steps(spec.steps_per_gen)
                .seed(SeedSpec::Empty);
            w.apply(Command::AddCa { ca })?;
            w.step();
            if w.fields[w.cas[0].field].state.iter().any(|&s| s != 0) {
                return Err(format!(
                    "{} brings dead cells to life with no live neighbour, so it cannot run on a window",
                    spec.rule
                ));
            }
        }
        if t.live
            .iter()
            .any(|c| t.open_dist[c.0 as usize] < t.margin_steps)
        {
            return Err("the window is too small for the pattern: raise the margin".into());
        }
        t.record_history();
        Ok(t)
    }

    fn assemble(
        pool: Arc<rayon::ThreadPool>,
        spec: &TrackSpec,
        opts: &WindowOpts,
        hier: Hier,
        win: WindowPatch,
        live: Vec<(u32, u8)>,
        c0: P,
    ) -> Result<Tracker, String> {
        let margin_steps = spec.edge_margin.max(spec.steps_per_gen + 2);
        let mut w = World::on_topology(win.topo.clone(), 1);
        let caspec = CaSpec::new(&spec.rule)
            .nb(spec.nb)
            .steps(spec.steps_per_gen)
            .seed(SeedSpec::Empty);
        w.apply(Command::AddCa { ca: caspec })?;
        let ca = w.cas[0].clone();
        let kind = w.fields[ca.field].kind;
        let n = win.len();
        let open_dist = bfs(&win.topo, spec.nb, (0..n).filter(|&x| win.topo.is_open(x)));
        let mut t = Tracker {
            spec: spec.clone(),
            opts: opts.clone(),
            hier,
            ca,
            kind,
            field: Field::new(kind, 0, false),
            pool,
            live: live.clone(),
            ring: vec![u32::MAX; n],
            step: vec![u32::MAX; n],
            open_dist,
            margin_steps,
            generation: 0,
            pmin: live.len(),
            pmax: live.len(),
            c0,
            recentres: 1,
            max_tiles: n,
            seconds_before: 0.0,
            session: Instant::now(),
            history: FxMap::default(),
            stop: None,
            period: None,
            just_built: true,
            build_secs: 0.0,
            failure: None,
            win,
        };
        t.make_field();
        Ok(t)
    }

    fn make_field(&mut self) {
        let mut f = Field::new(self.kind, self.win.len(), false);
        for &(x, s) in &self.live {
            f.set(x as usize, s as u32);
        }
        self.field = f;
    }

    /// Resume from a checkpoint.
    pub fn resume(
        spec: &TrackSpec,
        opts: &WindowOpts,
        cp: &WindowCheckpoint,
    ) -> Result<Tracker, String> {
        let pool = one_thread()?;
        pool.clone()
            .install(|| Tracker::resume_in(pool, spec, opts, cp))
    }

    fn resume_in(
        pool: Arc<rayon::ThreadPool>,
        spec: &TrackSpec,
        opts: &WindowOpts,
        cp: &WindowCheckpoint,
    ) -> Result<Tracker, String> {
        check_rule(spec)?;
        let mut hier = Hier::with_frame(&spec.patch.family, cp.frame.clone(), opts.fixed_levels)?;
        let win = hier.build(&cp.discs)?;
        let mut live = Vec::new();
        for (k, s) in &cp.live {
            let t = win
                .find(&k.key())
                .ok_or("a live cell of the checkpoint is not on its window")?;
            live.push((t, *s));
        }
        live.sort_unstable();
        let mut t = Tracker::assemble(pool, spec, opts, hier, win, live, cp.c0)?;
        for (k, r, s) in &cp.labels {
            if let Some(i) = t.win.find(&k.key()) {
                t.ring[i as usize] = *r;
                t.step[i as usize] = *s;
            }
        }
        t.generation = cp.generation;
        t.pmin = cp.min_population;
        t.pmax = cp.max_population;
        t.recentres = cp.recentres;
        t.max_tiles = cp.max_tiles;
        t.seconds_before = cp.seconds;
        Ok(t)
    }

    /// The state right after the last window was built, if no generation has run since.
    pub fn checkpoint(&self) -> Option<WindowCheckpoint> {
        if !self.just_built || self.generation <= REPEAT_GENS {
            return None;
        }
        let key = |i: u32| Key::of(&self.win.keys[i as usize]);
        Some(WindowCheckpoint {
            frame: self.hier.frame.clone(),
            discs: self.win.discs.clone(),
            live: self.live.iter().map(|&(x, s)| (key(x), s)).collect(),
            labels: (0..self.win.len() as u32)
                .filter(|&i| self.ring[i as usize] != u32::MAX)
                .map(|i| (key(i), self.ring[i as usize], self.step[i as usize]))
                .collect(),
            generation: self.generation,
            min_population: self.pmin,
            max_population: self.pmax,
            recentres: self.recentres,
            max_tiles: self.max_tiles,
            seconds: self.seconds(),
            c0: self.c0,
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn stopped(&self) -> Option<Stop> {
        self.stop
    }
    pub fn window(&self) -> &WindowPatch {
        &self.win
    }
    pub fn level(&self) -> u32 {
        self.hier.level()
    }
    pub fn frame(&self) -> &Frame {
        &self.hier.frame
    }
    pub fn seconds(&self) -> f64 {
        self.seconds_before + self.session.elapsed().as_secs_f64()
    }
    /// Live cells by exact placement, sorted.
    pub fn live_keys(&self) -> Vec<((u16, Place), u8)> {
        let mut v: Vec<((u16, Place), u8)> = self
            .live
            .iter()
            .map(|&(x, s)| (self.win.keys[x as usize], s))
            .collect();
        v.sort_unstable_by_key(|(k, _)| Key::of(k).sort_key());
        v
    }
    /// Live cells as window tiles.
    pub fn live(&self) -> &[(u32, u8)] {
        &self.live
    }
    /// Edge rings from the start to the nearest live tile.
    pub fn rings(&self) -> u32 {
        self.live
            .iter()
            .map(|c| self.ring[c.0 as usize])
            .min()
            .unwrap_or(0)
    }

    fn record_history(&mut self) {
        if self.generation > REPEAT_GENS {
            if !self.history.is_empty() {
                self.history = FxMap::default();
            }
            return;
        }
        let mut snap: Vec<(Key, u8)> = self
            .live
            .iter()
            .map(|&(x, s)| (Key::of(&self.win.keys[x as usize]), s))
            .collect();
        snap.sort_unstable_by_key(|(k, s)| (k.sort_key(), *s));
        let mut h = crate::rng::Fnv::default();
        for (k, s) in &snap {
            h.bytes(&k.0.to_le_bytes());
            for v in k.1 {
                h.bytes(&v.to_le_bytes());
            }
            h.bytes(&[k.2, k.3 as u8, *s]);
        }
        let e = self.history.entry(h.0).or_default();
        if let Some((g0, _)) = e.iter().find(|(_, v)| *v == snap) {
            self.stop = Some(Stop::Repeated);
            self.period = Some(self.generation - g0);
            return;
        }
        e.push((self.generation, snap));
    }

    /// Run one generation (and move the window when the live cells near its edge). Returns
    /// why the run ended, once it has.
    pub fn step_once(&mut self) -> Result<Option<Stop>, String> {
        let pool = self.pool.clone();
        pool.install(|| self.step_in())
    }

    fn step_in(&mut self) -> Result<Option<Stop>, String> {
        if self.stop.is_some() {
            return Ok(self.stop);
        }
        if self.generation >= self.spec.max_gens {
            self.stop = Some(Stop::GenLimit);
            return Ok(self.stop);
        }
        let topo = self.win.topo.clone();
        for s in 0..self.ca.steps_per_gen {
            crate::step::step_ca(&topo, &mut self.ca, &mut self.field, s == 0);
        }
        self.generation += 1;
        self.just_built = false;
        let st = &self.field.state;
        self.live = st
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .map(|(x, &s)| (x as u32, s))
            .collect();
        let n = self.live.len();
        self.pmin = self.pmin.min(n);
        self.pmax = self.pmax.max(n);
        if n == 0 {
            self.stop = Some(Stop::Died);
            return Ok(self.stop);
        }
        if n > self.spec.max_pop {
            self.stop = Some(Stop::Grew);
            return Ok(self.stop);
        }
        self.record_history();
        if self.stop.is_some() {
            return Ok(self.stop);
        }
        if self
            .live
            .iter()
            .any(|c| self.open_dist[c.0 as usize] < self.margin_steps)
            && let Err(e) = self.recentre()
        {
            // Every generation so far was exact; the window cannot follow further.
            self.failure = Some(e);
            self.stop = Some(Stop::Edge);
            return Ok(self.stop);
        }
        if let Some(t) = self.opts.target_rings
            && self.rings() >= t
        {
            self.stop = Some(Stop::Target);
        }
        Ok(self.stop)
    }

    /// Run until the run ends, `stop` is set, or `until` passes (then at the next new window,
    /// so a checkpoint can be taken). Returns why it ended, if it did.
    pub fn run(
        &mut self,
        until: Option<Instant>,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Option<Stop>, String> {
        let pool = self.pool.clone();
        pool.install(|| self.run_in(until, stop))
    }

    fn run_in(
        &mut self,
        until: Option<Instant>,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Option<Stop>, String> {
        loop {
            if let Some(s) = self.step_in()? {
                return Ok(Some(s));
            }
            let late = until.is_some_and(|u| Instant::now() >= u);
            let asked = stop.is_some_and(|s| s.load(std::sync::atomic::Ordering::SeqCst));
            if asked || (late && (self.just_built || self.generation <= REPEAT_GENS)) {
                return Ok(None);
            }
        }
    }

    /// End the run on request (a job pauses).
    pub fn halt(&mut self) {
        if self.stop.is_none() {
            self.stop = Some(Stop::Stopped);
        }
    }

    fn recentre(&mut self) -> Result<(), String> {
        let t0 = Instant::now();
        let centres: Vec<P> = self
            .live
            .iter()
            .map(|c| self.win.centres[c.0 as usize])
            .collect();
        let discs = discs_for(&centres, self.opts.margin);
        let win = self.hier.build(&discs)?;
        let n = win.len();
        let mut ring = vec![u32::MAX; n];
        let mut step = vec![u32::MAX; n];
        for i in 0..n {
            if let Some(j) = self.win.find(&win.keys[i]) {
                ring[i] = self.ring[j as usize];
                step[i] = self.step[j as usize];
            }
        }
        relax(&win.topo, Nb::Edge, &mut ring);
        relax(&win.topo, self.spec.nb, &mut step);
        let mut live = Vec::with_capacity(self.live.len());
        for &(x, s) in &self.live {
            let t = win
                .find(&self.win.keys[x as usize])
                .ok_or("a live cell fell outside the new window")?;
            live.push((t, s));
        }
        live.sort_unstable();
        let open_dist = bfs(
            &win.topo,
            self.spec.nb,
            (0..n).filter(|&x| win.topo.is_open(x)),
        );
        if live
            .iter()
            .any(|c| open_dist[c.0 as usize] < self.margin_steps)
        {
            return Err(format!(
                "the window is too small for the pattern at generation {}: raise the margin",
                self.generation
            ));
        }
        self.win = win;
        self.ring = ring;
        self.step = step;
        self.open_dist = open_dist;
        self.live = live;
        self.recentres += 1;
        self.max_tiles = self.max_tiles.max(n);
        self.just_built = true;
        self.make_field();
        self.build_secs += t0.elapsed().as_secs_f64();
        Ok(())
    }

    fn reach(&self) -> Reach {
        let topo = &self.win.topo;
        if self.live.is_empty() {
            return Reach::default();
        }
        let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
        let cs: Vec<P> = self
            .live
            .iter()
            .map(|c| topo.centroid(c.0 as usize))
            .collect();
        let n = cs.len() as f64;
        let c1 = cs
            .iter()
            .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
        let euclid = cs
            .iter()
            .map(|&p| dist(p, self.c0))
            .fold(f64::INFINITY, f64::min);
        let shift = dist(self.c0, c1);
        Reach {
            rings: self.rings(),
            rings_far: self
                .live
                .iter()
                .map(|c| self.ring[c.0 as usize])
                .max()
                .unwrap_or(0),
            steps: self
                .live
                .iter()
                .map(|c| self.step[c.0 as usize])
                .min()
                .unwrap_or(0),
            euclid: r3(euclid),
            centroid_shift: r3(shift),
            heading_deg: (shift > 1e-9).then(|| {
                r3(crate::detmath::atan2(c1[1] - self.c0[1], c1[0] - self.c0[0]).to_degrees())
            }),
        }
    }

    /// The verdict so far (or at the end).
    pub fn report(&self) -> MoverReport {
        let stop = self.stop.unwrap_or(Stop::Stopped);
        let reach = self.reach();
        let secs = self.seconds();
        let travelled = reach.rings >= 2 && reach.euclid > 0.0;
        let verified = stop.verifies() && travelled;
        let family = self.spec.patch.family.clone();
        let level = self.hier.level();
        let nbn = match self.spec.nb {
            Nb::Touch => "touching-neighbour",
            Nb::Edge => "edge-neighbour",
        };
        let summary = if verified {
            format!(
                "verified to distance {} edge rings ({} {} steps, {:.1} units) over {} generations on a sliding window of {} (supertile level {}, at most {} tiles), population {}-{}; {}",
                reach.rings,
                reach.steps,
                nbn,
                reach.euclid,
                self.generation,
                family,
                level,
                self.max_tiles,
                self.pmin,
                self.pmax,
                match (stop, &self.failure) {
                    (Stop::Target, _) => "stopped at the requested distance".to_string(),
                    (Stop::GenLimit, _) => "stopped at the generation limit".to_string(),
                    (Stop::Edge, Some(e)) => format!("the window could not be built further: {e}"),
                    _ => "stopped on request".to_string(),
                }
            )
        } else {
            let why = match stop {
                Stop::Repeated => {
                    format!("{} (period {})", stop.describe(), self.period.unwrap_or(0))
                }
                s if s.verifies() => "did not travel".into(),
                s => s.describe().into(),
            };
            format!(
                "not a mover: {why} after {} generations on a sliding window of {family} (reached {} edge rings, {:.1} units)",
                self.generation, reach.rings, reach.euclid
            )
        };
        let speed = if self.generation > 0 {
            ((reach.euclid / self.generation as f64) * 1000.0).round() / 1000.0
        } else {
            0.0
        };
        let tiles = self.max_tiles;
        let groups = groups_around(&self.win.topo, self.spec.nb, self.c0, &self.live);
        let per = |x: f64| ((x / secs.max(1e-9)) * 10.0).round() / 10.0;
        MoverReport {
            verified,
            summary,
            stop,
            level,
            tiles,
            generations: self.generation,
            reach: reach.clone(),
            min_population: self.pmin,
            max_population: self.pmax,
            speed,
            levels: vec![LevelRun {
                level,
                tiles,
                generations: self.generation,
                stop,
                period: self.period,
                reach: reach.clone(),
                min_population: self.pmin,
                max_population: self.pmax,
                agrees_with_previous: None,
                seconds: (secs * 1000.0).round() / 1000.0,
            }],
            groups,
            window: Some(WindowStats {
                sliding: true,
                frame: self.hier.frame.clone(),
                top_level: level,
                max_tiles: self.max_tiles,
                tiles: self.win.len(),
                margin: self.opts.margin,
                fixed_levels: self.opts.fixed_levels,
                recentres: self.recentres,
                seconds: (secs * 1000.0).round() / 1000.0,
                gens_per_s: per(self.generation as f64),
                rings_per_s: per(reach.rings as f64),
                build_seconds: (self.build_secs * 1000.0).round() / 1000.0,
                snap_residual: self.hier.snap,
            }),
            rule: self.spec.rule.clone(),
            nb: self.spec.nb,
            family,
            note: NOTE.into(),
            jetcarp_version: crate::VERSION.into(),
            date: now_utc(),
        }
    }
}

const NOTE: &str = "Verified to a distance on a sliding window, not proven immortal: the run is exact on one infinite tiling (the window's supertile hierarchy), generation by generation; rings and steps are counted through the tiles the window held.";

fn one_thread() -> Result<Arc<rayon::ThreadPool>, String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .thread_name(|_| "window".into())
        .build()
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

/// Track a pattern on a sliding window until it stops (the spec's generation limit, the
/// options' target distance, or the pattern dies, grows, or repeats).
pub fn track_window(spec: &TrackSpec, opts: &WindowOpts) -> Result<MoverReport, String> {
    let mut t = Tracker::new(spec, opts)?;
    t.run(None, None)?;
    Ok(t.report())
}

impl Key {
    /// A total order for sorting.
    pub fn sort_key(&self) -> (u16, [i32; 4], u8, bool) {
        (self.0, self.1, self.2, self.3)
    }
}
