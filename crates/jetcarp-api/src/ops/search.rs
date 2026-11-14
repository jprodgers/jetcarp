//! `search.start`: start (or carry on) a search of any strategy, as a catalog job run by the job
//! runner (`jetcarp_search::jobs`): here until it ends, is stopped, or runs out of budget; on a
//! thread of this process (`background`, kept in the context's
//! [`Registry`](jetcarp_search::jobs::Registry)); or in a process of its own (`detach`, `jetcarp
//! jobs resume JOB`).
//!
//! | Strategy (aliases) | Job kind | What | Spec |
//! | --- | --- | --- | --- |
//! | `hunt` (`soup`) | `hunt` | small soups on an unbounded plane | [`HuntIn`] |
//! | `search` (`bounded`) | `search` | soups on a bounded grid of any tiling | [`BoundedIn`] |
//! | `ants` | `ants` | ant and turmite rules | [`AntsIn`] |
//! | `census` | `census` | rules classed by fate | [`CensusIn`] |
//! | `mover` (`movers`) | `mover` | a mover followed on a sliding window | [`MoverIn`] |
//! | `pile` (`sandpile`) | `pile` | a large sandpile, built by doubling | [`PileIn`] |
//! | `collide` | `collide` | one pattern into another at every offset | [`CollideIn`] |
//! | `evolve` | `evolve` | the evolutionary glider search | [`EvolveIn`] |
//! | `explore` | `explore` | the broad quality-diversity search over tilings, families, rules, seeds (not deterministic: a stopped one continues from its archive) | [`ExploreIn`] |
//!
//! Each spec reads the forms every front end wrote (lists or text, `"WxH"` or `[w, h]`, ...) and
//! makes the strategy's own spec, so a search means the same from the command line, MCP, Rhai,
//! and the app. A run here reports its progress ([`Progress::report`]) and stops at its next
//! checkpoint when the caller cancels; a job that ends in the call gives its result (a bounded
//! search's report, a collision's runs, a census's table, an evolve search's champions, a pile's
//! metadata) and writes the files asked for ([`FilesIn`]). The budget given is this run's own:
//! the job stays resumable (`jobs.resume`) past it.
//!
//! A bounded search or a collision run in the call keeps within the context's limits
//! (`Limits::max_search_*`, `max_collide_*`: the MCP server's are [`Limits::INTERACTIVE`]): its
//! rules, tilings, soups, generations, cell updates, and placements. One in the background or
//! detached has no such bound.
//!
//! [`Limits::INTERACTIVE`]: crate::Limits::INTERACTIVE

use super::jobs::{noun, progress_text, summary_of};
use super::library::catalog_err;
use crate::args;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::census::{self, CensusReport, CensusSpec, RunClass};
use jetcarp_core::evolve::EvolveSpec;
use jetcarp_core::hunt::bits::bs_rule;
use jetcarp_core::hunt::soup::{SoupShape, SoupSpec};
use jetcarp_core::pattern::collide::{CollideOutput, CollideSpec};
use jetcarp_core::pattern::movers::{MoverReport, TrackSpec};
use jetcarp_core::pattern::{self, PatternClass, tiling_list};
use jetcarp_core::pile::{self, Centre, PileSpec};
use jetcarp_core::rules::Rule;
use jetcarp_core::window::track::WindowOpts;
use jetcarp_core::{Boundary, Nb, SizeSpec, TilingSpec, Topology, VERSION, render, sheet};
use jetcarp_search::hunt::{HuntSpec, StorePolicy};
use jetcarp_search::jobs::collide::{CollideJobSpec, CollideRun};
use jetcarp_search::jobs::evolve::{EvolveJobSpec, EvolveResult, EvolveRun};
use jetcarp_search::jobs::explore::{ExploreRun, ExploreSpec};
use jetcarp_search::jobs::pile::PileRunOn;
use jetcarp_search::jobs::search::{SearchJobSpec, SearchResult, SearchRun};
use jetcarp_search::jobs::{
    self, Budget, JobSpec, Outcome, RunOptions, Runner, Stopped, Strategy, status_name,
};
use jetcarp_search::pile::PileJobSpec;
use jetcarp_search::window::{MoverJobSpec, MoverProgress, travelling_start};
use jetcarp_store::library::JobRow;
use jetcarp_store::{JobStatus, Library};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn invalid(e: impl Into<String>) -> ApiError {
    ApiError::invalid(e)
}

// ---------------------------------------------------------------------------------------------
// The forms the specs read

fn texts_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "string"], "items": {"type": "string"}})
}
fn pair_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "integer", "string"], "items": {"type": "integer"}})
}
fn tilings_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "string"]})
}
fn tiling_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["object", "string"]})
}
fn digits_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "string"], "items": {"type": "integer"}})
}
fn cells_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "string"], "items": {"type": "array", "items": {"type": "integer"}}})
}
fn number_or_text_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["integer", "string"]})
}
fn spec_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object"})
}

/// Items given as a list of text, or as one text separated by any of `seps`.
pub(crate) fn texts(v: Option<&Value>, key: &str, seps: &[char]) -> Result<Vec<String>, ApiError> {
    Ok(match v {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split(seps)
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(|s| s.trim().to_string())
                    .ok_or_else(|| invalid(format!("`{key}` must be text")))
            })
            .collect::<Result<_, _>>()?,
        Some(x) => return Err(invalid(format!("`{key}` must be a list of text, got {x}"))),
    })
}

/// B/S and Generations rules: commas, semicolons, and spaces separate them.
const RULE_SEPS: &[char] = &[',', ';', ' ', '\t', '\n'];
/// Ant rules: turmite tables hold commas, so only semicolons and spaces separate them.
const ANT_SEPS: &[char] = &[';', ' ', '\t', '\n'];

/// The rules given (`rules`, then a file's, one per line with `#` comments), each parsed and
/// passed by `check` before the job starts. A rule that cannot run refuses the whole job, named
/// by its place (its item in `rules`, or its line in the file), so a typo in a long list fails
/// at once instead of once per tiling after the soups start (#5).
fn checked_rules(
    cx: &Ctx,
    given: Option<&Value>,
    file: Option<&str>,
    check: fn(&Rule) -> Result<(), String>,
) -> Result<Vec<String>, ApiError> {
    let mut rules: Vec<(String, String)> = texts(given, "rules", RULE_SEPS)?
        .into_iter()
        .enumerate()
        .map(|(i, r)| (format!("rule {} of `rules`", i + 1), r))
        .collect();
    if let Some(f) = file {
        let p = cx.fs.readable(f)?;
        let text = std::fs::read_to_string(&p)
            .map_err(|e| ApiError::io(format!("reading {}: {e}", p.display())))?;
        for (n, line) in text.lines().enumerate() {
            for r in census::parse_rule_list(line) {
                rules.push((format!("line {} of {f}", n + 1), r));
            }
        }
    }
    let bad: Vec<String> = rules
        .iter()
        .filter_map(|(at, r)| {
            Rule::parse(r)
                .and_then(|(rule, _)| check(&rule))
                .err()
                .map(|e| format!("{at}: {r}: {e}"))
        })
        .collect();
    if !bad.is_empty() {
        const SHOWN: usize = 20;
        let mut msg = format!(
            "{} rule{} cannot run, so the job was not started:",
            bad.len(),
            if bad.len() == 1 { "" } else { "s" }
        );
        for b in bad.iter().take(SHOWN) {
            let _ = write!(msg, "\n  {b}");
        }
        if bad.len() > SHOWN {
            let _ = write!(msg, "\n  and {} more", bad.len() - SHOWN);
        }
        return Err(invalid(msg));
    }
    Ok(rules.into_iter().map(|(_, r)| r).collect())
}

/// A pair of whole numbers: `n`, `"AxB"`, `[a, b]`, or a map with the two names.
fn pair(
    v: Option<&Value>,
    key: &str,
    d: (u32, u32),
    names: (&str, &str),
) -> Result<(u32, u32), ApiError> {
    match v {
        None | Some(Value::Null) => Ok(d),
        Some(x) => args::get_pair(&json!({ key: x }), key, d, names).map_err(invalid),
    }
}

/// Tilings: text (ids, catalog ids, vertex configurations, sets such as `archimedean`), a list of
/// them, or a list of tiling maps; default square.
fn tilings(v: Option<&Value>) -> Result<Vec<TilingSpec>, ApiError> {
    match v {
        Some(Value::Array(a)) if a.iter().any(|x| x.is_object()) => {
            a.iter().map(one_tiling).collect()
        }
        _ => {
            let t = texts(v, "tilings", &[',', ';', ' '])?.join(",");
            args::tilings_or_square(&t).map_err(invalid)
        }
    }
}

/// One tiling: text, or a tiling map.
fn one_tiling(v: &Value) -> Result<TilingSpec, ApiError> {
    match v {
        Value::String(s) => tiling_list(s)
            .map_err(invalid)?
            .into_iter()
            .next()
            .ok_or_else(|| invalid(format!("no tiling in {s:?}"))),
        Value::Object(_) => {
            serde_json::from_value(v.clone()).map_err(|e| invalid(format!("tiling: {e}")))
        }
        x => Err(invalid(format!("a tiling is text or a map, not {x}"))),
    }
}

/// `touch` or `edge`, if given.
fn nb_of(v: Option<&str>) -> Result<Option<Nb>, ApiError> {
    match v {
        None => Ok(None),
        Some(x) => args::nb_word(x)
            .map(Some)
            .ok_or_else(|| invalid(format!("`nb` is touch or edge, not {x}"))),
    }
}

/// Whole numbers: a list, or text separated by commas or spaces.
fn numbers(v: Option<&Value>, key: &str) -> Result<Option<Vec<u64>>, ApiError> {
    Ok(match v {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => Some(vec![
            n.as_u64()
                .ok_or_else(|| invalid(format!("`{key}`: whole numbers")))?,
        ]),
        Some(Value::String(s)) => Some(
            s.split([',', ' ', ';'])
                .filter(|x| !x.trim().is_empty())
                .map(|x| x.trim().parse::<u64>())
                .collect::<Result<_, _>>()
                .map_err(|_| invalid(format!("`{key}`: whole numbers, like 1,2,4")))?,
        ),
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|x| x.as_u64())
                .collect::<Option<_>>()
                .ok_or_else(|| invalid(format!("`{key}`: whole numbers")))?,
        ),
        Some(x) => return Err(invalid(format!("`{key}`: whole numbers, not {x}"))),
    })
}

// ---------------------------------------------------------------------------------------------
// Hunts

/// A hunt (strategy `hunt`, also `soup`): small soups on an unbounded plane, bit-parallel, until
/// they settle; every object classed, new patterns verified and stored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HuntIn {
    /// Rules (B/S and Generations without B0; a `V` suffix means edge neighbours): a list, or text separated by spaces, commas, or semicolons (default B3/S23).
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub rules: Option<Value>,
    /// A file of more rules, one per line (`#` comments).
    #[serde(default)]
    pub rules_file: Option<String>,
    /// Tilings: family ids, catalog ids, or sets (all-periodic, archimedean, kuniform:2, isohedral), as a list or text; tiling maps work too (default square).
    #[serde(default)]
    #[schemars(schema_with = "tilings_schema")]
    pub tilings: Option<Value>,
    /// Neighbourhood for every rule: touch or edge (default: the notation decides).
    #[serde(default)]
    pub nb: Option<String>,
    /// Soups per rule and tiling (default 10,000; 0: until stopped).
    #[serde(default)]
    pub soups: Option<u64>,
    /// Soup size in unit cells: "WxH", n, or [w, h] (default 16x16).
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub size: Option<Value>,
    /// Soup density (default 0.5).
    #[serde(default)]
    pub density: Option<f64>,
    /// Soup shape: rect (default) or disc.
    #[serde(default)]
    pub shape: Option<String>,
    /// Soup i uses an RNG state from (seed, i) (default 1).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Generations before a soup that has not settled is censused as it is (default 20,000).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Longest period looked for (default 64).
    #[serde(default)]
    pub max_period: Option<u64>,
    /// Live cells above which a soup counts as exploded (default 0: automatic).
    #[serde(default)]
    pub pop_cap: Option<u64>,
    /// Empty margin around the soup before escaping ships are cut (default 0: automatic).
    #[serde(default)]
    pub margin: Option<u32>,
    /// Storage policy: "all" (every new object as a full record), or {max_cells, max_volume, min_seen} (default 64, 4000, 2: larger still lifes and oscillators are counted, not stored, until seen that often).
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub store: Option<Value>,
    /// Stop, paused and resumable, when the process uses more memory than this many GB (checked before each batch of soups, as an explore search does).
    #[serde(default)]
    pub max_rss_gb: Option<f64>,
}

impl HuntIn {
    fn spec(&self, cx: &Ctx) -> Result<HuntSpec, ApiError> {
        let d = HuntSpec::default();
        let mut rules = checked_rules(cx, self.rules.as_ref(), self.rules_file.as_deref(), |r| {
            bs_rule(r).map(|_| ())
        })?;
        if rules.is_empty() {
            rules = d.rules.clone();
        }
        let (w, h) = pair(self.size.as_ref(), "size", (d.soup.w, d.soup.h), ("w", "h"))
            .map_err(|e| invalid(format!("soup {}", e.message)))?;
        let soups = self.soups.unwrap_or(10_000);
        Ok(HuntSpec {
            rules,
            tilings: tilings(self.tilings.as_ref())?,
            neighborhood: nb_of(self.nb.as_deref())?,
            soups: (soups > 0).then_some(soups),
            soup: SoupSpec {
                w,
                h,
                density: self.density.unwrap_or(d.soup.density),
                shape: match &self.shape {
                    Some(s) => SoupShape::parse(s).map_err(invalid)?,
                    None => SoupShape::Rect,
                },
            },
            seed: self.seed.unwrap_or(d.seed),
            max_gens: self.gens.unwrap_or(d.max_gens),
            max_period: self.max_period.unwrap_or(d.max_period),
            pop_cap: self.pop_cap.unwrap_or(0),
            margin: self.margin.unwrap_or(0),
            store: store_policy(self.store.as_ref())?,
            max_rss_gb: self.max_rss_gb,
        })
    }
}

fn store_policy(v: Option<&Value>) -> Result<StorePolicy, ApiError> {
    let d = StorePolicy::default();
    Ok(match v {
        None | Some(Value::Null) => d,
        Some(Value::String(s)) if s == "all" => StorePolicy::all(),
        Some(m @ Value::Object(_)) => {
            args::check_keys(m, &["max_cells", "max_volume", "min_seen"], "store")
                .map_err(invalid)?;
            let n = |k: &str| args::get_u64(m, k).map_err(invalid);
            StorePolicy {
                max_cells: n("max_cells")?.unwrap_or(d.max_cells),
                max_volume: n("max_volume")?.unwrap_or(d.max_volume),
                min_seen: n("min_seen")?.unwrap_or(d.min_seen),
            }
        }
        Some(x) => return Err(invalid(format!("`store` is \"all\" or a map, not {x}"))),
    })
}

// ---------------------------------------------------------------------------------------------
// Bounded searches

/// A bounded soup search (strategy `search`, also `bounded`): soups on a finite grid of any
/// tiling, aperiodic patches too; every verified still life, oscillator, and spaceship stored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundedIn {
    /// Rules (Life-like and Generations rules without B0): a list, or text separated by spaces, commas, or semicolons (default B3/S23).
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub rules: Option<Value>,
    /// A file of more rules, one per line (`#` comments).
    #[serde(default)]
    pub rules_file: Option<String>,
    /// Tilings: ids, catalog ids, vertex configurations, or sets, as a list or text (default square).
    #[serde(default)]
    #[schemars(schema_with = "tilings_schema")]
    pub tilings: Option<Value>,
    /// Grid size: "WxH" unit cells, about N cells ("4000"), "L<level>" on aperiodic tilings, or [w, h] (default 64x64).
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub size: Option<Value>,
    /// Grid width in unit cells (with `height`, instead of `size`).
    #[serde(default)]
    pub width: Option<u32>,
    /// Grid height in unit cells.
    #[serde(default)]
    pub height: Option<u32>,
    /// wrap (default) or dead.
    #[serde(default)]
    pub boundary: Option<String>,
    /// Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge).
    #[serde(default)]
    pub nb: Option<String>,
    /// Soups per rule and tiling (default 16).
    #[serde(default)]
    pub soups: Option<u32>,
    /// Soup density (default 0.37).
    #[serde(default)]
    pub density: Option<f64>,
    /// Generations per soup (default 2000).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Soup s uses RNG state seed + s (default 1).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Longest period looked for (default 64).
    #[serde(default)]
    pub max_period: Option<u64>,
    /// Soup only a disc at the centre: its radius as a fraction of half the shorter side; with a dead boundary, escaping spaceships are caught at the edge.
    #[serde(default)]
    pub soup_radius: Option<f64>,
    /// Store what is found in the catalog (default true; false only reports it).
    #[serde(default)]
    pub store: Option<bool>,
    /// Stop, paused and resumable, when the process uses more memory than this many GB (checked before each batch of soups, as an explore search does).
    #[serde(default)]
    pub max_rss_gb: Option<f64>,
}

impl BoundedIn {
    fn spec(&self, cx: &Ctx) -> Result<SearchJobSpec, ApiError> {
        let d = jetcarp_search::bounded::SearchSpec::default();
        let mut rules = checked_rules(cx, self.rules.as_ref(), self.rules_file.as_deref(), |r| {
            jetcarp_core::objects::supported(r).map(|_| ())
        })?;
        if rules.is_empty() {
            rules = d.rules.clone();
        }
        let size = match (&self.size, self.width, self.height) {
            (_, Some(w), Some(h)) => SizeSpec::Rect { w, h },
            (None | Some(Value::Null), _, _) => d.size,
            (Some(Value::String(s)), _, _) => args::size_spec(s).map_err(invalid)?,
            (Some(v), _, _) => {
                let (w, h) = pair(Some(v), "size", (64, 64), ("w", "h"))?;
                SizeSpec::Rect { w, h }
            }
        };
        if let SizeSpec::Rect { w, h } = size
            && (w as u64 * h as u64) > cx.limits.max_cells
        {
            return Err(ApiError::limit(format!(
                "{} cells is over this run's limit of {}",
                w as u64 * h as u64,
                cx.limits.max_cells
            )));
        }
        let boundary = match self.boundary.as_deref() {
            None => d.boundary,
            Some(b) => match args::parse_boundary(b) {
                Ok(b @ (Boundary::Wrap | Boundary::Dead)) => b,
                _ => return Err(invalid("`boundary` is wrap or dead")),
            },
        };
        Ok(SearchJobSpec {
            search: jetcarp_search::bounded::SearchSpec {
                rules,
                tilings: tilings(self.tilings.as_ref())?,
                size,
                boundary,
                neighborhood: nb_of(self.nb.as_deref())?,
                soups: self.soups.unwrap_or(d.soups),
                density: self.density.unwrap_or(d.density),
                gens: self.gens.unwrap_or(d.gens),
                seed: self.seed.unwrap_or(d.seed),
                max_period: self.max_period.unwrap_or(d.max_period),
                explode_fraction: d.explode_fraction,
                soup_radius: self.soup_radius,
            },
            store: self.store.unwrap_or(true),
            max_rss_gb: self.max_rss_gb,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Ant searches

/// An ant and turmite search (strategy `ants`): turn strings and turmite tables on unbounded
/// periodic tilings, from an empty field or seeded soups; every run classed (highways proved
/// exactly) and stored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AntsIn {
    /// Periodic tilings: ids, catalog ids, vertex configurations, or sets (default square).
    #[serde(default)]
    #[schemars(schema_with = "tilings_schema")]
    pub tilings: Option<Value>,
    /// Turn string lengths: [min, max], one length, or "2-6".
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub strings: Option<Value>,
    /// Turmite tables: [states, colours] or "2x2".
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub tables: Option<Value>,
    /// Turn letters to use, e.g. "LR" (default: the tiling's).
    #[serde(default)]
    pub alphabet: Option<String>,
    /// Tables: enumerate every table up to this many, else sample `sample` of them (default 1,000,000).
    #[serde(default)]
    pub all_max: Option<u64>,
    /// Tables sampled when there are more than `all_max` (default 10,000).
    #[serde(default)]
    pub sample: Option<u64>,
    /// Explicit rules ("RL", "LLRR", "RL2N", "1R1,1L0/1R1,0N0"): a list, or text separated by spaces or semicolons. With no strings, tables, or rules: strings of 2 to 6.
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub rules: Option<Value>,
    /// Seeded soups per rule (default: an empty field).
    #[serde(default)]
    pub soups: Option<u64>,
    /// Soup size in unit cells: n, "WxH", or [w, h] (default 8x8).
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub size: Option<Value>,
    /// Soup density (default 0.5).
    #[serde(default)]
    pub density: Option<f64>,
    /// Seed of the soups and of sampled tables (default 1).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Steps before a run is classed as it is (default 1,000,000).
    #[serde(default)]
    pub steps: Option<u64>,
    /// Longest highway or orbit period looked for (default 20,000).
    #[serde(default)]
    pub max_period: Option<u64>,
    /// Test for mirror-symmetric growth (default true).
    #[serde(default)]
    pub symmetry: Option<bool>,
}

impl AntsIn {
    fn spec(&self) -> Result<jetcarp_search::ants::AntSpec, ApiError> {
        use jetcarp_core::ants::search::{Strings, Tables};
        use jetcarp_search::ants::{AntSpec, Soups};
        let d = AntSpec::default();
        let strings = match &self.strings {
            None | Some(Value::Null) | Some(Value::Bool(false)) => None,
            Some(Value::String(s)) => Some(args::count_range(s).map_err(invalid)?),
            Some(Value::Number(n)) => {
                let n = n
                    .as_u64()
                    .ok_or_else(|| invalid("`strings` is a length or [min, max]"))?
                    as usize;
                Some((n, n))
            }
            Some(v) => {
                let (a, b) = pair(Some(v), "strings", (2, 6), ("a", "b"))?;
                Some((a as usize, b as usize))
            }
        }
        .map(|(min, max)| Strings {
            min,
            max,
            alphabet: self.alphabet.clone(),
        });
        let tables = match &self.tables {
            None | Some(Value::Null) => None,
            Some(v) => {
                let (n, k) = pair(Some(v), "tables", (2, 2), ("a", "b"))?;
                Some(Tables {
                    states: n as u8,
                    colors: k as u8,
                    alphabet: self.alphabet.clone(),
                    all_max: self.all_max.unwrap_or(1_000_000),
                    sample: self.sample.unwrap_or(10_000),
                    seed: self.seed.unwrap_or(1),
                })
            }
        };
        let rules = texts(self.rules.as_ref(), "rules", ANT_SEPS)?;
        let (strings, tables) = if strings.is_none() && tables.is_none() && rules.is_empty() {
            (d.strings.clone(), None)
        } else {
            (strings, tables)
        };
        let soup = match self.soups {
            None | Some(0) => None,
            Some(n) => {
                let (w, h) = pair(self.size.as_ref(), "size", (8, 8), ("a", "b"))?;
                Some(Soups {
                    w,
                    h,
                    density: self.density.unwrap_or(0.5),
                    soups: n,
                    seed: self.seed.unwrap_or(1),
                })
            }
        };
        Ok(AntSpec {
            tilings: tilings(self.tilings.as_ref())?,
            rules,
            strings,
            tables,
            soup,
            steps: self.steps.unwrap_or(d.steps),
            max_period: self.max_period.unwrap_or(d.max_period),
            symmetry: self.symmetry.unwrap_or(true),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Censuses

/// A rule census (strategy `census`): rules run from seeded soups on one tiling, each run classed
/// (died, frozen, oscillating, active, exploded), a summary per rule.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CensusIn {
    /// The tiling: an id ("hex", "k2-01", "spectre") or a tiling map (default square).
    #[serde(default)]
    #[schemars(schema_with = "tiling_schema")]
    pub tiling: Option<Value>,
    /// Grid size for a tiling given by id: "WxH" unit cells, about N cells, or "L<level>" (default 64x64; aperiodic tilings: their default level).
    #[serde(default)]
    pub size: Option<String>,
    /// Aperiodic patch level (instead of `size`).
    #[serde(default)]
    pub level: Option<u32>,
    /// About this many cells (instead of `size`).
    #[serde(default)]
    pub cells: Option<u32>,
    /// wrap, dead, or live, for a tiling given by id (default: as the id gives it, dead).
    #[serde(default)]
    pub boundary: Option<String>,
    /// Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge).
    #[serde(default)]
    pub nb: Option<String>,
    /// Rules: a list, or text separated by spaces, commas, or semicolons ("B3/S23 B36/S23 B2/S/C3").
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub rules: Option<Value>,
    /// A file of more rules, one per line (`#` comments).
    #[serde(default)]
    pub rules_file: Option<String>,
    /// Every B/S rule over `birth` and `survive` (with `rules` too: added to them).
    #[serde(default)]
    pub all: bool,
    /// B/S rules sampled (seeded, distinct) when there are more than `all_max` (default 2000).
    #[serde(default)]
    pub sample: Option<usize>,
    /// The sample's seed (default: from `seed`).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub sample_seed: Option<u64>,
    /// With no rules: every B/S rule over `birth` and `survive` when there are at most this many, else a sample (default 32,768; 0: always a sample, added to `rules` when there are some).
    #[serde(default)]
    pub all_max: Option<u64>,
    /// Birth digits for B/S rules: "1-8", "1,2,3,6", "1-max" (the tiling's largest degree), or a list (default 1 to max).
    #[serde(default)]
    #[schemars(schema_with = "digits_schema")]
    pub birth: Option<Value>,
    /// Survival digits for B/S rules, as `birth` (default 0 to max).
    #[serde(default)]
    #[schemars(schema_with = "digits_schema")]
    pub survive: Option<Value>,
    /// Generations rules with this many states (B/S rules otherwise).
    #[serde(default)]
    pub states: Option<u8>,
    /// Soups per rule (default 8; soup s uses RNG state seed + s).
    #[serde(default)]
    pub soups: Option<u32>,
    /// Soup density (default 0.37).
    #[serde(default)]
    pub density: Option<f64>,
    /// Generation limit per run (default 2000).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Default 1.
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// A run is called exploded once its population exceeds this fraction of the cells (default 0.5).
    #[serde(default)]
    pub explode: Option<f64>,
    /// A whole census spec (as a census report's `spec` holds it), instead of the fields above.
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub census: Option<Value>,
}

fn digit_list(v: Option<&Value>, key: &str, max: u32, from: u32) -> Result<Vec<u32>, ApiError> {
    let d: Vec<u32> = match v {
        None | Some(Value::Null) => (from..=max).collect(),
        Some(Value::String(s)) => {
            census::parse_digits(&s.replace("max", &max.to_string())).map_err(invalid)?
        }
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_u64().map(|d| d as u32))
            .collect(),
        Some(x) => return Err(invalid(format!("`{key}` must be digits, got {x}"))),
    };
    Ok(d.into_iter().filter(|&x| x >= from && x <= max).collect())
}

impl CensusIn {
    fn spec(&self, cx: &Ctx) -> Result<CensusSpec, ApiError> {
        if let Some(c) = &self.census {
            return serde_json::from_value(c.clone())
                .map_err(|e| invalid(format!("census spec: {e}")));
        }
        let d = CensusSpec::default();
        let mut tiling = match &self.tiling {
            None | Some(Value::Null) => TilingSpec::new("square"),
            Some(Value::String(s)) if !s.contains(',') && self.size.is_some() => {
                jetcarp_core::pattern::tiling_from_key(s).map_err(invalid)?
            }
            Some(v) => one_tiling(v)?,
        };
        if let Some(l) = self.level {
            tiling.size = Some(SizeSpec::Level(l));
        } else if let Some(n) = self.cells {
            tiling.size = Some(SizeSpec::Cells(n));
        } else if let Some(s) = &self.size {
            tiling.size = Some(args::size_spec(s).map_err(invalid)?);
        } else if tiling.size.is_none() && args::is_aperiodic(&tiling) {
            tiling.size = Some(SizeSpec::Level(jetcarp_core::aperiodic::default_level(
                &tiling.family,
            )));
        }
        if let Some(b) = &self.boundary {
            tiling.boundary = args::parse_boundary(b)
                .map_err(|_| invalid("`boundary` is wrap, dead, or live"))?;
        }
        let nb = nb_of(self.nb.as_deref())?;
        let deterministic = |r: &Rule| match r.is_stochastic() {
            true => Err("a census needs deterministic rules".to_string()),
            false => Ok(()),
        };
        let mut rules = checked_rules(
            cx,
            self.rules.as_ref(),
            self.rules_file.as_deref(),
            deterministic,
        )?;
        let seed = self.seed.unwrap_or(d.seed);
        let all_max = self.all_max.unwrap_or(1 << 15);
        if self.all || rules.is_empty() || all_max == 0 {
            let topo = Topology::build(&tiling).map_err(ApiError::engine)?;
            let max = census::max_degree(&topo, nb.unwrap_or(Nb::Touch)) as u32;
            let birth = digit_list(self.birth.as_ref(), "birth", max, 1)?;
            let survive = digit_list(self.survive.as_ref(), "survive", max, 0)?;
            let states = self.states.filter(|&s| s > 2);
            let bits = (birth.len() + survive.len()) as u32;
            let all = self.all || (bits < 63 && (1u64 << bits) <= all_max);
            let mut set = if all {
                census::all_bs_rules(&birth, &survive, states).map_err(invalid)?
            } else {
                census::sample_bs_rules(
                    &birth,
                    &survive,
                    states,
                    self.sample.unwrap_or(2000),
                    self.sample_seed.unwrap_or(seed ^ 0x5eed_ce05),
                )
            };
            if nb == Some(Nb::Edge) {
                // The notation says the neighbourhood.
                for r in &mut set {
                    r.push('V');
                }
            }
            rules.extend(set);
        }
        if rules.is_empty() {
            return Err(invalid(
                "no rules: give rules, a rules file, all, or a sample",
            ));
        }
        Ok(CensusSpec {
            tiling,
            neighborhood: nb,
            rules,
            soups: self.soups.unwrap_or(d.soups),
            density: self.density.unwrap_or(d.density),
            gens: self.gens.unwrap_or(d.gens),
            seed,
            explode_fraction: self.explode.unwrap_or(d.explode_fraction),
            activity_window: d.activity_window,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Movers

/// A mover followed on a sliding window (strategy `mover`, also `movers`): a pattern on an
/// aperiodic tiling runs on a window that generates supertiles around it with exact coordinates,
/// so it can be followed far past the fixed patches; a verified mover is stored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoverIn {
    /// Rule notation (default SpectreGlider; "Priority 4: ...", B/S).
    #[serde(default)]
    pub rule: Option<String>,
    /// Aperiodic tiling: spectre (default), hat, penrose-kd, penrose-rhomb.
    #[serde(default)]
    pub tiling: Option<String>,
    /// Patch level the cells are given on (default 4).
    #[serde(default)]
    pub level: Option<u32>,
    /// Neighbourhood: touch (default) or edge.
    #[serde(default)]
    pub nb: Option<String>,
    /// Starting cells: [[tile, state], ...] or "1203:1,1204:3".
    #[serde(default)]
    #[schemars(schema_with = "cells_schema")]
    pub cells: Option<Value>,
    /// Instead of cells: states on a tile and an edge neighbour (default [1, 3]), tried near the patch's deepest tile (or at `at`) until one travels.
    #[serde(default)]
    #[schemars(schema_with = "digits_schema")]
    pub states: Option<Value>,
    /// The tile for `states`.
    #[serde(default)]
    pub at: Option<u32>,
    /// Stop, verified, at this many edge rings (default: run to `gens`).
    #[serde(default)]
    pub rings: Option<u32>,
    /// Generation limit (default 100 per ring plus 20,000).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Stop as grown above this many live cells (default 2000).
    #[serde(default)]
    pub max_pop: Option<usize>,
    /// World units of tiles kept around the live cells (default 24).
    #[serde(default)]
    pub margin: Option<f64>,
    /// Spectre: follow the fixed patches' Delta chain up to this level (default 8).
    #[serde(default)]
    pub fixed_levels: Option<u32>,
    /// Store a verified mover in the catalog (default false).
    #[serde(default)]
    pub store: Option<bool>,
    /// Who started it, for the record's provenance (default "search.start").
    #[serde(default)]
    pub source: Option<String>,
}

/// Cells as `[[tile, state], ...]` or `"1203:1,1204:3"`.
fn cell_list(v: &Value) -> Result<Vec<(u32, u8)>, ApiError> {
    let bad = || invalid("cells are [[tile, state], ...] or \"TILE:STATE,...\"");
    match v {
        Value::String(s) => s
            .split([',', ' ', ';'])
            .filter(|t| !t.is_empty())
            .map(|t| {
                let (c, x) = t
                    .split_once(':')
                    .ok_or_else(|| invalid(format!("cells are CELL:STATE, got '{t}'")))?;
                Ok((
                    c.trim().parse().map_err(|_| bad())?,
                    x.trim().parse().map_err(|_| bad())?,
                ))
            })
            .collect(),
        Value::Array(a) => a
            .iter()
            .map(|c| {
                let c = c.as_array().filter(|c| c.len() == 2).ok_or_else(bad)?;
                Ok((
                    c[0].as_u64().ok_or_else(bad)? as u32,
                    c[1].as_u64().ok_or_else(bad)? as u8,
                ))
            })
            .collect(),
        _ => Err(bad()),
    }
}

impl MoverIn {
    fn spec(&self) -> Result<MoverJobSpec, ApiError> {
        let family = self.tiling.as_deref().unwrap_or("spectre");
        let level = self.level.unwrap_or(4);
        let patch = TilingSpec::new(family).with_size(SizeSpec::Level(level));
        let rule = self.rule.clone().unwrap_or_else(|| "SpectreGlider".into());
        let nb = nb_of(self.nb.as_deref())?.unwrap_or(Nb::Touch);
        let d = WindowOpts::default();
        let opts = WindowOpts {
            margin: self.margin.unwrap_or(d.margin),
            fixed_levels: self.fixed_levels.unwrap_or(d.fixed_levels),
            target_rings: self.rings,
        };
        let cells = match &self.cells {
            Some(c) if !c.is_null() => cell_list(c)?,
            _ => {
                let states: Vec<u8> = numbers(self.states.as_ref(), "states")?
                    .unwrap_or_else(|| vec![1, 3])
                    .into_iter()
                    .map(|x| x as u8)
                    .collect();
                travelling_start(&rule, nb, &patch, &states, self.at, &opts)
                    .map_err(ApiError::engine)?
            }
        };
        let mut track = TrackSpec::new(&rule, nb, patch, cells);
        track.max_gens = self.gens.unwrap_or(match self.rings {
            Some(r) => r as u64 * 100 + 20_000,
            None => 20_000,
        });
        if let Some(p) = self.max_pop {
            track.max_pop = p.max(1);
        }
        Ok(MoverJobSpec {
            track,
            window: opts,
            store: self.store.unwrap_or(false),
            source: self.source.clone().unwrap_or_else(|| "search.start".into()),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Piles

/// A large sandpile (strategy `pile`, also `sandpile`): N grains on one spot of an unbounded
/// grid, built by doubling, each finished step saved in the pile's folder (and a step stopped part
/// way as a checkpoint, carried on when resumed); then a cropped
/// `pile.png`, a `preview.png`, `growth.csv`, and `pile.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PileIn {
    /// Tiling id: square (default), hex, triangle, snubsq, cairo, k3-012, ih41, spectre, hat, ...
    #[serde(default)]
    pub tiling: Option<String>,
    /// Grains: 1000000, "1e7", "2^30" (up to 2^64 - 1; default 10,000).
    #[serde(default)]
    #[schemars(schema_with = "number_or_text_schema")]
    pub grains: Option<Value>,
    /// edge (default; von Neumann on squares) or touch (Moore).
    #[serde(default)]
    pub nb: Option<String>,
    /// cell (default), vertex, or edge: where the grains go.
    #[serde(default)]
    pub centre: Option<String>,
    /// Aperiodic tilings: patch level (default: the smallest that should hold the pile).
    #[serde(default)]
    pub level: Option<u32>,
    /// The pile's own folder: carried on from where it stopped when it holds a step file (the job whose folder it is, or a new job for it), else the new pile is built there. A finished pile given picture options (palette, reverse, px, margin, preview, png) is exported again in them.
    #[serde(default)]
    pub folder: Option<String>,
    /// The folder a new pile's run folder goes in (default `<output>/piles`).
    #[serde(default)]
    pub out: Option<String>,
    /// The run folder's name (default `<tiling>-<grains>`, after the date).
    #[serde(default)]
    pub name: Option<String>,
    /// Pixels across a cell in pile.png (default 2; below 1.5 each pixel averages the cells in it).
    #[serde(default)]
    pub px: Option<f64>,
    /// Empty cells around the pile in the image (default 4).
    #[serde(default)]
    pub margin: Option<f64>,
    /// Palette for the heights, with its background and stage (default Ember, or the folder's own).
    #[serde(default)]
    pub palette: Option<String>,
    /// Run the palette the other way (height 1 at its last stop).
    #[serde(default)]
    pub reverse: Option<bool>,
    /// Longest side of preview.png (default 1024; 0: none).
    #[serde(default)]
    pub preview: Option<u32>,
    /// Write the full-size pile.png (default true).
    #[serde(default)]
    pub png: Option<bool>,
    /// Topple on the GPU when there is one (periodic tilings).
    #[serde(default)]
    pub gpu: Option<bool>,
}

/// What a spec makes: a new job's spec, or a job that is there already (a pile's folder).
enum Built {
    New(Value),
    Existing(i64),
}

fn grains_of(v: Option<&Value>) -> Result<u64, ApiError> {
    match v {
        None | Some(Value::Null) => Ok(10_000),
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| *f >= 0.0 && f.fract() == 0.0 && *f < 1.8446744073709552e19)
                    .map(|f| f as u64)
            })
            .ok_or_else(|| invalid("`grains` must be a whole number")),
        Some(Value::String(s)) => pile::parse_grains(s).map_err(invalid),
        Some(x) => Err(invalid(format!(
            "`grains`: a number or text like \"1e7\" or \"2^30\", not {x}"
        ))),
    }
}

impl PileIn {
    fn given(&self) -> bool {
        self.tiling.is_some()
            || self.grains.is_some()
            || self.nb.is_some()
            || self.centre.is_some()
            || self.level.is_some()
    }

    /// Any of the picture options given (palette, reverse, px, margin, preview, png).
    fn picture_given(&self) -> bool {
        self.palette.is_some()
            || self.reverse.is_some()
            || self.px.is_some()
            || self.margin.is_some()
            || self.preview.is_some()
            || self.png.is_some()
    }

    /// A finished pile's job made to export again: its picture options replaced by the ones
    /// given, or, with none given, kept but taken again when the folder's `look.json` has other
    /// colours than its `pile.json` (the Sandpile window changed them). Returns whether it will.
    fn reexport(&self, lib: &Library, job: i64, dir: &std::path::Path) -> Result<bool, ApiError> {
        let row = lib.job(job).map_err(catalog_err)?;
        let mut s: PileJobSpec = serde_json::from_value(row.spec.clone())
            .map_err(|e| ApiError::engine(format!("job {job}: {e}")))?;
        let look = pile::export::read_look_file(dir);
        let exported = std::fs::read(dir.join("pile.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .map(|m| {
                (
                    m["image"]["palette"].as_str().unwrap_or("").to_string(),
                    m["image"]["reverse"].as_bool().unwrap_or(false),
                )
            });
        if !self.picture_given() && (look.is_none() || look == exported) {
            return Ok(false);
        }
        if let Some(p) = &self.palette {
            render::builtin_palette(p).map_err(invalid)?;
            s.palette = p.clone();
        } else if let Some((p, _)) = &look {
            s.palette = p.clone();
        }
        s.reverse = match (self.reverse, &look) {
            (Some(r), _) => r,
            (None, Some((_, r))) if self.palette.is_none() => *r,
            _ => false,
        };
        s.px_per_cell = self.px.unwrap_or(s.px_per_cell);
        s.margin = self.margin.unwrap_or(s.margin);
        s.preview_side = self.preview.unwrap_or(s.preview_side);
        s.png = self.png.unwrap_or(s.png);
        pile::export::write_look_file(dir, &s.palette, s.reverse).map_err(ApiError::io)?;
        lib.set_job_spec(job, &to_json(&s)?).map_err(catalog_err)?;
        lib.update_job(job, Some(JobStatus::Paused), None, None)
            .map_err(catalog_err)?;
        Ok(true)
    }

    fn pile(&self) -> Result<(String, PileSpec), ApiError> {
        let id = self.tiling.clone().unwrap_or_else(|| "square".into());
        let mut tiling = args::tiling_by_id(&id).map_err(invalid)?;
        if let Some(l) = self.level {
            tiling.size = Some(SizeSpec::Level(l));
        }
        Ok((
            id,
            PileSpec {
                tiling,
                nb: args::parse_pile_nb(self.nb.as_deref().unwrap_or("edge")).map_err(invalid)?,
                grains: grains_of(self.grains.as_ref())?,
                centre: Centre::parse(self.centre.as_deref().unwrap_or("cell")).map_err(invalid)?,
            },
        ))
    }

    fn job_spec(&self, id: &str, spec: PileSpec, folder: PathBuf) -> Result<PileJobSpec, ApiError> {
        // A folder's own colouring (look.json) unless the input gives one.
        let kept = pile::export::read_look_file(&folder);
        let palette = self
            .palette
            .clone()
            .or_else(|| kept.as_ref().map(|k| k.0.clone()))
            .unwrap_or_else(|| "Ember".into());
        render::builtin_palette(&palette).map_err(invalid)?;
        Ok(PileJobSpec {
            pile: spec,
            px_per_cell: self.px.unwrap_or(2.0),
            margin: self.margin.unwrap_or(4.0),
            palette,
            reverse: self
                .reverse
                .unwrap_or(self.palette.is_none() && kept.is_some_and(|k| k.1)),
            folder,
            preview_side: self.preview.unwrap_or(1024),
            png: self.png.unwrap_or(true),
            gpu: self.gpu.unwrap_or(false),
            extra: json!({
                "tiling_id": id,
                "started": crate::out::now_rfc3339(),
            }),
        })
    }

    fn build(&self, cx: &Ctx, lib: &Library, p: &dyn Progress) -> Result<Built, ApiError> {
        if let Some(f) = &self.folder {
            let dir = cx.fs.writable_dir(f)?;
            if let Some(last) = pile::latest_step_file(&dir) {
                let saved = pile::file::load(&last).map_err(ApiError::io)?;
                if self.given() && self.pile()?.1 != saved.spec {
                    return Err(ApiError::conflict(format!(
                        "{} holds a different pile; use a new folder",
                        dir.display()
                    )));
                }
                let want = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
                for j in lib.jobs().map_err(catalog_err)?.into_iter().rev() {
                    if j.kind != jetcarp_search::pile::KIND {
                        continue;
                    }
                    if let Ok(s) = serde_json::from_value::<PileJobSpec>(j.spec.clone())
                        && std::fs::canonicalize(&s.folder).unwrap_or(s.folder.clone()) == want
                    {
                        // A finished pile with new picture options (or new colours in its
                        // look.json) is exported again in them.
                        if saved.finished() && self.reexport(lib, j.id, &dir)? {
                            p.log(&format!(
                                "exporting {} again in the new picture options",
                                dir.display()
                            ));
                        }
                        return Ok(Built::Existing(j.id));
                    }
                }
                p.log(&format!(
                    "resuming {} from {}",
                    dir.display(),
                    last.display()
                ));
                let id = self
                    .tiling
                    .clone()
                    .unwrap_or_else(|| saved.spec.tiling.family.clone());
                let js = self.job_spec(&id, saved.spec.clone(), dir)?;
                return Ok(Built::New(to_json(&js)?));
            }
            let (id, spec) = self.pile()?;
            let js = self.job_spec(&id, spec, dir)?;
            return Ok(Built::New(to_json(&js)?));
        }
        let (id, spec) = self.pile()?;
        let root = match &self.out {
            Some(o) => cx.fs.writable_dir(o)?,
            None => cx.output_root().join("piles"),
        };
        std::fs::create_dir_all(&root)
            .map_err(|e| ApiError::io(format!("{}: {e}", root.display())))?;
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| format!("{id}-{}", spec.grains));
        let dir = crate::out::make_out_dir(&root, &name).map_err(ApiError::io)?;
        let js = self.job_spec(&id, spec, dir)?;
        Ok(Built::New(to_json(&js)?))
    }
}

// ---------------------------------------------------------------------------------------------
// Collisions

/// A collision (strategy `collide`): pattern B into pattern A at every offset in a range and
/// every phase of B, in a rule both work in; each run classed, new objects verified and stored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollideIn {
    /// Pattern A (stays at the origin): id, unique prefix, or name.
    #[serde(default)]
    pub a: String,
    /// Pattern B (placed at each offset).
    #[serde(default)]
    pub b: String,
    /// B's corner offsets on both axes: n (-n..n) or "lo..hi" (default 6).
    #[serde(default)]
    #[schemars(schema_with = "number_or_text_schema")]
    pub offsets: Option<Value>,
    /// B's x offsets instead: [lo, hi] or "lo..hi".
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub dx: Option<Value>,
    /// B's y offsets instead: [lo, hi] or "lo..hi".
    #[serde(default)]
    #[schemars(schema_with = "pair_schema")]
    pub dy: Option<Value>,
    /// B's phases: "all" (default), a list, or "0,1".
    #[serde(default)]
    #[schemars(schema_with = "digits_schema")]
    pub phases: Option<Value>,
    /// A's orientation (index into the tiling's symmetries).
    #[serde(default)]
    pub orientation_a: Option<u32>,
    /// B's orientation.
    #[serde(default)]
    pub orientation_b: Option<u32>,
    /// Generations per run (default 1024).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Empty cells around the pair (default 40).
    #[serde(default)]
    pub margin: Option<i32>,
    /// The rule to run (both must work in it; default: A's rule if B works in it, else B's).
    #[serde(default)]
    pub rule: Option<String>,
    /// Store the products in the catalog (default true; false only reports them).
    #[serde(default)]
    pub store: Option<bool>,
}

fn range(v: &Value, key: &str) -> Result<[i32; 2], ApiError> {
    match v {
        Value::String(s) => args::offset_range(s).map_err(invalid),
        Value::Number(n) => {
            let n = n
                .as_i64()
                .ok_or_else(|| invalid(format!("`{key}`: a whole number")))?
                as i32;
            Ok([-n.abs(), n.abs()])
        }
        Value::Array(a) if a.len() == 2 => Ok([
            a[0].as_i64()
                .ok_or_else(|| invalid(format!("`{key}` is [lo, hi]")))? as i32,
            a[1].as_i64()
                .ok_or_else(|| invalid(format!("`{key}` is [lo, hi]")))? as i32,
        ]),
        _ => Err(invalid(format!("`{key}` is [lo, hi] or \"lo..hi\""))),
    }
}

impl CollideIn {
    fn spec(&self, lib: &Library) -> Result<CollideJobSpec, ApiError> {
        if self.a.trim().is_empty() || self.b.trim().is_empty() {
            return Err(invalid("give patterns `a` and `b`"));
        }
        let a = lib.get(&self.a).map_err(catalog_err)?;
        let b = lib.get(&self.b).map_err(catalog_err)?;
        let d = CollideSpec::default();
        let both = match &self.offsets {
            None | Some(Value::Null) => d.dx,
            Some(v) => range(v, "offsets")?,
        };
        let phases = match &self.phases {
            Some(Value::String(s)) if s.eq_ignore_ascii_case("all") => vec![],
            v => numbers(v.as_ref(), "phases")?
                .unwrap_or_default()
                .into_iter()
                .map(|x| x as u32)
                .collect(),
        };
        Ok(CollideJobSpec {
            a: a.id,
            b: b.id,
            collide: CollideSpec {
                dx: self
                    .dx
                    .as_ref()
                    .map(|v| range(v, "dx"))
                    .transpose()?
                    .unwrap_or(both),
                dy: self
                    .dy
                    .as_ref()
                    .map(|v| range(v, "dy"))
                    .transpose()?
                    .unwrap_or(both),
                phases,
                orientation_a: self.orientation_a.unwrap_or(0),
                orientation_b: self.orientation_b.unwrap_or(0),
                gens: self.gens.unwrap_or(d.gens),
                margin: self.margin.unwrap_or(d.margin),
                rule: self.rule.clone(),
                ..d
            },
            store: self.store.unwrap_or(true),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Evolutionary searches

/// An evolutionary glider search (strategy `evolve`, after Edmond's method): a population of
/// multi-state priority rules evolves on a tiling towards rules whose seeds travel far; the best
/// are verified and the movers stored, and the run's files written to its folder.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvolveIn {
    /// Tiling: an id: spectre (default), hat, penrose-kd, square, hex, k2-017, ...
    #[serde(default)]
    pub tiling: Option<String>,
    /// touch (default; the paper's vertex adjacency) or edge.
    #[serde(default)]
    pub nb: Option<String>,
    /// States per cell, 2 to 8 (default 4).
    #[serde(default)]
    pub states: Option<u8>,
    /// Population size (default 256).
    #[serde(default)]
    pub pop: Option<usize>,
    /// GA generations (default 800).
    #[serde(default)]
    pub gens: Option<usize>,
    /// RNG seed: the run is a pure function of it (default 1).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Arena radius in edge rings (default 48).
    #[serde(default)]
    pub radius: Option<u32>,
    /// Generations per seed run (default 2500).
    #[serde(default)]
    pub horizon: Option<u64>,
    /// Population cap during the GA (default 64; explosive rules score 0).
    #[serde(default)]
    pub cap: Option<usize>,
    /// Distinct top genomes verified at the end (default 16).
    #[serde(default)]
    pub champions: Option<usize>,
    /// Verification radii as multiples of the radius: a list or "1,2,4" (default 1,2,4).
    #[serde(default)]
    #[schemars(schema_with = "digits_schema")]
    pub verify: Option<Value>,
    /// Stop at the first genome that passes verification.
    #[serde(default)]
    pub stop_on_glider: bool,
    /// The search's own worker threads (its results do not depend on them).
    #[serde(default)]
    pub threads: Option<usize>,
    /// Store the verified movers in the catalog (default true).
    #[serde(default)]
    pub store: Option<bool>,
    /// Generations the mover tracker may run on aperiodic patches (default 20,000).
    #[serde(default)]
    pub track_gens: Option<u64>,
    /// The run's folder (default: a new `evolve-<tiling>` folder in `out`).
    #[serde(default)]
    pub folder: Option<String>,
    /// The folder a new run folder goes in (default: the output root).
    #[serde(default)]
    pub out: Option<String>,
}

impl EvolveIn {
    fn spec(&self, cx: &Ctx) -> Result<EvolveJobSpec, ApiError> {
        let d = EvolveSpec::default();
        let key = self.tiling.clone().unwrap_or_else(|| "spectre".into());
        let factors = match numbers(self.verify.as_ref(), "verify")? {
            Some(v) => v.into_iter().map(|x| x as u32).collect(),
            None => d.verify_factors.clone(),
        };
        let nb = match self.nb.as_deref() {
            None => Nb::Touch,
            Some(o) => args::nb_word(o).ok_or_else(|| {
                invalid(format!(
                    "neighbourhood '{}' is not touch or edge",
                    o.trim().to_ascii_lowercase()
                ))
            })?,
        };
        let spec = EvolveSpec {
            tiling: pattern::tiling_from_key(&key).map_err(invalid)?,
            nb,
            states: self.states.unwrap_or(d.states),
            pop: self.pop.unwrap_or(d.pop),
            gens: self.gens.unwrap_or(d.gens),
            seed: self.seed.unwrap_or(d.seed),
            radius: self.radius.unwrap_or(d.radius),
            horizon: self.horizon.unwrap_or(d.horizon),
            pop_cap: self.cap.unwrap_or(d.pop_cap),
            champions: self.champions.unwrap_or(d.champions),
            verify_factors: factors,
            stop_on_glider: self.stop_on_glider,
            threads: self.threads.filter(|&t| t > 0),
        };
        let folder = match &self.folder {
            Some(f) => cx.fs.writable_dir(f)?,
            None => {
                let root = match &self.out {
                    Some(o) => cx.fs.writable_dir(o)?,
                    None => cx.output_root(),
                };
                crate::out::make_out_dir(&root, &format!("evolve-{}", key.replace(':', "-")))
                    .map_err(ApiError::io)?
            }
        };
        Ok(EvolveJobSpec {
            search: spec,
            store: self.store.unwrap_or(true),
            track_gens: self.track_gens.unwrap_or(20_000),
            folder: Some(folder),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Explore searches

/// The broad exploratory search (strategy `explore`): MAP-Elites over tiling, rule family, rule, and seed. It keeps the best few runs of each niche (family, tiling group, behaviour), breeds from them, confirms the best on bigger grids, and stores every run in its own folder (`searches/<uuid>/` beside the library: Parquet segments of a million runs, and a working database). It runs until stopped or out of budget, and is not deterministic: a stopped search continues from its archive.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExploreIn {
    /// Only these tilings: ids (square, hex, trihex, spectre, ih28, k2-001, ...), a list or text separated by commas or spaces (default: every tiling the search knows, 1,359 today; the progress line counts them).
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub tilings: Option<Value>,
    /// Only these rule families (life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider), a list or text (default: all).
    #[serde(default)]
    #[schemars(schema_with = "texts_schema")]
    pub families: Option<Value>,
    /// About this many cells in a screening run's grid (default 16,000).
    #[serde(default)]
    pub screen_cells: Option<u32>,
    /// Generations of a screening run (default 1024).
    #[serde(default)]
    pub screen_gens: Option<u32>,
    /// Cells of a confirmation run, on a bigger grid (default 64,000).
    #[serde(default)]
    pub confirm_cells: Option<u32>,
    /// Generations of a confirmation run (default 4096).
    #[serde(default)]
    pub confirm_gens: Option<u32>,
    /// Seeds an elite is confirmed with (default 3).
    #[serde(default)]
    pub confirm_seeds: Option<u32>,
    /// Most worlds in a GPU batch (default 1024).
    #[serde(default)]
    pub batch: Option<usize>,
    /// Where the random draws start (default 0).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Run the rules a GPU can run on the GPU when there is one, in this and later runs (default false; without a GPU the CPU runs everything).
    #[serde(default)]
    pub gpu: Option<bool>,
    /// Stop (paused, resumable) when the process uses more memory than this many GB.
    #[serde(default)]
    pub max_rss_gb: Option<f64>,
    /// Score runs with a fitted score model (a file's path, or the model) instead of the built-in score; the job keeps it.
    #[serde(default)]
    #[schemars(with = "Option<Value>")]
    pub score: Option<Value>,
    /// Take parents from another explore job: the JACA rules of its reviewed runs (or its best runs, when it has no reviews), which this search transplants to other tilings and tunes for balance early on.
    #[serde(default)]
    pub parents_from: Option<i64>,
    /// With parents_from: the verdicts whose runs to take, keep, maybe or drop, separated by commas (default keep,maybe).
    #[serde(default)]
    pub parents_reviewed: Option<String>,
}

/// The rule families, for hints.
const FAMILIES: &str = "families: life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider";

impl ExploreIn {
    /// The explore job's spec these settings make.
    pub(crate) fn spec(&self) -> Result<ExploreSpec, ApiError> {
        use jetcarp_search::explore::job::Settings;
        use jetcarp_search::explore::store::fam_named;
        let d = Settings::default();
        let seps = &[',', ' ', ';', '\t', '\n'];
        let tilings = texts(self.tilings.as_ref(), "tilings", seps)?
            .iter()
            .map(|t| jetcarp_core::explore::space::tiling_key(t))
            .collect();
        let families = texts(self.families.as_ref(), "families", seps)?
            .iter()
            .map(|f| {
                fam_named(f).ok_or_else(|| invalid(format!("no rule family '{f}'")).hint(FAMILIES))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ExploreSpec {
            search: Settings {
                screen_cells: self.screen_cells.unwrap_or(d.screen_cells),
                screen_gens: self.screen_gens.unwrap_or(d.screen_gens),
                confirm_cells: self.confirm_cells.unwrap_or(d.confirm_cells),
                confirm_gens: self.confirm_gens.unwrap_or(d.confirm_gens),
                confirm_seeds: self.confirm_seeds.unwrap_or(d.confirm_seeds),
                batch: self.batch.unwrap_or(d.batch),
                seed: self.seed.unwrap_or(d.seed),
                tilings,
                families,
                score: None,
                parents: vec![],
            },
            gpu: self.gpu.unwrap_or(false),
            max_rss_gb: self.max_rss_gb,
            uuid: String::new(),
            seal_rows: None,
            build: None,
        })
    }
}

/// An explore run with the platform's GPU backend when the job or the run asks for one (and a
/// note, logged, when there is none: it runs on the CPU only).
pub(crate) fn explore_run(
    cx: &Ctx,
    row: &JobRow,
    gpu: bool,
    p: &dyn Progress,
) -> Result<ExploreRun, ApiError> {
    let spec: ExploreSpec = serde_json::from_value(row.spec.clone())
        .map_err(|e| ApiError::engine(format!("job {}: {e}", row.id)))?;
    if !(spec.gpu || gpu) || row.status == JobStatus::Done {
        return Ok(ExploreRun::default());
    }
    Ok(match cx.platform.explore_backend() {
        Ok(b) => {
            p.log(&format!("GPU: {}", b.name()));
            ExploreRun::with_backend(b)
        }
        Err(e) => {
            p.log(&format!("no GPU ({e}): running on the CPU only"));
            let mut r = ExploreRun::default();
            r.no_gpu = Some(e);
            r
        }
    })
}

/// Every strategy's spec, for the schema.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum AnySpec {
    Hunt(HuntIn),
    Bounded(BoundedIn),
    Ants(AntsIn),
    Census(CensusIn),
    Mover(MoverIn),
    Pile(PileIn),
    Collide(CollideIn),
    Evolve(EvolveIn),
    Explore(ExploreIn),
}

fn to_json<T: Serialize>(t: &T) -> Result<Value, ApiError> {
    serde_json::to_value(t).map_err(|e| ApiError::engine(e.to_string()))
}

/// The job kind a strategy name (or alias) makes.
pub fn strategy_kind(name: &str) -> Result<&'static str, ApiError> {
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "hunt" | "soup" | "soups" => "hunt",
        "search" | "bounded" => "search",
        "ants" | "ant" => "ants",
        "census" => "census",
        "mover" | "movers" => "mover",
        "pile" | "sandpile" => "pile",
        "collide" | "collision" | "collisions" => "collide",
        "evolve" => "evolve",
        "explore" => "explore",
        x => {
            return Err(invalid(format!("no strategy '{x}'")).hint(
                "strategies: hunt (soup), search (bounded), ants, census, mover, pile, collide, evolve, explore",
            ));
        }
    })
}

fn read_spec<T: serde::de::DeserializeOwned>(kind: &str, v: &Value) -> Result<T, ApiError> {
    let v = if v.is_null() { json!({}) } else { v.clone() };
    serde_json::from_value(v).map_err(|e| invalid(format!("{kind} spec: {e}")))
}

/// The strategy's own spec (JSON) for a new job, or the job its spec names.
fn build(
    cx: &Ctx,
    lib: &Library,
    kind: &str,
    spec: &Value,
    p: &dyn Progress,
) -> Result<Built, ApiError> {
    Ok(Built::New(match kind {
        "hunt" => to_json(&read_spec::<HuntIn>(kind, spec)?.spec(cx)?)?,
        "search" => to_json(&read_spec::<BoundedIn>(kind, spec)?.spec(cx)?)?,
        "ants" => to_json(&read_spec::<AntsIn>(kind, spec)?.spec()?)?,
        "census" => to_json(&read_spec::<CensusIn>(kind, spec)?.spec(cx)?)?,
        "mover" => to_json(&read_spec::<MoverIn>(kind, spec)?.spec()?)?,
        "pile" => return read_spec::<PileIn>(kind, spec)?.build(cx, lib, p),
        "collide" => to_json(&read_spec::<CollideIn>(kind, spec)?.spec(lib)?)?,
        "evolve" => to_json(&read_spec::<EvolveIn>(kind, spec)?.spec(cx)?)?,
        "explore" => {
            let e = read_spec::<ExploreIn>(kind, spec)?;
            let mut s = e.spec()?;
            s.search.score = match &e.score {
                Some(v) => super::explore::load_model(cx, v)?,
                None => None,
            };
            if let Some(j) = e.parents_from {
                s.search.parents =
                    super::explore::parents_from(cx, j, e.parents_reviewed.as_deref(), p)?;
            }
            to_json(&s)?
        }
        k => return Err(invalid(format!("no strategy '{k}'"))),
    }))
}

// ---------------------------------------------------------------------------------------------
// search.start

/// How long one run may go on.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BudgetIn {
    /// Stop at the next checkpoint after this many minutes.
    #[serde(default)]
    pub minutes: Option<f64>,
    /// Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed.
    #[serde(default)]
    pub until: Option<u64>,
    /// Stop at the next checkpoint after this many units in this run: soups (per rule and tiling for hunts), runs, rules, or generations.
    #[serde(default)]
    pub items: Option<u64>,
}

impl BudgetIn {
    pub(crate) fn budget(self) -> Budget {
        Budget {
            minutes: self.minutes,
            until: self.until,
            items: self.items,
        }
    }
}

/// Files a job that ends in the call writes.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesIn {
    /// The full report as JSON (bounded searches, collisions, censuses, movers; hunts and ant searches: their progress).
    #[serde(default)]
    pub json: Option<String>,
    /// A table as CSV: a census's rules, a collision's runs.
    #[serde(default)]
    pub csv: Option<String>,
    /// A labelled contact sheet (PNG): a census's rules (each one's first soup at the end), a collision's runs (each one's end, objects coloured by class).
    #[serde(default)]
    pub sheet: Option<String>,
    /// Thumbnail size in pixels on the sheet (default 160 for a census, 96 for a collision).
    #[serde(default)]
    pub thumb: Option<u32>,
    /// Columns of the sheet (default 8 for a census, 13 for a collision).
    #[serde(default)]
    pub cols: Option<u32>,
}

impl FilesIn {
    fn any(&self) -> bool {
        self.json.is_some() || self.csv.is_some() || self.sheet.is_some()
    }
}

/// `search.start`'s input.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchStartIn {
    /// The strategy: hunt (also soup), search (also bounded), ants, census, mover (also movers), pile, collide, evolve, or explore. With `resume`, it may be left out.
    #[serde(default)]
    pub strategy: Option<String>,
    /// The strategy's settings (HuntIn, BoundedIn, AntsIn, CensusIn, MoverIn, PileIn, CollideIn, EvolveIn, ExploreIn); missing keys take the defaults. Ignored with `resume`.
    #[serde(default)]
    #[schemars(schema_with = "spec_schema")]
    pub spec: Value,
    /// A name to find the job by (scripts, the dashboard).
    #[serde(default)]
    pub label: Option<String>,
    /// With `label`: when a job of this strategy has that label, carry it on (or, when it is done, give it as it is) instead of starting another, so a script of labelled jobs resumes after a reboot.
    #[serde(default)]
    pub reuse_label: bool,
    /// Carry on this job (stopped, paused, or cut off) instead of starting one.
    #[serde(default)]
    pub resume: Option<i64>,
    /// This run's budget; the job stays resumable past it.
    #[serde(default)]
    pub budget: Option<BudgetIn>,
    /// Worker threads (default 0: all cores, and all but two for an explore search; results do not depend on them).
    #[serde(default)]
    pub threads: Option<usize>,
    /// Hunts: run the start of each soup on the GPU (results are identical). Piles: topple on the GPU (periodic tilings). Explore: run the rules a GPU can run on it, when there is one.
    #[serde(default)]
    pub gpu: bool,
    /// Run on a thread of this process and answer at once (jobs.status follows it, jobs.stop stops it).
    #[serde(default)]
    pub background: bool,
    /// Run in a process of its own (`jetcarp jobs resume JOB`) and answer at once.
    #[serde(default)]
    pub detach: bool,
    /// Files to write when the job ends in this call.
    #[serde(default)]
    pub files: FilesIn,
}

/// A job started or carried on.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SearchStarted {
    pub job: i64,
    /// The job's kind (`hunt`, `search`, `ants`, ...).
    pub kind: String,
    /// `Hunt`, `Soup search`, `Ant search`, ...
    pub kind_label: String,
    /// The catalog file.
    pub catalog: String,
    pub label: Option<String>,
    /// A new job (false: one carried on, or a labelled job found).
    pub created: bool,
    /// Where it runs: `here` (it ran in the call), `background`, or `detached`; `none` for a done job given as it is.
    pub mode: String,
    /// `running`, `paused`, `done`, or `failed`.
    pub status: String,
    /// Why a run here stopped before the job's end: `asked`, `budget`, or `lost_lease`.
    pub stopped: Option<String>,
    /// The job's progress (its kind's progress JSON).
    #[schemars(with = "Value")]
    pub progress: Value,
    /// The progress as one line.
    pub line: String,
    /// What a job that ended in the call produced: a bounded search's report, a collision's runs and products, a census's rules, an evolve search's report and movers, a pile's metadata, a mover's tracker report.
    #[schemars(with = "Option<Value>")]
    pub result: Option<Value>,
    /// Files written.
    pub files: Vec<String>,
    /// Seconds this call took.
    pub seconds: f64,
}

impl SearchStarted {
    /// The text, with this many finds per group of a bounded search (the default is 12).
    pub fn text_with_top(&self, top: usize) -> String {
        let mut s = match (self.mode.as_str(), &self.result) {
            ("here" | "none", Some(r)) => {
                result_text(&self.kind, self.job, &self.catalog, r, self, top)
                    .unwrap_or_else(|| progress_text(&self.kind, &self.progress, &self.line))
            }
            ("here" | "none", None) => progress_text(&self.kind, &self.progress, &self.line),
            (m, _) => format!(
                "{} job {} {} ({m}) in {}\n{}\n",
                noun(&self.kind),
                self.job,
                if self.created { "started" } else { "resumed" },
                self.catalog,
                self.line
            ),
        };
        for f in &self.files {
            let _ = writeln!(s, "wrote {f}");
        }
        s
    }
}

impl Report for SearchStarted {
    fn text(&self) -> String {
        self.text_with_top(12)
    }
    fn notes(&self) -> String {
        if self.status == "paused" && self.mode == "here" {
            let note = self.progress.get("note").and_then(|v| v.as_str());
            let why = match (self.stopped.as_deref(), note) {
                (Some("budget"), _) => "at its budget",
                (Some("lost_lease"), _) => "another runner took it over",
                (_, Some(n)) => n,
                _ => "as asked",
            };
            return format!(
                "{} {} paused ({why}); `jetcarp jobs resume {}` {}\n",
                noun(&self.kind),
                self.job,
                self.job,
                super::jobs::resume_words(&self.kind)
            );
        }
        if self.status == "failed" {
            let e = self
                .progress
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return format!("{} {} failed: {e}\n", noun(&self.kind), self.job);
        }
        String::new()
    }
}

/// `search.start`.
pub struct SearchStart;
impl Op for SearchStart {
    const NAME: &'static str = "search.start";
    const SUMMARY: &'static str = "Start or carry on a search of any strategy as a catalog job: here, in the background, or in a process of its own.";
    const READ_ONLY: bool = false;
    type In = SearchStartIn;
    type Out = SearchStarted;
    fn run(cx: &Ctx, a: SearchStartIn, p: &dyn Progress) -> Result<SearchStarted, ApiError> {
        let t0 = Instant::now();
        let lib = cx.library();
        let kind = match (&a.strategy, a.resume) {
            (Some(s), _) => Some(strategy_kind(s)?),
            (None, Some(_)) => None,
            (None, None) => {
                return Err(invalid("give the `strategy`").hint(
                    "strategies: hunt (soup), search (bounded), ants, census, mover, pile, collide, evolve, explore",
                ));
            }
        };
        let label = a.label.clone().filter(|l| !l.trim().is_empty());
        // Which job: the one to carry on, a labelled one, or a new one.
        let (job, created) = if let Some(j) = a.resume {
            let row = lib.job(j).map_err(catalog_err)?;
            if let Some(k) = kind
                && row.kind != k
            {
                return Err(invalid(format!("job {j} is a {} job, not {k}", row.kind)));
            }
            (j, false)
        } else {
            let kind = kind.expect("strategy");
            let found = match (&label, a.reuse_label) {
                (Some(l), true) => lib
                    .jobs_labelled(l)
                    .map_err(catalog_err)?
                    .into_iter()
                    .rev()
                    .find(|j| j.kind == kind)
                    .map(|j| j.id),
                _ => None,
            };
            match found {
                Some(j) => (j, false),
                None => match build(cx, &lib, kind, &a.spec, p)? {
                    Built::Existing(j) => (j, false),
                    Built::New(spec) => {
                        if !a.background && !a.detach {
                            within_limits(cx, &lib, kind, &spec)?;
                        }
                        let js = JobSpec {
                            strategy: kind.into(),
                            spec,
                            label: label.clone(),
                            budget: Budget::default(),
                        };
                        let j = jobs::create(&lib, &js).map_err(ApiError::invalid)?;
                        (j, true)
                    }
                },
            }
        };
        let how = How {
            threads: a.threads.unwrap_or(0),
            gpu: a.gpu,
            budget: a.budget.map(BudgetIn::budget).unwrap_or_default(),
            background: a.background,
            detach: a.detach,
            files: a.files,
        };
        let mut out = run_job(cx, &lib, job, &how, p)?;
        out.created = created;
        out.seconds = t0.elapsed().as_secs_f64();
        Ok(out)
    }
}

/// A bounded search or a collision that runs in the call, within the context's limits
/// (`Limits::max_search_*` and `max_collide_*`): an interactive front end's calls finish in
/// seconds to a minute. Jobs run in the background or in a process of their own have no such
/// bound.
fn within_limits(cx: &Ctx, lib: &Library, kind: &str, spec: &Value) -> Result<(), ApiError> {
    use jetcarp_search::dashboard::digits;
    let l = &cx.limits;
    let over = |m: String| Err(ApiError::limit(m));
    let spec_err = |e: serde_json::Error| ApiError::engine(format!("{kind} spec: {e}"));
    match kind {
        "search" => {
            let s = serde_json::from_value::<SearchJobSpec>(spec.clone())
                .map_err(spec_err)?
                .search;
            for (n, what) in [(s.rules.len(), "rules"), (s.tilings.len(), "tilings")] {
                if n > l.max_search_rules {
                    return over(format!(
                        "{n} {what} is more than {} for one call",
                        l.max_search_rules
                    ));
                }
            }
            let soups = s.soups as u64 * s.rules.len() as u64 * s.tilings.len() as u64;
            if soups > l.max_search_soups {
                return over(format!(
                    "{soups} soups is more than {}; use fewer soups, rules, or tilings",
                    digits(l.max_search_soups)
                ));
            }
            if s.gens > l.max_search_gens {
                return over(format!(
                    "{} generations is more than {} for one call",
                    digits(s.gens),
                    digits(l.max_search_gens)
                ));
            }
            let cells = match s.size {
                SizeSpec::Rect { w, h } => Some(4 * w as u64 * h as u64),
                SizeSpec::Cells(n) => Some(n as u64),
                SizeSpec::Level(_) => None,
            };
            if let Some(c) = cells
                && soups.saturating_mul(c).saturating_mul(s.gens) > l.max_search_work
            {
                return over(
                    "that search is too large for one call; use smaller grids or fewer generations"
                        .into(),
                );
            }
        }
        "collide" => {
            let c: CollideJobSpec = serde_json::from_value(spec.clone()).map_err(spec_err)?;
            let phases = if c.collide.phases.is_empty() {
                lib.get(&c.b).map_err(catalog_err)?.period.max(1)
            } else {
                c.collide.phases.len() as u64
            };
            let span = |r: [i32; 2]| (r[1] as i64 - r[0] as i64 + 1).max(0) as u64;
            let runs = span(c.collide.dx) * span(c.collide.dy) * phases;
            if runs > l.max_collide_runs {
                return over(format!(
                    "{} placements is more than {}: use smaller offsets or fewer phases",
                    digits(runs),
                    digits(l.max_collide_runs)
                ));
            }
            if c.collide.gens > l.max_collide_gens {
                return over(format!(
                    "{} generations is more than {} for one call",
                    digits(c.collide.gens),
                    digits(l.max_collide_gens)
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// How to run a job.
pub(crate) struct How {
    pub threads: usize,
    pub gpu: bool,
    pub budget: Budget,
    pub background: bool,
    pub detach: bool,
    pub files: FilesIn,
}

/// The result slots of a run here.
enum Slot {
    None,
    Search(Arc<Mutex<Option<SearchResult>>>),
    Collide(Arc<Mutex<Option<(CollideOutput, Vec<String>)>>>),
    Evolve(Arc<Mutex<Option<EvolveResult>>>),
}

/// The strategy for a run here, with the hooks that give its result.
fn strategy_here(
    cx: &Ctx,
    row: &JobRow,
    how: &How,
    p: &dyn Progress,
) -> Result<(Box<dyn Strategy>, Slot), ApiError> {
    let k = jobs::kind(&row.kind);
    if !k.runnable(row) {
        return Err(ApiError::conflict(format!(
            "{} jobs cannot be run or resumed here (job {})",
            row.kind, row.id
        )));
    }
    let spec_err = |e: serde_json::Error| ApiError::engine(format!("job {}: {e}", row.id));
    Ok(match row.kind.as_str() {
        "search" => {
            let js: SearchJobSpec = serde_json::from_value(row.spec.clone()).map_err(spec_err)?;
            let slot = Arc::new(Mutex::new(None));
            let mut s = SearchRun::new(js.search);
            s.store = js.store;
            s.result = Some(slot.clone());
            (Box::new(s), Slot::Search(slot))
        }
        "collide" => {
            let js: CollideJobSpec = serde_json::from_value(row.spec.clone()).map_err(spec_err)?;
            let slot = Arc::new(Mutex::new(None));
            let mut s = CollideRun::new(js);
            s.keep = true;
            s.thumb_px = if how.files.sheet.is_some() {
                how.files.thumb.unwrap_or(96)
            } else {
                0
            };
            s.result = Some(slot.clone());
            (Box::new(s), Slot::Collide(slot))
        }
        "evolve" => {
            let slot = Arc::new(Mutex::new(None));
            let mut s = EvolveRun::default();
            s.result = Some(slot.clone());
            (Box::new(s), Slot::Evolve(slot))
        }
        "explore" => (Box::new(explore_run(cx, row, how.gpu, p)?), Slot::None),
        "pile" => {
            let js: PileJobSpec = serde_json::from_value(row.spec.clone()).map_err(spec_err)?;
            let toppler = if (js.gpu || how.gpu) && row.status != JobStatus::Done {
                if pile::is_patch(&js.pile.tiling) {
                    p.log("aperiodic patches topple on the CPU");
                    None
                } else {
                    match pile::gpu_toppler() {
                        Some(t) => {
                            let t = t.map_err(|e| ApiError::engine(format!("GPU: {e}")))?;
                            p.log(&format!("GPU: {}", t.name()));
                            Some(t)
                        }
                        None => {
                            return Err(ApiError::engine(
                                "this program has no GPU toppler".to_string(),
                            ));
                        }
                    }
                }
            } else {
                None
            };
            (Box::new(PileRunOn::new(toppler)), Slot::None)
        }
        _ => {
            let s = k.build(row).map_err(ApiError::engine)?;
            (s, Slot::None)
        }
    })
}

/// The run options from `how` (the GPU soup batcher from the platform when asked for).
fn run_options(
    cx: &Ctx,
    row: &JobRow,
    how: &How,
    p: &dyn Progress,
) -> Result<RunOptions, ApiError> {
    let gpu = if how.gpu && row.kind == "hunt" {
        let g = cx
            .platform
            .soup_batcher()
            .map_err(|e| ApiError::engine(format!("gpu: {e}")))?;
        p.log(&format!("GPU: {}", g.name()));
        Some(g)
    } else {
        None
    };
    Ok(RunOptions {
        threads: how.threads,
        gpu,
        budget: how.budget,
        ..Default::default()
    })
}

/// Run (or resume) a job as `how` says, and report on it.
pub(crate) fn run_job(
    cx: &Ctx,
    lib: &Library,
    job: i64,
    how: &How,
    p: &dyn Progress,
) -> Result<SearchStarted, ApiError> {
    let t0 = Instant::now();
    let row = lib.job(job).map_err(catalog_err)?;
    let k = jobs::kind(&row.kind);
    how.budget.check(k).map_err(invalid)?;
    let catalog = lib.path().display().to_string();
    let mut out = SearchStarted {
        job,
        kind: row.kind.clone(),
        kind_label: k.label().into(),
        catalog: catalog.clone(),
        label: row.label().map(String::from),
        created: false,
        mode: "here".into(),
        status: status_name(row.status).into(),
        stopped: None,
        progress: Value::Null,
        line: String::new(),
        result: None,
        files: vec![],
        seconds: 0.0,
    };
    if row.status == JobStatus::Done {
        // Nothing to run: the job as it is.
        out.mode = "none".into();
        out.progress = k.saved_progress(lib, &row);
        out.line = line_of(k, &row, &out.progress);
        out.result = saved_result(lib, &row);
        return Ok(out);
    }
    // A job whose row already says it stopped is letting go of its thread: give it a moment,
    // so a resume right after a stop isn't refused.
    let letting_go = Instant::now();
    while row.status != JobStatus::Running
        && cx.registry().is_running(lib, job)
        && letting_go.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if cx.registry().is_running(lib, job) {
        return Err(ApiError::conflict(format!(
            "{} {job} is already running here",
            noun(&row.kind)
        )));
    }
    if how.detach {
        let mut args = Vec::new();
        if how.threads > 0 {
            args.extend(["--threads".to_string(), how.threads.to_string()]);
        }
        if how.gpu {
            args.push("--gpu".into());
        }
        if let Some(m) = how.budget.minutes {
            args.extend(["--minutes".to_string(), m.to_string()]);
        }
        if let Some(u) = how.budget.until {
            args.extend(["--until".to_string(), u.to_string()]);
        }
        if let Some(n) = how.budget.items {
            args.extend(["--limit".to_string(), n.to_string()]);
        }
        // Marked running before the new process takes it, so a `jobs.wait` or `jobs.status`
        // straight after sees it running (a process that never starts leaves it to go stale).
        if row.status != JobStatus::Running {
            lib.update_job(job, Some(JobStatus::Running), None, None)
                .map_err(catalog_err)?;
        }
        if let Err(e) = cx.platform.spawn_job(lib.path(), job, &args) {
            if row.status != JobStatus::Running {
                let _ = lib.update_job(job, Some(row.status), None, None);
            }
            return Err(ApiError::engine(e));
        }
        out.mode = "detached".into();
        out.status = "running".into();
        out.progress = k.saved_progress(lib, &row);
        out.line = line_of(k, &row, &out.progress);
        return Ok(out);
    }
    let o = run_options(cx, &row, how, p)?;
    if how.background {
        // A catalog handle of its own, so the run's writes never wait on the caller's reads.
        let own = Library::open(lib.path());
        // An explore run takes the platform's GPU backend with it.
        let s: Option<Box<dyn Strategy + Send>> = match row.kind.as_str() {
            "explore" => Some(Box::new(explore_run(cx, &row, how.gpu, p)?)),
            _ => None,
        };
        cx.registry()
            .start_with(own, job, s, o)
            .map_err(ApiError::conflict)?;
        out.mode = "background".into();
        out.status = "running".into();
        out.progress = cx
            .registry()
            .progress(lib, job)
            .unwrap_or_else(|| k.saved_progress(lib, &row));
        out.line = line_of(k, &row, &out.progress);
        return Ok(out);
    }
    p.log(&format!("{} job {job} in {catalog}", noun(&row.kind)));
    if row.kind == "evolve"
        && let Some(f) = row.spec.get("folder").and_then(|v| v.as_str())
    {
        p.log(&format!("output: {f}"));
    }
    if let Some(cells) = row.spec["track"]["cells"].as_array() {
        let start: Vec<String> = cells.iter().map(|c| format!("{}:{}", c[0], c[1])).collect();
        p.log(&format!("start: {}", start.join(",")));
    }
    let (mut s, slot) = strategy_here(cx, &row, how, p)?;
    let ended = run_here(lib, &row, &mut *s, o, p).map_err(ApiError::engine)?;
    out.status = status_name(ended.status).into();
    out.stopped = ended.stopped.map(|s| {
        match s {
            Stopped::Asked => "asked",
            Stopped::Budget => "budget",
            Stopped::LostLease => "lost_lease",
        }
        .to_string()
    });
    out.progress = ended.progress.clone();
    out.line = line_of(k, &row, &out.progress);
    if ended.status == JobStatus::Done {
        let row = lib.job(job).map_err(catalog_err)?;
        let (result, extra) = finished(lib, &row, slot, &out.progress);
        out.result = result;
        out.files = write_files(
            cx,
            &row,
            &how.files,
            out.result.as_ref(),
            extra,
            &out.progress,
        )?;
    } else if how.files.any() {
        p.log("the files are written when the job ends");
    }
    out.seconds = t0.elapsed().as_secs_f64();
    Ok(out)
}

/// The line of a job a pack's sightings named, with the pack called `name` (its manifest when
/// none is given). Nothing ran here, so its counters say nothing; None for any other job (an
/// imported explore search has a progress of its own, rebuilt from its runs).
pub(crate) fn imported_line(row: &JobRow, name: Option<&str>) -> Option<String> {
    let manifest = row.spec.get("imported_from").and_then(|v| v.as_str())?;
    if row.kind == jobs::explore::KIND_NAME {
        return None;
    }
    Some(format!(
        "{} {}: imported from pack {} (its finds came with the pack; nothing ran here)",
        row.kind,
        row.id,
        name.unwrap_or(manifest)
    ))
}

/// The progress line of a job's progress (its kind's own, else one from its summary).
pub(crate) fn line_of(k: &dyn jobs::StrategyKind, row: &JobRow, progress: &Value) -> String {
    if let Some(l) = imported_line(row, None) {
        return l;
    }
    let l = k.line(progress);
    if !l.is_empty() {
        return l;
    }
    let n = |key: &str| progress.get(key).and_then(|v| v.as_u64());
    match row.kind.as_str() {
        "search" | "collide" => {
            let (unit, done, total) = if row.kind == "search" {
                ("soups", n("soups_done"), n("soups"))
            } else {
                (
                    "runs",
                    n("runs_done").or(n("runs").filter(|_| n("products").is_some())),
                    n("runs"),
                )
            };
            let mut l = format!(
                "{} {}: {}/{} {unit}",
                row.kind,
                row.id,
                done.unwrap_or(0),
                total.unwrap_or(0)
            );
            if let (Some(d), Some(v)) = (n("distinct").or(n("products")), n("verified")) {
                let _ = write!(l, ", {v}/{d} verified");
            }
            if let Some(r) = n("records").or(n("products").filter(|_| n("added").is_some())) {
                let _ = write!(
                    l,
                    ", {r} {}, {} new",
                    if row.kind == "search" {
                        "patterns"
                    } else {
                        "products"
                    },
                    n("added").unwrap_or(0)
                );
            }
            return l;
        }
        _ => {}
    }
    let s = summary_of(row, progress);
    let total = s.total.map(|t| format!("/{t}")).unwrap_or_default();
    let mut l = format!(
        "{} {}: {}{total} {}, {}",
        row.kind, row.id, s.done, s.unit, s.status
    );
    for (k, v) in s.counters.iter().take(4) {
        let _ = write!(l, ", {k} {v}");
    }
    l
}

/// Run a job on this thread, its progress reported and the caller's cancel watched on a helper
/// thread.
fn run_here(
    lib: &Library,
    row: &JobRow,
    s: &mut dyn Strategy,
    mut o: RunOptions,
    p: &dyn Progress,
) -> Result<Outcome, String> {
    let stop = Arc::new(AtomicBool::new(false));
    o.stop = Some(stop.clone());
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let tx = Mutex::new(tx);
    o.observe = Some(Arc::new(move |v: &Value| {
        let _ = tx.lock().unwrap().send(v.clone());
    }));
    let k = jobs::kind(&row.kind);
    let stop2 = stop.clone();
    std::thread::scope(|sc| {
        let watcher = sc.spawn(move || {
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(v) => {
                        let sum = summary_of(row, &v);
                        p.report(sum.done, sum.total, &line_of(k, row, &v));
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if p.cancelled() {
                    stop2.store(true, Ordering::SeqCst);
                }
            }
        });
        let r = Runner::new(lib.clone()).run_with(row.id, s, &o);
        drop(o);
        let _ = watcher.join();
        r
    })
}

/// A done job's result from the catalog and its folder (what does not need the run itself).
fn saved_result(lib: &Library, row: &JobRow) -> Option<Value> {
    match row.kind.as_str() {
        "census" => jetcarp_search::census::job_report(lib, row.id)
            .ok()
            .map(|r| census_value(&r)),
        "pile" => {
            let js: PileJobSpec = serde_json::from_value(row.spec.clone()).ok()?;
            let text = std::fs::read_to_string(js.folder.join("pile.json")).ok()?;
            serde_json::from_str(&text).ok()
        }
        "mover" => row.progress.get("report").cloned(),
        _ => None,
    }
}

/// A census report without the per-run results.
fn census_value(r: &CensusReport) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or_default();
    if let Some(rules) = v.get_mut("rules").and_then(|x| x.as_array_mut()) {
        for x in rules {
            if let Some(o) = x.as_object_mut() {
                o.remove("results");
            }
        }
    }
    if let Some(o) = v.as_object_mut() {
        o.remove("rng_states");
    }
    v
}

/// What the files need beyond the result: a collision's thumbnails, a census's report.
enum Extra {
    None,
    Thumbs(
        Vec<(
            String,
            jetcarp_core::pattern::collide::Outcome,
            render::Image,
        )>,
    ),
    Census(Box<CensusReport>),
}

/// A finished run's result.
fn finished(lib: &Library, row: &JobRow, slot: Slot, progress: &Value) -> (Option<Value>, Extra) {
    match slot {
        Slot::Search(s) => (
            s.lock().unwrap().take().and_then(|r| serde_json::to_value(r).ok()),
            Extra::None,
        ),
        Slot::Collide(s) => match s.lock().unwrap().take() {
            Some((o, added)) => {
                let products: Vec<Value> = o
                    .records
                    .iter()
                    .map(|x| {
                        json!({"id": x.id, "label": x.label(), "runs": x.seen,
                               "new": added.contains(&x.id)})
                    })
                    .collect();
                let store = row.spec.get("store") != Some(&Value::Bool(false));
                (
                    Some(json!({"report": o.report, "products": products, "added": added,
                                "store": store})),
                    Extra::Thumbs(o.thumbs),
                )
            }
            None => (None, Extra::None),
        },
        Slot::Evolve(s) => (
            s.lock().unwrap().take().map(|r| {
                json!({"report": r.report, "found": r.found,
                       "files": r.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>()})
            }),
            Extra::None,
        ),
        Slot::None => match row.kind.as_str() {
            "census" => match jetcarp_search::census::job_report(lib, row.id) {
                Ok(r) => (Some(census_value(&r)), Extra::Census(Box::new(r))),
                Err(_) => (None, Extra::None),
            },
            "mover" => (progress.get("report").cloned(), Extra::None),
            _ => (saved_result(lib, row), Extra::None),
        },
    }
}

fn class_color(c: Option<RunClass>) -> [u8; 4] {
    match c {
        Some(RunClass::Died) => [0x8c, 0x8c, 0x8c, 0xff],
        Some(RunClass::Frozen) => [0x4d, 0x9d, 0xff, 0xff],
        Some(RunClass::Oscillating) => [0x3d, 0xdc, 0x6d, 0xff],
        Some(RunClass::Active) => [0xff, 0xb0, 0x2e, 0xff],
        Some(RunClass::Exploded) => [0xff, 0x4d, 0x4d, 0xff],
        None => [0xff, 0x4d, 0xc4, 0xff],
    }
}

fn write_text(cx: &Ctx, path: &str, text: &str) -> Result<PathBuf, ApiError> {
    let p = cx.fs.writable(path)?;
    std::fs::write(&p, text).map_err(|e| ApiError::io(format!("writing {}: {e}", p.display())))?;
    Ok(p)
}

/// Write the files asked for, once the job is done.
fn write_files(
    cx: &Ctx,
    row: &JobRow,
    f: &FilesIn,
    result: Option<&Value>,
    extra: Extra,
    progress: &Value,
) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap_or_default();
    if let Some(j) = &f.json {
        let text = match (row.kind.as_str(), result, &extra) {
            ("census", _, Extra::Census(r)) => r.to_json(),
            ("search" | "collide", Some(r), _) => pretty(&r["report"]),
            ("mover", Some(r), _) => pretty(r) + "\n",
            (_, Some(r), _) => pretty(r),
            (_, None, _) => pretty(progress),
        };
        out.push(write_text(cx, j, &text)?);
    }
    if let Some(c) = &f.csv {
        let text = match (row.kind.as_str(), result, &extra) {
            ("census", _, Extra::Census(r)) => r.to_csv(),
            ("collide", Some(r), _) => {
                let mut s = String::from("dx,dy,phase,outcome,gens,result,products\n");
                for x in r["report"]["runs"].as_array().into_iter().flatten() {
                    let _ = writeln!(
                        s,
                        "{},{},{},{},{},\"{}\",{}",
                        x["dx"],
                        x["dy"],
                        x["phase"],
                        x["outcome"].as_str().unwrap_or(""),
                        x["gens"],
                        x["result"].as_str().unwrap_or("").replace('"', "'"),
                        x["products"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
                s
            }
            (k, _, _) => return Err(invalid(format!("{k} jobs have no CSV table"))),
        };
        out.push(write_text(cx, c, &text)?);
    }
    if let Some(sh) = &f.sheet {
        let path = cx.fs.writable(sh)?;
        match (row.kind.as_str(), extra) {
            ("census", Extra::Census(r)) => {
                let thumbs = census::thumbnails(&r.spec, f.thumb.unwrap_or(160))
                    .map_err(ApiError::engine)?;
                let items: Vec<(sheet::Label, &render::Image)> = r
                    .rules
                    .iter()
                    .zip(&thumbs)
                    .filter_map(|(s, t)| {
                        let t = t.as_ref()?;
                        let label = vec![
                            (s.notation.clone(), [0xd0, 0xd4, 0xdc, 0xff]),
                            (CensusReport::label(s), class_color(s.majority)),
                        ];
                        Some((label, t))
                    })
                    .collect();
                let img = sheet::labelled_sheet(&items, f.cols.unwrap_or(8));
                let meta = vec![
                    ("jetcarp:version", VERSION.to_string()),
                    (
                        "jetcarp:census",
                        serde_json::to_string(&r.spec).unwrap_or_default(),
                    ),
                    ("jetcarp:dead_boundary", r.dead_boundary.to_string()),
                ];
                render::write_png(&path, &img, &meta).map_err(ApiError::io)?;
            }
            ("collide", Extra::Thumbs(thumbs)) => {
                let items: Vec<(sheet::Label, &render::Image)> = thumbs
                    .iter()
                    .map(|(l, o, img)| {
                        (
                            vec![
                                (l.clone(), [0xd0, 0xd4, 0xdc, 0xff]),
                                (o.name().to_string(), o.color()),
                            ],
                            img,
                        )
                    })
                    .collect();
                let img = sheet::labelled_sheet(&items, f.cols.unwrap_or(13));
                let meta = vec![
                    ("jetcarp:version", VERSION.to_string()),
                    (
                        "jetcarp:collide",
                        serde_json::to_string(&row.spec).unwrap_or_default(),
                    ),
                ];
                render::write_png(&path, &img, &meta).map_err(ApiError::io)?;
            }
            (k, _) => return Err(invalid(format!("{k} jobs have no contact sheet"))),
        }
        out.push(path);
    }
    Ok(out.into_iter().map(|p| p.display().to_string()).collect())
}

// ---------------------------------------------------------------------------------------------
// Text

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

/// A finished job's report from its result, as the command line prints it.
fn result_text(
    kind: &str,
    job: i64,
    catalog: &str,
    r: &Value,
    out: &SearchStarted,
    top: usize,
) -> Option<String> {
    let mut s = String::new();
    match kind {
        "search" => {
            let res: SearchResult = serde_json::from_value(r.clone()).ok()?;
            let rep = &res.report;
            let spec = &rep.spec;
            let _ = writeln!(
                s,
                "search: {} rule(s) x {} tiling(s) x {} soups, {:?} {:?}, density {}, up to {} generations, seeds {}..{}",
                spec.rules.len(),
                spec.tilings.len(),
                spec.soups,
                spec.size,
                spec.boundary,
                spec.density,
                spec.gens,
                spec.seed,
                spec.seed + spec.soups as u64 - 1
            );
            let mut ships = Vec::new();
            for g in &rep.groups {
                let _ = writeln!(s, "\n== {} on {}", g.rule, g.tiling);
                if let Some(e) = &g.error {
                    let _ = writeln!(s, "   error: {e}");
                    continue;
                }
                let _ = writeln!(
                    s,
                    "   soups: {} settled, {} still active, {} exploded; objects: {}",
                    g.settled,
                    g.active,
                    g.exploded,
                    g.objects
                        .iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let (mut sl, mut osc, mut sh) = (0, 0, 0);
                for f in &g.found {
                    match f.class {
                        PatternClass::StillLife => sl += 1,
                        PatternClass::Oscillator => osc += 1,
                        PatternClass::Spaceship => sh += 1,
                        _ => {}
                    }
                }
                let _ = writeln!(
                    s,
                    "   distinct: {sl} still lifes, {osc} oscillators, {sh} spaceships"
                );
                for f in g.found.iter().take(top) {
                    let _ = writeln!(
                        s,
                        "   {:>6} x  {:<36} {:<11} p{:<3} {:>3}c  {}{}{}",
                        f.count,
                        f.label,
                        f.class.name(),
                        f.period,
                        f.cells,
                        &f.id[..8.min(f.id.len())],
                        if f.new { "  NEW" } else { "" },
                        f.error
                            .as_deref()
                            .map(|e| format!("  UNVERIFIED: {e}"))
                            .unwrap_or_default()
                    );
                }
                if g.found.len() > top {
                    let _ = writeln!(s, "   ... {} more", g.found.len() - top);
                }
                for f in g
                    .found
                    .iter()
                    .filter(|f| f.class == PatternClass::Spaceship)
                {
                    ships.push(format!(
                        "{} on {}: {} ({}, {} cells, seen {}){}",
                        g.rule,
                        g.tiling,
                        f.id,
                        f.speed.as_deref().unwrap_or("?"),
                        f.cells,
                        f.count,
                        if f.new { ", new" } else { "" }
                    ));
                }
            }
            let _ = writeln!(
                s,
                "\nspaceships: {}",
                if ships.is_empty() {
                    "none".into()
                } else {
                    ships.len().to_string()
                }
            );
            for x in &ships {
                let _ = writeln!(s, "  {x}");
            }
            if res.stored {
                let _ = writeln!(
                    s,
                    "catalog: {} new pattern(s), {} already known ({} of them now verified in another rule); job {job} in {catalog}",
                    res.added.len(),
                    res.merged.len(),
                    res.new_rules.len(),
                );
            } else {
                let _ = writeln!(s, "not stored (job {job} in {catalog})");
            }
            let _ = writeln!(s, "time: {:.1}s", out.seconds);
        }
        "collide" => {
            let rep: jetcarp_core::pattern::collide::CollideReport =
                serde_json::from_value(r["report"].clone()).ok()?;
            let _ = writeln!(
                s,
                "collide: A = {} ({}), B = {} ({})",
                rep.a_label, rep.a, rep.b_label, rep.b
            );
            let _ = writeln!(
                s,
                "rule {} on {}; B offsets dx {:?} dy {:?}, phases {:?}",
                rep.rule,
                rep.tiling,
                rep.spec.dx,
                rep.spec.dy,
                if rep.spec.phases.is_empty() {
                    "all".to_string()
                } else {
                    format!("{:?}", rep.spec.phases)
                }
            );
            let _ = writeln!(
                s,
                "outcomes: {}",
                rep.counts
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let products = r["products"].as_array().cloned().unwrap_or_default();
            let store = r["store"] != false;
            let added = r["added"].as_array().map(|a| a.len()).unwrap_or(0);
            let _ = writeln!(
                s,
                "products: {} distinct verified object(s){}",
                products.len(),
                if store {
                    format!(", {added} new to the library")
                } else {
                    String::new()
                }
            );
            for x in &products {
                let _ = writeln!(
                    s,
                    "  {}  {} (from {} run(s)){}",
                    x["id"].as_str().unwrap_or(""),
                    x["label"].as_str().unwrap_or(""),
                    x["runs"],
                    if x["new"] == true { "  NEW" } else { "" }
                );
            }
            let _ = writeln!(s, "job {job} in {catalog}; time: {:.1}s", out.seconds);
        }
        "census" => {
            let rep: CensusReport = serde_json::from_value(r.clone()).ok().or_else(|| {
                let mut v = r.clone();
                if let Some(rules) = v.get_mut("rules").and_then(|x| x.as_array_mut()) {
                    for x in rules {
                        x["results"] = json!([]);
                    }
                }
                v["rng_states"] = json!([]);
                serde_json::from_value(v).ok()
            })?;
            census_text(&mut s, &rep);
            let _ = writeln!(
                s,
                "job:      {job} in {catalog}\ntime:     {:.2}s this session ({:.0}s over every session)",
                out.seconds,
                out.progress["elapsed_s"].as_f64().unwrap_or(0.0)
            );
        }
        "evolve" => {
            let rep: jetcarp_core::evolve::EvolveReport =
                serde_json::from_value(r["report"].clone()).ok()?;
            let _ = writeln!(
                s,
                "evolve on {} ({} tiles, margin {}, boundary ring {}): {} generations, {:.1}s",
                rep.spec.tiling.family,
                rep.arena_tiles,
                rep.margin,
                rep.boundary,
                rep.generations.len(),
                rep.elapsed_s
            );
            if let Some(g) = rep.generations.last() {
                let _ = writeln!(
                    s,
                    "best ever {:.2}, {} genome evaluations ({} distinct)",
                    g.best_ever, g.evaluations, g.unique_evaluations
                );
            }
            for c in &rep.champions {
                let v = c.verify.as_ref();
                let _ = writeln!(
                    s,
                    "  {:>6.2}  gen {:>4}  {}  [{}]",
                    c.fitness,
                    c.first_generation,
                    c.rule,
                    v.map(|v| if v.passed {
                        "GLIDER".to_string()
                    } else {
                        v.reason.clone()
                    })
                    .unwrap_or_default()
                );
            }
            for f in r["found"].as_array().into_iter().flatten() {
                let _ = writeln!(s, "  mover: {}", f["summary"].as_str().unwrap_or(""));
            }
            for f in r["files"].as_array().into_iter().flatten() {
                let _ = writeln!(s, "wrote {}", f.as_str().unwrap_or(""));
            }
        }
        "pile" => {
            let p: jetcarp_search::pile::PileProgress =
                serde_json::from_value(out.progress.clone()).ok()?;
            let _ = writeln!(
                s,
                "done: {} grains, {} topplings, {} sweeps, {} of toppling",
                p.grains,
                p.topplings,
                p.sweeps,
                pile::fmt_seconds(p.seconds)
            );
            if let Some(png) = &p.png {
                let _ = writeln!(s, "image:   {png}");
            }
            if let Some(pv) = &p.preview {
                let _ = writeln!(s, "preview: {pv}");
            }
            let _ = writeln!(s, "folder:  {}", p.folder);
            let _ = writeln!(s, "job:     {job}");
        }
        "mover" => {
            let p: MoverProgress = serde_json::from_value(out.progress.clone()).ok()?;
            let _rep: MoverReport = serde_json::from_value(r.clone()).ok()?;
            let _ = writeln!(s, "{}", p.line());
            if let Some(id) = &p.record {
                let _ = writeln!(s, "library: {id}");
            }
            if let Some(e) = &p.error {
                let _ = writeln!(s, "error: {e}");
            }
        }
        _ => return None,
    }
    Some(s)
}

/// A census's table, as `jetcarp search census` prints it.
pub(crate) fn census_text(s: &mut String, r: &CensusReport) {
    let sp = &r.spec;
    let _ = writeln!(s, "tiling:   {}", r.tiling);
    let _ = writeln!(
        s,
        "runs:     {} rules x {} soups, density {}, up to {} generations, seeds {}..{}",
        r.rules.len(),
        sp.soups,
        sp.density,
        sp.gens,
        sp.seed,
        sp.seed + sp.soups as u64 - 1,
    );
    let mut totals = [0usize; 6];
    for x in &r.rules {
        let k = match x.majority {
            Some(RunClass::Died) => 0,
            Some(RunClass::Frozen) => 1,
            Some(RunClass::Oscillating) => 2,
            Some(RunClass::Active) => 3,
            Some(RunClass::Exploded) => 4,
            None => 5,
        };
        totals[k] += 1;
    }
    let _ = writeln!(
        s,
        "majority: died {}, frozen {}, oscillating {}, active {}, exploded {}, errors {}",
        totals[0], totals[1], totals[2], totals[3], totals[4], totals[5]
    );
    let _ = writeln!(
        s,
        "\n{:<18} {:>5} {:>6} {:>5} {:>5} {:>6} {:>8} {:>8} {:>7} {:>8}  name",
        "rule", "died", "frozen", "osc", "act", "expl", "med.per", "settle", "peak", "activity"
    );
    let show = 60;
    for x in r.rules.iter().take(show) {
        if let Some(e) = &x.error {
            let _ = writeln!(s, "{:<18} error: {e}", x.rule);
            continue;
        }
        let o = |v: Option<f64>| v.map(|v| format!("{v}")).unwrap_or_else(|| "-".into());
        let _ = writeln!(
            s,
            "{:<18} {:>5} {:>6} {:>5} {:>5} {:>6} {:>8} {:>8} {:>7} {:>8}  {}",
            x.notation,
            pct(x.died),
            pct(x.frozen),
            pct(x.oscillating),
            pct(x.active),
            pct(x.exploded),
            o(x.median_period),
            o(x.median_settle),
            x.peak_population,
            x.mean_activity
                .map(|a| format!("{a:.4}"))
                .unwrap_or_else(|| "-".into()),
            x.name.as_deref().unwrap_or("")
        );
    }
    if r.rules.len() > show {
        let _ = writeln!(
            s,
            "... {} more rules (see the CSV or JSON file)",
            r.rules.len() - show
        );
    }
}
