//! `files.list`: a read-only look into JETCARP's output folders (script runs, sweeps, piles,
//! media, website exports) and the catalog's folder, so a caller can find what a run wrote.
//!
//! Only two roots are visible: the output root (`JETCARP_OUT`, else `<Pictures>/JETCARP`) and the
//! folder holding the catalog (`JETCARP_LIBRARY`'s folder). A path may be relative to a root or an
//! absolute path inside one; it is resolved (symbolic links and `..` included) and refused when it
//! leaves its root. Nothing is ever written or deleted. Small text files (JSON, CSV, text, at most
//! 64 KB shown) can be read with `file`.

use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report, to_pretty};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Most bytes of a text file shown.
pub const MAX_TEXT: usize = 64 * 1024;

/// What to list or read.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesIn {
    /// Which root: "out" (default: the output folder) or "catalog" (the catalog's folder).
    #[serde(default)]
    pub root: Option<String>,
    /// Folder inside the root, relative or absolute (default: the root itself).
    #[serde(default)]
    pub path: Option<String>,
    /// A text file inside the root to read instead of listing (json, csv, txt, md, rhai, log,
    /// html, tsv; first 64 KB).
    #[serde(default)]
    pub file: Option<String>,
    /// Most entries listed, newest first (default 100, at most 500).
    #[serde(default)]
    pub limit: Option<u64>,
}

/// One entry of a folder.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FileEntry {
    pub name: String,
    /// `dir` or `file`.
    pub kind: String,
    /// Modified time (UTC).
    pub modified: String,
    /// Files: size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Folders: how many entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<usize>,
    /// Folders: the files that mark what it is (manifest.json = a sweep or export, pile.json = a
    /// pile, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has: Option<Vec<String>>,
    #[serde(skip)]
    pub modified_unix: u64,
}

/// A folder's entries.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Listing {
    pub folder: String,
    pub root: String,
    /// Entries in the folder.
    pub entries: usize,
    /// Entries shown.
    pub shown: usize,
    pub newest_first: Vec<FileEntry>,
}

/// A text file.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FileText {
    pub path: String,
    pub root: String,
    pub bytes: usize,
    /// Only the first 64 KB are shown.
    pub truncated: bool,
    pub text: String,
}

/// A listing, or a file's text.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FilesOut {
    Listing(Listing),
    File(FileText),
}

impl Report for FilesOut {
    fn text(&self) -> String {
        match self {
            FilesOut::Listing(l) => to_pretty(l),
            FilesOut::File(f) => format!(
                "{} ({} bytes{}, inside {}):\n{}",
                f.path,
                f.bytes,
                if f.truncated {
                    ", first 64 KB shown"
                } else {
                    ""
                },
                f.root,
                f.text
            ),
        }
    }
}

fn roots(cx: &Ctx) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("out", cx.output_root()),
        ("catalog", cx.library().folder()),
    ]
}

/// Resolve `p` (relative to the root named `root_name`, or absolute) and check it stays inside a
/// root. Returns the path and the root it is in.
pub fn resolve(cx: &Ctx, root_name: &str, p: Option<&str>) -> Result<(PathBuf, PathBuf), ApiError> {
    let all = roots(cx);
    let base = all
        .iter()
        .find(|(n, _)| *n == root_name)
        .map(|(_, r)| r.clone())
        .ok_or_else(|| ApiError::invalid(format!("`root` is out or catalog, not {root_name}")))?;
    let want = match p {
        None | Some("") | Some(".") => base.clone(),
        Some(s) if Path::new(s).is_absolute() => PathBuf::from(s),
        Some(s) => base.join(s),
    };
    let real = want
        .canonicalize()
        .map_err(|_| ApiError::not_found(format!("{} does not exist", want.display())))?;
    for (_, r) in &all {
        if let Ok(rr) = r.canonicalize()
            && real.starts_with(&rr)
        {
            return Ok((real, rr));
        }
    }
    Err(ApiError::io(format!(
        "{} is outside JETCARP's output folder ({}) and the catalog's folder ({}); files_list only reads those",
        real.display(),
        all[0].1.display(),
        all[1].1.display()
    )))
}

fn modified(m: &std::fs::Metadata) -> (u64, String) {
    let t = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (t, jetcarp_core::pattern::utc_of(t))
}

/// `files.list`.
pub struct FilesList;
impl Op for FilesList {
    const NAME: &'static str = "files.list";
    const SUMMARY: &'static str = "List the output folder or the catalog's folder (read only), or read a small text file there.";
    const READ_ONLY: bool = true;
    type In = FilesIn;
    type Out = FilesOut;
    fn run(cx: &Ctx, a: FilesIn, _: &dyn Progress) -> Result<FilesOut, ApiError> {
        let root = a.root.as_deref().unwrap_or("out");
        if let Some(f) = a.file.as_deref() {
            let (p, r) = resolve(cx, root, Some(f))?;
            if !p.is_file() {
                return Err(ApiError::invalid(format!("{} is not a file", p.display())));
            }
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !["json", "csv", "txt", "md", "rhai", "log", "html", "tsv"].contains(&ext.as_str()) {
                return Err(ApiError::invalid(format!(
                    "{} is not a text file files_list shows (json, csv, txt, md, rhai, log, html)",
                    p.display()
                )));
            }
            let b = std::fs::read(&p).map_err(|e| ApiError::io(format!("{}: {e}", p.display())))?;
            return Ok(FilesOut::File(FileText {
                path: p.display().to_string(),
                root: r.display().to_string(),
                bytes: b.len(),
                truncated: b.len() > MAX_TEXT,
                text: String::from_utf8_lossy(&b[..b.len().min(MAX_TEXT)]).to_string(),
            }));
        }
        let (dir, r) = resolve(cx, root, a.path.as_deref())?;
        if !dir.is_dir() {
            return Err(ApiError::invalid(format!(
                "{} is not a folder (use `file` to read it)",
                dir.display()
            )));
        }
        let limit = a.limit.unwrap_or(100).clamp(1, 500) as usize;
        let mut items: Vec<FileEntry> = Vec::new();
        let rd =
            std::fs::read_dir(&dir).map_err(|e| ApiError::io(format!("{}: {e}", dir.display())))?;
        for e in rd {
            let Ok(e) = e else { continue };
            let Ok(m) = e.metadata() else { continue };
            let (t, when) = modified(&m);
            let name = e.file_name().to_string_lossy().to_string();
            let mut v = FileEntry {
                name,
                kind: "file".into(),
                modified: when,
                bytes: None,
                entries: None,
                has: None,
                modified_unix: t,
            };
            if m.is_dir() {
                v.kind = "dir".into();
                let marks: Vec<String> = [
                    "manifest.json",
                    "pile.json",
                    "favourites.json",
                    "growth.csv",
                    "catalog.sqlite",
                    "index.html",
                ]
                .into_iter()
                .filter(|f| e.path().join(f).exists())
                .map(String::from)
                .collect();
                if !marks.is_empty() {
                    v.has = Some(marks);
                }
                v.entries = Some(std::fs::read_dir(e.path()).map(|d| d.count()).unwrap_or(0));
            } else {
                v.bytes = Some(m.len());
            }
            items.push(v);
        }
        let total = items.len();
        items.sort_by(|a, b| {
            b.modified_unix
                .cmp(&a.modified_unix)
                .then_with(|| a.name.cmp(&b.name))
        });
        items.truncate(limit);
        Ok(FilesOut::Listing(Listing {
            folder: dir.display().to_string(),
            root: r.display().to_string(),
            entries: total,
            shown: items.len(),
            newest_first: items,
        }))
    }
}
