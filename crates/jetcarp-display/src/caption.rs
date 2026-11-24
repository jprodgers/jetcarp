//! The caption: a few lines saying what is on screen (the rule, the grid, the rotation). The words
//! come from the world here, so every display says the same thing. The app draws them with egui;
//! [`CaptionPainter`] draws them on a plain wgpu surface, in the font the app's setting names
//! ([`FontWatch`]): Atkinson Hyperlegible Next by default ([`crate::fonts`]), with egui's Ubuntu
//! Light behind it for what Atkinson lacks (Greek). Its fonts are read with skrifa and its letters
//! drawn with vello_cpu, the font reader and rasteriser egui draws the app's words with ([`Face`]).

use crate::fonts::{Fallbacks, FontChoice};
use jetcarp_core::rules::{self, Rule};
use jetcarp_core::topology::{Boundary, Topology};
use jetcarp_core::{World, families};
use serde::{Deserialize, Serialize};
use skrifa::raw::TableProvider as _;
use skrifa::{FontRef, GlyphId, MetadataProvider as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Corner {
    #[default]
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::BottomLeft,
        Corner::BottomRight,
        Corner::TopLeft,
        Corner::TopRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
        }
    }

    pub fn right(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::TopRight)
    }

    pub fn bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct CaptionSettings {
    pub show: bool,
    pub corner: Corner,
    /// Height of the first line, in points.
    pub size: f32,
    /// Add the running generation count.
    pub generation: bool,
    /// Seconds to stay up after each new item (0: always).
    pub seconds: f32,
}

impl Default for CaptionSettings {
    fn default() -> Self {
        CaptionSettings {
            show: false,
            corner: Corner::BottomLeft,
            size: 20.0,
            generation: false,
            seconds: 0.0,
        }
    }
}

impl CaptionSettings {
    /// How opaque the caption is `age` seconds after the item started (0: not drawn). Fades out
    /// over its last second.
    pub fn opacity(&self, age: f64) -> f32 {
        if !self.show {
            0.0
        } else if self.seconds <= 0.0 {
            1.0
        } else {
            (self.seconds as f64 + 1.0 - age).clamp(0.0, 1.0) as f32
        }
    }
}

/// Pixels along each edge of a surface that something else covers, which the caption keeps clear
/// of: on a wallpaper, the Windows taskbar, or the macOS menu bar and Dock (the monitor less its
/// work area, [`crate::workarea`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    /// From left, top, right and bottom, in pixels.
    pub fn from_pixels([left, top, right, bottom]: [u32; 4]) -> Insets {
        Insets {
            left: left as f32,
            top: top as f32,
            right: right as f32,
            bottom: bottom as f32,
        }
    }
}

/// Where a caption of `size` pixels goes on a target of `target` pixels: `margin` in from its
/// corner of the part `insets` leave uncovered.
pub fn caption_origin(
    corner: Corner,
    target: [f32; 2],
    size: [f32; 2],
    margin: f32,
    insets: Insets,
) -> [f32; 2] {
    let x = if corner.right() {
        target[0] - insets.right - margin - size[0]
    } else {
        insets.left + margin
    };
    let y = if corner.bottom() {
        target[1] - insets.bottom - margin - size[1]
    } else {
        insets.top + margin
    };
    [x, y]
}

/// Where the shown item sits in its rotation.
#[derive(Clone, Copy, Debug)]
pub struct Place<'a> {
    pub rotation: &'a str,
    /// Counted from 1.
    pub number: usize,
    pub count: usize,
}

const MAX_NOTATION: usize = 32;

fn rule_name(rule: &Rule, nb: jetcarp_core::topology::Nb, topo: &Topology) -> String {
    if let Rule::Wolfram(n) = rule {
        return format!("Rule {n}");
    }
    let notation = rule.notation();
    let name = rules::catalog_name_on(rule, nb, topo)
        .map(str::to_string)
        .or_else(|| {
            rules::named_rules()
                .iter()
                .find(|r| r.notation == notation)
                .map(|r| r.name.clone())
        });
    match name {
        Some(n) if n == notation => n,
        Some(n) if notation.chars().count() <= MAX_NOTATION => format!("{n} · {notation}"),
        Some(n) => n,
        None if notation.chars().count() <= MAX_NOTATION => notation,
        None => {
            let short: String = notation.chars().take(MAX_NOTATION).collect();
            format!("{short}…")
        }
    }
}

/// The rules running, once each: `Life · B3/S23`, or `A + B + 2 more`.
pub fn rule_line(world: &World) -> String {
    let mut names: Vec<String> = vec![];
    for c in world.cas.iter().filter(|c| c.enabled) {
        let n = rule_name(&c.rule, c.nb, &world.topo);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    match names.len() {
        0 => "No rule running".into(),
        1..=2 => names.join(" + "),
        n => format!("{} + {} + {} more", names[0], names[1], n - 2),
    }
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The grid: `Square grid · 480 × 270 · wraps`, or `Penrose rhombs (P3) · 12,345 cells`.
pub fn grid_line(world: &World) -> String {
    let spec = &world.topo.spec;
    let name = match spec.family.as_str() {
        "square" => "Square grid".to_string(),
        "hex" | "hex_flat" => "Hexagon grid".to_string(),
        "triangle" => "Triangle grid".to_string(),
        f => families::family(f).map_or_else(|| f.to_string(), |x| x.name.to_string()),
    };
    let size = match world.topo.lattice() {
        Some(g) if g.ntypes == 1 => format!("{} × {}", grouped(g.w as u64), grouped(g.h as u64)),
        _ => format!("{} cells", grouped(world.topo.len() as u64)),
    };
    let edges = world.topo.lattice().map(|_| match spec.boundary {
        Boundary::Wrap => "wraps",
        Boundary::Dead => "dead edges",
        Boundary::Live => "live edges",
    });
    match edges {
        Some(e) => format!("{name} · {size} · {e}"),
        None => format!("{name} · {size}"),
    }
}

/// The caption's lines, most important first.
pub fn lines(world: &World, place: Option<Place<'_>>, generation: bool) -> Vec<String> {
    let mut out = vec![rule_line(world), grid_line(world)];
    let mut third = vec![];
    if let Some(p) = place.filter(|p| p.count > 1) {
        third.push(format!("{} · {} of {}", p.rotation, p.number, p.count));
    }
    if generation {
        third.push(format!("generation {}", grouped(world.generation)));
    }
    if !third.is_empty() {
        out.push(third.join(" · "));
    }
    out
}

/// Later lines are this much smaller than the first.
pub const LATER_LINES: f32 = 0.75;
const PADDING: f32 = 0.6;
const MARGIN: f32 = 1.2;
const BACKDROP_ALPHA: u8 = 170;

const SHADER: &str = r#"
struct U { rect: vec4<f32>, alpha: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    let uv = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    var o: V;
    let p = u.rect.xy + uv * u.rect.zw;
    o.pos = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, v.uv) * u.alpha.x;
}
"#;

struct Picture {
    bind: wgpu::BindGroup,
    size: [u32; 2],
}

/// The caption's last resort (#273), as the app has: a character none of its fonts has is drawn in
/// an installed font that has it ([`Fallbacks`]). The installed fonts' character maps are read
/// once, on a thread of their own, so no frame waits for them (such a character is a box until
/// then, and the caption is drawn again once they are read); a font file is read whole the first
/// time one of its characters is drawn, and kept.
pub struct LastResort {
    index: OnceLock<Fallbacks>,
    faces: Mutex<Faces>,
}

/// Larger files are not read for one character (as the app's last resort).
const MAX_FALLBACK_FILE: u64 = 256 << 20;

/// The last resort's fonts read so far.
#[derive(Default)]
struct Faces {
    /// In the order first needed.
    list: Vec<Face>,
    /// Where each file and face is in `list` (None: it couldn't be read).
    by_file: std::collections::HashMap<(PathBuf, u32), Option<usize>>,
}

impl LastResort {
    /// Start reading the character maps of the fonts in `dirs` ([`crate::fonts::system_dirs`]),
    /// on a thread of their own. With no folders there is nothing to read.
    pub fn start(dirs: Vec<PathBuf>) -> Arc<LastResort> {
        Self::start_with(move || Fallbacks::find(&dirs))
    }

    /// Start `scan` on a thread of its own, its fonts used once it returns.
    pub fn start_with(scan: impl FnOnce() -> Fallbacks + Send + 'static) -> Arc<LastResort> {
        let last = Arc::new(LastResort {
            index: OnceLock::new(),
            faces: Default::default(),
        });
        let filled = Arc::clone(&last);
        let spawned = std::thread::Builder::new()
            .name("caption font fallback scan".into())
            .spawn(move || {
                let _ = filled.index.set(scan());
            });
        if spawned.is_err() {
            // No thread: no last resort, rather than a wait.
            let _ = last.index.set(Fallbacks::default());
        }
        last
    }

    /// The fonts `index` names, ready now: for fonts copied beside the program (the macOS screen
    /// saver's bundle).
    pub fn ready(index: Fallbacks) -> Arc<LastResort> {
        let index = OnceLock::from(index);
        Arc::new(LastResort {
            index,
            faces: Default::default(),
        })
    }

    /// Whether the character maps have been read.
    pub fn is_ready(&self) -> bool {
        self.index.get().is_some()
    }

    /// Where in the list the font that draws `c` is, read the first time.
    fn face_for(&self, c: char) -> Option<usize> {
        let f = self.index.get()?.for_char(c)?;
        let mut faces = self.faces.lock().unwrap_or_else(|e| e.into_inner());
        let key = (f.path.clone(), f.index);
        if let Some(at) = faces.by_file.get(&key) {
            return *at;
        }
        let font = std::fs::metadata(&f.path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= MAX_FALLBACK_FILE)
            .and_then(|_| std::fs::read(&f.path).ok())
            .and_then(|b| Face::read(b, f.index));
        let at = font.map(|font| {
            faces.list.push(font);
            faces.list.len() - 1
        });
        faces.by_file.insert(key, at);
        at
    }

    /// The `i`th font read.
    fn face(&self, i: usize) -> Face {
        let faces = self.faces.lock().unwrap_or_else(|e| e.into_inner());
        faces.list[i].clone()
    }
}

/// The caption's fonts: Atkinson Hyperlegible Next (or the font chosen in the app), then Ubuntu
/// Light (egui's) for a character it lacks, then, as a last resort, an installed font that has it
/// ([`LastResort`]). Each character is drawn in the first that has it.
#[derive(Clone)]
pub struct CaptionFont {
    fonts: Vec<Face>,
    last: Option<Arc<LastResort>>,
}

fn builtin(bytes: &'static [u8]) -> Face {
    Face::new(Bytes::Builtin(bytes), 0).expect("a font that comes with JETCARP")
}

impl Default for CaptionFont {
    fn default() -> Self {
        CaptionFont {
            fonts: vec![
                builtin(crate::fonts::NEXT_REGULAR),
                builtin(epaint_default_fonts::UBUNTU_LIGHT),
            ],
            last: None,
        }
    }
}

impl CaptionFont {
    /// The caption's fonts for the app's font setting `choice`, as the app draws its words: the
    /// chosen font with egui's Ubuntu Light behind it. A font file that can't be used is an error
    /// that names the file.
    pub fn for_choice(choice: &FontChoice) -> Result<CaptionFont, String> {
        let ubuntu = builtin(epaint_default_fonts::UBUNTU_LIGHT);
        match choice {
            FontChoice::Atkinson => Ok(CaptionFont::default()),
            FontChoice::Egui => Ok(CaptionFont {
                fonts: vec![ubuntu],
                last: None,
            }),
            FontChoice::File { path, index, .. } => {
                let file = crate::fonts::load(path, Some(*index))?;
                let font = Face::read(file.bytes, file.index).ok_or_else(|| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("{name} isn't a font JETCARP can read.")
                })?;
                Ok(CaptionFont {
                    fonts: vec![font, ubuntu],
                    last: None,
                })
            }
        }
    }

    /// These fonts with `last` behind them, for characters none of them has.
    pub fn with_last_resort(self, last: Arc<LastResort>) -> CaptionFont {
        CaptionFont {
            last: Some(last),
            ..self
        }
    }

    /// Whether the last resort's fonts are ready (true without one): the caption is drawn again
    /// once when they become so.
    pub fn last_resort_ready(&self) -> bool {
        self.last.as_ref().is_none_or(|l| l.is_ready())
    }

    /// The `i`th font: those in place, then the last resort's.
    fn font(&self, i: usize) -> Face {
        match (self.fonts.get(i), &self.last) {
            (Some(f), _) => f.clone(),
            (None, Some(l)) => l.face(i - self.fonts.len()),
            (None, None) => self.fonts[0].clone(),
        }
    }

    /// The font that draws `c` (the first, for a character none has), and its glyph there. Past
    /// the fonts in place, the last resort's.
    pub fn glyph(&self, c: char) -> (usize, GlyphId) {
        for (i, f) in self.fonts.iter().enumerate() {
            let g = f.glyph_id(c);
            if g != GlyphId::NOTDEF {
                return (i, g);
            }
        }
        if let Some(l) = &self.last
            && let Some(i) = l.face_for(c)
        {
            let g = l.face(i).glyph_id(c);
            if g != GlyphId::NOTDEF {
                return (self.fonts.len() + i, g);
            }
        }
        (0, self.fonts[0].glyph_id(c))
    }

    /// The advance from glyph `last` to `g` at `px`, with their kerning when one font has both.
    fn advance(&self, last: Option<(usize, GlyphId)>, (i, g): (usize, GlyphId), px: f32) -> f32 {
        let font = self.font(i);
        let kern = match last {
            Some((j, l)) if j == i => font.kern(l, g, px),
            _ => 0.0,
        };
        kern + font.h_advance(g, px)
    }

    /// Ascent, descent and line gap at `px` (of the first font: lines keep one height).
    fn metrics(&self, px: f32) -> (f32, f32, f32) {
        self.fonts[0].metrics(px)
    }

    pub fn line_width(&self, text: &str, px: f32) -> f32 {
        let mut w = 0.0;
        let mut last = None;
        for c in text.chars() {
            let g = self.glyph(c);
            w += self.advance(last, g, px);
            last = Some(g);
        }
        w
    }

    /// `text`, cut with an ellipsis to fit `max` pixels.
    fn fit(&self, text: &str, px: f32, max: f32) -> String {
        if self.line_width(text, px) <= max {
            return text.to_string();
        }
        let mut t: Vec<char> = text.chars().collect();
        while !t.is_empty() {
            t.pop();
            let s = format!("{}…", t.iter().collect::<String>());
            if self.line_width(&s, px) <= max {
                return s;
            }
        }
        String::new()
    }

    /// The caption as premultiplied RGBA: (pixels, width, height).
    pub fn rasterize(&self, lines: &[String], first_px: f32, max_w: f32) -> (Vec<u8>, u32, u32) {
        let pad = (first_px * PADDING).ceil();
        let inner = (max_w - 2.0 * pad).max(1.0);
        let px_of = |i: usize| {
            if i == 0 {
                first_px
            } else {
                first_px * LATER_LINES
            }
        };
        let fitted: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(i, l)| self.fit(l, px_of(i), inner))
            .collect();
        let width = fitted
            .iter()
            .enumerate()
            .map(|(i, l)| self.line_width(l, px_of(i)))
            .fold(0.0, f32::max);
        let line_h = |i: usize| {
            let (a, d, g) = self.metrics(px_of(i));
            a - d + g
        };
        let height: f32 = (0..fitted.len()).map(line_h).sum();
        let (w, h) = (
            (width + 2.0 * pad).ceil() as u32,
            (height + 2.0 * pad).ceil() as u32,
        );
        let mut img = vec![0u8; (w * h * 4) as usize];
        for p in img.as_chunks_mut::<4>().0 {
            p[3] = BACKDROP_ALPHA;
        }
        let mut y = pad;
        let mut pen = Pen::new();
        for (i, line) in fitted.iter().enumerate() {
            let px = px_of(i);
            let baseline = y + self.metrics(px).0;
            let mut x = pad;
            let mut last = None;
            for c in line.chars() {
                let (fi, id) = self.glyph(c);
                let font = &self.font(fi);
                if let Some((j, l)) = last
                    && j == fi
                {
                    x += font.kern(l, id, px);
                }
                pen.draw(font, id, px, [x, baseline], |gx, gy, value| {
                    if gx < 0 || gy < 0 || gx >= w as i64 || gy >= h as i64 {
                        return;
                    }
                    let at = (gy as usize * w as usize + gx as usize) * 4;
                    let a = BACKDROP_ALPHA as f32 / 255.0;
                    let cov = value as f32 / 255.0;
                    // White text over the backdrop, premultiplied.
                    let alpha = ((cov + (1.0 - cov) * a) * 255.0).round() as u8;
                    img[at] = img[at].max(value);
                    img[at + 1] = img[at + 1].max(value);
                    img[at + 2] = img[at + 2].max(value);
                    img[at + 3] = img[at + 3].max(alpha);
                });
                x += font.h_advance(id, px);
                last = Some((fi, id));
            }
            y += line_h(i);
        }
        (img, w, h)
    }
}

/// A font file's bytes: built into JETCARP, or read from a file.
#[derive(Clone)]
enum Bytes {
    Builtin(&'static [u8]),
    Read(Arc<Vec<u8>>),
}

impl Bytes {
    fn get(&self) -> &[u8] {
        match self {
            Bytes::Builtin(b) => b,
            Bytes::Read(b) => b,
        }
    }
}

/// One face of a font file, as the caption draws it. It is read with skrifa, egui's own font
/// reader, and measured as JETCARP measured it with ab_glyph before 0.4.3-rc.3, so captions keep
/// their sizes and layout: a line `px` pixels high is `px` from the face's ascent to its descent,
/// and letters are spaced by their advances and the font's `kern` table (not GPOS, as before).
#[derive(Clone)]
struct Face {
    bytes: Bytes,
    index: u32,
    /// How characters are looked up in its character map, worked out once.
    charmap: skrifa::charmap::MappingIndex,
    /// Ascent, descent and line gap, in font units ([`line_metrics`]).
    ascent: f32,
    descent: f32,
    line_gap: f32,
    glyphs: u32,
}

impl Face {
    /// Face `index` of `bytes`, if the caption can draw in it ([`can_draw_in`]).
    fn new(bytes: Bytes, index: u32) -> Option<Face> {
        let font = FontRef::from_index(bytes.get(), index).ok()?;
        let (glyphs, [ascent, descent, line_gap]) = sizes(&font)?;
        let charmap = skrifa::charmap::MappingIndex::new(&font);
        Some(Face {
            charmap,
            ascent,
            descent,
            line_gap,
            glyphs,
            bytes,
            index,
        })
    }

    /// Face `index` of a font file read whole.
    fn read(bytes: Vec<u8>, index: u32) -> Option<Face> {
        Face::new(Bytes::Read(Arc::new(bytes)), index)
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::from_index(self.bytes.get(), self.index).expect("read when the face was made")
    }

    /// Pixels per font unit for lines `px` high.
    fn scale(&self, px: f32) -> f32 {
        let height = self.ascent - self.descent;
        if height > 0.0 { px / height } else { 0.0 }
    }

    fn glyph_id(&self, c: char) -> GlyphId {
        let font = self.font();
        self.charmap
            .charmap(&font)
            .map(c)
            .unwrap_or(GlyphId::NOTDEF)
    }

    /// Ascent, descent and line gap at `px`.
    fn metrics(&self, px: f32) -> (f32, f32, f32) {
        let s = self.scale(px);
        (s * self.ascent, s * self.descent, s * self.line_gap)
    }

    /// Glyph `g`'s advance at `px` (none for a glyph the face doesn't have).
    fn h_advance(&self, g: GlyphId, px: f32) -> f32 {
        let font = self.font();
        let units = match font.hmtx() {
            Ok(hmtx) if g.to_u32() < self.glyphs => hmtx.advance(g).unwrap_or(0),
            _ => 0,
        };
        self.scale(px) * f32::from(units)
    }

    /// The kerning between glyphs `l` and `r` at `px`, from the font's `kern` table: the first of
    /// its horizontal subtables that has the pair.
    fn kern(&self, l: GlyphId, r: GlyphId, px: f32) -> f32 {
        use skrifa::raw::tables::kern::SubtableKind;
        let font = self.font();
        let Ok(kern) = font.kern() else {
            return 0.0;
        };
        let units = kern
            .subtables()
            .filter_map(Result::ok)
            .filter(|st| st.is_horizontal() && !st.is_variable())
            .find_map(|st| match st.kind().ok()? {
                SubtableKind::Format0(t) => t.kerning(l, r),
                SubtableKind::Format2(t) => t.kerning(l, r),
                SubtableKind::Format3(t) => t.kerning(l, r),
                SubtableKind::Format1(_) => None,
            })
            .unwrap_or(0);
        self.scale(px) * units as f32
    }
}

/// Whether the caption can draw in face `index` of the font file `bytes`: skrifa reads it, and it
/// has the tables a face's size and spacing come from (`head`, `hhea`, `maxp`), as ab_glyph
/// required before 0.4.3-rc.3.
pub fn can_draw_in(bytes: &[u8], index: u32) -> bool {
    FontRef::from_index(bytes, index).is_ok_and(|font| sizes(&font).is_some())
}

/// A face's number of glyphs, and its ascent, descent and line gap in font units.
fn sizes(font: &FontRef) -> Option<(u32, [f32; 3])> {
    font.head().ok()?;
    let glyphs = u32::from(font.maxp().ok()?.num_glyphs());
    let (ascent, descent, gap) = line_metrics(font)?;
    Some((glyphs, [ascent, descent, gap]))
}

/// A face's ascent, descent and line gap in font units, as ttf-parser chose them for ab_glyph
/// (`Face::ascender`, `descender` and `line_gap` in ttf-parser 0.25): OS/2's typographic values
/// when the font asks for them, else hhea's, with OS/2's in place of zeros. None without hhea.
fn line_metrics(font: &FontRef) -> Option<(f32, f32, f32)> {
    let table = |tag: &[u8; 4]| font.table_data(skrifa::Tag::new(tag)).map(|d| d.as_bytes());
    let i16_at = |b: &[u8], at: usize| b.get(at..at + 2).map(|v| i16::from_be_bytes([v[0], v[1]]));
    let hhea = table(b"hhea").filter(|b| b.len() >= 36)?;
    let (h_ascent, h_descent, h_gap) = (i16_at(hhea, 4)?, i16_at(hhea, 6)?, i16_at(hhea, 8)?);
    // ttf-parser read OS/2 only when it was as long as its version says.
    let os2 = table(b"OS/2").filter(|b| {
        let need = match i16_at(b, 0).map(|v| v as u16) {
            Some(0) => 78,
            Some(1) => 86,
            Some(2..=4) => 96,
            Some(5) => 100,
            _ => usize::MAX,
        };
        b.len() >= need
    });
    let Some(os2) = os2 else {
        return Some((h_ascent.into(), h_descent.into(), h_gap.into()));
    };
    let at = |offset| i16_at(os2, offset).unwrap_or(0);
    let (version, selection) = (at(0) as u16, at(62) as u16);
    let (typo_ascent, typo_descent, typo_gap) = (at(68), at(70), at(72));
    let (win_ascent, win_descent) = (at(74), at(76).wrapping_neg());
    if version >= 4 && selection & (1 << 7) != 0 {
        return Some((typo_ascent.into(), typo_descent.into(), typo_gap.into()));
    }
    let or_os2 = |hhea: i16, typo: i16, win: i16| match (hhea, typo) {
        (0, 0) => win,
        (0, t) => t,
        (h, _) => h,
    };
    let ascent = or_os2(h_ascent, typo_ascent, win_ascent);
    let descent = or_os2(h_descent, typo_descent, win_descent);
    let gap = if h_ascent == 0 || h_descent == 0 {
        if typo_ascent != 0 || typo_descent != 0 {
            typo_gap
        } else {
            0
        }
    } else {
        h_gap
    };
    Some((ascent.into(), descent.into(), gap.into()))
}

/// The widest or tallest glyph drawn, in pixels: a caption's letters are at most 200 pixels high.
const MAX_GLYPH_SIDE: i64 = 4096;

/// Draws glyphs' outlines with vello_cpu, one glyph at a time, as ab_glyph drew them: the outline
/// scaled to the line's size, placed with its origin at the pen's point on the baseline, covering
/// the whole pixels its outline's box touches.
struct Pen {
    path: vello_cpu::kurbo::BezPath,
    resources: vello_cpu::Resources,
    /// Made for the first glyph, and sized again for each.
    ctx: Option<vello_cpu::RenderContext>,
}

impl Pen {
    fn new() -> Pen {
        Pen {
            path: Default::default(),
            resources: vello_cpu::Resources::new(),
            ctx: None,
        }
    }

    /// Draw glyph `g` of `face` at `px` with its origin at `at`, calling `put` with each pixel
    /// its box covers and how much of the pixel the glyph covers (0 to 255).
    fn draw(
        &mut self,
        face: &Face,
        g: GlyphId,
        px: f32,
        at: [f32; 2],
        mut put: impl FnMut(i64, i64, u8),
    ) {
        use skrifa::instance::{LocationRef, Size};
        use vello_cpu::kurbo::Affine;
        let font = face.font();
        let Some(outline) = font.outline_glyphs().get(g) else {
            return;
        };
        self.path.truncate(0);
        let mut to = ToPixels {
            path: &mut self.path,
            scale: face.scale(px),
            at,
            now: at,
        };
        let settings =
            skrifa::outline::DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        if outline.draw(settings, &mut to).is_err() {
            return;
        }
        let b = self.path.control_box();
        let (x0, y0) = (b.x0.floor(), b.y0.floor());
        let (w, h) = ((b.x1.ceil() - x0) as i64, (b.y1.ceil() - y0) as i64);
        // Nothing to draw, or a glyph far larger than any letter a caption has (a broken font).
        if !(1..=MAX_GLYPH_SIDE).contains(&w) || !(1..=MAX_GLYPH_SIDE).contains(&h) {
            return;
        }
        let (w, h) = (w as u16, h as u16);
        let ctx = self
            .ctx
            .get_or_insert_with(|| vello_cpu::RenderContext::new(w, h));
        ctx.reset_and_resize(w, h);
        ctx.set_transform(Affine::translate((-x0, -y0)));
        ctx.set_paint(vello_cpu::color::OpaqueColor::<vello_cpu::color::Srgb>::WHITE);
        ctx.fill_path(&self.path);
        let mut pixmap = vello_cpu::Pixmap::new(w, h);
        ctx.render(&mut pixmap, &mut self.resources);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let pixels = pixmap.data_as_u8_slice().as_chunks::<4>().0;
        for (i, p) in pixels.iter().enumerate() {
            let (x, y) = ((i % w as usize) as i64, (i / w as usize) as i64);
            // Premultiplied white: the alpha is the coverage.
            put(x0 + x, y0 + y, p[3]);
        }
    }
}

/// A glyph's outline, in font units with y up, as a path of straight lines in the caption's
/// pixels: scaled, y down, from the pen's point. Its curves are cut into lines here, as ab_glyph
/// cut them (finer than vello_cpu's own quarter-pixel tolerance, which would flatten small letters'
/// curves visibly), all in f32, as ab_glyph worked them out.
struct ToPixels<'a> {
    path: &'a mut vello_cpu::kurbo::BezPath,
    scale: f32,
    at: [f32; 2],
    /// The point the outline is at, in pixels.
    now: [f32; 2],
}

impl ToPixels<'_> {
    fn pt(&self, x: f32, y: f32) -> [f32; 2] {
        let [ax, ay] = self.at;
        [ax + x * self.scale, ay - y * self.scale]
    }

    fn line(&mut self, p: [f32; 2]) {
        self.path.line_to((p[0] as f64, p[1] as f64));
        self.now = p;
    }

    /// A cubic curve cut in halves until each is nearly straight, as stb_truetype and
    /// ab_glyph_rasterizer cut them.
    fn cubic(&mut self, p: [[f32; 2]; 4], depth: u8) {
        let dist =
            |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let long = dist(p[0], p[1]) + dist(p[1], p[2]) + dist(p[2], p[3]);
        let short = dist(p[0], p[3]);
        if depth < 16 && long * long - short * short > 0.35 * 0.35 {
            let p01 = lerp(0.5, p[0], p[1]);
            let p12 = lerp(0.5, p[1], p[2]);
            let p23 = lerp(0.5, p[2], p[3]);
            let (pa, pb) = (lerp(0.5, p01, p12), lerp(0.5, p12, p23));
            let mid = lerp(0.5, pa, pb);
            self.cubic([p[0], p01, pa, mid], depth + 1);
            self.cubic([mid, pb, p23, p[3]], depth + 1);
        } else {
            self.line(p[3]);
        }
    }
}

/// `a` + (`b` − `a`) × `t`, as ab_glyph_rasterizer works it out.
fn lerp(t: f32, a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

impl skrifa::outline::OutlinePen for ToPixels<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.path.move_to((p[0] as f64, p[1] as f64));
        self.now = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.line(p);
    }

    /// In as many equal steps of the curve as its bend calls for, as ab_glyph_rasterizer cuts it.
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.now, self.pt(cx0, cy0), self.pt(x, y));
        let (dx, dy) = (p0[0] - 2.0 * p1[0] + p2[0], p0[1] - 2.0 * p1[1] + p2[1]);
        let dev = dx * dx + dy * dy;
        if dev < 0.333 {
            self.line(p2);
            return;
        }
        let n = 1 + (3.0 * dev).sqrt().sqrt().floor() as usize;
        let step = (n as f32).recip();
        let mut t = 0.0;
        for _ in 0..n - 1 {
            t += step;
            self.line(lerp(t, lerp(t, p0, p1), lerp(t, p1, p2)));
        }
        self.line(p2);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p = [
            self.now,
            self.pt(cx0, cy0),
            self.pt(cx1, cy1),
            self.pt(x, y),
        ];
        self.cubic(p, 0);
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// The caption's fonts as the app's font setting names them (`app.json` in the settings folder,
/// [`crate::fonts::saved_in`]), read again at each [`FontWatch::check`]: the desktop background
/// checks every couple of seconds, so a font chosen in the app shows without a restart.
pub struct FontWatch {
    settings: Option<PathBuf>,
    choice: FontChoice,
    font: Arc<CaptionFont>,
    /// Behind whichever font is chosen.
    last: Option<Arc<LastResort>>,
}

impl FontWatch {
    /// The fonts the setting in `settings` names now (none: JETCARP's), and a note when the chosen
    /// font can't be used (the caption is then in Atkinson Hyperlegible Next).
    pub fn new(settings: Option<PathBuf>) -> (FontWatch, Option<String>) {
        let mut w = FontWatch {
            settings,
            choice: FontChoice::Atkinson,
            font: Arc::new(CaptionFont::default()),
            last: None,
        };
        let choice = w.saved();
        let note = if choice == FontChoice::Atkinson {
            None
        } else {
            w.apply(choice).err()
        };
        (w, note)
    }

    /// With `last` behind the caption's fonts from now on, whichever is chosen: an installed font
    /// that has a character none of them has ([`LastResort`]).
    pub fn with_last_resort(mut self, last: Arc<LastResort>) -> FontWatch {
        let font = (*self.font).clone().with_last_resort(Arc::clone(&last));
        self.font = Arc::new(font);
        self.last = Some(last);
        self
    }

    /// Read the setting again. A note when the caption's font changed, or the newly chosen one
    /// can't be used.
    pub fn check(&mut self) -> Option<String> {
        let choice = self.saved();
        if choice == self.choice {
            return None;
        }
        let label = choice.label(false);
        match self.apply(choice) {
            Ok(()) => Some(format!(
                "the caption is in {label} now, the font chosen in the app"
            )),
            Err(note) => Some(note),
        }
    }

    /// The text font the app's setting names now.
    fn saved(&self) -> FontChoice {
        self.settings
            .as_deref()
            .map(|d| crate::fonts::saved_in(d).0)
            .unwrap_or_default()
    }

    /// Draw in `choice` from now on: Atkinson Hyperlegible Next, and a note, when it can't be
    /// used. The choice is kept either way, so a font that can't be used is tried again only
    /// when the setting changes.
    fn apply(&mut self, choice: FontChoice) -> Result<(), String> {
        let font = CaptionFont::for_choice(&choice);
        let label = choice.label(false);
        self.choice = choice;
        let behind = |f: CaptionFont| match &self.last {
            Some(l) => Arc::new(f.with_last_resort(Arc::clone(l))),
            None => Arc::new(f),
        };
        match font {
            Ok(f) => {
                self.font = behind(f);
                Ok(())
            }
            Err(e) => {
                self.font = behind(CaptionFont::default());
                Err(format!(
                    "the caption can't be drawn in {label}, the font chosen in the app ({}); it \
                     is in Atkinson Hyperlegible Next",
                    e.trim_end_matches('.')
                ))
            }
        }
    }

    /// The fonts to draw the caption in.
    pub fn font(&self) -> &Arc<CaptionFont> {
        &self.font
    }
}

/// Draws a caption on a plain wgpu target.
pub struct CaptionPainter {
    font: Arc<CaptionFont>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    picture: Option<Picture>,
    /// What the picture is of: the lines, the size, the room, and whether the last resort's fonts
    /// were ready.
    drawn: Option<(Vec<String>, u32, u32, bool)>,
    visible: bool,
}

impl CaptionPainter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> CaptionPainter {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("caption"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("caption"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let pipe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("caption"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("caption"),
            layout: Some(&pipe_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        CaptionPainter {
            font: Arc::new(CaptionFont::default()),
            pipeline,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor::default()),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("caption"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            picture: None,
            drawn: None,
            visible: false,
        }
    }

    /// Draw the caption in `font` from now on (laid out again at the next [`Self::prepare`]).
    pub fn set_font(&mut self, font: &Arc<CaptionFont>) {
        if !Arc::ptr_eq(&self.font, font) {
            self.font = Arc::clone(font);
            self.drawn = None;
        }
    }

    /// Prepares `lines` for a target of `target` pixels. `scale` is pixels per point; `opacity`
    /// comes from [`CaptionSettings::opacity`]. Text is laid out again only when it changes.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lines: &[String],
        settings: &CaptionSettings,
        target: [u32; 2],
        scale: f32,
        opacity: f32,
    ) {
        let none = Insets::default();
        self.prepare_within(device, queue, lines, settings, target, scale, opacity, none);
    }

    /// [`Self::prepare`], keeping clear of `insets` (a taskbar, the menu bar, the Dock).
    pub fn prepare_within(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lines: &[String],
        settings: &CaptionSettings,
        target: [u32; 2],
        scale: f32,
        opacity: f32,
        insets: Insets,
    ) {
        self.visible = false;
        let free_w = target[0] as f32 - insets.left - insets.right;
        let free_h = target[1] as f32 - insets.top - insets.bottom;
        if opacity <= 0.0 || lines.is_empty() || free_w < 16.0 || free_h < 16.0 {
            return;
        }
        let px = (settings.size * scale).clamp(6.0, 200.0);
        // Drawn again once when the last resort's fonts are ready, for a character only they have.
        let ready = self.font.last_resort_ready();
        let key = (lines.to_vec(), px.to_bits(), free_w as u32, ready);
        if self.drawn.as_ref() != Some(&key) {
            let margin = (px * MARGIN).ceil();
            let (img, w, h) = self.font.rasterize(lines, px, free_w - 2.0 * margin);
            let size = wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("caption"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &img,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                size,
            );
            let view = texture.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("caption"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.picture = Some(Picture { bind, size: [w, h] });
            self.drawn = Some(key);
        }
        let Some(p) = &self.picture else { return };
        let margin = (px * MARGIN).ceil();
        let (tw, th) = (target[0] as f32, target[1] as f32);
        let (w, h) = (p.size[0] as f32, p.size[1] as f32);
        let [x, y] = caption_origin(settings.corner, [tw, th], [w, h], margin, insets);
        let u: [f32; 8] = [x / tw, y / th, w / tw, h / th, opacity, 0.0, 0.0, 0.0];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&u));
        self.visible = true;
    }

    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (true, Some(p)) = (self.visible, &self.picture) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &p.bind, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::{Scene, SizeSpec, TilingSpec, topology};

    #[test]
    fn a_caption_keeps_clear_of_the_taskbar_and_the_menu_bar() {
        let t = [1920.0, 1080.0];
        let size = [400.0, 60.0];
        let none = Insets::default();
        assert_eq!(
            caption_origin(Corner::BottomLeft, t, size, 24.0, none),
            [24.0, 996.0]
        );
        // The Acer: a 48-pixel taskbar along the bottom.
        let taskbar = Insets::from_pixels([0, 0, 0, 48]);
        assert_eq!(
            caption_origin(Corner::BottomLeft, t, size, 24.0, taskbar),
            [24.0, 948.0]
        );
        assert_eq!(
            caption_origin(Corner::TopRight, t, size, 24.0, taskbar),
            [1496.0, 24.0]
        );
        // macOS: the menu bar above, the Dock on the left.
        let mac = Insets::from_pixels([140, 50, 0, 0]);
        assert_eq!(
            caption_origin(Corner::TopLeft, t, size, 24.0, mac),
            [164.0, 74.0]
        );
    }

    fn world(rule: &str, spec: TilingSpec) -> World {
        let mut sc = Scene::new(spec);
        sc.cas.push(jetcarp_core::CaSpec::new(rule));
        World::from_scene(&sc).expect("a world")
    }

    #[test]
    fn says_what_is_running() {
        let square = TilingSpec::rect("square", 480, 270, topology::Boundary::Wrap);
        let w = world("B3/S23", square.clone());
        let l = lines(&w, None, false);
        assert!(l[0].contains("Life"), "{l:?}");
        assert!(l[0].contains("B3/S23"), "{l:?}");
        assert_eq!(l[1], "Square grid · 480 × 270 · wraps");
        assert_eq!(l.len(), 2);

        let w = world("W110", square.clone());
        assert_eq!(rule_line(&w), "Rule 110");

        let place = Place {
            rotation: "Wolfram series",
            number: 31,
            count: 256,
        };
        let l = lines(&w, Some(place), true);
        assert_eq!(l[2], "Wolfram series · 31 of 256 · generation 0");

        // A lone item has no "1 of 1".
        let one = Place {
            rotation: "one",
            number: 1,
            count: 1,
        };
        assert_eq!(lines(&w, Some(one), false).len(), 2);

        let ap = TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(3));
        let w = world("B3/S23", ap);
        let g = grid_line(&w);
        assert!(g.starts_with("Penrose rhombs"), "{g}");
        assert!(g.ends_with(" cells"), "{g}");
    }

    /// The caption is drawn in Atkinson Hyperlegible Next, and a character it lacks in Ubuntu
    /// Light behind it.
    #[test]
    fn the_caption_is_in_atkinson_with_ubuntu_light_behind() {
        let f = CaptionFont::default();
        let atkinson = FontRef::new(crate::fonts::NEXT_REGULAR).unwrap().charmap();
        for c in "Life B3/S23 · 480 × 270 · wraps … généré".chars() {
            assert_eq!(f.glyph(c), (0, atkinson.map(c).unwrap()), "{c}");
        }
        // Greek letters Atkinson lacks.
        for c in "λφαβγ".chars() {
            assert_eq!(f.glyph(c).0, 1, "{c}");
            assert_ne!(f.glyph(c).1, GlyphId::NOTDEF, "{c}");
        }
        // Atkinson's widths, so its picture is not the Ubuntu one.
        let w = f.line_width("Square grid", 20.0);
        let uw = width_in(epaint_default_fonts::UBUNTU_LIGHT, "Square grid");
        assert!((w - uw).abs() > 1.0, "{w} vs Ubuntu Light's {uw}");
        let (img, iw, ih) = f.rasterize(&["Life".into(), "λ".into()], 20.0, 400.0);
        assert_eq!(img.len(), (iw * ih * 4) as usize);
        assert!(
            img.as_chunks::<4>().0.iter().any(|p| p[0] > 200),
            "text drawn"
        );
    }

    /// `text`'s width at 20 pixels in `font` alone.
    fn width_in(font: &'static [u8], text: &str) -> f32 {
        let alone = CaptionFont {
            fonts: vec![builtin(font)],
            last: None,
        };
        alone.line_width(text, 20.0)
    }

    /// The desktop background's caption is drawn in the font the app's setting names (`app.json`
    /// in the settings folder): a font file chosen there, from the next check without a restart,
    /// and at the next start; egui's original; Atkinson. A chosen file since removed gives
    /// Atkinson, and a note that names the file (never its folder).
    #[test]
    fn the_caption_follows_the_app_s_font_setting() {
        let dir = jetcarp_core::scratch::ScratchDir::new("caption-font");
        let settings = dir.join("settings");
        let fonts = dir.join("fonts");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::create_dir_all(&fonts).unwrap();
        let hack_file = fonts.join("Hack-Regular.ttf");
        std::fs::write(&hack_file, epaint_default_fonts::HACK_REGULAR).unwrap();
        let save = |font: serde_json::Value| {
            let app = serde_json::json!({
                "version": 1,
                "accessibility": { "font": font, "mono_font": "atkinson" },
            });
            std::fs::write(settings.join("app.json"), app.to_string()).unwrap();
        };
        let hack_choice = serde_json::json!({
            "file": { "family": "Hack", "path": hack_file, "index": 0 }
        });
        let text = "Square grid · 480 × 270";
        let atkinson = width_in(crate::fonts::NEXT_REGULAR, text);
        let hack = width_in(epaint_default_fonts::HACK_REGULAR, text);
        let ubuntu = width_in(epaint_default_fonts::UBUNTU_LIGHT, text);
        let drawn = |w: &FontWatch| w.font().line_width(text, 20.0);
        assert!((atkinson - hack).abs() > 1.0 && (atkinson - ubuntu).abs() > 1.0);

        // Nothing saved yet: Atkinson Hyperlegible Next.
        let (mut watch, note) = FontWatch::new(Some(settings.clone()));
        assert_eq!(note, None);
        assert_eq!(drawn(&watch), atkinson);

        // A font file chosen in the app: in it from the next check.
        save(hack_choice.clone());
        let note = watch.check().expect("a note that the font changed");
        assert!(note.contains("Hack"), "{note}");
        assert_eq!(drawn(&watch), hack);
        // Nothing changed since: no note, the same fonts.
        let before = Arc::clone(watch.font());
        assert_eq!(watch.check(), None);
        assert!(Arc::ptr_eq(&before, watch.font()));
        // And so at the next start.
        let (again, note) = FontWatch::new(Some(settings.clone()));
        assert_eq!(note, None);
        assert_eq!(drawn(&again), hack);

        // egui's original font, then JETCARP's.
        save(serde_json::json!("egui"));
        assert!(watch.check().is_some());
        assert_eq!(drawn(&watch), ubuntu);
        save(serde_json::json!("atkinson"));
        assert!(watch.check().is_some());
        assert_eq!(drawn(&watch), atkinson);

        // The chosen file removed: Atkinson, and a note naming the file.
        save(hack_choice);
        std::fs::remove_file(&hack_file).unwrap();
        let (gone, note) = FontWatch::new(Some(settings.clone()));
        assert_eq!(drawn(&gone), atkinson);
        let note = note.expect("a note that the font can't be used");
        assert!(note.contains("Hack-Regular.ttf"), "{note}");
        assert!(!note.contains(&*fonts.to_string_lossy()), "{note}");
        let note = watch.check().expect("a note that the font can't be used");
        assert!(note.contains("Hack-Regular.ttf"), "{note}");
        assert_eq!(drawn(&watch), atkinson);

        // No settings folder: Atkinson.
        let (none, note) = FontWatch::new(None);
        assert_eq!((drawn(&none), note), (atkinson, None));
    }

    /// A stand-in for an installed font that has `chars` (egui's Hack's X for each), which no
    /// machine's fonts can be counted on for, in a scratch folder of its own.
    fn installed_stand_in(name: &str, chars: &str) -> jetcarp_core::scratch::ScratchDir {
        let dir = jetcarp_core::scratch::ScratchDir::new(name);
        std::fs::create_dir_all(&*dir).unwrap();
        let hack = epaint_default_fonts::HACK_REGULAR;
        let font = crate::fonts::stand_in::font_with(hack, chars, 'X', "Stand-in CJK");
        std::fs::write(dir.join("StandInCJK.ttf"), font).unwrap();
        dir
    }

    fn hack_x() -> GlyphId {
        let hack = FontRef::new(epaint_default_fonts::HACK_REGULAR).unwrap();
        hack.charmap().map('X').unwrap()
    }

    /// A character none of the caption's fonts has (Chinese, Japanese, Korean, the macOS key
    /// symbols) is drawn, as a last resort, in an installed font that has it, whichever font is
    /// chosen, as the app draws it; what the fonts in place have stays theirs.
    #[test]
    fn a_character_only_an_installed_font_has_is_drawn_in_it() {
        let only = "中文⌘";
        let dir = installed_stand_in("caption-last-resort", only);
        let plain = CaptionFont::default();
        for c in only.chars() {
            assert_eq!(plain.glyph(c).1, GlyphId::NOTDEF, "{c} is a box without it");
        }
        let last = LastResort::ready(Fallbacks::find(&[dir.to_path_buf()]));
        let f = CaptionFont::default().with_last_resort(Arc::clone(&last));
        for c in only.chars() {
            assert_eq!(f.glyph(c), (2, hack_x()), "{c} in the installed font");
        }
        for c in "Life · 480 × 270 λ…".chars() {
            assert_eq!(f.glyph(c), plain.glyph(c), "{c} in the fonts in place");
        }
        let x = width_in(epaint_default_fonts::HACK_REGULAR, "X");
        assert_eq!(f.line_width("中", 20.0), x);
        let line = vec!["中文 · Life".to_string()];
        assert_ne!(
            f.rasterize(&line, 20.0, 400.0),
            plain.rasterize(&line, 20.0, 400.0),
            "drawn in it, not as boxes"
        );

        // The desktop background's: behind egui's original font, and Atkinson after a change.
        let settings = dir.join("settings");
        std::fs::create_dir_all(&settings).unwrap();
        let save = |font: &str| {
            let app = serde_json::json!({ "accessibility": { "font": font } });
            std::fs::write(settings.join("app.json"), app.to_string()).unwrap();
        };
        save("egui");
        let (watch, note) = FontWatch::new(Some(settings.clone()));
        assert_eq!(note, None);
        let mut watch = watch.with_last_resort(Arc::clone(&last));
        assert_eq!(
            watch.font().glyph('中'),
            (1, hack_x()),
            "behind Ubuntu Light"
        );
        save("atkinson");
        assert!(watch.check().is_some());
        assert_eq!(watch.font().glyph('中'), (2, hack_x()), "behind Atkinson");
    }

    /// The installed fonts' character maps are read on a thread of their own, so the caption is
    /// drawn meanwhile without waiting (a box for a character only they have), and in them once
    /// they are read.
    #[test]
    fn the_installed_fonts_are_read_off_the_frame_path() {
        let dir = installed_stand_in("caption-last-resort-thread", "中文");
        let dirs = vec![dir.to_path_buf()];
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let t0 = std::time::Instant::now();
        // The reading is held until the test lets it go.
        let last = LastResort::start_with(move || {
            let _ = wait.recv();
            Fallbacks::find(&dirs)
        });
        let started = t0.elapsed();
        let f = CaptionFont::default().with_last_resort(Arc::clone(&last));
        assert!(!f.last_resort_ready());
        assert_eq!(
            f.glyph('中').1,
            GlyphId::NOTDEF,
            "a box while the fonts are read"
        );
        let (_, w, h) = f.rasterize(&["中文 · Life".into()], 20.0, 400.0);
        assert!(w > 0 && h > 0, "the caption is drawn meanwhile");
        let _ = go.send(());
        for _ in 0..500 {
            if f.last_resort_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(f.last_resort_ready(), "the fonts are in place once read");
        assert_eq!(f.glyph('中'), (2, hack_x()));
        assert!(started.as_millis() < 50, "starting took {started:?}");
        // No folders (tests, screenshot runs): nothing to read, and boxes.
        let none = CaptionFont::default().with_last_resort(LastResort::start(vec![]));
        for _ in 0..500 {
            if none.last_resort_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(none.last_resort_ready());
        assert_eq!(none.glyph('中').1, GlyphId::NOTDEF);
    }

    /// The caption's fonts with `bytes` chosen as a font file, copied as `name` into `dir`.
    fn chosen(dir: &std::path::Path, name: &str, bytes: &[u8]) -> CaptionFont {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let choice = FontChoice::File {
            family: name.into(),
            path,
            index: 0,
        };
        CaptionFont::for_choice(&choice).unwrap()
    }

    /// The pixels of a caption picture `w` wide that are mostly letter: left, top, right, bottom.
    fn lit(img: &[u8], w: u32) -> [u32; 4] {
        let mut b = [u32::MAX, u32::MAX, 0, 0];
        for (i, p) in img.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            if p[0] >= 128 {
                b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            }
        }
        b
    }

    /// The caption's sizes and layout, pinned as they were drawn before 0.4.3-rc.3 (#186): in
    /// Atkinson Hyperlegible Next, egui's Ubuntu Light, Hack and Atkinson Hyperlegible Mono, its
    /// line heights, the widths of lines with accents and with a Greek letter drawn in the font
    /// behind, the picture's size, with lines cut to fit, and where its letters fall (to a pixel,
    /// for a rasteriser's antialiasing).
    #[test]
    fn the_caption_s_sizes_and_layout_are_pinned() {
        let dir = jetcarp_core::scratch::ScratchDir::new("caption-pins");
        let fonts = [
            ("Next", CaptionFont::default()),
            (
                "Ubuntu",
                CaptionFont::for_choice(&FontChoice::Egui).unwrap(),
            ),
            (
                "Hack",
                chosen(&dir, "Hack.ttf", epaint_default_fonts::HACK_REGULAR),
            ),
            ("Mono", chosen(&dir, "Mono.ttf", crate::fonts::MONO_REGULAR)),
        ];
        let font = |name: &str| &fonts.iter().find(|f| f.0 == name).unwrap().1;
        let texts = [
            "Life · B3/S23",
            "Square grid · 480 × 270 · wraps",
            "généré λ…",
        ];
        // Ascent, descent and line gap at 12, 20 and 37.5 pixels; each text's width at each.
        #[rustfmt::skip]
        let sizes: [(&str, f32, [f32; 3], [f32; 3]); 12] = [
            ("Next", 12.0, [9.083_077, -2.916_923, 0.0], [52.956_924, 129.147_69, 42.383_65]),
            ("Next", 20.0, [15.138_461, -4.861_538, 0.0], [88.261_55, 215.246_17, 70.639_42]),
            ("Next", 37.5, [28.384_615, -9.115_385, 0.0], [165.490_39, 403.586_58, 132.448_91]),
            ("Ubuntu", 12.0, [9.976_807, -2.023_194, 0.299_732], [59.550_407, 145.284_58, 52.196_25]),
            ("Ubuntu", 20.0, [16.628_01, -3.371_989, 0.499_554], [99.250_67, 242.140_9, 86.993_76]),
            ("Ubuntu", 37.5, [31.177_52, -6.322_48, 0.936_664], [186.095_02, 454.014_28, 163.113_3]),
            ("Hack", 12.0, [9.568_791, -2.431_208, 0.0], [80.682_884, 192.397_64, 55.857_38]),
            ("Hack", 20.0, [15.947_987, -4.052_013, 0.0], [134.471_5, 320.662_84, 93.095_64]),
            ("Hack", 37.5, [29.902_473, -7.597_525, 0.0], [252.134_05, 601.242_6, 174.554_34]),
            ("Mono", 12.0, [9.083_077, -2.916_923, 0.0], [75.840_004, 180.849_24, 51.937_5]),
            ("Mono", 20.0, [15.138_461, -4.861_538, 0.0], [126.399_994, 301.415_5, 86.562_49]),
            ("Mono", 37.5, [28.384_615, -9.115_385, 0.0], [237.000_05, 565.153_93, 162.304_69]),
        ];
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        for (name, px, metrics, widths) in sizes {
            let (a, d, g) = font(name).metrics(px);
            assert!(
                near(a, metrics[0]) && near(d, metrics[1]) && near(g, metrics[2]),
                "{name} at {px}: {a} {d} {g}, not {metrics:?}"
            );
            for (text, want) in texts.into_iter().zip(widths) {
                let w = font(name).line_width(text, px);
                assert!(near(w, want), "{name} at {px}: {text} is {w}, not {want}");
            }
        }
        // The caption of three lines at 12, 20 and 40 pixels in room enough, and at 20 in 200
        // pixels: the picture's width and height, and the pixels its letters cover.
        let lines: Vec<String> = [
            "Life · B3/S23",
            "Square grid · 480 × 270 · wraps",
            "Wolfram series · 31 of 256 · generation 1,234",
        ]
        .map(String::from)
        .to_vec();
        #[rustfmt::skip]
        let pictures = [
            ("Next", 12.0, 2000.0, [153, 46], [8, 10, 144, 37]),
            ("Next", 20.0, 2000.0, [252, 74], [12, 16, 238, 61]),
            ("Next", 40.0, 2000.0, [503, 148], [25, 32, 478, 121]),
            ("Next", 20.0, 200.0, [200, 74], [12, 16, 187, 61]),
            ("Ubuntu", 12.0, 2000.0, [177, 47], [8, 10, 167, 38]),
            ("Ubuntu", 20.0, 2000.0, [291, 76], [13, 15, 278, 63]),
            ("Ubuntu", 40.0, 2000.0, [582, 151], [25, 29, 556, 126]),
            ("Ubuntu", 20.0, 200.0, [199, 76], [13, 15, 185, 61]),
            ("Hack", 12.0, 2000.0, [226, 46], [8, 10, 217, 38]),
            ("Hack", 20.0, 2000.0, [374, 74], [12, 15, 360, 62]),
            ("Hack", 40.0, 2000.0, [747, 148], [24, 30, 721, 123]),
            ("Hack", 20.0, 200.0, [195, 74], [12, 15, 182, 59]),
            ("Mono", 12.0, 2000.0, [213, 46], [8, 10, 204, 37]),
            ("Mono", 20.0, 2000.0, [353, 74], [12, 16, 339, 61]),
            ("Mono", 40.0, 2000.0, [705, 148], [24, 32, 679, 121]),
            ("Mono", 20.0, 200.0, [200, 74], [12, 16, 186, 58]),
        ];
        for (name, px, room, size, letters) in pictures {
            let (img, w, h) = font(name).rasterize(&lines, px, room);
            assert_eq!([w, h], size, "{name} at {px} in {room}");
            let got = lit(&img, w);
            assert!(
                got.iter().zip(letters).all(|(&g, l)| g.abs_diff(l) <= 1),
                "{name} at {px} in {room}: letters in {got:?}, not {letters:?}"
            );
        }
        // Where a line is cut to fit.
        let long = "Wolfram series · 31 of 256 · generation 1,234";
        let cuts = [
            ("Next", "Wolfram series · 31 …"),
            ("Hack", "Wolfram serie…"),
        ];
        for (name, want) in cuts {
            assert_eq!(font(name).fit(long, 20.0, 150.0), want, "{name}");
        }
    }

    /// Captions drawn into `JETCARP_CAPTION_SHOTS` (`--ignored`), as PNGs of the picture the
    /// caption painter shows (premultiplied, over its backdrop), for comparing two builds' pixels:
    /// several scenes' captions at five sizes in Atkinson Hyperlegible Next and Mono, Ubuntu Light,
    /// and each font file in `JETCARP_CAPTION_SHOT_FONTS`; and a line of characters those lack, in
    /// the font behind and in the installed fonts of `JETCARP_FONT_DIRS` (both separated as `PATH`
    /// is).
    #[test]
    #[ignore = "draws captions into JETCARP_CAPTION_SHOTS, for comparing builds"]
    fn draw_captions_for_comparison() {
        let Some(out) = std::env::var_os("JETCARP_CAPTION_SHOTS").map(PathBuf::from) else {
            eprintln!("skipped: set JETCARP_CAPTION_SHOTS to a folder");
            return;
        };
        std::fs::create_dir_all(&out).unwrap();
        let paths = |var: &str| -> Vec<PathBuf> {
            std::env::var_os(var).map_or_else(Vec::new, |v| std::env::split_paths(&v).collect())
        };
        let last = LastResort::ready(Fallbacks::find(&paths("JETCARP_FONT_DIRS")));
        let dir = jetcarp_core::scratch::ScratchDir::new("caption-shots");
        let mut fonts = vec![
            ("next".to_string(), CaptionFont::default()),
            (
                "mono".into(),
                chosen(&dir, "Mono.ttf", crate::fonts::MONO_REGULAR),
            ),
            (
                "ubuntu".into(),
                CaptionFont::for_choice(&FontChoice::Egui).unwrap(),
            ),
        ];
        for path in paths("JETCARP_CAPTION_SHOT_FONTS") {
            let name = path.file_stem().unwrap().to_string_lossy().to_lowercase();
            let choice = FontChoice::File {
                family: name.clone(),
                path,
                index: 0,
            };
            fonts.push((name, CaptionFont::for_choice(&choice).unwrap()));
        }
        let square = TilingSpec::rect("square", 480, 270, topology::Boundary::Wrap);
        let hex = TilingSpec::rect("hex", 200, 120, topology::Boundary::Dead);
        let penrose = TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(3));
        let wolfram = Place {
            rotation: "Wolfram series",
            number: 31,
            count: 256,
        };
        let mut generations = world("W110", square.clone());
        generations.generation = 1_234_567;
        let scenes: Vec<(&str, Vec<String>)> = vec![
            ("life", lines(&world("B3/S23", square), None, false)),
            ("wolfram", lines(&generations, Some(wolfram), true)),
            ("hex", lines(&world("B2/S34H", hex), None, false)),
            ("penrose", lines(&world("B3/S23", penrose), None, true)),
            (
                "fallback",
                vec![
                    "生命游戏 · λόγος · ⌘ ⌥".into(),
                    "Rotation 中文 · ελληνικά".into(),
                ],
            ),
        ];
        let mut n = 0;
        for (name, font) in &fonts {
            let font = font.clone().with_last_resort(Arc::clone(&last));
            for (scene, text) in &scenes {
                for px in [12.0, 16.0, 20.0, 28.0, 40.0] {
                    let (rgba, width, height) = font.rasterize(text, px, 1600.0);
                    let img = jetcarp_core::render::Image {
                        width,
                        height,
                        rgba,
                    };
                    let file = out.join(format!("{name}-{scene}-{px}.png"));
                    jetcarp_core::render::write_png(&file, &img, &[]).unwrap();
                    n += 1;
                }
            }
        }
        eprintln!("{n} captions drawn into {}", out.display());
    }

    #[test]
    fn numbers_are_grouped() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn opacity_follows_the_settings() {
        let mut c = CaptionSettings::default();
        assert_eq!(c.opacity(0.0), 0.0, "off by default");
        c.show = true;
        assert_eq!(c.opacity(1e6), 1.0, "always");
        c.seconds = 5.0;
        assert_eq!(c.opacity(0.0), 1.0);
        assert_eq!(c.opacity(5.0), 1.0);
        assert!((c.opacity(5.5) - 0.5).abs() < 1e-6);
        assert_eq!(c.opacity(6.5), 0.0);
    }
}
