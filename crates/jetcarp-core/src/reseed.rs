//! Reseeding: putting new life into a world that has settled down.
//!
//! Three modes, all on any tiling:
//!
//! - **Random** flips every cell with a probability (live to dead, dead to live).
//! - **Rain** scatters drops: small, roughly round patches whose cells come alive with a
//!   probability. Drops only add cells.
//! - **Blobs** drops a few large patches with an irregular outline. Every cell inside is set live
//!   or dead at a set ratio, overwriting what was there.
//!
//! On JACA fields (signed cells, see [`crate::jaca`]) every mode also takes a [`Sign`]: new live
//! cells can be +1, -1, a mix, one random sign per drop or blob, or alternate. Other fields
//! ignore it: their live cells are always state 1.
//!
//! Sizes use the typical tile size (the square root of the mean tile area, see
//! [`crate::world::cell_size`]) and cell centroids, so a drop 6 tiles across looks the same on
//! squares, hexagons, Archimedean tilings, and aperiodic patches. Wrap-edge lattices wrap patches
//! across their edges.
//!
//! A reseed is planned as a list of [`Edits`] that depends only on the tiling and the world RNG,
//! not on the field's current states (a rain drop "births" a cell only if it is dead, but which
//! cells a drop covers does not depend on that). The CPU applies the list to a field directly;
//! `jetcarp-gpu` applies the same list to its copy of a running field, so both give identical states.
//!
//! The world runs [`AutoReseed`] policies after each generation (see `World::step`): when the
//! world has settled (activity below a threshold for a number of generations, a short exact
//! period, or an empty field), or every N generations. [`Command::Reseed`](crate::Command)
//! reseeds by hand.

use crate::geom::P;
use crate::rng::Rng;
use crate::rules::FieldKind;
use crate::topology::Topology;
use crate::world::Field;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// How to reseed a field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReseedMode {
    /// Each cell flips with probability `flip`: dead cells become live (state 1), live and dying
    /// cells (any nonzero state) become dead.
    Random {
        #[serde(default = "d_flip")]
        flip: f64,
        #[serde(default, skip_serializing_if = "Sign::is_default")]
        sign: Sign,
    },
    /// `drops` drops at random cells. Each drop is a round patch `min_size..=max_size` tiles
    /// across (small sizes more likely); each dead cell in it comes alive with probability
    /// `density`.
    Rain {
        #[serde(default = "d_drops")]
        drops: u32,
        #[serde(default = "d_min_size")]
        min_size: f64,
        #[serde(default = "d_max_size")]
        max_size: f64,
        #[serde(default = "d_density")]
        density: f64,
        #[serde(default, skip_serializing_if = "Sign::is_default")]
        sign: Sign,
    },
    /// `count` blobs at random cells, each with a radius between `min_radius` and `max_radius`
    /// (fractions of the grid's shorter side) and a wobbly outline. Every cell inside is set live
    /// with probability `live` and dead otherwise.
    Blobs {
        #[serde(default = "d_count")]
        count: u32,
        #[serde(default = "d_min_radius")]
        min_radius: f64,
        #[serde(default = "d_max_radius")]
        max_radius: f64,
        #[serde(default = "d_live")]
        live: f64,
        #[serde(default, skip_serializing_if = "Sign::is_default")]
        sign: Sign,
    },
}

/// The sign of new live cells on a JACA field (ignored elsewhere: live is state 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    /// Every new cell +1.
    Pos,
    /// Every new cell -1.
    Neg,
    /// Each cell +1 or -1 at random.
    Mixed,
    /// Each drop or blob all +1 or all -1, at random (random mode: each cell, as mixed).
    #[default]
    Either,
    /// Drops and blobs alternate +1, -1, ...; the first sign flips with each reseed, so one
    /// blob per reseed alternates too (random mode: the whole reseed takes one sign).
    Alternate,
}

impl Sign {
    pub const ALL: [Sign; 5] = [
        Sign::Either,
        Sign::Mixed,
        Sign::Alternate,
        Sign::Pos,
        Sign::Neg,
    ];
    pub fn is_default(&self) -> bool {
        *self == Sign::Either
    }
    pub fn name(self) -> &'static str {
        match self {
            Sign::Pos => "pos",
            Sign::Neg => "neg",
            Sign::Mixed => "mixed",
            Sign::Either => "either",
            Sign::Alternate => "alternate",
        }
    }
    /// Parses a name (also `+`, `+1`, `positive`, `-`, `-1`, `negative`, `mix`, `random`,
    /// `alt`).
    pub fn parse(s: &str) -> Option<Sign> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "pos" | "+" | "+1" | "1" | "positive" | "plus" => Sign::Pos,
            "neg" | "-" | "-1" | "negative" | "minus" => Sign::Neg,
            "mixed" | "mix" | "cells" => Sign::Mixed,
            "either" | "random" | "each" | "auto" => Sign::Either,
            "alternate" | "alt" | "alternating" => Sign::Alternate,
            _ => return None,
        })
    }
    /// Is cell `c` of patch `k` (drop or blob number; 0 for random mode) negative? `seed` is
    /// the reseed's sign seed, `turn` counts earlier reseeds (for alternate).
    fn neg(self, seed: u64, turn: u64, k: u64, c: u64, per_patch: bool) -> bool {
        match self {
            Sign::Pos => false,
            Sign::Neg => true,
            Sign::Mixed => mix(seed, c) & 1 == 1,
            Sign::Either if per_patch => mix(seed ^ 0x5eed, k) & 1 == 1,
            Sign::Either => mix(seed, c) & 1 == 1,
            Sign::Alternate => (turn + k) & 1 == 1,
        }
    }
}

fn d_flip() -> f64 {
    0.05
}
fn d_drops() -> u32 {
    24
}
fn d_min_size() -> f64 {
    2.0
}
fn d_max_size() -> f64 {
    10.0
}
fn d_density() -> f64 {
    0.5
}
fn d_count() -> u32 {
    1
}
fn d_min_radius() -> f64 {
    0.06
}
fn d_max_radius() -> f64 {
    0.18
}
fn d_live() -> f64 {
    0.35
}

impl ReseedMode {
    /// Random with the default flip probability.
    pub fn random() -> ReseedMode {
        ReseedMode::Random {
            flip: d_flip(),
            sign: Sign::default(),
        }
    }
    /// Rain with the default drop count, sizes, and density.
    pub fn rain() -> ReseedMode {
        ReseedMode::Rain {
            drops: d_drops(),
            min_size: d_min_size(),
            max_size: d_max_size(),
            density: d_density(),
            sign: Sign::default(),
        }
    }
    /// One blob with the default radius range and live ratio.
    pub fn blobs() -> ReseedMode {
        ReseedMode::Blobs {
            count: d_count(),
            min_radius: d_min_radius(),
            max_radius: d_max_radius(),
            live: d_live(),
            sign: Sign::default(),
        }
    }
    /// "random", "rain", or "blobs".
    pub fn name(&self) -> &'static str {
        match self {
            ReseedMode::Random { .. } => "random",
            ReseedMode::Rain { .. } => "rain",
            ReseedMode::Blobs { .. } => "blobs",
        }
    }
    /// The sign of new cells on JACA fields.
    pub fn sign(&self) -> Sign {
        match *self {
            ReseedMode::Random { sign, .. }
            | ReseedMode::Rain { sign, .. }
            | ReseedMode::Blobs { sign, .. } => sign,
        }
    }
    pub fn sign_mut(&mut self) -> &mut Sign {
        match self {
            ReseedMode::Random { sign, .. }
            | ReseedMode::Rain { sign, .. }
            | ReseedMode::Blobs { sign, .. } => sign,
        }
    }
    /// The same mode with `n` drops or blobs (random mode is unchanged).
    pub fn with_count(mut self, n: u32) -> ReseedMode {
        match &mut self {
            ReseedMode::Rain { drops, .. } => *drops = n,
            ReseedMode::Blobs { count, .. } => *count = n,
            ReseedMode::Random { .. } => {}
        }
        self
    }
    /// The same mode with another sign.
    pub fn with_sign(mut self, s: Sign) -> ReseedMode {
        *self.sign_mut() = s;
        self
    }
    /// Check the parameters.
    pub fn validate(&self) -> Result<(), String> {
        let prob = |x: f64, what: &str| {
            if (0.0..=1.0).contains(&x) {
                Ok(())
            } else {
                Err(format!("reseed: {what} must be between 0 and 1 (got {x})"))
            }
        };
        match *self {
            ReseedMode::Random { flip, .. } => prob(flip, "flip"),
            ReseedMode::Rain {
                drops,
                min_size,
                max_size,
                density,
                ..
            } => {
                prob(density, "density")?;
                if !(min_size > 0.0 && max_size >= min_size && max_size <= 10_000.0) {
                    return Err(format!(
                        "reseed: rain sizes need 0 < min_size <= max_size (got {min_size}..{max_size})"
                    ));
                }
                if drops > 1_000_000 {
                    return Err("reseed: at most 1,000,000 drops".into());
                }
                Ok(())
            }
            ReseedMode::Blobs {
                count,
                min_radius,
                max_radius,
                live,
                ..
            } => {
                prob(live, "live")?;
                if !(min_radius > 0.0 && max_radius >= min_radius && max_radius <= 2.0) {
                    return Err(format!(
                        "reseed: blob radii need 0 < min_radius <= max_radius <= 2 (fractions of the shorter side; got {min_radius}..{max_radius})"
                    ));
                }
                if count > 10_000 {
                    return Err("reseed: at most 10,000 blobs".into());
                }
                Ok(())
            }
        }
    }
}

/// When an automatic reseed fires.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReseedWhen {
    /// The world has settled. Checked every [`SETTLE_CHECK`] generations (once the cooldown is
    /// over), it fires when any of these holds:
    ///
    /// - the fraction of cells that changed per generation (over the fields of the running CAs)
    ///   stayed below `activity` for the last `for_gens` generations;
    /// - the number of cells changed per generation has repeated with a period of at most
    ///   `cycle` for the last `for_gens` generations: only oscillators and gliders are left (Life
    ///   ash keeps about 1% of cells blinking, so a threshold alone would not see it);
    /// - the exact period watch found a period of at most `period`;
    /// - the running fields are empty;
    /// - `latest` (if set) generations have passed since the policy last fired (or was installed),
    ///   whatever the activity: a screensaver never stalls on a rule whose activity never looks
    ///   settled, or while other policies' rain keeps disturbing it.
    Settled {
        #[serde(default = "d_activity")]
        activity: f64,
        #[serde(default = "d_for_gens", with = "crate::json_int::uint")]
        for_gens: u64,
        #[serde(default = "d_cycle")]
        cycle: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period: Option<u64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "crate::json_int::uint_opt"
        )]
        latest: Option<u64>,
    },
    /// Every `gens` generations (1 = every generation, a continuous drizzle).
    Every {
        #[serde(with = "crate::json_int::uint")]
        gens: u64,
    },
}

/// Settled policies fire only at generations that are multiples of this, so a GPU stepper can
/// run whole batches up to the next check instead of stopping after each generation.
pub const SETTLE_CHECK: u64 = 16;
/// Longest `cycle` the steady check looks for.
pub const MAX_CYCLE: u64 = 64;

fn d_activity() -> f64 {
    0.002
}
fn d_for_gens() -> u64 {
    60
}
fn d_cycle() -> Option<u64> {
    Some(12)
}

impl ReseedWhen {
    /// Settled with the defaults: activity below 0.2%, or a change count that repeats with a
    /// period up to 12, for 60 generations (no exact-period check).
    pub fn settled() -> ReseedWhen {
        ReseedWhen::Settled {
            activity: d_activity(),
            for_gens: d_for_gens(),
            cycle: d_cycle(),
            period: None,
            latest: None,
        }
    }
}

/// An automatic reseed: a mode, a trigger, a target, and a cooldown. Saved in scenes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoReseed {
    #[serde(deserialize_with = "mode_or_name")]
    pub mode: ReseedMode,
    #[serde(deserialize_with = "when_or_name")]
    pub when: ReseedWhen,
    /// The CA whose field is reseeded; `None` reseeds the field of every running CA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<u32>,
    /// Generations after this policy fires (or is installed, or a manual reseed) during which it
    /// cannot fire, so a new seed can develop. Settled policies start counting quiet generations
    /// only after it.
    #[serde(default, with = "crate::json_int::uint")]
    pub cooldown: u64,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// A mode as its object or just its name ("random", "rain", "blobs": the defaults), optionally
/// with a sign ("rain:mixed").
pub fn mode_or_name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ReseedMode, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let v = match v {
        serde_json::Value::String(s) => mode_json(&s),
        v => v,
    };
    serde_json::from_value(v).map_err(serde::de::Error::custom)
}

/// `"rain"` or `"rain:mixed"` as a mode object (`{"kind": "rain", "sign": "mixed"}`); signs
/// may use any name [`Sign::parse`] takes.
pub fn mode_json(s: &str) -> serde_json::Value {
    match s.split_once(':') {
        Some((k, sign)) => {
            let sign = Sign::parse(sign).map_or(sign.trim().to_string(), |x| x.name().into());
            serde_json::json!({ "kind": k.trim(), "sign": sign })
        }
        None => serde_json::json!({ "kind": s.trim() }),
    }
}

/// A trigger as its object or just its name ("settled": the defaults).
pub fn when_or_name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ReseedWhen, D::Error> {
    named(d)
}

fn named<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let v = match v {
        serde_json::Value::String(s) => serde_json::json!({ "kind": s }),
        v => v,
    };
    serde_json::from_value(v).map_err(serde::de::Error::custom)
}

impl AutoReseed {
    pub fn new(mode: ReseedMode, when: ReseedWhen) -> AutoReseed {
        AutoReseed {
            mode,
            when,
            ca: None,
            cooldown: 0,
            enabled: true,
        }
    }
    pub fn cooldown(mut self, gens: u64) -> Self {
        self.cooldown = gens;
        self
    }
    pub fn target(mut self, ca: u32) -> Self {
        self.ca = Some(ca);
        self
    }
    pub fn validate(&self) -> Result<(), String> {
        self.mode.validate()?;
        match self.when {
            ReseedWhen::Settled {
                activity,
                for_gens,
                cycle,
                latest,
                ..
            } => {
                if latest == Some(0) {
                    return Err("reseed: latest must be at least 1 generation".into());
                }
                if !(0.0..=1.0).contains(&activity) {
                    return Err(format!(
                        "reseed: activity must be between 0 and 1 (got {activity})"
                    ));
                }
                if for_gens == 0 {
                    return Err("reseed: for_gens must be at least 1".into());
                }
                if let Some(c) = cycle
                    && !(1..=MAX_CYCLE).contains(&c)
                {
                    return Err(format!("reseed: cycle must be 1 to {MAX_CYCLE} (got {c})"));
                }
                if cycle.is_some() && for_gens + cycle.unwrap_or(0) > 500 {
                    return Err("reseed: for_gens + cycle must be at most 500".into());
                }
            }
            ReseedWhen::Every { gens } => {
                if gens == 0 {
                    return Err("reseed: every needs at least 1 generation".into());
                }
            }
        }
        Ok(())
    }
}

/// What a policy has done so far. Part of the state hash while policies are installed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyStatus {
    /// Generation the policy was installed or last fired (or a manual reseed ran): cooldowns and
    /// `every` count from here.
    pub since: u64,
    /// Quiet generations in a row (settled policies; counted after the cooldown).
    pub streak: u64,
    /// Times fired.
    pub fired: u64,
    pub last_fired: Option<u64>,
    /// Why the last firing did nothing (no such CA, a sandpile field), if it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Edits

/// Bits of an edit word that hold the cell index (fields have at most 2^28 cells).
pub const EDIT_CELL: u32 = 0x0fff_ffff;
/// Edit ops (the top four bits of an edit word).
pub const EDIT_BIRTH: u32 = 0;
pub const EDIT_KILL: u32 = 1;
pub const EDIT_LIVE: u32 = 2;
pub const EDIT_FLIP: u32 = 3;
/// Set on a birth, live, or flip word whose new cell is -1 (JACA fields only; see [`Sign`]).
pub const EDIT_NEG: u32 = 1 << 31;

/// A planned reseed of one field: one word per cell, `cell | op << 28`, each cell at most once,
/// in increasing cell order. Ops: birth (a dead cell becomes live), kill (dead), live (live), and
/// flip (dead to live, anything else to dead). Live cells get state 1 and age 1 (state 2, the -1
/// of a JACA field, when the word has [`EDIT_NEG`]); dead cells state 0 and age 0. Continuous fields take a value in (0, 1] from `value_seed` for a live cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edits {
    pub words: Vec<u32>,
    pub value_seed: u64,
}

impl Edits {
    pub fn len(&self) -> usize {
        self.words.len()
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Apply to a state or continuous field (sandpile fields are refused when planning).
    pub fn apply(&self, f: &mut Field) {
        let n = f.state.len().max(f.value.len());
        if f.kind == FieldKind::Continuous {
            for &w in &self.words {
                let c = (w & EDIT_CELL) as usize;
                if c >= n {
                    continue;
                }
                let v = 1.0 - unit(mix(self.value_seed, c as u64)) as f32;
                let old = f.value[c];
                match w >> 28 & 3 {
                    EDIT_BIRTH if old == 0.0 => f.set_value(c, v),
                    EDIT_KILL => f.set_value(c, 0.0),
                    EDIT_LIVE => f.set_value(c, v),
                    EDIT_FLIP => f.set_value(c, if old == 0.0 { v } else { 0.0 }),
                    _ => {}
                }
            }
            return;
        }
        if f.kind == FieldKind::Height {
            return;
        }
        for &w in &self.words {
            let c = (w & EDIT_CELL) as usize;
            if c >= n {
                continue;
            }
            let live = match w >> 28 & 3 {
                EDIT_BIRTH if f.state[c] == 0 => true,
                EDIT_BIRTH => continue,
                EDIT_KILL => false,
                EDIT_LIVE => true,
                _ => f.state[c] == 0,
            };
            f.state[c] = if !live {
                0
            } else if w & EDIT_NEG != 0 {
                crate::jaca::NEG
            } else {
                1
            };
            f.age[c] = live as u16;
        }
    }

    /// Each word's cell and op (without the sign).
    pub fn cells(&self) -> impl Iterator<Item = (usize, u32)> + '_ {
        self.words
            .iter()
            .map(|&w| ((w & EDIT_CELL) as usize, w >> 28 & 3))
    }
}

/// SplitMix64's finaliser over a seed and an index: per-cell coin flips that do not depend on
/// the order cells are visited in.
#[inline]
pub fn mix(seed: u64, i: u64) -> u64 {
    let mut z = seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
#[inline]
pub(crate) fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Plan a reseed of a field of `kind` on `topo`, drawing from `rng`. `tile` is the typical tile
/// size ([`crate::world::cell_size`]). Live cells are state 1 (see [`plan_signed`]).
pub fn plan(
    topo: &Topology,
    tile: f64,
    kind: FieldKind,
    mode: &ReseedMode,
    rng: &mut Rng,
) -> Result<Edits, String> {
    plan_signed(topo, tile, kind, mode, rng, None)
}

/// [`plan`], for a JACA field when `turn` is given: new live cells take the mode's [`Sign`]
/// (`turn` counts earlier reseeds, for alternate). The cells covered and the RNG draws are the
/// same either way.
pub fn plan_signed(
    topo: &Topology,
    tile: f64,
    kind: FieldKind,
    mode: &ReseedMode,
    rng: &mut Rng,
    turn: Option<u64>,
) -> Result<Edits, String> {
    mode.validate()?;
    if kind == FieldKind::Height {
        return Err("reseeding needs a state field; sandpiles have grain counts".into());
    }
    let n = topo.len();
    if n as u64 > EDIT_CELL as u64 + 1 {
        return Err(format!(
            "reseeding supports up to {} cells",
            EDIT_CELL as u64 + 1
        ));
    }
    let seed = rng.next_u64();
    let value_seed = rng.next_u64();
    let sign = mode.sign();
    let sseed = mix(seed, 0x0005_1612);
    // The sign bit for cell `c` of patch `k`.
    let neg = |k: u64, c: u32, per_patch: bool| match turn {
        Some(t) if sign.neg(sseed, t, k, c as u64, per_patch) => EDIT_NEG,
        _ => 0,
    };
    let mut words: Vec<u32> = Vec::new();
    if n == 0 {
        return Ok(Edits { words, value_seed });
    }
    match *mode {
        ReseedMode::Random { flip, .. } => {
            if flip > 0.0 {
                words = (0..n as u32)
                    .into_par_iter()
                    .with_min_len(1 << 16)
                    .filter(|&c| unit(mix(seed, c as u64)) < flip)
                    .map(|c| c | EDIT_FLIP << 28 | neg(0, c, false))
                    .collect();
            }
        }
        ReseedMode::Rain {
            drops,
            min_size,
            max_size,
            density,
            ..
        } => {
            for k in 0..drops as u64 {
                let at = topo.centroid(rng.below(n as u64) as usize);
                let u = rng.next_f64();
                // Small drops are more likely: size = min + (max - min) u^2.
                let size = min_size + (max_size - min_size) * u * u;
                let r = size * tile / 2.0;
                let r2 = r * r;
                let dseed = mix(seed, k);
                topo.for_cells_in_box([at[0] - r, at[1] - r, at[0] + r, at[1] + r], |c, p| {
                    let (dx, dy) = (p[0] - at[0], p[1] - at[1]);
                    if dx * dx + dy * dy <= r2 && unit(mix(dseed, c as u64)) < density {
                        words.push(c as u32 | EDIT_BIRTH << 28 | neg(k, c as u32, true));
                    }
                });
            }
            // One word per cell (+1 where drops of both signs overlap).
            words.par_sort_unstable_by_key(|w| (w & EDIT_CELL, w >> 28));
            words.dedup_by_key(|w| *w & EDIT_CELL);
        }
        ReseedMode::Blobs {
            count,
            min_radius,
            max_radius,
            live,
            ..
        } => {
            let b = topo.bounds;
            let side = (b[2] - b[0]).min(b[3] - b[1]).max(tile);
            for k in 0..count {
                let at = topo.centroid(rng.below(n as u64) as usize);
                let rr = side * (min_radius + (max_radius - min_radius) * rng.next_f64());
                let outline = Outline::draw(rng);
                let reach = rr * outline.max_factor();
                let bseed = mix(seed, k as u64);
                topo.for_cells_in_box(
                    [at[0] - reach, at[1] - reach, at[0] + reach, at[1] + reach],
                    |c, p| {
                        if outline.inside([p[0] - at[0], p[1] - at[1]], rr) {
                            let op = if unit(mix(bseed, c as u64)) < live {
                                EDIT_LIVE << 28 | neg(k as u64, c as u32, true)
                            } else {
                                EDIT_KILL << 28
                            };
                            words.push(c as u32 | op);
                        }
                    },
                );
            }
            // Later blobs overwrite earlier ones: keep the last word for each cell.
            let mut keyed: Vec<(u32, u32)> = words
                .iter()
                .enumerate()
                .map(|(i, &w)| (w & EDIT_CELL, i as u32))
                .collect();
            keyed.par_sort_unstable();
            let mut out = Vec::with_capacity(keyed.len());
            for (i, &(c, at)) in keyed.iter().enumerate() {
                if keyed.get(i + 1).is_none_or(|x| x.0 != c) {
                    out.push(words[at as usize]);
                }
            }
            words = out;
        }
    }
    Ok(Edits { words, value_seed })
}

/// A blob's outline: radius `r (1 + sum_k a_k cos(k theta - phi_k))` for k = 2..=5, which gives
/// lumpy, organic shapes rather than discs. Uses only arithmetic and square roots, so it is the
/// same on every platform.
pub(crate) struct Outline {
    /// Per harmonic: (a_k cos phi_k, a_k sin phi_k).
    h: [(f64, f64); 4],
}

impl Outline {
    fn draw(rng: &mut Rng) -> Outline {
        Outline::draw_wobbled(rng, 1.0)
    }
    /// An outline with its lumps scaled by `wobble` (0 is a circle).
    pub(crate) fn draw_wobbled(rng: &mut Rng, wobble: f64) -> Outline {
        let mut h = [(0.0, 0.0); 4];
        for (i, x) in h.iter_mut().enumerate() {
            let k = (i + 2) as f64;
            let a = 0.34 / k * (0.4 + 0.6 * rng.next_f64());
            // A random direction without trigonometry: a point in the unit disc, normalised.
            let (mut dx, mut dy);
            loop {
                dx = 2.0 * rng.next_f64() - 1.0;
                dy = 2.0 * rng.next_f64() - 1.0;
                let d2 = dx * dx + dy * dy;
                if d2 > 1e-6 && d2 <= 1.0 {
                    let d = d2.sqrt();
                    dx /= d;
                    dy /= d;
                    break;
                }
            }
            *x = (wobble * a * dx, wobble * a * dy);
        }
        Outline { h }
    }
    pub(crate) fn max_factor(&self) -> f64 {
        1.0 + self
            .h
            .iter()
            .map(|(c, s)| (c * c + s * s).sqrt())
            .sum::<f64>()
    }
    fn factor(&self, d: P, len: f64) -> f64 {
        // z = e^{i theta}; Re(z^k e^{-i phi}) = Re(z^k) cos phi + Im(z^k) sin phi.
        let (zr, zi) = (d[0] / len, d[1] / len);
        let (mut pr, mut pi) = (zr * zr - zi * zi, 2.0 * zr * zi);
        let mut f = 1.0;
        for &(c, s) in &self.h {
            f += pr * c + pi * s;
            let (nr, ni) = (pr * zr - pi * zi, pr * zi + pi * zr);
            pr = nr;
            pi = ni;
        }
        f
    }
    fn inside(&self, d: P, r: f64) -> bool {
        self.ratio(d, r) <= 1.0
    }
    /// How far `d` is from the centre, as a share of the outline's reach in that direction: at
    /// most 1 inside.
    pub(crate) fn ratio(&self, d: P, r: f64) -> f64 {
        let l2 = d[0] * d[0] + d[1] * d[1];
        if l2 == 0.0 {
            return 0.0;
        }
        let len = l2.sqrt();
        len / (r * self.factor(d, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_is_not_a_disc() {
        let mut rng = Rng::new(3);
        let o = Outline::draw(&mut rng);
        let fs: Vec<f64> = (0..64)
            .map(|k| {
                let t = k as f64 / 64.0 * std::f64::consts::TAU;
                o.factor([t.cos(), t.sin()], 1.0)
            })
            .collect();
        let (lo, hi) = fs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(lo > 0.4 && hi < o.max_factor() + 1e-9);
        assert!(hi - lo > 0.15, "outline too round: {lo}..{hi}");
    }

    #[test]
    fn modes_round_trip() {
        for m in [
            ReseedMode::random(),
            ReseedMode::rain(),
            ReseedMode::blobs(),
        ] {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ReseedMode>(&j).unwrap(), m);
        }
        let m: ReseedMode = serde_json::from_str(r#"{"kind":"rain","drops":5}"#).unwrap();
        assert!(matches!(m, ReseedMode::Rain { drops: 5, .. }));
    }
}
