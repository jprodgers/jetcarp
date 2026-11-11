//! JETCARP's scripting engine. Scripts are [Rhai](https://rhai.rs); the simulation stays in Rust
//! and Rhai orchestrates: make worlds, run them until something happens, save pictures, loop
//! over rules and tilings, and run large parallel rule sweeps into output folders.
//!
//! Hosts (the CLI's `jetcarp script`, the app's script editor, the MCP `run_script` tool) use:
//!
//! - [`check`]: syntax check with line and column;
//! - [`run`]: run a script on this thread, with an event callback and a cancel flag;
//! - [`spawn`]: run it on a background thread and poll a [`Job`] for log lines and progress;
//! - [`help_text`]: the function reference;
//! - [`sweep_to_script`]: a sweep built from a form, as an editable script;
//! - [`estimate`]: time a few runs of a sweep before starting it;
//! - [`run_world`]: rebuild one run of a finished sweep (to open it in a live view).
//!
//! Scripts write only inside the output root (`<Pictures>/JETCARP`, `$JETCARP_OUT`, or
//! [`RunOptions::out_root`]) and folders in [`RunOptions::allow_dirs`].

mod ants;
mod api;
pub mod describe;
mod hl;
pub mod jobs;
pub mod library;
pub mod media;
mod pile;
pub mod results;
mod search;
pub mod series;
pub mod sim;
pub mod sweep;

/// The argument parsers scripts use, for crates that run world-only scripts (jetcarp-display)
/// without depending on jetcarp-api themselves.
pub use jetcarp_api::args;
pub use jetcarp_api::args::Screen;
pub use jetcarp_api::sweeps::run_world;
pub use sweep::{Estimate, estimate, sweep_to_script};

use jetcarp_api::out::{self, Guard};
use rhai::{AST, Dynamic, EvalAltResult, Position, Scope};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// How a script runs.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// `--set k=v` values, seen by the script as the `params` map (numbers and true/false are
    /// converted; everything else is text).
    pub params: BTreeMap<String, String>,
    /// The screen `size: "screen"` fills and images default to.
    pub screen: Screen,
    /// Worker threads for the whole script (default: all cores). Results do not depend on it.
    pub threads: Option<usize>,
    /// Output root instead of `$JETCARP_OUT` or `<Pictures>/JETCARP`.
    pub out_root: Option<PathBuf>,
    /// Further folders scripts may write into.
    pub allow_dirs: Vec<PathBuf>,
    /// The script file, recorded in `run.json`; its folder is readable.
    pub script_path: Option<PathBuf>,
    pub limits: Limits,
    /// The host has a live view: `show(w)` sends a copy of the world as [`Event::Show`].
    /// Otherwise `show` only logs a line.
    pub live_view: bool,
    /// The pattern catalog `library()` opens (default: `JETCARP_LIBRARY`, else
    /// `<data>/catalog.sqlite`; a folder means `<folder>/catalog.sqlite`).
    pub library: Option<PathBuf>,
}

/// Bounds for untrusted or interactive runs (the MCP tool sets these).
#[derive(Clone, Debug)]
pub struct Limits {
    /// Stop the script after this many seconds (it ends as cancelled, with a timeout note).
    pub max_seconds: Option<f64>,
    /// Largest world (cells) a script may build.
    pub max_cells: Option<u64>,
    /// Keep at most this many log lines in the summary (the rest are counted).
    pub max_log_lines: usize,
    /// Rhai operation limit (a runaway loop guard), if any.
    pub max_operations: Option<u64>,
    /// Reads only from the output root and allowed folders (otherwise anywhere).
    pub restrict_reads: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_seconds: None,
            max_cells: None,
            max_log_lines: 100_000,
            max_operations: None,
            restrict_reads: false,
        }
    }
}

/// Something a running script reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A line from `print`, `log`, `debug`, or the engine (sweep start and end).
    Log(String),
    Progress(Progress),
    /// `show(w)`: a world for the host's live view (only when [`RunOptions::live_view`] is set).
    Show(Shown),
}

/// A world a script asked the host to show. The world is a copy, taken once with
/// [`Shown::take_world`].
#[derive(Clone)]
pub struct Shown {
    pub label: String,
    pub generation: u64,
    pub hash: String,
    world: Arc<Mutex<Option<jetcarp_core::World>>>,
}

impl Shown {
    pub fn new(label: String, world: jetcarp_core::World) -> Shown {
        Shown {
            label,
            generation: world.generation,
            hash: world.hash_hex(),
            world: Arc::new(Mutex::new(Some(world))),
        }
    }
    /// The world (`None` if it was taken already).
    pub fn take_world(&self) -> Option<jetcarp_core::World> {
        self.world.lock().ok()?.take()
    }
}

impl PartialEq for Shown {
    fn eq(&self, o: &Shown) -> bool {
        self.label == o.label && self.generation == o.generation && self.hash == o.hash
    }
}

impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Shown({:?}, generation {}, hash {})",
            self.label, self.generation, self.hash
        )
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Progress {
    /// What is progressing, e.g. `sweep HexW3`.
    pub label: String,
    pub done: u64,
    pub total: u64,
    /// The current item, e.g. the rule just finished.
    pub item: String,
    /// Seconds since this task started.
    pub elapsed: f64,
    /// Estimated seconds left.
    pub eta: Option<f64>,
}

impl Progress {
    /// `sweep HexW3: 1200/19683 (6.1%), 14.2s, ~3m 40s left, HexW3 1199`.
    pub fn line(&self) -> String {
        let pct = if self.total > 0 {
            format!(" ({:.1}%)", self.done as f64 * 100.0 / self.total as f64)
        } else {
            String::new()
        };
        let eta = self
            .eta
            .map(|e| format!(", ~{} left", duration(e)))
            .unwrap_or_default();
        let item = if self.item.is_empty() {
            String::new()
        } else {
            format!(", {}", self.item)
        };
        format!(
            "{}: {}/{}{pct}, {}{eta}{item}",
            self.label,
            self.done,
            self.total,
            duration(self.elapsed)
        )
    }
}

/// `3m 40s`, `1h 02m`, `12.3s`.
pub fn duration(s: f64) -> String {
    if s < 60.0 {
        format!("{s:.1}s")
    } else if s < 3600.0 {
        format!("{}m {:02}s", (s / 60.0) as u64, (s % 60.0) as u64)
    } else {
        format!(
            "{}h {:02}m",
            (s / 3600.0) as u64,
            ((s % 3600.0) / 60.0) as u64
        )
    }
}

/// A script error with its place in the source (1-based line and column), when known.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ScriptError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "line {l}, column {c}: {}", self.message),
            (Some(l), None) => write!(f, "line {l}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

impl ScriptError {
    fn at(message: String, pos: Position) -> ScriptError {
        ScriptError {
            message,
            line: pos.line(),
            column: pos.position(),
        }
    }
    /// Undo the column shift of [`preprocess`] (each quoted `until` adds two characters).
    fn unshift(mut self, quoted: &[(usize, usize)]) -> ScriptError {
        if let (Some(l), Some(c)) = (self.line, self.column) {
            let k = quoted.iter().filter(|&&(ql, qc)| ql == l && qc < c).count();
            self.column = Some(c.saturating_sub(2 * k).max(1));
        }
        self
    }
}

/// `until` is a Rhai keyword (`do { } until x`), but scripts use it as a map key:
/// `#{ until: "done" }`. Quote `until` wherever it is directly followed by `:` (not `::`),
/// outside strings and comments. Returns the new source and the (line, column) of each change.
pub fn preprocess(src: &str) -> (String, Vec<(usize, usize)>) {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut quoted = vec![];
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    let mut depth = 0usize; // block comment nesting
    let mut in_line_comment = false;
    let mut quote: Option<char> = None;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        let mut take = 1;
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
        } else if depth > 0 {
            if c == '*' && next == Some('/') {
                depth -= 1;
                take = 2;
            } else if c == '/' && next == Some('*') {
                depth += 1;
                take = 2;
            }
        } else if let Some(q) = quote {
            if c == '\\' {
                take = 2;
            } else if c == q {
                quote = None;
            }
        } else if c == '/' && next == Some('/') {
            in_line_comment = true;
        } else if c == '/' && next == Some('*') {
            depth = 1;
            take = 2;
        } else if c == '"' || c == '`' || c == '\'' {
            quote = Some(c);
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            let mut j = i;
            while j < b.len() && (b[j].is_alphanumeric() || b[j] == '_') {
                j += 1;
            }
            let word: String = b[start..j].iter().collect();
            let mut k = j;
            while k < b.len() && (b[k] == ' ' || b[k] == '\t') {
                k += 1;
            }
            let prev_dot = start > 0 && b[start - 1] == '.';
            if word == "until" && !prev_dot && b.get(k) == Some(&':') && b.get(k + 1) != Some(&':')
            {
                out.push_str("\"until\"");
                quoted.push((line, col));
            } else {
                out.push_str(&word);
            }
            col += j - start;
            i = j;
            continue;
        }
        for &ch in &b[i..(i + take).min(b.len())] {
            out.push(ch);
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        i += take;
    }
    (out, quoted)
}

/// The result of a run.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Summary {
    pub ok: bool,
    pub error: Option<ScriptError>,
    /// Stopped by the cancel flag or the time limit.
    pub cancelled: bool,
    pub timed_out: bool,
    pub log: Vec<String>,
    /// Log lines beyond [`Limits::max_log_lines`].
    pub dropped_log_lines: u64,
    /// Output folders made by `out_dir` and `sweep`.
    pub output_dirs: Vec<PathBuf>,
    pub files_written: u64,
    pub seconds: f64,
    /// The script's final value as text (empty for `()`).
    pub result: String,
    /// Each sweep's summary.
    pub sweeps: Vec<Value>,
}

impl Summary {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Check a script for syntax errors without running it.
pub fn check(source: &str) -> Result<(), ScriptError> {
    let engine = api::bare_engine();
    let (src, quoted) = preprocess(source);
    engine.compile(src).map(|_| ()).map_err(|e| {
        ScriptError::at(with_hint(e.err_type().to_string()), e.position()).unshift(&quoted)
    })
}

/// A syntax error message with a hint for habits of other languages.
fn with_hint(msg: String) -> String {
    if msg.contains("'null' is a reserved keyword") || msg.contains("'None' is a reserved") {
        format!("{msg}. Rhai has no null: JSON null reads as (), so test `x == ()`")
    } else {
        msg
    }
}

/// Run a script on this thread. `on_event` receives log lines and progress as they happen
/// (from worker threads too); setting `cancel` stops the script at the next check.
pub fn run(
    source: &str,
    opts: RunOptions,
    on_event: impl Fn(Event) + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Summary {
    let ctx = Arc::new(Ctx::new(source, opts, Arc::new(on_event), cancel));
    let engine = api::engine(ctx.clone());
    let finish = |ctx: &Ctx, error: Option<ScriptError>, result: String| -> Summary {
        let (log, dropped) = {
            let l = ctx.log.lock().unwrap();
            (l.0.clone(), l.1)
        };
        let cancelled = ctx.cancel.load(Ordering::Relaxed) || ctx.timed_out.load(Ordering::Relaxed);
        let s = Summary {
            ok: error.is_none(),
            error,
            cancelled,
            timed_out: ctx.timed_out.load(Ordering::Relaxed),
            log,
            dropped_log_lines: dropped,
            output_dirs: ctx.out_dirs.lock().unwrap().clone(),
            files_written: ctx.files_written.load(Ordering::Relaxed),
            seconds: ctx.start.elapsed().as_secs_f64(),
            result,
            sweeps: ctx.sweeps.lock().unwrap().clone(),
        };
        ctx.write_run_records(&s);
        s
    };
    let (src, quoted) = preprocess(source);
    let ast = match engine.compile(src) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            let err =
                ScriptError::at(with_hint(e.err_type().to_string()), e.position()).unshift(&quoted);
            ctx.log(format!("error: {err}"));
            return finish(&ctx, Some(err), String::new());
        }
    };
    *ctx.ast.lock().unwrap() = Some(ast.clone());
    let mut scope = Scope::new();
    scope.push_constant("params", api::params_map(&ctx.opts.params));
    let mut eval = || engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast);
    let result = match ctx.opts.threads {
        Some(t) => match rayon::ThreadPoolBuilder::new()
            .num_threads(t.max(1))
            .build()
        {
            Ok(pool) => pool.install(eval),
            Err(e) => Err(format!("thread pool: {e}").into()),
        },
        None => eval(),
    };
    *ctx.ast.lock().unwrap() = None;
    match result {
        Ok(v) => {
            let text = if v.is_unit() {
                String::new()
            } else {
                v.to_string()
            };
            finish(&ctx, None, text)
        }
        Err(e) => {
            let err = script_error(&ctx, *e).unshift(&quoted);
            ctx.log(format!("error: {err}"));
            finish(&ctx, Some(err), String::new())
        }
    }
}

fn script_error(ctx: &Ctx, mut e: EvalAltResult) -> ScriptError {
    if ctx.timed_out.load(Ordering::Relaxed) {
        let limit = ctx.opts.limits.max_seconds.unwrap_or(0.0);
        return ScriptError::at(
            format!("stopped: time limit of {limit} s reached"),
            e.position(),
        );
    }
    if ctx.cancel.load(Ordering::Relaxed) {
        return ScriptError::at("cancelled".into(), e.position());
    }
    let pos = e.take_position();
    // Errors inside closures carry the position of the call; report the inner place.
    match e {
        EvalAltResult::ErrorInFunctionCall(name, _, inner, _) if name.starts_with("anon$") => {
            let mut inner = *inner;
            let p = inner.take_position();
            ScriptError::at(inner.to_string(), if p.is_none() { pos } else { p })
        }
        EvalAltResult::ErrorRuntime(v, _) => ScriptError::at(v.to_string(), pos),
        e => {
            let msg = e.to_string();
            let msg = match missing_function_hint(&msg) {
                Some(h) => format!("{msg}. {h}"),
                None => msg,
            };
            ScriptError::at(msg, pos)
        }
    }
}

/// A hint for "Function not found" errors from habits of other languages (Python, JavaScript),
/// which language models bring to Rhai.
fn missing_function_hint(msg: &str) -> Option<&'static str> {
    let name = msg.strip_prefix("Function not found: ")?;
    let name = name.split([' ', '(']).next()?.trim_start_matches('.');
    Some(match name {
        "type" | "typeof" => "Rhai: type_of(x) gives the type name",
        "len" | "length" | "count" | "size" => "Rhai: x.len() for strings, lists, and maps",
        "str" | "String" | "toString" => "Rhai: x.to_string(), or `${x}` in a back-quoted string",
        "int" | "parseInt" => "Rhai: x.to_int() or parse_int(s)",
        "float" | "parseFloat" => "Rhai: x.to_float() or parse_float(s)",
        "range" => "Rhai: for i in 0..n { }",
        "append" => "Rhai: list.push(x)",
        "keys" | "values" => "Rhai: map.keys() and map.values()",
        "sorted" | "sort" => {
            "Rhai: list.sort() sorts in place; list.sort(|a, b| ...) with a comparer returning an int"
        }
        "items" | "entries" => "Rhai: for k in map.keys() { let v = map[k]; }",
        "enumerate" => "Rhai: for (x, i) in list { } gives each item and its index",
        "sum" | "max" | "min" if msg.contains("array") => {
            "Rhai: loop over the list, e.g. let m = 0; for x in list { if x > m { m = x; } }"
        }
        "push" | "contains" | "filter" | "map" | "reduce" => {
            "check the argument types: list methods are called as list.push(x), list.filter(|x| ...), list.contains(x)"
        }
        _ => {
            "call script_help (MCP) or `jetcarp script --functions` for JETCARP's functions; Rhai's own are listed at rhai.rs/book"
        }
    })
}

/// A script running on a background thread.
pub struct Job {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Event>,
    handle: Option<std::thread::JoinHandle<Summary>>,
}

impl Job {
    /// Ask the script to stop (it stops at its next check, within a fraction of a second).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
    /// Events since the last call (never blocks).
    pub fn events(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
    /// Wait for the end and return the summary.
    pub fn wait(mut self) -> Summary {
        match self.handle.take().map(|h| h.join()) {
            Some(Ok(s)) => s,
            _ => Summary {
                ok: false,
                error: Some(ScriptError {
                    message: "the script thread panicked".into(),
                    line: None,
                    column: None,
                }),
                cancelled: false,
                timed_out: false,
                log: vec![],
                dropped_log_lines: 0,
                output_dirs: vec![],
                files_written: 0,
                seconds: 0.0,
                result: String::new(),
                sweeps: vec![],
            },
        }
    }
}

/// Run a script on a background thread. Poll [`Job::events`] for log lines and progress.
pub fn spawn(source: String, opts: RunOptions) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let c = cancel.clone();
    let handle = std::thread::Builder::new()
        .name("jetcarp-script".into())
        .spawn(move || {
            run(
                &source,
                opts,
                move |e| {
                    let _ = tx.lock().unwrap().send(e);
                },
                c,
            )
        })
        .expect("spawn script thread");
    Job {
        cancel,
        rx,
        handle: Some(handle),
    }
}

/// The function reference (also shown by `jetcarp script --functions` and the MCP tool).
pub fn help_text() -> &'static str {
    include_str!("help.md")
}

// ---------------------------------------------------------------------------------------------
// Shared state of one run

type Sink = Arc<dyn Fn(Event) + Send + Sync>;

pub(crate) struct Ctx {
    pub opts: RunOptions,
    pub source: String,
    pub root: PathBuf,
    sink: Sink,
    cancel: Arc<AtomicBool>,
    timed_out: AtomicBool,
    start: Instant,
    started_wall: String,
    guard: Mutex<Guard>,
    out_dirs: Mutex<Vec<PathBuf>>,
    log: Mutex<(Vec<String>, u64)>,
    pub files_written: AtomicU64,
    sweeps: Mutex<Vec<Value>>,
    pub ast: Mutex<Option<Arc<AST>>>,
    /// The world `jetcarp(op, #{...})` acts on for world operations (made on first use).
    pub op_world: Mutex<Option<jetcarp_api::world::Session>>,
}

impl Ctx {
    fn new(source: &str, opts: RunOptions, sink: Sink, cancel: Arc<AtomicBool>) -> Ctx {
        let root = out::out_root(opts.out_root.as_deref());
        let mut roots = vec![root.clone()];
        roots.extend(opts.allow_dirs.iter().map(|d| out::absolute(d)));
        Ctx {
            source: source.to_string(),
            root,
            sink,
            cancel,
            timed_out: AtomicBool::new(false),
            start: Instant::now(),
            started_wall: out::now_rfc3339(),
            guard: Mutex::new(Guard { roots }),
            out_dirs: Mutex::new(vec![]),
            log: Mutex::new((vec![], 0)),
            files_written: AtomicU64::new(0),
            sweeps: Mutex::new(vec![]),
            ast: Mutex::new(None),
            op_world: Mutex::new(None),
            opts,
        }
    }

    /// The context of the operations a script calls: this run's file rules (writes inside the
    /// output root and the allowed folders, relative paths from the newest output folder, reads
    /// restricted when the run says so), its output root, its world-size limit, and the catalog
    /// `lib` (default: the run's `library` option, else the default catalog).
    pub fn api(&self, lib: Option<jetcarp_store::Library>) -> jetcarp_api::Ctx {
        let roots = self.guard.lock().unwrap().roots.clone();
        let mut limits = jetcarp_api::Limits::NONE;
        if let Some(m) = self.opts.limits.max_cells {
            limits.max_cells = m;
        }
        let cx = jetcarp_api::Ctx::new()
            .with_limits(limits)
            .with_fs(jetcarp_api::FsPolicy::guarded(
                self.base(),
                roots,
                self.opts.limits.restrict_reads,
            ))
            .with_out_root(Some(self.root.clone()));
        match lib {
            Some(l) => cx.with_store(l),
            None => cx.with_library(self.opts.library.clone()),
        }
    }

    pub fn log(&self, line: String) {
        {
            let mut l = self.log.lock().unwrap();
            if l.0.len() < self.opts.limits.max_log_lines {
                l.0.push(line.clone());
            } else {
                l.1 += 1;
            }
        }
        (self.sink)(Event::Log(line));
    }

    pub fn progress(&self, p: Progress) {
        (self.sink)(Event::Progress(p));
    }

    pub fn show(&self, s: Shown) {
        (self.sink)(Event::Show(s));
    }

    /// Cancelled, or past the time limit.
    pub fn stopped(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) || self.timed_out.load(Ordering::Relaxed) {
            return true;
        }
        if let Some(s) = self.opts.limits.max_seconds
            && self.start.elapsed().as_secs_f64() > s
        {
            self.timed_out.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub fn check_tiling(&self, t: &jetcarp_core::TilingSpec) -> Result<(), String> {
        if let Some(max) = self.opts.limits.max_cells {
            let n = match t.size {
                Some(jetcarp_core::SizeSpec::Rect { w, h }) => w as u64 * h as u64,
                Some(jetcarp_core::SizeSpec::Cells(c)) => c as u64,
                _ => 0,
            };
            if n > max {
                return Err(format!("{n} cells is over this run's limit of {max}"));
            }
        }
        Ok(())
    }

    pub fn check_cells(&self, n: usize) -> Result<(), String> {
        match self.opts.limits.max_cells {
            Some(max) if n as u64 > max => {
                Err(format!("{n} cells is over this run's limit of {max}"))
            }
            _ => Ok(()),
        }
    }

    /// A new timestamped output folder under the root.
    pub fn out_dir(&self, label: &str) -> Result<PathBuf, String> {
        let dir = out::make_out_dir(&self.root, label)?;
        self.add_out_dir(&dir);
        Ok(dir)
    }

    pub fn add_out_dir(&self, dir: &Path) {
        let mut d = self.out_dirs.lock().unwrap();
        if !d.iter().any(|x| x == dir) {
            d.push(dir.to_path_buf());
        }
    }

    /// Relative paths are relative to the newest output folder (else the root).
    fn base(&self) -> PathBuf {
        self.out_dirs
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_else(|| self.root.clone())
    }

    /// A file path a script may write (parent folders are created).
    pub fn writable(&self, path: &str) -> Result<PathBuf, String> {
        self.guard.lock().unwrap().writable(&self.base(), path)
    }

    /// A folder a script may write into (created).
    pub fn writable_dir(&self, path: &str) -> Result<PathBuf, String> {
        let p = self.writable(&format!("{}/.keep", path.trim_end_matches(['/', '\\'])))?;
        let dir = p.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }

    /// A file path a script may read.
    pub fn readable(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let p = if p.is_absolute() {
            out::absolute(p)
        } else {
            let base = self
                .opts
                .script_path
                .as_ref()
                .and_then(|s| s.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let first = out::absolute(&base.join(p));
            if first.exists() {
                first
            } else {
                out::absolute(&self.base().join(p))
            }
        };
        if self.opts.limits.restrict_reads && !self.guard.lock().unwrap().allows(&p) {
            return Err(format!(
                "this run may only read inside the output folders, not {}",
                p.display()
            ));
        }
        Ok(p)
    }

    pub fn params_json(&self) -> Value {
        Value::Object(
            self.opts
                .params
                .iter()
                .map(|(k, v)| (k.clone(), api::param_value(v)))
                .collect(),
        )
    }

    pub fn record_sweep(&self, v: Value) {
        self.sweeps.lock().unwrap().push(v);
    }

    /// `run.json` in every output folder that has none (sweeps write their own).
    fn write_run_records(&self, s: &Summary) {
        for dir in self.out_dirs.lock().unwrap().iter() {
            let p = dir.join("run.json");
            if p.exists() {
                continue;
            }
            let v = json!({
                "jetcarp_version": jetcarp_core::VERSION,
                "kind": "script",
                "started": self.started_wall,
                "finished": out::now_rfc3339(),
                "seconds": (s.seconds * 1000.0).round() / 1000.0,
                "ok": s.ok,
                "error": s.error,
                "cancelled": s.cancelled,
                "os": std::env::consts::OS,
                "threads": self.opts.threads.unwrap_or_else(rayon::current_num_threads),
                "screen": self.opts.screen,
                "params": self.params_json(),
                "script_path": self.opts.script_path.as_ref().map(|p| p.display().to_string()),
                "script": self.source,
            });
            let _ = std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }
}

pub(crate) fn unix_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn until_keys_are_quoted() {
        let src = "let a = #{ until: \"done\", n: 1 }; // until: x\nlet s = \"until: y\";\ndo { x += 1 } until x > 3;\nlet m = #{until:1};";
        let (out, q) = preprocess(src);
        assert!(out.contains("#{ \"until\": \"done\""));
        assert!(out.contains("// until: x"));
        assert!(out.contains("\"until: y\""));
        assert!(out.contains("} until x > 3"));
        assert!(out.contains("#{\"until\":1}"));
        assert_eq!(q, vec![(1, 12), (4, 11)]);
    }
}

/// Example scripts shipped with JETCARP (the files in this crate's `scripts/` folder), as
/// (file name, source). The app lists them in its Scripts window.
pub const EXAMPLES: &[(&str, &str)] = &[
    (
        "hexw3-atlas.rhai",
        include_str!("../scripts/hexw3-atlas.rhai"),
    ),
    ("hexw-all.rhai", include_str!("../scripts/hexw-all.rhai")),
    (
        "wolfram-all.rhai",
        include_str!("../scripts/wolfram-all.rhai"),
    ),
    ("soup-hunt.rhai", include_str!("../scripts/soup-hunt.rhai")),
    (
        "catalog-hunt.rhai",
        include_str!("../scripts/catalog-hunt.rhai"),
    ),
    (
        "script-five.rhai",
        include_str!("../scripts/script-five.rhai"),
    ),
];
