//! The Draw pane: mouse tool, target CA, state.

use super::*;

impl JetcarpApp {
    pub(super) fn draw_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.tool, Tool::Pan, "Pan")
                .on_hover_text("Drag to pan, scroll to zoom");
            ui.radio_value(&mut self.tool, Tool::Draw, "Draw")
                .on_hover_text("Paint cells into the chosen CA's field");
            ui.radio_value(&mut self.tool, Tool::Erase, "Erase");
        });
        ui.label(
            RichText::new("Right- or middle-drag always pans. Scroll zooms.")
                .small()
                .weak(),
        );
        if self.tool != Tool::Pan {
            let name = self
                .draw_target
                .and_then(|id| self.world.ca(id))
                .map(|c| c.name.clone())
                .unwrap_or("none".into());
            ui.horizontal(|ui| {
                ui.label("Into");
                egui::ComboBox::from_id_salt("target")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for c in &self.world.cas {
                            ui.selectable_value(&mut self.draw_target, Some(c.id), &c.name);
                        }
                    });
            });
            if let Some(ca) = self.draw_target.and_then(|id| self.world.ca(id)) {
                let f = &self.world.fields[ca.field];
                let max = match f.kind {
                    FieldKind::Height => 100_000,
                    k => match k {
                        FieldKind::Multi(n) => n as u32 - 1,
                        _ => 1,
                    },
                };
                if max > 1 && self.tool == Tool::Draw {
                    ui.horizontal(|ui| {
                        ui.label(if f.kind == FieldKind::Height {
                            "Grains per click"
                        } else if matches!(ca.rule, Rule::Jaca(_)) {
                            "Sign"
                        } else {
                            "State"
                        });
                        if ca.rule == Rule::VonNeumann {
                            jvn_state_picker(ui, &mut self.draw_value);
                        } else if matches!(ca.rule, Rule::Jaca(_)) {
                            ui.radio_value(&mut self.draw_value, jetcarp_core::jaca::POS as u32, "+1");
                            ui.radio_value(&mut self.draw_value, jetcarp_core::jaca::NEG as u32, "−1");
                        } else {
                            ui.add(egui::DragValue::new(&mut self.draw_value).range(1..=max));
                        }
                    });
                    if ca.rule == Rule::VonNeumann {
                        ui.label(
                            RichText::new(
                                "Erase paints ground (U). Exported PNGs add direction arrows.",
                            )
                            .small()
                            .weak(),
                        );
                    }
                    if ca.rule == Rule::WireWorld {
                        ui.label(
                            RichText::new("WireWorld: 3 wire, 1 electron head, 2 tail")
                                .small()
                                .weak(),
                        );
                    }
                } else {
                    self.draw_value = self.draw_value.clamp(1, max.max(1));
                }
            }
        }
    }
}

/// Draw-tool state picker for von Neumann's 29 states, with a colour swatch per state.
pub(super) fn jvn_state_picker(ui: &mut egui::Ui, value: &mut u32) {
    use jetcarp_core::vonneumann as jvn;
    let swatch = |ui: &mut egui::Ui, s: u8| {
        let [r, g, b, _] = jvn::COLORS[s as usize];
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
    };
    let cur = (*value).clamp(1, jvn::STATES as u32 - 1) as u8;
    swatch(ui, cur);
    egui::ComboBox::from_id_salt("jvn-state")
        .selected_text(jvn::state_name(cur))
        .height(600.0)
        .show_ui(ui, |ui| {
            for s in 1..jvn::STATES {
                ui.horizontal(|ui| {
                    swatch(ui, s);
                    ui.selectable_value(value, s as u32, jvn::state_name(s));
                });
            }
        });
}
