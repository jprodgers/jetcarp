//! The Sequence pane: a scene's script of steps and triggers.

use super::*;

impl JetcarpApp {
    pub(super) fn sequence_ui(&mut self, ui: &mut egui::Ui) {
        use jetcarp_core::script::{CaRef, Script, ScriptStep, Trigger};
        let title = match &self.world.script {
            Some(s) if !s.steps.is_empty() => {
                let st = &self.world.script_state;
                if st.finished {
                    "Sequence (finished)".to_string()
                } else {
                    format!("Sequence: step {} of {}", st.step + 1, s.steps.len())
                }
            }
            _ => "Sequence".to_string(),
        };
        let mut cmds = Vec::new();
        ui.scope(|ui| {
                ui.label(RichText::new(title).strong());
                ui.label(
                    RichText::new(
                        "A sequence of steps saved with the scene: each step chooses which CAs run, and its trigger moves on to the next. (Rhai scripts and sweeps are under Tools → Scripts and sweeps.)",
                    )
                    .small()
                    .weak(),
                );
                let cas: Vec<(u32, String)> = self
                    .world
                    .cas
                    .iter()
                    .map(|c| (c.id, c.name.clone()))
                    .collect();
                if self.script_edit.is_none() {
                    if ui.button("New sequence").clicked() {
                        self.script_edit = Some(Script {
                            steps: vec![ScriptStep {
                                name: "Step 1".into(),
                                run: cas.iter().map(|c| CaRef::Id(c.0)).collect(),
                                until: Trigger::Generations { n: 100 },
                                max_gens: None,
                            }],
                            repeat: false,
                        });
                        self.script_dirty = true;
                    }
                    return;
                }
                let current = self
                    .world
                    .script
                    .as_ref()
                    .map(|_| self.world.script_state.step);
                let live = self.world.script == self.script_edit;
                let mut dirty = false;
                let sc = self.script_edit.as_mut().unwrap();
                let mut remove = None;
                let mut swap = None;
                let n_steps = sc.steps.len();
                for (k, step) in sc.steps.iter_mut().enumerate() {
                    let here = live && current == Some(k) && !self.world.script_state.finished;
                    let frame = if here {
                        egui::Frame::group(ui.style())
                            .stroke(egui::Stroke::new(1.5, Color32::from_rgb(0xff, 0x8a, 0x3c)))
                    } else {
                        egui::Frame::group(ui.style())
                    };
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}.", k + 1)).strong());
                            dirty |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut step.name).desired_width(140.0),
                                )
                                .changed();
                            if live
                                && ui
                                    .small_button("Go")
                                    .on_hover_text("Jump to this step")
                                    .clicked()
                            {
                                cmds.push(Command::ScriptGoto { step: k });
                            }
                            if k > 0 && ui.small_button("^").on_hover_text("Move up").clicked() {
                                swap = Some(k);
                            }
                            if ui.small_button("x").on_hover_text("Remove step").clicked() {
                                remove = Some(k);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Run");
                            for (id, name) in &cas {
                                let mut on = step.run.iter().any(|r| match r {
                                    CaRef::Id(x) => x == id,
                                    CaRef::Name(n) => n.eq_ignore_ascii_case(name),
                                });
                                if ui.checkbox(&mut on, name.as_str()).changed() {
                                    step.run.retain(|r| match r {
                                        CaRef::Id(x) => x != id,
                                        CaRef::Name(n) => !n.eq_ignore_ascii_case(name),
                                    });
                                    if on {
                                        step.run.push(CaRef::Id(*id));
                                    }
                                    dirty = true;
                                }
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Until");
                            let kinds = [
                                "generations",
                                "population ≤",
                                "population ≥",
                                "activity below",
                                "activity above",
                                "repeats",
                                "never",
                                "done",
                            ];
                            let cur = match step.until {
                                Trigger::Generations { .. } => 0,
                                Trigger::PopulationBelow { .. } => 1,
                                Trigger::PopulationAbove { .. } => 2,
                                Trigger::ActivityBelow { .. } => 3,
                                Trigger::ActivityAbove { .. } => 4,
                                Trigger::Repeats { .. } => 5,
                                Trigger::Never => 6,
                                Trigger::Done => 7,
                            };
                            let mut sel = cur;
                            egui::ComboBox::from_id_salt(("trig", k))
                                .selected_text(kinds[cur])
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    for (i, t) in kinds.iter().enumerate() {
                                        ui.selectable_value(&mut sel, i, *t);
                                    }
                                });
                            if sel != cur {
                                step.until = match sel {
                                    0 => Trigger::Generations { n: 100 },
                                    1 => Trigger::PopulationBelow { n: 10 },
                                    2 => Trigger::PopulationAbove { n: 1000 },
                                    3 => Trigger::ActivityBelow {
                                        fraction: 0.002,
                                        for_gens: 20,
                                    },
                                    4 => Trigger::ActivityAbove {
                                        fraction: 0.05,
                                        for_gens: 5,
                                    },
                                    5 => Trigger::Repeats { max_period: 2 },
                                    7 => Trigger::Done,
                                    _ => Trigger::Never,
                                };
                                dirty = true;
                            }
                            match &mut step.until {
                                Trigger::Generations { n }
                                | Trigger::PopulationBelow { n }
                                | Trigger::PopulationAbove { n } => {
                                    dirty |= ui
                                        .add(egui::DragValue::new(n).range(0..=u64::MAX))
                                        .changed();
                                }
                                Trigger::ActivityBelow { fraction, for_gens }
                                | Trigger::ActivityAbove { fraction, for_gens } => {
                                    let mut pct = *fraction * 100.0;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut pct)
                                                .range(0.0..=100.0)
                                                .speed(0.01)
                                                .suffix("%"),
                                        )
                                        .changed()
                                    {
                                        *fraction = pct / 100.0;
                                        dirty = true;
                                    }
                                    ui.label("for");
                                    dirty |= ui
                                        .add(
                                            egui::DragValue::new(for_gens)
                                                .range(1..=100_000)
                                                .suffix(" gens"),
                                        )
                                        .changed();
                                }
                                Trigger::Repeats { max_period } => {
                                    ui.label("period ≤");
                                    dirty |= ui
                                        .add(egui::DragValue::new(max_period).range(1..=100_000))
                                        .changed();
                                }
                                Trigger::Never | Trigger::Done => {}
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut limit = step.max_gens.is_some();
                            if ui
                                .checkbox(&mut limit, "Limit")
                                .on_hover_text("Move on after this many generations regardless")
                                .changed()
                            {
                                step.max_gens = if limit { Some(500) } else { None };
                                dirty = true;
                            }
                            if let Some(m) = &mut step.max_gens {
                                dirty |= ui
                                    .add(
                                        egui::DragValue::new(m).range(1..=u64::MAX).suffix(" gens"),
                                    )
                                    .changed();
                            }
                        });
                    });
                }
                if let Some(k) = remove {
                    sc.steps.remove(k);
                    dirty = true;
                }
                if let Some(k) = swap {
                    sc.steps.swap(k - 1, k);
                    dirty = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("+ Step").clicked() {
                        sc.steps.push(ScriptStep {
                            name: format!("Step {}", n_steps + 1),
                            run: vec![],
                            until: Trigger::Generations { n: 100 },
                            max_gens: None,
                        });
                        dirty = true;
                    }
                    dirty |= ui.checkbox(&mut sc.repeat, "Repeat").changed();
                });
                self.script_dirty |= dirty;
                ui.horizontal(|ui| {
                    let label = if live {
                        "Restart sequence"
                    } else {
                        "Apply and start"
                    };
                    if ui.button(RichText::new(label).strong()).clicked() {
                        cmds.push(Command::SetScript {
                            script: self.script_edit.clone(),
                        });
                    }
                    if ui.button("Remove sequence").clicked() {
                        cmds.push(Command::SetScript { script: None });
                        self.script_edit = None;
                    }
                });
                if !live && self.script_edit.is_some() {
                    ui.colored_label(Color32::YELLOW, "Edited: apply to use these steps.");
                }
                if live
                    && let (Some(s), false) = (&self.world.script, self.world.script_state.finished)
                {
                    let st = &self.world.script_state;
                    let step = &s.steps[st.step];
                    let mut line = format!(
                        "Now: {} · {} gens in · {}",
                        step.name,
                        self.world.generation - st.started,
                        step.until.describe()
                    );
                    if let Some(a) = st.last_activity {
                        line.push_str(&format!(" · activity {:.3}%", a * 100.0));
                    }
                    ui.label(RichText::new(line).small());
                }
            });
        for c in cmds {
            let set = matches!(c, Command::SetScript { .. });
            self.exec(c);
            if set {
                self.script_edit = self.world.script.clone();
                self.script_dirty = false;
            }
        }
    }
}
