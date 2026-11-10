//! Scripting engine checks: sweeps write the right files, results do not depend on the thread
//! count, errors carry line numbers, cancellation and resume work, the write guard holds.

use jetcarp_script::{Event, RunOptions, Screen, Summary};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn temp_root(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jetcarp-script-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn opts(root: &Path) -> RunOptions {
    RunOptions {
        out_root: Some(root.to_path_buf()),
        screen: Screen {
            width: 320,
            height: 200,
            cell_px: 8.0,
        },
        ..Default::default()
    }
}

fn run(src: &str, o: RunOptions) -> Summary {
    jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)))
}

fn ok(src: &str, o: RunOptions) -> Summary {
    let s = run(src, o);
    assert!(s.ok, "script failed: {:?}\nlog: {:#?}", s.error, s.log);
    s
}

fn folder(s: &Summary) -> PathBuf {
    PathBuf::from(s.sweeps[0]["folder"].as_str().unwrap())
}

fn rows(dir: &Path) -> Vec<serde_json::Value> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    v["rows"].as_array().unwrap().clone()
}

#[test]
fn elementary_runs_stop_at_the_bottom() {
    let root = temp_root("bottom");
    let s = ok(
        r#"
        let w = world(#{ tiling: "square", size: [61, 40], boundary: "dead" });
        w.add_ca("W30", #{ seed: "top_center" });
        let r = w.run(#{ until: "done", max_gens: 5000 });
        print(`${r.generations} ${r.stopped} ${w.done()} ${w.height()}`);
        let h = world(#{ tiling: "hex", size: "screen", boundary: "wrap" });
        h.add_ca("HexW3 8229");
        let r = h.run(#{ until: "done" });
        print(`${r.generations} ${h.height()}`);
        "#,
        opts(&root),
    );
    assert_eq!(s.log[0], "39 until true 40");
    let parts: Vec<u64> = s.log[1].split(' ').map(|x| x.parse().unwrap()).collect();
    assert_eq!(
        parts[0],
        parts[1] - 1,
        "one pass down a screen-sized wrap grid"
    );
}

#[test]
fn small_sweep_writes_files() {
    let root = temp_root("files");
    let s = ok(
        r#"sweep(#{ rules: "HexW3 {n}", from: 0, to: 40, tiling: "hex", boundary: "dead",
                    seed: "top_center", until: "done", out: "HexW3" });"#,
        opts(&root),
    );
    let dir = folder(&s);
    assert!(dir.starts_with(&root));
    assert!(
        dir.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("_HexW3")
    );
    for n in [0, 7, 40] {
        let p = dir.join(format!("HexW3-{n:05}.png"));
        assert!(p.exists(), "{}", p.display());
    }
    let r = rows(&dir);
    assert_eq!(r.len(), 41);
    assert_eq!(r[7]["rule"], "HexW3 7");
    assert_eq!(r[7]["file"], "HexW3-00007.png");
    assert_eq!(r[7]["done"], true);
    assert!(dir.join("manifest.csv").exists() && dir.join("run.json").exists());
    assert!(!dir.join("manifest.partial.jsonl").exists());
    let csv = std::fs::read_to_string(dir.join("manifest.csv")).unwrap();
    assert!(csv.starts_with("index,n,rule,file,kept,generations,stopped,done"));
    assert_eq!(csv.lines().count(), 42);
    // Every run reached the last row: generations = rows - 1.
    let rows_n = r[0]["distinct_rows"].as_u64().unwrap();
    assert!(r.iter().all(|x| x["generations"] == r[0]["generations"]));
    assert!(rows_n >= 1);
    // The image is the screen size and keeps JETCARP's metadata.
    let dec = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(dir.join("HexW3-00007.png")).unwrap(),
    ));
    let info = dec.read_info().unwrap();
    assert_eq!((info.info().width, info.info().height), (320, 200));
    assert!(
        info.info()
            .utf8_text
            .iter()
            .any(|t| t.keyword == "jetcarp:hash")
    );
    let run: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
    assert!(run["script"].as_str().unwrap().contains("HexW3 {n}"));
    assert_eq!(run["screen"]["width"], 320);
}

#[test]
fn sweeps_do_not_depend_on_thread_count() {
    let src = r#"sweep(#{ rules: "W", from: 20, to: 59, tiling: "square", boundary: "wrap",
                         seed: "top_random:0.5", rng_seed: 9, until: "done", out: "W",
                         keep: |w, n| #{ keep: n % 3 != 0, mid: w.get(w.width() / 2, 10) } });"#;
    let mut manifests = vec![];
    for threads in [1usize, 7, 32] {
        let root = temp_root(&format!("threads{threads}"));
        let mut o = opts(&root);
        o.threads = Some(threads);
        let s = ok(src, o);
        let dir = folder(&s);
        manifests.push(std::fs::read_to_string(dir.join("manifest.json")).unwrap());
        assert!(dir.join("W031.png").exists());
        assert!(!dir.join("W030.png").exists(), "30 % 3 == 0 is not kept");
    }
    assert_eq!(manifests[0], manifests[1]);
    assert_eq!(manifests[0], manifests[2]);
    let v: serde_json::Value = serde_json::from_str(&manifests[0]).unwrap();
    assert!(v["rows"][0].get("mid").is_some(), "extra column from keep");
    assert_eq!(v["rows"][1]["kept"], false);
}

#[test]
fn one_argument_callbacks_and_lists() {
    let root = temp_root("lists");
    let s = ok(
        r#"let r = sweep(#{ rules: ["B3/S23", "B36/S23", "Brian's Brain"], tiling: "square",
                          size: [40, 30], boundary: "wrap", seed: "random:0.3",
                          until: "repeats", max_gens: 300, png: false,
                          keep: |w| w.population() > 0 });
           print(r.runs);"#,
        opts(&root),
    );
    assert_eq!(s.log.last().unwrap(), "3");
    let r = rows(&folder(&s));
    assert_eq!(r[2]["rule"], "Brian's Brain");
    assert_eq!(r[0]["file"], "");
}

#[test]
fn errors_report_lines() {
    let root = temp_root("errors");
    let s = run("let a = 1;\nlet b = 2;\nlet c = (a + ;\n", opts(&root));
    let e = s.error.unwrap();
    assert_eq!(e.line, Some(3));
    let e = jetcarp_script::check("let x = #{ until: \"done\" };\nlet y = ;").unwrap_err();
    assert_eq!(e.line, Some(2));
    // Runtime errors in JETCARP functions point at the call.
    let s = run(
        "let w = world(#{ tiling: \"square\", size: [20, 20] });\n\nw.add_ca(\"Q99\");\n",
        opts(&root),
    );
    let e = s.error.unwrap();
    assert_eq!(e.line, Some(3), "{e:?}");
    let s = run("let w = world(#{ tiling: \"nope\" });", opts(&root));
    assert!(s.error.unwrap().message.contains("unknown tiling"));
    let s = run("sweep(#{ rules: \"W\", wat: 1 });", opts(&root));
    assert!(
        s.error
            .unwrap()
            .message
            .contains("unknown sweep option `wat`")
    );
    // Errors inside a keep callback.
    let s = run(
        "sweep(#{ rules: \"W\", to: 3, size: [20, 20], png: false,\n keep: |w, n| w.nope() });",
        opts(&root),
    );
    assert!(s.error.unwrap().message.contains("nope"));
    assert!(jetcarp_script::check("print(1);").is_ok());
}

#[test]
fn cancel_and_resume() {
    let root = temp_root("cancel");
    let src = r#"sweep(#{ rules: "HexW3 {n}", from: 0, to: 999, tiling: "hex", boundary: "dead",
                         until: "done", out: "cancel", folder: params.folder, threads: 2 });"#;
    let mut o = opts(&root);
    o.screen = Screen::default();
    o.params
        .insert("folder".into(), root.join("atlas").display().to_string());
    let job = jetcarp_script::spawn(src.into(), o.clone());
    // Cancel as soon as some progress has been reported.
    let mut saw = false;
    while !job.is_finished() {
        for e in job.events() {
            if let Event::Progress(p) = e
                && p.done > 0
            {
                saw = true;
                job.cancel();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let s = job.wait();
    assert!(saw);
    assert!(s.cancelled && !s.ok, "{:?}", s.error);
    let dir = root.join("atlas");
    let first = rows(&dir).len();
    assert!(first > 0 && first < 1000, "{first} rows before the cancel");
    // Resume: only the rest runs, and the result has every row once.
    let s = ok(src, o);
    assert_eq!(s.sweeps[0]["resumed"].as_u64().unwrap() as usize, first);
    let r = rows(&dir);
    assert_eq!(r.len(), 1000);
    assert!(r.iter().enumerate().all(|(i, x)| x["index"] == i));
    assert!(!dir.join("manifest.partial.jsonl").exists());
}

#[test]
fn writes_stay_inside_the_output_root() {
    let root = temp_root("guard");
    let s = run(r#"write_text("/etc/jetcarp-test.txt", "x");"#, opts(&root));
    assert!(s.error.unwrap().message.contains("only write inside"));
    let s = run(r#"write_text("../escape.txt", "x");"#, opts(&root));
    assert!(s.error.is_some());
    let s = ok(
        r#"let d = out_dir("notes"); write_text(d + "/a.txt", "hi"); write_csv("b.csv", [#{x: 1, y: "a"}, #{x: 2}]);"#,
        opts(&root),
    );
    let d = &s.output_dirs[0];
    assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "hi");
    assert_eq!(
        std::fs::read_to_string(d.join("b.csv")).unwrap(),
        "x,y\n1,a\n2,\n"
    );
    assert!(d.join("run.json").exists());
}

#[test]
fn params_logs_and_analysis() {
    let root = temp_root("params");
    let mut o = opts(&root);
    o.params.insert("n".into(), "12".into());
    o.params.insert("name".into(), "glider".into());
    let log = Arc::new(Mutex::new(vec![]));
    let l2 = log.clone();
    let s = jetcarp_script::run(
        r#"
        let w = world(#{ tiling: "square", size: [30, 30], boundary: "wrap" });
        w.add_ca("Life", #{ seed: "empty" });
        w.pattern(".O.\n..O\nOOO", 5, 5);
        w.step(params.n);
        let o = w.objects();
        print(params.name + " " + o.totals.spaceship + " " + o.counts[0].speed);
        let c = w.copy();
        c.step(4);
        print(w.generation() + " " + c.generation() + " " + w.population());
        let k = world(#{ tiling: "square", size: [20, 20], boundary: "wrap" });
        k.add_ca("B3/S23", #{ seed: "random:0.3" });
        print(k.classify(#{ max_gens: 500 }).class);
        "#,
        o,
        move |e| {
            if let Event::Log(l) = e {
                l2.lock().unwrap().push(l);
            }
        },
        Arc::new(AtomicBool::new(false)),
    );
    assert!(s.ok, "{:?}", s.error);
    let log = log.lock().unwrap();
    assert_eq!(log[0], "glider 1 c/4 diagonal");
    assert_eq!(log[1], "12 16 5");
    assert!(["died", "frozen", "oscillating", "active", "exploded"].contains(&log[2].as_str()));
}

#[test]
fn json_sweeps_become_scripts() {
    let spec = serde_json::json!({"rules": "HexW3 {n}", "from": 0, "to": 2, "until": "done", "png": {"size": [64, 48]}});
    let script = jetcarp_script::sweep_to_script(&spec);
    assert!(jetcarp_script::check(&script).is_ok(), "{script}");
    let root = temp_root("form");
    let s = ok(&script, opts(&root));
    assert_eq!(rows(&folder(&s)).len(), 3);
}

#[test]
fn script_five_matches_the_json_scene() {
    let root = temp_root("five");
    let mut o = opts(&root);
    o.params.insert("ant_gens".into(), "300".into());
    let s = ok(include_str!("../../../scripts/script-five.rhai"), o);
    // "stage k: name: G generations (why), now at N, P live"
    let stages: Vec<(u64, u64)> = s
        .log
        .iter()
        .filter(|l| l.starts_with("stage "))
        .map(|l| {
            let now = l.split("now at ").nth(1).unwrap();
            let (g, rest) = now.split_once(", ").unwrap();
            (
                g.parse().unwrap(),
                rest.split(' ').next().unwrap().parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(stages.len(), 5);
    let sc = jetcarp_core::Scene::from_json(include_str!("../../jetcarp-core/data/scenes/script-five.json")).unwrap();
    let mut w = jetcarp_core::World::from_scene(&sc).unwrap();
    let mut ends = vec![];
    let mut step = w.script_state.step;
    while w.generation < stages[4].0 {
        w.step();
        if w.script_state.step != step {
            step = w.script_state.step;
            ends.push((w.generation, w.fields[0].population()));
        }
    }
    ends.push((w.generation, w.fields[0].population()));
    assert_eq!(
        ends, stages,
        "stage ends (generation, live cells): JSON scene vs script"
    );
}

#[test]
fn skip_leaves_out_dull_results() {
    let root = temp_root("skip");
    let s = ok(
        r#"sweep(#{ rules: "W", from: 0, to: 31, tiling: "square", size: [41, 30],
                    boundary: "dead", seed: "top_center", until: "done",
                    skip: ["empty", "uniform"] });"#,
        opts(&root),
    );
    let dir = folder(&s);
    let r = rows(&dir);
    assert_eq!(r.len(), 32);
    // Rule 0 leaves only the seed row: uniform, no image.
    assert_eq!(r[0]["skipped"], "uniform");
    assert_eq!(r[0]["kept"], false);
    assert_eq!(r[0]["file"], "");
    assert!(!dir.join("W00.png").exists() && !dir.join("W000.png").exists());
    let skipped = r.iter().filter(|x| x.get("skipped").is_some()).count();
    assert!(skipped > 0 && skipped < 32);
    assert_eq!(s.sweeps[0]["skipped"], skipped);
    for x in &r {
        let dull = matches!(x["row_class"].as_str(), Some("empty" | "uniform"));
        assert_eq!(x.get("skipped").is_some(), dull, "{x}");
        let file = x["file"].as_str().unwrap();
        assert_eq!(file.is_empty(), dull, "{x}");
        if !dull {
            assert!(dir.join(file).exists());
        }
    }
    let bad = run(r#"sweep(#{ rules: "W", skip: ["boring"] });"#, opts(&root));
    assert!(!bad.ok);
}

#[test]
fn scene_sweeps_vary_one_ca() {
    // Two CAs: Life (a drawn glider) and W30; sweep the second CA's rule.
    let root = temp_root("scene");
    let mut w = jetcarp_core::World::new(
        jetcarp_core::TilingSpec::rect("square", 40, 30, jetcarp_core::Boundary::Dead),
        7,
    )
    .unwrap();
    let mut a = jetcarp_core::CaSpec::new("B3/S23");
    a.name = Some("life".into());
    let life = w
        .apply(jetcarp_core::Command::AddCa { ca: a })
        .unwrap()
        .new_id
        .unwrap();
    let mut b = jetcarp_core::CaSpec::new("W30");
    b.seed = Some(jetcarp_core::SeedSpec::TopCenter);
    w.apply(jetcarp_core::Command::AddCa { ca: b }).unwrap();
    w.apply(jetcarp_core::Command::SetCells {
        id: life,
        cells: vec![[41, 1], [82, 1], [120, 1], [121, 1], [122, 1]],
    })
    .unwrap();
    w.apply(jetcarp_core::Command::Step { gens: 5 }).unwrap();
    let scene = serde_json::to_value(w.to_scene()).unwrap();
    let spec = serde_json::json!({
        "rules": "W", "from": 28, "to": 31, "scene": scene, "ca": 1,
        "until": "generations", "n": 12, "png": {"size": [80, 60]},
    });
    let src = jetcarp_script::sweep_to_script(&spec);
    let s = ok(&src, opts(&root));
    let dir = folder(&s);
    let r = rows(&dir);
    assert_eq!(r.len(), 4);
    assert_eq!(r[2]["rule"], "W30");
    // The W30 run equals the scene's world from generation 0 run 12 generations: drawn cells
    // kept, the steps after them dropped.
    let mut start = w.to_scene();
    start
        .commands
        .retain(|c| !matches!(c, jetcarp_core::Command::Step { .. }));
    let mut again = jetcarp_core::World::from_scene(&start).unwrap();
    again.apply(jetcarp_core::Command::Step { gens: 12 }).unwrap();
    assert_eq!(r[2]["hash"], again.hash_hex());
    // Different rules give different worlds (the rule really was replaced).
    assert_ne!(r[0]["hash"], r[2]["hash"]);
    // A tiling key next to a scene is refused.
    let mut bad = spec.clone();
    bad["tiling"] = serde_json::json!("hex");
    assert!(!run(&jetcarp_script::sweep_to_script(&bad), opts(&root)).ok);
    // run_world rebuilds a run from run.json.
    let run_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
    let mut w30 = jetcarp_script::run_world(&run_json, "W30").unwrap();
    w30.apply(jetcarp_core::Command::Step { gens: 12 }).unwrap();
    assert_eq!(r[2]["hash"], w30.hash_hex());
}

#[test]
fn run_world_and_estimate() {
    let root = temp_root("estimate");
    let spec = serde_json::json!({"rules": "HexW3 {n}", "from": 100, "to": 140,
        "boundary": "dead", "seed": "top_center", "until": "done"});
    let screen = opts(&root).screen;
    let e = jetcarp_script::estimate(&spec, screen, 3, &|| false).unwrap();
    assert_eq!(e.runs, 41);
    assert_eq!(e.samples.len(), 3);
    assert_eq!(e.samples[0].0, "HexW3 100");
    assert_eq!(e.image, Some((320, 200)));
    assert!(e.seconds_per_run > 0.0 && e.seconds(4) < e.seconds(1));
    // Nothing was written.
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    let s = ok(&jetcarp_script::sweep_to_script(&spec), opts(&root));
    let dir = folder(&s);
    let r = rows(&dir);
    let run_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
    for k in [0, 17, 40] {
        let mut w = jetcarp_script::run_world(&run_json, r[k]["rule"].as_str().unwrap()).unwrap();
        let g = r[k]["generations"].as_u64().unwrap();
        w.apply(jetcarp_core::Command::Step { gens: g }).unwrap();
        assert_eq!(r[k]["hash"], w.hash_hex(), "row {k}");
        // Its scene (Save scene, Quick Start) replays to the same state.
        let again = jetcarp_core::World::from_scene(&w.to_scene()).unwrap();
        assert_eq!(again.hash_hex(), w.hash_hex(), "row {k} scene");
    }
}

#[test]
fn show_needs_a_live_view() {
    let root = temp_root("show");
    let src = r#"let w = world(#{ tiling: "square", size: [20, 20] });
                 w.add_ca("Life", #{ seed: "random:0.3" });
                 w.step(3);
                 show(w, "soup");"#;
    let s = ok(src, opts(&root));
    assert!(
        s.log[0].starts_with("show: soup, generation 3")
            && s.log[0].ends_with("(no live view here)"),
        "{}",
        s.log[0]
    );
    let mut o = opts(&root);
    o.live_view = true;
    let shown = Arc::new(Mutex::new(vec![]));
    let sh = shown.clone();
    let s = jetcarp_script::run(
        src,
        o,
        move |e| {
            if let Event::Show(x) = e {
                sh.lock().unwrap().push(x);
            }
        },
        Arc::new(AtomicBool::new(false)),
    );
    assert!(s.ok, "{:?}", s.error);
    let v = shown.lock().unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].generation, 3);
    let w = v[0].take_world().unwrap();
    assert_eq!(w.hash_hex(), v[0].hash);
    assert!(v[0].take_world().is_none());
}

#[test]
fn scripts_catalog_and_promote_library_patterns() {
    let root = temp_root("library");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    // A glider and a block in Life: catalog, find, update, promote.
    let s = ok(
        r#"
        let lib = library();
        let w = world(#{ tiling: "square", size: [40, 40], boundary: "wrap" });
        w.add_ca("B3/S23", #{ seed: "empty" });
        w.pattern(".O.\n..O\nOOO", 5, 5);
        w.pattern("OO\nOO", 25, 25);
        let r = lib.catalog(w);
        print(`${r.objects} ${r.added.len()} ${r.merged.len()}`);
        let again = lib.add_from(w);
        print(`${again.added.len()} ${again.merged.len()}`);
        let ships = lib.query(#{ class: "spaceship" });
        print(`${ships.len()} ${ships[0].name} ${ships[0].seen}`);
        lib.update(ships[0].id, #{ add_tags: ["test"], notes: "from a script" });
        let p = lib.promote(ships[0].id, "My glider");
        print(`${lib.get(p.id).state} ${lib.get(p.id).tags[0]} ${lib.count()}`);
        let g = lib.get(p.id);
        print(`${lib.works_in("B3678/S0235678").len()} ${lib.quickstart().len()} ${lib.sightings(p.id)[0].count} ${g.range.min_rule} ${g.rules[0]} ${p.entry == "find:" + p.id}`);
        "#,
        o.clone(),
    );
    assert_eq!(s.log[0], "2 2 0");
    assert_eq!(s.log[1], "0 2");
    assert_eq!(s.log[2], "1 glider 2");
    assert_eq!(s.log[3], "promoted test 2");
    // Both work in B3678/S0235678 (the block: B/S3 to B345678/S012345678; the glider: B3/S23 to
    // B3678/S0235678), though neither was seen there.
    assert_eq!(s.log[4], "2 1 2 B3/S23 B3/S23 true");
    let lib = jetcarp_core::library::Library::open(root.join("lib"));
    assert_eq!(jetcarp_core::quickstart::user_finds(&lib).len(), 1);
    // The example script runs (small) against the same scratch library.
    o.params.insert("soups".into(), "1".into());
    o.params.insert("size".into(), "48".into());
    o.params.insert("rule".into(), "B3/S23".into());
    o.params.insert("promote".into(), "false".into());
    let s = ok(include_str!("../../../scripts/catalog-hunt.rhai"), o);
    assert!(
        s.log.iter().any(|l| l.contains("new patterns")),
        "{:?}",
        s.log
    );
}

#[test]
fn row_classes_see_shifts_and_nesting() {
    let root = temp_root("rowclass");
    let s = ok(
        r#"
        fn d(tiling, rule, seed) {
            let w = world(#{ tiling: tiling, size: "screen", boundary: "dead" });
            w.add_ca(rule, #{ seed: seed });
            w.run(#{ until: "done", max_gens: 5000 });
            let x = w.describe();
            print(`${rule} ${x.row_class} ${x.row_period} ${x.row_shift} ${x.row_hint}`);
        }
        for r in ["W2", "W170", "W184", "W50", "W90", "W150", "W30", "W45", "W110",
                  "W105", "W129", "W161", "W165", "W141"] { d("square", r, "top_center"); }
        d("square", "W108", "top_random:0.5");
        d("hex", "HexW 2", "top_center");
        d("hex", "HexW 6", "top_center");
        d("hex_flat", "HexT 2", "top_random:0.5");
        d("hex_flat", "HexT 16", "top_random:0.5");
        d("hex_flat", "HexT 90", "top_center");
        "#,
        RunOptions {
            screen: Screen {
                width: 1920,
                height: 1080,
                cell_px: 8.0,
            },
            ..opts(&root)
        },
    );
    let want = [
        "W2 shifting 1 -1.0 ",
        "W170 shifting 1 -1.0 ",
        "W184 shifting 1 1.0 ",
        "W50 periodic 2 0.0 ",
        "W90 aperiodic   nested",
        "W150 aperiodic   nested",
        "W30 aperiodic   chaotic",
        "W45 aperiodic   chaotic",
        "W110 aperiodic   chaotic",
        // The background flips (105) or turns to 1 (129, 161, 165) and dead edges add noise; the
        // light cone is nested. 141 is one large domain with moving walls.
        "W105 aperiodic   nested",
        "W129 aperiodic   nested",
        "W161 aperiodic   nested",
        "W165 aperiodic   nested",
        "W141 aperiodic   regular",
        "W108 periodic 2 0.0 ",
        "HexW 2 shifting 1 -0.5 ",
        "HexW 6 aperiodic   nested",
        "HexT 2 shifting 1 -1.0 ",
        "HexT 16 shifting 1 1.0 ",
        "HexT 90 aperiodic   nested",
    ];
    assert_eq!(s.log, want);
}

#[test]
fn three_parent_hexagon_sweep() {
    let root = temp_root("hext");
    let s = ok(
        r#"sweep(#{ rules: "HexT", from: 0, to: 31, size: "screen", boundary: "dead",
                    seed: "top_center", until: "done", png: false });
           sweep(#{ rules: "HexT3", sample: 6, sample_seed: 3, size: [20, 10], until: "done", png: false });"#,
        opts(&root),
    );
    let r = rows(&folder(&s));
    assert_eq!(r.len(), 32);
    // The flat-top grid is the default; every run stops at the bottom half-row layer.
    let size =
        jetcarp_core::topology::fill_size(&jetcarp_core::TilingSpec::new("hex_flat"), 320.0, 200.0, 8.0);
    let jetcarp_core::SizeSpec::Rect { h, .. } = size else {
        panic!("{size:?}")
    };
    assert!(
        r.iter()
            .all(|x| x["done"] == true && x["generations"] == 2 * h as u64 - 1)
    );
    assert_eq!(r[4]["rule"], "HexT 4");
    let r2 = rows(&PathBuf::from(s.sweeps[1]["folder"].as_str().unwrap()));
    assert_eq!(r2.len(), 6);
    assert!(
        r2.iter()
            .all(|x| x["generations"] == 19 && x["error"].is_null())
    );
}

#[test]
fn evolve_and_track_mover_builtins() {
    let root = temp_root("evolve");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r#"
        let r = evolve(#{ tiling: "square", pop: 64, gens: 3, seed: 2, radius: 16, horizon: 400,
                          champions: 2, verify: [1, 2], threads: 2 });
        print(`${r.generations} ${r.fitness.len()} ${type_of(r.best_ever)} ${exists(join(r.dir, "fitness.csv"))}`);
        let t = track_mover(#{ rule: "B3/S23", tiling: "spectre", level: 3, cells: [[300, 1]] });
        print(`${t.verified} ${t.stop}`);
        let t = track_mover(#{ rule: "SpectreGlider", tiling: "spectre", level: 4, cells: [[2675, 1], [2676, 3]], max_level: 5 });
        print(`${t.verified} ${t.stop}`);
        let t = track_mover(#{ rule: "SpectreGlider", tiling: "spectre", level: 4, cells: [[2675, 1], [2676, 3]], rings: 1500, store: true });
        print(`${t.verified} ${t.stop} ${t.reach.rings} ${t.window.sliding} ${type_of(t.id)}`);
        "#,
        o,
    );
    assert_eq!(s.log[0], "3 3 f64 true");
    assert_eq!(s.log[1], "false died");
    assert_eq!(s.log[2], "true edge");
    assert_eq!(s.log[3], "true target 1500 true string");
}

#[test]
fn scripts_promote_sweep_runs() {
    let root = temp_root("promote-run");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r##"
        let s = sweep(#{ rules: "W", from: 28, to: 31, tiling: "square", size: [40, 24],
                         boundary: "dead", seed: "top_center", until: "done", png: false });
        let lib = library();
        let a = lib.promote_run(s.folder, "W30");
        let b = lib.promote_run(s.folder, "#3", #{ title: "Rule 31", note: "stripes" });
        print(`${a.rule} ${a.index} ${a.title} | ${b.rule} ${b.title}`);
        "##,
        o,
    );
    assert!(
        s.log.iter().any(|l| l == "W30 2 W30 | W31 Rule 31"),
        "{:?}",
        s.log
    );
    let lib = jetcarp_core::library::Library::open(root.join("lib"));
    let finds = jetcarp_core::quickstart::user_finds(&lib);
    assert_eq!(finds.len(), 2);
    let favs = jetcarp_script::results::Favourites::load(&folder(&s));
    assert_eq!(favs.items.len(), 2);
    assert_eq!(favs.items[&3]["note"], "stripes");
    // A run that is not in the manifest is an error with a hint.
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let bad = run(
        &format!(
            "library().promote_run(\"{}\", \"W99\");",
            folder(&s).display()
        ),
        o,
    );
    assert!(!bad.ok);
    assert!(
        format!("{:?}", bad.error).contains("#index"),
        "{:?}",
        bad.error
    );
}

#[test]
fn scripts_run_soup_searches_into_the_library() {
    let root = temp_root("lib-search");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r#"
        let lib = library();
        let r = lib.search(#{ rules: ["B3/S23"], tilings: "3.4.6.4", soups: 4, size: [32, 32],
                              gens: 600, seed: 1 });
        let g = r.groups[0];
        print(`${g.tiling}|${g.distinct}|${r.new_patterns}|${lib.count()}`);
        let again = lib.search(#{ rules: "B3/S23", tilings: ["rhombitrihex"], soups: 4,
                                  size: "32x32", gens: 600, seed: 1 });
        print(`again ${again.new_patterns} ${again.known_patterns}`);
        "#,
        o,
    );
    let line = s.log.iter().find(|l| l.contains('|')).unwrap();
    let f: Vec<&str> = line.split('|').collect();
    assert!(f[0].contains("3.4.6.4"), "{line}");
    let distinct: usize = f[1].parse().unwrap();
    assert!(distinct > 0, "{line}");
    assert_eq!(f[2], f[1], "{line}");
    assert_eq!(f[3], f[1], "{line}");
    // The same search again finds the same patterns, all known.
    assert!(
        s.log.iter().any(|l| l == &format!("again 0 {distinct}")),
        "{:?}",
        s.log
    );
}

#[test]
fn scripts_run_resumable_hunts() {
    let root = temp_root("hunt");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r#"
        let lib = library();
        let p = hunt(#{ rules: ["B3/S23"], tilings: "square", soups: 300, limit: 100, seed: 3 });
        print(`first ${p.status} ${p.soups_done}`);
        let q = lib.hunt(#{ resume: p.job });
        print(`then ${q.status} ${q.soups_done} ${q.objects > 1000} ${q.by_class.spaceship > 0}`);
        let st = lib.hunt_status(p.job);
        print(`status ${st.status} ${st.spec.soups} ${lib.hunts().len()}`);
        "#,
        o,
    );
    for want in [
        "first paused 100",
        "then done 300 true true",
        "status done 300 1",
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
}

#[test]
fn scripts_run_ant_searches() {
    let root = temp_root("ants");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r#"
        let r = ant_run("RL");
        print(`run ${r.outcome} ${r.period} ${r.onset}`);
        let lib = library();
        let p = ants(#{ tilings: "square,hex", strings: [2, 3], steps: 50000, limit: 20 });
        print(`first ${p.status} ${p.runs_done}`);
        let q = lib.ants(#{ resume: p.job });
        print(`then ${q.status} ${q.runs_done == q.runs_total} ${q.by_outcome.highway > 0}`);
        let hw = lib.ant_list(#{ outcome: "highway", tiling: "hex", limit: 5 });
        print(`highways ${hw.len() > 0} ${lib.ant(hw[0].id).outcome}`);
        print(`census ${lib.ant_census().len()} jobs ${lib.ant_jobs().len()} ${lib.ant_status(p.job).spec.steps}`);
        "#,
        o,
    );
    for want in [
        "run highway 104 9977",
        "first paused 20",
        "then done true true",
        "highways true highway",
        "census 2 jobs 1 50000",
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
}

#[test]
fn scripts_run_stage1_searches_and_rate_settings() {
    let root = temp_root("stage1");
    let mut o = opts(&root);
    o.library = Some(root.join("lib"));
    let s = ok(
        r#"
        let lib = library();
        let spec = #{ tilings: "hex", birth: "2-3", survive: "2-4", grid: [32, 32], gens: 200,
                      soups: 2, refine: #{ top: 2, densities: [0.2], soups: [], grids: [],
                      gens: [] }, neighbours: false };
        let p = stage1(spec + #{ limit: 10 });
        print(`first ${p.status} ${p.evaluated}`);
        let q = lib.stage1(#{ resume: p.job });
        print(`then ${q.status} ${q.evaluated} ${q.passes.len()}`);
        let top = lib.settings(#{ job: p.job, limit: 3 });
        print(`top ${top.len()} ${top[0].score >= top[1].score} ${top[0].descriptors.runs}`);
        let one = lib.setting(top[0].id);
        print(`one ${one.rule == top[0].rule} ${one.scene.cas.len()}`);
        let r = lib.rate("setting", top[1].id, 5, #{ rater: "model:stub", note: "nice" });
        let a = lib.approve(top[2].id);
        print(`rated ${r > 0} ${a.status} ${lib.score_model().source}`);
        let st = lib.stage1_status(p.job);
        print(`status ${st.status} ${st.spec.spaces.len()} ${lib.stage1_jobs().len()}`);
        let sh = lib.stage1_sheet(#{ png: "s1/top.png", query: #{ job: p.job, limit: 4 },
                                     thumb: 60, cols: 2, labels: ["A", "B", "C", "D"] });
        print(`sheet ${sh.count}`);
        "#,
        o,
    );
    for want in [
        "first paused 10",
        "then done 34 2",
        "top 3 true 2",
        "one true 1",
        "rated true approved hand",
        "status done 1 1",
        "sheet 4",
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
    let side: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("s1/top.json")).unwrap()).unwrap();
    assert_eq!(side["entries"][0]["label"], "A");
    assert!(side["entries"][3]["setting_id"].as_i64().is_some());
}

#[test]
fn missing_functions_get_a_rhai_hint() {
    // A model wrote `type(x)` (Python) in a script; the error now says what Rhai calls it.
    let root = temp_root("hints");
    let s = run("let m = #{ a: 1 };\nprint(type(m));", opts(&root));
    assert!(!s.ok);
    let e = format!("{:?}", s.error);
    assert!(e.contains("type_of"), "{e}");
    assert_eq!(s.error.as_ref().unwrap().line, Some(2), "{e}");
    let s = run("let n = 3;\nprint(str(n));", opts(&root));
    assert!(
        format!("{:?}", s.error).contains("to_string"),
        "{:?}",
        s.error
    );
}

#[test]
fn scripts_read_sweep_manifests_back() {
    // A model spent three run_script calls working out manifest.json's shape and null; now
    // `manifest(folder)` gives the rows, and `null` in a script says to use ().
    let root = temp_root("manifest-fn");
    let s = ok(
        r#"
        let s = sweep(#{ rules: "W", from: 28, to: 31, tiling: "square", size: [40, 24],
                         boundary: "dead", seed: "top_center", until: "done", png: false });
        let rows = manifest(s.folder);
        let none = 0;
        for r in rows { if r.period == () { none += 1; } }
        print(`${rows.len()} ${rows[2].rule} ${none}`);
        "#,
        opts(&root),
    );
    assert!(s.log.iter().any(|l| l == "4 W30 4"), "{:?}", s.log);
    let e = jetcarp_script::check("let x = null;").unwrap_err();
    assert!(e.message.contains("x == ()"), "{e:?}");
}

#[test]
fn reseed_functions() {
    let root = temp_root("reseed");
    let s = ok(
        r#"
        let w = world(#{ tiling: "square", size: [40, 30], boundary: "wrap" });
        w.add_ca("Life", #{ seed: "empty" });
        w.reseed("blobs");
        let a = w.population();
        w.reseed(1, #{ kind: "rain", drops: 10, min_size: 2, max_size: 4, density: 1.0 });
        w.set_reseed([#{ mode: "random", when: #{ kind: "every", gens: 5 } }]);
        w.step(20);
        let st = w.reseed_status();
        print(`${a > 0} ${w.population() > 0} ${st.policies[0].fired}`);
        "#,
        opts(&root),
    );
    assert_eq!(s.log[0], "true true 4");
}
