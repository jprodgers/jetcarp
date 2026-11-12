//! `jetcarp wallpaper`: desktop-sized PNGs of a scene or Quick Start entry, for one monitor or a
//! span of monitors (one image per monitor, cut so they line up). The work is in the operation
//! `wallpaper.export`.

use crate::show;
use anyhow::{Result, anyhow, bail};
use jetcarp_api::Ctx;
use jetcarp_api::op::{Quiet, WorldOp};
use jetcarp_api::ops::wallpaper::{WallpaperExport, WallpaperIn};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct WallpaperArgs {
    /// A scene file, or a Quick Start id (`jetcarp quickstart` lists the built-in ones; `find:...`
    /// for "My finds").
    source: String,
    /// WIDTHxHEIGHT or a preset (HD, QHD, 4K, 5K, 8K, UWQHD, WUXGA, WQXGA, Phone). Ignored with
    /// --monitors (the span's size is used).
    #[arg(long, default_value = "3840x2160")]
    size: String,
    /// cover (fill and crop, no stage showing), contain (whole grid, letterboxed), or repeat
    /// (a wrap-edge grid tiled at --cell-px).
    #[arg(long, default_value = "cover")]
    fit: String,
    /// Look at this world point (`X,Y`) at --scale pixels per world unit instead of fitting.
    #[arg(long)]
    center: Option<String>,
    /// Pixels per world unit, with --center.
    #[arg(long)]
    scale: Option<f64>,
    /// Build a fresh grid of the scene's tiling sized to fill the image at --cell-px (the
    /// scene's CAs and seeds, without its edits). An aperiodic tiling gets the substitution
    /// level nearest that.
    #[arg(long)]
    fresh: bool,
    /// Tile size in pixels for --fresh and --fit repeat.
    #[arg(long, default_value_t = 4.0)]
    cell_px: f64,
    /// Generations to run before rendering.
    #[arg(long, default_value_t = 0)]
    gens: u64,
    /// Step on the GPU where the rules allow it (results are identical).
    #[arg(long)]
    gpu: bool,
    /// Colour every CA with this palette (`jetcarp palettes` lists them), or with hex colours
    /// like `#4fc3ff` or `#103050-#4fc3ff`.
    #[arg(long)]
    palette: Option<String>,
    /// Monitors as WIDTHxHEIGHT+X+Y, comma separated (xrandr's layout), for a span image.
    #[arg(long)]
    monitors: Option<String>,
    /// With --monitors: write one image per monitor instead of the whole span.
    #[arg(long)]
    split: bool,
    /// Draw tile outlines when tiles are at least this many pixels across (0 = never).
    #[arg(long, default_value_t = 0.0)]
    outline: f64,
    /// Output file (.png) or folder. Default: <Pictures>/JETCARP/wallpapers/.
    #[arg(short, long)]
    out: Option<PathBuf>,
}

pub fn run(cx: &Ctx, a: WallpaperArgs, json: bool) -> Result<()> {
    // Flag combinations, checked here with the flags' names.
    if a.split && a.monitors.is_none() {
        bail!("--split needs --monitors");
    }
    let center = match (&a.center, a.scale) {
        (Some(c), Some(_)) => {
            let (x, y) = c.split_once(',').ok_or_else(|| anyhow!("--center X,Y"))?;
            Some([x.trim().parse()?, y.trim().parse()?])
        }
        (None, None) => None,
        _ => bail!("--center and --scale go together"),
    };
    let input = WallpaperIn {
        source: Some(a.source),
        size: Some(a.size),
        fit: Some(a.fit),
        center,
        scale: a.scale,
        cell_px: a.cell_px,
        fresh: a.fresh,
        gens: Some(a.gens),
        gpu: a.gpu,
        palette: a.palette,
        monitors: a.monitors,
        split: a.split,
        outline: a.outline,
        out: a.out.map(|p| p.display().to_string()),
    };
    // The world comes from the source: this session is never drawn.
    let mut session = jetcarp_api::world::Session::new();
    show(
        &WallpaperExport::run(cx, &mut session, input, &Quiet)?,
        json,
    )
}
