//! JETCARP's MCP server: AI tools over the same command layer the UI and the CLI use.
//!
//! Everything here is transport independent. [`Server::handle`] takes one JSON-RPC message and
//! returns the reply; [`Host`] is the world it acts on. The CLI (`jetcarp mcp`) runs it over stdio
//! against a headless [`Session`]; the app runs it on its UI thread against the world the user is
//! watching, fed by the local [`link`] endpoint. Every world change a tool makes is a
//! [`jetcarp_core::Command`] applied through the host.

pub mod app_tools;
pub mod cells;
pub mod display_tools;
pub mod explore_tools;
pub mod files_tools;
pub mod host;
pub mod hunt_tools;
pub mod job_tools;
pub mod library_tools;
pub mod link;
pub mod media_tools;
pub mod protocol;
pub mod stage1_tools;
pub mod stdio;
pub mod tools;
mod util;

pub use host::{Host, Session};
pub use protocol::Server;
pub use tools::ToolOutput;
