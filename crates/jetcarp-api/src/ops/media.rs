//! `media.*` and `site.export`: media for the catalog (per-step PNG frames, an animation, a
//! poster, and a thumbnail, in one house style) for a pattern, a Quick Start entry, an ant run,
//! or any scene, and the incremental website export of the public part of the catalog.
//!
//! The MCP tools and Rhai read their options from a JSON map the same way
//! ([`MediaRenderIn::from_map`], [`SiteIn::from_map`]); the command line from its flags.

use super::library::catalog_err;
use super::scene::read_scene;
use crate::args::{check_keys, get_bool, get_f64, get_list, get_str, get_u64};
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::library::{CurationState, PatternClass, QsStatus};
use jetcarp_core::media::{
    self, BatchReport, MediaFormat, MediaMode, MediaOptions, MediaRow, MediaStyle, MediaTarget,
    MissingFilter,
};
use jetcarp_core::website::{self, ExportFilter, ExportOptions, ExportReport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::PathBuf;

fn classes(v: &[String]) -> Result<Vec<PatternClass>, ApiError> {
    v.iter()
        .map(|c| PatternClass::parse(c).map_err(ApiError::invalid))
        .collect()
}

fn in_pool<T>(
    threads: Option<usize>,
    f: impl FnOnce() -> Result<T, ApiError> + Send,
) -> Result<T, ApiError>
where
    T: Send,
{
    match threads {
        Some(t) => rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .map_err(|e| ApiError::engine(format!("thread pool: {e}")))?
            .install(f),
        None => f(),
    }
}

// ---------------------------------------------------------------------------------------------
// media.render

/// What to render, and how. Give one of `pattern`, `quickstart`, `ant`, `scene`, or `missing`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaRenderIn {
    /// A catalog pattern: id, unique id prefix, or name.
    #[serde(default)]
    pub pattern: Option<String>,
    /// A Quick Start entry: one of the catalog's (`find:...`, its media stored in the catalog),
    /// or a built-in one by id (`life`) or number in `jetcarp quickstart`, rendered from its
    /// seed like `scene` (into `out`, default `<catalog folder>/media/scenes/<id>`).
    #[serde(default)]
    pub quickstart: Option<String>,
    /// An ant run (`ant-...`): a highway's moving loop alone, or the build-up from the seed.
    #[serde(default)]
    pub ant: Option<String>,
    /// Any scene (JSON: the object or its text), from its seed, into `out` (not the catalog).
    #[serde(default)]
    pub scene: Option<Value>,
    /// Every pattern, shown Quick Start entry, and notable ant run whose media are missing or
    /// stale.
    #[serde(default)]
    pub missing: bool,
    /// With `missing`: only patterns of these classes (spaceship, oscillator, gun, ...).
    #[serde(default)]
    pub class: Vec<String>,
    /// With `missing`: patterns only.
    #[serde(default)]
    pub only_patterns: bool,
    /// With `missing`: Quick Start entries only.
    #[serde(default)]
    pub only_quickstart: bool,
    /// With `missing`: at most this many targets (0: no limit).
    #[serde(default)]
    pub limit: usize,
    /// auto (default: patterns alone, everything else from the seed), alone, or seed.
    #[serde(default)]
    pub mode: Option<String>,
    /// auto (default: GIF up to 320x320 pixels and 256 colours, else WebP), gif, or webp.
    #[serde(default)]
    pub format: Option<String>,
    /// Alone: full periods shown (default 1).
    #[serde(default)]
    pub periods: Option<u64>,
    /// From the seed: generations (default 200; row rules one per row).
    #[serde(default)]
    pub gens: Option<u64>,
    /// From the seed: a frame every this many generations (default 1).
    #[serde(default)]
    pub every: Option<u64>,
    /// Write the per-step PNG frames (default true).
    #[serde(default)]
    pub frames: Option<bool>,
    /// Render even when up to date.
    #[serde(default)]
    pub force: bool,
    /// Colour the media with this palette instead of the house style: a built-in name or hex
    /// colours like `#103050-#4fc3ff` (part of the render key: media stored in another palette
    /// render again).
    #[serde(default)]
    pub palette: Option<String>,
    /// With `scene` or a built-in Quick Start entry: the output folder (default
    /// `<catalog folder>/media/scenes/<scene name>`).
    #[serde(default)]
    pub out: Option<String>,
    /// Worker threads (default: all cores; the files do not depend on it).
    #[serde(default)]
    pub threads: Option<usize>,
}

/// The keys [`MediaRenderIn::from_map`] reads.
pub const MEDIA_KEYS: &[&str] = &[
    "pattern",
    "quickstart",
    "ant",
    "missing",
    "class",
    "limit",
    "mode",
    "format",
    "periods",
    "gens",
    "every",
    "frames",
    "force",
    "palette",
];

impl MediaRenderIn {
    /// The options as the MCP tool and Rhai's `media(#{...})` give them: [`MEDIA_KEYS`], with
    /// numbers as text accepted and `class` as a list or comma-separated text.
    pub fn from_map(v: &Value) -> Result<MediaRenderIn, String> {
        check_keys(v, MEDIA_KEYS, "media")?;
        Ok(MediaRenderIn {
            mode: get_str(v, "mode")?.map(String::from),
            format: get_str(v, "format")?.map(String::from),
            periods: get_u64(v, "periods")?,
            gens: get_u64(v, "gens")?,
            every: get_u64(v, "every")?,
            frames: get_bool(v, "frames")?,
            force: get_bool(v, "force")?.unwrap_or(false),
            palette: get_str(v, "palette")?.map(String::from),
            missing: get_bool(v, "missing")?.unwrap_or(false),
            class: get_list(v, "class"),
            limit: get_u64(v, "limit")?.unwrap_or(0) as usize,
            pattern: get_str(v, "pattern")?.map(String::from),
            quickstart: get_str(v, "quickstart")?.map(String::from),
            ant: get_str(v, "ant")?.map(String::from),
            ..Default::default()
        })
    }

    /// The render options (checked: mode, format, palette).
    pub fn media_options(&self) -> Result<MediaOptions, ApiError> {
        Ok(MediaOptions {
            mode: MediaMode::parse(self.mode.as_deref().unwrap_or("auto"))
                .map_err(ApiError::invalid)?,
            format: MediaFormat::parse(self.format.as_deref().unwrap_or("auto"))
                .map_err(ApiError::invalid)?,
            periods: self.periods.unwrap_or(1).max(1),
            gens: self.gens,
            every: self.every.unwrap_or(1).max(1),
            frames: self.frames.unwrap_or(true),
            force: self.force,
            style: MediaStyle::with_palette(self.palette.as_deref()).map_err(ApiError::invalid)?,
        })
    }
}

/// One target's media.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TargetMedia {
    /// `pattern`, `quickstart`, or `ant`.
    pub kind: String,
    pub id: String,
    /// `alone` or `seed`.
    pub mode: String,
    /// False when the stored media were already up to date.
    pub written: bool,
    /// The files by kind (`anim`, `poster`, `thumb`, `frames`), with `width` and `height`.
    #[schemars(with = "Value")]
    pub files: Value,
}

/// A target that could not be rendered.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TargetFailed {
    pub kind: String,
    pub id: String,
    pub error: String,
}

/// What was rendered.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct MediaReport {
    /// Targets rendered.
    pub written: usize,
    /// Targets already up to date.
    pub skipped: usize,
    pub done: Vec<TargetMedia>,
    pub failed: Vec<TargetFailed>,
    /// A scene's output folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// A scene's files by kind, with `width` and `height`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub files: Option<Value>,
    /// A scene's animation frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames_count: Option<u32>,
    /// The catalog's report, as `media::render_many` gave it.
    #[serde(skip)]
    pub report: Option<BatchReport>,
    /// A scene's files.
    #[serde(skip)]
    pub rows: Vec<MediaRow>,
}

/// Media files by kind (`anim` for the GIF or WebP), with the first one's size.
pub fn files_by_kind(rows: &[MediaRow]) -> Value {
    let mut m = serde_json::Map::new();
    for r in rows {
        let k = match r.kind.as_str() {
            "gif" | "webp" => "anim",
            k => k,
        };
        m.insert(k.into(), json!(r.abs_path.display().to_string()));
    }
    m.insert(
        "width".into(),
        json!(rows.first().map(|r| r.width).unwrap_or(0)),
    );
    m.insert(
        "height".into(),
        json!(rows.first().map(|r| r.height).unwrap_or(0)),
    );
    Value::Object(m)
}

impl MediaReport {
    fn of(r: BatchReport) -> MediaReport {
        MediaReport {
            written: r.written(),
            skipped: r.skipped(),
            done: r
                .done
                .iter()
                .map(|d| TargetMedia {
                    kind: d.target_kind.clone(),
                    id: d.target_id.clone(),
                    mode: d.mode.name().to_string(),
                    written: d.written,
                    files: files_by_kind(&d.rows),
                })
                .collect(),
            failed: r
                .failed
                .iter()
                .map(|(k, id, x)| TargetFailed {
                    kind: k.clone(),
                    id: id.clone(),
                    error: x.clone(),
                })
                .collect(),
            folder: None,
            files: None,
            frames_count: None,
            report: Some(r),
            rows: Vec::new(),
        }
    }
}

impl Report for MediaReport {
    fn text(&self) -> String {
        let mut s = String::new();
        let Some(r) = &self.report else {
            for r in &self.rows {
                let _ = writeln!(s, "{:<6} {}", r.kind, r.abs_path.display());
            }
            return s;
        };
        for d in &r.done {
            let what = if d.written { "rendered" } else { "up to date" };
            let anim = d
                .animation()
                .map(|a| {
                    format!(
                        "{} ({} x {}, {} frames, {} bytes)",
                        a.abs_path.display(),
                        a.width,
                        a.height,
                        a.frames,
                        a.bytes
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(
                s,
                "{} {} [{}] {what}: {anim}",
                d.target_kind,
                d.target_id,
                d.mode.name()
            );
        }
        s
    }
    fn notes(&self) -> String {
        let Some(r) = &self.report else {
            return String::new();
        };
        let mut s = String::new();
        for (k, id, e) in &r.failed {
            let _ = writeln!(s, "{k} {id}: {e}");
        }
        let _ = writeln!(
            s,
            "{} rendered, {} up to date, {} failed",
            r.written(),
            r.skipped(),
            r.failed.len()
        );
        s
    }
    fn check(&self) -> Result<(), ApiError> {
        match &self.report {
            Some(r) if !r.failed.is_empty() && r.done.is_empty() => {
                Err(ApiError::engine("nothing rendered"))
            }
            _ => Ok(()),
        }
    }
}

/// `media.render`.
pub struct MediaRender;
impl Op for MediaRender {
    const NAME: &'static str = "media.render";
    const SUMMARY: &'static str = "Render frames, an animation (GIF or WebP), a poster, and a thumbnail for a catalog pattern (alone), a Quick Start entry or a scene (from the seed), or an ant run; up-to-date media are kept.";
    const READ_ONLY: bool = false;
    type In = MediaRenderIn;
    type Out = MediaReport;
    fn run(cx: &Ctx, a: MediaRenderIn, _: &dyn Progress) -> Result<MediaReport, ApiError> {
        let threads = a.threads;
        in_pool(threads, || render(cx, a))
    }
}

fn render(cx: &Ctx, a: MediaRenderIn) -> Result<MediaReport, ApiError> {
    let o = a.media_options()?;
    let lib = cx.library();
    if a.missing {
        let cls = classes(&a.class)?;
        let filter = MissingFilter {
            patterns: !a.only_quickstart,
            quickstart: !a.only_patterns && cls.is_empty(),
            ants: !a.only_patterns && !a.only_quickstart && cls.is_empty(),
            classes: cls,
            limit: a.limit,
        };
        let r = media::render_missing(&lib, &filter, &o).map_err(catalog_err)?;
        return Ok(MediaReport::of(r));
    }
    // A scene, or a built-in Quick Start entry (not in the catalog: its media go in a folder,
    // as a scene's do).
    let scene = match (&a.scene, &a.quickstart) {
        (Some(v), _) => {
            let sc = read_scene(cx, Some(v), None)?;
            let name = if sc.name.is_empty() {
                "scene".to_string()
            } else {
                sc.name.clone()
            };
            Some((sc, name))
        }
        (None, Some(q)) => builtin_entry(&lib, q)?.map(|e| (e.scene, e.id)),
        (None, None) => None,
    };
    if let Some((sc, name)) = scene {
        let dir = match &a.out {
            Some(d) => cx.fs.writable_dir(d)?,
            None => {
                let d = lib
                    .folder()
                    .join("media")
                    .join("scenes")
                    .join(crate::out::sanitize(&name));
                cx.fs.writable_dir(&d.display().to_string())?
            }
        };
        let rows = media::render_scene_to(&sc, &dir, &o).map_err(ApiError::engine)?;
        return Ok(MediaReport {
            written: 1,
            skipped: 0,
            done: Vec::new(),
            failed: Vec::new(),
            folder: Some(dir),
            files: Some(files_by_kind(&rows)),
            frames_count: Some(rows.first().map(|r| r.frames).unwrap_or(0)),
            report: None,
            rows,
        });
    }
    let t = if let Some(p) = &a.pattern {
        MediaTarget::Pattern(p.clone())
    } else if let Some(q) = &a.quickstart {
        MediaTarget::QuickStart(q.clone())
    } else if let Some(x) = &a.ant {
        MediaTarget::Ant(x.clone())
    } else {
        return Err(ApiError::invalid(
            "media: give pattern, quickstart, ant, or missing: true",
        ));
    };
    let r = media::render_many(&lib, &[t], &o).map_err(catalog_err)?;
    Ok(MediaReport::of(r))
}

/// The built-in Quick Start entry `q` names (its id, or its number in `jetcarp quickstart`), if
/// it is not one of the catalog's own (`find:...`, or an id the catalog has), which come first.
/// None for a catalog entry; an error if it is neither.
fn builtin_entry(
    lib: &jetcarp_core::library::Library,
    q: &str,
) -> Result<Option<jetcarp_core::quickstart::QuickStart>, ApiError> {
    let q = q.trim();
    if q.starts_with("find:") || lib.get_quickstart(q).is_ok() {
        return Ok(None);
    }
    let all = jetcarp_core::quickstart::entries();
    let by_number = q.parse::<usize>().ok().and_then(|k| all.get(k));
    match all.iter().find(|e| e.id == q).or(by_number) {
        Some(e) => Ok(Some(e.clone())),
        None => Err(ApiError::not_found(format!(
            "no Quick Start entry '{q}': neither a built-in one (`jetcarp quickstart` lists their \
             ids and numbers) nor one of the catalog's (`jetcarp library quickstart`)"
        ))),
    }
}

// ---------------------------------------------------------------------------------------------
// media.list

/// Whose media to list.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaListIn {
    /// `pattern`, `quickstart`, or `ant`.
    pub kind: String,
    /// The target's id (a pattern's full id).
    pub id: String,
}

/// A target's stored media files.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct MediaList {
    pub kind: String,
    pub id: String,
    #[schemars(with = "Vec<Value>")]
    pub media: Vec<MediaRow>,
}

impl Report for MediaList {
    fn text(&self) -> String {
        if self.media.is_empty() {
            return format!("no media for {} {}\n", self.kind, self.id);
        }
        let mut s = String::new();
        for r in &self.media {
            let _ = writeln!(
                s,
                "{:<6} {:<5} {:>4} x {:<4} {:>4} frames {:>9} bytes  {}",
                r.kind,
                if r.from_seed { "seed" } else { "alone" },
                r.width,
                r.height,
                r.frames,
                r.bytes,
                r.abs_path.display()
            );
        }
        s
    }
}

/// `media.list`.
pub struct MediaListOp;
impl Op for MediaListOp {
    const NAME: &'static str = "media.list";
    const SUMMARY: &'static str =
        "A catalog target's stored media files (kind, size, frames, bytes, path).";
    const READ_ONLY: bool = true;
    type In = MediaListIn;
    type Out = MediaList;
    fn run(cx: &Ctx, a: MediaListIn, _: &dyn Progress) -> Result<MediaList, ApiError> {
        let media = media::stored(&cx.library(), &a.kind, &a.id).map_err(catalog_err)?;
        Ok(MediaList {
            kind: a.kind,
            id: a.id,
            media,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// site.export

fn yes() -> bool {
    true
}

/// What to export, and where.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteIn {
    /// The export folder.
    pub to: String,
    /// Pattern curation states exported (default promoted and reviewed; rejected never).
    #[serde(default)]
    pub state: Option<Vec<String>>,
    /// Only patterns with at least this interest score.
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Only patterns rated at least this (1-5).
    #[serde(default)]
    pub min_rating: Option<u8>,
    /// Leave out oscillators with a shorter period than this.
    #[serde(default)]
    pub min_osc_period: Option<u64>,
    /// Only patterns of these classes.
    #[serde(default)]
    pub class: Vec<String>,
    /// Quick Start statuses exported (default auto and approved; empty: none).
    #[serde(default)]
    pub quickstart: Option<Vec<String>>,
    /// Ant and turmite curation states exported (default promoted and reviewed; empty: none).
    #[serde(default)]
    pub ants: Option<Vec<String>>,
    /// Only ants with these outcomes (highway, symmetric, ...; default all).
    #[serde(default)]
    pub ant_outcome: Vec<String>,
    /// Copy the per-step PNG frames too.
    #[serde(default)]
    pub frames: bool,
    /// Render missing media first (default true).
    #[serde(default = "yes")]
    pub render: bool,
    /// Render missing media in this built-in palette instead of the house style.
    #[serde(default)]
    pub palette: Option<String>,
    /// Write the viewer page, index.html (a grid of every item over manifest.json; default
    /// true).
    #[serde(default = "yes")]
    pub page: bool,
}

/// The keys [`SiteIn::from_map`] reads.
pub const SITE_KEYS: &[&str] = &[
    "to",
    "state",
    "min_score",
    "min_rating",
    "min_osc_period",
    "class",
    "quickstart",
    "frames",
    "render",
    "palette",
];

impl SiteIn {
    /// The options as the MCP tool and Rhai's `lib.site_export(#{...})` give them
    /// ([`SITE_KEYS`]; no viewer page); `to` is the folder already checked.
    pub fn from_map(v: &Value, to: &str) -> Result<SiteIn, String> {
        check_keys(v, SITE_KEYS, "site_export")?;
        let state = get_list(v, "state");
        Ok(SiteIn {
            to: to.to_string(),
            state: (!state.is_empty()).then_some(state),
            min_score: get_f64(v, "min_score")?,
            min_rating: get_u64(v, "min_rating")?.map(|x| x.min(5) as u8),
            min_osc_period: get_u64(v, "min_osc_period")?,
            class: get_list(v, "class"),
            quickstart: v.get("quickstart").is_some().then(|| {
                get_list(v, "quickstart")
                    .into_iter()
                    .filter(|s| s.as_str() != "none")
                    .collect()
            }),
            ants: None,
            ant_outcome: Vec::new(),
            frames: get_bool(v, "frames")?.unwrap_or(false),
            render: get_bool(v, "render")?.unwrap_or(true),
            palette: get_str(v, "palette")?.map(String::from),
            page: false,
        })
    }
}

/// What the export wrote.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct SiteReport(#[schemars(with = "Value")] pub ExportReport);

impl Report for SiteReport {
    fn text(&self) -> String {
        let mut s = String::new();
        for p in &self.0.written {
            let _ = writeln!(s, "wrote   {p}");
        }
        for p in &self.0.deleted {
            let _ = writeln!(s, "deleted {p}");
        }
        s
    }
    fn notes(&self) -> String {
        let r = &self.0;
        let mut s = String::new();
        for (k, id, x) in &r.media_failed {
            let _ = writeln!(s, "no media for {k} {id}: {x}");
        }
        let _ = writeln!(
            s,
            "{}: {} patterns, {} Quick Start entries; {} files written, {} deleted, {} items unchanged, {} media rendered",
            r.dir.display(),
            r.patterns,
            r.quickstart,
            r.written.len(),
            r.deleted.len(),
            r.unchanged,
            r.rendered
        );
        s
    }
}

/// `site.export`.
pub struct SiteExport;
impl Op for SiteExport {
    const NAME: &'static str = "site.export";
    const SUMMARY: &'static str = "Write the public part of the catalog (JSON and media, with manifest.json and a viewer page) to a folder for a static website; only new or changed files are written, and items no longer public are deleted.";
    const READ_ONLY: bool = false;
    type In = SiteIn;
    type Out = SiteReport;
    fn run(cx: &Ctx, a: SiteIn, _: &dyn Progress) -> Result<SiteReport, ApiError> {
        let mut f = ExportFilter::default();
        if let Some(st) = &a.state {
            f.states = st
                .iter()
                .map(|x| CurationState::parse(x).map_err(ApiError::invalid))
                .collect::<Result<_, _>>()?;
        }
        if let Some(q) = &a.quickstart {
            f.quickstart = q
                .iter()
                .map(|x| QsStatus::parse(x).map_err(ApiError::invalid))
                .collect::<Result<_, _>>()?;
        }
        f.min_score = a.min_score;
        f.min_rating = a.min_rating;
        f.min_osc_period = a.min_osc_period;
        f.classes = classes(&a.class)?;
        if let Some(s) = &a.ants {
            f.ant_states = s.clone();
        }
        f.ant_outcomes = a.ant_outcome.clone();
        let o = ExportOptions {
            filter: f,
            frames: a.frames,
            render: a.render,
            media: MediaOptions {
                style: MediaStyle::with_palette(a.palette.as_deref()).map_err(ApiError::invalid)?,
                ..Default::default()
            },
        };
        let dir = cx.fs.writable_dir(&a.to)?;
        let mut r = website::export(&cx.library(), &dir, &o).map_err(catalog_err)?;
        if a.page && website::write_page(&dir).map_err(ApiError::io)? {
            r.written.push("index.html".into());
        }
        Ok(SiteReport(r))
    }
}
