//! `search.start` and `jobs.*`: searches of every strategy as catalog jobs, run here (to their
//! end or their budget), in the background, carried on, followed, stopped, removed, and
//! reported on.

mod common;

use common::{fails, scratch};
use jetcarp_api::{Ctx, ErrorKind, call_json};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn ctx(dir: &std::path::Path) -> Ctx {
    Ctx::new()
        .with_library(Some(dir.join("lib")))
        .with_out_root(Some(dir.join("out")))
}

fn start(cx: &Ctx, input: Value) -> Value {
    call_json(cx, "search.start", input).unwrap()
}

#[test]
fn hunts_run_to_their_budget_resume_and_report() {
    let dir = scratch("jobs-hunt");
    let cx = ctx(&dir);
    let spec = json!({"rules": "B3/S23", "soups": 120, "seed": 3});
    let r = start(
        &cx,
        json!({"strategy": "soup", "spec": spec, "budget": {"items": 40}, "threads": 2,
               "label": "t · hunt"}),
    );
    assert_eq!(
        (&r["kind"], &r["status"], &r["stopped"], &r["created"]),
        (
            &json!("hunt"),
            &json!("paused"),
            &json!("budget"),
            &json!(true)
        ),
        "{r}"
    );
    assert_eq!(r["progress"]["soups_done"], 40);
    assert!(r["line"].as_str().unwrap().contains("40/120"), "{r}");
    let job = r["job"].as_i64().unwrap();
    // Carried on to its end: the same finds as an uninterrupted hunt.
    let done = call_json(&cx, "jobs.resume", json!({"job": job, "threads": 3})).unwrap();
    assert_eq!(
        (&done["status"], &done["created"]),
        (&json!("done"), &json!(false))
    );
    assert_eq!(done["progress"]["soups_done"], 120);
    let whole = start(&cx, json!({"strategy": "hunt", "spec": spec}));
    for k in ["objects", "distinct", "by_class", "settled", "exploded"] {
        assert_eq!(done["progress"][k], whole["progress"][k], "{k}");
    }
    // A labelled job is found again: done, it is given as it is.
    let again = start(
        &cx,
        json!({"strategy": "hunt", "spec": {"rules": "B36/S23"}, "label": "t · hunt",
               "reuse_label": true}),
    );
    assert_eq!(
        (&again["job"], &again["mode"], &again["created"]),
        (&json!(job), &json!("none"), &json!(false))
    );
    // The list, newest first, and one job with its finds.
    let l = call_json(&cx, "jobs.list", json!({})).unwrap();
    let jobs = l["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[1]["job"], job);
    assert_eq!(jobs[1]["label"], "t · hunt");
    let l = call_json(
        &cx,
        "jobs.list",
        json!({"label": "t · hunt", "kind": "soup"}),
    )
    .unwrap();
    assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
    let v = call_json(&cx, "jobs.status", json!({"job": job, "finds": 3})).unwrap();
    assert_eq!((&v["kind"], &v["status"]), (&json!("hunt"), &json!("done")));
    assert_eq!(v["done"], 120);
    assert!(
        v["title"].as_str().unwrap().starts_with("t · hunt: B3/S23"),
        "{v}"
    );
    assert!(!v["finds"]["newest"].as_array().unwrap().is_empty(), "{v}");
    let st = call_json(&cx, "jobs.status", json!({"kind": "hunt"})).unwrap();
    assert_eq!(st["jobs"].as_array().unwrap().len(), 2);
    // The report: what was searched and found.
    let rep = call_json(&cx, "jobs.report", json!({"job": job})).unwrap();
    let md = rep["markdown"].as_str().unwrap();
    for want in [
        "# Hunt ",
        "## Jobs",
        "| hunt | t · hunt | done | 120/120 soups |",
        "## Hunts: per hunted rule",
        "| B3/S23 | 120 |",
    ] {
        assert!(md.contains(want), "{want}:\n{md}");
    }
    let rep = call_json(&cx, "jobs.report", json!({"prefix": "t · "})).unwrap();
    assert_eq!(rep["jobs"], json!([job]));
    // A done job is not stopped; removing one takes its sightings.
    let s = call_json(&cx, "jobs.stop", json!({"job": job})).unwrap();
    assert_eq!(
        (&s["stopping"], &s["status"]),
        (&json!(false), &json!("done"))
    );
    let rm = call_json(&cx, "jobs.remove", json!({"job": whole["job"]})).unwrap();
    assert!(rm["sightings"].as_u64().unwrap() > 0, "{rm}");
    let l = call_json(&cx, "jobs.list", json!({})).unwrap();
    assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
    // Errors name what is wrong.
    let e = fails(
        call_json(&cx, "search.start", json!({"strategy": "nonesuch"})),
        ErrorKind::Invalid,
    );
    assert!(e.hint.unwrap().contains("collide"));
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "hunt", "spec": {"bogus": 1}}),
        ),
        ErrorKind::Invalid,
    );
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "ants", "resume": job}),
        ),
        ErrorKind::Invalid,
    );
    fails(
        call_json(&cx, "jobs.status", json!({"job": 999})),
        ErrorKind::NotFound,
    );
    // This context's platform cannot start a process of its own.
    let paused = start(
        &cx,
        json!({"strategy": "hunt", "spec": {"soups": 50}, "budget": {"items": 5}}),
    );
    assert_eq!(paused["status"], "paused");
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"resume": paused["job"], "detach": true}),
        ),
        ErrorKind::Engine,
    );
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "ants", "spec": {"rules": "RL"}, "budget": {"minutes": 0}}),
        ),
        ErrorKind::Invalid,
    );
}

#[test]
fn background_jobs_are_followed_stopped_and_waited_for() {
    let dir = scratch("jobs-background");
    let cx = ctx(&dir);
    let r = start(
        &cx,
        json!({"strategy": "hunt", "spec": {"soups": 0}, "threads": 2, "background": true}),
    );
    assert_eq!(
        (&r["mode"], &r["status"]),
        (&json!("background"), &json!("running"))
    );
    let job = r["job"].as_i64().unwrap();
    let t0 = Instant::now();
    loop {
        let v = call_json(&cx, "jobs.status", json!({"job": job})).unwrap();
        assert_eq!(v["here"], true, "{v}");
        if v["progress"]["soups_done"].as_u64().unwrap_or(0) > 0 {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no progress: {v}");
        std::thread::sleep(Duration::from_millis(50));
    }
    // It cannot be started twice, or removed, while it runs.
    fails(
        call_json(&cx, "jobs.resume", json!({"job": job})),
        ErrorKind::Conflict,
    );
    fails(
        call_json(&cx, "jobs.remove", json!({"job": job})),
        ErrorKind::Conflict,
    );
    let s = call_json(&cx, "jobs.stop", json!({"job": job})).unwrap();
    assert_eq!((&s["stopping"], &s["here"]), (&json!(true), &json!(true)));
    let v = call_json(&cx, "jobs.status", json!({"job": job, "wait_s": 60})).unwrap();
    assert_eq!(v["status"], "paused", "{v}");
    assert!(v["waited_s"].as_f64().is_some());
    // Waiting on a job that is not running answers at once.
    let t1 = Instant::now();
    call_json(&cx, "jobs.status", json!({"job": job, "wait_s": 30})).unwrap();
    assert!(t1.elapsed() < Duration::from_secs(5));
    let rm = call_json(
        &cx,
        "jobs.remove",
        json!({"job": job, "delete_results": true}),
    )
    .unwrap();
    assert!(!rm["patterns"].as_array().unwrap().is_empty(), "{rm}");
    fails(
        call_json(&cx, "jobs.status", json!({"job": job})),
        ErrorKind::NotFound,
    );
}

#[test]
fn bounded_searches_collisions_and_censuses_give_their_results_and_files() {
    let dir = scratch("jobs-results");
    let cx = ctx(&dir);
    let f = |name: &str| dir.join(name).display().to_string();
    let s = start(
        &cx,
        json!({"strategy": "bounded", "label": "r · search", "spec": {"rules": ["B3/S23"],
               "soups": 8, "size": [32, 32], "gens": 600}, "files": {"json": f("s.json")}}),
    );
    assert_eq!(
        (&s["kind"], &s["status"]),
        (&json!("search"), &json!("done"))
    );
    let res = &s["result"];
    assert_eq!(res["stored"], true);
    let found = res["report"]["groups"][0]["found"].as_array().unwrap();
    assert!(!found.is_empty());
    assert_eq!(res["added"].as_array().unwrap().len(), found.len());
    assert!(found.iter().all(|x| x["new"] == true));
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("s.json")).unwrap()).unwrap();
    assert_eq!(
        saved["groups"][0]["found"].as_array().unwrap().len(),
        found.len()
    );
    // Only reported: nothing stored, the job kept.
    let n = cx.library().count(&Default::default()).unwrap();
    let r = start(
        &cx,
        json!({"strategy": "search", "spec": {"rules": "B36/S23", "soups": 4, "size": "32x32",
               "gens": 300, "store": false}}),
    );
    assert_eq!(r["result"]["stored"], false);
    assert_eq!(cx.library().count(&Default::default()).unwrap(), n);
    // A collision: its runs, products, and table.
    let c = start(
        &cx,
        json!({"strategy": "collide", "label": "r · collide", "spec": {"a": "block",
               "b": "glider", "offsets": 3, "gens": 256}, "files": {"csv": f("c.csv")}}),
    );
    assert_eq!(c["status"], "done", "{c}");
    let rep = &c["result"]["report"];
    assert!(rep["counts"].as_object().unwrap().len() >= 2, "{rep}");
    let runs = rep["runs"].as_array().unwrap().len();
    assert_eq!(runs as u64, c["progress"]["runs"].as_u64().unwrap());
    let csv = std::fs::read_to_string(dir.join("c.csv")).unwrap();
    assert!(csv.starts_with("dx,dy,phase,outcome,gens,result,products\n"));
    assert_eq!(csv.lines().count(), runs + 1);
    // A census: its rules, CSV, and JSON.
    let k = start(
        &cx,
        json!({"strategy": "census", "label": "r · census", "spec": {"tiling": "hex",
               "size": "16x16", "rules": "B2/S34 B3/S23", "soups": 2, "gens": 100},
               "files": {"csv": f("k.csv"), "json": f("k.json")}}),
    );
    assert_eq!(k["status"], "done");
    assert_eq!(k["result"]["rules"].as_array().unwrap().len(), 2);
    assert!(k["result"]["rules"][0].get("results").is_none());
    let csv = std::fs::read_to_string(dir.join("k.csv")).unwrap();
    assert_eq!(
        csv.lines().filter(|l| !l.starts_with('#')).count(),
        3,
        "{csv}"
    );
    let full: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("k.json")).unwrap()).unwrap();
    assert!(full["rules"][0]["results"].as_array().is_some());
    // B/S rules over digits: every one when there are few.
    let all = start(
        &cx,
        json!({"strategy": "census", "spec": {"tiling": "hex", "size": "16x16", "birth": "2",
               "survive": "3-4", "soups": 1, "gens": 50}, "budget": {"items": 3}}),
    );
    assert_eq!(all["status"], "paused");
    assert_eq!(all["progress"]["rules_total"], 8);
    // The report of the labelled jobs.
    let rep = call_json(&cx, "jobs.report", json!({"prefix": "r · ", "top": 2})).unwrap();
    let md = rep["markdown"].as_str().unwrap();
    for want in [
        "# Jobs labelled `r · …`",
        "## Censuses: rules by fate",
        "## Soup searches",
        "## Collisions",
        "B2/S34",
    ] {
        assert!(md.contains(want), "{want}:\n{md}");
    }
    assert_eq!(rep["jobs"].as_array().unwrap().len(), 3);
}

#[test]
fn ants_piles_evolve_searches_and_movers_run_as_jobs() {
    let dir = scratch("jobs-kinds");
    let cx = ctx(&dir);
    let a = start(
        &cx,
        json!({"strategy": "ants", "spec": {"rules": "RL LLRR", "steps": 20000}, "threads": 2}),
    );
    assert_eq!(
        (&a["status"], &a["progress"]["runs_done"]),
        (&json!("done"), &json!(2))
    );
    // A pile in its own folder; the folder again finds its job.
    let p = start(
        &cx,
        json!({"strategy": "pile", "spec": {"tiling": "hex", "grains": 2000, "preview": 64}}),
    );
    assert_eq!(p["status"], "done", "{p}");
    assert_eq!(p["result"]["grains"], 2000);
    let folder = p["progress"]["folder"].as_str().unwrap().to_string();
    assert!(folder.starts_with(&dir.join("out/piles").display().to_string()));
    assert!(std::path::Path::new(&folder).join("pile.png").is_file());
    let again = start(
        &cx,
        json!({"strategy": "pile", "spec": {"folder": folder, "tiling": "hex", "grains": 2000}}),
    );
    assert_eq!(
        (&again["job"], &again["created"], &again["mode"]),
        (&p["job"], &json!(false), &json!("none"))
    );
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "pile", "spec": {"folder": folder, "grains": 3000}}),
        ),
        ErrorKind::Conflict,
    );
    // An evolve search stopped after a generation and carried on: its files at the end.
    let ev = dir.join("evolve").display().to_string();
    let e1 = start(
        &cx,
        json!({"strategy": "evolve", "spec": {"tiling": "hex", "pop": 8, "gens": 3, "radius": 8,
               "horizon": 60, "folder": ev}, "budget": {"items": 1}, "threads": 2}),
    );
    assert_eq!(
        (&e1["status"], &e1["stopped"]),
        (&json!("paused"), &json!("budget"))
    );
    let e2 = call_json(&cx, "jobs.resume", json!({"job": e1["job"], "threads": 2})).unwrap();
    assert_eq!(e2["status"], "done");
    assert_eq!(
        e2["result"]["report"]["generations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(dir.join("evolve/fitness.csv").is_file());
    // A mover on a sliding window, its tracker's report the result.
    let m = start(
        &cx,
        json!({"strategy": "movers", "spec": {"rings": 200}, "files": {"json": dir.join("m.json").display().to_string()}}),
    );
    assert_eq!(m["status"], "done", "{m}");
    assert!(m["progress"]["rings"].as_u64().unwrap() >= 200);
    assert_eq!(m["result"]["verified"], true, "{m}");
    assert!(dir.join("m.json").is_file());
    // Every kind in the list, with a line each.
    let l = call_json(&cx, "jobs.list", json!({})).unwrap();
    let kinds: Vec<&str> = l["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["mover", "evolve", "pile", "ants"]);
    assert!(
        l["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| !j["line"].as_str().unwrap().is_empty())
    );
    let rep = call_json(&cx, "jobs.report", json!({"job": e1["job"]})).unwrap();
    assert!(
        rep["markdown"]
            .as_str()
            .unwrap()
            .contains("## Evolutionary searches")
    );
}
