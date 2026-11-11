//! The JSON documents of a pack (manifests, indexes, refs, key statements): RFC 8785 canonical
//! form ([`jetcarp_core::jcs`]), no non-integer numbers, integers within ±2^53, nesting to 32
//! levels, and at most 16 MiB. Reading checks all of it before the document is used.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};

/// Largest JSON document (manifest, index, ref, key statement), bytes.
pub const MAX_DOC: u64 = 16 << 20;
/// Deepest nesting of arrays and objects.
pub const MAX_DEPTH: usize = 32;
/// Largest integer magnitude: 2^53.
pub const MAX_INT: u64 = 1 << 53;

/// The canonical bytes of a document, refusing what a pack document may not hold.
pub fn canonical(v: &Value) -> Result<Vec<u8>, String> {
    check(v, 0)?;
    Ok(jetcarp_core::jcs::to_string(v).into_bytes())
}

/// The canonical bytes of a serializable document.
pub fn canonical_of<T: Serialize>(t: &T) -> Result<Vec<u8>, String> {
    canonical(&serde_json::to_value(t).map_err(|e| e.to_string())?)
}

fn check(v: &Value, depth: usize) -> Result<(), String> {
    match v {
        Value::Number(n) => check_number(n),
        Value::Array(a) => {
            if depth >= MAX_DEPTH {
                return Err(format!("JSON nested more than {MAX_DEPTH} levels"));
            }
            a.iter().try_for_each(|x| check(x, depth + 1))
        }
        Value::Object(m) => {
            if depth >= MAX_DEPTH {
                return Err(format!("JSON nested more than {MAX_DEPTH} levels"));
            }
            m.values().try_for_each(|x| check(x, depth + 1))
        }
        _ => Ok(()),
    }
}

fn check_number(n: &Number) -> Result<(), String> {
    if let Some(i) = n.as_i64() {
        if i.unsigned_abs() <= MAX_INT {
            return Ok(());
        }
    } else if let Some(u) = n.as_u64()
        && u <= MAX_INT
    {
        return Ok(());
    }
    Err(format!(
        "a pack document holds only integers within ±2^53 (decimals are strings), not {n}"
    ))
}

/// The nesting depth of JSON text, counted without parsing it (so a hostile document cannot
/// exhaust the parser's stack): `[` and `{` outside strings.
pub fn depth_of(bytes: &[u8]) -> usize {
    let (mut depth, mut max, mut in_str, mut esc) = (0usize, 0usize, false, false);
    for &b in bytes {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => {
                depth += 1;
                max = max.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// Parse a pack document: at most [`MAX_DOC`] bytes, nested at most [`MAX_DEPTH`] levels, UTF-8,
/// its numbers integers within ±2^53, and (with `strict`) written in canonical form.
pub fn parse(bytes: &[u8], what: &str, strict: bool) -> Result<Value, String> {
    if bytes.len() as u64 > MAX_DOC {
        return Err(format!("{what}: larger than 16 MiB"));
    }
    if depth_of(bytes) > MAX_DEPTH {
        return Err(format!("{what}: JSON nested more than {MAX_DEPTH} levels"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{what}: not UTF-8"))?;
    if text.starts_with('\u{feff}') {
        return Err(format!("{what}: starts with a byte-order mark"));
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{what}: not JSON: {e}"))?;
    check(&v, 0).map_err(|e| format!("{what}: {e}"))?;
    if strict && jetcarp_core::jcs::to_string(&v).as_bytes() != bytes {
        return Err(format!("{what}: not in RFC 8785 canonical form"));
    }
    Ok(v)
}

/// [`parse`] into a type.
pub fn parse_as<T: DeserializeOwned>(bytes: &[u8], what: &str, strict: bool) -> Result<T, String> {
    let v = parse(bytes, what, strict)?;
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

/// A value made fit for a pack document: every non-integer number, and every integer beyond
/// ±2^53, becomes a string holding the number as ECMAScript writes it (RFC 8785's form), so a
/// search's settings (densities such as `0.35`) can be copied into an index. Integers and
/// everything else stay as they are.
pub fn sanitize(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            if check_number(&n).is_ok() {
                Value::Number(n)
            } else {
                let mut s = String::new();
                jetcarp_core::jcs::write(&mut s, &Value::Number(n));
                Value::String(s)
            }
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sanitize).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, sanitize(v)))
                .collect::<Map<String, Value>>(),
        ),
        x => x,
    }
}

/// A decimal as a string: the shortest digits that read back to the same double (`0.15`, `2`).
pub fn decimal(x: f64) -> String {
    let mut s = String::new();
    jetcarp_core::jcs::write(&mut s, &serde_json::json!(x));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn documents_are_canonical_and_bounded() {
        let v = json!({"b": 1, "a": [true, null, "x"], "c": {"z": -5, "y": 9007199254740992u64}});
        let b = canonical(&v).unwrap();
        assert_eq!(
            String::from_utf8(b.clone()).unwrap(),
            r#"{"a":[true,null,"x"],"b":1,"c":{"y":9007199254740992,"z":-5}}"#
        );
        assert_eq!(parse(&b, "doc", true).unwrap(), v);
        // Not canonical: refused when strict.
        assert!(parse(br#"{"b":1, "a":2}"#, "doc", true).is_err());
        assert!(parse(br#"{"b":1, "a":2}"#, "doc", false).is_ok());
        // Floats and big integers are refused; sanitize writes them as strings.
        assert!(canonical(&json!({"x": 0.5})).is_err());
        assert!(canonical(&json!({"x": 9007199254740993u64})).is_err());
        assert_eq!(
            sanitize(json!({"x": 0.35, "n": [1e21, 3]})),
            json!({"x": "0.35", "n": ["1e+21", 3]})
        );
        assert_eq!(decimal(2.0), "2");
        assert_eq!(decimal(0.000001), "0.000001");
        // Depth: counted without parsing, strings ignored.
        let deep = "[".repeat(40) + &"]".repeat(40);
        assert_eq!(depth_of(deep.as_bytes()), 40);
        assert!(parse(deep.as_bytes(), "doc", false).is_err());
        assert_eq!(depth_of(br#"{"a":"[[[[[[["}"#), 1);
        let ok = "[".repeat(32) + &"]".repeat(32);
        assert!(parse(ok.as_bytes(), "doc", false).is_ok());
        // A byte-order mark is refused.
        assert!(parse("\u{feff}{}".as_bytes(), "doc", false).is_err());
    }
}
