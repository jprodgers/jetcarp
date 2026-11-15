//! Output folders, the write guard, PNG encoding, time stamps, and CSV.
//!
//! Everything JETCARP writes for people goes under the output root (`<Pictures>/JETCARP`,
//! `JETCARP_OUT`, or the host's override), in a folder per run ([`make_out_dir`]). Scripts write
//! only inside that root and folders the user named (`--out`, `jetcarp_script::RunOptions`'s
//! `allow_dirs`): [`Guard`] checks every path, with its symbolic links followed ([`resolve`]).

use jetcarp_core::render::Image;
use std::path::{Component, Path, PathBuf};

/// The output root: `explicit` (host option), else [`jetcarp_core::paths::output_dir`]
/// (`$JETCARP_OUT`, else `<Pictures>/JETCARP`).
pub fn out_root(explicit: Option<&Path>) -> PathBuf {
    match explicit {
        Some(p) => absolute(p),
        None => absolute(&jetcarp_core::paths::output_dir()),
    }
}

/// Local time as `2026-11-05_14-30-02` (safe in file names on every system).
pub fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Local time as RFC 3339, for records.
/// A count and its noun: `1 row`, `3 rows`.
pub fn count(n: impl Into<u64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

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

/// Most symbolic links one path may pass through (Linux's own limit), so that a loop of links
/// ends instead of being followed for ever.
const MAX_LINKS: u32 = 40;

/// `p` as the file system will see it when a file is opened or made there: made absolute, the
/// deepest part of it that exists resolved with every symbolic link followed
/// ([`std::fs::canonicalize`]), and the rest, which does not exist yet, joined on with its `.`
/// and `..` worked out.
///
/// The folder rules ([`Guard`]) check this path rather than the path's text, because a link
/// inside an allowed folder may point anywhere. A link whose target does not exist is followed by
/// hand: `canonicalize` refuses it, but a write through it would make the target. `..` follows
/// the system's rule, so on Unix `link/..` is the folder holding the link's target, not the
/// folder holding the link.
pub fn resolve(p: &Path) -> std::io::Result<PathBuf> {
    resolve_links(&std::path::absolute(p)?, 0)
}

/// [`resolve`] for an absolute path, `links` links into it.
fn resolve_links(p: &Path, links: u32) -> std::io::Result<PathBuf> {
    let parts: Vec<Component> = p.components().collect();
    let mut k = parts.len();
    let mut real = PathBuf::new();
    while k > 0 {
        let head: PathBuf = parts[..k].iter().collect();
        if let Ok(r) = std::fs::canonicalize(&head) {
            real = r;
            break;
        }
        if matches!(parts[k - 1], Component::Normal(_))
            && std::fs::symlink_metadata(&head).is_ok_and(|m| m.file_type().is_symlink())
        {
            if links >= MAX_LINKS {
                return Err(std::io::Error::other(format!(
                    "{}: too many symbolic links",
                    p.display()
                )));
            }
            // A relative target starts in the folder that holds the link.
            let mut next = head
                .parent()
                .unwrap_or(Path::new(""))
                .join(std::fs::read_link(&head)?);
            next.extend(&parts[k..]);
            return resolve_links(&std::path::absolute(&next)?, links + 1);
        }
        k -= 1;
    }
    // What is left does not exist, so it holds no links; but a `..` in it can step back into
    // what does exist (`new/../link`), so then the result is resolved again.
    let mut stepped_back = false;
    for c in &parts[k..] {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                real.pop();
                stepped_back = true;
            }
            c => real.push(c.as_os_str()),
        }
    }
    if stepped_back {
        resolve_links(&real, links)
    } else {
        Ok(real)
    }
}

/// Where a script may write.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    pub roots: Vec<PathBuf>,
}

impl Guard {
    /// `p` (absolute, or relative to the working directory) if it is inside one of the folders
    /// once both are resolved ([`resolve`]), else None (also when it cannot be resolved: a loop
    /// of links). The path returned is the folder as the guard holds it followed by the rest of
    /// the resolved path, so it shows the folder as its user named it, and opening it opens the
    /// file that was checked, whatever links lie on the way.
    pub fn inside(&self, p: &Path) -> Option<PathBuf> {
        self.locate(p).map(|(path, _)| path)
    }

    /// [`Guard::inside`], with the folder the path is in.
    pub fn locate(&self, p: &Path) -> Option<(PathBuf, PathBuf)> {
        let real = resolve(p).ok()?;
        self.roots.iter().find_map(|r| {
            let r = absolute(r);
            let rest = real.strip_prefix(resolve(&r).ok()?).ok()?;
            let path = if rest.as_os_str().is_empty() {
                r.clone()
            } else {
                r.join(rest)
            };
            Some((path, r))
        })
    }

    /// `p` is inside one of the folders once resolved ([`Guard::inside`]).
    pub fn allows(&self, p: &Path) -> bool {
        self.inside(p).is_some()
    }

    /// `path` (relative paths start at `base`) resolved and checked, nothing created. A path
    /// outside is refused naming the path as given, in the words of `who` (`the MCP tools`), or
    /// the script's words when there is none.
    pub fn check(&self, base: &Path, path: &str, who: Option<&str>) -> Result<PathBuf, String> {
        let given = Path::new(path);
        let given = if given.is_absolute() {
            given.to_path_buf()
        } else {
            base.join(given)
        };
        self.inside(&given).ok_or_else(|| {
            let roots: Vec<String> = self.roots.iter().map(|r| r.display().to_string()).collect();
            let got = absolute(&given);
            match who {
                Some(who) => format!(
                    "{who} may only write inside {} (got {})",
                    roots.join(" or "),
                    got.display()
                ),
                None => format!(
                    "scripts may only write inside {} (got {}); use out_dir(\"label\") for a new output folder",
                    roots.join(" or "),
                    got.display()
                ),
            }
        })
    }

    /// Resolve `path` (relative paths are relative to `base`) and check it may be written.
    /// Creates the parent folder.
    pub fn writable(&self, base: &Path, path: &str) -> Result<PathBuf, String> {
        let p = self.check(base, path, None)?;
        if let Some(parent) = p.parent() {
            create_folder(parent)?;
        }
        Ok(p)
    }

    /// Resolve the folder `path` (relative paths are relative to `base`), check it may be
    /// written into, and create it. A refusal names the folder, not a file in it.
    pub fn writable_dir(&self, base: &Path, path: &str) -> Result<PathBuf, String> {
        let dir = self.check(base, path, None)?;
        create_folder(&dir)?;
        Ok(dir)
    }
}

/// Create a folder and those above it. A folder that cannot be made (a read-only output folder)
/// is named with the system's reason, such as "Permission denied", so it is not mistaken for the
/// folder rules.
pub fn create_folder(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("could not create folder {}: {e}", dir.display()))
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

/// Encode an image as an RGBA PNG with text chunks, default compression: the bytes the MCP
/// `render_image` tool has always returned.
pub fn encode_png_rgba(img: &Image, text: &[(&str, String)]) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        for (k, v) in text {
            enc.add_itxt_chunk(k.to_string(), v.clone())
                .map_err(|e| e.to_string())?;
        }
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
        wr.finish().map_err(|e| e.to_string())?;
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

    /// A fresh scratch folder for one test.
    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jetcarp-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out/sub")).unwrap();
        std::fs::create_dir_all(d.join("outside")).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn the_guard_follows_links_before_it_checks() {
        use std::os::unix::fs::symlink;
        let d = scratch("links");
        let out = d.join("out");
        let g = Guard {
            roots: vec![out.clone()],
        };
        // A linked folder, a linked file, and a link to a file that does not exist yet.
        symlink(d.join("outside"), out.join("dir")).unwrap();
        std::fs::write(d.join("outside/f.txt"), "x").unwrap();
        symlink(d.join("outside/f.txt"), out.join("file.txt")).unwrap();
        symlink(d.join("outside/new.txt"), out.join("dangling.txt")).unwrap();
        for p in ["dir/a.png", "dir", "file.txt", "dangling.txt"] {
            assert!(!g.allows(&out.join(p)), "{p}");
            assert!(g.writable(&out, p).is_err(), "{p}");
        }
        assert!(!d.join("outside/new.txt").exists());
        // `..` after a link is worked out from where the link leads.
        symlink(d.join("outside"), out.join("sub/up")).unwrap();
        assert!(!g.allows(&out.join("sub/up/../x")));
        assert_eq!(
            resolve(&out.join("sub/up/../x")).unwrap(),
            std::fs::canonicalize(&d).unwrap().join("x")
        );
        // A link that stays inside is followed, and the path keeps the folder's own name.
        symlink(out.join("sub"), out.join("alias")).unwrap();
        assert_eq!(
            g.inside(&out.join("alias/a.png")),
            Some(out.join("sub/a.png"))
        );
        assert_eq!(g.inside(&out.join("new/../sub")), Some(out.join("sub")));
        // A loop of links ends, and is refused.
        symlink(out.join("loop-b"), out.join("loop-a")).unwrap();
        symlink(out.join("loop-a"), out.join("loop-b")).unwrap();
        assert!(resolve(&out.join("loop-a/x")).is_err());
        assert!(!g.allows(&out.join("loop-a/x")));
        let _ = std::fs::remove_dir_all(&d);
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
