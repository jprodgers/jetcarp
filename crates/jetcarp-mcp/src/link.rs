//! The local link between `jetcarp mcp --app` and a running JETCARP app.
//!
//! The app (when the user turns AI control on) listens on `127.0.0.1` only. A connection first
//! sends one line `{"jetcarp_token": "..."}`; the app answers `{"ok": true}` or closes. After that
//! both sides exchange newline-delimited JSON-RPC messages, exactly as MCP over stdio. The app
//! answers them on its UI thread with the same [`crate::Server`] the CLI uses.
//!
//! The port and token are also written to [`endpoint_file`] (readable only by this user) so the
//! CLI finds them without configuration.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 47811;
/// Longest line accepted on the link (a large scene JSON fits).
const MAX_LINE: usize = 256 << 20;
/// How long a connection waits for the UI thread to answer one request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// The file the app records its port and token in.
pub const APP_ENDPOINT: &str = "app-endpoint.json";
/// The file the background host records its port and token in.
pub const HOST_ENDPOINT: &str = "host-endpoint.json";

/// Where the running app records its port and token.
pub fn endpoint_file() -> Option<PathBuf> {
    runtime_file(APP_ENDPOINT)
}

/// A file in JETCARP's per-user runtime folder (`JETCARP_RUNTIME_DIR` when set, which tests use since
/// Windows has no `XDG_RUNTIME_DIR`).
pub fn runtime_file(name: &str) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("JETCARP_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join(name));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    Some(base.join("jetcarp").join(name))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointInfo {
    pub port: u16,
    pub token: String,
}

impl EndpointInfo {
    pub fn read() -> Result<EndpointInfo, String> {
        EndpointInfo::read_from(
            APP_ENDPOINT,
            "the JETCARP app is not accepting AI control. Open the JETCARP app and switch on 'AI control' (top bar), then try again.",
        )
    }

    /// The endpoint recorded in runtime file `name`; `absent` is the error when there is none.
    pub fn read_from(name: &str, absent: &str) -> Result<EndpointInfo, String> {
        let p = runtime_file(name).ok_or("no home or runtime directory")?;
        let text = std::fs::read_to_string(&p).map_err(|_| absent.to_string())?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(EndpointInfo {
            port: v["port"].as_u64().ok_or("endpoint file has no port")? as u16,
            token: v["token"]
                .as_str()
                .ok_or("endpoint file has no token")?
                .to_string(),
        })
    }
}

/// 128 random bits as hex. std's `RandomState` keys come from the OS random source.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|k| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(k);
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            );
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// A request waiting for the UI thread.
pub struct Pending {
    pub message: Value,
    reply: Sender<Option<Value>>,
    notes: Sender<Value>,
}

impl Pending {
    pub fn reply(self, response: Option<Value>) {
        let _ = self.reply.send(response);
    }
    /// Sends notifications (`notifications/progress`) to the client while this request runs;
    /// the app sets it as its server's notifier for the request.
    pub fn notifier(&self) -> crate::protocol::Notifier {
        let tx = Mutex::new(self.notes.clone());
        Arc::new(move |v: Value| {
            let _ = tx.lock().unwrap().send(v);
        })
    }
}

/// The app side: a loopback listener feeding [`Pending`] requests to the UI thread.
pub struct Listener {
    pub port: u16,
    pub token: String,
    rx: Receiver<Pending>,
    stop: Arc<AtomicBool>,
    pub connections: Arc<Mutex<u32>>,
    wrote_file: bool,
    /// The runtime file this listener records itself in.
    file: &'static str,
    /// Cancel flags of the requests in flight: a connection sets one as soon as a
    /// `notifications/cancelled` arrives, while the app is still busy with the call. The app
    /// gives its server the same map.
    pub cancels: crate::protocol::CancelMap,
    /// Requests taken from a connection whose answer has not been written back yet.
    in_flight: Arc<AtomicUsize>,
}

/// Counts one request as in flight until dropped, which is after its answer is written.
struct Busy(Arc<AtomicUsize>);

impl Busy {
    fn new(count: &Arc<AtomicUsize>) -> Busy {
        count.fetch_add(1, Ordering::SeqCst);
        Busy(count.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Listener {
    /// Listen on `127.0.0.1:port` (0 = any free port). `wake` is called after each request is
    /// queued, so an idle UI can repaint and answer it.
    pub fn start(port: u16, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Listener> {
        Listener::start_as(port, APP_ENDPOINT, wake)
    }

    /// [`start`](Self::start), recording the endpoint in runtime file `file`.
    pub fn start_as(
        port: u16,
        file: &'static str,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Listener> {
        let sock = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        sock.set_nonblocking(true)?;
        let port = sock.local_addr()?.port();
        let token = new_token();
        let (tx, rx) = channel::<Pending>();
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(0));
        let cancels = crate::protocol::CancelMap::default();
        let in_flight = Arc::new(AtomicUsize::new(0));
        {
            let (stop, token, connections, cancels, in_flight) = (
                stop.clone(),
                token.clone(),
                connections.clone(),
                cancels.clone(),
                in_flight.clone(),
            );
            std::thread::Builder::new()
                .name("jetcarp-ai-listen".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        match sock.accept() {
                            Ok((s, addr)) if addr.ip().is_loopback() => {
                                let (tx, token, wake, stop, connections, cancels, in_flight) = (
                                    tx.clone(),
                                    token.clone(),
                                    wake.clone(),
                                    stop.clone(),
                                    connections.clone(),
                                    cancels.clone(),
                                    in_flight.clone(),
                                );
                                let _ = std::thread::Builder::new()
                                    .name("jetcarp-ai-conn".into())
                                    .spawn(move || {
                                        *connections.lock().unwrap() += 1;
                                        let _ = serve(
                                            s, &token, &tx, &*wake, &stop, cancels, &in_flight,
                                        );
                                        *connections.lock().unwrap() -= 1;
                                    });
                            }
                            Ok(_) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(40));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(200)),
                        }
                    }
                })?;
        }
        let mut l = Listener {
            port,
            token,
            rx,
            stop,
            connections,
            wrote_file: false,
            file,
            cancels,
            in_flight,
        };
        l.wrote_file = l.write_file().is_ok();
        Ok(l)
    }

    fn write_file(&self) -> std::io::Result<()> {
        let p =
            runtime_file(self.file).ok_or_else(|| std::io::Error::other("no runtime directory"))?;
        let dir = p.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let text =
            json!({"port": self.port, "token": self.token, "pid": std::process::id()}).to_string();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&p)?;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            f.write_all(text.as_bytes())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&p, text)
        }
    }

    /// Requests that arrived since the last call.
    pub fn poll(&self) -> Vec<Pending> {
        self.rx.try_iter().collect()
    }

    pub fn connection_count(&self) -> u32 {
        *self.connections.lock().unwrap()
    }

    /// Whether some answer has been given but not yet written back to its connection. A program
    /// that quits on request waits for this, or the answer to "quit" is lost and the caller sees
    /// the connection reset.
    pub fn busy(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) > 0
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if self.wrote_file
            && let Some(p) = runtime_file(self.file)
        {
            // Only remove the file if it is still ours.
            if let Ok(info) = EndpointInfo::read_from(self.file, "")
                && info.token == self.token
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

fn read_line_limited<R: BufRead>(r: &mut R, buf: &mut String) -> std::io::Result<usize> {
    buf.clear();
    let n = Read::take(&mut *r, MAX_LINE as u64).read_line(buf)?;
    if n == MAX_LINE && !buf.ends_with('\n') {
        return Err(std::io::Error::other("line too long"));
    }
    Ok(n)
}

fn serve(
    s: TcpStream,
    token: &str,
    tx: &Sender<Pending>,
    wake: &(dyn Fn() + Send + Sync),
    stop: &AtomicBool,
    cancels: crate::protocol::CancelMap,
    in_flight: &Arc<AtomicUsize>,
) -> std::io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut w = s.try_clone()?;
    let mut r = BufReader::new(s);
    let mut line = String::new();
    read_line_limited(&mut r, &mut line)?;
    let ok = serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|v| {
            v.get("jetcarp_token")
                .and_then(|t| t.as_str())
                .map(String::from)
        })
        .is_some_and(|t| constant_time_eq(t.as_bytes(), token.as_bytes()));
    if !ok {
        writeln!(
            w,
            "{}",
            json!({"ok": false, "error": "bad or missing token"})
        )?;
        return Ok(());
    }
    writeln!(
        w,
        "{}",
        json!({"ok": true, "server": crate::protocol::server_info()})
    )?;
    r.get_ref().set_read_timeout(None)?;
    // Read on a thread of its own, so a cancellation reaches its flag while the app is still
    // busy with the request it names; everything else is answered in order.
    let (ltx, lrx) = channel::<Result<Value, String>>();
    std::thread::Builder::new()
        .name("jetcarp-ai-read".into())
        .spawn(move || {
            let mut line = String::new();
            loop {
                match read_line_limited(&mut r, &mut line) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let m = serde_json::from_str::<Value>(line.trim()).map_err(|e| e.to_string());
                if let Ok(v) = &m
                    && crate::protocol::cancel_request(&cancels, v)
                {
                    continue;
                }
                if ltx.send(m).is_err() {
                    return;
                }
            }
        })?;
    for m in lrx {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _busy = Busy::new(in_flight);
        let reply = match m {
            Err(e) => Some(crate::protocol::error_response(
                &Value::Null,
                crate::protocol::PARSE_ERROR,
                &format!("parse error: {e}"),
                None,
            )),
            Ok(msg) => {
                let (rtx, rrx) = channel();
                let (ntx, nrx) = channel();
                let id = msg.get("id").cloned();
                if tx
                    .send(Pending {
                        message: msg,
                        reply: rtx,
                        notes: ntx,
                    })
                    .is_err()
                {
                    return Ok(());
                }
                wake();
                let t0 = std::time::Instant::now();
                loop {
                    for n in nrx.try_iter() {
                        writeln!(w, "{n}")?;
                        w.flush()?;
                    }
                    match rrx.recv_timeout(Duration::from_millis(50)) {
                        Ok(r) => {
                            for n in nrx.try_iter() {
                                writeln!(w, "{n}")?;
                            }
                            break r;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                            if t0.elapsed() < REPLY_TIMEOUT => {}
                        Err(_) => {
                            break id.map(|id| {
                                crate::protocol::error_response(
                                    &id,
                                    crate::protocol::INTERNAL_ERROR,
                                    "the JETCARP app did not answer in time (is its window open and not minimised?)",
                                    None,
                                )
                            });
                        }
                    }
                }
            }
        };
        if let Some(r) = reply {
            writeln!(w, "{r}")?;
            w.flush()?;
        }
    }
    Ok(())
}

/// The host's answer to one request: `args` holds its `action`. Fails when no host is running.
pub fn host_call(args: &Value) -> Result<Value, String> {
    const NOT_RUNNING: &str = "the JETCARP background host is not running";
    let info = EndpointInfo::read_from(HOST_ENDPOINT, NOT_RUNNING)?;
    let mut c = AppClient::connect(Some(info)).map_err(|_| NOT_RUNNING.to_string())?;
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": HOST_TOOL, "arguments": args}});
    let r = c.call(&msg)?;
    if let Some(e) = r.get("error") {
        return Err(e["message"].as_str().unwrap_or("host error").to_string());
    }
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
    if r["result"]["isError"] == true {
        return Err(text.to_string());
    }
    serde_json::from_str(text).map_err(|e| format!("host: bad reply: {e}"))
}

/// Whether the background host has gone: its endpoint file is gone, or nothing listens on its
/// port any more. Unlike a status call this never waits for an answer, so it can be polled while
/// a host shuts down: the host leaves its still picture (if it leaves one) before it closes its
/// port and removes the file.
pub fn host_gone() -> bool {
    let Ok(info) = EndpointInfo::read_from(HOST_ENDPOINT, "") else {
        return true;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err()
}

/// The one tool the host answers.
pub const HOST_TOOL: &str = "host_control";

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The CLI side: one authenticated connection to the app.
pub struct AppClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    pub port: u16,
}

impl AppClient {
    /// Connect using `info`, or the endpoint file when `None`.
    pub fn connect(info: Option<EndpointInfo>) -> Result<AppClient, String> {
        let info = match info {
            Some(i) => i,
            None => EndpointInfo::read()?,
        };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| {
            format!(
                "could not reach the JETCARP app on 127.0.0.1:{} ({e}). Is the app open with AI control on?",
                info.port
            )
        })?;
        let mut writer = s.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(s);
        writeln!(writer, "{}", json!({"jetcarp_token": info.token})).map_err(|e| e.to_string())?;
        reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("app handshake: {e}"))?;
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        if v["ok"] != json!(true) {
            return Err(format!(
                "the JETCARP app refused the connection: {}. Turn AI control off and on again in the app, or pass the token it shows.",
                v["error"].as_str().unwrap_or("no answer")
            ));
        }
        reader
            .get_ref()
            .set_read_timeout(Some(REPLY_TIMEOUT + Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        Ok(AppClient {
            reader,
            writer,
            port: info.port,
        })
    }

    /// A second handle on the connection, for writing a cancellation while a call waits.
    pub fn writer(&self) -> Option<TcpStream> {
        self.writer.try_clone().ok()
    }

    /// Send one request and wait for its response.
    pub fn call(&mut self, msg: &Value) -> Result<Value, String> {
        self.call_with(msg, &mut |_| {})
    }

    /// [`call`](Self::call), passing the notifications the app sends meanwhile (progress) to
    /// `note`.
    pub fn call_with(
        &mut self,
        msg: &Value,
        note: &mut dyn FnMut(&Value),
    ) -> Result<Value, String> {
        writeln!(self.writer, "{msg}").map_err(|e| format!("app link: {e}"))?;
        self.writer.flush().map_err(|e| e.to_string())?;
        if msg.get("id").is_none() {
            return Ok(Value::Null);
        }
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("app link: {e}"))?;
            if n == 0 {
                return Err("the JETCARP app closed the connection".into());
            }
            let v: Value =
                serde_json::from_str(&line).map_err(|e| format!("app link: bad reply: {e}"))?;
            if v.get("method").is_some() && v.get("id").is_none() {
                note(&v);
                continue;
            }
            return Ok(v);
        }
    }
}
