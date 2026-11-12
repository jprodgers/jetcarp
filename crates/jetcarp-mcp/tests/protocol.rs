//! MCP protocol and the world tools: the handshake (legacy and modern), tools/list (every tool
//! well formed, each schema built from its operations), resources, errors, and the world tools'
//! round trips: build, draw, step, read, objects, render, save inside the folder rules (and
//! refused outside them), reseed, commands, scripts with progress, help and reference.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session, protocol, tools};
use serde_json::{Value, json};
use std::sync::Arc;

fn new() -> (Server, Session) {
    setup("protocol");
    (Server::new(), Session::new())
}

/// The shared tools, in order.
const TOOLS: &[&str] = &[
    "help",
    "reference",
    "world_new",
    "world_status",
    "world_read",
    "world_edit",
    "world_step",
    "world_render",
    "run_script",
    "library_query",
    "library_curate",
    "search_start",
    "job_status",
    "job_stop",
    "search_runs",
    "media_render",
    "pack",
    "files_list",
];

#[test]
fn legacy_initialize_negotiates_version() {
    let (mut s, mut h) = new();
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
    assert!(r["result"]["capabilities"]["resources"].is_object());
    let instr = r["result"]["instructions"].as_str().unwrap();
    for t in [
        "world_new",
        "world_edit",
        "search_start",
        "help",
        "reference",
    ] {
        assert!(instr.contains(t), "instructions name {t}: {instr}");
    }
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
    let (mut s, mut h) = new();
    let meta = |v: &str| json!({"_meta": {"io.modelcontextprotocol/protocolVersion": v}});
    let r = s
        .handle(&mut h, &req(1, "server/discover", meta("2026-07-28")))
        .unwrap();
    assert!(
        r["result"]["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
    assert!(r["result"]["capabilities"]["resources"].is_object());
    let r = s
        .handle(&mut h, &req(2, "tools/list", meta("2026-07-28")))
        .unwrap();
    assert_eq!(
        r["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "jetcarp"
    );
    let r = s
        .handle(&mut h, &req(3, "tools/list", meta("1999-01-01")))
        .unwrap();
    assert_eq!(r["error"]["code"], protocol::UNSUPPORTED_PROTOCOL_VERSION);
}

#[test]
fn tools_list_is_well_formed_and_from_the_operations() {
    let (mut s, mut h) = new();
    let r = s.handle(&mut h, &req(1, "tools/list", json!({}))).unwrap();
    let list = r["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = list.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names, TOOLS,
        "the headless server lists the shared tools only"
    );
    assert_eq!(tools::names(), TOOLS);
    for t in list {
        let name = t["name"].as_str().unwrap();
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object", "{name}");
        assert_eq!(schema["additionalProperties"], false, "{name}");
        assert!(
            t["description"].as_str().unwrap().len() > 200,
            "{name} explains itself"
        );
        assert!(t["title"].is_string() && t["annotations"]["title"].is_string());
        assert!(t["annotations"]["readOnlyHint"].is_boolean());
        // Strict clients: no null types, no $ref, no unknown formats.
        let text = schema.to_string();
        assert!(!text.contains("\"$ref\""), "{name}: {text}");
        assert!(!text.contains("\"null\""), "{name}: {text}");
        assert!(!text.contains("\"format\":\"uint"), "{name}: {text}");
        // Every property says what it is for.
        for (k, p) in schema["properties"].as_object().unwrap() {
            assert!(
                p["description"].as_str().is_some_and(|d| !d.is_empty()),
                "{name}.{k} has no description"
            );
        }
        // Old names are gone, also from the descriptions.
        for old in [
            "new_world",
            "read_cells",
            "inspect_cell",
            "render_image",
            "apply_commands",
            "get_scene",
            "load_scene",
            "jetcarp_help",
            "list_rules",
            "list_tilings",
            "library_list",
            "library_get",
            "library_promote",
            "paste_pattern",
            "hunt_start",
            "explore_start",
            "explore_query",
            "export_wallpaper",
            "site_export",
        ] {
            assert!(
                !t.to_string().contains(old),
                "{name} still names {old}: {}",
                t["description"]
            );
        }
    }
    // The schemas are the operations' own: a field documented on an operation's input shows
    // up with the same words.
    let by = |n: &str| list.iter().find(|t| t["name"] == n).unwrap().clone();
    let draw = jetcarp_api::op::input_schema::<jetcarp_api::ops::world::DrawIn>();
    assert_eq!(
        by("world_edit")["inputSchema"]["properties"]["cells"]["properties"]["rle"],
        draw["properties"]["rle"]
    );
    let hunt = jetcarp_api::op::input_schema::<jetcarp_api::ops::search::HuntIn>();
    assert_eq!(
        by("search_start")["inputSchema"]["properties"]["hunt"]["properties"],
        hunt["properties"]
    );
    let new = jetcarp_api::op::input_schema::<jetcarp_api::ops::world::NewWorldIn>();
    for (k, v) in new["properties"].as_object().unwrap() {
        assert_eq!(&by("world_new")["inputSchema"]["properties"][k], v, "{k}");
    }
    // Packs: no rater consent, no key making.
    let pack = by("pack");
    for a in ["info", "check", "import", "publish", "export", "site"] {
        assert!(pack["inputSchema"]["properties"][a].is_object(), "pack {a}");
    }
    assert!(pack["inputSchema"]["properties"].get("rater").is_none());
    assert!(pack["inputSchema"]["properties"].get("key").is_none());
    assert!(
        pack["inputSchema"]["properties"]["publish"]["properties"]
            .get("key")
            .is_none()
    );
    // And the search strategies, each its operation's.
    for st in jetcarp_mcp::job_tools::strategies() {
        assert!(
            by("search_start")["inputSchema"]["properties"][st]["properties"].is_object(),
            "{st}"
        );
    }
}

#[test]
fn resources_are_listed_and_read() {
    let (mut s, mut h) = new();
    let r = s
        .handle(&mut h, &req(1, "resources/list", json!({})))
        .unwrap();
    let list = r["result"]["resources"].as_array().unwrap();
    let uris: Vec<&str> = list.iter().map(|x| x["uri"].as_str().unwrap()).collect();
    for u in [
        "jetcarp://reference/rules",
        "jetcarp://reference/tilings",
        "jetcarp://reference/palettes",
        "jetcarp://reference/script-api",
        "jetcarp://schema/scene.json",
        "jetcarp://schema/pack.json",
        "jetcarp://docs/introduction",
        "jetcarp://docs/tutorials/mcp",
        "jetcarp://docs/reference/mcp",
    ] {
        assert!(uris.contains(&u), "{u} listed");
    }
    assert!(
        uris.iter()
            .filter(|u| u.starts_with("jetcarp://docs/"))
            .count()
            >= 70
    );
    assert!(!uris.contains(&"jetcarp://docs/SUMMARY"));
    let t = s
        .handle(&mut h, &req(2, "resources/templates/list", json!({})))
        .unwrap();
    let templates: Vec<&str> = t["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["uriTemplate"].as_str().unwrap())
        .collect();
    for u in [
        "jetcarp://docs/{page}",
        "jetcarp://job/{id}",
        "jetcarp://pattern/{id}",
        "jetcarp://quickstart/{handle}",
    ] {
        assert!(templates.contains(&u), "{u}");
    }
    let read = |s: &mut Server, h: &mut Session, uri: &str| {
        s.handle(h, &req(3, "resources/read", json!({"uri": uri})))
            .unwrap()
    };
    let r = read(&mut s, &mut h, "jetcarp://docs/guides/searching");
    let c = &r["result"]["contents"][0];
    assert_eq!(c["mimeType"], "text/markdown");
    assert!(c["text"].as_str().unwrap().starts_with("# Searching"));
    let r = read(&mut s, &mut h, "jetcarp://reference/rules");
    assert!(
        r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Brian's Brain")
    );
    let r = read(&mut s, &mut h, "jetcarp://schema/scene.json");
    let schema: Value =
        serde_json::from_str(r["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(schema["title"], "JETCARP scene");
    let r = read(&mut s, &mut h, "jetcarp://quickstart/life");
    assert!(
        r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"scene\"")
    );
    for bad in [
        "jetcarp://docs/nope",
        "jetcarp://job/x",
        "https://example.org",
    ] {
        let r = read(&mut s, &mut h, bad);
        assert_eq!(r["error"]["code"], protocol::RESOURCE_NOT_FOUND, "{bad}");
    }
    // The help tool serves the same pages.
    let page = ok(&call(
        &mut s,
        &mut h,
        "help",
        json!({"page": "guides/searching"}),
    ));
    assert!(page.starts_with("# Searching"));
    let pages = ok(&call(&mut s, &mut h, "help", json!({"topic": "pages"})));
    assert!(pages.contains("tutorials/mcp: "), "{pages}");
}

#[test]
fn glider_round_trip() {
    let (mut s, mut h) = new();
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"tiling": "square", "width": 40, "height": 40, "boundary": "wrap",
               "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    assert!(t.contains("\"rule\": \"B3/S23\""), "{t}");
    // Numbers as text and nested objects as JSON text are read as they are meant.
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": "{\"pattern\": \".O.\\n..O\\nOOO\", \"x\": \"5\", \"y\": 5}"}),
    ));
    assert!(
        t.contains("Set 5 cell(s)") && t.contains("x 5..7, y 5..7"),
        "{t}"
    );
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_step",
        json!({"generations": "8"}),
    ));
    assert!(t.contains("Generation 8"), "{t}");
    let t = ok(&call(&mut s, &mut h, "world_read", json!({})));
    assert!(t.contains("bounding box x 7..9, y 7..9"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_read",
        json!({"format": "list", "x": 7, "y": 7, "width": 3, "height": 3}),
    ));
    assert!(t.contains("[8,7,1]") || t.contains("[8, 7, 1]"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_read",
        json!({"format": "objects"}),
    ));
    assert!(
        t.contains("glider (c/4) x 1") && t.contains("x 7..9, y 7..9"),
        "{t}"
    );
    let cell: Value = json_of(&call(
        &mut s,
        &mut h,
        "world_read",
        json!({"cell": {"x": 8, "y": 7}}),
    ));
    assert_eq!(cell["values"][0]["value"], 1);
    assert_eq!(
        cell["touch_neighbors_clockwise"].as_array().unwrap().len(),
        8
    );
    let r = call(&mut s, &mut h, "world_render", json!({"max_side": 64}));
    assert!(ok(&r).contains("64 x 64 px"));
    assert!(has_png(&r));
    // The scene replays to the same state.
    let scene = ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"scene": true}),
    ));
    let hash = h.world.hash_hex();
    ok(&call(&mut s, &mut h, "world_new", json!({"cells": 4096})));
    assert_ne!(h.world.hash_hex(), hash);
    let t = ok(&call(&mut s, &mut h, "world_new", json!({"scene": scene})));
    assert!(t.contains("Replayed to its recorded hash") || h.world.hash_hex() == hash);
    assert_eq!(h.world.hash_hex(), hash);
}

#[test]
fn edits_commands_reseed_and_paste() {
    let (mut s, mut h) = new();
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 32, "height": 32, "cas": [{"rule": "Life", "seed": "empty"}, {"rule": "Brian's Brain", "seed": "empty"}]}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"ca": 2, "cells": {"cells": [[3, 3], [4, 4, 2]]}}),
    ));
    assert!(t.contains("id 2"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"commands": [{"op": "step", "gens": 3}, {"op": "update_ca", "id": 1, "change": {"palette": "Tide"}}]}),
    ));
    assert!(t.contains("Generation 3"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"reseed": {"policies": [{"mode": "rain", "when": {"kind": "every", "gens": 10}}]}}),
    ));
    assert!(t.contains("policies"), "{t}");
    ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"reseed": {"mode": "blobs"}}),
    ));
    // One action at a time, `ca` not with commands, and errors that say what to do.
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": {"pattern": "O"}, "commands": []}),
    ));
    assert!(e.contains("give one of"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"ca": 1, "commands": [{"op": "step", "gens": 1}]}),
    ));
    assert!(e.contains("`ca` goes with"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": {"patern": "O"}}),
    ));
    assert!(
        e.contains("unknown field `patern`") && e.contains("pattern"),
        "{e}"
    );
    let e = fails(&call(&mut s, &mut h, "world_edit", json!({})));
    assert!(e.contains("give one of"), "{e}");
    // A library pattern, pasted and opened: the object finder stores it first.
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 30, "height": 30, "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": {"pattern": ".O.\n..O\nOOO", "x": 3, "y": 3}}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_read",
        json!({"format": "objects", "catalog": true}),
    ));
    assert!(
        t.contains("Library: 1 new") || t.contains("already known"),
        "{t}"
    );
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"paste": {"id": "glider", "x": 20, "y": 20, "orientation": 2}}),
    ));
    assert!(t.contains("Pasted glider"), "{t}");
    assert_eq!(h.world.fields[0].population(), 10);
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"pattern": "glider"}),
    ));
    assert!(t.contains("Scene loaded"), "{t}");
    assert_eq!(h.world.fields[0].population(), 5);
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"quickstart": "life"}),
    ));
    assert!(t.contains("Scene loaded"), "{t}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"quickstart": "life", "width": 20, "height": 20}),
    ));
    assert!(e.contains("give one or the other"), "{e}");
}

#[test]
fn files_stay_inside_the_folder_rules() {
    let (mut s, mut h) = new();
    let root = setup("protocol");
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 16, "height": 16, "cas": ["Life"]}),
    ));
    // Relative paths start in the output folder; folders are made.
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_render",
        json!({"max_side": 64, "save_path": "a/b/shot.png"}),
    ));
    assert!(t.contains("Saved to"), "{t}");
    assert!(root.join("out/a/b/shot.png").is_file());
    let scene = root.join("out").join("c").join("scene.json");
    ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": path_json(&scene)}),
    ));
    assert!(scene.is_file());
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"path": "c/scene.json"}),
    ));
    assert!(t.contains("Replayed to its recorded hash"), "{t}");
    // Outside them: refused, saying where the tools may go.
    let outside = root.join("elsewhere").join("x.json");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": path_json(&outside)}),
    ));
    assert!(e.contains("the MCP tools may only write inside"), "{e}");
    assert!(!outside.exists());
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_render",
        json!({"save_path": "../escape.png"}),
    ));
    assert!(e.contains("may only write inside"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"path": path_json(&root.join("config").join("none.json"))}),
    ));
    assert!(e.contains("may only read inside"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_render",
        json!({"wallpaper": {"size": "320x200", "out": path_json(&outside.with_extension("png"))}}),
    ));
    assert!(e.contains("may only write inside"), "{e}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_render",
        json!({"wallpaper": {"size": "320x200", "out": "wall.png"}}),
    ));
    assert!(t.contains("\"width\": 320"), "{t}");
    assert!(root.join("out").join("wall.png").is_file());
    // files_list sees what was written; the folders outside are not its roots.
    let l = json_of(&call(&mut s, &mut h, "files_list", json!({"path": "a/b"})));
    assert_eq!(l["newest_first"][0]["name"], "shot.png");
    let e = fails(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"path": path_json(&root.join("config"))}),
    ));
    assert!(!e.is_empty());
}

#[test]
fn errors_are_useful_not_fatal() {
    let (mut s, mut h) = new();
    // Unknown tool: a JSON-RPC error.
    let r = s
        .handle(
            &mut h,
            &req(
                1,
                "tools/call",
                json!({"name": "new_world", "arguments": {}}),
            ),
        )
        .unwrap();
    assert_eq!(r["error"]["code"], protocol::INVALID_PARAMS);
    // Unknown argument: a tool error listing the arguments.
    let e = fails(&call(&mut s, &mut h, "world_step", json!({"gens": 3})));
    assert!(
        e.contains("has no argument `gens`") && e.contains("generations"),
        "{e}"
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"tiling": "nonsense"}),
    ));
    assert!(
        e.contains("unknown tiling") && e.contains("reference"),
        "{e}"
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_step",
        json!({"generations": 99_000_000_000u64}),
    ));
    assert!(e.contains("at most"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "reference",
        json!({"what": "rules", "search": "x"}),
    ));
    assert!(e.contains("does not apply"), "{e}");
    // Garbage and batches.
    assert!(
        s.handle_line(&mut h, "{not json")
            .unwrap()
            .contains("-32700")
    );
    let r = s
        .handle(
            &mut h,
            &json!([req(1, "ping", json!({})), req(2, "ping", json!({}))]),
        )
        .unwrap();
    assert_eq!(r.as_array().unwrap().len(), 2);
    // The world survived all of that.
    assert_eq!(h.world.generation, 0);
}

#[test]
fn sandpile_settles_and_reports() {
    let (mut s, mut h) = new();
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 21, "height": 21, "boundary": "dead",
               "cas": [{"rule": "Sandpile", "seed": "empty"}]}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": {"cells": [[10, 10, 3000]]}}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"commands": [{"op": "sandpile", "id": 1, "action": "stabilize"}]}),
    ));
    assert!(t.contains("stable: no cell can topple"), "{t}");
    let st = json_of(&call(&mut s, &mut h, "world_status", json!({})));
    let f = &st["fields"][0];
    assert_eq!(f["stable"], true);
    assert_eq!(
        f["population"].as_u64().unwrap() + f["grains_lost_off_edges"].as_u64().unwrap(),
        3000
    );
    // stop_when_settled runs without a count.
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 16, "height": 16, "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    ok(&call(
        &mut s,
        &mut h,
        "world_edit",
        json!({"cells": {"pattern": "OO\nOO"}}),
    ));
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_step",
        json!({"stop_when_settled": true}),
    ));
    assert!(t.contains("settled"), "{t}");
}

#[test]
fn run_script_with_limits_and_progress() {
    let (mut s, mut h) = new();
    let t = ok(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let w = world(#{ tiling: \"square\", size: [41, 30], boundary: \"dead\" });\nw.add_ca(\"W30\");\nlet r = w.run(#{ until: \"done\" });\nprint(`rows ${r.generations} ${params.tag}`);",
               "params": {"tag": "x"}}),
    ));
    assert!(t.contains("rows 29 x"), "{t}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let x = ;"}),
    ));
    assert!(e.contains("error"), "{e}");
    assert!(
        ok(&call(
            &mut s,
            &mut h,
            "run_script",
            json!({"script": "let x = 1;", "check_only": true}),
        ))
        .contains("no syntax errors")
    );
    // Scripts write only inside the output folder (the path as a Rhai string, escaped).
    let outside = setup("protocol").join("elsewhere").join("x.png");
    let script = format!(
        "let w = world(#{{ size: [16, 16] }}); w.save_png({});",
        serde_json::to_string(&outside.display().to_string()).unwrap()
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": script}),
    ));
    assert!(e.contains("only write inside"), "{e}");
    assert!(!outside.exists());
    // Progress, when the client asks for it.
    let sent = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = sent.clone();
    s.notifier = Some(Arc::new(move |v: Value| sink.lock().unwrap().push(v)));
    let script = "for i in 0..=4 { progress(i, 4, `item ${i}`); }\nprint(\"done\");";
    ok(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": script}),
    ));
    assert!(sent.lock().unwrap().is_empty());
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
    }
}

#[test]
fn help_and_reference() {
    let (mut s, mut h) = new();
    let t = ok(&call(&mut s, &mut h, "help", json!({})));
    for tool in TOOLS {
        assert!(t.contains(tool), "the overview names {tool}");
    }
    for topic in ["world", "library", "searches", "scripts", "packs"] {
        let t = ok(&call(&mut s, &mut h, "help", json!({"topic": topic})));
        assert!(t.len() > 500, "{topic}");
    }
    let t = ok(&call(
        &mut s,
        &mut h,
        "reference",
        json!({"what": "rules", "family": "Generations"}),
    ));
    assert!(t.contains("Brian's Brain | B2/S/C3"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "reference",
        json!({"what": "tilings"}),
    ));
    assert!(t.contains("- hex |") && t.contains("k-uniform"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "reference",
        json!({"what": "tilings", "search": "k2-00", "limit": 2}),
    ));
    assert!(t.contains("k2-001"), "{t}");
    for w in ["isohedral", "palettes", "seeds", "commands"] {
        let t = ok(&call(&mut s, &mut h, "reference", json!({"what": w})));
        assert!(t.len() > 200, "{w}: {t}");
    }
    let e = fails(&call(&mut s, &mut h, "help", json!({"page": "nope/nope"})));
    assert!(e.contains("pages"), "{e}");
}
