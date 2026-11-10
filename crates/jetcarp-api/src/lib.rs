//! JETCARP's operations layer: the one set of operations that every front end calls. The command
//! line, the MCP server, Rhai scripts, and the app each turn their own input (flags, JSON tool
//! arguments, script maps, widgets) into calls here and present the result their own way, so an
//! operation is written once and behaves the same wherever it is started; the engine itself
//! stays in `jetcarp-core`. So far the crate holds what the front ends shared or had copied
//! between them: the argument parsers ([`args`]), output folders and PNG files ([`out`]), the
//! world a front end acts on ([`world`]), and cell coordinates and text patterns ([`cells`]).

pub mod args;
pub mod cells;
pub mod out;
pub mod world;
