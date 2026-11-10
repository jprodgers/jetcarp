//! The operation registry: names, flags, schemas simple enough for MCP clients, and calls by
//! name with JSON in and out.

mod common;

use common::fails;
use jetcarp_api::ops::OPS;
use jetcarp_api::world::{Host, Session};
use jetcarp_api::{Ctx, ErrorKind, Quiet, call, call_json, call_json_on, find};
use serde_json::{Map, Value, json};

#[test]
fn every_operation_is_named_once_and_found_by_name() {
    let mut names: Vec<&str> = OPS.iter().map(|o| o.name()).collect();
    assert!(names.len() >= 49, "{names:?}");
    for o in OPS {
        let n = o.name();
        let (group, rest) = n.split_once('.').expect(n);
        assert!(
            [
                "system",
                "reference",
                "files",
                "scene",
                "world",
                "library",
                "quickstart",
                "media",
                "site",
                "wallpaper",
                "sheet"
            ]
            .contains(&group),
            "{n}"
        );
        assert!(
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{n}"
        );
        assert!(o.summary().ends_with('.') && o.summary().len() > 30, "{n}");
        // World operations, and a wallpaper of the world.
        assert_eq!(
            o.on_world(),
            group == "world" || n == "wallpaper.export",
            "{n}"
        );
        let found = find(n).unwrap();
        assert_eq!((found.name(), found.on_world()), (n, o.on_world()));
    }
    names.sort();
    names.dedup();
    assert_eq!(names.len(), OPS.len(), "duplicate names");
    // Reading operations say so; the ones that write files or change worlds do not.
    for (n, ro) in [
        ("system.paths", true),
        ("system.uninstall", false),
        ("reference.rules", true),
        ("scene.save", false),
        ("scene.run", false),
        ("world.status", true),
        ("world.read", true),
        ("world.draw", false),
        ("world.step", false),
        ("library.query", true),
        ("library.get", true),
        ("library.update", false),
        ("quickstart.list", true),
        ("quickstart.add", false),
        ("media.list", true),
        ("media.render", false),
        ("site.export", false),
        ("wallpaper.export", false),
    ] {
        assert_eq!(find(n).unwrap().read_only(), ro, "{n}");
    }
}

/// Every object in a schema, depth first.
fn objects<'a>(v: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match v {
        Value::Object(o) => {
            out.push(o);
            o.values().for_each(|x| objects(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| objects(x, out)),
        _ => {}
    }
}

#[test]
fn schemas_are_simple_enough_for_mcp_clients() {
    for o in OPS {
        let n = o.name();
        let input = o.input_schema();
        let output = o.output_schema();
        // Inputs: an object of described properties, unknown keys refused.
        assert_eq!(input["type"], "object", "{n} input");
        assert_eq!(input["additionalProperties"], false, "{n} input");
        if let Some(props) = input["properties"].as_object() {
            for (k, p) in props {
                assert!(p["description"].is_string(), "{n}.{k} has no description");
                let d = p["description"].as_str().unwrap();
                assert!(!d.contains('\n'), "{n}.{k}: wrapped description");
            }
        }
        for (which, s) in [("input", &input), ("output", &output)] {
            let text = s.to_string();
            for bad in ["$ref", "$defs", "\"definitions\"", "$schema"] {
                assert!(!text.contains(bad), "{n} {which} has {bad}: {text}");
            }
            let mut all = vec![];
            objects(s, &mut all);
            for obj in all {
                match obj.get("type") {
                    Some(Value::String(t)) => assert_ne!(t, "null", "{n} {which}"),
                    Some(Value::Array(ts)) => {
                        assert!(!ts.iter().any(|t| t == "null"), "{n} {which}: {ts:?}")
                    }
                    _ => {}
                }
                if obj.get("type").is_some_and(|t| t == "integer") {
                    assert!(obj.get("format").is_none(), "{n} {which}: integer format");
                }
                assert!(
                    obj.get("default").is_none_or(|d| !d.is_null()),
                    "{n} {which}: null default"
                );
            }
        }
    }
    // A few shapes in detail.
    let s = find("world.draw").unwrap().input_schema();
    assert_eq!(s["properties"]["ca"]["type"], json!(["integer", "string"]));
    assert_eq!(s["properties"]["pattern"]["type"], "string");
    assert_eq!(s["properties"]["erase"]["type"], "boolean");
    assert!(
        s.get("required")
            .is_none_or(|r| r.as_array().unwrap().is_empty())
    );
    let s = find("scene.run").unwrap().input_schema();
    assert_eq!(s["required"], json!(["scene"]));
    assert_eq!(s["properties"]["max_side"]["default"], 1600);
    let s = find("files.list").unwrap().output_schema();
    assert_eq!(s["anyOf"].as_array().unwrap().len(), 2);
}

#[test]
fn calls_by_name_take_json_and_name_their_errors() {
    let cx = Ctx::new();
    let v = call_json(&cx, "reference.rules", json!({"family": "generations"})).unwrap();
    let rules = v["rules"].as_array().unwrap();
    assert!(!rules.is_empty());
    assert!(rules.iter().all(|r| r["family"] == "Generations"), "{v}");
    // `null` is no input.
    assert_eq!(
        call_json(&cx, "reference.palettes", Value::Null).unwrap(),
        call_json(&cx, "reference.palettes", json!({})).unwrap()
    );
    // Unknown fields and wrong types are refused, naming the operation.
    let e = fails(
        call_json(&cx, "reference.rules", json!({"bogus": 1})),
        ErrorKind::Invalid,
    );
    assert!(
        e.message
            .starts_with("reference.rules: unknown field `bogus`"),
        "{e}"
    );
    fails(
        call_json(&cx, "reference.tilings", json!({"limit": "many"})),
        ErrorKind::Invalid,
    );
    let e = fails(call_json(&cx, "no.such", json!({})), ErrorKind::NotFound);
    assert!(e.hint.unwrap().contains("world.step"));
    // The operation's own errors come through with their kinds.
    fails(
        call_json(&cx, "reference.rules", json!({"family": "nonesuch"})),
        ErrorKind::NotFound,
    );
    // Text as well as JSON.
    let c = call(&cx, "system.paths", json!({}), &Quiet).unwrap();
    assert!(c.text.starts_with("settings: "), "{}", c.text);
    assert!(c.check.is_ok());
}

#[test]
fn world_operations_act_on_a_session_or_a_host() {
    // Without a host: the context's own session, kept between calls.
    let cx = Ctx::new();
    call_json(
        &cx,
        "world.new",
        json!({"tiling": "hex", "width": 20, "height": 16, "cas": ["Brian's Brain"]}),
    )
    .unwrap();
    let st = call_json(&cx, "world.status", json!({})).unwrap();
    assert_eq!(st["status"]["tiling"]["id"], "hex");
    assert_eq!(st["status"]["cas"][0]["rule"], "B2/S/C3");
    call_json(&cx, "world.step", json!({"generations": 3})).unwrap();
    let st = call_json(&cx, "world.status", json!({})).unwrap();
    assert_eq!(st["status"]["generation"], 3);
    // With a host: that host's world, and not the session.
    let mut h = Session::new();
    call_json_on(
        &cx,
        &mut h,
        "world.new",
        json!({"width": 16, "height": 16, "cas": [{"rule": "Life", "seed": "empty"}]}),
    )
    .unwrap();
    assert_eq!(h.world().topo.len(), 256);
    let st = call_json(&cx, "world.status", json!({})).unwrap();
    assert_eq!(st["status"]["generation"], 3);
    // A plain operation ignores the host.
    assert!(call_json_on(&cx, &mut h, "reference.seeds", json!({})).is_ok());
}
