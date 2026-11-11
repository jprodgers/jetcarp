//! The engine: cell fields, CA instances, the command layer, scenes, and state hashes.
//!
//! - Every CA declares the field kind it needs. Binary CAs share one field by default; multi-state
//!   and height CAs get their own. **Affects others** switches a CA between the shared field and a
//!   private one that is only drawn alongside.
//! - Every change goes through [`Command`]. The world keeps a base (tiling, RNG state, CA specs) and
//!   a log of commands since then; a [`Scene`] is exactly that, so replaying it reproduces the state.
//! - Stepping is a pure function of the previous state, parallel over cells, so the state hash is the
//!   same at any thread count.

use crate::render::{ColorBy, Palette};
use crate::reseed::{AutoReseed, Edits, PolicyStatus, ReseedMode};
use crate::rng::{Fnv, Rng};
use crate::rules::{FieldKind, Rule};
use crate::script::{self, CaRef, Metrics, Script, ScriptState};
use crate::seeding::{SeedFill, SeedShape, num, pct};
use crate::step::{glider_travels, step_ca, wire_demo};
use crate::table::{Compiled, RuleTable, TableSet};
use crate::topology::{Nb, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SCENE_FORMAT: u32 = 1;

// Hooks for stepping outside the engine (the `jetcarp-gpu` crate). See `world/external.rs`.
mod external;
pub use external::{ExternalPlan, GenMetrics, Halt, WatchPlan};
// Completion detection (`World::done`). See `world/done.rs`.
mod done;
// Automatic reseeding and the activity history. See `world/reseeding.rs`.
mod reseeding;
pub use reseeding::{ACTIVITY_HISTORY, Activity};

// ---------------------------------------------------------------------------------------------
// Fields

#[derive(Clone, Debug)]
pub struct Field {
    pub kind: FieldKind,
    pub shared: bool,
    pub state: Vec<u8>,
    pub age: Vec<u16>,
    pub height: Vec<u32>,
    /// Continuous fields: the value of each cell in [0, 1] (`state` holds `ceil(255 v)`).
    pub value: Vec<f32>,
    /// Grains that fell off a dead edge or into an open boundary cell.
    pub lost: u64,
    pub(crate) back: Vec<u8>,
    pub(crate) back_age: Vec<u16>,
    pub(crate) back_h: Vec<u32>,
    pub(crate) back_v: Vec<f32>,
}

impl Field {
    pub fn new(kind: FieldKind, n: usize, shared: bool) -> Field {
        let (s, h) = if kind == FieldKind::Height {
            (0, n)
        } else {
            (n, 0)
        };
        let v = if kind == FieldKind::Continuous { n } else { 0 };
        Field {
            kind,
            shared,
            state: vec![0; s],
            age: vec![0; s],
            height: vec![0; h],
            value: vec![0.0; v],
            lost: 0,
            back: vec![0; s],
            back_age: vec![0; s],
            back_h: vec![0; h],
            back_v: vec![0.0; v],
        }
    }
    pub fn clear(&mut self) {
        self.state.fill(0);
        self.age.fill(0);
        self.height.fill(0);
        self.value.fill(0.0);
        self.lost = 0;
    }
    /// The 8-bit level shown for a continuous value: `ceil(255 v)`, nonzero exactly when v is.
    #[inline]
    pub fn level(v: f32) -> u8 {
        (v.clamp(0.0, 1.0) * 255.0).ceil() as u8
    }
    /// Continuous fields: refresh the 8-bit copy (`state`) and ages from the values.
    pub fn sync_mirror(&mut self) {
        if self.kind != FieldKind::Continuous {
            return;
        }
        for ((s, a), &v) in self
            .state
            .iter_mut()
            .zip(self.age.iter_mut())
            .zip(&self.value)
        {
            *s = Field::level(v);
            *a = (*s != 0) as u16;
        }
    }
    /// Set a continuous value (clamped to [0, 1]); other fields: state 1 when `v` > 0.
    pub fn set_value(&mut self, i: usize, v: f32) {
        if self.kind == FieldKind::Continuous {
            let v = v.clamp(0.0, 1.0);
            self.value[i] = v;
            self.state[i] = Field::level(v);
            self.age[i] = (v > 0.0) as u16;
        } else if self.kind != FieldKind::Height {
            self.set(i, (v > 0.0) as u32);
        }
    }
    /// Continuous fields: the sum of the values (Lenia's mass), else the population.
    pub fn mass(&self) -> f64 {
        if self.kind == FieldKind::Continuous {
            self.value.iter().map(|&v| v as f64).sum()
        } else {
            self.population() as f64
        }
    }
    /// Live cells (nonzero state), or for height fields the total grain count.
    pub fn population(&self) -> u64 {
        if self.kind == FieldKind::Height {
            self.height.iter().map(|&h| h as u64).sum()
        } else {
            self.state.iter().filter(|&&s| s != 0).count() as u64
        }
    }
    pub fn is_empty(&self) -> bool {
        if self.kind == FieldKind::Height {
            self.height.iter().all(|&h| h == 0)
        } else {
            self.state.iter().all(|&s| s == 0)
        }
    }
    pub fn max_state(&self) -> u8 {
        match self.kind {
            FieldKind::Binary => 1,
            FieldKind::Multi(n) => n - 1,
            FieldKind::Height => 0,
            FieldKind::Continuous => 255,
        }
    }
    /// Set a cell's state (height fields: add grains; continuous fields: value `v / 255`).
    pub fn set(&mut self, i: usize, v: u32) {
        match self.kind {
            FieldKind::Height => self.height[i] = self.height[i].saturating_add(v),
            FieldKind::Continuous => self.set_value(i, v.min(255) as f32 / 255.0),
            _ => {
                let v = (v.min(self.max_state() as u32)) as u8;
                self.state[i] = v;
                self.age[i] = if v != 0 { 1 } else { 0 };
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CA specs and instances

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedSpec {
    /// Leave the field as it is.
    Empty,
    /// Each cell becomes live with this probability (height fields: a random stable height).
    Random { density: f64 },
    /// One live cell at the centre.
    Center,
    /// Random live cells in a disc at the centre. `radius` is a fraction of the half-width of the
    /// tiling's shorter side.
    Blob { radius: f64, density: f64 },
    /// One live cell in the middle of the top row (elementary rules).
    TopCenter,
    /// A random top row (elementary rules).
    TopRandom { density: f64 },
    /// Turmite agents. One ant starts at the centre; more start at random cells.
    Ants { count: u32 },
    /// One turmite agent on this cell, facing this edge slot (ant searches' scenes).
    AntAt { cell: u32, heading: u16 },
    /// Grains on the centre cell.
    Pile { grains: u64 },
    /// Every cell gets a uniformly random state (multi-state rules).
    RandomStates,
    /// JACA: each cell is live with probability `density`, and a live cell is -1 with
    /// probability `negative` (else +1).
    Signed { density: f64, negative: f64 },
    /// JACA: random +1 cells on the left half and -1 cells on the right half, at `density`.
    Halves { density: f64 },
    /// A state-1 cell next to a state-3 cell near the centre, chosen by the RNG (Spectre glider).
    GliderPair,
    /// Clock loops feeding wires (WireWorld on the square grid).
    WireDemo,
    /// WireWorld wiring on any tiling: conductors along the borders of `sites` Voronoi regions,
    /// carrying `electrons` signals, each a head with a tail behind it.
    Circuit { sites: u32, electrons: u32 },
    /// Explicit `[cell, state]` pairs (height fields: grains added).
    Cells { cells: Vec<[u32; 2]> },
    /// Continuous fields: `count` Gaussian bumps of height `peak` and width `radius` cells (one
    /// at the centre, the rest at random cells), added and clipped to 1.
    Gaussian { count: u32, radius: f64, peak: f64 },
    /// A built-in Lenia pattern (`orbium`) at the centre, `scale` cells per pattern cell.
    Lenia {
        name: String,
        #[serde(default = "one_f", skip_serializing_if = "is_one_f")]
        scale: f64,
    },
    /// A shape and a fill: blobs, clusters, stripes, gradients, symmetric seeds, and more, on
    /// any tiling (see [`crate::seeding`]).
    Pattern {
        #[serde(deserialize_with = "crate::seeding::shape_or_name")]
        shape: SeedShape,
        #[serde(default)]
        fill: SeedFill,
    },
}

impl SeedSpec {
    /// A short label for the seed, such as `3 blobs r0.1, 40% density`.
    pub fn describe(&self) -> String {
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        match self {
            SeedSpec::Empty => "empty".into(),
            SeedSpec::Random { density } => format!("random {}", pct(*density)),
            SeedSpec::Center => "center cell".into(),
            SeedSpec::Blob { radius, density } => {
                format!("blob r{}, {} density", num(*radius), pct(*density))
            }
            SeedSpec::TopCenter => "top center".into(),
            SeedSpec::TopRandom { density } => format!("random top row {}", pct(*density)),
            SeedSpec::Ants { count } => format!("{count} ant{}", plural(*count as usize)),
            SeedSpec::AntAt { cell, heading } => format!("ant at {cell} facing {heading}"),
            SeedSpec::Pile { grains } => format!("pile of {grains}"),
            SeedSpec::RandomStates => "random states".into(),
            SeedSpec::Signed { density, negative } => {
                format!("signed {}, {} neg", pct(*density), pct(*negative))
            }
            SeedSpec::Halves { density } => format!("halves {}", pct(*density)),
            SeedSpec::GliderPair => "glider pair".into(),
            SeedSpec::WireDemo => "wire demo".into(),
            SeedSpec::Circuit { sites, electrons } => {
                format!("circuit, {sites} regions, {electrons} signals")
            }
            SeedSpec::Cells { cells } => format!("{} cell{}", cells.len(), plural(cells.len())),
            SeedSpec::Gaussian {
                count,
                radius,
                peak,
            } => format!(
                "{count} gaussian{} r{} peak {}",
                plural(*count as usize),
                num(*radius),
                num(*peak)
            ),
            SeedSpec::Lenia { name, scale } if *scale == 1.0 => format!("lenia {name}"),
            SeedSpec::Lenia { name, scale } => format!("lenia {name} x{}", num(*scale)),
            SeedSpec::Pattern { shape, fill } => {
                let (s, f) = (shape.describe(), fill.describe());
                if f.is_empty() { s } else { format!("{s}, {f}") }
            }
        }
    }
}
fn one_f() -> f64 {
    1.0
}
fn is_one_f(v: &f64) -> bool {
    *v == 1.0
}

/// A CA as saved in scenes. Missing fields take the rule's defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood: Option<Nb>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affects_others: Option<bool>,
    #[serde(default = "one")]
    pub steps_per_gen: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SeedSpec>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub drop_per_gen: u32,
    #[serde(default, skip_serializing_if = "is_auto")]
    pub color_by: ColorBy,
    #[serde(default, skip_serializing_if = "is_bounce")]
    pub agent_edge: AgentEdge,
    /// JACA: the palette of -1 cells (`palette` colours +1 cells).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette_neg: Option<String>,
    /// JACA: draw a fading glow where cells were annihilated.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub glow: bool,
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_auto(c: &ColorBy) -> bool {
    *c == ColorBy::Auto
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl CaSpec {
    pub fn new(rule: &str) -> CaSpec {
        CaSpec {
            id: None,
            name: None,
            rule: rule.into(),
            neighborhood: None,
            enabled: true,
            affects_others: None,
            steps_per_gen: 1,
            palette: None,
            seed: None,
            drop_per_gen: 0,
            color_by: ColorBy::Auto,
            agent_edge: AgentEdge::Bounce,
            palette_neg: None,
            glow: true,
        }
    }
    pub fn seed(mut self, s: SeedSpec) -> Self {
        self.seed = Some(s);
        self
    }
    pub fn steps(mut self, n: u32) -> Self {
        self.steps_per_gen = n;
        self
    }
    pub fn nb(mut self, nb: Nb) -> Self {
        self.neighborhood = Some(nb);
        self
    }
    pub fn private(mut self) -> Self {
        self.affects_others = Some(false);
        self
    }
    pub fn named(mut self, n: &str) -> Self {
        self.name = Some(n.into());
        self
    }
}

/// What an ant does at a dead or live edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEdge {
    /// Turn around and stay on the grid.
    #[default]
    Bounce,
    /// Leave the grid and disappear.
    FallOff,
}
fn is_bounce(e: &AgentEdge) -> bool {
    *e == AgentEdge::Bounce
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub cell: u32,
    /// Slot index (into the cell's edge-neighbour list) the agent faces.
    pub heading: u16,
    /// Internal state (turmites with several states; 0 for turn strings).
    #[serde(default)]
    pub state: u8,
}

#[derive(Clone, Debug)]
pub struct CaInstance {
    pub id: u32,
    pub name: String,
    pub rule: Rule,
    pub nb: Nb,
    pub enabled: bool,
    pub affects_others: bool,
    pub steps_per_gen: u32,
    pub palette: String,
    pub seed: SeedSpec,
    pub drop_per_gen: u32,
    /// Index into `World::fields`.
    pub field: usize,
    pub agents: Vec<Agent>,
    /// Elementary rules: the row most recently written.
    pub cursor: u32,
    /// Rule tables: the compiled table.
    pub table: Option<Arc<Compiled>>,
    /// Larger than Life: the precomputed neighbourhood (built on first step).
    pub reach: Option<Arc<crate::ltl::Reach>>,
    /// Lenia: the precomputed kernel (built on first step).
    pub kernel: Option<Arc<crate::lenia::Kernel>>,
    /// Layered rules: each cell's layer (distance from the seed), fixed at the first step.
    pub layers: Option<Arc<Vec<u32>>>,
    /// Stochastic rules: per-CA noise seed (drawn from the world RNG when added) and step counter.
    pub noise: u64,
    pub ticks: u64,
    pub color_by: ColorBy,
    pub agent_edge: AgentEdge,
    /// JACA: the palette of -1 cells.
    pub palette_neg: String,
    /// JACA: draw the annihilation glow.
    pub glow: bool,
    /// JACA: annihilation glow and counts (display and status only; not part of the state).
    pub heat: Option<Arc<crate::jaca::Heat>>,
}

impl CaInstance {
    pub fn spec(&self) -> CaSpec {
        CaSpec {
            id: Some(self.id),
            name: Some(self.name.clone()),
            rule: self.rule.notation(),
            neighborhood: Some(self.nb),
            enabled: self.enabled,
            affects_others: Some(self.affects_others),
            steps_per_gen: self.steps_per_gen,
            palette: Some(self.palette.clone()),
            seed: Some(self.seed.clone()),
            drop_per_gen: self.drop_per_gen,
            color_by: self.color_by,
            agent_edge: self.agent_edge,
            palette_neg: matches!(self.rule, Rule::Jaca(_)).then(|| self.palette_neg.clone()),
            glow: self.glow,
        }
    }
    /// The rule in notation, with the V suffix for edge neighbourhoods on B/S rules.
    pub fn notation(&self) -> String {
        let n = self.rule.notation();
        match &self.rule {
            Rule::LifeLike { .. } | Rule::Generations { .. } if self.nb == Nb::Edge => {
                format!("{n}V")
            }
            Rule::Jaca(j) if self.nb == Nb::Edge => j.notation_with(true),
            _ => n,
        }
    }
}

/// Mean cell size of a tiling: the square root of the mean tile area.
pub fn cell_size(topo: &Topology) -> f64 {
    match topo.lattice() {
        Some(g) => (crate::geom::cross(g.a1, g.a2).abs() / g.ntypes as f64).sqrt(),
        None => {
            let n = topo.len().max(1);
            let k = n.min(200);
            ((0..k)
                .map(|i| crate::geom::area(&topo.polygon(i * n / k)).abs())
                .sum::<f64>()
                / k as f64)
                .sqrt()
        }
    }
}

pub fn default_seed(rule: &Rule, n_cells: usize) -> SeedSpec {
    match rule {
        Rule::Cyclic { .. }
        | Rule::Rps { .. }
        | Rule::Hodgepodge { .. }
        | Rule::GreenbergHastings { .. } => SeedSpec::RandomStates,
        Rule::ForestFire { .. } => SeedSpec::Random { density: 0.5 },
        Rule::HexWolfram { .. } | Rule::HexTri { .. } => SeedSpec::TopCenter,
        Rule::Table(_) => SeedSpec::Center,
        Rule::LifeLike { .. } | Rule::Generations { .. } => SeedSpec::Random { density: 0.3 },
        Rule::Ltl(_) => SeedSpec::Random { density: 0.4 },
        Rule::Layered { .. } => SeedSpec::Center,
        Rule::Wolfram(_) => SeedSpec::TopCenter,
        Rule::Turmite(_) => SeedSpec::Ants { count: 1 },
        Rule::WireWorld => SeedSpec::WireDemo,
        Rule::Sandpile => SeedSpec::Pile {
            grains: (n_cells as u64 * 3).max(1000),
        },
        Rule::SpectreGlider => SeedSpec::GliderPair,
        Rule::VonNeumann => SeedSpec::Empty,
        Rule::Jaca(_) => SeedSpec::Signed {
            density: 0.3,
            negative: 0.5,
        },
        Rule::Margolus(_) => SeedSpec::Blob {
            radius: 0.3,
            density: 0.3,
        },
        Rule::Lenia(_) => SeedSpec::Blob {
            radius: 0.25,
            density: 0.5,
        },
        Rule::Priority(_) => SeedSpec::Center,
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood: Option<Nb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affects_others: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps_per_gen: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_per_gen: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SeedSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_edge: Option<AgentEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette_neg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<bool>,
}

/// Every change to a world. The UI, the CLI, and future AI tools all issue these.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// Rebuild on a new tiling; keeps the CA list and reseeds. Starts a new base.
    SetTiling {
        tiling: TilingSpec,
    },
    /// Set the RNG state and reseed everything. Starts a new base.
    SetRngState {
        state: u64,
    },
    /// Clear all fields, reseed every CA from its seed spec, generation 0. Starts a new base.
    Reset,
    AddCa {
        ca: CaSpec,
    },
    RemoveCa {
        id: u32,
    },
    UpdateCa {
        id: u32,
        change: CaChange,
    },
    /// Apply a seed to a CA's field (additive).
    Seed {
        id: u32,
        seed: SeedSpec,
    },
    /// Clear a CA's field.
    Clear {
        id: u32,
    },
    /// Paint `[cell, state]` pairs into a CA's field (height fields: add grains).
    SetCells {
        id: u32,
        cells: Vec<[u32; 2]>,
    },
    Step {
        gens: u64,
    },
    /// Advance `gens` generations with HashLife (`hashlife.rs`): one CA on a wrap-edge lattice
    /// whose torus has power-of-two sides. States equal `Step`; live cells end with age 1.
    Jump {
        gens: u64,
    },
    /// Make a rule table available to `Table <name>` rules.
    AddTable {
        table: RuleTable,
    },
    /// Install a script (or remove it with `null`) and start it at its first step.
    SetScript {
        script: Option<Script>,
    },
    /// Jump a running script to a step.
    ScriptGoto {
        step: usize,
    },
    /// Reseed now (see [`crate::reseed`]): the field of CA `ca`, or with `null` the field of
    /// every running CA. Draws from the world RNG; restarts every policy's cooldown.
    Reseed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ca: Option<u32>,
        #[serde(deserialize_with = "crate::reseed::mode_or_name")]
        mode: ReseedMode,
    },
    /// Replace the automatic reseed policies (an empty list removes them).
    SetReseed {
        #[serde(default)]
        policies: Vec<AutoReseed>,
    },
    /// Add or replace a named palette.
    SetPalette {
        palette: Palette,
    },
    /// Sandpile tools on a sandpile CA's field.
    Sandpile {
        id: u32,
        action: SandpileOp,
    },
    /// Paste a pattern-library pattern (by id, or inline cells) into a CA's field at a lattice
    /// position, turned by `orientation` (index into the tiling's lattice symmetries) and in
    /// phase `phase`. Logged with the cells resolved inline. See `library::paste`.
    PastePattern {
        id: u32,
        pattern: crate::library::PatternSource,
        #[serde(default)]
        at: crate::library::PasteAt,
        #[serde(default, skip_serializing_if = "is_zero")]
        orientation: u32,
        #[serde(default, skip_serializing_if = "is_zero")]
        phase: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandpileOp {
    /// Add this many grains to every cell (sinks excepted).
    AddEverywhere { grains: u32 },
    /// Drop this many single grains on random cells (from the world RNG).
    AddRandom { count: u64 },
    /// Topple until stable, without counting generations.
    Stabilize,
    /// Replace the field with the identity of the sandpile group.
    Identity,
}

impl Command {
    fn rebases(&self) -> bool {
        matches!(
            self,
            Command::SetTiling { .. } | Command::SetRngState { .. } | Command::Reset
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub new_id: Option<u32>,
}

// ---------------------------------------------------------------------------------------------
// Scenes

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub generation: u64,
    pub hash: String,
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    /// True when some cells have missing neighbours (dead edges or a patch boundary).
    pub dead_boundary: bool,
}

/// A reproducible run: tiling, CA specs, the initial RNG state, and the command log since.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    #[serde(default = "fmt1")]
    pub format: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub tiling: TilingSpec,
    /// Initial SplitMix64 state. Replaying the commands advances it identically.
    #[serde(default)]
    pub rng_state: u64,
    /// Rule tables used by this scene, beyond the built-in ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<RuleTable>,
    #[serde(default)]
    pub cas: Vec<CaSpec>,
    /// User palettes used by this scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palettes: Vec<Palette>,
    /// A scripted sequence that starts with the scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    /// Automatic reseed policies that start with the scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reseed: Vec<AutoReseed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
    /// App view state (camera, palette display); ignored by the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<serde_json::Value>,
    /// Generation and hash when saved, for verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,
}
fn fmt1() -> u32 {
    SCENE_FORMAT
}

impl Scene {
    pub fn new(tiling: TilingSpec) -> Scene {
        Scene {
            jetcarp_version: crate::VERSION.into(),
            format: SCENE_FORMAT,
            name: String::new(),
            description: String::new(),
            tiling,
            rng_state: 1,
            tables: vec![],
            cas: vec![],
            palettes: vec![],
            script: None,
            reseed: vec![],
            commands: vec![],
            view: None,
            record: None,
        }
    }
    pub fn from_json(s: &str) -> Result<Scene, String> {
        serde_json::from_str(s).map_err(|e| format!("scene JSON: {e}"))
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }
}

// ---------------------------------------------------------------------------------------------
// Period detection with exact comparison (Brent), ignoring ages.

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    states: Vec<Vec<u8>>,
    heights: Vec<Vec<u32>>,
    agents: Vec<Vec<Agent>>,
    cursors: Vec<u32>,
    /// Phase-dependent rules (Margolus): the partition parity, so a repeat means the same phase.
    phases: Vec<u64>,
    /// Continuous fields' values (compared exactly).
    values: Vec<Vec<f32>>,
}

#[derive(Clone, Debug, Default)]
struct PeriodWatch {
    power: u64,
    lam: u64,
    snap: Option<Snapshot>,
    found: Option<(u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Empty,
    /// Period (1 = frozen) confirmed by exact comparison, first seen at this generation.
    Repeats {
        period: u64,
        seen_at: u64,
    },
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Running => write!(f, "Running"),
            Status::Empty => write!(f, "Empty"),
            Status::Repeats { period: 1, seen_at } => {
                write!(f, "Frozen (seen by generation {seen_at})")
            }
            Status::Repeats { period, seen_at } => {
                write!(
                    f,
                    "Repeats every {period} generations (exact check, seen by {seen_at})"
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// World

#[derive(Clone, Debug)]
struct Base {
    tiling: TilingSpec,
    rng_state: u64,
    tables: Vec<RuleTable>,
    cas: Vec<CaSpec>,
    script: Option<Script>,
    palettes: Vec<Palette>,
    reseed: Vec<AutoReseed>,
}

pub struct World {
    pub topo: Arc<Topology>,
    pub fields: Vec<Field>,
    pub cas: Vec<CaInstance>,
    pub generation: u64,
    pub rng: Rng,
    next_id: u32,
    base: Base,
    log: Vec<Command>,
    watch: PeriodWatch,
    pub tables: TableSet,
    pub script: Option<Script>,
    pub script_state: ScriptState,
    pub palettes: Vec<Palette>,
    /// Changes on every command and step; see [`World::revision`].
    rev: u64,
    /// Count changed cells every generation (see [`World::set_track_changes`]). Not hashed.
    track_changes: bool,
    /// Cells changed by the last generation, when counted. Cleared by every command.
    last_changed: Option<u64>,
    /// The last generation moved a row cursor or an ant (counted with `last_changed`).
    last_moved: bool,
    /// A topology built elsewhere for the next `SetTiling` of its spec (see
    /// [`World::set_tiling_built`]).
    prebuilt: Option<Arc<Topology>>,
    /// Automatic reseed policies and what each has done (see `world/reseeding.rs`).
    reseed: Vec<AutoReseed>,
    reseed_status: Vec<PolicyStatus>,
    /// Reseeds planned at the end of an externally stepped batch, applied once the stepper has
    /// written the fields back (see [`World::finish_external`]).
    pending_reseed: Vec<(usize, Edits)>,
    /// Manual reseeds so far (`Command::Reseed`), for the alternate sign.
    manual_reseeds: u64,
    /// Recent (generation, fraction of cells changed), whenever changes were counted.
    activity: std::collections::VecDeque<reseeding::Activity>,
    /// Typical tile size, measured on first use (explicit patches sample tiles).
    tile_size: std::sync::OnceLock<f64>,
}

impl World {
    pub fn new(tiling: TilingSpec, rng_state: u64) -> Result<World, String> {
        let topo = Arc::new(Topology::build(&tiling)?);
        Ok(World::with_topology(topo, tiling, rng_state))
    }

    /// A world on an already built tiling, for many runs on one topology (census, object checks).
    pub fn on_topology(topo: Arc<Topology>, rng_state: u64) -> World {
        let tiling = topo.spec.clone();
        World::with_topology(topo, tiling, rng_state)
    }

    fn with_topology(topo: Arc<Topology>, tiling: TilingSpec, rng_state: u64) -> World {
        World {
            topo,
            fields: vec![],
            cas: vec![],
            generation: 0,
            rng: Rng::new(rng_state),
            next_id: 1,
            base: Base {
                tiling,
                rng_state,
                tables: vec![],
                cas: vec![],
                script: None,
                palettes: vec![],
                reseed: vec![],
            },
            log: vec![],
            watch: PeriodWatch {
                power: 1,
                ..Default::default()
            },
            tables: TableSet::with_builtins(),
            script: None,
            script_state: ScriptState::default(),
            palettes: vec![],
            rev: external::next_rev(),
            track_changes: false,
            last_changed: None,
            last_moved: false,
            prebuilt: None,
            reseed: vec![],
            reseed_status: vec![],
            pending_reseed: vec![],
            manual_reseeds: 0,
            activity: Default::default(),
            tile_size: Default::default(),
        }
    }

    /// `apply(SetTiling { tiling: topo.spec })` with the topology already built (the app builds
    /// large patches on a background thread). The result, log, and hash are the same.
    pub fn set_tiling_built(&mut self, topo: Arc<Topology>) -> Result<Applied, String> {
        let tiling = topo.spec.clone();
        self.prebuilt = Some(topo);
        let r = self.apply(Command::SetTiling { tiling });
        self.prebuilt = None;
        r
    }

    fn check_format(scene: &Scene) -> Result<(), String> {
        if scene.format > SCENE_FORMAT {
            return Err(format!(
                "scene format {} is newer than this JETCARP supports ({SCENE_FORMAT})",
                scene.format
            ));
        }
        Ok(())
    }

    pub fn from_scene(scene: &Scene) -> Result<World, String> {
        Self::check_format(scene)?;
        let topo = Arc::new(Topology::build(&scene.tiling)?);
        Self::from_scene_on(scene, topo)
    }

    /// [`from_scene`](Self::from_scene) on a tiling already built, for many scenes on one
    /// topology (Quick Start previews).
    pub fn from_scene_on(scene: &Scene, topo: Arc<Topology>) -> Result<World, String> {
        Self::check_format(scene)?;
        let mut w = World::with_topology(topo, scene.tiling.clone(), scene.rng_state);
        for t in &scene.tables {
            w.tables.insert(t.clone())?;
        }
        w.base.tables = scene.tables.clone();
        w.palettes = scene.palettes.clone();
        w.base.palettes = scene.palettes.clone();
        for spec in &scene.cas {
            w.add_ca(spec.clone())?;
        }
        w.base.cas = w.cas.iter().map(|c| c.spec()).collect();
        if let Some(sc) = &scene.script {
            w.start_script(Some(sc.clone()));
            w.base.script = Some(sc.clone());
        }
        if !scene.reseed.is_empty() {
            w.set_reseed_policies(scene.reseed.clone())?;
            w.base.reseed = scene.reseed.clone();
        }
        for (i, c) in scene.commands.iter().enumerate() {
            w.apply(c.clone())
                .map_err(|e| format!("scene command {}: {e}", i + 1))?;
        }
        Ok(w)
    }

    pub fn to_scene(&self) -> Scene {
        Scene {
            jetcarp_version: crate::VERSION.into(),
            format: SCENE_FORMAT,
            name: String::new(),
            description: String::new(),
            tiling: self.base.tiling.clone(),
            rng_state: self.base.rng_state,
            tables: self.base.tables.clone(),
            cas: self.base.cas.clone(),
            palettes: self.base.palettes.clone(),
            script: self.base.script.clone(),
            reseed: self.base.reseed.clone(),
            commands: self.log.clone(),
            view: None,
            record: Some(self.record()),
        }
    }

    pub fn record(&self) -> Record {
        Record {
            generation: self.generation,
            hash: self.hash_hex(),
            jetcarp_version: crate::VERSION.into(),
            dead_boundary: self.topo.has_boundary(),
        }
    }

    pub fn log(&self) -> &[Command] {
        &self.log
    }

    pub fn ca(&self, id: u32) -> Option<&CaInstance> {
        self.cas.iter().find(|c| c.id == id)
    }
    fn ca_index(&self, id: u32) -> Result<usize, String> {
        self.cas
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| format!("no CA with id {id}"))
    }

    /// Apply a command and record it in the log.
    pub fn apply(&mut self, cmd: Command) -> Result<Applied, String> {
        self.flush_pending_reseed();
        let mut out = Applied::default();
        // Library references are resolved to inline cells first, so the log replays without it.
        let cmd = match cmd {
            Command::PastePattern {
                id,
                pattern,
                at,
                orientation,
                phase,
            } => Command::PastePattern {
                pattern: crate::library::paste::resolve(&pattern, phase, None)?,
                id,
                at,
                orientation,
                phase: 0,
            },
            c => c,
        };
        match &cmd {
            Command::SetTiling { tiling } => {
                let topo = match self.prebuilt.take() {
                    Some(topo) if topo.spec == *tiling => topo,
                    _ => Arc::new(Topology::build(tiling)?),
                };
                self.rebuild(Some((topo, tiling.clone())), self.base.rng_state)?;
            }
            Command::SetRngState { state } => self.rebuild(None, *state)?,
            Command::Reset => self.rebuild(None, self.base.rng_state)?,
            Command::AddCa { ca } => out.new_id = Some(self.add_ca(ca.clone())?),
            Command::RemoveCa { id } => {
                let i = self.ca_index(*id)?;
                self.cas.remove(i);
                self.gc_fields();
            }
            Command::UpdateCa { id, change } => self.update_ca(*id, change)?,
            Command::Seed { id, seed } => {
                let i = self.ca_index(*id)?;
                self.apply_seed(i, seed)?;
            }
            Command::Clear { id } => {
                let i = self.ca_index(*id)?;
                let f = self.cas[i].field;
                self.fields[f].clear();
                self.cas[i].cursor = 0;
                self.cas[i].layers = None;
            }
            Command::SetCells { id, cells } => {
                let i = self.ca_index(*id)?;
                let n = self.topo.len();
                let f = self.cas[i].field;
                for &[c, v] in cells {
                    if (c as usize) < n {
                        self.fields[f].set(c as usize, v);
                    }
                }
            }
            Command::Step { gens } => {
                for _ in 0..*gens {
                    self.step();
                }
            }
            Command::Jump { gens } => {
                crate::hashlife::jump_world(self, *gens, &crate::hashlife::command_options())?;
            }
            Command::AddTable { table } => {
                self.tables.insert(table.clone())?;
            }
            Command::SetScript { script } => self.start_script(script.clone()),
            Command::Reseed { ca, mode } => self.reseed_now(*ca, mode)?,
            Command::SetReseed { policies } => self.set_reseed_policies(policies.clone())?,
            Command::Sandpile { id, action } => {
                let i = self.ca_index(*id)?;
                let fi = self.cas[i].field;
                let nb = self.cas[i].nb;
                if self.fields[fi].kind != FieldKind::Height {
                    return Err("sandpile tools need a sandpile CA".into());
                }
                let topo = self.topo.clone();
                let n = topo.len();
                match action {
                    SandpileOp::AddEverywhere { grains } => {
                        for c in 0..n {
                            if !topo.is_open(c) {
                                self.fields[fi].set(c, *grains);
                            }
                        }
                    }
                    SandpileOp::AddRandom { count } => {
                        for _ in 0..*count {
                            let c = self.rng.below(n as u64) as usize;
                            self.fields[fi].set(c, 1);
                        }
                    }
                    SandpileOp::Stabilize => {
                        crate::sandpile::stabilize(&topo, nb, &mut self.fields[fi])?;
                    }
                    SandpileOp::Identity => {
                        let h = crate::sandpile::identity(&topo, nb)?;
                        let f = &mut self.fields[fi];
                        f.height = h;
                        f.lost = 0;
                    }
                }
            }
            Command::SetPalette { palette } => {
                if palette.stops.is_empty() {
                    return Err("a palette needs at least one colour".into());
                }
                self.palettes.retain(|p| p.name != palette.name);
                self.palettes.push(palette.clone());
            }
            Command::PastePattern {
                id,
                pattern,
                at,
                orientation,
                ..
            } => {
                let i = self.ca_index(*id)?;
                let cells = crate::library::paste::paste_cells(
                    &self.topo,
                    self.cas[i].nb,
                    pattern,
                    at,
                    *orientation,
                )?;
                let f = self.cas[i].field;
                for [c, v] in cells {
                    self.fields[f].set(c as usize, v);
                }
            }
            Command::ScriptGoto { step } => {
                let n = self.script.as_ref().map(|s| s.steps.len()).unwrap_or(0);
                if *step >= n {
                    return Err(format!("script has {n} steps"));
                }
                self.enter_step(*step);
            }
        }
        if !matches!(cmd, Command::Step { .. }) {
            self.watch = PeriodWatch {
                power: 1,
                ..Default::default()
            };
            self.last_changed = None;
        }
        if cmd.rebases() {
            self.log.clear();
        } else {
            self.push_log(cmd);
        }
        self.rev = external::next_rev();
        Ok(out)
    }

    fn push_log(&mut self, cmd: Command) {
        match (self.log.last_mut(), cmd) {
            (Some(Command::Step { gens }), Command::Step { gens: g }) => *gens += g,
            (Some(Command::SetCells { id, cells }), Command::SetCells { id: id2, cells: c2 })
                if *id == id2 =>
            {
                cells.extend(c2)
            }
            (_, cmd) => self.log.push(cmd),
        }
    }

    fn rebuild(
        &mut self,
        topo: Option<(Arc<Topology>, TilingSpec)>,
        rng_state: u64,
    ) -> Result<(), String> {
        let specs: Vec<CaSpec> = self.cas.iter().map(|c| c.spec()).collect();
        let (topo, tiling) = topo.unwrap_or_else(|| (self.topo.clone(), self.base.tiling.clone()));
        let mut w = World::with_topology(topo, tiling, rng_state);
        // Tables added since the last base become part of the new base.
        w.base.tables = self.base.tables.clone();
        for cmd in &self.log {
            if let Command::AddTable { table } = cmd {
                w.base.tables.retain(|t| t.name != table.name);
                w.base.tables.push(table.clone());
            }
        }
        w.tables = self.tables.clone();
        w.palettes = self.palettes.clone();
        w.base.palettes = self.palettes.clone();
        let script = self.script.clone();
        for s in specs {
            // CAs that cannot run on the new tiling are dropped with an error.
            w.add_ca(s)?;
        }
        w.base.cas = w.cas.iter().map(|c| c.spec()).collect();
        if script.is_some() {
            w.start_script(script.clone());
        }
        w.base.script = script;
        w.track_changes = self.track_changes;
        let policies = self.reseed.clone();
        w.set_reseed_policies(policies.clone())?;
        w.base.reseed = policies;
        *self = w;
        Ok(())
    }

    // ------------------------------------------------------------------ scripts

    fn start_script(&mut self, script: Option<Script>) {
        self.script = script;
        self.script_state = ScriptState::default();
        if self.script.as_ref().is_some_and(|s| !s.steps.is_empty()) {
            self.enter_step(0);
        }
    }

    fn resolve(&self, r: &CaRef) -> Option<u32> {
        match r {
            CaRef::Id(id) => self.ca(*id).map(|c| c.id),
            CaRef::Name(n) => self
                .cas
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(n))
                .map(|c| c.id),
        }
    }

    /// Enter a script step: only its CAs run.
    fn enter_step(&mut self, k: usize) {
        let Some(sc) = &self.script else { return };
        let run: Vec<u32> = sc.steps[k]
            .run
            .iter()
            .filter_map(|r| self.resolve(r))
            .collect();
        for ca in &mut self.cas {
            ca.enabled = run.contains(&ca.id);
        }
        self.script_state = ScriptState {
            step: k,
            started: self.generation,
            ..Default::default()
        };
        self.watch = PeriodWatch {
            power: 1,
            ..Default::default()
        };
    }

    /// Fields used by the CAs that are currently running.
    fn running_fields(&self) -> Vec<usize> {
        let mut f: Vec<usize> = self
            .cas
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.field)
            .collect();
        f.sort_unstable();
        f.dedup();
        f
    }

    fn script_after_step(&mut self, changes: Option<(u64, u64)>, population: u64) {
        if self.script.is_none() || self.script_state.finished {
            return;
        }
        let needs_activity = self.script.as_ref().unwrap().steps[self.script_state.step]
            .until
            .needs_activity();
        let activity = changes
            .filter(|_| needs_activity)
            .map(|(changed, total)| changed as f64 / total.max(1) as f64);
        let done = self.script_wants_done().then(|| self.done());
        self.script_check(population, activity, done);
    }

    /// Check the running script step's trigger after a generation; true when it moved on.
    /// `done` is [`World::done`] (measured only when the step's trigger is `done`).
    fn script_check(&mut self, population: u64, activity: Option<f64>, done: Option<bool>) -> bool {
        let Some(sc) = self.script.clone() else {
            return false;
        };
        if self.script_state.finished {
            return false;
        }
        let k = self.script_state.step;
        let step = &sc.steps[k];
        self.script_state.last_activity = activity;
        let m = Metrics {
            generation: self.generation,
            population,
            activity,
            period: self.watch.found.map(|f| f.0),
            done: done.unwrap_or(false),
        };
        let timed_out = step
            .max_gens
            .is_some_and(|n| self.generation - self.script_state.started >= n);
        if script::fires(&step.until, &mut self.script_state, &m) || timed_out {
            if k + 1 < sc.steps.len() {
                self.enter_step(k + 1);
            } else if sc.repeat {
                self.enter_step(0);
            } else {
                self.script_state.finished = true;
            }
            return true;
        }
        false
    }

    fn field_for(&mut self, kind: FieldKind, affects: bool) -> usize {
        if affects
            && kind != FieldKind::Height
            && let Some(i) = self.fields.iter().position(|f| f.shared && f.kind == kind)
        {
            return i;
        }
        self.fields.push(Field::new(
            kind,
            self.topo.len(),
            affects && kind != FieldKind::Height,
        ));
        self.fields.len() - 1
    }

    fn gc_fields(&mut self) {
        let mut keep = vec![false; self.fields.len()];
        for c in &self.cas {
            keep[c.field] = true;
        }
        let mut remap = vec![usize::MAX; self.fields.len()];
        let mut j = 0;
        for (i, k) in keep.iter().enumerate() {
            if *k {
                remap[i] = j;
                j += 1;
            }
        }
        let mut i = 0;
        self.fields.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        for c in &mut self.cas {
            c.field = remap[c.field];
        }
    }

    fn check_rule(&self, rule: &Rule) -> Result<(), String> {
        if let Rule::Wolfram(_) = rule
            && self.topo.rows().is_none()
        {
            return Err("Elementary (Wolfram) rules need the square grid. Remove that CA before switching tilings.".into());
        }
        if let Rule::HexWolfram { .. } = rule
            && self.topo.hex_rows().is_none()
        {
            return Err("Hexagonal elementary rules need the hexagon grid. Remove that CA before switching tilings.".into());
        }
        if let Rule::HexTri { .. } = rule
            && self.topo.flat_hex_rows().is_none()
        {
            return Err("Three-parent hexagon rules (HexT) need the flat-top hexagon grid (hex_flat). Remove that CA before switching tilings.".into());
        }
        if let Rule::Ltl(l) = rule {
            crate::ltl::reach(&self.topo, l.hood, l.range.min(1))?;
        }
        if let Rule::Lenia(l) = rule {
            // Fails early when the kernel would be empty or too large for an aperiodic patch.
            if let crate::topology::Storage::Explicit(_) = self.topo.storage
                && self.topo.len() as f64 * std::f64::consts::PI * l.r * l.r > 60e6
            {
                return Err(format!(
                    "Lenia R={} on {} cells needs too many kernel taps; use a smaller R or patch",
                    l.r,
                    self.topo.len()
                ));
            }
        }
        if let Rule::Margolus(_) = rule
            && self.topo.rows().is_none()
        {
            return Err("Margolus block rules need the square grid. Remove that CA before switching tilings.".into());
        }
        if let Rule::VonNeumann = rule
            && self.topo.rows().is_none()
        {
            return Err("von Neumann's 29-state rule is direction dependent and needs the square grid. Remove that CA before switching tilings.".into());
        }
        if let Rule::Table(name) = rule
            && self.tables.get(name).is_none()
        {
            return Err(format!("no rule table named '{name}'"));
        }
        Ok(())
    }

    /// Colour stops for a palette name: user palettes first, then built-ins, else Ember.
    pub fn palette_stops(&self, name: &str) -> Vec<u32> {
        self.palette(name).packed()
    }
    /// A palette by name: user palettes first, then built-ins, else Ember.
    pub fn palette(&self, name: &str) -> Palette {
        self.palettes
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .or_else(|| Palette::builtin(name))
            .or_else(|| Palette::from_hex(name))
            .unwrap_or_else(|| Palette::builtin("Ember").unwrap())
    }
    /// All palette names: built-ins then user palettes.
    pub fn palette_names(&self) -> Vec<String> {
        let mut v: Vec<String> = crate::render::palette_names().map(String::from).collect();
        for p in &self.palettes {
            if !v.contains(&p.name) {
                v.push(p.name.clone());
            }
        }
        v
    }
    /// Whether [`palette`](Self::palette) knows `name` (one of the world's own palettes, a
    /// built-in, or hex colours like `#4fc3ff` or `#103050-#4fc3ff`) rather than falling back.
    /// Adding or changing a CA checks its palettes with this, so a scene, script, or command
    /// that names an unknown palette is refused with the names it could use.
    pub fn check_palette(&self, name: &str) -> Result<(), String> {
        let known = self.palettes.iter().any(|p| p.name == name)
            || Palette::builtin(name).is_some()
            || Palette::from_hex(name).is_some();
        if known {
            return Ok(());
        }
        Err(format!(
            "no palette named '{name}'. Palettes: {}, or colours like #4fc3ff or #103050-#4fc3ff",
            self.palette_names().join(", ")
        ))
    }

    /// The field a rule needs; table rules take their state count from the table.
    pub fn kind_of(&self, rule: &Rule) -> FieldKind {
        match rule {
            Rule::Table(name) => match self.tables.get(name) {
                Some(t) if t.table.states == 2 => FieldKind::Binary,
                Some(t) => FieldKind::Multi(t.table.states),
                None => FieldKind::Multi(2),
            },
            r => r.field_kind(),
        }
    }
    fn table_for(&self, rule: &Rule) -> Option<Arc<Compiled>> {
        match rule {
            Rule::Table(name) => self.tables.get(name),
            _ => None,
        }
    }

    pub fn add_ca(&mut self, spec: CaSpec) -> Result<u32, String> {
        let (rule, nb_from_text) = Rule::parse(&spec.rule)?;
        self.check_rule(&rule)?;
        // A misspelt palette is refused rather than quietly drawn in the default.
        for p in [&spec.palette, &spec.palette_neg].into_iter().flatten() {
            self.check_palette(p)?;
        }
        let kind = self.kind_of(&rule);
        let affects = if kind == FieldKind::Height {
            false
        } else {
            spec.affects_others
                .unwrap_or_else(|| rule.default_affects_others())
        };
        let nb = if rule.nb_fixed() {
            rule.default_nb()
        } else {
            spec.neighborhood
                .or(nb_from_text)
                .unwrap_or_else(|| rule.default_nb())
        };
        let id = match spec.id {
            Some(id) if !self.cas.iter().any(|c| c.id == id) => id,
            _ => self.next_id,
        };
        self.next_id = self.next_id.max(id + 1);
        let field = self.field_for(kind, affects);
        // A B/S rule joining a shared field that already has cells starts from those cells
        // rather than scattering random ones over another CA's work.
        let joins_busy_field = self.fields[field].shared && !self.fields[field].is_empty();
        let seed = spec.seed.clone().unwrap_or_else(|| match rule {
            Rule::LifeLike { .. } | Rule::Generations { .. } if joins_busy_field => SeedSpec::Empty,
            _ => default_seed(&rule, self.topo.len()),
        });
        let table = self.table_for(&rule);
        let nb = match &table {
            Some(t) if spec.neighborhood.is_none() => t.table.neighborhood,
            _ => nb,
        };
        let noise = self.rng.next_u64();
        let is_jaca = matches!(rule, Rule::Jaca(_));
        let ca = CaInstance {
            id,
            name: spec.name.clone().unwrap_or_else(|| {
                crate::rules::catalog_name_on(&rule, nb, &self.topo)
                    .map(String::from)
                    .unwrap_or_else(|| rule.notation())
            }),
            rule,
            nb,
            enabled: spec.enabled,
            affects_others: affects,
            steps_per_gen: spec.steps_per_gen.clamp(1, 100_000),
            palette: spec
                .palette
                .clone()
                .unwrap_or_else(|| if is_jaca { "Tide" } else { "Ember" }.into()),
            palette_neg: spec.palette_neg.clone().unwrap_or_else(|| "Ember".into()),
            glow: spec.glow,
            heat: None,
            seed: seed.clone(),
            drop_per_gen: spec.drop_per_gen,
            field,
            agents: vec![],
            cursor: 0,
            table,
            reach: None,
            kernel: None,
            layers: None,
            noise,
            ticks: 0,
            color_by: spec.color_by,
            agent_edge: spec.agent_edge,
        };
        self.cas.push(ca);
        let i = self.cas.len() - 1;
        self.apply_seed(i, &seed)?;
        Ok(id)
    }

    fn update_ca(&mut self, id: u32, ch: &CaChange) -> Result<(), String> {
        let i = self.ca_index(id)?;
        // Checked before anything changes, so a refused change leaves the CA as it was.
        for p in [&ch.palette, &ch.palette_neg].into_iter().flatten() {
            self.check_palette(p)?;
        }
        if let Some(r) = &ch.rule {
            let (rule, nb) = Rule::parse(r)?;
            self.check_rule(&rule)?;
            let old_kind = self.fields[self.cas[i].field].kind;
            let new_kind = self.kind_of(&rule);
            let table = self.table_for(&rule);
            let was_agent = self.cas[i].rule.is_agent();
            let ca = &mut self.cas[i];
            if ch.name.is_none() && ca.name == ca.rule.display_name() {
                ca.name = rule.display_name();
            }
            if rule.nb_fixed() {
                ca.nb = rule.default_nb();
            } else if let Some(nb) = nb {
                ca.nb = nb;
            } else if ca.rule.nb_fixed() {
                ca.nb = rule.default_nb();
            }
            if let Some(t) = &table {
                ca.nb = t.table.neighborhood;
            }
            ca.table = table;
            ca.reach = None;
            ca.kernel = None;
            ca.layers = None;
            ca.heat = None;
            ca.ticks = 0;
            let is_agent = rule.is_agent();
            let default = default_seed(&rule, self.topo.len());
            let seed_kind_changed = std::mem::discriminant(&default_seed(&ca.rule, 1))
                != std::mem::discriminant(&default);
            ca.rule = rule;
            if seed_kind_changed {
                ca.seed = default.clone();
            }
            if new_kind != old_kind {
                let affects = new_kind != FieldKind::Height && ca.affects_others;
                self.cas[i].affects_others = affects;
                self.cas[i].field = self.field_for(new_kind, affects);
                self.cas[i].agents.clear();
                self.gc_fields();
                let seed = self.cas[i].seed.clone();
                self.apply_seed(i, &seed)?;
            } else if is_agent && !was_agent {
                self.apply_seed(i, &default)?;
            }
        }
        let ca = &mut self.cas[i];
        if let Some(n) = &ch.name {
            ca.name = n.clone();
        }
        if let Some(nb) = ch.neighborhood
            && !ca.rule.nb_fixed()
        {
            ca.nb = nb;
        }
        if let Some(e) = ch.enabled {
            ca.enabled = e;
        }
        if let Some(s) = ch.steps_per_gen {
            ca.steps_per_gen = s.clamp(1, 100_000);
        }
        if let Some(p) = &ch.palette {
            ca.palette = p.clone();
        }
        if let Some(d) = ch.drop_per_gen {
            ca.drop_per_gen = d;
        }
        if let Some(s) = &ch.seed {
            ca.seed = s.clone();
        }
        if let Some(c) = ch.color_by {
            ca.color_by = c;
        }
        if let Some(e) = ch.agent_edge {
            ca.agent_edge = e;
        }
        if let Some(p) = &ch.palette_neg {
            ca.palette_neg = p.clone();
        }
        if let Some(g) = ch.glow {
            ca.glow = g;
        }
        if let Some(a) = ch.affects_others {
            self.set_affects(i, a);
        }
        Ok(())
    }

    fn set_affects(&mut self, i: usize, affects: bool) {
        let kind = self.fields[self.cas[i].field].kind;
        if kind == FieldKind::Height || self.cas[i].affects_others == affects {
            return;
        }
        let old = self.cas[i].field;
        if affects {
            // Join the shared field, merging this CA's private cells into it.
            let target = self.fields.iter().position(|f| f.shared && f.kind == kind);
            match target {
                Some(t) => {
                    let (src_s, src_a, src_v) = (
                        self.fields[old].state.clone(),
                        self.fields[old].age.clone(),
                        self.fields[old].value.clone(),
                    );
                    let dst = &mut self.fields[t];
                    for k in 0..src_s.len() {
                        if src_s[k] != 0 {
                            dst.state[k] = src_s[k];
                            dst.age[k] = src_a[k];
                            if let Some(&v) = src_v.get(k) {
                                dst.value[k] = v;
                            }
                        }
                    }
                    self.cas[i].field = t;
                }
                None => {
                    let only_user = self.cas.iter().filter(|c| c.field == old).count() == 1;
                    if only_user {
                        self.fields[old].shared = true;
                    } else {
                        let mut f = self.fields[old].clone();
                        f.shared = true;
                        self.fields.push(f);
                        self.cas[i].field = self.fields.len() - 1;
                    }
                }
            }
        } else {
            // Leave with a private copy of the shared field, drawn alongside it.
            let mut f = self.fields[old].clone();
            f.shared = false;
            self.fields.push(f);
            self.cas[i].field = self.fields.len() - 1;
        }
        self.cas[i].affects_others = affects;
        self.gc_fields();
    }

    fn apply_seed(&mut self, i: usize, seed: &SeedSpec) -> Result<(), String> {
        let topo = self.topo.clone();
        let n = topo.len();
        let fi = self.cas[i].field;
        let rng = &mut self.rng;
        let field = &mut self.fields[fi];
        let ca = &mut self.cas[i];
        if matches!(ca.rule, Rule::Layered { .. }) {
            ca.layers = None;
            ca.cursor = 0;
        }
        match seed {
            SeedSpec::Empty => {}
            SeedSpec::Random { density } => {
                for c in 0..n {
                    if field.kind == FieldKind::Continuous {
                        // A random value in (0, 1] with this probability.
                        if rng.next_f64() < *density {
                            field.set_value(c, 1.0 - rng.next_f64() as f32);
                        }
                    } else if field.kind == FieldKind::Height {
                        let d = topo.degree(c, ca.nb).max(1) as u64;
                        if rng.next_f64() < *density {
                            field.height[c] = rng.below(d) as u32;
                        }
                    } else if rng.next_f64() < *density {
                        field.state[c] = 1;
                        field.age[c] = 1;
                    }
                }
            }
            SeedSpec::Center => field.set_value(topo.center_cell(), 1.0),
            SeedSpec::Blob { radius, density } => {
                let b = topo.bounds;
                let c0 = topo.centroid(topo.center_cell());
                let r = radius * (b[2] - b[0]).min(b[3] - b[1]) / 2.0;
                for c in 0..n {
                    if crate::detmath::dist(topo.centroid(c), c0) <= r && rng.next_f64() < *density
                    {
                        if field.kind == FieldKind::Continuous {
                            field.set_value(c, 1.0 - rng.next_f64() as f32);
                        } else {
                            field.set(c, 1);
                        }
                    }
                }
            }
            SeedSpec::Signed { density, negative } => {
                let FieldKind::Multi(3) = field.kind else {
                    return Err("signed seeds need a JACA CA".into());
                };
                for c in 0..n {
                    if rng.next_f64() < *density {
                        let s = if rng.next_f64() < *negative {
                            crate::jaca::NEG
                        } else {
                            crate::jaca::POS
                        };
                        field.set(c, s as u32);
                    }
                }
            }
            SeedSpec::Halves { density } => {
                let FieldKind::Multi(3) = field.kind else {
                    return Err("half-and-half seeds need a JACA CA".into());
                };
                let b = topo.bounds;
                let mid = (b[0] + b[2]) / 2.0;
                for c in 0..n {
                    if rng.next_f64() < *density {
                        let s = if topo.centroid(c)[0] < mid {
                            crate::jaca::POS
                        } else {
                            crate::jaca::NEG
                        };
                        field.set(c, s as u32);
                    }
                }
            }
            SeedSpec::RandomStates if field.kind == FieldKind::Continuous => {
                for c in 0..n {
                    field.set_value(c, rng.next_f64() as f32);
                }
            }
            SeedSpec::RandomStates => {
                let s = match field.kind {
                    FieldKind::Multi(k) => k as u64,
                    FieldKind::Binary => 2,
                    FieldKind::Height | FieldKind::Continuous => {
                        return Err("random states need a state field".into());
                    }
                };
                for c in 0..n {
                    let v = rng.below(s) as u8;
                    field.state[c] = v;
                    field.age[c] = (v != 0) as u16;
                }
            }
            SeedSpec::TopCenter | SeedSpec::TopRandom { .. } => {
                // On the flat-top hexagon grid the top layer is the type-0 hexagons of row 0.
                let g = topo
                    .rows()
                    .or_else(|| topo.hex_rows())
                    .or_else(|| topo.flat_hex_rows())
                    .ok_or("top-row seeds need the square or hexagon grid")?;
                for u in 0..g.w as i32 {
                    let on = match seed {
                        SeedSpec::TopCenter => u == g.w as i32 / 2,
                        SeedSpec::TopRandom { density } => rng.next_f64() < *density,
                        _ => unreachable!(),
                    };
                    if on {
                        field.set(g.index(u, 0, 0), 1);
                    }
                }
                ca.cursor = 0;
            }
            SeedSpec::Ants { count } => {
                if !ca.rule.is_agent() {
                    return Err("ant seeds apply only to turmites".into());
                }
                for k in 0..*count {
                    let (cell, heading) = if k == 0 && ca.agents.is_empty() {
                        (topo.center_cell(), 0)
                    } else {
                        let c = rng.below(n as u64) as usize;
                        (
                            c,
                            rng.below(topo.degree(c, Nb::Edge).max(1) as u64) as usize,
                        )
                    };
                    ca.agents.push(Agent {
                        cell: cell as u32,
                        heading: heading as u16,
                        state: 0,
                    });
                }
            }
            SeedSpec::AntAt { cell, heading } => {
                if !ca.rule.is_agent() {
                    return Err("ant seeds apply only to turmites".into());
                }
                let c = *cell as usize;
                if c >= n || (*heading as usize) >= topo.degree(c, Nb::Edge).max(1) {
                    return Err(format!("no cell {cell} with edge slot {heading}"));
                }
                ca.agents.push(Agent {
                    cell: *cell,
                    heading: *heading,
                    state: 0,
                });
            }
            SeedSpec::Pile { grains } => {
                if field.kind != FieldKind::Height {
                    return Err("pile seeds apply only to sandpiles".into());
                }
                let c = topo.center_cell();
                field.height[c] =
                    field.height[c].saturating_add((*grains).min(u32::MAX as u64) as u32);
            }
            SeedSpec::GliderPair => {
                let cs: Vec<_> = (0..n).map(|c| topo.centroid(c)).collect();
                let c0 = topo.centroid(topo.center_cell());
                let mut order: Vec<usize> = (0..n).filter(|&c| topo.is_interior(c)).collect();
                order.sort_by(|&a, &b| {
                    crate::detmath::dist(cs[a], c0)
                        .partial_cmp(&crate::detmath::dist(cs[b], c0))
                        .unwrap()
                        .then(a.cmp(&b))
                });
                if order.is_empty() {
                    return Err("tiling too small for a glider pair".into());
                }
                // Candidate pairs near the centre, starting from an RNG-chosen one. Not every pair
                // launches a glider, so test each on a scratch field and keep the first that travels.
                let mut cands = Vec::new();
                for &t in order.iter().take(12) {
                    for k in 0..topo.degree(t, Nb::Edge) {
                        if let Some(u) = topo.slot(t, Nb::Edge, k) {
                            cands.push((t, u));
                        }
                    }
                }
                let start = rng.below(cands.len() as u64) as usize;
                let mut chosen = cands[start];
                for k in 0..cands.len() {
                    let (t, u) = cands[(start + k) % cands.len()];
                    if glider_travels(&topo, ca.nb, t, u, &cs) {
                        chosen = (t, u);
                        break;
                    }
                }
                field.set(chosen.0, 1);
                field.set(chosen.1, 3);
            }
            SeedSpec::WireDemo => {
                let g = topo
                    .rows()
                    .ok_or("the WireWorld demo needs the square grid")?;
                wire_demo(g, field);
            }
            SeedSpec::Circuit { sites, electrons } => {
                if field.kind != FieldKind::Multi(4) {
                    return Err("circuit seeds need a four-state rule (WireWorld)".into());
                }
                let shape = crate::seeding::SeedShape::Voronoi {
                    sites: (*sites).max(2),
                    keep: 1.0,
                };
                let at = crate::seeding::place(&topo, &shape, rng)?;
                // One side of each border only, so wires are one cell thick.
                let mut wires = Vec::new();
                for c in 0..n {
                    let mut border = false;
                    topo.for_each_neighbor(c, Nb::Edge, |j| {
                        border |= at.region[j] < at.region[c];
                    });
                    if border {
                        field.set(c, 3);
                        wires.push(c);
                    }
                }
                if wires.is_empty() {
                    return Err("tiling too small for a circuit".into());
                }
                for _ in 0..*electrons {
                    let head = wires[rng.below(wires.len() as u64) as usize];
                    let mut tail = None;
                    topo.for_each_neighbor(head, Nb::Edge, |j| {
                        if field.state[j] == 3 {
                            tail = Some(j);
                        }
                    });
                    field.set(head, 1);
                    if let Some(t) = tail {
                        field.set(t, 2);
                    }
                }
            }
            SeedSpec::Cells { cells } => {
                for &[c, v] in cells {
                    if (c as usize) < n {
                        field.set(c as usize, v);
                    }
                }
            }
            SeedSpec::Gaussian {
                count,
                radius,
                peak,
            } => {
                if field.kind != FieldKind::Continuous {
                    return Err("Gaussian seeds need a continuous field (Lenia)".into());
                }
                let cs: Vec<crate::geom::P> = (0..n).map(|c| topo.centroid(c)).collect();
                let size = cell_size(&topo);
                let sigma = (radius * size).max(1e-9);
                for k in 0..*count {
                    let c0 = if k == 0 {
                        topo.center_cell()
                    } else {
                        rng.below(n as u64) as usize
                    };
                    for (c, p) in cs.iter().enumerate() {
                        let d = crate::detmath::dist(*p, cs[c0]);
                        if d <= 3.0 * sigma {
                            let add = peak * crate::detmath::exp(-(d * d) / (2.0 * sigma * sigma));
                            let v = field.value[c] + add as f32;
                            field.set_value(c, v);
                        }
                    }
                }
            }
            SeedSpec::Pattern { shape, fill } => {
                crate::seeding::apply(&topo, &ca.rule, ca.nb, field, shape, fill, rng)?;
            }
            SeedSpec::Lenia { name, scale } => {
                if field.kind != FieldKind::Continuous {
                    return Err("Lenia patterns need a continuous field (Lenia)".into());
                }
                let pat = crate::lenia::pattern(name)
                    .ok_or_else(|| format!("no Lenia pattern named '{name}' (try orbium)"))?;
                let (ph, pw) = (pat.len() as f64, pat.first().map_or(0, |r| r.len()) as f64);
                let size = cell_size(&topo) * scale.max(0.05);
                let c0 = topo.centroid(topo.center_cell());
                // Every cell takes the pattern value under its centroid (pattern centred).
                for c in 0..n {
                    let p = topo.centroid(c);
                    let (x, y) = (
                        (p[0] - c0[0]) / size + pw / 2.0,
                        (p[1] - c0[1]) / size + ph / 2.0,
                    );
                    if x >= 0.0 && y >= 0.0 && x < pw && y < ph {
                        let v = pat[y as usize][x as usize];
                        if v > 0.0 {
                            field.set_value(c, v);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Advance one generation: each enabled CA, in list order, runs its steps per generation.
    pub fn step(&mut self) {
        self.flush_pending_reseed();
        let before = (self.track_changes
            || self.script_wants_changes()
            || self.reseed_wants_changes(self.generation + 1))
        .then(|| self.running_snapshot());
        let topo = self.topo.clone();
        for ca in self.cas.iter_mut() {
            if !ca.enabled {
                continue;
            }
            let field = &mut self.fields[ca.field];
            for s in 0..ca.steps_per_gen {
                step_ca(&topo, ca, field, s == 0);
            }
        }
        self.generation += 1;
        self.watch_step();
        let changes = before.map(|b| self.count_changes(&b));
        self.last_changed = changes.map(|c| c.0);
        if let Some((c, t)) = changes {
            self.push_activity(c, t);
        }
        // Measured over the CAs that ran this generation, before a script step changes them.
        let fields = self.running_fields();
        let population = if (self.script.is_some() && !self.script_state.finished)
            || self.reseed_wants_population()
        {
            fields.iter().map(|&f| self.fields[f].population()).sum()
        } else {
            0
        };
        self.script_after_step(changes, population);
        let activity = changes.map(|(c, t)| c as f64 / t.max(1) as f64);
        self.reseed_after_step(activity, population, !fields.is_empty(), false);
        self.rev = external::next_rev();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            values: self.fields.iter().map(|f| f.value.clone()).collect(),
            states: self.fields.iter().map(|f| f.state.clone()).collect(),
            heights: self.fields.iter().map(|f| f.height.clone()).collect(),
            agents: self.cas.iter().map(|c| c.agents.clone()).collect(),
            cursors: self.cas.iter().map(|c| c.cursor).collect(),
            phases: self.phases(),
        }
    }
    /// Per CA, the step parity of phase-dependent rules (Margolus partitions), else 0.
    pub(crate) fn phases(&self) -> Vec<u64> {
        self.cas
            .iter()
            .map(|c| {
                if matches!(c.rule, Rule::Margolus(_)) {
                    c.ticks % 2
                } else {
                    0
                }
            })
            .collect()
    }
    fn matches_snapshot(&self, s: &Snapshot) -> bool {
        self.fields.len() == s.states.len()
            && self
                .fields
                .iter()
                .zip(&s.states)
                .all(|(f, x)| &f.state == x)
            && self
                .fields
                .iter()
                .zip(&s.heights)
                .all(|(f, x)| &f.height == x)
            && self.cas.iter().zip(&s.agents).all(|(c, x)| &c.agents == x)
            && self.cas.iter().zip(&s.cursors).all(|(c, x)| c.cursor == *x)
            && self.phases() == s.phases
            && self
                .fields
                .iter()
                .zip(&s.values)
                .all(|(f, x)| &f.value == x)
    }
    fn watch_step(&mut self) {
        if self.watch.found.is_some() {
            return;
        }
        self.watch.lam += 1;
        if let Some(s) = &self.watch.snap
            && self.matches_snapshot(s)
        {
            self.watch.found = Some((self.watch.lam, self.generation));
            return;
        }
        if self.watch.lam == self.watch.power {
            self.watch.snap = Some(self.snapshot());
            self.watch.power *= 2;
            self.watch.lam = 0;
        }
    }

    pub fn status(&self) -> Status {
        if let Some((period, seen_at)) = self.watch.found {
            return Status::Repeats { period, seen_at };
        }
        if self.fields.iter().all(|f| f.is_empty()) && self.cas.iter().all(|c| c.agents.is_empty())
        {
            return Status::Empty;
        }
        Status::Running
    }

    /// FNV-1a over generation, every field, and every CA's dynamic state.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv::default();
        h.u64(self.generation);
        for f in &self.fields {
            h.bytes(format!("{:?}{}", f.kind, f.shared).as_bytes());
            h.bytes(&f.state);
            h.u16s(&f.age);
            h.u32s(&f.height);
            // Continuous values by their exact bits (empty for other fields).
            h.f32s(&f.value);
            h.u64(f.lost);
        }
        if self.script.is_some() {
            h.u64(self.script_state.step as u64);
            h.u64(self.script_state.started);
            h.u64(self.script_state.finished as u64);
        }
        for st in &self.reseed_status {
            h.u64(st.since);
            h.u64(st.streak);
            h.u64(st.fired);
        }
        for c in &self.cas {
            h.u64(c.id as u64);
            h.u64(c.cursor as u64);
            h.u64(c.ticks);
            h.u64(c.enabled as u64);
            for a in &c.agents {
                h.u64(a.cell as u64);
                h.u64(a.heading as u64);
            }
        }
        h.0
    }
    pub fn hash_hex(&self) -> String {
        format!("{:016x}", self.state_hash())
    }

    /// FNV-1a over every field's cell states only (no generation, ages, heights, values, or CA
    /// bookkeeping), to compare with engines that do not track ages, such as HashLife.
    pub fn states_hash(&self) -> u64 {
        let mut h = Fnv::default();
        for f in &self.fields {
            h.bytes(&f.state);
        }
        h.0
    }
}
