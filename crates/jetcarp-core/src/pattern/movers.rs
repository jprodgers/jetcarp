//! Movers on aperiodic tilings, verified to a distance.
//!
//! An aperiodic tiling has no translation symmetry, so a glider never repeats exactly and cannot
//! be proven immortal by a recurrence test. Instead the tracker runs the pattern on successively
//! larger patches of the same tiling (substitution or size-class levels, which nest: the same
//! tiles sit at the same place on every level) and reports how far it got: **verified to
//! distance D after N generations**, or why it stopped.
//!
//! Each level is run from generation 0 until a live cell comes within `edge_margin` neighbour
//! steps of the patch's open boundary (tiles there have incomplete neighbourhoods; up to that
//! generation the run equals the run on the whole infinite tiling, because activity spreads at
//! most one neighbour step per generation), the pattern dies, grows past `max_pop` cells,
//! repeats exactly (a stationary oscillator, not a mover), or the generation limit is reached.
//! When a level stops at its edge, the next level is run and checked against it: at the
//! generation the smaller level stopped, the live cells must be the same tiles in the same states.
//!
//! Distances are measured from the start: edge rings (edge-BFS steps from the starting tiles to
//! the nearest live tile, so the whole pattern is at least that far), neighbour steps in the
//! rule's neighbourhood, and Euclidean distance (world units; tile edges are about 1) from the
//! starting centroid to the nearest live tile.

use super::site::{interior_point, patch_topology};
use super::{
    CurationState, DiscoveredBy, PatternClass, PatternRecord, Provenance, RECORD_FORMAT,
    Verification, encode_cells, now_utc, tiling_display_name, tiling_identity,
};
use crate::evolve::arena::same_polygon;
use crate::geom::P;
use crate::objects::{LCell, Phase};
use crate::topology::{Nb, SizeSpec, TilingSpec, Topology};
use crate::world::{CaSpec, Command, SeedSpec, World};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// What to track.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackSpec {
    /// Rule notation (`SpectreGlider`, `Priority 4: ...`, `B2/S34`).
    pub rule: String,
    pub nb: Nb,
    #[serde(default = "one")]
    pub steps_per_gen: u32,
    /// The patch the cells are given on (family and level).
    pub patch: TilingSpec,
    /// Starting cells: (tile on `patch`, state).
    pub cells: Vec<(u32, u8)>,
    /// Largest level to try (default: [`crate::aperiodic::auto_level_max`]).
    #[serde(default)]
    pub max_level: Option<u32>,
    /// Generation limit over all levels.
    pub max_gens: u64,
    /// Stop as "grew" above this many live cells.
    pub max_pop: usize,
    /// Neighbour steps kept between live cells and the open boundary.
    pub edge_margin: u32,
}

fn one() -> u32 {
    1
}

impl TrackSpec {
    pub fn new(rule: &str, nb: Nb, patch: TilingSpec, cells: Vec<(u32, u8)>) -> TrackSpec {
        TrackSpec {
            rule: rule.into(),
            nb,
            steps_per_gen: 1,
            patch,
            cells,
            max_level: None,
            max_gens: 20_000,
            max_pop: 2_000,
            edge_margin: 3,
        }
    }
}

/// Why a level's run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    /// Came within the edge margin of the patch boundary (go to the next level).
    Edge,
    Died,
    /// Population above the limit.
    Grew,
    /// The whole state repeated exactly: a stationary oscillator (or still life).
    Repeated,
    /// Generation limit.
    GenLimit,
    /// Reached the requested distance (sliding window).
    Target,
    /// Stopped on request (sliding window; a job resumes it).
    Stopped,
}

impl Stop {
    pub fn describe(self) -> &'static str {
        match self {
            Stop::Edge => "reached the edge of the largest patch",
            Stop::Died => "died",
            Stop::Grew => "grew without bound",
            Stop::Repeated => "repeated exactly in place (an oscillator, not a mover)",
            Stop::GenLimit => "generation limit",
            Stop::Target => "reached the requested distance",
            Stop::Stopped => "stopped on request",
        }
    }
    /// The run ended without the pattern failing: it is verified as far as it got.
    pub fn verifies(self) -> bool {
        matches!(
            self,
            Stop::Edge | Stop::GenLimit | Stop::Target | Stop::Stopped
        )
    }
}

/// How a mover was verified on a sliding window ([`crate::window`]) instead of fixed patches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowStats {
    /// Always true: verified on a sliding window (supertiles generated around the live cells).
    pub sliding: bool,
    /// The supertile hierarchy at the end (the Spectre's ancestor chain above the start patch,
    /// or the hat and Penrose generation depth).
    pub frame: crate::window::Frame,
    /// Its top level.
    pub top_level: u32,
    /// Most tiles held at once, and tiles held at the end.
    pub max_tiles: usize,
    pub tiles: usize,
    /// World units of tiles kept around the live cells.
    pub margin: f64,
    /// Spectre: levels on which the window follows the fixed patches' Δ chain.
    pub fixed_levels: u32,
    /// Windows built (one per move of the window).
    pub recentres: u64,
    pub seconds: f64,
    pub gens_per_s: f64,
    pub rings_per_s: f64,
    /// Seconds of the last session spent building windows.
    #[serde(default)]
    pub build_seconds: f64,
    /// Hat: the largest distance of a float hat position from the lattice point it was snapped
    /// to (the float metatile hierarchy's error; at most 0.05 is accepted).
    #[serde(default)]
    pub snap_residual: f64,
}

/// Distances from the start at one generation.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Reach {
    /// Edge rings from the starting tiles to the nearest live tile (and to the farthest).
    pub rings: u32,
    pub rings_far: u32,
    /// Steps in the rule's neighbourhood to the nearest live tile.
    pub steps: u32,
    /// World units from the starting centroid to the nearest live tile's centroid.
    pub euclid: f64,
    /// World units the centroid of the live tiles moved.
    pub centroid_shift: f64,
    /// Direction of the centroid's move, degrees anticlockwise from +x.
    pub heading_deg: Option<f64>,
}

/// One level's run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelRun {
    pub level: u32,
    pub tiles: usize,
    pub generations: u64,
    pub stop: Stop,
    /// Period, when the state repeated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    pub reach: Reach,
    pub min_population: usize,
    pub max_population: usize,
    /// At the generation the previous level stopped, the same live tiles and states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agrees_with_previous: Option<bool>,
    pub seconds: f64,
}

/// A group of live tiles at the end (gliders flying apart are separate groups).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub cells: usize,
    pub centroid: P,
    pub euclid: f64,
    pub heading_deg: f64,
}

/// The tracker's verdict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoverReport {
    /// It travelled and survived until it reached the edge of the largest patch (or the
    /// generation limit) without dying, growing, or repeating in place.
    pub verified: bool,
    /// `verified to distance ...` or the reason it stopped.
    pub summary: String,
    pub stop: Stop,
    /// The last level run and its tile count.
    pub level: u32,
    pub tiles: usize,
    /// Generations verified (on the last level).
    pub generations: u64,
    pub reach: Reach,
    pub min_population: usize,
    pub max_population: usize,
    /// World units per generation (nearest live tile), over the whole run.
    pub speed: f64,
    pub levels: Vec<LevelRun>,
    pub groups: Vec<Group>,
    /// Verified on a sliding window: how ([`crate::window::track`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowStats>,
    pub rule: String,
    pub nb: Nb,
    pub family: String,
    pub note: String,
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    pub date: String,
}

const NOTE: &str = "Verified to a distance, not proven immortal: on an aperiodic tiling no exact repeat is expected, so the run is checked generation by generation on nested patches until it nears the largest patch's edge.";

fn level_of(spec: &TilingSpec) -> u32 {
    match spec.size {
        Some(SizeSpec::Level(l)) => l,
        _ => crate::aperiodic::default_level(&spec.family),
    }
}

/// Map tiles of one patch level onto a larger level of the same tiling (the same tiles, placed
/// by [`crate::aperiodic::embedding`]).
fn map_cells(
    from: &Topology,
    to: &Topology,
    cells: &[(u32, u8)],
) -> Result<Vec<(u32, u8)>, String> {
    let (lf, lt) = (level_of(&from.spec), level_of(&to.spec));
    let m = crate::aperiodic::embedding(&from.spec.family, lf, lt);
    let mut out = Vec::with_capacity(cells.len());
    for &(x, s) in cells {
        // Exact numbering where the family has it (the Spectre).
        if let Some(t) = crate::aperiodic::map_tile(&from.spec.family, lf, lt, x as usize, from, to)
        {
            out.push((t? as u32, s));
            continue;
        }
        let poly: Vec<P> = from
            .polygon(x as usize)
            .iter()
            .map(|&q| m.apply(q))
            .collect();
        let t = to
            .locate(interior_point(&poly))
            .ok_or_else(|| format!("tile {x} is outside the larger patch"))?;
        if !same_polygon(&poly, &to.polygon(t)) {
            return Err(format!(
                "tile {x} has no matching tile on the larger patch (the patches do not nest)"
            ));
        }
        out.push((t as u32, s));
    }
    out.sort_unstable();
    Ok(out)
}

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

fn live_of(w: &World, fi: usize) -> Vec<(u32, u8)> {
    w.fields[fi]
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != 0)
        .map(|(x, &s)| (x as u32, s))
        .collect()
}

fn snapshot_hash(v: &[(u32, u8)]) -> u64 {
    let mut h = crate::rng::Fnv::default();
    for &(x, s) in v {
        h.bytes(&x.to_le_bytes());
        h.bytes(&[s]);
    }
    h.0
}

struct LevelResult {
    run: LevelRun,
    /// Live cells at the last valid generation.
    last: Vec<(u32, u8)>,
    /// Live cells at a requested check generation.
    at_check: Option<Vec<(u32, u8)>>,
    topo: Arc<Topology>,
    start: Vec<(u32, u8)>,
}

fn run_level(
    spec: &TrackSpec,
    topo: Arc<Topology>,
    start: Vec<(u32, u8)>,
    check_gen: Option<u64>,
) -> Result<LevelResult, String> {
    let t0 = std::time::Instant::now();
    let level = level_of(&topo.spec);
    let mut w = World::on_topology(topo.clone(), 1);
    let ca = CaSpec::new(&spec.rule)
        .nb(spec.nb)
        .steps(spec.steps_per_gen)
        .seed(SeedSpec::Empty);
    w.apply(Command::AddCa { ca })?;
    let id = w.cas[0].id;
    let fi = w.cas[0].field;
    w.apply(Command::SetCells {
        id,
        cells: start.iter().map(|&(x, s)| [x, s as u32]).collect(),
    })?;
    let open = bfs(&topo, spec.nb, (0..topo.len()).filter(|&x| topo.is_open(x)));
    let near = |v: &[(u32, u8)]| v.iter().any(|&(x, _)| open[x as usize] < spec.edge_margin);
    let mut live = live_of(&w, fi);
    if near(&live) {
        return Err(format!(
            "the pattern starts within {} steps of the level {level} patch's edge",
            spec.edge_margin
        ));
    }
    // State hash -> (generation, live cells) seen with that hash.
    type Seen = HashMap<u64, Vec<(u64, Vec<(u32, u8)>)>>;
    let mut seen: Seen = HashMap::new();
    seen.entry(snapshot_hash(&live))
        .or_default()
        .push((0, live.clone()));
    let (mut pmin, mut pmax) = (live.len(), live.len());
    let mut at_check = (check_gen == Some(0)).then(|| live.clone());
    let mut stop = Stop::GenLimit;
    let mut period = None;
    let mut g = 0;
    while g < spec.max_gens {
        w.apply(Command::Step { gens: 1 })?;
        let now = live_of(&w, fi);
        if near(&now) {
            stop = Stop::Edge;
            break;
        }
        g += 1;
        live = now;
        pmin = pmin.min(live.len());
        pmax = pmax.max(live.len());
        if check_gen == Some(g) {
            at_check = Some(live.clone());
        }
        if live.is_empty() {
            stop = Stop::Died;
            break;
        }
        if live.len() > spec.max_pop {
            stop = Stop::Grew;
            break;
        }
        let h = snapshot_hash(&live);
        let e = seen.entry(h).or_default();
        if let Some((g0, _)) = e.iter().find(|(_, v)| *v == live) {
            stop = Stop::Repeated;
            period = Some(g - g0);
            break;
        }
        e.push((g, live.clone()));
    }
    let reach = reach_of(&topo, spec.nb, &start, &live);
    Ok(LevelResult {
        run: LevelRun {
            level,
            tiles: topo.len(),
            generations: g,
            stop,
            period,
            reach,
            min_population: pmin,
            max_population: pmax,
            agrees_with_previous: None,
            seconds: t0.elapsed().as_secs_f64(),
        },
        last: live,
        at_check,
        topo,
        start,
    })
}

fn centroid_of(topo: &Topology, cells: &[(u32, u8)]) -> Option<P> {
    if cells.is_empty() {
        return None;
    }
    let mut c = [0.0, 0.0];
    for &(x, _) in cells {
        let p = topo.centroid(x as usize);
        c[0] += p[0];
        c[1] += p[1];
    }
    Some([c[0] / cells.len() as f64, c[1] / cells.len() as f64])
}

fn reach_of(topo: &Topology, nb: Nb, start: &[(u32, u8)], live: &[(u32, u8)]) -> Reach {
    if live.is_empty() {
        return Reach::default();
    }
    let rings = bfs(topo, Nb::Edge, start.iter().map(|c| c.0 as usize));
    let steps = bfs(topo, nb, start.iter().map(|c| c.0 as usize));
    let c0 = centroid_of(topo, start).unwrap_or([0.0, 0.0]);
    let c1 = centroid_of(topo, live).unwrap_or(c0);
    let euclid = live
        .iter()
        .map(|&(x, _)| crate::geom::dist(topo.centroid(x as usize), c0))
        .fold(f64::INFINITY, f64::min);
    let shift = crate::geom::dist(c0, c1);
    Reach {
        rings: live.iter().map(|c| rings[c.0 as usize]).min().unwrap_or(0),
        rings_far: live.iter().map(|c| rings[c.0 as usize]).max().unwrap_or(0),
        steps: live.iter().map(|c| steps[c.0 as usize]).min().unwrap_or(0),
        euclid: r3(euclid),
        centroid_shift: r3(shift),
        heading_deg: (shift > 1e-9)
            .then(|| r3(crate::detmath::atan2(c1[1] - c0[1], c1[0] - c0[0]).to_degrees())),
    }
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Groups of live tiles at most 2 neighbour steps apart.
fn groups_of(topo: &Topology, nb: Nb, start: &[(u32, u8)], live: &[(u32, u8)]) -> Vec<Group> {
    groups_around(
        topo,
        nb,
        centroid_of(topo, start).unwrap_or([0.0, 0.0]),
        live,
    )
}

/// Groups of live tiles at most 2 neighbour steps apart, with distances from `c0`.
pub(crate) fn groups_around(topo: &Topology, nb: Nb, c0: P, live: &[(u32, u8)]) -> Vec<Group> {
    let set: HashMap<u32, usize> = live.iter().enumerate().map(|(i, c)| (c.0, i)).collect();
    let mut comp = vec![usize::MAX; live.len()];
    let mut out = Vec::new();
    for i in 0..live.len() {
        if comp[i] != usize::MAX {
            continue;
        }
        let g = out.len();
        comp[i] = g;
        let mut stack = vec![i];
        let mut members = vec![];
        while let Some(a) = stack.pop() {
            members.push(live[a]);
            let x = live[a].0 as usize;
            let mut near = Vec::new();
            topo.for_each_neighbor(x, nb, |y| {
                near.push(y);
                topo.for_each_neighbor(y, nb, |z| near.push(z));
            });
            for y in near {
                if let Some(&b) = set.get(&(y as u32))
                    && comp[b] == usize::MAX
                {
                    comp[b] = g;
                    stack.push(b);
                }
            }
        }
        let c = centroid_of(topo, &members).unwrap();
        out.push(Group {
            cells: members.len(),
            centroid: [r3(c[0]), r3(c[1])],
            euclid: r3(crate::geom::dist(c, c0)),
            heading_deg: r3(crate::detmath::atan2(c[1] - c0[1], c[0] - c0[0]).to_degrees()),
        });
    }
    out
}

/// Track a pattern over growing patch levels.
pub fn track(spec: &TrackSpec) -> Result<MoverReport, String> {
    let family = spec.patch.family.clone();
    if !crate::evolve::arena::is_aperiodic(&family) {
        return Err(format!(
            "the mover tracker is for aperiodic patches (spectre, hat, penrose-kd, penrose-rhomb), not {family}"
        ));
    }
    if spec.cells.is_empty() {
        return Err("no starting cells".into());
    }
    crate::rules::Rule::parse(&spec.rule)?;
    let ident = tiling_identity(&spec.patch);
    let l0 = level_of(&spec.patch);
    let (_, hi) = crate::aperiodic::level_range(&family);
    let auto = crate::aperiodic::auto_level_max(&family);
    let top = spec.max_level.unwrap_or(auto).min(hi).max(l0);
    let topo0 = patch_topology(&ident.clone().with_size(SizeSpec::Level(l0)))?;
    let mut cells0 = spec.cells.clone();
    cells0.sort_unstable();
    cells0.dedup_by_key(|c| c.0);
    if let Some(&(x, _)) = cells0.iter().find(|c| c.0 as usize >= topo0.len()) {
        return Err(format!("no tile {x} on level {l0}"));
    }
    let mut levels: Vec<LevelRun> = Vec::new();
    let mut prev: Option<LevelResult> = None;
    let mut last: Option<LevelResult> = None;
    for l in l0..=top {
        let topo = if l == l0 {
            topo0.clone()
        } else {
            patch_topology(&ident.clone().with_size(SizeSpec::Level(l)))?
        };
        let start = if l == l0 {
            cells0.clone()
        } else {
            map_cells(&topo0, &topo, &cells0)?
        };
        let check = prev.as_ref().map(|p| p.run.generations);
        let mut r = match run_level(spec, topo, start, check) {
            Ok(r) => r,
            // Too close to this level's edge (a smaller level can sit near a larger one's
            // edge): try the next level.
            Err(e) if l < top && e.contains("starts within") => continue,
            Err(e) => return Err(e),
        };
        if let Some(p) = &prev {
            let agree = match &r.at_check {
                Some(now) => map_cells(&p.topo, &r.topo, &p.last).is_ok_and(|m| m == *now),
                None => false,
            };
            r.run.agrees_with_previous = Some(agree);
            if !agree {
                levels.push(r.run.clone());
                return Err(format!(
                    "level {l} disagrees with level {} at generation {}: the patches do not nest",
                    p.run.level, p.run.generations
                ));
            }
        }
        levels.push(r.run.clone());
        let go_on = r.run.stop == Stop::Edge;
        prev = Some(LevelResult {
            run: r.run.clone(),
            last: r.last.clone(),
            at_check: None,
            topo: r.topo.clone(),
            start: r.start.clone(),
        });
        last = Some(r);
        if !go_on {
            break;
        }
    }
    let r = last.ok_or("the pattern is too close to the edge of every level")?;
    let run = &r.run;
    let travelled = run.reach.rings >= 2 && run.reach.euclid > 0.0;
    let verified = run.stop.verifies() && travelled;
    let summary = if verified {
        format!(
            "verified to distance {} edge rings ({} {} steps, {:.1} units) over {} generations on {} level {} ({} tiles), population {}-{}; {}",
            run.reach.rings,
            run.reach.steps,
            nb_name(spec.nb),
            run.reach.euclid,
            run.generations,
            family,
            run.level,
            run.tiles,
            run.min_population,
            run.max_population,
            if run.stop == Stop::Edge {
                "stopped at the edge of the largest patch"
            } else {
                "stopped at the generation limit"
            }
        )
    } else {
        let why = match run.stop {
            Stop::Repeated => format!(
                "{} (period {})",
                run.stop.describe(),
                run.period.unwrap_or(0)
            ),
            Stop::Edge | Stop::GenLimit => "did not travel".into(),
            s => s.describe().into(),
        };
        format!(
            "not a mover: {why} after {} generations on {} level {} (reached {} edge rings, {:.1} units)",
            run.generations, family, run.level, run.reach.rings, run.reach.euclid
        )
    };
    let speed = if run.generations > 0 {
        r3(run.reach.euclid / run.generations as f64)
    } else {
        0.0
    };
    Ok(MoverReport {
        verified,
        summary,
        stop: run.stop,
        level: run.level,
        tiles: run.tiles,
        generations: run.generations,
        reach: run.reach.clone(),
        min_population: levels.iter().map(|l| l.min_population).min().unwrap_or(0),
        max_population: levels.iter().map(|l| l.max_population).max().unwrap_or(0),
        speed,
        groups: groups_of(&r.topo, spec.nb, &r.start, &r.last),
        levels,
        window: None,
        rule: spec.rule.clone(),
        nb: spec.nb,
        family,
        note: NOTE.into(),
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}

fn nb_name(nb: Nb) -> &'static str {
    match nb {
        Nb::Touch => "touching-neighbour",
        Nb::Edge => "edge-neighbour",
    }
}

/// The verification record kept in a library record for a mover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoverRecord {
    /// Edge rings from the start to the nearest live tile when verification stopped.
    pub distance_rings: u32,
    /// Steps in the rule's neighbourhood.
    pub distance_steps: u32,
    /// World units from the starting centroid to the nearest live tile.
    pub distance_euclid: f64,
    pub generations: u64,
    /// Largest patch level reached, and its tiles.
    pub level: u32,
    pub tiles: usize,
    pub stop: Stop,
    pub min_population: usize,
    pub max_population: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f64>,
    pub speed: f64,
    /// Every level run, smallest first.
    pub levels: Vec<LevelRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    pub summary: String,
    /// Verified on a sliding window (rings and steps counted through the tiles it held).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowStats>,
}

impl MoverRecord {
    pub fn from_report(r: &MoverReport) -> MoverRecord {
        MoverRecord {
            distance_rings: r.reach.rings,
            distance_steps: r.reach.steps,
            distance_euclid: r.reach.euclid,
            generations: r.generations,
            level: r.level,
            tiles: r.tiles,
            stop: r.stop,
            min_population: r.min_population,
            max_population: r.max_population,
            heading_deg: r.reach.heading_deg,
            speed: r.speed,
            levels: r.levels.clone(),
            groups: r.groups.clone(),
            summary: r.summary.clone(),
            window: r.window.clone(),
        }
    }
}

/// A library record for a verified mover: its starting configuration (phase 0) with its site,
/// class `mover`, and the tracker's verification. Fails unless the report verified it.
pub fn mover_record(
    spec: &TrackSpec,
    report: &MoverReport,
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<PatternRecord, String> {
    if !report.verified {
        return Err(report.summary.clone());
    }
    let topo = patch_topology(&spec.patch)?;
    let rule = crate::rules::Rule::parse(&spec.rule)?.0;
    let notation = rule.notation();
    let phase0: Phase = spec
        .cells
        .iter()
        .map(|&(x, s)| LCell {
            i: x as i32,
            j: 0,
            k: 0,
            s,
        })
        .collect();
    let (site, phases, canonical) =
        super::site_graph::build_mover_site(&topo, spec.nb, &rule, &phase0)?;
    let tiling = tiling_identity(&topo.spec);
    let now = now_utc();
    let p0 = &phases[0];
    let mut rec = PatternRecord {
        format: RECORD_FORMAT,
        id: String::new(),
        name: None,
        rule: notation,
        rules: vec![],
        range: None,
        neighborhood: spec.nb,
        steps_per_gen: spec.steps_per_gen,
        tiling_name: tiling_display_name(&tiling),
        tiling,
        tile_types: 0,
        class: PatternClass::Mover,
        period: 1,
        displacement: None,
        displacement_xy: None,
        speed: Some(format!("{:.3} units/gen", report.speed)),
        cells: p0.len(),
        min_cells: report.min_population,
        max_cells: report.max_population,
        bbox: [0, 0, 0, 0],
        canonical,
        phases: vec![encode_cells(p0)],
        site: Some(site),
        verified: Verification {
            periods: 1,
            generations: report.generations,
            field: verified_field(report),
            sites: 1,
            jetcarp_version: crate::VERSION.into(),
            date: now.clone(),
        },
        mover: Some(MoverRecord::from_report(report)),
        gun: None,
        seen: 1,
        provenance,
        discovered_by,
        tags: vec!["mover".into(), "aperiodic".into()],
        notes: String::new(),
        score: 0.0,
        rating: None,
        state: CurationState::New,
        created: now.clone(),
        updated: now,
        geometry_pin: String::new(),
        engine_version: crate::VERSION.into(),
        needs_reverify: false,
    };
    rec.geometry_pin = super::record_pin(&rec)?.pin;
    rec.id = super::pattern_id(&rec)?;
    rec.rescore();
    Ok(rec)
}

/// The `verified.field` text of a mover record.
pub fn verified_field(r: &MoverReport) -> String {
    match &r.window {
        Some(w) => format!(
            "tracked on a sliding window of {} (supertile level {}, at most {} tiles)",
            r.family, w.top_level, w.max_tiles
        ),
        None => format!(
            "tracked on {} levels {}-{}, {} tiles at the largest",
            r.family,
            r.levels.first().map(|l| l.level).unwrap_or(0),
            r.level,
            r.tiles
        ),
    }
}

/// Track a stored mover again from its site (on its original patch) and return the fresh report.
pub fn retrack(
    rec: &PatternRecord,
    max_gens: Option<u64>,
) -> Result<(TrackSpec, MoverReport), String> {
    let site = rec.site.as_ref().ok_or("not an aperiodic pattern")?;
    let topo = patch_topology(&site.patch)?;
    let map = super::site::match_site(&topo, site, site.anchor as usize)?;
    let cells: Vec<(u32, u8)> = rec
        .phase(0)?
        .iter()
        .map(|c| {
            map.get(c.i as usize)
                .map(|&x| (x, c.s))
                .ok_or_else(|| "cell outside the site".to_string())
        })
        .collect::<Result<_, _>>()?;
    let mut spec = TrackSpec::new(&rec.rule, rec.neighborhood, site.patch.clone(), cells);
    spec.steps_per_gen = rec.steps_per_gen;
    if let Some(g) = max_gens {
        spec.max_gens = g;
    } else if let Some(m) = &rec.mover {
        spec.max_gens = spec.max_gens.max(m.generations + 1);
    }
    // A sliding-window record is tracked on a window again, to its recorded generation (at
    // most [`WINDOW_REVERIFY_GENS`] when re-verifying: a million rings take minutes).
    if let Some(w) = rec.mover.as_ref().and_then(|m| m.window.as_ref()) {
        if max_gens.is_none() {
            spec.max_gens = rec
                .mover
                .as_ref()
                .map(|m| m.generations.min(WINDOW_REVERIFY_GENS))
                .unwrap_or(spec.max_gens);
        }
        let opts = crate::window::track::WindowOpts {
            margin: w.margin,
            fixed_levels: w.fixed_levels,
            target_rings: None,
        };
        let report = crate::window::track::track_window(&spec, &opts)?;
        return Ok((spec, report));
    }
    // Up to the level it was verified on (a larger level would only add work).
    if let Some(m) = &rec.mover {
        spec.max_level = Some(m.level);
    }
    let report = track(&spec)?;
    Ok((spec, report))
}

/// Generations a sliding-window record is re-run for by [`verify_mover`] (about a minute).
pub const WINDOW_REVERIFY_GENS: u64 = 400_000;

/// Re-verification for [`super::verify`]: the tracker must verify the mover again, at least as
/// far as recorded (a sliding-window record over [`WINDOW_REVERIFY_GENS`] generations: verified
/// again up to that generation).
pub fn verify_mover(rec: &PatternRecord) -> Result<Verification, String> {
    let (_, rep) = retrack(rec, None)?;
    if !rep.verified {
        return Err(rep.summary);
    }
    if let Some(m) = &rec.mover
        && rep.generations >= m.generations
        && rep.reach.rings < m.distance_rings
    {
        return Err(format!(
            "verified to {} rings now, the record says {}",
            rep.reach.rings, m.distance_rings
        ));
    }
    Ok(Verification {
        periods: 1,
        generations: rep.generations,
        field: verified_field(&rep),
        sites: 1,
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}
