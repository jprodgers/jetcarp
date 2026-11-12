//! The local link between `jetcarp mcp --app` and a running JETCARP app.
//!
//! The app (when the user turns AI control on) listens on `127.0.0.1` only. A connection first
//! sends one line `{"jetcarp_token": "..."}`; the app answers `{"ok": true}` or closes. After that
//! both sides exchange newline-delimited JSON-RPC messages, exactly as MCP over stdio. The app
//! answers them on its UI thread with the same [`crate::Server`] the CLI uses.
//!
//! The token is 128 bits from the operating system's random source, made afresh each time a
//! listener starts. The port and token are also written to [`endpoint_file`] (on Unix in a
//! folder of mode 0700, the file 0600, both checked: readable only by this user) so the CLI finds
//! them without configuration. A connection that sends a wrong token is answered `{"ok": false}`
//! and closed; the comparison takes the same time whichever character differs.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
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
            "the JETCARP app is not accepting AI control. Open the JETCARP app, switch on AI control (Tools, AI control..., Allow AI control), then try again.",
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

/// A new token: 128 bits from the operating system's random source (`getrandom`: the
/// `getrandom` system call on Linux, `getentropy` on macOS, `ProcessPrng` on Windows), as 32
/// lower-case hex digits. Fails rather than fall back to anything weaker.
pub fn new_token() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b)
        .map_err(|e| std::io::Error::other(format!("no random source for the link token: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Write `text` to `p` so that only this user can read it: on Unix the folder is made (or made
/// again) mode 0700 and the file 0600, both checked afterwards; elsewhere the file inherits the
/// user's own profile folder's permissions. The file is written beside its place and renamed
/// over it, so a reader never sees half of it.
pub fn write_private(p: &Path, text: &str) -> std::io::Result<()> {
    let dir = p
        .parent()
        .ok_or_else(|| std::io::Error::other("the endpoint file has no folder"))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        p.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Err(std::io::Error::other(format!(
                "{} is not a folder",
                dir.display()
            )));
        }
        if meta.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, p)?;
        let (d, m) = (private_mode(dir)?, private_mode(p)?);
        if d != 0o700 || m != 0o600 {
            let _ = std::fs::remove_file(p);
            return Err(std::io::Error::other(format!(
                "{} could not be kept private (folder mode {d:o}, file mode {m:o})",
                p.display()
            )));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, p)
    }
}

/// A file's or folder's permission bits (Unix).
#[cfg(unix)]
pub fn private_mode(p: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::symlink_metadata(p)?.permissions().mode() & 0o777)
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
        let token = new_token()?;
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
        let text =
            json!({"port": self.port, "token": self.token, "pid": std::process::id()}).to_string();
        write_private(&p, &text)
    }

    /// Whether the endpoint file was written (the token is then readable by this user only).
    pub fn wrote_file(&self) -> bool {
        self.wrote_file
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
    let mut c = AppClient::connect_as(info, Peer::Host).map_err(|_| NOT_RUNNING.to_string())?;
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

/// Which program a connection is to, so that its errors name it: the app (AI control) or the
/// background host (`jetcarp host ...`), which listen the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    App,
    Host,
}

impl Peer {
    fn name(self) -> &'static str {
        match self {
            Peer::App => "the JETCARP app",
            Peer::Host => "the JETCARP background host",
        }
    }
    /// What to try when it will not talk.
    fn hint(self) -> &'static str {
        match self {
            Peer::App => "Turn AI control off and on again in the app, or pass the token it shows.",
            Peer::Host => "It may be stopping or starting; try again in a moment.",
        }
    }
}

/// The CLI side: one authenticated connection to the app (or the background host).
pub struct AppClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    pub port: u16,
    peer: Peer,
}

impl AppClient {
    /// Connect to the app using `info`, or the endpoint file when `None`.
    pub fn connect(info: Option<EndpointInfo>) -> Result<AppClient, String> {
        let info = match info {
            Some(i) => i,
            None => EndpointInfo::read()?,
        };
        Self::connect_as(info, Peer::App)
    }

    /// Connect to `peer` at `info`; errors name it.
    pub fn connect_as(info: EndpointInfo, peer: Peer) -> Result<AppClient, String> {
        let who = peer.name();
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| match peer {
            Peer::App => format!(
                "could not reach the JETCARP app on 127.0.0.1:{} ({e}). Is the app open with AI control on?",
                info.port
            ),
            Peer::Host => format!("could not reach {who} on 127.0.0.1:{} ({e})", info.port),
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
            .map_err(|e| format!("{who} did not answer the handshake ({e}). {}", peer.hint()))?;
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        if v["ok"] != json!(true) {
            return Err(format!(
                "{who} refused the connection: {}. {}",
                v["error"].as_str().unwrap_or("no answer"),
                peer.hint()
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
            peer,
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
        let who = self.peer.name();
        writeln!(self.writer, "{msg}").map_err(|e| format!("link to {who}: {e}"))?;
        self.writer.flush().map_err(|e| e.to_string())?;
        if msg.get("id").is_none() {
            return Ok(Value::Null);
        }
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("link to {who}: {e}"))?;
            if n == 0 {
                return Err(match self.peer {
                    Peer::App => format!("{who} closed the connection"),
                    Peer::Host => format!("{who} closed the connection (it may be stopping)"),
                });
            }
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| format!("link to {who}: bad reply: {e}"))?;
            if v.get("method").is_some() && v.get("id").is_none() {
                note(&v);
                continue;
            }
            return Ok(v);
        }
    }
}
