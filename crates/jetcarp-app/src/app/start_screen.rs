//! The Quick Start screen.

use super::*;

impl JetcarpApp {
    // ---------------------------------------------------------------- UI

    pub(super) fn start_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        let mut open: Option<start::MenuAction> = None;
        let mut blank = false;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("JETCARP").size(30.0).strong());
                ui.label(
                    RichText::new("cellular automata research tool")
                        .size(16.0)
                        .weak(),
                );
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Quick Start").size(20.0).strong());
                ui.label("Pick a running example to open it, or start from an");
                if ui.button(RichText::new("Empty world").size(16.0)).clicked() {
                    blank = true;
                }
                ui.add_space(12.0);
                if ui.button("Open scene…").clicked() {
                    self.open_scene();
                }
                ui.add_space(12.0);
                if ui
                    .button("Scripts and sweeps…")
                    .on_hover_text("Run every rule of a family and save a picture of each, or write a Rhai script")
                    .clicked()
                {
                    blank = true;
                    self.scripts.open = true;
                }
                if ui.button("Sweep results…").clicked() {
                    blank = true;
                    self.results.open = true;
                }
                if ui.button("Pattern library…").clicked() {
                    blank = true;
                    self.library.open = true;
                    self.library.reload();
                }
                let busy = if self.dash.busy() { " ●" } else { "" };
                if ui
                    .button(format!("Hunts{busy}…"))
                    .on_hover_text("Every hunt and search job in the catalog: progress, heat map, finds")
                    .clicked()
                {
                    self.dash.open = !self.dash.open;
                }
            });
            if let Some((m, err)) = &self.message {
                ui.colored_label(
                    if *err {
                        Color32::LIGHT_RED
                    } else {
                        Color32::LIGHT_GREEN
                    },
                    m,
                );
            }
            ui.add_space(4.0);
            open = self.menu.show(ui);
        });
        match open {
            Some(start::MenuAction::Open(i)) => {
                self.menu.note_open(i);
                match self.menu.full_scene(i) {
                    Ok(s) => self.load_scene_bg(&s),
                    Err(e) => self.message = Some((e, true)),
                }
            }
            Some(start::MenuAction::OpenPreview(i)) => {
                self.menu.note_open(i);
                match self.menu.preview_scene(i) {
                    Some(s) => self.load_scene_bg(&s),
                    None => self.message = Some(("no preview for this entry".into(), true)),
                }
            }
            None => {}
        }
        if blank {
            self.empty_world();
        }
    }
}
