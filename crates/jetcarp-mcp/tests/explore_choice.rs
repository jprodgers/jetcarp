//! An explore search over MCP told which tilings and rule families to search (#239): its spec
//! keeps the choice, its runs keep to it, and isotropic rules come only when named; a choice
//! that leaves nothing to search is refused with what is wrong.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session};
use serde_json::json;

#[test]
fn an_explore_search_keeps_to_the_tilings_and_families_chosen() {
    setup("explore_choice");
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = call(
        &mut s,
        &mut h,
        "search_start",
        json!({"explore": {"tiling_groups": ["square", "pentagon"],
                           "families": ["isotropic", "generations"],
                           "skip_tilings": "fourfold",
                           "screen_cells": 600, "screen_gens": 80,
                           "confirm_cells": 600, "confirm_gens": 80, "seed": 9},
               "gpu": false, "threads": 2, "budget": {"items": 150}, "wait_s": 300}),
    );
    let facts = last_json(&r);
    assert_eq!(facts["status"], "paused", "{}", text(&r));
    let job = facts["job"].as_i64().unwrap();
    // The job's report says what it searches, from its spec.
    let st = text(&call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": job, "report": true}),
    ));
    assert!(
        st.contains("square, pentagon tilings but fourfold, isotropic, generations"),
        "{st}"
    );
    let runs = json_of(&call(
        &mut s,
        &mut h,
        "search_runs",
        json!({"job": job, "limit": 0}),
    ));
    let list = runs["runs"].as_array().unwrap();
    assert!(list.len() >= 150, "{}", list.len());
    let mut isotropic = 0;
    for r in list {
        let (f, t) = (r["family"].as_str().unwrap(), r["tiling"].as_str().unwrap());
        assert!(f == "isotropic" || f == "generations", "{f}");
        assert!(["square", "quad", "glide", "cairo"].contains(&t), "{t}");
        if f == "isotropic" {
            assert_eq!(t, "square");
            isotropic += 1;
        }
    }
    assert!(isotropic > 0, "no isotropic run");
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"explore": {"tiling_groups": "pentagon", "families": "isotropic"}}),
    ));
    assert!(
        e.contains("isotropic rules run only on the square tiling"),
        "{e}"
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"explore": {"skip_tiling_groups": "squares"}}),
    ));
    assert!(e.contains("no tiling group 'squares'"), "{e}");
}
