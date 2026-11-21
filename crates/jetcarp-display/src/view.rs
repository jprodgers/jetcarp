//! What a view shows this frame: the cells in view at its camera and size, coloured only where the
//! GPU's copy is not current. The result is a [`TileDraw`] for the renderer.

use crate::gpu::{Geometry, SlotSet, TileDraw, Window, window_budget};
use jetcarp_core::{World, geom, render};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One view's colouring state on the CPU side (the GPU side is in the renderer).
#[derive(Default)]
struct ViewCpu {
    last_window: Option<Window>,
    epoch: u64,
    lost: Arc<AtomicBool>,
}

/// Lattice colours of one window at one colour epoch.
/// A window's colours, the epoch they are for, and whether faint outlines are marked in them.
type ColorCache = (Window, u64, bool, Arc<Vec<[u8; 4]>>);

/// The tile shaders' outline colour, as bytes on a plain target.
const LINE: [u8; 4] = [51, 56, 69, 255];

/// Marks, in the colours of `cells`, the cells whose outlines the shaders draw faint (alpha 0):
/// where outlines would form strong stripes, as the processor's pictures draw them
/// ([`render::fade_striped_outlines`]): cells of an even bright field
/// ([`render::outline_faint`]), when such cells cover more than a quarter of the view (`view_px`
/// pixels, at `zoom` pixels per world unit) and the tiling's outlines, drawn at `line_alpha`, make
/// stripes at this size ([`render::outlines_make_stripes`]). The fill shaders draw every cell
/// opaque whatever its alpha.
fn mark_faint(
    world: &World,
    cells: &[u32],
    colors: &mut [[u8; 4]],
    zoom: f64,
    view_px: f64,
    line_alpha: f32,
) {
    let faint = render::outline_faint_cells(world, cells, LINE);
    let cell_px = cell_unit(world) * zoom;
    let share = faint.iter().filter(|f| **f).count() as f64 * cell_px * cell_px / view_px.max(1.0);
    // Both cells beside an edge draw it: the line over white, as a plain target blends it.
    let both = 1.0 - (1.0 - line_alpha) * (1.0 - line_alpha);
    let mix = |l: u8| (255.0 + (l as f32 - 255.0) * both).round() as u8;
    let line = [mix(LINE[0]), mix(LINE[1]), mix(LINE[2]), 255];
    let on = share > jetcarp_core::strobe::AREA_SHARE
        && render::outlines_make_stripes(world, zoom, line);
    for (c, f) in colors.iter_mut().zip(faint) {
        c[3] = if on && f { 0 } else { 255 };
    }
}

/// One view of a world for this frame.
#[derive(Clone, Copy, Debug)]
pub struct ViewRequest {
    /// Which view this is. Each has its own colour state on the GPU.
    pub view: u64,
    /// The world point at the view's centre.
    pub center: [f64; 2],
    /// Physical pixels per world unit.
    pub zoom: f64,
    /// The view's size in physical pixels.
    pub size: [f32; 2],
    /// Draw cell outlines when the cells are large enough.
    pub outlines: bool,
}

/// What colouring a view cost this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ColorWork {
    pub cells: usize,
    pub secs: f64,
}

/// Colouring state of every view of one world.
pub struct Views {
    views: HashMap<u64, ViewCpu>,
    color_cache: Option<ColorCache>,
    slot_cap: usize,
    patch_valid: SlotSet,
    patch_epoch: u64,
    /// Whether the patch colours on the GPU have faint outlines marked.
    patch_faint: bool,
}

impl Views {
    /// `slot_cap` is [`crate::gpu::slot_capacity`] for the device that will draw.
    pub fn new(slot_cap: usize) -> Views {
        Views {
            views: Default::default(),
            color_cache: None,
            slot_cap,
            patch_valid: Default::default(),
            patch_epoch: 0,
            patch_faint: false,
        }
    }

    /// This frame's drawing for one view: colours the cells in view that are not current on the
    /// GPU for this view. `epoch` must change whenever any cell's colour may have changed.
    pub fn draw(
        &mut self,
        world: &World,
        geom: &Arc<Geometry>,
        epoch: u64,
        req: ViewRequest,
    ) -> (TileDraw, ColorWork) {
        let ViewRequest {
            view,
            center,
            zoom,
            size,
            outlines,
        } = req;
        let line_alpha = if outlines {
            let tile_px = cell_unit(world) * zoom;
            ((tile_px - 6.0) / 10.0).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        // Outlines are drawn: mark where they are drawn faint.
        let faint = line_alpha > 0.01;
        let px = size[0] as f64 * size[1] as f64;
        let budget = window_budget(px, self.slot_cap);
        // Lattices: colour only the window of cells in view (sampled when huge).
        let window = geom
            .lattice
            .as_ref()
            .and_then(|l| l.window(center, zoom, size, budget, self.slot_cap));
        // Explicit patches: the instances in view.
        let patch = geom.patch_view(center, zoom, size).map(Arc::new);
        let t0 = std::time::Instant::now();
        let mut colored = 0;
        let mut colors = None;
        let mut st = self.views.remove(&view).unwrap_or_default();
        let lost = st.lost.swap(false, Ordering::SeqCst);
        if let (Some(w), Some(g)) = (window, world.topo.lattice()) {
            if lost || st.epoch != epoch || st.last_window != Some(w) {
                // Views with the same window (mirrors of one size) share the colours.
                let c = match &self.color_cache {
                    Some((cw, ce, cf, c)) if *cw == w && *ce == epoch && *cf == faint => c.clone(),
                    _ => {
                        let cells = w.cells(g);
                        let mut v = render::colors_for(world, &cells);
                        if faint {
                            mark_faint(world, &cells, &mut v, zoom, px, line_alpha);
                        }
                        let c = Arc::new(v);
                        colored = c.len();
                        self.color_cache = Some((w, epoch, faint, c.clone()));
                        c
                    }
                };
                colors = Some(c);
                st.epoch = epoch;
            }
        } else if let Some(v) = &patch {
            // Colours of explicit patches are per instance slot, shared by every view: colour
            // the slots in view that are not current (all after a change, the ones that came
            // into view after a pan, none otherwise).
            let reset = self.patch_epoch != epoch || self.patch_faint != faint;
            if reset {
                self.patch_epoch = epoch;
                self.patch_faint = faint;
                self.patch_valid.clear();
            }
            let missing = self.patch_valid.missing(&v.slots(), 4096);
            let cells = geom.slot_cells(&missing);
            let mut all = render::colors_for(world, &cells);
            if faint {
                mark_faint(world, &cells, &mut all, zoom, px, line_alpha);
            }
            colored = all.len();
            let mut rest = all.as_slice();
            let chunks = missing
                .iter()
                .map(|r| {
                    let (a, b) = rest.split_at(r.len());
                    rest = b;
                    (r.start, a.to_vec())
                })
                .collect();
            geom.queue_colors(reset, chunks);
            self.patch_valid.insert(&missing);
        }
        st.last_window = window;
        let lost_flag = st.lost.clone();
        self.views.insert(view, st);
        let work = ColorWork {
            cells: colored,
            secs: t0.elapsed().as_secs_f64(),
        };
        let draw = TileDraw {
            geometry: geom.clone(),
            colors,
            window,
            patch,
            center,
            zoom,
            rect_size: size,
            line_alpha,
            view,
            lost: lost_flag,
        };
        (draw, work)
    }
}

/// World units of one cell's edge (the square root of the average tile area).
pub fn cell_unit(world: &World) -> f64 {
    match world.topo.lattice() {
        Some(g) => {
            let a = (g.a1[0] * g.a2[1] - g.a1[1] * g.a2[0]).abs() / g.ntypes.max(1) as f64;
            a.sqrt()
        }
        None => geom::area(&world.topo.shapes[0]).sqrt(),
    }
}

/// Where to look (centre, pixels per world unit) so a cell is `px` pixels on a canvas of `canvas`
/// pixels: centred, with cell corners on pixel corners. A world smaller than the canvas at that
/// size is fitted instead.
pub fn place(world: &World, canvas: [f64; 2], px: f64) -> ([f64; 2], f64) {
    let b = world.topo.bounds;
    let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let zoom = (px.max(0.05) / cell_unit(world).max(1e-9)).clamp(1e-3, 1e4);
    let (w, h) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
    let fit = (canvas[0] / w).min(canvas[1] / h);
    if fit > zoom {
        return (center, fit);
    }
    let origin = world.topo.polygon(0).first().copied().unwrap_or([0.0, 0.0]);
    let snap = |c: f64, half: f64, o: f64| o + (((c - o) * zoom - half).round() + half) / zoom;
    (
        [
            snap(center[0], canvas[0] / 2.0, origin[0]),
            snap(center[1], canvas[1] / 2.0, origin[1]),
        ],
        zoom,
    )
}
