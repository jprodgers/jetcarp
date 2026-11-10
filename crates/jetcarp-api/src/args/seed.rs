//! Seed shorthands: how a CA's cells start.

use super::json::get_f64;
use jetcarp_core::SeedSpec;
use serde_json::{Value, json};

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

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::{SeedFill, SeedShape, SeedStates};

    #[test]
    fn pattern_seeds() {
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
    fn seed_shorthands() {
        assert_eq!(
            parse_seed(&json!("random:0.2")).unwrap(),
            SeedSpec::Random { density: 0.2 }
        );
        assert_eq!(
            parse_seed(&json!("Soup=20%")).unwrap(),
            SeedSpec::Random { density: 0.2 }
        );
        assert_eq!(
            parse_seed(&json!("top_center")).unwrap(),
            SeedSpec::TopCenter
        );
        assert_eq!(
            parse_seed(&json!("top-centre")).unwrap(),
            SeedSpec::TopCenter
        );
        assert_eq!(
            parse_seed(&json!({"density": 0.4})).unwrap(),
            SeedSpec::Random { density: 0.4 }
        );
        assert_eq!(
            parse_seed(&json!("ants:200000")).unwrap(),
            SeedSpec::Ants { count: 100_000 }
        );
        assert_eq!(
            parse_seed(&json!("lenia:orbium:2")).unwrap(),
            SeedSpec::Lenia {
                name: "orbium".into(),
                scale: 2.0
            }
        );
        assert_eq!(
            parse_seed(&json!("gauss:3:5")).unwrap(),
            SeedSpec::Gaussian {
                count: 3,
                radius: 5.0,
                peak: 1.0
            }
        );
        assert!(parse_seed(&json!("random:250")).is_err());
        assert!(
            parse_seed(&json!("nonsense"))
                .unwrap_err()
                .starts_with("bad seed")
        );
    }
}
