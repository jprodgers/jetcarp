//! Tilings by id or as maps, tiling lists, boundaries, and the scripts' tiling options.

use super::json::{get, get_str, get_u64};
use super::size::Screen;
use jetcarp_core::families::{self, FamilyKind};
use jetcarp_core::topology::fill_size;
use jetcarp_core::{Boundary, SizeSpec, TilingSpec, aperiodic, catalog, pattern};
use serde_json::{Value, json};

/// Tiling id → spec: a family id (`square`, `hex`, `cairo`, `spectre`, `ih41`, ...), a catalog id
/// (`k3-012`, `snubsq`), or a common alias (`hexagon`, `triangles`). Any case.
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

/// A tiling list as searches take it ([`pattern::tiling_list`]: keys separated by commas or
/// spaces, plus sets such as `all-periodic` and `kuniform:2`), with an empty list meaning the
/// square grid.
pub fn tilings_or_square(text: &str) -> Result<Vec<TilingSpec>, String> {
    pattern::tiling_list(if text.trim().is_empty() {
        "square"
    } else {
        text
    })
}

/// Whether the tiling is an aperiodic patch (spectre, hat, Penrose): sized by level, never
/// wrapped.
pub fn is_aperiodic(spec: &TilingSpec) -> bool {
    families::family(&spec.family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

/// What lies past the grid's edge: `dead` (also `none`, `empty`), `wrap` (`torus`, `wrapped`,
/// `periodic`), or `live` (`alive`). Any case.
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
        (Some(s), None) => size_value(s, &spec, screen)?,
    });
    if let Some(SizeSpec::Rect { w, h }) = spec.size
        && (w == 0 || h == 0)
    {
        return Err("size must be at least 1 x 1".into());
    }
    Ok(spec)
}

/// The `size` option of [`tiling_spec`].
fn size_value(s: &Value, spec: &TilingSpec, screen: &Screen) -> Result<SizeSpec, String> {
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
    fn tiling_ids_any_case_and_lists() {
        assert_eq!(tiling_by_id(" HEX ").unwrap().family, "hex");
        let k = tiling_by_id("K2-005").unwrap();
        assert_eq!(k.params, Some(json!({"catalog": "k2-005"})));
        assert!(
            tiling_by_id("nope")
                .unwrap_err()
                .starts_with("unknown tiling 'nope'")
        );
        let sq = tilings_or_square("  ").unwrap();
        assert_eq!(sq.len(), 1);
        assert_eq!(sq[0].family, "square");
        assert_eq!(tilings_or_square("square, hex").unwrap().len(), 2);
    }

    #[test]
    fn boundaries_by_every_name() {
        for (s, b) in [
            ("dead", Boundary::Dead),
            ("None", Boundary::Dead),
            ("empty", Boundary::Dead),
            (" wrap ", Boundary::Wrap),
            ("TORUS", Boundary::Wrap),
            ("wrapped", Boundary::Wrap),
            ("periodic", Boundary::Wrap),
            ("live", Boundary::Live),
            ("alive", Boundary::Live),
        ] {
            assert_eq!(parse_boundary(s).unwrap(), b, "{s}");
        }
        assert_eq!(
            parse_boundary("open").unwrap_err(),
            "boundary must be dead, wrap, or live, got 'open'"
        );
    }
}
