//! The caption: a few lines saying what is on screen (the rule, the grid, the rotation). The words
//! come from the world here, so every display says the same thing. The app draws them with egui;
//! [`CaptionPainter`] draws them on a plain wgpu surface, in the font the app's setting names
//! ([`FontWatch`]): Atkinson Hyperlegible Next by default ([`crate::fonts`]), with egui's Ubuntu
//! Light behind it for what Atkinson lacks (Greek).

use crate::fonts::{Fallbacks, FontChoice};
use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};
use jetcarp_core::rules::{self, Rule};
use jetcarp_core::topology::{Boundary, Topology};
use jetcarp_core::{World, families};
use serde::{Deserialize, Serialize};
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

/// The last resort's fonts read so far.
#[derive(Default)]
struct Faces {
    /// In the order first needed.
    list: Vec<FontArc>,
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
        let _ = scan;
        Arc::new(LastResort {
            index: OnceLock::new(),
            faces: Default::default(),
        })
    }

    /// The fonts `index` names, ready now: for fonts copied beside the program (the macOS screen
    /// saver's bundle).
    pub fn ready(index: Fallbacks) -> Arc<LastResort> {
        let _ = index;
        Self::start_with(Fallbacks::default)
    }

    /// Whether the character maps have been read.
    pub fn is_ready(&self) -> bool {
        self.index.get().is_some()
    }

    /// Where in the list the font that draws `c` is, read the first time.
    fn face_for(&self, c: char) -> Option<usize> {
        let _ = (c, &self.faces);
        None
    }

    /// The `i`th font read.
    fn face(&self, i: usize) -> FontArc {
        let faces = self.faces.lock().unwrap_or_else(|e| e.into_inner());
        faces.list[i].clone()
    }
}

/// The caption's fonts: Atkinson Hyperlegible Next (or the font chosen in the app), then Ubuntu
/// Light (egui's) for a character it lacks, then, as a last resort, an installed font that has it
/// ([`LastResort`]). Each character is drawn in the first that has it.
#[derive(Clone)]
pub struct CaptionFont {
    fonts: Vec<FontArc>,
    last: Option<Arc<LastResort>>,
}

fn builtin(bytes: &'static [u8]) -> FontArc {
    FontArc::try_from_slice(bytes).expect("a font that comes with JETCARP")
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
                let font = ab_glyph::FontVec::try_from_vec_and_index(file.bytes, file.index)
                    .map_err(|_| {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        format!("{name} isn't a font JETCARP can read.")
                    })?;
                Ok(CaptionFont {
                    fonts: vec![FontArc::new(font), ubuntu],
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
    fn font(&self, i: usize) -> FontArc {
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
            if g.0 != 0 {
                return (i, g);
            }
        }
        if let Some(l) = &self.last
            && let Some(i) = l.face_for(c)
        {
            let g = l.face(i).glyph_id(c);
            if g.0 != 0 {
                return (self.fonts.len() + i, g);
            }
        }
        (0, self.fonts[0].glyph_id(c))
    }

    /// The advance from glyph `last` to `g` at `px`, with their kerning when one font has both.
    fn advance(&self, last: Option<(usize, GlyphId)>, (i, g): (usize, GlyphId), px: f32) -> f32 {
        let font = self.font(i);
        let f = font.as_scaled(PxScale::from(px));
        let kern = match last {
            Some((j, l)) if j == i => f.kern(l, g),
            _ => 0.0,
        };
        kern + f.h_advance(g)
    }

    /// Ascent, descent and line gap at `px` (of the first font: lines keep one height).
    fn metrics(&self, px: f32) -> (f32, f32, f32) {
        let f = self.fonts[0].as_scaled(PxScale::from(px));
        (f.ascent(), f.descent(), f.line_gap())
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
        for (i, line) in fitted.iter().enumerate() {
            let px = px_of(i);
            let scale = PxScale::from(px);
            let baseline = y + self.metrics(px).0;
            let mut x = pad;
            let mut last = None;
            for c in line.chars() {
                let (fi, id) = self.glyph(c);
                let font = &self.font(fi);
                if let Some((j, l)) = last
                    && j == fi
                {
                    x += font.as_scaled(scale).kern(l, id);
                }
                let glyph = id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
                if let Some(o) = font.outline_glyph(glyph) {
                    let b = o.px_bounds();
                    o.draw(|gx, gy, cov| {
                        let (px, py) = (b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64);
                        if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                            return;
                        }
                        let at = (py as usize * w as usize + px as usize) * 4;
                        let a = BACKDROP_ALPHA as f32 / 255.0;
                        let cov = cov.clamp(0.0, 1.0);
                        // White text over the backdrop, premultiplied.
                        let value = (cov * 255.0).round() as u8;
                        let alpha = ((cov + (1.0 - cov) * a) * 255.0).round() as u8;
                        img[at] = img[at].max(value);
                        img[at + 1] = img[at + 1].max(value);
                        img[at + 2] = img[at + 2].max(value);
                        img[at + 3] = img[at + 3].max(alpha);
                    });
                }
                x += font.as_scaled(scale).h_advance(id);
                last = Some((fi, id));
            }
            y += line_h(i);
        }
        (img, w, h)
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    use ab_glyph::FontRef;
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
        let atkinson = FontRef::try_from_slice(crate::fonts::NEXT_REGULAR).unwrap();
        for c in "Life B3/S23 · 480 × 270 · wraps … généré".chars() {
            assert_eq!(f.glyph(c), (0, atkinson.glyph_id(c)), "{c}");
        }
        // Greek letters Atkinson lacks.
        for c in "λφαβγ".chars() {
            assert_eq!(f.glyph(c).0, 1, "{c}");
            assert_ne!(f.glyph(c).1.0, 0, "{c}");
        }
        // Atkinson's widths, so its picture is not the Ubuntu one.
        let ubuntu = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        let w = f.line_width("Square grid", 20.0);
        let uw: f32 = "Square grid"
            .chars()
            .map(|c| {
                ubuntu
                    .as_scaled(PxScale::from(20.0))
                    .h_advance(ubuntu.glyph_id(c))
            })
            .sum();
        assert!((w - uw).abs() > 1.0, "{w} vs Ubuntu Light's {uw}");
        let (img, iw, ih) = f.rasterize(&["Life".into(), "λ".into()], 20.0, 400.0);
        assert_eq!(img.len(), (iw * ih * 4) as usize);
        assert!(
            img.as_chunks::<4>().0.iter().any(|p| p[0] > 200),
            "text drawn"
        );
    }

    /// `text`'s width at 20 pixels in `font` alone.
    fn width_in(font: &[u8], text: &str) -> f32 {
        let f = FontRef::try_from_slice(font).unwrap();
        let s = f.as_scaled(PxScale::from(20.0));
        let mut w = 0.0;
        let mut last = None;
        for c in text.chars() {
            let g = f.glyph_id(c);
            w += last.map_or(0.0, |l| s.kern(l, g)) + s.h_advance(g);
            last = Some(g);
        }
        w
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
        FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR)
            .unwrap()
            .glyph_id('X')
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
            assert_eq!(plain.glyph(c).1.0, 0, "{c} is a box without it");
        }
        let last = LastResort::ready(Fallbacks::find(&[dir.to_path_buf()]));
        let f = CaptionFont::default().with_last_resort(Arc::clone(&last));
        for c in only.chars() {
            assert_eq!(f.glyph(c), (2, hack_x()), "{c} in the installed font");
        }
        for c in "Life · 480 × 270 λ…".chars() {
            assert_eq!(f.glyph(c), plain.glyph(c), "{c} in the fonts in place");
        }
        let hack = FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR).unwrap();
        let x = hack.as_scaled(PxScale::from(20.0)).h_advance(hack_x());
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
        assert_eq!(f.glyph('中').1.0, 0, "a box while the fonts are read");
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
        assert_eq!(none.glyph('中').1.0, 0);
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
