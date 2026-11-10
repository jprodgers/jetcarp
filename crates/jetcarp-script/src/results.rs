//! Finished (or running) sweep folders: the manifest's rows, favourites (`favourites.json`),
//! promotion of a run to the Quick Start ("My finds"), and contact sheets of chosen runs. Shared
//! by the app's results browser, `jetcarp library promote-run`, `jetcarp sheet --folder`, and the LLM
//! curation tool (`tools/curate_sweep.py`).

use jetcarp_core::Command;
use jetcarp_core::render::{Image, STAGE};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub type Row = Map<String, Value>;

/// A sweep folder's manifest and settings.
pub struct Manifest {
    pub folder: PathBuf,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// `run.json` (settings, timing, summary).
    pub run: Value,
    /// Read from `manifest.partial.jsonl`: the sweep is running or was interrupted.
    pub partial: bool,
}

pub fn num(r: &Row, k: &str) -> Option<f64> {
    r.get(k).and_then(|v| v.as_f64())
}

pub fn text<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

impl Manifest {
    pub fn load(folder: &Path) -> Result<Manifest, String> {
        let run = std::fs::read_to_string(folder.join("run.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null);
        let full = folder.join("manifest.json");
        let partial = folder.join("manifest.partial.jsonl");
        let mut rows: Vec<Row>;
        let mut columns: Vec<String> = vec![];
        let is_partial;
        if partial.exists() {
            // Running (or interrupted): the finished manifest (if any, from an earlier part)
            // plus the rows written since.
            is_partial = true;
            rows = vec![];
            if let Ok(t) = std::fs::read_to_string(&full)
                && let Ok(v) = serde_json::from_str::<Value>(&t)
                && let Some(r) = v["rows"].as_array()
            {
                rows.extend(r.iter().filter_map(|x| x.as_object().cloned()));
            }
            let t = std::fs::read_to_string(&partial).map_err(|e| e.to_string())?;
            rows.extend(
                t.lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .filter_map(|x| x.as_object().cloned()),
            );
        } else if full.exists() {
            is_partial = false;
            let t = std::fs::read_to_string(&full).map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&t).map_err(|e| format!("manifest.json: {e}"))?;
            rows = v["rows"]
                .as_array()
                .ok_or("manifest.json has no rows")?
                .iter()
                .filter_map(|x| x.as_object().cloned())
                .collect();
            columns = v["columns"]
                .as_array()
                .map(|c| {
                    c.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
        } else {
            return Err(format!(
                "{} has no manifest (not a sweep folder?)",
                folder.display()
            ));
        }
        // One row per index (a resumed sweep may list one twice), sorted.
        let mut by_index: BTreeMap<u64, Row> = BTreeMap::new();
        for r in rows {
            if let Some(i) = r.get("index").and_then(|i| i.as_u64()) {
                by_index.insert(i, r);
            }
        }
        let rows: Vec<Row> = by_index.into_values().collect();
        if columns.is_empty() {
            for r in &rows {
                for k in r.keys() {
                    if !columns.contains(k) {
                        columns.push(k.clone());
                    }
                }
            }
        }
        Ok(Manifest {
            folder: folder.to_path_buf(),
            columns,
            rows,
            run,
            partial: is_partial,
        })
    }

    /// Image height / width, from the sweep's image settings (default 9/16).
    pub fn aspect(&self) -> f32 {
        let c = &self.run["sweep"]["image"]["canvas"];
        match (c[0].as_f64(), c[1].as_f64()) {
            (Some(w), Some(h)) if w > 0.0 => (h / w) as f32,
            _ => {
                let s = &self.run["screen"];
                match (s["width"].as_f64(), s["height"].as_f64()) {
                    (Some(w), Some(h)) if w > 0.0 => (h / w) as f32,
                    _ => 9.0 / 16.0,
                }
            }
        }
        .clamp(0.2, 3.0)
    }

    /// Numeric columns (for sorting and ranges).
    pub fn numeric_columns(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|c| {
                !matches!(c.as_str(), "hash" | "kept" | "done")
                    && self
                        .rows
                        .iter()
                        .any(|r| r.get(*c).is_some_and(|v| v.is_number()))
            })
            .cloned()
            .collect()
    }

    pub fn image(&self, r: &Row) -> Option<PathBuf> {
        let f = text(r, "file");
        (!f.is_empty()).then(|| self.folder.join(f))
    }

    /// The folder's display name: `2026-11-05_14-17-34_HexW3`.
    pub fn name(&self) -> String {
        self.folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Runs the sweep has in all (from run.json), if known.
    pub fn total_runs(&self) -> Option<u64> {
        self.run["runs"].as_u64()
    }
}

/// Favourite runs of a sweep, saved as `favourites.json` in its folder (for the pattern library
/// and Quick Start later).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Favourites {
    /// By manifest index.
    pub items: BTreeMap<u64, Value>,
}

impl Favourites {
    pub const FILE: &'static str = "favourites.json";

    pub fn load(folder: &Path) -> Favourites {
        let mut f = Favourites::default();
        if let Ok(t) = std::fs::read_to_string(folder.join(Self::FILE))
            && let Ok(v) = serde_json::from_str::<Value>(&t)
            && let Some(a) = v["favourites"].as_array()
        {
            for x in a {
                if let Some(i) = x["index"].as_u64() {
                    f.items.insert(i, x.clone());
                }
            }
        }
        f
    }

    pub fn save(&self, folder: &Path) -> Result<(), String> {
        let v = json!({
            "jetcarp_version": jetcarp_core::VERSION,
            "sweep": folder.file_name().map(|n| n.to_string_lossy().into_owned()),
            "favourites": self.items.values().collect::<Vec<_>>(),
        });
        std::fs::write(
            folder.join(Self::FILE),
            serde_json::to_string_pretty(&v).unwrap_or_default(),
        )
        .map_err(|e| format!("could not save favourites: {e}"))
    }

    pub fn has(&self, r: &Row) -> bool {
        r.get("index")
            .and_then(|i| i.as_u64())
            .is_some_and(|i| self.items.contains_key(&i))
    }

    /// Add or remove a run; returns whether it is now a favourite.
    pub fn toggle(&mut self, r: &Row) -> bool {
        let Some(i) = r.get("index").and_then(|i| i.as_u64()) else {
            return false;
        };
        if self.items.remove(&i).is_some() {
            return false;
        }
        let mut x = Map::new();
        for k in [
            "index",
            "n",
            "rule",
            "file",
            "hash",
            "generations",
            "row_class",
            "period",
        ] {
            if let Some(v) = r.get(k) {
                x.insert(k.into(), v.clone());
            }
        }
        x.insert("note".into(), json!(""));
        x.insert(
            "added".into(),
            json!(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ),
        );
        self.items.insert(i, Value::Object(x));
        true
    }
}

/// Sweep folders under the output root, newest first.
pub fn sweep_folders(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && (p.join("manifest.json").exists()
                            || p.join("manifest.partial.jsonl").exists())
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// A Quick Start entry ("My finds") for run `row` of a sweep, stored in the catalog's
/// `quickstart` table: the run's tiling, CAs, and start, so it runs as in the sweep (row rules
/// run to the bottom and stop). Returns the entry id (`sweep-<hash>`; the app shows it as
/// `find:<id>`).
pub fn promote_run(
    m: &Manifest,
    row: usize,
    lib: &jetcarp_core::library::Library,
) -> Result<String, String> {
    promote_run_with(m, row, lib, None, None)
}

/// [`promote_run`] with a title (default: the rule) and a note put before the description (e.g. a
/// curator's one-line reason). Promoting the same run again replaces the same entry.
pub fn promote_run_with(
    m: &Manifest,
    row: usize,
    lib: &jetcarp_core::library::Library,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<String, String> {
    let r = m.rows.get(row).ok_or("no such run in the manifest")?;
    let rule = text(r, "rule").to_string();
    let w = crate::run_world(&m.run, &rule)?;
    let mut sc = w.to_scene();
    // The CAs belong in the scene's list (the Quick Start reads its rule from there).
    while matches!(sc.commands.first(), Some(Command::AddCa { .. })) {
        if let Command::AddCa { ca } = sc.commands.remove(0) {
            sc.cas.push(ca);
        }
    }
    sc.record = None;
    sc.name = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(&rule)
        .to_string();
    sc.description = format!(
        "{rule} from the sweep {}, run #{}: {}{}",
        m.name(),
        r["index"],
        subtitle(r),
        if m.run["sweep"]["until"] == "done" {
            "; it runs until done"
        } else {
            ""
        }
    );
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        sc.description = format!("{n} ({})", sc.description);
    }
    sc.view = Some(json!({ "speed": 30.0 }));
    let key = format!("{}:{}", m.name(), r["index"]);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in key.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let id = format!("sweep-{h:016x}");
    let mut q = jetcarp_core::library::QuickStartRow::for_scene(&id, "sweep", sc);
    q.source_id = Some(key);
    q.behaviour = r
        .get("row_class")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    lib.put_quickstart(&q)?;
    Ok(id)
}

/// A run's facts in one line: row class, row period, period, density, PNG size.
pub fn subtitle(r: &Row) -> String {
    let mut s = vec![];
    if let Some(c) = r.get("row_class").and_then(|c| c.as_str()) {
        s.push(c.to_string());
    }
    if let Some(p) = r.get("row_period").and_then(|p| p.as_u64()) {
        s.push(format!("rows ÷{p}"));
    }
    if let Some(p) = r.get("period").and_then(|p| p.as_u64()) {
        s.push(format!("period {p}"));
    }
    if let Some(d) = num(r, "density") {
        s.push(format!("{:.1}%", d * 100.0));
    }
    if let Some(b) = num(r, "png_bytes") {
        s.push(format!("{:.0} KB", b / 1024.0));
    }
    s.join(" · ")
}

impl Manifest {
    /// The row for a run given as its rule text (`HexT3 sum 588`, any case), its manifest index
    /// (`#12` or `12`), or its series number (`n=588`).
    pub fn find(&self, key: &str) -> Option<usize> {
        let k = key.trim();
        if let Some(i) = self
            .rows
            .iter()
            .position(|r| text(r, "rule").eq_ignore_ascii_case(k))
        {
            return Some(i);
        }
        let by = |col: &str, v: &str| -> Option<usize> {
            let v: u64 = v.trim().parse().ok()?;
            self.rows
                .iter()
                .position(|r| r.get(col).and_then(|x| x.as_u64()) == Some(v))
        };
        if let Some(n) = k.strip_prefix("n=") {
            return by("n", n);
        }
        by("index", k.strip_prefix('#').unwrap_or(k))
    }
}

/// What [`promote_folder_run`] did.
#[derive(Clone, Debug)]
pub struct Promoted {
    /// The Quick Start entry id (`sweep-<hash>`).
    pub id: String,
    pub rule: String,
    pub index: u64,
    pub title: String,
}

/// Promote run `run` (rule text, `#index`, or `n=number`) of the sweep in `folder` to the Quick
/// Start of catalog `lib` ([`promote_run_with`]) and mark it a favourite in the folder's
/// `favourites.json` with the entry (`find:<id>`) and the note, if any. Used by
/// `jetcarp library promote-run`, the script function `lib.promote_run`, and MCP `library_promote`.
pub fn promote_folder_run(
    folder: &Path,
    run: &str,
    lib: &jetcarp_core::library::Library,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<Promoted, String> {
    let m = Manifest::load(folder)?;
    let row = m.find(run).ok_or_else(|| {
        format!(
            "no run '{run}' in {} (give the rule text, #index, or n=number from manifest.json)",
            folder.display()
        )
    })?;
    let id = promote_run_with(&m, row, lib, title, note)?;
    let r = &m.rows[row];
    let mut favs = Favourites::load(folder);
    let mut extra = Map::new();
    extra.insert("promoted".into(), json!(format!("find:{id}")));
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        extra.insert("note".into(), json!(n));
    }
    favs.add(r, &extra);
    favs.save(folder)?;
    let title = lib.get_quickstart(&id).map(|q| q.title).unwrap_or_default();
    Ok(Promoted {
        id,
        rule: text(r, "rule").to_string(),
        index: r.get("index").and_then(|i| i.as_u64()).unwrap_or(0),
        title,
    })
}

impl Favourites {
    /// Add a run (or update it if it is one already) with these extra fields, e.g.
    /// `{"note": "...", "llm_note": "..."}`. Other fields of an existing favourite are kept.
    pub fn add(&mut self, r: &Row, fields: &Map<String, Value>) -> bool {
        let Some(i) = r.get("index").and_then(|i| i.as_u64()) else {
            return false;
        };
        if !self.items.contains_key(&i) {
            self.toggle(r);
        }
        if let Some(Value::Object(f)) = self.items.get_mut(&i) {
            for (k, v) in fields {
                f.insert(k.clone(), v.clone());
            }
        }
        true
    }
}

/// Decode a PNG (any colour type, indexed included) into RGBA.
pub fn load_png(path: &Path) -> Result<Image, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| err(&e))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| err(&e))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| err(&e))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in buf[..w * h * ch].chunks_exact(ch) {
        rgba.extend_from_slice(&match ch {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        });
    }
    Ok(Image {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

/// Shrink an image to fit `side` x `side` pixels (box filter; never enlarges).
pub fn shrink(img: &Image, side: u32) -> Image {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let s = (side as f64 / w.max(h) as f64).min(1.0);
    let tw = ((w as f64 * s).round() as u32).max(1);
    let th = ((h as f64 * s).round() as u32).max(1);
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h / th).clamp(y0 + 1, h);
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).clamp(x0 + 1, w);
            let mut acc = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let o = ((y * w + x) * 4) as usize;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += img.rgba[o + c] as u64;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            rgba.extend(acc.iter().map(|a| (a / n) as u8));
        }
    }
    Image {
        width: tw,
        height: th,
        rgba,
    }
}

/// A contact sheet of chosen runs of a sweep: each run's picture shrunk to `thumb` pixels on its
/// long side, under its label lines, `cols` per row, labels drawn at up to `label_scale` times
/// the built-in 3x5 font. Runs without a picture get an empty panel.
pub fn contact_sheet(
    m: &Manifest,
    items: &[(usize, Vec<String>)],
    thumb: u32,
    cols: u32,
    label_scale: u32,
) -> Result<Image, String> {
    let bh = (thumb as f32 * m.aspect()).round().max(1.0) as u32;
    let blank = Image {
        width: thumb,
        height: bh,
        rgba: STAGE
            .iter()
            .copied()
            .cycle()
            .take((thumb * bh * 4) as usize)
            .collect(),
    };
    let mut imgs = Vec::with_capacity(items.len());
    for (row, _) in items {
        let r = m.rows.get(*row).ok_or("no such run in the manifest")?;
        imgs.push(match m.image(r) {
            Some(p) if p.exists() => shrink(&load_png(&p)?, thumb),
            _ => blank.clone(),
        });
    }
    let white = [235, 235, 235, 255];
    let labelled: Vec<(jetcarp_core::sheet::Label, &Image)> = items
        .iter()
        .zip(&imgs)
        .map(|((_, lines), img)| (lines.iter().map(|l| (l.clone(), white)).collect(), img))
        .collect();
    Ok(jetcarp_core::sheet::labelled_sheet_scaled(
        &labelled,
        cols,
        label_scale,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_promote_favourite_and_sheet() {
        let root = std::env::temp_dir().join(format!("jetcarp-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = crate::run(
            r#"sweep(#{ rules: "W", from: 28, to: 31, tiling: "square", size: [40, 24],
                        boundary: "dead", seed: "top_center", until: "done",
                        png: #{ size: [80, 48] } });"#,
            crate::RunOptions {
                out_root: Some(root.clone()),
                ..Default::default()
            },
            |_| {},
            Default::default(),
        );
        assert!(s.ok, "{:?}", s.error);
        let m = Manifest::load(&sweep_folders(&root)[0]).unwrap();
        let k = m.find("w30").unwrap();
        assert_eq!(text(&m.rows[k], "rule"), "W30");
        assert_eq!(m.find("#2"), Some(k));
        assert_eq!(m.find("n=30"), Some(k));
        assert_eq!(m.find("W99"), None);
        // Favourite with an extra field, kept on a second add.
        let mut f = Favourites::load(&m.folder);
        let mut extra = Map::new();
        extra.insert("llm_note".into(), json!("nested triangles"));
        assert!(f.add(&m.rows[k], &extra));
        assert!(f.add(&m.rows[k], &Map::new()));
        f.save(&m.folder).unwrap();
        let back = Favourites::load(&m.folder);
        assert_eq!(back.items.len(), 1);
        assert_eq!(
            back.items.values().next().unwrap()["llm_note"],
            "nested triangles"
        );
        // Promotion with a title and note.
        let lib = jetcarp_core::library::Library::open(root.join("lib"));
        let id = promote_run_with(&m, k, &lib, Some("Rule thirty"), Some("chaotic")).unwrap();
        let finds = jetcarp_core::quickstart::user_finds(&lib);
        assert_eq!(finds.len(), 1);
        assert_eq!(finds[0].id, format!("find:{id}"));
        let sc = &finds[0].scene;
        assert_eq!(sc.name, "Rule thirty");
        assert!(sc.description.starts_with("chaotic ("));
        // Contact sheet of two runs.
        let img = contact_sheet(
            &m,
            &[(0, vec!["A W28".into()]), (k, vec!["B W30".into()])],
            60,
            2,
            3,
        )
        .unwrap();
        assert!(img.width > 120 && img.height > 36);
        let _ = std::fs::remove_dir_all(&root);
    }
}
