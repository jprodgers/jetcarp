//! A whole stdio session with the `jetcarp-mcp` binary, as an MCP client sees it: the handshake,
//! tools/list and resources/list, every tool called at least once (the transport's own
//! `use_app` and `host_control` too), resources read, a folder allowed with `--allow`, and a call
//! cancelled with `notifications/cancelled`.

mod common;

use common::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: i64,
}

impl Client {
    fn start(root: &std::path::Path, allow: &std::path::Path) -> Client {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_jetcarp-mcp"));
        cmd.arg("--allow").arg(allow);
        for (k, v) in env_of(root) {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("jetcarp-mcp starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            id: 0,
        }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// A request's reply (notifications on the way are skipped).
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(&req(id, method, params));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "the server quit"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(r.get("error").is_none(), "{name}: {r}");
        r["result"].clone()
    }
}

#[test]
fn a_whole_session_over_stdio() {
    let root = setup("stdio");
    let golden = jetcarp_api::out::absolute(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jetcarp-pack/tests/golden"),
    );
    let mut c = Client::start(root, &golden);
    let init = c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "stdio-test", "version": "1"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "jetcarp");
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let tools = c.request("tools/list", json!({}));
    let names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    let mut want: Vec<String> = jetcarp_mcp::tools::names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    want.extend(["use_app", "host_control", "app_display"].map(String::from));
    assert_eq!(names, want);
    // The whole list goes to the model with every request: the long settings are resources.
    let size = tools.to_string().len();
    assert!(size <= 45_000, "tools/list is {size} bytes");

    let res = c.request("resources/list", json!({}));
    let n = res["result"]["resources"].as_array().unwrap().len();
    assert!(n > 70, "{n} resources");
    let t = c.request("resources/templates/list", json!({}));
    assert_eq!(
        t["result"]["resourceTemplates"].as_array().unwrap().len(),
        5
    );
    let r = c.request(
        "resources/read",
        json!({"uri": "jetcarp://docs/tutorials/mcp"}),
    );
    assert!(
        r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("AI assistant")
    );

    let mut called = std::collections::BTreeSet::new();
    let mut tool = |c: &mut Client, name: &str, args: Value| {
        called.insert(name.to_string());
        c.call(name, args)
    };
    assert!(ok(&tool(&mut c, "help", json!({}))).contains("world_new"));
    assert!(ok(&tool(&mut c, "reference", json!({"what": "palettes"}))).contains("Ember"));
    ok(&tool(
        &mut c,
        "world_new",
        json!({"width": 30, "height": 30, "cas": [{"rule": "Life", "seed": "empty"}]}),
    ));
    ok(&tool(
        &mut c,
        "world_edit",
        json!({"cells": {"pattern": ".O.\n..O\nOOO", "x": 4, "y": 4}}),
    ));
    ok(&tool(&mut c, "world_step", json!({"generations": 4})));
    assert!(ok(&tool(&mut c, "world_read", json!({"format": "objects"}))).contains("glider"));
    assert!(has_png(&tool(
        &mut c,
        "world_render",
        json!({"max_side": 64})
    )));
    ok(&tool(
        &mut c,
        "world_status",
        json!({"save_path": "scenes/glider.json"}),
    ));
    assert!(root.join("out/scenes/glider.json").is_file());
    assert!(
        ok(&tool(
            &mut c,
            "run_script",
            json!({"script": "print(1 + 1);"})
        ))
        .contains('2')
    );
    let h = last_json(&tool(
        &mut c,
        "search_start",
        json!({"hunt": {"rules": ["B3/S23"], "soups": 200}, "wait_s": 120}),
    ));
    assert_eq!(h["status"], "done");
    let job = h["job"].as_i64().unwrap();
    assert!(ok(&tool(&mut c, "job_status", json!({"job": job}))).contains("hunt"));
    let stop = json_of(&tool(&mut c, "job_stop", json!({"job": job})));
    assert_eq!(stop["stopping"], false, "a done job is not running");
    assert!(ok(&tool(&mut c, "library_query", json!({"kind": "summary"}))).contains("patterns"));
    let t = ok(&tool(
        &mut c,
        "library_curate",
        json!({"quickstart": {"scene": "scenes/glider.json", "title": "Glider scene"}}),
    ));
    assert!(t.contains("Glider scene"), "{t}");
    let m = json_of(&tool(
        &mut c,
        "media_render",
        json!({"pattern": "glider", "frames": false}),
    ));
    assert_eq!(m["failed"], json!([]), "{m}");
    let t = ok(&tool(
        &mut c,
        "search_runs",
        json!({"try": {"tiling": "square", "rule": "B3/S23", "cells": 1000, "gens": 50, "px": 64}}),
    ));
    assert!(t.contains("\"family\""), "{t}");
    // A folder given with --allow can be read; others cannot.
    let minimal = golden.join("minimal");
    let t = ok(&tool(
        &mut c,
        "pack",
        json!({"info": {"path": path_json(&minimal)}}),
    ));
    assert!(
        t.contains("golden-a") && t.contains("signed by its publisher"),
        "{t}"
    );
    let e = fails(&tool(
        &mut c,
        "pack",
        json!({"info": {"path": path_json(&root.join("config"))}}),
    ));
    assert!(e.contains("may only read inside"), "{e}");
    let l = json_of(&tool(&mut c, "files_list", json!({"path": "scenes"})));
    assert_eq!(l["newest_first"][0]["name"], "glider.json");
    let t = ok(&tool(&mut c, "use_app", json!({"attach": false})));
    assert!(t.contains("private world"), "{t}");
    // No host or app runs in this test's runtime folder.
    let e = fails(&tool(&mut c, "host_control", json!({})));
    assert!(e.contains("not running"), "{e}");
    // Until the photosensitive seizure warning has been accepted, host_control refuses to start
    // or show scenes, with the warning; once it has, the call goes through (#96).
    let e = fails(&tool(&mut c, "host_control", json!({"action": "resume"})));
    assert!(
        e.contains("won't start or show scenes in the background host") && e.contains("B0"),
        "{e}"
    );
    let e = fails(&tool(
        &mut c,
        "host_control",
        json!({"action": "wallpaper"}),
    ));
    assert!(e.contains("photosensitive seizure warning"), "{e}");
    use jetcarp_core::photosensitivity::{Answer, record_in};
    record_in(&root.join("config"), Answer::NoDisorder, "test").unwrap();
    let e = fails(&tool(&mut c, "host_control", json!({"action": "resume"})));
    assert!(e.contains("not running"), "{e}");
    let e = fails(&tool(&mut c, "app_display", json!({})));
    assert!(e.contains("only available in the JETCARP app"), "{e}");
    let missing: Vec<&String> = names.iter().filter(|n| !called.contains(*n)).collect();
    assert!(missing.is_empty(), "never called: {missing:?}");

    // A long call stops soon after the client cancels it.
    c.id += 1;
    let id = c.id;
    c.send(&req(
        id,
        "tools/call",
        json!({"name": "run_script", "arguments": {"script": "let n = 0; loop { n += 1; }", "max_seconds": 120}}),
    ));
    std::thread::sleep(Duration::from_millis(300));
    let t0 = Instant::now();
    c.send(
        &json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id}}),
    );
    let reply = loop {
        let mut line = String::new();
        assert!(c.stdout.read_line(&mut line).unwrap() > 0);
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == id {
            break v;
        }
    };
    assert!(t0.elapsed() < Duration::from_secs(30));
    assert!(fails(&reply["result"]).contains("cancelled"));

    drop(c.stdin);
    let status = c.child.wait().unwrap();
    assert!(status.success());
}

/// A search on the server's own thread is stopped at its next checkpoint when the client goes
/// (stdin closed), and left paused and resumable, not marked running with nothing running it
/// (#64).
#[test]
fn the_server_stops_its_own_jobs_when_its_client_goes() {
    let root = setup("stdio");
    let mut c = Client::start(root, &root.join("out"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "stdio-test", "version": "1"}}),
    );
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let h = json_of(&c.call(
        "search_start",
        json!({"hunt": {"rules": ["B3/S23"], "soups": 50_000_000}, "threads": 1}),
    ));
    assert_eq!(h["mode"], "background", "{h}");
    let job = h["job"].as_i64().unwrap();
    let t0 = Instant::now();
    loop {
        let s = last_json(&c.call("job_status", json!({"job": job})));
        if s["done"].as_u64().unwrap_or(0) > 0 {
            assert_eq!(s["status"], "running", "{s}");
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no progress: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(c.stdin);
    let t1 = Instant::now();
    let status = c.child.wait().unwrap();
    assert!(status.success());
    assert!(t1.elapsed() < Duration::from_secs(30), "{:?}", t1.elapsed());
    let cx = jetcarp_api::Ctx::new().with_library(Some(root.join("lib")));
    let v = jetcarp_api::call_json(&cx, "jobs.status", json!({"job": job})).unwrap();
    assert_eq!(
        (&v["status"], &v["stale"]),
        (&json!("paused"), &json!(false)),
        "{v}"
    );
    assert!(v["done"].as_u64().unwrap() > 0, "{v}");
}
