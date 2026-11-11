//! RFC 8785 canonical JSON (the JSON Canonicalization Scheme, JCS): one spelling for every JSON
//! value, so every implementation writes the same bytes, and so the same hash.
//!
//! - **Objects**: members sorted by their names' UTF-16 code units (RFC 8785 section 3.2.3), at
//!   every depth; arrays keep their order.
//! - **No whitespace** between tokens.
//! - **Strings** (section 3.2.2.2): `\"`, `\\`, `\b`, `\t`, `\n`, `\f` and `\r`, the other control
//!   characters below U+0020 as lower-case `\u00hh`, and every other character as it is. Rust
//!   strings are UTF-8, so they hold no lone surrogates.
//! - **Numbers** (section 3.2.2.3): the IEEE 754 double as ECMAScript's `Number::toString` writes
//!   it, the shortest digits that read back to the same double ([`ryu_js`]): `1`, not `1.0`;
//!   `0.000001` but `1e-7`; `1e+21`; minus zero as `0`. Integers are numbers too, so one beyond
//!   2^53 is written as the double nearest it, as RFC 8785 says. A `serde_json` value holds no NaN
//!   or infinity, which RFC 8785 refuses.
//!
//! [`crate::pattern::identity_key`] writes the `tiling=` line of an identity text with it. The
//! tests check it against RFC 8785's own samples (Appendix B's numbers, the sorting sample of
//! section 3.2.3, the sample of section 3.2.2 and its bytes) and against vectors written by an
//! independent implementation, the `rfc8785` Python package (`tests/data/jcs_vectors.txt`, made
//! by `tools/jcs_vectors.py`).

use serde_json::{Number, Value};
use std::fmt::Write;

/// The canonical text of a JSON value.
pub fn to_string(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v);
    out
}

/// Append the canonical text of a JSON value.
pub fn write(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut members: Vec<(&String, &Value)> = m.iter().collect();
            members.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push('{');
            for (i, (k, x)) in members.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write(out, x);
            }
            out.push('}');
        }
    }
}

/// Append a number: its double, as ECMAScript writes it.
fn write_number(out: &mut String, n: &Number) {
    match n.as_f64() {
        // Minus zero is `0` in ECMAScript; written out here rather than left to the formatter.
        Some(x) if x == 0.0 => out.push('0'),
        Some(x) if x.is_finite() => out.push_str(ryu_js::Buffer::new().format_finite(x)),
        // Not reachable: a serde_json number is always a finite double.
        _ => out.push_str("null"),
    }
}

/// Append a string, quoted and escaped as RFC 8785 says.
pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// RFC 8785 Appendix B: IEEE 754 bits and their JSON (NaN and Infinity, which a JSON value
    /// cannot hold, left out).
    const APPENDIX_B: [(u64, &str); 25] = [
        (0x0000000000000000, "0"),
        (0x8000000000000000, "0"),
        (0x0000000000000001, "5e-324"),
        (0x8000000000000001, "-5e-324"),
        (0x7fefffffffffffff, "1.7976931348623157e+308"),
        (0xffefffffffffffff, "-1.7976931348623157e+308"),
        (0x4340000000000000, "9007199254740992"),
        (0xc340000000000000, "-9007199254740992"),
        (0x4430000000000000, "295147905179352830000"),
        (0x44b52d02c7e14af5, "9.999999999999997e+22"),
        (0x44b52d02c7e14af6, "1e+23"),
        (0x44b52d02c7e14af7, "1.0000000000000001e+23"),
        (0x444b1ae4d6e2ef4e, "999999999999999700000"),
        (0x444b1ae4d6e2ef4f, "999999999999999900000"),
        (0x444b1ae4d6e2ef50, "1e+21"),
        (0x3eb0c6f7a0b5ed8c, "9.999999999999997e-7"),
        (0x3eb0c6f7a0b5ed8d, "0.000001"),
        (0x41b3de4355555553, "333333333.3333332"),
        (0x41b3de4355555554, "333333333.33333325"),
        (0x41b3de4355555555, "333333333.3333333"),
        (0x41b3de4355555556, "333333333.3333334"),
        (0x41b3de4355555557, "333333333.33333343"),
        (0xbecbf647612f3696, "-0.0000033333333333333333"),
        (0x43143ff3c1cb0959, "1424953923781206.2"),
        // Not in the table: the two spellings this module's notes give as examples.
        (0x3e7ad7f29abcaf48, "1e-7"),
    ];

    #[test]
    fn numbers_match_rfc_8785_appendix_b() {
        for (bits, want) in APPENDIX_B {
            let x = f64::from_bits(bits);
            assert_eq!(to_string(&json!(x)), want, "{bits:016x}");
            // Read back, the text is the same double (minus zero aside).
            let back: f64 = want.parse().unwrap();
            assert!(back == x, "{want} reads back as {back}, not {x}");
        }
        // Integers and the float of the same value are one spelling.
        assert_eq!(
            to_string(&json!([1, 1.0, -3, -3.0, 0.5, 100, 1e2])),
            "[1,1,-3,-3,0.5,100,100]"
        );
        // Beyond 2^53 an integer is the double nearest it.
        assert_eq!(to_string(&json!(9007199254740993u64)), "9007199254740992");
    }

    #[test]
    fn members_sort_by_utf16_code_units() {
        // RFC 8785 section 3.2.3's sample: the astral U+1F600 (surrogates D83D DE00) comes before
        // U+FB33, as UTF-16 sorts, though UTF-8 and code points sort it after.
        let v: Value = serde_json::from_str(
            r#"{
              "\u20ac": "Euro Sign",
              "\r": "Carriage Return",
              "\ufb33": "Hebrew Letter Dalet With Dagesh",
              "1": "One",
              "\ud83d\ude00": "Emoji: Grinning Face",
              "\u0080": "Control",
              "\u00f6": "Latin Small Letter O With Diaeresis"
            }"#,
        )
        .unwrap();
        let text = to_string(&v);
        let order: Vec<&str> = [
            "Carriage Return",
            "One",
            "Control",
            "Latin Small Letter O With Diaeresis",
            "Euro Sign",
            "Emoji: Grinning Face",
            "Hebrew Letter Dalet With Dagesh",
        ]
        .into_iter()
        .collect();
        let at: Vec<usize> = order.iter().map(|s| text.find(s).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(
            text.starts_with("{\"\\r\":\"Carriage Return\",\"1\":\"One\","),
            "{text}"
        );
        // Nested objects too, and arrays keep their order.
        assert_eq!(
            to_string(&json!({"b": [{"z": 1, "a": 2}, 0], "a": {"y": null, "x": true}})),
            r#"{"a":{"x":true,"y":null},"b":[{"a":2,"z":1},0]}"#
        );
    }

    #[test]
    fn rfc_8785_sample_and_its_bytes() {
        // Section 3.2.2's sample input, canonicalised as section 3.2.3 shows, in the bytes of
        // section 3.2.4.
        let v: Value = serde_json::from_str(
            r#"{
              "numbers": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
              "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
              "literals": [null, true, false]
            }"#,
        )
        .unwrap();
        let text = to_string(&v);
        assert_eq!(
            text,
            r#"{"literals":[null,true,false],"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],"string":"€$\u000f\nA'B\"\\\\\"/"}"#
        );
        let hex = "7b 22 6c 69 74 65 72 61 6c 73 22 3a 5b 6e 75 6c 6c 2c 74 72
                   75 65 2c 66 61 6c 73 65 5d 2c 22 6e 75 6d 62 65 72 73 22 3a
                   5b 33 33 33 33 33 33 33 33 33 2e 33 33 33 33 33 33 33 2c 31
                   65 2b 33 30 2c 34 2e 35 2c 30 2e 30 30 32 2c 31 65 2d 32 37
                   5d 2c 22 73 74 72 69 6e 67 22 3a 22 e2 82 ac 24 5c 75 30 30
                   30 66 5c 6e 41 27 42 5c 22 5c 5c 5c 5c 5c 22 2f 22 7d";
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect();
        assert_eq!(text.as_bytes(), &bytes[..]);
    }

    #[test]
    fn strings_escape_only_what_rfc_8785_escapes() {
        let s: String = (0u32..0x80)
            .chain([0x7f, 0x80, 0x2028, 0x1f600])
            .filter_map(char::from_u32)
            .collect();
        let text = to_string(&json!(s));
        assert!(text.starts_with(
            r#""\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e"#
        ));
        assert!(text.contains(r##"\u001f !\"#"##));
        assert!(text.contains(r#"[\\]"#));
        assert!(text.ends_with("\u{7f}\u{7f}\u{80}\u{2028}\u{1f600}\""));
    }
}
