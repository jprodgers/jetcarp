//! MCP jobs, ants, piles, collisions, files, and cancellation: ants_start and pile_start in the
//! background, job_status / job_stop over every kind, the ant records through library_list,
//! library_get, library_update, library_promote, and media_render, library_collide,
//! files_list inside its roots only, and notifications/cancelled stopping a long call (in the
//! server and through the stdio binary). One test in this binary, since it points JETCARP_LIBRARY
//! and JETCARP_OUT at temp folders.

use jetcarp_mcp::{Server, Session};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
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

fn wait(s: &mut Server, h: &mut Session, job: i64, want: &str) -> Value {
    let t0 = Instant::now();
    loop {
        let st = call(s, h, "job_status", json!({"job": job}));
        if st["status"] == want {
            return st;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(180),
            "job {job} never became {want}: {st}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn jobs_ants_piles_files_and_cancel_over_mcp() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-mcp-jobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    // SAFETY: the only test in this binary, before any thread reads the environment.
    unsafe {
        std::env::set_var("JETCARP_LIBRARY", root.join("lib"));
        std::env::set_var("JETCARP_OUT", root.join("out"));
    }
    let (mut s, mut h) = (Server::new(), Session::new());

    // Ants: Langton's ant and a hexagon string, in the background.
    let r = call(
        &mut s,
        &mut h,
        "ants_start",
        json!({"tilings": "square,hex", "rules": ["RL", "LNL"], "steps": 300000}),
    );
    let ants = r["job"].as_i64().unwrap();
    let st = wait(&mut s, &mut h, ants, "done");
    assert_eq!(st["kind"], "ants");
    assert_eq!(st["runs_done"], 4, "{st}");
    let l = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"kind": "ant", "outcome": "highway", "tiling": "square"}),
    );
    let rl = &l["ants"][0];
    assert_eq!(rl["rule"], "Ant RL", "{l}");
    assert_eq!(rl["period"], 104);
    assert_eq!(rl["onset"], 9977);
    let id = rl["id"].as_str().unwrap().to_string();
    let g = raw(&mut s, &mut h, "library_get", json!({"id": id}));
    assert!(text(&g).contains("highway p104"), "{}", text(&g));
    let u = call(
        &mut s,
        &mut h,
        "library_update",
        json!({"id": id, "name": "Langton's highway", "rating": 5}),
    );
    assert_eq!(u["name"], "Langton's highway");
    let m = call(&mut s, &mut h, "media_render", json!({"ant": id}));
    assert_eq!(m["done"][0]["mode"], "alone", "{m}");
    let p = call(&mut s, &mut h, "library_promote", json!({"id": id}));
    assert_eq!(p["entry"], format!("find:{id}"));
    let q = call(
        &mut s,
        &mut h,
        "library_list",
        json!({"kind": "quickstart"}),
    );
    assert!(q.to_string().contains(&id), "{q}");

    // A small pile to the end, then its files.
    let r = call(
        &mut s,
        &mut h,
        "pile_start",
        json!({"tiling": "hex", "grains": 20000}),
    );
    let pile = r["job"].as_i64().unwrap();
    let st = wait(&mut s, &mut h, pile, "done");
    assert_eq!(st["kind"], "pile");
    assert_eq!(st["grains_done"], 20000, "{st}");
    let png = st["png"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&png).is_file());
    let f = call(&mut s, &mut h, "files_list", json!({"path": "piles"}));
    assert_eq!(f["shown"], 1, "{f}");
    assert!(f.to_string().contains("pile.json"), "{f}");
    let folder = st["folder"].as_str().unwrap();
    let meta = raw(
        &mut s,
        &mut h,
        "files_list",
        json!({"file": format!("{folder}/pile.json")}),
    );
    assert!(text(&meta).contains("20000"), "{}", text(&meta));
    // Outside the roots, and up out of them, is refused; a binary file is not shown.
    for bad in [
        json!({"path": "/etc"}),
        json!({"path": "../.."}),
        json!({"file": png}),
    ] {
        let r = raw(&mut s, &mut h, "files_list", bad.clone());
        assert_eq!(r["isError"], true, "{bad}: {}", text(&r));
    }
    let c = call(&mut s, &mut h, "files_list", json!({"root": "catalog"}));
    assert!(c.to_string().contains("library.sqlite"), "{c}");

    // A big pile, stopped, then resumed.
    let r = call(
        &mut s,
        &mut h,
        "pile_start",
        json!({"tiling": "square", "grains": "1e8"}),
    );
    let big = r["job"].as_i64().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    call(&mut s, &mut h, "job_stop", json!({"job": big}));
    let st = wait(&mut s, &mut h, big, "paused");
    assert!(st["step"].as_u64().unwrap() < st["steps"].as_u64().unwrap());
    let all = call(&mut s, &mut h, "job_status", json!({}));
    let kinds: Vec<&str> = all["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|j| j["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"ants") && kinds.contains(&"pile"), "{all}");

    // Collisions: a block hit by gliders, after a search stored both.
    call(
        &mut s,
        &mut h,
        "library_search",
        json!({"rules": ["B3/S23"], "soups": 24, "gens": 1500}),
    );
    let c = call(
        &mut s,
        &mut h,
        "library_collide",
        json!({"a": "block", "b": "glider", "offsets": 3, "gens": 512}),
    );
    assert!(c["outcomes"].as_object().unwrap().len() >= 2, "{c}");

    // A small evolutionary search to its end.
    let r = call(
        &mut s,
        &mut h,
        "evolve_start",
        json!({"tiling": "hex", "pop": 8, "gens": 3, "radius": 8, "horizon": 60, "threads": 2}),
    );
    let ev = r["job"].as_i64().unwrap();
    let st = wait(&mut s, &mut h, ev, "done");
    assert_eq!(st["kind"], "evolve");
    assert_eq!(st["generation"], 3, "{st}");

    // A mover followed on a sliding window to 1,500 rings and stored; a long one stopped and
    // resumed.
    let r = call(&mut s, &mut h, "mover_start", json!({"rings": 1500}));
    let mv = r["job"].as_i64().unwrap();
    let st = wait(&mut s, &mut h, mv, "done");
    assert_eq!(st["kind"], "mover");
    assert!(st["rings"].as_u64().unwrap() >= 1500, "{st}");
    let id = st["record"].as_str().unwrap().to_string();
    let rec = call(&mut s, &mut h, "library_get", json!({"id": id}));
    assert!(rec.to_string().contains("sliding"), "{rec}");
    let r = call(&mut s, &mut h, "mover_start", json!({"rings": 10000000}));
    let mv = r["job"].as_i64().unwrap();
    // Once under way (a step written), a stop runs on to the next window and checkpoints there.
    let t0 = Instant::now();
    while call(&mut s, &mut h, "job_status", json!({"job": mv}))["generation"]
        .as_u64()
        .unwrap_or(0)
        == 0
    {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "mover {mv} never started"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    call(&mut s, &mut h, "job_stop", json!({"job": mv}));
    let st = wait(&mut s, &mut h, mv, "paused");
    let g = st["generation"].as_u64().unwrap();
    assert!(g > 4096, "{st}");
    call(&mut s, &mut h, "mover_start", json!({"resume": mv}));
    std::thread::sleep(Duration::from_millis(300));
    call(&mut s, &mut h, "job_stop", json!({"job": mv}));
    let st = wait(&mut s, &mut h, mv, "paused");
    assert!(st["generation"].as_u64().unwrap() > g, "{st}");

    // Cancellation in the server: a script that never ends stops when its request is cancelled.
    let cancels = s.cancels.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let msg = json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                         "params": {"requestId": 7, "reason": "test"}});
        assert!(jetcarp_mcp::protocol::cancel_request(&cancels, &msg));
    });
    let t0 = Instant::now();
    let r = raw(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let i = 0; loop { i += 1; }", "max_seconds": 60}),
    );
    t.join().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(10));
    assert!(text(&r).contains("cancelled"), "{}", text(&r));

    // And through the stdio server, whose stdin is read while the call runs.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_jetcarp-mcp"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "step",
               "arguments": {"generations": 1000000}}})
    )
    .unwrap();
    // The first call answers at once (the default world has no CA to step); the second runs
    // until it is cancelled.
    let mut line = String::new();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "run_script",
               "arguments": {"script": "loop { }", "max_seconds": 60}}})
    )
    .unwrap();
    stdin.flush().unwrap();
    out.read_line(&mut line).unwrap();
    assert!(line.contains("\"id\":1"), "{line}");
    std::thread::sleep(Duration::from_millis(400));
    let t0 = Instant::now();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2}})
    )
    .unwrap();
    stdin.flush().unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    assert!(
        line.contains("\"id\":2") && line.contains("cancelled"),
        "{line}"
    );
    assert!(t0.elapsed() < Duration::from_secs(10));
    drop(stdin);
    let _ = child.wait();
}
