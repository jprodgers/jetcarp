//! The CAs and Add panes, and the rule catalog menu. The palette window is in `palette_ui.rs`.

use super::*;

impl JetcarpApp {
    pub(super) fn cas_ui(&mut self, ui: &mut egui::Ui) {
        if self.world.cas.is_empty() {
            ui.label(
                RichText::new("None yet. Add one in the Add pane or CA → Add from catalog.").weak(),
            );
        }
        let mut cmds: Vec<Command> = Vec::new();
        let field_users = |w: &World, f: usize| w.cas.iter().filter(|c| c.field == f).count();
        let snapshot: Vec<world::CaInstance> = self.world.cas.clone();
        let n_cas = snapshot.len();
        for ca in &snapshot {
            let id = ca.id;
            let frame = egui::Frame::group(ui.style());
            let default_open = n_cas <= 2;
            frame.show(ui, |ui| {
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id(("ca-card", id)),
                    default_open,
                );
                state
                    .show_header(ui, |ui| {
                    let mut en = ca.enabled;
                    if ui.checkbox(&mut en, "").on_hover_text("Run this CA").changed() {
                        cmds.push(Command::UpdateCa { id, change: CaChange { enabled: Some(en), ..Default::default() } });
                    }
                    ui.label(RichText::new(&ca.name).strong());
                    if ca.notation() != ca.name {
                        ui.label(RichText::new(ca.notation()).monospace().weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").on_hover_text("Remove this CA").clicked() {
                            cmds.push(Command::RemoveCa { id });
                        }
                    });
                    })
                    .body(|ui| {
                // Rule
                ui.horizontal(|ui| {
                    ui.label("Rule");
                    let text = self.rule_text.entry(id).or_insert_with(|| ca.notation());
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0).font(egui::TextStyle::Monospace));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        cmds.push(Command::UpdateCa { id, change: CaChange { rule: Some(text.clone()), ..Default::default() } });
                    }
                    egui::ComboBox::from_id_salt(("preset", id)).selected_text("Named…").width(90.0).show_ui(ui, |ui| {
                        if let Some(i) = rule_menu(ui, None) {
                            cmds.push(Command::UpdateCa { id, change: CaChange { rule: Some(catalog()[i].notation.into()), ..Default::default() } });
                        }
                    });
                });
                // Elementary rules: step through rule numbers, export a contact sheet.
                if let Some((kind, n)) = jetcarp_core::sheet::SheetKind::of_rule(&ca.rule) {
                    ui.horizontal(|ui| {
                        ui.label("Rule number");
                        let mut m = n;
                        if ui.small_button("<").on_hover_text("Previous rule, restarted from its seed").clicked() && m > 0 {
                            m -= 1;
                        }
                        ui.add(egui::DragValue::new(&mut m).range(0..=kind.max()));
                        if ui.small_button(">").on_hover_text("Next rule, restarted from its seed").clicked() && m < kind.max() {
                            m += 1;
                        }
                        if m != n {
                            cmds.push(Command::UpdateCa { id, change: CaChange { rule: Some(kind.notation(m)), ..Default::default() } });
                            cmds.push(Command::Clear { id });
                            cmds.push(Command::Seed { id, seed: ca.seed.clone() });
                        }
                        if ui.small_button("Contact sheet…").on_hover_text("Export a labelled PNG of this rule and the ones after it").clicked() {
                            self.pending_sheet = Some((kind, n, ca.palette.clone(), matches!(ca.seed, SeedSpec::TopRandom { .. })));
                        }
                    });
                }
                if let Rule::Jaca(j) = &ca.rule {
                    self.jaca_ui(ui, ca, j, &mut cmds);
                }
                // Neighbourhood
                if !ca.rule.nb_fixed() {
                    ui.horizontal(|ui| {
                        ui.label("Neighbours");
                        for (nb, label, tip) in [
                            (Nb::Touch, "Touching", "Edge or corner contact (Moore on squares)"),
                            (Nb::Edge, "Edge", "Shared edge only (von Neumann on squares)"),
                        ] {
                            if ui.radio(ca.nb == nb, label).on_hover_text(tip).clicked() && ca.nb != nb {
                                cmds.push(Command::UpdateCa { id, change: CaChange { neighborhood: Some(nb), ..Default::default() } });
                            }
                        }
                    });
                }
                // Field and Affects others
                let f = &self.world.fields[ca.field];
                ui.horizontal(|ui| {
                    let mut aff = ca.affects_others;
                    let can = f.kind != FieldKind::Height;
                    let r = ui.add_enabled(can, egui::Checkbox::new(&mut aff, "Affects others")).on_hover_text(
                        "On: reads and writes the shared field, so CAs on it interact.\nOff: runs on a private field, drawn alongside.",
                    );
                    if r.changed() {
                        cmds.push(Command::UpdateCa { id, change: CaChange { affects_others: Some(aff), ..Default::default() } });
                    }
                    let users = field_users(&self.world, ca.field);
                    ui.label(
                        RichText::new(format!(
                            "field {} · {} {}{}",
                            ca.field,
                            if f.shared { "shared" } else { "private" },
                            f.kind.name(),
                            if users > 1 { format!(" · {users} CAs") } else { String::new() }
                        ))
                        .small()
                        .weak(),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Steps per generation");
                    let mut s = ca.steps_per_gen;
                    if ui.add(egui::DragValue::new(&mut s).range(1..=100_000).speed(0.5)).changed() {
                        cmds.push(Command::UpdateCa { id, change: CaChange { steps_per_gen: Some(s), ..Default::default() } });
                    }
                });
                if ca.rule == Rule::Sandpile {
                    ui.horizontal(|ui| {
                        ui.label("Grains dropped per generation");
                        let mut d = ca.drop_per_gen;
                        if ui.add(egui::DragValue::new(&mut d).range(0..=1_000_000)).changed() {
                            cmds.push(Command::UpdateCa { id, change: CaChange { drop_per_gen: Some(d), ..Default::default() } });
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        use jetcarp_core::world::SandpileOp;
                        if ui.small_button("+1 everywhere").on_hover_text("Add one grain to every cell").clicked() {
                            cmds.push(Command::Sandpile { id, action: SandpileOp::AddEverywhere { grains: 1 } });
                        }
                        if ui.small_button("+ random").on_hover_text(format!("Drop {} grains on random cells", self.random_grains)).clicked() {
                            cmds.push(Command::Sandpile { id, action: SandpileOp::AddRandom { count: self.random_grains } });
                        }
                        ui.add(egui::DragValue::new(&mut self.random_grains).range(1..=10_000_000).speed(50.0));
                    });
                    ui.horizontal_wrapped(|ui| {
                        use jetcarp_core::world::SandpileOp;
                        if ui.small_button("Stabilize").on_hover_text("Topple until stable, without counting generations").clicked() {
                            cmds.push(Command::Sandpile { id, action: SandpileOp::Stabilize });
                        }
                        if ui
                            .small_button("Identity")
                            .on_hover_text("Replace with the identity of the sandpile group (needs dead edges). Large grids take a few seconds.")
                            .clicked()
                        {
                            cmds.push(Command::Sandpile { id, action: SandpileOp::Identity });
                        }
                        ui.label(RichText::new("Drop at a point: Draw tool").small().weak());
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Colour by");
                    let modes = [
                        (ColorBy::Auto, "Auto"),
                        (ColorBy::Age, "Age"),
                        (ColorBy::State, "State"),
                        (ColorBy::Row, "Row"),
                    ];
                    let cur = modes.iter().find(|m| m.0 == ca.color_by).map(|m| m.1).unwrap_or("Auto");
                    egui::ComboBox::from_id_salt(("colorby", id)).selected_text(cur).width(70.0).show_ui(ui, |ui| {
                        for (m, label) in modes {
                            if ui.selectable_label(ca.color_by == m, label).clicked() && ca.color_by != m {
                                cmds.push(Command::UpdateCa { id, change: CaChange { color_by: Some(m), ..Default::default() } });
                            }
                        }
                    })
                    .response
                    .on_hover_text("Row: a gradient from top to bottom, for Wolfram rows or any tiling.");
                });
                ui.horizontal(|ui| {
                    ui.label(if matches!(ca.rule, Rule::Jaca(_)) { "+1 palette" } else { "Palette" });
                    let names = self.world.palette_names();
                    egui::ComboBox::from_id_salt(("pal", id)).selected_text(&ca.palette).width(80.0).show_ui(ui, |ui| {
                        for p in &names {
                            if ui.selectable_label(&ca.palette == p, p).clicked() {
                                cmds.push(Command::UpdateCa { id, change: CaChange { palette: Some(p.clone()), ..Default::default() } });
                            }
                        }
                    });
                    if ui.small_button("Seed").on_hover_text(format!("Apply this CA's seed: {:?}", ca.seed)).clicked() {
                        cmds.push(Command::Seed { id, seed: ca.seed.clone() });
                    }
                    if matches!(ca.rule, Rule::Jaca(_))
                        && let Some(c) = colour_button(ui, &self.world, &ca.palette, "Use one colour for +1 cells")
                    {
                        cmds.push(Command::UpdateCa { id, change: CaChange { palette: Some(c), ..Default::default() } });
                    }
                    if !ca.rule.is_agent() && !matches!(ca.rule, Rule::Wolfram(_) | Rule::Sandpile | Rule::WireWorld | Rule::VonNeumann)
                        && ui.small_button("Random").on_hover_text(if matches!(ca.rule, Rule::Jaca(_)) {
                            "Add random +1 cells (30%)"
                        } else {
                            "Add random live cells (30%)"
                        }).clicked() {
                            cmds.push(Command::Seed { id, seed: SeedSpec::Random { density: 0.3 } });
                        }
                    if ca.rule.is_agent() {
                        use jetcarp_core::world::AgentEdge;
                        let fall = ca.agent_edge == AgentEdge::FallOff;
                        if ui
                            .small_button(if fall { "Edge: fall off" } else { "Edge: bounce" })
                            .on_hover_text("What ants do at a dead or live edge. Click to switch.")
                            .clicked()
                        {
                            let e = if fall { AgentEdge::Bounce } else { AgentEdge::FallOff };
                            cmds.push(Command::UpdateCa { id, change: CaChange { agent_edge: Some(e), ..Default::default() } });
                        }
                    }
                    if ca.rule.is_agent() && ui.small_button("+ Ant").clicked() {
                        cmds.push(Command::Seed { id, seed: SeedSpec::Ants { count: 1 } });
                    }
                    if ui.small_button("Clear").on_hover_text("Clear this CA's field").clicked() {
                        cmds.push(Command::Clear { id });
                    }
                });
                    });
            });
        }
        let rule_changed = cmds
            .iter()
            .any(|c| matches!(c, Command::UpdateCa { change, .. } if change.rule.is_some()));
        for c in cmds {
            self.exec(c);
        }
        if rule_changed {
            self.rule_text.clear();
            self.jaca_text.clear();
        }
    }

    /// JACA controls: a B/S rule per sign, blast radius, the -1 palette, glow, seeds, counts.
    fn jaca_ui(
        &mut self,
        ui: &mut egui::Ui,
        ca: &world::CaInstance,
        j: &jetcarp_core::jaca::JacaRule,
        cmds: &mut Vec<Command>,
    ) {
        use jetcarp_core::jaca::JacaRule;
        let id = ca.id;
        let edge = ca.nb == Nb::Edge;
        let set_rule = |cmds: &mut Vec<Command>, r: &JacaRule| {
            cmds.push(Command::UpdateCa {
                id,
                change: CaChange {
                    rule: Some(r.notation_with(edge)),
                    ..Default::default()
                },
            });
        };
        egui::Grid::new(("jaca", id)).num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
            for (sign, label) in [(true, "+1 rule"), (false, "−1 rule")] {
                ui.label(label);
                let key = (id, sign);
                let text = self.jaca_text.entry(key).or_insert_with(|| if sign { j.pos_bs() } else { j.neg_bs() });
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(110.0).font(egui::TextStyle::Monospace))
                    .on_hover_text("A B/S rule, counting only neighbours of this sign. Press Enter to apply.");
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    match Rule::parse(text.trim()) {
                        Ok((Rule::LifeLike { birth, survive }, _)) if birth & 1 == 0 => {
                            let mut n = j.clone();
                            if sign {
                                (n.pos_birth, n.pos_survive) = (birth, survive);
                            } else {
                                (n.neg_birth, n.neg_survive) = (birth, survive);
                            }
                            set_rule(cmds, &n);
                        }
                        Ok(_) => self.message = Some(("JACA rules are two-state B/S rules without B0, like B3/S23".into(), true)),
                        Err(e) => self.message = Some((e, true)),
                    }
                }
                ui.end_row();
            }
            ui.label("Blast radius");
            ui.horizontal(|ui| {
                let mut b = j.blast;
                if ui
                    .add(egui::DragValue::new(&mut b).range(0..=jetcarp_core::jaca::MAX_BLAST))
                    .on_hover_text("0: only the touching cells annihilate. N: every cell within N neighbourhood steps of them is destroyed too.")
                    .changed()
                {
                    let mut n = j.clone();
                    n.blast = b;
                    set_rule(cmds, &n);
                }
                if !j.is_symmetric() && ui.small_button("Swap").on_hover_text("Swap the +1 and −1 rules").clicked() {
                    let mut n = j.clone();
                    std::mem::swap(&mut n.pos_birth, &mut n.neg_birth);
                    std::mem::swap(&mut n.pos_survive, &mut n.neg_survive);
                    set_rule(cmds, &n);
                }
            });
            ui.end_row();
            ui.label("−1 palette");
            ui.horizontal(|ui| {
                let names = self.world.palette_names();
                egui::ComboBox::from_id_salt(("palneg", id)).selected_text(&ca.palette_neg).width(80.0).show_ui(ui, |ui| {
                    for p in &names {
                        if ui.selectable_label(&ca.palette_neg == p, p).clicked() {
                            cmds.push(Command::UpdateCa { id, change: CaChange { palette_neg: Some(p.clone()), ..Default::default() } });
                        }
                    }
                });
                if let Some(c) = colour_button(ui, &self.world, &ca.palette_neg, "Use one colour for −1 cells") {
                    cmds.push(Command::UpdateCa { id, change: CaChange { palette_neg: Some(c), ..Default::default() } });
                }
                let mut g = ca.glow;
                if ui.checkbox(&mut g, "Glow").on_hover_text("Annihilated cells flash and fade over a few generations").changed() {
                    cmds.push(Command::UpdateCa { id, change: CaChange { glow: Some(g), ..Default::default() } });
                }
            });
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Seed");
            if ui
                .small_button("Halves")
                .on_hover_text(
                    "Clear, then +1 soup on the left half and −1 soup on the right (30%)",
                )
                .clicked()
            {
                cmds.push(Command::Clear { id });
                cmds.push(Command::Seed {
                    id,
                    seed: SeedSpec::Halves { density: 0.3 },
                });
            }
            if ui
                .small_button("Mixed")
                .on_hover_text("Add random cells (30%), half +1 and half −1")
                .clicked()
            {
                cmds.push(Command::Seed {
                    id,
                    seed: SeedSpec::Signed {
                        density: 0.3,
                        negative: 0.5,
                    },
                });
            }
            if ui
                .small_button("−1 random")
                .on_hover_text("Add random −1 cells (30%)")
                .clicked()
            {
                cmds.push(Command::Seed {
                    id,
                    seed: SeedSpec::Signed {
                        density: 0.3,
                        negative: 1.0,
                    },
                });
            }
        });
        let f = &self.world.fields[ca.field];
        let pos = f
            .state
            .iter()
            .filter(|&&s| s == jetcarp_core::jaca::POS)
            .count();
        let neg = f
            .state
            .iter()
            .filter(|&&s| s == jetcarp_core::jaca::NEG)
            .count();
        let h = ca.heat.as_deref().cloned().unwrap_or_default();
        ui.label(
            RichText::new(format!(
                "+1: {pos} · −1: {neg} · last step {} touching, {} destroyed · total {} destroyed",
                h.last.contacts, h.last.destroyed, h.total_destroyed
            ))
            .small()
            .weak(),
        );
    }

    pub(super) fn add_ui(&mut self, ui: &mut egui::Ui) {
        if ui
            .small_button("Load rule table…")
            .on_hover_text("A JETCARP rule table (JSON). See README: Rule tables.")
            .clicked()
        {
            self.load_table();
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("add")
                .selected_text(catalog()[self.add_choice].name)
                .width(190.0)
                .show_ui(ui, |ui| {
                    if let Some(i) = rule_menu(ui, Some(self.add_choice)) {
                        self.add_choice = i;
                    }
                });
            if ui.button("Add").clicked() {
                let spec = CaSpec::new(catalog()[self.add_choice].notation);
                if let Some(a) = self.exec(Command::AddCa { ca: spec }) {
                    self.draw_target = a.new_id;
                }
            }
        });
        ui.label(
            RichText::new(catalog()[self.add_choice].blurb)
                .small()
                .weak(),
        );
        ui.horizontal(|ui| {
            ui.label("or notation");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.add_text)
                    .hint_text("B36/S23, W90, Ant RLR")
                    .desired_width(130.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || enter)
                && !self.add_text.trim().is_empty()
                && let Some(a) = self.exec(Command::AddCa {
                    ca: CaSpec::new(self.add_text.trim()),
                })
            {
                self.draw_target = a.new_id;
                self.add_text.clear();
            }
        });
    }
}

/// A colour button that picks one solid colour, returned as a `#rrggbb` palette name.
fn colour_button(ui: &mut egui::Ui, w: &World, current: &str, tip: &str) -> Option<String> {
    let first = w
        .palette(current)
        .stops
        .first()
        .copied()
        .unwrap_or([0xff, 0xff, 0xff]);
    let mut c = first;
    let r = ui.color_edit_button_srgb(&mut c).on_hover_text(tip);
    (r.changed() && c != first).then(|| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
}

/// Named rules grouped by family (families in catalog order, rules by name), one submenu each.
/// Returns the catalog index picked.
pub(super) fn rule_menu(ui: &mut egui::Ui, current: Option<usize>) -> Option<usize> {
    let cat = catalog();
    let mut fams: Vec<&str> = Vec::new();
    for e in cat {
        if !fams.contains(&e.family) {
            fams.push(e.family);
        }
    }
    let mut picked = None;
    for f in fams {
        let mut idx: Vec<usize> = (0..cat.len()).filter(|&i| cat[i].family == f).collect();
        idx.sort_by_key(|&i| cat[i].name.to_lowercase());
        ui.menu_button(format!("{f} ({})", idx.len()), |ui| {
            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    for i in idx {
                        let e = &cat[i];
                        let mut r = ui.selectable_label(
                            current == Some(i),
                            format!("{}  {}", e.name, e.notation),
                        );
                        if !e.blurb.is_empty() {
                            r = r.on_hover_text(e.blurb);
                        }
                        if r.clicked() {
                            picked = Some(i);
                            ui.close();
                        }
                    }
                });
        });
    }
    picked
}
