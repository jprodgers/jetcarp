//! The palette window: every palette as a swatch on its own background, one click to apply it
//! to the chosen CA or every CA, a live preview on the grid while a swatch is hovered or has
//! keyboard focus (arrow keys move between swatches; Enter or Space applies), and an editor for
//! the stops, background, and stage colours that saves a named palette into the scene
//! (`Command::SetPalette`).
//!
//! The preview sets the CAs' palette names directly, without a command, and puts them back when
//! the preview ends; applying a palette goes through `Command::UpdateCa` like any other change.

use super::*;

/// The palette window's state (the stops and name being edited are `JetcarpApp::pal_stops` and
/// `pal_name`).
pub(super) struct PaletteUi {
    /// Apply to this CA (None: every CA).
    pub target: Option<u32>,
    /// The edited palette's background and stage colours.
    pub background: [u8; 3],
    pub stage: [u8; 3],
    /// A preview in place: the palette shown, and each previewed CA's own palette.
    preview: Option<(String, Vec<(u32, String)>)>,
}

impl Default for PaletteUi {
    fn default() -> Self {
        let d = Palette::default();
        PaletteUi {
            target: None,
            background: rgb(d.background_rgba()),
            stage: rgb(d.stage_rgba()),
            preview: None,
        }
    }
}

fn rgb(c: render::Rgba) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Text colour that reads on background `bg`.
pub(super) fn ink_for(bg: [u8; 3]) -> Color32 {
    let lum = 0.299 * bg[0] as f32 + 0.587 * bg[1] as f32 + 0.114 * bg[2] as f32;
    if lum > 140.0 {
        Color32::from_gray(30)
    } else {
        Color32::from_gray(225)
    }
}

/// A swatch: the palette's stops as a little patch of cells on its background, its name below.
fn swatch(ui: &mut egui::Ui, p: &Palette, current: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(118.0, 54.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter_at(rect);
    let bg = rgb(p.background_rgba());
    painter.rect_filled(rect, 4.0, c32(bg));
    // Two rows of cells, each stop in turn, with gaps of background between.
    let n = p.stops.len().max(1);
    let cell = 9.0;
    let cols = 10;
    let x0 = rect.left() + 7.0;
    let y0 = rect.top() + 7.0;
    for row in 0..2 {
        for col in 0..cols {
            if (row + col) % 3 == 2 {
                continue;
            }
            let k = (col * n / cols + row) % n;
            let c = p.stops.get(k).copied().unwrap_or([255, 255, 255]);
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    x0 + col as f32 * (cell + 1.5),
                    y0 + row as f32 * (cell + 1.5),
                ),
                egui::vec2(cell, cell),
            );
            painter.rect_filled(r, 1.5, c32(c));
        }
    }
    painter.text(
        egui::pos2(rect.left() + 7.0, rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        &p.name,
        egui::FontId::proportional(12.0),
        ink_for(bg),
    );
    let focused = resp.has_focus();
    let stroke = if current {
        egui::Stroke::new(2.0, Color32::from_rgb(255, 200, 80))
    } else if focused || resp.hovered() {
        egui::Stroke::new(2.0, Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, Color32::from_gray(70))
    };
    painter.rect_stroke(rect.shrink(1.0), 4.0, stroke, egui::StrokeKind::Inside);
    resp
}

impl JetcarpApp {
    /// Load a palette into the editor.
    pub(super) fn pick_palette(&mut self, name: &str) {
        let p = self.world.palette(name);
        self.pal_name = p.name.clone();
        self.pal_stops = p.stops.clone();
        self.pal_ui.background = rgb(p.background_rgba());
        self.pal_ui.stage = rgb(p.stage_rgba());
    }

    /// The CAs a palette goes to.
    fn palette_targets(&self) -> Vec<u32> {
        match self.pal_ui.target {
            Some(id) if self.world.ca(id).is_some() => vec![id],
            _ => self.world.cas.iter().map(|c| c.id).collect(),
        }
    }

    /// Show palette `name` on the grid without a command (None: end the preview).
    fn preview_palette(&mut self, name: Option<&str>) {
        if self.pal_ui.preview.as_ref().map(|p| p.0.as_str()) == name {
            return;
        }
        if let Some((shown, saved)) = self.pal_ui.preview.take() {
            for (id, pal) in saved {
                // Only CAs still showing the preview (the world may have been replaced).
                if let Some(c) = self.world.cas.iter_mut().find(|c| c.id == id)
                    && c.palette == shown
                {
                    c.palette = pal;
                }
            }
            self.colors_dirty = true;
        }
        let Some(name) = name else { return };
        let targets = self.palette_targets();
        let mut saved = vec![];
        for c in self
            .world
            .cas
            .iter_mut()
            .filter(|c| targets.contains(&c.id))
        {
            saved.push((c.id, std::mem::replace(&mut c.palette, name.to_string())));
        }
        self.pal_ui.preview = Some((name.to_string(), saved));
        self.colors_dirty = true;
    }

    /// End any palette preview (the window closed or the screen changed).
    pub(super) fn end_palette_preview(&mut self) {
        self.preview_palette(None);
    }

    /// Commands that give the target CAs palette `name`.
    fn apply_palette_cmds(&self, name: &str) -> Vec<Command> {
        self.palette_targets()
            .into_iter()
            .map(|id| Command::UpdateCa {
                id,
                change: CaChange {
                    palette: Some(name.to_string()),
                    ..Default::default()
                },
            })
            .collect()
    }

    /// The palette being edited.
    fn edited_palette(&self, name: &str) -> Palette {
        Palette {
            name: name.to_string(),
            stops: self.pal_stops.clone(),
            background: Some(self.pal_ui.background),
            stage: Some(self.pal_ui.stage),
        }
    }

    pub(super) fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.pal_open {
            self.end_palette_preview();
            return;
        }
        let mut open = true;
        let mut cmds = Vec::new();
        let mut want_preview: Option<String> = None;
        let mut pick: Option<String> = None;
        let spec =
            crate::popout::Spec::new(menu::ToolWin::Palettes.key(), "Palettes").size(540.0, 600.0);
        let names = self.world.palette_names();
        // Palettes as the swatches show them (the world's own versions of any user palettes).
        let palettes: Vec<Palette> = names.iter().map(|n| self.world.palette(n)).collect();
        let cas: Vec<(u32, String, String)> = self
            .world
            .cas
            .iter()
            .map(|c| {
                // The CA's own palette, not a preview.
                let own = self
                    .pal_ui
                    .preview
                    .as_ref()
                    .and_then(|(_, s)| s.iter().find(|x| x.0 == c.id).map(|x| x.1.clone()))
                    .unwrap_or_else(|| c.palette.clone());
                (c.id, c.name.clone(), own)
            })
            .collect();
        crate::popout::tool_window(ctx, spec, &mut open, |ui| {
            ui.horizontal(|ui| {
                ui.label("Apply to");
                let text = match self.pal_ui.target {
                    Some(id) => cas
                        .iter()
                        .find(|c| c.0 == id)
                        .map(|c| c.1.clone())
                        .unwrap_or_else(|| "every CA".into()),
                    None => "every CA".into(),
                };
                egui::ComboBox::from_id_salt("pal-target")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.pal_ui.target.is_none(), "every CA")
                            .clicked()
                        {
                            self.pal_ui.target = None;
                        }
                        for (id, name, pal) in &cas {
                            if ui
                                .selectable_label(
                                    self.pal_ui.target == Some(*id),
                                    format!("{name} ({pal})"),
                                )
                                .clicked()
                            {
                                self.pal_ui.target = Some(*id);
                            }
                        }
                    });
            });
            ui.label(
                RichText::new("Click a palette to apply it. Hover, or use the arrow keys after a click, to preview it on the grid; Enter applies.")
                    .small()
                    .weak(),
            );
            // The palette the target shows now.
            let current: Option<String> = match self.pal_ui.target {
                Some(id) => cas.iter().find(|c| c.0 == id).map(|c| c.2.clone()),
                None => cas.first().map(|c| c.2.clone()),
            };
            egui::ScrollArea::vertical()
                .id_salt("pal-swatches")
                .max_height(300.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                        for p in &palettes {
                            let is_current = current.as_deref() == Some(p.name.as_str());
                            let r = swatch(ui, p, is_current);
                            let r = r.on_hover_text(format!(
                                "{}: {} stops, background #{:02x}{:02x}{:02x}",
                                p.name,
                                p.stops.len(),
                                p.background_rgba()[0],
                                p.background_rgba()[1],
                                p.background_rgba()[2]
                            ));
                            if r.clicked() {
                                r.request_focus();
                                pick = Some(p.name.clone());
                            }
                            if r.hovered() || (r.has_focus() && want_preview.is_none()) {
                                want_preview = Some(p.name.clone());
                            }
                        }
                    });
                });
            ui.separator();
            ui.strong("Edit");
            ui.horizontal(|ui| {
                ui.label("Start from");
                egui::ComboBox::from_id_salt("pal-pick")
                    .selected_text("choose…")
                    .show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(false, n).clicked() {
                                self.pick_palette(n);
                            }
                        }
                    });
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.pal_name).desired_width(140.0));
            });
            // Gradient preview on the background.
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().min(480.0), 30.0),
                Sense::hover(),
            );
            ui.painter()
                .rect_filled(rect, 3.0, c32(self.pal_ui.background));
            let inner = rect.shrink(6.0);
            let packed: Vec<u32> = self
                .pal_stops
                .iter()
                .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
                .collect();
            for k in 0..64 {
                let c = render::ramp_stops(&packed, k as f64 / 63.0);
                let x0 = inner.left() + inner.width() * k as f32 / 64.0;
                let r = egui::Rect::from_min_max(
                    egui::pos2(x0, inner.top()),
                    egui::pos2(x0 + inner.width() / 64.0 + 0.5, inner.bottom()),
                );
                ui.painter()
                    .rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
            }
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.label("Stops");
                for (k, c) in self.pal_stops.iter_mut().enumerate() {
                    ui.vertical(|ui| {
                        egui::color_picker::color_edit_button_srgb(ui, c);
                        if ui
                            .small_button("x")
                            .on_hover_text("Remove this stop")
                            .clicked()
                        {
                            remove = Some(k);
                        }
                    });
                }
                if ui.button("+").on_hover_text("Add a stop").clicked() {
                    let last = self.pal_stops.last().copied().unwrap_or([255, 255, 255]);
                    self.pal_stops.push(last);
                }
            });
            if let Some(k) = remove
                && self.pal_stops.len() > 1
            {
                self.pal_stops.remove(k);
            }
            ui.horizontal(|ui| {
                ui.label("Background");
                egui::color_picker::color_edit_button_srgb(ui, &mut self.pal_ui.background)
                    .on_hover_text("Empty cells");
                ui.label("Stage");
                egui::color_picker::color_edit_button_srgb(ui, &mut self.pal_ui.stage)
                    .on_hover_text("Outside the tiling (and letterboxing in wallpapers)");
            });
            ui.label(
                RichText::new(
                    "The grid's background and stage come from the palette of the first CA.",
                )
                .small()
                .weak(),
            );
            ui.separator();
            ui.horizontal(|ui| {
                let builtin = Palette::builtin(&self.pal_name);
                let unchanged = builtin.as_ref() == Some(&self.edited_palette(&self.pal_name));
                let save_name = if builtin.is_some() {
                    format!("{} copy", self.pal_name)
                } else {
                    self.pal_name.trim().to_string()
                };
                let can_save = !self.pal_stops.is_empty() && !save_name.is_empty();
                if ui
                    .add_enabled(
                        can_save,
                        egui::Button::new(format!("Save as “{save_name}”")),
                    )
                    .on_hover_text("Add it to this scene's palettes")
                    .clicked()
                {
                    cmds.push(Command::SetPalette {
                        palette: self.edited_palette(&save_name),
                    });
                    self.pal_name = save_name.clone();
                }
                let target = match self.pal_ui.target {
                    Some(_) => "the CA",
                    None => "every CA",
                };
                if ui
                    .add_enabled(
                        can_save || unchanged,
                        egui::Button::new(format!("Apply to {target}")),
                    )
                    .clicked()
                {
                    let name = if unchanged {
                        self.pal_name.clone()
                    } else {
                        cmds.push(Command::SetPalette {
                            palette: self.edited_palette(&save_name),
                        });
                        save_name.clone()
                    };
                    self.pal_name = name.clone();
                    cmds.extend(self.apply_palette_cmds(&name));
                }
            });
        });
        self.pal_open = open;
        if !open {
            want_preview = None;
        }
        if let Some(name) = pick {
            // Apply: end the preview first so the command sees each CA's own palette.
            self.preview_palette(None);
            self.pick_palette(&name);
            cmds.extend(self.apply_palette_cmds(&name));
            want_preview = None;
        }
        if !cmds.is_empty() {
            self.preview_palette(None);
        }
        for c in cmds {
            self.exec(c);
        }
        self.preview_palette(want_preview.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_contrasts_with_the_background() {
        for b in render::PALETTES {
            let bg = [
                (b.background >> 16) as u8,
                (b.background >> 8) as u8,
                b.background as u8,
            ];
            let ink = ink_for(bg);
            let lum = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
            let d = (lum(bg) - lum([ink.r(), ink.g(), ink.b()])).abs();
            assert!(d > 90.0, "{}: contrast {d}", b.name);
        }
    }

    #[test]
    fn builtins_include_light_ones() {
        assert_eq!(render::PALETTES.len(), 30);
        for light in ["Candy", "Glacier", "Desert", "Paper"] {
            let p = Palette::builtin(light).unwrap();
            assert_eq!(
                ink_for(p.background.unwrap()),
                Color32::from_gray(30),
                "{light}"
            );
        }
    }
}
