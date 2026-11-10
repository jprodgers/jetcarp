//! `jetcarp media` (frames, animations, posters, thumbnails for catalog patterns, Quick Start
//! entries, settings, and scenes) and `jetcarp site` (incremental static export for a website).
//! The work is in `jetcarp_core::media` and `jetcarp_core::website`.

use anyhow::{Result, bail};
use jetcarp_core::library::{CurationState, Library, PatternClass, QsStatus};
use jetcarp_core::media::{
    self, BatchReport, MediaFormat, MediaMode, MediaOptions, MediaTarget, MissingFilter,
};
use jetcarp_core::website::{self, ExportFilter, ExportOptions};
use jetcarp_core::world::Scene;
use std::path::PathBuf;

fn open(p: &Option<PathBuf>) -> Library {
    match p {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    }
}

fn err(e: String) -> anyhow::Error {
    anyhow::Error::msg(e)
}

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
        /// `pattern`, `quickstart`, or `setting`.
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
    /// A Quick Start entry by id.
    #[arg(long)]
    quickstart: Option<String>,
    /// A stage 1 setting by id (rendered from its Quick Start entry's scene).
    #[arg(long)]
    setting: Option<String>,
    /// An ant run by id (`jetcarp ants list`): a highway's moving loop alone, the build-up from the
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
    /// Colour the media with this built-in palette (its background, stage, and stops) instead
    /// of the house style (`jetcarp palettes` lists them). The palette is part of the render key,
    /// so media stored in another palette render again.
    #[arg(long)]
    palette: Option<String>,
    /// --scene: output folder (default <Pictures>/JETCARP/media/<scene name>).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Worker threads (default: all cores).
    #[arg(long)]
    threads: Option<usize>,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

fn classes(s: &Option<String>) -> Result<Vec<PatternClass>> {
    s.as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| PatternClass::parse(x).map_err(err))
        .collect()
}

fn print_report(r: &BatchReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(r)?);
        return Ok(());
    }
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
        println!(
            "{} {} [{}] {what}: {anim}",
            d.target_kind,
            d.target_id,
            d.mode.name()
        );
    }
    for (k, id, e) in &r.failed {
        eprintln!("{k} {id}: {e}");
    }
    eprintln!(
        "{} rendered, {} up to date, {} failed",
        r.written(),
        r.skipped(),
        r.failed.len()
    );
    Ok(())
}

pub fn run(a: MediaArgs) -> Result<()> {
    let lib = open(&a.library);
    match a.cmd {
        MediaCmd::List { kind, id, json } => {
            let rows = media::stored(&lib, &kind, &id).map_err(err)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if rows.is_empty() {
                println!("no media for {kind} {id}");
            } else {
                for r in rows {
                    println!(
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
            }
            Ok(())
        }
        MediaCmd::Render(r) => render(&lib, *r),
    }
}

fn render(lib: &Library, a: RenderArgs) -> Result<()> {
    if let Some(t) = a.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .ok();
    }
    let o = MediaOptions {
        mode: MediaMode::parse(&a.mode).map_err(err)?,
        format: MediaFormat::parse(&a.format).map_err(err)?,
        periods: a.periods.max(1),
        gens: a.gens,
        every: a.every.max(1),
        frames: !a.no_frames,
        force: a.force,
        style: media::MediaStyle::with_palette(a.palette.as_deref()).map_err(err)?,
    };
    let picked = [
        a.pattern.is_some(),
        a.quickstart.is_some(),
        a.setting.is_some(),
        a.ant.is_some(),
        a.scene.is_some(),
        a.missing,
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if picked != 1 {
        bail!(
            "give one of --pattern ID, --quickstart ID, --setting ID, --ant ID, --scene FILE, --missing"
        );
    }
    if let Some(path) = &a.scene {
        let text = std::fs::read_to_string(path)?;
        let sc = Scene::from_json(&text).map_err(err)?;
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
        let rows = media::render_scene_to(&sc, &dir, &o).map_err(err)?;
        if a.json {
            println!("{}", serde_json::to_string_pretty(&rows)?);
        } else {
            for r in rows {
                println!("{:<6} {}", r.kind, r.abs_path.display());
            }
        }
        return Ok(());
    }
    let report = if a.missing {
        let filter = MissingFilter {
            patterns: !a.only_quickstart,
            classes: classes(&a.class)?,
            quickstart: !a.only_patterns && a.class.is_none(),
            ants: !a.only_patterns && !a.only_quickstart && a.class.is_none(),
            limit: a.limit,
        };
        media::render_missing(lib, &filter, &o).map_err(err)?
    } else {
        let t = if let Some(p) = a.pattern {
            MediaTarget::Pattern(p)
        } else if let Some(q) = a.quickstart {
            MediaTarget::QuickStart(q)
        } else if let Some(x) = a.ant {
            MediaTarget::Ant(x)
        } else {
            MediaTarget::Setting(a.setting.unwrap())
        };
        media::render_many(lib, &[t], &o).map_err(err)?
    };
    print_report(&report, a.json)?;
    if !report.failed.is_empty() && report.done.is_empty() {
        bail!("nothing rendered");
    }
    Ok(())
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
    /// Write the public part of the catalog (JSON and media, with manifest.json) to a folder;
    /// only new or changed files are written, and items no longer public are deleted.
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
    /// Render missing media in this built-in palette instead of the house style.
    #[arg(long)]
    palette: Option<String>,
    #[arg(long)]
    json: bool,
}

/// A comma-separated list ("none" or empty: none).
fn list(s: &str) -> Vec<String> {
    if s.trim() == "none" {
        return Vec::new();
    }
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

pub fn run_site(a: SiteArgs) -> Result<()> {
    let lib = open(&a.library);
    match a.cmd {
        SiteCmd::Export(e) => {
            let states = e
                .state
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| CurationState::parse(x).map_err(err))
                .collect::<Result<Vec<_>>>()?;
            let quickstart = if e.quickstart.trim() == "none" {
                Vec::new()
            } else {
                e.quickstart
                    .split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(|x| QsStatus::parse(x).map_err(err))
                    .collect::<Result<Vec<_>>>()?
            };
            let o = ExportOptions {
                filter: ExportFilter {
                    states,
                    min_score: e.min_score,
                    min_rating: e.min_rating,
                    classes: classes(&e.class)?,
                    min_osc_period: e.min_osc_period,
                    ids: Vec::new(),
                    quickstart,
                    ant_states: list(&e.ants),
                    ant_outcomes: e.ant_outcome.as_deref().map(list).unwrap_or_default(),
                },
                frames: e.frames,
                render: !e.no_render,
                media: MediaOptions {
                    style: media::MediaStyle::with_palette(e.palette.as_deref()).map_err(err)?,
                    ..Default::default()
                },
            };
            let r = website::export(&lib, &e.to, &o).map_err(err)?;
            if e.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                for p in &r.written {
                    println!("wrote   {p}");
                }
                for p in &r.deleted {
                    println!("deleted {p}");
                }
                for (k, id, x) in &r.media_failed {
                    eprintln!("no media for {k} {id}: {x}");
                }
                eprintln!(
                    "{}: {} patterns, {} Quick Start entries; {} files written, {} deleted, {} items unchanged, {} media rendered",
                    r.dir.display(),
                    r.patterns,
                    r.quickstart,
                    r.written.len(),
                    r.deleted.len(),
                    r.unchanged,
                    r.rendered
                );
            }
            Ok(())
        }
    }
}
