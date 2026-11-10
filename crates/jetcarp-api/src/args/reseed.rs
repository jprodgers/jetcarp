//! Reseed modes and automatic reseed policies (the book's "Reseeding" guide).

use jetcarp_core::reseed::{self, AutoReseed, ReseedMode};
use serde_json::Value;

/// A reseed mode: a map (`{"kind": "rain", "drops": 30}`), or text: a mode name with its
/// defaults (`"random"`, `"rain"`, `"blobs"`), a name and a JACA sign (`"rain:pos"`, see
/// [`reseed::Sign::parse`]), or the mode written as JSON (text starting with `{` or `[`). The
/// error is the reader's own message, for the caller to put in context.
pub fn reseed_mode(v: &Value) -> Result<ReseedMode, String> {
    let v = match v {
        Value::String(s) if s.trim_start().starts_with(['{', '[']) => {
            return serde_json::from_str(s).map_err(|e| e.to_string());
        }
        Value::String(s) => reseed::mode_json(s),
        v => v.clone(),
    };
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// One automatic reseed policy: `{"mode": ..., "when": ..., "cooldown": 200, ...}`, where the
/// mode and the trigger may be bare names (`"rain"`, `"settled"`).
pub fn reseed_policy(v: &Value) -> Result<AutoReseed, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}

/// Policies: a list of [`reseed_policy`] maps (`[]` clears them), or a single one.
pub fn reseed_policies(v: &Value) -> Result<Vec<AutoReseed>, String> {
    if v.is_array() {
        serde_json::from_value(v.clone()).map_err(|e| e.to_string())
    } else {
        reseed_policy(v).map(|p| vec![p])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::reseed::{ReseedWhen, Sign};
    use serde_json::json;

    #[test]
    fn modes_by_name_sign_map_and_json_text() {
        assert_eq!(reseed_mode(&json!("rain")).unwrap(), ReseedMode::rain());
        assert_eq!(reseed_mode(&json!(" blobs ")).unwrap(), ReseedMode::blobs());
        assert_eq!(
            reseed_mode(&json!({"kind": "random"})).unwrap(),
            ReseedMode::random()
        );
        let m = reseed_mode(&json!("rain:neg")).unwrap();
        assert!(matches!(
            m,
            ReseedMode::Rain {
                sign: Sign::Neg,
                ..
            }
        ));
        let m = reseed_mode(&json!(r#" {"kind": "rain", "drops": 30}"#)).unwrap();
        assert!(matches!(m, ReseedMode::Rain { drops: 30, .. }));
        let m = reseed_mode(&json!(r#"["random", 0.1]"#)).unwrap();
        assert!(matches!(m, ReseedMode::Random { flip: 0.1, .. }));
        assert!(reseed_mode(&json!("drizzle")).is_err());
        assert!(reseed_mode(&json!("{not json")).is_err());
    }

    #[test]
    fn one_policy_or_a_list() {
        let one = json!({"mode": "rain", "when": "settled", "cooldown": 300});
        let p = reseed_policies(&one).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].mode, ReseedMode::rain());
        assert!(matches!(p[0].when, ReseedWhen::Settled { .. }));
        let p = reseed_policies(
            &json!([one, {"mode": "blobs", "when": {"kind": "every", "gens": 500}}]),
        )
        .unwrap();
        assert_eq!(p.len(), 2);
        assert!(matches!(p[1].when, ReseedWhen::Every { gens: 500 }));
        assert!(reseed_policies(&json!([])).unwrap().is_empty());
        assert!(reseed_policy(&json!({"mode": "drizzle"})).is_err());
    }
}
