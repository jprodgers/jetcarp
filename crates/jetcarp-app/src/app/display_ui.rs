//! The Display pane: where the grid shows (window, full screen, outputs), the outputs'
//! arrangement, the cell size, keep-awake, and pointers to the Screensaver and Desktop background
//! panes.

use super::display::{OutputMode, OutputSpec};
use super::*;
use crate::span::{self, Arrangement, Units};

impl JetcarpApp {
    pub(super) fn display_pane(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let monitors = self.monitors_deduped();
        // ------------------------------------------------ where
        ui.label(RichText::new("Where the grid shows").strong());
        let mut mode = self.shell.display.mode;
        ui.horizontal(|ui| {
            ui.radio_value(&mut mode, OutputMode::Window, "This window");
            ui.radio_value(&mut mode, OutputMode::FullScreen, "Full screen")
                .on_hover_text("F11");
            ui.radio_value(&mut mode, OutputMode::Outputs, "Outputs")
                .on_hover_text("Chrome-less full-screen windows on the monitors chosen below");
        });
        self.shell.display.mode = mode;
        if mode == OutputMode::FullScreen {
            ui.horizontal(|ui| {
                ui.label("Monitor");
                let cur = self.shell.display.fullscreen_monitor;
                let text = match cur {
                    None => "The one this window is on".to_string(),
                    Some(i) => format!("{i}"),
                };
                egui::ComboBox::from_id_salt("fs-monitor")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.shell.display.fullscreen_monitor,
                            None,
                            "The one this window is on",
                        );
                        for m in &monitors {
                            ui.selectable_value(
                                &mut self.shell.display.fullscreen_monitor,
                                Some(m.index),
                                format!("{}: {}", m.index, m.describe()),
                            );
                        }
                    });
            });
        }
        // ------------------------------------------------ outputs
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Outputs")
            .default_open(mode == OutputMode::Outputs)
            .show(ui, |ui| self.outputs_ui(ui, &monitors));
        // ------------------------------------------------ cells
        ui.add_space(4.0);
        ui.label(RichText::new("Cell size").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.shell.display.cell_px, display::CELL_PX_RANGE)
                    .logarithmic(true)
                    .integer()
                    .suffix(" px"),
            )
            .on_hover_text("Physical pixels per cell edge, for Fill and the pixel-exact zoom");
        });
        ui.horizontal(|ui| {
            let sim = self.screen == Screen::Sim;
            if ui
                .add_enabled(sim, egui::Button::new("Fill the canvas"))
                .on_hover_text("Rebuild this tiling to cover the canvas at the cell size")
                .clicked()
            {
                self.fill_canvas();
            }
            if ui
                .add_enabled(sim, egui::Button::new("Pixel-exact zoom"))
                .on_hover_text("Ctrl+1: one cell edge is the cell size in pixels")
                .clicked()
            {
                self.pixel_zoom(self.shell.display.cell_px as f64);
            }
        });
        let c = self.canvas_px();
        ui.label(
            RichText::new(format!(
                "Canvas {:.0} × {:.0} px; now {:.2} px per cell.",
                c[0],
                c[1],
                self.zoom * self.cell_unit()
            ))
            .small()
            .weak(),
        );
        // ------------------------------------------------ keep awake
        ui.add_space(4.0);
        ui.checkbox(
            &mut self.shell.display.keep_awake,
            "Keep the computer awake",
        )
        .on_hover_text("No screen blanking, locking, or suspend while JETCARP is open");
        ui.label(
            RichText::new(format!("Keep-awake: {}", self.display.keep.describe()))
                .small()
                .weak(),
        );
        // ------------------------------------------------ screensaver
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Screensaver").strong());
            if ui
                .button("Screensaver settings…")
                .on_hover_text(
                    "Profiles, rotations, reseeding, and options (View → Screensaver settings…)",
                )
                .clicked()
            {
                self.shell.layout.show_pane("screensaver");
            }
            let label = if self.saver_active() {
                "Leave"
            } else {
                "Start"
            };
            if ui.button(label).on_hover_text("F12").clicked() {
                self.saver_toggle(&ctx);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Desktop background").strong());
            if ui
                .button("Desktop background settings…")
                .on_hover_text(
                    "The background host: scenes behind your windows, the lock screen, and fitting them to this computer (View → Desktop background settings…)",
                )
                .clicked()
            {
                self.shell.layout.show_pane("desktop");
            }
        });
        // ------------------------------------------------ monitors
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Monitors")
            .default_open(false)
            .show(ui, |ui| {
                if monitors.is_empty() {
                    ui.label(RichText::new("No monitors reported (yet).").weak());
                }
                for m in &monitors {
                    let mut t = format!("{}: {}", m.index, m.describe());
                    if m.current {
                        t.push_str("  (this window)");
                    }
                    ui.label(RichText::new(t).monospace().small());
                }
                let dups = self.shell.monitors.list().len() - monitors.len();
                if dups > 0 {
                    ui.label(
                        RichText::new(format!(
                            "{dups} duplicate entr{} hidden (the compositor lists a monitor twice).",
                            if dups == 1 { "y" } else { "ies" }
                        ))
                        .small()
                        .weak(),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.shell.monitors.refresh();
                    }
                    if ui
                        .button("Identify")
                        .on_hover_text("Show each monitor's number on it for three seconds")
                        .clicked()
                    {
                        self.identify_monitors(&ctx);
                    }
                });
            });
    }

    fn outputs_ui(&mut self, ui: &mut egui::Ui, monitors: &[crate::monitors::MonitorInfo]) {
        let d = &mut self.shell.display;
        ui.label(RichText::new("Monitors to show the grid on:").small());
        for m in monitors {
            let on = d
                .outputs
                .iter()
                .any(|o| o.monitor == m.index && o.window.is_none());
            let mut c = on;
            ui.checkbox(&mut c, format!("{}: {}", m.index, m.describe()));
            if c && !on {
                d.outputs.push(OutputSpec {
                    monitor: m.index,
                    pos: None,
                    window: None,
                });
            } else if !c && on {
                d.outputs
                    .retain(|o| !(o.monitor == m.index && o.window.is_none()));
            }
        }
        let windows = d.outputs.iter().filter(|o| o.window.is_some()).count();
        ui.horizontal(|ui| {
            if ui
                .button("Add a test window")
                .on_hover_text(
                    "An output as an ordinary 800 × 600 window: try spans on one monitor",
                )
                .clicked()
            {
                let x = d
                    .outputs
                    .iter()
                    .map(|o| o.pos.unwrap_or([0, 0])[0] + o.window.map_or(0, |w| w[0] as i32))
                    .max()
                    .unwrap_or(0);
                d.outputs.push(OutputSpec {
                    monitor: 0,
                    pos: Some([x, 0]),
                    window: Some([800, 600]),
                });
            }
            if windows > 0 && ui.button("Remove test windows").clicked() {
                d.outputs.retain(|o| o.window.is_none());
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut d.arrangement, Arrangement::Span, "Span")
                .on_hover_text("One canvas across the outputs, laid out by their positions");
            ui.radio_value(&mut d.arrangement, Arrangement::Mirror, "Mirror")
                .on_hover_text("The same view on each output");
        });
        ui.horizontal(|ui| {
            ui.label("Units");
            ui.radio_value(&mut d.units, Units::Pixels, "Pixels");
            ui.radio_value(&mut d.units, Units::Logical, "Logical")
                .on_hover_text("Mixed scale factors with positions in logical pixels");
        });
        ui.checkbox(
            &mut d.hide_preview,
            "No grid in this window while outputs show",
        )
        .on_hover_text("Saves the GPU work of drawing the preview");
        if d.outputs.is_empty() {
            ui.label(RichText::new("No outputs chosen.").weak());
            return;
        }
        // The arrangement: positions (manual overrides) and a small map.
        ui.label(RichText::new("Arrangement (canvas units; drag the boxes)").small());
        let screens = self.screens();
        let d = &mut self.shell.display;
        let mut moved: Option<(usize, [f64; 2])> = None;
        if let Some(can) = span::canvas(&screens, d.units) {
            let w = ui.available_width().min(320.0);
            let k = (w as f64 / can.size()[0].max(1.0)).min(120.0 / can.size()[1].max(1.0));
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(w, (can.size()[1] * k) as f32 + 4.0),
                Sense::hover(),
            );
            let p = ui.painter_at(rect);
            for (i, s) in screens.iter().enumerate() {
                let r = s.rect(d.units);
                let a = rect.min
                    + egui::vec2(
                        ((r.min[0] - can.min[0]) * k) as f32,
                        ((r.min[1] - can.min[1]) * k) as f32,
                    );
                let b = a + egui::vec2((r.size()[0] * k) as f32, (r.size()[1] * k) as f32);
                let box_ = egui::Rect::from_min_max(a, b).shrink(1.0);
                let resp = ui.interact(box_, egui::Id::unique(("out-box", i)), Sense::drag());
                if resp.dragged() {
                    let dd = resp.drag_delta();
                    moved = Some((i, [dd.x as f64 / k, dd.y as f64 / k]));
                }
                p.rect_filled(box_, 2.0, Color32::from_rgb(0x30, 0x40, 0x58));
                p.rect_stroke(
                    box_,
                    2.0,
                    egui::Stroke::new(1.0, Color32::from_gray(160)),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    box_.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
        }
        if let Some((i, dd)) = moved
            && let Some(o) = d.outputs.get_mut(i)
        {
            let base = o
                .pos
                .unwrap_or([screens[i].pos[0] as i32, screens[i].pos[1] as i32]);
            o.pos = Some([
                base[0] + dd[0].round() as i32,
                base[1] + dd[1].round() as i32,
            ]);
        }
        let mut remove = None;
        egui::Grid::new("outputs-grid")
            .num_columns(4)
            .show(ui, |ui| {
                for (i, o) in d.outputs.iter_mut().enumerate() {
                    let what = match o.window {
                        Some(w) => format!("{}: window {}×{}", i + 1, w[0], w[1]),
                        None => format!("{}: monitor {}", i + 1, o.monitor),
                    };
                    ui.label(what);
                    let mut pos = o
                        .pos
                        .unwrap_or([screens[i].pos[0] as i32, screens[i].pos[1] as i32]);
                    let a = ui.add(egui::DragValue::new(&mut pos[0]).prefix("x "));
                    let b = ui.add(egui::DragValue::new(&mut pos[1]).prefix("y "));
                    if a.changed() || b.changed() {
                        o.pos = Some(pos);
                    }
                    ui.horizontal(|ui| {
                        if o.pos.is_some()
                            && ui
                                .small_button("auto")
                                .on_hover_text("Use the position winit reports")
                                .clicked()
                        {
                            o.pos = None;
                        }
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            d.outputs.remove(i);
        }
        ui.horizontal(|ui| {
            if ui
                .button("Side by side")
                .on_hover_text("Place the outputs left to right in this order")
                .clicked()
            {
                let mut s = self.screens();
                span::side_by_side(&mut s, self.shell.display.units);
                for (o, s) in self.shell.display.outputs.iter_mut().zip(&s) {
                    o.pos = Some([s.pos[0] as i32, s.pos[1] as i32]);
                }
            }
            if ui
                .button("Guess units")
                .on_hover_text(
                    "Logical when the monitors overlap in pixels but not in logical pixels",
                )
                .clicked()
            {
                self.shell.display.units = span::guess_units(&self.screens());
            }
        });
    }
}
