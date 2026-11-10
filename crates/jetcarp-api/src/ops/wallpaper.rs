//! `wallpaper.export`: desktop-sized PNGs of a world, for one monitor, a span of monitors, or one
//! image per monitor cut so they line up. The world is a scene file or Quick Start entry
//! (`source`, as `jetcarp wallpaper` gives it), or the host's world (the MCP tool): recolouring
//! that one is a logged command, and `fresh` leaves it alone.

use super::quickstart::scene_source;
use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Progress, Report, WorldOp};
use crate::world::Host;
use jetcarp_core::wallpaper::{self, Fit};
use jetcarp_core::{CaChange, Command, Topology, World};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn cell_px_default() -> f64 {
    4.0
}

/// What to draw, and where.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallpaperIn {
    /// A scene file, or a Quick Start id (`life`, its number in `jetcarp quickstart`, or
    /// `find:...`). Default: the host's world.
    #[serde(default)]
    pub source: Option<String>,
    /// WIDTHxHEIGHT or a preset: HD, WUXGA, QHD, WQXGA, UWQHD, 4K, 5K, 8K, Phone (default
    /// 3840x2160). Ignored with `monitors`.
    #[serde(default)]
    pub size: Option<String>,
    /// cover (default: fill and crop, no stage showing), contain (the whole grid, letterboxed),
    /// or repeat (a wrap-edge grid tiled at `cell_px`).
    #[serde(default)]
    pub fit: Option<String>,
    /// World point at the image centre (with `scale`) instead of fitting.
    #[serde(default)]
    pub center: Option<[f64; 2]>,
    /// Pixels per world unit (with `center`).
    #[serde(default)]
    pub scale: Option<f64>,
    /// Tile size in pixels for `fresh` and `fit` repeat (default 4).
    #[serde(default = "cell_px_default")]
    pub cell_px: f64,
    /// Render a new grid of the world's tiling sized to fill the image at `cell_px` (its CAs and
    /// seeds, without its edits).
    #[serde(default)]
    pub fresh: bool,
    /// Generations to run first (on the host's world: only with `fresh`).
    #[serde(default)]
    pub gens: Option<u64>,
    /// Step on the GPU where the rules allow it (results are identical).
    #[serde(default)]
    pub gpu: bool,
    /// Colour every CA with this palette first.
    #[serde(default)]
    pub palette: Option<String>,
    /// Monitors as WIDTHxHEIGHT+X+Y, comma separated (xrandr's layout): the image spans them.
    #[serde(default)]
    pub monitors: Option<String>,
    /// With `monitors`: one image per monitor instead of the whole span.
    #[serde(default)]
    pub split: bool,
    /// Draw tile outlines when tiles are at least this many pixels across (default 0: never).
    #[serde(default)]
    pub outline: f64,
    /// Output .png file or folder (default <Pictures>/JETCARP/wallpapers/).
    #[serde(default)]
    pub out: Option<String>,
}

/// One image written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WallpaperFile {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// With `split`: the monitor it is for (WIDTHxHEIGHT+X+Y).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
}

/// The images written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WallpaperOut {
    pub files: Vec<WallpaperFile>,
    #[schemars(with = "Value")]
    pub fit: Fit,
    /// The generation and state hash of the world drawn.
    pub generation: u64,
    pub hash: String,
    /// The paths as the user wrote the output folder (the command line prints these).
    #[serde(skip)]
    shown: Vec<PathBuf>,
}

impl Report for WallpaperOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for (f, p) in self.files.iter().zip(&self.shown) {
            let _ = writeln!(s, "{} ({} x {})", p.display(), f.width, f.height);
        }
        s
    }
}

/// Every CA of `w` recoloured with palette `p`.
fn recolour(w: &World, p: &str) -> Vec<Command> {
    w.cas
        .iter()
        .map(|c| Command::UpdateCa {
            id: c.id,
            change: CaChange {
                palette: Some(p.to_string()),
                ..Default::default()
            },
        })
        .collect()
}

/// `wallpaper.export`.
pub struct WallpaperExport;
impl WorldOp for WallpaperExport {
    const NAME: &'static str = "wallpaper.export";
    const SUMMARY: &'static str = "Desktop-sized PNGs of a scene, a Quick Start entry, or the current world: one monitor, a span, or one image per monitor cut so they line up.";
    const READ_ONLY: bool = false;
    type In = WallpaperIn;
    type Out = WallpaperOut;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        a: WallpaperIn,
        _: &dyn Progress,
    ) -> Result<WallpaperOut, ApiError> {
        let monitors = a
            .monitors
            .as_deref()
            .map(wallpaper::parse_monitors)
            .transpose()
            .kind(ErrorKind::Invalid)?;
        if a.split && monitors.is_none() {
            return Err(ApiError::invalid("`split` needs `monitors`"));
        }
        let (width, height) = match &monitors {
            Some(m) => {
                let (_, _, w, h) = wallpaper::span_box(m);
                (w, h)
            }
            None => wallpaper::parse_size(a.size.as_deref().unwrap_or("3840x2160"))
                .kind(ErrorKind::Invalid)?,
        };
        let fit = match (a.center, a.scale) {
            (Some(center), Some(scale)) => Fit::View { center, scale },
            (None, None) => match a.fit.as_deref().unwrap_or("cover") {
                "repeat" => Fit::Repeat { cell_px: a.cell_px },
                f => Fit::parse(f).kind(ErrorKind::Invalid)?,
            },
            _ => {
                return Err(ApiError::invalid(
                    "`center` ([x, y]) and `scale` go together",
                ));
            }
        };
        let own;
        let (w, name): (&World, String) = match &a.source {
            Some(source) => {
                // A world of its own, from a file or a Quick Start entry.
                let mut sc = scene_source(cx, source)?;
                if a.fresh {
                    sc.tiling = wallpaper::fresh_tiling(&sc.tiling, width, height, a.cell_px);
                    sc.commands.clear();
                    sc.record = None;
                }
                let mut w = World::from_scene(&sc)?;
                if let Some(p) = &a.palette {
                    if !w.palette_names().contains(p) {
                        return Err(ApiError::not_found(format!(
                            "no palette named '{p}' (see `jetcarp palettes`)"
                        )));
                    }
                    for c in recolour(&w, p) {
                        w.apply(c)?;
                    }
                }
                let gens = a.gens.unwrap_or(0);
                if gens > 0 {
                    match a.gpu.then(|| cx.platform.gpu_stepper()) {
                        Some(Ok(mut sim)) => {
                            sim.step_mixed(&mut w, gens)?;
                        }
                        _ => {
                            w.apply(Command::Step { gens })?;
                        }
                    }
                }
                let name = if sc.name.is_empty() {
                    Path::new(source)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| source.clone())
                } else {
                    sc.name.clone()
                };
                own = w;
                (&own, name)
            }
            None => {
                if let Some(p) = &a.palette {
                    let w = host.world();
                    if !w.palette_names().iter().any(|n| n == p) {
                        return Err(ApiError::not_found(format!(
                            "no palette named {p}. Palettes: {}",
                            w.palette_names().join(", ")
                        )));
                    }
                    for c in recolour(w, p) {
                        host.exec(c)?;
                    }
                }
                let w: &World = if a.fresh {
                    let mut sc = host.scene();
                    sc.tiling = wallpaper::fresh_tiling(&sc.tiling, width, height, a.cell_px);
                    sc.commands.clear();
                    sc.record = None;
                    let probe = Topology::build(&sc.tiling)?;
                    let max = cx.limits.max_cells.saturating_mul(4);
                    if probe.len() as u64 > max {
                        return Err(ApiError::limit(format!(
                            "a fresh grid of {} cells is too large here (at most {max}); use a larger cell_px",
                            probe.len()
                        )));
                    }
                    let mut fw = World::from_scene(&sc)?;
                    let gens = a.gens.unwrap_or(0);
                    if gens.saturating_mul(fw.topo.len() as u64)
                        > cx.limits.max_work.saturating_mul(4)
                    {
                        return Err(ApiError::limit(
                            "too many generations for a grid this size; lower gens or raise cell_px",
                        ));
                    }
                    match (a.gpu && gens > 0).then(|| cx.platform.gpu_stepper()) {
                        Some(Ok(mut sim)) => {
                            sim.step_mixed(&mut fw, gens)?;
                        }
                        _ => {
                            fw.apply(Command::Step { gens })?;
                        }
                    }
                    own = fw;
                    &own
                } else {
                    if a.gens.is_some() {
                        return Err(ApiError::invalid(
                            "`gens` goes with `fresh` (use step for the current world)",
                        ));
                    }
                    host.world()
                };
                let s = host.scene().name;
                (w, if s.is_empty() { "wallpaper".into() } else { s })
            }
        };
        let img = wallpaper::wallpaper(w, width, height, &fit, a.outline)?;
        let base = crate::out::sanitize(&name);
        let stamp = crate::out::stamp();
        // The folder as written (shown), and where the files go.
        let (shown_dir, stem) = match &a.out {
            Some(p) if p.to_ascii_lowercase().ends_with(".png") => {
                let p = Path::new(p);
                (
                    p.parent().map(Path::to_path_buf).unwrap_or_default(),
                    Some(
                        p.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    ),
                )
            }
            Some(p) => (PathBuf::from(p), None),
            None => (cx.output_root().join("wallpapers"), None),
        };
        let dir = if shown_dir.as_os_str().is_empty() {
            cx.fs.writable_dir(".")?
        } else {
            cx.fs.writable_dir(&shown_dir.display().to_string())?
        };
        let mut meta = jetcarp_core::render::png_metadata(w);
        meta.push((
            "jetcarp:wallpaper",
            serde_json::to_string(&fit).unwrap_or_default(),
        ));
        let (mut files, mut shown) = (Vec::new(), Vec::new());
        let mut write = |file: String,
                         img: &jetcarp_core::render::Image,
                         meta: &[(&str, String)],
                         monitor: Option<String>|
         -> Result<(), ApiError> {
            let path = dir.join(&file);
            jetcarp_core::render::write_png(&path, img, meta).map_err(ApiError::io)?;
            shown.push(shown_dir.join(&file));
            files.push(WallpaperFile {
                path,
                width: img.width,
                height: img.height,
                monitor,
            });
            Ok(())
        };
        match (&monitors, a.split) {
            (Some(m), true) => {
                for (k, (mon, part)) in m.iter().zip(wallpaper::split(&img, m)).enumerate() {
                    let file = match &stem {
                        Some(s) => format!("{s}-{}.png", k + 1),
                        None => {
                            format!("{base}-{}-{}x{}-{stamp}.png", k + 1, mon.width, mon.height)
                        }
                    };
                    let at = format!("{}x{}+{}+{}", mon.width, mon.height, mon.x, mon.y);
                    let mut mm = meta.clone();
                    mm.push(("jetcarp:monitor", at.clone()));
                    write(file, &part, &mm, Some(at))?;
                }
            }
            _ => {
                let file = match &stem {
                    Some(s) => format!("{s}.png"),
                    None => format!("{base}-{width}x{height}-{stamp}.png"),
                };
                write(file, &img, &meta, None)?;
            }
        }
        Ok(WallpaperOut {
            files,
            fit,
            generation: w.generation,
            hash: w.hash_hex(),
            shown,
        })
    }
}
