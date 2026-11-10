//! MCP hunt tools: start a hunt in the background, poll it, stop it, resume it, and see its
//! finds in the catalog. One test in this binary, since it points JETCARP_LIBRARY at a temp folder.

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
            "hunt {job} never became {want}: {st}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn hunts_start_report_stop_and_resume_over_mcp() {
    let dir = std::env::temp_dir().join(format!("jetcarp-mcp-hunt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: the only test in this binary, before any thread reads the environment.
    unsafe { std::env::set_var("JETCARP_LIBRARY", &dir) };
    let (mut s, mut h) = (Server::new(), Session::new());
    let r = call(
        &mut s,
        &mut h,
        "hunt_start",
        json!({"rules": ["B3/S23"], "soups": 0, "threads": 2}),
    );
    let job = r["job"].as_i64().unwrap();
    // It runs in the background until stopped.
    let t0 = Instant::now();
    loop {
        let st = call(&mut s, &mut h, "job_status", json!({"job": job}));
        if st["soups_done"].as_u64().unwrap_or(0) >= 200 {
            assert_eq!(st["status"], "running");
            assert!(st["line"].as_str().unwrap().contains("soups/s"), "{st}");
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "no progress: {st}");
        std::thread::sleep(Duration::from_millis(100));
    }
    call(&mut s, &mut h, "job_stop", json!({"job": job}));
    let st = wait(&mut s, &mut h, job, "paused");
    let done = st["soups_done"].as_u64().unwrap();
    assert!(st["by_class"]["spaceship"].as_u64().unwrap() > 0, "{st}");
    let list = call(&mut s, &mut h, "job_status", json!({"kind": "hunt"}));
    assert_eq!(list["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(list["jobs"][0]["kind"], "hunt");
    // The finds are in the catalog.
    let l = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"class": "spaceship"}),
    );
    assert!(l.to_string().contains("glider"), "{l}");
    // Resume: it carries on from its checkpoint.
    call(
        &mut s,
        &mut h,
        "hunt_start",
        json!({"resume": job, "threads": 2}),
    );
    let t1 = Instant::now();
    loop {
        let st = call(&mut s, &mut h, "job_status", json!({"job": job}));
        if st["soups_done"].as_u64().unwrap_or(0) > done {
            break;
        }
        assert!(
            t1.elapsed() < Duration::from_secs(120),
            "no progress after resume"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    call(&mut s, &mut h, "job_stop", json!({"job": job}));
    wait(&mut s, &mut h, job, "paused");

    // A finite hunt and one job_status call that waits for it instead of polling.
    let r = call(
        &mut s,
        &mut h,
        "hunt_start",
        json!({"rules": ["B36/S23"], "soups": 300, "threads": 2}),
    );
    let fin = r["job"].as_i64().unwrap();
    let st = call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": fin, "wait_s": 120}),
    );
    assert_eq!(st["status"], "done", "{st}");
    assert_eq!(st["soups_done"].as_u64(), Some(300), "{st}");
    assert!(st["waited_s"].as_u64().is_some(), "{st}");
    // wait_s on a job that is not running answers at once.
    let t2 = Instant::now();
    let st = call(
        &mut s,
        &mut h,
        "job_status",
        json!({"job": job, "wait_s": 60}),
    );
    assert_eq!(st["status"], "paused");
    assert!(t2.elapsed() < Duration::from_secs(5));
}
