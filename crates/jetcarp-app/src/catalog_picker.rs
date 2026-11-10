//! Searchable list of catalog tilings (Archimedean and the 1,248 k-uniform tilings). Picking one
//! builds the `periodic` family with `{"catalog": id}`. State lives in egui memory, so the app only
//! calls [`show`].

use jetcarp_core::catalog::{self, CatalogTiling};
use eframe::egui::{self, RichText};

#[derive(Clone, Default)]
struct State {
    query: String,
    /// 0 = any k.
    k: u32,
}

fn matches(t: &CatalogTiling, k: u32, words: &[String]) -> bool {
    if k != 0 && t.k != k {
        return false;
    }
    let hay = format!("{} {} {} {}", t.id, t.name, t.group, t.note).to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// Draw the picker. `current` is the catalog id now shown, if any. Returns a newly picked id.
pub fn show(ui: &mut egui::Ui, current: Option<&str>) -> Option<&'static str> {
    let id = ui.make_persistent_id("catalog_picker");
    let mut st: State = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let mut picked = None;
    let all = catalog::all();
    egui::CollapsingHeader::new(format!("Tiling catalog ({} tilings)", all.len()))
        .id_salt("catalog_picker_header")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("k");
                egui::ComboBox::from_id_salt("catalog_k")
                    .selected_text(if st.k == 0 {
                        "any".into()
                    } else {
                        st.k.to_string()
                    })
                    .width(48.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.k, 0, "any");
                        for k in 1..=6 {
                            ui.selectable_value(&mut st.k, k, k.to_string());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut st.query)
                        .hint_text("search: 3^6, 4.6.12, p6m, k4-012…")
                        .desired_width(170.0),
                );
            });
            let words: Vec<String> = st
                .query
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();
            let list: Vec<&'static CatalogTiling> =
                all.iter().filter(|t| matches(t, st.k, &words)).collect();
            ui.label(
                RichText::new(format!(
                    "{} shown · vertex types per orbit in brackets",
                    list.len()
                ))
                .small()
                .weak(),
            );
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, list.len(), |ui, range| {
                    for t in &list[range] {
                        let text = format!("{}  {}", t.id, t.name);
                        let r = ui
                            .selectable_label(current == Some(t.id.as_str()), text)
                            .on_hover_text(format!("{}\n{}", t.group, t.note));
                        if r.clicked() {
                            picked = Some(t.id.as_str());
                        }
                    }
                });
        });
    ui.data_mut(|d| d.insert_temp(id, st));
    picked
}
