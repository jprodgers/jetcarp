//! `world.*`: operations on a world: make one, open a scene, report its status, read and inspect
//! cells, draw, paste a catalog pattern, reseed, apply commands, step, render, save.
//!
//! They act through a [`Host`], so in the app every change goes through the app's command path
//! (logged, undoable, shown at once) and headless front ends use a [`crate::world::Session`].
//! Limits come from [`Ctx::limits`]; the MCP tools' texts are these operations' [`Report`]s.

use crate::args::mcp::{parse_ca, parse_seed, resolve_rule};
use crate::args::{self, rule::squash};
use crate::cells::{self, Grid};
use crate::ctx::{Ctx, Limits};
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Progress, Report, WorldOp, to_pretty};
use crate::ops::scene::{Recorded, read_scene};
use crate::world::Host;
use jetcarp_core::pattern::{self, PasteAt};
use jetcarp_core::render::{self, RenderOptions};
use jetcarp_core::reseed::{AutoReseed, ReseedMode};
use jetcarp_core::rules::{FieldKind, Rule};
use jetcarp_core::world::{CaInstance, CaSpec, Command};
use jetcarp_core::{
    Boundary, Nb, Scene, SeedSpec, SizeSpec, Status, TilingSpec, Topology, World, families,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;

/// How far `world.step` runs with `stop_when_settled` when no count is given.
pub const SETTLE_GENS: u64 = 5000;

/// The schema of a CA reference: an id number or a name.
fn ca_ref_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": ["integer", "string"],
        "description": "Which CA: its id number or its name (world.status lists them). Default: the first CA."
    })
}

/// A signed 64-bit integer as the front ends read them.
type Int = i64;

// ---------------------------------------------------------------------------------------------
// Checks and lookups

/// Largest aperiodic level within `max_cells` tiles (Spectre 7, hat and Penrose 6 for the MCP
/// server's 4 million).
pub fn max_level(family: &str, max_cells: u64) -> u32 {
    let (lo, hi) = jetcarp_core::aperiodic::level_range(family);
    (lo..=hi)
        .rev()
        .find(|&l| {
            jetcarp_core::aperiodic::tile_count(family, l).is_none_or(|n| n as u64 <= max_cells)
        })
        .unwrap_or(lo)
}

/// A tiling's size against the limits (an opened scene, `set_tiling`).
pub fn check_tiling_size(spec: &TilingSpec, l: &Limits) -> Result<(), ApiError> {
    match spec.size {
        Some(SizeSpec::Rect { w, h }) => {
            if w < 8 || h < 8 || w > l.max_grid_side || h > l.max_grid_side {
                return Err(ApiError::invalid(format!(
                    "width and height must be 8 to {}, got {w} x {h}",
                    l.max_grid_side
                )));
            }
            if w as u64 * h as u64 > l.max_cells {
                return Err(ApiError::limit(format!(
                    "{w} x {h} is too large; keep width x height at most {}",
                    l.max_cells
                )));
            }
        }
        Some(SizeSpec::Cells(n)) if n as u64 > l.max_cells => {
            return Err(ApiError::limit(format!("at most {} cells", l.max_cells)));
        }
        Some(SizeSpec::Level(lv)) => {
            let hi = max_level(&spec.family, l.max_cells);
            if !(1..=hi).contains(&lv) {
                return Err(ApiError::invalid(format!("level must be 1 to {hi}")));
            }
        }
        _ => {}
    }
    Ok(())
}

/// `gens` generations of `w` against the limits.
pub fn check_work(w: &World, gens: u64, l: &Limits) -> Result<(), ApiError> {
    if gens > l.max_gens {
        return Err(ApiError::limit(format!(
            "at most {} generations per call",
            l.max_gens
        )));
    }
    let work = gens.saturating_mul(w.topo.len() as u64);
    if work > l.max_work {
        let ok = (l.max_work / w.topo.len().max(1) as u64).max(1);
        return Err(ApiError::limit(format!(
            "{gens} generations on {} cells is too much for one call; run at most {ok} at a time",
            w.topo.len()
        )));
    }
    Ok(())
}

/// At most `max_ants` ants.
fn cap_seed(seed: &mut SeedSpec, l: &Limits) {
    if let SeedSpec::Ants { count } = seed {
        *count = (*count).min(l.max_ants);
    }
}

/// A CA item ([`parse_ca`]) within the limits.
fn ca_item(v: &Value, l: &Limits) -> Result<CaSpec, ApiError> {
    let mut spec = parse_ca(v).map_err(ApiError::invalid)?;
    if let Some(s) = spec.seed.as_mut() {
        cap_seed(s, l);
    }
    Ok(spec)
}

/// Index into `world.cas` for a CA reference: its id (a number, or a number as text), its name
/// (any case, ignoring punctuation), or its rule; the first CA when there is none.
pub fn ca_index(w: &World, v: Option<&Value>) -> Result<usize, ApiError> {
    if w.cas.is_empty() {
        return Err(ApiError::invalid(
            "this world has no CAs yet. Make a world with world_new (e.g. cas: [\"Life\"]), or add a CA with the command add_ca (world_edit commands).",
        ));
    }
    let list = || {
        w.cas
            .iter()
            .map(|c| format!("id {} '{}' ({})", c.id, c.name, c.notation()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let found = match v {
        None | Some(Value::Null) => Some(0),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|id| w.cas.iter().position(|c| c.id as u64 == id)),
        Some(Value::String(s)) => {
            let s = s.trim();
            s.parse::<u32>()
                .ok()
                .and_then(|id| w.cas.iter().position(|c| c.id == id))
                .or_else(|| w.cas.iter().position(|c| c.name.eq_ignore_ascii_case(s)))
                .or_else(|| w.cas.iter().position(|c| squash(&c.name) == squash(s)))
                .or_else(|| {
                    let n = resolve_rule(s).to_ascii_uppercase();
                    w.cas
                        .iter()
                        .position(|c| c.notation().to_ascii_uppercase() == n)
                })
        }
        Some(_) => None,
    };
    found.ok_or_else(|| {
        ApiError::not_found(format!(
            "no CA {}. CAs: {}",
            v.cloned().unwrap_or(Value::Null),
            list()
        ))
    })
}

fn field_of(w: &World, ci: usize) -> &jetcarp_core::world::Field {
    &w.fields[w.cas[ci].field]
}

// ---------------------------------------------------------------------------------------------
// Status

/// The tiling in a status report.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TilingStatus {
    pub id: String,
    pub name: String,
    /// `wrap`, `dead`, or `live`.
    pub boundary: String,
    /// `{width, height, tile_types_per_unit_cell, coordinates}`, or a note that an aperiodic
    /// patch has no grid (cells are addressed by index).
    pub grid: Value,
    pub description: String,
}

/// One CA in a status report.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CaStatus {
    pub id: u32,
    pub name: String,
    pub rule: String,
    pub enabled: bool,
    pub field: usize,
    /// `touch` or `edge`.
    pub neighborhood: String,
    pub affects_others: bool,
    pub steps_per_gen: u32,
    pub palette: String,
    /// Agent rules: how many ants.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ants: Option<usize>,
    /// JACA: the palette of -1 cells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette_neg: Option<String>,
    /// JACA: cells touching the opposite sign, destroyed, and births cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annihilation: Option<Value>,
}

/// One field in a status report.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FieldStatus {
    pub field: usize,
    pub kind: String,
    pub shared: bool,
    pub population: u64,
    pub ca_ids: Vec<u32>,
    /// Sandpiles: what `population` counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub population_means: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grains_lost_off_edges: Option<u64>,
    /// Sandpiles: no cell can topple.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_state: Option<u8>,
    /// JACA: +1 cells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positive: Option<usize>,
    /// JACA: -1 cells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative: Option<usize>,
}

/// A scene script's progress.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ScriptStatus {
    pub steps: usize,
    pub current_step: usize,
    pub current_name: Option<String>,
    pub finished: bool,
}

/// A world's status: what `world_status` reports.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WorldStatus {
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    /// Running, empty, frozen, or repeating with a period (checked exactly).
    pub status: String,
    /// Whether anything can still change, and why not.
    pub done: String,
    pub hash: String,
    pub hash_note: String,
    /// Hash of the cell states only: equal when the picture repeats.
    pub cells_hash: String,
    pub cells: usize,
    pub tiling: TilingStatus,
    pub cas: Vec<CaStatus>,
    pub fields: Vec<FieldStatus>,
    pub script: Option<ScriptStatus>,
    /// Facts about the host (headless, or the app and whether it is playing).
    pub host: Value,
}

impl Report for WorldStatus {
    fn text(&self) -> String {
        to_pretty(self)
    }
}

fn nb_name(nb: Nb) -> &'static str {
    match nb {
        Nb::Touch => "touch",
        Nb::Edge => "edge",
    }
}

fn ca_status(c: &CaInstance) -> CaStatus {
    let jaca = matches!(c.rule, Rule::Jaca(_));
    CaStatus {
        id: c.id,
        name: c.name.clone(),
        rule: c.notation(),
        enabled: c.enabled,
        field: c.field,
        neighborhood: nb_name(c.nb).into(),
        affects_others: c.affects_others,
        steps_per_gen: c.steps_per_gen,
        palette: c.palette.clone(),
        ants: c.rule.is_agent().then_some(c.agents.len()),
        palette_neg: jaca.then(|| c.palette_neg.clone()),
        annihilation: jaca.then(|| {
            let h = c.heat.as_deref().cloned().unwrap_or_default();
            json!({
                "last_touching": h.last.contacts, "last_destroyed": h.last.destroyed,
                "last_cancelled_births": h.last.cancelled,
                "total_touching": h.total_contacts, "total_destroyed": h.total_destroyed,
            })
        }),
    }
}

fn boundary_name(t: &Topology) -> &'static str {
    if t.wraps() {
        "wrap"
    } else if t.live_edges() {
        "live"
    } else {
        "dead"
    }
}

/// A height field is stable when every cell (sinks aside) holds fewer grains than it has
/// neighbours, so nothing can topple.
fn sandpile_stable(w: &World, field: usize) -> bool {
    let Some(ca) = w.cas.iter().find(|c| c.field == field) else {
        return true;
    };
    let (t, f) = (&w.topo, &w.fields[field]);
    (0..t.len()).all(|i| t.is_open(i) || (f.height[i] as usize) < t.degree(i, ca.nb))
}

/// The world's status in words. A world whose cells are all dead reads "Empty" even when the
/// exact repeat check has also seen it (an empty world is trivially frozen).
pub fn status_text(w: &World) -> String {
    let dead = w.fields.iter().all(|f| f.is_empty()) && w.cas.iter().all(|c| c.agents.is_empty());
    match w.status() {
        Status::Repeats { seen_at, .. } if dead => {
            format!("Empty: every cell is dead (died out by generation {seen_at})")
        }
        Status::Empty => "Empty: every cell is dead".into(),
        s => s.to_string(),
    }
}

/// The world's status.
pub fn world_status(host: &dyn Host) -> WorldStatus {
    let w = host.world();
    let t = &w.topo;
    let grid = Grid::of(t).map(|g| {
        json!({
            "width": g.w, "height": g.h, "tile_types_per_unit_cell": g.types,
            "coordinates": "x = column 0..width-1, y = row 0..height-1 from the top; index = y * width + x",
        })
    });
    let fields = w
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let height = f.kind == FieldKind::Height;
            let jaca = w
                .cas
                .iter()
                .any(|c| c.field == i && matches!(c.rule, Rule::Jaca(_)));
            let count = |s: u8| f.state.iter().filter(|&&x| x == s).count();
            FieldStatus {
                field: i,
                kind: f.kind.name().to_string(),
                shared: f.shared,
                population: f.population(),
                ca_ids: w
                    .cas
                    .iter()
                    .filter(|c| c.field == i)
                    .map(|c| c.id)
                    .collect(),
                population_means: height.then(|| "total grains".to_string()),
                grains_lost_off_edges: height.then_some(f.lost),
                stable: height.then(|| sandpile_stable(w, i)),
                max_state: (!height).then(|| f.max_state()),
                positive: jaca.then(|| count(jetcarp_core::jaca::POS)),
                negative: jaca.then(|| count(jetcarp_core::jaca::NEG)),
            }
        })
        .collect();
    let script = w.script.as_ref().map(|s| {
        let st = &w.script_state;
        ScriptStatus {
            steps: s.steps.len(),
            current_step: st.step,
            current_name: s.steps.get(st.step).map(|x| x.name.clone()),
            finished: st.finished,
        }
    });
    WorldStatus {
        generation: w.generation,
        status: status_text(w),
        done: w
            .done_reason()
            .unwrap_or_else(|| "no: something can still change".into()),
        hash: w.hash_hex(),
        hash_note: "`hash` covers the generation number and cell ages too, so it never repeats; `cells_hash` covers only the cell states: equal cells_hash = same picture. For periods, trust `status` (an exact check).".into(),
        cells_hash: format!("{:016x}", w.states_hash()),
        cells: t.len(),
        tiling: TilingStatus {
            id: t.family.id.to_string(),
            name: t.family.name.to_string(),
            boundary: boundary_name(t).to_string(),
            grid: grid.unwrap_or(json!("none: aperiodic patch, address cells by index")),
            description: t.describe(),
        },
        cas: w.cas.iter().map(ca_status).collect(),
        fields,
        script,
        host: host.info(),
    }
}

/// The world's status in a few lines (after a change).
pub fn short_status(w: &World) -> String {
    let pops: Vec<String> = w
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let users: Vec<String> = w
                .cas
                .iter()
                .filter(|c| c.field == i)
                .map(|c| format!("{} (id {})", c.name, c.id))
                .collect();
            let what = if f.kind == FieldKind::Height {
                if sandpile_stable(w, i) {
                    "grains (stable: no cell can topple)"
                } else {
                    "grains (not yet stable: cells will topple when stepped)"
                }
            } else {
                "live"
            };
            format!(
                "field {i} [{}]: {} {what}",
                users.join(", "),
                f.population()
            )
        })
        .collect();
    format!(
        "Generation {}. Status: {}. Hash {} (with generation and ages); cells hash {} (cell states only: equal when the cells repeat).\n{}",
        w.generation,
        status_text(w),
        w.hash_hex(),
        format_args!("{:016x}", w.states_hash()),
        pops.join("\n")
    )
}

/// What a status report includes.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusIn {
    /// Also return the scene: tiling, CAs, random seed, and the command log since the last
    /// reset (opening it reproduces this exact state).
    #[serde(default)]
    pub scene: bool,
}

/// A status report, and the scene when asked for.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub status: WorldStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub scene: Option<Scene>,
}

impl Report for StatusOut {
    fn text(&self) -> String {
        match &self.scene {
            Some(s) => s.to_json(),
            None => to_pretty(&self.status),
        }
    }
}

/// `world.status`.
pub struct WorldStatusOp;
impl WorldOp for WorldStatusOp {
    const NAME: &'static str = "world.status";
    const SUMMARY: &'static str = "The world's tiling and size, generation, hashes, whether it is running, empty, frozen or repeating, populations, and its CAs; optionally its scene.";
    const READ_ONLY: bool = true;
    type In = StatusIn;
    type Out = StatusOut;
    fn run(
        _: &Ctx,
        host: &mut dyn Host,
        a: StatusIn,
        _: &dyn Progress,
    ) -> Result<StatusOut, ApiError> {
        Ok(StatusOut {
            status: world_status(host),
            scene: a.scene.then(|| host.scene()),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// New, open, save

/// A new world: a tiling and the CAs on it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewWorldIn {
    /// Tiling id (reference.tilings): "square" (default), "hex", "triangle", "cairo", "spectre"
    /// (aperiodic), a vertex configuration such as "3.4.6.4", or a catalog id such as "k2-005".
    #[serde(default)]
    pub tiling: Option<String>,
    /// Grid width in unit cells (with height).
    #[serde(default)]
    pub width: Option<Int>,
    /// Grid height in unit cells.
    #[serde(default)]
    pub height: Option<Int>,
    /// Instead of width and height: about this many cells in a roughly square region.
    #[serde(default)]
    pub cells: Option<Int>,
    /// Aperiodic tilings: the substitution level.
    #[serde(default)]
    pub level: Option<Int>,
    /// "wrap" (default), "dead", or "live" (aperiodic tilings are always dead).
    #[serde(default)]
    pub boundary: Option<String>,
    /// The CAs, in order: rule names or notation ("Life", "B3/S23", "Brian's Brain"), or CA
    /// objects in the scene format (`{"rule": "Life", "seed": "random:0.2", "neighborhood":
    /// "edge", "palette": "Tide"}`). A single string is one CA.
    #[serde(default)]
    pub cas: Option<Value>,
    /// Random seed (default 1): the same seed gives the same run. Any `u64`; beyond 2^53 - 1 also
    /// as a string of decimal digits.
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub rng_seed: Option<u64>,
    /// Scene name.
    #[serde(default)]
    pub name: Option<String>,
    /// Advanced: tile shape parameters (JETCARP tile JSON `params`).
    #[serde(default)]
    pub tiling_params: Option<Value>,
}

/// The tiling a new world asks for.
pub fn tiling_of(a: &NewWorldIn, l: &Limits) -> Result<TilingSpec, ApiError> {
    let id = a.tiling.as_deref().unwrap_or("square").trim();
    let mut spec = args::tiling_by_id(id).map_err(|_| {
        let ids: Vec<&str> = families::FAMILIES.iter().map(|f| f.id).collect();
        ApiError::not_found(format!(
            "unknown tiling '{id}'. Tiling ids: {}. See reference {{\"what\": \"tilings\"}}.",
            ids.join(", ")
        ))
    })?;
    let fam = families::family(&spec.family).unwrap();
    let aperiodic = fam.kind == families::FamilyKind::Aperiodic;
    let side = l.max_grid_side as Int;
    let max_cells = l.max_cells.min(Int::MAX as u64) as Int;
    spec.size = match (a.width, a.height, a.cells, a.level) {
        (Some(_), None, _, _) | (None, Some(_), _, _) => {
            return Err(ApiError::invalid("give both width and height"));
        }
        (Some(_), Some(_), Some(_), _) => {
            return Err(ApiError::invalid("give width/height or cells, not both"));
        }
        (Some(_), Some(_), _, _) if aperiodic => {
            return Err(ApiError::invalid(format!(
                "{} is aperiodic: give `level` (1-{}) instead of width/height",
                fam.id,
                max_level(fam.id, l.max_cells)
            )));
        }
        (Some(w), Some(h), None, _) => {
            if !(8..=side).contains(&w) || !(8..=side).contains(&h) {
                return Err(ApiError::invalid(format!(
                    "width and height must be 8 to {side}, got {w} x {h}"
                )));
            }
            Some(SizeSpec::Rect {
                w: w as u32,
                h: h as u32,
            })
        }
        (None, None, Some(c), _) => {
            if !(64..=max_cells).contains(&c) {
                return Err(ApiError::invalid(format!(
                    "cells must be 64 to {max_cells}"
                )));
            }
            Some(SizeSpec::Cells(c.min(u32::MAX as Int) as u32))
        }
        (None, None, None, Some(lv)) => {
            if !aperiodic {
                return Err(ApiError::invalid(format!(
                    "`level` is only for aperiodic tilings; {} takes width/height or cells",
                    fam.id
                )));
            }
            let hi = max_level(fam.id, l.max_cells) as Int;
            if !(1..=hi).contains(&lv) {
                return Err(ApiError::invalid(format!("level must be 1 to {hi}")));
            }
            Some(SizeSpec::Level(lv as u32))
        }
        (None, None, None, None) if aperiodic => None,
        (None, None, None, None) => Some(SizeSpec::Cells(4096)),
    };
    spec.boundary = match a.boundary.as_deref().map(|s| s.to_ascii_lowercase()) {
        None => Boundary::Wrap,
        Some(b) => args::parse_boundary(&b).map_err(|_| {
            ApiError::invalid(format!("boundary must be wrap, dead, or live, got '{b}'"))
        })?,
    };
    if aperiodic {
        spec.boundary = Boundary::Dead;
    }
    if let Some(p) = a.tiling_params.as_ref().filter(|p| !p.is_null()) {
        let Some(o) = p.as_object() else {
            return Err(ApiError::invalid("tiling_params must be an object"));
        };
        let mut merged = spec.params.take().unwrap_or(json!({}));
        for (k, v) in o {
            merged[k] = v.clone();
        }
        spec.params = Some(merged);
    }
    Ok(spec)
}

/// A world just made or opened.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct NewWorldOut {
    pub status: WorldStatus,
}

impl Report for NewWorldOut {
    fn text(&self) -> String {
        format!("New world created.\n{}", to_pretty(&self.status))
    }
}

/// `world.new`.
pub struct WorldNew;
impl WorldOp for WorldNew {
    const NAME: &'static str = "world.new";
    const SUMMARY: &'static str = "Replace the world with a new one: a tiling and the CAs that run on it (each from its rule's default seed unless given one).";
    const READ_ONLY: bool = false;
    type In = NewWorldIn;
    type Out = NewWorldOut;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: NewWorldIn,
        _: &dyn Progress,
    ) -> Result<NewWorldOut, ApiError> {
        let l = &cx.limits;
        let tiling = tiling_of(&a, l)?;
        let mut scene = Scene::new(tiling);
        scene.rng_state = a.rng_seed.unwrap_or(1);
        scene.name = a.name.clone().unwrap_or_default();
        match &a.cas {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for it in items {
                    scene.cas.push(ca_item(it, l)?);
                }
            }
            Some(Value::String(s)) => scene.cas.push(CaSpec::new(&resolve_rule(s))),
            Some(v) => return Err(ApiError::invalid(format!("cas must be a list, got {v}"))),
        }
        if scene.cas.len() > 32 {
            return Err(ApiError::limit("at most 32 CAs"));
        }
        // Palettes named here are typed, so an unknown one is refused (a scene file's would load,
        // drawn in the default, with a warning). A new world has no palettes of its own.
        for c in &scene.cas {
            for p in [&c.palette, &c.palette_neg].into_iter().flatten() {
                render::builtin_palette(p).map_err(ApiError::invalid)?;
            }
        }
        host.replace(scene)?;
        Ok(NewWorldOut {
            status: world_status(host),
        })
    }
}

/// What to open: a scene (JSON or a file), a Quick Start entry, or a library pattern. Give one.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenIn {
    /// Scene JSON: the object, or its text.
    #[serde(default)]
    pub scene: Option<Value>,
    /// A scene .json file.
    #[serde(default)]
    pub path: Option<String>,
    /// A Quick Start entry by handle (`life`, `find:<id>`, `atlas:<...>`, `qs-<hex>`): the scene
    /// the app's Quick Start opens.
    #[serde(default)]
    pub quickstart: Option<String>,
    /// A library pattern (id, unique prefix, or name such as `glider`): the pattern alone,
    /// centred on a wrapped grid sized for it, in the rule it runs in (on an aperiodic patch,
    /// where it was found).
    #[serde(default)]
    pub pattern: Option<String>,
}

/// The scene an [`OpenIn`] names.
fn scene_to_open(cx: &Ctx, a: &OpenIn) -> Result<Scene, ApiError> {
    let scene = a.scene.as_ref().filter(|v| !v.is_null());
    let given = [
        scene.is_some(),
        a.path.is_some(),
        a.quickstart.is_some(),
        a.pattern.is_some(),
    ];
    match given.iter().filter(|g| **g).count() {
        0 => {
            return Err(ApiError::invalid(
                "give `scene` (JSON), `path`, `quickstart`, or `pattern`",
            ));
        }
        1 => {}
        _ => {
            return Err(ApiError::invalid(
                "give one of `scene`, `path`, `quickstart`, or `pattern`",
            ));
        }
    }
    if let Some(key) = &a.quickstart {
        return jetcarp_store::quickstart::scene(&cx.library(), key)
            .map_err(|e| ApiError::not_found(e).hint("quickstart.list lists the entries"));
    }
    if let Some(key) = &a.pattern {
        let rec = cx
            .library()
            .get(key)
            .map_err(crate::ops::library::catalog_err)?;
        return pattern::promote::showcase_scene(&rec, None, None).kind(ErrorKind::Engine);
    }
    read_scene(cx, scene, a.path.as_deref())
}

/// A scene opened.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct OpenOut {
    pub status: WorldStatus,
    /// What the scene recorded when saved, if it did.
    pub recorded: Option<Recorded>,
    /// The replay reached the recorded generation and hash (when one was recorded).
    pub replayed_to_record: Option<bool>,
    /// What loading it let pass (palettes this JETCARP does not know, drawn in the default).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl Report for OpenOut {
    fn text(&self) -> String {
        let check = match (&self.recorded, self.replayed_to_record) {
            (Some(_), Some(true)) => " Replayed to its recorded hash.".to_string(),
            (Some(r), _) => format!(
                " Warning: recorded generation {} hash {}, replayed generation {} hash {}.",
                r.generation, r.hash, self.status.generation, self.status.hash
            ),
            _ => String::new(),
        };
        let warn: String = self
            .warnings
            .iter()
            .map(|w| format!(" Warning: {w}."))
            .collect();
        format!("Scene loaded.{check}{warn}\n{}", to_pretty(&self.status))
    }
}

/// `world.open`.
pub struct WorldOpen;
impl WorldOp for WorldOpen {
    const NAME: &'static str = "world.open";
    const SUMMARY: &'static str = "Replace the world with a scene (JSON or a .json file, its commands replayed so the state matches the saved one), a Quick Start entry, or a library pattern alone on a grid sized for it.";
    const READ_ONLY: bool = false;
    type In = OpenIn;
    type Out = OpenOut;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: OpenIn,
        _: &dyn Progress,
    ) -> Result<OpenOut, ApiError> {
        let scene = scene_to_open(cx, &a)?;
        check_tiling_size(&scene.tiling, &cx.limits)?;
        let recorded = scene.record.as_ref().map(|r| Recorded {
            generation: r.generation,
            hash: r.hash.clone(),
        });
        host.replace(scene)?;
        let w = host.world();
        let replayed = recorded
            .as_ref()
            .map(|r| r.generation == w.generation && r.hash == w.hash_hex());
        let warnings = w.warnings().to_vec();
        Ok(OpenOut {
            status: world_status(host),
            recorded,
            replayed_to_record: replayed,
            warnings,
        })
    }
}

/// Where to save the scene.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveIn {
    /// The .json file to write (its folder is created).
    pub path: String,
}

/// The scene saved.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SavedScene {
    pub path: std::path::PathBuf,
    pub bytes: usize,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub hash: String,
}

impl Report for SavedScene {
    fn text(&self) -> String {
        format!(
            "Saved the scene ({} bytes, generation {}, hash {}) to {}.",
            self.bytes,
            self.generation,
            self.hash,
            self.path.display()
        )
    }
}

/// `world.save`.
pub struct WorldSave;
impl WorldOp for WorldSave {
    const NAME: &'static str = "world.save";
    const SUMMARY: &'static str =
        "Write the world's scene to a .json file (the app and `jetcarp run` open it).";
    const READ_ONLY: bool = false;
    type In = SaveIn;
    type Out = SavedScene;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: SaveIn,
        _: &dyn Progress,
    ) -> Result<SavedScene, ApiError> {
        let p = cx.fs.writable(&a.path)?;
        let json = host.scene().to_json();
        std::fs::write(&p, &json)
            .map_err(|e| ApiError::io(format!("could not write {}: {e}", p.display())))?;
        Ok(SavedScene {
            path: p,
            bytes: json.len(),
            generation: host.world().generation,
            hash: host.world().hash_hex(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing and pasting

/// Cells to draw into a CA's field. Give one of `pattern`, `rle`, `cells`, or `indices` (or
/// only `clear_first`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawIn {
    #[serde(default)]
    #[schemars(schema_with = "ca_ref_schema")]
    pub ca: Option<Value>,
    /// Text picture, one line per row: '.' empty, 'O' (or '*', '#', 'X') live, '1'-'9' states.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Run-length encoded pattern (LifeWiki RLE; header lines are ignored).
    #[serde(default)]
    pub rle: Option<String>,
    /// Column of the pattern's top-left corner (default: centred).
    #[serde(default)]
    pub x: Option<Int>,
    /// Row of the pattern's top-left corner (default: centred).
    #[serde(default)]
    pub y: Option<Int>,
    /// Cells as [x, y] or [x, y, state] (or {"x", "y", "state"}).
    #[serde(default)]
    pub cells: Option<Value>,
    /// Cells by index, as index or [index, state] (the only form on aperiodic tilings).
    #[serde(default)]
    pub indices: Option<Value>,
    /// State for cells given without one (default 1).
    #[serde(default)]
    pub state: Option<Int>,
    /// Empty the whole field before drawing.
    #[serde(default)]
    pub clear_first: bool,
    /// Set the listed or pattern cells to empty instead.
    #[serde(default)]
    pub erase: bool,
}

/// Cells drawn.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Drawn {
    pub ca: u32,
    pub ca_name: String,
    /// Cells written.
    pub count: usize,
    pub erased: bool,
    pub cleared: bool,
    /// The pattern's box `[x0, y0, x1, y1]`, when a pattern was placed.
    pub pattern_box: Option<[Int; 4]>,
    /// The field's population after drawing.
    pub population: u64,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
}

impl Report for Drawn {
    fn text(&self) -> String {
        let mut s = format!(
            "{} {} cell(s) in CA '{}' (id {}){}.",
            if self.erased { "Erased" } else { "Set" },
            self.count,
            self.ca_name,
            self.ca,
            if self.cleared {
                " after clearing its field"
            } else {
                ""
            }
        );
        if let Some([x0, y0, x1, y1]) = self.pattern_box {
            let _ = write!(s, " Pattern box: x {x0}..{x1}, y {y0}..{y1}.");
        }
        let _ = write!(
            s,
            " Field population now {}. Generation {}.",
            self.population, self.generation
        );
        s
    }
}

/// `world.draw`.
pub struct WorldDraw;
impl WorldOp for WorldDraw {
    const NAME: &'static str = "world.draw";
    const SUMMARY: &'static str = "Draw cells into a CA's field: a text picture, RLE, [x, y] cells, or cell indices; optionally clear the field first, or erase.";
    const READ_ONLY: bool = false;
    type In = DrawIn;
    type Out = Drawn;
    fn run(cx: &Ctx, host: &mut dyn Host, a: DrawIn, _: &dyn Progress) -> Result<Drawn, ApiError> {
        let max_set = cx.limits.max_set;
        let w = host.world();
        let ci = ca_index(w, a.ca.as_ref())?;
        let (id, ca_name) = (w.cas[ci].id, w.cas[ci].name.clone());
        let field = field_of(w, ci);
        let height = field.kind == FieldKind::Height;
        let max_state = if height {
            u32::MAX
        } else {
            field.max_state() as u32
        };
        let n = w.topo.len();
        let grid = Grid::of(&w.topo);
        let (erase, clear_first) = (a.erase, a.clear_first);
        if erase && height {
            return Err(ApiError::invalid(
                "erase does not apply to sandpile grains; use the command {\"op\":\"clear\",\"id\":..} (world_edit commands) to empty the field",
            ));
        }
        let default_state = a.state.unwrap_or(1);
        if default_state < 0 {
            return Err(ApiError::invalid("state must be 0 or more"));
        }
        let need_grid = || {
            grid.ok_or_else(|| {
                ApiError::invalid("this tiling is an aperiodic patch with no x/y grid; give `indices` instead (world_read with `cell` shows a cell's index and neighbours)")
            })
        };
        let mut writes: Vec<[u32; 2]> = Vec::new();
        let mut given = 0;
        let pattern = match (&a.pattern, &a.rle) {
            (Some(_), Some(_)) => return Err(ApiError::invalid("give pattern or rle, not both")),
            (Some(p), None) => Some(cells::parse_plaintext(p).kind(ErrorKind::Invalid)?),
            (None, Some(r)) => Some(cells::parse_rle(r).kind(ErrorKind::Invalid)?),
            (None, None) => None,
        };
        let mut placed = None;
        if let Some(p) = pattern {
            given += 1;
            let g = need_grid()?;
            if p.is_empty() {
                return Err(ApiError::invalid(
                    "the pattern has no live cells (use 'O' for live, '.' for empty)",
                ));
            }
            let pw = p.iter().map(|c| c.0).max().unwrap() + 1;
            let ph = p.iter().map(|c| c.1).max().unwrap() + 1;
            let ox = a.x.unwrap_or((g.w as Int - pw) / 2);
            let oy = a.y.unwrap_or((g.h as Int - ph) / 2);
            if p.len() > max_set {
                return Err(ApiError::limit(format!(
                    "pattern has more than {max_set} cells"
                )));
            }
            for (dx, dy, v) in &p {
                let i = g.index(ox + dx, oy + dy).kind(ErrorKind::Invalid)?;
                writes.push([i as u32, if erase { 0 } else { *v }]);
            }
            placed = Some([ox, oy, ox + pw - 1, oy + ph - 1]);
        } else if a.x.is_some() || a.y.is_some() {
            return Err(ApiError::invalid(
                "x and y place a `pattern` or `rle`; for single cells use cells: [[x, y]]",
            ));
        }
        if let Some(list) = a.cells.as_ref().filter(|v| !v.is_null()) {
            given += 1;
            let g = need_grid()?;
            let items = list.as_array().ok_or_else(|| {
                ApiError::invalid("cells must be a list like [[x, y], [x, y, state]]")
            })?;
            if items.len() > max_set {
                return Err(ApiError::limit(format!("at most {max_set} cells per call")));
            }
            for it in items {
                let xy: Option<Vec<i64>> = match it {
                    Value::Array(a) => a.iter().map(|v| v.as_i64()).collect(),
                    Value::Object(o) => ["x", "y", "state"]
                        .iter()
                        .filter_map(|k| o.get(*k))
                        .map(|v| v.as_i64())
                        .collect(),
                    _ => None,
                };
                let xy = xy.filter(|v| v.len() == 2 || v.len() == 3).ok_or_else(|| {
                    ApiError::invalid(format!(
                        "each cell must be [x, y] or [x, y, state], got {it}"
                    ))
                })?;
                let v = if erase {
                    0
                } else {
                    xy.get(2).copied().unwrap_or(default_state)
                };
                if v < 0 {
                    return Err(ApiError::invalid(format!("negative state in {it}")));
                }
                let i = g.index(xy[0], xy[1]).kind(ErrorKind::Invalid)?;
                writes.push([i as u32, v as u32]);
            }
        }
        if let Some(list) = a.indices.as_ref().filter(|v| !v.is_null()) {
            given += 1;
            let items = list
                .as_array()
                .ok_or_else(|| ApiError::invalid("indices must be a list"))?;
            if items.len() > max_set {
                return Err(ApiError::limit(format!("at most {max_set} cells per call")));
            }
            for it in items {
                let (i, v) = match it {
                    Value::Number(_) => (it.as_i64(), Some(default_state)),
                    Value::Array(a) if a.len() == 2 => (a[0].as_i64(), a[1].as_i64()),
                    _ => (None, None),
                };
                let (Some(i), Some(v)) = (i, v) else {
                    return Err(ApiError::invalid(format!(
                        "each index must be a number or [index, state], got {it}"
                    )));
                };
                if i < 0 || i as usize >= n || v < 0 {
                    return Err(ApiError::invalid(format!(
                        "cell index {i} is out of range (0..{})",
                        n - 1
                    )));
                }
                writes.push([i as u32, if erase { 0 } else { v as u32 }]);
            }
        }
        if given == 0 && !clear_first {
            return Err(ApiError::invalid(
                "nothing to draw: give pattern, rle, cells, or indices (or clear_first: true)",
            ));
        }
        if let Some(bad) = writes.iter().find(|c| c[1] > max_state) {
            return Err(ApiError::invalid(format!(
                "state {} is too high: CA '{ca_name}' has states 0 to {max_state}",
                bad[1]
            )));
        }
        if clear_first {
            host.exec(Command::Clear { id })?;
        }
        let count = writes.len();
        if !writes.is_empty() {
            host.exec(Command::SetCells { id, cells: writes })?;
        }
        let w = host.world();
        Ok(Drawn {
            ca: id,
            ca_name,
            count,
            erased: erase,
            cleared: clear_first,
            pattern_box: placed,
            population: field_of(w, ci).population(),
            generation: w.generation,
        })
    }
}

/// A library pattern to paste, and where.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PasteIn {
    /// Pattern id, unique id prefix, or name ("glider").
    pub id: String,
    #[serde(default)]
    #[schemars(schema_with = "ca_ref_schema")]
    pub ca: Option<Value>,
    /// Column of the pattern's top-left corner (with y).
    #[serde(default)]
    pub x: Option<Int>,
    /// Row of the pattern's top-left corner (with x).
    #[serde(default)]
    pub y: Option<Int>,
    /// Cell index for the corner (lattice) or the anchor tile (aperiodic patches).
    #[serde(default)]
    pub index: Option<Int>,
    /// "center" (default on lattices) or "original" (aperiodic: where it was found; the default
    /// there).
    #[serde(default)]
    pub at: Option<String>,
    /// Index into the tiling's symmetries (0 = as stored).
    #[serde(default)]
    pub orientation: Option<Int>,
    /// Which phase to paste (default 0).
    #[serde(default)]
    pub phase: Option<Int>,
}

/// A pattern pasted.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Pasted {
    pub id: String,
    pub label: String,
    pub cells: usize,
    pub ca: u32,
    /// The CA's rule or neighbourhood differs from the pattern's.
    pub warning: Option<String>,
    #[serde(skip)]
    status_line: String,
}

impl Report for Pasted {
    fn text(&self) -> String {
        format!(
            "Pasted {} ({}, {} cells) into CA {}.{}\n{}",
            self.label,
            self.id,
            self.cells,
            self.ca,
            self.warning
                .as_ref()
                .map(|w| format!("\n{w}"))
                .unwrap_or_default(),
            self.status_line
        )
    }
}

/// `world.paste`.
pub struct WorldPaste;
impl WorldOp for WorldPaste {
    const NAME: &'static str = "world.paste";
    const SUMMARY: &'static str = "Paste a catalog pattern into a CA's field (a logged, replayable command), at a corner, a cell, the centre, or where it was found; turned and in any phase.";
    const READ_ONLY: bool = false;
    type In = PasteIn;
    type Out = Pasted;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: PasteIn,
        _: &dyn Progress,
    ) -> Result<Pasted, ApiError> {
        let rec = cx.library().get(&a.id).kind(ErrorKind::NotFound)?;
        let w = host.world();
        let ci = ca_index(w, a.ca.as_ref())?;
        let ca_id = w.cas[ci].id;
        let at = match (a.x, a.y, a.index, a.at.as_deref()) {
            (Some(x), Some(y), None, None) => PasteAt::Cell {
                x: x as i32,
                y: y as i32,
            },
            (None, None, Some(i), None) => PasteAt::Index {
                cell: i.max(0) as u32,
            },
            (None, None, None, Some("original")) => PasteAt::Original,
            (None, None, None, Some("center")) => PasteAt::Center,
            (None, None, None, None) => {
                if rec.site.is_some() {
                    PasteAt::Original
                } else {
                    PasteAt::Center
                }
            }
            _ => {
                return Err(ApiError::invalid(
                    "give x and y, or index, or at (\"center\"/\"original\"), not several",
                ));
            }
        };
        let phase = a.phase.unwrap_or(0).max(0) as u64;
        let orientation = a.orientation.unwrap_or(0).max(0) as u32;
        let ca_rule = w.cas[ci].notation();
        let warning = (w.cas[ci].nb != rec.neighborhood || !rec.works_in(&ca_rule)).then(|| {
            format!(
                "Note: the pattern runs as stored in {}{}; this CA runs {ca_rule}, so it may behave differently.",
                rec.rule,
                rec.range
                    .as_ref()
                    .map(|x| format!(" ({})", x.describe()))
                    .unwrap_or_default(),
            )
        });
        host.exec(Command::PastePattern {
            id: ca_id,
            pattern: pattern::paste::source_from_record(&rec, phase),
            at,
            orientation,
            phase: 0,
        })?;
        Ok(Pasted {
            label: rec.label(),
            cells: rec.phase(phase).kind(ErrorKind::Invalid)?.len(),
            id: rec.id.clone(),
            ca: ca_id,
            warning,
            status_line: short_status(host.world()),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Reading

/// What part of a field to show.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadIn {
    #[serde(default)]
    #[schemars(schema_with = "ca_ref_schema")]
    pub ca: Option<Value>,
    /// Left column of the region (default: the box around every non-empty cell).
    #[serde(default)]
    pub x: Option<Int>,
    /// Top row of the region.
    #[serde(default)]
    pub y: Option<Int>,
    /// Region width (default 40 with x, y; at most 200).
    #[serde(default)]
    pub width: Option<Int>,
    /// Region height (default 40 with x, y; at most 200).
    #[serde(default)]
    pub height: Option<Int>,
    /// "picture" (default) or "list" of non-empty cells as [x, y, state].
    #[serde(default)]
    pub format: Option<String>,
}

/// The CA a reading is of.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CaBrief {
    pub id: u32,
    pub name: String,
    pub rule: String,
    pub field: usize,
}

/// A rectangle of cells.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// A field as text (and, as a list, its non-empty cells).
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CellsView {
    pub ca: CaBrief,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    /// Non-empty cells in the whole field.
    pub non_empty: usize,
    /// Grid `[width, height]` (none on aperiodic patches).
    pub grid: Option<[usize; 2]>,
    /// Box around the non-empty cells `[x0, y0, x1, y1]`.
    pub bounding_box: Option<[usize; 4]>,
    /// The region shown.
    pub region: Option<Region>,
    /// List format: `[x, y, state]` of the non-empty cells in the region (at most 4,000).
    pub cells: Option<Vec<[u64; 3]>>,
    #[serde(skip)]
    text: String,
}

impl Report for CellsView {
    fn text(&self) -> String {
        self.text.clone()
    }
}

/// `world.read`.
pub struct WorldRead;
impl WorldOp for WorldRead {
    const NAME: &'static str = "world.read";
    const SUMMARY: &'static str = "A CA's field as a text picture with a ruler (or a list of [x, y, state]): the box around its live cells, or a given region.";
    const READ_ONLY: bool = true;
    type In = ReadIn;
    type Out = CellsView;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: ReadIn,
        _: &dyn Progress,
    ) -> Result<CellsView, ApiError> {
        let w = host.world();
        let ci = ca_index(w, a.ca.as_ref())?;
        let ca = &w.cas[ci];
        let f = field_of(w, ci);
        let binary = f.kind == FieldKind::Binary;
        let n = w.topo.len();
        let list_mode = match a.format.as_deref() {
            None | Some("picture") => false,
            Some("list") => true,
            Some(o) => {
                return Err(ApiError::invalid(format!(
                    "format must be picture or list, got {o}"
                )));
            }
        };
        let mut s = format!(
            "CA '{}' (id {}, {}), field {}, generation {}.\n",
            ca.name,
            ca.id,
            ca.notation(),
            ca.field,
            w.generation
        );
        let mut view = CellsView {
            ca: CaBrief {
                id: ca.id,
                name: ca.name.clone(),
                rule: ca.notation(),
                field: ca.field,
            },
            generation: w.generation,
            non_empty: 0,
            grid: None,
            bounding_box: None,
            region: None,
            cells: None,
            text: String::new(),
        };
        let nonzero: Vec<usize> = (0..n).filter(|&i| cells::value(f, i) != 0).collect();
        view.non_empty = nonzero.len();
        let agents = |s: &mut String, g: Option<Grid>| {
            if ca.rule.is_agent() {
                let a: Vec<String> = ca
                    .agents
                    .iter()
                    .map(|a| match g {
                        Some(g) => {
                            let (x, y) = g.xy(a.cell as usize);
                            format!("({x}, {y})")
                        }
                        None => format!("cell {}", a.cell),
                    })
                    .collect();
                let _ = writeln!(s, "Ants at: {}", a.join(", "));
            }
        };
        let Some(g) = Grid::of(&w.topo) else {
            // Aperiodic patch: list cells with centre points.
            let _ = writeln!(
                s,
                "Aperiodic tiling: no grid. {} non-empty of {n} cells.",
                nonzero.len()
            );
            for &i in nonzero.iter().take(500) {
                let c = w.topo.centroid(i);
                let _ = writeln!(
                    s,
                    "index {i}: {} at ({:.2}, {:.2})",
                    cells::value(f, i),
                    c[0],
                    c[1]
                );
            }
            if nonzero.len() > 500 {
                s.push_str("... (first 500 shown)\n");
            }
            agents(&mut s, None);
            view.text = s;
            return Ok(view);
        };
        view.grid = Some([g.w, g.h]);
        let bbox = (!nonzero.is_empty()).then(|| {
            let mut b = (usize::MAX, usize::MAX, 0, 0);
            for &i in &nonzero {
                let (x, y) = g.xy(i);
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
            b
        });
        view.bounding_box = bbox.map(|b| [b.0, b.1, b.2, b.3]);
        let _ = write!(
            s,
            "Grid {} x {}. Non-empty cells in the whole field: {}",
            g.w,
            g.h,
            nonzero.len()
        );
        if let Some(b) = bbox {
            let _ = write!(
                s,
                "; bounding box x {}..{}, y {}..{} ({} x {})",
                b.0,
                b.2,
                b.1,
                b.3,
                b.2 - b.0 + 1,
                b.3 - b.1 + 1
            );
            if g.wrap && (b.2 - b.0 + 1 == g.w || b.3 - b.1 + 1 == g.h) {
                s.push_str(" (spans a wrapped edge or the whole grid)");
            }
        }
        s.push_str(".\n");
        agents(&mut s, Some(g));
        let (x, y, rw, rh) = match (a.x, a.y) {
            (None, None) if a.width.is_none() && a.height.is_none() => {
                let Some(b) = bbox else {
                    s.push_str("The field is empty.\n");
                    view.text = s;
                    return Ok(view);
                };
                let x0 = b.0.saturating_sub(1);
                let y0 = b.1.saturating_sub(1);
                let x1 = (b.2 + 1).min(g.w - 1);
                let y1 = (b.3 + 1).min(g.h - 1);
                (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
            }
            (x, y) => {
                let x = x.unwrap_or(0);
                let y = y.unwrap_or(0);
                if x < 0 || y < 0 || x as usize >= g.w || y as usize >= g.h {
                    return Err(ApiError::invalid(format!(
                        "x, y must be inside the grid (0..{}, 0..{})",
                        g.w - 1,
                        g.h - 1
                    )));
                }
                let rw = a.width.unwrap_or(40).clamp(1, 200) as usize;
                let rh = a.height.unwrap_or(40).clamp(1, 200) as usize;
                let (x, y) = (x as usize, y as usize);
                (x, y, rw.min(g.w - x), rh.min(g.h - y))
            }
        };
        view.region = Some(Region {
            x,
            y,
            width: rw,
            height: rh,
        });
        if list_mode {
            let mut items = Vec::new();
            for yy in y..y + rh {
                for xx in x..x + rw {
                    let v = cells::value(f, yy * g.w + xx);
                    if v != 0 {
                        items.push([xx as u64, yy as u64, v as u64]);
                    }
                }
            }
            let total = items.len();
            items.truncate(4000);
            let _ = writeln!(
                s,
                "Non-empty cells in x {}..{}, y {}..{} as [x, y, state] ({total}{}):\n{}",
                x,
                x + rw - 1,
                y,
                y + rh - 1,
                if total > 4000 { ", first 4000" } else { "" },
                json!(items)
            );
            view.cells = Some(items);
            view.text = s;
            return Ok(view);
        }
        let max_picture = cx.limits.max_picture;
        if rw * rh > max_picture {
            return Err(ApiError::limit(format!(
                "{s}The region x {x}..{}, y {y}..{} ({rw} x {rh}) is too large to print. Give x, y, width, height for a region of at most {max_picture} cells (e.g. 100 x 100), or use format \"list\".",
                x + rw - 1,
                y + rh - 1
            )));
        }
        let _ = writeln!(
            s,
            "Region x {}..{}, y {}..{}. '.' = empty, {}. Row 0 is the top.",
            x,
            x + rw - 1,
            y,
            y + rh - 1,
            if binary {
                "'O' = live"
            } else if f.kind == FieldKind::Height {
                "digits/letters = grains (a = 10, ..., # = 36 or more)"
            } else {
                "digits = state (a = 10, ...)"
            }
        );
        // Ruler: tens digit line (when wide) and units digit line.
        let tens: String = (x..x + rw)
            .map(|c| {
                if c % 10 == 0 {
                    char::from(b'0' + ((c / 10) % 10) as u8)
                } else {
                    ' '
                }
            })
            .collect();
        let units: String = (x..x + rw)
            .map(|c| char::from(b'0' + (c % 10) as u8))
            .collect();
        if rw > 10 {
            let _ = writeln!(s, "x tens  {tens}");
        }
        let _ = writeln!(s, "x units {units}");
        for yy in y..y + rh {
            let row: String = (x..x + rw)
                .map(|xx| cells::state_char(cells::value(f, yy * g.w + xx), binary))
                .collect();
            let _ = writeln!(s, "y={yy:<5} {row}");
        }
        view.text = s;
        Ok(view)
    }
}

/// Which cell to inspect: x and y, or an index.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectIn {
    /// Column.
    #[serde(default)]
    pub x: Option<Int>,
    /// Row.
    #[serde(default)]
    pub y: Option<Int>,
    /// Cell index (instead of x, y; required on aperiodic tilings).
    #[serde(default)]
    pub index: Option<Int>,
}

/// One field's value in a cell.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CellValue {
    pub field: usize,
    pub kind: String,
    pub value: u64,
}

/// One cell: where it is, its shape, its values, and its neighbours.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CellInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<usize>,
    pub index: usize,
    pub tile_type: usize,
    pub corners: usize,
    pub center: [f64; 2],
    pub values: Vec<CellValue>,
    /// Edge-sharing neighbours in clockwise order: `{x, y, index}`, or "past the edge".
    pub edge_neighbors_clockwise: Vec<Value>,
    /// Edge- or corner-sharing neighbours in clockwise order.
    pub touch_neighbors_clockwise: Vec<Value>,
}

impl Report for CellInfo {
    fn text(&self) -> String {
        to_pretty(self)
    }
}

/// `world.inspect`.
pub struct WorldInspect;
impl WorldOp for WorldInspect {
    const NAME: &'static str = "world.inspect";
    const SUMMARY: &'static str = "One cell: index and (x, y), tile type, centre, value in every field, and its neighbours in clockwise order.";
    const READ_ONLY: bool = true;
    type In = InspectIn;
    type Out = CellInfo;
    fn run(
        _: &Ctx,
        host: &mut dyn Host,
        a: InspectIn,
        _: &dyn Progress,
    ) -> Result<CellInfo, ApiError> {
        let w = host.world();
        let t = &w.topo;
        let g = Grid::of(t);
        let i = match (a.index, a.x, a.y) {
            (Some(i), None, None) => {
                if i < 0 || i as usize >= t.len() {
                    return Err(ApiError::invalid(format!(
                        "index must be 0 to {}",
                        t.len() - 1
                    )));
                }
                i as usize
            }
            (None, Some(x), Some(y)) => g
                .ok_or_else(|| ApiError::invalid("this tiling has no x/y grid; give `index`"))?
                .index(x, y)
                .kind(ErrorKind::Invalid)?,
            _ => return Err(ApiError::invalid("give x and y, or index")),
        };
        let at = |j: usize| match g {
            Some(g) => {
                let (x, y) = g.xy(j);
                json!({"x": x, "y": y, "index": j})
            }
            None => json!({"index": j}),
        };
        let nbs = |nb: Nb| -> Vec<Value> {
            (0..t.degree(i, nb))
                .map(|k| t.slot(i, nb, k).map(at).unwrap_or(json!("past the edge")))
                .collect()
        };
        let c = t.centroid(i);
        let xy = g.map(|g| g.xy(i));
        Ok(CellInfo {
            x: xy.map(|p| p.0),
            y: xy.map(|p| p.1),
            index: i,
            tile_type: g.map(|g| i % g.types).unwrap_or(t.shape_of(i)),
            corners: t.polygon(i).len(),
            center: [
                (c[0] * 1000.0).round() / 1000.0,
                (c[1] * 1000.0).round() / 1000.0,
            ],
            values: w
                .fields
                .iter()
                .enumerate()
                .map(|(fi, f)| CellValue {
                    field: fi,
                    kind: f.kind.name().to_string(),
                    value: cells::value(f, i) as u64,
                })
                .collect(),
            edge_neighbors_clockwise: nbs(Nb::Edge),
            touch_neighbors_clockwise: nbs(Nb::Touch),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Objects

/// Which CA's objects to find, and whether to store them.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectsIn {
    #[serde(default)]
    #[schemars(schema_with = "ca_ref_schema")]
    pub ca: Option<Value>,
    /// Longest period looked for (default 64, at most 1024).
    #[serde(default)]
    pub max_period: Option<u64>,
    /// Also verify each distinct still life, oscillator and spaceship and add it to the library
    /// (copies add to its seen count), as a search would.
    #[serde(default)]
    pub catalog: bool,
}

/// One object found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FoundObject {
    pub id: usize,
    /// `glider (c/4)`, `blinker (p2)`, `block`; `other` for what is none of these.
    pub label: String,
    /// still_life, oscillator, spaceship, puffer, gun, or other.
    pub kind: String,
    pub period: Option<u64>,
    /// Spaceships: the speed (`c/4 diagonal`).
    pub speed: Option<String>,
    /// Spaceships: the lattice displacement per period.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displacement: Option<[i32; 2]>,
    /// Live cells now.
    pub size: usize,
    /// Grid tilings: the cells' columns and rows now, `[x0, x1, y0, y1]` (an object that wraps
    /// across the edge spans the whole width or height).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy_box: Option<[usize; 4]>,
    /// The first cell's index (every tiling).
    pub first_cell: Option<u32>,
    /// Why an `other` object is none of the classes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One kind of object and how many there are.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ObjectCount {
    pub label: String,
    pub kind: String,
    pub count: usize,
}

/// The objects a world holds now.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ObjectsOut {
    /// `block x 41, blinker (p2) x 12, glider (c/4) x 3`.
    pub summary: String,
    pub generation: u64,
    /// The CA examined.
    pub ca_id: u32,
    pub rule: String,
    pub live_cells: u64,
    /// Objects by class: still_life, oscillator, spaceship, other.
    pub totals: std::collections::BTreeMap<String, usize>,
    pub counts: Vec<ObjectCount>,
    /// The objects, at most 200.
    pub objects: Vec<FoundObject>,
    /// More objects than are listed.
    pub more: usize,
    /// With `catalog`: what was stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<crate::catalog::CatalogReport>,
}

impl Report for ObjectsOut {
    fn text(&self) -> String {
        let mut s = format!(
            "Generation {}, CA {} ({}): {} live cells. {}\n",
            self.generation, self.ca_id, self.rule, self.live_cells, self.summary
        );
        for o in &self.objects {
            let at = match (o.xy_box, o.first_cell) {
                (Some([x0, x1, y0, y1]), _) => format!("x {x0}..{x1}, y {y0}..{y1}"),
                (None, Some(c)) => format!("cell {c}"),
                _ => String::new(),
            };
            let _ = writeln!(
                s,
                "  {:>3}  {:<28} {:>4} cells  {at}{}",
                o.id,
                o.label,
                o.size,
                o.reason
                    .as_deref()
                    .map(|r| format!("  ({r})"))
                    .unwrap_or_default()
            );
        }
        if self.more > 0 {
            let _ = writeln!(s, "  ... and {} more", self.more);
        }
        if let Some(c) = &self.catalog {
            let _ = writeln!(
                s,
                "Library: {} new, {} already known, {} skipped, {} failed verification.",
                c.added.len(),
                c.merged.len(),
                c.skipped,
                c.failed.len()
            );
            for id in c.added.iter().take(20) {
                let _ = writeln!(s, "  new {id}");
            }
        }
        s
    }
}

/// `world.objects`.
pub struct WorldObjects;
impl WorldOp for WorldObjects {
    const NAME: &'static str = "world.objects";
    const SUMMARY: &'static str = "The objects a CA's field holds now (still lifes, oscillators, spaceships, guns, and the rest), each run alone to class it, with where it is; optionally verify and add each distinct one to the library.";
    const READ_ONLY: bool = false;
    type In = ObjectsIn;
    type Out = ObjectsOut;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: ObjectsIn,
        _: &dyn Progress,
    ) -> Result<ObjectsOut, ApiError> {
        use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects};
        let w = host.world();
        let ca = match &a.ca {
            None | Some(Value::Null) => None,
            v => Some(w.cas[ca_index(w, v.as_ref())?].id),
        };
        let max_period = a.max_period.unwrap_or(64).clamp(1, 1024);
        let opts = ObjectOptions {
            ca,
            max_period,
            ..ObjectOptions::default()
        };
        let r = find_objects(w, &opts).kind(ErrorKind::Invalid)?;
        let g = Grid::of(&w.topo);
        let kind = |c: &ObjClass| c.kind().replace(' ', "_");
        let label = |name: &str, c: &ObjClass| match c.tag() {
            Some(t) if !name.contains(&t) => format!("{name} ({t})"),
            _ => name.to_string(),
        };
        const LISTED: usize = 200;
        let objects = r
            .objects
            .iter()
            .take(LISTED)
            .map(|o| {
                let xy_box = g.filter(|_| !o.cells.is_empty()).map(|g| {
                    let (mut b, mut first) = ([usize::MAX, 0, usize::MAX, 0], true);
                    for &c in &o.cells {
                        let (x, y) = g.xy(c as usize);
                        if first {
                            b = [x, x, y, y];
                            first = false;
                        }
                        b = [b[0].min(x), b[1].max(x), b[2].min(y), b[3].max(y)];
                    }
                    b
                });
                FoundObject {
                    id: o.id,
                    label: label(&o.name, &o.class),
                    kind: kind(&o.class),
                    period: o.class.period(),
                    speed: o.class.speed_in(r.lattice),
                    displacement: match o.class {
                        ObjClass::Spaceship { dx, dy, .. } => Some([dx, dy]),
                        _ => None,
                    },
                    size: o.size,
                    xy_box,
                    first_cell: o.cells.first().copied(),
                    reason: match &o.class {
                        ObjClass::Other { reason } => Some(reason.clone()),
                        _ => None,
                    },
                }
            })
            .collect();
        let t = r.class_totals();
        let totals = ["still_life", "oscillator", "spaceship", "other"]
            .iter()
            .zip(t)
            .map(|(k, n)| (k.to_string(), n))
            .collect();
        let counts = r
            .counts
            .iter()
            .map(|c| ObjectCount {
                label: c.label(),
                kind: kind(&c.class),
                count: c.count,
            })
            .collect();
        let catalog = if a.catalog {
            Some(
                crate::catalog::catalog_world(
                    &cx.library(),
                    w,
                    Some(r.ca_id),
                    Some(max_period),
                    "world.objects",
                    jetcarp_core::pattern::DiscoveredBy::User,
                )
                .map_err(crate::ops::library::catalog_err)?,
            )
        } else {
            None
        };
        Ok(ObjectsOut {
            summary: r.summary_line(),
            generation: w.generation,
            ca_id: r.ca_id,
            rule: r.rule.clone(),
            live_cells: r.live_cells,
            totals,
            counts,
            more: r.objects.len().saturating_sub(LISTED),
            objects,
            catalog,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Reseeding

/// Reseed now, set the automatic policies, or (with neither) report them.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReseedIn {
    /// Reseed now with this mode: "random", "rain", "blobs", a name and a sign ("rain:pos"), or
    /// an object such as {"kind": "rain", "drops": 30}.
    #[serde(default)]
    pub mode: Option<Value>,
    /// With `mode`: only this CA's field. Default: every running CA's field.
    #[serde(default)]
    #[schemars(schema_with = "ca_ref_schema")]
    pub ca: Option<Value>,
    /// Replace the automatic policies ([] clears them).
    #[serde(default)]
    pub policies: Option<Value>,
    /// Recent generations of activity to report (default 64, at most 512).
    #[serde(default)]
    pub activity_gens: Option<Int>,
}

/// A policy and what it has done.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PolicyStatus {
    pub policy: Value,
    pub status: Value,
}

/// Recent activity: one row per generation.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Activity {
    /// `generation`, `changed`, `fraction`.
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
}

/// Reseed policies, their status, and recent activity.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ReseedReport {
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub policies: Vec<PolicyStatus>,
    pub last_activity: Value,
    pub activity: Activity,
    /// What this call did.
    pub done: Vec<String>,
}

impl Report for ReseedReport {
    fn text(&self) -> String {
        to_pretty(self)
    }
}

/// Policies, their status, and the last `recent` generations of activity.
pub fn reseed_report(w: &World, recent: usize) -> ReseedReport {
    let hist = w.activity_history();
    ReseedReport {
        generation: w.generation,
        policies: w
            .reseed_policies()
            .iter()
            .zip(w.reseed_status())
            .map(|(p, s)| PolicyStatus {
                policy: json!(p),
                status: json!(s),
            })
            .collect(),
        last_activity: json!(w.last_activity()),
        activity: Activity {
            columns: vec!["generation".into(), "changed".into(), "fraction".into()],
            rows: hist
                .iter()
                .skip(hist.len().saturating_sub(recent))
                .map(|a| json!([a.generation, a.changed, a.fraction]))
                .collect(),
        },
        done: vec![],
    }
}

/// A mode from an object, a bare name, a name and a sign ("rain:pos"), or JSON text.
fn reseed_mode(v: &Value) -> Result<ReseedMode, ApiError> {
    args::reseed_mode(v).map_err(|e| {
        ApiError::invalid(match v {
            Value::String(t) if t.contains(':') && !t.trim_start().starts_with('{') => {
                format!("could not read `mode` {t}: {e}")
            }
            Value::String(s) => format!(
                "`mode` is random, rain, blobs, or an object such as {{\"kind\": \"rain\", \"drops\": 30}}, not {s}"
            ),
            v => format!("could not read `mode` {v}: {e}"),
        })
    })
}

/// A policy (mode and trigger may be bare names: "rain", "settled").
fn reseed_policy(v: &Value) -> Result<AutoReseed, ApiError> {
    args::reseed_policy(v).map_err(|e| {
        ApiError::invalid(format!("could not read policy {v}: {e}. A policy is {{\"mode\": \"rain\" or {{\"kind\": \"rain\", ...}}, \"when\": \"settled\" or {{\"kind\": \"every\", \"gens\": 500}}, \"cooldown\": 200}}"))
    })
}

/// `world.reseed`.
pub struct WorldReseed;
impl WorldOp for WorldReseed {
    const NAME: &'static str = "world.reseed";
    const SUMMARY: &'static str = "Put new life into a settled world now (random flips, rain, blobs), set or clear the automatic reseed policies, and report their status and recent activity.";
    const READ_ONLY: bool = false;
    type In = ReseedIn;
    type Out = ReseedReport;
    fn run(
        _: &Ctx,
        host: &mut dyn Host,
        a: ReseedIn,
        _: &dyn Progress,
    ) -> Result<ReseedReport, ApiError> {
        let mut did = Vec::new();
        if let Some(list) = a.policies.as_ref().filter(|v| !v.is_null()) {
            let Value::Array(items) = list else {
                return Err(ApiError::invalid(
                    "`policies` must be a list of policy objects ([] clears them)",
                ));
            };
            let policies = items
                .iter()
                .map(reseed_policy)
                .collect::<Result<Vec<_>, _>>()?;
            let n = policies.len();
            host.exec(Command::SetReseed { policies })?;
            did.push(format!("set {n} automatic reseed policies"));
        }
        if let Some(m) = a.mode.as_ref().filter(|v| !v.is_null()) {
            let mode = reseed_mode(m)?;
            let ca = match a.ca.as_ref().filter(|v| !v.is_null()) {
                Some(v) => {
                    let w = host.world();
                    Some(w.cas[ca_index(w, Some(v))?].id)
                }
                None => None,
            };
            let name = mode.name();
            host.exec(Command::Reseed { ca, mode })?;
            did.push(format!("reseeded ({name})"));
        } else if a.ca.as_ref().is_some_and(|v| !v.is_null()) {
            return Err(ApiError::invalid("`ca` goes with `mode`"));
        }
        let recent = a.activity_gens.unwrap_or(64).clamp(0, 512) as usize;
        let mut r = reseed_report(host.world(), recent);
        r.done = did;
        Ok(r)
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

/// Rule-name and seed shorthands inside a raw command, plus the limits.
pub fn prepare_command(v: &Value, w: &World, l: &Limits) -> Result<Command, ApiError> {
    let mut v = v.clone();
    let op = v
        .get("op")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();
    if op.is_empty() {
        return Err(ApiError::invalid(format!(
            "command {v} has no \"op\". Ops: set_tiling, set_rng_state, reset, add_ca, remove_ca, update_ca, seed, clear, set_cells, step, add_table, set_script, script_goto, set_palette, sandpile, reseed, set_reseed"
        )));
    }
    if op == "add_ca"
        && let Some(ca) = v.get("ca")
    {
        let spec = ca_item(ca, l)?;
        v["ca"] = serde_json::to_value(spec).kind(ErrorKind::Invalid)?;
    }
    if let Some(r) = v.pointer("/change/rule").and_then(|r| r.as_str()) {
        v["change"]["rule"] = json!(resolve_rule(r));
    }
    for ptr in ["/seed", "/change/seed"] {
        if let Some(s) = v.pointer(ptr).cloned() {
            let mut seed = parse_seed(&s).map_err(ApiError::invalid)?;
            cap_seed(&mut seed, l);
            *v.pointer_mut(ptr).unwrap() = serde_json::to_value(seed).kind(ErrorKind::Invalid)?;
        }
    }
    if op == "sandpile"
        && let Some(a) = v.get("action").and_then(|a| a.as_str())
    {
        v["action"] = json!(a.to_ascii_lowercase());
    }
    let cmd: Command = serde_json::from_value(v.clone()).map_err(|e| {
        ApiError::invalid(format!("could not read command {v}: {e}. reference {{\"what\": \"commands\"}} shows the command shapes."))
    })?;
    match &cmd {
        Command::Step { gens } => check_work(w, *gens, l)?,
        Command::SetTiling { tiling } => check_tiling_size(tiling, l)?,
        Command::SetCells { cells, .. } if cells.len() > l.max_set => {
            return Err(ApiError::limit(format!(
                "at most {} cells per set_cells",
                l.max_set
            )));
        }
        Command::Sandpile {
            action: jetcarp_core::world::SandpileOp::AddRandom { count },
            ..
        } if *count > 100_000_000 => {
            return Err(ApiError::limit(
                "add_random count must be at most 100,000,000",
            ));
        }
        _ => {}
    }
    Ok(cmd)
}

/// Commands to apply, in order.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyIn {
    /// Command objects, each with an "op" field (one object, or their JSON text, works too):
    /// add_ca, remove_ca, update_ca, seed, clear, set_cells, step, reset, set_rng_state,
    /// set_tiling, sandpile, set_palette, reseed, set_reseed, ...
    pub commands: Value,
}

/// Commands applied.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Applied {
    /// One line per command: "1. step: ok", "2. add_ca: ok, new CA id 2".
    pub lines: Vec<String>,
    /// Ids of CAs added, in order.
    pub new_ca_ids: Vec<u32>,
    #[serde(skip)]
    status_line: String,
}

impl Report for Applied {
    fn text(&self) -> String {
        format!("{}\n{}", self.lines.join("\n"), self.status_line)
    }
}

/// `world.apply`.
pub struct WorldApply;
impl WorldOp for WorldApply {
    const NAME: &'static str = "world.apply";
    const SUMMARY: &'static str = "Apply JETCARP commands in order (the commands the app's buttons issue, recorded in the scene's log); stops at the first that fails.";
    const READ_ONLY: bool = false;
    type In = ApplyIn;
    type Out = Applied;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: ApplyIn,
        _: &dyn Progress,
    ) -> Result<Applied, ApiError> {
        let list = match a.commands {
            Value::Array(a) => a,
            o @ Value::Object(_) => vec![o],
            Value::String(s) => match serde_json::from_str::<Value>(&s) {
                Ok(Value::Array(a)) => a,
                Ok(o @ Value::Object(_)) => vec![o],
                _ => {
                    return Err(ApiError::invalid(
                        "commands must be a list of command objects",
                    ));
                }
            },
            _ => {
                return Err(ApiError::invalid(
                    "commands must be a list of command objects, e.g. [{\"op\":\"step\",\"gens\":10}]",
                ));
            }
        };
        if list.is_empty() {
            return Err(ApiError::invalid("commands is empty"));
        }
        let mut lines = Vec::new();
        let mut new_ids = Vec::new();
        let stop = |lines: &[String], kind: ErrorKind, host: &dyn Host| {
            ApiError::new(
                kind,
                format!(
                    "{}\nStopped; later commands were not applied.\n{}",
                    lines.join("\n"),
                    short_status(host.world())
                ),
            )
        };
        for (k, v) in list.iter().enumerate() {
            let cmd = match prepare_command(v, host.world(), &cx.limits) {
                Ok(c) => c,
                Err(e) => {
                    lines.push(format!("{}. not applied: {}", k + 1, e.message));
                    return Err(stop(&lines, e.kind, host));
                }
            };
            let op = v
                .get("op")
                .and_then(|o| o.as_str())
                .unwrap_or("?")
                .to_string();
            match host.exec(cmd) {
                Ok(a) => lines.push(match a.new_id {
                    Some(id) => {
                        new_ids.push(id);
                        format!("{}. {op}: ok, new CA id {id}", k + 1)
                    }
                    None => format!("{}. {op}: ok", k + 1),
                }),
                Err(e) => {
                    lines.push(format!("{}. {op}: failed: {e}", k + 1));
                    return Err(stop(&lines, ErrorKind::Engine, host));
                }
            }
        }
        Ok(Applied {
            lines,
            new_ca_ids: new_ids,
            status_line: short_status(host.world()),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Stepping and pictures

/// How far to run.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepIn {
    /// Generations to run (default 1, or up to 5000 with `stop_when_settled`).
    #[serde(default)]
    pub generations: Option<Int>,
    /// Stop as soon as the world is empty, frozen, or exactly periodic.
    #[serde(default)]
    pub stop_when_settled: bool,
}

/// Generations run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Stepped {
    pub ran: u64,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub seconds: f64,
    /// `cancelled`, or `settled` (stopped early), when it stopped before the count.
    pub stopped: Option<String>,
    /// Every cell is dead and nothing can change.
    pub died_out: bool,
    pub status: String,
    #[serde(skip)]
    text: String,
}

impl Report for Stepped {
    fn text(&self) -> String {
        self.text.clone()
    }
}

/// `world.step`.
pub struct WorldStep;
impl WorldOp for WorldStep {
    const NAME: &'static str = "world.step";
    const SUMMARY: &'static str = "Run generations (every enabled CA together), optionally stopping once the world is empty, frozen, or periodic.";
    const READ_ONLY: bool = false;
    type In = StepIn;
    type Out = Stepped;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: StepIn,
        p: &dyn Progress,
    ) -> Result<Stepped, ApiError> {
        let settle = a.stop_when_settled;
        let gens = a
            .generations
            .unwrap_or(if settle { SETTLE_GENS as Int } else { 1 });
        if gens < 1 {
            return Err(ApiError::invalid("generations must be at least 1"));
        }
        let gens = gens as u64;
        check_work(host.world(), gens, &cx.limits)?;
        if host.world().cas.iter().all(|c| !c.enabled) {
            return Err(ApiError::invalid(
                "no CA is enabled, so nothing would change. Enable one with the command update_ca {\"enabled\": true} (world_edit commands), or add one.",
            ));
        }
        let g0 = host.world().generation;
        let t0 = std::time::Instant::now();
        if settle {
            for _ in 0..gens {
                host.exec(Command::Step { gens: 1 })?;
                if host.world().status() != Status::Running || p.cancelled() {
                    break;
                }
            }
        } else {
            // In chunks of about a second's work, so a cancel stops it.
            let cells = host.world().topo.len().max(1) as u64;
            let chunk = (50_000_000 / cells).clamp(1, gens);
            let mut left = gens;
            while left > 0 && !p.cancelled() {
                let n = chunk.min(left);
                host.exec(Command::Step { gens: n })?;
                left -= n;
                p.report(gens - left, Some(gens), "generations");
            }
        }
        let seconds = t0.elapsed().as_secs_f64();
        let w = host.world();
        let ran = w.generation - g0;
        let dead =
            w.fields.iter().all(|f| f.is_empty()) && w.cas.iter().all(|c| c.agents.is_empty());
        let hint = if dead {
            "\nThe world has died out: nothing will change from here. Some rules die out on some tilings \
             from any random start (other seeds usually behave the same). If the task asked for this run, \
             report that it died out; otherwise try another rule, tiling, or neighbourhood."
        } else {
            ""
        };
        let stopped = if p.cancelled() && ran < gens {
            Some("cancelled")
        } else if settle && ran < gens {
            Some("settled")
        } else {
            None
        };
        let text = format!(
            "Ran {ran} generation(s) in {seconds:.2}s{}.\n{}{hint}",
            match stopped {
                Some("cancelled") => " (cancelled)",
                Some(_) => " (stopped early: settled)",
                None => "",
            },
            short_status(w)
        );
        Ok(Stepped {
            ran,
            generation: w.generation,
            seconds,
            stopped: stopped.map(String::from),
            died_out: dead,
            status: status_text(w),
            text,
        })
    }
}

/// A picture of the world.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderIn {
    /// Longest image side in pixels, 64 to 2048 (default 640).
    #[serde(default)]
    pub max_side: Option<Int>,
    /// Draw tile outlines (default: only when tiles are at least 7 px).
    #[serde(default)]
    pub outlines: Option<bool>,
    /// Also write the PNG to this file (scene embedded; its folder is created).
    #[serde(default)]
    pub save_path: Option<String>,
}

/// A picture made.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub tiling: String,
    pub cells: usize,
    pub hash: String,
    /// Where the PNG was saved.
    pub saved: Option<std::path::PathBuf>,
    /// The PNG (version, generation, and hash in its text chunks).
    #[serde(skip)]
    pub png: Vec<u8>,
}

impl Report for Rendered {
    fn text(&self) -> String {
        let mut text = format!(
            "{} x {} px picture of generation {} ({}, {} cells; hash {}).",
            self.width, self.height, self.generation, self.tiling, self.cells, self.hash
        );
        if let Some(p) = &self.saved {
            let _ = write!(text, " Saved to {}.", p.display());
        }
        text
    }
}

/// `world.render`.
pub struct WorldRender;
impl WorldOp for WorldRender {
    const NAME: &'static str = "world.render";
    const SUMMARY: &'static str = "A PNG picture of the whole world (empty cells dark, live cells in the CA's palette), optionally saved to a file with the scene embedded.";
    const READ_ONLY: bool = true;
    type In = RenderIn;
    type Out = Rendered;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: RenderIn,
        _: &dyn Progress,
    ) -> Result<Rendered, ApiError> {
        let side = a.max_side.unwrap_or(640);
        if !(64..=2048).contains(&side) {
            return Err(ApiError::invalid("max_side must be 64 to 2048"));
        }
        let mut opts = RenderOptions {
            max_side: side as u32,
            ..Default::default()
        };
        match a.outlines {
            Some(true) => opts.outline_min_px = 2.0,
            Some(false) => opts.outline_min_px = 0.0,
            None => {}
        }
        let w = host.world();
        let img = render::render(w, &opts);
        let r = w.record();
        let png = crate::out::encode_png_rgba(
            &img,
            &[
                ("jetcarp:version", r.jetcarp_version.clone()),
                ("jetcarp:generation", r.generation.to_string()),
                ("jetcarp:hash", r.hash.clone()),
            ],
        )?;
        let saved = match &a.save_path {
            Some(p) => {
                let path = cx.fs.writable(p)?;
                render::write_png(&path, &img, &render::png_metadata(w))?;
                Some(path)
            }
            None => None,
        };
        Ok(Rendered {
            width: img.width,
            height: img.height,
            generation: w.generation,
            tiling: w.topo.family.name.to_string(),
            cells: w.topo.len(),
            hash: r.hash,
            saved,
            png,
        })
    }
}
