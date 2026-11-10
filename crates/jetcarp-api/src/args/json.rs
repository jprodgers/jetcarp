//! Reading options out of a JSON map. Missing keys and `null` both mean "not given"; a value of
//! the wrong type is an error that names the key.

use serde_json::Value;

/// The value under `key`, unless it is missing or `null`.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

/// Text under `key`.
pub fn get_str<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(x) => Err(format!("`{key}` should be text, got {x}")),
    }
}

/// A whole number ≥ 0 under `key`: a number (`3`, `3.0`) or text (`"3"`).
pub fn get_u64(v: &Value, key: &str) -> Result<Option<u64>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(x) => x
            .as_u64()
            .or_else(|| {
                x.as_f64()
                    .filter(|f| *f >= 0.0 && f.fract() == 0.0)
                    .map(|f| f as u64)
            })
            .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` should be a whole number ≥ 0, got {x}")),
    }
}

/// A number under `key`: a number or text (`"0.5"`).
pub fn get_f64(v: &Value, key: &str) -> Result<Option<f64>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(x) => x
            .as_f64()
            .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` should be a number, got {x}")),
    }
}

/// `true` or `false` under `key`.
pub fn get_bool(v: &Value, key: &str) -> Result<Option<bool>, String> {
    match get(v, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(x) => Err(format!("`{key}` should be true or false, got {x}")),
    }
}

/// Reject keys that are not in `allowed`, listing the ones that are. `what` names the options
/// in the message ("unknown world option ..."). `null` stands for no options.
pub fn check_keys(v: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(o) = v.as_object() else {
        return if v.is_null() {
            Ok(())
        } else {
            Err(format!(
                "{what} options should be a map like #{{ key: value }}, got {v}"
            ))
        };
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!(
                "unknown {what} option `{k}`; options are: {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn readers_take_numbers_as_text_and_skip_null() {
        let v = json!({"n": "12", "f": 2.0, "x": 0.5, "s": "hi", "b": true, "z": null});
        assert_eq!(get_u64(&v, "n").unwrap(), Some(12));
        assert_eq!(get_u64(&v, "f").unwrap(), Some(2));
        assert!(get_u64(&v, "x").is_err());
        assert_eq!(get_f64(&v, "x").unwrap(), Some(0.5));
        assert_eq!(get_str(&v, "s").unwrap(), Some("hi"));
        assert!(get_str(&v, "n").is_ok() && get_str(&v, "b").is_err());
        assert_eq!(get_bool(&v, "b").unwrap(), Some(true));
        assert_eq!(get_u64(&v, "z").unwrap(), None);
        assert!(get(&v, "z").is_none() && get(&v, "missing").is_none());
    }

    #[test]
    fn unknown_keys_are_named() {
        let v = json!({"tiling": "hex", "sise": 3});
        let e = check_keys(&v, &["tiling", "size"], "world").unwrap_err();
        assert_eq!(e, "unknown world option `sise`; options are: tiling, size");
        assert!(check_keys(&Value::Null, &[], "world").is_ok());
        assert!(check_keys(&json!(3), &[], "world").is_err());
    }
}
