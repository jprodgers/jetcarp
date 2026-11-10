//! `jetcarp wallpaper`: desktop-sized PNGs of a scene or Quick Start entry, for one monitor or a
//! span of monitors (one image per monitor, cut so they line up).

use anyhow::{Context, Result, anyhow, bail};
use jetcarp_core::render::{png_metadata, write_png};
use jetcarp_core::wallpaper::{self, Fit};
use jetcarp_core::*;
use std::path::{Path, PathBuf};

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
    /// scene's CAs and seeds, without its edits).
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
    /// Colour every CA with this palette (`jetcarp palettes` lists them).
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
    /// Output file (.png) or folder. Default: <jetcarp-out>/wallpapers/.
    #[arg(short, long)]
    out: Option<PathBuf>,
}

/// A scene from a file path or a Quick Start id.
pub fn load_source(source: &str) -> Result<Scene> {
    let p = Path::new(source);
    if p.exists() {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        return Scene::from_json(&text).map_err(anyhow::Error::msg);
    }
    let all = quickstart::entries();
    if let Some(e) = all.iter().find(|e| e.id == source) {
        return Ok(e.scene.clone());
    }
    // The number `jetcarp quickstart` lists it under.
    if let Ok(k) = source.parse::<usize>()
        && let Some(e) = all.get(k)
    {
        return Ok(e.scene.clone());
    }
    if source.starts_with("find:") {
        let lib = jetcarp_core::library::Library::open_default();
        let row = lib.get_quickstart(source).map_err(anyhow::Error::msg)?;
        return Ok(quickstart::find_entry(&lib, row).scene);
    }
    bail!("'{source}' is neither a scene file nor a Quick Start id (see `jetcarp quickstart`)")
}

pub fn run(a: WallpaperArgs) -> Result<()> {
    let mut sc = load_source(&a.source)?;
    let monitors = a
        .monitors
        .as_deref()
        .map(wallpaper::parse_monitors)
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let (width, height) = match &monitors {
        Some(m) => {
            let (_, _, w, h) = wallpaper::span_box(m);
            (w, h)
        }
        None => wallpaper::parse_size(&a.size).map_err(anyhow::Error::msg)?,
    };
    if a.split && monitors.is_none() {
        bail!("--split needs --monitors");
    }
    if a.fresh {
        sc.tiling = wallpaper::fresh_tiling(&sc.tiling, width, height, a.cell_px);
        sc.commands.clear();
        sc.record = None;
    }
    let mut w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
    if let Some(p) = &a.palette {
        if !w.palette_names().contains(p) {
            bail!("no palette named '{p}' (see `jetcarp palettes`)");
        }
        let ids: Vec<u32> = w.cas.iter().map(|c| c.id).collect();
        for id in ids {
            w.apply(Command::UpdateCa {
                id,
                change: CaChange {
                    palette: Some(p.clone()),
                    ..Default::default()
                },
            })
            .map_err(anyhow::Error::msg)?;
        }
    }
    if a.gens > 0 {
        match a.gpu.then(jetcarp_gpu::GpuSim::headless) {
            Some(Ok(mut sim)) => {
                sim.step_mixed(&mut w, a.gens).map_err(anyhow::Error::msg)?;
            }
            _ => {
                w.apply(Command::Step { gens: a.gens })
                    .map_err(anyhow::Error::msg)?;
            }
        }
    }
    let fit = match (&a.center, a.scale) {
        (Some(c), Some(scale)) => {
            let (x, y) = c.split_once(',').ok_or_else(|| anyhow!("--center X,Y"))?;
            Fit::View {
                center: [x.trim().parse()?, y.trim().parse()?],
                scale,
            }
        }
        (None, None) => match a.fit.as_str() {
            "repeat" => Fit::Repeat { cell_px: a.cell_px },
            f => Fit::parse(f).map_err(anyhow::Error::msg)?,
        },
        _ => bail!("--center and --scale go together"),
    };
    let img =
        wallpaper::wallpaper(&w, width, height, &fit, a.outline).map_err(anyhow::Error::msg)?;
    let name = if sc.name.is_empty() {
        Path::new(&a.source)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| a.source.clone())
    } else {
        sc.name.clone()
    };
    let base = jetcarp_script::out::sanitize(&name);
    let stamp = jetcarp_script::out::stamp();
    let (dir, file) = match &a.out {
        Some(p) if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) => (
            p.parent().map(Path::to_path_buf).unwrap_or_default(),
            Some(p.file_stem().unwrap().to_string_lossy().to_string()),
        ),
        Some(p) => (p.clone(), None),
        None => (jetcarp_script::out::out_root(None).join("wallpapers"), None),
    };
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut meta = png_metadata(&w);
    let fit_json = serde_json::to_string(&fit)?;
    meta.push(("jetcarp:wallpaper", fit_json));
    let mut written = Vec::new();
    match (&monitors, a.split) {
        (Some(m), true) => {
            for (k, (mon, part)) in m.iter().zip(wallpaper::split(&img, m)).enumerate() {
                let stem = match &file {
                    Some(f) => format!("{f}-{}", k + 1),
                    None => format!("{base}-{}-{}x{}-{stamp}", k + 1, mon.width, mon.height),
                };
                let path = dir.join(format!("{stem}.png"));
                let mut mm = meta.clone();
                mm.push((
                    "jetcarp:monitor",
                    format!("{}x{}+{}+{}", mon.width, mon.height, mon.x, mon.y),
                ));
                write_png(&path, &part, &mm).map_err(anyhow::Error::msg)?;
                written.push((path, part.width, part.height));
            }
        }
        _ => {
            let stem = file
                .clone()
                .unwrap_or_else(|| format!("{base}-{width}x{height}-{stamp}"));
            let path = dir.join(format!("{stem}.png"));
            write_png(&path, &img, &meta).map_err(anyhow::Error::msg)?;
            written.push((path, width, height));
        }
    }
    for (p, w, h) in written {
        println!("{} ({w} x {h})", p.display());
    }
    Ok(())
}
