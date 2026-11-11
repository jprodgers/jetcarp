//! `files_list`: a read-only look into JETCARP's output folders (script runs, sweeps, piles, media,
//! website exports) and the catalog's folder, so a model can find what a run wrote.
//!
//! Only two roots are visible: the output root (`JETCARP_OUT`, else `<Pictures>/JETCARP`) and the
//! folder holding the catalog (`JETCARP_LIBRARY`'s folder). A path may be relative to a root or an
//! absolute path inside one; it is resolved (symbolic links and `..` included) and refused when it
//! leaves its root. Nothing is ever written or deleted. Small text files (JSON, CSV, text, at most
//! 64 KB shown) can be read with `file`.

use crate::tools::{Res, ToolOutput, check_unknown, err, int, string, tool};
use jetcarp_api::op::{self, Op, Quiet, Report};
use jetcarp_api::ops::files::{self, FilesIn, FilesList, FilesOut};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const NAMES: &[&str] = &["files_list"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn definitions() -> Vec<Value> {
    vec![tool(
        "files_list",
        "List output files",
        true,
        "Lists what JETCARP wrote, read only: the output folder (root \"out\", default: <Pictures>/JETCARP with script runs, sweep folders, piles/, media) or the catalog's folder (root \"catalog\": library.sqlite, media/, website exports). `path` is a folder relative to the root, or an absolute folder from another tool's output (it must be inside a root). Each entry has its name, kind (dir or file), size, modified time, and for folders what marks them (manifest.json = a sweep or export, pile.json = a pile, favourites.json). `file` returns a small text file (JSON, CSV, text; first 64 KB). Examples: {}, {\"path\": \"piles\"}, {\"root\": \"catalog\", \"path\": \"media/ant\"}, {\"file\": \"piles/hex-1000000/pile.json\"}.",
        json!({"type": "object", "properties": {
            "root": {"type": "string", "enum": ["out", "catalog"], "description": "Which root (default out)."},
            "path": {"type": "string", "description": "Folder inside the root (default: the root itself)."},
            "file": {"type": "string", "description": "A text file inside the root to read instead of listing."},
            "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Most entries (default 100, newest first)."},
        }, "additionalProperties": false}),
    )]
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "files_list" => list(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

/// Resolve `p` (relative to `root`, or absolute) and check it stays inside a root. Returns the
/// path and the root it is in.
pub fn resolve(root_name: &str, p: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    files::resolve(crate::tools::ctx(), root_name, p).map_err(err)
}

fn list(args: &Value) -> Res {
    check_unknown(args, &["root", "path", "file", "limit"])?;
    let a = FilesIn {
        root: string(args, "root")?.map(String::from),
        path: string(args, "path")?.map(String::from),
        file: string(args, "file")?.map(String::from),
        limit: int(args, "limit")?.map(|n| n.max(0) as u64),
    };
    Ok(
        match FilesList::run(crate::tools::ctx(), a, &Quiet).map_err(err)? {
            FilesOut::Listing(l) => ToolOutput::json(&op::to_value(&l)),
            f @ FilesOut::File(_) => ToolOutput::text(f.text()),
        },
    )
}
