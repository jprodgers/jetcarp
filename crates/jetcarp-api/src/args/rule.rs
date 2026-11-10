//! Rule names, neighbourhoods, and the options of one CA.

use super::json::{get, get_bool, get_str, get_u64};
use super::seed::parse_seed;
use jetcarp_core::world::{AgentEdge, CaSpec};
use jetcarp_core::{Nb, rules};
use serde_json::Value;

/// Letters and digits only, lowercased: how rule names are compared, so "Brian's Brain",
/// "brians-brain", and "BRIANS BRAIN" are one name (the MCP tools compare CA and family names
/// the same way).
pub fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Other names for catalog rules: (squashed nickname, catalog name).
const NICKNAMES: &[(&str, &str)] = &[
    ("gameoflife", "Life"),
    ("conway", "Life"),
    ("conwayslife", "Life"),
];

/// The notation of the catalog rule whose [`squash`]ed name (or nickname: "Game of Life",
/// "Conway") is `key`.
pub(crate) fn catalog_notation(key: &str) -> Option<&'static str> {
    let key = NICKNAMES
        .iter()
        .find(|a| a.0 == key)
        .map(|a| squash(a.1))
        .unwrap_or_else(|| key.to_string());
    rules::catalog()
        .iter()
        .find(|e| squash(e.name) == key)
        .map(|e| e.notation)
}

/// A rule as the scripts take it: notation JETCARP reads is kept as written (trimmed); otherwise
/// a catalog name or nickname (any case, ignoring spaces and punctuation: "Brian's Brain",
/// "Game of Life") becomes its notation; anything else is returned trimmed, for the engine to
/// reject or read.
pub fn resolve_rule(text: &str) -> String {
    let t = text.trim();
    if rules::Rule::parse(t).is_ok() {
        return t.to_string();
    }
    catalog_notation(&squash(t))
        .map(String::from)
        .unwrap_or_else(|| t.to_string())
}

/// A neighbourhood by name: `touch` (also `touching`, `moore`, `corner`, `vertex`, `t`, `8`) or
/// `edge` (`von_neumann`, `vonneumann`, `vn`, `e`, `4`). Any case; `-` and spaces count as `_`,
/// so `von-neumann` and `von Neumann` work too. `None` for anything else, so each caller can
/// say what it expected.
pub fn nb_word(s: &str) -> Option<Nb> {
    match s
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "touch" | "touching" | "moore" | "corner" | "vertex" | "t" | "8" => Some(Nb::Touch),
        "edge" | "von_neumann" | "vonneumann" | "vn" | "e" | "4" => Some(Nb::Edge),
        _ => None,
    }
}

/// A neighbourhood ([`nb_word`]), as a CA's `neighborhood` option.
pub fn parse_nb(s: &str) -> Result<Nb, String> {
    nb_word(s).ok_or_else(|| format!("neighborhood must be touch or edge, got '{s}'"))
}

/// A sandpile's neighbourhood ([`nb_word`]); the message puts `edge`, the sandpile default,
/// first.
pub fn parse_pile_nb(s: &str) -> Result<Nb, String> {
    nb_word(s).ok_or_else(|| {
        format!("neighborhood must be edge (von Neumann) or touch (Moore), got '{s}'")
    })
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
        for n in ["Game of Life", "Conway", "conway's life", "life"] {
            assert_eq!(resolve_rule(n), "B3/S23", "{n}");
        }
    }

    #[test]
    fn neighbourhoods_by_every_name() {
        for s in ["touch", "Touching", "moore", "corner", "vertex", "t", "8"] {
            assert_eq!(nb_word(s), Some(Nb::Touch), "{s}");
        }
        for s in [
            "edge",
            "von_neumann",
            "von-neumann",
            "Von Neumann",
            "vonneumann",
            "vn",
        ] {
            assert_eq!(nb_word(s), Some(Nb::Edge), "{s}");
        }
        assert_eq!(nb_word(" e "), Some(Nb::Edge));
        assert_eq!(nb_word("4"), Some(Nb::Edge));
        assert_eq!(nb_word("hex"), None);
        assert_eq!(
            parse_nb("hex").unwrap_err(),
            "neighborhood must be touch or edge, got 'hex'"
        );
        assert!(
            parse_pile_nb("hex")
                .unwrap_err()
                .contains("edge (von Neumann)")
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
