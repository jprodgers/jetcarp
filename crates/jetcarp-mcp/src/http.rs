//! MCP over HTTP: MCP's Streamable HTTP transport, for clients that connect to a URL rather than
//! start a program (Postman, web tools, remote setups). Off unless asked for: `jetcarp mcp --http
//! [PORT]` ([`serve`]), or the app's AI control window, which answers on its UI thread from
//! [`Listener::poll`].
//!
//! One endpoint, `http://127.0.0.1:PORT/mcp` ([`url`]):
//! - `POST` carries one JSON-RPC message. A request is answered with `application/json`, or with
//!   an event stream (`text/event-stream`: the call's `notifications/progress`, then its answer)
//!   when it is a `tools/call` with a progress token and the client accepts one. A notification
//!   or a response is answered `202 Accepted`.
//! - `DELETE` ends the session its `Mcp-Session-Id` names.
//! - `GET`, a stream of messages the server starts, is not offered: `405`.
//!
//! What guards it, in the order it is checked:
//! - The socket is bound to `127.0.0.1` only, which other computers cannot reach.
//! - `Host` must be `127.0.0.1:PORT` or `localhost:PORT`, and `Origin`, when there is one, `http://`
//!   one of those: a web page from anywhere else, DNS rebinding included, is refused (`403`).
//! - `Authorization: Bearer TOKEN`. The token is 128 bits from the operating system's random
//!   source, made afresh each time the endpoint starts and recorded in a runtime file only this
//!   user can read, as the app link's is ([`crate::link::write_private`]). It is compared in
//!   constant time. Without it, or with another, `401`.
//! - Sessions: `initialize` starts one, and its answer carries the id (`Mcp-Session-Id`, 128
//!   random bits) that every later request must carry (`400` without one, `404` once it has
//!   ended). A modern client (MCP 2026-07-28) that names its version in each request's `_meta`
//!   and never calls `initialize` is answered without a session.
//! - [`Limits`]: request heads of 16 KiB and bodies of 16 MiB (`431`, `413`), 16 connections at once
//!   (`503`), a head within 10 s and a body within 30 s (`408`), idle connections closed after
//!   30 s, and sessions ended after an hour unused.
//!
//! Each request is queued as a [`Pending`] for whoever answers it: `jetcarp mcp --http`'s main
//! thread, with a [`Client`](crate::stdio::Client) per session as over stdio (the same tools, the
//! same folder rules, a private world each), or the app's UI thread, which answers every session
//! on the world in its window. Requests are answered one at a time, in the order they come.

use crate::link;
use crate::protocol::{self, CancelMap, Notifier};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// The port asked for when none is given (the app link's is 47811).
pub const DEFAULT_PORT: u16 = 47812;
/// The endpoint's path.
pub const PATH: &str = "/mcp";
/// The runtime file `jetcarp mcp --http` records its URL and token in.
pub const CLI_ENDPOINT: &str = "mcp-http-endpoint.json";
/// The runtime file the app's HTTP endpoint records its URL and token in.
///
/// On Windows the runtime folder is also the data folder, so `jetcarp uninstall --data` counts
/// these two files as JETCARP's (`DATA_FILES` in `jetcarp-api`'s `ops/system.rs`, which cannot
/// see these): the names must stay in step.
pub const APP_ENDPOINT: &str = "app-http-endpoint.json";
/// The settings folder's private subfolder (0700 on Linux and macOS) that holds kept tokens.
/// `jetcarp uninstall --settings` counts it and its two files as JETCARP's (`HTTP_TOKEN_FILES`
/// in `jetcarp-api`'s `ops/system.rs`, which cannot see these): the names must stay in step.
pub const KEPT_DIR: &str = "http-tokens";
/// The file in [`KEPT_DIR`] holding `jetcarp mcp --http`'s kept token.
pub const CLI_KEPT: &str = "mcp-http.json";
/// The file in [`KEPT_DIR`] holding the app's kept token.
pub const APP_KEPT: &str = "app-http.json";

/// Where a kept token is kept: the file `name` ([`CLI_KEPT`], [`APP_KEPT`]) in the settings
/// folder's [`KEPT_DIR`]. `None` when there is no settings folder.
pub fn kept_file(name: &str) -> Option<PathBuf> {
    Some(jetcarp_core::paths::config_dir()?.join(KEPT_DIR).join(name))
}

const SESSION_HEADER: &str = "mcp-session-id";
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// Headers that may be given once only: two of any of them are refused.
const SINGLE: &[&str] = &[
    "host",
    "origin",
    "authorization",
    "content-length",
    "transfer-encoding",
    "content-type",
    SESSION_HEADER,
    "mcp-protocol-version",
];

/// How the endpoint is set up, beyond its port and file.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub limits: Limits,
    /// Keep the token for the next start, in [`kept_file`](Options::kept_file), and take the one
    /// kept there.
    pub keep_token: bool,
    /// Where the token is kept ([`kept_file`]).
    pub kept_file: Option<PathBuf>,
    /// Make a new token even when one is kept (and keep it).
    pub new_token: bool,
    /// Origins, as [`parse_origin`] gives them, whose pages may connect.
    pub allow_origins: Vec<String>,
}

/// An origin a user allows (`--allow-origin`, the app's list), checked, in the form browsers
/// send it: `http://` or `https://`, a host, and a port only when it is not the scheme's own, the
/// scheme and host in lower case. Only an exact origin: a wildcard, `null` (which pages with no
/// origin of their own send: files, sandboxed frames), a path, query, fragment or user name is
/// refused. Requests' `Origin` headers are put in the same form before they are compared.
pub fn parse_origin(s: &str) -> Result<String, String> {
    let bad = |why: &str| {
        Err(format!(
            "{s:?} is not an origin that can be allowed: {why}. Give one exactly, such as \
             https://client.example or http://localhost:6274"
        ))
    };
    let t = s.trim();
    if t.is_empty() {
        return bad("it is empty");
    }
    if t.eq_ignore_ascii_case("null") {
        return bad("\"null\" is sent by pages that have no origin of their own, from anywhere");
    }
    if t.contains('*') {
        return bad("wildcards are not allowed");
    }
    let Some((scheme, rest)) = t.split_once("://") else {
        return bad("it has no scheme (http:// or https://)");
    };
    let scheme = scheme.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return bad("the scheme must be http or https"),
    };
    if rest.contains(['/', '?', '#']) {
        return bad("an origin has no path, query or fragment");
    }
    if rest.contains('@') {
        return bad("an origin has no user name");
    }
    let (host, port) = match rest.strip_prefix('[') {
        // An IPv6 address.
        Some(r) => {
            let Some((h, after)) = r.split_once(']') else {
                return bad("an IPv6 address needs its closing ]");
            };
            if h.is_empty()
                || !h
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
            {
                return bad("the IPv6 address is not one");
            }
            let port = match after {
                "" => None,
                a => match a.strip_prefix(':') {
                    Some(p) => Some(p),
                    None => return bad("only a port may follow the host"),
                },
            };
            (format!("[{}]", h.to_ascii_lowercase()), port)
        }
        None => {
            let (h, port) = match rest.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (rest, None),
            };
            let ok = !h.is_empty()
                && h.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
                && !h.starts_with(['.', '-'])
                && !h.ends_with('-')
                && !h.contains("..");
            if !ok {
                return bad("the host is not a name or an address");
            }
            (h.trim_end_matches('.').to_ascii_lowercase(), port)
        }
    };
    let port = match port {
        None => None,
        Some(p) => match p.parse::<u16>() {
            Ok(n) if n > 0 && p.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return bad("a port is a number from 1 to 65535"),
        },
    };
    Ok(match port {
        Some(p) if p != default => format!("{scheme}://{host}:{p}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// The token kept in the file at `p` ([`kept_file`]), if it was written as kept, the file is
/// still readable by this user only (on Linux and macOS a file of mode 0600, not a link, in a
/// folder of mode 0700), and the token looks like one this endpoint makes.
fn kept_token(p: &Path) -> Option<String> {
    #[cfg(unix)]
    if link::private_mode(p).ok()? != 0o600 || link::private_mode(p.parent()?).ok()? != 0o700 {
        return None;
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    if v["kept"] != true {
        return None;
    }
    let t = v["token"].as_str()?;
    (t.len() == 32
        && t.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| t.to_string())
}

/// Keep `token` in the file at `p`: made readable by this user only, in a folder of its own
/// inside the settings folder (made, if need be, as any settings folder is).
fn write_kept(p: &Path, token: &str) -> std::io::Result<()> {
    if let Some(settings) = p.parent().and_then(Path::parent) {
        std::fs::create_dir_all(settings)?;
    }
    link::write_private(p, &json!({"token": token, "kept": true}).to_string())
}

/// The token recorded in the endpoint's runtime file at `p`.
fn recorded_token(p: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    v["token"].as_str().map(String::from)
}

/// The endpoint's URL on `port`.
pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// How much the endpoint takes, and for how long it waits.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Longest request head (request line and headers), in bytes.
    pub max_head: usize,
    /// Most headers in one request.
    pub max_headers: usize,
    /// Largest request body, in bytes.
    pub max_body: usize,
    /// Most connections open at once; one more is answered `503` and closed.
    pub max_connections: usize,
    /// Most sessions at once; a new one ends the one unused longest.
    pub max_sessions: usize,
    /// A request head must arrive within this of its first byte.
    pub head_timeout: Duration,
    /// A request body must arrive within this of its head.
    pub body_timeout: Duration,
    /// A connection with no request coming is closed after this.
    pub idle_timeout: Duration,
    /// A write to a client that does not read fails after this.
    pub write_timeout: Duration,
    /// Longest wait for a request's answer (the longest tool calls are ten minutes).
    pub reply_timeout: Duration,
    /// A session unused for this long is ended.
    pub session_idle: Duration,
    /// An event stream waiting for a long call gets a comment this often, so that it is not
    /// taken for dead.
    pub keepalive: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_head: 16 << 10,
            max_headers: 64,
            max_body: 16 << 20,
            max_connections: 16,
            max_sessions: 16,
            head_timeout: Duration::from_secs(10),
            body_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            reply_timeout: Duration::from_secs(15 * 60),
            session_idle: Duration::from_secs(60 * 60),
            keepalive: Duration::from_secs(15),
        }
    }
}

/// Called on the connection's thread for each `notifications/cancelled`, with the session it
/// came in (none: without one), after the call's flag is set: `jetcarp mcp --http` passes it to
/// the app when the call runs there.
pub type CancelHook = Arc<dyn Fn(Option<&str>, &Value) + Send + Sync>;

/// A request waiting for its answer.
pub struct Pending {
    pub message: Value,
    session: Option<String>,
    cancels: CancelMap,
    reply: Sender<Option<Value>>,
    notes: Sender<Value>,
}

impl Pending {
    /// The session the request came in (none: a modern client's, without one).
    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }
    /// The session's cancel flags: the server that answers sets them as its `cancels`, so that a
    /// `notifications/cancelled` reaches the call while it runs.
    pub fn cancels(&self) -> CancelMap {
        self.cancels.clone()
    }
    /// Sends notifications (`notifications/progress`) to the client while this request runs: the
    /// server's `notifier` for the request.
    pub fn notifier(&self) -> Notifier {
        let tx = self.notes.clone();
        Arc::new(move |v: Value| {
            let _ = tx.send(v);
        })
    }
    pub fn reply(self, response: Option<Value>) {
        let _ = self.reply.send(response);
    }
}

struct Session {
    cancels: CancelMap,
    last: Instant,
    /// Requests of the session being answered: a busy session is never ended for being idle.
    busy: usize,
}

struct Shared {
    port: u16,
    /// Made afresh, or taken from the file it was kept in; replaced by a new one on request.
    token: Mutex<String>,
    /// The other origins whose pages may connect, as [`parse_origin`] gives them.
    origins: Mutex<Vec<String>>,
    limits: Limits,
    stop: AtomicBool,
    connections: AtomicUsize,
    /// A handle on each open connection, to shut it down when the listener stops: a thread
    /// waiting to read wakes at once.
    open: Mutex<HashMap<u64, TcpStream>>,
    next: AtomicU64,
    sessions: Mutex<HashMap<String, Session>>,
    /// Cancel flags of the requests made without a session.
    sessionless: CancelMap,
    tx: Sender<Pending>,
    wake: Arc<dyn Fn() + Send + Sync>,
    on_cancel: Option<CancelHook>,
}

/// The endpoint: a listener on `127.0.0.1`, feeding [`Pending`] requests to whoever answers them.
/// Dropped, it stops accepting connections, closes the ones open soon after, and removes its
/// runtime file.
pub struct Listener {
    pub port: u16,
    pub token: String,
    rx: Receiver<Pending>,
    shared: Arc<Shared>,
    file: Option<PathBuf>,
    wrote_file: bool,
    /// The token is kept, in `kept_file`, for the next start to take.
    keep: bool,
    kept_file: Option<PathBuf>,
    /// Whether the token is in `kept_file` now, and why not when keeping failed.
    kept_ok: bool,
    kept_error: Option<String>,
}

impl Listener {
    /// Listen on `127.0.0.1:port` (0: any free port) with the default [`Limits`], recording the
    /// URL and token in the file at `file`. `wake` is called after each request is queued.
    pub fn start(
        port: u16,
        file: Option<PathBuf>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Listener> {
        Listener::start_with(port, file, Options::default(), wake, None)
    }

    /// [`start`](Self::start) with these options, and `on_cancel` called for each cancellation.
    pub fn start_with(
        port: u16,
        file: Option<PathBuf>,
        opts: Options,
        wake: Arc<dyn Fn() + Send + Sync>,
        on_cancel: Option<CancelHook>,
    ) -> std::io::Result<Listener> {
        let limits = opts.limits;
        let sock = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        sock.set_nonblocking(true)?;
        let port = sock.local_addr()?.port();
        let kept = (opts.keep_token && !opts.new_token)
            .then(|| opts.kept_file.as_deref().and_then(kept_token))
            .flatten();
        let token = match kept {
            Some(t) => t,
            None => link::new_token()?,
        };
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            port,
            token: Mutex::new(token.clone()),
            origins: Mutex::new(opts.allow_origins.clone()),
            limits,
            stop: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            open: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            sessions: Mutex::new(HashMap::new()),
            sessionless: CancelMap::default(),
            tx,
            wake,
            on_cancel,
        });
        {
            let sh = shared.clone();
            std::thread::Builder::new()
                .name("jetcarp-http-listen".into())
                .spawn(move || accept_loop(sock, sh))?;
        }
        let mut l = Listener {
            port,
            token,
            rx,
            shared,
            file,
            wrote_file: false,
            keep: opts.keep_token || opts.new_token,
            kept_file: opts.kept_file,
            kept_ok: false,
            kept_error: None,
        };
        // Kept: the token goes into the kept file now, so that it is kept even if this process
        // ends without stopping. Not kept: a token kept before is deleted.
        l.sync_kept();
        l.wrote_file = l.write_file().is_ok();
        Ok(l)
    }

    /// Put the kept file in step with `keep`: the token written in it, or the file deleted.
    fn sync_kept(&mut self) {
        self.kept_ok = false;
        self.kept_error = None;
        let Some(p) = &self.kept_file else {
            if self.keep {
                self.kept_error = Some("there is no settings folder".into());
            }
            return;
        };
        if self.keep {
            match write_kept(p, &self.token) {
                Ok(()) => self.kept_ok = true,
                Err(e) => self.kept_error = Some(e.to_string()),
            }
        } else {
            let _ = std::fs::remove_file(p);
        }
    }

    fn write_file(&self) -> std::io::Result<()> {
        let p =
            (self.file.as_ref()).ok_or_else(|| std::io::Error::other("no runtime directory"))?;
        let text = json!({"url": self.url(), "port": self.port, "token": self.token,
                          "pid": std::process::id(), "kept": self.keep})
        .to_string();
        link::write_private(p, &text)
    }

    /// The URL clients connect to.
    pub fn url(&self) -> String {
        url(self.port)
    }

    /// The header clients send.
    pub fn header(&self) -> String {
        format!("Authorization: Bearer {}", self.token)
    }

    /// The file the URL and token are recorded in, when they could be (readable by this user
    /// only).
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref().filter(|_| self.wrote_file)
    }

    /// Requests that arrived since the last call.
    pub fn poll(&self) -> Vec<Pending> {
        self.rx.try_iter().collect()
    }

    /// The next request, waiting up to `wait` for one.
    pub fn recv_timeout(&self, wait: Duration) -> Option<Pending> {
        self.rx.recv_timeout(wait).ok()
    }

    pub fn connection_count(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// Whether the token is kept for the next start: asked for, and written in the kept file.
    pub fn kept(&self) -> bool {
        self.keep && self.kept_ok
    }

    /// The file the token is kept in, while it is.
    pub fn kept_file(&self) -> Option<&Path> {
        self.kept_file.as_deref().filter(|_| self.kept())
    }

    /// Why the token could not be kept, when it was to be.
    pub fn kept_error(&self) -> Option<&str> {
        self.kept_error.as_deref()
    }

    /// Keep the token for the next start, or not: not keeping deletes the kept file at once.
    pub fn set_keep(&mut self, keep: bool) {
        self.keep = keep;
        self.sync_kept();
        self.wrote_file = self.write_file().is_ok();
    }

    /// A new token: the old one is refused from now on, the sessions started with it end, the
    /// kept file is deleted (and holds the new one instead, while keeping), and the runtime file
    /// records the new one.
    pub fn renew_token(&mut self) -> std::io::Result<()> {
        let t = link::new_token()?;
        *self.shared.token.lock().unwrap() = t.clone();
        self.shared.sessions.lock().unwrap().clear();
        self.token = t;
        if let Some(p) = &self.kept_file {
            let _ = std::fs::remove_file(p);
        }
        self.sync_kept();
        self.wrote_file = self.write_file().is_ok();
        Ok(())
    }

    /// The other origins whose pages may connect.
    pub fn origins(&self) -> Vec<String> {
        self.shared.origins.lock().unwrap().clone()
    }

    /// Set the other origins whose pages may connect, as [`parse_origin`] gives them.
    pub fn set_origins(&self, origins: Vec<String>) {
        *self.shared.origins.lock().unwrap() = origins;
    }

    /// The sessions open now.
    pub fn session_ids(&self) -> Vec<String> {
        self.shared
            .sessions
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for s in self.shared.open.lock().unwrap().values() {
            let _ = s.shutdown(Shutdown::Both);
        }
        // The runtime file goes, if it is still this listener's. A kept token stays in its own
        // file, in the settings folder.
        if self.wrote_file
            && let Some(p) = &self.file
            && recorded_token(p).as_deref() == Some(self.token.as_str())
        {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Counts one open connection, and keeps a handle on it, until dropped.
struct Counted(Arc<Shared>, u64);

impl Counted {
    fn new(sh: &Arc<Shared>, s: &TcpStream) -> Counted {
        sh.connections.fetch_add(1, Ordering::SeqCst);
        let id = sh.next.fetch_add(1, Ordering::SeqCst);
        if let Ok(handle) = s.try_clone() {
            sh.open.lock().unwrap().insert(id, handle);
        }
        Counted(sh.clone(), id)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.open.lock().unwrap().remove(&self.1);
        self.0.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept_loop(sock: TcpListener, sh: Arc<Shared>) {
    let mut swept = Instant::now();
    while !sh.stop.load(Ordering::SeqCst) {
        match sock.accept() {
            // Bound to 127.0.0.1, so every peer is this computer; checked all the same.
            Ok((s, addr)) if addr.ip().is_loopback() => {
                if sh.connections.load(Ordering::SeqCst) >= sh.limits.max_connections {
                    refuse_busy(s);
                    continue;
                }
                // Counted here, before the thread starts, so that a burst cannot pass the limit;
                // a thread that cannot start drops it.
                let counted = Counted::new(&sh, &s);
                let _ = std::thread::Builder::new()
                    .name("jetcarp-http-conn".into())
                    .spawn(move || {
                        let c = counted;
                        serve_connection(s, &c.0);
                    });
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
        if swept.elapsed() >= Duration::from_secs(5) {
            swept = Instant::now();
            let idle = sh.limits.session_idle;
            sh.sessions
                .lock()
                .unwrap()
                .retain(|_, s| s.busy > 0 || s.last.elapsed() < idle);
        }
    }
}

/// One connection too many: told so, and closed.
fn refuse_busy(mut s: TcpStream) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_write_timeout(Some(Duration::from_millis(500)));
    let r = Refusal::new(503, "too many connections at once; try again in a moment")
        .with("Retry-After", "1");
    let (ct, body) = r.body();
    let _ = respond(
        &mut s,
        r.status,
        &r.headers,
        Some((ct, &body)),
        false,
        false,
    );
    let _ = s.shutdown(Shutdown::Write);
    // What the client has sent already is read, without waiting for more, so that closing
    // does not reset the connection before the client reads the answer.
    let _ = s.set_nonblocking(true);
    let mut sink = [0u8; 4096];
    for _ in 0..16 {
        if !matches!(s.read(&mut sink), Ok(n) if n > 0) {
            break;
        }
    }
}

/// An HTTP error answer: its status, a JSON-RPC error with no id as its body (as the transport
/// allows), and any headers. The connection is closed after it.
#[derive(Debug)]
struct Refusal {
    status: u16,
    code: i64,
    message: String,
    headers: Vec<(&'static str, String)>,
}

impl Refusal {
    fn new(status: u16, message: impl Into<String>) -> Refusal {
        Refusal {
            status,
            code: match status {
                400 | 404 | 406 | 411 | 413 | 415 | 431 => protocol::INVALID_REQUEST,
                _ => -32000,
            },
            message: message.into(),
            headers: vec![],
        }
    }
    fn with(mut self, name: &'static str, value: impl Into<String>) -> Refusal {
        self.headers.push((name, value.into()));
        self
    }
    fn code(mut self, code: i64) -> Refusal {
        self.code = code;
        self
    }
    fn body(&self) -> (&'static str, Vec<u8>) {
        let v = protocol::error_response(&Value::Null, self.code, &self.message, None);
        ("application/json", v.to_string().into_bytes())
    }
}

/// Why a connection ends: refused (answered, then closed), or it failed or went quiet.
enum End {
    Refused(Refusal),
    Quiet,
}

impl From<Refusal> for End {
    fn from(r: Refusal) -> End {
        End::Refused(r)
    }
}

impl From<std::io::Error> for End {
    fn from(_: std::io::Error) -> End {
        End::Quiet
    }
}

/// What a connection does after a request has been answered.
enum Flow {
    KeepAlive,
    Close,
}

/// A request head.
struct Head {
    method: String,
    target: String,
    /// HTTP/1.`minor`.
    minor: u8,
    /// Names in lower case.
    headers: Vec<(String, String)>,
}

impl Head {
    fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.trim())
    }
    fn count(&self, name: &str) -> usize {
        self.headers.iter().filter(|(n, _)| n == name).count()
    }
    /// Whether the connection stays open after this request: HTTP/1.1 without
    /// `Connection: close`. HTTP/1.0 connections are closed.
    fn keep_alive(&self) -> bool {
        self.minor == 1
            && !self
                .headers
                .iter()
                .filter(|(n, _)| n == "connection")
                .flat_map(|(_, v)| v.split(','))
                .any(|t| t.trim().eq_ignore_ascii_case("close"))
    }
}

/// A connection's socket, with the bytes read past the last request.
struct Conn {
    s: TcpStream,
    buf: Vec<u8>,
    /// The request's `Origin`, when it is one the user allows: every answer to it carries the
    /// CORS headers that let its page read it.
    cors: Option<String>,
}

impl Conn {
    /// Read more into the buffer, by `deadline`. `Ok(0)`: the client closed, or the listener
    /// stopped (it shuts its connections down).
    ///
    /// One wait for the whole time left, and none after it runs out: on Windows a socket whose
    /// read has timed out is not to be read again.
    fn fill(&mut self, deadline: Instant) -> std::io::Result<usize> {
        let mut chunk = [0u8; 16 << 10];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::Error::new(ErrorKind::TimedOut, "timed out"));
            }
            self.s.set_read_timeout(Some(left))?;
            match self.s.read(&mut chunk) {
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(n);
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Err(std::io::Error::new(ErrorKind::TimedOut, "timed out"));
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// The next request head. `Ok(None)`: the client closed, or sent nothing for the idle time.
    fn read_head(&mut self, sh: &Shared) -> Result<Option<Head>, End> {
        let idle_until = Instant::now() + sh.limits.idle_timeout;
        let mut head_until = None;
        loop {
            if !self.buf.is_empty() {
                head_until.get_or_insert_with(|| Instant::now() + sh.limits.head_timeout);
                if let Some(h) = self.parse_head(sh)? {
                    return Ok(Some(h));
                }
                if self.buf.len() > sh.limits.max_head {
                    return Err(head_too_long(sh).into());
                }
            }
            match self.fill(head_until.unwrap_or(idle_until)) {
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::TimedOut && head_until.is_some() => {
                    return Err(Refusal::new(408, "the request head did not arrive in time").into());
                }
                Err(_) => return Ok(None),
            }
        }
    }

    /// Parse a whole head from the buffer, taking it out; `None` while it is incomplete.
    fn parse_head(&mut self, sh: &Shared) -> Result<Option<Head>, End> {
        let mut headers = vec![httparse::EMPTY_HEADER; sh.limits.max_headers];
        let mut req = httparse::Request::new(&mut headers);
        let n = match req.parse(&self.buf) {
            Ok(httparse::Status::Complete(n)) => n,
            Ok(httparse::Status::Partial) => return Ok(None),
            Err(httparse::Error::TooManyHeaders) => {
                return Err(Refusal::new(
                    431,
                    format!("more than {} headers", sh.limits.max_headers),
                )
                .into());
            }
            Err(httparse::Error::Version) => {
                return Err(Refusal::new(505, "this server speaks HTTP/1.1").into());
            }
            Err(e) => return Err(Refusal::new(400, format!("not an HTTP request ({e})")).into()),
        };
        // A head may arrive whole, longer than the limit, before the incomplete one is checked.
        if n > sh.limits.max_head {
            return Err(head_too_long(sh).into());
        }
        let head = Head {
            method: req.method.unwrap_or("").to_string(),
            target: req.path.unwrap_or("").to_string(),
            minor: req.version.unwrap_or(1),
            headers: req
                .headers
                .iter()
                .map(|h| {
                    (
                        h.name.to_ascii_lowercase(),
                        String::from_utf8_lossy(h.value).into_owned(),
                    )
                })
                .collect(),
        };
        self.buf.drain(..n);
        Ok(Some(head))
    }

    /// Exactly `n` more bytes, by `deadline`.
    fn take(&mut self, n: usize, deadline: Instant) -> Result<Vec<u8>, End> {
        while self.buf.len() < n {
            match self.fill(deadline) {
                Ok(0) => return Err(End::Quiet),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::TimedOut => {
                    return Err(Refusal::new(408, "the request body did not arrive in time").into());
                }
                Err(_) => return Err(End::Quiet),
            }
        }
        Ok(self.buf.drain(..n).collect())
    }

    /// One line of a chunked body (without its CRLF), of at most `max` bytes.
    fn line(&mut self, max: usize, deadline: Instant) -> Result<Vec<u8>, End> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                if i > max + 1 {
                    return Err(Refusal::new(400, "a chunk line is too long").into());
                }
                let mut l: Vec<u8> = self.buf.drain(..=i).collect();
                l.pop();
                if l.last() == Some(&b'\r') {
                    l.pop();
                }
                return Ok(l);
            }
            if self.buf.len() > max + 1 {
                return Err(Refusal::new(400, "a chunk line is too long").into());
            }
            self.take_more(deadline)?;
        }
    }

    fn take_more(&mut self, deadline: Instant) -> Result<(), End> {
        match self.fill(deadline) {
            Ok(0) => Err(End::Quiet),
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                Err(Refusal::new(408, "the request body did not arrive in time").into())
            }
            Err(_) => Err(End::Quiet),
        }
    }

    /// A body sent in chunks, of at most `max` bytes in all.
    fn chunked(&mut self, max: usize, deadline: Instant) -> Result<Vec<u8>, End> {
        let mut body = Vec::new();
        loop {
            let line = self.line(1024, deadline)?;
            let size = std::str::from_utf8(&line)
                .ok()
                .map(|l| l.split(';').next().unwrap_or("").trim())
                .filter(|s| !s.is_empty() && s.len() <= 8)
                .and_then(|s| usize::from_str_radix(s, 16).ok())
                .ok_or_else(|| Refusal::new(400, "a bad chunk size"))?;
            if size == 0 {
                // Trailers, up to the empty line.
                let mut trailers = 0;
                loop {
                    let l = self.line(1024, deadline)?;
                    if l.is_empty() {
                        return Ok(body);
                    }
                    trailers += l.len();
                    if trailers > 8 << 10 {
                        return Err(Refusal::new(400, "too many trailers").into());
                    }
                }
            }
            if body.len() + size > max {
                return Err(too_large(max).into());
            }
            body.extend(self.take(size, deadline)?);
            if !self.line(0, deadline)?.is_empty() {
                return Err(Refusal::new(400, "a chunk does not end where its size says").into());
            }
        }
    }

    /// Write an answer, stopping (`End::Quiet`) when the client does not take it.
    fn respond(
        &mut self,
        status: u16,
        headers: &[(&str, String)],
        body: Option<(&str, &[u8])>,
        keep: bool,
    ) -> Result<(), End> {
        let mut all = headers.to_vec();
        all.extend(
            self.cors_headers()
                .into_iter()
                .filter(|(k, _)| !headers.iter().any(|(h, _)| h.eq_ignore_ascii_case(k))),
        );
        respond(&mut self.s, status, &all, body, keep, false)?;
        Ok(())
    }

    /// The CORS headers an answer to an allowed origin's page carries: none for anyone else.
    fn cors_headers(&self) -> Vec<(&'static str, String)> {
        match &self.cors {
            Some(o) => vec![
                ("Access-Control-Allow-Origin", o.clone()),
                (
                    "Access-Control-Expose-Headers",
                    "Mcp-Session-Id, WWW-Authenticate".into(),
                ),
                ("Vary", "Origin".into()),
            ],
            None => vec![],
        }
    }

    /// One event of an event stream.
    fn event(&mut self, v: &Value) -> std::io::Result<()> {
        self.s
            .write_all(format!("event: message\ndata: {v}\n\n").as_bytes())
    }

    /// Close: no more writes, and what the client still sends is read and dropped for a moment,
    /// so that the last answer is not lost to a reset.
    fn close(mut self) {
        let _ = self.s.flush();
        let _ = self.s.shutdown(Shutdown::Write);
        let _ = self.s.set_read_timeout(Some(Duration::from_millis(200)));
        let t = Instant::now();
        let mut sink = [0u8; 8192];
        let mut total = 0usize;
        while t.elapsed() < Duration::from_millis(500) && total < 1 << 20 {
            match self.s.read(&mut sink) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Internal Server Error",
    }
}

/// Write a whole answer. `body` is left out for a `HEAD` request (`head_only`).
fn respond(
    s: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
    body: Option<(&str, &[u8])>,
    keep: bool,
    head_only: bool,
) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {status} {}\r\nDate: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: {}\r\n",
        reason(status),
        http_date(SystemTime::now()),
        if keep { "keep-alive" } else { "close" }
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    match body {
        Some((ct, b)) => out.push_str(&format!(
            "Content-Type: {ct}\r\nContent-Length: {}\r\n",
            b.len()
        )),
        None if status != 204 => out.push_str("Content-Length: 0\r\n"),
        None => {}
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    if let Some((_, b)) = body
        && !head_only
    {
        bytes.extend_from_slice(b);
    }
    s.write_all(&bytes)?;
    s.flush()
}

fn serve_connection(s: TcpStream, sh: &Shared) {
    if s.set_nonblocking(false).is_err()
        || s.set_write_timeout(Some(sh.limits.write_timeout)).is_err()
    {
        return;
    }
    let _ = s.set_nodelay(true);
    let mut c = Conn {
        s,
        buf: Vec::new(),
        cors: None,
    };
    loop {
        // Stopped: what is open is closed (a connection accepted as the listener stopped was
        // not among those it shut down).
        if sh.stop.load(Ordering::SeqCst) {
            return c.close();
        }
        c.cors = None;
        let (head_only, r) = match c.read_head(sh) {
            Ok(None) | Err(End::Quiet) => return c.close(),
            Err(End::Refused(r)) => (false, Err(End::Refused(r))),
            Ok(Some(h)) => (h.method == "HEAD", handle(&mut c, sh, &h)),
        };
        match r {
            Ok(Flow::KeepAlive) => {}
            Ok(Flow::Close) | Err(End::Quiet) => return c.close(),
            Err(End::Refused(r)) => {
                let body = r.body();
                let mut headers = r.headers.clone();
                headers.extend(c.cors_headers());
                let _ = respond(
                    &mut c.s,
                    r.status,
                    &headers,
                    Some((body.0, &body.1)),
                    false,
                    head_only,
                );
                return c.close();
            }
        }
    }
}

/// Whether `host` (a `Host` header) names this server: `127.0.0.1` or `localhost`, on its port.
fn host_ok(host: &str, port: u16) -> bool {
    let h = host.trim().to_ascii_lowercase();
    ["127.0.0.1", "localhost"]
        .iter()
        .any(|n| h == format!("{n}:{port}") || (port == 80 && h == *n))
}

/// Whether `origin` (an `Origin` header) is this server's own: a page anywhere else is refused.
fn origin_ok(origin: &str, port: u16) -> bool {
    let o = origin.trim().to_ascii_lowercase();
    ["127.0.0.1", "localhost"]
        .iter()
        .any(|n| o == format!("http://{n}:{port}") || (port == 80 && o == format!("http://{n}")))
}

/// The token of an `Authorization: Bearer TOKEN` header.
fn bearer(v: Option<&str>) -> Option<&str> {
    let (scheme, rest) = v?.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| rest.trim())
}

/// Compares every byte, with no early exit, so that the time taken does not say where two
/// tokens of the same length differ.
fn same_token(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .fold(0u8, |acc, (x, y)| std::hint::black_box(acc | (x ^ y)))
            == 0
}

/// Whether an `Accept` header allows JSON and an event stream. None given: both.
fn accepts(accept: Option<&str>) -> (bool, bool) {
    let Some(a) = accept.filter(|a| !a.trim().is_empty()) else {
        return (true, true);
    };
    let (mut json, mut sse) = (false, false);
    for part in a.split(',') {
        let mut it = part.split(';');
        let range = it.next().unwrap_or("").trim().to_ascii_lowercase();
        let refused = it.any(|p| {
            let p = p.trim().to_ascii_lowercase();
            p.strip_prefix("q=")
                .and_then(|q| q.parse::<f64>().ok())
                .is_some_and(|q| q <= 0.0)
        });
        if refused {
            continue;
        }
        match range.as_str() {
            "*/*" => (json, sse) = (true, true),
            "application/*" | "application/json" => json = true,
            "text/*" | "text/event-stream" => sse = true,
            _ => {}
        }
    }
    (json, sse)
}

/// Whether a `Content-Type` header says JSON.
fn is_json(ct: &str) -> bool {
    ct.split(';')
        .next()
        .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
}

fn head_too_long(sh: &Shared) -> Refusal {
    Refusal::new(
        431,
        format!("the request head is over {} KiB", sh.limits.max_head >> 10),
    )
}

fn too_large(max: usize) -> Refusal {
    Refusal::new(413, format!("the request body is over {} MiB", max >> 20))
}

/// How a request's body is sent.
enum Framing {
    None,
    Length(usize),
    Chunked,
}

fn framing(h: &Head, max: usize) -> Result<Framing, Refusal> {
    match (h.get("transfer-encoding"), h.get("content-length")) {
        (Some(_), Some(_)) => Err(Refusal::new(
            400,
            "a request may have Transfer-Encoding or Content-Length, not both",
        )),
        (Some(t), None) if t.eq_ignore_ascii_case("chunked") => Ok(Framing::Chunked),
        (Some(t), None) => Err(Refusal::new(
            501,
            format!("transfer coding \"{t}\" is not supported (chunked is)"),
        )),
        (None, Some(l)) => {
            let n = (!l.is_empty() && l.len() <= 19 && l.bytes().all(|b| b.is_ascii_digit()))
                .then(|| l.parse::<u64>().ok())
                .flatten()
                .ok_or_else(|| Refusal::new(400, "a bad Content-Length"))?;
            if n > max as u64 {
                return Err(too_large(max));
            }
            Ok(Framing::Length(n as usize))
        }
        (None, None) => Ok(Framing::None),
    }
}

fn handle(c: &mut Conn, sh: &Shared, h: &Head) -> Result<Flow, End> {
    if let Some(name) = SINGLE.iter().find(|n| h.count(n) > 1) {
        return Err(Refusal::new(400, format!("more than one {name} header")).into());
    }
    // DNS rebinding: a page elsewhere that has its own name resolve to 127.0.0.1 sends its name
    // as the Host, and its own origin.
    let host = h
        .get("host")
        .ok_or_else(|| Refusal::new(400, "a Host header is needed"))?;
    if !host_ok(host, sh.port) {
        return Err(Refusal::new(
            403,
            format!(
                "this server answers requests to 127.0.0.1:{0} or localhost:{0} only",
                sh.port
            ),
        )
        .into());
    }
    // A page's request: from this server's own origin, or from one the user allows exactly
    // (its answers then carry CORS headers); any other is refused.
    if let Some(o) = h.get("origin")
        && !origin_ok(o, sh.port)
    {
        let allowed = parse_origin(o)
            .ok()
            .is_some_and(|k| sh.origins.lock().unwrap().contains(&k));
        if !allowed {
            return Err(Refusal::new(
                403,
                format!("requests from web pages at other origins are refused (Origin: {o})"),
            )
            .into());
        }
        c.cors = Some(o.to_string());
    }
    let path = h.target.split(['?', '#']).next().unwrap_or("");
    if path != PATH {
        return Err(Refusal::new(
            404,
            format!("not found: JETCARP's MCP endpoint is {}", url(sh.port)),
        )
        .into());
    }
    // An allowed page's preflight: browsers send it without the token, and its answer says what
    // the page may send.
    if h.method == "OPTIONS" && c.cors.is_some() {
        return preflight(c, h);
    }
    let token = sh.token.lock().unwrap().clone();
    match bearer(h.get("authorization")) {
        Some(t) if same_token(t.as_bytes(), token.as_bytes()) => {}
        given => {
            let (err, msg) = if given.is_some() {
                (
                    ", error=\"invalid_token\"",
                    "the token is not this server's (it is made afresh each time the server starts)",
                )
            } else {
                (
                    "",
                    "send the token as an Authorization: Bearer TOKEN header (it is shown where the server was started)",
                )
            };
            return Err(Refusal::new(401, msg)
                .with("WWW-Authenticate", format!("Bearer realm=\"jetcarp\"{err}"))
                .into());
        }
    }
    if let Some(v) = h.get("mcp-protocol-version")
        && !protocol::supported_versions().contains(&v)
    {
        return Err(Refusal::new(
            400,
            format!(
                "MCP-Protocol-Version {v} is not supported; this server speaks {}",
                protocol::supported_versions().join(", ")
            ),
        )
        .into());
    }
    match h.method.as_str() {
        "POST" => post(c, sh, h),
        "DELETE" => delete(c, sh, h),
        m => Err(Refusal::new(
            405,
            format!(
                "{m} is not offered here: send each message as a POST, and DELETE to end a session"
            ),
        )
        .with("Allow", "POST, DELETE")
        .into()),
    }
}

/// The answer to an allowed page's CORS preflight: the methods and headers MCP's transport uses,
/// for ten minutes. Chrome asks a public page's leave to reach this computer
/// (`Access-Control-Request-Private-Network`); the answer gives it.
fn preflight(c: &mut Conn, h: &Head) -> Result<Flow, End> {
    let mut headers: Vec<(&str, String)> = vec![
        ("Access-Control-Allow-Methods", "POST, DELETE".into()),
        (
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type, Accept, Mcp-Session-Id, MCP-Protocol-Version, Last-Event-ID"
                .into(),
        ),
        ("Access-Control-Max-Age", "600".into()),
        (
            "Vary",
            "Origin, Access-Control-Request-Method, Access-Control-Request-Headers".into(),
        ),
    ];
    if h.get("access-control-request-private-network")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        headers.push(("Access-Control-Allow-Private-Network", "true".into()));
    }
    let keep = h.keep_alive()
        && matches!(
            framing(h, usize::MAX),
            Ok(Framing::None | Framing::Length(0))
        );
    c.respond(204, &headers, None, keep)?;
    Ok(if keep { Flow::KeepAlive } else { Flow::Close })
}

/// End a session.
fn delete(c: &mut Conn, sh: &Shared, h: &Head) -> Result<Flow, End> {
    let id = h.get(SESSION_HEADER).ok_or_else(|| {
        Refusal::new(400, "DELETE needs the Mcp-Session-Id of the session to end")
    })?;
    if sh.sessions.lock().unwrap().remove(id).is_none() {
        return Err(not_found_session().into());
    }
    // A body is not expected; if one was sent, the connection closes rather than read it.
    let keep = h.keep_alive()
        && matches!(
            framing(h, usize::MAX),
            Ok(Framing::None | Framing::Length(0))
        );
    c.respond(204, &[], None, keep)?;
    Ok(if keep { Flow::KeepAlive } else { Flow::Close })
}

fn not_found_session() -> Refusal {
    Refusal::new(
        404,
        "no such session: it has ended, or was never started; start a new one with initialize",
    )
    .code(-32001)
}

/// Marks a session busy while one of its requests is answered.
struct InUse<'a> {
    sh: &'a Shared,
    id: Option<String>,
}

impl Drop for InUse<'_> {
    fn drop(&mut self) {
        if let Some(id) = &self.id
            && let Some(s) = self.sh.sessions.lock().unwrap().get_mut(id)
        {
            s.busy = s.busy.saturating_sub(1);
            s.last = Instant::now();
        }
    }
}

/// A new session, its id and its cancel flags; the one unused longest is ended when there are
/// too many.
fn new_session(sh: &Shared) -> Result<(String, CancelMap), Refusal> {
    let id = link::new_token().map_err(|e| Refusal::new(500, e.to_string()))?;
    let mut all = sh.sessions.lock().unwrap();
    if all.len() >= sh.limits.max_sessions {
        let oldest = all
            .iter()
            .filter(|(_, s)| s.busy == 0)
            .min_by_key(|(_, s)| s.last)
            .map(|(k, _)| k.clone())
            .ok_or_else(|| Refusal::new(503, "too many sessions at once"))?;
        all.remove(&oldest);
    }
    let cancels = CancelMap::default();
    all.insert(
        id.clone(),
        Session {
            cancels: cancels.clone(),
            last: Instant::now(),
            busy: 1,
        },
    );
    Ok((id, cancels))
}

fn post(c: &mut Conn, sh: &Shared, h: &Head) -> Result<Flow, End> {
    if !h.get("content-type").is_some_and(is_json) {
        return Err(Refusal::new(
            415,
            "send the JSON-RPC message with Content-Type: application/json",
        )
        .into());
    }
    let (json_ok, sse_ok) = accepts(h.get("accept"));
    if !json_ok && !sse_ok {
        return Err(Refusal::new(
            406,
            "answers are application/json or text/event-stream; the Accept header allows neither",
        )
        .into());
    }
    let framing = framing(h, sh.limits.max_body)?;
    if matches!(framing, Framing::None) {
        return Err(Refusal::new(411, "a POST needs a Content-Length").into());
    }
    match h.get("expect") {
        None => {}
        Some(e) if e.eq_ignore_ascii_case("100-continue") => {
            c.s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        }
        Some(e) => {
            return Err(Refusal::new(417, format!("cannot meet Expect: {e}")).into());
        }
    }
    let until = Instant::now() + sh.limits.body_timeout;
    let body = match framing {
        Framing::Length(n) => c.take(n, until)?,
        Framing::Chunked => c.chunked(sh.limits.max_body, until)?,
        Framing::None => unreachable!(),
    };
    let msg: Value = serde_json::from_slice(&body)
        .map_err(|e| Refusal::new(400, format!("parse error: {e}")).code(protocol::PARSE_ERROR))?;
    if msg.is_array() {
        return Err(Refusal::new(
            400,
            "send one JSON-RPC message per request (MCP's HTTP transport has no batches since 2025-06-18)",
        )
        .into());
    }
    if !msg.is_object() {
        return Err(Refusal::new(400, "a JSON-RPC message must be an object").into());
    }
    let keep = h.keep_alive();
    let method = msg.get("method").and_then(Value::as_str);
    let is_response =
        method.is_none() && (msg.get("result").is_some() || msg.get("error").is_some());
    let is_request = msg.get("id").is_some() && !is_response;
    let is_init = is_request && method == Some("initialize");
    let modern = msg
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_VERSION))
        .is_some()
        || method == Some("server/discover");

    // The session the message belongs to, and its cancel flags.
    let given = h.get(SESSION_HEADER);
    let (session, cancels) = if is_init {
        let (id, cancels) = new_session(sh)?;
        (Some(id), cancels)
    } else if let Some(id) = given {
        let mut all = sh.sessions.lock().unwrap();
        let s = all.get_mut(id).ok_or_else(not_found_session)?;
        s.last = Instant::now();
        if is_request {
            s.busy += 1;
        }
        (Some(id.to_string()), s.cancels.clone())
    } else if !is_request || modern {
        (None, sh.sessionless.clone())
    } else {
        return Err(Refusal::new(
            400,
            "an Mcp-Session-Id header is needed: start a session with initialize, whose answer gives its id",
        )
        .into());
    };
    let _in_use = InUse {
        sh,
        id: session.clone().filter(|_| is_request),
    };

    if !is_request {
        // A notification or a response: nothing to answer. A cancellation acts at once, while
        // the call it names runs.
        if method == Some("notifications/cancelled") {
            protocol::cancel_request(&cancels, &msg);
            if let Some(hook) = &sh.on_cancel {
                hook(session.as_deref(), &msg);
            }
        }
        c.respond(202, &[], None, keep)?;
        return Ok(if keep { Flow::KeepAlive } else { Flow::Close });
    }

    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let wants_progress = method == Some("tools/call")
        && msg
            .get("params")
            .and_then(|p| p.get("_meta"))
            .and_then(|m| m.get("progressToken"))
            .is_some_and(|t| t.is_string() || t.is_number());
    let stream = sse_ok && (wants_progress || !json_ok);
    let (rtx, rrx) = channel();
    let (ntx, nrx) = channel();
    let drop_new = |sh: &Shared| {
        if is_init && let Some(s) = &session {
            sh.sessions.lock().unwrap().remove(s);
        }
    };
    if sh
        .tx
        .send(Pending {
            message: msg,
            session: session.clone(),
            cancels,
            reply: rtx,
            notes: ntx,
        })
        .is_err()
    {
        drop_new(sh);
        return Err(Refusal::new(503, "the server is stopping").into());
    }
    (sh.wake)();
    let extra: Vec<(&str, String)> = match (&session, is_init) {
        (Some(s), true) => vec![("Mcp-Session-Id", s.clone())],
        _ => vec![],
    };
    let t0 = Instant::now();
    let late = |id: &Value| {
        Some(protocol::error_response(
            id,
            protocol::INTERNAL_ERROR,
            "JETCARP did not answer in time",
            None,
        ))
    };
    let stopping = |id: &Value| {
        Some(protocol::error_response(
            id,
            protocol::INTERNAL_ERROR,
            "the server stopped before it answered",
            None,
        ))
    };

    if stream {
        // An event stream: the progress as it comes, then the answer, then the end.
        let mut head = format!(
            "HTTP/1.1 200 OK\r\nDate: {}\r\nContent-Type: text/event-stream\r\n\
             Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
            http_date(SystemTime::now())
        );
        for (k, v) in extra.iter().cloned().chain(c.cors_headers()) {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        // A client that has gone is not a cancellation (the transport says so): the call runs on.
        let mut alive = c.s.write_all(head.as_bytes()).is_ok();
        let mut wrote = Instant::now();
        let reply = loop {
            for n in nrx.try_iter() {
                if alive {
                    alive = c.event(&n).is_ok();
                    wrote = Instant::now();
                }
            }
            match rrx.recv_timeout(Duration::from_millis(50)) {
                Ok(r) => {
                    for n in nrx.try_iter() {
                        if alive {
                            alive = c.event(&n).is_ok();
                        }
                    }
                    break r;
                }
                Err(RecvTimeoutError::Timeout) if t0.elapsed() < sh.limits.reply_timeout => {
                    if alive && wrote.elapsed() >= sh.limits.keepalive {
                        alive = c.s.write_all(b": waiting\n\n").is_ok();
                        wrote = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Timeout) => break late(&id),
                Err(RecvTimeoutError::Disconnected) => break stopping(&id),
            }
        };
        if is_init && !reply.as_ref().is_some_and(|r| r.get("result").is_some()) {
            drop_new(sh);
        }
        if alive && let Some(r) = reply {
            let _ = c.event(&r);
        }
        return Ok(Flow::Close);
    }

    let reply = loop {
        match rrx.recv_timeout(Duration::from_millis(250)) {
            Ok(r) => break r,
            Err(RecvTimeoutError::Timeout) if t0.elapsed() < sh.limits.reply_timeout => {}
            Err(RecvTimeoutError::Timeout) => break late(&id),
            Err(RecvTimeoutError::Disconnected) => break stopping(&id),
        }
    };
    // Progress the client did not ask to stream is dropped.
    drop(nrx);
    let started = !is_init || reply.as_ref().is_some_and(|r| r.get("result").is_some());
    if !started {
        drop_new(sh);
    }
    let extra = if started { extra } else { vec![] };
    match reply {
        Some(r) => c.respond(
            200,
            &extra,
            Some(("application/json", r.to_string().as_bytes())),
            keep,
        )?,
        None => c.respond(202, &extra, None, keep)?,
    }
    Ok(if keep { Flow::KeepAlive } else { Flow::Close })
}

/// The date as HTTP gives it (IMF-fixdate): `Sun, 06 Nov 1994 08:49:37 GMT`.
fn http_date(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {d:02} {} {y} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[days.rem_euclid(7) as usize],
        MONTHS[(m - 1) as usize],
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// What `jetcarp mcp --http` was asked for.
pub struct Serve {
    /// The port (0: any free one). `None`: [`DEFAULT_PORT`], or any free port when that one is
    /// taken.
    pub port: Option<u16>,
    /// Sessions start attached to the JETCARP app, as `jetcarp mcp --app` does.
    pub app: bool,
    /// The app's port and token, when not the ones it recorded.
    pub app_port: Option<u16>,
    pub app_token: Option<String>,
    /// Print the endpoint as one line of JSON instead of text.
    pub json: bool,
    /// Keep the token for the next start (`--keep-token`), or make a new one and keep it
    /// (`--new-token`).
    pub keep_token: bool,
    pub new_token: bool,
    /// Origins whose pages may connect (`--allow-origin`).
    pub allow_origins: Vec<String>,
}

/// Set when `jetcarp mcp --http` should stop: by the first Ctrl-C, and on Linux and macOS by the
/// first SIGTERM or SIGHUP too (what `kill` sends, and a session as it ends, #278), so the server
/// stops as Ctrl-C stops it, its jobs at their next checkpoint and its endpoint file and memory
/// record removed. A second quits at once.
pub fn stop_on_signal() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let stopping = move |sig: &str| {
        s.store(true, Ordering::SeqCst);
        // Never `eprintln!`, which panics when standard error has gone (the terminal closed).
        let _ = if sig == "SIGINT" || sig == "Ctrl-C" {
            // On a line of its own, after the terminal's `^C`.
            writeln!(
                std::io::stderr(),
                "\njetcarp mcp: stopping (press Ctrl-C again to quit now)"
            )
        } else {
            writeln!(
                std::io::stderr(),
                "jetcarp mcp: {sig}: stopping (a second signal quits now)"
            )
        };
    };
    // Linux and macOS: the program's shared handling of the signals that stop it
    // (`jetcarp_core::quit_signal`, as the app and the host stop). A second ends it at once with
    // 128 + its number (130 for Ctrl-C).
    #[cfg(unix)]
    jetcarp_core::quit_signal::on_signal(stopping);
    #[cfg(windows)]
    {
        let presses = AtomicUsize::new(0);
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                stopping("Ctrl-C");
            } else {
                std::process::exit(130);
            }
        });
    }
    #[cfg(not(any(unix, windows)))]
    let _ = stopping;
    stop
}

/// `jetcarp mcp --http`: serve MCP over HTTP until `stop` is set, a [`Client`](crate::stdio::Client)
/// for each session, then stop the jobs running on the server's own threads, as the stdio
/// server does when its client goes.
pub fn serve(opts: Serve, stop: Arc<AtomicBool>) -> Result<(), String> {
    use crate::stdio::{Canceller, Client};
    let info = crate::stdio::info_from(opts.app_port, opts.app_token.clone())?;
    // Cancellations reach a session's calls in the app too: the connection that brings one
    // writes it to that session's app link while the call runs there.
    let cancellers: Arc<Mutex<HashMap<Option<String>, Canceller>>> = Default::default();
    let hook: CancelHook = {
        let cancellers = cancellers.clone();
        Arc::new(move |session: Option<&str>, msg: &Value| {
            if let Some(c) = cancellers.lock().unwrap().get(&session.map(String::from)) {
                c.cancel(msg);
            }
        })
    };
    let file = link::runtime_file(CLI_ENDPOINT);
    let allow_origins = opts
        .allow_origins
        .iter()
        .map(|o| parse_origin(o))
        .collect::<Result<Vec<_>, _>>()?;
    let options = Options {
        keep_token: opts.keep_token || opts.new_token,
        kept_file: kept_file(CLI_KEPT),
        new_token: opts.new_token,
        allow_origins,
        ..Options::default()
    };
    let start = |port: u16| {
        Listener::start_with(
            port,
            file.clone(),
            options.clone(),
            Arc::new(|| {}),
            Some(hook.clone()),
        )
    };
    let listener = match opts.port {
        Some(p) => start(p).map_err(|e| format!("could not listen on 127.0.0.1:{p}: {e}"))?,
        None => start(DEFAULT_PORT)
            .or_else(|_| start(0))
            .map_err(|e| format!("could not listen on 127.0.0.1: {e}"))?,
    };
    announce(&listener, &opts);
    let mut clients: HashMap<Option<String>, Client> = HashMap::new();
    while !stop.load(Ordering::SeqCst) {
        if let Some(p) = listener.recv_timeout(Duration::from_millis(200)) {
            let key = p.session().map(String::from);
            let c = clients.entry(key.clone()).or_insert_with(|| {
                let mut c = Client::new(opts.app, info.clone());
                c.server.cancels = p.cancels();
                c.connect_app();
                cancellers.lock().unwrap().insert(key, c.canceller());
                c
            });
            c.server.notifier = Some(p.notifier());
            let msg = p.message.clone();
            let reply = c.answer(&msg);
            c.server.notifier = None;
            p.reply(reply);
        }
        // Sessions that have ended take their worlds and app links with them.
        let open = listener.session_ids();
        clients.retain(|k, _| k.as_ref().is_none_or(|k| open.contains(k)));
        cancellers
            .lock()
            .unwrap()
            .retain(|k, _| k.as_ref().is_none_or(|k| open.contains(k)));
    }
    drop(listener);
    crate::stdio::stop_own_jobs();
    Ok(())
}

/// The startup lines: where to connect, with what, and that only this computer can.
fn announce(l: &Listener, opts: &Serve) {
    // The notes on standard error come first: whoever stops the server once it has read the line
    // saying it is ready (a client starting it, a test) has had them too.
    // Keeping the token is the user's choice; what it means is said each time it is kept.
    if let Some(p) = l.kept_file() {
        eprintln!(
            "jetcarp mcp: the token is kept in {}, in JETCARP's settings folder: it survives \
             restarts and reboots, and stays valid until a new one is made (--new-token)",
            p.display()
        );
    } else if let Some(e) = l.kept_error() {
        eprintln!("jetcarp mcp: the token could not be kept ({e}), so it is new each time");
    }
    if opts.port.is_none() && l.port != DEFAULT_PORT {
        eprintln!(
            "jetcarp mcp: port {DEFAULT_PORT} is taken, so the server is on port {} instead",
            l.port
        );
    }
    let file = l.file().map(|p| p.display().to_string());
    if opts.json {
        println!(
            "{}",
            json!({
                "transport": "streamable-http",
                "url": l.url(),
                "port": l.port,
                "bound": "127.0.0.1",
                "loopback_only": true,
                "token": l.token,
                "header": l.header(),
                "endpoint_file": file,
                "kept": l.kept(),
                "kept_file": l.kept_file().map(|p| p.display().to_string()),
                "allow_origins": l.origins(),
                "app": opts.app,
            })
        );
    } else {
        let world = if opts.app {
            "Each session acts on the world in the JETCARP app."
        } else {
            "Each session has a private world of its own."
        };
        let token = match (&file, l.kept()) {
            (_, true) => "The token is kept for the next start (--keep-token; --new-token \
                          replaces it)."
                .to_string(),
            (Some(f), false) => format!(
                "The token is new each time it starts, and is recorded in {f}, which only you can \
                 read."
            ),
            (None, false) => "The token is new each time it starts.".to_string(),
        };
        let origins = l.origins();
        let pages = if origins.is_empty() {
            String::new()
        } else {
            format!(
                "Web pages from {} may connect too, with the token.\n",
                origins.join(", ")
            )
        };
        println!(
            "JETCARP's MCP server is answering HTTP clients (MCP Streamable HTTP) on this computer only.\n  \
             URL:    {}\n  \
             Header: {}\n\
             It listens on 127.0.0.1, which other computers cannot reach. {token}\n\
             {pages}{world} Ctrl-C stops the server.",
            l.url(),
            l.header(),
        );
    }
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_imf_fixdates() {
        let t = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        assert_eq!(http_date(t(784_111_777)), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(http_date(t(0)), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(t(951_782_400)), "Tue, 29 Feb 2000 00:00:00 GMT");
        // As Python's email.utils.formatdate gives them.
        assert_eq!(http_date(t(1_795_478_399)), "Mon, 23 Nov 2026 23:59:59 GMT");
        assert_eq!(http_date(t(1_795_737_599)), "Thu, 26 Nov 2026 23:59:59 GMT");
    }

    #[test]
    fn only_this_server_s_host_and_origin_pass() {
        for h in [
            "127.0.0.1:5000",
            "localhost:5000",
            "LOCALHOST:5000",
            " 127.0.0.1:5000 ",
        ] {
            assert!(host_ok(h, 5000), "{h}");
        }
        for h in [
            "127.0.0.1:5001",
            "127.0.0.1",
            "evil.example:5000",
            "127.0.0.1.evil.example:5000",
            "[::1]:5000",
            "0.0.0.0:5000",
            "",
        ] {
            assert!(!host_ok(h, 5000), "{h}");
        }
        assert!(host_ok("localhost", 80));
        for o in ["http://127.0.0.1:5000", "http://localhost:5000"] {
            assert!(origin_ok(o, 5000), "{o}");
        }
        for o in [
            "null",
            "https://127.0.0.1:5000",
            "http://127.0.0.1:5001",
            "http://evil.example",
            "http://localhost:5000.evil.example",
            "file://",
            "",
        ] {
            assert!(!origin_ok(o, 5000), "{o}");
        }
    }

    #[test]
    fn only_exact_origins_can_be_allowed() {
        for (given, want) in [
            ("https://client.example", "https://client.example"),
            ("HTTPS://Client.Example:443", "https://client.example"),
            (" http://localhost:6274 ", "http://localhost:6274"),
            ("http://localhost:80", "http://localhost"),
            ("https://client.example:8443", "https://client.example:8443"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("http://192.168.1.5:8080", "http://192.168.1.5:8080"),
        ] {
            assert_eq!(parse_origin(given).as_deref(), Ok(want), "{given}");
        }
        for bad in [
            "*",
            "null",
            "NULL",
            "https://*.example.com",
            "*.example.com",
            "https://client.*",
            "https://client.example/",
            "https://client.example/mcp",
            "client.example",
            "ftp://client.example",
            "file://",
            "https://user@client.example",
            "https://client.example:0",
            "https://client.example:99999",
            "https://client.example:+80",
            "https://client..example",
            "https://-client.example",
            "https://client.example?x=1",
            "https://client.example#x",
            "",
            "https://",
            "http://[::1",
            "https://cli\u{e9}nt.example",
        ] {
            let e = parse_origin(bad).unwrap_err();
            assert!(e.contains("not an origin"), "{bad}: {e}");
        }
    }

    #[test]
    fn bearer_tokens_are_read_and_compared_whole() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer")), None);
        assert_eq!(bearer(None), None);
        assert!(same_token(b"0123456789abcdef", b"0123456789abcdef"));
        assert!(!same_token(b"0123456789abcdef", b"0123456789abcdee"));
        assert!(!same_token(b"0123456789abcdef", b"1123456789abcdef"));
        assert!(!same_token(b"0123456789abcdef", b"0123456789abcde"));
        assert!(!same_token(b"", b"x"));
    }

    #[test]
    fn accept_headers_are_read() {
        assert_eq!(accepts(None), (true, true));
        assert_eq!(accepts(Some("")), (true, true));
        assert_eq!(accepts(Some("*/*")), (true, true));
        assert_eq!(
            accepts(Some("application/json, text/event-stream")),
            (true, true)
        );
        assert_eq!(accepts(Some("application/json")), (true, false));
        assert_eq!(accepts(Some("text/event-stream")), (false, true));
        assert_eq!(accepts(Some("text/html")), (false, false));
        assert_eq!(
            accepts(Some("application/json;q=0, text/event-stream")),
            (false, true)
        );
        assert!(is_json("application/json"));
        assert!(is_json("Application/JSON; charset=utf-8"));
        assert!(!is_json("text/plain"));
        assert!(!is_json("application/x-www-form-urlencoded"));
    }
}
