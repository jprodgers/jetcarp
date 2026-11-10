//! The MCP tools' readings of rule names, seeds, and CA items, where they give different
//! results from the shared parsers. Kept as the tools have always read them, so tool results do
//! not change; settling on one reading is part of moving the tools onto operations.
//!
//! How they differ:
//!
//! - [`resolve_rule`] also knows "Game of Life", "Conway", and "Conway's Life" as Life, looks
//!   the text up in the catalog even when it is also notation JETCARP reads (a name the engine
//!   knows, such as `wireworld`, becomes the catalog's notation for it), and returns text it does
//!   not know unchanged (not trimmed). [`super::resolve_rule`] keeps readable notation as
//!   written.
//! - [`parse_seed`] splits a shorthand at its first `:`, `=`, or space (`"random 0.2"` works,
//!   `"random:0.2:5"` does not), caps `ants:N` at 10,000 (not 100,000), reads `lenia:NAME`
//!   without a scale and `lenia` alone as Orbium, reads `gauss:N` as a count only (radius 8,
//!   peak 1), takes `{"random": d}` beside other keys, and does not know `soup`, `blob`,
//!   `top_random`, `centre`, or `-` for `_`. JACA seeds, pattern seeds, and the scene format
//!   go to [`super::parse_seed`].
//! - [`parse_ca`] reads a CA object as the scene format's CA spec (any of its fields), where
//!   [`super::ca_spec`] takes the scripts' option names.

use super::rule::{catalog_notation, squash};
use jetcarp_core::SeedSpec;
use jetcarp_core::world::CaSpec;
use serde_json::{Value, json};

/// Names for catalog rules that only the MCP tools take: (squashed nickname, catalog name).
const NICKNAMES: &[(&str, &str)] = &[
    ("gameoflife", "Life"),
    ("conway", "Life"),
    ("conwayslife", "Life"),
];

/// A catalog rule name ("Brian's Brain", "life", "Conway") becomes its notation; anything else
/// is kept as given.
pub fn resolve_rule(text: &str) -> String {
    let k = squash(text);
    let k = NICKNAMES
        .iter()
        .find(|a| a.0 == k)
        .map(|a| squash(a.1))
        .unwrap_or(k);
    catalog_notation(&k)
        .map(String::from)
        .unwrap_or_else(|| text.to_string())
}

/// Seed shorthands: "random", "random:0.2", "ants:3", "pile:5000", {"random": 0.2}.
pub fn parse_seed(v: &Value) -> Result<SeedSpec, String> {
    let bad = |e: String| {
        format!(
            "bad seed {v}: {e}. Use \"empty\", \"random\", \"random:0.2\", \"center\", \"top_center\", \"random_states\", \"ants:3\", \"pile:10000\", JACA \"signed:0.3:0.5\" (density, share of -1) or \"halves:0.3\" (+1 left, -1 right), \"pattern:blobs:0.4:by_region\", {{\"shape\": \"blobs\", \"count\": 3, \"density\": 0.4}}, or {{\"random\": {{\"density\": 0.2}}}}."
        )
    };
    if let Some(s) = v.as_str() {
        let s = s.trim().to_ascii_lowercase();
        let (head, arg) = match s.split_once([':', '=', ' ']) {
            Some((h, a)) => (h.trim().to_string(), Some(a.trim().to_string())),
            None => (s.clone(), None),
        };
        let num = |d: f64| -> Result<f64, String> {
            arg.as_deref()
                .map(|a| {
                    a.trim_end_matches('%')
                        .parse::<f64>()
                        .map_err(|e| e.to_string())
                })
                .transpose()
                .map(|x| x.unwrap_or(d))
        };
        return match head.as_str() {
            "random" => {
                let mut d = num(0.3).map_err(bad)?;
                if d > 1.0 {
                    d /= 100.0;
                }
                if !(0.0..=1.0).contains(&d) {
                    return Err(bad("density must be between 0 and 1".into()));
                }
                Ok(SeedSpec::Random { density: d })
            }
            "ants" => Ok(SeedSpec::Ants {
                count: num(1.0).map_err(bad)?.clamp(1.0, 10_000.0) as u32,
            }),
            "pile" => Ok(SeedSpec::Pile {
                grains: num(10_000.0).map_err(bad)?.clamp(1.0, 1e12) as u64,
            }),
            // Lenia: a built-in creature at the centre ("lenia:orbium"), or Gaussian bumps.
            "lenia" => Ok(SeedSpec::Lenia {
                name: arg.unwrap_or_else(|| "orbium".into()),
                scale: 1.0,
            }),
            "gauss" | "gaussian" => Ok(SeedSpec::Gaussian {
                count: num(1.0).map_err(bad)?.clamp(1.0, 10_000.0) as u32,
                radius: 8.0,
                peak: 1.0,
            }),
            "signed" | "mixed" | "halves" | "half" | "pattern" => super::parse_seed(v),
            _ => serde_json::from_value(Value::String(head)).map_err(|e| bad(e.to_string())),
        };
    }
    if v.get("shape").is_some() || v.get("pattern").is_some() {
        return super::parse_seed(v);
    }
    if let Some(d) = v.get("random").and_then(|d| d.as_f64()) {
        return parse_seed(&json!(format!("random:{d}")));
    }
    serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))
}

/// A CA item from `new_world` or `add_ca`: a rule string or a CaSpec-like object.
pub fn parse_ca(v: &Value) -> Result<CaSpec, String> {
    if let Some(s) = v.as_str() {
        return Ok(CaSpec::new(&resolve_rule(s)));
    }
    let mut o = v
        .as_object()
        .cloned()
        .ok_or_else(|| format!("a CA must be a rule string or an object, got {v}"))?;
    let rule = o
        .get("rule")
        .and_then(|r| r.as_str())
        .ok_or("a CA object needs \"rule\", e.g. {\"rule\": \"Life\"}")?;
    o.insert("rule".into(), json!(resolve_rule(rule)));
    let seed = o.remove("seed").map(|s| parse_seed(&s)).transpose()?;
    let mut spec: CaSpec =
        serde_json::from_value(Value::Object(o)).map_err(|e| format!("bad CA {v}: {e}"))?;
    if seed.is_some() {
        spec.seed = seed;
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_nicknames_and_text_kept() {
        assert_eq!(resolve_rule("Game of Life"), resolve_rule("Life"));
        assert_eq!(resolve_rule("conway"), super::super::resolve_rule("Life"));
        assert_eq!(resolve_rule(" no such rule "), " no such rule ");
        assert_eq!(resolve_rule("Brian's Brain"), "B2/S/C3");
    }

    #[test]
    fn seeds_as_the_tools_read_them() {
        assert_eq!(
            parse_seed(&json!("random 20%")).unwrap(),
            SeedSpec::Random { density: 0.2 }
        );
        assert_eq!(
            parse_seed(&json!("ants:50000")).unwrap(),
            SeedSpec::Ants { count: 10_000 }
        );
        assert_eq!(
            parse_seed(&json!("lenia")).unwrap(),
            SeedSpec::Lenia {
                name: "orbium".into(),
                scale: 1.0
            }
        );
        assert_eq!(
            parse_seed(&json!({"random": 0.4, "note": "x"})).unwrap(),
            SeedSpec::Random { density: 0.4 }
        );
        // Delegated to the shared reader.
        assert_eq!(
            parse_seed(&json!("halves:0.3")).unwrap(),
            super::super::parse_seed(&json!("halves:0.3")).unwrap()
        );
        assert!(parse_seed(&json!("random:0.2:5")).is_err());
    }

    #[test]
    fn ca_items() {
        let c = parse_ca(&json!("Brian's Brain")).unwrap();
        assert_eq!(c.rule, "B2/S/C3");
        let c =
            parse_ca(&json!({"rule": "Life", "seed": "random:0.1", "palette": "Paper"})).unwrap();
        assert_eq!(c.seed, Some(SeedSpec::Random { density: 0.1 }));
        assert_eq!(c.palette.as_deref(), Some("Paper"));
        assert!(parse_ca(&json!({"seed": "random"})).is_err());
        assert!(parse_ca(&json!(3)).is_err());
    }
}
