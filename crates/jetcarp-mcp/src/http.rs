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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
pub const APP_ENDPOINT: &str = "app-http-endpoint.json";

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
    token: String,
    limits: Limits,
    stop: AtomicBool,
    connections: AtomicUsize,
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
}

impl Listener {
    /// Listen on `127.0.0.1:port` (0: any free port) with the default [`Limits`], recording the
    /// URL and token in the file at `file`. `wake` is called after each request is queued.
    pub fn start(
        port: u16,
        file: Option<PathBuf>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Listener> {
        Listener::start_with(port, file, Limits::default(), wake, None)
    }

    /// [`start`](Self::start) with these limits, and `on_cancel` called for each cancellation.
    pub fn start_with(
        port: u16,
        file: Option<PathBuf>,
        limits: Limits,
        wake: Arc<dyn Fn() + Send + Sync>,
        on_cancel: Option<CancelHook>,
    ) -> std::io::Result<Listener> {
        let sock = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        sock.set_nonblocking(true)?;
        let port = sock.local_addr()?.port();
        let token = link::new_token()?;
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            port,
            token: token.clone(),
            limits,
            stop: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
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
        };
        l.wrote_file = l.write_file().is_ok();
        Ok(l)
    }

    fn write_file(&self) -> std::io::Result<()> {
        let p =
            (self.file.as_ref()).ok_or_else(|| std::io::Error::other("no runtime directory"))?;
        let text = json!({"url": self.url(), "port": self.port, "token": self.token,
                          "pid": std::process::id()})
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
        if self.wrote_file
            && let Some(p) = &self.file
            && let Ok(info) = link::EndpointInfo::read_at(p, "")
            && info.token == self.token
        {
            // Only if it is still this listener's.
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Counts one open connection until dropped.
struct Counted(Arc<Shared>);

impl Counted {
    fn new(sh: &Arc<Shared>) -> Counted {
        sh.connections.fetch_add(1, Ordering::SeqCst);
        Counted(sh.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
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
                let counted = Counted::new(&sh);
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
}

impl Conn {
    /// Read more into the buffer, by `deadline`. `Ok(0)`: the client closed.
    fn fill(&mut self, deadline: Instant, stop: &AtomicBool) -> std::io::Result<usize> {
        let mut chunk = [0u8; 16 << 10];
        loop {
            if stop.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(ErrorKind::Interrupted, "stopping"));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::Error::new(ErrorKind::TimedOut, "timed out"));
            }
            // A slice at a time, so that a stop is seen soon.
            self.s
                .set_read_timeout(Some(left.min(Duration::from_millis(250))))?;
            match self.s.read(&mut chunk) {
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(n);
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
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
            match self.fill(head_until.unwrap_or(idle_until), &sh.stop) {
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
    fn take(&mut self, n: usize, deadline: Instant, stop: &AtomicBool) -> Result<Vec<u8>, End> {
        while self.buf.len() < n {
            match self.fill(deadline, stop) {
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
    fn line(&mut self, max: usize, deadline: Instant, stop: &AtomicBool) -> Result<Vec<u8>, End> {
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
            self.take_more(deadline, stop)?;
        }
    }

    fn take_more(&mut self, deadline: Instant, stop: &AtomicBool) -> Result<(), End> {
        match self.fill(deadline, stop) {
            Ok(0) => Err(End::Quiet),
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                Err(Refusal::new(408, "the request body did not arrive in time").into())
            }
            Err(_) => Err(End::Quiet),
        }
    }

    /// A body sent in chunks, of at most `max` bytes in all.
    fn chunked(
        &mut self,
        max: usize,
        deadline: Instant,
        stop: &AtomicBool,
    ) -> Result<Vec<u8>, End> {
        let mut body = Vec::new();
        loop {
            let line = self.line(1024, deadline, stop)?;
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
                    let l = self.line(1024, deadline, stop)?;
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
            body.extend(self.take(size, deadline, stop)?);
            if !self.line(0, deadline, stop)?.is_empty() {
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
        respond(&mut self.s, status, headers, body, keep, false)?;
        Ok(())
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
    let mut c = Conn { s, buf: Vec::new() };
    loop {
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
                let _ = respond(
                    &mut c.s,
                    r.status,
                    &r.headers,
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
    if let Some(o) = h.get("origin")
        && !origin_ok(o, sh.port)
    {
        return Err(Refusal::new(
            403,
            format!("requests from web pages at other origins are refused (Origin: {o})"),
        )
        .into());
    }
    let path = h.target.split(['?', '#']).next().unwrap_or("");
    if path != PATH {
        return Err(Refusal::new(
            404,
            format!("not found: JETCARP's MCP endpoint is {}", url(sh.port)),
        )
        .into());
    }
    match bearer(h.get("authorization")) {
        Some(t) if same_token(t.as_bytes(), sh.token.as_bytes()) => {}
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
        Framing::Length(n) => c.take(n, until, &sh.stop)?,
        Framing::Chunked => c.chunked(sh.limits.max_body, until, &sh.stop)?,
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
        for (k, v) in &extra {
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
}

/// Set (by the first Ctrl-C) when `jetcarp mcp --http` should stop; a second Ctrl-C quits at
/// once.
pub fn stop_on_ctrl_c() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let presses = AtomicUsize::new(0);
    let _ = ctrlc::set_handler(move || {
        if presses.fetch_add(1, Ordering::SeqCst) == 0 {
            s.store(true, Ordering::SeqCst);
            eprintln!("\njetcarp mcp: stopping (press Ctrl-C again to quit now)");
        } else {
            std::process::exit(130);
        }
    });
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
    let start = |port: u16| {
        Listener::start_with(
            port,
            file.clone(),
            Limits::default(),
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
                "app": opts.app,
            })
        );
    } else {
        let world = if opts.app {
            "Each session acts on the world in the JETCARP app."
        } else {
            "Each session has a private world of its own."
        };
        println!(
            "JETCARP's MCP server is answering HTTP clients (MCP Streamable HTTP) on this computer only.\n  \
             URL:    {}\n  \
             Header: {}\n\
             It listens on 127.0.0.1, which other computers cannot reach. The token is new each time it starts{}.\n\
             {world} Ctrl-C stops the server.",
            l.url(),
            l.header(),
            match &file {
                Some(f) => format!(", and is kept in {f}, which only you can read"),
                None => String::new(),
            }
        );
    }
    if opts.port.is_none() && l.port != DEFAULT_PORT {
        eprintln!(
            "jetcarp mcp: port {DEFAULT_PORT} is taken, so the server is on port {} instead",
            l.port
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
