//! An explore search over MCP: started in the background (this server cannot detach), waited
//! for, then its runs read with search_runs: the listing, a contact sheet, a run's scene opened
//! in the world, a configuration tried, and a verdict recorded.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session};
use serde_json::{Value, json};

#[test]
fn an_explore_search_and_its_runs() {
    setup("search_runs");
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = call(
        &mut s,
        &mut h,
        "search_start",
        json!({"explore": {"tilings": "square,hex", "families": "life_like,cyclic",
                           "screen_cells": 1500, "screen_gens": 120,
                           "confirm_cells": 3000, "confirm_gens": 200},
               "gpu": false, "budget": {"items": 400}, "wait_s": 300}),
    );
    let facts = last_json(&r);
    assert_eq!(facts["kind"], "explore", "{}", text(&r));
    assert_eq!(facts["status"], "paused", "{}", text(&r));
    let job = facts["job"].as_i64().unwrap();
    let runs = json_of(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "limit": 5}),
    ));
    let list = runs["runs"].as_array().unwrap();
    assert!(!list.is_empty(), "{runs}");
    let ids: Vec<i64> = list.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    // Best first.
    let scores: Vec<f64> = list.iter().map(|r| r["score"].as_f64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    let f = json_of(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"family": "cyclic", "limit": 3}),
    ));
    assert!(
        f["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["family"] == "cyclic")
    );
    let sheet = call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "sheet": {"ids": &ids[..2], "px": 64}}),
    );
    assert!(ok(&sheet).contains("2 runs"));
    assert!(has_png(&sheet));
    let scene = json_of(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "scene": {"run": ids[0]}}),
    ));
    assert!(scene["tiling"].is_object(), "{scene}");
    let t = ok(&call(&mut s, &mut h, "world_new", json!({"scene": scene})));
    assert!(t.contains("Scene loaded"), "{t}");
    let t = call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"try": {"tiling": "hex", "rule": "B2/S34", "cells": 1000, "gens": 60, "px": 64}}),
    );
    let v: Value = json_of(&t);
    assert_eq!(v["family"], "life_like");
    assert!(has_png(&t));
    let t = ok(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "review": {"run": ids[0], "verdict": "keep", "note": "a test verdict"}}),
    ));
    assert!(t.contains("recorded keep"), "{t}");
    let k = json_of(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "reviewed": "keep"}),
    ));
    assert_eq!(k["runs"][0]["id"], ids[0]);
    // Filters belong to the listing, and one action at a time.
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"family": "cyclic", "scene": {"run": ids[0]}}),
    ));
    assert!(e.contains("for listing runs"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"scene": {"run": 1}, "sheet": {"ids": [1]}}),
    ));
    assert!(e.contains("give one of"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"review": {"run": ids[0], "verdict": "keep", "rater": "someone"}}),
    ));
    assert!(e.contains("rater"), "{e}");
}
