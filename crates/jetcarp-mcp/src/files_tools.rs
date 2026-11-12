//! `files_list`: a read-only look into JETCARP's output folder (script runs, sweeps, piles,
//! media, wallpapers, website exports) and the library's folder, so a model can find what a run
//! wrote (`files.list`).
//!
//! Only two roots are visible: the output folder (`JETCARP_OUT`, else `<Pictures>/JETCARP`) and
//! the folder holding the library. A path may be relative to a root or an absolute path inside
//! one; it is resolved (symbolic links and `..` included) and refused when it leaves its root.
//! Nothing is ever written or deleted. Small text files can be read with `file`.

use crate::schema::{self, object};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Op, Quiet, Report, input_schema};
use jetcarp_api::ops::files::{self, FilesIn, FilesList, FilesOut};
use std::path::PathBuf;

pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "files_list",
        title: "List output files",
        read_only: true,
        destructive: false,
        description: || {
            "Lists what JETCARP wrote, read only: the output folder (root \"out\", the default: script runs, sweep folders, piles/, wallpapers/, media of scenes) or the library's folder (root \"catalog\": library.sqlite, media/, search folders). `path` is a folder relative to the root, or an absolute folder from another tool's output (it must be inside a root). Each entry has its name, kind, size, modified time, and for folders what marks them (manifest.json: a sweep or export; pile.json: a pile). `file` returns a small text file (JSON, CSV, text, Markdown, Rhai; the first 64 KB). Examples: {}, {\"path\": \"piles\"}, {\"file\": \"piles/hex-1000000/pile.json\"}.".into()
        },
        schema: || object(schema::props(&input_schema::<FilesIn>()), &[]),
        run: files_list,
    }]
}

/// Resolve `p` (relative to `root`, or absolute) and check it stays inside a root. Returns the
/// path and the root it is in.
pub fn resolve(root_name: &str, p: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    files::resolve(ctx(), root_name, p).map_err(err)
}

fn files_list(c: &mut Call) -> Res {
    let mut a: FilesIn = schema::read(&input_schema::<FilesIn>(), "files_list", c.args.clone())?;
    a.limit = Some(a.limit.unwrap_or(100).min(500));
    Ok(match FilesList::run(ctx(), a, &Quiet).map_err(err)? {
        FilesOut::Listing(l) => ToolOutput::json(&op::to_value(&l)),
        f @ FilesOut::File(_) => ToolOutput::text(f.text()),
    })
}
