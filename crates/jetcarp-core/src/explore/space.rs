//! The search space: which tilings, rule families and rules exist, and how to sample and mutate
//! them.

use crate::aperiodic;
use crate::catalog;
use crate::evolve::priority::{PriorityRule, Row};
use crate::jaca::JacaRule;
use crate::lenia::LeniaRule;
use crate::ltl::{Hood, LtlRule};
use crate::rng::Rng;
use crate::rules::{HexTMode, Rule};
use crate::topology::{Boundary, Csr, Nb, SizeSpec, TilingSpec, Topology};
use crate::turmite::{Entry, Mite};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The rule families the search knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fam {
    LifeLike,
    Generations,
    Cyclic,
    Rps,
    GreenbergHastings,
    Hodgepodge,
    WireWorld,
    Ltl,
    Jaca,
    Priority,
    ForestFire,
    Turmite,
    Layered,
    Wolfram,
    HexWolfram,
    HexTri,
    Margolus,
    Lenia,
    Sandpile,
    Table,
    VonNeumann,
    SpectreGlider,
}

pub const ALL_FAMS: [Fam; 22] = [
    Fam::LifeLike,
    Fam::Generations,
    Fam::Cyclic,
    Fam::Rps,
    Fam::GreenbergHastings,
    Fam::Hodgepodge,
    Fam::WireWorld,
    Fam::Ltl,
    Fam::Jaca,
    Fam::Priority,
    Fam::ForestFire,
    Fam::Turmite,
    Fam::Layered,
    Fam::Wolfram,
    Fam::HexWolfram,
    Fam::HexTri,
    Fam::Margolus,
    Fam::Lenia,
    Fam::Sandpile,
    Fam::Table,
    Fam::VonNeumann,
    Fam::SpectreGlider,
];

impl Fam {
    pub fn name(self) -> &'static str {
        match self {
            Fam::LifeLike => "life_like",
            Fam::Generations => "generations",
            Fam::Cyclic => "cyclic",
            Fam::Rps => "rps",
            Fam::GreenbergHastings => "greenberg_hastings",
            Fam::Hodgepodge => "hodgepodge",
            Fam::WireWorld => "wireworld",
            Fam::Ltl => "ltl",
            Fam::Jaca => "jaca",
            Fam::Priority => "priority",
            Fam::ForestFire => "forest_fire",
            Fam::Turmite => "turmite",
            Fam::Layered => "layered",
            Fam::Wolfram => "wolfram",
            Fam::HexWolfram => "hex_wolfram",
            Fam::HexTri => "hex_tri",
            Fam::Margolus => "margolus",
            Fam::Lenia => "lenia",
            Fam::Sandpile => "sandpile",
            Fam::Table => "table",
            Fam::VonNeumann => "von_neumann",
            Fam::SpectreGlider => "spectre_glider",
        }
    }

    pub fn of(rule: &Rule) -> Fam {
        match rule {
            Rule::LifeLike { .. } => Fam::LifeLike,
            Rule::Generations { .. } => Fam::Generations,
            Rule::Wolfram(_) => Fam::Wolfram,
            Rule::Turmite(_) => Fam::Turmite,
            Rule::WireWorld => Fam::WireWorld,
            Rule::Sandpile => Fam::Sandpile,
            Rule::SpectreGlider => Fam::SpectreGlider,
            Rule::Cyclic { .. } => Fam::Cyclic,
            Rule::GreenbergHastings { .. } => Fam::GreenbergHastings,
            Rule::Rps { .. } => Fam::Rps,
            Rule::ForestFire { .. } => Fam::ForestFire,
            Rule::Hodgepodge { .. } => Fam::Hodgepodge,
            Rule::HexWolfram { .. } => Fam::HexWolfram,
            Rule::HexTri { .. } => Fam::HexTri,
            Rule::Margolus(_) => Fam::Margolus,
            Rule::Lenia(_) => Fam::Lenia,
            Rule::Table(_) => Fam::Table,
            Rule::Ltl(_) => Fam::Ltl,
            Rule::Layered { .. } => Fam::Layered,
            Rule::VonNeumann => Fam::VonNeumann,
            Rule::Priority(_) => Fam::Priority,
            Rule::Jaca(_) => Fam::Jaca,
        }
    }

    /// Rules that write a sweep of rows or layers once, then stop.
    pub fn is_rows(self) -> bool {
        matches!(
            self,
            Fam::Wolfram | Fam::HexWolfram | Fam::HexTri | Fam::Layered
        )
    }

    /// Can this family run on the grid at all?
    pub fn fits(self, g: &Grid) -> bool {
        let t = &g.topo;
        match self {
            Fam::Wolfram | Fam::Margolus | Fam::Table | Fam::VonNeumann => t.rows().is_some(),
            Fam::HexWolfram => t.hex_rows().is_some(),
            Fam::HexTri => t.flat_hex_rows().is_some(),
            _ => true,
        }
    }

    /// Is the neighbourhood a free choice for this family?
    pub fn nb_matters(self) -> bool {
        matches!(
            self,
            Fam::LifeLike
                | Fam::Generations
                | Fam::Cyclic
                | Fam::Rps
                | Fam::GreenbergHastings
                | Fam::Hodgepodge
                | Fam::WireWorld
                | Fam::Jaca
                | Fam::Priority
                | Fam::ForestFire
                | Fam::Layered
                | Fam::Sandpile
                | Fam::SpectreGlider
        )
    }
}

/// One entry in the tiling list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TilingChoice {
    /// `square`, `ih28`, `catalog:k2-001`.
    pub key: String,
    /// Where it sits in the Quick Start tiling menu.
    pub group: String,
    pub aperiodic: bool,
    /// The share of search effort this tiling gets, before coverage rules.
    pub weight: f64,
    /// Another tiling with the same neighbour structure (same behaviour): the key that got the
    /// full search.
    pub twin_of: Option<String>,
}

const NAMED: &[(&str, &str, f64)] = &[
    ("square", "square", 6.0),
    ("hex", "hexagon", 6.0),
    ("hex_flat", "hexagon", 1.5),
    ("triangle", "triangle", 6.0),
    ("brick", "hexagon", 0.5),
    ("herring", "hexagon", 0.7),
    ("lizard", "hexagon", 0.7),
    ("cairo", "pentagon", 3.0),
    ("quad", "square", 0.5),
    ("glide", "square", 0.5),
    ("fourfold", "square", 0.5),
    ("oct", "archimedean", 2.0),
    ("trihex", "archimedean", 3.0),
    ("elongtri", "archimedean", 2.0),
    ("snubsq", "archimedean", 2.0),
    ("snubhex", "archimedean", 2.0),
    ("rhombitrihex", "archimedean", 3.0),
    ("trunchex", "archimedean", 1.5),
    ("trunctrihex", "archimedean", 2.0),
    ("pyth", "archimedean", 1.5),
    ("penrose-rhomb", "aperiodic", 3.0),
    ("penrose-kd", "aperiodic", 2.0),
    ("spectre", "aperiodic", 3.0),
    ("hat", "aperiodic", 2.0),
];

fn base_spec(key: &str) -> TilingSpec {
    if let Some(id) = key.strip_prefix("catalog:") {
        let mut spec = TilingSpec::new("periodic");
        spec.params = Some(serde_json::json!({ "catalog": id }));
        return spec;
    }
    TilingSpec::new(key)
}

pub fn is_aperiodic(key: &str) -> bool {
    matches!(key, "penrose-rhomb" | "penrose-kd" | "spectre" | "hat")
}

/// The spec of `key` at about `cells` cells. Periodic tilings wrap; aperiodic patches have a
/// dead edge.
pub fn sized_spec(key: &str, cells: u32) -> Result<(TilingSpec, Arc<Topology>), String> {
    let base = base_spec(key);
    if is_aperiodic(key) {
        let mut level = aperiodic::default_level(key);
        loop {
            let spec = base
                .clone()
                .with_size(SizeSpec::Level(level))
                .with_boundary(Boundary::Dead);
            let topo = Topology::build(&spec)?;
            if topo.len() as u32 >= cells / 2 || level > 14 {
                return Ok((spec, Arc::new(topo)));
            }
            level += 1;
        }
    }
    let spec = base
        .with_size(SizeSpec::Cells(cells))
        .with_boundary(Boundary::Wrap);
    let topo = Topology::build(&spec)?;
    Ok((spec, Arc::new(topo)))
}

/// A hash of the tiling's local neighbour structure: colour refinement on the edge and touch
/// graphs. Tilings with the same hash behave identically under any rule that only sees neighbour
/// counts.
pub fn structure_hash(topo: &Topology) -> u64 {
    let n = topo.len();
    let mix = |a: u64, b: u64| -> u64 {
        (a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .rotate_left(23)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9)
    };
    let mut label: Vec<u64> = (0..n)
        .map(|i| {
            let (mut e, mut t) = (0u64, 0u64);
            topo.for_each_neighbor(i, Nb::Edge, |_| e += 1);
            topo.for_each_neighbor(i, Nb::Touch, |_| t += 1);
            mix(e, t)
        })
        .collect();
    for _ in 0..6 {
        label = (0..n)
            .into_par_iter()
            .map(|i| {
                let side = |nb: Nb| {
                    let mut v = Vec::new();
                    topo.for_each_neighbor(i, nb, |x| v.push(label[x]));
                    v.sort_unstable();
                    v.iter().fold(0x51ED_270B_u64, |h, &x| mix(h, x))
                };
                mix(mix(label[i], side(Nb::Edge)), side(Nb::Touch))
            })
            .collect();
    }
    let mut set: Vec<u64> = label;
    set.sort_unstable();
    set.dedup();
    set.iter().fold(n.min(1) as u64, |h, &x| mix(h, x))
}

/// Every tiling the search covers, the twins marked.
pub fn all_tilings() -> Vec<TilingChoice> {
    let mut keys: Vec<(String, String, f64)> = NAMED
        .iter()
        .map(|(k, g, w)| (k.to_string(), g.to_string(), *w))
        .collect();
    for t in catalog::all() {
        // The catalog also holds the Archimedean tilings under their own ids.
        keys.push((format!("catalog:{}", t.id), "k-uniform".into(), 0.02));
    }
    for num in 1..=93u32 {
        if let Some(id) = crate::isohedral::id_for(num) {
            keys.push((id.to_string(), "isohedral".into(), 0.1));
        }
    }
    let probes: Vec<(usize, u64)> = keys
        .par_iter()
        .enumerate()
        .filter_map(|(i, (k, _, _))| {
            if is_aperiodic(k) {
                return Some((i, i as u64));
            }
            let (_, t) = sized_spec(k, 3000).ok()?;
            Some((i, structure_hash(&t)))
        })
        .collect();
    let mut first: HashMap<u64, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, h) in probes {
        let (key, group, weight) = &keys[i];
        let twin = match first.get(&h) {
            Some(&j) => Some(keys[j].0.clone()),
            None => {
                first.insert(h, i);
                None
            }
        };
        out.push(TilingChoice {
            key: key.clone(),
            group: group.clone(),
            aperiodic: is_aperiodic(key),
            weight: if twin.is_some() {
                *weight * 0.05
            } else {
                *weight
            },
            twin_of: twin,
        });
    }
    out
}

/// A built grid plus what the search needs to know about it.
pub struct Grid {
    pub key: String,
    pub cells_target: u32,
    pub spec: TilingSpec,
    pub topo: Arc<Topology>,
    pub csr: [Csr; 2],
    pub max_deg: [usize; 2],
    pub mean_deg: [f64; 2],
    pub has_edge: bool,
    pub cent: Vec<[f32; 2]>,
    pub tile: f32,
    /// Spatial bin per cell (a coarse grid) and the bin count.
    pub bin: Vec<u16>,
    pub nbins: usize,
    /// Larger than Life reach sizes, measured on demand: (hood, r) -> neighbours per cell.
    reach_sizes: Mutex<HashMap<(u8, u8), usize>>,
}

impl Grid {
    pub fn build(key: &str, cells: u32) -> Result<Grid, String> {
        let (spec, topo) = sized_spec(key, cells)?;
        let n = topo.len();
        // Built lazily inside parallel loops these lazily initialised tables can deadlock (a
        // worker blocked on the lock steals the job that holds it), so build them now.
        let b = topo.bounds;
        let _ = topo.locate([(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5]);
        topo.for_cells_in_box([b[0], b[1], b[0], b[1]], |_, _| {});
        let csr = [
            super::batch::neighbor_csr(&topo, Nb::Touch, None)?,
            super::batch::neighbor_csr(&topo, Nb::Edge, None)?,
        ];
        let deg = |c: &Csr| -> (usize, f64) {
            let mut m = 0;
            for i in 0..n {
                m = m.max((c.off[i + 1] - c.off[i]) as usize);
            }
            (m, c.nb.len() as f64 / n.max(1) as f64)
        };
        let (d0, m0) = deg(&csr[0]);
        let (d1, m1) = deg(&csr[1]);
        let has_edge = csr[0].nb != csr[1].nb;
        let cent: Vec<[f32; 2]> = (0..n)
            .into_par_iter()
            .map(|i| {
                let p = topo.centroid(i);
                [p[0] as f32, p[1] as f32]
            })
            .collect();
        let tile = crate::world::cell_size(&topo) as f32;
        let b = topo.bounds;
        const SIDE: usize = 16;
        let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
        let bin: Vec<u16> = cent
            .iter()
            .map(|p| {
                let x =
                    (((p[0] as f64 - b[0]) / bw.max(1e-9) * SIDE as f64) as usize).min(SIDE - 1);
                let y =
                    (((p[1] as f64 - b[1]) / bh.max(1e-9) * SIDE as f64) as usize).min(SIDE - 1);
                (y * SIDE + x) as u16
            })
            .collect();
        Ok(Grid {
            key: key.to_string(),
            cells_target: cells,
            spec,
            topo,
            csr,
            max_deg: [d0, d1],
            mean_deg: [m0, m1],
            has_edge,
            cent,
            tile,
            bin,
            nbins: SIDE * SIDE,
            reach_sizes: Mutex::new(HashMap::new()),
        })
    }

    pub fn csr(&self, nb: Nb) -> &Csr {
        &self.csr[nb as usize]
    }

    pub fn degree(&self, nb: Nb) -> usize {
        self.max_deg[nb as usize]
    }

    /// Neighbours in a Larger than Life reach (the largest over cells near the middle).
    pub fn reach_size(&self, hood: Hood, r: u8) -> usize {
        let key = (hood as u8, r);
        let mut m = self.reach_sizes.lock().unwrap();
        *m.entry(key).or_insert_with(|| {
            super::batch::neighbor_csr(&self.topo, Nb::Touch, Some((hood, r)))
                .map(|c| {
                    (0..self.topo.len())
                        .map(|i| (c.off[i + 1] - c.off[i]) as usize)
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        })
    }
}

fn uniform(rng: &mut Rng, lo: u64, hi: u64) -> u64 {
    lo + rng.below(hi - lo + 1)
}

fn pick_count(rng: &mut Rng, d: usize, centre: f64, spread: f64) -> usize {
    let x = centre * d as f64 + spread * d as f64 * (rng.next_f64() + rng.next_f64() - 1.0);
    (x.round().max(0.0) as usize).min(d)
}

/// A count mask of about `k` counts around a centre fraction of `d` (bit 0 allowed if `zero`).
fn mask(rng: &mut Rng, d: usize, k: usize, centre: f64, spread: f64, zero: bool) -> u32 {
    let mut m = 0u32;
    for _ in 0..k {
        let c = pick_count(rng, d, centre, spread);
        if c > 0 || zero {
            m |= 1 << c.min(30);
        }
    }
    m
}

fn bs_rule(rng: &mut Rng, d: usize) -> (u32, u32) {
    let d = d.clamp(3, 30);
    let (bk, sk) = (1 + rng.below(4) as usize, rng.below(6) as usize);
    let (bc, sc) = (0.15 + 0.35 * rng.next_f64(), 0.15 + 0.45 * rng.next_f64());
    let mut b = mask(rng, d, bk, bc, 0.45, false);
    if b == 0 {
        b = 1 << (1 + rng.below(d.min(5) as u64));
    }
    let s = mask(rng, d, sk, sc, 0.5, true);
    (b, s)
}

fn toggle(rng: &mut Rng, m: u32, d: usize, zero: bool) -> u32 {
    let low = if zero { 0 } else { 1 };
    let bit = uniform(rng, low, d.clamp(1, 30) as u64) as u32;
    m ^ (1 << bit)
}

/// The largest state count a CA in a batch may use.
const MAX_STATES: u64 = 64;

/// A random Larger than Life rule on this grid.
pub fn sample_ltl(grid: &Grid, rng: &mut Rng, fixed: Option<(Hood, u8)>) -> Option<Rule> {
    let (hood, range) = fixed.unwrap_or_else(|| {
        (
            if rng.below(4) == 0 {
                Hood::VonNeumann
            } else {
                Hood::Moore
            },
            uniform(rng, 2, 6) as u8,
        )
    });
    let max = grid.reach_size(hood, range);
    if max < 6 {
        return None;
    }
    let f = max as f64;
    let (b0, s0) = (0.22 + 0.2 * rng.next_f64(), 0.22 + 0.25 * rng.next_f64());
    let (bw, sw) = (0.03 + 0.18 * rng.next_f64(), 0.05 + 0.3 * rng.next_f64());
    let span = |a: f64, w: f64| -> (usize, usize) {
        let lo = (a * f).round() as usize;
        (lo.max(1), ((a + w) * f).round() as usize)
    };
    let (ba, bb) = span(b0, bw);
    let (sa, sb) = span(s0, sw);
    let states = if rng.below(3) == 0 {
        uniform(rng, 3, 12)
    } else {
        2
    };
    let hood_txt = match hood {
        Hood::VonNeumann => ",NN",
        _ => "",
    };
    let t = format!("R{range},C{states},S{sa}-{sb},B{ba}-{bb}{hood_txt}");
    LtlRule::parse(&t)
        .and_then(|r| r.ok())
        .map(|r| Rule::Ltl(Box::new(r)))
}

fn sample_turmite(rng: &mut Rng, grid: &Grid) -> Rule {
    // A turn string, the common case; occasionally a table.
    let turns = |rng: &mut Rng, degree: usize| -> crate::rules::Turn {
        use crate::rules::Turn::*;
        let opts: &[crate::rules::Turn] = if degree >= 6 {
            &[R, L, N, U, R2, L2]
        } else {
            &[R, L, N, U]
        };
        opts[rng.below(opts.len() as u64) as usize]
    };
    let degree = grid.max_deg[Nb::Edge as usize];
    let (states, colors) = if rng.below(4) == 0 {
        (uniform(rng, 2, 4) as u8, uniform(rng, 2, 4) as u8)
    } else {
        (1, uniform(rng, 2, 8) as u8)
    };
    let table = (0..states as usize * colors as usize)
        .map(|i| Entry {
            write: if states == 1 {
                ((i + 1) % colors as usize) as u8
            } else {
                rng.below(colors as u64) as u8
            },
            turn: turns(rng, degree),
            next: rng.below(states as u64) as u8,
        })
        .collect();
    Rule::Turmite(Mite {
        states,
        colors,
        table,
    })
}

fn sample_priority(rng: &mut Rng, grid: &Grid, nb: Nb) -> Rule {
    let states = uniform(rng, 2, 6) as u8;
    let d = grid.degree(nb).clamp(3, 12) as u64;
    let nrows = uniform(rng, 2, 7) as usize;
    let rows = (0..nrows)
        .map(|_| {
            let ncond = rng.below(3) as usize;
            let conds = (0..ncond)
                .map(|_| {
                    (
                        rng.below(states as u64) as u8,
                        uniform(rng, 1, d.min(4)) as u8,
                    )
                })
                .collect();
            Row {
                own: (rng.below(5) != 0).then(|| rng.below(states as u64) as u8),
                conds,
                next: rng.below(states as u64) as u8,
            }
        })
        .collect();
    Rule::Priority(Box::new(PriorityRule { states, rows }))
}

/// A random rule of this family on this grid, or `None` if the family has nothing there.
pub fn sample_rule(fam: Fam, grid: &Grid, nb: Nb, rng: &mut Rng) -> Option<Rule> {
    let d = grid.degree(nb);
    Some(match fam {
        Fam::LifeLike => {
            let (birth, survive) = bs_rule(rng, d);
            Rule::LifeLike { birth, survive }
        }
        Fam::Generations => {
            let (birth, survive) = bs_rule(rng, d);
            Rule::Generations {
                birth,
                survive,
                states: uniform(rng, 3, 32) as u8,
            }
        }
        Fam::Cyclic => Rule::Cyclic {
            states: uniform(rng, 3, 24) as u8,
            threshold: uniform(rng, 1, (d as u64 / 2).max(2)) as u8,
        },
        Fam::Rps => Rule::Rps {
            states: uniform(rng, 3, 9) as u8,
            threshold: uniform(rng, 1, (d as u64 / 2).max(1)) as u8,
        },
        Fam::GreenbergHastings => Rule::GreenbergHastings {
            states: uniform(rng, 3, 24) as u8,
            threshold: uniform(rng, 1, 4.min(d as u64)) as u8,
        },
        Fam::Hodgepodge => Rule::Hodgepodge {
            n: uniform(rng, 20, 200) as u8,
            k1: uniform(rng, 1, 6) as u32,
            k2: uniform(rng, 1, 6) as u32,
            g: uniform(rng, 1, 60) as u32,
        },
        Fam::WireWorld => Rule::WireWorld,
        Fam::Ltl => return sample_ltl(grid, rng, None),
        Fam::Jaca => {
            let (pb, ps) = bs_rule(rng, d);
            let (nb_, ns) = if rng.below(2) == 0 {
                (pb, ps)
            } else {
                bs_rule(rng, d)
            };
            Rule::Jaca(Box::new(JacaRule {
                pos_birth: pb,
                pos_survive: ps,
                neg_birth: nb_,
                neg_survive: ns,
                blast: [0, 0, 0, 1, 1, 2, 3][rng.below(7) as usize],
            }))
        }
        Fam::Priority => sample_priority(rng, grid, nb),
        Fam::ForestFire => Rule::ForestFire {
            p: 10f64.powf(-3.0 + 2.5 * rng.next_f64()),
            f: 10f64.powf(-6.0 + 3.5 * rng.next_f64()),
        },
        Fam::Turmite => sample_turmite(rng, grid),
        Fam::Layered => {
            let states = uniform(rng, 2, 6) as u8;
            let len = uniform(rng, 2, 8) as usize;
            let mut digits: Vec<u8> = (0..len).map(|_| rng.below(states as u64) as u8).collect();
            digits[0] = 0;
            if digits.iter().all(|&x| x == 0) {
                digits[1] = 1;
            }
            Rule::Layered { states, digits }
        }
        Fam::Wolfram => Rule::Wolfram(rng.below(256) as u8),
        Fam::HexWolfram => {
            let states = uniform(rng, 2, 3) as u8;
            Rule::HexWolfram {
                states,
                number: rng.below((states as u64).pow((states * states) as u32).min(1 << 32))
                    as u32,
            }
        }
        Fam::HexTri => {
            let states = uniform(rng, 2, 3) as u8;
            let (mode, max) = if states == 2 {
                (HexTMode::Full, 256u64)
            } else if rng.below(2) == 0 {
                (HexTMode::Sum, 2187)
            } else {
                (HexTMode::Full, 1u64 << 40)
            };
            Rule::HexTri {
                states,
                mode,
                number: rng.below(max),
            }
        }
        Fam::Margolus => {
            // A random involution-free table, with the empty block kept empty most of the time.
            let mut t = [0u8; 16];
            for (i, x) in t.iter_mut().enumerate() {
                *x = if rng.below(2) == 0 {
                    i as u8
                } else {
                    rng.below(16) as u8
                };
            }
            if rng.below(4) != 0 {
                t[0] = 0;
            }
            Rule::Margolus(t)
        }
        Fam::Lenia => {
            let r = uniform(rng, 8, 18) as f64;
            let m = 0.1 + 0.3 * rng.next_f64();
            let s = 0.006 + 0.05 * rng.next_f64();
            let t = [5.0, 10.0, 20.0][rng.below(3) as usize];
            let peaks = 1 + rng.below(3) as usize;
            let b = (0..peaks)
                .map(|i| {
                    if i == 0 {
                        1.0
                    } else {
                        (rng.next_f64() * 10.0).round() / 10.0
                    }
                })
                .collect();
            Rule::Lenia(Box::new(LeniaRule {
                r,
                t,
                b,
                m,
                s,
                kn: 1 + rng.below(2) as u8,
                gn: 1 + rng.below(2) as u8,
            }))
        }
        Fam::Sandpile => Rule::Sandpile,
        Fam::Table => Rule::Table("parity".into()),
        Fam::VonNeumann => Rule::VonNeumann,
        Fam::SpectreGlider => Rule::SpectreGlider,
    })
}

/// A rule one small step from `rule`.
pub fn mutate_rule(rule: &Rule, grid: &Grid, nb: Nb, rng: &mut Rng) -> Option<Rule> {
    let d = grid.degree(nb);
    Some(match rule {
        Rule::LifeLike { birth, survive } => {
            if rng.below(2) == 0 {
                Rule::LifeLike {
                    birth: toggle(rng, *birth, d, false),
                    survive: *survive,
                }
            } else {
                Rule::LifeLike {
                    birth: *birth,
                    survive: toggle(rng, *survive, d, true),
                }
            }
        }
        Rule::Generations {
            birth,
            survive,
            states,
        } => match rng.below(3) {
            0 => Rule::Generations {
                birth: toggle(rng, *birth, d, false),
                survive: *survive,
                states: *states,
            },
            1 => Rule::Generations {
                birth: *birth,
                survive: toggle(rng, *survive, d, true),
                states: *states,
            },
            _ => Rule::Generations {
                birth: *birth,
                survive: *survive,
                states: (*states as i64 + if rng.below(2) == 0 { 1 } else { -1 })
                    .clamp(3, MAX_STATES as i64) as u8,
            },
        },
        Rule::Cyclic { states, threshold } => {
            let (s, t) = bump(
                rng,
                *states as i64,
                *threshold as i64,
                3,
                1,
                d as i64 / 2 + 1,
            );
            Rule::Cyclic {
                states: s as u8,
                threshold: t as u8,
            }
        }
        Rule::Rps { states, threshold } => {
            let (s, t) = bump(
                rng,
                *states as i64,
                *threshold as i64,
                3,
                1,
                d as i64 / 2 + 1,
            );
            Rule::Rps {
                states: s as u8,
                threshold: t as u8,
            }
        }
        Rule::GreenbergHastings { states, threshold } => {
            let (s, t) = bump(
                rng,
                *states as i64,
                *threshold as i64,
                3,
                1,
                d as i64 / 2 + 1,
            );
            Rule::GreenbergHastings {
                states: s as u8,
                threshold: t as u8,
            }
        }
        Rule::Hodgepodge { n, k1, k2, g } => {
            let jitter = |rng: &mut Rng, v: u32, lo: i64, hi: i64| -> u32 {
                (v as i64 + uniform(rng, 0, 4) as i64 - 2).clamp(lo, hi) as u32
            };
            Rule::Hodgepodge {
                n: jitter(rng, *n as u32, 10, 250) as u8,
                k1: jitter(rng, *k1, 1, 8),
                k2: jitter(rng, *k2, 1, 8),
                g: jitter(rng, *g, 1, 90),
            }
        }
        Rule::Ltl(l) => {
            let mut l = l.clone();
            let flip = |v: &mut Vec<bool>, rng: &mut Rng| {
                // Move one end of a run of counts by a little.
                let on: Vec<usize> = (0..v.len()).filter(|&i| v[i]).collect();
                if on.is_empty() {
                    return;
                }
                let (lo, hi) = (on[0], *on.last().unwrap());
                let step = uniform(rng, 1, ((hi - lo) / 6 + 1) as u64) as usize;
                match rng.below(4) {
                    0 if lo > 1 => {
                        for x in v.iter_mut().skip(lo - step.min(lo - 1)).take(step) {
                            *x = true;
                        }
                    }
                    1 => {
                        for x in v.iter_mut().skip(lo).take(step.min(hi - lo)) {
                            *x = false;
                        }
                    }
                    2 if hi + step < v.len() => {
                        for x in v.iter_mut().skip(hi + 1).take(step) {
                            *x = true;
                        }
                    }
                    _ => {
                        let n = step.min(hi - lo);
                        for x in v.iter_mut().skip(hi + 1 - n).take(n) {
                            *x = false;
                        }
                    }
                }
            };
            if rng.below(2) == 0 {
                flip(&mut l.birth, rng);
            } else {
                flip(&mut l.survive, rng);
            }
            Rule::Ltl(l)
        }
        Rule::Jaca(j) => {
            let mut j = j.clone();
            match rng.below(5) {
                0 => j.pos_birth = toggle(rng, j.pos_birth, d, false),
                1 => j.pos_survive = toggle(rng, j.pos_survive, d, true),
                2 => j.neg_birth = toggle(rng, j.neg_birth, d, false),
                3 => j.neg_survive = toggle(rng, j.neg_survive, d, true),
                _ => {
                    j.blast =
                        (j.blast as i64 + if rng.below(2) == 0 { 1 } else { -1 }).clamp(0, 4) as u8
                }
            }
            Rule::Jaca(j)
        }
        Rule::Priority(p) => {
            let mut p = p.clone();
            let i = rng.below(p.rows.len().max(1) as u64) as usize;
            if let Some(row) = p.rows.get_mut(i) {
                match rng.below(3) {
                    0 => row.next = rng.below(p.states as u64) as u8,
                    1 => row.own = (rng.below(5) != 0).then(|| rng.below(p.states as u64) as u8),
                    _ => {
                        if let Some(c) = row.conds.first_mut() {
                            c.1 = uniform(rng, 1, 4) as u8;
                        } else {
                            row.conds
                                .push((rng.below(p.states as u64) as u8, uniform(rng, 1, 3) as u8));
                        }
                    }
                }
            }
            Rule::Priority(p)
        }
        Rule::ForestFire { p, f } => Rule::ForestFire {
            p: (p * 10f64.powf(0.4 * (rng.next_f64() - 0.5))).clamp(1e-4, 0.5),
            f: (f * 10f64.powf(0.6 * (rng.next_f64() - 0.5))).clamp(1e-7, 0.05),
        },
        Rule::Turmite(_) => sample_turmite(rng, grid),
        Rule::Layered { states, digits } => {
            let mut digits = digits.clone();
            let i = rng.below(digits.len() as u64) as usize;
            digits[i] = rng.below(*states as u64) as u8;
            Rule::Layered {
                states: *states,
                digits,
            }
        }
        Rule::Wolfram(n) => Rule::Wolfram(*n ^ (1 << rng.below(8))),
        Rule::HexWolfram { states, number } if *states == 2 => Rule::HexWolfram {
            states: 2,
            number: *number ^ (1 << rng.below(4)),
        },
        Rule::Margolus(t) => {
            let mut t = *t;
            t[rng.below(16) as usize] = rng.below(16) as u8;
            Rule::Margolus(t)
        }
        Rule::Lenia(l) => {
            let mut l = l.clone();
            match rng.below(3) {
                0 => l.m = (l.m + 0.02 * (rng.next_f64() - 0.5)).clamp(0.05, 0.5),
                1 => l.s = (l.s * (0.8 + 0.4 * rng.next_f64())).clamp(0.003, 0.09),
                _ => l.r = (l.r + uniform(rng, 0, 4) as f64 - 2.0).clamp(6.0, 24.0),
            }
            Rule::Lenia(l)
        }
        _ => return None,
    })
}

fn bump(rng: &mut Rng, s: i64, t: i64, smin: i64, tmin: i64, tmax: i64) -> (i64, i64) {
    if rng.below(2) == 0 {
        (
            (s + if rng.below(2) == 0 { 1 } else { -1 }).clamp(smin, MAX_STATES as i64),
            t,
        )
    } else {
        (
            s,
            (t + if rng.below(2) == 0 { 1 } else { -1 }).clamp(tmin, tmax.max(tmin)),
        )
    }
}
