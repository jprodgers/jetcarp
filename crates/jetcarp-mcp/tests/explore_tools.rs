//! The MCP explore tools on the operations: explore_start (in the background here, where the
//! server cannot start a process of its own), explore_status, explore_stop, a continued search,
//! and the run records through explore_query, explore_sheet, explore_scene, explore_review, and
//! explore_try. One test in this binary, since it points JETCARP_LIBRARY and JETCARP_OUT at a
//! scratch folder.

use jetcarp_mcp::{Server, Session};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn raw(server: &mut Server, host: &mut Session, name: &str, args: Value) -> Value {
    server
        .handle(
            host,
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                    "params": {"name": name, "arguments": args}}),
        )
        .expect("reply")["result"]
        .clone()
}

fn text(res: &Value) -> String {
    res["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn call(server: &mut Server, host: &mut Session, name: &str, args: Value) -> Value {
    let res = raw(server, host, name, args);
    assert_eq!(res["isError"], false, "{name} failed: {}", text(&res));
    let t = text(&res);
    serde_json::from_str(&t).unwrap_or(Value::String(t))
}

fn status_until(s: &mut Server, h: &mut Session, job: i64, ok: impl Fn(&Value) -> bool) -> Value {
    let t0 = Instant::now();
    loop {
        let st = call(s, h, "explore_status", json!({"job": job}));
        if ok(&st) {
            return st;
        }
        assert!(t0.elapsed() < Duration::from_secs(180), "never: {st}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn explore_tools_run_a_search_and_read_its_runs() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mcp-explore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    // SAFETY: the only test in this binary, before any thread reads the environment.
    unsafe {
        std::env::set_var("JETCARP_LIBRARY", root.join("lib"));
        std::env::set_var("JETCARP_OUT", root.join("out"));
        std::env::set_var("JETCARP_DATA_DIR", root.join("data"));
    }
    let (mut s, mut h) = (Server::new(), Session::new());
    // No explore job yet.
    let r = raw(&mut s, &mut h, "explore_query", json!({}));
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("no explore job"), "{}", text(&r));
    // Started in the background (this server cannot detach), on the CPU (no GPU here).
    let small = json!({"minutes": 5, "tilings": "square,hex", "families": "life_like,cyclic",
                       "screen_cells": 400, "screen_gens": 40, "confirm_cells": 900,
                       "confirm_gens": 60, "cpu_threads": 2, "label": "mcp explore"});
    let r = call(&mut s, &mut h, "explore_start", small.clone());
    assert_eq!(
        (&r["mode"], &r["started"]),
        (&json!("background"), &json!(true)),
        "{r}"
    );
    let job = r["job"].as_i64().unwrap();
    let st = status_until(&mut s, &mut h, job, |v| {
        v["screened"].as_u64().unwrap_or(0) >= 100
    });
    assert_eq!(st["status"], "running");
    assert!(
        st["backend"].as_str().unwrap().starts_with("cpu only"),
        "{st}"
    );
    // Stopped, then the newest explore job is the default.
    let r = call(&mut s, &mut h, "explore_stop", json!({}));
    assert_eq!((&r["job"], &r["stopping"]), (&json!(job), &json!(true)));
    let st = status_until(&mut s, &mut h, job, |v| v["status"] == "paused");
    let n = st["screened"].as_u64().unwrap();
    assert!(st["families"].as_array().unwrap().len() == 2, "{st}");
    let l = call(&mut s, &mut h, "explore_status", json!({"list": true}));
    assert_eq!(l["jobs"][0]["job"], job);
    // Its runs: listed, drawn, opened, reviewed.
    let q = call(&mut s, &mut h, "explore_query", json!({"limit": 5}));
    let runs = q["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 5);
    let id = runs[0]["id"].as_i64().unwrap();
    let q = call(
        &mut s,
        &mut h,
        "explore_query",
        json!({"job": job, "family": "cyclic", "limit": 3}),
    );
    assert!(
        q["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["family"] == "cyclic")
    );
    let r = raw(
        &mut s,
        &mut h,
        "explore_sheet",
        json!({"job": job, "ids": [id], "px": 64}),
    );
    assert_eq!(r["isError"], false, "{}", text(&r));
    assert_eq!(r["content"][1]["mimeType"], "image/png");
    let sc = call(&mut s, &mut h, "explore_scene", json!({"id": id}));
    assert_eq!(sc["cas"][0]["rule"], runs[0]["rule"], "{sc}");
    let r = call(
        &mut s,
        &mut h,
        "explore_review",
        json!({"id": id, "verdict": "keep", "note": "lively"}),
    );
    assert!(r.as_str().unwrap().contains("recorded keep"), "{r}");
    let q = call(
        &mut s,
        &mut h,
        "explore_query",
        json!({"job": job, "reviewed": "keep"}),
    );
    assert_eq!(q["runs"][0]["id"], id);
    let r = raw(
        &mut s,
        &mut h,
        "explore_try",
        json!({"tiling": "hex", "rule": "B2/S34", "cells": 400, "gens": 40, "px": 64}),
    );
    assert_eq!(r["isError"], false, "{}", text(&r));
    let t: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(t["family"], "life_like");
    // Continued: it carries on counting from the saved archive.
    let r = call(
        &mut s,
        &mut h,
        "explore_start",
        json!({"resume": job, "minutes": 5, "cpu_threads": 2}),
    );
    assert_eq!((&r["job"], &r["started"]), (&json!(job), &json!(false)));
    status_until(&mut s, &mut h, job, |v| {
        v["screened"].as_u64().unwrap_or(0) >= n + 50
    });
    call(&mut s, &mut h, "explore_stop", json!({"job": job}));
    let st = status_until(&mut s, &mut h, job, |v| v["status"] == "paused");
    assert!(st["screened"].as_u64().unwrap() >= n + 50);
}
