//! Settings shared by scripts, sweeps, and hosts, parsed from JSON values (a Rhai map is
//! converted to JSON first), so the app's sweep form and the scripts accept the same keys.

use jetcarp_core::families::{self, FamilyKind};
use jetcarp_core::script::Trigger;
use jetcarp_core::topology::fill_size;
use jetcarp_core::world::{AgentEdge, CaSpec};
use jetcarp_core::*;
use serde_json::{Value, json};

/// The screen a script fills: `size: "screen"` worlds and default image sizes.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    /// Tile size in pixels (square root of the mean tile area), as the app's *Tile size*.
    pub cell_px: f64,
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            width: 1920,
            height: 1080,
            cell_px: 8.0,
        }
    }
}

impl Screen {
    /// `"1920x1080"`.
    pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
        let (w, h) = s
            .trim()
            .split_once(['x', 'X', '*', ','])
            .ok_or_else(|| format!("screen size '{s}' should look like 1920x1080"))?;
        let w: u32 = w
            .trim()
            .parse()
            .map_err(|_| format!("bad screen width '{w}'"))?;
        let h: u32 = h
            .trim()
            .parse()
            .map_err(|_| format!("bad screen height '{h}'"))?;
        if !(16..=65536).contains(&w) || !(16..=65536).contains(&h) {
            return Err(format!("screen size {w}x{h} out of range (16 to 65536)"));
        }
        Ok((w, h))
    }
}

pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

pub fn get_str<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(x) => Err(format!("`{key}` should be text, got {x}")),
    }
}

pub fn get_u64(v: &Value, key: &str) -> Result<Option<u64>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(x) => x
            .as_u64()
            .or_else(|| {
                x.as_f64()
                    .filter(|f| *f >= 0.0 && f.fract() == 0.0)
                    .map(|f| f as u64)
            })
            .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` should be a whole number ≥ 0, got {x}")),
    }
}

pub fn get_f64(v: &Value, key: &str) -> Result<Option<f64>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(x) => x
            .as_f64()
            .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` should be a number, got {x}")),
    }
}

pub fn get_bool(v: &Value, key: &str) -> Result<Option<bool>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(x) => Err(format!("`{key}` should be true or false, got {x}")),
    }
}

/// Reject keys that are not in `allowed`, naming the closest one.
pub fn check_keys(v: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(o) = v.as_object() else {
        return if v.is_null() {
            Ok(())
        } else {
            Err(format!(
                "{what} options should be a map like #{{ key: value }}, got {v}"
            ))
        };
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!(
                "unknown {what} option `{k}`; options are: {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Tilings

/// Tiling id → spec: a family id (`square`, `hex`, `cairo`, `spectre`, `ih41`, ...), a catalog id
/// (`k3-012`, `snubsq`), or a common alias (`hexagon`, `triangles`).
pub fn tiling_by_id(id: &str) -> Result<TilingSpec, String> {
    let low = id.trim().to_ascii_lowercase();
    if families::family(&low).is_some() {
        return Ok(TilingSpec::new(&low));
    }
    if catalog::get(&low).is_some() {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(json!({ "catalog": low }));
        return Ok(s);
    }
    let alias = match families::alias(&low) {
        Some(a) => a,
        None => {
            let ids: Vec<&str> = families::FAMILIES.iter().map(|f| f.id).collect();
            return Err(format!(
                "unknown tiling '{id}'. Use a family ({}), an isohedral type ih01..ih93, or a catalog id like k2-005 (see tilings())",
                ids.join(", ")
            ));
        }
    };
    Ok(TilingSpec::new(alias))
}

pub fn is_aperiodic(spec: &TilingSpec) -> bool {
    families::family(&spec.family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

pub fn parse_boundary(s: &str) -> Result<Boundary, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "dead" | "none" | "empty" => Ok(Boundary::Dead),
        "wrap" | "torus" | "wrapped" | "periodic" => Ok(Boundary::Wrap),
        "live" | "alive" => Ok(Boundary::Live),
        _ => Err(format!("boundary must be dead, wrap, or live, got '{s}'")),
    }
}

/// Keys accepted by [`tiling_spec`].
pub const TILING_KEYS: &[&str] = &["tiling", "size", "boundary", "level", "params", "edges"];

/// A tiling from `tiling`, `size`, `boundary`, `level`, `params`, `edges`. `size` is
/// `"screen"` (default: fill the screen), a cell count, `[w, h]`, `"WxH"`, `#{w, h}`,
/// `#{cells}`, or `#{level}` (aperiodic tilings, also `level: n`).
pub fn tiling_spec(v: &Value, screen: &Screen) -> Result<TilingSpec, String> {
    let mut spec = match get(v, "tiling") {
        None => TilingSpec::new("square"),
        Some(Value::String(s)) => tiling_by_id(s)?,
        Some(t @ Value::Object(_)) => {
            let mut t = t.clone();
            if t.get("family").is_none() {
                return Err("a tiling map needs `family`".into());
            }
            if let Some(id) = t.get("family").and_then(|f| f.as_str()) {
                // Accept catalog ids and aliases as the family too.
                let base = tiling_by_id(id)?;
                t["family"] = json!(base.family);
                if t.get("params").is_none()
                    && let Some(p) = base.params
                {
                    t["params"] = p;
                }
            }
            serde_json::from_value(t).map_err(|e| format!("bad tiling map: {e}"))?
        }
        Some(x) => {
            return Err(format!(
                "`tiling` should be an id like \"hex\" or a map, got {x}"
            ));
        }
    };
    if let Some(p) = get(v, "params") {
        let mut merged = spec.params.take().unwrap_or(json!({}));
        for (k, x) in p.as_object().ok_or("`params` should be a map")? {
            merged[k] = x.clone();
        }
        spec.params = Some(merged);
    }
    if let Some(e) = get(v, "edges") {
        spec.edges =
            Some(serde_json::from_value(e.clone()).map_err(|e| format!("bad edges: {e}"))?);
    }
    if let Some(b) = get_str(v, "boundary")? {
        spec.boundary = parse_boundary(b)?;
    }
    let aperiodic = is_aperiodic(&spec);
    if aperiodic {
        spec.boundary = Boundary::Dead;
    }
    let level = get_u64(v, "level")?;
    spec.size = Some(match (get(v, "size"), level) {
        (_, Some(l)) if aperiodic => SizeSpec::Level(l as u32),
        (_, Some(_)) => {
            return Err("`level` is only for aperiodic tilings (spectre, hat, penrose)".into());
        }
        (None, None) if aperiodic => SizeSpec::Level(aperiodic::default_level(&spec.family)),
        (None, None) => fill_size(
            &spec,
            screen.width as f64,
            screen.height as f64,
            screen.cell_px,
        ),
        (Some(s), None) => parse_size(s, &spec, screen)?,
    });
    if let Some(SizeSpec::Rect { w, h }) = spec.size
        && (w == 0 || h == 0)
    {
        return Err("size must be at least 1 x 1".into());
    }
    Ok(spec)
}

fn parse_size(s: &Value, spec: &TilingSpec, screen: &Screen) -> Result<SizeSpec, String> {
    let aperiodic = is_aperiodic(spec);
    Ok(match s {
        Value::String(t) if t.eq_ignore_ascii_case("screen") || t.eq_ignore_ascii_case("fill") => {
            if aperiodic {
                SizeSpec::Level(aperiodic::default_level(&spec.family))
            } else {
                fill_size(
                    spec,
                    screen.width as f64,
                    screen.height as f64,
                    screen.cell_px,
                )
            }
        }
        Value::String(t) => {
            let (w, h) = t
                .split_once(['x', 'X'])
                .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)))
                .ok_or_else(|| format!("size '{t}' should be \"screen\", \"WxH\", or a number"))?;
            SizeSpec::Rect { w, h }
        }
        Value::Number(n) => {
            let c = n
                .as_u64()
                .ok_or("size should be a positive number of cells")?;
            if aperiodic {
                SizeSpec::Level(c as u32)
            } else {
                SizeSpec::Cells(c.min(u32::MAX as u64) as u32)
            }
        }
        Value::Array(a) if a.len() == 2 => {
            let w = a[0].as_u64().ok_or("size [w, h] needs whole numbers")?;
            let h = a[1].as_u64().ok_or("size [w, h] needs whole numbers")?;
            SizeSpec::Rect {
                w: w as u32,
                h: h as u32,
            }
        }
        Value::Object(_) => {
            if let (Some(w), Some(h)) = (get_u64(s, "w")?, get_u64(s, "h")?) {
                SizeSpec::Rect {
                    w: w as u32,
                    h: h as u32,
                }
            } else if let (Some(w), Some(h)) = (get_u64(s, "width")?, get_u64(s, "height")?) {
                SizeSpec::Rect {
                    w: w as u32,
                    h: h as u32,
                }
            } else if let Some(c) = get_u64(s, "cells")? {
                SizeSpec::Cells(c as u32)
            } else if let Some(l) = get_u64(s, "level")? {
                SizeSpec::Level(l as u32)
            } else {
                return Err(format!(
                    "size map should have w and h, cells, or level; got {s}"
                ));
            }
        }
        x => return Err(format!("bad size {x}")),
    })
}

// ---------------------------------------------------------------------------------------------
// CAs and seeds

/// Seed shorthands: `"empty"`, `"random"` (30%), `"random:0.2"`, `"center"`, `"top_center"`,
/// `"top_random:0.5"`, `"random_states"`, `"ants:3"`, `"pile:10000"`, `"blob:0.3:0.5"`
/// (radius, density), `"glider_pair"`, `"wire_demo"`; maps `#{random: 0.2}`,
/// `#{density: 0.2}`, `#{cells: [[i, s], ...]}`, or the scene format's seed objects. Pattern
/// seeds: `"pattern:blobs"`, `"pattern:blobs:0.4:by_region"` (shape, density, states), or
/// `#{shape: "blobs", count: 3, density: 0.4, states: "by_region"}`.
pub fn parse_seed(v: &Value) -> Result<SeedSpec, String> {
    let bad = |e: String| {
        format!(
            "bad seed {v}: {e}. Use \"empty\", \"random\", \"random:0.2\", \"center\", \"top_center\", \"top_random:0.5\", \"random_states\", \"ants:3\", \"pile:10000\", \"blob:0.3:0.5\", \"gauss:3:8:1\", \"lenia:orbium\", \"signed:0.3:0.5\" (JACA: density, share of -1), \"halves:0.3\" (JACA: +1 left, -1 right), \"pattern:blobs:0.4:by_region\", #{{shape: \"blobs\", count: 3, density: 0.4}}, or #{{random: 0.2}}"
        )
    };
    if let Some(pattern) = flat_pattern(v.get("pattern").unwrap_or(v)) {
        return pattern.map_err(bad);
    }
    if let Some(s) = v.as_str() {
        let s = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        if let Some(rest) = s.strip_prefix("pattern:") {
            return pattern_text(rest).map_err(bad);
        }
        // Lenia patterns by name: "lenia:orbium", "lenia:orbium:2" (scale).
        if let Some(rest) = s.strip_prefix("lenia:") {
            let (name, scale) = rest.split_once(':').unwrap_or((rest, "1"));
            let scale = scale.parse::<f64>().map_err(|e| bad(e.to_string()))?;
            return Ok(SeedSpec::Lenia {
                name: name.to_string(),
                scale,
            });
        }
        let mut parts = s.split([':', '=']);
        let head = parts.next().unwrap_or("").trim().to_string();
        let args: Vec<f64> = parts
            .map(|a| {
                a.trim()
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .map_err(|e| bad(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let arg = |k: usize, d: f64| args.get(k).copied().unwrap_or(d);
        let density = |d: f64| -> Result<f64, String> {
            let d = if d > 1.0 { d / 100.0 } else { d };
            if (0.0..=1.0).contains(&d) {
                Ok(d)
            } else {
                Err(bad("density must be between 0 and 1".into()))
            }
        };
        return match head.as_str() {
            "random" | "soup" => Ok(SeedSpec::Random {
                density: density(arg(0, 0.3))?,
            }),
            "top_random" => Ok(SeedSpec::TopRandom {
                density: density(arg(0, 0.5))?,
            }),
            "blob" => Ok(SeedSpec::Blob {
                radius: arg(0, 0.3),
                density: density(arg(1, 0.5))?,
            }),
            "ants" => Ok(SeedSpec::Ants {
                count: arg(0, 1.0).clamp(1.0, 100_000.0) as u32,
            }),
            "pile" => Ok(SeedSpec::Pile {
                grains: arg(0, 10_000.0).clamp(1.0, 1e12) as u64,
            }),
            // Gaussian bumps (continuous fields): "gauss:count:radius_cells:peak".
            "gauss" | "gaussian" => Ok(SeedSpec::Gaussian {
                count: arg(0, 1.0).clamp(1.0, 10_000.0) as u32,
                radius: arg(1, 8.0).max(0.1),
                peak: arg(2, 1.0).clamp(0.0, 1.0),
            }),
            "signed" | "mixed" => Ok(SeedSpec::Signed {
                density: density(arg(0, 0.3))?,
                negative: density(arg(1, 0.5))?,
            }),
            "halves" | "half" => Ok(SeedSpec::Halves {
                density: density(arg(0, 0.3))?,
            }),
            "centre" => Ok(SeedSpec::Center),
            "top" | "top_centre" => Ok(SeedSpec::TopCenter),
            _ => serde_json::from_value(Value::String(head)).map_err(|e| bad(e.to_string())),
        };
    }
    if let Some(d) = get_f64(v, "random")?.or(get_f64(v, "density")?)
        && v.as_object().is_some_and(|o| o.len() == 1)
    {
        return parse_seed(&json!(format!("random:{d}")));
    }
    serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))
}

/// `shape[:density][:states]`, as in `blobs:0.4:by_region` or `voronoi:signed:0.7`. The shape
/// takes its default parameters.
fn pattern_text(text: &str) -> Result<SeedSpec, String> {
    let (shape, rest) = text.split_once(':').unwrap_or((text, ""));
    let mut fill = serde_json::Map::new();
    let mut rest = rest;
    if !rest.is_empty() {
        let (first, more) = rest.split_once(':').unwrap_or((rest, ""));
        if let Ok(d) = first.trim_end_matches('%').parse::<f64>() {
            fill.insert("density".into(), json!(percent(d)));
            rest = more;
        }
    }
    if !rest.is_empty() {
        fill.insert("states".into(), states_text(rest)?);
    }
    serde_json::from_value(json!({"pattern": {"shape": shape, "fill": fill}}))
        .map_err(|e| e.to_string())
}

/// `signed`, `signed:0.7` (share of -1), `range:1:5`, or a plain name (`uniform`, `by_region`,
/// `one`) as the scene format's states.
fn states_text(text: &str) -> Result<Value, String> {
    let mut parts = text.split(':');
    let head = parts.next().unwrap_or("");
    let num = |p: Option<&str>, what: &str| -> Result<Option<f64>, String> {
        p.map(|x| x.parse::<f64>().map_err(|e| format!("{what}: {e}")))
            .transpose()
    };
    Ok(match head {
        "signed" => {
            json!({"signed": {"negative": num(parts.next(), "share of -1")?.unwrap_or(0.5)}})
        }
        "range" => {
            let lo = num(parts.next(), "range")?.ok_or("a range needs range:lo:hi")?;
            let hi = num(parts.next(), "range")?.unwrap_or(lo);
            json!({"range": {"lo": lo as u32, "hi": hi as u32}})
        }
        _ => json!(head),
    })
}

/// A density given as a percentage (above 1) or a share.
fn percent(d: f64) -> f64 {
    if d > 1.0 { d / 100.0 } else { d }
}

/// Pattern seeds with the shape's parameters beside it: `{shape: "blobs", count: 3, density: 0.4,
/// states: "by_region"}`. `None` when `v` is not one (the scene format's nested form included).
fn flat_pattern(v: &Value) -> Option<Result<SeedSpec, String>> {
    let o = v.as_object()?;
    let shape = o.get("shape")?;
    if !shape.is_string() && o.keys().all(|k| k == "shape" || k == "fill") {
        return None;
    }
    Some((|| {
        let mut body = match shape {
            Value::String(name) => {
                let name = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
                json!({"kind": name})
            }
            other => other.clone(),
        };
        let mut fill = o.get("fill").cloned().unwrap_or_else(|| json!({}));
        for (k, val) in o {
            match k.as_str() {
                "shape" | "fill" => {}
                "density" => {
                    let d = val.as_f64().ok_or("density must be a number")?;
                    fill["density"] = json!(percent(d));
                }
                "states" => {
                    fill["states"] = match val {
                        Value::String(t) => states_text(&t.trim().to_ascii_lowercase())?,
                        other => other.clone(),
                    };
                }
                _ => body[k] = val.clone(),
            }
        }
        serde_json::from_value(json!({"pattern": {"shape": body, "fill": fill}}))
            .map_err(|e| e.to_string())
    })())
}

pub fn parse_nb(s: &str) -> Result<Nb, String> {
    match s
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "touch" | "touching" | "moore" | "corner" => Ok(Nb::Touch),
        "edge" | "von_neumann" | "vonneumann" => Ok(Nb::Edge),
        _ => Err(format!("neighborhood must be touch or edge, got '{s}'")),
    }
}

/// Keys accepted by [`ca_spec`].
pub const CA_KEYS: &[&str] = &[
    "seed",
    "neighborhood",
    "palette",
    "steps_per_gen",
    "name",
    "affects_others",
    "enabled",
    "drop_per_gen",
    "color_by",
    "agent_edge",
    "palette_neg",
    "glow",
];

/// A CA spec from a rule and options (`seed`, `neighborhood`, `palette`, `steps_per_gen`,
/// `name`, `affects_others`, `enabled`, `drop_per_gen`, `color_by`, `agent_edge`, and for JACA
/// `palette_neg` and `glow`).
pub fn ca_spec(rule: &str, v: &Value) -> Result<CaSpec, String> {
    let mut spec = CaSpec::new(&resolve_rule(rule));
    apply_ca_options(&mut spec, v)?;
    Ok(spec)
}

/// Set the options present in `v` on an existing CA spec (others are left as they are).
pub fn apply_ca_options(spec: &mut CaSpec, v: &Value) -> Result<(), String> {
    if let Some(s) = get(v, "seed") {
        spec.seed = Some(parse_seed(s)?);
    }
    if let Some(n) = get_str(v, "neighborhood")? {
        spec.neighborhood = Some(parse_nb(n)?);
    }
    if let Some(p) = get_str(v, "palette")? {
        spec.palette = Some(p.to_string());
    }
    if let Some(n) = get_u64(v, "steps_per_gen")? {
        spec.steps_per_gen = n.clamp(1, 100_000) as u32;
    }
    if let Some(n) = get_str(v, "name")? {
        spec.name = Some(n.to_string());
    }
    if let Some(b) = get_bool(v, "affects_others")? {
        spec.affects_others = Some(b);
    }
    if let Some(b) = get_bool(v, "enabled")? {
        spec.enabled = b;
    }
    if let Some(n) = get_u64(v, "drop_per_gen")? {
        spec.drop_per_gen = n.min(u32::MAX as u64) as u32;
    }
    if let Some(c) = get(v, "color_by") {
        spec.color_by =
            serde_json::from_value(c.clone()).map_err(|e| format!("bad color_by: {e}"))?;
    }
    if let Some(p) = get_str(v, "palette_neg")? {
        spec.palette_neg = Some(p.to_string());
    }
    if let Some(b) = get_bool(v, "glow")? {
        spec.glow = b;
    }
    if let Some(e) = get_str(v, "agent_edge")? {
        spec.agent_edge = match e.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "bounce" => AgentEdge::Bounce,
            "fall_off" | "falloff" | "fall" => AgentEdge::FallOff,
            _ => return Err(format!("agent_edge must be bounce or fall_off, got '{e}'")),
        };
    }
    Ok(())
}

/// A rule name from the catalog (any case, ignoring spaces and punctuation) or notation as is.
pub fn resolve_rule(text: &str) -> String {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let t = text.trim();
    if rules::Rule::parse(t).is_ok() {
        return t.to_string();
    }
    let key = squash(t);
    rules::catalog()
        .iter()
        .find(|e| squash(e.name) == key)
        .map(|e| e.notation.to_string())
        .unwrap_or_else(|| t.to_string())
}

// ---------------------------------------------------------------------------------------------
// Run limits

/// When a run stops: a trigger (as in scene scripts, plus `done`) and a generation limit.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Until {
    pub trigger: Trigger,
    pub max_gens: u64,
}

/// Keys accepted by [`parse_until`].
pub const UNTIL_KEYS: &[&str] = &[
    "until",
    "max_gens",
    "gens",
    "n",
    "fraction",
    "for_gens",
    "max_period",
];

/// `until` (`"done"`, `"repeats"`, `"empty"`, `"generations"`, `"population_below"`,
/// `"population_above"`, `"activity_below"`, `"activity_above"`, `"never"`, or a scene-format
/// trigger map), with `n`, `fraction`, `for_gens`, `max_period`, and `max_gens` (or `gens`).
/// Without `until`, runs `gens`/`max_gens` generations.
pub fn parse_until(v: &Value, default_max: u64) -> Result<Until, String> {
    let max_gens = get_u64(v, "max_gens")?.or(get_u64(v, "gens")?);
    let n = get_u64(v, "n")?;
    let trigger = match get(v, "until") {
        None => Trigger::Generations {
            n: max_gens.or(n).unwrap_or(default_max),
        },
        Some(t @ Value::Object(_)) => {
            serde_json::from_value(t.clone()).map_err(|e| format!("bad until map: {e}"))?
        }
        Some(Value::String(s)) => {
            let fraction = get_f64(v, "fraction")?;
            let for_gens = get_u64(v, "for_gens")?.unwrap_or(1);
            let need_n = |what: &str| n.ok_or(format!("until: \"{what}\" needs `n`"));
            match s
                .trim()
                .to_ascii_lowercase()
                .replace([' ', '-'], "_")
                .as_str()
            {
                "done" | "finished" | "bottom" => Trigger::Done,
                "repeats" | "repeat" | "settled" | "periodic" => Trigger::Repeats {
                    max_period: get_u64(v, "max_period")?.unwrap_or(1000),
                },
                "frozen" => Trigger::Repeats { max_period: 1 },
                "empty" | "dead" | "died" => Trigger::PopulationBelow { n: 0 },
                "generations" | "gens" => Trigger::Generations {
                    n: n.or(max_gens).unwrap_or(default_max),
                },
                "population_below" => Trigger::PopulationBelow {
                    n: need_n("population_below")?,
                },
                "population_above" => Trigger::PopulationAbove {
                    n: need_n("population_above")?,
                },
                "activity_below" => Trigger::ActivityBelow {
                    fraction: fraction.ok_or("until: \"activity_below\" needs `fraction`")?,
                    for_gens,
                },
                "activity_above" => Trigger::ActivityAbove {
                    fraction: fraction.ok_or("until: \"activity_above\" needs `fraction`")?,
                    for_gens,
                },
                "never" => Trigger::Never,
                other => {
                    return Err(format!(
                        "unknown until \"{other}\"; use done, repeats, frozen, empty, generations, population_below, population_above, activity_below, activity_above, or never"
                    ));
                }
            }
        }
        Some(x) => return Err(format!("`until` should be text like \"done\", got {x}")),
    };
    let max_gens = match (&trigger, max_gens) {
        (_, Some(m)) => m,
        (Trigger::Generations { n }, None) => *n,
        _ => default_max,
    };
    Ok(Until { trigger, max_gens })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilings_and_sizes() {
        let scr = Screen::default();
        let t = tiling_spec(&json!({"tiling": "hex", "boundary": "dead"}), &scr).unwrap();
        assert_eq!(t.family, "hex");
        assert!(matches!(t.size, Some(SizeSpec::Rect { .. })));
        let t = tiling_spec(&json!({"tiling": "square", "size": [30, 20]}), &scr).unwrap();
        assert_eq!(t.size, Some(SizeSpec::Rect { w: 30, h: 20 }));
        let t = tiling_spec(&json!({"tiling": "k2-005"}), &scr).unwrap();
        assert_eq!(t.family, "periodic");
        let t = tiling_spec(&json!({"tiling": "spectre"}), &scr).unwrap();
        assert!(matches!(t.size, Some(SizeSpec::Level(_))));
        let t = tiling_spec(&json!({"tiling": "ih41", "size": 900}), &scr).unwrap();
        assert_eq!(t.size, Some(SizeSpec::Cells(900)));
        assert!(tiling_spec(&json!({"tiling": "nope"}), &scr).is_err());
    }

    #[test]
    fn pattern_seeds() {
        use jetcarp_core::{SeedFill, SeedShape, SeedStates};
        let s = parse_seed(&json!("pattern:blobs")).unwrap();
        assert!(matches!(
            s,
            SeedSpec::Pattern {
                shape: SeedShape::Blobs { .. },
                ..
            }
        ));
        let s = parse_seed(&json!("pattern:blobs:40%:by_region")).unwrap();
        assert!(matches!(
            s,
            SeedSpec::Pattern {
                fill: SeedFill {
                    density,
                    states: SeedStates::ByRegion
                },
                ..
            } if density == 0.4
        ));
        let s = parse_seed(&json!("pattern:radial-gradient:signed:0.7")).unwrap();
        assert!(matches!(
            s,
            SeedSpec::Pattern {
                shape: SeedShape::RadialGradient { .. },
                fill: SeedFill {
                    states: SeedStates::Signed { negative },
                    ..
                },
            } if negative == 0.7
        ));
        let s = parse_seed(&json!("pattern:all:0.5:range:2:4")).unwrap();
        assert!(matches!(
            s,
            SeedSpec::Pattern {
                fill: SeedFill {
                    states: SeedStates::Range { lo: 2, hi: 4 },
                    ..
                },
                ..
            }
        ));
        // Flat maps, and the scene format's nested form.
        let s = parse_seed(
            &json!({"shape": "blobs", "count": 5, "density": 0.4, "states": "by_region"}),
        )
        .unwrap();
        assert!(matches!(
            s,
            SeedSpec::Pattern {
                shape: SeedShape::Blobs { count: 5, .. },
                fill: SeedFill {
                    density,
                    states: SeedStates::ByRegion
                },
            } if density == 0.4
        ));
        let nested = SeedSpec::Pattern {
            shape: SeedShape::Disc { radius: 0.2 },
            fill: SeedFill::density(0.3),
        };
        let v = serde_json::to_value(&nested).unwrap();
        assert_eq!(parse_seed(&v).unwrap(), nested);
        assert!(parse_seed(&json!("pattern:nonsense")).is_err());
    }

    #[test]
    fn seeds_and_until() {
        assert_eq!(
            parse_seed(&json!("random:0.2")).unwrap(),
            SeedSpec::Random { density: 0.2 }
        );
        assert_eq!(
            parse_seed(&json!("top_center")).unwrap(),
            SeedSpec::TopCenter
        );
        assert_eq!(
            parse_seed(&json!({"density": 0.4})).unwrap(),
            SeedSpec::Random { density: 0.4 }
        );
        let u = parse_until(&json!({"until": "done", "max_gens": 5000}), 1000).unwrap();
        assert_eq!(
            u,
            Until {
                trigger: Trigger::Done,
                max_gens: 5000
            }
        );
        let u = parse_until(&json!({"gens": 50}), 1000).unwrap();
        assert_eq!(u.trigger, Trigger::Generations { n: 50 });
        assert_eq!(u.max_gens, 50);
        assert_eq!(resolve_rule("brian's brain"), "B2/S/C3");
    }
}
