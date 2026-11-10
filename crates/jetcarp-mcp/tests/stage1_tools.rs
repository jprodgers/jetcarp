//! MCP stage 1 tools: start a settings search in the background, poll it, stop and resume it,
//! list candidates, rate, and approve. One test in this binary, since it points JETCARP_LIBRARY at
//! a temp folder.

use jetcarp_mcp::{Server, Session};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn call(server: &mut Server, host: &mut Session, name: &str, args: Value) -> Value {
    let r = server
        .handle(
            host,
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                    "params": {"name": name, "arguments": args}}),
        )
        .expect("reply");
    let res = &r["result"];
    assert_eq!(res["isError"], false, "{name} failed: {res}");
    let t: String = res["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    serde_json::from_str(&t).unwrap_or(Value::String(t))
}

fn wait(s: &mut Server, h: &mut Session, job: i64, want: &str) -> Value {
    let t0 = Instant::now();
    loop {
        let st = call(s, h, "job_status", json!({"job": job}));
        if st["status"] == want {
            return st;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(120),
            "stage 1 job {job} never became {want}: {st}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn stage1_searches_start_stop_resume_and_review_over_mcp() {
    let dir = std::env::temp_dir().join(format!("jetcarp-mcp-stage1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: the only test in this binary, before any thread reads the environment.
    unsafe { std::env::set_var("JETCARP_LIBRARY", &dir) };
    let (mut s, mut h) = (Server::new(), Session::new());
    // Every hexagon rule (8,192) on small grids: long enough to stop in the middle.
    let r = call(
        &mut s,
        &mut h,
        "stage1_start",
        json!({"tilings": "hex", "grid": "24x24", "gens": 300, "soups": 2,
               "refine": false, "neighbours": false, "threads": 2}),
    );
    let job = r["job"].as_i64().unwrap();
    let t0 = Instant::now();
    loop {
        let st = call(&mut s, &mut h, "job_status", json!({"job": job}));
        if st["evaluated"].as_u64().unwrap_or(0) >= 100 {
            assert_eq!(st["status"], "running");
            assert!(st["line"].as_str().unwrap().contains("coarse"), "{st}");
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "no progress: {st}");
        std::thread::sleep(Duration::from_millis(50));
    }
    call(&mut s, &mut h, "job_stop", json!({"job": job}));
    let st = wait(&mut s, &mut h, job, "paused");
    let done = st["evaluated"].as_u64().unwrap();
    assert!((100..8192).contains(&done), "{st}");
    let list = call(&mut s, &mut h, "job_status", json!({"kind": "stage1"}));
    assert_eq!(list["jobs"].as_array().unwrap().len(), 1);
    // Resume to the end.
    call(&mut s, &mut h, "stage1_start", json!({"resume": job}));
    let st = wait(&mut s, &mut h, job, "done");
    assert_eq!(st["evaluated"], 8192, "{st}");
    // The best settings, any status.
    let top = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"kind": "setting", "status": ["stored", "candidate", "auto"], "limit": 3}),
    );
    let rows = top["settings"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows[0]["score"].as_f64() >= rows[1]["score"].as_f64());
    let id = rows[1]["id"].as_i64().unwrap();
    let r = call(
        &mut s,
        &mut h,
        "library_update",
        json!({"kind": "setting", "id": id, "rating": 5, "note": "lovely"}),
    );
    assert!(r["rating_id"].as_i64().unwrap() > 0);
    let r = call(
        &mut s,
        &mut h,
        "library_update",
        json!({"id": format!("setting-{id}"), "decision": "approve"}),
    );
    assert_eq!(r["status"], "approved");
    let ql = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"kind": "quickstart", "status": "approved"}),
    );
    assert!(ql.to_string().contains(&format!("setting-{id}")), "{ql}");
}
