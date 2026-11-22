//! The background host (`jetcarp host serve`) and `jetcarp mcp --http` ended by a signal (#278):
//! SIGTERM (what `kill` sends, launchd at logout on the Mac, and a Linux session as it ends),
//! SIGHUP and SIGINT stop each as its own stop does. The host stops as `jetcarp host stop` stops
//! it, and the HTTP server as Ctrl-C stopped it: its jobs at their next checkpoint. Each removes
//! its endpoint file and its record in the memory pool (`memory/<pid>.json`) from the runtime
//! folder, which before were left behind. A second signal ends either at once, for one that does
//! not stop.
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

/// A second signal ends `jetcarp mcp --http` at once, with 128 + its number, while the first
/// waits on a call that has not returned: a `job_status` waiting for a hunt to end.
#[test]
fn a_second_signal_ends_mcp_over_http_at_once() {
    let home = Home::new("mcp-twice");
    let file = home.runtime().join("mcp-http-endpoint.json");
    let mut server = home.mcp_http();
    server.wait_until("endpoint file", || file.exists());
    let (port, token) = endpoint(&file);
    let (sid, job) = session_with_a_hunt(port, &token);
    let waiting = std::thread::spawn(move || {
        let body = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "job_status", "arguments": {"job": job, "wait_s": 120}}})
        .to_string();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let _ = write!(
            s,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
             Mcp-Session-Id: {sid}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        // Whatever comes back, if anything, once the server has gone.
        let _ = s.read_to_end(&mut vec![]);
    });
    // The server takes the call in and waits on it.
    std::thread::sleep(Duration::from_secs(1));

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
    // The waiting call's connection closed with the server.
    let _ = waiting.join();
}
