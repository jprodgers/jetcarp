//! A compact palette picker shared by every tool window: a combo box whose entries show each
//! palette's stops on its own background, so light palettes look right.
//!
//! - [`picker`]: choose one palette by name (built-ins plus any `extra` palettes, e.g. a scene's).
//! - [`picker_opt`]: the same with a "none" entry first (`None` = the caller's default, such as
//!   "the scene's own palette" or "house style").

use jetcarp_core::render::{self, Palette};
use eframe::egui::{self, Color32, Sense};
use serde::{Deserialize, Serialize};

/// Each tool window's palette choice, saved in the app settings (`tool_palettes`). `None` is
/// the tool's own default (named beside each field).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ToolPalettes {
    /// Large sandpile: the height colours (default Ember) and their direction.
    pub sandpile: String,
    pub sandpile_reverse: bool,
    /// Ants: media and opened runs (`None`: the house style; runs open in Prism).
    pub ants: Option<String>,
    /// HashLife view (`None`: the world's palette).
    pub hashlife: Option<String>,
    /// Pattern library thumbnails and phase strips (`None`: the rule's own colours).
    pub library: Option<String>,
    /// Hunts dashboard finds and its *Render media* (`None`: the house style).
    pub hunts: Option<String>,
}

impl Default for ToolPalettes {
    fn default() -> Self {
        ToolPalettes {
            sandpile: "Ember".into(),
            sandpile_reverse: false,
            ants: None,
            hashlife: None,
            library: None,
            hunts: None,
        }
    }
}

impl ToolPalettes {
    /// Unknown names (a palette removed since, a hand-edited file) fall back to the defaults.
    pub fn sanitize(mut self) -> ToolPalettes {
        let ok = |n: &Option<String>| n.as_deref().is_none_or(|n| Palette::builtin(n).is_some());
        if Palette::builtin(&self.sandpile).is_none() {
            self.sandpile = "Ember".into();
        }
        for f in [&mut self.ants, &mut self.library, &mut self.hunts] {
            if !ok(f) {
                *f = None;
            }
        }
        // The HashLife view may name a world's own palette (drawn with the world's palette
        // when the loaded world lacks it); keep it.
        self
    }
}

/// Every palette offered: the built-ins, then `extra` palettes not already listed by name.
pub fn all_palettes(extra: &[Palette]) -> Vec<Palette> {
    let mut v: Vec<Palette> = render::palette_names()
        .filter_map(Palette::builtin)
        .collect();
    for p in extra {
        if !v.iter().any(|q| q.name == p.name) {
            v.push(p.clone());
        }
    }
    v
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn rgb(c: render::Rgba) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// A strip of the palette's stops on its background, `w` × `h` points, at `rect`.
pub fn paint_strip(painter: &egui::Painter, rect: egui::Rect, p: &Palette) {
    painter.rect_filled(rect, 2.0, c32(rgb(p.background_rgba())));
    let n = p.stops.len().max(1);
    let inner = rect.shrink(2.0);
    let w = inner.width() / n as f32;
    for (k, c) in p.stops.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            egui::pos2(inner.left() + k as f32 * w, inner.top()),
            egui::vec2((w - 1.0).max(1.0), inner.height()),
        );
        painter.rect_filled(r, 1.0, c32(*c));
    }
}

/// One selectable row: strip plus name. True when clicked.
fn row(ui: &mut egui::Ui, p: Option<&Palette>, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(210.0, 20.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        if selected || resp.hovered() {
            painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let strip =
            egui::Rect::from_min_size(rect.min + egui::vec2(2.0, 3.0), egui::vec2(70.0, 14.0));
        match p {
            Some(p) => paint_strip(&painter, strip, p),
            None => {
                painter.rect_stroke(
                    strip,
                    2.0,
                    egui::Stroke::new(1.0, Color32::from_gray(110)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        painter.text(
            egui::pos2(strip.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            if selected {
                Color32::from_rgb(255, 200, 80)
            } else {
                ui.visuals().text_color()
            },
        );
    }
    resp.clicked()
}

/// Choose a palette by name. True when the choice changed.
pub fn picker(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &mut String,
    extra: &[Palette],
) -> bool {
    let mut opt = Some(current.clone());
    let changed = picker_inner(ui, id_salt, &mut opt, extra, None);
    if let Some(n) = opt {
        *current = n;
    }
    changed
}

/// Choose a palette or none (`none_label`, e.g. "Scene's own"). True when the choice changed.
pub fn picker_opt(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &mut Option<String>,
    extra: &[Palette],
    none_label: &str,
) -> bool {
    picker_inner(ui, id_salt, current, extra, Some(none_label))
}

fn picker_inner(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &mut Option<String>,
    extra: &[Palette],
    none_label: Option<&str>,
) -> bool {
    let all = all_palettes(extra);
    let chosen = current
        .as_ref()
        .and_then(|n| all.iter().find(|p| &p.name == n));
    let text = match (current.as_deref(), none_label) {
        (None, Some(l)) => l.to_string(),
        (Some(n), _) => n.to_string(),
        (None, None) => String::new(),
    };
    let mut changed = false;
    ui.horizontal(|ui| {
        // The current choice as a small strip beside the box.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 16.0), Sense::hover());
        match chosen {
            Some(p) => paint_strip(ui.painter(), rect, p),
            None => {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0, Color32::from_gray(110)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(text)
            .height(420.0)
            .show_ui(ui, |ui| {
                if let Some(l) = none_label
                    && row(ui, None, l, current.is_none())
                {
                    changed |= current.is_some();
                    *current = None;
                    ui.close();
                }
                for p in &all {
                    let sel = current.as_deref() == Some(p.name.as_str());
                    if row(ui, Some(p), &p.name, sel) {
                        changed |= !sel;
                        *current = Some(p.name.clone());
                        ui.close();
                    }
                }
            });
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_offered_once_and_extras_follow() {
        let extra = vec![
            Palette::new("Ember", vec![[1, 2, 3]]),
            Palette::new("Mine", vec![[1, 2, 3], [4, 5, 6]]),
        ];
        let all = all_palettes(&extra);
        let n = render::palette_names().count();
        assert_eq!(all.len(), n + 1);
        assert_eq!(all.last().unwrap().name, "Mine");
        // The built-in wins over a scene palette of the same name.
        assert_ne!(
            all.iter().find(|p| p.name == "Ember").unwrap().stops,
            vec![[1, 2, 3]]
        );
    }
}
