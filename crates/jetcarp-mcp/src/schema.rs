//! Tool input schemas made from the operations' own input types, and tool arguments read back
//! into those types.
//!
//! A tool's `inputSchema` is put together from the JSON Schemas of the operations it calls
//! (`jetcarp_api::op::input_schema`): the properties of one operation's input, or one property
//! per action whose value is that action's input. The arguments are then read with serde into the
//! same input types, so what `tools/list` shows, what the command line takes and what scripts
//! pass cannot drift apart.
//!
//! Small models often send numbers as text (`"10"`), `"true"` for true, a list as one
//! comma-separated text, or a nested object as its JSON text. [`coerce`] turns those into what
//! the schema asks for before the arguments are read.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

/// An operation's input schema as a tool (or a part of one) shows it: without the Rust type's
/// `title`, and with `description` replaced when one is given.
pub fn op(mut s: Value, description: Option<&str>) -> Value {
    if let Some(o) = s.as_object_mut() {
        o.remove("title");
        if let Some(d) = description {
            o.insert("description".into(), json!(d));
        }
    }
    s
}

/// The properties of an operation's input schema.
pub fn props(s: &Value) -> Map<String, Value> {
    s.get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// `s` without these properties (and without them in `required`).
pub fn without(mut s: Value, keys: &[&str]) -> Value {
    if let Some(p) = s.get_mut("properties").and_then(Value::as_object_mut) {
        for k in keys {
            p.remove(*k);
        }
    }
    if let Some(r) = s.get_mut("required").and_then(Value::as_array_mut) {
        r.retain(|k| !keys.iter().any(|x| k == x));
        if r.is_empty() {
            s.as_object_mut().map(|o| o.remove("required"));
        }
    }
    s
}

/// `s` with property `key`'s description replaced.
pub fn describe(mut s: Value, key: &str, description: &str) -> Value {
    if let Some(p) = s.get_mut("properties").and_then(|p| p.get_mut(key)) {
        p["description"] = json!(description);
    }
    s
}

/// A tool's input schema: an object with these properties, nothing else.
pub fn object(props: Map<String, Value>, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}

/// Properties from `(name, schema)` pairs.
pub fn named(items: Vec<(&str, Value)>) -> Map<String, Value> {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// A text property limited to these words.
pub fn words(values: &[&str], description: &str) -> Value {
    json!({"type": "string", "enum": values, "description": description})
}

fn has_type(s: &Value, t: &str) -> bool {
    match s.get("type") {
        Some(Value::String(x)) => x == t,
        Some(Value::Array(a)) => a.iter().any(|x| x == t),
        _ => false,
    }
}

fn only_type(s: &Value, t: &str) -> bool {
    match s.get("type") {
        Some(Value::String(x)) => x == t,
        Some(Value::Array(a)) => a.len() == 1 && a[0] == t,
        _ => false,
    }
}

/// Turn the forms small models send into what `schema` asks for: numbers and booleans given as
/// text, a whole number given as `10.0`, a list given as comma-separated text, and an object
/// given as its JSON text. Values the schema allows as they are stay as they are.
pub fn coerce(schema: &Value, v: &mut Value) {
    if has_type(schema, "string") && v.is_string() {
        return;
    }
    match v {
        Value::String(s) => {
            let t = s.trim();
            if has_type(schema, "integer") {
                if let Ok(n) = t.parse::<i64>() {
                    *v = json!(n);
                    return;
                }
                if let Ok(n) = t.parse::<u64>() {
                    *v = json!(n);
                    return;
                }
            }
            if has_type(schema, "number")
                && let Ok(n) = t.parse::<f64>()
                && n.is_finite()
            {
                *v = json!(n);
                return;
            }
            if has_type(schema, "boolean") && (t == "true" || t == "false") {
                *v = json!(t == "true");
                return;
            }
            if has_type(schema, "object")
                && t.starts_with('{')
                && let Ok(o @ Value::Object(_)) = serde_json::from_str::<Value>(t)
            {
                *v = o;
                coerce(schema, v);
                return;
            }
            if has_type(schema, "array") {
                if t.starts_with('[')
                    && let Ok(a @ Value::Array(_)) = serde_json::from_str::<Value>(t)
                {
                    *v = a;
                    coerce(schema, v);
                    return;
                }
                let items_text = schema.get("items").is_some_and(|i| only_type(i, "string"));
                if items_text {
                    *v = Value::Array(
                        t.split(',')
                            .map(str::trim)
                            .filter(|x| !x.is_empty())
                            .map(|x| json!(x))
                            .collect(),
                    );
                }
            }
        }
        Value::Number(n) if only_type(schema, "integer") => {
            if let Some(f) = n.as_f64()
                && n.as_i64().is_none()
                && n.as_u64().is_none()
                && f.fract() == 0.0
                && f.abs() < 9.0e15
            {
                *v = json!(f as i64);
            }
        }
        Value::Object(o) => {
            if let Some(p) = schema.get("properties").and_then(Value::as_object) {
                for (k, x) in o.iter_mut() {
                    if let Some(ps) = p.get(k) {
                        coerce(ps, x);
                    }
                }
            }
        }
        Value::Array(a) => {
            if let Some(items) = schema.get("items") {
                for x in a.iter_mut() {
                    coerce(items, x);
                }
            }
        }
        _ => {}
    }
}

/// Read `v` (coerced to `schema` first; `null` counts as `{}`) into an operation's input type.
/// `what` names the part in the error (`world_edit cells`).
pub fn read<T: DeserializeOwned>(schema: &Value, what: &str, v: Value) -> Result<T, String> {
    let mut v = if v.is_null() { json!({}) } else { v };
    coerce(schema, &mut v);
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

/// The properties of `args` named `keys`, as one object (the arguments an action's input takes
/// when they sit beside other arguments at the top level of a tool).
pub fn pick(args: &Value, keys: &[&str]) -> Value {
    let mut o = Map::new();
    if let Some(a) = args.as_object() {
        for k in keys {
            if let Some(v) = a.get(*k).filter(|v| !v.is_null()) {
                o.insert(k.to_string(), v.clone());
            }
        }
    }
    Value::Object(o)
}

/// Refuse arguments the tool's schema does not list, naming the ones it does.
pub fn check_known(tool: &str, schema: &Value, args: &Value) -> Result<(), String> {
    let p = props(schema);
    if let Some(a) = args.as_object() {
        for k in a.keys() {
            if !p.contains_key(k) {
                let mut names: Vec<&String> = p.keys().collect();
                names.sort();
                return Err(format!(
                    "{tool} has no argument `{k}`. Its arguments: {}",
                    names
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    Ok(())
}

/// Which one of `keys` the arguments give (none, or more than one, is an error naming them).
pub fn one_of<'a>(tool: &str, args: &Value, keys: &[&'a str]) -> Result<Option<&'a str>, String> {
    let given: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| args.get(*k).is_some_and(|v| !v.is_null()))
        .collect();
    match given.len() {
        0 => Ok(None),
        1 => Ok(Some(given[0])),
        _ => Err(format!(
            "{tool}: give one of {}, not {}",
            keys.iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", "),
            given
                .iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_reads_the_forms_small_models_send() {
        let s = json!({"type": "object", "properties": {
            "n": {"type": "integer"}, "f": {"type": "number"}, "b": {"type": "boolean"},
            "t": {"type": ["integer", "string"]}, "l": {"type": "array", "items": {"type": "string"}},
            "o": {"type": "object", "properties": {"x": {"type": "integer"}}},
        }});
        let mut v = json!({"n": "10", "f": "0.5", "b": "true", "t": "7", "l": "a, b", "o": "{\"x\": \"3\"}"});
        coerce(&s, &mut v);
        assert_eq!(
            v,
            json!({"n": 10, "f": 0.5, "b": true, "t": "7", "l": ["a", "b"], "o": {"x": 3}})
        );
        let mut v = json!({"n": 4.0});
        coerce(&s, &mut v);
        assert_eq!(v, json!({"n": 4}));
        // What cannot be read stays, for serde's own error.
        let mut v = json!({"n": "ten"});
        coerce(&s, &mut v);
        assert_eq!(v, json!({"n": "ten"}));
    }

    #[test]
    fn without_drops_properties_and_their_requirement() {
        let s = json!({"type": "object", "properties": {"a": {}, "b": {}}, "required": ["a"]});
        assert_eq!(
            without(s, &["a"]),
            json!({"type": "object", "properties": {"b": {}}})
        );
    }
}
