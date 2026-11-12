//! Integers in JSON: one beyond ±(2^53 − 1) is written as a decimal string.
//!
//! A JSON number is read as an IEEE 754 double by most programs (JavaScript, and RFC 8785
//! canonical JSON, [`crate::jcs`]), and a double holds every integer exactly only up to 2^53, so
//! I-JSON ([RFC 7493](https://www.rfc-editor.org/rfc/rfc7493) section 2.2) asks for larger ones to
//! be strings. JETCARP writes an integer within ±(2^53 − 1) as a number, and one outside as its
//! decimal digits in a string (`"rng_state": "17705299566207515059"`), in every JSON it writes:
//! scenes, specs, records, packs, the website export, and the API's answers. It reads either.
//!
//! - The `with` modules ([`uint`], [`int`], [`uint_opt`], [`int_opt`], [`uint_vec`]) do it for a
//!   typed field: `#[serde(with = "jetcarp_core::json_int::uint")] rng_state: u64`. An `Option`
//!   field also needs `default`, so a missing member is still `None`.
//! - [`safe`] does it for a free-form [`Value`] (a tiling's parameters, a record's details), and
//!   [`value_u64`] and [`value_i64`] read such a member back.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::fmt;

/// The largest integer every JSON reader holds exactly: 2^53 − 1 (JavaScript's
/// `Number.MAX_SAFE_INTEGER`).
pub const MAX_SAFE: u64 = (1 << 53) - 1;

/// Does a JSON reader hold this integer exactly?
pub fn safe_u64(x: u64) -> bool {
    x <= MAX_SAFE
}

/// Does a JSON reader hold this integer exactly?
pub fn safe_i64(x: i64) -> bool {
    x.unsigned_abs() <= MAX_SAFE
}

/// A free-form value with every integer beyond ±(2^53 − 1) written as a decimal string, at every
/// depth. Everything else is left as it is.
pub fn safe(v: Value) -> Value {
    match v {
        Value::Number(n) => match (n.as_u64(), n.as_i64()) {
            (Some(u), _) if !safe_u64(u) => Value::String(u.to_string()),
            (None, Some(i)) if !safe_i64(i) => Value::String(i.to_string()),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(safe).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, safe(v))).collect()),
        x => x,
    }
}

/// `#[serde(serialize_with = "jetcarp_core::json_int::safe_opt")]` on an `Option<Value>`: the
/// value written as [`safe`] makes it.
pub fn safe_opt<S: Serializer>(v: &Option<Value>, s: S) -> Result<S::Ok, S::Error> {
    v.as_ref().map(|v| safe(v.clone())).serialize(s)
}

/// An unsigned integer member written either way: a number, or a string of decimal digits.
pub fn value_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => parse_u64(s),
        _ => None,
    }
}

/// A signed integer member written either way: a number, or a string of decimal digits with an
/// optional `-`.
pub fn value_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => parse_i64(s),
        _ => None,
    }
}

fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_i64(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The integer of a JSON value, a number or a decimal string, for the visitors.
struct IntVisitor<T>(std::marker::PhantomData<T>);

impl<'de> Visitor<'de> for IntVisitor<u64> {
    type Value = u64;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a non-negative integer, as a number or a string of decimal digits")
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
        Ok(v)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
        u64::try_from(v).map_err(|_| E::invalid_value(de::Unexpected::Signed(v), &self))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
        parse_u64(v).ok_or_else(|| E::invalid_value(de::Unexpected::Str(v), &self))
    }
}

impl<'de> Visitor<'de> for IntVisitor<i64> {
    type Value = i64;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an integer, as a number or a string of decimal digits")
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<i64, E> {
        i64::try_from(v).map_err(|_| E::invalid_value(de::Unexpected::Unsigned(v), &self))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<i64, E> {
        Ok(v)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
        parse_i64(v).ok_or_else(|| E::invalid_value(de::Unexpected::Str(v), &self))
    }
}

/// A `u64` read either way (for the `with` modules).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct U64(pub u64);

impl Serialize for U64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        uint::serialize(&self.0, s)
    }
}

impl<'de> Deserialize<'de> for U64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<U64, D::Error> {
        uint::deserialize(d).map(U64)
    }
}

/// An `i64` read either way (for the `with` modules).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I64(pub i64);

impl Serialize for I64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        int::serialize(&self.0, s)
    }
}

impl<'de> Deserialize<'de> for I64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<I64, D::Error> {
        int::deserialize(d).map(I64)
    }
}

/// `#[serde(with = "jetcarp_core::json_int::uint")]` on a `u64`.
pub mod uint {
    use super::*;
    pub fn serialize<S: Serializer>(x: &u64, s: S) -> Result<S::Ok, S::Error> {
        if safe_u64(*x) {
            s.serialize_u64(*x)
        } else {
            s.collect_str(x)
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        d.deserialize_any(IntVisitor::<u64>(std::marker::PhantomData))
    }
}

/// `#[serde(with = "jetcarp_core::json_int::int")]` on an `i64`.
pub mod int {
    use super::*;
    pub fn serialize<S: Serializer>(x: &i64, s: S) -> Result<S::Ok, S::Error> {
        if safe_i64(*x) {
            s.serialize_i64(*x)
        } else {
            s.collect_str(x)
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        d.deserialize_any(IntVisitor::<i64>(std::marker::PhantomData))
    }
}

/// `#[serde(default, with = "jetcarp_core::json_int::uint_opt")]` on an `Option<u64>`.
pub mod uint_opt {
    use super::*;
    pub fn serialize<S: Serializer>(x: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        x.map(U64).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        Ok(Option::<U64>::deserialize(d)?.map(|x| x.0))
    }
}

/// `#[serde(default, with = "jetcarp_core::json_int::int_opt")]` on an `Option<i64>`.
pub mod int_opt {
    use super::*;
    pub fn serialize<S: Serializer>(x: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
        x.map(I64).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        Ok(Option::<I64>::deserialize(d)?.map(|x| x.0))
    }
}

/// `#[serde(with = "jetcarp_core::json_int::uint_vec")]` on a `Vec<u64>`.
pub mod uint_vec {
    use super::*;
    pub fn serialize<S: Serializer>(x: &[u64], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(x.iter().map(|v| U64(*v)))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u64>, D::Error> {
        Ok(Vec::<U64>::deserialize(d)?
            .into_iter()
            .map(|x| x.0)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct T {
        #[serde(with = "uint")]
        a: u64,
        #[serde(with = "int")]
        b: i64,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "uint_opt")]
        c: Option<u64>,
        #[serde(default, with = "int_opt")]
        d: Option<i64>,
        #[serde(default, with = "uint_vec")]
        e: Vec<u64>,
    }

    #[test]
    fn large_integers_are_strings_and_read_either_way() {
        let t = T {
            a: 17705299566207515059,
            b: -(1 << 60),
            c: Some(MAX_SAFE),
            d: Some(-(MAX_SAFE as i64)),
            e: vec![1, MAX_SAFE + 1, u64::MAX],
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({"a": "17705299566207515059", "b": "-1152921504606846976",
                   "c": 9007199254740991u64, "d": -9007199254740991i64,
                   "e": [1, "9007199254740992", "18446744073709551615"]})
        );
        assert_eq!(serde_json::from_value::<T>(v).unwrap(), t);
        // Text too, and old files' numbers.
        let text = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<T>(&text).unwrap(), t);
        let old: T = serde_json::from_str(
            r#"{"a": 17705299566207515059, "b": -1152921504606846976, "e": [3]}"#,
        )
        .unwrap();
        assert_eq!(
            (old.a, old.b, old.c, old.d, old.e),
            (t.a, t.b, None, None, vec![3])
        );
        // Not integers, or not decimal digits: refused.
        for bad in [
            r#"{"a": "1e3", "b": 0}"#,
            r#"{"a": " 1", "b": 0}"#,
            r#"{"a": "+1", "b": 0}"#,
            r#"{"a": "", "b": 0}"#,
            r#"{"a": -1, "b": 0}"#,
            r#"{"a": "-1", "b": 0}"#,
            r#"{"a": 1.5, "b": 0}"#,
            r#"{"a": 1, "b": "9223372036854775808"}"#,
            r#"{"a": "18446744073709551616", "b": 0}"#,
        ] {
            assert!(serde_json::from_str::<T>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn free_form_values() {
        let v = safe(json!({"x": [MAX_SAFE, MAX_SAFE + 1, -(1i64 << 62), 0.5], "y": "z"}));
        assert_eq!(
            v,
            json!({"x": [9007199254740991u64, "9007199254740992", "-4611686018427387904", 0.5],
                   "y": "z"})
        );
        assert_eq!(value_u64(&v["x"][1]), Some(MAX_SAFE + 1));
        assert_eq!(value_u64(&v["x"][0]), Some(MAX_SAFE));
        assert_eq!(value_i64(&v["x"][2]), Some(-(1i64 << 62)));
        assert_eq!(value_u64(&v["y"]), None);
        assert_eq!(value_u64(&json!(-3)), None);
    }
}
