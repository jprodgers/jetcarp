//! The global `--json` flag and `jetcarp api`, run through the real program.

use serde_json::{Value, json};
use std::process::{Command, Output};

/// The tests' own folders: the ones the tests were given (`JETCARP_OUT`, `JETCARP_DATA_DIR`,
/// `JETCARP_CONFIG_DIR`, as CI sets them), else folders in Cargo's scratch folder for this target,
/// so a local `cargo test` never writes to the user's own folders.
fn test_dir(var: &str, name: &str) -> std::path::PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                .join("cli-tests")
                .join(format!("{name}-{}", std::process::id()))
        })
}

/// The output root the program runs with.
fn out_root() -> std::path::PathBuf {
    test_dir("JETCARP_OUT", "out")
}

/// Run `jetcarp ARGS` with the tests' own folders.
fn jetcarp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jetcarp"))
        .args(args)
        .env("JETCARP_OUT", out_root())
        .env("JETCARP_DATA_DIR", test_dir("JETCARP_DATA_DIR", "data"))
        .env(
            "JETCARP_CONFIG_DIR",
            test_dir("JETCARP_CONFIG_DIR", "config"),
        )
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
    assert_eq!(
        keys,
        ["bundled_packs", "data", "output", "runtime", "settings"]
    );
    // Without --json: the text, as before, with the runtime folder last (#85).
    let text = String::from_utf8(jetcarp(&["paths"]).stdout).unwrap();
    assert!(text.starts_with("settings: "), "{text}");
    assert!(text.contains("\ndata:     "), "{text}");
    assert!(text.contains("\nruntime:  "), "{text}");
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
    let dir = out_root().join(format!("cli-json-test-{}", std::process::id()));
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
    assert_eq!(hash.len(), 64);
    assert_eq!(
        v,
        json!({"generation": 12, "hash": hash, "hash_version": 2, "hash_v2": hash})
    );
    // --expect still prints the summary, then fails.
    let zeros = "0".repeat(64);
    let o = jetcarp(&["run", s, "--gens", "12", "--expect", &zeros]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains(&format!("hash:       {hash}")));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(&format!("hash mismatch: expected {zeros}"))
    );
    // The same run by name.
    let input = json!({"scene": s, "gens": 12}).to_string();
    let c = json_of(&["api", "call", "scene.run", &input]);
    assert_eq!(c["hash"], hash.as_str());
    let _ = std::fs::remove_dir_all(dir);
}

/// `jetcarp run` steps a large world on the GPU by default (the GPU setting is automatic) and
/// on the CPU with `--cpu`: the same hash either way, which `--expect` checks.
#[test]
fn run_gives_one_hash_on_the_cpu_and_by_default() {
    use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
    let dir = out_root().join(format!("cli-gpu-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("big.json");
    // 400 x 400 cells of Life: 1.28M neighbour reads a generation, over the automatic threshold.
    let mut sc = Scene::new(TilingSpec::rect("square", 400, 400, Boundary::Wrap));
    sc.rng_state = 9;
    sc.cas
        .push(CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 }));
    std::fs::write(&scene, sc.to_json()).unwrap();
    let s = scene.to_str().unwrap();
    let cpu = json_of(&["run", s, "--gens", "60", "--cpu", "--json"]);
    let hash = cpu["hash"].as_str().unwrap().to_string();
    assert!(cpu["backend"].as_str().unwrap().starts_with("cpu"), "{cpu}");
    let auto = json_of(&["run", s, "--gens", "60", "--expect", &hash, "--json"]);
    assert_eq!(auto["hash"], hash.as_str());
    // On the GPU when one is usable here (CI: JETCARP_GPU=software); on the CPU, with a note, when
    // there is none.
    let b = auto["backend"].as_str().unwrap();
    assert!(b.starts_with("gpu (") || b.contains("no GPU"), "{b}");
    let gpu = json_of(&[
        "run", s, "--gens", "60", "--gpu", "--expect", &hash, "--json",
    ]);
    assert_eq!(gpu["hash"], hash.as_str());
    // A setting of off keeps it on the CPU; --gpu still wins.
    let off = Command::new(env!("CARGO_BIN_EXE_jetcarp"))
        .args(["run", s, "--gens", "60", "--expect", &hash, "--json"])
        .env("JETCARP_OUT", out_root())
        .env("JETCARP_DATA_DIR", test_dir("JETCARP_DATA_DIR", "data"))
        .env(
            "JETCARP_CONFIG_DIR",
            test_dir("JETCARP_CONFIG_DIR", "config"),
        )
        .env("JETCARP_GPU", "off")
        .output()
        .unwrap();
    assert!(off.status.success());
    let off: Value = serde_json::from_slice(&off.stdout).unwrap();
    assert!(off["backend"].as_str().unwrap().starts_with("cpu"), "{off}");
    // --gpu and --cpu together make no sense.
    assert!(
        !jetcarp(&["run", s, "--gens", "1", "--gpu", "--cpu"])
            .status
            .success()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// With the GPU simulation off, doctor tests no adapter: it says so, and writes no self-test
/// results (#84). It bites only where there is a hardware adapter to test (not on CI's lavapipe).
#[test]
fn doctor_with_the_gpu_off_runs_no_self_test() {
    let cfg =
        test_dir("JETCARP_CONFIG_DIR", "config").join(format!("gpu-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_jetcarp"))
        .args(["doctor", "--json"])
        .env("JETCARP_OUT", out_root())
        .env("JETCARP_DATA_DIR", test_dir("JETCARP_DATA_DIR", "data"))
        .env("JETCARP_CONFIG_DIR", &cfg)
        .env("JETCARP_GPU", "off")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let d: Value = serde_json::from_slice(&o.stdout).unwrap();
    for c in d["gpu_policy"]["choices"].as_array().into_iter().flatten() {
        assert!(
            c["self_test"].is_null(),
            "a self-test with the GPU off: {c}"
        );
    }
    assert!(
        !cfg.join("gpu-checks.json").exists(),
        "doctor ran the GPU self-test with the GPU off"
    );
    let _ = std::fs::remove_dir_all(&cfg);
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
    use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, record_from_object};
    use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
    use jetcarp_store::Library;
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
    let dir = out_root().join(format!("cli-json-lib-{}", std::process::id()));
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
    // New: promotion, hiding an entry, a drawing.
    let add = json_of(&["library", "--library", l, "promote", "glider", "--json"]);
    assert_eq!(add["source"], "pattern");
    let entry = add["entry"].as_str().unwrap().to_string();
    assert_eq!(entry, format!("find:{}", list[0]["id"].as_str().unwrap()));
    let entries = json_of(&["library", "--library", l, "quickstart", "--json"]);
    assert_eq!(entries[0]["id"], entry.as_str());
    let st = json_of(&[
        "--json",
        "library",
        "--library",
        l,
        "quickstart",
        "--hide",
        &entry,
    ]);
    assert_eq!((&st["id"], &st["hidden"]), (&json!(entry), &json!(true)));
    let hidden = json_of(&[
        "library",
        "--library",
        l,
        "quickstart",
        "--hidden",
        "--json",
    ]);
    assert_eq!(hidden.as_array().unwrap().len(), 1);
    // The built-in examples are entries too.
    let b = json_of(&[
        "library",
        "--library",
        l,
        "quickstart",
        "--source",
        "builtin",
        "--json",
    ]);
    assert!(b.as_array().unwrap().iter().any(|e| e["id"] == "life"));
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

#[test]
fn search_and_jobs_commands_as_json() {
    let dir = out_root().join(format!("cli-json-jobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = dir.join("lib");
    let l = lib.to_str().unwrap();
    // A hunt to its budget, carried on to its end.
    let h = json_of(&[
        "search",
        "hunt",
        "--library",
        l,
        "--rules",
        "B3/S23",
        "--soups",
        "60",
        "--limit",
        "20",
        "--threads",
        "2",
        "--quiet",
        "--json",
    ]);
    assert_eq!(
        (&h["kind"], &h["status"], &h["stopped"]),
        (&json!("hunt"), &json!("paused"), &json!("budget"))
    );
    assert_eq!(h["progress"]["soups_done"], 20);
    let job = h["job"].to_string();
    let r = json_of(&["jobs", "--library", l, "resume", &job, "--quiet", "--json"]);
    assert_eq!(
        (&r["status"], &r["progress"]["soups_done"]),
        (&json!("done"), &json!(60))
    );
    // Listed, followed, and reported on.
    let list = json_of(&["--json", "jobs", "--library", l, "list"]);
    assert_eq!(list["jobs"][0]["job"].to_string(), job);
    let st = json_of(&[
        "jobs",
        "--library",
        l,
        "status",
        &job,
        "--finds",
        "2",
        "--json",
    ]);
    assert_eq!(
        (&st["status"], &st["kind"]),
        (&json!("done"), &json!("hunt"))
    );
    assert!(st["finds"]["newest"].as_array().is_some(), "{st}");
    let text =
        String::from_utf8(jetcarp(&["jobs", "--library", l, "status", &job]).stdout).unwrap();
    assert!(
        text.starts_with(&format!("hunt {job}: 60/60 soups")),
        "{text}"
    );
    let o = jetcarp(&["jobs", "--library", l, "report", &job]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("# Hunt "));
    let md = dir.join("report.md");
    let o = jetcarp(&[
        "jobs",
        "--library",
        l,
        "report",
        &job,
        "--md",
        md.to_str().unwrap(),
    ]);
    assert!(o.status.success());
    assert!(
        std::fs::read_to_string(&md)
            .unwrap()
            .contains("## Hunts: per hunted rule")
    );
    // A census with its files; a collision of what the hunt found.
    let csv = dir.join("census.csv");
    let c = json_of(&[
        "search",
        "census",
        "--library",
        l,
        "--tilings",
        "hex",
        "--size",
        "16x16",
        "--rules",
        "B2/S34 B3/S23",
        "--soups",
        "2",
        "--gens",
        "50",
        "--csv",
        csv.to_str().unwrap(),
        "--quiet",
        "--json",
    ]);
    assert_eq!(c["result"]["rules"].as_array().unwrap().len(), 2);
    // The program prints the path normalised (on Windows, with `\` throughout).
    assert_eq!(std::path::Path::new(c["files"][0].as_str().unwrap()), csv);
    let k = json_of(&[
        "search",
        "collide",
        "block",
        "glider",
        "--library",
        l,
        "--offsets=-2..2",
        "--gens",
        "128",
        "--quiet",
        "--json",
    ]);
    assert_eq!(k["kind"], "collide");
    assert!(k["result"]["report"]["runs"].as_array().unwrap().len() > 4);
    // Stopped (it is done), then removed.
    let s = json_of(&["jobs", "--library", l, "stop", &job, "--json"]);
    assert_eq!(s["stopping"], false);
    let rm = json_of(&["jobs", "--library", l, "remove", &job, "--json"]);
    assert_eq!(rm["job"].to_string(), job);
    let list = json_of(&["jobs", "--library", l, "list", "--json"]);
    assert_eq!(list["jobs"].as_array().unwrap().len(), 2);
    // The commands that `search` and `jobs` replace are gone.
    for old in [
        &["hunt", "--list"][..],
        &["evolve", "--help"],
        &["census", "--help"],
        &["movers", "list"],
        &["ants", "list"],
        &["library", "jobs"],
        &["library", "search", "--rules", "B3/S23"],
    ] {
        assert!(!jetcarp(old).status.success(), "{old:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// An explore job's runs out with their measures: as text, CSV on standard output or in a file,
/// and JSON; a refit without reviews is refused.
#[test]
fn explore_runs_with_measures_as_csv_and_json() {
    let dir = out_root().join(format!("cli-explore-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = dir.join("lib");
    let l = lib.to_str().unwrap();
    let e = json_of(&[
        "search",
        "explore",
        "--library",
        l,
        "--families",
        "jaca",
        "--tilings",
        "square,hex",
        "--screen-cells",
        "400",
        "--screen-gens",
        "64",
        "--confirm-cells",
        "400",
        "--confirm-gens",
        "64",
        "--limit",
        "40",
        "--threads",
        "2",
        "--quiet",
        "--json",
    ]);
    let job = e["job"].to_string();
    let o = jetcarp(&[
        "jobs",
        "--library",
        l,
        "runs",
        &job,
        "--limit",
        "0",
        "--measures",
        "sign_pos,sign_lost",
        "--format",
        "csv",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8(o.stdout).unwrap();
    let mut lines = text.lines();
    let head = lines.next().unwrap();
    assert!(
        head.starts_with("id,stage,parent,parent_job,parent_search,family,"),
        "{head}"
    );
    assert!(head.ends_with(",score,sign_pos,sign_lost"), "{head}");
    let n = lines.count();
    assert!(n >= 40, "{n} runs");
    let out = dir.join("runs.csv");
    let w = json_of(&[
        "jobs",
        "--library",
        l,
        "runs",
        &job,
        "--limit",
        "0",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(w["written"].as_u64(), Some(n as u64));
    assert_eq!(
        std::fs::read_to_string(&out).unwrap().lines().count(),
        n + 1
    );
    let j = json_of(&[
        "jobs",
        "--library",
        l,
        "runs",
        &job,
        "--limit",
        "3",
        "--measures",
        "all",
        "--format",
        "json",
    ]);
    assert_eq!(j["runs"].as_array().unwrap().len(), 3);
    assert!(j["runs"][0]["measures"]["sign_sorted"].is_number(), "{j}");
    let o = jetcarp(&["jobs", "--library", l, "runs", &job, "--measures", "all"]);
    let text = String::from_utf8(o.stdout).unwrap();
    assert!(
        text.lines().next().unwrap().contains("sign_trade"),
        "{text}"
    );
    let o = jetcarp(&["jobs", "--library", l, "refit", &job]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("reviewed runs"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A pack carries every run of a stopped explore search that never sealed (one under a million
/// runs keeps them all in its working database), its screened runs and its confirmation runs:
/// publishing seals them first (#35), and the imported library lists them.
#[test]
fn a_pack_carries_the_runs_of_a_search_never_sealed() {
    let dir = out_root().join(format!("cli-pack-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = |name: &str| dir.join(name).display().to_string();
    let l = p("lib");
    let e = json_of(&[
        "search",
        "explore",
        "--library",
        &l,
        "--families",
        "jaca",
        "--tilings",
        "square",
        "--screen-cells",
        "400",
        "--screen-gens",
        "64",
        "--confirm-cells",
        "400",
        "--confirm-gens",
        "64",
        "--limit",
        "40",
        "--threads",
        "2",
        "--cpu",
        "--quiet",
        "--json",
    ]);
    // On the processor alone, so it confirms elites on every system (a GPU's first batch would
    // screen a thousand runs and confirm none).
    let pr = &e["progress"];
    let (screened, confirmed) = (
        pr["screened"].as_u64().unwrap_or(0),
        pr["confirmed"].as_u64().unwrap_or(0),
    );
    assert!(screened >= 40 && confirmed > 0, "{e}");
    json_of(&["--json", "pack", "key", "--name", "Test Publisher"]);
    let o = jetcarp(&[
        "pack",
        "--library",
        &l,
        "publish",
        "--feed",
        "cli-search",
        "--to",
        &p("store"),
        "--no-media",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("sealed its newest"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    // Sealed first, so no note asks for a seal that was made, or names a command MCP lacks (#68).
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        !all.contains("jobs seal") && !all.contains("not sealed yet"),
        "{all}"
    );
    json_of(&[
        "--json",
        "pack",
        "--library",
        &p("lib2"),
        "import",
        &p("store"),
    ]);
    let jobs = json_of(&["--json", "jobs", "--library", &p("lib2"), "list"]);
    let job = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["kind"] == "explore")
        .unwrap_or_else(|| panic!("no explore job in {jobs}"));
    let id = job["job"].to_string();
    // `jobs runs` lists the screened runs unless asked for the confirmation runs (`--stage 2`).
    for (stage, want) in [("1", screened), ("2", confirmed)] {
        let o = jetcarp(&[
            "jobs",
            "--library",
            &p("lib2"),
            "runs",
            &id,
            "--limit",
            "0",
            "--stage",
            stage,
            "--format",
            "csv",
        ]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let n = String::from_utf8(o.stdout).unwrap().lines().count() - 1;
        assert_eq!(
            n as u64, want,
            "every run of stage {stage} of the search, in the imported library"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A running explore search is refused by an export, even when every run it has stored is
/// sealed (or it has stored none yet): whatever it stores next would be left out (#57). The
/// refused export leaves no `--to` folder behind, nor the hidden store beside it, and once the
/// search stops it exports.
#[test]
fn an_export_refuses_a_running_search_and_leaves_no_folder() {
    use jetcarp_search::jobs::Lease;
    use jetcarp_store::{JobStatus, Library};
    let dir = out_root().join(format!("cli-pack-running-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = |name: &str| dir.join(name).display().to_string();
    let l = p("lib");
    let e = json_of(&[
        "search",
        "explore",
        "--library",
        &l,
        "--families",
        "life_like",
        "--tilings",
        "square",
        "--screen-cells",
        "400",
        "--screen-gens",
        "32",
        "--confirm-cells",
        "400",
        "--confirm-gens",
        "32",
        "--limit",
        "10",
        "--threads",
        "2",
        "--cpu",
        "--quiet",
        "--json",
    ]);
    let job = e["job"].as_i64().unwrap();
    let o = jetcarp(&["jobs", "--library", &l, "seal", &job.to_string()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    json_of(&["--json", "pack", "key", "--name", "Test Publisher"]);
    // Another process runs it now: it holds the job's lock, and the job is marked running.
    let lib = Library::open(Library::file_for(dir.join("lib")));
    let lease = Lease::acquire(&lib, job).unwrap();
    lib.update_job(job, Some(JobStatus::Running), None, None)
        .unwrap();
    let export = || {
        jetcarp(&[
            "pack",
            "--library",
            &l,
            "export",
            "--to",
            &p("export"),
            "--feed",
            "cli-running",
            "--no-media",
        ])
    };
    let o = export();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "a running search exported: {err}");
    assert!(
        err.contains(&format!("explore job {job} is running"))
            && err.contains("every run it stores from now on"),
        "{err}"
    );
    assert!(
        !dir.join("export").exists(),
        "the refused export left its folder"
    );
    assert!(
        !dir.join(".export.store").exists(),
        "the refused export left its hidden store beside it"
    );
    // Stopped, it exports.
    lib.update_job(job, Some(JobStatus::Paused), None, None)
        .unwrap();
    lease.release(false);
    let o = export();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(dir.join("export").join("pack.json").is_file());
    let _ = std::fs::remove_dir_all(dir);
}

// `jetcarp pack`: a key, a publish of a small library, a check and an import, through the
// program, with the tests' own settings folder (the key is made there, never in ~/.ssh).
#[test]
fn pack_commands_publish_check_and_import() {
    let dir = out_root().join(format!("cli-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = glider_catalog(&dir);
    let l = lib.to_str().unwrap();
    let p = |name: &str| dir.join(name).display().to_string();
    let key = json_of(&["--json", "pack", "key", "--name", "Test Publisher"]);
    assert!(
        key["fingerprint"].as_str().unwrap().starts_with("SHA256:"),
        "{key}"
    );
    let out = json_of(&[
        "--json",
        "pack",
        "--library",
        l,
        "publish",
        "--feed",
        "cli-test",
        "--to",
        &p("store"),
        "--created",
        "2026-11-11T00:00:00Z",
    ]);
    assert_eq!(out["seq"], 1);
    assert!(out["manifest"].as_str().unwrap().starts_with("sha256:"));
    let check = json_of(&[
        "--json",
        "pack",
        "--library",
        &p("empty"),
        "check",
        &p("store"),
    ]);
    assert_eq!(check["signed"], "publisher");
    assert_eq!(check["dry_run"], true);
    let imp = json_of(&[
        "--json",
        "pack",
        "--library",
        &p("lib2"),
        "import",
        &p("store"),
    ]);
    assert_eq!(imp["report"]["patterns_added"], 1, "{imp}");
    assert_eq!(imp["report"]["sample"]["passed"], 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// `jetcarp limits` shows, sets and resets the machine's limits, in the tests' own settings
/// folder; `jetcarp score --quick` measures the processor with every answer right; doctor shows
/// the hardware.
#[test]
fn limits_and_a_short_score() {
    let entry = |v: &Value, name: &str| {
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap()
    };
    let v = json_of(&["--json", "limits"]);
    assert_eq!(v["entries"].as_array().unwrap().len(), 12, "{v}");
    assert!(v["memory_total"].as_u64().unwrap() > 0);
    let o = jetcarp(&["limits", "set", "animation-frames", "1234"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("1,234 frames"), "{text}");
    let f = entry(&json_of(&["--json", "limits"]), "animation-frames");
    assert_eq!(
        (f["value"].as_f64(), f["source"].as_str()),
        (Some(1234.0), Some("own"))
    );
    let bad = jetcarp(&["limits", "set", "no-such-limit", "1"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no limit called 'no-such-limit'"));
    assert!(
        jetcarp(&["limits", "reset", "animation-frames"])
            .status
            .success()
    );
    let f = entry(&json_of(&["--json", "limits"]), "animation-frames");
    assert_eq!(f["source"], "suggested");
    let s = json_of(&["--json", "score", "--quick", "--no-gpu", "--no-save"]);
    assert!(s["score"]["score"].as_f64().unwrap() > 0.0, "{s}");
    assert!(s["score"].get("problems").is_none(), "{s}");
    assert!(s["machine_file"].is_null());
    let d = json_of(&["--json", "doctor"]);
    assert!(
        d["machine"]["hardware"]["threads"].as_u64().unwrap() >= 1,
        "{d}"
    );
}
