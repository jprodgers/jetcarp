//! The Tiling pane: family, size, edges, catalog picker, tile JSON.

use super::*;

impl JetcarpApp {
    pub(super) fn tiling_spec_from_ui(&self, keep_shape: bool) -> TilingSpec {
        let mut spec = TilingSpec::new(&self.t_family);
        if keep_shape && self.world.topo.spec.family == self.t_family {
            spec.params = self.world.topo.spec.params.clone();
            spec.edges = self.world.topo.spec.edges.clone();
        }
        let aperiodic = jetcarp_core::families::family(&self.t_family)
            .is_some_and(|f| f.kind == FamilyKind::Aperiodic);
        spec.boundary = if aperiodic {
            Boundary::Dead
        } else {
            self.t_boundary
        };
        spec.size = Some(match self.t_size {
            _ if aperiodic => SizeSpec::Level(match self.t_size {
                SizeChoice::Level(l) => l,
                _ => aperiodic::default_level(&self.t_family),
            }),
            SizeChoice::Cells(c) => SizeSpec::Cells(c),
            SizeChoice::Level(_) => SizeSpec::Cells(900),
            SizeChoice::Fill => self.fill_size(&spec),
        });
        spec
    }

    /// Lattice dimensions that fill the canvas (the grid tile, the full-screen window, or the
    /// outputs' span) at the cell size in physical pixels.
    pub(super) fn fill_size(&self, spec: &TilingSpec) -> SizeSpec {
        let c = self.canvas_px();
        display::fill_size(spec, c[0], c[1], self.shell.display.cell_px as f64)
    }

    /// The size choice after switching to `t_family`: its default level for aperiodic
    /// families, Fill if that was chosen, else about 900 cells.
    fn size_for_new_family(&self) -> SizeChoice {
        let aperiodic = jetcarp_core::families::family(&self.t_family)
            .is_some_and(|f| f.kind == FamilyKind::Aperiodic);
        if aperiodic {
            SizeChoice::Level(aperiodic::default_level(&self.t_family))
        } else if self.t_size == SizeChoice::Fill {
            SizeChoice::Fill
        } else {
            SizeChoice::Cells(900)
        }
    }

    /// Switch to another tiling family (Tiling → Family).
    pub(super) fn pick_family(&mut self, id: &str) {
        self.t_family = id.into();
        self.t_size = self.size_for_new_family();
        self.apply_tiling_ui(false);
    }

    pub(super) fn apply_tiling_ui(&mut self, keep_shape: bool) {
        let spec = self.tiling_spec_from_ui(keep_shape);
        self.set_tiling_bg(spec);
    }

    /// Catalog picker (k-uniform and Archimedean tilings). True when a tiling was picked.
    pub(super) fn catalog_picker_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let spec = &self.world.topo.spec;
        let current = (spec.family == "periodic")
            .then(|| spec.params.as_ref()?.get("catalog")?.as_str())
            .flatten()
            .map(String::from);
        let Some(id) = crate::catalog_picker::show(ui, current.as_deref()) else {
            return false;
        };
        self.t_family = "periodic".into();
        if matches!(self.t_size, SizeChoice::Level(_)) {
            self.t_size = SizeChoice::Cells(900);
        }
        let mut spec = self.tiling_spec_from_ui(false);
        spec.params = Some(serde_json::json!({ "catalog": id }));
        if self.t_size == SizeChoice::Fill {
            spec.size = Some(self.fill_size(&spec));
        }
        self.exec(Command::SetTiling { tiling: spec });
        true
    }

    pub(super) fn tiling_ui(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let mut family_changed = false;
        let fam_name = jetcarp_core::families::family(&self.t_family)
            .map(|f| f.name)
            .unwrap_or("?");
        egui::ComboBox::from_id_salt("family")
            .selected_text(fam_name)
            .width(240.0)
            .show_ui(ui, |ui| {
                let mut group = "";
                for f in FAMILIES {
                    if f.group != group {
                        group = f.group;
                        ui.label(RichText::new(group).weak().small());
                    }
                    if ui
                        .selectable_label(self.t_family == f.id, format!("{}  ({})", f.name, f.sym))
                        .clicked()
                        && self.t_family != f.id
                    {
                        self.t_family = f.id.into();
                        family_changed = true;
                    }
                }
                if let Some(id) = crate::isohedral_ui::combo_entry(ui, &self.t_family) {
                    self.t_family = id.into();
                    family_changed = true;
                }
            });
        let spec_params = (self.world.topo.spec.family == self.t_family)
            .then_some(self.world.topo.spec.params.as_ref())
            .flatten();
        match crate::isohedral_ui::controls(ui, &self.t_family, spec_params) {
            Some(crate::isohedral_ui::IhChange::Type(id)) => {
                self.t_family = id.into();
                family_changed = true;
            }
            Some(crate::isohedral_ui::IhChange::Params(ev)) if ev.finished => {
                let mut spec = self.tiling_spec_from_ui(true);
                spec.params = Some(ev.params);
                self.exec(Command::SetTiling { tiling: spec });
            }
            _ => {}
        }
        if self.catalog_picker_ui(ui) {
            return;
        }
        let aperiodic = jetcarp_core::families::family(&self.t_family)
            .is_some_and(|f| f.kind == FamilyKind::Aperiodic);
        if family_changed {
            self.t_size = self.size_for_new_family();
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.label("Size");
            let text = match self.t_size {
                SizeChoice::Level(l) => level_label(&self.t_family, l),
                SizeChoice::Cells(c) => CELL_PRESETS
                    .iter()
                    .find(|p| p.0 == c)
                    .map(|p| p.1.to_string())
                    .unwrap_or(format!("About {c}")),
                SizeChoice::Fill => "Fill the view".into(),
            };
            egui::ComboBox::from_id_salt("size")
                .selected_text(text)
                .show_ui(ui, |ui| {
                    if aperiodic {
                        for &(l, _) in aperiodic::levels(&self.t_family) {
                            let t = level_label(&self.t_family, l);
                            changed |= ui
                                .selectable_value(&mut self.t_size, SizeChoice::Level(l), t)
                                .changed();
                        }
                    } else {
                        for (c, t) in CELL_PRESETS {
                            changed |= ui
                                .selectable_value(&mut self.t_size, SizeChoice::Cells(c), t)
                                .changed();
                        }
                        changed |= ui
                            .selectable_value(&mut self.t_size, SizeChoice::Fill, "Fill the view")
                            .changed();
                    }
                });
        });
        if self.t_size == SizeChoice::Fill && !aperiodic {
            ui.horizontal(|ui| {
                ui.label("Cell size");
                let r = ui
                    .add(
                        egui::Slider::new(&mut self.shell.display.cell_px, display::CELL_PX_RANGE)
                            .logarithmic(true)
                            .integer()
                            .suffix(" px"),
                    )
                    .on_hover_text(
                        "Physical pixels per cell edge; Fill covers the canvas (this view, the full screen, or the outputs)",
                    );
                changed |= r.drag_stopped() || (r.changed() && !r.dragged());
            });
        }
        ui.add_enabled_ui(!aperiodic, |ui| {
            ui.horizontal(|ui| {
                ui.label("Edges");
                changed |= ui
                    .radio_value(&mut self.t_boundary, Boundary::Dead, "Dead")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.t_boundary, Boundary::Wrap, "Wrap")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.t_boundary, Boundary::Live, "Live")
                    .on_hover_text("Cells past the edge count as live for B/S, Generations, elementary, and table rules")
                    .changed();
            });
        });
        if changed {
            self.apply_tiling_ui(!family_changed);
        }
        let (e, t) = &self.degrees;
        ui.label(
            RichText::new(format!(
                "{} cells · {} storage · neighbours: {:?} by edge, {:?} touching",
                self.world.topo.len(),
                self.world.topo.storage_name(),
                e,
                t
            ))
            .small(),
        );
        for w in &self.world.topo.warnings {
            ui.colored_label(Color32::YELLOW, w);
        }
        ui.horizontal(|ui| {
            if ui
                .button("Edit tile…")
                .on_hover_text("Open the tile editor on the current tiling")
                .clicked()
            {
                self.set_tool_open(menu::ToolWin::TileEditor, true);
            }
            if !aperiodic
                && ui
                    .button("Edit unit cell…")
                    .on_hover_text(
                        "Edit the lattice and all tiles of one cell: mixed tiles, any contact",
                    )
                    .clicked()
            {
                self.set_tool_open(menu::ToolWin::CellEditor, true);
            }
        });
        egui::CollapsingHeader::new("Paste tile JSON from the prototype").show(ui, |ui| {
            ui.label(
                RichText::new("In the prototype, click “Copy tile JSON”, then paste it here.")
                    .small(),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.tile_json)
                    .desired_rows(5)
                    .code_editor()
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Load tile").clicked() {
                match serde_json::from_str::<TilingSpec>(&self.tile_json) {
                    Ok(mut spec) => {
                        let aperiodic = jetcarp_core::families::family(&spec.family)
                            .is_some_and(|f| f.kind == FamilyKind::Aperiodic);
                        self.t_family = spec.family.clone();
                        if spec.size.is_none() {
                            spec.size = Some(if aperiodic {
                                SizeSpec::Level(
                                    spec.params
                                        .as_ref()
                                        .and_then(|p| p.get("level"))
                                        .and_then(|l| l.as_u64())
                                        .unwrap_or(4) as u32,
                                )
                            } else {
                                SizeSpec::Cells(2500)
                            });
                        }
                        spec.boundary = if aperiodic {
                            Boundary::Dead
                        } else {
                            self.t_boundary
                        };
                        if self.exec(Command::SetTiling { tiling: spec }).is_some() {
                            self.message = Some(("Tile loaded.".into(), false));
                        }
                    }
                    Err(e) => self.message = Some((format!("Tile JSON: {e}"), true)),
                }
            }
        });
    }
}

/// "Level 4 (4,000 tiles)" for an aperiodic family's size choice.
pub(super) fn level_label(family: &str, l: u32) -> String {
    match aperiodic::tile_count(family, l) {
        Some(n) => {
            let d = n.to_string();
            let mut t = String::new();
            for (i, ch) in d.chars().enumerate() {
                if i > 0 && (d.len() - i) % 3 == 0 {
                    t.push(',');
                }
                t.push(ch);
            }
            format!("Level {l} ({t} tiles)")
        }
        None => format!("Level {l}"),
    }
}
