//! The strobe guard on the GPU (#146, #148): where a frame is drawn on the GPU, it is measured
//! there, and softened there, in one compute pass over the frame.
//!
//! The pass reads the frame's bytes and decodes each pixel to linear light with the processor's
//! own tables ([`jetcarp_core::strobe::decode_tables`]), and gives each tile
//! ([`jetcarp_core::strobe::TILE`] pixels square) its mean relative luminance and mean red
//! measure, and its fine patterns of both (#269; [`jetcarp_core::strobe::frame::Fine`]: its light
//! and dark parts, and which of its 49 windows of 4 by 4 pixels are clearly above or below its
//! mean, each window the sum of four 2 by 2 blocks, summed as the processor sums them):
//! the same [`TileMap`] the processor's reduction gives
//! ([`jetcarp_core::strobe::frame::reduce`]). It decodes by table rather than through an sRGB
//! view because a texture unit's sRGB decoding differs by driver, and the red measure steps from
//! 0 to over 100 at its saturation test, so a pixel decoded a little differently can change a
//! tile's mean by a whole step; with the same tables, and that test in whole numbers
//! ([`jetcarp_core::strobe::red_measure_fixed`]), every driver measures what the processor does,
//! to the rounding of the sums. On the frames the stripe check wants, it also writes the
//! half-resolution luminance image. A workgroup measures four tiles side by side, sixteen threads
//! a tile, each reading four 2 by 2 blocks of pixels once: a column of blocks, every other row. A
//! thread adds its blocks' sums as the first two levels of a tree over the tile's 64 blocks, and
//! the threads the rest, so every sum is the one the pass made when each block had a thread of its
//! own, to the bit, with a quarter of the threads, of the waits for one another (`workgroupBarrier`)
//! and of the shared memory's traffic (#343: on an Intel UHD the pass made the desktop background
//! cost a third of the GPU at 30 frames a second, whatever its cells; see `tests/frame_cost.rs`).
//! The windows' bits are gathered from each thread's, not set one by one in shared memory. When
//! the guard's [`Plan`] softens or veils anything, the same pass then
//! carries it out as [`jetcarp_core::strobe::soften::Softener`] does: it keeps the last frame
//! shown in linear light (16 bits a channel; while nothing is softened, only each tile's mean),
//! moves each softened tile towards the new frame by the fraction that keeps its mean within the
//! step, veils (interpolating the plan between tiles by hand, as the processor does, rather than
//! with a sampler, whose filtering precision is the driver's), writes the frame to show (each
//! byte from the processor's table) and measures that too. With nothing to soften the frame is
//! shown as drawn and nothing is written but the tiles' readings, and only those as drawn are
//! read back (80 bytes a tile, 2.6 MB a frame at 3840 by 2160).
//!
//! The maps come back to the processor through a ring of [`SLOTS`] readback buffers, a frame or two
//! later, never blocking a frame: the guard decides on the frames already shown, and the softening
//! itself needs no round trip, since each tile's step is computed on the GPU from the frame being
//! drawn.
//!
//! [`StrobeGpu::blit`] draws the frame to show (the one drawn, or the softened one) into a render
//! pass: the app's view inside egui's pass, or a surface.

use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::frame::{Fine, LumImage, TileMap, tiles_for};
use jetcarp_core::strobe::guard::{Frame, Guard, Pace};
use jetcarp_core::strobe::soften::Plan;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Readback buffers in flight at once.
pub const SLOTS: usize = 4;

/// Tiles a workgroup of the pass measures, side by side (sixteen threads each, #343).
const TILES_A_GROUP: u32 = 4;

/// Bytes a tile's readings take, as drawn (and again as shown): its means, its fine patterns'
/// light and dark parts and windows, and its mean colour.
const TILE_RECORD: u64 = 80;

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
// Five records a tile, as drawn: the means (in, out), then the fine patterns' light and dark
// parts, luminance's windows, the red measure's windows, and the mean colour in linear light; then
// the same for each tile as shown.
@group(0) @binding(2) var<storage, read_write> tiles: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> halfimg: array<f32>;
@group(0) @binding(4) var<storage, read_write> state: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read_write> prev: array<vec2<f32>>;
@group(0) @binding(6) var<storage, read> plan: array<vec4<f32>>;
@group(0) @binding(7) var shown: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(8) var<storage, read> lut: array<u32>;

const GUARD: u32 = 1u;
const FRESH: u32 = 2u;
const HALF: u32 = 4u;
const DECODE: u32 = 8u;
const PREV: u32 = 16u;
const TABLE: u32 = 32u;
// Where the 16-bit to sRGB byte table starts in `lut` (after the two decode tables).
const ENC_AT: u32 = 512u;

// A workgroup measures four tiles side by side, sixteen threads each (#343). A tile is 64 blocks
// of 2 by 2 pixels, numbered row by row; thread `t` of a tile reads blocks `t`, `t + 16`, `t + 32`
// and `t + 48` (a column of blocks, every other row). Its sums are (b0 + b2) + (b1 + b3) of those
// blocks' sums: the first two levels of the tree a tile's 64 blocks were summed by when each had a
// thread of its own (each block with the one 32 after it, then each pair with the pair 16 after).
// The threads sum the rest of that tree as before, so every mean, part and window is the one that
// pass gave, to the bit, with a quarter of the threads, of the barriers and of the shared memory's
// traffic.
const PER: u32 = 16u;

// Each tile's threads' sums, at `slot * 16 + t`.
var<workgroup> acc: array<vec4<f32>, 64>;
var<workgroup> acc2: array<vec4<f32>, 64>;
var<workgroup> cnt: array<f32, 64>;
// Each tile's blocks' sums (luminance, red measure) and their pixels in the frame, at
// `slot * 64 + block`.
var<workgroup> blk: array<vec2<f32>, 256>;
var<workgroup> blkn: array<u32, 256>;
// Each thread's windows' bits, (luminance clear, luminance above) then (red clear, red above),
// two words each; then each tile's, gathered.
var<workgroup> part: array<vec4<u32>, 128>;
var<workgroup> words: array<vec4<u32>, 8>;

fn dec(x: f32) -> f32 {
  if (x <= 0.04045) { return x / 12.92; }
  return pow((x + 0.055) / 1.055, 2.4);
}
// An sRGB byte's linear value, from the processor's own table.
fn lin8(b: u32) -> f32 { return bitcast<f32>(lut[b]); }
// The red measure of fixed-point linear values (the processor's `red_measure_fixed`): the
// saturation test in whole numbers, so every driver classifies a pixel as the processor does.
fn red_fixed(r: u32, g: u32, b: u32) -> f32 {
  let s = r + g + b;
  if (s > 0u && 5u * r >= 4u * s) { return f32(r - g - b) * 0.000019073486328125; }
  return 0.0;
}
// A linear value's sRGB byte (as 0 to 1), from the processor's table of 16-bit values.
fn enc8(x: f32) -> f32 {
  let i = pack2x16unorm(vec2<f32>(x, 0.0)) & 0xffffu;
  let b = (lut[ENC_AT + (i >> 2u)] >> ((i & 3u) * 8u)) & 0xffu;
  return f32(b) / 255.0;
}
// The veil between tile centres, interpolated as the processor's `Plan::veil_at` does (by hand: a
// sampler's filtering precision is the driver's). A pixel's veil comes from the four tiles whose
// centres are around it, the same four for every pixel of a 2 by 2 block (a block never straddles
// the middle of a tile), so a block reads them once (`veils`); where none of the four is veiled
// the veil is none, as the interpolation would give it.
struct Veils { a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>, any: bool }
fn veil_uv(q: vec2<u32>) -> vec2<f32> {
  return vec2<f32>(clamp((f32(q.x) + 0.5) * 0.0625 - 0.5, 0.0, f32(p.cols - 1u)),
                   clamp((f32(q.y) + 0.5) * 0.0625 - 0.5, 0.0, f32(p.rows - 1u)));
}
fn veils(q: vec2<u32>) -> Veils {
  let uv = veil_uv(q);
  let c0 = u32(floor(uv.x));
  let r0 = u32(floor(uv.y));
  let c1 = min(c0 + 1u, p.cols - 1u);
  let r1 = min(r0 + 1u, p.rows - 1u);
  var k: Veils;
  k.a = plan[r0 * p.cols + c0].yz;
  k.b = plan[r0 * p.cols + c1].yz;
  k.c = plan[r1 * p.cols + c0].yz;
  k.d = plan[r1 * p.cols + c1].yz;
  k.any = any(k.a != vec2<f32>(0.0)) || any(k.b != vec2<f32>(0.0)) || any(k.c != vec2<f32>(0.0)) || any(k.d != vec2<f32>(0.0));
  return k;
}
// The veil at pixel q, of its block's four tiles `k`.
fn veil_at(q: vec2<u32>, k: Veils) -> vec2<f32> {
  if (!k.any) { return vec2<f32>(0.0); }
  let uv = veil_uv(q);
  let fu = uv.x - f32(u32(floor(uv.x)));
  let fv = uv.y - f32(u32(floor(uv.y)));
  let top = k.a + (k.b - k.a) * fu;
  let bottom = k.c + (k.d - k.c) * fu;
  return top + (bottom - top) * fv;
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
// Pixel q's colour in linear light, and its red measure.
fn pixel(q: vec2<u32>) -> vec4<f32> {
  var c = textureLoad(src, vec2<i32>(q), 0).rgb;
  if ((p.mode & TABLE) != 0u) {
    // 8-bit sRGB values: back to their bytes, decoded with the processor's tables.
    let b = vec3<u32>(round(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
    return vec4<f32>(lin8(b.x), lin8(b.y), lin8(b.z), red_fixed(lut[256u + b.x], lut[256u + b.y], lut[256u + b.z]));
  }
  if ((p.mode & DECODE) != 0u) { c = vec3<f32>(dec(c.r), dec(c.g), dec(c.b)); }
  return vec4<f32>(c, red(c));
}
fn inside(q: vec2<u32>) -> bool { return q.x < p.size.x && q.y < p.size.y; }

// A block of 2 by 2 pixels, in reading order: each pixel's luminance (l), red measure (r) and
// colour in linear light (c0 to c3), which are in the frame (ok), and the block's sums, pixel by
// pixel in that order, of luminance, red measure and colour (a: luminance, red measure, red,
// green; b: blue), and its pixels in the frame (n). Where the stripe check wants it, the block's
// mean luminance goes into the half-resolution image.
struct Block {
  l: vec4<f32>,
  r: vec4<f32>,
  c0: vec3<f32>,
  c1: vec3<f32>,
  c2: vec3<f32>,
  c3: vec3<f32>,
  ok: vec4<bool>,
  a: vec4<f32>,
  b: f32,
  n: f32,
}
fn read_block(base: vec2<u32>) -> Block {
  var k: Block;
  var crgb = vec3<f32>(0.0);
  var l = 0.0;
  var r = 0.0;
  var n = 0.0;
  let q0 = base;
  let q1 = base + vec2<u32>(1u, 0u);
  let q2 = base + vec2<u32>(0u, 1u);
  let q3 = base + vec2<u32>(1u, 1u);
  k.ok = vec4<bool>(inside(q0), inside(q1), inside(q2), inside(q3));
  if (k.ok.x) { let x = pixel(q0); k.c0 = x.rgb; crgb += x.rgb; k.l.x = lum(x.rgb); k.r.x = x.w; l += k.l.x; r += x.w; n += 1.0; }
  if (k.ok.y) { let x = pixel(q1); k.c1 = x.rgb; crgb += x.rgb; k.l.y = lum(x.rgb); k.r.y = x.w; l += k.l.y; r += x.w; n += 1.0; }
  if (k.ok.z) { let x = pixel(q2); k.c2 = x.rgb; crgb += x.rgb; k.l.z = lum(x.rgb); k.r.z = x.w; l += k.l.z; r += x.w; n += 1.0; }
  if (k.ok.w) { let x = pixel(q3); k.c3 = x.rgb; crgb += x.rgb; k.l.w = lum(x.rgb); k.r.w = x.w; l += k.l.w; r += x.w; n += 1.0; }
  if ((p.mode & HALF) != 0u && n > 0.0) {
    halfimg[(base.y >> 1u) * p.half_w + (base.x >> 1u)] = l / n;
  }
  k.a = vec4<f32>(l, r, crgb.r, crgb.g);
  k.b = crgb.b;
  k.n = n;
  return k;
}
// One block's part of the tile's fine patterns, as the processor's
// `jetcarp_core::strobe::frame::fine_of` works them out, around the tile's means m (luminance in
// x, the red measure in y): the sums of its pixels above them and at or below them (s), and how
// many (c); its sums and pixels go to `blk` and `blkn` at `at`, for the windows.
struct Parts { s: vec4<f32>, c: vec4<f32> }
fn one(x: vec2<f32>, m: vec2<f32>, q: ptr<function, Parts>) {
  if (x.x > m.x) { (*q).s.x += x.x; (*q).c.x += 1.0; } else { (*q).s.y += x.x; (*q).c.y += 1.0; }
  if (x.y > m.y) { (*q).s.z += x.y; (*q).c.z += 1.0; } else { (*q).s.w += x.y; (*q).c.w += 1.0; }
}
fn block_parts(k: Block, m: vec2<f32>, at: u32) -> Parts {
  var q: Parts;
  var b = vec2<f32>(0.0);
  var nb = 0u;
  if (k.ok.x) { let x = vec2<f32>(k.l.x, k.r.x); one(x, m, &q); b += x; nb += 1u; }
  if (k.ok.y) { let x = vec2<f32>(k.l.y, k.r.y); one(x, m, &q); b += x; nb += 1u; }
  if (k.ok.z) { let x = vec2<f32>(k.l.z, k.r.z); one(x, m, &q); b += x; nb += 1u; }
  if (k.ok.w) { let x = vec2<f32>(k.l.w, k.r.w); one(x, m, &q); b += x; nb += 1u; }
  blk[at] = b;
  blkn[at] = nb;
  return q;
}
// The rest of the tree over a tile's sixteen sums, from `s`: each level adds in the sum `st`
// further on, as the tree over 64 did from its level of 8.
fn reduce3(t: u32, s: u32) {
  for (var st = 8u; st > 0u; st = st >> 1u) {
    if (t < st) {
      acc[s + t] = acc[s + t] + acc[s + t + st];
      acc2[s + t] = acc2[s + t] + acc2[s + t + st];
      cnt[s + t] = cnt[s + t] + cnt[s + t + st];
    }
    workgroupBarrier();
  }
}
fn reduce2(t: u32, s: u32) {
  for (var st = 8u; st > 0u; st = st >> 1u) {
    if (t < st) {
      acc[s + t] = acc[s + t] + acc[s + t + st];
      acc2[s + t] = acc2[s + t] + acc2[s + t + st];
    }
    workgroupBarrier();
  }
}
// The tile's fine patterns, of luminance and of the red measure, from the thread's four blocks
// around the tile's means m: the first thread writes them as records `at + 1` to `at + 3` (where
// the tile is `valid`, inside the frame). Called by every thread.
fn fine(t: u32, slot: u32, k0: Block, k1: Block, k2: Block, k3: Block, m: vec2<f32>, at: u32, valid: bool) {
  let s = slot * PER;
  let bs = slot * 64u;
  let q0 = block_parts(k0, m, bs + t);
  let q1 = block_parts(k1, m, bs + t + 16u);
  let q2 = block_parts(k2, m, bs + t + 32u);
  let q3 = block_parts(k3, m, bs + t + 48u);
  acc[s + t] = (q0.s + q2.s) + (q1.s + q3.s);
  acc2[s + t] = (q0.c + q2.c) + (q1.c + q3.c);
  workgroupBarrier();
  reduce2(t, s);
  let ts = acc[s];
  let tc = acc2[s];
  let ll = select(m.x, ts.x / tc.x, tc.x > 0.0);
  let ld = select(m.x, ts.y / tc.y, tc.y > 0.0);
  let rl = select(m.y, ts.z / tc.z, tc.z > 0.0);
  let rd = select(m.y, ts.w / tc.w, tc.w > 0.0);
  // Windows of 4 by 4 pixels, every 2: 7 across, 7 down, each four whole blocks. Thread t takes
  // windows t, t + 16, t + 32 and t + 48. Luminance's clear (xy: the two words) and above (zw);
  // the red measure's the same.
  var lw = vec4<u32>(0u);
  var rw = vec4<u32>(0u);
  for (var w = t; w < 49u; w += PER) {
    let b0 = bs + (w / 7u) * 8u + w % 7u;
    if (blkn[b0] == 4u && blkn[b0 + 1u] == 4u && blkn[b0 + 8u] == 4u && blkn[b0 + 9u] == 4u) {
      let d = ((blk[b0] + blk[b0 + 1u]) + (blk[b0 + 8u] + blk[b0 + 9u])) / 16.0 - m;
      let bit = 1u << (w & 31u);
      let in_word = select(vec2<u32>(0u, bit), vec2<u32>(bit, 0u), w < 32u);
      if (ll > ld && abs(d.x) >= 0.1 * (ll - ld)) {
        lw = lw | vec4<u32>(in_word, select(vec2<u32>(0u), in_word, d.x > 0.0));
      }
      if (rl > rd && abs(d.y) >= 0.1 * (rl - rd)) {
        rw = rw | vec4<u32>(in_word, select(vec2<u32>(0u), in_word, d.y > 0.0));
      }
    }
  }
  part[(s + t) * 2u] = lw;
  part[(s + t) * 2u + 1u] = rw;
  workgroupBarrier();
  // Gathered by two threads a tile, one for luminance and one for the red measure.
  if (t < 2u) {
    var x = vec4<u32>(0u);
    for (var u = 0u; u < PER; u++) { x = x | part[(s + u) * 2u + t]; }
    words[slot * 2u + t] = x;
  }
  workgroupBarrier();
  if (t == 0u && valid) {
    tiles[at + 1u] = bitcast<vec4<u32>>(vec4<f32>(ll, ld, rl, rd));
    tiles[at + 2u] = words[slot * 2u];
    tiles[at + 3u] = words[slot * 2u + 1u];
  }
  // Everyone has read the sums before the arrays are used again.
  workgroupBarrier();
}

// A pixel as shown: softened towards the last frame shown, by `beta`, and veiled; written to
// `state` (softened, before the veil) and `shown`. Its colour as shown (o) and softened (c).
struct Px { o: vec3<f32>, c: vec3<f32> }
fn shown_px(q: vec2<u32>, px: vec3<f32>, beta: f32, pv: vec2<f32>, vk: Veils) -> Px {
  let at = q.y * p.size.x + q.x;
  var c = px;
  if (beta < 1.0) {
    // The first softened frame moves from a grey at the tile's last mean.
    var was = vec3<f32>(pv.x);
    if ((p.mode & FRESH) == 0u) {
      let st = state[at];
      was = vec3<f32>(unpack2x16unorm(st.x), unpack2x16unorm(st.y).x);
    }
    c = was + (px - was) * beta;
  }
  state[at] = vec2<u32>(pack2x16unorm(c.rg), pack2x16unorm(vec2<f32>(c.b, 0.0)));
  c = q16(c);
  let v = veil_at(q, vk);
  let o = c * (1.0 - v.x) + vec3<f32>(v.y);
  textureStore(shown, vec2<i32>(q), vec4<f32>(enc8(o.r), enc8(o.g), enc8(o.b), 1.0));
  var r: Px;
  r.o = o;
  r.c = c;
  return r;
}
// A block as shown: its pixels' luminance and red measure as shown (in `k`), the sums of the
// softened pixels' luminance and red measure and of the shown ones' (sums), and of their colour.
struct OutBlock { k: Block, sums: vec4<f32>, orgb: vec3<f32> }
fn add_out(x: Px, o: ptr<function, OutBlock>) -> vec2<f32> {
  let v = vec2<f32>(lum(x.o), red(x.o));
  (*o).orgb += x.o;
  (*o).sums += vec4<f32>(lum(x.c), red(x.c), v.x, v.y);
  return v;
}
fn out_block(k: Block, base: vec2<u32>, beta: f32, pv: vec2<f32>) -> OutBlock {
  var o: OutBlock;
  o.k.ok = k.ok;
  o.k.n = k.n;
  let vk = veils(base);
  if (k.ok.x) { let v = add_out(shown_px(base, k.c0, beta, pv, vk), &o); o.k.l.x = v.x; o.k.r.x = v.y; }
  if (k.ok.y) { let v = add_out(shown_px(base + vec2<u32>(1u, 0u), k.c1, beta, pv, vk), &o); o.k.l.y = v.x; o.k.r.y = v.y; }
  if (k.ok.z) { let v = add_out(shown_px(base + vec2<u32>(0u, 1u), k.c2, beta, pv, vk), &o); o.k.l.z = v.x; o.k.r.z = v.y; }
  if (k.ok.w) { let v = add_out(shown_px(base + vec2<u32>(1u, 1u), k.c3, beta, pv, vk), &o); o.k.l.w = v.x; o.k.r.w = v.y; }
  return o;
}

@compute @workgroup_size(16, 4)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_id) li: vec3<u32>) {
  let t = li.x;
  let slot = li.y;
  let s = slot * PER;
  let tx = wg.x * 4u + slot;
  let valid = tx < p.cols;
  let tile = wg.y * p.cols + tx;
  let corner = vec2<u32>(tx * 16u + (t % 8u) * 2u, wg.y * 16u + (t / 8u) * 2u);
  let c1 = corner + vec2<u32>(0u, 4u);
  let c2 = corner + vec2<u32>(0u, 8u);
  let c3 = corner + vec2<u32>(0u, 12u);
  // A workgroup past the frame's last column still meets every barrier; its tile writes nothing.
  let k0 = read_block(corner);
  let k1 = read_block(c1);
  let k2 = read_block(c2);
  let k3 = read_block(c3);
  acc[s + t] = (k0.a + k2.a) + (k1.a + k3.a);
  acc2[s + t] = vec4<f32>((k0.b + k2.b) + (k1.b + k3.b), 0.0, 0.0, 0.0);
  cnt[s + t] = (k0.n + k2.n) + (k1.n + k3.n);
  workgroupBarrier();
  reduce3(t, s);
  let total = max(cnt[s], 1.0);
  let lin = acc[s].x / total;
  let rin = acc[s].y / total;
  let cin = vec3<f32>(acc[s].z, acc[s].w, acc2[s].x) / total;
  // Everyone has read the sums before the arrays are used again.
  workgroupBarrier();
  let rec = tile * 5u;
  if (t == 0u && valid) {
    tiles[rec + 4u] = bitcast<vec4<u32>>(vec4<f32>(cin, 0.0));
  }
  fine(t, slot, k0, k1, k2, k3, vec2<f32>(lin, rin), rec, valid);
  if ((p.mode & GUARD) == 0u) {
    if (t == 0u && valid) {
      tiles[rec] = bitcast<vec4<u32>>(vec4<f32>(lin, rin, lin, rin));
      prev[tile] = vec2<f32>(lin, rin);
    }
    return;
  }
  var beta = 1.0;
  var pl = vec4<f32>(0.0);
  var pv = vec2<f32>(0.0);
  if (valid) {
    pl = plan[tile];
    pv = prev[tile];
  }
  if ((p.mode & PREV) != 0u && pl.x > 0.5) {
    let dl = abs(lin - pv.x);
    let dr = abs(rin - pv.y);
    if (dl > p.lum_step) { beta = min(beta, p.lum_step / dl); }
    if (dr > p.red_step) { beta = min(beta, p.red_step / dr); }
  }
  let o0 = out_block(k0, corner, beta, pv);
  let o1 = out_block(k1, c1, beta, pv);
  let o2 = out_block(k2, c2, beta, pv);
  let o3 = out_block(k3, c3, beta, pv);
  acc[s + t] = (o0.sums + o2.sums) + (o1.sums + o3.sums);
  acc2[s + t] = vec4<f32>((o0.orgb + o2.orgb) + (o1.orgb + o3.orgb), 0.0);
  cnt[s + t] = (k0.n + k2.n) + (k1.n + k3.n);
  workgroupBarrier();
  reduce3(t, s);
  let tt = max(cnt[s], 1.0);
  let a0 = acc[s];
  let cc = acc2[s];
  workgroupBarrier();
  let out_rec = (p.cols * p.rows + tile) * 5u;
  if (t == 0u && valid) {
    prev[tile] = vec2<f32>(a0.x / tt, a0.y / tt);
    tiles[rec] = bitcast<vec4<u32>>(vec4<f32>(lin, rin, a0.z / tt, a0.w / tt));
    tiles[out_rec + 4u] = bitcast<vec4<u32>>(vec4<f32>(cc.xyz / tt, 0.0));
  }
  // The fine patterns shown.
  fine(t, slot, o0.k, o1.k, o2.k, o3.k, vec2<f32>(a0.z / tt, a0.w / tt), out_rec, valid);
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
const PREV: u32 = 16;
const TABLE: u32 = 32;

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

/// A tile's fine patterns, of luminance and of the red measure, from its record (its second to
/// fourth 16 bytes: the light and dark parts, then each one's windows clear and above, in two
/// words each).
fn fines(r: &[u32]) -> (Fine, Fine) {
    let f = f32::from_bits;
    let bits = |lo: u32, hi: u32| lo as u64 | (hi as u64) << 32;
    (
        Fine {
            light: f(r[4]),
            dark: f(r[5]),
            sig: bits(r[8], r[9]),
            sign: bits(r[10], r[11]),
        },
        Fine {
            light: f(r[6]),
            dark: f(r[7]),
            sig: bits(r[12], r[13]),
            sign: bits(r[14], r[15]),
        },
    )
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
    /// The plan, a tile's `[soften, veil amount, amount times grey, 0]` each.
    plan: wgpu::Buffer,
    /// The plan last uploaded.
    plan_now: Option<Plan>,
}

/// A texture to draw frames into, for the app's view or a surface that cannot be read.
struct Scene {
    tex: wgpu::Texture,
    render: wgpu::TextureView,
    decode: wgpu::TextureView,
    plain: wgpu::TextureView,
    /// The view the pass measures through, and how it reads it ([`measuring_view`]).
    measure: wgpu::TextureView,
    read: u32,
}

/// The strobe guard's GPU half, for one view.
pub struct StrobeGpu {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_module: wgpu::ShaderModule,
    blits: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    params: wgpu::Buffer,
    /// The processor's decode tables and its 16-bit to sRGB byte table.
    lut: wgpu::Buffer,
    dummy_state: wgpu::Buffer,
    dummy_shown: wgpu::TextureView,
    dummy_plan: wgpu::Buffer,
    sized: Option<Sized>,
    soft: Option<Soft>,
    scene: Option<Scene>,
    /// The state holds the last frame shown (the last frame was softened, at this size).
    state_valid: bool,
    /// The last tiles buffer holds the last frame's tile means, at this size.
    prev_valid: bool,
    /// What the last frame showed, and the blit set up for it.
    shown: Shown,
    blit_bind: Option<wgpu::BindGroup>,
    blit_format: Option<wgpu::TextureFormat>,
    /// The device cannot hold the softening's state for frames this large.
    too_large: bool,
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

fn read_only(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
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

/// A view of `tex` whose reads give linear light (for drawing it to targets that encode sRGB),
/// where its format has an sRGB twin; else its plain view.
fn decoding_view(tex: &wgpu::Texture) -> wgpu::TextureView {
    let f = tex.format();
    let s = f.add_srgb_suffix();
    if s.is_srgb() {
        tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("strobe guard: decoding view"),
            format: Some(s),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        })
    } else {
        tex.create_view(&Default::default())
    }
}

/// The view of `tex` the pass measures through, and how the shader reads it: 8-bit formats
/// with an sRGB twin as their bytes, decoded with the processor's own tables, so the GPU
/// measures exactly what the processor measures from the same pixels (a texture unit's sRGB
/// decoding, and `pow`, differ by driver); other formats decoded by hand.
fn measuring_view(tex: &wgpu::Texture) -> (wgpu::TextureView, u32) {
    let f = tex.format();
    if f.add_srgb_suffix().is_srgb() {
        let v = tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("strobe guard: measuring view"),
            format: Some(f.remove_srgb_suffix()),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        });
        (v, TABLE)
    } else {
        (tex.create_view(&Default::default()), DECODE)
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
                read_only(6),
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
                read_only(8),
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
        let lut = {
            use wgpu::util::DeviceExt;
            let mut words: Vec<u32> = jetcarp_core::strobe::decode_tables().to_vec();
            words.extend(
                jetcarp_core::strobe::soften::encode_table()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b)),
            );
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("strobe guard tables"),
                contents: bytemuck::cast_slice(&words),
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
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
        let dummy_plan = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("strobe guard: no plan"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        StrobeGpu {
            layout,
            pipeline,
            blit_layout,
            blit_module,
            blits: HashMap::new(),
            params,
            lut,
            dummy_state,
            dummy_shown,
            dummy_plan,
            sized: None,
            soft: None,
            scene: None,
            state_valid: false,
            prev_valid: false,
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
            let decode = decoding_view(&tex);
            let (measure, read) = measuring_view(&tex);
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
                measure,
                read,
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
        // Five records of 16 bytes a tile as drawn, and five as shown.
        let tile_bytes = cols as u64 * rows as u64 * TILE_RECORD * 2;
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
        self.prev_valid = false;
        self.too_large = false;
    }

    fn soft_resources(&mut self, device: &wgpu::Device) -> bool {
        let Some(s) = &self.sized else { return false };
        if self.soft.is_some() {
            return true;
        }
        let bytes = s.w as u64 * s.h as u64 * 8;
        let limits = device.limits();
        if bytes > limits.max_storage_buffer_binding_size || bytes > limits.max_buffer_size {
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
        let plan = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("strobe guard: plan"),
            size: (s.cols as u64 * s.rows as u64 * 16).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.soft = Some(Soft {
            state,
            shown,
            shown_store,
            shown_plain,
            shown_srgb,
            plan,
            plan_now: None,
        });
        true
    }

    fn upload_plan(&mut self, queue: &wgpu::Queue, plan: &Plan) {
        let Some(soft) = &mut self.soft else { return };
        if soft.plan_now.as_ref() == Some(plan) {
            return;
        }
        // As the processor's `Plan::veil_at` takes them: the amount, and the grey times it.
        let data: Vec<[f32; 4]> = plan
            .soften
            .iter()
            .zip(&plan.veil)
            .map(|(s, v)| [if *s { 1.0 } else { 0.0 }, v[0], v[0] * v[1], 0.0])
            .collect();
        queue.write_buffer(&soft.plan, 0, bytemuck::cast_slice(&data));
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
        let (src_view, read, w, h) = match source {
            Some(t) => {
                let (v, m) = measuring_view(t);
                (v, m, t.width(), t.height())
            }
            None => {
                let Some(s) = &self.scene else {
                    return Shown::Drawn;
                };
                (s.measure.clone(), s.read, s.tex.width(), s.tex.height())
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
        mode |= read;
        if self.prev_valid {
            mode |= PREV;
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
                    resource: soft
                        .map_or(&self.dummy_plan, |x| &x.plan)
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(
                        soft.map_or(&self.dummy_shown, |x| &x.shown_store),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: self.lut.as_entire_binding(),
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
            pass.dispatch_workgroups(cols.div_ceil(TILES_A_GROUP), rows, 1);
        }
        // The readings, if a slot is free (else this frame goes unread; the softening holds).
        if let Some(slot) = s.slots.iter_mut().find(|x| x.state == SlotState::Free) {
            // The tiles as shown only when they were softened (else they are the tiles drawn).
            let drawn = cols as u64 * rows as u64 * TILE_RECORD;
            enc.copy_buffer_to_buffer(
                &s.tiles,
                0,
                &slot.tiles,
                0,
                if soften { drawn * 2 } else { drawn },
            );
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
        self.prev_valid = true;
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
                        let rec = TILE_RECORD as usize / 4;
                        let u: &[u32] = bytemuck::cast_slice(&v[..n * rec * 4 * 2]);
                        let f = f32::from_bits;
                        for t in 0..n {
                            let r = &u[t * rec..(t + 1) * rec];
                            input.lum[t] = f(r[0]);
                            input.red[t] = f(r[1]);
                            output.lum[t] = f(r[2]);
                            output.red[t] = f(r[3]);
                            (input.fine[t], input.red_fine[t]) = fines(r);
                            input.rgb[t] = [f(r[16]), f(r[17]), f(r[18])];
                            if slot.soft {
                                let o = &u[(n + t) * rec..(n + t + 1) * rec];
                                (output.fine[t], output.red_fine[t]) = fines(o);
                                output.rgb[t] = [f(o[16]), f(o[17]), f(o[18])];
                            }
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

/// The strobe guard for one view drawn on the GPU: the pass ([`StrobeGpu`]) and the decisions
/// ([`Guard`]) together, as every GPU output uses them: the app's view and outputs, and the
/// shared painter behind the desktop background and the screen savers.
///
/// Each frame: [`update`](Self::update) before drawing (the readings of frames the GPU has
/// finished go to the guard, which decides), then draw, then [`frame`](Self::frame) to measure
/// and soften what was drawn, then [`StrobeGpu::after_submit`] once the work is submitted.
pub struct GuardedGpu {
    pub gpu: StrobeGpu,
    pub guard: Guard,
    pace: Pace,
    last_time: Option<f64>,
}

impl GuardedGpu {
    /// A guard for a view watched as `viewing` says, all of a screen or part of one of
    /// `screen_px` pixels, on or off as the person's setting says.
    pub fn new(
        device: &wgpu::Device,
        viewing: Viewing,
        screen_px: Option<f64>,
        on: bool,
    ) -> GuardedGpu {
        GuardedGpu {
            gpu: StrobeGpu::new(device),
            guard: Guard::new(viewing, screen_px, on),
            pace: Pace::default(),
            last_time: None,
        }
    }

    /// Hands the readings of finished frames to the guard, with the pace the scene runs at now.
    pub fn update(&mut self, device: &wgpu::Device, pace: Pace) {
        self.pace = pace;
        for r in self.gpu.take(device) {
            self.guard.observe(
                Frame {
                    time: r.time,
                    input: &r.input,
                    output: &r.output,
                    half: r.half.as_ref(),
                },
                pace,
            );
        }
        self.guard.set_can_soften(self.gpu.can_soften());
    }

    /// Measures, and softens as the guard's plan says, the frame drawn at `now` (in `source`, or
    /// the scene texture).
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        source: Option<&wgpu::Texture>,
        now: f64,
    ) -> Shown {
        let dt = self.last_time.map_or(1.0 / 60.0, |l| (now - l).max(0.0)) as f32;
        self.last_time = Some(now);
        let half = self.guard.wants_half(now);
        let plan = if self.guard.on() {
            self.guard.plan().clone()
        } else {
            Plan::default()
        };
        self.gpu
            .process(device, queue, enc, source, &plan, dt, now, half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dispatch matches the pass's workgroups: four tiles side by side, sixteen threads each.
    #[test]
    fn a_workgroup_measures_four_tiles() {
        assert!(SHADER.contains("@compute @workgroup_size(16, 4)"));
        assert!(SHADER.contains("const PER: u32 = 16u;"));
        assert!(SHADER.contains(&format!("let tx = wg.x * {TILES_A_GROUP}u + slot;")));
    }

    #[test]
    fn view_formats_pair_srgb_twins() {
        use wgpu::TextureFormat as F;
        assert_eq!(view_formats(F::Bgra8Unorm), vec![F::Bgra8UnormSrgb]);
        assert_eq!(view_formats(F::Rgba8UnormSrgb), vec![F::Rgba8Unorm]);
        assert!(view_formats(F::Rgba16Float).is_empty());
    }
}
