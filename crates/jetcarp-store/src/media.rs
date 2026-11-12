//! The media index: media for library targets (patterns, Quick Start entries, ant runs), made by
//! the engine (`jetcarp_core::media`: frames, animations, the house style, encoding) and recorded
//! in the library.
//!
//! **Storage.** Files are content-addressed, beside the library file wherever it is:
//! `<library folder>/media/ab/<sha256>.<ext>`, named by the SHA-256 of their bytes (`ab` its
//! first two hex digits), so identical renders (a pattern and the Quick Start entry made from
//! it, an oscillator's repeated phases) are stored once. A target has one `media` row per file
//! and mode (`frames`, `gif` or `webp`, `poster`, `thumb`; alone or from the seed) with the
//! file's hash and extension, the render key, and the render's details. A frame sequence is its
//! PNG frames, each content-addressed, and a **frame list** (`<hash>.txt`: the line
//! `jetcarp-frames 1`, then one frame hash per line, in order), whose hash is the `frames` row's:
//! the frames stay plain PNG files that viewers and the website export use as they are, and a
//! loop's repeated phases are stored once. Files are written to a temporary name beside their
//! final one and renamed. Rendering again with the same inputs, options, and style does nothing.
//!
//! **Removal.** Deleting a target's rows never deletes a file (another row may name it); [`gc`]
//! deletes the files no row names, and leftover temporary files. [`export`] copies a target's
//! files out under readable names, and [`render_scene_to`] writes a scene that is not in the
//! library to a folder of its own the same way.

pub mod ants;

use crate::{Library, QsSource};
use jetcarp_core::media::{
    Animation, MediaMode, MediaOptions, STYLE_VERSION, content_hash, encode, pattern_animation,
    scene_animation,
};
use jetcarp_core::pattern::{PatternClass, PatternRecord};
use jetcarp_core::render::Image;
use jetcarp_core::world::Scene;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// Targets and the catalog

/// What media are made for.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaTarget {
    /// A catalog pattern by id (or unique prefix, or name).
    Pattern(String),
    /// A Quick Start entry by id.
    QuickStart(String),
    /// An ant run by id ([`crate::media::ants`]): alone is the moving highway loop, seed the
    /// build-up from step 0.
    Ant(String),
}

impl MediaTarget {
    pub fn kind(&self) -> &'static str {
        match self {
            MediaTarget::Pattern(_) => "pattern",
            MediaTarget::QuickStart(_) => "quickstart",
            MediaTarget::Ant(_) => "ant",
        }
    }
}

/// One row of `media`, with the file's paths.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaRow {
    pub target_kind: String,
    pub target_id: String,
    /// `frames`, `gif`, `webp`, `poster`, `thumb`.
    pub kind: String,
    /// Relative to the library's folder: `media/ab/<hash>.<ext>` (for `frames`, its frame list).
    pub path: String,
    pub abs_path: PathBuf,
    /// The file's extension: `png`, `gif`, `webp`, or `txt` (a frame list).
    pub ext: String,
    /// `frames`: every frame's file, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frame_files: Vec<PathBuf>,
    pub from_seed: bool,
    pub render: Value,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub bytes: u64,
    /// The render key the file was made with.
    pub key: String,
    /// SHA-256 of the file (frames: of the frame list).
    pub hash: String,
    pub created: String,
}

/// The media folder inside the library's folder.
pub const DIR: &str = "media";

/// What the first line of a frame list says.
pub const FRAME_LIST: &str = "jetcarp-frames 1";

/// Where a file of this hash and extension is, relative to the library's folder.
pub fn rel_path(hash: &str, ext: &str) -> String {
    format!("{DIR}/{}/{hash}.{ext}", hash.get(..2).unwrap_or("00"))
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Store bytes as a content-addressed file under `folder` (written to a temporary name and
/// renamed, unless a file of that hash is there already). Returns its hash.
pub fn put_file(folder: &Path, bytes: &[u8], ext: &str) -> Result<String, String> {
    let hash = content_hash(bytes);
    let p = folder.join(rel_path(&hash, ext));
    if std::fs::metadata(&p).is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Ok(hash);
    }
    let d = p.parent().unwrap_or(folder);
    std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = d.join(format!(".{hash}.{ext}.{}-{n}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", p.display())
    })?;
    Ok(hash)
}

/// The frame hashes a frame list file names, in order.
pub fn frame_list(folder: &Path, hash: &str) -> Result<Vec<String>, String> {
    let p = folder.join(rel_path(hash, "txt"));
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut lines = text.lines();
    if lines.next() != Some(FRAME_LIST) {
        return Err(format!("{} is not a frame list", p.display()));
    }
    Ok(lines.filter(|l| is_hash(l)).map(String::from).collect())
}

/// What [`render`] did for one target.
#[derive(Clone, Debug, Serialize)]
pub struct Rendered {
    pub target_kind: String,
    pub target_id: String,
    pub mode: MediaMode,
    /// False when the stored media were already up to date.
    pub written: bool,
    pub rows: Vec<MediaRow>,
}

impl Rendered {
    /// The animation's row (GIF or WebP).
    pub fn animation(&self) -> Option<&MediaRow> {
        self.rows
            .iter()
            .find(|r| r.kind == "gif" || r.kind == "webp")
    }
    pub fn file(&self, kind: &str) -> Option<&MediaRow> {
        self.rows.iter().find(|r| r.kind == kind)
    }
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A readable folder name for a target's media in one mode (for [`export`]):
/// `<target kind>/<target id>/<alone|seed>`.
pub fn media_dir(target_kind: &str, target_id: &str, mode: MediaMode) -> String {
    format!(
        "{target_kind}/{}/{}",
        safe(target_id),
        if mode == MediaMode::Seed {
            "seed"
        } else {
            "alone"
        }
    )
}

/// What a target renders: a pattern alone, or a scene.
#[derive(Clone)]
enum Source {
    Pattern(Box<PatternRecord>),
    Scene(Box<Scene>),
    /// An ant highway's moving loop.
    Ant(Box<crate::AntRow>),
}

struct Job {
    kind: &'static str,
    id: String,
    mode: MediaMode,
    source: Source,
    key: String,
    /// Generations from the seed when the options give none (ant build-ups).
    gens: Option<u64>,
}

/// The render key: the inputs that change the pictures, the options, and the style.
fn render_key(source: &Source, mode: MediaMode, o: &MediaOptions) -> String {
    let input = match source {
        Source::Pattern(r) => json!({
            "id": r.id, "rule": r.rule, "nb": r.neighborhood, "steps": r.steps_per_gen,
            "tiling": r.tiling, "class": r.class, "period": r.period, "d": r.displacement,
            "phases": r.phases, "gun": r.gun, "site": r.site.as_ref().map(|s| &s.patch),
            // Patterns on a patch: rendered again since past the patch's edge is background.
            "patch_edge": r.site.as_ref().map(|_| "background"),
        }),
        Source::Ant(a) => json!({
            "id": a.id, "rule": a.rule, "tiling": a.tiling, "start": a.start, "period": a.period,
            "d": a.d, "onset": a.onset,
        }),
        Source::Scene(s) => {
            let mut s = (**s).clone();
            s.jetcarp_version.clear();
            s.record = None;
            s.view = None;
            s.name.clear();
            s.description.clear();
            serde_json::to_value(&s).unwrap_or_default()
        }
    };
    let opts = json!({
        "mode": mode.name(), "format": o.format.name(), "periods": o.periods, "gens": o.gens,
        "every": o.every, "frames": o.frames,
    });
    let text = json!({"input": input, "opts": opts, "style": o.style.hash(), "v": STYLE_VERSION})
        .to_string();
    content_hash(text.as_bytes())
}

fn resolve(lib: &Library, t: &MediaTarget, mode: MediaMode) -> Result<Job, String> {
    let mut gens = None;
    let (kind, id, source, auto) = match t {
        MediaTarget::Pattern(key) => {
            let rec = lib.get(key)?;
            let id = rec.id.clone();
            let src = match mode {
                MediaMode::Seed => Source::Scene(Box::new(
                    jetcarp_core::pattern::promote::showcase_scene(&rec, None, None)?,
                )),
                _ => Source::Pattern(Box::new(rec)),
            };
            ("pattern", id, src, MediaMode::Alone)
        }
        MediaTarget::QuickStart(id) => {
            let q = lib.get_quickstart(id)?;
            let src = match (mode, q.target_kind.as_str(), &q.target_id) {
                (MediaMode::Alone, "pattern", Some(pid)) => {
                    Source::Pattern(Box::new(lib.get(pid)?))
                }
                (MediaMode::Alone, k, _) => {
                    return Err(format!(
                        "Quick Start entry {id} comes from a {k}, not a pattern: only patterns render alone"
                    ));
                }
                _ => Source::Scene(Box::new(q.scene)),
            };
            ("quickstart", q.id, src, MediaMode::Seed)
        }
        MediaTarget::Ant(key) => {
            let a = lib.ant(key)?;
            let highway = a.outcome == jetcarp_core::ants::run::Outcome::Highway;
            let want = match mode {
                MediaMode::Auto if highway => MediaMode::Alone,
                MediaMode::Auto => MediaMode::Seed,
                m => m,
            };
            let src = match want {
                MediaMode::Alone if !highway => {
                    return Err(format!(
                        "{} is {}: only highways have a moving loop (mode seed shows the run)",
                        a.id,
                        a.outcome.name()
                    ));
                }
                MediaMode::Alone => Source::Ant(Box::new(a.clone())),
                _ => Source::Scene(Box::new(crate::media::ants::build_scene(&a)?)),
            };
            gens = Some(crate::media::ants::build_gens(&a));
            ("ant", a.id.clone(), src, want)
        }
    };
    let mode = if mode == MediaMode::Auto { auto } else { mode };
    Ok(Job {
        kind,
        id,
        mode,
        source,
        key: String::new(),
        gens,
    })
}

/// Stored media rows of a target (every mode), from the library, with their files' paths.
pub fn stored(lib: &Library, target_kind: &str, target_id: &str) -> Result<Vec<MediaRow>, String> {
    let folder = lib.folder();
    let mut rows = lib.read(Vec::new(), |c| {
        let mut st = c
            .prepare(
                "SELECT target_kind, target_id, kind, ext, from_seed, render, width, height,
                        frames, bytes, key, hash, created
                 FROM media WHERE target_kind = ?1 AND target_id = ?2 ORDER BY from_seed, kind",
            )
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map(rusqlite::params![target_kind, target_id], |r| {
                let ext: String = r.get(3)?;
                let hash: String = r.get(11)?;
                let path = rel_path(&hash, &ext);
                Ok(MediaRow {
                    target_kind: r.get(0)?,
                    target_id: r.get(1)?,
                    kind: r.get(2)?,
                    abs_path: folder.join(&path),
                    path,
                    ext,
                    frame_files: Vec::new(),
                    from_seed: r.get::<_, i64>(4)? != 0,
                    render: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                    width: r.get::<_, Option<i64>>(6)?.unwrap_or(0) as u32,
                    height: r.get::<_, Option<i64>>(7)?.unwrap_or(0) as u32,
                    frames: r.get::<_, Option<i64>>(8)?.unwrap_or(0) as u32,
                    bytes: r.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
                    key: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    hash,
                    created: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    })?;
    for r in rows.iter_mut().filter(|r| r.kind == "frames") {
        r.frame_files = frame_list(&folder, &r.hash)
            .unwrap_or_default()
            .iter()
            .map(|h| folder.join(rel_path(h, "png")))
            .collect();
    }
    Ok(rows)
}

/// Every target with media: (target kind, target id).
pub fn targets_with_media(lib: &Library) -> Result<Vec<(String, String)>, String> {
    lib.read(Vec::new(), |c| {
        let mut st = c
            .prepare("SELECT DISTINCT target_kind, target_id FROM media ORDER BY 1, 2")
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    })
}

/// The stored thumbnail of a target (the first mode that has one), decoded, if any. The app
/// calls this for Quick Start cards and the dashboard, and falls back to
/// [`pattern_thumbnail`] when it is `None`.
pub fn thumbnail(
    lib: &Library,
    target_kind: &str,
    target_id: &str,
) -> Result<Option<Image>, String> {
    for r in stored(lib, target_kind, target_id)? {
        if r.kind == "thumb"
            && let Ok(b) = std::fs::read(&r.abs_path)
        {
            return encode::decode_png(&b).map(Some);
        }
    }
    Ok(None)
}

/// The stored animation's frames of a target (decoded from its GIF or WebP), if any.
pub fn animation_frames(
    lib: &Library,
    target_kind: &str,
    target_id: &str,
) -> Result<Option<Vec<Image>>, String> {
    for r in stored(lib, target_kind, target_id)? {
        let Ok(b) = std::fs::read(&r.abs_path) else {
            continue;
        };
        match r.kind.as_str() {
            "gif" => return encode::decode_gif(&b).map(Some),
            "webp" => return encode::decode_webp(&b).map(Some),
            _ => {}
        }
    }
    Ok(None)
}

/// Are the stored rows for this job up to date (same key, files present)?
fn fresh(rows: &[MediaRow], job: &Job) -> bool {
    let mine: Vec<&MediaRow> = rows
        .iter()
        .filter(|r| r.from_seed == (job.mode == MediaMode::Seed))
        .collect();
    !mine.is_empty()
        && mine.iter().any(|r| r.kind == "poster")
        && mine.iter().all(|r| {
            r.key == job.key
                && r.abs_path.exists()
                && (r.kind != "frames"
                    || (r.frame_files.len() == r.frames as usize
                        && r.frame_files.iter().all(|f| f.exists())))
        })
}

/// A job's animation in memory.
fn job_animation(job: &Job, o: &MediaOptions) -> Result<Animation, String> {
    Ok(match &job.source {
        Source::Pattern(r) => pattern_animation(
            r,
            &MediaOptions {
                mode: job.mode,
                ..o.clone()
            },
        )?,
        Source::Scene(s) => scene_animation(
            s,
            &MediaOptions {
                gens: o.gens.or(job.gens),
                ..o.clone()
            },
        )?,
        Source::Ant(a) => crate::media::ants::highway_animation(a, o)?,
    })
}

/// A target's animation in memory, as [`render`] would make it, without writing files or
/// catalog rows (for previews in another palette).
pub fn target_animation(
    lib: &Library,
    target: &MediaTarget,
    o: &MediaOptions,
) -> Result<Animation, String> {
    job_animation(&resolve(lib, target, o.mode)?, o)
}

/// Render a job's files into the library's media folder, content-addressed (no library
/// writes). Pure apart from the files.
fn make_files(folder: &Path, job: &Job, o: &MediaOptions) -> Result<Vec<MediaRow>, String> {
    let anim = job_animation(job, o)?;
    let enc = encode_all(&anim, o, job)?;
    let mut rows = Vec::new();
    for f in &enc.files {
        let (hash, ext, frame_files) = match &f.data {
            Data::One(b) => (put_file(folder, b, &f.ext)?, f.ext.clone(), Vec::new()),
            Data::Frames(pngs) => {
                let mut list = String::from(FRAME_LIST);
                let mut files = Vec::with_capacity(pngs.len());
                for b in pngs {
                    let h = put_file(folder, b, "png")?;
                    list.push('\n');
                    list.push_str(&h);
                    files.push(folder.join(rel_path(&h, "png")));
                }
                list.push('\n');
                (
                    put_file(folder, list.as_bytes(), "txt")?,
                    "txt".into(),
                    files,
                )
            }
        };
        let path = rel_path(&hash, &ext);
        rows.push(MediaRow {
            abs_path: folder.join(&path),
            path,
            ext,
            frame_files,
            hash,
            ..f.row.clone()
        });
    }
    Ok(rows)
}

/// A file's bytes: one file, or a frame sequence.
enum Data {
    One(Vec<u8>),
    Frames(Vec<Vec<u8>>),
}

/// A file to write, with its row (paths and hash not yet filled in).
struct EncFile {
    /// Its name in a readable folder (`anim.gif`, `poster.png`, `frames`).
    name: String,
    ext: String,
    data: Data,
    row: MediaRow,
}

/// An animation's files, encoded.
struct Encoded {
    files: Vec<EncFile>,
}

/// Write a job's files to `dir` under readable names (`frames/0000.png`..., `anim.gif` or
/// `anim.webp`, `poster.png`, `thumb.png`), replacing only those files (the folder may be the
/// user's own). Rows' paths are the files' names in `dir`.
fn write_tree(dir: &Path, enc: &Encoded) -> Result<Vec<MediaRow>, String> {
    let frames_dir = dir.join("frames");
    if frames_dir.is_dir() {
        std::fs::remove_dir_all(&frames_dir)
            .map_err(|e| format!("{}: {e}", frames_dir.display()))?;
    }
    for f in ["anim.gif", "anim.webp", "poster.png", "thumb.png"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let put = |name: &str, b: &[u8]| -> Result<(), String> {
        let p = dir.join(name);
        std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))
    };
    let mut rows = Vec::new();
    for f in &enc.files {
        let (hash, frame_files) = match &f.data {
            Data::One(b) => {
                put(&f.name, b)?;
                (content_hash(b), Vec::new())
            }
            Data::Frames(pngs) => {
                std::fs::create_dir_all(&frames_dir)
                    .map_err(|e| format!("{}: {e}", frames_dir.display()))?;
                let mut files = Vec::new();
                let mut list = String::from(FRAME_LIST);
                for (k, b) in pngs.iter().enumerate() {
                    let name = format!("frames/{k:04}.png");
                    put(&name, b)?;
                    files.push(dir.join(&name));
                    list.push('\n');
                    list.push_str(&content_hash(b));
                }
                list.push('\n');
                (content_hash(list.as_bytes()), files)
            }
        };
        rows.push(MediaRow {
            abs_path: dir.join(&f.name),
            path: f.name.clone(),
            ext: f.ext.clone(),
            frame_files,
            hash,
            ..f.row.clone()
        });
    }
    Ok(rows)
}

/// Encode an animation's files (frames, animation, poster, thumbnail) with their rows.
fn encode_all(anim: &Animation, o: &MediaOptions, job: &Job) -> Result<Encoded, String> {
    let st = &o.style;
    let (fmt, bytes) = anim.encode(o.format, st)?;
    let render = json!({
        "key": job.key,
        "mode": anim.mode.name(),
        "format": fmt.name(),
        "style_version": st.version,
        "style_hash": st.hash(),
        "window": anim.window_kind,
        "window_world": anim.window,
        "scale": anim.scale,
        "cell_px": anim.cell_px,
        "frames": anim.frames.len(),
        "generations": [anim.generations.first(), anim.generations.last()],
        "every": if anim.mode == MediaMode::Seed { o.every.max(1) } else { 1 },
        "periods": if anim.mode == MediaMode::Alone { o.periods.max(1) } else { 0 },
        "delay_ms": anim.delay_ms,
        "seamless": anim.seamless,
        "poster_frame": anim.poster,
        "jetcarp_version": jetcarp_core::VERSION,
    });
    let mut render = render;
    if let Some(p) = &st.palette {
        render["palette"] = p.clone().into();
    }
    let now = jetcarp_core::pattern::now_utc();
    let row = |kind: &str, w: u32, h: u32, frames: u32, bytes: u64| MediaRow {
        target_kind: job.kind.into(),
        target_id: job.id.clone(),
        kind: kind.into(),
        path: String::new(),
        abs_path: PathBuf::new(),
        ext: String::new(),
        frame_files: Vec::new(),
        from_seed: anim.mode == MediaMode::Seed,
        render: render.clone(),
        width: w,
        height: h,
        frames,
        bytes,
        key: job.key.clone(),
        hash: String::new(),
        created: now.clone(),
    };
    let (w, h) = (anim.width(), anim.height());
    let mut files = Vec::new();
    if o.frames {
        let pngs: Vec<Vec<u8>> = anim
            .frames
            .par_iter()
            .map(encode::png)
            .collect::<Result<_, _>>()?;
        let total = pngs.iter().map(|b| b.len() as u64).sum();
        files.push(EncFile {
            name: "frames".into(),
            ext: "txt".into(),
            row: row("frames", w, h, pngs.len() as u32, total),
            data: Data::Frames(pngs),
        });
    }
    files.push(EncFile {
        name: format!("anim.{}", fmt.name()),
        ext: fmt.name().into(),
        row: row(
            fmt.name(),
            w,
            h,
            anim.frames.len() as u32,
            bytes.len() as u64,
        ),
        data: Data::One(bytes),
    });
    let poster = encode::png(anim.poster_image())?;
    files.push(EncFile {
        name: "poster.png".into(),
        ext: "png".into(),
        row: row("poster", w, h, 1, poster.len() as u64),
        data: Data::One(poster),
    });
    let t = anim.thumbnail(st);
    let thumb = encode::png(&t)?;
    files.push(EncFile {
        name: "thumb.png".into(),
        ext: "png".into(),
        row: row("thumb", t.width, t.height, 1, thumb.len() as u64),
        data: Data::One(thumb),
    });
    Ok(Encoded { files })
}

fn store_rows(lib: &Library, done: &[(String, String, bool, Vec<MediaRow>)]) -> Result<(), String> {
    if done.is_empty() {
        return Ok(());
    }
    lib.write(|tx| {
        let mut del = tx
            .prepare(
                "DELETE FROM media WHERE target_kind = ?1 AND target_id = ?2 AND from_seed = ?3",
            )
            .map_err(|e| e.to_string())?;
        let mut ins = tx
            .prepare(
                "INSERT INTO media (target_kind, target_id, kind, ext, from_seed, render, width,
                    height, frames, bytes, key, hash, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
            )
            .map_err(|e| e.to_string())?;
        for (kind, id, seed, rows) in done {
            del.execute(rusqlite::params![kind, id, *seed as i64])
                .map_err(|e| e.to_string())?;
            for r in rows {
                ins.execute(rusqlite::params![
                    r.target_kind,
                    r.target_id,
                    r.kind,
                    r.ext,
                    r.from_seed as i64,
                    r.render.to_string(),
                    r.width as i64,
                    r.height as i64,
                    r.frames as i64,
                    r.bytes as i64,
                    r.key,
                    r.hash,
                    r.created,
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })
}

/// Render media for one target into the catalog (skipped when up to date unless `force`).
pub fn render(lib: &Library, target: &MediaTarget, o: &MediaOptions) -> Result<Rendered, String> {
    let mut r = render_many(lib, std::slice::from_ref(target), o)?;
    match r.failed.pop() {
        Some((_, _, e)) => Err(e),
        None => r.done.pop().ok_or_else(|| "nothing rendered".to_string()),
    }
}

/// What [`render_many`] and [`render_missing`] did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct BatchReport {
    pub done: Vec<Rendered>,
    /// (target kind, target id, error)
    pub failed: Vec<(String, String, String)>,
}

impl BatchReport {
    pub fn written(&self) -> usize {
        self.done.iter().filter(|d| d.written).count()
    }
    pub fn skipped(&self) -> usize {
        self.done.iter().filter(|d| !d.written).count()
    }
}

/// Render several targets in parallel (rayon): each target's files are a pure function of its
/// inputs, options, and style, so the bytes are the same at any thread count; the catalog rows
/// are written afterwards in one transaction, in target order.
pub fn render_many(
    lib: &Library,
    targets: &[MediaTarget],
    o: &MediaOptions,
) -> Result<BatchReport, String> {
    let folder = lib.folder();
    let mut report = BatchReport::default();
    let mut jobs: Vec<Job> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in targets {
        match resolve(lib, t, o.mode) {
            // The same target twice (an id and its prefix): once.
            Ok(j) if !seen.insert((j.kind, j.id.clone(), j.mode)) => {}
            Ok(mut j) => {
                j.key = render_key(&j.source, j.mode, o);
                let rows = stored(lib, j.kind, &j.id)?;
                if !o.force && fresh(&rows, &j) {
                    report.done.push(Rendered {
                        target_kind: j.kind.into(),
                        target_id: j.id.clone(),
                        mode: j.mode,
                        written: false,
                        rows: rows
                            .into_iter()
                            .filter(|r| r.from_seed == (j.mode == MediaMode::Seed))
                            .collect(),
                    });
                } else {
                    jobs.push(j);
                }
            }
            Err(e) => report.failed.push((t.kind().into(), id_of(t), e)),
        }
    }
    let results: Vec<Result<Vec<MediaRow>, String>> =
        jobs.par_iter().map(|j| make_files(&folder, j, o)).collect();
    let mut sets = Vec::new();
    let mut rendered = Vec::new();
    for (j, r) in jobs.iter().zip(results) {
        match r {
            Ok(rows) => {
                sets.push((
                    j.kind.to_string(),
                    j.id.clone(),
                    j.mode == MediaMode::Seed,
                    rows.clone(),
                ));
                rendered.push(Rendered {
                    target_kind: j.kind.into(),
                    target_id: j.id.clone(),
                    mode: j.mode,
                    written: true,
                    rows,
                });
            }
            Err(e) => report.failed.push((j.kind.into(), j.id.clone(), e)),
        }
    }
    store_rows(lib, &sets)?;
    report.done.extend(rendered);
    Ok(report)
}

fn id_of(t: &MediaTarget) -> String {
    match t {
        MediaTarget::Pattern(x) | MediaTarget::QuickStart(x) | MediaTarget::Ant(x) => x.clone(),
    }
}

/// Which targets [`render_missing`] looks at.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MissingFilter {
    /// Catalog patterns (all, or only these classes).
    pub patterns: bool,
    pub classes: Vec<PatternClass>,
    /// Quick Start entries shown in the menu (`auto` and `approved`).
    pub quickstart: bool,
    /// Ant runs that are highways or symmetric growth.
    #[serde(default)]
    pub ants: bool,
    /// At most this many targets rendered (0 = no limit).
    pub limit: usize,
}

/// Every pattern or Quick Start entry whose media are missing or stale (another key: changed
/// inputs, options, or style), rendered in parallel ([`render_many`]).
pub fn render_missing(
    lib: &Library,
    filter: &MissingFilter,
    o: &MediaOptions,
) -> Result<BatchReport, String> {
    let mut targets = Vec::new();
    if filter.patterns {
        let mut recs = lib.list()?;
        recs.sort_by(|a, b| a.id.cmp(&b.id));
        for r in recs {
            if filter.classes.is_empty() || filter.classes.contains(&r.class) {
                targets.push(MediaTarget::Pattern(r.id));
            }
        }
    }
    if filter.quickstart {
        let mut q = lib.quickstart(&[QsSource::Promotion, QsSource::Pack])?;
        q.retain(|q| !q.hidden);
        q.sort_by(|a, b| a.id.cmp(&b.id));
        targets.extend(q.into_iter().map(|q| MediaTarget::QuickStart(q.id)));
    }
    if filter.ants {
        for o in [
            jetcarp_core::ants::run::Outcome::Highway,
            jetcarp_core::ants::run::Outcome::Symmetric,
        ] {
            let mut a = lib.ants(&crate::AntQuery {
                outcome: Some(o),
                sort: Some("rule".into()),
                ..Default::default()
            })?;
            a.sort_by(|x, y| x.id.cmp(&y.id));
            targets.extend(a.into_iter().map(|a| MediaTarget::Ant(a.id)));
        }
    }
    // Only the stale ones count against the limit, so check freshness first.
    let mut pending = Vec::new();
    let mut fresh_report = BatchReport::default();
    for t in targets {
        let Ok(mut j) = resolve(lib, &t, o.mode) else {
            pending.push(t);
            continue;
        };
        j.key = render_key(&j.source, j.mode, o);
        if !o.force && fresh(&stored(lib, j.kind, &j.id)?, &j) {
            fresh_report.done.push(Rendered {
                target_kind: j.kind.into(),
                target_id: j.id,
                mode: j.mode,
                written: false,
                rows: Vec::new(),
            });
        } else {
            pending.push(t);
        }
    }
    if filter.limit > 0 {
        pending.truncate(filter.limit);
    }
    let mut r = render_many(lib, &pending, o)?;
    r.done.extend(fresh_report.done);
    Ok(r)
}

/// Render a scene that is not in the catalog into `dir` (frames, animation, poster,
/// thumbnail); returns the files written.
pub fn render_scene_to(
    scene: &Scene,
    dir: &Path,
    o: &MediaOptions,
) -> Result<Vec<MediaRow>, String> {
    let mode = if o.mode == MediaMode::Auto {
        MediaMode::Seed
    } else {
        o.mode
    };
    if mode == MediaMode::Alone {
        return Err("a scene renders from its seed; use a catalog pattern for alone".into());
    }
    let src = Source::Scene(Box::new(scene.clone()));
    let job = Job {
        kind: "scene",
        id: String::new(),
        mode,
        key: render_key(&src, mode, o),
        source: src,
        gens: None,
    };
    let anim = scene_animation(scene, o)?;
    write_tree(dir, &encode_all(&anim, o, &job)?)
}

/// Write an animation already in memory to `dir` (for scripts and tools).
pub fn write_animation(
    anim: &Animation,
    dir: &Path,
    o: &MediaOptions,
) -> Result<Vec<MediaRow>, String> {
    let job = Job {
        kind: "scene",
        id: String::new(),
        mode: anim.mode,
        key: String::new(),
        source: Source::Scene(Box::new(Scene::new(
            jetcarp_core::topology::TilingSpec::new("square"),
        ))),
        gens: None,
    };
    write_tree(dir, &encode_all(anim, o, &job)?)
}

/// Copy a target's stored media (one mode: `seed`, or alone when false; `None`: the mode with a
/// poster, alone first) into `dir` under readable names: `frames/0000.png`..., `anim.gif` or
/// `anim.webp`, `poster.png`, `thumb.png`. Returns the files written.
pub fn export(
    lib: &Library,
    target_kind: &str,
    target_id: &str,
    seed: Option<bool>,
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let rows = stored(lib, target_kind, target_id)?;
    let has = |s: bool| rows.iter().any(|r| r.from_seed == s && r.kind == "poster");
    let seed = match seed {
        Some(s) => s,
        None if has(false) => false,
        None => true,
    };
    let mine: Vec<&MediaRow> = rows.iter().filter(|r| r.from_seed == seed).collect();
    if mine.is_empty() {
        return Err(format!(
            "{target_kind} {target_id} has no media {} (media.render makes them)",
            if seed { "from the seed" } else { "alone" }
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    let copy = |from: &Path, to: PathBuf, out: &mut Vec<PathBuf>| -> Result<(), String> {
        std::fs::copy(from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        out.push(to);
        Ok(())
    };
    for r in mine {
        match r.kind.as_str() {
            "frames" => {
                let fd = dir.join("frames");
                std::fs::create_dir_all(&fd).map_err(|e| format!("{}: {e}", fd.display()))?;
                if r.frame_files.len() != r.frames as usize {
                    return Err(format!(
                        "the frame list {} is missing",
                        r.abs_path.display()
                    ));
                }
                for (k, f) in r.frame_files.iter().enumerate() {
                    copy(f, fd.join(format!("{k:04}.png")), &mut out)?;
                }
            }
            "gif" | "webp" => copy(&r.abs_path, dir.join(format!("anim.{}", r.kind)), &mut out)?,
            k => copy(&r.abs_path, dir.join(format!("{k}.png")), &mut out)?,
        }
    }
    Ok(out)
}

/// Forget a target's media (one mode, or both with `None`): its rows go, its files stay for
/// [`gc`] (another row may name the same file). Returns the rows removed.
pub fn remove(
    lib: &Library,
    target_kind: &str,
    target_id: &str,
    seed: Option<bool>,
) -> Result<u64, String> {
    lib.write(|tx| {
        tx.execute(
            "DELETE FROM media WHERE target_kind = ?1 AND target_id = ?2
             AND (?3 IS NULL OR from_seed = ?3)",
            rusqlite::params![target_kind, target_id, seed.map(|s| s as i64)],
        )
        .map(|n| n as u64)
        .map_err(|e| e.to_string())
    })
}

/// What [`gc`] found and did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct GcReport {
    /// Files the index names (frames through their lists), and their bytes.
    pub kept: u64,
    pub kept_bytes: u64,
    /// Files no row names, deleted (or, with `dry_run`, to delete), and their bytes.
    pub deleted: u64,
    pub deleted_bytes: u64,
    /// Leftover temporary files deleted.
    pub temp_files: u64,
    /// Unnamed or temporary files left alone because they are newer than `min_age_s` (a render
    /// in another process may not have recorded them yet).
    pub too_new: u64,
    /// Rows whose file is missing (rendering them again fixes it).
    pub missing: u64,
    pub dry_run: bool,
    /// The files deleted (or to delete), relative to the library's folder.
    pub files: Vec<String>,
}

/// Delete the media files no `media` row names (a frame list's frames count as named), and
/// leftover temporary files, older than `min_age_s` seconds. With `dry_run`, only say which.
/// Files outside `media/<two hex digits>/` are never touched.
pub fn gc(lib: &Library, dry_run: bool, min_age_s: u64) -> Result<GcReport, String> {
    let folder = lib.folder();
    let rows: Vec<(String, String)> = lib.read(Vec::new(), |c| {
        let mut st = c
            .prepare("SELECT DISTINCT hash, ext FROM media")
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        v.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    })?;
    let mut named: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = GcReport {
        dry_run,
        ..Default::default()
    };
    for (h, x) in &rows {
        let rel = rel_path(h, x);
        if !folder.join(&rel).is_file() {
            out.missing += 1;
            continue;
        }
        if x == "txt" {
            for f in frame_list(&folder, h).unwrap_or_default() {
                named.insert(rel_path(&f, "png"));
            }
        }
        named.insert(rel);
    }
    let now = std::time::SystemTime::now();
    let old = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|d| d.as_secs() >= min_age_s)
    };
    let root = folder.join(DIR);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Ok(out);
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|d| {
            let n = d.file_name().to_string_lossy().into_owned();
            n.len() == 2 && n.bytes().all(|b| b.is_ascii_hexdigit()) && d.path().is_dir()
        })
        .map(|d| d.path())
        .collect();
    dirs.sort();
    for d in dirs {
        let mut files: Vec<std::fs::DirEntry> = std::fs::read_dir(&d)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        files.sort_by_key(|f| f.file_name());
        let mut left = 0;
        for f in files {
            let name = f.file_name().to_string_lossy().into_owned();
            let rel = format!(
                "{DIR}/{}/{name}",
                d.file_name().unwrap_or_default().to_string_lossy()
            );
            let Ok(m) = f.metadata() else { continue };
            if !m.is_file() {
                left += 1;
                continue;
            }
            let temp = name.starts_with('.') && name.ends_with(".tmp");
            if !temp && named.contains(&rel) {
                out.kept += 1;
                out.kept_bytes += m.len();
                left += 1;
                continue;
            }
            if !old(&m) {
                out.too_new += 1;
                left += 1;
                continue;
            }
            if temp {
                out.temp_files += 1;
            } else {
                out.deleted += 1;
                out.deleted_bytes += m.len();
                out.files.push(rel.clone());
            }
            if dry_run {
                left += 1;
            } else {
                std::fs::remove_file(f.path())
                    .map_err(|e| format!("{}: {e}", f.path().display()))?;
            }
        }
        if left == 0 && !dry_run {
            let _ = std::fs::remove_dir(&d);
        }
    }
    Ok(out)
}
