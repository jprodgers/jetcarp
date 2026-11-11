//! `jetcarp media` (frames, animations, posters, thumbnails for catalog patterns, Quick Start
//! entries, ant runs, and scenes) and `jetcarp site` (incremental static export for a website).
//! The work is in the operations `media.render`, `media.list`, and `site.export`.

use crate::show;
use anyhow::{Result, bail};
use jetcarp_api::op::{Op, Quiet, Report};
use jetcarp_api::ops::media::{
    MediaListIn, MediaListOp, MediaRender, MediaRenderIn, SiteExport, SiteIn,
};
use jetcarp_store::Library;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct MediaArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite). Media files go under its folder.
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: MediaCmd,
}

#[derive(clap::Subcommand)]
enum MediaCmd {
    /// Render frames, an animation, a poster, and a thumbnail (skipped when up to date).
    Render(Box<RenderArgs>),
    /// List a target's stored media files.
    List {
        /// `pattern`, `quickstart`, or `ant`.
        kind: String,
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
struct RenderArgs {
    /// A catalog pattern (id, unique prefix, or name).
    #[arg(long)]
    pattern: Option<String>,
    /// A Quick Start entry: a built-in one by id or number (`jetcarp quickstart` lists them),
    /// written like --scene, or one of the catalog's (`find:...`, `jetcarp library quickstart`).
    #[arg(long)]
    quickstart: Option<String>,
    /// An ant run by id (`jetcarp library ants list`): a highway's moving loop alone, the build-up from the
    /// seed.
    #[arg(long)]
    ant: Option<String>,
    /// Any scene file (from the seed); written to --out, not the catalog.
    #[arg(long)]
    scene: Option<PathBuf>,
    /// Every pattern and shown Quick Start entry whose media are missing or stale.
    #[arg(long)]
    missing: bool,
    /// With --missing: only these classes (comma-separated: spaceship,oscillator,gun,...).
    #[arg(long)]
    class: Option<String>,
    /// With --missing: patterns only, or Quick Start entries only.
    #[arg(long, conflicts_with = "only_quickstart")]
    only_patterns: bool,
    #[arg(long)]
    only_quickstart: bool,
    /// With --missing: at most this many targets.
    #[arg(long, default_value_t = 0)]
    limit: usize,
    /// auto (patterns alone, everything else from the seed), alone, or seed.
    #[arg(long, default_value = "auto")]
    mode: String,
    /// auto (GIF up to 320 x 320 pixels and 256 colours, else WebP), gif, or webp.
    #[arg(long, default_value = "auto")]
    format: String,
    /// Alone: full periods to show.
    #[arg(long, default_value_t = 1)]
    periods: u64,
    /// From the seed: generations (default 200; row rules one per row).
    #[arg(long)]
    gens: Option<u64>,
    /// From the seed: a frame every this many generations.
    #[arg(long, default_value_t = 1)]
    every: u64,
    /// Skip the per-step PNG frames.
    #[arg(long)]
    no_frames: bool,
    /// Render even when up to date.
    #[arg(long)]
    force: bool,
    /// Colour the media with this palette (its background, stage, and stops) instead of the
    /// house style: a name (`jetcarp palettes` lists them) or hex colours like `#103050-#4fc3ff`.
    /// The palette is part of the render key, so media stored in another palette render again.
    #[arg(long)]
    palette: Option<String>,
    /// --scene or a built-in --quickstart entry: output folder (default
    /// `<catalog folder>/media/scenes/<scene file's name, or the entry's id>`).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Worker threads (default: all cores).
    #[arg(long)]
    threads: Option<usize>,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

/// A comma-separated list.
fn split(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

/// A comma-separated list ("none" or empty: none).
fn list(s: &str) -> Vec<String> {
    if s.trim() == "none" {
        return Vec::new();
    }
    split(s)
}

pub fn run(a: MediaArgs, json: bool) -> Result<()> {
    let cx = crate::platform::ctx().with_library(a.library.clone());
    match a.cmd {
        MediaCmd::List { kind, id, json: j } => {
            let out = MediaListOp::run(&cx, MediaListIn { kind, id }, &Quiet)?;
            if json || j {
                // The rows, as they have always printed.
                println!("{}", serde_json::to_string_pretty(&out.media)?);
                return Ok(());
            }
            show(&out, false)
        }
        MediaCmd::Render(r) => render(&cx, *r, json),
    }
}

/// `abs` as the user would write it: under `given` (relative) instead of `dir` (absolute).
fn as_given(abs: &Path, dir: &Path, given: &Path) -> PathBuf {
    match abs.strip_prefix(dir) {
        Ok(rest) if given.is_relative() => given.join(rest),
        _ => abs.to_path_buf(),
    }
}

fn render(cx: &jetcarp_api::Ctx, a: RenderArgs, json: bool) -> Result<()> {
    let json = json || a.json;
    let picked = [
        a.pattern.is_some(),
        a.quickstart.is_some(),
        a.ant.is_some(),
        a.scene.is_some(),
        a.missing,
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if picked != 1 {
        bail!("give one of --pattern ID, --quickstart ID, --ant ID, --scene FILE, --missing");
    }
    let mut input = MediaRenderIn {
        pattern: a.pattern,
        // A built-in entry is written like a scene, to --out if given.
        out: a
            .quickstart
            .as_ref()
            .and(a.out.as_ref())
            .map(|d| d.display().to_string()),
        quickstart: a.quickstart,
        ant: a.ant,
        missing: a.missing,
        class: a.class.as_deref().map(split).unwrap_or_default(),
        only_patterns: a.only_patterns,
        only_quickstart: a.only_quickstart,
        limit: a.limit,
        mode: Some(a.mode),
        format: Some(a.format),
        periods: Some(a.periods),
        gens: a.gens,
        every: Some(a.every),
        frames: Some(!a.no_frames),
        force: a.force,
        palette: a.palette,
        threads: a.threads,
        ..Default::default()
    };
    let mut out_dir = None;
    if let Some(path) = &a.scene {
        let text = std::fs::read_to_string(path)?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "scene".into());
        let dir = a.out.clone().unwrap_or_else(|| {
            Library::default_path()
                .parent()
                .map(|p| p.join("media").join("scenes").join(&stem))
                .unwrap_or_else(|| PathBuf::from(&stem))
        });
        input.scene = Some(serde_json::Value::String(text));
        input.out = Some(dir.display().to_string());
        out_dir = Some(dir);
    }
    let out = MediaRender::run(cx, input, &Quiet)?;
    if let Some(dir) = &out.folder {
        // A scene or a built-in Quick Start entry: its files, as they have always printed.
        if json {
            println!("{}", serde_json::to_string_pretty(&out.rows)?);
        } else {
            for r in &out.rows {
                let shown = match &out_dir {
                    Some(given) => as_given(&r.abs_path, dir, given),
                    None => r.abs_path.clone(),
                };
                println!("{:<6} {}", r.kind, shown.display());
            }
        }
        return Ok(());
    }
    if json {
        // The catalog's report, as it has always printed.
        if let Some(r) = &out.report {
            println!("{}", serde_json::to_string_pretty(r)?);
        }
        return out.check().map_err(anyhow::Error::new);
    }
    show(&out, false)
}

#[derive(clap::Args)]
pub struct SiteArgs {
    /// Catalog (as `jetcarp library --library`).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: SiteCmd,
}

#[derive(clap::Subcommand)]
enum SiteCmd {
    /// Write the public part of the catalog (JSON and media, with manifest.json, and a viewer
    /// page, index.html) to a folder; only new or changed files are written, and items no longer
    /// public are deleted.
    Export(Box<ExportArgs>),
}

#[derive(clap::Args)]
struct ExportArgs {
    /// The export folder.
    #[arg(long)]
    to: PathBuf,
    /// Curation states exported (comma-separated; default promoted,reviewed; never rejected).
    #[arg(long, default_value = "promoted,reviewed")]
    state: String,
    /// Only patterns with at least this interest score.
    #[arg(long)]
    min_score: Option<f64>,
    /// Only patterns rated at least this (1-5).
    #[arg(long)]
    min_rating: Option<u8>,
    /// Leave out oscillators with a shorter period than this.
    #[arg(long)]
    min_osc_period: Option<u64>,
    /// Only these classes (comma-separated).
    #[arg(long)]
    class: Option<String>,
    /// Quick Start statuses exported (comma-separated; default auto,approved; "none" for none).
    #[arg(long, default_value = "auto,approved")]
    quickstart: String,
    /// Ant and turmite curation states exported (comma-separated; default promoted,reviewed;
    /// "none" for none).
    #[arg(long, default_value = "promoted,reviewed")]
    ants: String,
    /// Only ants with these outcomes (comma-separated: highway, symmetric, ...; default all).
    #[arg(long)]
    ant_outcome: Option<String>,
    /// Copy the per-step PNG frames too.
    #[arg(long)]
    frames: bool,
    /// Do not render missing media first.
    #[arg(long)]
    no_render: bool,
    /// Render missing media in this palette (a name or hex colours) instead of the house style.
    #[arg(long)]
    palette: Option<String>,
    /// Do not write the viewer page (index.html, a grid of every item over manifest.json).
    #[arg(long)]
    no_page: bool,
    #[arg(long)]
    json: bool,
}

pub fn run_site(a: SiteArgs, json: bool) -> Result<()> {
    let cx = crate::platform::ctx().with_library(a.library.clone());
    match a.cmd {
        SiteCmd::Export(e) => {
            let input = SiteIn {
                to: e.to.display().to_string(),
                state: Some(split(&e.state)),
                min_score: e.min_score,
                min_rating: e.min_rating,
                min_osc_period: e.min_osc_period,
                class: e.class.as_deref().map(split).unwrap_or_default(),
                quickstart: Some(if e.quickstart.trim() == "none" {
                    Vec::new()
                } else {
                    split(&e.quickstart)
                }),
                ants: Some(list(&e.ants)),
                ant_outcome: e.ant_outcome.as_deref().map(list).unwrap_or_default(),
                frames: e.frames,
                render: !e.no_render,
                palette: e.palette,
                page: !e.no_page,
            };
            let mut out = SiteExport::run(&cx, input, &Quiet)?;
            // The folder as given.
            out.0.dir = e.to.clone();
            if json || e.json {
                // In the report's own key order, as it has always printed.
                println!("{}", serde_json::to_string_pretty(&out.0)?);
                return Ok(());
            }
            show(&out, false)
        }
    }
}
