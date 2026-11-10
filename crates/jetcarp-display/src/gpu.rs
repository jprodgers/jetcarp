//! Tile renderer, two paths:
//!
//! - **Lattice tilings** (square, hex, every periodic tiling): no per-cell data on the GPU. Each
//!   frame the app picks the visible window of unit cells and a sampling stride; the vertex shader
//!   rebuilds every cell's position from its instance number, the lattice vectors, the row shear
//!   table, and its tile type's transform. Memory and upload size depend on the screen, not on the
//!   grid, so grids of hundreds of millions of cells draw. When more cells are in view than the
//!   budget, every `stride`-th cell is drawn `stride` times larger (cells are then below a pixel).
//!   Colours: a texture with one texel per window slot.
//! - **Explicit patches** (aperiodic): every cell is an instance (position and orientation, 12
//!   bytes). Instances are sorted into a grid of bins by position, so a frame draws only the bin
//!   rows in view; when tiles shrink to a few pixels they are drawn as straight polygons, below
//!   two pixels as one triangle each, and below half a pixel only every 16th instance is drawn,
//!   16 times larger (a level-8 patch has 17 million tiles). Colours: a storage buffer with one
//!   colour per instance slot. The app colours only the slots in view that are not already
//!   current (after a generation, the ones in view; after a pan, the ones that came into view)
//!   and queues them on the geometry; the renderer uploads the queue at its next frame.
//!
//! Positions are relative to a lattice point near the camera, so f32 stays precise far from the
//! origin.

use bytemuck::{Pod, Zeroable};
use jetcarp_core::Topology;
use jetcarp_core::geom::triangulate;
use jetcarp_core::topology::Storage;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Lattice colour textures are this many texels wide; their height follows the window.
pub const TEX_W: u32 = 4096;
/// Fewest cells drawn in a lattice frame before sampling kicks in. A view's budget is larger
/// when it has more pixels ([`window_budget`]), so every cell of a full-screen view at one
/// pixel per cell draws.
pub const MIN_BUDGET: usize = 2_000_000;
/// Lattice colour texture rows are allocated in steps of this many (a texture grows, never
/// shrinks, while its view lives).
const ROW_STEP: u32 = 256;

/// Cells a lattice view of `px` physical pixels draws before sampling: 1.25 per pixel (cells
/// partly in view at the edges, and tilings whose tiles are not quite one pixel), at least
/// [`MIN_BUDGET`], at most `cap` (the colour texture's capacity).
pub fn window_budget(px: f64, cap: usize) -> usize {
    ((px * 1.25) as usize).max(MIN_BUDGET).min(cap.max(1))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    center: [f32; 2],
    scale: [f32; 2],
    /// 1 when the target is sRGB (colours are sRGB bytes and are decoded in the shader).
    srgb: u32,
    line_alpha: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LatUniforms {
    scale: [f32; 2],
    a1: [f32; 2],
    a2: [f32; 2],
    off: [f32; 2],
    u0: i32,
    j0: i32,
    nu: u32,
    nj: u32,
    stride: i32,
    ic: i32,
    jc: i32,
    per_type: u32,
    tex_w: u32,
    line_alpha: f32,
    _pad: [u32; 2],
}

/// Lattice data the shader needs to place any cell.
pub struct LatticeInfo {
    pub a1: [f64; 2],
    pub a2: [f64; 2],
    pub w: u32,
    pub h: u32,
    pub ntypes: u32,
    pub row_start: Vec<i32>,
    /// Per tile type: affine rows (x row, y row) and shape index.
    types: Vec<([f32; 8], usize)>,
}

/// The cells drawn this frame on a lattice: unit cells `u0 + a*stride`, `j0 + b*stride` for
/// `a < nu`, `b < nj`, every tile type. Colour slot of (k, b, a) is `(k*nj + b)*nu + a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub u0: i32,
    pub j0: i32,
    pub nu: u32,
    pub nj: u32,
    pub stride: i32,
}

impl Window {
    pub fn slots(&self, ntypes: u32) -> usize {
        self.nu as usize * self.nj as usize * ntypes as usize
    }
    /// Cell ids in colour-slot order.
    pub fn cells(&self, g: &jetcarp_core::topology::LatticeGrid) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.slots(g.ntypes));
        for k in 0..g.ntypes {
            for b in 0..self.nj as i32 {
                for a in 0..self.nu as i32 {
                    v.push(g.index(self.u0 + a * self.stride, self.j0 + b * self.stride, k) as u32);
                }
            }
        }
        v
    }
}

impl LatticeInfo {
    /// The window of unit cells covering a view (world centre, pixels per unit, view size in
    /// pixels). More than `budget` cells in view are sampled; at most `cap` slots are used.
    pub fn window(
        &self,
        center: [f64; 2],
        zoom: f64,
        size: [f32; 2],
        budget: usize,
        cap: usize,
    ) -> Option<Window> {
        let (hw, hh) = (size[0] as f64 / zoom / 2.0, size[1] as f64 / zoom / 2.0);
        let mut fi = (f64::MAX, f64::MIN);
        let mut fj = (f64::MAX, f64::MIN);
        for (x, y) in [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh)] {
            let p = [center[0] + x, center[1] + y];
            let j = p[1] / self.a2[1];
            let i = (p[0] - j * self.a2[0]) / self.a1[0];
            fi = (fi.0.min(i), fi.1.max(i));
            fj = (fj.0.min(j), fj.1.max(j));
        }
        let pad = 3.0;
        let j0 = ((fj.0 - pad).floor() as i64).max(0);
        let j1 = ((fj.1 + pad).ceil() as i64).min(self.h as i64);
        if j0 >= j1 {
            return None;
        }
        let rs = |j: i64| self.row_start[j.clamp(0, self.h as i64 - 1) as usize] as i64;
        let (rlo, rhi) = (rs(j0).min(rs(j1 - 1)), rs(j0).max(rs(j1 - 1)));
        let u0 = ((fi.0 - pad).floor() as i64 - rhi).max(0);
        let u1 = ((fi.1 + pad).ceil() as i64 - rlo).min(self.w as i64);
        if u0 >= u1 {
            return None;
        }
        let count = (u1 - u0) as f64 * (j1 - j0) as f64 * self.ntypes as f64;
        let mut stride = (count / budget.max(1) as f64).sqrt().ceil().max(1.0) as i64;
        loop {
            // Align to the stride so the sampled cells stay put while panning.
            let (a0, b0) = (u0 / stride * stride, j0 / stride * stride);
            let nu = (u1 - a0 + stride - 1) / stride;
            let nj = (j1 - b0 + stride - 1) / stride;
            // Keep the last sample inside the grid.
            let nu = nu.min((self.w as i64 - 1 - a0) / stride + 1);
            let nj = nj.min((self.h as i64 - 1 - b0) / stride + 1);
            let slots = nu * nj * self.ntypes as i64;
            if slots <= cap as i64 {
                return Some(Window {
                    u0: a0 as i32,
                    j0: b0 as i32,
                    nu: nu as u32,
                    nj: nj as u32,
                    stride: stride as i32,
                });
            }
            stride += 1;
        }
    }
}

/// Vertex ranges of one shape and its instance range.
#[derive(Clone, Debug)]
struct ShapeDraw {
    /// Triangles of the drawn outline (curved edges flattened).
    tri: std::ops::Range<u32>,
    line: std::ops::Range<u32>,
    /// Triangles of the straight corner polygon (tiles a few pixels across).
    lo: std::ops::Range<u32>,
    /// One triangle of the tile's area about its vertex average (tiles under two pixels).
    dot: std::ops::Range<u32>,
    /// One triangle of `SAMPLE` tiles' area (the sampled instances, tiles under half a pixel).
    big: std::ops::Range<u32>,
    inst: std::ops::Range<u32>,
}

/// Explicit patches with more tiles than this also get a sampled instance set.
const SAMPLE_MIN: usize = 1 << 18;
/// One instance in `SAMPLE` (in bin order) is drawn, `SAMPLE` times larger, when tiles are under
/// half a pixel: millions of sub-pixel tiles cost a sixteenth of the vertices and colours.
const SAMPLE: usize = 16;

/// Explicit patches: instances sorted by a grid of bins over their positions, row by row, so the
/// bins in view are a few contiguous instance ranges.
struct Bins {
    origin: [f64; 2],
    cell: f64,
    cols: usize,
    rows: usize,
    /// Largest distance from an instance's position to its tile's corners (and to the corners
    /// of the sampled set's enlarged triangles).
    reach: f64,
    /// Per shape: first instance of each bin (row-major), plus the end.
    start: Vec<Vec<u32>>,
    /// The same for the sampled instances, which follow the full set.
    sampled: Option<Vec<Vec<u32>>>,
}

/// One explicit-patch instance: the tile's position and its orientation (an index into the
/// table of distinct linear parts), 12 bytes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    t: [f32; 2],
    orient: u32,
}

/// Colours waiting for the renderer: (first slot, colours).
pub type ColorChunk = (u32, Vec<[u8; 4]>);

/// CPU-side geometry for a topology, built once per tiling.
pub struct Geometry {
    pub id: u64,
    tris: Vec<[f32; 2]>,
    lines: Vec<[f32; 2]>,
    /// Per shape. Lattices have no instances.
    shapes: Vec<ShapeDraw>,
    /// Explicit patches: taken by the first renderer to upload them (not needed after that).
    instances: std::sync::Mutex<Vec<Instance>>,
    /// That upload, which any other renderer reuses. The host and the saver have a renderer per
    /// screen, all on one device; uploading again would find `instances` empty.
    uploaded: std::sync::OnceLock<wgpu::Buffer>,
    /// Distinct linear parts (a, b, c, d) of the tile transforms.
    orients: Vec<[f32; 4]>,
    /// Explicit patches: the cell drawn by each instance slot (colours are per slot).
    slot_cell: Vec<u32>,
    bins: Option<Bins>,
    /// Colours computed by the app, uploaded by the renderer at its next frame.
    pending: std::sync::Mutex<Vec<ColorChunk>>,
    /// Typical tile size (square root of the first shape's area), world units.
    tile: f64,
    pub lattice: Option<LatticeInfo>,
}

/// Which instances of an explicit patch a frame draws.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchView {
    /// 0 full outline, 1 straight polygon, 2 one triangle, 3 the sampled set's enlarged triangle.
    pub lod: u8,
    /// Per shape: instance (slot) ranges in view.
    ranges: Vec<Vec<std::ops::Range<u32>>>,
}

impl PatchView {
    /// Every slot range in view, sorted.
    pub fn slots(&self) -> Vec<std::ops::Range<u32>> {
        let mut v: Vec<_> = self.ranges.iter().flatten().cloned().collect();
        v.sort_by_key(|r| r.start);
        v
    }
}

impl Geometry {
    pub fn build(topo: &Topology, id: u64) -> Geometry {
        let mut tris = Vec::new();
        let mut lines = Vec::new();
        let mut ranges = Vec::new();
        let fan = |tris: &mut Vec<[f32; 2]>, poly: &[[f64; 2]]| -> std::ops::Range<u32> {
            let t0 = tris.len() as u32;
            for t in triangulate(poly) {
                for k in t {
                    let p = poly[k as usize];
                    tris.push([p[0] as f32, p[1] as f32]);
                }
            }
            t0..tris.len() as u32
        };
        // An equilateral triangle of `k` times the tile's area about its vertex average.
        let dot = |tris: &mut Vec<[f32; 2]>, poly: &[[f64; 2]], k: f64| -> std::ops::Range<u32> {
            let c = jetcarp_core::geom::centroid(poly);
            let r = (k * jetcarp_core::geom::area(poly) * 4.0 / (3.0 * 3f64.sqrt())).sqrt();
            let d0 = tris.len() as u32;
            for j in 0..3 {
                let a = std::f64::consts::FRAC_PI_2 + j as f64 * 2.0 * std::f64::consts::PI / 3.0;
                tris.push([(c[0] + r * a.cos()) as f32, (c[1] + r * a.sin()) as f32]);
            }
            d0..tris.len() as u32
        };
        let mut big_reach = 0f64;
        for (s, outline) in topo.outlines.iter().enumerate() {
            let tri = fan(&mut tris, outline);
            let l0 = lines.len() as u32;
            for k in 0..outline.len() {
                let (a, b) = (outline[k], outline[(k + 1) % outline.len()]);
                lines.push([a[0] as f32, a[1] as f32]);
                lines.push([b[0] as f32, b[1] as f32]);
            }
            let straight = topo.shapes.get(s).unwrap_or(outline);
            let lo = fan(&mut tris, straight);
            let d = dot(&mut tris, straight, 1.0);
            let big = dot(&mut tris, straight, SAMPLE as f64);
            for p in &tris[big.start as usize..big.end as usize] {
                big_reach = big_reach.max((p[0] as f64).hypot(p[1] as f64));
            }
            ranges.push(ShapeDraw {
                tri,
                line: l0..lines.len() as u32,
                lo,
                dot: d,
                big,
                inst: 0..0,
            });
        }
        let n = topo.len();
        let tile = jetcarp_core::geom::area(&topo.shapes[0]).sqrt();
        if let Storage::Lattice(g) = &topo.storage {
            let types = g
                .types
                .iter()
                .map(|(m, s)| {
                    let m = m.0;
                    (
                        [
                            m[0] as f32,
                            m[1] as f32,
                            m[2] as f32,
                            0.0,
                            m[3] as f32,
                            m[4] as f32,
                            m[5] as f32,
                            0.0,
                        ],
                        *s,
                    )
                })
                .collect();
            return Geometry {
                id,
                tris,
                lines,
                shapes: ranges,
                instances: Default::default(),
                uploaded: Default::default(),
                orients: vec![],
                slot_cell: vec![],
                bins: None,
                pending: Default::default(),
                tile,
                lattice: Some(LatticeInfo {
                    a1: g.a1,
                    a2: g.a2,
                    w: g.w,
                    h: g.h,
                    ntypes: g.ntypes,
                    row_start: g.row_start.clone(),
                    types,
                }),
            };
        }
        // Bins of about 64 tiles, at most 1024 by 1024.
        let b = topo.bounds;
        let (w, h) = ((b[2] - b[0]).max(1e-9), (b[3] - b[1]).max(1e-9));
        let mut cell = (w * h * 64.0 / n.max(1) as f64).sqrt().max(1e-9);
        cell = cell.max(w / 1024.0).max(h / 1024.0);
        let cols = ((w / cell).ceil() as usize).max(1);
        let rows = ((h / cell).ceil() as usize).max(1);
        let reach = topo
            .shapes
            .iter()
            .flat_map(|s| s.iter().map(|p| p[0].hypot(p[1])))
            .chain(
                topo.outlines
                    .iter()
                    .flat_map(|s| s.iter().map(|p| p[0].hypot(p[1]))),
            )
            .fold(big_reach, f64::max);
        let nshapes = topo.outlines.len();
        let bin_of = |m: &[f64; 6]| -> usize {
            let c = (((m[2] - b[0]) / cell).floor().max(0.0) as usize).min(cols - 1);
            let r = (((m[5] - b[1]) / cell).floor().max(0.0) as usize).min(rows - 1);
            r * cols + c
        };
        // Counting sort by (shape, bin), cells in order within a bin.
        let nb = rows * cols;
        let mut count = vec![vec![0u32; nb + 1]; nshapes];
        let mut key = Vec::with_capacity(n);
        for i in 0..n {
            let m = topo.transform(i).0;
            let k = bin_of(&m);
            let s = topo.shape_of(i);
            count[s][k + 1] += 1;
            key.push((s as u32, k as u32));
        }
        let mut base = 0u32;
        for c in count.iter_mut() {
            c[0] = base;
            for k in 1..=nb {
                c[k] += c[k - 1];
            }
            base = c[nb];
        }
        let start = count.clone();
        let mut next: Vec<Vec<u32>> = count;
        let mut slot_cell = vec![0u32; n];
        for (i, &(s, k)) in key.iter().enumerate() {
            let slot = &mut next[s as usize][k as usize];
            slot_cell[*slot as usize] = i as u32;
            *slot += 1;
        }
        drop(key);
        // The sampled set: every SAMPLE-th slot of each shape, in bin order, after the full set.
        let sampled = (n >= SAMPLE_MIN).then(|| {
            let mut st = vec![vec![0u32; nb + 1]; nshapes];
            let mut extra = Vec::with_capacity(n / SAMPLE + nshapes);
            for (s, ss) in st.iter_mut().enumerate() {
                let mut j = 0usize;
                for k in 0..nb {
                    ss[k] = (n + extra.len()) as u32;
                    for slot in start[s][k]..start[s][k + 1] {
                        if j.is_multiple_of(SAMPLE) {
                            extra.push(slot_cell[slot as usize]);
                        }
                        j += 1;
                    }
                }
                ss[nb] = (n + extra.len()) as u32;
            }
            slot_cell.extend_from_slice(&extra);
            st
        });
        // Instances, one per slot; orientations deduplicated.
        let mut orients: Vec<[f32; 4]> = Vec::new();
        let mut seen: std::collections::HashMap<[u32; 4], u32> = Default::default();
        let instances: Vec<Instance> = slot_cell
            .iter()
            .map(|&i| {
                let m = topo.transform(i as usize).0;
                let o = [m[0] as f32, m[1] as f32, m[3] as f32, m[4] as f32];
                let orient = *seen.entry(o.map(f32::to_bits)).or_insert_with(|| {
                    orients.push(o);
                    orients.len() as u32 - 1
                });
                Instance {
                    t: [m[2] as f32, m[5] as f32],
                    orient,
                }
            })
            .collect();
        let shapes = ranges
            .into_iter()
            .enumerate()
            .map(|(s, mut d)| {
                d.inst = start[s][0]..start[s][nb];
                d
            })
            .collect();
        Geometry {
            id,
            tris,
            lines,
            shapes,
            instances: std::sync::Mutex::new(instances),
            uploaded: Default::default(),
            orients,
            slot_cell,
            bins: Some(Bins {
                origin: [b[0], b[1]],
                cell,
                cols,
                rows,
                reach,
                start,
                sampled,
            }),
            pending: Default::default(),
            tile,
            lattice: None,
        }
    }

    /// Explicit patches: the instances a view draws (bin rows in view, and the level of detail).
    pub fn patch_view(&self, center: [f64; 2], zoom: f64, size: [f32; 2]) -> Option<PatchView> {
        let b = self.bins.as_ref()?;
        let px = self.tile * zoom;
        let lod = match px {
            _ if px >= 6.0 => 0,
            _ if px >= 2.0 => 1,
            _ if px >= 0.5 || b.sampled.is_none() => 2,
            _ => 3,
        };
        let starts = match (&b.sampled, lod) {
            (Some(s), 3) => s,
            _ => &b.start,
        };
        let hw = size[0] as f64 / zoom / 2.0 + b.reach;
        let hh = size[1] as f64 / zoom / 2.0 + b.reach;
        let col = |x: f64| ((x - b.origin[0]) / b.cell).floor();
        let row = |y: f64| ((y - b.origin[1]) / b.cell).floor();
        let (c0, c1) = (col(center[0] - hw), col(center[0] + hw));
        let (r0, r1) = (row(center[1] - hh), row(center[1] + hh));
        let mut ranges = Vec::with_capacity(starts.len());
        for st in starts {
            let mut out: Vec<std::ops::Range<u32>> = Vec::new();
            if c1 >= 0.0 && r1 >= 0.0 && c0 < b.cols as f64 && r0 < b.rows as f64 {
                let c0 = c0.max(0.0) as usize;
                let c1 = (c1 as usize).min(b.cols - 1);
                let (r0, r1) = (r0.max(0.0) as usize, (r1 as usize).min(b.rows - 1));
                for r in r0..=r1 {
                    let (a, z) = (st[r * b.cols + c0], st[r * b.cols + c1 + 1]);
                    if a < z {
                        // Merge with the previous row when contiguous (whole rows in view).
                        match out.last_mut() {
                            Some(last) if last.end == a => last.end = z,
                            _ => out.push(a..z),
                        }
                    }
                }
            }
            ranges.push(out);
        }
        Some(PatchView { lod, ranges })
    }

    /// The cells drawn by these slots, in order.
    pub fn slot_cells(&self, slots: &[std::ops::Range<u32>]) -> Vec<u32> {
        let mut v = Vec::with_capacity(slots.iter().map(|r| r.len()).sum());
        for r in slots {
            v.extend_from_slice(&self.slot_cell[r.start as usize..r.end as usize]);
        }
        v
    }

    /// Queue colours for slots; `reset` first drops colours not yet uploaded (they are stale).
    pub fn queue_colors(&self, reset: bool, chunks: Vec<ColorChunk>) {
        let mut p = self.pending.lock().unwrap();
        if reset {
            p.clear();
        }
        p.extend(chunks);
    }
}

/// Sorted, disjoint slot ranges.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct SlotSet(Vec<std::ops::Range<u32>>);

impl SlotSet {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The parts of the sorted ranges `want` that are not in the set, joining pieces less than
    /// `gap` apart (the slots between are recoloured too, which costs less than many uploads).
    pub fn missing(&self, want: &[std::ops::Range<u32>], gap: u32) -> Vec<std::ops::Range<u32>> {
        let mut out: Vec<std::ops::Range<u32>> = Vec::new();
        let mut push = |r: std::ops::Range<u32>| {
            if r.is_empty() {
                return;
            }
            match out.last_mut() {
                Some(l) if r.start <= l.end.saturating_add(gap) => l.end = l.end.max(r.end),
                _ => out.push(r),
            }
        };
        let have = &self.0;
        let mut k = 0;
        for w in want {
            while k < have.len() && have[k].end <= w.start {
                k += 1;
            }
            let mut at = w.start;
            let mut j = k;
            while at < w.end {
                match have.get(j) {
                    Some(h) if h.start < w.end => {
                        if h.start > at {
                            push(at..h.start);
                        }
                        at = at.max(h.end);
                        j += 1;
                    }
                    _ => {
                        push(at..w.end);
                        at = w.end;
                    }
                }
            }
        }
        out
    }

    /// Add sorted ranges.
    pub fn insert(&mut self, add: &[std::ops::Range<u32>]) {
        let mut all: Vec<_> = self.0.drain(..).chain(add.iter().cloned()).collect();
        all.sort_by_key(|r| r.start);
        for r in all {
            match self.0.last_mut() {
                Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
                _ => self.0.push(r),
            }
        }
    }
}

/// A geometry on the GPU, shared by every view of it.
struct GpuGeometry {
    id: u64,
    tris: wgpu::Buffer,
    lines: wgpu::Buffer,
    instances: wgpu::Buffer,
    shapes: Vec<ShapeDraw>,
    /// Lattices: tile type transforms and row shears.
    types: Option<(wgpu::Buffer, wgpu::Buffer)>,
    /// Explicit patches: one colour per instance slot (the same in every view, so shared) and
    /// the orientation table.
    colors: Option<(wgpu::Buffer, wgpu::Buffer)>,
    /// Lattice path: shape of each tile type.
    type_shapes: Vec<usize>,
}

/// One view's own GPU state: its uniforms and, on lattices, its colour texture (one texel per
/// slot of the view's window). Views are the grid tile, each output window, and mirrors; each
/// keeps its own colours, so two views never fight over one texture.
struct ViewGpu {
    /// The geometry the bind group was made for.
    geom: u64,
    uniforms: wgpu::Buffer,
    lat_uniforms: wgpu::Buffer,
    /// Lattices: the texture and its height in rows.
    texture: Option<(wgpu::Texture, u32)>,
    bind: Option<wgpu::BindGroup>,
    /// Last use (for dropping views that are gone).
    used: u64,
}

/// Views kept at once; the least recently drawn beyond this are dropped.
const MAX_VIEWS: usize = 16;

pub struct TileRenderer {
    fill: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    lat_fill: wgpu::RenderPipeline,
    lat_line: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    lat_layout: wgpu::BindGroupLayout,
    tex_format: wgpu::TextureFormat,
    /// Most texture rows the device allows.
    max_rows: u32,
    geom: Option<GpuGeometry>,
    views: std::collections::HashMap<u64, ViewGpu>,
    tick: u64,
}

/// Colour slots a lattice view can hold on `device` (texture width times the largest height).
pub fn slot_capacity(device: &wgpu::Device) -> usize {
    TEX_W as usize * device.limits().max_texture_dimension_2d as usize
}

const SHADER: &str = r#"
struct U { center: vec2<f32>, scale: vec2<f32>, srgb: u32, line_alpha: f32, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> colors: array<u32>;
@group(0) @binding(2) var<storage, read> orients: array<vec4<f32>>;
struct VIn {
  @location(0) pos: vec2<f32>,
  @location(1) t: vec2<f32>,
  @location(2) orient: u32,
};
struct VOut { @builtin(position) p: vec4<f32>, @location(0) @interpolate(flat) c: vec4<f32> };
fn decode(x: f32) -> f32 {
  if (x <= 0.04045) { return x / 12.92; }
  return pow((x + 0.055) / 1.055, 2.4);
}
@vertex fn vs(v: VIn, @builtin(instance_index) ii: u32) -> VOut {
  let m = orients[v.orient];
  let w = (v.t - u.center) + vec2<f32>(m.x * v.pos.x + m.y * v.pos.y, m.z * v.pos.x + m.w * v.pos.y);
  var o: VOut;
  o.p = vec4<f32>(w * u.scale, 0.0, 1.0);
  var c = unpack4x8unorm(colors[ii]);
  if (u.srgb != 0u) {
    c = vec4<f32>(decode(c.r), decode(c.g), decode(c.b), c.a);
  }
  o.c = c;
  return o;
}
@fragment fn fs_fill(i: VOut) -> @location(0) vec4<f32> {
  return i.c;
}
@fragment fn fs_line(i: VOut) -> @location(0) vec4<f32> {
  return vec4<f32>(0.20, 0.22, 0.27, 1.0) * u.line_alpha;
}
"#;

const LAT_SHADER: &str = r#"
struct LU {
  scale: vec2<f32>, a1: vec2<f32>, a2: vec2<f32>, off: vec2<f32>,
  u0: i32, j0: i32, nu: u32, nj: u32,
  stride: i32, ic: i32, jc: i32, per_type: u32,
  tex_w: u32, line_alpha: f32, p0: u32, p1: u32,
};
@group(0) @binding(0) var<uniform> u: LU;
@group(0) @binding(1) var colors: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> types: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> rows: array<i32>;
struct VOut { @builtin(position) p: vec4<f32>, @location(0) @interpolate(flat) slot: u32 };
@vertex fn vs(@location(0) pos: vec2<f32>, @builtin(instance_index) ii: u32) -> VOut {
  let k = ii / u.per_type;
  let r = ii % u.per_type;
  let a = r % u.nu;
  let b = r / u.nu;
  let uu = u.u0 + i32(a) * u.stride;
  let j = u.j0 + i32(b) * u.stride;
  let i = uu + rows[j];
  let m0 = types[2u * k];
  let m1 = types[2u * k + 1u];
  let q = vec3<f32>(pos, 1.0);
  let local = vec2<f32>(dot(m0.xyz, q), dot(m1.xyz, q)) * f32(u.stride);
  let w = f32(i - u.ic) * u.a1 + f32(j - u.jc) * u.a2 + local - u.off;
  var o: VOut;
  o.p = vec4<f32>(w * u.scale, 0.0, 1.0);
  o.slot = ii;
  return o;
}
@fragment fn fs_fill(i: VOut) -> @location(0) vec4<f32> {
  return textureLoad(colors, vec2<i32>(i32(i.slot % u.tex_w), i32(i.slot / u.tex_w)), 0);
}
@fragment fn fs_line(i: VOut) -> @location(0) vec4<f32> {
  return vec4<f32>(0.20, 0.22, 0.27, 1.0) * u.line_alpha;
}
"#;

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl TileRenderer {
    /// A renderer drawing into targets of `target_format`.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> TileRenderer {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp tiles"),
            entries: &[uniform_entry(0), storage_entry(1), storage_entry(2)],
        });
        let lat_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp lattice tiles"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                storage_entry(2),
                storage_entry(3),
            ],
        });
        let pos_layout = wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let inst_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![1 => Float32x2, 2 => Uint32],
        };
        let make = |src: &str,
                    layout: &wgpu::BindGroupLayout,
                    buffers: &[Option<wgpu::VertexBufferLayout>]|
         -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("jetcarp tiles"),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("jetcarp tiles"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            let pipe = |topology, fs: &str, blend| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(fs),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        buffers,
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fs),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target_format,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            (
                pipe(wgpu::PrimitiveTopology::TriangleList, "fs_fill", None),
                pipe(
                    wgpu::PrimitiveTopology::LineList,
                    "fs_line",
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                ),
            )
        };
        let (fill, line) = make(
            SHADER,
            &layout,
            &[Some(pos_layout.clone()), Some(inst_layout)],
        );
        let (lat_fill, lat_line) = make(LAT_SHADER, &lat_layout, &[Some(pos_layout)]);
        // Colours are sRGB bytes. On an sRGB target, decode them so the output matches the PNGs.
        let tex_format = if target_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        TileRenderer {
            fill,
            line,
            lat_fill,
            lat_line,
            layout,
            lat_layout,
            tex_format,
            max_rows: device.limits().max_texture_dimension_2d,
            geom: None,
            views: Default::default(),
            tick: 0,
        }
    }

    fn upload_geometry(&mut self, device: &wgpu::Device, g: &Geometry) {
        let mk = |label, data: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if data.is_empty() { &[0; 16] } else { data },
                usage,
            })
        };
        // The instances are only needed here: take them, so the CPU copy is freed.
        let instances = g
            .uploaded
            .get_or_init(|| {
                let cpu = std::mem::take(&mut *g.instances.lock().unwrap());
                mk(
                    "jetcarp instances",
                    bytemuck::cast_slice(&cpu),
                    wgpu::BufferUsages::VERTEX,
                )
            })
            .clone();
        let (types, colors, type_shapes) = match &g.lattice {
            Some(l) => {
                let types: Vec<f32> = l.types.iter().flat_map(|t| t.0).collect();
                let tbuf = mk(
                    "jetcarp types",
                    bytemuck::cast_slice(&types),
                    wgpu::BufferUsages::STORAGE,
                );
                let rbuf = mk(
                    "jetcarp rows",
                    bytemuck::cast_slice(&l.row_start),
                    wgpu::BufferUsages::STORAGE,
                );
                (
                    Some((tbuf, rbuf)),
                    None,
                    l.types.iter().map(|t| t.1).collect(),
                )
            }
            None => {
                let colors = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("jetcarp cell colours"),
                    size: (g.slot_cell.len().max(4) * 4) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let obuf = mk(
                    "jetcarp orientations",
                    bytemuck::cast_slice(&g.orients),
                    wgpu::BufferUsages::STORAGE,
                );
                (None, Some((colors, obuf)), vec![])
            }
        };
        self.geom = Some(GpuGeometry {
            id: g.id,
            tris: mk(
                "jetcarp tris",
                bytemuck::cast_slice(&g.tris),
                wgpu::BufferUsages::VERTEX,
            ),
            lines: mk(
                "jetcarp lines",
                bytemuck::cast_slice(&g.lines),
                wgpu::BufferUsages::VERTEX,
            ),
            instances,
            shapes: g.shapes.clone(),
            types,
            colors,
            type_shapes,
        });
    }

    /// Make view `id` ready for the current geometry and (lattices) `slots` colour slots.
    /// True when its colour texture is new (the colours it held are gone).
    fn prepare_view(&mut self, device: &wgpu::Device, id: u64, slots: Option<usize>) -> bool {
        self.tick += 1;
        let tick = self.tick;
        if !self.views.contains_key(&id) && self.views.len() >= MAX_VIEWS {
            let oldest = self
                .views
                .iter()
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.views.remove(&k);
            }
        }
        let ubuf = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let v = self.views.entry(id).or_insert_with(|| ViewGpu {
            geom: 0,
            uniforms: ubuf("jetcarp uniforms", std::mem::size_of::<Uniforms>()),
            lat_uniforms: ubuf("jetcarp lattice uniforms", std::mem::size_of::<LatUniforms>()),
            texture: None,
            bind: None,
            used: 0,
        });
        v.used = tick;
        let Some(g) = &self.geom else { return false };
        let mut lost = false;
        if v.geom != g.id {
            v.geom = g.id;
            v.bind = None;
            if g.types.is_none() {
                v.texture = None;
            }
        }
        if let (Some(_), Some(n)) = (&g.types, slots) {
            let rows = (n as u32).div_ceil(TEX_W).max(1);
            let have = v.texture.as_ref().map_or(0, |t| t.1);
            if rows > have {
                let rows = rows
                    .div_ceil(ROW_STEP)
                    .saturating_mul(ROW_STEP)
                    .min(self.max_rows);
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("jetcarp cell colours"),
                    size: wgpu::Extent3d {
                        width: TEX_W,
                        height: rows,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.tex_format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                v.texture = Some((texture, rows));
                v.bind = None;
                lost = true;
            }
        }
        if v.bind.is_none() {
            v.bind = Some(match (&g.types, &g.colors, &v.texture) {
                (Some((tbuf, rbuf)), _, Some((tex, _))) => {
                    let view = tex.create_view(&Default::default());
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("jetcarp lattice tiles"),
                        layout: &self.lat_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: v.lat_uniforms.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: tbuf.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: rbuf.as_entire_binding(),
                            },
                        ],
                    })
                }
                (_, Some((colors, obuf)), _) => {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("jetcarp tiles"),
                        layout: &self.layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: v.uniforms.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: colors.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: obuf.as_entire_binding(),
                            },
                        ],
                    })
                }
                // A lattice view with no window yet: nothing to draw.
                _ => return lost,
            });
        }
        lost
    }
}

/// One frame's draw request.
#[derive(Clone)]
pub struct TileDraw {
    pub geometry: Arc<Geometry>,
    /// Lattices: new colours for the window if they changed since the last frame. (Explicit
    /// patches queue theirs on the geometry.)
    pub colors: Option<Arc<Vec<[u8; 4]>>>,
    /// Lattices: the window being drawn.
    pub window: Option<Window>,
    /// Explicit patches: the instances drawn.
    pub patch: Option<Arc<PatchView>>,
    /// World point at the centre of the rect, and points per world unit.
    pub center: [f64; 2],
    pub zoom: f64,
    /// The view's size in physical pixels (the zoom is in pixels per world unit too).
    pub rect_size: [f32; 2],
    pub line_alpha: f32,
    /// Which view this is (its uniforms and colour texture are its own).
    pub view: u64,
    /// Set by the renderer when the view's colour texture was made anew: its colours are gone
    /// and the app colours the window again at its next frame.
    pub lost: Arc<std::sync::atomic::AtomicBool>,
}

impl TileRenderer {
    /// Uploads what `d` needs: geometry when it is new, the view's colours and camera. Call once
    /// per draw, before the render pass.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, d: &TileDraw) {
        let fresh = self.geom.as_ref().map(|g| g.id) != Some(d.geometry.id);
        if fresh {
            self.upload_geometry(device, &d.geometry);
        }
        let slots = match (&d.geometry.lattice, &d.window) {
            (Some(l), Some(w)) => Some(w.slots(l.ntypes)),
            _ => None,
        };
        if self.prepare_view(device, d.view, slots) && d.colors.is_none() {
            d.lost.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let srgb = self.tex_format.is_srgb();
        let (Some(g), Some(v)) = (&self.geom, self.views.get(&d.view)) else {
            return;
        };
        let scale = [
            (2.0 * d.zoom / d.rect_size[0] as f64) as f32,
            (-2.0 * d.zoom / d.rect_size[1] as f64) as f32,
        ];
        match (&d.geometry.lattice, &d.window) {
            (Some(l), Some(w)) => {
                // Lattice point nearest the camera; positions are relative to it.
                let jc = (d.center[1] / l.a2[1]).round();
                let ic = ((d.center[0] - jc * l.a2[0]) / l.a1[0]).round();
                let off = [
                    d.center[0] - ic * l.a1[0] - jc * l.a2[0],
                    d.center[1] - ic * l.a1[1] - jc * l.a2[1],
                ];
                let u = LatUniforms {
                    scale,
                    a1: [l.a1[0] as f32, l.a1[1] as f32],
                    a2: [l.a2[0] as f32, l.a2[1] as f32],
                    off: [off[0] as f32, off[1] as f32],
                    u0: w.u0,
                    j0: w.j0,
                    nu: w.nu,
                    nj: w.nj,
                    stride: w.stride,
                    ic: ic as i32,
                    jc: jc as i32,
                    per_type: w.nu * w.nj,
                    tex_w: TEX_W,
                    line_alpha: d.line_alpha,
                    _pad: [0; 2],
                };
                queue.write_buffer(&v.lat_uniforms, 0, bytemuck::bytes_of(&u));
            }
            _ => {
                let u = Uniforms {
                    center: [d.center[0] as f32, d.center[1] as f32],
                    scale,
                    srgb: srgb as u32,
                    line_alpha: d.line_alpha,
                    _pad: [0.0; 2],
                };
                queue.write_buffer(&v.uniforms, 0, bytemuck::bytes_of(&u));
            }
        }
        if let Some((buf, _)) = &g.colors {
            // Explicit patches: upload the colours the app queued.
            let chunks = std::mem::take(&mut *d.geometry.pending.lock().unwrap());
            for (at, c) in chunks {
                queue.write_buffer(buf, at as u64 * 4, bytemuck::cast_slice(&c));
            }
        }
        if let (Some(c), Some((tex, have))) = (&d.colors, &v.texture) {
            let tw = TEX_W as usize;
            let n = c.len().min(tw * *have as usize);
            let bytes: &[u8] = bytemuck::cast_slice(&c[..n]);
            let write = |row0: usize, nrows: usize, data: &[u8]| {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: row0 as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(TEX_W * 4),
                        rows_per_image: Some(nrows as u32),
                    },
                    wgpu::Extent3d {
                        width: TEX_W,
                        height: nrows as u32,
                        depth_or_array_layers: 1,
                    },
                );
            };
            // Whole rows straight from the colours; the last, partial row padded.
            let full = n / tw;
            if full > 0 {
                write(0, full, &bytes[..full * tw * 4]);
            }
            if n % tw != 0 {
                let mut last = bytes[full * tw * 4..].to_vec();
                last.resize(tw * 4, 0);
                write(full, 1, &last);
            }
        }
    }

    /// Draws `d` into the current render pass. [`prepare`](Self::prepare) must have run for it.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>, d: &TileDraw) {
        let Some(g) = &self.geom else { return };
        let Some(bind) = self.views.get(&d.view).and_then(|v| v.bind.as_ref()) else {
            return;
        };
        pass.set_bind_group(0, bind, &[]);
        if let (Some(_), Some(w)) = (&d.geometry.lattice, &d.window) {
            let per = w.nu * w.nj;
            if per == 0 {
                return;
            }
            pass.set_pipeline(&self.lat_fill);
            pass.set_vertex_buffer(0, g.tris.slice(..));
            for (k, &s) in g.type_shapes.iter().enumerate() {
                let tri = g.shapes[s].tri.clone();
                if !tri.is_empty() {
                    let k = k as u32;
                    pass.draw(tri, k * per..(k + 1) * per);
                }
            }
            if d.line_alpha > 0.01 && w.stride == 1 {
                pass.set_pipeline(&self.lat_line);
                pass.set_vertex_buffer(0, g.lines.slice(..));
                for (k, &s) in g.type_shapes.iter().enumerate() {
                    let line = g.shapes[s].line.clone();
                    if !line.is_empty() {
                        let k = k as u32;
                        pass.draw(line, k * per..(k + 1) * per);
                    }
                }
            }
            return;
        }
        let Some(view) = &d.patch else { return };
        pass.set_pipeline(&self.fill);
        pass.set_vertex_buffer(1, g.instances.slice(..));
        pass.set_vertex_buffer(0, g.tris.slice(..));
        for (d, rs) in g.shapes.iter().zip(&view.ranges) {
            let tri = match view.lod {
                0 => d.tri.clone(),
                1 => d.lo.clone(),
                2 => d.dot.clone(),
                _ => d.big.clone(),
            };
            if tri.is_empty() {
                continue;
            }
            for inst in rs {
                pass.draw(tri.clone(), inst.clone());
            }
        }
        if d.line_alpha > 0.01 && view.lod == 0 {
            pass.set_pipeline(&self.line);
            pass.set_vertex_buffer(0, g.lines.slice(..));
            for (d, rs) in g.shapes.iter().zip(&view.ranges) {
                if d.line.is_empty() {
                    continue;
                }
                for inst in rs {
                    pass.draw(d.line.clone(), inst.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::{SizeSpec, TilingSpec};

    // Explicit patches: the binned instances hold every cell once, each in the bin of its
    // position, with its transform; every corner lies within `reach` of that position (so the bin
    // rows chosen for a view, widened by `reach`, hold every tile reaching into it). The sampled
    // set holds every 16th slot of each shape, in the same bins.
    #[test]
    fn explicit_bins_cover_every_cell_once() {
        for (fam, level) in [("spectre", 4), ("penrose-kd", 3), ("spectre", 6)] {
            let t =
                Topology::build(&TilingSpec::new(fam).with_size(SizeSpec::Level(level))).unwrap();
            let g = Geometry::build(&t, 1);
            let b = g.bins.as_ref().unwrap();
            let inst = g.instances.lock().unwrap();
            assert_eq!(inst.len(), g.slot_cell.len());
            let bin = |i: usize| {
                let m = t.transform(i).0;
                let c = ((m[2] - b.origin[0]) / b.cell).floor().max(0.0) as usize;
                let r = ((m[5] - b.origin[1]) / b.cell).floor().max(0.0) as usize;
                r.min(b.rows - 1) * b.cols + c.min(b.cols - 1)
            };
            let mut seen = vec![false; t.len()];
            for (s, d) in g.shapes.iter().enumerate() {
                let st = &b.start[s];
                assert_eq!(st[0], d.inst.start);
                assert_eq!(*st.last().unwrap(), d.inst.end);
                for k in 0..b.rows * b.cols {
                    for i in st[k]..st[k + 1] {
                        let cell = g.slot_cell[i as usize] as usize;
                        assert!(!seen[cell]);
                        seen[cell] = true;
                        assert_eq!(t.shape_of(cell), s);
                        assert_eq!(bin(cell), k);
                        let m = t.transform(cell).0;
                        let x = inst[i as usize];
                        assert_eq!(x.t, [m[2] as f32, m[5] as f32]);
                        let o = g.orients[x.orient as usize];
                        assert_eq!(o, [m[0] as f32, m[1] as f32, m[3] as f32, m[4] as f32]);
                    }
                }
            }
            assert!(seen.iter().all(|&x| x));
            assert!(g.orients.len() <= 100, "{} orientations", g.orients.len());
            for i in 0..t.len() {
                let m = t.transform(i);
                for p in t.polygon(i) {
                    let d = (p[0] - m.0[2]).hypot(p[1] - m.0[5]);
                    assert!(d <= b.reach + 1e-9);
                }
            }
            match &b.sampled {
                None => assert!(t.len() < SAMPLE_MIN),
                Some(ss) => {
                    let n = t.len();
                    assert_eq!(ss[0][0] as usize, n);
                    assert_eq!(
                        *ss.last().unwrap().last().unwrap() as usize,
                        g.slot_cell.len()
                    );
                    let extra = g.slot_cell.len() - n;
                    assert!(extra.abs_diff(n / SAMPLE) <= g.shapes.len());
                    for (s, st) in ss.iter().enumerate() {
                        for k in 0..b.rows * b.cols {
                            for i in st[k]..st[k + 1] {
                                let cell = g.slot_cell[i as usize] as usize;
                                assert_eq!(t.shape_of(cell), s);
                                assert_eq!(bin(cell), k);
                            }
                        }
                    }
                }
            }
        }
    }

    // Views: zoomed out far enough, a large patch draws its sampled set; every view's ranges lie
    // in the set it draws.
    #[test]
    fn patch_views_pick_level_of_detail() {
        let t = Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(6))).unwrap();
        let g = Geometry::build(&t, 1);
        let b = t.bounds;
        let c = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        let size = [1000.0, 800.0];
        let n = t.len() as u32;
        for (zoom, lod) in [(0.05, 3), (0.3, 2), (1.0, 1), (10.0, 0)] {
            let v = g.patch_view(c, zoom, size).unwrap();
            assert_eq!(v.lod, lod, "zoom {zoom}");
            let slots = v.slots();
            assert!(!slots.is_empty());
            for r in &slots {
                if lod == 3 {
                    assert!(r.start >= n && r.end as usize <= g.slot_cell.len());
                } else {
                    assert!(r.end <= n);
                }
            }
        }
        let whole: usize = g
            .patch_view(c, 0.3, size)
            .unwrap()
            .slots()
            .iter()
            .map(|r| r.len())
            .sum();
        assert_eq!(whole, t.len());
        let near: usize = g
            .patch_view(c, 10.0, size)
            .unwrap()
            .slots()
            .iter()
            .map(|r| r.len())
            .sum();
        assert!(near < t.len() / 20, "{near} tiles in a zoomed view");
    }

    #[test]
    fn slot_sets_track_what_is_current() {
        let mut s = SlotSet::default();
        assert_eq!(s.missing(&[0..10, 20..30], 0), vec![0..10, 20..30]);
        assert_eq!(s.missing(&[0..10, 20..30], 10), vec![0..30]);
        s.insert(&[5..8, 22..25]);
        assert_eq!(
            s.missing(&[0..10, 20..30], 0),
            vec![0..5, 8..10, 20..22, 25..30]
        );
        s.insert(&[0..5, 8..10, 20..22, 25..30]);
        assert_eq!(s.0, vec![0..10, 20..30]);
        assert!(s.missing(&[0..10, 20..30], 0).is_empty());
        assert_eq!(
            s.missing(std::slice::from_ref(&(0..40)), 0),
            vec![10..20, 30..40]
        );
        s.insert(std::slice::from_ref(&(10..20)));
        assert_eq!(s.0, vec![0..30]);
        s.clear();
        assert_eq!(
            s.missing(std::slice::from_ref(&(3..4)), 0),
            std::slice::from_ref(&(3..4))
        );
    }
}
