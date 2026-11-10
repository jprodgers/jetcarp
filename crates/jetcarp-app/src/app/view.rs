//! The grid view: camera, running generations, drawing tiles, mouse tools.
//!
//! The camera is the canvas camera (`cam`, the world point at the canvas centre, and `zoom`, in
//! canvas units, normally physical pixels, per world unit). The canvas is the grid tile in the
//! main window, the full-screen window, or the outputs' span (`display.rs`). The grid tile maps
//! the world through a [`TileXf`]: the camera itself, or a fitted preview of the outputs.

use super::*;
use jetcarp_display::view::ViewRequest;

/// How the main window's grid tile shows the world this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct TileXf {
    pub rect: egui::Rect,
    /// Physical pixels per point.
    pub ppp: f32,
    pub center: [f64; 2],
    /// Physical pixels per world unit in this tile.
    pub zoom_px: f64,
}

impl TileXf {
    pub fn to_world(self, p: egui::Pos2) -> [f64; 2] {
        let c = self.rect.center();
        let k = self.ppp as f64 / self.zoom_px;
        [
            self.center[0] + (p.x - c.x) as f64 * k,
            self.center[1] + (p.y - c.y) as f64 * k,
        ]
    }
    pub fn to_screen(self, w: [f64; 2]) -> egui::Pos2 {
        let c = self.rect.center();
        let k = self.zoom_px / self.ppp as f64;
        egui::pos2(
            c.x + ((w[0] - self.center[0]) * k) as f32,
            c.y + ((w[1] - self.center[1]) * k) as f32,
        )
    }
    pub fn size_px(self) -> egui::Vec2 {
        self.rect.size() * self.ppp
    }
}

/// View ids: the grid tile, the grid mirror experiment, and outputs from `OUTPUT_VIEW0` up.
pub(super) const TILE_VIEW: u64 = 0;
pub(super) const MIRROR_VIEW: u64 = 1;
pub(super) const OUTPUT_VIEW0: u64 = 16;

impl JetcarpApp {
    /// Zoom about the canvas centre.
    pub(super) fn zoom_by(&mut self, f: f64) {
        self.zoom = (self.zoom * f).clamp(1e-3, 1e4);
    }

    /// Physical pixels per point of the main window.
    pub(super) fn ppp(&self) -> f32 {
        self.ctx.as_ref().map_or(1.0, |c| c.pixels_per_point())
    }

    /// Fit the world into the canvas. After *Fill*, the zoom is pixel-exact instead: the tiling
    /// was built to cover the canvas at the cell size.
    pub(super) fn fit_view(&mut self) {
        let b = self.world.topo.bounds;
        self.cam = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        let filled = self.t_size == SizeChoice::Fill && self.world.topo.lattice().is_some();
        if filled {
            self.pixel_zoom(self.shell.display.cell_px as f64);
            return;
        }
        let (w, h) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
        let c = self.canvas_px();
        self.zoom = ((c[0] / w).min(c[1] / h) * 0.98).max(1e-3);
    }

    /// World units of one cell's edge (the square root of the average tile area).
    pub(super) fn cell_unit(&self) -> f64 {
        jetcarp_display::view::cell_unit(&self.world)
    }

    /// Pixel-exact zoom: one cell edge is `px` canvas pixels, the camera snapped so that cell
    /// edges fall on pixel edges (square cells at whole pixels then map one to one).
    pub(super) fn pixel_zoom(&mut self, px: f64) {
        self.zoom = (px.max(0.05) / self.cell_unit().max(1e-9)).clamp(1e-3, 1e4);
        self.snap_camera();
    }

    /// Snap the camera so lattice corners land on pixel corners of the canvas.
    pub(super) fn snap_camera(&mut self) {
        let origin = self
            .world
            .topo
            .polygon(0)
            .first()
            .copied()
            .unwrap_or([0.0, 0.0]);
        let cc = self.canvas_center_px();
        let cam = crate::span::Camera {
            center: self.cam,
            zoom: self.zoom,
        };
        self.cam = crate::span::snap_center(cam, cc, origin);
    }

    // ---------------------------------------------------------------- running

    pub(super) fn run_steps(&mut self, now: f64) {
        let dt = (now - self.last_time).clamp(0.0, 0.25);
        self.last_time = now;
        if !self.playing || self.world.cas.is_empty() {
            self.acc = 0.0;
            return;
        }
        let t0 = std::time::Instant::now();
        let deadline = t0 + std::time::Duration::from_millis(14);
        let want = if self.max_speed {
            u64::MAX
        } else {
            self.acc_after(dt)
        };
        if want > 0 {
            let done = self.sim.step(&mut self.world, want, deadline);
            if done > 0 {
                self.colors_dirty = true;
                self.acc = (self.acc - done as f64).max(0.0);
            }
        }
        if self.max_speed {
            while t0.elapsed().as_millis() < 14 {
                self.exec(Command::Step { gens: 1 });
            }
            return;
        }
        while self.acc >= 1.0 && t0.elapsed().as_millis() < 14 {
            self.exec(Command::Step { gens: 1 });
            self.acc -= 1.0;
        }
    }

    /// Add this frame's time to the generation accumulator; returns whole generations due.
    pub(super) fn acc_after(&mut self, dt: f64) -> u64 {
        self.acc = (self.acc + dt * self.speed as f64).min(self.speed as f64 * 0.25 + 1.0);
        self.acc.floor() as u64
    }

    /// The grid tile's mapping for `rect`: the camera itself, or (outputs on) a preview of the
    /// whole output canvas fitted into the tile.
    pub(super) fn tile_xf(&self, rect: egui::Rect, ppp: f32) -> TileXf {
        let mut zoom_px = self.zoom;
        if let Some(c) = self.output_canvas() {
            let s = rect.size() * ppp;
            let k = (s.x as f64 / c[0].max(1.0)).min(s.y as f64 / c[1].max(1.0));
            zoom_px = self.zoom * k;
        }
        TileXf {
            rect,
            ppp,
            center: self.cam,
            zoom_px,
        }
    }

    /// The grid view, filling `ui` (a tile in the docked layout, or the whole window).
    pub(super) fn grid_view(&mut self, ui: &mut egui::Ui) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, self.stage_color());
        let ppp = ui.ctx().pixels_per_point();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.view_size = rect.size();
        if self.fit_pending && rect.width() > 10.0 && self.canvas_ready() {
            self.fit_view();
            self.zoom *= self.zoom_mul;
            self.fit_pending = false;
        }
        if self.outputs_active() && self.shell.display.hide_preview {
            self.preview_placeholder(ui, rect);
            self.message_overlay(ui, rect);
            return;
        }
        let xf = self.tile_xf(rect, ppp);
        // Zoom about the cursor.
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
            if scroll != 0.0
                && let Some(p) = resp.hover_pos()
            {
                self.zoom_at(xf.to_world(p), (scroll as f64 * 0.0025).exp());
            }
        }
        let pan = resp.dragged_by(egui::PointerButton::Secondary)
            || resp.dragged_by(egui::PointerButton::Middle)
            || (self.tool == Tool::Pan && resp.dragged_by(egui::PointerButton::Primary));
        if pan {
            let d = resp.drag_delta() * ppp;
            self.pan_px([d.x as f64, d.y as f64], xf.zoom_px);
        }
        let xf = self.tile_xf(rect, ppp);
        // A library pattern waiting to be pasted: a click places it.
        if self.paste.is_some() {
            if let Some(p) = resp.hover_pos() {
                let label = self
                    .paste
                    .as_ref()
                    .map(|x| x.label.clone())
                    .unwrap_or_default();
                ui.painter().text(
                    p + egui::vec2(14.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    format!("paste {label} (Esc stops)"),
                    egui::FontId::proportional(13.0),
                    Color32::from_rgb(0xff, 0xd0, 0x80),
                );
            }
            if resp.clicked_by(egui::PointerButton::Primary)
                && let Some(p) = resp.interact_pointer_pos()
                && let Some(cell) = self.world.topo.locate(xf.to_world(p))
            {
                self.paste_at(cell);
            }
        }
        // Paint.
        let painting = self.paste.is_none()
            && self.tool != Tool::Pan
            && (resp.dragged_by(egui::PointerButton::Primary)
                || resp.clicked_by(egui::PointerButton::Primary));
        if painting {
            if let (Some(p), Some(id)) = (resp.interact_pointer_pos(), self.draw_target) {
                let wp = xf.to_world(p);
                if let Some(cell) = self.world.topo.locate(wp)
                    && (self.last_painted != Some(cell) || resp.clicked())
                {
                    let v = if self.tool == Tool::Erase {
                        0
                    } else {
                        self.draw_value
                    };
                    self.exec(Command::SetCells {
                        id,
                        cells: vec![[cell as u32, v]],
                    });
                    self.last_painted = Some(cell);
                }
            }
        } else {
            self.last_painted = None;
        }
        if self.gpu {
            let size = xf.size_px();
            let cb = self.tile_callback(TILE_VIEW, xf.center, xf.zoom_px, size);
            ui.painter()
                .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
            self.outputs_overlay(ui, &xf);
            self.objects_overlay(ui, rect);
            self.build_overlay(ui, rect);
            self.message_overlay(ui, rect);
            if !self.outputs_active()
                && let Some(c) = self.saver_caption(ui.input(|i| i.time))
            {
                screensaver::paint_caption(ui.painter(), rect, &c);
            }
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "GPU renderer unavailable",
                egui::FontId::proportional(18.0),
                Color32::LIGHT_RED,
            );
        }
        if let Some(p) = resp.hover_pos().filter(|_| !self.saver_active()) {
            let wp = xf.to_world(p);
            if let Some(cell) = self.world.topo.locate(wp) {
                let mut tip = format!("cell {cell}");
                for (i, f) in self.world.fields.iter().enumerate() {
                    let v = if f.kind == FieldKind::Height {
                        f.height[cell]
                    } else {
                        f.state[cell] as u32
                    };
                    tip.push_str(&format!(" · field {i}: {v}"));
                }
                ui.painter().text(
                    rect.left_bottom() + egui::vec2(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    tip,
                    egui::FontId::monospace(12.0),
                    Color32::from_gray(170),
                );
            }
        }
    }

    /// Pan the canvas by a drag of `d` pixels in a view of `zoom_px` pixels per world unit.
    pub(super) fn pan_px(&mut self, d: [f64; 2], zoom_px: f64) {
        self.cam[0] -= d[0] / zoom_px;
        self.cam[1] -= d[1] / zoom_px;
    }

    /// Zoom the canvas by `f`, keeping world point `about` in place on every view.
    pub(super) fn zoom_at(&mut self, about: [f64; 2], f: f64) {
        let f = (self.zoom * f).clamp(1e-3, 1e4) / self.zoom;
        let c = crate::span::zoom_about(
            crate::span::Camera {
                center: self.cam,
                zoom: self.zoom,
            },
            f,
            about,
        );
        self.cam = c.center;
        self.zoom = c.zoom;
    }

    /// The colours changed (a generation, an edit, a palette): every view colours again.
    fn take_color_change(&mut self) {
        if std::mem::take(&mut self.colors_dirty) {
            self.color_epoch += 1;
        }
    }

    /// This frame's tile drawing for view `view`, `size` physical pixels at `zoom` pixels per
    /// world unit about `center`: colours the cells in view that are not current on the GPU for
    /// this view. Used by the grid tile, the grid mirror, and the outputs.
    pub(super) fn tile_callback(
        &mut self,
        view: u64,
        center: [f64; 2],
        zoom: f64,
        size: egui::Vec2,
    ) -> TileCallback {
        self.take_color_change();
        let req = ViewRequest {
            view,
            center,
            zoom,
            size: [size.x, size.y],
            outlines: self.shell.cell_outlines,
        };
        let (draw, work) = self
            .views
            .draw(&self.world, &self.geom, self.color_epoch, req);
        if work.cells > 0
            && let Some(f) = &mut self.frametime
        {
            f.add_color(work.secs * 1e3, work.cells);
        }
        TileCallback(draw)
    }

    /// `JETCARP_GRID_POPOUT=1|deferred`: the grid, mirrored in a second OS window (wave 1's
    /// experiment, kept as a check of the paint callback in an immediate viewport).
    pub(super) fn grid_mirror(&mut self, ctx: &egui::Context) {
        if !self.gpu {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(crate::popout::os_title("Grid mirror"))
            .with_app_id("jetcarp")
            .with_inner_size([640.0, 420.0]);
        let (cam, zoom) = (self.cam, self.zoom);
        let stage = self.stage_color();
        match self.shell.grid_mirror {
            shell::Mirror::Off => {}
            shell::Mirror::Immediate => {
                let (_, closed, _) =
                    crate::popout::os_window(ctx, "grid-mirror", builder, |ui, _class| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::new().fill(stage))
                            .show(ui, |ui| {
                                let rect = ui.max_rect();
                                let size = rect.size() * ui.ctx().pixels_per_point();
                                let cb = self.tile_callback(MIRROR_VIEW, cam, zoom, size);
                                ui.painter()
                                    .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
                            });
                    });
                if closed {
                    self.shell.grid_mirror = shell::Mirror::Off;
                }
            }
            shell::Mirror::Deferred => {
                let slot = self.shell.mirror_slot.clone();
                let size = slot.lock().unwrap().1;
                let cb = self.tile_callback(MIRROR_VIEW, cam, zoom, size);
                slot.lock().unwrap().0 = Some(cb);
                let vid = crate::popout::viewport_id("grid-mirror");
                ctx.show_viewport_deferred(vid, builder, move |ui, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(stage))
                        .show(ui, |ui| {
                            let rect = ui.max_rect();
                            let mut g = slot.lock().unwrap();
                            g.1 = rect.size() * ui.ctx().pixels_per_point();
                            if let Some(cb) = g.0.clone() {
                                ui.painter()
                                    .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
                            }
                        });
                });
                ctx.request_repaint_of(vid);
            }
        }
    }
}

impl JetcarpApp {
    /// The colour behind the grid (outside the tiling): the backdrop palette's stage colour.
    pub(super) fn stage_color(&self) -> Color32 {
        let (_, s) = jetcarp_core::render::backdrop(&self.world);
        Color32::from_rgb(s[0], s[1], s[2])
    }
}
