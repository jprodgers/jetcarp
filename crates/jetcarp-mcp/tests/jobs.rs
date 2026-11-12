//! Searches as jobs over MCP: search_start for every strategy (in the background, with waits),
//! job_status (one job, its finds and report, the list), job_stop and resume, a wait the client
//! cancels; and what the searches leave: library_query and library_curate over patterns, ant
//! runs and Quick Start entries, media_render, and files_list.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session, protocol};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn new() -> (Server, Session) {
    setup("jobs");
    (Server::new(), Session::new())
}

/// Start a search and wait for its end; the facts after its summary.
fn run(s: &mut Server, h: &mut Session, args: Value) -> Value {
    let r = call(s, h, "search_start", args.clone());
    let facts = last_json(&r);
    assert_eq!(facts["status"], "done", "{args}: {}", text(&r));
    facts
}

#[test]
fn every_strategy_runs_in_the_background() {
    let (mut s, mut h) = new();
    let ants = run(
        &mut s,
        &mut h,
        json!({"ants": {"rules": ["RL"]}, "wait_s": 120}),
    );
    assert_eq!(ants["kind"], "ants");
    let pile = run(
        &mut s,
        &mut h,
        json!({"pile": {"tiling": "square", "grains": "1e4"}, "wait_s": 120}),
    );
    let folder = std::path::PathBuf::from(pile["folder"].as_str().unwrap());
    assert!(folder.join("pile.png").is_file());
    assert!(folder.starts_with(setup("jobs").join("out")));
    run(
        &mut s,
        &mut h,
        json!({"search": {"rules": ["B3/S23"], "tilings": "hex", "soups": 4, "width": 24, "height": 24, "gens": 200}, "wait_s": 120}),
    );
    run(
        &mut s,
        &mut h,
        json!({"census": {"rules": "B3/S23 B36/S23", "soups": 2, "size": "24x24", "gens": 100}, "wait_s": 120}),
    );
    run(
        &mut s,
        &mut h,
        json!({"mover": {"rings": 20}, "wait_s": 120}),
    );
    run(
        &mut s,
        &mut h,
        json!({"evolve": {"tiling": "square", "pop": 8, "gens": 2, "radius": 8, "horizon": 50}, "wait_s": 120}),
    );
    // A hunt gives the library a glider and a block; then they collide.
    run(
        &mut s,
        &mut h,
        json!({"hunt": {"rules": ["B3/S23"], "soups": 400}, "wait_s": 120}),
    );
    let c = call(
        &mut s,
        &mut h,
        "search_start",
        json!({"collide": {"a": "block", "b": "glider", "offsets": 2}, "wait_s": 120}),
    );
    assert!(ok(&c).contains("Collisions"), "{}", text(&c));
    // The list, newest first, and by kind.
    let l = json_of(&call(&mut s, &mut h, "job_status", json!({})));
    let kinds: Vec<&str> = l["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["kind"].as_str().unwrap())
        .collect();
    for k in [
        "ants", "pile", "search", "census", "mover", "evolve", "hunt", "collide",
    ] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    let l = json_of(&call(&mut s, &mut h, "job_status", json!({"kind": "ants"})));
    assert!(
        l["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["kind"] == "ants")
    );
    // Wrong settings are refused with the strategy's words.
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"hunt": {"rulez": ["B3/S23"]}}),
    ));
    assert!(
        e.contains("search_start hunt") && e.contains("rulez"),
        "{e}"
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"hunt": {}, "ants": {}}),
    ));
    assert!(e.contains("give one of"), "{e}");
    let e = fails(&call(&mut s, &mut h, "search_start", json!({})));
    assert!(e.contains("resume"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"hunt": {}, "detach": true}),
    ));
    assert!(e.contains("cannot start a process"), "{e}");
}

#[test]
fn stop_resume_finds_report_and_cancelled_waits() {
    let (mut s, mut h) = new();
    // A hunt until stopped.
    let r = json_of(&call(
        &mut s,
        &mut h,
        "search_start",
        json!({"hunt": {"rules": ["B3/S23"], "soups": 0}, "label": "until-stopped"}),
    ));
    let job = r["job"].as_i64().unwrap();
    assert_eq!(r["mode"], "background");
    // A wait the client cancels ends soon, with the job still running.
    let cancels = s.cancels.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        protocol::cancel_request(
            &cancels,
            &json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7}}),
        );
    });
    let t0 = Instant::now();
    let st = last_json(&call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": job, "wait_s": 600}),
    ));
    canceller.join().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(30), "the wait ended");
    assert_eq!(st["status"], "running");
    let stop = json_of(&call(&mut s, &mut h, "job_stop", json!({"job": job})));
    assert_eq!(stop["stopping"], true);
    assert!(
        stop["note"]
            .as_str()
            .unwrap()
            .contains("search_start {\"resume\"")
    );
    let t0 = Instant::now();
    loop {
        let st = last_json(&call(&mut s, &mut h, "job_status", json!({"job": job})));
        if st["status"] == "paused" {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "{st}");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Carried on for a budget of soups, it pauses again at the budget.
    let r = call(
        &mut s,
        &mut h,
        "search_start",
        json!({"resume": job, "budget": {"items": 200}, "wait_s": 120}),
    );
    let facts = last_json(&r);
    assert_eq!(facts["job"], job);
    assert_eq!(facts["status"], "paused", "{}", text(&r));
    // Its finds (pattern ids for library_query) and its report.
    let t = ok(&call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": job, "finds": 5}),
    ));
    assert!(t.contains("newest") && t.contains("glider"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": job, "report": true}),
    ));
    assert!(t.starts_with("# Hunt"), "{t}");
    let e = fails(&call(&mut s, &mut h, "job_status", json!({"report": true})));
    assert!(e.contains("needs the `job`"), "{e}");
    let e = fails(&call(&mut s, &mut h, "job_stop", json!({})));
    assert!(e.contains("job"), "{e}");
    // The job's resource.
    let r = s
        .handle(
            &mut h,
            &req(
                3,
                "resources/read",
                json!({"uri": format!("jetcarp://job/{job}")}),
            ),
        )
        .unwrap();
    let v: Value =
        serde_json::from_str(r["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["job"], job);
    assert_eq!(v["kind"], "hunt");
}

#[test]
fn the_library_is_read_and_curated() {
    let (mut s, mut h) = new();
    run(
        &mut s,
        &mut h,
        json!({"hunt": {"rules": ["B3/S23"], "soups": 300}, "wait_s": 120}),
    );
    run(
        &mut s,
        &mut h,
        json!({"ants": {"rules": ["RL", "LLRR"]}, "wait_s": 120}),
    );
    let l = json_of(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"class": "spaceship", "limit": "5"}),
    ));
    assert!(l["shown"].as_u64().unwrap() >= 1, "{l}");
    let glider = l["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "glider")
        .expect("a glider")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"id": "glider", "phases": 2}),
    ));
    assert!(t.contains(&glider) && t.contains("phase 1:"), "{t}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"id": "glider", "outcome": "highway"}),
    ));
    assert!(
        e.contains("do not apply") || e.contains("does not apply"),
        "{e}"
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"kind": "ant", "class": "spaceship"}),
    ));
    assert!(e.contains("ant runs"), "{e}");
    let ants = json_of(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"kind": "ant", "outcome": "highway"}),
    ));
    let ant = ants["ants"][0]["id"].as_str().unwrap().to_string();
    assert!(ant.starts_with("ant-"));
    let t = ok(&call(&mut s, &mut h, "library_query", json!({"id": ant})));
    assert!(t.contains("highway"), "{t}");
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"kind": "summary"}),
    ));
    assert!(t.contains("patterns") && t.contains("ants"), "{t}");
    // Curate: changes, then the Quick Start.
    let p = json_of(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": "glider", "changes": {"rating": 5, "add_tags": ["llm-reviewed"], "append_notes": "LLM: the c/4 glider"}}),
    ));
    assert_eq!(p["rating"], 5);
    assert_eq!(p["tags"], json!(["llm-reviewed"]));
    let a = json_of(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": ant, "changes": {"name": "Langton's highway"}}),
    ));
    assert_eq!(a["name"], "Langton's highway");
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"quickstart": {"pattern": "glider", "title": "A glider"}}),
    ));
    let handle = format!("find:{glider}");
    assert!(t.contains(&handle), "{t}");
    let q = json_of(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"kind": "quickstart", "text": "a glider"}),
    ));
    assert_eq!(q["entries"][0]["id"], json!(handle));
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": handle, "hidden": true}),
    ));
    assert!(t.contains("hidden"), "{t}");
    let q = json_of(&call(
        &mut s,
        &mut h,
        "library_query",
        json!({"kind": "quickstart", "hidden": true}),
    ));
    assert_eq!(q["entries"][0]["id"], json!(handle));
    let e = fails(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": "glider", "hidden": true}),
    ));
    assert!(e.contains("for Quick Start entries"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": "glider"}),
    ));
    assert!(e.contains("changes"), "{e}");
    let r = s
        .handle(
            &mut h,
            &req(
                4,
                "resources/read",
                json!({"uri": format!("jetcarp://pattern/{glider}")}),
            ),
        )
        .unwrap();
    assert!(
        r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"sightings\"")
    );
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"id": handle, "remove": true}),
    ));
    assert!(t.contains("removed"), "{t}");
    // The compounds pass: a job of its own, its report beside the library.
    let t = ok(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"compounds": {}}),
    ));
    assert!(t.contains("report:") && t.contains("compounds-"), "{t}");
    let l = json_of(&call(
        &mut s,
        &mut h,
        "job_status",
        json!({"kind": "compounds"}),
    ));
    assert_eq!(l["jobs"][0]["kind"], "compounds", "{l}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "library_curate",
        json!({"compounds": {"report": "/x.jsonl"}}),
    ));
    assert!(e.contains("not taken here"), "{e}");
    // Media, inline poster and all, into the library's media folder.
    let r = call(
        &mut s,
        &mut h,
        "media_render",
        json!({"pattern": "glider", "poster": true, "frames": false}),
    );
    let m = json_of(&r);
    assert!(has_png(&r));
    let poster = std::path::PathBuf::from(m["done"][0]["files"]["poster"].as_str().unwrap());
    assert!(poster.starts_with(setup("jobs").join("lib")));
    let l = json_of(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"root": "catalog", "path": "media"}),
    ));
    assert!(l["shown"].as_u64().unwrap() >= 1, "{l}");
}
