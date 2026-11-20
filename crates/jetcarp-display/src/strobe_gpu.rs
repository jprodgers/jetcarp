//! The strobe guard on the GPU (#146, #148): where a frame is drawn on the GPU, it is measured
//! there, and softened there, in one compute pass over the frame.
//!
//! The pass reads the frame through an sRGB view, so the texture unit decodes each pixel to linear
//! light for free, and gives each tile ([`jetcarp_core::strobe::TILE`] pixels square) its mean relative luminance and mean red
//! measure: the same [`TileMap`] the processor's reduction gives
//! ([`jetcarp_core::strobe::frame::reduce`]). On the frames the stripe check wants, it also writes
//! the half-resolution luminance image. One workgroup is one tile: 64 threads, each reading a 2 by 2
//! block of pixels once. When the guard's [`Plan`] softens or veils anything, the same pass then
//! carries it out exactly as [`jetcarp_core::strobe::soften::Softener`] does: it keeps the last
//! frame shown in linear light (16 bits a channel), moves each softened tile towards the new frame
//! by the fraction that keeps its mean within the step, veils, writes the frame to show, and
//! measures that too. With nothing to soften the frame is shown as drawn and nothing is written
//! but the tile means.
//!
//! The maps come back to the processor through a ring of [`SLOTS`] readback buffers, a frame or two
//! later, never blocking a frame: the guard decides on the frames already shown, and the softening
//! itself needs no round trip, since each tile's step is computed on the GPU from the frame being
//! drawn.
//!
//! [`StrobeGpu::blit`] draws the frame to show (the one drawn, or the softened one) into a render
//! pass: the app's view inside egui's pass, or a surface.

use jetcarp_core::strobe::frame::{LumImage, TileMap, tiles_for};
use jetcarp_core::strobe::soften::Plan;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Readback buffers in flight at once.
pub const SLOTS: usize = 4;

const SHADER: &str = r#"
struct P {
  size: vec2<u32>,
  cols: u32,
  rows: u32,
  mode: u32,
  half_w: u32,
  lum_step: f32,
  red_step: f32,
};
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> tiles: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> halfimg: array<f32>;
@group(0) @binding(4) var<storage, read_write> state: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read_write> prev: array<vec2<f32>>;
@group(0) @binding(6) var plan: texture_2d<f32>;
@group(0) @binding(7) var shown: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(8) var samp: sampler;

const GUARD: u32 = 1u;
const FRESH: u32 = 2u;
const HALF: u32 = 4u;
const DECODE: u32 = 8u;

var<workgroup> acc: array<vec4<f32>, 64>;
var<workgroup> cnt: array<f32, 64>;

fn dec(x: f32) -> f32 {
  if (x <= 0.04045) { return x / 12.92; }
  return pow((x + 0.055) / 1.055, 2.4);
}
fn enc(x: f32) -> f32 {
  let v = clamp(x, 0.0, 1.0);
  if (v <= 0.0031308) { return v * 12.92; }
  return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
fn lum(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }
fn red(c: vec3<f32>) -> f32 {
  let s = c.r + c.g + c.b;
  if (s > 0.0 && c.r >= 0.8 * s) { return max(0.0, (c.r - c.g - c.b) * 320.0); }
  return 0.0;
}
fn q16(c: vec3<f32>) -> vec3<f32> {
  return vec3<f32>(unpack2x16unorm(pack2x16unorm(c.rg)), unpack2x16unorm(pack2x16unorm(vec2<f32>(c.b, 0.0))).x);
}
fn reduce(lx: u32) {
  for (var st = 32u; st > 0u; st = st >> 1u) {
    if (lx < st) {
      acc[lx] = acc[lx] + acc[lx + st];
      cnt[lx] = cnt[lx] + cnt[lx + st];
    }
    workgroupBarrier();
  }
}

@compute @workgroup_size(8, 8)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_id) li: vec3<u32>,
        @builtin(local_invocation_index) lx: u32) {
  let tile = wg.y * p.cols + wg.x;
  let base = vec2<u32>(wg.x * 16u + li.x * 2u, wg.y * 16u + li.y * 2u);
  var px: array<vec3<f32>, 4>;
  var ok: array<bool, 4>;
  var l = 0.0;
  var r = 0.0;
  var n = 0.0;
  for (var k = 0u; k < 4u; k++) {
    let q = base + vec2<u32>(k & 1u, k >> 1u);
    ok[k] = q.x < p.size.x && q.y < p.size.y;
    px[k] = vec3<f32>(0.0);
    if (ok[k]) {
      var c = textureLoad(src, vec2<i32>(q), 0).rgb;
      if ((p.mode & DECODE) != 0u) { c = vec3<f32>(dec(c.r), dec(c.g), dec(c.b)); }
      px[k] = c;
      l += lum(c);
      r += red(c);
      n += 1.0;
    }
  }
  if ((p.mode & HALF) != 0u && n > 0.0) {
    halfimg[(base.y >> 1u) * p.half_w + (base.x >> 1u)] = l / n;
  }
  acc[lx] = vec4<f32>(l, r, 0.0, 0.0);
  cnt[lx] = n;
  workgroupBarrier();
  reduce(lx);
  let total = max(cnt[0], 1.0);
  let lin = acc[0].x / total;
  let rin = acc[0].y / total;
  if ((p.mode & GUARD) == 0u) {
    if (lx == 0u) { tiles[tile] = vec4<f32>(lin, rin, lin, rin); }
    return;
  }
  // Everyone has read the sums before the arrays are used again.
  workgroupBarrier();
  var beta = 1.0;
  let pl = textureLoad(plan, vec2<i32>(i32(wg.x), i32(wg.y)), 0);
  if ((p.mode & FRESH) == 0u && pl.x > 0.5) {
    let pv = prev[tile];
    let dl = abs(lin - pv.x);
    let dr = abs(rin - pv.y);
    if (dl > p.lum_step) { beta = min(beta, p.lum_step / dl); }
    if (dr > p.red_step) { beta = min(beta, p.red_step / dr); }
  }
  let grid = vec2<f32>(f32(p.cols) * 16.0, f32(p.rows) * 16.0);
  var sums = vec4<f32>(0.0);
  for (var k = 0u; k < 4u; k++) {
    if (ok[k]) {
      let q = base + vec2<u32>(k & 1u, k >> 1u);
      let i = q.y * p.size.x + q.x;
      var s = px[k];
      if (beta < 1.0) {
        let st = state[i];
        let rg = unpack2x16unorm(st.x);
        let b = unpack2x16unorm(st.y).x;
        let was = vec3<f32>(rg, b);
        s = was + (px[k] - was) * beta;
      }
      state[i] = vec2<u32>(pack2x16unorm(s.rg), pack2x16unorm(vec2<f32>(s.b, 0.0)));
      s = q16(s);
      let v = textureSampleLevel(plan, samp, (vec2<f32>(q) + 0.5) / grid, 0.0);
      let o = s * (1.0 - v.y) + vec3<f32>(v.z);
      sums += vec4<f32>(lum(s), red(s), lum(o), red(o));
      textureStore(shown, vec2<i32>(q), vec4<f32>(enc(o.r), enc(o.g), enc(o.b), 1.0));
    }
  }
  acc[lx] = sums;
  cnt[lx] = n;
  workgroupBarrier();
  reduce(lx);
  if (lx == 0u) {
    let t = max(cnt[0], 1.0);
    prev[tile] = vec2<f32>(acc[0].x / t, acc[0].y / t);
    tiles[tile] = vec4<f32>(lin, rin, acc[0].z / t, acc[0].w / t);
  }
}
"#;

const BLIT: &str = r#"
@group(0) @binding(0) var t: texture_2d<f32>;
struct VO { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VO {
  let x = f32((i << 1u) & 2u);
  let y = f32(i & 2u);
  var o: VO;
  o.p = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  o.uv = vec2<f32>(x, y);
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4<f32> {
  let d = vec2<f32>(textureDimensions(t));
  let q = clamp(vec2<i32>(floor(i.uv * d)), vec2<i32>(0), vec2<i32>(d) - vec2<i32>(1));
  return vec4<f32>(textureLoad(t, q, 0).rgb, 1.0);
}
"#;

const GUARD: u32 = 1;
const FRESH: u32 = 2;
const HALF: u32 = 4;
const DECODE: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size: [u32; 2],
    cols: u32,
    rows: u32,
    mode: u32,
    half_w: u32,
    lum_step: f32,
    red_step: f32,
}

/// One frame's reading from the GPU.
#[derive(Clone, Debug)]
pub struct GpuReading {
    /// The time the frame was drawn for (as passed to [`StrobeGpu::process`]).
    pub time: f64,
    /// The frame as drawn.
    pub input: TileMap,
    /// The frame as shown (the same as `input` when nothing was softened).
    pub output: TileMap,
    /// The half-resolution luminance image, on frames that asked for it.
    pub half: Option<LumImage>,
}

/// Which texture holds the frame to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// The frame as drawn.
    Drawn,
    /// The guard's softened frame.
    Softened,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotState {
    Free,
    /// Copied into, in an encoder not yet submitted.
    Copied,
    /// Waiting for the map.
    Mapping,
}

struct Slot {
    tiles: wgpu::Buffer,
    half: wgpu::Buffer,
    /// Maps finished (or failed: 100 and above).
    done: Arc<AtomicU8>,
    want: u8,
    state: SlotState,
    time: f64,
    has_half: bool,
    soft: bool,
}

/// Buffers for frames of one size.
struct Sized {
    w: u32,
    h: u32,
    cols: u32,
    rows: u32,
    half_w: u32,
    half_h: u32,
    tiles: wgpu::Buffer,
    half: wgpu::Buffer,
    prev: wgpu::Buffer,
    slots: Vec<Slot>,
}

/// The softening's resources for frames of one size, made the first time it is needed.
struct Soft {
    state: wgpu::Buffer,
    shown: wgpu::Texture,
    shown_store: wgpu::TextureView,
    shown_plain: wgpu::TextureView,
    shown_srgb: wgpu::TextureView,
    plan: wgpu::Texture,
    plan_view: wgpu::TextureView,
    /// The plan last uploaded.
    plan_now: Option<Plan>,
}

/// A texture to draw frames into, for the app's view or a surface that cannot be read.
struct Scene {
    tex: wgpu::Texture,
    render: wgpu::TextureView,
    decode: wgpu::TextureView,
    plain: wgpu::TextureView,
    manual: bool,
}

/// The strobe guard's GPU half, for one view.
pub struct StrobeGpu {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_module: wgpu::ShaderModule,
    blits: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    params: wgpu::Buffer,
    sampler: wgpu::Sampler,
    dummy_state: wgpu::Buffer,
    dummy_shown: wgpu::TextureView,
    dummy_plan: wgpu::TextureView,
    sized: Option<Sized>,
    soft: Option<Soft>,
    scene: Option<Scene>,
    /// The state holds the last frame shown (the last frame was softened, at this size).
    state_valid: bool,
    /// What the last frame showed, and the blit set up for it.
    shown: Shown,
    blit_bind: Option<wgpu::BindGroup>,
    blit_format: Option<wgpu::TextureFormat>,
    /// The device cannot hold the softening's state for frames this large.
    too_large: bool,
}

/// `v` as a 16-bit float's bits (for values from 0 to a few thousand).
fn f16_bits(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xff) as i32 - 127 + 15;
    let mant = x & 0x7f_ffff;
    if v == 0.0 || exp < -10 {
        return sign;
    }
    if exp <= 0 {
        let m = (mant | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | ((((exp as u32) << 10) | (mant >> 13)) + ((mant >> 12) & 1)) as u16
}

fn storage(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture(binding: u32, filterable: bool, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// A view of `tex` whose reads give linear light, and whether the shader must decode by hand
/// (formats with no sRGB twin).
fn decoding_view(tex: &wgpu::Texture) -> (wgpu::TextureView, bool) {
    let f = tex.format();
    let s = f.add_srgb_suffix();
    if s.is_srgb() {
        let v = tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("strobe guard: decoding view"),
            format: Some(s),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        });
        (v, false)
    } else {
        (tex.create_view(&Default::default()), true)
    }
}

/// The formats a texture of format `f` needs among its view formats, for [`decoding_view`] and
/// for drawing it to targets of either kind.
pub fn view_formats(f: wgpu::TextureFormat) -> Vec<wgpu::TextureFormat> {
    let mut v = vec![];
    for g in [f.add_srgb_suffix(), f.remove_srgb_suffix()] {
        if g != f && !v.contains(&g) {
            v.push(g);
        }
    }
    v
}

impl StrobeGpu {
    pub fn new(device: &wgpu::Device) -> StrobeGpu {
        let cs = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("strobe guard"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: cs,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, false, cs),
                storage(2),
                storage(3),
                storage(4),
                storage(5),
                texture(6, true, cs),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: cs,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: cs,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("strobe guard"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("strobe guard"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("strobe guard"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("strobe guard blit"),
            entries: &[texture(0, false, wgpu::ShaderStages::FRAGMENT)],
        });
        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("strobe guard blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("strobe guard params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("strobe guard plan"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy_state = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("strobe guard: no state"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let tiny = |format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("strobe guard: nothing"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let dummy_shown = tiny(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING,
        );
        let dummy_plan = tiny(
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING,
        );
        StrobeGpu {
            layout,
            pipeline,
            blit_layout,
            blit_module,
            blits: HashMap::new(),
            params,
            sampler,
            dummy_state,
            dummy_shown,
            dummy_plan,
            sized: None,
            soft: None,
            scene: None,
            state_valid: false,
            shown: Shown::Drawn,
            blit_bind: None,
            blit_format: None,
            too_large: false,
        }
    }

    /// Whether frames of the last size can be softened on this device (a very large frame's
    /// state may not fit in one buffer).
    pub fn can_soften(&self) -> bool {
        !self.too_large
    }

    /// A texture of `size` and `format` to draw a frame into for [`process`](Self::process),
    /// cleared to nothing: kept between frames while the size and format hold.
    pub fn scene_target(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        let size = [size[0].max(1), size[1].max(1)];
        let ok = self.scene.as_ref().is_some_and(|s| {
            s.tex.format() == format && s.tex.width() == size[0] && s.tex.height() == size[1]
        });
        if !ok {
            let formats = view_formats(format);
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("strobe guard: frame"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &formats,
            });
            let render = tex.create_view(&Default::default());
            let (decode, manual) = decoding_view(&tex);
            let plain = tex.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format.remove_srgb_suffix()),
                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                ..Default::default()
            });
            self.scene = Some(Scene {
                tex,
                render,
                decode,
                plain,
                manual,
            });
            self.blit_bind = None;
        }
        self.scene.as_ref().expect("made").render.clone()
    }

    /// The texture [`scene_target`](Self::scene_target) made.
    pub fn scene_texture(&self) -> Option<&wgpu::Texture> {
        self.scene.as_ref().map(|s| &s.tex)
    }

    fn sized(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.sized.as_ref().is_some_and(|s| s.w == w && s.h == h) {
            return;
        }
        let (cols, rows) = tiles_for(w, h);
        let (half_w, half_h) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
        let buf = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        let tile_bytes = cols as u64 * rows as u64 * 16;
        let half_bytes = half_w as u64 * half_h as u64 * 4;
        let st = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        let rb = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
        self.sized = Some(Sized {
            w,
            h,
            cols,
            rows,
            half_w,
            half_h,
            tiles: buf("strobe guard tiles", tile_bytes, st),
            half: buf("strobe guard half image", half_bytes, st),
            prev: buf(
                "strobe guard last tiles",
                cols as u64 * rows as u64 * 8,
                wgpu::BufferUsages::STORAGE,
            ),
            slots: (0..SLOTS)
                .map(|_| Slot {
                    tiles: buf("strobe guard tiles readback", tile_bytes, rb),
                    half: buf("strobe guard half readback", half_bytes, rb),
                    done: Arc::new(AtomicU8::new(0)),
                    want: 0,
                    state: SlotState::Free,
                    time: 0.0,
                    has_half: false,
                    soft: false,
                })
                .collect(),
        });
        self.soft = None;
        self.state_valid = false;
        self.too_large = false;
    }

    fn soft_resources(&mut self, device: &wgpu::Device) -> bool {
        let Some(s) = &self.sized else { return false };
        if self.soft.is_some() {
            return true;
        }
        let bytes = s.w as u64 * s.h as u64 * 8;
        let limits = device.limits();
        if bytes > limits.max_storage_buffer_binding_size as u64 || bytes > limits.max_buffer_size {
            self.too_large = true;
            return false;
        }
        let state = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("strobe guard: last frame shown"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let shown = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("strobe guard: softened frame"),
            size: wgpu::Extent3d {
                width: s.w,
                height: s.h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        let shown_store = shown.create_view(&Default::default());
        let shown_plain = shown.create_view(&wgpu::TextureViewDescriptor {
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        });
        let shown_srgb = shown.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        });
        let plan = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("strobe guard: plan"),
            size: wgpu::Extent3d {
                width: s.cols,
                height: s.rows,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let plan_view = plan.create_view(&Default::default());
        self.soft = Some(Soft {
            state,
            shown,
            shown_store,
            shown_plain,
            shown_srgb,
            plan,
            plan_view,
            plan_now: None,
        });
        true
    }

    fn upload_plan(&mut self, queue: &wgpu::Queue, plan: &Plan) {
        let Some(soft) = &mut self.soft else { return };
        if soft.plan_now.as_ref() == Some(plan) {
            return;
        }
        let mut data = Vec::with_capacity(plan.soften.len() * 8);
        for (s, v) in plan.soften.iter().zip(&plan.veil) {
            let amount = v[0].clamp(0.0, 1.0);
            for x in [
                if *s { 1.0 } else { 0.0 },
                amount,
                amount * v[1].clamp(0.0, 1.0),
                0.0,
            ] {
                data.extend_from_slice(&f16_bits(x).to_le_bytes());
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &soft.plan,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plan.cols * 8),
                rows_per_image: Some(plan.rows),
            },
            wgpu::Extent3d {
                width: plan.cols,
                height: plan.rows,
                depth_or_array_layers: 1,
            },
        );
        soft.plan_now = Some(plan.clone());
    }

    /// Measures the frame in `source` (`size` pixels), and carries out `plan` on it, in `enc`.
    /// `source` is the texture the frame was drawn into: the one [`scene_target`] gave, or a
    /// surface's texture that allows `TEXTURE_BINDING` and lists the views of [`view_formats`].
    /// `dt` is the time since the last frame and `time` this frame's, for the readings;
    /// `want_half` asks for the half-resolution image as well. Call [`after_submit`] once `enc`
    /// has been submitted. Returns which texture now holds the frame to show.
    ///
    /// [`scene_target`]: Self::scene_target
    /// [`after_submit`]: Self::after_submit
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        source: Option<&wgpu::Texture>,
        plan: &Plan,
        dt: f32,
        time: f64,
        want_half: bool,
    ) -> Shown {
        let (src_view, manual, w, h) = match source {
            Some(t) => {
                let (v, m) = decoding_view(t);
                (v, m, t.width(), t.height())
            }
            None => {
                let Some(s) = &self.scene else {
                    return Shown::Drawn;
                };
                (s.decode.clone(), s.manual, s.tex.width(), s.tex.height())
            }
        };
        self.sized(device, w, h);
        let (cols, rows) = tiles_for(w, h);
        let soften = plan.active() && plan.fits(cols, rows) && self.soft_resources(device);
        if soften {
            self.upload_plan(queue, plan);
        }
        let fresh = soften && !self.state_valid;
        let (lum_step, red_step) = plan.steps(dt);
        let s = self.sized.as_mut().expect("sized");
        let mut mode = 0;
        if soften {
            mode |= GUARD;
        }
        if fresh {
            mode |= FRESH;
        }
        if want_half {
            mode |= HALF;
        }
        if manual {
            mode |= DECODE;
        }
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                size: [w, h],
                cols,
                rows,
                mode,
                half_w: s.half_w,
                lum_step,
                red_step,
            }),
        );
        let soft = self.soft.as_ref().filter(|_| soften);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("strobe guard"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: s.tiles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: s.half.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: soft
                        .map_or(&self.dummy_state, |x| &x.state)
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: s.prev.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(
                        soft.map_or(&self.dummy_plan, |x| &x.plan_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(
                        soft.map_or(&self.dummy_shown, |x| &x.shown_store),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("strobe guard"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(cols, rows, 1);
        }
        // The readings, if a slot is free (else this frame goes unread; the softening holds).
        if let Some(slot) = s.slots.iter_mut().find(|x| x.state == SlotState::Free) {
            enc.copy_buffer_to_buffer(&s.tiles, 0, &slot.tiles, 0, cols as u64 * rows as u64 * 16);
            if want_half {
                enc.copy_buffer_to_buffer(
                    &s.half,
                    0,
                    &slot.half,
                    0,
                    s.half_w as u64 * s.half_h as u64 * 4,
                );
            }
            slot.state = SlotState::Copied;
            slot.time = time;
            slot.has_half = want_half;
            slot.soft = soften;
        }
        self.state_valid = soften;
        let shown = if soften {
            Shown::Softened
        } else {
            Shown::Drawn
        };
        if shown != self.shown {
            self.blit_bind = None;
        }
        self.shown = shown;
        shown
    }

    /// Starts mapping the readbacks of frames whose encoder has been submitted. Call after each
    /// submit (or at the start of the next frame, where the submit is someone else's).
    pub fn after_submit(&mut self) {
        let Some(s) = &mut self.sized else { return };
        for slot in &mut s.slots {
            if slot.state != SlotState::Copied {
                continue;
            }
            slot.state = SlotState::Mapping;
            slot.done.store(0, Ordering::SeqCst);
            slot.want = if slot.has_half { 2 } else { 1 };
            let bufs: Vec<&wgpu::Buffer> = if slot.has_half {
                vec![&slot.tiles, &slot.half]
            } else {
                vec![&slot.tiles]
            };
            for b in bufs {
                let done = slot.done.clone();
                b.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    done.fetch_add(if r.is_ok() { 1 } else { 100 }, Ordering::SeqCst);
                });
            }
        }
    }

    /// The readings of frames the GPU has finished, oldest first. Never blocks.
    pub fn take(&mut self, device: &wgpu::Device) -> Vec<GpuReading> {
        let _ = device.poll(wgpu::PollType::Poll);
        let Some(s) = &mut self.sized else {
            return vec![];
        };
        let mut out = vec![];
        for slot in &mut s.slots {
            if slot.state != SlotState::Mapping {
                continue;
            }
            let done = slot.done.load(Ordering::SeqCst);
            if done < slot.want {
                continue;
            }
            if done < 100 {
                let n = (s.cols * s.rows) as usize;
                let (w, h, hw, hh) = (s.w, s.h, s.half_w, s.half_h);
                let read = || -> Option<GpuReading> {
                    let mut input = TileMap::new(w, h);
                    let mut output = TileMap::new(w, h);
                    {
                        let v = slot.tiles.slice(..).get_mapped_range().ok()?;
                        let f: &[f32] = bytemuck::cast_slice(&v[..n * 16]);
                        for t in 0..n {
                            input.lum[t] = f[t * 4];
                            input.red[t] = f[t * 4 + 1];
                            output.lum[t] = f[t * 4 + 2];
                            output.red[t] = f[t * 4 + 3];
                        }
                    }
                    let half = if slot.has_half {
                        let k = (hw * hh) as usize;
                        let v = slot.half.slice(..).get_mapped_range().ok()?;
                        Some(LumImage {
                            width: hw,
                            height: hh,
                            lum: bytemuck::cast_slice::<u8, f32>(&v[..k * 4]).to_vec(),
                        })
                    } else {
                        None
                    };
                    if !slot.soft {
                        output = input.clone();
                    }
                    Some(GpuReading {
                        time: slot.time,
                        input,
                        output,
                        half,
                    })
                };
                if let Some(r) = read() {
                    out.push(r);
                }
            }
            slot.tiles.unmap();
            if slot.has_half {
                slot.half.unmap();
            }
            slot.state = SlotState::Free;
        }
        out.sort_by(|a, b| a.time.total_cmp(&b.time));
        out
    }

    /// Sets up [`blit`](Self::blit) for a target of `format`: the frame drawn (in `source`, or
    /// the scene texture) or the softened one, whichever the last [`process`](Self::process)
    /// chose.
    pub fn prepare_blit(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        source: Option<&wgpu::Texture>,
    ) {
        if !self.blits.contains_key(&format) {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("strobe guard blit"),
                bind_group_layouts: &[Some(&self.blit_layout)],
                immediate_size: 0,
            });
            let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("strobe guard blit"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &self.blit_module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.blit_module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            self.blits.insert(format, pipe);
        }
        // A target that encodes sRGB takes linear light; a plain one, the bytes as they are.
        let srgb = format.is_srgb();
        let view = match (self.shown, &self.soft, source, &self.scene) {
            (Shown::Softened, Some(s), _, _) => {
                if srgb {
                    s.shown_srgb.clone()
                } else {
                    s.shown_plain.clone()
                }
            }
            (_, _, Some(t), _) => t.create_view(&wgpu::TextureViewDescriptor {
                format: Some(if srgb {
                    t.format().add_srgb_suffix()
                } else {
                    t.format().remove_srgb_suffix()
                }),
                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                ..Default::default()
            }),
            (_, _, None, Some(sc)) => {
                if srgb {
                    sc.decode.clone()
                } else {
                    sc.plain.clone()
                }
            }
            _ => {
                self.blit_bind = None;
                return;
            }
        };
        self.blit_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("strobe guard blit"),
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        }));
        self.blit_format = Some(format);
    }

    /// Draws the frame to show over the whole of `pass`'s viewport (set up by
    /// [`prepare_blit`](Self::prepare_blit)).
    pub fn blit(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (Some(bind), Some(f)) = (&self.blit_bind, self.blit_format) else {
            return;
        };
        let Some(pipe) = self.blits.get(&f) else {
            return;
        };
        pass.set_pipeline(pipe);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The softened frame's texture (Rgba8Unorm, sRGB values), while frames are softened.
    pub fn softened_texture(&self) -> Option<&wgpu::Texture> {
        self.soft
            .as_ref()
            .filter(|_| self.shown == Shown::Softened)
            .map(|s| &s.shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_trip_the_plan_s_values() {
        let back = |b: u16| {
            let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
            let e = ((b >> 10) & 0x1f) as i32;
            let m = (b & 0x3ff) as f32;
            sign * if e == 0 {
                m / 1024.0 * 2f32.powi(-14)
            } else {
                (1.0 + m / 1024.0) * 2f32.powi(e - 15)
            }
        };
        for v in [0.0f32, 1.0, 0.5, 0.25, 0.3, 0.999, 1e-3, 1e-5, 2.0, 100.0] {
            let r = back(f16_bits(v));
            assert!((r - v).abs() <= v.abs() / 1000.0 + 1e-7, "{v} -> {r}");
        }
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.0), 0);
    }

    #[test]
    fn view_formats_pair_srgb_twins() {
        use wgpu::TextureFormat as F;
        assert_eq!(view_formats(F::Bgra8Unorm), vec![F::Bgra8UnormSrgb]);
        assert_eq!(view_formats(F::Rgba8UnormSrgb), vec![F::Rgba8Unorm]);
        assert!(view_formats(F::Rgba16Float).is_empty());
    }
}
