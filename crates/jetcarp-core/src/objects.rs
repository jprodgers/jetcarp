//! Still-life, oscillator, and spaceship detection in a finished run.
//!
//! 1. Live cells are grouped by the CA's neighbour graph (touching cells), and groups closer than
//!    the interaction distance (default 2: they share a neighbour) are merged.
//! 2. Each group is copied onto an empty field with an empty margin and run alone with the same
//!    rule. On lattice tilings the copy goes into a small dead-edged grid; if any live cell comes
//!    near its edge the run restarts with a margin the pattern cannot cross in the time allowed
//!    (one neighbour offset per generation), so the result equals the infinite plane. B/S and
//!    Generations rules at one step per generation run on the bit-parallel stepper's unbounded
//!    plane instead ([`crate::hunt::bits`]): the same run, without the grid. On explicit
//!    (aperiodic) patches the group runs alone on the same patch.
//! 3. The run is compared with the starting cells by exact comparison every generation: equal
//!    means a still life (period 1) or an oscillator; equal after a lattice translation means a
//!    spaceship. Otherwise the group is "other" (it dies, grows, or does not repeat in time).
//! 4. A merged group whose parts are each periodic, and whose joint run is exactly the union of
//!    the parts' runs over a full common period, is reported as its parts (a blinker next to a
//!    block is a blinker and a block). Otherwise it is one object. Pieces that die or never repeat
//!    alone are retried together when they lie a few steps apart, since some oscillator phases
//!    fall apart into pieces (the pentadecathlon's do); the merge is kept only if it is periodic.
//!    A group that grows is tested for a **puffer** ([`puffer`]): run alone on the unbounded
//!    lattice, its moving front recurs exactly, shifted by a lattice vector every `p`
//!    generations, while the debris behind it grows.
//! 5. Canonical forms: the minimum over every phase and every symmetry of the tiling of the
//!    normalized cell list. The symmetries are the library's
//!    ([`crate::pattern::canon::tiling_symmetries`]): the integer lattice maps that preserve the
//!    neighbour offsets on one-tile lattices (the 8 of the square grid, 12 on the hexagon grid),
//!    and on lattices with several tile types the maps that also permute the types (4 on
//!    `k2-017`); only the identity for rule tables. On lattices each object is labelled, and
//!    counted, by the content id the library gives it, so a ship and its mirror image are one row
//!    of the census. Explicit patches get an invariant (not exact).

use crate::geom::{P, bbox, centroid};
use crate::pattern::CellMap;
use crate::render::Rgba;
use crate::rng::Fnv;
use crate::rules::Rule;
use crate::topology::{Boundary, LatticeGrid, Nb, SizeSpec, TilingSpec, Topology};
use crate::world::{CaSpec, Command, SeedSpec, World};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

pub mod compound;
pub mod puffer;

#[derive(Clone, Debug)]
pub struct ObjectOptions {
    /// The CA whose rule and field are analysed (default: the first enabled CA that qualifies).
    pub ca: Option<u32>,
    /// Longest period (and longest isolated run) looked for.
    pub max_period: u64,
    /// Live cells within this many neighbour steps belong to one group (1 = touching only).
    pub merge_distance: u32,
    /// Split merged groups whose parts provably do not interact.
    pub separate: bool,
    /// Pieces that die alone are retried together when within this many steps (0 = never).
    pub piece_distance: u32,
}

impl Default for ObjectOptions {
    fn default() -> Self {
        ObjectOptions {
            ca: None,
            max_period: 64,
            merge_distance: 2,
            separate: true,
            piece_distance: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum ObjClass {
    StillLife,
    Oscillator {
        period: u64,
    },
    /// Repeats shifted by the lattice vector (dx, dy) every `period` generations.
    Spaceship {
        period: u64,
        dx: i32,
        dy: i32,
    },
    /// Travels like a spaceship (its front recurs shifted by (dx, dy) every `period`
    /// generations) while leaving growing debris behind.
    Puffer {
        period: u64,
        dx: i32,
        dy: i32,
    },
    /// Dies, grows, or does not repeat within the period limit.
    Other {
        reason: String,
    },
}

impl ObjClass {
    pub fn kind(&self) -> &'static str {
        match self {
            ObjClass::StillLife => "still life",
            ObjClass::Oscillator { .. } => "oscillator",
            ObjClass::Spaceship { .. } => "spaceship",
            ObjClass::Puffer { .. } => "puffer",
            ObjClass::Other { .. } => "other",
        }
    }
    pub fn period(&self) -> Option<u64> {
        match self {
            ObjClass::StillLife => Some(1),
            ObjClass::Oscillator { period }
            | ObjClass::Spaceship { period, .. }
            | ObjClass::Puffer { period, .. } => Some(*period),
            ObjClass::Other { .. } => None,
        }
    }
    /// Spaceship speed in lattice steps, like `c/4 diagonal` or `c/2 orthogonal`, on a lattice
    /// with basis `lattice` (`dx`, `dy` count its vectors). The fraction is the larger of `|dx|`
    /// and `|dy|` over the period, in the basis as it is, on hexagonal lattices too (the convention
    /// Golly's hexagonal rules use: there (1, 1) counts as one step, as (1, 0) does). The
    /// direction is named in the lattice's own terms
    /// ([`direction`]), or left out where the lattice has no axes to name it by; `None` names
    /// it on the square grid's basis.
    pub fn speed_in(&self, lattice: Option<(P, P)>) -> Option<String> {
        let (ObjClass::Spaceship { dx, dy, .. } | ObjClass::Puffer { dx, dy, .. }) = self else {
            return None;
        };
        let s = self.fraction()?;
        let (a1, a2) = lattice.unwrap_or(([1.0, 0.0], [0.0, 1.0]));
        Some(match direction([*dx, *dy], a1, a2) {
            Some(dir) => format!("{s} {dir}"),
            None => s,
        })
    }
    /// The speed's fraction alone: `c/4`, `2c/5`.
    fn fraction(&self) -> Option<String> {
        let (ObjClass::Spaceship { period, dx, dy } | ObjClass::Puffer { period, dx, dy }) = self
        else {
            return None;
        };
        let m = dx.unsigned_abs().max(dy.unsigned_abs()) as u64;
        let g = gcd(m, *period).max(1);
        let (a, b) = (m / g, period / g);
        let num = if a == 1 {
            "c".to_string()
        } else {
            format!("{a}c")
        };
        Some(if b == 1 { num } else { format!("{num}/{b}") })
    }
    /// Short tag for summaries: `p2`, `c/4`, or nothing.
    pub fn tag(&self) -> Option<String> {
        match self {
            ObjClass::Oscillator { period } => Some(format!("p{period}")),
            ObjClass::Spaceship { .. } | ObjClass::Puffer { .. } => self.fraction(),
            _ => None,
        }
    }
    pub fn color(&self) -> Rgba {
        match self {
            ObjClass::StillLife => [0x4d, 0x9d, 0xff, 0xff],
            ObjClass::Oscillator { .. } => [0x3d, 0xdc, 0x6d, 0xff],
            ObjClass::Spaceship { .. } => [0xff, 0x4d, 0xc4, 0xff],
            ObjClass::Puffer { .. } => [0xff, 0xd8, 0x3c, 0xff],
            ObjClass::Other { .. } => [0xff, 0x9a, 0x3c, 0xff],
        }
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The shape of a lattice, from its shortest translations: what a spaceship's direction can be
/// named by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatticeShape {
    /// Two equal shortest translations at right angles: the square grid's lattice.
    Square,
    /// Three equal shortest translations 60° apart: hexagons, triangles, kagome and the many
    /// tilings with six-fold or three-fold symmetry.
    Hexagonal,
    /// Two perpendicular axes of mirror symmetry and no more: a rectangular lattice (its shortest
    /// translations at right angles) or a centred one (a rhombus, or rows offset by half a step,
    /// as in the elongated triangular tiling). `axes` are the two directions, as combinations of
    /// the reduced basis `u`, `v` ([`reduce_basis`]).
    Rectangular { axes: [[i64; 2]; 2] },
    /// An oblique lattice: no axes of symmetry to name a direction by.
    Other,
}

/// `a1`, `a2` reduced (Gauss, as integer combinations of them) to the lattice's shortest
/// translations `u`, `v`: `|u| <= |v|`, the angle between them at least 60°, and on a hexagonal
/// lattice 120°, so that `u`, `v` and `u + v` are the three shortest. Returns the shape and the
/// combinations (`u = m[0][0] a1 + m[0][1] a2`, `v = m[1][0] a1 + m[1][1] a2`).
pub fn reduce_basis(a1: P, a2: P) -> (LatticeShape, [[i64; 2]; 2]) {
    let dot = |p: P, q: P| p[0] * q[0] + p[1] * q[1];
    let comb = |m: [i64; 2]| -> P {
        [
            m[0] as f64 * a1[0] + m[1] as f64 * a2[0],
            m[0] as f64 * a1[1] + m[1] as f64 * a2[1],
        ]
    };
    let (mut mu, mut mv) = ([1i64, 0], [0i64, 1]);
    for _ in 0..64 {
        let (u, v) = (comb(mu), comb(mv));
        if dot(u, u) > dot(v, v) * (1.0 + 1e-9) {
            std::mem::swap(&mut mu, &mut mv);
            continue;
        }
        let r = dot(u, v) / dot(u, u);
        if r.abs() <= 0.5 + 1e-9 {
            break;
        }
        let k = r.round() as i64;
        mv = [mv[0] - k * mu[0], mv[1] - k * mu[1]];
    }
    let (u, v) = (comb(mu), comb(mv));
    let (uu, vv) = (dot(u, u), dot(v, v));
    let tol = 1e-6 * uu;
    if dot(u, v) > tol {
        // An obtuse angle (120° rather than 60° on a hexagonal lattice), so that u + v is the
        // third shortest translation.
        mv = [-mv[0], -mv[1]];
    }
    let uv = dot(u, comb(mv));
    let equal = (vv - uu).abs() <= tol;
    let right = uv.abs() <= tol;
    let half = (uv.abs() - uu / 2.0).abs() <= tol;
    let shape = match (equal, right, half) {
        (true, true, _) => LatticeShape::Square,
        (true, false, true) => LatticeShape::Hexagonal,
        (false, true, _) => LatticeShape::Rectangular {
            axes: [[1, 0], [0, 1]],
        },
        // A rhombus: mirrors along its diagonals.
        (true, false, false) => LatticeShape::Rectangular {
            axes: [[1, 1], [1, -1]],
        },
        // Rows offset by half a step (u . v = -|u|^2 / 2): mirrors along u and u + 2v.
        (false, false, true) => LatticeShape::Rectangular {
            axes: [[1, 0], [1, 2]],
        },
        (false, false, false) => LatticeShape::Other,
    };
    (shape, [mu, mv])
}

/// The direction of a displacement `d` (in the basis `a1`, `a2`) in the lattice's own terms:
///
/// | Lattice | orthogonal | diagonal | oblique |
/// | --- | --- | --- | --- |
/// | square | along a shortest translation | halfway between two | anything else |
/// | hexagonal | along one of the three shortest translations | halfway between two (30° off) | anything else |
/// | rectangular or centred rectangular | along one of its two mirror axes | (none) | anything else |
///
/// An oblique lattice has no such axes, and gets no word (`None`). The square grid's words were
/// once used on every lattice, in its basis' coordinates, which on a hexagonal lattice (whose
/// basis vectors are 60° apart) called the shortest step `(1, -1)` diagonal.
pub fn direction(d: [i32; 2], a1: P, a2: P) -> Option<&'static str> {
    let (shape, m) = reduce_basis(a1, a2);
    // d = x u + y v: solve with the inverse of the (unimodular) integer matrix.
    let (dx, dy) = (d[0] as i64, d[1] as i64);
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det == 0 {
        return None;
    }
    let x = (dx * m[1][1] - dy * m[1][0]) / det;
    let y = (dy * m[0][0] - dx * m[0][1]) / det;
    match shape {
        LatticeShape::Square => Some(if x == 0 || y == 0 {
            "orthogonal"
        } else if x.abs() == y.abs() {
            "diagonal"
        } else {
            "oblique"
        }),
        LatticeShape::Hexagonal => Some(if x == 0 || y == 0 || x == y {
            "orthogonal"
        } else if x == 2 * y || y == 2 * x || x == -y {
            "diagonal"
        } else {
            "oblique"
        }),
        LatticeShape::Rectangular { axes } => {
            Some(if axes.iter().any(|[p, q]| x * q - y * p == 0) {
                "orthogonal"
            } else {
                "oblique"
            })
        }
        LatticeShape::Other => None,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Object {
    pub id: usize,
    pub name: String,
    pub class: ObjClass,
    /// Live cells now.
    pub size: usize,
    /// World cell ids, sorted.
    pub cells: Vec<u32>,
    /// World-space bounding box of the cells' polygons (spans the seam if the object wraps).
    pub bbox: [f64; 4],
    /// Lattice tilings: unwrapped lattice coordinates `[i0, j0, i1, j1]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lattice_bbox: Option<[i32; 4]>,
    pub centroid: [f64; 2],
    /// Canonical form shared by every copy of this object in any phase and orientation (empty for
    /// "other"): on lattice tilings the library's (a record's `canonical`).
    pub canonical: String,
    /// Objects with equal keys are counted together. On lattice tilings the first 8 hex digits of
    /// the library's content id (so a ship and its mirror image share one); on explicit patches a
    /// short hash of the canonical invariant.
    pub key: String,
    /// Lattice tilings: the id the library gives this object (as
    /// [`crate::pattern::record_from_object`] computes it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    /// False on explicit patches, where the canonical form is an invariant rather than exact.
    pub canonical_exact: bool,
    /// True when `name` is a well-known name (Life objects on the square grid).
    #[serde(skip)]
    pub known_name: bool,
    /// One period of generations as run alone, starting with the cells as found (unwrapped
    /// lattice coordinates; explicit patches: cell ids). Empty for "other".
    #[serde(skip)]
    pub phases: Vec<Phase>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CountRow {
    pub name: String,
    pub class: ObjClass,
    pub count: usize,
    pub key: String,
    /// Indices into `ObjectReport::objects`.
    pub objects: Vec<usize>,
}

impl CountRow {
    /// `blinker (p2)`, `glider (c/4)`, `block`.
    pub fn label(&self) -> String {
        match self.class.tag() {
            Some(t) if !self.name.contains(&t) => format!("{} ({t})", self.name),
            _ => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectReport {
    pub jetcarp_version: String,
    pub generation: u64,
    pub hash: String,
    pub tiling: String,
    pub dead_boundary: bool,
    pub rule: String,
    pub ca_id: u32,
    pub field: usize,
    pub live_cells: u64,
    pub max_period: u64,
    pub merge_distance: u32,
    /// Lattice symmetries used for canonical forms (1 = translation only).
    pub symmetries: usize,
    pub counts: Vec<CountRow>,
    pub objects: Vec<Object>,
    /// The tiling's lattice basis, which spaceship speeds are named in ([`ObjClass::speed_in`]).
    #[serde(skip)]
    pub lattice: Option<(P, P)>,
}

impl ObjectReport {
    /// `block x 41, blinker (p2) x 12, glider (c/4) x 3`.
    pub fn summary_line(&self) -> String {
        if self.counts.is_empty() {
            return "no objects".into();
        }
        self.counts
            .iter()
            .map(|r| format!("{} x {}", r.label(), r.count))
            .collect::<Vec<_>>()
            .join(", ")
    }
    /// Objects of each class (still life, oscillator, spaceship, other); puffers count as
    /// other here (see [`ObjectReport::puffers`]).
    pub fn class_totals(&self) -> [usize; 4] {
        let mut t = [0; 4];
        for o in &self.objects {
            t[match o.class {
                ObjClass::StillLife => 0,
                ObjClass::Oscillator { .. } => 1,
                ObjClass::Spaceship { .. } => 2,
                ObjClass::Puffer { .. } | ObjClass::Other { .. } => 3,
            }] += 1;
        }
        t
    }
    /// Puffers found.
    pub fn puffers(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| matches!(o.class, ObjClass::Puffer { .. }))
            .count()
    }
    /// One colour per cell: each object's cells in its class colour, everything else background.
    pub fn colors(&self, n: usize) -> Vec<Rgba> {
        let mut c = vec![crate::render::BACKGROUND; n];
        for o in &self.objects {
            for &id in &o.cells {
                if (id as usize) < n {
                    c[id as usize] = o.class.color();
                }
            }
        }
        c
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }
}

/// Cells from rows like `.o./..o/ooo` (`o`, `O`, `*` = state 1, digits = that state) placed with
/// the top-left at lattice column `u0`, row `j0` of tile type 0. For building test patterns.
pub fn place_rows(g: &LatticeGrid, rows: &str, u0: i32, j0: i32) -> Vec<[u32; 2]> {
    let mut out = Vec::new();
    for (r, line) in rows.split('/').enumerate() {
        for (c, ch) in line.chars().enumerate() {
            let s = match ch {
                'o' | 'O' | '*' => 1,
                d if d.is_ascii_digit() => d.to_digit(10).unwrap(),
                _ => 0,
            };
            if s != 0 {
                let j = j0 + r as i32;
                let i = g.row_start[j.rem_euclid(g.h as i32) as usize] + u0 + c as i32;
                if let Some(idx) = g.resolve(i, j, 0) {
                    out.push([idx as u32, s]);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Internals

/// A cell in unwrapped lattice coordinates `(i, j)` with tile type `k` and state `s` (explicit
/// patches: `i` is the cell id, `j` and `k` are 0). Ordered by row, then column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LCell {
    pub j: i32,
    pub i: i32,
    pub k: u32,
    pub s: u8,
}

/// One generation of an object: its live cells, sorted.
pub type Phase = Vec<LCell>;

enum Outcome {
    Repeat {
        period: u64,
        di: i32,
        dj: i32,
    },
    Died,
    Escaped,
    /// More live cells than the run's cap (only runs with a cap).
    Grows,
    Unsettled,
}

struct Evolution {
    /// Phase 0 (the input) and every later generation until the run stopped (not the repeat).
    phases: Vec<Phase>,
    outcome: Outcome,
}

struct Ctx {
    topo: Arc<Topology>,
    ca: CaSpec,
    table: Option<crate::table::RuleTable>,
    nb: Nb,
    max_period: u64,
    separate: bool,
    /// Pieces that die alone are retried with objects within this many steps.
    piece_distance: u32,
    /// Lattice: how far (in rows or row positions) one generation can reach.
    reach: i32,
    /// The tiling's symmetries as maps on lattice cells (the library's, from
    /// [`crate::pattern::canon::tiling_symmetries`]; only the identity for rule tables and
    /// explicit patches).
    syms: Arc<Vec<CellMap>>,
    /// The CA's rule, steps per generation, and whether orientation counts (for content ids).
    notation: String,
    steps_per_gen: u32,
    isotropic: bool,
    frames: Mutex<HashMap<(u32, u32), Arc<Topology>>>,
    frame_spec: TilingSpec,
    /// Lattices: the sparse runner for puffer tests (rules it supports).
    sparse: Option<puffer::Sparse>,
    /// Lattices, B/S and Generations rules at one step per generation: the bit-parallel
    /// stepper, for the many short runs that split compounds.
    bits: Option<Arc<crate::hunt::bits::Plan>>,
    /// Lattices: the neighbour offsets `(di, dj, k2)` of each tile type.
    offs: Vec<Vec<(i32, i32, u32)>>,
}

fn round8(v: i64) -> u32 {
    (v.max(8) as u32).div_ceil(8) * 8
}

impl Ctx {
    fn lattice(&self) -> Option<&LatticeGrid> {
        self.topo.lattice()
    }

    fn frame(&self, w: u32, h: u32) -> Result<Arc<Topology>, String> {
        let mut m = self.frames.lock().unwrap();
        if let Some(t) = m.get(&(w, h)) {
            return Ok(t.clone());
        }
        let mut spec = self.frame_spec.clone();
        spec.size = Some(SizeSpec::Rect { w, h });
        let t = Arc::new(Topology::build(&spec)?);
        m.insert((w, h), t.clone());
        Ok(t)
    }

    fn new_world(&self, topo: Arc<Topology>, cells: Vec<[u32; 2]>) -> Result<World, String> {
        let mut w = World::on_topology(topo, 1);
        if let Some(t) = &self.table {
            w.apply(Command::AddTable { table: t.clone() })?;
        }
        w.apply(Command::AddCa {
            ca: self.ca.clone(),
        })?;
        let id = w.cas[0].id;
        w.apply(Command::SetCells { id, cells })?;
        Ok(w)
    }

    /// Run `cells` alone for up to `gens` generations; with `stop`, stop at the first exact repeat.
    fn evolve(&self, cells: &[LCell], gens: u64, stop: bool) -> Result<Evolution, String> {
        self.evolve_capped(cells, gens, stop, usize::MAX)
    }

    /// [`Ctx::evolve`], stopping as `Grows` once more than `cap` cells are live.
    fn evolve_capped(
        &self,
        cells: &[LCell],
        gens: u64,
        stop: bool,
        cap: usize,
    ) -> Result<Evolution, String> {
        let mut start: Phase = cells.to_vec();
        start.sort_unstable();
        match self.lattice() {
            Some(_) => self.evolve_lattice(start, gens, stop, cap),
            None => self.evolve_explicit(start, gens, stop, cap),
        }
    }

    fn evolve_lattice(
        &self,
        start: Phase,
        gens: u64,
        stop: bool,
        cap: usize,
    ) -> Result<Evolution, String> {
        if let Some(plan) = &self.bits {
            return Ok(evolve_bits(plan, start, gens, stop, cap));
        }
        let e = self.reach;
        let big = (gens as i64 + 1) * e as i64 + 1;
        let mut margin = 2 * e as i64 + 2;
        let s = {
            let g = self.lattice().unwrap();
            g.a2[0] / g.a1[0]
        };
        let rs = |j: i64| -> i64 { -((j as f64 * s + 1e-9).floor() as i64) };
        'restart: loop {
            let jmin = start.iter().map(|c| c.j).min().unwrap() as i64;
            let jmax = start.iter().map(|c| c.j).max().unwrap() as i64;
            let j0 = margin - jmin;
            let h = round8(jmax - jmin + 1 + 2 * margin);
            let q: Vec<i64> = start
                .iter()
                .map(|c| c.i as i64 - rs(c.j as i64 + j0))
                .collect();
            let (qmin, qmax) = (*q.iter().min().unwrap(), *q.iter().max().unwrap());
            let i0 = margin - qmin;
            let w = round8(qmax - qmin + 1 + 2 * margin);
            let topo = self.frame(w, h)?;
            let g = topo.lattice().ok_or("frame is not a lattice")?;
            let mut idx = Vec::with_capacity(start.len());
            for c in &start {
                let jf = c.j as i64 + j0;
                let u = c.i as i64 + i0 - g.row_start[jf as usize] as i64;
                if u < 0 || u >= w as i64 {
                    return Err("object placement left the frame (internal error)".into());
                }
                idx.push([g.index(u as i32, jf as i32, c.k) as u32, c.s as u32]);
            }
            let mut world = self.new_world(topo.clone(), idx)?;
            let fi = world.cas[0].field;
            let mut phases = vec![start.clone()];
            let (w, h, e) = (w as i32, h as i32, e);
            let (i0, j0) = (i0 as i32, j0 as i32);
            for t in 1..=gens {
                world.apply(Command::Step { gens: 1 })?;
                let f = &world.fields[fi];
                let mut live = Vec::new();
                let mut near = false;
                for (x, &st) in f.state.iter().enumerate() {
                    if st != 0 {
                        let (u, jf, k) = g.decompose(x);
                        near |= u < e || u >= w - e || jf < e || jf >= h - e;
                        live.push(LCell {
                            j: jf - j0,
                            i: u + g.row_start[jf as usize] - i0,
                            k,
                            s: st,
                        });
                    }
                }
                if near {
                    if margin < big {
                        margin = big;
                        continue 'restart;
                    }
                    return Ok(Evolution {
                        phases,
                        outcome: Outcome::Escaped,
                    });
                }
                if live.is_empty() {
                    return Ok(Evolution {
                        phases,
                        outcome: Outcome::Died,
                    });
                }
                if live.len() > cap {
                    return Ok(Evolution {
                        phases,
                        outcome: Outcome::Grows,
                    });
                }
                if stop && let Some((di, dj)) = translated(&phases[0], &live) {
                    return Ok(Evolution {
                        phases,
                        outcome: Outcome::Repeat { period: t, di, dj },
                    });
                }
                phases.push(live);
            }
            return Ok(Evolution {
                phases,
                outcome: Outcome::Unsettled,
            });
        }
    }

    fn evolve_explicit(
        &self,
        start: Phase,
        gens: u64,
        stop: bool,
        cap: usize,
    ) -> Result<Evolution, String> {
        let cells = start.iter().map(|c| [c.i as u32, c.s as u32]).collect();
        let mut world = self.new_world(self.topo.clone(), cells)?;
        let fi = world.cas[0].field;
        let mut phases = vec![start];
        for t in 1..=gens {
            world.apply(Command::Step { gens: 1 })?;
            let live: Phase = world.fields[fi]
                .state
                .iter()
                .enumerate()
                .filter(|(_, s)| **s != 0)
                .map(|(x, &s)| LCell {
                    j: 0,
                    i: x as i32,
                    k: 0,
                    s,
                })
                .collect();
            if live.is_empty() {
                return Ok(Evolution {
                    phases,
                    outcome: Outcome::Died,
                });
            }
            if live.len() > cap {
                return Ok(Evolution {
                    phases,
                    outcome: Outcome::Grows,
                });
            }
            if stop && live == phases[0] {
                return Ok(Evolution {
                    phases,
                    outcome: Outcome::Repeat {
                        period: t,
                        di: 0,
                        dj: 0,
                    },
                });
            }
            phases.push(live);
        }
        Ok(Evolution {
            phases,
            outcome: Outcome::Unsettled,
        })
    }

    fn classify(&self, cells: &[LCell]) -> Result<(ObjClass, Vec<Phase>), String> {
        let ev = self.evolve(cells, self.max_period, true)?;
        let class = match ev.outcome {
            Outcome::Repeat {
                period: 1,
                di: 0,
                dj: 0,
            } => ObjClass::StillLife,
            Outcome::Repeat {
                period,
                di: 0,
                dj: 0,
            } => ObjClass::Oscillator { period },
            Outcome::Repeat { period, di, dj } => ObjClass::Spaceship {
                period,
                dx: di,
                dy: dj,
            },
            Outcome::Died => ObjClass::Other {
                reason: "dies".into(),
            },
            Outcome::Escaped | Outcome::Grows | Outcome::Unsettled => {
                if let Some(sp) = &self.sparse
                    && let Some(f) =
                        puffer::detect(sp, &ev.phases[0], self.max_period, puffer::TEST_CAP)
                {
                    return Ok((
                        ObjClass::Puffer {
                            period: f.period,
                            dx: f.d[0],
                            dy: f.d[1],
                        },
                        vec![ev.phases[0].clone()],
                    ));
                }
                let (a, b) = (ev.phases[0].len(), ev.phases.last().unwrap().len());
                let escaped = matches!(ev.outcome, Outcome::Escaped);
                ObjClass::Other {
                    reason: if escaped || b > 2 * a + 8 {
                        "grows"
                    } else {
                        "unsettled"
                    }
                    .into(),
                }
            }
        };
        Ok((class, ev.phases))
    }

    /// Objects in a group: its parts if they provably do not interact, else the whole group.
    fn analyse(&self, g: &Group) -> Result<Vec<Part>, String> {
        if let Some(reason) = &g.unsplittable {
            return Ok(vec![Part {
                class: ObjClass::Other {
                    reason: reason.clone(),
                },
                phases: vec![],
                members: (0..g.cells.len()).collect(),
            }]);
        }
        if self.separate && g.subs.len() > 1 {
            let mut parts = Vec::new();
            let mut l = 1u64;
            let mut ok = true;
            for sub in &g.subs {
                let cells: Vec<LCell> = sub.iter().map(|&m| g.cells[m]).collect();
                let (class, phases) = self.classify(&cells)?;
                match class.period() {
                    Some(p)
                        if !matches!(
                            class,
                            ObjClass::Spaceship { .. } | ObjClass::Puffer { .. }
                        ) =>
                    {
                        l = l / gcd(l, p) * p;
                    }
                    _ => ok = false,
                }
                parts.push(Part {
                    class,
                    phases,
                    members: sub.clone(),
                });
                if !ok || l > self.max_period {
                    ok = false;
                    break;
                }
            }
            if ok {
                let joint = self.evolve(&g.cells, l, false)?;
                let fits = joint.phases.len() as u64 == l + 1
                    && (1..=l as usize).all(|t| {
                        let mut u: Phase = parts
                            .iter()
                            .flat_map(|p| p.phases[t % p.phases.len()].iter().copied())
                            .collect();
                        u.sort_unstable();
                        u == joint.phases[t]
                    });
                if fits {
                    return Ok(parts
                        .into_iter()
                        .flat_map(|p| split_part(self, g, p))
                        .collect());
                }
            }
        }
        let (class, phases) = self.classify(&g.cells)?;
        Ok(split_part(
            self,
            g,
            Part {
                class,
                phases,
                members: (0..g.cells.len()).collect(),
            },
        ))
    }

    /// Minimum normalized form over phases and symmetries; returns (text, cell count, exact).
    fn canonical(&self, phases: &[Phase]) -> (String, usize, bool) {
        let Some(g) = self.lattice() else {
            return self.invariant(phases);
        };
        let mut best: Option<Phase> = None;
        for ph in phases {
            for m in self.syms.iter() {
                let (v, _) = crate::pattern::orient(ph, m);
                if best.as_ref().is_none_or(|b| v < *b) {
                    best = Some(v);
                }
            }
        }
        let b = best.unwrap_or_default();
        (picture(&b, g.ntypes), b.len(), true)
    }

    /// The library's content id and canonical form of a periodic object on a lattice tiling,
    /// built as [`crate::pattern::record_from_object`] builds a record, kept per tiling, rule and
    /// canonical form `text` (the same object has the same `text` in every phase and orientation).
    /// `None` on explicit patches, or when no record can be made (a rule the library cannot
    /// replay).
    fn content(&self, class: &ObjClass, phases: &[Phase], text: &str) -> Option<(String, String)> {
        use crate::pattern::{DiscoveredBy, PatternClass, Provenance, RecordParts};
        type Ids = HashMap<String, Option<(String, String)>>;
        static CACHE: OnceLock<Mutex<Ids>> = OnceLock::new();
        self.lattice()?;
        let (class, period, d) = match *class {
            ObjClass::StillLife => (PatternClass::StillLife, 1, [0, 0]),
            ObjClass::Oscillator { period } => (PatternClass::Oscillator, period, [0, 0]),
            ObjClass::Spaceship { period, dx, dy } => (PatternClass::Spaceship, period, [dx, dy]),
            ObjClass::Puffer { period, dx, dy } => (PatternClass::Puffer, period, [dx, dy]),
            ObjClass::Other { .. } => return None,
        };
        let key = format!(
            "{}|{:?}|{}|{}|{}|{text}",
            crate::pattern::tiling_key(&self.topo.spec),
            self.nb,
            self.steps_per_gen,
            self.notation,
            class.key()
        );
        let cache = CACHE.get_or_init(Default::default);
        if let Some(v) = cache.lock().unwrap().get(&key) {
            return v.clone();
        }
        let what = RecordParts {
            rule: self.notation.clone(),
            nb: self.nb,
            steps_per_gen: self.steps_per_gen,
            isotropic: self.isotropic,
            class,
            period,
            d,
            name: None,
        };
        let got = crate::pattern::build_record_with(
            &self.topo,
            Some(&self.syms),
            &what,
            phases,
            Provenance::default(),
            DiscoveredBy::Search,
        )
        .ok()
        .map(|r| (r.id, r.canonical));
        let mut m = cache.lock().unwrap();
        if m.len() > 100_000 {
            m.clear();
        }
        m.insert(key, got.clone());
        got
    }

    /// Explicit patches: sorted (state, degree, live neighbours) per cell, minimum over phases.
    fn invariant(&self, phases: &[Phase]) -> (String, usize, bool) {
        let mut best: Option<Vec<(u8, usize, usize)>> = None;
        for ph in phases {
            let set: HashSet<i32> = ph.iter().map(|c| c.i).collect();
            let mut v: Vec<(u8, usize, usize)> = ph
                .iter()
                .map(|c| {
                    let x = c.i as usize;
                    let mut n = 0;
                    self.topo
                        .for_each_neighbor(x, self.nb, |y| n += set.contains(&(y as i32)) as usize);
                    (c.s, self.topo.degree(x, self.nb), n)
                })
                .collect();
            v.sort_unstable();
            if best.as_ref().is_none_or(|b| v < *b) {
                best = Some(v);
            }
        }
        let b = best.unwrap_or_default();
        let text = b
            .iter()
            .map(|(s, d, n)| format!("{s}.{d}.{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        (text, b.len(), false)
    }
}

impl Ctx {
    /// The largest `|di|` and `|dj|` of a neighbour offset (lattices).
    fn offset_reach(&self) -> (i32, i32) {
        let g = self.lattice().unwrap();
        let offs = g.offsets[self.nb as usize].iter().flatten();
        let ri = offs.clone().map(|o| o.di.abs()).max().unwrap_or(1);
        let rj = offs.map(|o| o.dj.abs()).max().unwrap_or(1);
        (ri, rj)
    }

    fn explicit_neighbours(&self, x: usize, out: &mut Vec<usize>) {
        self.topo.for_each_neighbor(x, self.nb, |y| out.push(y));
    }
}

impl compound::Runner for Ctx {
    fn run(&self, cells: &[LCell], gens: u64) -> Option<Vec<Phase>> {
        if let Some(p) = &self.bits {
            return crate::hunt::separate::BitRunner(p.clone()).run(cells, gens);
        }
        let cap = 4 * cells.len() + 256;
        let ev = self.evolve_capped(cells, gens, false, cap).ok()?;
        match ev.outcome {
            Outcome::Escaped | Outcome::Grows => None,
            _ => {
                let mut v = ev.phases;
                v.resize(gens as usize + 1, Vec::new());
                Some(v)
            }
        }
    }
    fn touching(&self, c: &LCell, out: &mut Vec<(i32, i32, u32)>) {
        match self.lattice() {
            Some(g) => {
                for o in &g.offsets[self.nb as usize][c.k as usize] {
                    out.push((c.i + o.di, c.j + o.dj, o.k));
                }
            }
            None => self
                .topo
                .for_each_neighbor(c.i as usize, self.nb, |y| out.push((y as i32, 0, 0))),
        }
    }
    fn close(&self, a: &[LCell], b: &[LCell]) -> bool {
        match self.lattice() {
            Some(_) => compound::lattice_meet(a, b, self.offset_reach(), &self.offs),
            None => compound::explicit_close(a, b, &|x, out| self.explicit_neighbours(x, out)),
        }
    }
    fn distance(&self, a: &[LCell], b: &[LCell]) -> f64 {
        match self.lattice() {
            Some(_) => compound::lattice_distance(a, b),
            None => compound::explicit_distance(a, b, &|x, out| self.explicit_neighbours(x, out)),
        }
    }
}

/// [`Ctx::evolve_lattice`] on the bit-parallel stepper's unbounded plane: the same run, since a
/// frame run restarts with a margin the pattern cannot cross and so equals the infinite plane,
/// but without building a frame or stepping its empty cells. A fragment that drifts towards the
/// frame's edge made that run restart on a frame hundreds of unit cells across (seconds for one
/// 4-cell piece of a ship on `k2-017`); here it costs what its live cells cost. Never `Escaped`.
fn evolve_bits(
    plan: &Arc<crate::hunt::bits::Plan>,
    start: Phase,
    gens: u64,
    stop: bool,
    cap: usize,
) -> Evolution {
    let mut u = crate::hunt::bits::Universe::with_cells(plan.clone(), &start);
    let mut phases = vec![start];
    for t in 1..=gens {
        u.step();
        if u.is_empty() {
            return Evolution {
                phases,
                outcome: Outcome::Died,
            };
        }
        if u.population() as usize > cap {
            return Evolution {
                phases,
                outcome: Outcome::Grows,
            };
        }
        let live = u.cells();
        if stop && let Some((di, dj)) = translated(&phases[0], &live) {
            return Evolution {
                phases,
                outcome: Outcome::Repeat { period: t, di, dj },
            };
        }
        phases.push(live);
    }
    Evolution {
        phases,
        outcome: Outcome::Unsettled,
    }
}

/// The class of a periodic object from its period and displacement per period.
fn periodic_class(period: u64, d: [i32; 2]) -> ObjClass {
    match (period, d) {
        (1, [0, 0]) => ObjClass::StillLife,
        (p, [0, 0]) => ObjClass::Oscillator { period: p },
        (p, [dx, dy]) => ObjClass::Spaceship { period: p, dx, dy },
    }
}

/// A periodic part of a group split into the independent objects it is made of
/// ([`compound`]); anything else, or one that does not come apart, as it is.
fn split_part(ctx: &Ctx, g: &Group, p: Part) -> Vec<Part> {
    if !ctx.separate || p.phases.is_empty() {
        return vec![p];
    }
    let d = match p.class {
        ObjClass::StillLife | ObjClass::Oscillator { .. } => [0, 0],
        ObjClass::Spaceship { dx, dy, .. } => [dx, dy],
        _ => return vec![p],
    };
    let Some(parts) = compound::split(ctx, &p.phases, d) else {
        return vec![p];
    };
    // Each cell now goes to the part whose phase 0 holds it.
    let mut owner: HashMap<LCell, usize> = HashMap::new();
    for (k, cp) in parts.iter().enumerate().rev() {
        for c in &cp.phases[0] {
            owner.insert(*c, k);
        }
    }
    let mut members = vec![Vec::new(); parts.len()];
    for &m in &p.members {
        match owner.get(&g.cells[m]) {
            Some(&k) => members[k].push(m),
            None => return vec![p],
        }
    }
    if members.iter().any(|m| m.is_empty()) {
        return vec![p];
    }
    parts
        .into_iter()
        .zip(members)
        .map(|(cp, members)| Part {
            class: periodic_class(cp.period, cp.d),
            phases: cp.phases,
            members,
        })
        .collect()
}

/// A part as a group of its own cells (members `0..n`).
fn own_group(g: &Group, p: Part) -> (Group, Part) {
    let cells: Vec<LCell> = p.members.iter().map(|&m| g.cells[m]).collect();
    let ids: Vec<u32> = p.members.iter().map(|&m| g.ids[m]).collect();
    let n = cells.len();
    (
        Group {
            cells,
            ids,
            subs: vec![(0..n).collect()],
            unsplittable: g.unsplittable.clone(),
        },
        Part {
            class: p.class,
            phases: p.phases,
            members: (0..n).collect(),
        },
    )
}

/// The displacement per period of a periodic object (`None` for anything else).
fn periodic_d(c: &ObjClass) -> Option<[i32; 2]> {
    match c {
        ObjClass::StillLife | ObjClass::Oscillator { .. } => Some([0, 0]),
        ObjClass::Spaceship { dx, dy, .. } => Some([*dx, *dy]),
        _ => None,
    }
}

/// Phase `g` of a periodic object: its phases repeated, moved on by its displacement.
fn phase_at(p: &Part, d: [i32; 2], g: u64) -> Phase {
    let n = p.phases.len() as u64;
    compound::shift(&p.phases[(g % n) as usize], d, (g / n) as i32)
}

fn lcm(a: u64, b: u64) -> u64 {
    a / gcd(a, b).max(1) * b
}

/// Objects that are periodic alone but interact with a neighbour are one object when together
/// they are periodic: the two halves of a bound pair that lie apart in this phase (two gliders
/// whose sparks hold each other), or two oscillators that touch only in other phases. Two
/// objects interact when running them together differs, at some generation of two common
/// periods and eight more, from the union of their runs; pairs whose runs never come within two
/// neighbour steps of each other cannot interact and are not run. Interacting objects are
/// joined (union-find), and each set is run as one: when it is periodic it is one object (split
/// again into any parts that are independent of the rest), otherwise its objects stay as they
/// were (two that are about to collide).
fn join_interacting(ctx: &Ctx, items: Vec<(Group, Part)>) -> Result<Vec<(Group, Part)>, String> {
    if !ctx.separate {
        return Ok(items);
    }
    let n = items.len();
    let ds: Vec<Option<[i32; 2]>> = items
        .iter()
        .map(|(g, p)| {
            if g.unsplittable.is_some() || p.phases.is_empty() {
                None
            } else {
                periodic_d(&p.class)
            }
        })
        .collect();
    let horizon_of = |a: usize, b: usize| -> Option<u64> {
        let l = lcm(
            items[a].1.phases.len() as u64,
            items[b].1.phases.len() as u64,
        );
        (l <= ctx.max_period).then(|| compound::horizon(l))
    };
    // Candidate pairs: on lattices, envelopes that come within reach; on patches, envelopes
    // (every phase) whose neighbourhoods meet.
    let mut cand: Vec<(usize, usize)> = Vec::new();
    let live: Vec<usize> = (0..n).filter(|&k| ds[k].is_some()).collect();
    match ctx.lattice() {
        Some(_) => {
            let (ri, rj) = ctx.offset_reach();
            let h = compound::horizon(ctx.max_period);
            let env: Vec<[i32; 4]> = (0..n)
                .map(|k| {
                    let Some(d) = ds[k] else {
                        return [0; 4];
                    };
                    let p = &items[k].1;
                    let reps = h / p.phases.len() as u64 + 1;
                    let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
                    for ph in &p.phases {
                        for c in ph {
                            for r in [0, reps as i32] {
                                let (i, j) = (c.i + d[0] * r, c.j + d[1] * r);
                                b = [b[0].min(i), b[1].max(i), b[2].min(j), b[3].max(j)];
                            }
                        }
                    }
                    b
                })
                .collect();
            let mut order = live.clone();
            order.sort_by_key(|&k| (env[k][0], k));
            for (x, &a) in order.iter().enumerate() {
                for &b in &order[x + 1..] {
                    if env[b][0] - env[a][1] > 2 * ri {
                        break;
                    }
                    if env[a][2] - env[b][3] > 2 * rj || env[b][2] - env[a][3] > 2 * rj {
                        continue;
                    }
                    cand.push((a.min(b), a.max(b)));
                }
            }
        }
        None => {
            let mut near: HashMap<usize, Vec<usize>> = HashMap::new();
            for &k in &live {
                let mut set: HashSet<usize> = HashSet::new();
                for ph in &items[k].1.phases {
                    for c in ph {
                        set.insert(c.i as usize);
                        ctx.topo
                            .for_each_neighbor(c.i as usize, ctx.nb, |y| _ = set.insert(y));
                    }
                }
                for x in set {
                    near.entry(x).or_default().push(k);
                }
            }
            let mut seen = HashSet::new();
            for ks in near.values() {
                for (x, &a) in ks.iter().enumerate() {
                    for &b in &ks[x + 1..] {
                        if seen.insert((a.min(b), a.max(b))) {
                            cand.push((a.min(b), a.max(b)));
                        }
                    }
                }
            }
            cand.sort_unstable();
        }
    }
    let interacting: Vec<(usize, usize)> = cand
        .into_par_iter()
        .filter(|&(a, b)| {
            let Some(l) = horizon_of(a, b) else {
                return false;
            };
            let (pa, pb) = (&items[a].1, &items[b].1);
            let (da, db) = (ds[a].unwrap(), ds[b].unwrap());
            // Two still lifes are where they will always be: one look.
            let last = match (&pa.class, &pb.class) {
                (ObjClass::StillLife, ObjClass::StillLife) => 0,
                _ => l,
            };
            let close = (0..=last)
                .any(|g| compound::Runner::close(ctx, &phase_at(pa, da, g), &phase_at(pb, db, g)));
            if !close {
                return false;
            }
            let mut cells = pa.phases[0].clone();
            cells.extend(pb.phases[0].iter().copied());
            match compound::Runner::run(ctx, &cells, l) {
                None => true,
                Some(joint) => (0..=l).any(|g| {
                    !compound::union_is(
                        &phase_at(pa, da, g),
                        &phase_at(pb, db, g),
                        &joint[g as usize],
                    )
                }),
            }
        })
        .collect();
    if interacting.is_empty() {
        return Ok(items);
    }
    let mut uf = Uf((0..n as u32).collect());
    for &(a, b) in &interacting {
        uf.union(a, b);
    }
    let mut sets: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(a, b) in &interacting {
        for k in [a, b] {
            let r = uf.find(k);
            let s = sets.entry(r).or_default();
            if !s.contains(&k) {
                s.push(k);
            }
        }
    }
    // Each set run as one; periodic sets replace their members (at the first one's place). A set
    // that is not periodic may be missing a piece that dies alone, shed by its members: it is
    // tried again with the loose pieces within `piece_distance` steps (all, then one by one).
    let run_set = |set: &[usize]| -> Result<Option<Vec<(Group, Part)>>, String> {
        let mut g = Group {
            cells: vec![],
            ids: vec![],
            subs: vec![],
            unsplittable: None,
        };
        for &k in set {
            g.cells.extend(items[k].0.cells.iter().copied());
            g.ids.extend(items[k].0.ids.iter().copied());
        }
        g.subs = vec![(0..g.cells.len()).collect()];
        let (class, phases) = ctx.classify(&g.cells)?;
        if periodic_d(&class).is_none() {
            return Ok(None);
        }
        let whole = Part {
            class,
            phases,
            members: (0..g.cells.len()).collect(),
        };
        Ok(Some(
            split_part(ctx, &g, whole)
                .into_iter()
                .map(|p| own_group(&g, p))
                .collect(),
        ))
    };
    let loose: HashMap<usize, usize> = items
        .iter()
        .enumerate()
        .filter(|(_, (g, p))| g.unsplittable.is_none() && matches!(p.class, ObjClass::Other { .. }))
        .flat_map(|(k, (g, _))| g.ids.iter().map(move |&c| (c as usize, k)))
        .collect();
    let mut replace: HashMap<usize, Vec<(Group, Part)>> = HashMap::new();
    let mut gone: HashSet<usize> = HashSet::new();
    let mut keys: Vec<usize> = sets.keys().copied().collect();
    keys.sort_unstable();
    for r in keys {
        let mut set = sets[&r].clone();
        set.sort_unstable();
        let mut got = run_set(&set)?;
        if got.is_none() && !loose.is_empty() && ctx.piece_distance > 0 {
            // Loose pieces near the set, nearest first.
            let mut seen: HashSet<usize> = set
                .iter()
                .flat_map(|&k| items[k].0.ids.iter().map(|&c| c as usize))
                .collect();
            let mut frontier: Vec<usize> = seen.iter().copied().collect();
            frontier.sort_unstable();
            let mut near: Vec<usize> = Vec::new();
            for _ in 0..ctx.piece_distance {
                let mut next = Vec::new();
                for &x in &frontier {
                    ctx.topo.for_each_neighbor(x, ctx.nb, |y| {
                        if seen.insert(y) {
                            next.push(y);
                            if let Some(&k) = loose.get(&y)
                                && !near.contains(&k)
                                && !gone.contains(&k)
                            {
                                near.push(k);
                            }
                        }
                    });
                }
                frontier = next;
            }
            near.truncate(4);
            let mut tries: Vec<Vec<usize>> = Vec::new();
            if near.len() > 1 {
                tries.push(near.clone());
            }
            tries.extend(near.iter().map(|&k| vec![k]));
            for t in tries {
                let mut s = set.clone();
                s.extend(t);
                s.sort_unstable();
                if let Some(x) = run_set(&s)? {
                    set = s;
                    got = Some(x);
                    break;
                }
            }
        }
        if let Some(out) = got {
            gone.extend(set.iter().copied());
            replace.insert(set[0], out);
        }
    }
    let mut out = Vec::with_capacity(n);
    for (k, it) in items.into_iter().enumerate() {
        if let Some(r) = replace.remove(&k) {
            out.extend(r);
        }
        if !gone.contains(&k) {
            out.push(it);
        }
    }
    Ok(out)
}

/// `Some((di, dj))` if `b` is `a` shifted by a lattice vector (both sorted by row).
pub fn translated(a: &Phase, b: &Phase) -> Option<(i32, i32)> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (di, dj) = (b[0].i - a[0].i, b[0].j - a[0].j);
    a.iter()
        .zip(b)
        .all(|(x, y)| y.i == x.i + di && y.j == x.j + dj && y.k == x.k && y.s == x.s)
        .then_some((di, dj))
}

const STATE_CHARS: &[u8] = b".o23456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Rows of `.`/`o` (other states as digits or letters) joined by `/`; multi-type lattices list
/// `k@i,j` entries instead.
pub fn picture(v: &Phase, ntypes: u32) -> String {
    if ntypes == 1 {
        let w = v.iter().map(|c| c.i).max().unwrap_or(-1) + 1;
        let h = v.iter().map(|c| c.j).max().unwrap_or(-1) + 1;
        let mut rows = vec![vec![b'.'; w as usize]; h as usize];
        for c in v {
            rows[c.j as usize][c.i as usize] =
                STATE_CHARS[(c.s as usize).min(STATE_CHARS.len() - 1)];
        }
        rows.iter()
            .map(|r| String::from_utf8_lossy(r).trim_end_matches('.').to_string())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        v.iter()
            .map(|c| {
                if c.s == 1 {
                    format!("{}@{},{}", c.k, c.i, c.j)
                } else {
                    format!("{}@{},{}:{}", c.k, c.i, c.j, c.s)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn short_key(text: &str) -> String {
    let mut h = Fnv::default();
    h.bytes(text.as_bytes());
    format!("{:08x}", h.0 >> 32)
}

struct Group {
    cells: Vec<LCell>,
    ids: Vec<u32>,
    /// Touching parts (indices into `cells`).
    subs: Vec<Vec<usize>>,
    /// Set when the group cannot be isolated (it wraps around the torus).
    unsplittable: Option<String>,
}

/// A merged cluster: (first group, its groups, the merged group, its object).
type Merged = (usize, Vec<usize>, Group, Part);

struct Part {
    class: ObjClass,
    phases: Vec<Phase>,
    members: Vec<usize>,
}

struct Uf(Vec<u32>);
impl Uf {
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

/// Lattice maps (integer 2x2, det ±1) that preserve the offset set of a one-tile lattice,
/// identity first (8 on the square grid with touching neighbours, 12 on the hexagon grid).
pub fn symmetries(g: &LatticeGrid, nb: Nb) -> Vec<[i32; 4]> {
    let o: HashSet<(i32, i32)> = g.offsets[nb as usize][0]
        .iter()
        .map(|o| (o.di, o.dj))
        .collect();
    let mut out = vec![[1, 0, 0, 1]];
    for a in -1i32..=1 {
        for b in -1..=1 {
            for c in -1..=1 {
                for d in -1..=1 {
                    let m = [a, b, c, d];
                    if m == [1, 0, 0, 1] || (a * d - b * c).abs() != 1 {
                        continue;
                    }
                    if o.iter()
                        .all(|&(x, y)| o.contains(&(a * x + b * y, c * x + d * y)))
                    {
                        out.push(m);
                    }
                }
            }
        }
    }
    out
}

/// Known Life objects, defined here from their well-known cell coordinates.
pub const KNOWN: &[(&str, &str, u64, bool)] = &[
    ("block", "oo/oo", 1, false),
    ("beehive", ".oo./o..o/.oo.", 1, false),
    ("loaf", ".oo./o..o/.o.o/..o.", 1, false),
    ("boat", "oo./o.o/.o.", 1, false),
    ("ship", "oo./o.o/.oo", 1, false),
    ("tub", ".o./o.o/.o.", 1, false),
    ("pond", ".oo./o..o/o..o/.oo.", 1, false),
    ("barge", ".o../o.o./.o.o/..o.", 1, false),
    ("long boat", "oo../o.o./.o.o/..o.", 1, false),
    ("long barge", ".o.../o.o../.o.o./..o.o/...o.", 1, false),
    ("mango", ".oo../o..o./.o..o/..oo.", 1, false),
    ("eater 1", "oo../o.o./..o./..oo", 1, false),
    ("snake", "oo.o/o.oo", 1, false),
    ("aircraft carrier", "oo../o..o/..oo", 1, false),
    ("blinker", "ooo", 2, false),
    ("toad", ".ooo/ooo.", 2, false),
    ("beacon", "oo../oo../..oo/..oo", 2, false),
    (
        "pulsar",
        "..ooo...ooo../............./o....o.o....o/o....o.o....o/o....o.o....o/..ooo...ooo../\
         ............./..ooo...ooo../o....o.o....o/o....o.o....o/o....o.o....o/............./\
         ..ooo...ooo..",
        3,
        false,
    ),
    (
        "pentadecathlon",
        "..o....o../oo.oooo.oo/..o....o..",
        15,
        false,
    ),
    ("glider", ".o./..o/ooo", 4, true),
    ("LWSS", ".o..o/o..../o...o/oooo.", 4, true),
    ("MWSS", "...o../.o...o/o...../o....o/ooooo.", 4, true),
    ("HWSS", "...oo../.o....o/o....../o.....o/oooooo.", 4, true),
];

/// Cells (state 1, tile type 0) from rows like `.o./..o/ooo`.
pub fn rows_to_cells(rows: &str) -> Vec<LCell> {
    let mut v = Vec::new();
    for (r, line) in rows.split('/').enumerate() {
        for (c, ch) in line.chars().enumerate() {
            if ch == 'o' {
                v.push(LCell {
                    j: r as i32,
                    i: c as i32,
                    k: 0,
                    s: 1,
                });
            }
        }
    }
    v
}

type NameMap = Arc<HashMap<String, String>>;

/// Names for this rule on the square grid: each known pattern is run with the rule, and named
/// only if it behaves as it does in Life (same class and period).
fn known_names(ctx: &Ctx, key: String) -> NameMap {
    static CACHE: OnceLock<Mutex<HashMap<String, NameMap>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(m) = cache.lock().unwrap().get(&key) {
        return m.clone();
    }
    let found: Vec<(String, String)> = KNOWN
        .par_iter()
        .filter_map(|&(name, rows, period, ship)| {
            let (class, phases) = ctx.classify(&rows_to_cells(rows)).ok()?;
            let ok = match class {
                ObjClass::StillLife => period == 1 && !ship,
                ObjClass::Oscillator { period: p } => p == period && !ship,
                ObjClass::Spaceship { period: p, .. } => p == period && ship,
                ObjClass::Puffer { .. } | ObjClass::Other { .. } => false,
            };
            ok.then(|| (ctx.canonical(&phases).0, name.to_string()))
        })
        .collect();
    let m: NameMap = Arc::new(found.into_iter().collect());
    cache.lock().unwrap().insert(key, m.clone());
    m
}

/// Can objects of this rule be isolated? `Ok(true)` for isotropic rules (B/S, Generations,
/// WireWorld, priority tables), `Ok(false)` for rule tables (no rotation or reflection), `Err` otherwise.
pub fn supported(rule: &Rule) -> Result<bool, String> {
    match rule {
        Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } if birth & 1 == 1 => {
            Err("B0 rules have no empty background, so objects cannot be isolated".into())
        }
        Rule::Priority(p) if (0..=24).any(|d| p.ignites(d)) => Err(
            "this priority rule turns quiet cells on with no live neighbours, so objects cannot be isolated"
                .into(),
        ),
        Rule::LifeLike { .. } | Rule::Generations { .. } | Rule::WireWorld | Rule::Priority(_) => {
            Ok(true)
        }
        Rule::Table(_) | Rule::SpectreGlider => Ok(false),
        _ => Err(format!(
            "object detection needs a B/S, Generations, WireWorld, or table rule, not {}",
            rule.notation()
        )),
    }
}

/// A [`compound::Runner`] for objects of a world's CA (the first that qualifies when `ca_id` is
/// `None`): its rule and neighbourhood on the world's tiling, run alone on dead-edged frames
/// (lattices) or on the world's own patch (explicit patches, whose cells are ids).
pub fn runner_for(
    world: &World,
    ca_id: Option<u32>,
) -> Result<Box<dyn compound::Runner + Send + Sync>, String> {
    let ca = match ca_id {
        Some(id) => world.ca(id).ok_or_else(|| format!("no CA with id {id}"))?,
        None => world
            .cas
            .iter()
            .find(|c| supported(&c.rule).is_ok())
            .ok_or("no CA with a B/S, Generations, WireWorld, or table rule")?,
    };
    Ok(Box::new(make_ctx(world, ca, 64, true, 8)?))
}

fn make_ctx(
    world: &World,
    ca: &crate::world::CaInstance,
    max_period: u64,
    separate: bool,
    piece_distance: u32,
) -> Result<Ctx, String> {
    let isotropic = supported(&ca.rule)?;
    let topo = world.topo.clone();
    let nb = ca.nb;
    let mut spec = CaSpec::new(&ca.rule.notation()).seed(SeedSpec::Empty);
    spec.neighborhood = Some(nb);
    spec.steps_per_gen = ca.steps_per_gen;
    let (reach, syms) = match topo.lattice() {
        Some(g) => {
            let maxoff = g.offsets[nb as usize]
                .iter()
                .flatten()
                .map(|o| o.di.abs().max(o.dj.abs()))
                .max()
                .unwrap_or(1)
                .max(1);
            let shear = (g.a2[0] / g.a1[0]).abs();
            let reach = maxoff + (maxoff as f64 * shear).ceil() as i32 + 1;
            let syms = crate::pattern::canon::tiling_symmetries(&topo.spec, nb, isotropic)
                .unwrap_or_else(|_| {
                    Arc::new(crate::pattern::canonical_symmetries(&topo, nb, isotropic))
                });
            (reach, syms)
        }
        None => (1, Arc::new(vec![CellMap::identity(1)])),
    };
    let mut frame_spec = topo.spec.clone();
    frame_spec.boundary = Boundary::Dead;
    Ok(Ctx {
        topo: topo.clone(),
        ca: spec,
        table: ca.table.as_ref().map(|t| t.table.clone()),
        nb,
        max_period,
        separate,
        piece_distance,
        reach,
        syms,
        notation: ca.notation(),
        steps_per_gen: ca.steps_per_gen,
        isotropic,
        frames: Mutex::new(HashMap::new()),
        frame_spec,
        sparse: topo
            .lattice()
            .and_then(|g| puffer::Sparse::new(&ca.rule, g, nb)),
        bits: topo
            .lattice()
            .filter(|_| ca.steps_per_gen == 1 && ca.table.is_none())
            .and_then(|g| crate::hunt::bits::Plan::new(&ca.rule, g, nb).ok())
            .map(Arc::new),
        offs: topo
            .lattice()
            .map(|g| {
                g.offsets[nb as usize]
                    .iter()
                    .map(|v| v.iter().map(|o| (o.di, o.dj, o.k)).collect())
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Find and classify the objects in a world's field.
pub fn find_objects(world: &World, opts: &ObjectOptions) -> Result<ObjectReport, String> {
    let ca = match opts.ca {
        Some(id) => world.ca(id).ok_or_else(|| format!("no CA with id {id}"))?,
        None => world
            .cas
            .iter()
            .filter(|c| c.enabled)
            .chain(world.cas.iter())
            .find(|c| supported(&c.rule).is_ok())
            .ok_or("no CA with a B/S, Generations, WireWorld, or table rule")?,
    };
    let topo = world.topo.clone();
    let field = &world.fields[ca.field];
    if field.state.is_empty() {
        return Err("that CA has no state field".into());
    }
    let max_period = opts.max_period.clamp(1, 100_000);
    let d = opts.merge_distance.clamp(1, 4);
    let ctx = make_ctx(
        world,
        ca,
        max_period,
        opts.separate,
        opts.piece_distance.min(16),
    )?;
    let nb = ctx.nb;

    let groups = build_groups(&ctx, &field.state, d);
    let parts: Result<Vec<Vec<Part>>, String> = groups.par_iter().map(|g| ctx.analyse(g)).collect();
    let parts = parts?;
    let merged = merge_pieces(
        &ctx,
        &groups,
        &parts,
        &field.state,
        opts.piece_distance.min(16),
    )?;
    let named = topo.family.id == "square" && ctx.syms.len() == 8;
    let names = if named {
        Some(known_names(
            &ctx,
            format!(
                "{}|{:?}|{}|{}",
                ca.rule.notation(),
                nb,
                ca.steps_per_gen,
                max_period
            ),
        ))
    } else {
        None
    };
    let mut absorbed = vec![false; groups.len()];
    for (_, members, _, _) in &merged {
        for &m in members {
            absorbed[m] = true;
        }
    }
    let mut merged_at: HashMap<usize, (Group, Part)> = merged
        .into_iter()
        .map(|(head, _, g, p)| (head, (g, p)))
        .collect();
    // Every object as a group of its own cells.
    let mut items: Vec<(Group, Part)> = Vec::new();
    for (gi, ps) in parts.into_iter().enumerate() {
        if let Some((g, p)) = merged_at.remove(&gi) {
            for q in split_part(&ctx, &g, p) {
                items.push(own_group(&g, q));
            }
        }
        if absorbed[gi] {
            continue;
        }
        for p in ps {
            items.push(own_group(&groups[gi], p));
        }
    }
    let items = join_interacting(&ctx, items)?;
    let mut objects = Vec::new();
    for (g, p) in items {
        objects.push(make_object(&ctx, objects.len(), p, &g, names.as_deref()));
    }
    let mut rows: Vec<CountRow> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for o in &objects {
        let k = by_key.entry(o.key.clone()).or_insert_with(|| {
            rows.push(CountRow {
                name: o.name.clone(),
                class: o.class.clone(),
                count: 0,
                key: o.key.clone(),
                objects: vec![],
            });
            rows.len() - 1
        });
        rows[*k].count += 1;
        rows[*k].objects.push(o.id);
    }
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    Ok(ObjectReport {
        jetcarp_version: crate::VERSION.into(),
        generation: world.generation,
        hash: world.hash_hex(),
        tiling: topo.describe(),
        dead_boundary: topo.has_boundary(),
        rule: ca.notation(),
        ca_id: ca.id,
        field: ca.field,
        live_cells: field.population(),
        max_period,
        merge_distance: d,
        symmetries: ctx.syms.len(),
        counts: rows,
        objects,
        lattice: topo.lattice().map(|g| (g.a1, g.a2)),
    })
}

fn make_object(
    ctx: &Ctx,
    id: usize,
    p: Part,
    g: &Group,
    names: Option<&HashMap<String, String>>,
) -> Object {
    let topo = &ctx.topo;
    let mut ids: Vec<u32> = p.members.iter().map(|&m| g.ids[m]).collect();
    ids.sort_unstable();
    let cells: Vec<LCell> = p.members.iter().map(|&m| g.cells[m]).collect();
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &c in &ids {
        let b = bbox(&topo.polygon(c as usize));
        bb = [
            bb[0].min(b[0]),
            bb[1].min(b[1]),
            bb[2].max(b[2]),
            bb[3].max(b[3]),
        ];
    }
    let (lattice_bbox, centroid) = match ctx.lattice() {
        Some(gr) => {
            let tc: Vec<P> = gr
                .types
                .iter()
                .map(|(m, s)| m.apply(centroid(&topo.shapes[*s])))
                .collect();
            let mut sum = [0.0, 0.0];
            for c in &cells {
                sum[0] += c.i as f64 * gr.a1[0] + c.j as f64 * gr.a2[0] + tc[c.k as usize][0];
                sum[1] += c.i as f64 * gr.a1[1] + c.j as f64 * gr.a2[1] + tc[c.k as usize][1];
            }
            let n = cells.len().max(1) as f64;
            (
                Some([
                    cells.iter().map(|c| c.i).min().unwrap_or(0),
                    cells.iter().map(|c| c.j).min().unwrap_or(0),
                    cells.iter().map(|c| c.i).max().unwrap_or(0),
                    cells.iter().map(|c| c.j).max().unwrap_or(0),
                ]),
                [sum[0] / n, sum[1] / n],
            )
        }
        None => {
            let mut sum = [0.0, 0.0];
            for &c in &ids {
                let q = topo.centroid(c as usize);
                sum[0] += q[0];
                sum[1] += q[1];
            }
            let n = ids.len().max(1) as f64;
            (None, [sum[0] / n, sum[1] / n])
        }
    };
    let mut known_name = false;
    let mut content_id = None;
    let (canonical, exact, key, name) = match &p.class {
        ObjClass::Other { reason } => (
            String::new(),
            false,
            format!("other-{reason}"),
            format!("other ({reason})"),
        ),
        class => {
            let (text, n, exact) = ctx.canonical(&p.phases);
            // On lattices the library's content id and canonical form, so copies in every
            // orientation count together and the label names the library's record.
            let (key, canonical) = match ctx.content(class, &p.phases, &text) {
                Some((id, form)) => {
                    let key = id[..8].to_string();
                    content_id = Some(id);
                    (key, form)
                }
                None => (short_key(&text), text.clone()),
            };
            let name = names
                .and_then(|m| m.get(&text).cloned())
                .inspect(|_| known_name = true)
                .unwrap_or_else(|| {
                    let k6 = &key[..6];
                    match class {
                        ObjClass::StillLife => format!("still life {n}c #{k6}"),
                        ObjClass::Oscillator { period } => {
                            format!("p{period} oscillator {n}c #{k6}")
                        }
                        ObjClass::Puffer { .. } => {
                            format!("{} puffer {n}c #{k6}", class.tag().unwrap_or_default())
                        }
                        _ => format!("{} ship {n}c #{k6}", class.tag().unwrap_or_default()),
                    }
                });
            (canonical, exact, key, name)
        }
    };
    Object {
        id,
        name,
        class: p.class.clone(),
        size: ids.len(),
        cells: ids,
        bbox: bb,
        lattice_bbox,
        centroid,
        canonical,
        key,
        content_id,
        canonical_exact: exact,
        known_name,
        phases: if matches!(p.class, ObjClass::Other { .. }) {
            vec![]
        } else {
            p.phases
        },
    }
}

/// Live cells in a loose cluster above which it is not tried with its periodic neighbours.
const LOOSE_CELLS: usize = 256;

/// Pieces that die or never repeat on their own may be parts of one object whose phases fall
/// apart (some pentadecathlon phases do), or the far member of a bound pair whose two halves lie
/// apart in this phase (a ship and the spark-supported partner it tows). Merge such pieces lying
/// within a few steps of each other, and failing that each cluster of them with its nearest
/// periodic neighbours (one at a time, then all), and keep the first merge that is periodic. A
/// merge that is really several objects comes apart again in [`split_part`]. Returns (first
/// group, groups, group, part).
fn merge_pieces(
    ctx: &Ctx,
    groups: &[Group],
    parts: &[Vec<Part>],
    state: &[u8],
    reach: u32,
) -> Result<Vec<Merged>, String> {
    let loose: Vec<usize> = (0..groups.len())
        .filter(|&g| {
            groups[g].unsplittable.is_none()
                && parts[g].len() == 1
                && matches!(parts[g][0].class, ObjClass::Other { .. })
        })
        .collect();
    if loose.is_empty() || reach == 0 {
        return Ok(vec![]);
    }
    let periodic = |g: usize| {
        groups[g].unsplittable.is_none()
            && parts[g].iter().all(|p| {
                matches!(
                    p.class,
                    ObjClass::StillLife | ObjClass::Oscillator { .. } | ObjClass::Spaceship { .. }
                )
            })
    };
    // Owners: loose groups by their index in `loose`, periodic ones by group.
    let mut owner: HashMap<usize, Result<usize, usize>> = HashMap::new();
    for (k, &g) in loose.iter().enumerate() {
        for &c in &groups[g].ids {
            owner.insert(c as usize, Ok(k));
        }
    }
    for g in (0..groups.len()).filter(|&g| periodic(g)) {
        for &c in &groups[g].ids {
            owner.insert(c as usize, Err(g));
        }
    }
    let mut uf = Uf((0..loose.len() as u32).collect());
    // Periodic groups met from each loose group: (steps, group).
    let mut near: Vec<Vec<(u32, usize)>> = vec![Vec::new(); loose.len()];
    for (k, &g) in loose.iter().enumerate() {
        let mut seen: HashSet<usize> = groups[g].ids.iter().map(|&c| c as usize).collect();
        let mut frontier: Vec<usize> = seen.iter().copied().collect();
        frontier.sort_unstable();
        for step in 1..=reach {
            let mut next = Vec::new();
            for &x in &frontier {
                ctx.topo.for_each_neighbor(x, ctx.nb, |y| {
                    if seen.insert(y) {
                        next.push(y);
                        match owner.get(&y) {
                            Some(Ok(o)) => uf.union(k, *o),
                            Some(Err(p)) if !near[k].iter().any(|n| n.1 == *p) => {
                                near[k].push((step, *p))
                            }
                            _ => {}
                        }
                    }
                });
            }
            frontier = next;
        }
    }
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut at: HashMap<usize, usize> = HashMap::new();
    for (k, &g) in loose.iter().enumerate() {
        let r = uf.find(k);
        let c = *at.entry(r).or_insert_with(|| {
            clusters.push(vec![]);
            members.push(vec![]);
            clusters.len() - 1
        });
        clusters[c].push(g);
        members[c].push(k);
    }
    let jobs: Vec<(Vec<usize>, Vec<usize>)> = clusters
        .into_iter()
        .zip(members)
        .map(|(c, ks)| {
            let mut cand: Vec<(u32, usize)> = ks.iter().flat_map(|&k| near[k].clone()).collect();
            cand.sort_unstable();
            let mut seen = HashSet::new();
            cand.retain(|x| seen.insert(x.1));
            (c, cand.into_iter().take(3).map(|x| x.1).collect())
        })
        .collect();
    let tried: Vec<Result<Option<Merged>, String>> = jobs
        .into_par_iter()
        .map(|(c, cand)| {
            let size: usize = c.iter().map(|&g| groups[g].ids.len()).sum();
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
                let mut ids: Vec<u32> = set
                    .iter()
                    .flat_map(|&g| groups[g].ids.iter().copied())
                    .collect();
                ids.sort_unstable();
                let (cells, bad) = embed(ctx, &ids, state, reach);
                if bad.is_some() {
                    continue;
                }
                let g = Group {
                    subs: vec![(0..ids.len()).collect()],
                    cells,
                    ids,
                    unsplittable: None,
                };
                let (class, phases) = ctx.classify(&g.cells)?;
                if matches!(class, ObjClass::Other { .. } | ObjClass::Puffer { .. }) {
                    continue;
                }
                let p = Part {
                    class,
                    phases,
                    members: (0..g.cells.len()).collect(),
                };
                return Ok(Some((set[0], set, g, p)));
            }
            Ok(None)
        })
        .collect();
    // A periodic group goes to the first merge that takes it.
    let mut taken: HashSet<usize> = HashSet::new();
    let mut out = Vec::new();
    for m in tried {
        if let Some(m) = m? {
            if m.1.iter().any(|g| taken.contains(g)) {
                continue;
            }
            taken.extend(m.1.iter().copied());
            out.push(m);
        }
    }
    Ok(out)
}

/// Group live cells: touching parts, merged when within `d` neighbour steps of each other.
fn build_groups(ctx: &Ctx, state: &[u8], d: u32) -> Vec<Group> {
    let topo = &ctx.topo;
    let nb = ctx.nb;
    let live: Vec<usize> = (0..state.len()).filter(|&i| state[i] != 0).collect();
    let mut pos = vec![u32::MAX; state.len()];
    for (k, &c) in live.iter().enumerate() {
        pos[c] = k as u32;
    }
    let mut touch = Uf((0..live.len() as u32).collect());
    let mut near = Uf((0..live.len() as u32).collect());
    for (k, &c) in live.iter().enumerate() {
        let mut frontier = vec![c];
        let mut seen = vec![c];
        for depth in 1..=d {
            let mut next = Vec::new();
            for &x in &frontier {
                topo.for_each_neighbor(x, nb, |y| {
                    if !seen.contains(&y) {
                        seen.push(y);
                        next.push(y);
                        if pos[y] != u32::MAX {
                            near.union(k, pos[y] as usize);
                            if depth == 1 {
                                touch.union(k, pos[y] as usize);
                            }
                        }
                    }
                });
            }
            frontier = next;
        }
    }
    // Groups in order of their first (lowest) cell.
    let mut gid: HashMap<usize, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for k in 0..live.len() {
        let r = near.find(k);
        let g = *gid.entry(r).or_insert_with(|| {
            members.push(vec![]);
            members.len() - 1
        });
        members[g].push(k);
    }
    members
        .into_iter()
        .map(|m| {
            let mut subs_of: HashMap<usize, usize> = HashMap::new();
            let mut subs: Vec<Vec<usize>> = Vec::new();
            for (local, &k) in m.iter().enumerate() {
                let r = touch.find(k);
                let s = *subs_of.entry(r).or_insert_with(|| {
                    subs.push(vec![]);
                    subs.len() - 1
                });
                subs[s].push(local);
            }
            let ids: Vec<u32> = m.iter().map(|&k| live[k] as u32).collect();
            let (cells, unsplittable) = embed(ctx, &ids, state, d);
            Group {
                cells,
                ids,
                subs,
                unsplittable,
            }
        })
        .collect()
}

/// Unwrapped lattice coordinates for a group's cells (explicit patches: cell ids).
fn embed(ctx: &Ctx, ids: &[u32], state: &[u8], d: u32) -> (Vec<LCell>, Option<String>) {
    let Some(g) = ctx.lattice() else {
        let cells = ids
            .iter()
            .map(|&x| LCell {
                j: 0,
                i: x as i32,
                k: 0,
                s: state[x as usize],
            })
            .collect();
        return (cells, None);
    };
    let nb = ctx.nb as usize;
    let index: HashMap<usize, usize> = ids
        .iter()
        .enumerate()
        .map(|(k, &x)| (x as usize, k))
        .collect();
    let mut coords: Vec<Option<(i32, i32, u32)>> = vec![None; ids.len()];
    let (u, j, k) = g.decompose(ids[0] as usize);
    coords[0] = Some((u + g.row_start[j as usize], j, k));
    let mut queue = VecDeque::from([0usize]);
    let mut bad = false;
    while let Some(a) = queue.pop_front() {
        let (ci, cj, ck) = coords[a].unwrap();
        let mut frontier = vec![(ids[a] as usize, ci, cj, ck)];
        let mut visited = HashSet::from([ids[a] as usize]);
        for depth in 1..=d {
            let mut next = Vec::new();
            for &(x, i, j, k) in &frontier {
                for (slot, o) in g.offsets[nb][k as usize].iter().enumerate() {
                    let Some(y) = g.neighbor(x, ctx.nb, slot) else {
                        continue;
                    };
                    if !visited.insert(y) && depth > 1 {
                        continue;
                    }
                    let c = (i + o.di, j + o.dj, o.k);
                    if let Some(&b) = index.get(&y) {
                        match coords[b] {
                            None => {
                                coords[b] = Some(c);
                                queue.push_back(b);
                            }
                            Some(old) if depth == 1 && old != c => bad = true,
                            _ => {}
                        }
                    }
                    if depth < d {
                        next.push((y, c.0, c.1, c.2));
                    }
                }
            }
            frontier = next;
        }
    }
    let cells: Vec<LCell> = ids
        .iter()
        .zip(&coords)
        .map(|(&x, c)| {
            let (i, j, k) = c.unwrap_or((0, 0, 0));
            LCell {
                j,
                i,
                k,
                s: state[x as usize],
            }
        })
        .collect();
    let span_i =
        cells.iter().map(|c| c.i).max().unwrap() - cells.iter().map(|c| c.i).min().unwrap();
    let span_j =
        cells.iter().map(|c| c.j).max().unwrap() - cells.iter().map(|c| c.j).min().unwrap();
    let e = 2 * ctx.reach;
    if bad
        || coords.iter().any(|c| c.is_none())
        || (g.wrap && (span_j + e >= g.h as i32 || span_i + e >= g.w as i32))
    {
        return (cells, Some("spans the grid".into()));
    }
    (cells, None)
}
