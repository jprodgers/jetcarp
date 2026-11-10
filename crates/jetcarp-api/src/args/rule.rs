//! Rule names, neighbourhoods, and the options of one CA.

use super::json::{get, get_bool, get_str, get_u64};
use super::seed::parse_seed;
use jetcarp_core::world::{AgentEdge, CaSpec};
use jetcarp_core::{Nb, rules};
use serde_json::Value;

/// Letters and digits only, lowercased: how rule names are compared, so "Brian's Brain",
/// "brians-brain", and "BRIANS BRAIN" are one name.
pub(crate) fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// The notation of the catalog rule whose [`squash`]ed name is `key`.
pub(crate) fn catalog_notation(key: &str) -> Option<&'static str> {
    rules::catalog()
        .iter()
        .find(|e| squash(e.name) == key)
        .map(|e| e.notation)
}

/// A rule as the scripts take it: notation JETCARP reads is kept as written (trimmed); otherwise
/// a catalog name (any case, ignoring spaces and punctuation) becomes its notation; anything
/// else is returned trimmed, for the engine to reject or read.
pub fn resolve_rule(text: &str) -> String {
    let t = text.trim();
    if rules::Rule::parse(t).is_ok() {
        return t.to_string();
    }
    catalog_notation(&squash(t))
        .map(String::from)
        .unwrap_or_else(|| t.to_string())
}

/// A neighbourhood by name: `touch` (also `touching`, `moore`, `corner`) or `edge`
/// (`von_neumann`, `vonneumann`). Any case; `-` and spaces count as `_`.
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

/// A CA spec from a rule ([`resolve_rule`]) and options (`seed`, `neighborhood`, `palette`,
/// `steps_per_gen`, `name`, `affects_others`, `enabled`, `drop_per_gen`, `color_by`,
/// `agent_edge`, and for JACA `palette_neg` and `glow`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rule_names_and_notation() {
        assert_eq!(resolve_rule("brian's brain"), "B2/S/C3");
        assert_eq!(resolve_rule("  BRIANS-BRAIN "), "B2/S/C3");
        assert_eq!(resolve_rule(" B3/S23 "), "B3/S23");
        assert_eq!(resolve_rule("no such rule"), "no such rule");
        assert_eq!(squash("Day & Night"), "daynight");
    }

    #[test]
    fn neighbourhoods() {
        for s in ["touch", "Touching", "moore", "corner"] {
            assert_eq!(parse_nb(s), Ok(Nb::Touch), "{s}");
        }
        for s in [
            "edge",
            "von_neumann",
            "von-neumann",
            "Von Neumann",
            "vonneumann",
        ] {
            assert_eq!(parse_nb(s), Ok(Nb::Edge), "{s}");
        }
        assert_eq!(
            parse_nb("hex").unwrap_err(),
            "neighborhood must be touch or edge, got 'hex'"
        );
    }

    #[test]
    fn ca_options() {
        let s = ca_spec(
            "Life",
            &json!({"seed": "random:0.2", "neighborhood": "edge", "steps_per_gen": 0,
                    "name": "a", "agent_edge": "fall-off"}),
        )
        .unwrap();
        assert_eq!(s.neighborhood, Some(Nb::Edge));
        assert_eq!(s.steps_per_gen, 1);
        assert_eq!(s.name.as_deref(), Some("a"));
        assert_eq!(s.agent_edge, AgentEdge::FallOff);
        assert!(ca_spec("Life", &json!({"agent_edge": "wrap"})).is_err());
        let mut s = CaSpec::new("B3/S23");
        apply_ca_options(&mut s, &json!({"palette": "Paper"})).unwrap();
        assert_eq!(s.palette.as_deref(), Some("Paper"));
        assert_eq!(s.seed, None);
    }
}
