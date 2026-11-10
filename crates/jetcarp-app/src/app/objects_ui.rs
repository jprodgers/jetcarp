//! The "Objects" section: find still lifes, oscillators, and spaceships in the current state and
//! highlight them. Read-only: it never changes the world. Its state lives in egui's memory so
//! `JetcarpApp` needs no new fields.

use super::JetcarpApp;
use eframe::egui::{self, Color32, RichText, Stroke};
use jetcarp_core::objects::{ObjectOptions, ObjectReport, find_objects};
use std::sync::Arc;

#[derive(Clone, Default)]
struct ObjectsState {
    report: Option<Arc<ObjectReport>>,
    /// Count row whose objects are highlighted.
    selected: Option<usize>,
    error: Option<String>,
    secs: f64,
}

fn state_id() -> egui::Id {
    egui::Id::unique("jetcarp-objects-section")
}

const HIGHLIGHT: Color32 = Color32::from_rgb(0xff, 0xe6, 0x50);

fn class_color(c: &jetcarp_core::objects::ObjClass) -> Color32 {
    let [r, g, b, _] = c.color();
    Color32::from_rgb(r, g, b)
}

impl JetcarpApp {
    pub(super) fn objects_ui(&mut self, ui: &mut egui::Ui) {
        let mut st: ObjectsState = ui
            .ctx()
            .data(|d| d.get_temp(state_id()))
            .unwrap_or_default();
        ui.scope(|ui| {
            ui.label(
                RichText::new(
                    "Still lifes, oscillators, and spaceships in the current state. Each group of \
                     live cells runs alone with the same rule; periods use exact comparison.",
                )
                .small(),
            );
            if ui.button("Find objects").clicked() {
                let t0 = std::time::Instant::now();
                match find_objects(&self.world, &ObjectOptions::default()) {
                    Ok(r) => {
                        st.report = Some(Arc::new(r));
                        st.error = None;
                    }
                    Err(e) => {
                        st.report = None;
                        st.error = Some(e);
                    }
                }
                st.selected = None;
                st.secs = t0.elapsed().as_secs_f64();
            }
            if let Some(e) = &st.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            let Some(r) = st.report.clone() else {
                return;
            };
            let t = r.class_totals();
            ui.label(format!(
                "{} objects at generation {} ({:.2}s): {} still, {} oscillating, {} moving, {} other",
                r.objects.len(),
                r.generation,
                st.secs,
                t[0],
                t[1],
                t[2],
                t[3]
            ));
            if r.generation != self.world.generation || r.tiling != self.world.topo.describe() {
                ui.label(
                    RichText::new("The world has changed since; find again to refresh.")
                        .small()
                        .color(Color32::LIGHT_RED),
                );
            }
            ui.label(RichText::new("Click a row to highlight those cells.").small());
            for (k, row) in r.counts.iter().enumerate() {
                let text = RichText::new(format!("{:>5} × {}", row.count, row.label()))
                    .monospace()
                    .color(class_color(&row.class));
                if ui.selectable_label(st.selected == Some(k), text).clicked() {
                    st.selected = if st.selected == Some(k) { None } else { Some(k) };
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(state_id(), st));
    }

    /// Outline the cells of the selected row's objects over the viewport.
    pub(super) fn objects_overlay(&self, ui: &egui::Ui, rect: egui::Rect) {
        let st: Option<ObjectsState> = ui.ctx().data(|d| d.get_temp(state_id()));
        let Some(ObjectsState {
            report: Some(r),
            selected: Some(k),
            ..
        }) = st
        else {
            return;
        };
        let Some(row) = r.counts.get(k) else { return };
        let topo = &self.world.topo;
        if r.tiling != topo.describe() {
            return;
        }
        let painter = ui.painter_at(rect);
        let xf = self.tile_xf(rect, ui.ctx().pixels_per_point());
        let to_screen = |p: [f64; 2]| xf.to_screen(p);
        let (ww, wh) = (
            topo.bounds[2] - topo.bounds[0],
            topo.bounds[3] - topo.bounds[1],
        );
        let mut drawn = 0usize;
        for &oi in &row.objects {
            let o = &r.objects[oi];
            if drawn < 50_000 {
                for &cell in &o.cells {
                    if cell as usize >= topo.len() {
                        continue;
                    }
                    let pts: Vec<egui::Pos2> = topo
                        .polygon(cell as usize)
                        .into_iter()
                        .map(to_screen)
                        .collect();
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, HIGHLIGHT)));
                    drawn += 1;
                }
            }
            // A box around each object so small ones show when zoomed out (skipped if it wraps).
            let b = o.bbox;
            if b[2] - b[0] < ww / 2.0 && b[3] - b[1] < wh / 2.0 {
                let r = egui::Rect::from_two_pos(to_screen([b[0], b[1]]), to_screen([b[2], b[3]]))
                    .expand(4.0);
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, HIGHLIGHT),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
}
