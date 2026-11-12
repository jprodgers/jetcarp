//! The caption: a few lines saying what is on screen (the rule, the grid, the rotation). The words
//! come from the world here, so every display says the same thing. The app draws them with egui;
//! [`CaptionPainter`] draws them on a plain wgpu surface.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use jetcarp_core::rules::{self, Rule};
use jetcarp_core::topology::{Boundary, Topology};
use jetcarp_core::{World, families};
use serde::{Deserialize, Serialize};

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

/// Draws a caption on a plain wgpu target.
pub struct CaptionPainter {
    font: FontRef<'static>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    picture: Option<Picture>,
    drawn: Option<(Vec<String>, u32, u32)>,
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
            font: FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
                .expect("the built-in font"),
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

    fn line_width(&self, text: &str, px: f32) -> f32 {
        let f = self.font.as_scaled(PxScale::from(px));
        let mut w = 0.0;
        let mut last = None;
        for c in text.chars() {
            let g = f.glyph_id(c);
            if let Some(l) = last {
                w += f.kern(l, g);
            }
            w += f.h_advance(g);
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
    fn rasterize(&self, lines: &[String], first_px: f32, max_w: f32) -> (Vec<u8>, u32, u32) {
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
            let f = self.font.as_scaled(PxScale::from(px_of(i)));
            f.ascent() - f.descent() + f.line_gap()
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
            let scale = PxScale::from(px_of(i));
            let f = self.font.as_scaled(scale);
            let baseline = y + f.ascent();
            let mut x = pad;
            let mut last = None;
            for c in line.chars() {
                let id = f.glyph_id(c);
                if let Some(l) = last {
                    x += f.kern(l, id);
                }
                let glyph = id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
                if let Some(o) = self.font.outline_glyph(glyph) {
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
                x += f.h_advance(id);
                last = Some(id);
            }
            y += line_h(i);
        }
        (img, w, h)
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
        self.prepare_within(
            device, queue, lines, settings, target, scale, opacity, none,
        );
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
        let key = (lines.to_vec(), px.to_bits(), free_w as u32);
        if self.drawn.as_ref() != Some(&key) {
            let margin = (px * MARGIN).ceil();
            let (img, w, h) = self.rasterize(lines, px, free_w - 2.0 * margin);
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
