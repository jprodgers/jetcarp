//! JETCARP's operations layer: the one set of operations that every front end calls. The command
//! line, the MCP server, Rhai scripts, and the app each turn their own input (flags, JSON tool
//! arguments, script maps, widgets) into calls here and present the result their own way, so an
//! operation is written once and behaves the same wherever it is started; the engine itself
//! stays in `jetcarp-core`.
//!
//! - [`op`]: the [`Op`] and [`WorldOp`] traits, text output ([`Report`]), [`Progress`], and calls
//!   by name ([`call_json`]); [`ops`] holds the operations and their registry ([`ops::OPS`]).
//! - [`ctx`]: what an operation runs with ([`Ctx`]: folders, [`Limits`], [`FsPolicy`], and the
//!   machine as the program sees it, [`Platform`]).
//! - [`error`]: [`ApiError`].
//!
//! And what the front ends shared or had copied between them: the argument parsers ([`args`]),
//! output folders and PNG files ([`out`]), the world a front end acts on ([`world`]), cell
//! coordinates and text patterns ([`cells`]), and sweep folders ([`sweeps`]: manifests,
//! favourites, promotion of a run, contact sheets, and the start of any run).

pub mod args;
pub mod cells;
pub mod ctx;
pub mod error;
pub mod op;
pub mod ops;
pub mod out;
pub mod sweeps;
pub mod world;

pub use ctx::{Ctx, FsPolicy, Limits, Paths, Platform};
pub use error::{ApiError, ErrorKind};
pub use op::{
    Callbacks, DynOp, NoInput, Op, Progress, Quiet, Report, WorldOp, call, call_json, call_json_on,
    call_on, find,
};
