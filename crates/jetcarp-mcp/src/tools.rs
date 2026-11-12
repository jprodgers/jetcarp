//! The MCP tools: the registry, what a tool returns, and what every tool runs with.
//!
//! Eighteen tools are shared by every transport (the stdio server and the app's link); the app
//! adds `app_display`, and the stdio server adds `use_app` and `host_control`. Each tool calls
//! one operation of `jetcarp_api::ops`, or a few closely related ones, and its input schema is
//! put together from those operations' input types ([`crate::schema`]).
//!
//! | Tool | Operations |
//! | --- | --- |
//! | `help` | (the guides, and the book's pages) |
//! | `reference` | `reference.rules`, `.tilings`, `.isohedral`, `.palettes`, `.seeds` |
//! | `world_new` | `world.new`, `world.open` |
//! | `world_status` | `world.status`, `world.save` |
//! | `world_read` | `world.read`, `world.inspect`, `world.objects` |
//! | `world_edit` | `world.draw`, `world.paste`, `world.reseed`, `world.apply` |
//! | `world_step` | `world.step` |
//! | `world_render` | `world.render`, `wallpaper.export` |
//! | `run_script` | (the Rhai engine) |
//! | `library_query` | `library.info`, `library.query`, `library.get`, `quickstart.list`, `quickstart.get` |
//! | `library_curate` | `library.update`, `library.remove`, `library.compounds`, `quickstart.add`, `quickstart.update`, `quickstart.remove` |
//! | `search_start` | `search.start` |
//! | `job_status` | `jobs.status`, `jobs.list`, `jobs.report` |
//! | `job_stop` | `jobs.stop` |
//! | `search_runs` | `search.runs`, `search.sheet`, `search.run_scene`, `search.try`, `search.review` |
//! | `media_render` | `media.render` |
//! | `pack` | `pack.info`, `pack.check`, `pack.import`, `pack.publish`, `pack.export`, `site.export` |
//! | `files_list` | `files.list` |
//!
//! Descriptions are written for a model that has never seen JETCARP: what the tool does, a
//! minimal example, and the tool to call next. Every call runs with the interactive limits
//! ([`Limits::INTERACTIVE`]) and the server's folder rules ([`ctx`]).

use crate::schema;
use jetcarp_api::op::Callbacks;
use jetcarp_api::world::Host;
use jetcarp_api::{ApiError, Ctx, FsPolicy, Limits};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// Largest world a tool will build.
pub const MAX_CELLS: u64 = Limits::INTERACTIVE.max_cells;
/// Most generations one call may run.
pub const MAX_GENS: u64 = Limits::INTERACTIVE.max_gens;
/// Longest `run_script` run, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = Limits::INTERACTIVE.max_script_seconds;

/// What a tool returns: MCP content blocks.
#[derive(Clone, Debug)]
pub struct ToolOutput {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(s: impl Into<String>) -> ToolOutput {
        ToolOutput {
            content: vec![json!({"type": "text", "text": s.into()})],
            is_error: false,
        }
    }
    pub fn error(s: impl Into<String>) -> ToolOutput {
        ToolOutput {
            is_error: true,
            ..ToolOutput::text(s)
        }
    }
    /// JSON text, an integer beyond ±(2^53 − 1) in it written as a decimal string.
    pub fn json(v: &Value) -> ToolOutput {
        let v = jetcarp_core::json_int::safe(v.clone());
        ToolOutput::text(serde_json::to_string_pretty(&v).unwrap_or_default())
    }
    /// Text, then a PNG image.
    pub fn with_png(text: impl Into<String>, png: &[u8]) -> ToolOutput {
        let mut o = ToolOutput::text(text);
        o.content.push(
            json!({"type": "image", "data": crate::util::base64(png), "mimeType": "image/png"}),
        );
        o
    }
    /// All text blocks joined (for tests and logs).
    pub fn joined_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) type Res = Result<ToolOutput, String>;

/// Progress of a long tool call: `(progress, total, message)`, sent by the transport as an MCP
/// `notifications/progress` when the client asked for it (a `progressToken` in `_meta`).
pub type ProgressFn = Arc<dyn Fn(f64, Option<f64>, String) + Send + Sync>;

/// Set when the client cancels the call (`notifications/cancelled`).
pub type Cancel = Arc<std::sync::atomic::AtomicBool>;

/// One call: the world, the arguments, and the client's progress and cancel channels.
pub(crate) struct Call<'a> {
    pub host: &'a mut dyn Host,
    pub args: &'a Value,
    pub progress: Option<ProgressFn>,
    pub cancel: Option<Cancel>,
}

impl Call<'_> {
    /// Progress and cancellation as an operation takes them.
    pub fn callbacks(&self) -> Callbacks {
        Callbacks {
            on_report: self.progress.clone().map(|f| {
                Arc::new(move |d: u64, t: Option<u64>, m: &str| {
                    f(d as f64, t.map(|t| t as f64), m.to_string())
                }) as Arc<dyn Fn(u64, Option<u64>, &str) + Send + Sync>
            }),
            cancel: self.cancel.clone(),
        }
    }
}

/// A tool: its listing and what runs it.
pub(crate) struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    /// It changes nothing: no world, file, library or job.
    pub read_only: bool,
    /// It may replace or delete something (the world, records).
    pub destructive: bool,
    pub description: fn() -> String,
    pub schema: fn() -> Value,
    pub run: fn(&mut Call) -> Res,
}

impl Tool {
    fn definition(&self) -> Value {
        let mut schema = (self.schema)();
        schema::mcp_words(&mut schema);
        json!({
            "name": self.name,
            "title": self.title,
            "description": (self.description)(),
            "inputSchema": schema,
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.read_only,
                "openWorldHint": false,
            },
        })
    }
}

/// The shared tools, in the order `tools/list` shows them.
fn registry() -> &'static [Tool] {
    static T: OnceLock<Vec<Tool>> = OnceLock::new();
    T.get_or_init(|| {
        let mut v = vec![];
        v.extend(crate::help_tools::tools());
        v.extend(crate::world_tools::tools());
        v.extend(crate::script_tools::tools());
        v.extend(crate::library_tools::tools());
        v.extend(crate::job_tools::tools());
        v.extend(crate::search_tools::tools());
        v.extend(crate::media_tools::tools());
        v.extend(crate::pack_tools::tools());
        v.extend(crate::files_tools::tools());
        v
    })
}

/// The shared tools' names, in order.
pub fn names() -> Vec<&'static str> {
    registry().iter().map(|t| t.name).collect()
}

/// A shared tool of this name exists.
pub fn exists(name: &str) -> bool {
    registry().iter().any(|t| t.name == name)
}

/// Tool definitions for `tools/list`, in a fixed order (made once).
pub fn definitions() -> Vec<Value> {
    static D: OnceLock<Vec<Value>> = OnceLock::new();
    D.get_or_init(|| registry().iter().map(Tool::definition).collect())
        .clone()
}

/// Run a tool. `name` must be a shared tool or an app tool.
pub fn call(host: &mut dyn Host, name: &str, args: &Value) -> ToolOutput {
    call_ctx(host, name, args, None, None)
}

/// [`call`], reporting progress of long calls through `progress`.
pub fn call_with_progress(
    host: &mut dyn Host,
    name: &str,
    args: &Value,
    progress: Option<ProgressFn>,
) -> ToolOutput {
    call_ctx(host, name, args, progress, None)
}

/// [`call`] with progress and a cancel flag: long calls (`run_script`, `world_step`, waits for a
/// job) stop soon after the flag is set (an MCP `notifications/cancelled` for the request).
pub fn call_ctx(
    host: &mut dyn Host,
    name: &str,
    args: &Value,
    progress: Option<ProgressFn>,
    cancel: Option<Cancel>,
) -> ToolOutput {
    let r = match registry().iter().find(|t| t.name == name) {
        Some(t) => {
            let schema = (t.schema)();
            match schema::check_known(name, &schema, args) {
                Ok(()) => (t.run)(&mut Call {
                    host,
                    args,
                    progress,
                    cancel,
                }),
                Err(e) => Err(e),
            }
        }
        None if crate::app_tools::handles(name) => crate::app_tools::call(host, name, args),
        None => Err(format!("unknown tool {name}")),
    };
    r.unwrap_or_else(ToolOutput::error)
}

// ---------------------------------------------------------------------------------------------
// What the tools run with

static PLATFORM: OnceLock<Arc<dyn jetcarp_api::Platform>> = OnceLock::new();
static ALLOWED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The machine the tools run on: the `jetcarp` program sets its own before it serves (the GPU,
/// jobs in processes of their own); without one, the tools run headless. Set it before the first
/// tool call (first call wins).
pub fn set_platform(p: Arc<dyn jetcarp_api::Platform>) {
    let _ = PLATFORM.set(p);
}

/// More folders the tools may read and write (`jetcarp mcp --allow DIR`). Set them before the
/// first tool call.
pub fn allow_dirs(dirs: Vec<PathBuf>) {
    ALLOWED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(dirs.into_iter().map(|d| jetcarp_api::out::absolute(&d)));
}

/// The folders the tools may write in: the output folder, the library's folder, and the ones
/// given with [`allow_dirs`]. Reading is allowed there too, and in the folder of packs that came
/// with the program.
pub fn folders() -> Folders {
    let cx = Ctx::new();
    let out = cx.output_root();
    let lib = jetcarp_api::out::absolute(&cx.library_path());
    let library = lib.parent().map(PathBuf::from).unwrap_or(lib);
    Folders {
        output: out,
        library,
        allowed: ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        bundled_packs: jetcarp_core::paths::bundled_packs_dir(),
    }
}

/// Where the tools may read and write files.
#[derive(Clone, Debug)]
pub struct Folders {
    /// `JETCARP_OUT`, else `<Pictures>/JETCARP`: relative paths start here.
    pub output: PathBuf,
    /// The folder holding the library (`JETCARP_LIBRARY`'s, else the data folder).
    pub library: PathBuf,
    /// Folders given with `--allow`.
    pub allowed: Vec<PathBuf>,
    /// The packs that came with the program (read only).
    pub bundled_packs: Option<PathBuf>,
}

impl Folders {
    pub fn writable(&self) -> Vec<PathBuf> {
        let mut v = vec![self.output.clone(), self.library.clone()];
        v.extend(self.allowed.iter().cloned());
        v
    }
    pub fn readable(&self) -> Vec<PathBuf> {
        let mut v = self.writable();
        v.extend(self.bundled_packs.iter().cloned());
        v
    }
}

/// What the tools run with: the interactive limits ([`Limits::INTERACTIVE`]), the platform set
/// by [`set_platform`], and the folder rules ([`folders`]): files are read and written only in
/// the output folder, the library's folder and the allowed folders (the bundled packs are read
/// too), and relative paths start in the output folder.
pub fn ctx() -> &'static Ctx {
    static CX: OnceLock<Ctx> = OnceLock::new();
    CX.get_or_init(|| {
        let f = folders();
        let mut fs = FsPolicy::guarded(f.output.clone(), f.writable(), true);
        fs.read = Some(jetcarp_api::out::Guard {
            roots: f.readable(),
        });
        fs.who = Some("the MCP tools".into());
        let cx = Ctx::interactive().with_fs(fs);
        match PLATFORM.get() {
            Some(p) => cx.with_platform(p.clone()),
            None => cx,
        }
    })
}

/// An operation's error as the tool's error text.
pub(crate) fn err(e: ApiError) -> String {
    e.to_string()
}

/// The `world_status` report.
pub fn status(host: &dyn Host) -> Value {
    jetcarp_api::op::to_value(&jetcarp_api::ops::world::world_status(host))
}
