//! The media index: media for catalog targets (patterns, Quick Start entries, ant runs), made by
//! the engine (`jetcarp_core::media`: frames, animations, the house style, encoding) and recorded
//! in the catalog.
//!
//! **Storage.** Files go under the catalog's folder in
//! `media/<target kind>/<target id>/<alone|seed>/` (`frames/0000.png`..., `anim.gif` or
//! `anim.webp`, `poster.png`, `thumb.png`), one `media` row per file (the frame folder is one
//! row), with paths relative to the catalog folder, the render key, and the content hash.
//! Rendering again with the same inputs, options, and style does nothing.

pub mod ants;

use crate::{Library, QsStatus};
use jetcarp_core::media::{
    Animation, MediaMode, MediaOptions, STYLE_VERSION, content_hash, encode, frames_hash,
    pattern_animation, scene_animation,
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

/// One row of `media`, with the absolute path.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaRow {
    pub target_kind: String,
    pub target_id: String,
    /// `frames`, `gif`, `webp`, `poster`, `thumb`.
    pub kind: String,
    /// Relative to the catalog's folder (a folder for `frames`).
    pub path: String,
    pub abs_path: PathBuf,
    pub from_seed: bool,
    pub render: Value,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub bytes: u64,
    /// The render key the file was made with.
    pub key: String,
    /// Content hash (FNV-1a 64; frames: of every frame's hash).
    pub hash: String,
    pub created: String,
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

/// The folder (relative to the catalog's folder) holding a target's media in one mode.
pub fn media_dir(target_kind: &str, target_id: &str, mode: MediaMode) -> String {
    format!(
        "media/{target_kind}/{}/{}",
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
            let src = match (mode, q.source_kind.as_str(), &q.source_id) {
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

/// Stored media rows of a target (every mode), from the catalog.
pub fn stored(lib: &Library, target_kind: &str, target_id: &str) -> Result<Vec<MediaRow>, String> {
    let folder = lib.folder();
    lib.read(Vec::new(), |c| {
        let mut st = c
            .prepare(
                "SELECT target_kind, target_id, kind, path, from_seed, render, width, height, frames,
                        bytes, key, hash, created
                 FROM media WHERE target_kind = ?1 AND target_id = ?2 ORDER BY from_seed, kind",
            )
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map(rusqlite::params![target_kind, target_id], |r| {
                let path: String = r.get(3)?;
                Ok(MediaRow {
                    target_kind: r.get(0)?,
                    target_id: r.get(1)?,
                    kind: r.get(2)?,
                    abs_path: folder.join(&path),
                    path,
                    from_seed: r.get::<_, i64>(4)? != 0,
                    render: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                    width: r.get::<_, Option<i64>>(6)?.unwrap_or(0) as u32,
                    height: r.get::<_, Option<i64>>(7)?.unwrap_or(0) as u32,
                    frames: r.get::<_, Option<i64>>(8)?.unwrap_or(0) as u32,
                    bytes: r.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
                    key: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    hash: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                    created: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    })
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
        && mine.iter().all(|r| r.key == job.key && r.abs_path.exists())
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

/// Render a job's files into the catalog folder (no catalog writes). Pure apart from the files.
fn make_files(folder: &Path, job: &Job, o: &MediaOptions) -> Result<Vec<MediaRow>, String> {
    let anim = job_animation(job, o)?;
    write_files(
        folder,
        &media_dir(job.kind, &job.id, job.mode),
        &anim,
        o,
        job,
    )
}

fn write_files(
    folder: &Path,
    rel: &str,
    anim: &Animation,
    o: &MediaOptions,
    job: &Job,
) -> Result<Vec<MediaRow>, String> {
    let dir = if rel.is_empty() {
        folder.to_path_buf()
    } else {
        folder.join(rel)
    };
    // Clear only the files this writes (the folder may be the user's own, for scenes).
    let frames_dir = dir.join("frames");
    if frames_dir.is_dir() {
        std::fs::remove_dir_all(&frames_dir)
            .map_err(|e| format!("{}: {e}", frames_dir.display()))?;
    }
    for f in ["anim.gif", "anim.webp", "poster.png", "thumb.png"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let rel_of = |name: &str| {
        if rel.is_empty() {
            name.to_string()
        } else {
            format!("{rel}/{name}")
        }
    };
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
    let row = |kind: &str, path: String, w: u32, h: u32, frames: u32, bytes: u64, hash: String| {
        MediaRow {
            target_kind: job.kind.into(),
            target_id: job.id.clone(),
            kind: kind.into(),
            abs_path: folder.join(&path),
            path,
            from_seed: anim.mode == MediaMode::Seed,
            render: render.clone(),
            width: w,
            height: h,
            frames,
            bytes,
            key: job.key.clone(),
            hash,
            created: now.clone(),
        }
    };
    let put = |name: &str, b: &[u8]| -> Result<String, String> {
        let p = dir.join(name);
        std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(rel_of(name))
    };
    let (w, h) = (anim.width(), anim.height());
    let mut rows = Vec::new();
    if o.frames {
        let fdir = dir.join("frames");
        std::fs::create_dir_all(&fdir).map_err(|e| format!("{}: {e}", fdir.display()))?;
        let pngs: Vec<Vec<u8>> = anim
            .frames
            .par_iter()
            .map(encode::png)
            .collect::<Result<_, _>>()?;
        let mut total = 0u64;
        for (k, b) in pngs.iter().enumerate() {
            put(&format!("frames/{k:04}.png"), b)?;
            total += b.len() as u64;
        }
        rows.push(row(
            "frames",
            rel_of("frames"),
            w,
            h,
            pngs.len() as u32,
            total,
            frames_hash(pngs.iter().map(|b| b.as_slice())),
        ));
    }
    let name = format!("anim.{}", fmt.name());
    let path = put(&name, &bytes)?;
    rows.push(row(
        fmt.name(),
        path,
        w,
        h,
        anim.frames.len() as u32,
        bytes.len() as u64,
        content_hash(&bytes),
    ));
    let poster = encode::png(anim.poster_image())?;
    rows.push(row(
        "poster",
        put("poster.png", &poster)?,
        w,
        h,
        1,
        poster.len() as u64,
        content_hash(&poster),
    ));
    let t = anim.thumbnail(st);
    let thumb = encode::png(&t)?;
    rows.push(row(
        "thumb",
        put("thumb.png", &thumb)?,
        t.width,
        t.height,
        1,
        thumb.len() as u64,
        content_hash(&thumb),
    ));
    Ok(rows)
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
                "INSERT INTO media (target_kind, target_id, kind, path, from_seed, render, width,
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
                    r.path,
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
        let mut q = lib.quickstart(&[QsStatus::Auto, QsStatus::Approved])?;
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
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_files(dir, "", &anim, o, &job)
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
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_files(dir, "", anim, o, &job)
}
