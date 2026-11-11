//! Media for the catalog and the website: per-step frames (PNG), an animation (GIF or animated
//! WebP), a poster frame, and a thumbnail, for a catalog pattern, a Quick Start entry, an ant run
//! (`jetcarp_store::media::ants`: a highway's moving loop alone, the build-up from step 0), or any
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
//! **Storage** in the catalog (the media index: targets, rows, render keys, and rendering many
//! targets at once) is `jetcarp_store::media`.

pub mod encode;
mod vp8l;

use crate::objects::{LCell, Phase};
use crate::pattern::verify::{Frame, envelope, lattice_info, rule_world};
use crate::pattern::{PatternClass, PatternRecord};
use crate::render::{self, Colorer, Image, Palette, RenderOptions, Rgba, ViewColors};
use crate::rng::Fnv;
use crate::rules::{FieldKind, Rule};
use crate::world::{Command, Scene, World};
use serde::{Deserialize, Serialize};

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
pub fn draw(
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
pub fn fit_scale(ww: f64, wh: f64, side: u32, cs: f64, min_px: f64, max_px: f64) -> (f64, f64) {
    let s = side as f64 / ww.max(wh).max(1e-9);
    let mut px = (s * cs).clamp(min_px, max_px.max(min_px));
    if px >= 2.0 {
        px = px.floor();
    }
    (px / cs, px)
}

pub fn bbox_union(b: &mut [f64; 4], poly: &[[f64; 2]], off: [f64; 2]) {
    for p in poly {
        b[0] = b[0].min(p[0] - off[0]);
        b[1] = b[1].min(p[1] - off[1]);
        b[2] = b[2].max(p[0] - off[0]);
        b[3] = b[3].max(p[1] - off[1]);
    }
}

pub const EMPTY_BOX: [f64; 4] = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];

pub fn grow(b: [f64; 4], m: f64) -> [f64; 4] {
    [b[0] - m, b[1] - m, b[2] + m, b[3] + m]
}

pub fn pixels(b: [f64; 4], s: f64) -> (u32, u32) {
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
// Hashes

/// FNV-1a 64 of bytes.
pub fn fnv(b: &[u8]) -> u64 {
    let mut h = Fnv::default();
    h.bytes(b);
    h.0
}

/// A 64-bit hash as 16 hex digits.
pub fn hex(v: u64) -> String {
    format!("{v:016x}")
}

/// FNV-1a 64 of bytes as 16 hex digits (the content hashes of media and the website export).
pub fn content_hash(b: &[u8]) -> String {
    hex(fnv(b))
}
