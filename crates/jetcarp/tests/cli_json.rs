//! The global `--json` flag and `jetcarp api`, run through the real program.

use serde_json::{Value, json};
use std::process::{Command, Output};

/// Run `jetcarp ARGS` (with the test's own folders when the tests were given them).
fn jetcarp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jetcarp"))
        .args(args)
        .output()
        .expect("run jetcarp")
}

fn json_of(args: &[&str]) -> Value {
    let o = jetcarp(args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", String::from_utf8_lossy(&o.stdout)))
}

#[test]
fn json_before_or_after_the_subcommand() {
    let after = json_of(&["paths", "--json"]);
    let before = json_of(&["--json", "paths"]);
    assert_eq!(after, before);
    let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["data", "output", "settings"]);
    // Without --json: the text, as before.
    let text = String::from_utf8(jetcarp(&["paths"]).stdout).unwrap();
    assert!(text.starts_with("settings: "), "{text}");
    assert!(text.contains("\ndata:     "), "{text}");
    let rules = json_of(&["rules", "--json"]);
    assert_eq!(rules["rules"][0]["notation"], "B3/S23");
    let tilings = json_of(&["--json", "tilings"]);
    assert!(tilings["families"].as_array().unwrap().len() > 20);
    let iso = json_of(&["isohedral", "--json"]);
    assert_eq!(iso["types"][0]["id"], "ih01");
    // Commands with no result to print say so.
    let o = jetcarp(&["--json", "mcp"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("has no JSON output"));
}

#[test]
fn run_and_verify_as_json() {
    use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
    let dir =
        jetcarp_core::paths::output_dir().join(format!("cli-json-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("soup.json");
    let mut sc = Scene::new(TilingSpec::rect("square", 24, 24, Boundary::Wrap));
    sc.rng_state = 4;
    sc.cas
        .push(CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }));
    std::fs::write(&scene, sc.to_json()).unwrap();
    let saved = dir.join("after.json");
    let (s, sv) = (scene.to_str().unwrap(), saved.to_str().unwrap());
    let r = json_of(&["run", s, "--gens", "12", "--save", sv, "--json"]);
    assert_eq!(r["generation"], 12);
    assert_eq!(r["scene"], s);
    let hash = r["hash"].as_str().unwrap().to_string();
    let v = json_of(&["verify", sv, "--json"]);
    assert_eq!(v, json!({"generation": 12, "hash": hash}));
    // --expect still prints the summary, then fails.
    let o = jetcarp(&["run", s, "--gens", "12", "--expect", "deadbeef"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains(&format!("hash:       {hash}")));
    assert!(String::from_utf8_lossy(&o.stderr).contains("hash mismatch: expected deadbeef"));
    // The same run by name.
    let input = json!({"scene": s, "gens": 12}).to_string();
    let c = json_of(&["api", "call", "scene.run", &input]);
    assert_eq!(c["hash"], hash.as_str());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn api_lists_describes_and_calls_operations() {
    let ops = json_of(&["api", "list"]);
    let names: Vec<&str> = ops
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    for n in ["system.paths", "scene.run", "world.new", "files.list"] {
        assert!(names.contains(&n), "{n}");
    }
    let s = json_of(&["api", "schema", "world.step"]);
    assert_eq!(
        s["input"]["properties"]["stop_when_settled"]["type"],
        "boolean"
    );
    let w = json_of(&[
        "api",
        "call",
        "world.new",
        r#"{"tiling": "hex", "width": 12, "height": 12, "cas": ["Life"]}"#,
    ]);
    assert_eq!(w["status"]["tiling"]["id"], "hex");
    // Errors come back as JSON with a kind, and a failing status.
    let o = jetcarp(&[
        "api",
        "call",
        "reference.rules",
        r#"{"family": "nonesuch"}"#,
    ]);
    assert!(!o.status.success());
    let e: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(e["error"]["kind"], "not_found");
    let o = jetcarp(&["api", "call", "reference.palettes", "--text"]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("Ember "));
}
