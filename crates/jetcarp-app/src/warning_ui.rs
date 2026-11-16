//! The photosensitive seizure warning (#96): the app's first screen, before the setup wizard and
//! before anything animates, until it has been answered. It fills the window: a large heading,
//! the key sentence in bold, and two clearly different buttons. Answering that you have, or may
//! have, a seizure disorder leads to the advice not to use JETCARP, whose main button quits. The
//! text and the saved answer are `jetcarp_core::photosensitivity`'s, which the command line, the
//! background host and the MCP server read too.

use eframe::egui::{self, Color32, RichText};
use jetcarp_core::photosensitivity::{self as ps, Answer};

/// The warning's heading colour.
const AMBER: Color32 = Color32::from_rgb(0xff, 0xc1, 0x4d);
/// The answer that goes on.
const GO_ON: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xb8);
/// The advice's main button.
const QUIT: Color32 = Color32::from_rgb(0xb8, 0x3a, 0x2f);
/// The widest the text runs, in points, so lines stay easy to read.
const COLUMN: f32 = 720.0;
/// The paragraphs' colour, brighter than egui's usual text so the warning reads easily.
const TEXT: Color32 = Color32::from_gray(210);

/// Which page of the warning shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The warning and its question.
    #[default]
    Warning,
    /// The advice for someone who has, or may have, a seizure disorder.
    AtRisk,
}

/// What was chosen on the warning's screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// An answer that goes on, to be saved.
    Answered(Answer),
    /// Quit JETCARP.
    Quit,
}

/// The warning's state.
#[derive(Debug, Default)]
pub struct Warning {
    /// It shows, and nothing else does: no answer is saved in the settings folder.
    pub showing: bool,
    pub page: Page,
}

impl Warning {
    /// For the app: it shows unless an answer is saved.
    pub fn for_app() -> Warning {
        Warning {
            showing: !ps::accepted(),
            page: Page::Warning,
        }
    }

    /// The whole window: the warning, or the advice. Returns what was chosen, if anything.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Choice> {
        let mut choice = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_gray(18)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width().min(COLUMN);
                        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
                        ui.add_space(36.0);
                        ui.horizontal(|ui| {
                            ui.add_space(margin);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                choice = match self.page {
                                    Page::Warning => self.warning_ui(ui),
                                    Page::AtRisk => risk_ui(ui),
                                };
                            });
                        });
                        ui.add_space(24.0);
                    });
            });
        choice
    }

    fn warning_ui(&mut self, ui: &mut egui::Ui) -> Option<Choice> {
        bold(ui, ps::TITLE, 36.0, AMBER);
        ui.add_space(18.0);
        bold(ui, ps::KEY, 21.0, Color32::WHITE);
        for p in ps::DETAILS {
            ui.add_space(12.0);
            ui.label(RichText::new(p).size(16.0).color(TEXT));
        }
        ui.add_space(26.0);
        bold(ui, ps::QUESTION, 19.0, Color32::WHITE);
        ui.add_space(12.0);
        let size = egui::vec2(ui.available_width().min(520.0), 46.0);
        let no = egui::Button::new(
            RichText::new(ps::NO_DISORDER)
                .size(17.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(GO_ON)
        .min_size(size);
        if ui.add(no).clicked() {
            return Some(Choice::Answered(Answer::NoDisorder));
        }
        ui.add_space(10.0);
        let at_risk = egui::Button::new(RichText::new(ps::AT_RISK).size(17.0).color(AMBER))
            .fill(Color32::from_gray(30))
            .stroke(egui::Stroke::new(1.5, AMBER))
            .min_size(size);
        if ui.add(at_risk).clicked() {
            self.page = Page::AtRisk;
        }
        None
    }
}

/// The advice for someone at risk: Quit JETCARP, or, smaller, go on at their own risk.
fn risk_ui(ui: &mut egui::Ui) -> Option<Choice> {
    bold(
        ui,
        ps::RISK_TITLE,
        36.0,
        Color32::from_rgb(0xff, 0x8a, 0x7a),
    );
    ui.add_space(18.0);
    bold(ui, ps::RISK_KEY, 21.0, Color32::WHITE);
    for p in ps::RISK_DETAILS {
        ui.add_space(12.0);
        ui.label(RichText::new(p).size(16.0).color(TEXT));
    }
    ui.add_space(28.0);
    let quit = egui::Button::new(
        RichText::new(ps::QUIT)
            .size(20.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(QUIT)
    .min_size(egui::vec2(ui.available_width().min(320.0), 52.0));
    if ui.add(quit).clicked() {
        return Some(Choice::Quit);
    }
    ui.add_space(22.0);
    let go_on = egui::Button::new(
        RichText::new(ps::CONTINUE)
            .size(14.0)
            .color(Color32::from_gray(175)),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(egui::Stroke::new(1.0, Color32::from_gray(80)));
    if ui.add(go_on).clicked() {
        return Some(Choice::Answered(Answer::AtRisk));
    }
    None
}

/// Text in bold. egui's built-in font has one weight, so the text is drawn twice, the second time
/// a fraction of a point to the right, which thickens every stroke; screen readers get it as one
/// label.
fn bold(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    let shift = (size / 30.0).max(0.6);
    let galley = ui.painter().layout(
        text.to_string(),
        egui::FontId::proportional(size),
        color,
        ui.available_width() - shift,
    );
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + egui::vec2(shift, 0.0), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    let p = ui.painter();
    p.galley(rect.min, galley.clone(), color);
    p.galley(rect.min + egui::vec2(shift, 0.0), galley, color);
}
