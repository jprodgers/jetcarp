//! What `jetcarp CMD --json` prints: the operation's output as pretty JSON with its keys sorted,
//! the way `serde_json::json!` printed the hand-built reports before the operations existed.

mod common;

use common::keys;
use jetcarp_api::op::{NoInput, Op, Quiet, to_pretty};
use jetcarp_api::ops::reference::*;
use jetcarp_api::ops::system::{SystemPaths, SystemUninstall};
use jetcarp_api::{Ctx, call};
use serde_json::{Value, json};

/// The JSON a command prints, read back, with its keys checked to be sorted at every level.
fn printed<T: serde::Serialize>(out: &T) -> Value {
    let text = to_pretty(out);
    let v: Value = serde_json::from_str(&text).unwrap();
    fn sorted(v: &Value) {
        match v {
            Value::Object(o) => {
                let k: Vec<&String> = o.keys().collect();
                let mut s = k.clone();
                s.sort();
                assert_eq!(k, s);
                o.values().for_each(sorted);
            }
            Value::Array(a) => a.iter().for_each(sorted),
            _ => {}
        }
    }
    sorted(&v);
    assert!(text.starts_with("{\n  \""), "{text}");
    v
}

#[test]
fn json_of_the_listing_commands() {
    let cx = Ctx::new();
    let rules = printed(&ReferenceRules::run(&cx, RulesIn::default(), &Quiet).unwrap());
    assert_eq!(keys(&rules), ["families", "rules"]);
    assert_eq!(
        rules["rules"][0],
        json!({"blurb": rules["rules"][0]["blurb"], "family": "Life-like", "name": "Life", "notation": "B3/S23"})
    );
    let tilings = printed(&ReferenceTilings::run(&cx, TilingsIn::default(), &Quiet).unwrap());
    assert_eq!(keys(&tilings), ["catalog", "families"]);
    let hex = tilings["families"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "hex")
        .unwrap();
    assert_eq!(hex["edge"], json!([6]));
    assert!(hex.get("error").is_none());
    let iso = printed(&ReferenceIsohedral::run(&cx, NoInput {}, &Quiet).unwrap());
    assert_eq!(iso["types"].as_array().unwrap().len(), 81);
    let pal = printed(&ReferencePalettes::run(&cx, NoInput {}, &Quiet).unwrap());
    assert_eq!(
        keys(&pal["palettes"][0]),
        ["background", "name", "stage", "stops"]
    );
}

#[test]
fn json_of_paths_uninstall_and_calls_by_name() {
    let cx = Ctx::new();
    let p = printed(&SystemPaths::run(&cx, NoInput {}, &Quiet).unwrap());
    assert_eq!(
        p["data"],
        json!(jetcarp_core::paths::data_dir().display().to_string())
    );
    let u = printed(&SystemUninstall::run(&cx, Default::default(), &Quiet).unwrap());
    assert_eq!(
        keys(&u),
        ["done", "kept_data", "kept_settings", "output", "steps"]
    );
    assert_eq!(u["done"], false);
    // `jetcarp api call` prints the same JSON as `--json` does.
    let c = call(&cx, "system.paths", json!({}), &Quiet).unwrap();
    assert_eq!(c.value, p);
}
