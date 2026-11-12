//! JETCARP's MCP server: AI tools over the same operations the app, the command line and scripts
//! use, and the book as resources.
//!
//! Everything here is transport independent. [`Server::handle`] takes one JSON-RPC message and
//! returns the reply; [`Host`] (from `jetcarp_api::world`) is the world it acts on. The CLI
//! (`jetcarp mcp`) runs it over stdio against a headless [`Session`]; the app runs it on its UI
//! thread against the world the user is watching, fed by the local [`link`] endpoint. Every world
//! change a tool makes is a [`jetcarp_core::Command`] applied through the host.
//!
//! [`tools`] lists the tools and what each calls; [`resources`] the resources.

pub mod app_tools;
mod compounds;
pub mod docs;
pub mod files_tools;
mod help_tools;
pub mod job_tools;
mod library_tools;
pub mod link;
mod media_tools;
mod pack_tools;
pub mod protocol;
pub mod resources;
pub mod schema;
mod script_tools;
mod search_tools;
pub mod stdio;
pub mod tools;
mod util;
mod world_tools;

pub use jetcarp_api::world::{Host, Session};
pub use protocol::Server;
pub use tools::ToolOutput;
