//! The background host (`jetcarp host serve`) and `jetcarp mcp --http` ended by a signal (#278):
//! SIGTERM (what `kill` sends, launchd at logout on the Mac, and a Linux session as it ends),
//! SIGHUP and SIGINT stop each as its own stop does. The host stops as `jetcarp host stop` stops
//! it, and the HTTP server as Ctrl-C stopped it: its jobs at their next checkpoint. Each removes
//! its endpoint file and its record in the memory pool (`memory/<pid>.json`) from the runtime
//! folder, which before were left behind. A second signal ends either at once, for one that does
//! not stop.
//!
//! The same signals stop `jetcarp mcp` over stdio as stdin closing does, a foreground `jetcarp
//! search` at its next checkpoint (so `--resume` carries it on), and `jetcarp script` at its next
//! step, each removing its memory record. A call still running when the MCP server stops (a
//! `job_status` waiting for a job to end) is cut short with an answer that says so, and the server
//! ends within a few seconds instead of waiting for it.
//!
//! Each test runs the real program, with every folder a scratch one under the target folder, and
//! signals only the process it started. The host runs headless, as on a machine with no desktop:
//! headless, `jetcarp host stop` leaves no still picture on the desktop (only a host showing a
//! desktop does, when `jetcarp host still on` asks for one), so there is none to look for here.
//! Linux and macOS: Windows has no such signals.
#![cfg(unix)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// A scratch home for one test, its photosensitive seizure warning answered (#96), as on a
/// computer where JETCARP has been set up. Removed when dropped.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("jetcarp-quit-signal-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config/jetcarp")).unwrap();
        jetcarp_core::photosensitivity::record_in(
            &dir.join("config/jetcarp"),
            jetcarp_core::photosensitivity::Answer::NoDisorder,
            "test",
        )
        .unwrap();
        Home(dir)
    }

    fn runtime(&self) -> PathBuf {
        self.0.join("run/jetcarp")
    }

    /// Process `pid`'s record in the memory pool.
    fn record(&self, pid: u32) -> PathBuf {
        self.runtime()
            .join(jetcarp_core::machine::pool::DIR)
            .join(format!("{pid}.json"))
    }

    /// `jetcarp args`, every folder it uses in here. The `JETCARP_` ones are what Windows and
    /// macOS go by; the XDG ones and `HOME` keep anything else on Linux in here too (the GNOME
    /// extension's folder the host looks at as it starts, among them). No display is named, so
    /// nothing reaches the screen.
    fn jetcarp(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_DATA_HOME", self.0.join("share"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("JETCARP_CONFIG_DIR", self.0.join("config/jetcarp"))
            .env("JETCARP_DATA_DIR", self.0.join("data"))
            .env("JETCARP_LIBRARY", self.0.join("data/library.sqlite"))
            .env("JETCARP_OUT", self.0.join("out"))
            .env("JETCARP_RUNTIME_DIR", self.runtime())
            .env("JETCARP_HOST_NO_PAUSE_RULES", "1")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        c
    }

    /// The headless host, on a small canvas.
    fn host(&self) -> Running {
        let mut c = self.jetcarp(&["host", "serve", "--headless", "--canvas", "160x120"]);
        // The host follows its own GPU setting, as in the host's own tests.
        c.env_remove("JETCARP_GPU");
        Running::start(c)
    }

    /// `jetcarp mcp --http` on any free port.
    fn mcp_http(&self) -> Running {
        Running::start(self.jetcarp(&["mcp", "--http", "0"]))
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A program a test started: what it says on standard error as it comes, and killed (by the
/// process id it was started with) if the test ends first.
struct Running {
    child: Child,
    lines: Receiver<String>,
    said: Vec<String>,
}

impl Running {
    fn start(mut c: Command) -> Running {
        let mut child = c.spawn().expect("jetcarp starts");
        let err = child.stderr.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        Running {
            child,
            lines,
            said: vec![],
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends it `sig` (`TERM`, `HUP`, `INT`).
    fn signal(&self, sig: &str) {
        let pid = self.pid().to_string();
        let sent = Command::new("kill")
            .args([&format!("-{sig}"), &pid])
            .status()
            .unwrap();
        assert!(sent.success(), "kill -{sig} {pid}");
    }

    /// Everything it has said on standard error so far.
    fn stderr(&mut self) -> String {
        while let Ok(l) = self.lines.try_recv() {
            self.said.push(l);
        }
        self.said.join("\n")
    }

    /// Waits up to a minute for `ready`, failing with what it said if it does not come.
    fn wait_until(&mut self, what: &str, ready: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !ready() {
            if let Ok(Some(s)) = self.child.try_wait() {
                panic!(
                    "it ended ({s:?}) before {what}; its stderr:\n{}",
                    self.stderr()
                );
            }
            if t0.elapsed() > Duration::from_secs(60) {
                panic!("no {what} after a minute; its stderr:\n{}", self.stderr());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits up to `limit` for a line on standard error that holds `text`.
    fn wait_for_line(&mut self, text: &str, limit: Duration) -> bool {
        let t0 = Instant::now();
        while let Some(left) = limit.checked_sub(t0.elapsed()) {
            match self
                .lines
                .recv_timeout(left.min(Duration::from_millis(100)))
            {
                Ok(l) => {
                    let found = l.contains(text);
                    self.said.push(l);
                    if found {
                        return true;
                    }
                }
                Err(_) => {
                    if let Ok(Some(_)) = self.child.try_wait() {
                        return false;
                    }
                }
            }
        }
        false
    }

    /// How it ended, if it did within `limit`.
    fn ended_within(&mut self, limit: Duration) -> Option<ExitStatus> {
        let t0 = Instant::now();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return Some(s);
            }
            if t0.elapsed() > limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Sends it SIGTERM and waits up to half a minute for it to end; how it ended, and what it
    /// said.
    fn terminate(&mut self) -> (ExitStatus, String) {
        self.signal("TERM");
        let status = self.ended_within(Duration::from_secs(30));
        // What it said as it went, once its end of the pipe has closed.
        std::thread::sleep(Duration::from_millis(100));
        let err = self.stderr();
        let status = status
            .unwrap_or_else(|| panic!("still running 30 s after SIGTERM; its stderr:\n{err}"));
        (status, err)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The port and token recorded in the endpoint file at `p`.
fn endpoint(p: &Path) -> (u16, String) {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    (
        v["port"].as_u64().unwrap() as u16,
        v["token"].as_str().unwrap().to_string(),
    )
}

/// SIGTERM stops the host as `jetcarp host stop` does: it ends with 0, its link closed, so
/// `host-endpoint.json` is gone from the runtime folder, and its record in the memory pool with
/// it. Before, it ended at once and left both, at every logout on the Mac (launchd ends it so).
#[test]
fn sigterm_stops_the_host_as_host_stop_does() {
    let home = Home::new("host");
    let file = home.runtime().join("host-endpoint.json");
    let mut host = home.host();
    let record = home.record(host.pid());
    host.wait_until("endpoint file and memory record", || {
        file.exists() && record.exists()
    });

    let (status, err) = host.terminate();
    assert!(
        !file.exists(),
        "SIGTERM left {} behind (the host ended with {status:?}); its stderr:\n{err}",
        file.display()
    );
    assert!(
        status.success(),
        "the host ends as `jetcarp host stop` ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the host left its memory record {}; its stderr:\n{err}",
        record.display()
    );
}

/// A second signal ends a stopping host at once, with the status a shell gives a program a
/// signal ended (128 + its number). A caller still connected holds a stopping host for up to a
/// second, as it waits for its callers to hang up, so the second signal comes while it waits.
#[test]
fn a_second_signal_ends_the_host_at_once() {
    let home = Home::new("host-twice");
    let file = home.runtime().join("host-endpoint.json");
    let mut host = home.host();
    host.wait_until("endpoint file", || file.exists());
    let (port, _) = endpoint(&file);
    let _caller = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // The link takes the connection in.
    std::thread::sleep(Duration::from_millis(300));

    host.signal("TERM");
    let stopping = host.wait_for_line("stopping", Duration::from_secs(30));
    assert!(
        stopping,
        "the host did not say it was stopping; it ended with {:?}; its stderr:\n{}",
        host.child.try_wait(),
        host.stderr()
    );
    host.signal("TERM");
    let status = host.ended_within(Duration::from_secs(5));
    let err = host.stderr();
    let status = status
        .unwrap_or_else(|| panic!("still running after a second SIGTERM; its stderr:\n{err}"));
    assert_eq!(
        status.code(),
        Some(128 + 15),
        "a second SIGTERM ends it at once: {status:?}; its stderr:\n{err}"
    );
}

/// One JSON-RPC message posted to `jetcarp mcp --http` on `port`: the head of the answer and its
/// body.
fn post(port: u16, token: &str, session: Option<&str>, body: &Value) -> (String, Value) {
    let body = body.to_string();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let sid = session
        .map(|s| format!("Mcp-Session-Id: {s}\r\n"))
        .unwrap_or_default();
    write!(
        s,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         {sid}Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap();
    (
        head.to_string(),
        serde_json::from_str(body).unwrap_or(Value::Null),
    )
}

/// A session with the server on `port`, and a hunt that runs until stopped started in it, on
/// the server's own threads: the session's id and the job's.
fn session_with_a_hunt(port: u16, token: &str) -> (String, i64) {
    let (head, init) = post(
        port,
        token,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "quit-signal-test", "version": "1"}}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "jetcarp", "{init}");
    let sid = head
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("a session")
        .to_string();
    let (_, r) = post(
        port,
        token,
        Some(&sid),
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "search_start", "arguments":
                    {"hunt": {"rules": ["B3/S23"], "soups": 0}, "label": "until-stopped"}}}),
    );
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
    let started: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let job = started["job"]
        .as_i64()
        .unwrap_or_else(|| panic!("a job: {r}"));
    (sid, job)
}

/// SIGTERM stops `jetcarp mcp --http` as Ctrl-C does: its jobs stop at their next checkpoint,
/// paused and resumable, and it ends with 0, `mcp-http-endpoint.json` and its memory record gone
/// from the runtime folder. Before, it ended at once and left both.
#[test]
fn sigterm_stops_mcp_over_http_as_its_own_shutdown_does() {
    let home = Home::new("mcp");
    let file = home.runtime().join("mcp-http-endpoint.json");
    let mut server = home.mcp_http();
    let record = home.record(server.pid());
    server.wait_until("endpoint file and memory record", || {
        file.exists() && record.exists()
    });
    let (port, token) = endpoint(&file);
    let (_, job) = session_with_a_hunt(port, &token);

    let (status, err) = server.terminate();
    assert!(
        !file.exists(),
        "SIGTERM left {} behind (the server ended with {status:?}); its stderr:\n{err}",
        file.display()
    );
    assert!(
        status.success(),
        "the server ends as its own shutdown ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the server left its memory record {}; its stderr:\n{err}",
        record.display()
    );
    assert!(
        err.contains(&format!("job {job} is paused")),
        "its hunt stops at a checkpoint, paused: its stderr:\n{err}"
    );
}

/// A second signal ends `jetcarp mcp --http` at once, with 128 + its number. A caller still
/// connected holds a stopping server for up to a second, as it waits for its callers to hang up
/// once the last answers are written, so the second signal comes while it waits. (Before the
/// calls in flight were cut short, a `job_status` still waiting held it.)
#[test]
fn a_second_signal_ends_mcp_over_http_at_once() {
    let home = Home::new("mcp-twice");
    let file = home.runtime().join("mcp-http-endpoint.json");
    let mut server = home.mcp_http();
    server.wait_until("endpoint file", || file.exists());
    let (port, token) = endpoint(&file);
    let _ = session_with_a_hunt(port, &token);
    let _caller = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // The server takes the connection in.
    std::thread::sleep(Duration::from_millis(300));

    server.signal("TERM");
    let stopping = server.wait_for_line("stopping", Duration::from_secs(30));
    assert!(
        stopping,
        "the server did not say it was stopping; it ended with {:?}; its stderr:\n{}",
        server.child.try_wait(),
        server.stderr()
    );
    server.signal("TERM");
    let status = server.ended_within(Duration::from_secs(5));
    let err = server.stderr();
    let status = status
        .unwrap_or_else(|| panic!("still running after a second SIGTERM; its stderr:\n{err}"));
    assert_eq!(
        status.code(),
        Some(128 + 15),
        "a second SIGTERM ends it at once: {status:?}; its stderr:\n{err}"
    );
}

/// A `job_status` call waiting for `job` to end (up to two minutes), as JSON-RPC request `id`.
fn waiting_call(id: i64, job: i64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": "job_status", "arguments": {"job": job, "wait_s": 120}}})
}

/// The first text of a `tools/call` answer.
fn answer_text(answer: &Value) -> &str {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
}

/// SIGTERM cuts a call still running short: a `job_status` waiting up to two minutes for a hunt
/// to end answers at once that the server is stopping, and the server ends within a few seconds,
/// with 0, its endpoint file and memory record gone. Before, the server waited for the call to
/// return, up to the 600 s a wait may take, longer than a session gives its programs at logout.
#[test]
fn sigterm_cuts_a_waiting_call_short_and_mcp_over_http_ends_soon() {
    let home = Home::new("mcp-call");
    let file = home.runtime().join("mcp-http-endpoint.json");
    let mut server = home.mcp_http();
    let record = home.record(server.pid());
    server.wait_until("endpoint file and memory record", || {
        file.exists() && record.exists()
    });
    let (port, token) = endpoint(&file);
    let (sid, job) = session_with_a_hunt(port, &token);
    let waiting = std::thread::spawn(move || post(port, &token, Some(&sid), &waiting_call(3, job)));
    // The server takes the call in and waits on it.
    std::thread::sleep(Duration::from_secs(1));

    let t0 = Instant::now();
    server.signal("TERM");
    let status = server.ended_within(Duration::from_secs(10));
    let took = t0.elapsed();
    std::thread::sleep(Duration::from_millis(100));
    let err = server.stderr();
    let status = status.unwrap_or_else(|| {
        panic!("still running 10 s after SIGTERM, held by the waiting call; its stderr:\n{err}")
    });
    assert!(
        took < Duration::from_secs(3),
        "the server took {took:?} to end after SIGTERM; its stderr:\n{err}"
    );
    assert!(
        status.success(),
        "the server ends with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        !file.exists(),
        "the server left {} behind; its stderr:\n{err}",
        file.display()
    );
    assert!(
        !record.exists(),
        "the server left its memory record {}; its stderr:\n{err}",
        record.display()
    );
    let (_, answer) = waiting.join().expect("the waiting call has its answer");
    assert!(
        answer_text(&answer).contains("the server is stopping"),
        "the waiting call is told the server is stopping: {answer}"
    );
    assert_eq!(answer["result"]["isError"], true, "{answer}");
}

/// A call that does not watch its cancel flag is left once the stop's grace (2 s) is over: the
/// server ends without its answer, with 0, its endpoint file and memory record gone, as it
/// would after its last call. The stand-in is `use_app` reaching for an "app" that takes the
/// connection and never answers, which waits 5 s for its handshake.
#[test]
fn a_call_that_does_not_stop_is_left_after_the_grace() {
    let home = Home::new("mcp-grace");
    let file = home.runtime().join("mcp-http-endpoint.json");
    let mut server = home.mcp_http();
    let record = home.record(server.pid());
    server.wait_until("endpoint file and memory record", || {
        file.exists() && record.exists()
    });
    let (port, token) = endpoint(&file);
    let (head, _) = post(
        port,
        &token,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "quit-signal-test", "version": "1"}}}),
    );
    let sid = head
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("a session")
        .to_string();
    let silent = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let app_port = silent.local_addr().unwrap().port();
    let _held = std::thread::spawn(move || {
        let held: Vec<TcpStream> = silent.incoming().take(1).map_while(Result::ok).collect();
        std::thread::sleep(Duration::from_secs(30));
        drop(held);
    });
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            post(
                port,
                &token,
                Some(&sid),
                &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": {"name": "use_app", "arguments":
                            {"attach": true, "port": app_port, "token": "silent"}}}),
            )
        });
    });
    // The server takes the call in and waits for the "app" to answer.
    std::thread::sleep(Duration::from_secs(1));

    let t0 = Instant::now();
    server.signal("TERM");
    let status = server.ended_within(Duration::from_secs(10));
    let took = t0.elapsed();
    std::thread::sleep(Duration::from_millis(100));
    let err = server.stderr();
    let status =
        status.unwrap_or_else(|| panic!("still running 10 s after SIGTERM; its stderr:\n{err}"));
    assert!(
        took < Duration::from_secs(3),
        "the server took {took:?} to end after SIGTERM; its stderr:\n{err}"
    );
    assert!(
        status.success(),
        "the server ends with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        err.contains("a call was still running"),
        "it says it ended without the call's answer; its stderr:\n{err}"
    );
    assert!(
        !file.exists(),
        "the server left {} behind; its stderr:\n{err}",
        file.display()
    );
    assert!(
        !record.exists(),
        "the server left its memory record {}; its stderr:\n{err}",
        record.display()
    );
}

/// SIGTERM stops `jetcarp mcp` over stdio as stdin closing does: a call still running is cut
/// short with an answer that says the server is stopping, the hunt it runs on its own threads
/// stops at its next checkpoint, paused and resumable, and it ends with 0, its memory record
/// gone. Before, it ended at once, the hunt left marked running with nothing running it.
#[test]
fn sigterm_stops_mcp_over_stdio_as_stdin_closing_does() {
    let home = Home::new("mcp-stdio");
    let mut c = home.jetcarp(&["mcp"]);
    c.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut server = Running::start(c);
    let record = home.record(server.pid());
    let mut stdin = server.child.stdin.take().unwrap();
    let out = server.child.stdout.take().unwrap();
    let (tx, replies) = channel::<Value>();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                let _ = tx.send(v);
            }
        }
    });
    let reply = |id: i64| loop {
        let v = replies
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| panic!("no answer to request {id}"));
        if v["id"] == id {
            return v;
        }
    };
    let mut send = |v: Value| {
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
    };
    send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "quit-signal-test", "version": "1"}}}));
    assert_eq!(reply(1)["result"]["serverInfo"]["name"], "jetcarp");
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "search_start", "arguments":
                    {"hunt": {"rules": ["B3/S23"], "soups": 0}, "label": "until-stopped"}}}));
    let started = reply(2);
    let job = serde_json::from_str::<Value>(answer_text(&started)).unwrap_or(Value::Null)["job"]
        .as_i64()
        .unwrap_or_else(|| panic!("a job: {started}"));
    send(waiting_call(3, job));
    server.wait_until("memory record", || record.exists());
    // The server takes the call in and waits on it.
    std::thread::sleep(Duration::from_secs(1));

    let t0 = Instant::now();
    server.signal("TERM");
    let status = server.ended_within(Duration::from_secs(10));
    let took = t0.elapsed();
    std::thread::sleep(Duration::from_millis(100));
    let err = server.stderr();
    let status =
        status.unwrap_or_else(|| panic!("still running 10 s after SIGTERM; its stderr:\n{err}"));
    assert!(
        status.success(),
        "the server ends as stdin closing ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        took < Duration::from_secs(3),
        "the server took {took:?} to end after SIGTERM; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the server left its memory record {}; its stderr:\n{err}",
        record.display()
    );
    assert!(
        err.contains(&format!("job {job} is paused")),
        "its hunt stops at a checkpoint, paused: its stderr:\n{err}"
    );
    let answer = reply(3);
    assert!(
        answer_text(&answer).contains("the server is stopping"),
        "the waiting call is told the server is stopping: {answer}"
    );
}

/// `jetcarp jobs status JOB --json` in `home`.
fn job_view(home: &Home, job: i64) -> Value {
    let job = job.to_string();
    let out = home
        .jetcarp(&["jobs", "status", &job, "--json"])
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jobs status {job}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// SIGTERM stops a foreground `jetcarp search` as Ctrl-C does: its job stops at its next
/// checkpoint, paused and resumable, and it ends with 0, its memory record gone. `--resume` then
/// carries the job on from that checkpoint. Before, it ended at once, the job left marked running
/// with nothing running it.
#[test]
fn sigterm_stops_a_search_at_its_checkpoint_and_resume_carries_it_on() {
    let home = Home::new("search");
    let mut search = Running::start(home.jetcarp(&[
        "search",
        "hunt",
        "--forever",
        "--label",
        "until-stopped",
        "--threads",
        "2",
    ]));
    let record = home.record(search.pid());
    assert!(
        search.wait_for_line("hunt job 1 in", Duration::from_secs(60)),
        "the hunt did not start; its stderr:\n{}",
        search.stderr()
    );
    search.wait_until("memory record", || record.exists());
    search.wait_until("a checkpoint", || {
        job_view(&home, 1)["done"].as_u64().unwrap_or(0) > 0
    });

    let (status, err) = search.terminate();
    assert!(
        status.success(),
        "the search ends as Ctrl-C ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        err.contains("SIGTERM: stopping at the next checkpoint"),
        "it says it is stopping; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the search left its memory record {}; its stderr:\n{err}",
        record.display()
    );
    let v = job_view(&home, 1);
    assert_eq!(v["status"], "paused", "the job is paused: {v}");
    assert_eq!(v["stale"], false, "{v}");
    assert_eq!(v["resumable"], true, "{v}");
    let done = v["done"].as_u64().unwrap();
    assert!(done > 0, "{v}");

    let resumed = home
        .jetcarp(&[
            "search", "hunt", "--resume", "1", "--limit", "300", "--quiet",
        ])
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "--resume: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let v = job_view(&home, 1);
    let after = v["done"].as_u64().unwrap();
    assert!(
        after >= done + 300,
        "--resume carries on from the checkpoint's {done} soups: {after} after 300 more: {v}"
    );
}

/// SIGTERM stops `jetcarp script` at its next step, as Ctrl-C does: it says it stopped, and ends
/// with 128 + the signal's number (130 for Ctrl-C, as before), its memory record gone. Before, it
/// ended at once.
#[test]
fn sigterm_stops_a_script_at_its_next_step() {
    let home = Home::new("script");
    let file = home.0.join("until-stopped.rhai");
    std::fs::write(
        &file,
        "let w = world(#{ tiling: \"square\", size: [64, 64], boundary: \"wrap\" });\n\
         w.add_ca(\"B3/S23\", #{ seed: \"random:0.4\" });\n\
         print(\"stepping\");\n\
         loop { w.step(1); }\n",
    )
    .unwrap();
    // --json: the script's own lines go to standard error, with the rest.
    let mut script = Running::start(home.jetcarp(&["script", file.to_str().unwrap(), "--json"]));
    let record = home.record(script.pid());
    assert!(
        script.wait_for_line("stepping", Duration::from_secs(60)),
        "the script did not start; its stderr:\n{}",
        script.stderr()
    );
    script.wait_until("memory record", || record.exists());

    let (status, err) = script.terminate();
    assert_eq!(
        status.code(),
        Some(128 + 15),
        "the script ends as Ctrl-C ends it, with 128 + the signal's number: {status:?}; its \
         stderr:\n{err}"
    );
    assert!(
        err.contains("SIGTERM: stopping") && err.contains("cancelled after"),
        "it says it stopped; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the script left its memory record {}; its stderr:\n{err}",
        record.display()
    );
}
