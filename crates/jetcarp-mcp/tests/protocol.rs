//! MCP protocol checks: handshake (legacy and modern), tools/list, tool round trips, errors,
//! and the app link over a real loopback socket.

use jetcarp_mcp::{Server, Session, link, protocol, tools};
use serde_json::{Value, json};
use std::sync::Arc;

fn req(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(server: &mut Server, host: &mut Session, name: &str, args: Value) -> Value {
    let r = server
        .handle(
            host,
            &req(7, "tools/call", json!({"name": name, "arguments": args})),
        )
        .expect("reply");
    assert_eq!(r["id"], 7);
    r["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ok(result: &Value) -> String {
    assert_eq!(result["isError"], false, "tool failed: {}", text(result));
    text(result)
}

#[test]
fn legacy_initialize_negotiates_version() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = s
        .handle(
            &mut h,
            &req(
                1,
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {},
                 "clientInfo": {"name": "test", "version": "0"}}),
            ),
        )
        .unwrap();
    assert_eq!(r["jsonrpc"], "2.0");
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r["result"]["serverInfo"]["name"], "jetcarp");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    // Unknown versions get the newest handshake version.
    let r = s
        .handle(
            &mut h,
            &req(2, "initialize", json!({"protocolVersion": "2099-01-01"})),
        )
        .unwrap();
    assert_eq!(r["result"]["protocolVersion"], protocol::LEGACY_VERSIONS[0]);
    // Notifications get no reply.
    assert!(
        s.handle(
            &mut h,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .is_none()
    );
    let r = s.handle(&mut h, &req(3, "ping", json!({}))).unwrap();
    assert!(r["result"].is_object());
}

#[test]
fn modern_discover_and_version_errors() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let meta = |v: &str| {
        json!({"_meta": {"io.modelcontextprotocol/protocolVersion": v,
        "io.modelcontextprotocol/clientInfo": {"name": "t", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {}}})
    };
    let r = s
        .handle(&mut h, &req(1, "server/discover", meta("2026-07-28")))
        .unwrap();
    let res = &r["result"];
    assert_eq!(res["resultType"], "complete");
    assert!(
        res["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
    assert_eq!(
        res["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "jetcarp"
    );
    // Modern requests work without initialize, and carry serverInfo.
    let r = s
        .handle(&mut h, &req(2, "tools/list", meta("2026-07-28")))
        .unwrap();
    assert!(r["result"]["tools"].as_array().unwrap().len() >= 10);
    assert!(r["result"]["ttlMs"].is_number());
    assert_eq!(
        r["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "jetcarp"
    );
    let r = s
        .handle(&mut h, &req(3, "tools/list", meta("1900-01-01")))
        .unwrap();
    assert_eq!(r["error"]["code"], protocol::UNSUPPORTED_PROTOCOL_VERSION);
    assert_eq!(r["error"]["data"]["requested"], "1900-01-01");
    assert!(r["error"]["data"]["supported"].as_array().unwrap().len() >= 2);
}

#[test]
fn tools_list_is_well_formed_and_stable() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let a = s.handle(&mut h, &req(1, "tools/list", json!({}))).unwrap();
    let b = s.handle(&mut h, &req(2, "tools/list", json!({}))).unwrap();
    assert_eq!(a["result"]["tools"], b["result"]["tools"]);
    let list = a["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = list.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, tools::NAMES);
    for t in list {
        let name = t["name"].as_str().unwrap();
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{name}"
        );
        assert!(t["description"].as_str().unwrap().len() > 60, "{name}");
        assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        // Every property has a description for the model.
        if let Some(props) = t["inputSchema"]["properties"].as_object() {
            for (k, p) in props {
                assert!(p["description"].is_string(), "{name}.{k}");
            }
        }
    }
}

#[test]
fn stop_when_settled_runs_without_a_count() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"tiling": "square", "width": 21, "height": 21, "boundary": "dead",
               "cas": [{"rule": "Sandpile", "seed": "pile:400"}]}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "step",
        json!({"stop_when_settled": true}),
    ));
    assert!(!t.contains("not yet stable"), "{t}");
    assert!(!t.contains("Ran 1 generation"), "{t}");
}

#[test]
fn glider_round_trip() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"tiling": "square", "width": 30, "height": 30,
               "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "set_cells",
        json!({"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}),
    ));
    assert!(t.contains("Set 5 cell"), "{t}");
    ok(&call(&mut s, &mut h, "step", json!({"generations": 8})));
    let t = ok(&call(&mut s, &mut h, "read_cells", json!({})));
    assert!(t.contains("bounding box x 7..9, y 7..9"), "{t}");
    assert!(t.contains("..O.."), "{t}");
    // Same glider written as RLE in a fresh world lands on the same hash.
    let hash = h.world.hash_hex();
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"width": 30, "height": 30, "cas": [{"rule": "B3/S23", "seed": "empty"}]}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "set_cells",
        json!({"rle": "bo$2bo$3o!", "x": 5, "y": 5}),
    ));
    ok(&call(&mut s, &mut h, "step", json!({"generations": 8})));
    assert_eq!(h.world.hash_hex(), hash);
    // The scene replays to the same state.
    let scene = ok(&call(&mut s, &mut h, "get_scene", json!({})));
    let mut h2 = Session::new();
    let t = ok(&call(
        &mut s,
        &mut h2,
        "load_scene",
        json!({"scene": scene}),
    ));
    assert!(t.contains("Replayed to its recorded hash"), "{t}");
    assert_eq!(h2.world.hash_hex(), hash);
}

#[test]
fn status_render_and_commands() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"tiling": "hex", "width": 40, "height": 30, "boundary": "wrap",
               "cas": ["Brian's Brain"], "rng_seed": 3}),
    ));
    let st: Value =
        serde_json::from_str(&ok(&call(&mut s, &mut h, "world_status", json!({})))).unwrap();
    assert_eq!(st["tiling"]["id"], "hex");
    assert_eq!(st["tiling"]["grid"]["width"], 40);
    assert_eq!(st["cas"][0]["rule"], "B2/S/C3");
    let r = call(&mut s, &mut h, "render_image", json!({"max_side": 128}));
    ok(&r);
    let img = r["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "image")
        .unwrap();
    assert_eq!(img["mimeType"], "image/png");
    assert!(img["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    let t = ok(&call(
        &mut s,
        &mut h,
        "apply_commands",
        json!({"commands": [{"op": "add_ca", "ca": {"rule": "Life", "seed": "random:0.1"}},
                            {"op": "step", "gens": 3}]}),
    ));
    assert!(t.contains("new CA id 2"), "{t}");
    assert_eq!(h.world.generation, 3);
    let c = ok(&call(
        &mut s,
        &mut h,
        "inspect_cell",
        json!({"x": 3, "y": 3}),
    ));
    let c: Value = serde_json::from_str(&c).unwrap();
    assert_eq!(c["edge_neighbors_clockwise"].as_array().unwrap().len(), 6);
}

#[test]
fn errors_are_useful_not_fatal() {
    let (mut s, mut h) = (Server::new(), Session::new());
    // Protocol errors.
    let r = s.handle(&mut h, &req(1, "no/such", json!({}))).unwrap();
    assert_eq!(r["error"]["code"], protocol::METHOD_NOT_FOUND);
    let r = s
        .handle(
            &mut h,
            &req(2, "tools/call", json!({"name": "nope", "arguments": {}})),
        )
        .unwrap();
    assert_eq!(r["error"]["code"], protocol::INVALID_PARAMS);
    let r = s.handle_line(&mut h, "{not json").unwrap();
    assert!(r.contains("-32700"), "{r}");
    let r = s
        .handle(&mut h, &json!({"id": 3, "method": "ping"}))
        .unwrap();
    assert_eq!(r["error"]["code"], protocol::INVALID_REQUEST);
    // Tool errors come back as isError results the model can read.
    let cases = [
        ("set_cells", json!({"pattern": "O"}), "no CAs"),
        (
            "new_world",
            json!({"tiling": "pentagonal-nonsense"}),
            "unknown tiling",
        ),
        ("new_world", json!({"width": 3, "height": 3}), "8 to 4096"),
        ("new_world", json!({"cas": ["Q7 nonsense"]}), "not a rule"),
        (
            "new_world",
            json!({"tiling": "hex", "cas": ["W30"]}),
            "square grid",
        ),
        ("step", json!({"generations": 0}), "at least 1"),
        ("render_image", json!({"max_side": 5}), "64 to 2048"),
        (
            "apply_commands",
            json!({"commands": [{"gens": 3}]}),
            "no \"op\"",
        ),
        ("load_scene", json!({"scene": "{}"}), "scene JSON"),
        ("world_status", json!({}), ""),
    ];
    for (name, args, needle) in cases {
        let r = call(&mut s, &mut h, name, args.clone());
        if name == "world_status" {
            ok(&r);
            continue;
        }
        assert_eq!(r["isError"], true, "{name} {args}: {}", text(&r));
        assert!(text(&r).contains(needle), "{name}: {}", text(&r));
    }
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"width": 20, "height": 20, "boundary": "dead", "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    for (args, needle) in [
        (json!({"cells": [[25, 3]]}), "outside the grid"),
        (json!({"cells": [[1, 1, 2]]}), "too high"),
        (json!({"pattern": "O?"}), "not understood"),
        (json!({"ca": 9, "cells": [[1, 1]]}), "no CA 9"),
        (json!({}), "nothing to draw"),
        (json!({"bogus": 1}), "unknown argument"),
    ] {
        let r = call(&mut s, &mut h, "set_cells", args.clone());
        assert_eq!(r["isError"], true, "{args}");
        assert!(text(&r).contains(needle), "{args}: {}", text(&r));
    }
    let r = call(&mut s, &mut h, "step", json!({"generations": 2_000_000}));
    assert!(text(&r).contains("at most"), "{}", text(&r));
    // The world survived all of that.
    assert_eq!(h.world.generation, 0);
}

#[test]
fn app_link_round_trip_with_token() {
    let listener = link::Listener::start(0, Arc::new(|| {})).expect("listen");
    let port = listener.port;
    let token = listener.token.clone();
    // A fake UI thread: answers queued requests with its own session.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let ui = std::thread::spawn(move || {
        let (mut server, mut host) = (Server::new(), Session::new());
        while !stop2.load(std::sync::atomic::Ordering::SeqCst) {
            for p in listener.poll() {
                let msg = p.message.clone();
                p.reply(server.handle(&mut host, &msg));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    let bad = link::AppClient::connect(Some(link::EndpointInfo {
        port,
        token: "wrong".into(),
    }));
    assert!(bad.is_err());
    let mut c =
        link::AppClient::connect(Some(link::EndpointInfo { port, token })).expect("connect");
    let r = c
        .call(&req(
            1,
            "tools/call",
            json!({"name": "new_world",
            "arguments": {"width": 16, "height": 16, "cas": ["Life"]}}),
        ))
        .unwrap();
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = c
        .call(&req(
            2,
            "tools/call",
            json!({"name": "step", "arguments": {"generations": 4}}),
        ))
        .unwrap();
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Generation 4")
    );
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    ui.join().unwrap();
}

#[test]
fn saving_creates_folders() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"width": 16, "height": 16, "cas": ["Life"]}),
    ));
    let dir = std::env::temp_dir().join(format!("jetcarp-mcp-test-{}", std::process::id()));
    let png = dir.join("a/b/shot.png");
    let scene = dir.join("c/scene.json");
    ok(&call(
        &mut s,
        &mut h,
        "render_image",
        json!({"max_side": 64, "save_path": png.to_str().unwrap()}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "get_scene",
        json!({"path": scene.to_str().unwrap()}),
    ));
    assert!(png.is_file() && scene.is_file());
    let t = ok(&call(
        &mut s,
        &mut h,
        "load_scene",
        json!({"path": scene.to_str().unwrap()}),
    ));
    assert!(t.contains("Replayed to its recorded hash"), "{t}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sandpile_reports_stability() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"width": 21, "height": 21, "boundary": "dead",
               "cas": [{"rule": "Sandpile", "seed": "empty"}]}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "set_cells",
        json!({"cells": [[10, 10, 3000]]}),
    ));
    assert!(t.contains("3000"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "apply_commands",
        json!({"commands": [{"op": "sandpile", "id": 1, "action": "stabilize"}]}),
    ));
    assert!(t.contains("stable: no cell can topple"), "{t}");
    let st: Value =
        serde_json::from_str(&ok(&call(&mut s, &mut h, "world_status", json!({})))).unwrap();
    let f = &st["fields"][0];
    assert_eq!(f["stable"], true);
    assert_eq!(
        f["population"].as_u64().unwrap() + f["grains_lost_off_edges"].as_u64().unwrap(),
        3000
    );
}

#[test]
fn run_script_tool() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let w = world(#{ tiling: \"square\", size: [41, 30], boundary: \"dead\" });\nw.add_ca(\"W30\");\nlet r = w.run(#{ until: \"done\" });\nprint(`rows ${r.generations} ${params.tag}`);",
               "params": {"tag": "x"}}),
    );
    let t = ok(&r);
    assert!(t.contains("rows 29 x"), "{t}");
    // Errors come back with the line.
    let r = call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let a = 1;\nlet b = (a + ;"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("line 2"), "{}", text(&r));
    let r = call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "print(1)", "check_only": true}),
    );
    assert!(ok(&r).contains("no syntax errors"));
    // Worlds over the size limit are refused.
    let r = call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "world(#{ size: [5000, 5000] });"}),
    );
    assert!(text(&r).contains("limit"), "{}", text(&r));
    assert!(
        ok(&call(
            &mut s,
            &mut h,
            "jetcarp_help",
            json!({"topic": "scripts"})
        ))
        .contains("sweep")
    );
}

#[test]
fn run_script_sends_progress_when_asked() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let sent = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = sent.clone();
    s.notifier = Some(Arc::new(move |v: Value| sink.lock().unwrap().push(v)));
    let script = "for i in 0..=4 { progress(i, 4, `item ${i}`); }\nprint(\"done\");";
    // Without a progress token: no notifications.
    ok(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": script}),
    ));
    assert!(sent.lock().unwrap().is_empty());
    // With one: notifications/progress carrying the token, the first and the last at least.
    let r = s
        .handle(
            &mut h,
            &req(
                9,
                "tools/call",
                json!({"name": "run_script", "arguments": {"script": script},
                       "_meta": {"progressToken": "tok-1"}}),
            ),
        )
        .unwrap();
    assert!(ok(&r["result"]).contains("done"));
    let v = sent.lock().unwrap().clone();
    assert!(v.len() >= 2, "{v:?}");
    for n in &v {
        assert_eq!(n["method"], "notifications/progress");
        assert_eq!(n["params"]["progressToken"], "tok-1");
        assert_eq!(n["params"]["total"], 4.0);
        assert!(n.get("id").is_none());
    }
    assert_eq!(v.last().unwrap()["params"]["progress"], 4.0);
}

#[test]
fn library_promote_takes_a_sweep_run() {
    let (mut s, mut h) = (Server::new(), Session::new());
    // Both kinds at once, or a folder without a run, are refused with a hint.
    let r = call(
        &mut s,
        &mut h,
        "library_promote",
        json!({"id": "glider", "folder": "/nonexistent", "run": "W30"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("not both"), "{}", text(&r));
    let r = call(
        &mut s,
        &mut h,
        "library_promote",
        json!({"folder": "/nonexistent"}),
    );
    assert!(text(&r).contains("`run`"), "{}", text(&r));
    let r = call(
        &mut s,
        &mut h,
        "library_promote",
        json!({"folder": "/nonexistent-jetcarp-sweep", "run": "W30"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("manifest"), "{}", text(&r));
}

#[test]
fn quickstart_list_checks_its_status() {
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"kind": "quickstart", "status": "maybe"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("candidate"), "{}", text(&r));
}

#[test]
fn list_tilings_is_compact_and_searchable() {
    let (mut s, mut h) = (Server::new(), Session::new());
    // The whole catalog (1,300+ tilings) once came back here: 220,000 characters, more than a
    // local model's context. Now families plus a summary, and `search` for the catalog.
    let t = ok(&call(&mut s, &mut h, "list_tilings", json!({})));
    assert!(t.len() < 10_000, "{} chars", t.len());
    assert!(t.contains("rhombitrihex") && t.contains("k-uniform"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "list_tilings",
        json!({"search": "k2-"}),
    ));
    assert!(t.starts_with("20 tilings match"), "{t}");
    assert!(t.contains("k2-005"), "{t}");
    // Vertex configurations work as tiling ids.
    let r = call(
        &mut s,
        &mut h,
        "new_world",
        json!({"tiling": "3.4.6.4", "width": 8, "height": 8, "cas": [{"rule": "B3/S23"}]}),
    );
    assert!(ok(&r).contains("rhombitrihex"), "{}", text(&r));
}

#[test]
fn cells_hash_repeats_when_the_cells_do() {
    // A model checked a period-15 oscillator by comparing `hash` before and after 15 generations
    // and was puzzled: `hash` covers the generation and ages. `cells_hash` covers the cells only.
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"tiling": "square", "width": 16, "height": 16, "cas": [{"rule": "B3/S23", "seed": "empty"}]}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "set_cells",
        json!({"pattern": "OOO", "x": 6, "y": 7}),
    ));
    let st = |s: &mut Server, h: &mut Session| -> Value {
        serde_json::from_str(&ok(&call(s, h, "world_status", json!({})))).unwrap()
    };
    let a = st(&mut s, &mut h);
    let t = ok(&call(&mut s, &mut h, "step", json!({"generations": 2})));
    assert!(t.contains("cells hash"), "{t}");
    let b = st(&mut s, &mut h);
    assert_ne!(a["hash"], b["hash"]);
    assert_eq!(a["cells_hash"], b["cells_hash"]);
}

#[test]
fn reseed_and_wallpaper_tools() {
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "new_world",
        json!({"width": 60, "height": 40, "boundary": "wrap", "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    // Reseed now: an empty world gets live cells, logged as a command.
    let r: Value = serde_json::from_str(&ok(&call(
        &mut s,
        &mut h,
        "reseed",
        json!({"mode": {"kind": "blobs", "count": 2, "live": 0.5}}),
    )))
    .unwrap();
    assert_eq!(r["done"][0], "reseeded (blobs)");
    assert!(h.world.fields[0].population() > 0);
    assert!(
        h.world
            .log()
            .iter()
            .any(|c| matches!(c, jetcarp_core::Command::Reseed { .. }))
    );
    // Policies: set, run, report with activity, then clear.
    ok(&call(
        &mut s,
        &mut h,
        "reseed",
        json!({"policies": [{"mode": "rain", "when": {"kind": "every", "gens": 10}},
                            {"mode": "blobs", "when": "settled", "cooldown": 50}]}),
    ));
    ok(&call(&mut s, &mut h, "step", json!({"generations": 100})));
    let r: Value = serde_json::from_str(&ok(&call(&mut s, &mut h, "reseed", json!({})))).unwrap();
    assert_eq!(r["policies"][0]["status"]["fired"], 10);
    assert!(!r["activity"]["rows"].as_array().unwrap().is_empty());
    // The policies are in the scene.
    let scene = ok(&call(&mut s, &mut h, "get_scene", json!({})));
    assert!(scene.contains("set_reseed"), "{scene}");
    let t = call(&mut s, &mut h, "reseed", json!({"mode": "hail"}));
    assert_eq!(t["isError"], true);
    ok(&call(&mut s, &mut h, "reseed", json!({"policies": []})));
    assert!(h.world.reseed_policies().is_empty());
    // Wallpapers: one image, and a span cut per monitor.
    let dir = std::env::temp_dir().join(format!("jetcarp-mcp-wallpaper-{}", std::process::id()));
    let r: Value = serde_json::from_str(&ok(&call(
        &mut s,
        &mut h,
        "export_wallpaper",
        json!({"size": "320x180", "palette": "Paper", "path": dir.join("one.png").to_str().unwrap()}),
    )))
    .unwrap();
    assert_eq!(r["files"][0]["width"], 320);
    assert!(dir.join("one.png").is_file());
    let r: Value = serde_json::from_str(&ok(&call(
        &mut s,
        &mut h,
        "export_wallpaper",
        json!({"monitors": "256x144+0+0,192x108+256+18", "split": true, "fresh": true,
               "cell_px": 2, "gens": 20, "path": dir.to_str().unwrap()}),
    )))
    .unwrap();
    let files = r["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[1]["height"], 108);
    assert!(std::path::Path::new(files[1]["path"].as_str().unwrap()).is_file());
    // The fresh grid left the current world alone.
    assert_eq!(h.world.topo.len(), 60 * 40);
    let _ = std::fs::remove_dir_all(dir);
}
