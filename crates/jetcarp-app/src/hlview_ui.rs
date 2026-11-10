//! The *HashLife view* window: the current world's pattern on HashLife's unbounded plane
//! (`jetcarp_core::hashlife::Plane`), jumped any number of generations and drawn at any zoom.
//!
//! - **Drawing from the quadtree.** Zoomed out, the view asks the plane for its nodes at the
//!   level where one node is about one screen pixel (`Plane::blocks_in`), so the work follows
//!   the pixels on screen, not the pattern's size: a Gosper gun after 2^40 generations (10^11
//!   cells) draws as fast as after 100. The blocks become a texture in lattice coordinates,
//!   drawn as one parallelogram (sheared lattices such as hexagons included). Close up, each
//!   tile in view is drawn as its own polygon in the tiling's real shape.
//! - **Jumps on a worker thread.** The window keeps showing the last frame while a jump runs.
//! - Drag to pan, scroll to zoom about the pointer; *Fit* frames the whole pattern.
//! - *Colours*: the world's palette (that of its first CA, as loaded) or any palette; its
//!   background fills empty space, its stops colour states and densities.
//! - `JETCARP_HLVIEW=<gens>[:<pixels per cell>[:<i>,<j>]]` (headless checks) opens the window on
//!   the loaded scene, jumps synchronously, and frames the pattern (or the given centre).

use jetcarp_core::World;
use jetcarp_core::geom::{Affine, P};
use jetcarp_core::hashlife::{self, HlColors, Options, Plane};
use jetcarp_core::render::Palette;
use jetcarp_core::topology::LatticeGrid;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Vec2};
use std::sync::mpsc;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "hashlife";

type JumpResult = (Plane, Result<f64, String>);
/// A lattice box `(i0, j0, w, h)`.
type Region = (i64, i64, u64, u64);

pub struct HlView {
    pub open: bool,
    plane: Option<Plane>,
    worker: Option<mpsc::Receiver<JumpResult>>,
    /// Lattice geometry: basis vectors, per-type transforms, outlines.
    lattice: Option<LatticeGrid>,
    shapes: Vec<Vec<P>>,
    /// View centre in world coordinates and pixels per world unit.
    centre: [f64; 2],
    scale: f64,
    gens: String,
    status: String,
    error: Option<String>,
    tex: Option<TextureHandle>,
    /// What the texture shows: (generation, region, level) it was made for.
    tex_key: Option<(u64, Region, u8)>,
    fit_pending: bool,
    wire: bool,
    /// Palette override (`None`: the world's; saved in the settings).
    pub palette: Option<String>,
    /// The loaded world's backdrop palette and its own palettes (offered in the picker).
    world_palette: Palette,
    extra: Vec<Palette>,
    colors: HlColors,
    outline: Color32,
}

impl Default for HlView {
    fn default() -> Self {
        HlView {
            open: false,
            plane: None,
            worker: None,
            lattice: None,
            shapes: vec![],
            centre: [0.0, 0.0],
            scale: 8.0,
            gens: "2^10".into(),
            status: String::new(),
            error: None,
            tex: None,
            tex_key: None,
            fit_pending: false,
            wire: false,
            palette: None,
            world_palette: Palette::builtin("Ember").unwrap(),
            extra: vec![],
            colors: HlColors::default(),
            outline: Color32::from_gray(18),
        }
    }
}

/// Visible tiles drawn one by one at most (beyond, the view rasterises).
const MAX_POLYS: u64 = 120_000;

impl HlView {
    /// Load a world's pattern onto the plane and open the window.
    pub fn load(&mut self, world: &World) -> Result<(), String> {
        let p = Plane::from_world(world, &Options::default())?;
        let g = world
            .topo
            .lattice()
            .ok_or("HashLife needs a lattice tiling")?
            .clone();
        self.lattice = Some(g);
        self.shapes = world.topo.outlines.clone();
        self.world_palette = jetcarp_core::render::backdrop_palette(world);
        self.extra = world.palettes.clone();
        self.colors_changed();
        self.wire = p.ca().rule.trim().eq_ignore_ascii_case("wireworld");
        self.plane = Some(p);
        self.worker = None;
        self.open = true;
        self.error = None;
        self.tex_key = None;
        self.fit_pending = true;
        self.status = "Loaded from the world.".into();
        Ok(())
    }

    /// `JETCARP_HLVIEW=<gens>[:<ppc>[:<i>,<j>]]`.
    pub fn open_env(&mut self, world: &World, s: &str) {
        let mut parts = s.split(':');
        let gens = parts.next().unwrap_or("0");
        if let Err(e) = self.load(world) {
            self.error = Some(e);
            self.open = true;
            return;
        }
        self.gens = gens.to_string();
        if let (Ok(n), Some(p)) = (hashlife::parse_gens(gens), self.plane.as_mut()) {
            let t = std::time::Instant::now();
            match p.step(n) {
                Ok(()) => self.status = jump_status(p, n, t.elapsed().as_secs_f64()),
                Err(e) => self.error = Some(e),
            }
        }
        self.fit();
        if let Some(ppc) = parts.next().and_then(|v| v.parse::<f64>().ok()) {
            self.scale = ppc / self.cell_size();
            self.fit_pending = false;
        }
        if let Some((i, j)) = parts.next().and_then(|v| v.split_once(','))
            && let (Ok(i), Ok(j)) = (i.parse::<f64>(), j.parse::<f64>())
        {
            self.centre = self.to_world(i, j);
        }
    }

    /// The palette drawn with: the chosen one, else the world's.
    fn effective_palette(&self) -> Palette {
        self.palette
            .as_deref()
            .and_then(|n| {
                self.extra
                    .iter()
                    .find(|p| p.name == n)
                    .cloned()
                    .or_else(|| Palette::builtin(n))
            })
            .unwrap_or_else(|| self.world_palette.clone())
    }

    /// Recompute the colours and redraw.
    fn colors_changed(&mut self) {
        let p = self.effective_palette();
        self.colors = HlColors::new(&p);
        let o = p.outline_rgba();
        self.outline = Color32::from_rgb(o[0], o[1], o[2]);
        self.tex_key = None;
    }

    fn cell_size(&self) -> f64 {
        self.lattice
            .as_ref()
            .map(|g| (g.a1[0] * g.a2[1] - g.a1[1] * g.a2[0]).abs().sqrt())
            .unwrap_or(1.0)
    }

    fn to_world(&self, i: f64, j: f64) -> P {
        let g = self.lattice.as_ref().unwrap();
        [i * g.a1[0] + j * g.a2[0], i * g.a1[1] + j * g.a2[1]]
    }

    fn to_lattice(&self, p: P) -> (f64, f64) {
        let g = self.lattice.as_ref().unwrap();
        let det = g.a1[0] * g.a2[1] - g.a2[0] * g.a1[1];
        (
            (p[0] * g.a2[1] - g.a2[0] * p[1]) / det,
            (g.a1[0] * p[1] - p[0] * g.a1[1]) / det,
        )
    }

    /// Frame the whole pattern (on the next frame, when the canvas size is known).
    fn fit(&mut self) {
        self.fit_pending = true;
    }

    fn do_fit(&mut self, size: Vec2) {
        let Some(p) = &self.plane else { return };
        let Some(b) = p.bounds() else {
            self.centre = [0.0, 0.0];
            return;
        };
        let corners = [
            self.to_world(b[0] as f64, b[1] as f64),
            self.to_world(b[2] as f64 + 1.0, b[1] as f64),
            self.to_world(b[0] as f64, b[3] as f64 + 1.0),
            self.to_world(b[2] as f64 + 1.0, b[3] as f64 + 1.0),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for c in corners {
            x0 = x0.min(c[0]);
            y0 = y0.min(c[1]);
            x1 = x1.max(c[0]);
            y1 = y1.max(c[1]);
        }
        self.centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let sx = size.x as f64 * 0.92 / (x1 - x0).max(1e-9);
        let sy = size.y as f64 * 0.92 / (y1 - y0).max(1e-9);
        self.scale = sx.min(sy).min(40.0 / self.cell_size());
        self.fit_pending = false;
    }

    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    fn start_jump(&mut self, n: u64) {
        let Some(mut p) = self.plane.take() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let r = p.step(n).map(|()| t.elapsed().as_secs_f64());
            let _ = tx.send((p, r));
        });
        self.worker = Some(rx);
        self.status = format!("Jumping {n} generations…");
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.worker else { return };
        match rx.try_recv() {
            Ok((p, r)) => {
                match r {
                    Ok(secs) => {
                        let n = hashlife::parse_gens(&self.gens).unwrap_or(0);
                        self.status = jump_status(&p, n, secs);
                    }
                    Err(e) => self.error = Some(e),
                }
                self.plane = Some(p);
                self.worker = None;
                self.tex_key = None;
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint(),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.worker = None;
                self.error = Some("the jump stopped unexpectedly".into());
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        self.poll(ctx);
        let mut open = self.open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "HashLife view")
                .size(900.0, 640.0)
                .configure(|w| w.default_size([900.0, 640.0]).resizable(true)),
            &mut open,
            |ui| self.body(ui),
        );
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Generations");
            ui.add(egui::TextEdit::singleline(&mut self.gens).desired_width(80.0))
                .on_hover_text("A number, or 2^40, 1e6");
            let n = hashlife::parse_gens(&self.gens);
            let can = n.is_ok() && !self.busy() && self.plane.is_some();
            if ui.add_enabled(can, egui::Button::new("Jump")).clicked()
                && let Ok(n) = n
            {
                self.start_jump(n);
            }
            if ui
                .add_enabled(
                    !self.busy() && self.plane.is_some(),
                    egui::Button::new("+1"),
                )
                .clicked()
            {
                self.gens = "1".into();
                self.start_jump(1);
            }
            if ui.button("Fit").clicked() {
                self.fit();
            }
            if ui.button("−").on_hover_text("Zoom out").clicked() {
                self.scale /= 2.0;
            }
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                self.scale *= 2.0;
            }
            if self.busy() {
                ui.spinner();
            }
            ui.separator();
            ui.label("Colours");
            let label = format!("World's ({})", self.world_palette.name);
            let extra = self.extra.clone();
            if crate::palette_pick::picker_opt(
                ui,
                "hlview-palette",
                &mut self.palette,
                &extra,
                &label,
            ) {
                self.colors_changed();
            }
        });
        if let Some(p) = &self.plane {
            let b = p.bounds();
            let extent = b
                .map(|b| format!("{} × {} unit cells", b[2] - b[0] + 1, b[3] - b[1] + 1))
                .unwrap_or_else(|| "empty".into());
            ui.label(
                egui::RichText::new(format!(
                    "Generation {}  ·  {} live sites  ·  {extent}  ·  {} px per cell  ·  {}",
                    p.generation,
                    p.population(),
                    fmt_scale(self.scale * self.cell_size()),
                    self.status
                ))
                .small(),
            );
        } else {
            ui.label(egui::RichText::new(&self.status).small());
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let size = ui.available_size().max(Vec2::new(200.0, 150.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        if self.fit_pending && self.plane.is_some() {
            self.do_fit(rect.size());
        }
        // Pan and zoom.
        if resp.dragged() {
            let d = resp.drag_delta();
            self.centre[0] -= d.x as f64 / self.scale;
            self.centre[1] -= d.y as f64 / self.scale;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
            if scroll != 0.0
                && let Some(m) = resp.hover_pos()
            {
                let before = self.screen_to_world(rect, m);
                self.scale = (self.scale * (scroll as f64 * 0.0025).exp()).clamp(1e-18, 400.0);
                let after = self.screen_to_world(rect, m);
                self.centre[0] += before[0] - after[0];
                self.centre[1] += before[1] - after[1];
            }
        }
        let painter = ui.painter_at(rect);
        let b = self.colors.background;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(b[0], b[1], b[2]));
        if self.lattice.is_some() {
            self.draw(ui.ctx(), &painter, rect);
        }
    }

    fn screen_to_world(&self, rect: Rect, s: Pos2) -> P {
        let c = rect.center();
        [
            self.centre[0] + (s.x - c.x) as f64 / self.scale,
            self.centre[1] + (s.y - c.y) as f64 / self.scale,
        ]
    }
    fn world_to_screen(&self, rect: Rect, p: P) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p[0] - self.centre[0]) * self.scale) as f32,
            c.y + ((p[1] - self.centre[1]) * self.scale) as f32,
        )
    }

    /// The lattice box `(i0, j0, w, h)` covering the canvas, aligned to `2^p`.
    fn visible(&self, rect: Rect, p: u8) -> (i64, i64, u64, u64) {
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ];
        let (mut i0, mut j0, mut i1, mut j1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for c in corners {
            let (i, j) = self.to_lattice(self.screen_to_world(rect, c));
            i0 = i0.min(i);
            j0 = j0.min(j);
            i1 = i1.max(i);
            j1 = j1.max(j);
        }
        let b = (1i128 << p) as f64;
        let lim = (i64::MAX / 4) as f64;
        let q = |v: f64, up: bool| -> i64 {
            let v = if up {
                (v / b).ceil() + 1.0
            } else {
                (v / b).floor() - 1.0
            };
            (v * b).clamp(-lim, lim) as i64
        };
        let (a0, c0, a1, c1) = (q(i0, false), q(j0, false), q(i1, true), q(j1, true));
        (a0, c0, (a1 - a0).max(1) as u64, (c1 - c0).max(1) as u64)
    }

    fn draw(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        let Some(plane) = &self.plane else {
            // A jump is running: keep the last texture.
            self.draw_texture(painter, rect);
            return;
        };
        let ppc = self.scale * self.cell_size();
        let area = (rect.width() * rect.height()) as f64 / (ppc * ppc).max(1e-300);
        let polys = ppc >= 3.0 && area <= MAX_POLYS as f64;
        if polys {
            self.draw_tiles(painter, rect);
            return;
        }
        // Rasterise: blocks of 2^p sites, about a pixel each.
        let mut p = 0u8;
        while ppc * ((1u128 << p) as f64) < 1.0 && p < 120 {
            p += 1;
        }
        let region = self.visible(rect, p);
        let key = (plane.generation, region, p);
        if self.tex_key != Some(key) {
            let iw = region.2 >> p;
            let ih = region.3 >> p;
            if iw == 0 || ih == 0 || iw * ih > 16_000_000 {
                return;
            }
            let (iw, ih) = (iw as usize, ih as usize);
            let mut px = vec![Color32::TRANSPARENT; iw * ih];
            let states = plane.layers[0].uni.rule.states;
            if p == 0 {
                let g = plane.site_grid(region);
                for (o, &v) in px.iter_mut().zip(&g) {
                    if v != 0 {
                        let c = self.colors.state(v, states, self.wire);
                        *o = Color32::from_rgb(c[0], c[1], c[2]);
                    }
                }
            } else {
                let mut acc = vec![0u64; iw * ih];
                let (i0, j0) = (region.0 as i128, region.1 as i128);
                plane.blocks_in(region, p, &mut |x, y, n| {
                    let bx = ((x - i0) >> p) as usize;
                    let by = ((y - j0) >> p) as usize;
                    if bx < iw && by < ih {
                        acc[by * iw + bx] += n;
                    }
                });
                let per = ((1u128 << p) * (1u128 << p)) as f64;
                let live_bg = plane.background().iter().any(|&b| b != 0);
                for (o, &n) in px.iter_mut().zip(&acc) {
                    let f = (n as f64 / per).clamp(0.0, 1.0);
                    let f = if live_bg { 1.0 - f } else { f };
                    if f > 0.0 {
                        let c = self.colors.density(f);
                        *o = Color32::from_rgb(c[0], c[1], c[2]);
                    }
                }
            }
            let img = egui::ColorImage {
                size: [iw, ih],
                source_size: Vec2::new(iw as f32, ih as f32),
                pixels: px,
            };
            let opts = egui::TextureOptions::NEAREST;
            match &mut self.tex {
                Some(t) => t.set(img, opts),
                None => self.tex = Some(ctx.load_texture("hashlife-view", img, opts)),
            }
            self.tex_key = Some(key);
        }
        self.draw_texture(painter, rect);
    }

    /// The texture as a parallelogram: lattice box corners mapped to the screen.
    fn draw_texture(&self, painter: &egui::Painter, rect: Rect) {
        let (Some(t), Some((_, r, _))) = (&self.tex, self.tex_key) else {
            return;
        };
        if self.lattice.is_none() {
            return;
        }
        let (i0, j0) = (r.0 as f64, r.1 as f64);
        let (i1, j1) = (i0 + r.2 as f64, j0 + r.3 as f64);
        let s = |i: f64, j: f64| self.world_to_screen(rect, self.to_world(i, j));
        let mut mesh = egui::Mesh::with_texture(t.id());
        let w = Color32::WHITE;
        mesh.vertices.extend([
            egui::epaint::Vertex {
                pos: s(i0, j0),
                uv: Pos2::new(0.0, 0.0),
                color: w,
            },
            egui::epaint::Vertex {
                pos: s(i1, j0),
                uv: Pos2::new(1.0, 0.0),
                color: w,
            },
            egui::epaint::Vertex {
                pos: s(i0, j1),
                uv: Pos2::new(0.0, 1.0),
                color: w,
            },
            egui::epaint::Vertex {
                pos: s(i1, j1),
                uv: Pos2::new(1.0, 1.0),
                color: w,
            },
        ]);
        mesh.indices.extend([0, 1, 2, 1, 3, 2]);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Close up: every tile in view in its own shape.
    fn draw_tiles(&self, painter: &egui::Painter, rect: Rect) {
        let (Some(plane), Some(g)) = (&self.plane, &self.lattice) else {
            return;
        };
        let (i0, j0, w, h) = self.visible(rect, 0);
        let states = plane.layers[0].uni.rule.states;
        // A live background fills the view; tiles in other states are drawn over it.
        let bg = plane.background();
        if bg.iter().any(|&b| b != 0) {
            let v = bg.iter().copied().max().unwrap_or(1);
            let c = self.colors.state(v, states, self.wire);
            painter.rect_filled(rect, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
        }
        let stroke = egui::Stroke::new(
            if self.scale * self.cell_size() > 8.0 {
                1.0
            } else {
                0.0
            },
            self.outline,
        );
        for li in 0..plane.layers.len() {
            for (i, j, k, v) in plane.layer_cells_in(li, i0, j0, w, h) {
                if bg.get(k as usize) == Some(&v) {
                    continue;
                }
                let t = Affine::trans(
                    i as f64 * g.a1[0] + j as f64 * g.a2[0],
                    i as f64 * g.a1[1] + j as f64 * g.a2[1],
                )
                .mul(&g.types[k as usize].0);
                let pts: Vec<Pos2> = self.shapes[g.types[k as usize].1]
                    .iter()
                    .map(|&q| self.world_to_screen(rect, t.apply(q)))
                    .collect();
                let c = self.colors.state(v, states, self.wire);
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::from_rgb(c[0], c[1], c[2]),
                    stroke,
                ));
            }
        }
    }
}

fn jump_status(p: &Plane, n: u64, secs: f64) -> String {
    let st = p.stats();
    format!("jumped {n} in {:.3} s ({} nodes)", secs, st.nodes)
}

fn fmt_scale(v: f64) -> String {
    if v >= 0.01 {
        format!("{v:.3}")
    } else {
        format!("{v:.2e}")
    }
}
