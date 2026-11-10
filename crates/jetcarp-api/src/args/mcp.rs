//! How the MCP tools read rule names, seeds, and CA items, where that differs from the shared
//! parsers on purpose.
//!
//! When the parsers were shared (refactor step 2) the tools kept their own readings so their
//! results would not change; step 3 settled each difference:
//!
//! - **Merged into the shared parsers** (they only widened what is accepted): the nicknames
//!   "Game of Life", "Conway", and "Conway's Life"; a space before a seed's first number
//!   (`"random 0.2"`, `"random 20%"`, `"random: 0.2"`); `"lenia"` alone for Orbium. In turn the
//!   tools now take every shorthand the scripts do (`soup`, `blob`, `top_random`, `centre`, `-`
//!   for `_`, `gauss:N:R:P`, `lenia:NAME:SCALE`, `{"density": d}`). The scene format's
//!   `{"random": {"density": d}}`, which the shared reader rejected, is read by both now.
//! - **Kept for the tools** (clients may rely on them):
//!   - [`resolve_rule`] looks a name up in the catalog first, even when it is also notation the
//!     engine reads, so a client sees the catalog's notation (`wireworld` becomes `WireWorld`).
//!   - [`parse_seed`] takes `{"random": d}` beside other keys, and lists its examples as JSON.
//!   - [`parse_ca`] reads a CA object in the scene format (any `CaSpec` field, as `get_scene`
//!     returns it), where the scripts' [`super::ca_spec`] takes their own option names.
//! - **Moved to `Ctx::limits`**: at most 10,000 ants for an `ants:N` seed
//!   ([`crate::Limits::max_ants`]; the shared reader allows 100,000), applied by the world
//!   operations with the tools' other limits.

use super::rule::{catalog_notation, squash};
use jetcarp_core::SeedSpec;
use jetcarp_core::world::CaSpec;
use serde_json::{Value, json};

/// The examples the tools' error message lists.
const TOOL_FORMS: &str = "\"empty\", \"random\", \"random:0.2\", \"center\", \"top_center\", \"random_states\", \"ants:3\", \"pile:10000\", JACA \"signed:0.3:0.5\" (density, share of -1) or \"halves:0.3\" (+1 left, -1 right), \"pattern:blobs:0.4:by_region\", {\"shape\": \"blobs\", \"count\": 3, \"density\": 0.4}, or {\"random\": {\"density\": 0.2}}.";

/// A catalog rule name or nickname ("Brian's Brain", "life", "Conway") becomes its catalog
/// notation, even where the text is notation too; anything else is read as the scripts read it.
pub fn resolve_rule(text: &str) -> String {
    let t = text.trim();
    catalog_notation(&squash(t))
        .map(String::from)
        .unwrap_or_else(|| super::resolve_rule(t))
}

/// Seed shorthands as the scripts read them ([`super::parse_seed`]), plus `{"random": d}` beside
/// other keys; errors list JSON examples.
pub fn parse_seed(v: &Value) -> Result<SeedSpec, String> {
    if let Some(d) = v.get("random").and_then(|d| d.as_f64())
        && v.get("shape").is_none()
        && v.get("pattern").is_none()
    {
        return super::seed::parse_seed_with(&json!(format!("random:{d}")), TOOL_FORMS);
    }
    super::seed::parse_seed_with(v, TOOL_FORMS)
}

/// A CA item from `new_world` or `add_ca`: a rule string or a CA object in the scene format.
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
    fn rule_nicknames_and_catalog_first() {
        assert_eq!(resolve_rule("Game of Life"), resolve_rule("Life"));
        assert_eq!(resolve_rule("conway"), super::super::resolve_rule("Life"));
        assert_eq!(resolve_rule(" no such rule "), "no such rule");
        assert_eq!(resolve_rule("Brian's Brain"), "B2/S/C3");
        assert_eq!(resolve_rule(" B36/S23 "), "B36/S23");
    }

    #[test]
    fn seeds_as_the_tools_read_them() {
        assert_eq!(
            parse_seed(&json!("random 20%")).unwrap(),
            SeedSpec::Random { density: 0.2 }
        );
        // The ants cap is a limit of the tools now (`Limits::max_ants`), not of the reader.
        assert_eq!(
            parse_seed(&json!("ants:50000")).unwrap(),
            SeedSpec::Ants { count: 50_000 }
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
        assert_eq!(
            parse_seed(&json!({"random": {"density": 0.3}})).unwrap(),
            SeedSpec::Random { density: 0.3 }
        );
        // Shared forms.
        assert_eq!(
            parse_seed(&json!("halves:0.3")).unwrap(),
            super::super::parse_seed(&json!("halves:0.3")).unwrap()
        );
        assert_eq!(
            parse_seed(&json!("soup:0.1")).unwrap(),
            SeedSpec::Random { density: 0.1 }
        );
        let e = parse_seed(&json!("hail")).unwrap_err();
        assert!(e.contains("{\"shape\": \"blobs\""), "{e}");
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
