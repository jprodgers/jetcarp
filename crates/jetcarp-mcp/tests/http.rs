//! MCP over HTTP (`jetcarp-mcp --http`, #115), as a client sees it, with nothing but a
//! `TcpStream`: a session (initialize, tools/list, tool calls, progress on an event stream,
//! cancelling, ending it), the token, the Origin and Host checks, loopback only, nothing listening
//! unless asked, the size, time and connection limits, and sessions as the transport specifies.

mod common;

use common::*;
use jetcarp_mcp::http::{Limits, Listener};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A `jetcarp-mcp --http 0 --json` process, killed when dropped.
struct Server {
    child: Child,
    _stdout: BufReader<ChildStdout>,
    start: Value,
    port: u16,
    token: String,
}

/// The environment of a server process of test `tag`: the binary's folders, with a runtime
/// folder of the test's own, so that the tests running at once do not share an endpoint file.
fn env_for(root: &Path, tag: &str) -> Vec<(&'static str, std::path::PathBuf)> {
    let mut env = env_of(root);
    env.retain(|(k, _)| *k != "JETCARP_RUNTIME_DIR");
    env.push(("JETCARP_RUNTIME_DIR", runtime(root, tag)));
    env
}

fn runtime(root: &Path, tag: &str) -> std::path::PathBuf {
    root.join(format!("runtime-{tag}"))
}

impl Server {
    fn start(root: &Path, tag: &str) -> Server {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_jetcarp-mcp"));
        cmd.args(["--http", "0", "--json"]);
        for (k, v) in env_for(root, tag) {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("jetcarp-mcp starts");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let start: Value = serde_json::from_str(&line).expect("the endpoint as JSON");
        Server {
            port: start["port"].as_u64().unwrap() as u16,
            token: start["token"].as_str().unwrap().to_string(),
            child,
            _stdout: stdout,
            start,
        }
    }

    fn req(&self) -> Req {
        Req::new(self.port).token(&self.token)
    }

    /// A new session's id, from its initialize.
    fn session(&self) -> String {
        let r = self.req().post(&init(1));
        assert_eq!(r.status, 200, "{}", r.body);
        let id = r
            .header("mcp-session-id")
            .expect("a session id")
            .to_string();
        let n = self
            .req()
            .session(&id)
            .post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        assert_eq!(n.status, 202, "{}", n.body);
        id
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn init(id: i64) -> Value {
    req(
        id,
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {},
               "clientInfo": {"name": "http-test", "version": "1"}}),
    )
}

fn tool(id: i64, name: &str, args: Value) -> Value {
    req(id, "tools/call", json!({"name": name, "arguments": args}))
}

/// One HTTP request, built by hand.
#[derive(Clone)]
struct Req {
    port: u16,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

/// One HTTP answer.
#[derive(Debug)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
    }
    /// The messages of an event stream.
    fn events(&self) -> Vec<Value> {
        self.body
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .map(|d| serde_json::from_str(d).unwrap())
            .collect()
    }
}

impl Req {
    fn new(port: u16) -> Req {
        Req {
            port,
            method: "POST".into(),
            path: "/mcp".into(),
            headers: vec![
                ("Host".into(), format!("127.0.0.1:{port}")),
                ("Content-Type".into(), "application/json".into()),
                (
                    "Accept".into(),
                    "application/json, text/event-stream".into(),
                ),
            ],
        }
    }
    fn set(mut self, name: &str, value: &str) -> Req {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.into(), value.into()));
        self
    }
    fn without(mut self, name: &str) -> Req {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self
    }
    fn token(self, t: &str) -> Req {
        self.set("Authorization", &format!("Bearer {t}"))
    }
    fn session(self, s: &str) -> Req {
        self.set("Mcp-Session-Id", s)
    }
    fn method(mut self, m: &str) -> Req {
        self.method = m.into();
        self
    }
    fn path(mut self, p: &str) -> Req {
        self.path = p.into();
        self
    }
    /// The request's bytes, with `body`.
    fn bytes(&self, body: &[u8]) -> Vec<u8> {
        let mut out = format!("{} {} HTTP/1.1\r\n", self.method, self.path);
        for (n, v) in &self.headers {
            out.push_str(&format!("{n}: {v}\r\n"));
        }
        if !self.headers.iter().any(|(n, _)| {
            n.eq_ignore_ascii_case("content-length") || n.eq_ignore_ascii_case("transfer-encoding")
        }) && (self.method == "POST" || !body.is_empty())
        {
            out.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        out.push_str("Connection: close\r\n\r\n");
        let mut b = out.into_bytes();
        b.extend_from_slice(body);
        b
    }
    fn post(&self, msg: &Value) -> Reply {
        exchange(self.port, &self.bytes(msg.to_string().as_bytes()))
    }
    fn send(&self) -> Reply {
        exchange(self.port, &self.bytes(b""))
    }
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(3),
    )
    .expect("connects");
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s
}

/// Send `raw` and read the answer to the end (the requests ask for the connection to close).
fn exchange(port: u16, raw: &[u8]) -> Reply {
    let mut s = connect(port);
    s.write_all(raw).unwrap();
    let mut answer = Vec::new();
    let _ = s.read_to_end(&mut answer);
    parse(&String::from_utf8_lossy(&answer))
}

/// An answer's status, headers and body; interim `100 Continue` answers are skipped.
fn parse(text: &str) -> Reply {
    let mut text = text;
    loop {
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
        let mut lines = head.lines();
        let status: u16 = lines
            .next()
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {text:?}"));
        if status == 100 {
            text = body;
            continue;
        }
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
            .collect();
        return Reply {
            status,
            headers,
            body: body.to_string(),
        };
    }
}

/// The text of a tool result.
fn result_text(r: &Value) -> String {
    text(&r["result"])
}

#[test]
fn a_session_over_http() {
    let root = setup("http");
    let s = Server::start(root, "session");
    assert_eq!(s.start["transport"], "streamable-http");
    assert_eq!(s.start["url"], format!("http://127.0.0.1:{}/mcp", s.port));
    assert_eq!(
        s.start["header"],
        format!("Authorization: Bearer {}", s.token)
    );
    assert_eq!(s.token.len(), 32);

    let r = s.req().post(&init(1));
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        r.header("content-type")
            .unwrap()
            .starts_with("application/json")
    );
    let v = r.json();
    assert_eq!(v["result"]["serverInfo"]["name"], "jetcarp");
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25");
    let id = r
        .header("mcp-session-id")
        .expect("initialize starts a session");
    assert_eq!(id.len(), 32);
    assert!(id.bytes().all(|b| (0x21..=0x7e).contains(&b)));
    let n = s
        .req()
        .session(id)
        .post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!(n.status, 202);
    assert!(n.body.is_empty());

    // The same tools as over stdio.
    let q = s
        .req()
        .session(id)
        .set("MCP-Protocol-Version", "2025-11-25");
    let tools = q.post(&req(2, "tools/list", json!({}))).json();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    let mut want = jetcarp_mcp::tools::names();
    want.extend(["use_app", "host_control", "app_display"]);
    assert_eq!(names, want);

    // Tool calls on the session's world.
    let r = q
        .post(&tool(
            3,
            "world_new",
            json!({"tiling": "square", "width": 40, "height": 40,
                   "cas": [{"rule": "Life", "seed": "empty"}]}),
        ))
        .json();
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = q
        .post(&tool(
            4,
            "world_edit",
            json!({"cells": {"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}}),
        ))
        .json();
    assert_eq!(r["result"]["isError"], false, "{r}");
    q.post(&tool(5, "world_step", json!({"generations": 8})));
    let r = q
        .post(&tool(6, "world_read", json!({"format": "objects"})))
        .json();
    assert!(result_text(&r).contains("glider"), "{r}");
    let r = q
        .post(&req(
            7,
            "resources/read",
            json!({"uri": "jetcarp://docs/tutorials/mcp"}),
        ))
        .json();
    assert!(
        r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("--http")
    );
    // A malformed request is a JSON-RPC error, answered 200 as any other answer.
    let r = q.post(&req(8, "no/such/method", json!({})));
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["error"]["code"], -32601);

    // With a progress token, an event stream: the progress, then the answer.
    let script = "for i in 0..=4 { progress(i, 4, `item ${i}`); }\nprint(\"done\");";
    let r = q.post(&req(
        9,
        "tools/call",
        json!({"name": "run_script", "arguments": {"script": script},
               "_meta": {"progressToken": "tok-1"}}),
    ));
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("text/event-stream"));
    let ev = r.events();
    assert!(ev.len() >= 3, "{ev:?}");
    let (last, progress) = ev.split_last().unwrap();
    for p in progress {
        assert_eq!(p["method"], "notifications/progress", "{p}");
        assert_eq!(p["params"]["progressToken"], "tok-1");
    }
    assert_eq!(last["id"], 9);
    assert!(result_text(last).contains("done"), "{last}");
    // A client that takes JSON only gets JSON, without the progress.
    let r = q.clone().set("Accept", "application/json").post(&req(
        10,
        "tools/call",
        json!({"name": "run_script", "arguments": {"script": script},
               "_meta": {"progressToken": "tok-2"}}),
    ));
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert!(result_text(&r.json()).contains("done"));

    // Two requests on one connection.
    let mut c = connect(s.port);
    for id in [11, 12] {
        let body = req(id, "ping", json!({})).to_string();
        let mut raw = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost:{}\r\nAuthorization: Bearer {}\r\n\
             Mcp-Session-Id: {id_}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n",
            s.port,
            s.token,
            body.len(),
            id_ = id_of(&q)
        );
        raw.push_str(&body);
        c.write_all(raw.as_bytes()).unwrap();
        let r = read_one(&mut c);
        assert_eq!(r.status, 200, "{r:?}");
        assert_eq!(r.header("connection"), Some("keep-alive"));
        assert_eq!(r.json()["id"], id);
    }

    // A chunked body, and one sent after 100 Continue.
    let body = req(13, "ping", json!({})).to_string();
    let (a, b) = body.split_at(10);
    let chunked = format!("{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len());
    let r = exchange(
        s.port,
        &q.clone()
            .set("Transfer-Encoding", "chunked")
            .bytes(chunked.as_bytes()),
    );
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["id"], 13);
    let mut c = connect(s.port);
    let body = req(14, "ping", json!({})).to_string();
    let head = q.clone().set("Expect", "100-continue").bytes(b"");
    // The head alone (its Content-Length is the body's), then the body once told to go on.
    let head = String::from_utf8(head).unwrap().replace(
        "Content-Length: 0",
        &format!("Content-Length: {}", body.len()),
    );
    c.write_all(head.as_bytes()).unwrap();
    let mut first = [0u8; 25];
    c.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"HTTP/1.1 100 Continue\r\n\r\n");
    c.write_all(body.as_bytes()).unwrap();
    let mut rest = String::new();
    let _ = c.read_to_string(&mut rest);
    assert_eq!(parse(&rest).json()["id"], 14);

    // Ending the session.
    let r = s.req().session(id).method("DELETE").send();
    assert_eq!(r.status, 204, "{r:?}");
    let r = q.post(&req(15, "ping", json!({})));
    assert_eq!(r.status, 404, "an ended session is not found");
}

/// The session id a request carries.
fn id_of(r: &Req) -> String {
    r.headers
        .iter()
        .find(|(n, _)| n == "Mcp-Session-Id")
        .map(|(_, v)| v.clone())
        .unwrap()
}

/// One answer read from a kept-alive connection, by its Content-Length.
fn read_one(c: &mut TcpStream) -> Reply {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        c.read_exact(&mut byte).unwrap();
        buf.push(byte[0]);
    }
    let head = parse(&String::from_utf8_lossy(&buf));
    let n: usize = head.header("content-length").unwrap().parse().unwrap();
    let mut body = vec![0u8; n];
    c.read_exact(&mut body).unwrap();
    Reply {
        body: String::from_utf8(body).unwrap(),
        ..head
    }
}

/// SECURITY.md: every request needs the token, sent as a bearer token; a wrong one is refused
/// as a missing one is, and the right one works.
#[test]
fn the_token_is_required() {
    let root = setup("http");
    let s = Server::start(root, "token");
    let anon = Req::new(s.port);
    let r = anon.post(&init(1));
    assert_eq!(r.status, 401, "{r:?}");
    assert!(r.header("www-authenticate").unwrap().starts_with("Bearer"));
    assert!(r.header("mcp-session-id").is_none());
    assert_eq!(r.json()["id"], Value::Null);
    let mut wrong = s.token.clone().into_bytes();
    wrong[31] = if wrong[31] == b'0' { b'1' } else { b'0' };
    let wrong = String::from_utf8(wrong).unwrap();
    for bad in [
        anon.clone().token(&wrong),
        anon.clone().token(&s.token[..31]),
        anon.clone().token(&format!("{}0", s.token)),
        anon.clone().token(""),
        anon.clone()
            .set("Authorization", &format!("Basic {}", s.token)),
        anon.clone().set("Authorization", &s.token),
    ] {
        let r = bad.post(&init(1));
        assert_eq!(r.status, 401, "{:?}: {r:?}", bad.headers);
    }
    let r = anon.clone().token(&wrong).post(&init(1));
    assert!(
        r.header("www-authenticate")
            .unwrap()
            .contains("invalid_token")
    );
    // Ending or using a session needs it too.
    let id = s.session();
    assert_eq!(
        anon.clone()
            .session(&id)
            .post(&req(2, "ping", json!({})))
            .status,
        401
    );
    assert_eq!(
        anon.clone().session(&id).method("DELETE").send().status,
        401
    );
    assert_eq!(
        s.req().session(&id).post(&req(2, "ping", json!({}))).status,
        200
    );
    assert_eq!(
        s.req()
            .set("Authorization", &format!("bearer {}", s.token))
            .session(&id)
            .post(&req(3, "ping", json!({})))
            .status,
        200,
        "the scheme is not case-sensitive"
    );
    // The token is the one recorded in the runtime folder, readable by this user only.
    let file = runtime(root, "token").join("mcp-http-endpoint.json");
    assert_eq!(s.start["endpoint_file"], file.display().to_string());
    let rec: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(rec["token"], s.token.as_str());
    assert_eq!(rec["url"], s.start["url"]);
    #[cfg(unix)]
    assert_eq!(jetcarp_mcp::link::private_mode(&file).unwrap(), 0o600);
}

/// DNS rebinding: a request from a web page at another origin, or addressed to another host
/// name, is refused even with the token, as MCP's transport requires.
#[test]
fn other_origins_and_hosts_are_refused() {
    let root = setup("http");
    let s = Server::start(root, "origin");
    let p = s.port;
    for origin in [
        "http://evil.example".to_string(),
        format!("http://evil.example:{p}"),
        "null".to_string(),
        format!("https://127.0.0.1:{p}"),
        format!("http://127.0.0.1:{}", p.wrapping_add(1)),
        format!("http://localhost:{p}.evil.example"),
    ] {
        let r = s.req().set("Origin", &origin).post(&init(1));
        assert_eq!(r.status, 403, "{origin}: {r:?}");
        assert!(r.header("mcp-session-id").is_none());
        assert!(r.header("access-control-allow-origin").is_none());
    }
    for host in [
        format!("evil.example:{p}"),
        format!("127.0.0.1.nip.io:{p}"),
        "127.0.0.1".to_string(),
    ] {
        let r = s.req().set("Host", &host).post(&init(1));
        assert_eq!(r.status, 403, "{host}: {r:?}");
    }
    assert_eq!(s.req().without("Host").post(&init(1)).status, 400);
    // A preflight from elsewhere is refused before anything else.
    let r = s
        .req()
        .set("Origin", "http://evil.example")
        .method("OPTIONS")
        .send();
    assert_eq!(r.status, 403);
    // This server's own origins, and none at all (curl, Postman, native clients), pass.
    for origin in [
        format!("http://127.0.0.1:{p}"),
        format!("http://localhost:{p}"),
    ] {
        let r = s.req().set("Origin", &origin).post(&init(1));
        assert_eq!(r.status, 200, "{origin}: {r:?}");
    }
    assert_eq!(
        s.req()
            .set("Host", &format!("localhost:{p}"))
            .post(&init(1))
            .status,
        200
    );
}

/// SECURITY.md: the endpoint is bound to 127.0.0.1, which other computers cannot reach, and
/// says so.
#[test]
fn the_endpoint_is_on_loopback_only() {
    let root = setup("http");
    let s = Server::start(root, "loopback");
    assert_eq!(s.start["bound"], "127.0.0.1");
    assert_eq!(s.start["loopback_only"], true);
    // Every other address of this machine refuses: only 127.0.0.1 is bound.
    let other = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|u| {
            u.connect("192.0.2.1:9")?;
            u.local_addr()
        })
        .map(|a| a.ip());
    if let Ok(ip) = other
        && !ip.is_loopback()
        && !ip.is_unspecified()
    {
        let r =
            TcpStream::connect_timeout(&SocketAddr::new(ip, s.port), Duration::from_millis(500));
        assert!(r.is_err(), "reachable on {ip}");
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        listening(s.child.id()),
        vec![format!("0100007F:{:04X}", s.port)],
        "its one listening socket is 127.0.0.1:{}",
        s.port
    );
    // The human-readable startup lines say so too.
    let out = Command::new(env!("CARGO_BIN_EXE_jetcarp-mcp"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("http://127.0.0.1:PORT/mcp"));
}

/// The sockets a process listens on, as `/proc/net/tcp` gives their local addresses (address
/// and port in hex).
#[cfg(target_os = "linux")]
fn listening(pid: u32) -> Vec<String> {
    let inodes: std::collections::HashSet<String> = std::fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
        .filter_map(|l| {
            let l = l.to_string_lossy();
            Some(l.strip_prefix("socket:[")?.strip_suffix(']')?.to_string())
        })
        .collect();
    let mut out = vec![];
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(t) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in t.lines().skip(1) {
            let c: Vec<&str> = line.split_whitespace().collect();
            // sl, local, remote, state (0A: listening), ..., inode.
            if c.len() > 9 && c[3] == "0A" && inodes.contains(c[9]) {
                out.push(c[1].to_string());
            }
        }
    }
    out
}

/// SECURITY.md: nothing listens unless asked. The MCP server over stdio opens no socket and
/// records no HTTP endpoint; only `--http` does.
#[test]
fn nothing_listens_unless_asked() {
    let root = setup("http");
    let file = runtime(root, "unasked").join("mcp-http-endpoint.json");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jetcarp-mcp"));
    for (k, v) in env_for(root, "unasked") {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(stdin, "{}", init(1)).unwrap();
    writeln!(stdin, "{}", tool(2, "world_status", json!({}))).unwrap();
    for _ in 0..2 {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert!(serde_json::from_str::<Value>(&line).unwrap()["result"].is_object());
    }
    #[cfg(target_os = "linux")]
    assert_eq!(listening(child.id()), Vec::<String>::new());
    assert!(!file.exists(), "no HTTP endpoint is recorded");
    drop(stdin);
    assert!(child.wait().unwrap().success());

    // With --http, one; stopped, it is gone.
    let s = Server::start(root, "unasked");
    assert!(file.exists());
    #[cfg(target_os = "linux")]
    assert_eq!(listening(s.child.id()).len(), 1);
    let port = s.port;
    drop(s);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

/// A body over the limit (16 MiB) is refused from its Content-Length, before any of it is read;
/// so is a head over 16 KiB.
#[test]
fn oversized_requests_are_refused() {
    let root = setup("http");
    let s = Server::start(root, "oversized");
    let mut c = connect(s.port);
    let head = s
        .req()
        .set("Content-Length", &(16 * 1024 * 1024 + 1).to_string())
        .bytes(b"");
    c.write_all(&head).unwrap();
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    let r = parse(&answer);
    assert_eq!(r.status, 413, "{r:?}");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("16 MiB")
    );
    // Just under is read (and is not JSON).
    let r = exchange(s.port, &s.req().bytes(&vec![b' '; 16 * 1024 * 1024 - 1]));
    assert_eq!(r.status, 400, "{:?}", r.headers);
    let r = s.req().set("X-Big", &"x".repeat(17 * 1024)).post(&init(1));
    assert_eq!(r.status, 431);
    let mut many = s.req();
    for i in 0..70 {
        many = many.set(&format!("X-{i}"), "1");
    }
    assert_eq!(many.post(&init(1)).status, 431);
}

/// Sessions as MCP's Streamable HTTP transport specifies: initialize starts one and its answer
/// gives the id; later requests must carry it (400 without, 404 for one that has ended or never
/// was); DELETE ends one; each has a world of its own; a modern client that never calls
/// initialize is answered without one.
#[test]
fn sessions_behave_as_specified() {
    let root = setup("http");
    let s = Server::start(root, "sessions");
    let (a, b) = (s.session(), s.session());
    assert_ne!(a, b);
    // A request without a session (after initialize) is refused.
    let r = s.req().post(&req(2, "tools/list", json!({})));
    assert_eq!(r.status, 400, "{r:?}");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Mcp-Session-Id")
    );
    let r = s
        .req()
        .session("0123456789abcdef0123456789abcdef")
        .post(&req(2, "ping", json!({})));
    assert_eq!(r.status, 404);

    // Each session has a world of its own, as each stdio client has.
    let r = s
        .req()
        .session(&a)
        .post(&tool(
            3,
            "world_new",
            json!({"tiling": "square", "width": 9, "height": 8}),
        ))
        .json();
    assert_eq!(r["result"]["isError"], false, "{r}");
    let cells = |id: &str| -> u64 {
        let r = s
            .req()
            .session(id)
            .post(&tool(4, "world_status", json!({})))
            .json();
        let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
        v["cells"].as_u64().unwrap()
    };
    assert_eq!(cells(&a), 72);
    assert_ne!(cells(&b), 72);

    // DELETE ends one session, and only that one.
    assert_eq!(s.req().session(&a).method("DELETE").send().status, 204);
    assert_eq!(s.req().session(&a).method("DELETE").send().status, 404);
    assert_eq!(
        s.req().session(&a).post(&req(5, "ping", json!({}))).status,
        404
    );
    assert_eq!(
        s.req().session(&b).post(&req(5, "ping", json!({}))).status,
        200
    );
    assert_eq!(s.req().method("DELETE").send().status, 400);

    // A modern client sends its version in each request and needs no session.
    let meta = json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}});
    let r = s.req().post(&req(6, "server/discover", meta.clone()));
    assert_eq!(r.status, 200, "{r:?}");
    assert!(r.header("mcp-session-id").is_none());
    let r = s.req().post(&req(7, "tools/list", meta)).json();
    assert!(r["result"]["tools"].as_array().unwrap().len() > 10);

    // What the transport refuses.
    let bad = |q: Req, msg: Value, want: u16| {
        let r = q.post(&msg);
        assert_eq!(r.status, want, "{:?} {msg}: {r:?}", q.headers);
        assert_eq!(r.header("connection"), Some("close"));
    };
    let q = s.req().session(&b);
    bad(
        q.clone().set("MCP-Protocol-Version", "1999-01-01"),
        req(8, "ping", json!({})),
        400,
    );
    bad(q.clone(), json!([req(8, "ping", json!({}))]), 400);
    bad(
        q.clone().set("Content-Type", "text/plain"),
        req(8, "ping", json!({})),
        415,
    );
    bad(
        q.clone()
            .set("Content-Type", "application/x-www-form-urlencoded"),
        req(8, "ping", json!({})),
        415,
    );
    bad(
        q.clone().set("Accept", "text/html"),
        req(8, "ping", json!({})),
        406,
    );
    bad(q.clone().path("/"), req(8, "ping", json!({})), 404);
    bad(q.clone().path("/mcp/x"), req(8, "ping", json!({})), 404);
    let r = exchange(s.port, &q.bytes(b"{not json"));
    assert_eq!(r.status, 400);
    assert_eq!(r.json()["error"]["code"], -32700);
    // GET (a stream the server starts) is not offered.
    let r = q
        .clone()
        .method("GET")
        .set("Accept", "text/event-stream")
        .send();
    assert_eq!(r.status, 405);
    assert_eq!(r.header("allow"), Some("POST, DELETE"));
    // A 404 tells a lost client where the endpoint is.
    let r = s.req().path("/").method("GET").send();
    assert_eq!(r.status, 404);
    assert!(
        r.body.contains(&format!("http://127.0.0.1:{}/mcp", s.port)),
        "{}",
        r.body
    );
    // Two of a header that may be given once.
    let r = exchange(
        s.port,
        &String::from_utf8(q.bytes(req(9, "ping", json!({})).to_string().as_bytes()))
            .unwrap()
            .replacen(
                "Host: ",
                &format!("Host: evil.example:{}\r\nHost: ", s.port),
                1,
            )
            .into_bytes(),
    );
    assert_eq!(r.status, 400);
}

/// A call stops soon after the client cancels it from another connection, as over stdio.
#[test]
fn a_cancelled_call_stops_soon() {
    let root = setup("http");
    let s = Server::start(root, "cancel");
    let id = s.session();
    let q = s.req().session(&id);
    let long = {
        let q = q.clone();
        std::thread::spawn(move || {
            q.post(&tool(
                21,
                "run_script",
                json!({"script": "let n = 0; loop { n += 1; }", "max_seconds": 120}),
            ))
        })
    };
    std::thread::sleep(Duration::from_millis(500));
    let t0 = Instant::now();
    let r = q.post(
        &json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                           "params": {"requestId": 21}}),
    );
    assert_eq!(r.status, 202);
    let r = long.join().unwrap().json();
    assert!(t0.elapsed() < Duration::from_secs(30));
    assert_eq!(r["id"], 21);
    assert!(result_text(&r).contains("cancelled"), "{r}");
}

/// A listener in this process with `limits`, answered by a server of its own on a thread until
/// the flag is set.
fn in_process(limits: Limits) -> (u16, String, Arc<AtomicBool>) {
    setup("http");
    let l = Listener::start_with(0, None, limits, Arc::new(|| {}), None).unwrap();
    let (port, token) = (l.port, l.token.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::spawn(move || {
        let mut server = jetcarp_mcp::Server::new();
        let mut host = jetcarp_mcp::Session::new();
        while !s.load(Ordering::SeqCst) {
            if let Some(p) = l.recv_timeout(Duration::from_millis(50)) {
                server.cancels = p.cancels();
                server.notifier = Some(p.notifier());
                let msg = p.message.clone();
                p.reply(server.handle(&mut host, &msg));
            }
        }
    });
    (port, token, stop)
}

/// At most so many connections at once: one more is told to try again, and closed.
#[test]
fn connections_are_limited() {
    let (port, token, stop) = in_process(Limits {
        max_connections: 2,
        ..Limits::default()
    });
    let held: Vec<TcpStream> = (0..2).map(|_| connect(port)).collect();
    std::thread::sleep(Duration::from_millis(300));
    // The third is answered at once, before it says anything.
    let mut c = connect(port);
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    let r = parse(&answer);
    assert_eq!(r.status, 503, "{r:?}");
    assert_eq!(r.header("retry-after"), Some("1"));
    drop(held);
    let t = Instant::now();
    loop {
        let r = Req::new(port).token(&token).post(&init(1));
        if r.status == 200 {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "{r:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, Ordering::SeqCst);
}

/// A client that sends its head too slowly is answered 408 and closed; one that sends nothing
/// is closed once idle; a body that does not arrive in time is refused too.
#[test]
fn slow_clients_time_out() {
    let (port, token, stop) = in_process(Limits {
        head_timeout: Duration::from_secs(1),
        idle_timeout: Duration::from_secs(1),
        body_timeout: Duration::from_secs(1),
        ..Limits::default()
    });
    let t = Instant::now();
    let mut c = connect(port);
    c.write_all(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1")
        .unwrap();
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    assert_eq!(parse(&answer).status, 408);
    assert!(t.elapsed() < Duration::from_secs(5));

    let t = Instant::now();
    let mut c = connect(port);
    let mut buf = Vec::new();
    assert_eq!(
        c.read_to_end(&mut buf).unwrap(),
        0,
        "closed, with nothing said"
    );
    assert!(t.elapsed() < Duration::from_secs(5));

    let mut c = connect(port);
    let head = Req::new(port)
        .token(&token)
        .set("Content-Length", "100")
        .bytes(b"");
    c.write_all(&head).unwrap();
    c.write_all(b"{\"jsonrpc\"").unwrap();
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    assert_eq!(parse(&answer).status, 408);
    stop.store(true, Ordering::SeqCst);
}

/// A chunked body over the limit is refused as soon as it passes it.
#[test]
fn a_chunked_body_over_the_limit_is_refused() {
    let (port, token, stop) = in_process(Limits {
        max_body: 1024,
        ..Limits::default()
    });
    let mut c = connect(port);
    let head = Req::new(port)
        .token(&token)
        .set("Transfer-Encoding", "chunked")
        .bytes(b"");
    c.write_all(&head).unwrap();
    c.write_all(format!("{:x}\r\n{}\r\n", 1000, "x".repeat(1000)).as_bytes())
        .unwrap();
    c.write_all(format!("{:x}\r\n", 100).as_bytes()).unwrap();
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    assert_eq!(parse(&answer).status, 413);
    stop.store(true, Ordering::SeqCst);
}

/// Stopped (the app's checkbox unticked, say), the endpoint closes the connections open on it at
/// once, rather than when they have been idle for 30 seconds, and nothing accepts on its port.
#[test]
fn stopping_closes_the_open_connections() {
    let (port, token, stop) = in_process(Limits::default());
    // One idle, one kept alive after an answer.
    let mut idle = connect(port);
    let mut kept = connect(port);
    let body = init(1).to_string();
    write!(
        kept,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    assert_eq!(read_one(&mut kept).status, 200);
    let t = Instant::now();
    stop.store(true, Ordering::SeqCst);
    for c in [&mut idle, &mut kept] {
        let mut rest = Vec::new();
        let _ = c.read_to_end(&mut rest);
    }
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "port {port} still accepts"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
