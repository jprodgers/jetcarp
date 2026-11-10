//! `files_list`: a read-only look into JETCARP's output folders (script runs, sweeps, piles, media,
//! website exports) and the catalog's folder, so a model can find what a run wrote.
//!
//! Only two roots are visible: the output root (`JETCARP_OUT`, else `<Documents>/jetcarp-out`) and the
//! folder holding the catalog (`JETCARP_LIBRARY`'s folder). A path may be relative to a root or an
//! absolute path inside one; it is resolved (symbolic links and `..` included) and refused when it
//! leaves its root. Nothing is ever written or deleted. Small text files (JSON, CSV, text, at most
//! 64 KB shown) can be read with `file`.

use crate::tools::{Res, ToolOutput, check_unknown, int, string, tool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const NAMES: &[&str] = &["files_list"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

const MAX_TEXT: usize = 64 * 1024;

pub fn definitions() -> Vec<Value> {
    vec![tool(
        "files_list",
        "List output files",
        true,
        "Lists what JETCARP wrote, read only: the output folder (root \"out\", default: <Documents>/jetcarp-out with script runs, sweep folders, piles/, media) or the catalog's folder (root \"catalog\": catalog.sqlite, media/, website exports). `path` is a folder relative to the root, or an absolute folder from another tool's output (it must be inside a root). Each entry has its name, kind (dir or file), size, modified time, and for folders what marks them (manifest.json = a sweep or export, pile.json = a pile, favourites.json). `file` returns a small text file (JSON, CSV, text; first 64 KB). Examples: {}, {\"path\": \"piles\"}, {\"root\": \"catalog\", \"path\": \"media/ant\"}, {\"file\": \"piles/hex-1000000/pile.json\"}.",
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

fn roots() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("out", jetcarp_script::out::out_root(None)),
        (
            "catalog",
            jetcarp_core::library::Library::open_default().folder(),
        ),
    ]
}

/// Resolve `p` (relative to `root`, or absolute) and check it stays inside a root. Returns the
/// path and the root it is in.
pub fn resolve(root_name: &str, p: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    let all = roots();
    let base = all
        .iter()
        .find(|(n, _)| *n == root_name)
        .map(|(_, r)| r.clone())
        .ok_or_else(|| format!("`root` is out or catalog, not {root_name}"))?;
    let want = match p {
        None | Some("") | Some(".") => base.clone(),
        Some(s) if Path::new(s).is_absolute() => PathBuf::from(s),
        Some(s) => base.join(s),
    };
    let real = want
        .canonicalize()
        .map_err(|_| format!("{} does not exist", want.display()))?;
    for (_, r) in &all {
        if let Ok(rr) = r.canonicalize()
            && real.starts_with(&rr)
        {
            return Ok((real, rr));
        }
    }
    Err(format!(
        "{} is outside JETCARP's output folder ({}) and the catalog's folder ({}); files_list only reads those",
        real.display(),
        all[0].1.display(),
        all[1].1.display()
    ))
}

fn modified(m: &std::fs::Metadata) -> (u64, String) {
    let t = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (t, jetcarp_core::library::utc_of(t))
}

fn list(args: &Value) -> Res {
    check_unknown(args, &["root", "path", "file", "limit"])?;
    let root = string(args, "root")?.unwrap_or("out");
    if let Some(f) = string(args, "file")? {
        let (p, r) = resolve(root, Some(f))?;
        if !p.is_file() {
            return Err(format!("{} is not a file", p.display()));
        }
        let ext = p
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !["json", "csv", "txt", "md", "rhai", "log", "html", "tsv"].contains(&ext.as_str()) {
            return Err(format!(
                "{} is not a text file files_list shows (json, csv, txt, md, rhai, log, html)",
                p.display()
            ));
        }
        let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let cut = b.len() > MAX_TEXT;
        let text = String::from_utf8_lossy(&b[..b.len().min(MAX_TEXT)]).to_string();
        return Ok(ToolOutput::text(format!(
            "{} ({} bytes{}, inside {}):\n{text}",
            p.display(),
            b.len(),
            if cut { ", first 64 KB shown" } else { "" },
            r.display()
        )));
    }
    let (dir, r) = resolve(root, string(args, "path")?)?;
    if !dir.is_dir() {
        return Err(format!(
            "{} is not a folder (use `file` to read it)",
            dir.display()
        ));
    }
    let limit = int(args, "limit")?.unwrap_or(100).clamp(1, 500) as usize;
    let mut items: Vec<(u64, Value)> = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let Ok(e) = e else { continue };
        let Ok(m) = e.metadata() else { continue };
        let (t, when) = modified(&m);
        let name = e.file_name().to_string_lossy().to_string();
        let mut v = json!({"name": name, "modified": when});
        if m.is_dir() {
            v["kind"] = json!("dir");
            let marks: Vec<&str> = [
                "manifest.json",
                "pile.json",
                "favourites.json",
                "growth.csv",
                "catalog.sqlite",
                "index.html",
            ]
            .into_iter()
            .filter(|f| e.path().join(f).exists())
            .collect();
            if !marks.is_empty() {
                v["has"] = json!(marks);
            }
            v["entries"] = json!(std::fs::read_dir(e.path()).map(|d| d.count()).unwrap_or(0));
        } else {
            v["kind"] = json!("file");
            v["bytes"] = json!(m.len());
        }
        items.push((t, v));
    }
    let total = items.len();
    items.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1["name"].as_str().cmp(&b.1["name"].as_str()))
    });
    items.truncate(limit);
    Ok(ToolOutput::json(&json!({
        "folder": dir.display().to_string(),
        "root": r.display().to_string(),
        "entries": total,
        "shown": items.len(),
        "newest_first": items.into_iter().map(|x| x.1).collect::<Vec<_>>(),
    })))
}
