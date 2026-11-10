//! Output folders, the write guard, and PNG encoding.
//!
//! Scripts write only inside the output root (`<Documents>/jetcarp-out`, `JETCARP_OUT`, or the host's
//! override) and folders the user named (`--out`, [`crate::RunOptions::allow_dirs`]).

use jetcarp_core::render::Image;
use std::path::{Component, Path, PathBuf};

/// The output root: `explicit` (host option), else `$JETCARP_OUT`, else `<Documents>/jetcarp-out`
/// (`dirs::document_dir()`, falling back to `~/Documents`, then the working directory).
pub fn out_root(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return absolute(p);
    }
    if let Some(p) = std::env::var_os("JETCARP_OUT").filter(|p| !p.is_empty()) {
        return absolute(Path::new(&p));
    }
    let docs = dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."));
    absolute(&docs.join("jetcarp-out"))
}

/// Local time as `2026-11-05_14-30-02` (safe in file names on every system).
pub fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Local time as RFC 3339, for records.
pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// A label made safe for a file name: letters, digits, `-`, `_`, `.`; spaces and anything else
/// become `-` (runs collapse), at most 80 characters.
pub fn sanitize(label: &str) -> String {
    let mut out = String::new();
    for c in label.trim().chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            c
        } else {
            '-'
        };
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    let out = out.trim_matches(|c| c == '-' || c == '.').to_string();
    let out: String = out.chars().take(80).collect();
    if out.is_empty() { "run".into() } else { out }
}

/// Create `<root>/<stamp>_<label>/` (with `_2`, `_3`, ... if that exists already).
pub fn make_out_dir(root: &Path, label: &str) -> Result<PathBuf, String> {
    let base = format!("{}_{}", stamp(), sanitize(label));
    let mut dir = root.join(&base);
    let mut k = 2;
    while dir.exists() {
        dir = root.join(format!("{base}_{k}"));
        k += 1;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Absolute, lexically normalised path (`.` and `..` resolved without touching the disk).
pub fn absolute(p: &Path) -> PathBuf {
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Where a script may write.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    pub roots: Vec<PathBuf>,
}

impl Guard {
    pub fn allows(&self, p: &Path) -> bool {
        let p = absolute(p);
        self.roots.iter().any(|r| p.starts_with(r))
    }
    /// Resolve `path` (relative paths are relative to `base`) and check it may be written.
    /// Creates the parent folder.
    pub fn writable(&self, base: &Path, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let p = if p.is_absolute() {
            absolute(p)
        } else {
            absolute(&base.join(p))
        };
        if !self.allows(&p) {
            let roots: Vec<String> = self.roots.iter().map(|r| r.display().to_string()).collect();
            return Err(format!(
                "scripts may only write inside {} (got {}); use out_dir(\"label\") for a new output folder",
                roots.join(" or "),
                p.display()
            ));
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        Ok(p)
    }
}

/// Encode a PNG with text chunks. Images with at most 256 colours are written as indexed PNGs.
/// Balanced deflate with adaptive filters: on a 1920x1080 hexagon picture about 19 KB in 5 ms,
/// where the fast mode without filters gives 800 KB.
pub fn encode_png(
    img: &Image,
    text: &[(&str, String)],
    compression: png::Compression,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::with_capacity(1 << 16);
    let mut enc = png::Encoder::new(&mut buf, img.width, img.height);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(compression);
    enc.set_filter(png::Filter::Adaptive);
    for (k, v) in text {
        enc.add_itxt_chunk(k.to_string(), v.clone())
            .map_err(|e| e.to_string())?;
    }
    match palette_of(&img.rgba) {
        Some((palette, indices)) => {
            let mut plte = Vec::with_capacity(palette.len() * 3);
            let mut trns = Vec::with_capacity(palette.len());
            for c in &palette {
                plte.extend_from_slice(&c.to_le_bytes()[..3]);
                trns.push(c.to_le_bytes()[3]);
            }
            enc.set_color(png::ColorType::Indexed);
            enc.set_palette(plte);
            if trns.iter().any(|&a| a != 255) {
                enc.set_trns(trns);
            }
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(&indices).map_err(|e| e.to_string())?;
            wr.finish().map_err(|e| e.to_string())?;
        }
        None => {
            enc.set_color(png::ColorType::Rgba);
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
            wr.finish().map_err(|e| e.to_string())?;
        }
    }
    Ok(buf)
}

/// Palette (RGBA as little-endian u32) and one index per pixel, if there are at most 256 colours.
fn palette_of(rgba: &[u8]) -> Option<(Vec<u32>, Vec<u8>)> {
    let mut palette: Vec<u32> = Vec::new();
    let mut indices = Vec::with_capacity(rgba.len() / 4);
    let (mut last, mut last_i) = (u32::MAX, 0u8);
    for px in rgba.as_chunks::<4>().0 {
        let c = u32::from_le_bytes(*px);
        if c != last || palette.is_empty() {
            let i = match palette.iter().position(|&p| p == c) {
                Some(i) => i,
                None => {
                    if palette.len() == 256 {
                        return None;
                    }
                    palette.push(c);
                    palette.len() - 1
                }
            };
            last = c;
            last_i = i as u8;
        }
        indices.push(last_i);
    }
    Some((palette, indices))
}

/// Write bytes to a file.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// CSV field quoting (RFC 4180).
pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Rows of JSON objects as CSV: columns in first-seen order (or `columns` first).
pub fn to_csv(rows: &[serde_json::Value], columns: &[String]) -> String {
    let mut cols: Vec<String> = columns.to_vec();
    for r in rows {
        if let Some(o) = r.as_object() {
            for k in o.keys() {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    let mut out = cols
        .iter()
        .map(|c| csv_field(c))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for r in rows {
        let line: Vec<String> = cols
            .iter()
            .map(|c| match r.get(c) {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => csv_field(s),
                Some(serde_json::Value::Number(n)) => match n.as_f64() {
                    Some(f) if n.is_f64() => format_float(f),
                    _ => n.to_string(),
                },
                Some(v) => csv_field(&v.to_string()),
            })
            .collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// Floats in CSV: up to 6 decimals, trailing zeros trimmed.
pub fn format_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        return format!("{f:.0}");
    }
    let s = format!("{f:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Host name for records (no extra dependency: environment, then /etc/hostname).
pub fn host_name() -> String {
    for k in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(h) = std::env::var(k)
            && !h.trim().is_empty()
        {
            return h.trim().to_string();
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_guard() {
        assert_eq!(sanitize("HexW3"), "HexW3");
        assert_eq!(sanitize("B3/S23 soup hunt!"), "B3-S23-soup-hunt");
        assert_eq!(sanitize("../.."), "run");
        let g = Guard {
            roots: vec![PathBuf::from("/tmp/jetcarp-out")],
        };
        assert!(g.allows(Path::new("/tmp/jetcarp-out/a/b.png")));
        assert!(!g.allows(Path::new("/tmp/jetcarp-out/../etc/passwd")));
        assert!(!g.allows(Path::new("/tmp/jetcarp-outx/a.png")));
    }

    #[test]
    fn csv_quoting() {
        let rows = vec![serde_json::json!({"a": 1, "b": "x,y", "c": 0.5})];
        assert_eq!(to_csv(&rows, &["c".into()]), "c,a,b\n0.5,1,\"x,y\"\n");
    }

    #[test]
    fn indexed_png_round_trip() {
        let mut img = Image {
            width: 3,
            height: 2,
            rgba: vec![0; 24],
        };
        img.rgba[4..8].copy_from_slice(&[255, 0, 0, 255]);
        let bytes = encode_png(
            &img,
            &[("jetcarp:test", "1".into())],
            png::Compression::Balanced,
        )
        .unwrap();
        let dec = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut r = dec.read_info().unwrap();
        assert_eq!(r.info().color_type, png::ColorType::Indexed);
        let mut out = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut out).unwrap();
        assert_eq!(out[1], 1);
    }
}
