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

/// A catalog holding Life's glider, in `dir`.
fn glider_catalog(dir: &std::path::Path) -> std::path::PathBuf {
    use jetcarp_core::library::{DiscoveredBy, Library, Provenance, record_from_object};
    use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
    use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
    let mut w = World::new(TilingSpec::rect("square", 30, 30, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells {
            cells: place_rows(&g, ".o./..o/ooo", 10, 10),
        }),
    })
    .unwrap();
    let lib = Library::open(dir.join("lib"));
    for o in find_objects(&w, &ObjectOptions::default())
        .unwrap()
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
    {
        let prov = Provenance {
            source: "cli test".into(),
            ..Default::default()
        };
        let rec = record_from_object(&w, w.cas[0].id, o, prov, DiscoveredBy::User).unwrap();
        lib.add(rec).unwrap();
    }
    dir.join("lib")
}

#[test]
fn library_media_and_picture_commands_as_json() {
    let dir =
        jetcarp_core::paths::output_dir().join(format!("cli-json-lib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = glider_catalog(&dir);
    let l = lib.to_str().unwrap();
    let p = |name: &str| dir.join(name).display().to_string();
    // Commands that printed JSON before print the same shapes: info in the record's own key
    // order, the list as an array of records.
    let o = jetcarp(&["library", "--library", l, "info", "--json"]);
    let text = String::from_utf8(o.stdout).unwrap();
    assert!(text.starts_with("{\n  \"path\": "), "{text}");
    let list = json_of(&["--json", "library", "--library", l, "list"]);
    assert_eq!(list[0]["name"], "glider");
    // New: promotion, a status change, a drawing.
    let add = json_of(&["library", "--library", l, "promote", "glider", "--json"]);
    assert_eq!(add["source"], "pattern");
    let entry = add["entry"].as_str().unwrap().to_string();
    assert_eq!(entry, format!("find:{}", list[0]["id"].as_str().unwrap()));
    let entries = json_of(&["library", "--library", l, "quickstart", "--json"]);
    assert_eq!(entries[0]["id"], entry.as_str());
    let set = format!("{entry}=rejected");
    let st = json_of(&[
        "--json",
        "library",
        "--library",
        l,
        "quickstart",
        "--set",
        &set,
    ]);
    assert_eq!(
        (&st["status"], &st["shown"]),
        (&json!("rejected"), &json!(false))
    );
    let r = json_of(&["library", "--library", l, "render", "glider", "--json"]);
    assert_eq!(r["phases"].as_array().unwrap().len(), 4);
    // Media: the catalog's report as before; the text with its counts on stderr.
    let m = json_of(&[
        "media",
        "--library",
        l,
        "render",
        "--pattern",
        "glider",
        "--no-frames",
        "--json",
    ]);
    assert_eq!(m["done"][0]["target_kind"], "pattern");
    let o = jetcarp(&[
        "media",
        "--library",
        l,
        "render",
        "--pattern",
        "glider",
        "--no-frames",
    ]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("[alone] up to date: "));
    assert!(String::from_utf8_lossy(&o.stderr).contains("0 rendered, 1 up to date, 0 failed"));
    // Site export: the report in its own key order.
    let o = jetcarp(&[
        "site",
        "--library",
        l,
        "export",
        "--to",
        &p("site"),
        "--no-render",
        "--json",
    ]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("{\n  \"dir\": "));
    // Pictures, which had no JSON before.
    let s = json_of(&[
        "sheet",
        "--png",
        &p("w.png"),
        "--count",
        "4",
        "--thumb",
        "20",
        "--width",
        "11",
        "--json",
    ]);
    assert_eq!(s["png"], p("w.png").as_str());
    let iso = json_of(&[
        "--json",
        "isohedral",
        "--png",
        &p("iso.png"),
        "--thumb",
        "12",
    ]);
    assert!(iso["width"].as_u64().unwrap() > 0);
    let scene = p("soup.json");
    let mut sc = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
        "square",
        16,
        12,
        jetcarp_core::Boundary::Wrap,
    ));
    sc.cas.push(jetcarp_core::CaSpec::new("B3/S23"));
    std::fs::write(&scene, sc.to_json()).unwrap();
    let w = json_of(&[
        "wallpaper",
        &scene,
        "--size",
        "40x30",
        "--out",
        &p("wp.png"),
        "--json",
    ]);
    assert_eq!(
        (&w["files"][0]["width"], &w["files"][0]["height"]),
        (&json!(40), &json!(30))
    );
    let q = json_of(&["quickstart", "--filter", "langton", "--json"]);
    assert_eq!(q["examples"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}
