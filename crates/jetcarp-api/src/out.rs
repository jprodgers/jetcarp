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
    Local::now().stamp()
}

/// [`stamp`] at `secs` seconds since 1970-01-01 UTC.
pub fn stamp_at(secs: i64) -> String {
    Local::at(secs).stamp()
}

/// A count and its noun: `1 row`, `3 rows`.
pub fn count(n: impl Into<u64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Local time as RFC 3339 to the second, `2026-11-05T14:30:02-05:00`, for records.
pub fn now_rfc3339() -> String {
    Local::now().rfc3339()
}

/// [`now_rfc3339`] at `secs` seconds since 1970-01-01 UTC.
pub fn rfc3339_at(secs: i64) -> String {
    Local::at(secs).rfc3339()
}

/// Local time to the minute, `2026-11-05 14:30`: the date of a machine's score.
pub fn date_and_minute() -> String {
    Local::now().date_and_minute()
}

/// [`date_and_minute`] at `secs` seconds since 1970-01-01 UTC.
pub fn date_and_minute_at(secs: i64) -> String {
    Local::at(secs).date_and_minute()
}

/// A moment's date and time in the system's time zone, written as JETCARP has always written
/// them (with chrono, before 0.4.3).
struct Local(jiff::ZonedDateTime);

impl Local {
    fn now() -> Local {
        Local::of(jiff::Timestamp::now())
    }

    fn at(secs: i64) -> Local {
        Local::of(jiff::Timestamp::from_second(secs).unwrap_or(jiff::Timestamp::UNIX_EPOCH))
    }

    fn of(t: jiff::Timestamp) -> Local {
        Local(t.to_zoned_datetime(jiff::tz::TimeZone::system()))
    }

    /// `2026-11-05`: the year in four digits, with a sign before one outside 0 to 9999.
    fn date(&self) -> String {
        let t = &self.0;
        let year = match t.year() {
            y @ 0..=9999 => format!("{y:04}"),
            y => format!("{y:+05}"),
        };
        format!("{year}-{:02}-{:02}", t.month(), t.day())
    }

    fn stamp(&self) -> String {
        let t = &self.0;
        let (h, m, s) = (t.hour(), t.minute(), t.second());
        format!("{}_{h:02}-{m:02}-{s:02}", self.date())
    }

    /// The offset in hours and minutes, rounded to the minute.
    fn rfc3339(&self) -> String {
        let t = &self.0;
        let (h, m, s) = (t.hour(), t.minute(), t.second());
        let offset = t.offset().seconds();
        let sign = if offset < 0 { '-' } else { '+' };
        let minutes = (offset.unsigned_abs() + 30) / 60;
        let (oh, om) = (minutes / 60, minutes % 60);
        format!("{}T{h:02}:{m:02}:{s:02}{sign}{oh:02}:{om:02}", self.date())
    }

    fn date_and_minute(&self) -> String {
        let t = &self.0;
        format!("{} {:02}:{:02}", self.date(), t.hour(), t.minute())
    }
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
    // Cut to 80 characters, then trimmed again: a cut can end on a `.`, and a name ending in one
    // is refused on every system ([`jetcarp_core::paths::check_name`]).
    let out: String = out.chars().take(80).collect();
    let mut out = out.trim_end_matches(['-', '.']).to_string();
    // A Windows device name (`con`, `aux.v2`) gets a `_` after its first part, so that a
    // scene called "Con" still has a folder every system can hold
    // ([`jetcarp_core::paths::reserved_on_windows`]).
    if jetcarp_core::paths::reserved_on_windows(&out).is_some() {
        let at = out.find('.').unwrap_or(out.len());
        out.insert(at, '_');
    }
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

// A path with its symbolic links followed, and a name JETCARP writes inside a checked folder
// checked to stay inside it. They live in `jetcarp_core::paths`, where the writers below this
// crate (media, packs, piles) reach them too.
pub use jetcarp_core::paths::{inside, resolve};

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
        self.find(p).ok().flatten()
    }

    /// [`Guard::locate`], with a path too long to read or write
    /// ([`jetcarp_core::paths::check_length`]), or a name below a folder that cannot be copied to
    /// every system ([`jetcarp_core::paths::check_name`]: a `:`, a Windows device name), refused
    /// in its own words rather than as outside the folders: the path as given, before anything is
    /// looked up, and the path that would be opened. The folders' own paths are the person's and
    /// are not checked for names.
    pub fn find(&self, p: &Path) -> Result<Option<(PathBuf, PathBuf)>, String> {
        jetcarp_core::paths::check_length(p)?;
        for r in &self.roots {
            jetcarp_core::paths::check_names_below(&absolute(r), p)?;
        }
        let real = match resolve(p) {
            Ok(real) => real,
            Err(e) => return jetcarp_core::paths::too_long(&e).map_or(Ok(None), Err),
        };
        for r in &self.roots {
            let r = absolute(r);
            let Ok(base) = resolve(&r) else { continue };
            let Ok(rest) = real.strip_prefix(&base) else {
                continue;
            };
            // The names that will really be read or made, if a link on the way changed them.
            jetcarp_core::paths::check_names_beyond(&base, &real)?;
            let path = if rest.as_os_str().is_empty() {
                r.clone()
            } else {
                r.join(rest)
            };
            jetcarp_core::paths::check_length(&path)?;
            return Ok(Some((path, r)));
        }
        Ok(None)
    }

    /// `p` is inside one of the folders once resolved ([`Guard::inside`]).
    pub fn allows(&self, p: &Path) -> bool {
        self.inside(p).is_some()
    }

    /// `path` (relative paths start at `base`) resolved and checked, nothing created. A path
    /// outside is refused naming the path as given, in the words of `who` (`the MCP tools`), or
    /// the script's words when there is none; a path too long, in its own ([`Guard::find`]).
    pub fn check(&self, base: &Path, path: &str, who: Option<&str>) -> Result<PathBuf, String> {
        let given = Path::new(path);
        let given = if given.is_absolute() {
            given.to_path_buf()
        } else {
            base.join(given)
        };
        self.find(&given)?.map(|(p, _)| p).ok_or_else(|| {
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
        // Windows' device names get a `_` so every system can hold the folder.
        assert_eq!(sanitize("Con"), "Con_");
        assert_eq!(sanitize("aux.v2"), "aux_.v2");
        assert_eq!(sanitize("NUL "), "NUL_");
        assert_eq!(sanitize("com1"), "com1_");
        assert_eq!(sanitize("console"), "console");
        assert_eq!(sanitize("com10"), "com10");
        assert_eq!(sanitize("a:b"), "a-b");
        // Never ending in a dot or a space, even where the 80-character cut falls on a dot.
        assert_eq!(sanitize("scene. "), "scene");
        assert_eq!(sanitize("..."), "run");
        let cut = sanitize(&format!("{}.x", "a".repeat(79)));
        assert_eq!(cut, "a".repeat(79));
        let cut = sanitize(&format!("{}-.-x", "b".repeat(78)));
        assert!(!cut.ends_with('.') && !cut.ends_with('-'), "{cut}");
        let g = Guard {
            roots: vec![PathBuf::from("/tmp/jetcarp-out")],
        };
        assert!(g.allows(Path::new("/tmp/jetcarp-out/a/b.png")));
        assert!(!g.allows(Path::new("/tmp/jetcarp-out/../etc/passwd")));
        assert!(!g.allows(Path::new("/tmp/jetcarp-outx/a.png")));
        // A path of 4,096 bytes or more, inside the folder or outside it, is refused in its own
        // words, before anything is looked up.
        for base in ["/tmp/jetcarp-out", "/tmp/elsewhere"] {
            let e = g
                .check(Path::new(base), &"a/".repeat(3000), Some("the MCP tools"))
                .unwrap_err();
            assert!(
                e.contains("bytes long") && !e.contains("may only write"),
                "{e}"
            );
        }
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

    /// Days from 1970-01-01 to `y`-`m`-`d` (the proleptic Gregorian calendar).
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
        era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
    }

    /// The moment an RFC 3339 time stamp such as `2026-11-05T14:30:02-05:00` names, in seconds
    /// since 1970-01-01 UTC.
    fn moment(rfc: &str) -> i64 {
        let n = |r: std::ops::Range<usize>| rfc[r].parse::<i64>().unwrap();
        assert_eq!(rfc.len(), 25, "{rfc}");
        let local = days_from_civil(n(0..4), n(5..7), n(8..10)) * 86_400
            + n(11..13) * 3600
            + n(14..16) * 60
            + n(17..19);
        let offset = n(20..22) * 3600 + n(23..25) * 60;
        match &rfc[19..20] {
            "+" => local - offset,
            "-" => local + offset,
            s => panic!("{s} in {rfc}"),
        }
    }

    /// Seconds since 1970-01-01 UTC now.
    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// The local times JETCARP writes (output folders' names, records, a machine's score) say
    /// the moment they were made, in this system's time zone, whatever it is, and agree with each
    /// other: the same date and time, written three ways.
    #[test]
    fn local_time_stamps_name_the_moment() {
        for secs in [1_000_000_000, 1_793_907_002, 1_772_953_200, 4_102_444_799] {
            let rfc = rfc3339_at(secs);
            assert_eq!(moment(&rfc), secs, "{rfc}");
            let stamp = format!("{}_{}", &rfc[..10], rfc[11..19].replace(':', "-"));
            assert_eq!(stamp_at(secs), stamp);
            assert_eq!(
                date_and_minute_at(secs),
                format!("{} {}", &rfc[..10], &rfc[11..16])
            );
        }
        // Now, in the second before or after.
        let before = now_secs();
        let (stamp, rfc, minute) = (stamp(), now_rfc3339(), date_and_minute());
        let after = now_secs();
        assert!((before..=after).any(|s| stamp_at(s) == stamp), "{stamp}");
        assert!((before..=after).any(|s| rfc3339_at(s) == rfc), "{rfc}");
        assert!(
            (before..=after).any(|s| date_and_minute_at(s) == minute),
            "{minute}"
        );
    }

    /// Set in a twin's process by [`in_time_zone`].
    #[cfg(unix)]
    const TZ_CHILD: &str = "JETCARP_OUT_TIME_ZONE_TEST";

    /// Moments, and the local time stamps of each in time zone `TZ`: [`stamp_at`],
    /// [`rfc3339_at`] and [`date_and_minute_at`]. Either side of daylight saving time's start and
    /// end, of midnight at the new year, and of 1970; zones with half and three-quarter hours, one
    /// set as a rule.
    #[cfg(unix)]
    #[rustfmt::skip]
    const IN_ZONES: &[(&str, i64, &str, &str, &str)] = &[
        ("America/New_York", 1_793_907_002, "2026-11-05_14-30-02", "2026-11-05T14:30:02-05:00", "2026-11-05 14:30"),
        ("America/New_York", 1_772_953_199, "2026-03-08_01-59-59", "2026-03-08T01:59:59-05:00", "2026-03-08 01:59"),
        ("America/New_York", 1_772_953_200, "2026-03-08_03-00-00", "2026-03-08T03:00:00-04:00", "2026-03-08 03:00"),
        ("America/New_York", 1_793_512_799, "2026-11-01_01-59-59", "2026-11-01T01:59:59-04:00", "2026-11-01 01:59"),
        ("America/New_York", 1_793_512_800, "2026-11-01_01-00-00", "2026-11-01T01:00:00-05:00", "2026-11-01 01:00"),
        ("America/New_York", 1_798_779_599, "2026-12-31_23-59-59", "2026-12-31T23:59:59-05:00", "2026-12-31 23:59"),
        ("America/New_York", 1_798_779_600, "2027-01-01_00-00-00", "2027-01-01T00:00:00-05:00", "2027-01-01 00:00"),
        ("America/New_York", -1, "1969-12-31_18-59-59", "1969-12-31T18:59:59-05:00", "1969-12-31 18:59"),
        ("EST5EDT,M3.2.0,M11.1.0", 1_772_953_200, "2026-03-08_03-00-00", "2026-03-08T03:00:00-04:00", "2026-03-08 03:00"),
        ("EST5EDT,M3.2.0,M11.1.0", 1_793_512_800, "2026-11-01_01-00-00", "2026-11-01T01:00:00-05:00", "2026-11-01 01:00"),
        ("Asia/Kathmandu", 1_793_907_002, "2026-11-06_01-15-02", "2026-11-06T01:15:02+05:45", "2026-11-06 01:15"),
        ("America/St_Johns", 1_768_478_400, "2026-01-15_08-30-00", "2026-01-15T08:30:00-03:30", "2026-01-15 08:30"),
        ("America/St_Johns", 1_782_907_200, "2026-07-01_09-30-00", "2026-07-01T09:30:00-02:30", "2026-07-01 09:30"),
        ("Europe/London", 1_768_478_400, "2026-01-15_12-00-00", "2026-01-15T12:00:00+00:00", "2026-01-15 12:00"),
        ("Europe/London", 1_782_907_200, "2026-07-01_13-00-00", "2026-07-01T13:00:00+01:00", "2026-07-01 13:00"),
        ("UTC", 1_793_907_002, "2026-11-05_19-30-02", "2026-11-05T19:30:02+00:00", "2026-11-05 19:30"),
    ];

    /// The local time stamps in each zone of [`IN_ZONES`], each set with `TZ` in a process of its
    /// own ([`twin_local_time_stamps_in_one_zone`]), are as written before.
    #[cfg(unix)]
    #[test]
    fn local_time_stamps_in_these_time_zones() {
        let mut zones: Vec<&str> = IN_ZONES.iter().map(|z| z.0).collect();
        zones.dedup();
        for tz in zones {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "out::tests::twin_local_time_stamps_in_one_zone",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(TZ_CHILD, "1")
                .env("TZ", tz)
                .output()
                .expect("start the test's own process");
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                out.status.success() && text.contains("1 passed"),
                "TZ={tz}:\n{text}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "a twin: local_time_stamps_in_these_time_zones runs it with TZ set"]
    fn twin_local_time_stamps_in_one_zone() {
        if std::env::var_os(TZ_CHILD).is_none() {
            return;
        }
        let tz = std::env::var("TZ").unwrap();
        let mut seen = 0;
        for (_, secs, stamp, rfc, minute) in IN_ZONES.iter().filter(|z| z.0 == tz) {
            assert_eq!(stamp_at(*secs), *stamp, "{secs} in {tz}");
            assert_eq!(rfc3339_at(*secs), *rfc, "{secs} in {tz}");
            assert_eq!(date_and_minute_at(*secs), *minute, "{secs} in {tz}");
            seen += 1;
        }
        assert!(seen > 0, "{tz}");
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
