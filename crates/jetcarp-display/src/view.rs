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
type ColorCache = (Window, u64, Arc<Vec<[u8; 4]>>);

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
                    Some((cw, ce, c)) if *cw == w && *ce == epoch => c.clone(),
                    _ => {
                        let c = Arc::new(render::colors_for(world, &w.cells(g)));
                        colored = c.len();
                        self.color_cache = Some((w, epoch, c.clone()));
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
            let reset = self.patch_epoch != epoch;
            if reset {
                self.patch_epoch = epoch;
                self.patch_valid.clear();
            }
            let missing = self.patch_valid.missing(&v.slots(), 4096);
            let all = render::colors_for(world, &geom.slot_cells(&missing));
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
        let line_alpha = if outlines {
            let tile_px = cell_unit(world) * zoom;
            ((tile_px - 6.0) / 10.0).clamp(0.0, 1.0) as f32
        } else {
            0.0
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
