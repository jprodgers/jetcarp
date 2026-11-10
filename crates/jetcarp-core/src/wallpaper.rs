//! Wallpapers: desktop-sized images of a world, one per monitor or one for a span of monitors.
//!
//! - [`Fit`] chooses what the image shows: the whole grid filling the image and cropped
//!   ([`Fit::Cover`]), the whole grid letterboxed in the stage colour ([`Fit::Contain`]), a given
//!   centre and scale ([`Fit::View`]), or a wrap-edge grid repeated at a cell size
//!   ([`Fit::Repeat`]).
//! - [`wallpaper`] renders exactly `width` x `height` pixels with [`render::render_view`].
//! - [`split`] cuts a span image into one image per monitor, from the monitors' rectangles in
//!   span pixels, so each monitor's wallpaper lines up with its neighbours.
//! - [`fresh_tiling`] sizes a tiling to fill an image at a given cell size, for a fresh grid made
//!   for the wallpaper.

use crate::geom::P;
use crate::render::{self, Image, ViewOptions};
use crate::topology::{SizeSpec, TilingSpec};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// What a wallpaper shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fit {
    /// The grid fills the image, cropped to the image's shape. The crop stays inside the tiling,
    /// so no stage colour shows (wrap-edge lattices repeat instead of being cropped tighter).
    Cover,
    /// The whole grid, as large as fits, centred on the stage colour.
    Contain,
    /// World point `center` at the image centre, `scale` pixels per world unit.
    View { center: P, scale: f64 },
    /// A wrap-edge lattice repeated over the whole image, tiles `cell_px` pixels across.
    Repeat { cell_px: f64 },
}

impl Fit {
    /// "cover", "contain", or "repeat" (repeat at 4 px per tile).
    pub fn parse(s: &str) -> Result<Fit, String> {
        match s {
            "cover" => Ok(Fit::Cover),
            "contain" => Ok(Fit::Contain),
            "repeat" => Ok(Fit::Repeat { cell_px: 4.0 }),
            _ => Err(format!("unknown fit '{s}' (cover, contain, repeat)")),
        }
    }
}

/// A monitor's rectangle in span pixels (the desktop's layout; may start at negative positions).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorRect {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

/// Common wallpaper sizes: (name, width, height).
pub const PRESETS: &[(&str, u32, u32)] = &[
    ("HD", 1920, 1080),
    ("WUXGA", 1920, 1200),
    ("QHD", 2560, 1440),
    ("WQXGA", 2560, 1600),
    ("UWQHD", 3440, 1440),
    ("4K", 3840, 2160),
    ("5K", 5120, 2880),
    ("8K", 7680, 4320),
    ("Phone", 1080, 2400),
];

/// Largest wallpaper rendered in memory (pixels).
pub const MAX_PIXELS: u64 = 160_000_000;

/// "3840x2160" or a preset name ("4K").
pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
    if let Some(p) = PRESETS.iter().find(|p| p.0.eq_ignore_ascii_case(s)) {
        return Ok((p.1, p.2));
    }
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("size '{s}': expected WIDTHxHEIGHT such as 3840x2160"))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| format!("size '{s}': bad width"))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| format!("size '{s}': bad height"))?;
    check_size(w, h)?;
    Ok((w, h))
}

fn check_size(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("a wallpaper needs a nonzero size".into());
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!(
            "{w} x {h} is more than {MAX_PIXELS} pixels; render the monitors one at a time"
        ));
    }
    Ok(())
}

/// Monitors as `WIDTHxHEIGHT+X+Y` separated by commas (X and Y may be negative:
/// `1920x1080-1920+0`), as `xrandr` prints them.
pub fn parse_monitors(s: &str) -> Result<Vec<MonitorRect>, String> {
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let bad = || format!("monitor '{part}': expected WIDTHxHEIGHT+X+Y");
        let at = part.find(['+', '-']).ok_or_else(bad)?;
        let (w, h) = parse_size(&part[..at])?;
        let rest = &part[at..];
        let second = rest[1..].find(['+', '-']).ok_or_else(bad)? + 1;
        let x: i64 = rest[..second].parse().map_err(|_| bad())?;
        let y: i64 = rest[second..].parse().map_err(|_| bad())?;
        out.push(MonitorRect {
            x,
            y,
            width: w,
            height: h,
        });
    }
    if out.is_empty() {
        return Err("no monitors given".into());
    }
    Ok(out)
}

/// The span's bounding box: (left, top, width, height) in span pixels.
pub fn span_box(monitors: &[MonitorRect]) -> (i64, i64, u32, u32) {
    let x0 = monitors.iter().map(|m| m.x).min().unwrap_or(0);
    let y0 = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    let x1 = monitors
        .iter()
        .map(|m| m.x + m.width as i64)
        .max()
        .unwrap_or(0);
    let y1 = monitors
        .iter()
        .map(|m| m.y + m.height as i64)
        .max()
        .unwrap_or(0);
    (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// Cut a span image (the size of [`span_box`]) into one image per monitor, in order.
pub fn split(span: &Image, monitors: &[MonitorRect]) -> Vec<Image> {
    let (x0, y0, _, _) = span_box(monitors);
    monitors
        .iter()
        .map(|m| {
            crop(
                span,
                (m.x - x0) as u32,
                (m.y - y0) as u32,
                m.width,
                m.height,
            )
        })
        .collect()
}

/// A `w` x `h` piece of `img` with its top-left corner at `(x, y)` (outside the image: stage).
pub fn crop(img: &Image, x: u32, y: u32, w: u32, h: u32) -> Image {
    let mut out = Image {
        width: w,
        height: h,
        rgba: render::STAGE.repeat(w as usize * h as usize),
    };
    for row in 0..h {
        let sy = y + row;
        if sy >= img.height || x >= img.width {
            continue;
        }
        let n = w.min(img.width - x) as usize * 4;
        let src = ((sy * img.width + x) * 4) as usize;
        let dst = (row * w * 4) as usize;
        out.rgba[dst..dst + n].copy_from_slice(&img.rgba[src..src + n]);
    }
    out
}

/// The tiling spec sized to fill a `width` x `height` image with tiles about `cell_px` pixels
/// across (see [`crate::topology::fill_size`]). Aperiodic tilings keep their size.
pub fn fresh_tiling(spec: &TilingSpec, width: u32, height: u32, cell_px: f64) -> TilingSpec {
    let size = crate::topology::fill_size(spec, width as f64, height as f64, cell_px);
    let mut s = spec.clone();
    if matches!(size, SizeSpec::Rect { .. }) {
        s.size = Some(size);
    }
    s
}

/// Where a wallpaper looks: (centre, pixels per world unit, repeat a wrap-edge grid).
pub fn view_for(
    world: &World,
    width: u32,
    height: u32,
    fit: &Fit,
) -> Result<(P, f64, bool), String> {
    let topo = &world.topo;
    let b = topo.bounds;
    let (bw, bh) = ((b[2] - b[0]).max(1e-9), (b[3] - b[1]).max(1e-9));
    let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let (w, h) = (width as f64, height as f64);
    let wraps = render::torus_periods(topo).is_some();
    Ok(match *fit {
        Fit::Contain => (center, (w / bw).min(h / bh), false),
        Fit::View { center, scale } => {
            if !(scale > 0.0 && scale.is_finite()) {
                return Err("a view needs a positive scale".into());
            }
            (center, scale, wraps)
        }
        Fit::Repeat { cell_px } => {
            if !wraps {
                return Err(
                    "repeat needs a lattice with wrap edges (a torus); use cover or contain".into(),
                );
            }
            if !(cell_px > 0.0 && cell_px.is_finite()) {
                return Err("repeat needs a positive cell size".into());
            }
            (center, cell_px / world.tile_size(), true)
        }
        Fit::Cover => {
            let s0 = (w / bw).max(h / bh);
            if wraps {
                (center, s0, true)
            } else {
                (center, cover_scale(world, center, w, h, s0), false)
            }
        }
    })
}

/// The smallest scale from `s0` up at which a `w` x `h` view centred on `center` lies inside the
/// tiling (sampled on a grid of points, edges included), so a cover crop shows no stage.
fn cover_scale(world: &World, center: P, w: f64, h: f64, s0: f64) -> f64 {
    let topo = &world.topo;
    let tile = world.tile_size();
    let check = |s: f64, fine: bool| {
        // Pixel centres along the four edges (every pixel when `fine`, else a third of a tile
        // apart), and a coarse grid inside.
        let (x0, y0) = (center[0] - w / (2.0 * s), center[1] - h / (2.0 * s));
        let at = |px: f64, py: f64| topo.locate([x0 + px / s, y0 + py / s]).is_some();
        let step = if fine { 1.0 } else { (tile * s / 3.0).max(1.0) };
        let along = |len: f64| {
            let n = ((len - 1.0) / step).ceil().max(1.0) as usize;
            (0..=n).map(move |k| 0.5 + (len - 1.0) * k as f64 / n as f64)
        };
        along(w).all(|x| at(x, 0.5) && at(x, h - 0.5))
            && along(h).all(|y| at(0.5, y) && at(w - 0.5, y))
            && (1..10).all(|a| (1..10).all(|c| at(w * a as f64 / 10.0, h * c as f64 / 10.0)))
    };
    let covered = |s: f64| check(s, false);
    let mut s = coarse_cover(&covered, s0);
    // Every edge pixel: nudge in until no gap of a ragged edge shows.
    for _ in 0..400 {
        if check(s, true) {
            break;
        }
        s *= 1.002;
    }
    s
}

fn coarse_cover(covered: &dyn Fn(f64) -> bool, s0: f64) -> f64 {
    if covered(s0) {
        return s0;
    }
    let (mut lo, mut hi) = (s0, s0 * 2.0);
    let mut k = 0;
    while !covered(hi) && k < 12 {
        lo = hi;
        hi *= 2.0;
        k += 1;
    }
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if covered(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Render a `width` x `height` wallpaper of the world.
pub fn wallpaper(
    world: &World,
    width: u32,
    height: u32,
    fit: &Fit,
    outline_min_px: f64,
) -> Result<Image, String> {
    check_size(width, height)?;
    let (center, scale, repeat) = view_for(world, width, height, fit)?;
    Ok(render::render_view(
        world,
        width,
        height,
        center,
        scale,
        &ViewOptions {
            outline_min_px,
            repeat,
            colors: None,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes_and_monitors() {
        assert_eq!(parse_size("3840x2160").unwrap(), (3840, 2160));
        assert_eq!(parse_size("4k").unwrap(), (3840, 2160));
        assert!(parse_size("0x5").is_err());
        let m = parse_monitors("2560x1440+0+0,1920x1080+2560+180").unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].x, 2560);
        assert_eq!(m[1].y, 180);
        assert_eq!(span_box(&m), (0, 0, 4480, 1440));
        let m = parse_monitors("1920x1080-1920+0, 2560x1440+0-200").unwrap();
        assert_eq!((m[0].x, m[0].y, m[1].y), (-1920, 0, -200));
        assert_eq!(span_box(&m), (-1920, -200, 4480, 1440));
    }
}
