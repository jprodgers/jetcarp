//! Media for the catalog and the website: per-step frames (PNG), an animation (GIF or animated
//! WebP), a poster frame, and a thumbnail, for a catalog pattern, a Quick Start entry, an ant run
//! ([`crate::ants::media`]: a highway's moving loop alone, the build-up from step 0), or any
//! scene.
//!
//! **Two modes** ([`MediaMode`]), chosen automatically with a manual override:
//!
//! - **Alone** (patterns): the pattern on an empty field. Still lifes, oscillators, and
//!   spaceships show `periods` full periods (default 1) from their stored, verified phases, so
//!   the loop is seamless: the frame after the last is the first again. A spaceship's window
//!   moves with it ([`AloneView`]): frame `t` shows the cells shifted back by the whole lattice
//!   steps of `t · d / p` and the window shifted by the fractional rest, so the ship stays
//!   centred, the window glides at the ship's speed, and frame `p` is frame 0 bit for bit. A gun
//!   is run until its stream fills a fixed window (the core plus four gun periods of stream) and
//!   then shown for one period, which loops because the stream is periodic. Puffers, movers on
//!   aperiodic patches, and other records run from their start in a window around everything
//!   they reach (not seamless).
//! - **Seed** (Quick Start entries, scenes; patterns on request): the scene from its
//!   first generation, the whole grid, `gens` generations (row rules: one frame per row).
//!
//! **House style** ([`MediaStyle`]): a fixed palette per field kind (cells coloured by state, never
//! by age, so loops close), the app's background, outlines when tiles are at least 8 pixels
//! across, a margin of 2 empty cells around a pattern alone, whole pixels per cell (fitted to
//! 320 pixels alone, 512 from the seed), no text. Its hash goes into every file's render key, so
//! changing the style marks all media stale.
//!
//! **Format** ([`MediaFormat::Auto`]): a GIF when a frame has at most
//! [`MediaStyle::gif_max_pixels`] pixels (320 x 320), the whole animation at most
//! [`MediaStyle::gif_max_volume`] (8 million pixels: width x height x frames, about 80 frames at
//! 320 x 320), and at most 256 colours; else an animated lossless WebP. Patterns alone are GIFs;
//! whole soups are WebPs. `--format` overrides it.
//!
//! **Storage.** Files go under the catalog's folder in
//! `media/<target kind>/<target id>/<alone|seed>/` (`frames/0000.png`..., `anim.gif` or
//! `anim.webp`, `poster.png`, `thumb.png`), one `media` row per file (the frame folder is one
//! row), with paths relative to the catalog folder, the render key, and the content hash.
//! Rendering again with the same inputs, options, and style does nothing.

pub mod encode;
mod vp8l;

use crate::library::{Library, QsStatus};
use crate::objects::{LCell, Phase};
use crate::pattern::verify::{Frame, envelope, lattice_info, rule_world};
use crate::pattern::{PatternClass, PatternRecord};
use crate::render::{self, Colorer, Image, Palette, RenderOptions, Rgba, ViewColors};
use crate::rng::Fnv;
use crate::rules::{FieldKind, Rule};
use crate::world::{Command, Scene, World};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Bumped when the renderer's look changes in a way the style fields do not capture.
pub const STYLE_VERSION: u32 = 1;

/// The house style for every media file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaStyle {
    pub version: u32,
    /// Empty cells.
    pub background: [u8; 3],
    /// Pixels outside every tile.
    pub stage: [u8; 3],
    pub outline: [u8; 3],
    /// Draw tile outlines when a tile is at least this many pixels across.
    pub outline_min_px: f64,
    /// Palette stops per field kind: two-state cells take the first `binary` colour;
    /// multi-state cells (Generations decay, tables) run along `multi`; sandpile heights along
    /// `height`; continuous values along `continuous`.
    pub binary: Vec<[u8; 3]>,
    pub multi: Vec<[u8; 3]>,
    pub height: Vec<[u8; 3]>,
    pub continuous: Vec<[u8; 3]>,
    /// Target longest side in pixels: a pattern alone, and a scene from its seed.
    pub alone_side: u32,
    pub seed_side: u32,
    /// Pixels per cell (the square root of a tile's area): at most `max_cell_px`; at least
    /// `min_cell_px` alone and `seed_min_cell_px` from the seed (a large grid then gives a
    /// larger image). Whole pixels from 2 up.
    pub max_cell_px: f64,
    pub min_cell_px: f64,
    pub seed_min_cell_px: f64,
    /// Empty cells around a pattern alone.
    pub margin_cells: u32,
    /// Milliseconds per frame.
    pub alone_frame_ms: u32,
    pub seed_frame_ms: u32,
    /// Generations from the seed when not given (row rules: one per row of the grid).
    pub seed_gens: u64,
    /// Gun periods of stream shown beside a gun's core.
    pub gun_stream_periods: u64,
    /// Longest side of a thumbnail.
    pub thumb_side: u32,
    /// The automatic format choice writes a GIF when a frame has at most `gif_max_pixels`
    /// pixels, the whole animation at most `gif_max_volume` (width x height x frames), and
    /// there are at most 256 colours; otherwise an animated WebP.
    pub gif_max_pixels: u64,
    pub gif_max_volume: u64,
    /// A built-in palette in place of the house colours ([`MediaStyle::set_palette`]); `None` is
    /// the house style. It is part of the hash, so media rendered in another palette are stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
}

fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

impl Default for MediaStyle {
    fn default() -> Self {
        let bg = render::BACKGROUND;
        let st = render::STAGE;
        let ol = render::OUTLINE;
        MediaStyle {
            version: STYLE_VERSION,
            background: [bg[0], bg[1], bg[2]],
            stage: [st[0], st[1], st[2]],
            outline: [ol[0], ol[1], ol[2]],
            outline_min_px: 8.0,
            binary: vec![rgb(0xffd27a)],
            multi: [0xffd27a, 0xff8a3c, 0xd6336c, 0x6a2c91, 0x2b2d6e]
                .map(rgb)
                .to_vec(),
            height: [0x58e0c2, 0x1f9fc9, 0x2a5bb8, 0xffd27a].map(rgb).to_vec(),
            continuous: [0xe8fff7, 0x58e0c2, 0x1f9fc9, 0x2a5bb8, 0x2b2470]
                .map(rgb)
                .to_vec(),
            alone_side: 320,
            seed_side: 512,
            max_cell_px: 24.0,
            min_cell_px: 2.0,
            seed_min_cell_px: 1.0,
            margin_cells: 2,
            alone_frame_ms: 150,
            seed_frame_ms: 60,
            seed_gens: 200,
            gun_stream_periods: 4,
            thumb_side: 160,
            gif_max_pixels: 320 * 320,
            gif_max_volume: 8_000_000,
            palette: None,
        }
    }
}

impl MediaStyle {
    /// The house style, or the house style in a built-in palette's colours.
    pub fn with_palette(name: Option<&str>) -> Result<MediaStyle, String> {
        let mut s = MediaStyle::default();
        if let Some(n) = name {
            s.set_palette(n)?;
        }
        Ok(s)
    }
    /// Colour everything from a built-in palette: its background, stage, and outline, two-state
    /// cells in its first stop, and multi-state cells, sandpile heights, and continuous values
    /// along all its stops. An unknown name is an error that lists the palettes.
    pub fn set_palette(&mut self, name: &str) -> Result<(), String> {
        let p = render::builtin_palette(name)?;
        let c3 = |c: Rgba| [c[0], c[1], c[2]];
        self.background = c3(p.background_rgba());
        self.stage = c3(p.stage_rgba());
        self.outline = c3(p.outline_rgba());
        self.binary = vec![p.stops[0]];
        self.multi = p.stops.clone();
        self.height = p.stops.clone();
        self.continuous = p.stops.clone();
        self.palette = Some(p.name);
        Ok(())
    }
    /// FNV-1a 64 of the style's JSON, as 16 hex digits.
    pub fn hash(&self) -> String {
        hex(fnv(serde_json::to_string(self).unwrap().as_bytes()))
    }
    fn rgba(c: [u8; 3]) -> Rgba {
        [c[0], c[1], c[2], 0xff]
    }
}

/// Pattern alone, or the scene from its seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaMode {
    /// Alone for patterns, from the seed for everything else.
    #[default]
    Auto,
    Alone,
    Seed,
}

impl MediaMode {
    pub fn parse(s: &str) -> Result<MediaMode, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(MediaMode::Auto),
            "alone" | "pattern" => Ok(MediaMode::Alone),
            "seed" | "from-seed" | "from_seed" => Ok(MediaMode::Seed),
            _ => Err(format!("unknown media mode '{s}' (auto, alone, seed)")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            MediaMode::Auto => "auto",
            MediaMode::Alone => "alone",
            MediaMode::Seed => "seed",
        }
    }
}

/// Animation file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFormat {
    /// GIF up to [`MediaStyle::gif_max_pixels`] pixels per frame, [`MediaStyle::gif_max_volume`]
    /// pixels in all, and 256 colours; else WebP.
    #[default]
    Auto,
    Gif,
    Webp,
}

impl MediaFormat {
    pub fn parse(s: &str) -> Result<MediaFormat, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(MediaFormat::Auto),
            "gif" => Ok(MediaFormat::Gif),
            "webp" => Ok(MediaFormat::Webp),
            _ => Err(format!("unknown media format '{s}' (auto, gif, webp)")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            MediaFormat::Auto => "auto",
            MediaFormat::Gif => "gif",
            MediaFormat::Webp => "webp",
        }
    }
}

/// What to render and how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaOptions {
    pub mode: MediaMode,
    pub format: MediaFormat,
    /// Alone: full periods shown (default 1).
    pub periods: u64,
    /// From the seed: generations (default: the style's `seed_gens`, row rules one per row).
    pub gens: Option<u64>,
    /// From the seed: a frame every this many generations (default 1).
    pub every: u64,
    /// Write the per-step PNG frames (default true).
    pub frames: bool,
    /// Render even when the stored media are up to date.
    pub force: bool,
    pub style: MediaStyle,
}

impl Default for MediaOptions {
    fn default() -> Self {
        MediaOptions {
            mode: MediaMode::Auto,
            format: MediaFormat::Auto,
            periods: 1,
            gens: None,
            every: 1,
            frames: true,
            force: false,
            style: MediaStyle::default(),
        }
    }
}

/// Frames in memory, before encoding.
#[derive(Clone)]
pub struct Animation {
    pub frames: Vec<Image>,
    /// Generation shown in each frame.
    pub generations: Vec<u64>,
    pub delay_ms: u32,
    /// `Alone` or `Seed` (never `Auto`).
    pub mode: MediaMode,
    /// The frame used as poster and thumbnail.
    pub poster: usize,
    /// Pixels per world unit, and pixels per cell.
    pub scale: f64,
    pub cell_px: f64,
    /// Window of the first frame in world units `[x0, y0, x1, y1]`.
    pub window: [f64; 4],
    /// The frame after the last is the first (alone: still lifes, oscillators, ships, guns).
    pub seamless: bool,
    /// How the window was chosen: `fixed`, `moving`, `stream`, `reach`, `grid`.
    pub window_kind: &'static str,
}

impl Animation {
    pub fn width(&self) -> u32 {
        self.frames.first().map(|f| f.width).unwrap_or(0)
    }
    pub fn height(&self) -> u32 {
        self.frames.first().map(|f| f.height).unwrap_or(0)
    }
    /// The format the automatic rule picks for these frames.
    pub fn auto_format(&self, style: &MediaStyle) -> MediaFormat {
        let px = self.width() as u64 * self.height() as u64;
        let refs: Vec<&Image> = self.frames.iter().collect();
        if px <= style.gif_max_pixels
            && px * self.frames.len() as u64 <= style.gif_max_volume
            && encode::palette(&refs).is_some()
        {
            MediaFormat::Gif
        } else {
            MediaFormat::Webp
        }
    }
    /// Encode the animation (`Auto` picks by [`auto_format`](Self::auto_format)).
    pub fn encode(
        &self,
        format: MediaFormat,
        style: &MediaStyle,
    ) -> Result<(MediaFormat, Vec<u8>), String> {
        let f = match format {
            MediaFormat::Auto => self.auto_format(style),
            f => f,
        };
        let refs: Vec<&Image> = self.frames.iter().collect();
        let bytes = match f {
            MediaFormat::Gif => encode::gif(&refs, self.delay_ms)?,
            _ => encode::webp(&refs, self.delay_ms, MediaStyle::rgba(style.background))?,
        };
        Ok((f, bytes))
    }
    pub fn poster_image(&self) -> &Image {
        &self.frames[self.poster.min(self.frames.len() - 1)]
    }
    pub fn thumbnail(&self, style: &MediaStyle) -> Image {
        encode::shrink(self.poster_image(), style.thumb_side)
    }
}

// ---------------------------------------------------------------------------------------------
// Colours

const PAL_BINARY: &str = "jetcarp-media-binary";
const PAL_MULTI: &str = "jetcarp-media-multi";
const PAL_HEIGHT: &str = "jetcarp-media-height";
const PAL_CONT: &str = "jetcarp-media-continuous";

/// Give every CA the house palette for its field kind, coloured by state (rules with their own
/// fixed colours, such as WireWorld and von Neumann's automaton, keep them).
pub fn apply_style(world: &mut World, style: &MediaStyle) {
    for (name, stops) in [
        (PAL_BINARY, &style.binary),
        (PAL_MULTI, &style.multi),
        (PAL_HEIGHT, &style.height),
        (PAL_CONT, &style.continuous),
    ] {
        world.palettes.retain(|p| p.name != name);
        world.palettes.push(Palette::new(name, stops.clone()));
    }
    for k in 0..world.cas.len() {
        let kind = world.fields[world.cas[k].field].kind;
        let ca = &mut world.cas[k];
        let own_colours = matches!(
            ca.rule,
            Rule::WireWorld | Rule::VonNeumann | Rule::ForestFire { .. } | Rule::SpectreGlider
        );
        ca.palette = match kind {
            FieldKind::Binary => PAL_BINARY,
            FieldKind::Multi(_) => PAL_MULTI,
            FieldKind::Height => PAL_HEIGHT,
            FieldKind::Continuous => PAL_CONT,
        }
        .into();
        if matches!(kind, FieldKind::Binary | FieldKind::Multi(_)) && !own_colours {
            ca.color_by = render::ColorBy::State;
        }
    }
}

fn view_opts(style: &MediaStyle) -> (RenderOptions, ViewColors) {
    (
        RenderOptions {
            scale: None,
            max_side: 0,
            outline_min_px: style.outline_min_px,
        },
        ViewColors {
            stage: MediaStyle::rgba(style.stage),
            outline: MediaStyle::rgba(style.outline),
        },
    )
}

/// Render a window of a styled world (see [`apply_style`]).
pub(crate) fn draw(
    world: &World,
    style: &MediaStyle,
    view: [f64; 2],
    scale: f64,
    w: u32,
    h: u32,
) -> Image {
    let c = Colorer::new(world);
    let bg = MediaStyle::rgba(style.background);
    let color = |i: usize| {
        let x = c.color(i);
        if x == render::BACKGROUND { bg } else { x }
    };
    let (opts, colors) = view_opts(style);
    render::render_rect(world, &color, view, scale, w, h, &opts, &colors)
}

/// Pixels per world unit for a window `ww` x `wh` (world units) fitted into `side` pixels, with
/// pixels per cell (`cs` world units) between `min_px` and `max_px`, whole from 2 up.
pub(crate) fn fit_scale(
    ww: f64,
    wh: f64,
    side: u32,
    cs: f64,
    min_px: f64,
    max_px: f64,
) -> (f64, f64) {
    let s = side as f64 / ww.max(wh).max(1e-9);
    let mut px = (s * cs).clamp(min_px, max_px.max(min_px));
    if px >= 2.0 {
        px = px.floor();
    }
    (px / cs, px)
}

pub(crate) fn bbox_union(b: &mut [f64; 4], poly: &[[f64; 2]], off: [f64; 2]) {
    for p in poly {
        b[0] = b[0].min(p[0] - off[0]);
        b[1] = b[1].min(p[1] - off[1]);
        b[2] = b[2].max(p[0] - off[0]);
        b[3] = b[3].max(p[1] - off[1]);
    }
}

pub(crate) const EMPTY_BOX: [f64; 4] = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];

pub(crate) fn grow(b: [f64; 4], m: f64) -> [f64; 4] {
    [b[0] - m, b[1] - m, b[2] + m, b[3] + m]
}

pub(crate) fn pixels(b: [f64; 4], s: f64) -> (u32, u32) {
    (
        (((b[2] - b[0]) * s).ceil() as u32).max(1),
        (((b[3] - b[1]) * s).ceil() as u32).max(1),
    )
}

// ---------------------------------------------------------------------------------------------
// Pattern alone

/// A still life, oscillator, or spaceship drawn alone: the frame topology, the window of
/// frame 0, and the scale. [`frame`](Self::frame) draws any generation from its cells, so a
/// test can step the last frame and compare with the first.
pub struct AloneView {
    rec: PatternRecord,
    frame: Frame,
    /// Window of frame 0, world units.
    pub window: [f64; 4],
    pub scale: f64,
    pub cell_px: f64,
    pub width: u32,
    pub height: u32,
    style: MediaStyle,
}

impl AloneView {
    pub fn new(rec: &PatternRecord, periods: u64, style: &MediaStyle) -> Result<AloneView, String> {
        if rec.site.is_some() {
            return Err("aperiodic patterns have no lattice view".into());
        }
        let p = rec.period.max(1);
        let n = p * periods.max(1);
        let d = ship_d(rec);
        let info = lattice_info(&rec.tiling, rec.neighborhood)?;
        let env = envelope(rec, n)?;
        let margin = style.margin_cells as i32 + 3 + d[0].abs().max(d[1].abs());
        let frame = Frame::around(&rec.tiling, &info, &env, margin)?;
        let mut v = AloneView {
            rec: rec.clone(),
            frame,
            window: [0.0; 4],
            scale: 1.0,
            cell_px: 1.0,
            width: 1,
            height: 1,
            style: style.clone(),
        };
        // The tight window: every phase of one period, each shifted back by the window's
        // offset at that generation.
        let mut b = EMPTY_BOX;
        for t in 0..p {
            let (cells, off) = v.shown(&rec.phase(t)?, t)?;
            for c in &cells {
                let i = v.index(c)?;
                bbox_union(&mut b, &v.frame.topo.polygon(i), off);
            }
        }
        let cs = crate::world::cell_size(&v.frame.topo);
        let b = grow(b, style.margin_cells as f64 * cs);
        let (s, px) = fit_scale(
            b[2] - b[0],
            b[3] - b[1],
            style.alone_side,
            cs,
            style.min_cell_px,
            style.max_cell_px,
        );
        let (w, h) = pixels(b, s);
        v.window = b;
        v.scale = s;
        v.cell_px = px;
        v.width = w;
        v.height = h;
        Ok(v)
    }

    fn index(&self, c: &LCell) -> Result<usize, String> {
        self.frame
            .index(c)
            .ok_or_else(|| "pattern cell outside the media frame (internal error)".to_string())
    }

    /// Cells of generation `t` moved back by the whole lattice steps of `t · d / p`, and the
    /// window offset (world units) for the fractional rest.
    fn shown(&self, cells: &[LCell], t: u64) -> Result<(Phase, [f64; 2]), String> {
        let d = ship_d(&self.rec);
        let p = self.rec.period.max(1) as i64;
        let num = [d[0] as i64 * t as i64, d[1] as i64 * t as i64];
        let l = [num[0].div_euclid(p), num[1].div_euclid(p)];
        let r = [
            (num[0] - l[0] * p) as f64 / p as f64,
            (num[1] - l[1] * p) as f64 / p as f64,
        ];
        let g = self.frame.topo.lattice().unwrap();
        let off = [
            r[0] * g.a1[0] + r[1] * g.a2[0],
            r[0] * g.a1[1] + r[1] * g.a2[1],
        ];
        let v = cells
            .iter()
            .map(|c| LCell {
                i: c.i - l[0] as i32,
                j: c.j - l[1] as i32,
                ..*c
            })
            .collect();
        Ok((v, off))
    }

    /// Frame for generation `t`, given its live cells in pattern coordinates (as
    /// [`PatternRecord::phase`] gives them).
    pub fn frame(&self, cells: &[LCell], t: u64) -> Result<Image, String> {
        let (shown, off) = self.shown(cells, t)?;
        let mut w = rule_world(&self.rec, self.frame.topo.clone())?;
        apply_style(&mut w, &self.style);
        let id = w.cas[0].id;
        w.apply(Command::SetCells {
            id,
            cells: self.frame.cells(&shown)?,
        })?;
        Ok(draw(
            &w,
            &self.style,
            [self.window[0] + off[0], self.window[1] + off[1]],
            self.scale,
            self.width,
            self.height,
        ))
    }

    /// A world on the view's frame holding `cells` (for stepping in checks).
    pub fn world_with(&self, cells: &[LCell]) -> Result<World, String> {
        let mut w = rule_world(&self.rec, self.frame.topo.clone())?;
        let id = w.cas[0].id;
        w.apply(Command::SetCells {
            id,
            cells: self.frame.cells(cells)?,
        })?;
        Ok(w)
    }

    /// The live cells of a world made by [`world_with`](Self::world_with), in pattern
    /// coordinates.
    pub fn live(&self, w: &World) -> Phase {
        self.frame
            .live(&w.fields[w.cas[0].field].state, self.rec.neighborhood)
            .0
    }
}

fn ship_d(rec: &PatternRecord) -> [i32; 2] {
    if rec.class == PatternClass::Spaceship {
        rec.displacement.unwrap_or([0, 0])
    } else {
        [0, 0]
    }
}

fn alone_periodic(rec: &PatternRecord, o: &MediaOptions) -> Result<Animation, String> {
    let v = AloneView::new(rec, o.periods, &o.style)?;
    let n = rec.period.max(1) * o.periods.max(1);
    let frames = (0..n)
        .map(|t| v.frame(&rec.phase(t)?, t))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Animation {
        frames,
        generations: (0..n).collect(),
        delay_ms: o.style.alone_frame_ms,
        mode: MediaMode::Alone,
        poster: 0,
        scale: v.scale,
        cell_px: v.cell_px,
        window: v.window,
        seamless: true,
        window_kind: if ship_d(rec) == [0, 0] {
            "fixed"
        } else {
            "moving"
        },
    })
}

/// Most pixels (width x height x frames) one animation holds in memory: 1.2 GB of RGBA.
pub const MAX_VOLUME: u64 = 300_000_000;

/// Frames, their generations, the window, pixels per world unit, and pixels per cell.
type RunOut = (Vec<Image>, Vec<u64>, [f64; 4], f64, f64);

/// Run a world (made fresh by `make`), drawing the generations in `shown` (every `every`) in a
/// fixed window, or (`None`) in a window around every live cell of those generations, found by
/// a first run. Frames are drawn while stepping, so memory stays at one world.
#[allow(clippy::too_many_arguments)]
fn run_world(
    make: &dyn Fn() -> Result<World, String>,
    shown: std::ops::Range<u64>,
    every: u64,
    window: Option<[f64; 4]>,
    side: u32,
    min_px: f64,
    margin_cells: f64,
    style: &MediaStyle,
) -> Result<RunOut, String> {
    let every = every.max(1);
    // Visit the shown generations of a fresh run.
    let visit = |f: &mut dyn FnMut(&World)| -> Result<(), String> {
        let mut w = make()?;
        apply_style(&mut w, style);
        while w.generation < shown.start {
            w.apply(Command::Step { gens: 1 })?;
        }
        loop {
            f(&w);
            if w.generation + every >= shown.end {
                return Ok(());
            }
            w.apply(Command::Step { gens: every })?;
        }
    };
    let probe = make()?;
    let topo = probe.topo.clone();
    let cs = crate::world::cell_size(&topo);
    let win = match window {
        Some(x) => x,
        None => {
            let mut b = EMPTY_BOX;
            visit(&mut |w: &World| {
                for f in &w.fields {
                    for (i, &s) in f.state.iter().enumerate() {
                        if s != 0 || f.height.get(i).is_some_and(|&h| h != 0) {
                            bbox_union(&mut b, &topo.polygon(i), [0.0, 0.0]);
                        }
                    }
                }
            })?;
            if b[0] == f64::MAX {
                topo.bounds
            } else {
                // Stay inside the tiling.
                let g = grow(b, margin_cells * cs);
                let tb = topo.bounds;
                [
                    g[0].max(tb[0]),
                    g[1].max(tb[1]),
                    g[2].min(tb[2]),
                    g[3].min(tb[3]),
                ]
            }
        }
    };
    let (s, px) = fit_scale(
        win[2] - win[0],
        win[3] - win[1],
        side,
        cs,
        min_px,
        style.max_cell_px,
    );
    let (pw, ph) = pixels(win, s);
    let n = (shown.end.saturating_sub(shown.start))
        .div_ceil(every)
        .max(1);
    if pw as u64 * ph as u64 * n > MAX_VOLUME {
        return Err(format!(
            "{n} frames of {pw} x {ph} pixels is too much for one animation (at most {} million pixels in all): use fewer generations, or a frame every few (`every`)",
            MAX_VOLUME / 1_000_000
        ));
    }
    let mut frames = Vec::new();
    let mut gens = Vec::new();
    visit(&mut |w: &World| {
        frames.push(draw(w, style, [win[0], win[1]], s, pw, ph));
        gens.push(w.generation);
    })?;
    Ok((frames, gens, win, s, px))
}

fn alone_gun(rec: &PatternRecord, o: &MediaOptions) -> Result<Animation, String> {
    let st = &o.style;
    let info = rec.gun.as_ref().ok_or("a gun record needs its gun info")?;
    let p = rec.period.max(1);
    let sp = info.ship_period.max(1);
    let d = info.ship_displacement;
    let phases = rec.all_phases()?;
    let li = lattice_info(&rec.tiling, rec.neighborhood)?;
    // The window: the core and the stream for `gun_stream_periods` gun periods.
    let hops = (st.gun_stream_periods.max(1) * p).div_ceil(sp) as i32;
    let dmax = d[0].abs().max(d[1].abs()).max(1);
    // Warm up until the first ship has crossed the window and its margin, on a period
    // boundary, so frame 0 shows the core at phase 0 with a full stream.
    let warm =
        ((hops as u64 + 3 + (st.margin_cells as u64).div_ceil(dmax as u64)) * sp).div_ceil(p) * p;
    let n = p * o.periods.max(1);
    let total_hops = ((warm + n) / sp + 3) as i32;
    let shift = |c: &LCell, k: i32| LCell {
        i: c.i + d[0] * k,
        j: c.j + d[1] * k,
        ..*c
    };
    let mut env: Vec<LCell> = Vec::new();
    for ph in &phases {
        for c in ph {
            env.push(*c);
            env.push(shift(c, total_hops));
        }
    }
    let frame = Frame::around(
        &rec.tiling,
        &li,
        &env,
        li.reach + 8 + st.margin_cells as i32,
    )?;
    let mut b = EMPTY_BOX;
    for ph in &phases {
        for c in ph {
            for k in [0, hops] {
                if let Some(i) = frame.index(&shift(c, k)) {
                    bbox_union(&mut b, &frame.topo.polygon(i), [0.0, 0.0]);
                }
            }
        }
    }
    let cs = crate::world::cell_size(&frame.topo);
    let win = grow(b, st.margin_cells as f64 * cs);
    let make = || -> Result<World, String> {
        let mut w = rule_world(rec, frame.topo.clone())?;
        let id = w.cas[0].id;
        w.apply(Command::SetCells {
            id,
            cells: frame.cells(&phases[0])?,
        })?;
        Ok(w)
    };
    let (frames, gens, win, s, px) = run_world(
        &make,
        warm..warm + n,
        1,
        Some(win),
        st.alone_side,
        st.min_cell_px,
        0.0,
        st,
    )?;
    Ok(Animation {
        frames,
        generations: gens,
        delay_ms: st.alone_frame_ms,
        mode: MediaMode::Alone,
        poster: 0,
        scale: s,
        cell_px: px,
        window: win,
        seamless: true,
        window_kind: "stream",
    })
}

/// Puffers and other lattice records: run from the start, window around all they reach.
fn alone_run_lattice(rec: &PatternRecord, o: &MediaOptions) -> Result<Animation, String> {
    let st = &o.style;
    let p = rec.period.max(1);
    let n = (p * o.periods.max(4)).clamp(16, 240);
    let li = lattice_info(&rec.tiling, rec.neighborhood)?;
    let start = rec.phase(0)?;
    let d = rec.displacement.unwrap_or([0, 0]);
    let k = (n / p + 2) as i32;
    let mut env = start.clone();
    env.extend(start.iter().map(|c| LCell {
        i: c.i + d[0] * k,
        j: c.j + d[1] * k,
        ..*c
    }));
    let frame = Frame::around(&rec.tiling, &li, &env, li.reach + 24)?;
    let make = || -> Result<World, String> {
        let mut w = rule_world(rec, frame.topo.clone())?;
        let id = w.cas[0].id;
        w.apply(Command::SetCells {
            id,
            cells: frame.cells(&start)?,
        })?;
        Ok(w)
    };
    let (frames, gens, win, s, px) = run_world(
        &make,
        0..n,
        1,
        None,
        st.alone_side,
        st.min_cell_px,
        st.margin_cells as f64,
        st,
    )?;
    let last = frames.len() - 1;
    Ok(Animation {
        frames,
        generations: gens,
        delay_ms: st.alone_frame_ms,
        mode: MediaMode::Alone,
        poster: last,
        scale: s,
        cell_px: px,
        window: win,
        seamless: false,
        window_kind: "reach",
    })
}

/// Movers on aperiodic patches: from the pattern's site on its patch.
fn alone_run_patch(rec: &PatternRecord, o: &MediaOptions) -> Result<Animation, String> {
    let st = &o.style;
    let sc = crate::pattern::promote::showcase_scene(rec, None, None)?;
    let p = rec.period.max(1);
    let n = (p * o.periods.max(8)).clamp(16, 240);
    let (frames, gens, win, s, px) = run_world(
        &|| World::from_scene(&sc),
        0..n,
        1,
        None,
        st.alone_side,
        st.min_cell_px,
        st.margin_cells as f64,
        st,
    )?;
    let last = frames.len() - 1;
    Ok(Animation {
        frames,
        generations: gens,
        delay_ms: st.alone_frame_ms,
        mode: MediaMode::Alone,
        poster: last,
        scale: s,
        cell_px: px,
        window: win,
        seamless: false,
        window_kind: "reach",
    })
}

/// A pattern's animation in memory (for the app: Quick Start cards, the hunt dashboard).
/// `Auto` and `Alone` show it alone; `Seed` runs its showcase scene (the pattern pasted on a
/// wrapped grid) from generation 0.
pub fn pattern_animation(rec: &PatternRecord, o: &MediaOptions) -> Result<Animation, String> {
    match o.mode {
        MediaMode::Seed => {
            let sc = crate::pattern::promote::showcase_scene(rec, None, None)?;
            scene_animation(&sc, o)
        }
        _ if rec.site.is_some() => alone_run_patch(rec, o),
        _ => match rec.class {
            PatternClass::StillLife | PatternClass::Oscillator | PatternClass::Spaceship => {
                alone_periodic(rec, o)
            }
            PatternClass::Gun => alone_gun(rec, o),
            _ => alone_run_lattice(rec, o),
        },
    }
}

/// Generations a scene runs from its seed by default: one per row for row rules (capped at
/// 1,024), else the style's `seed_gens`.
pub fn default_seed_gens(w: &World, style: &MediaStyle) -> u64 {
    if w.cas.iter().any(|c| c.rule.is_rows()) {
        let rows = match w.topo.lattice() {
            Some(g) => g.h as u64,
            None => style.seed_gens,
        };
        return rows.clamp(1, 1024);
    }
    style.seed_gens
}

/// A scene from its seed: the whole grid, generation 0 to `gens` (every `every`).
pub fn scene_animation(scene: &Scene, o: &MediaOptions) -> Result<Animation, String> {
    let st = &o.style;
    let w = World::from_scene(scene)?;
    let start = w.generation;
    let gens = o.gens.unwrap_or_else(|| default_seed_gens(&w, st));
    let win = w.topo.bounds;
    drop(w);
    let (frames, generations, win, s, px) = run_world(
        &|| World::from_scene(scene),
        start..start + gens + 1,
        o.every,
        Some(win),
        st.seed_side,
        st.seed_min_cell_px,
        0.0,
        st,
    )?;
    let last = frames.len() - 1;
    Ok(Animation {
        frames,
        generations,
        delay_ms: st.seed_frame_ms,
        mode: MediaMode::Seed,
        poster: last,
        scale: s,
        cell_px: px,
        window: win,
        seamless: false,
        window_kind: "grid",
    })
}

/// A pattern's thumbnail in memory: its poster frame (alone) shrunk to the style's
/// `thumb_side`.
pub fn pattern_thumbnail(rec: &PatternRecord, style: &MediaStyle) -> Result<Image, String> {
    let o = MediaOptions {
        style: style.clone(),
        ..Default::default()
    };
    Ok(pattern_animation(rec, &o)?.thumbnail(style))
}

// ---------------------------------------------------------------------------------------------
// Targets and the catalog

/// What media are made for.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaTarget {
    /// A catalog pattern by id (or unique prefix, or name).
    Pattern(String),
    /// A Quick Start entry by id.
    QuickStart(String),
    /// An ant run by id ([`crate::ants::media`]): alone is the moving highway loop, seed the
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

fn fnv(b: &[u8]) -> u64 {
    let mut h = Fnv::default();
    h.bytes(b);
    h.0
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

/// FNV-1a 64 of bytes as 16 hex digits (the content hashes of media and the website export).
pub fn content_hash(b: &[u8]) -> String {
    hex(fnv(b))
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
    Ant(Box<crate::library::AntRow>),
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
                    crate::pattern::promote::showcase_scene(&rec, None, None)?,
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
            let highway = a.outcome == crate::ants::run::Outcome::Highway;
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
                _ => Source::Scene(Box::new(crate::ants::media::build_scene(&a)?)),
            };
            gens = Some(crate::ants::media::build_gens(&a));
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
        Source::Ant(a) => crate::ants::media::highway_animation(a, o)?,
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
        "jetcarp_version": crate::VERSION,
    });
    let mut render = render;
    if let Some(p) = &st.palette {
        render["palette"] = p.clone().into();
    }
    let now = crate::pattern::now_utc();
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
        let mut h_all = Fnv::default();
        let mut total = 0u64;
        for (k, b) in pngs.iter().enumerate() {
            put(&format!("frames/{k:04}.png"), b)?;
            h_all.u64(fnv(b));
            total += b.len() as u64;
        }
        rows.push(row(
            "frames",
            rel_of("frames"),
            w,
            h,
            pngs.len() as u32,
            total,
            hex(h_all.0),
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
            crate::ants::run::Outcome::Highway,
            crate::ants::run::Outcome::Symmetric,
        ] {
            let mut a = lib.ants(&crate::library::AntQuery {
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
        source: Source::Scene(Box::new(Scene::new(crate::topology::TilingSpec::new(
            "square",
        )))),
        gens: None,
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_files(dir, "", anim, o, &job)
}
