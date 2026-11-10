//! Picker for Kaplan's isohedral types (IH01–IH93) and their shape parameters, shared by the
//! tiling panel and the tile editor. The 81 types are kept out of the flat family lists: those
//! show one "Isohedral" entry, and this picker chooses the type.

use eframe::egui::{self, RichText};
use jetcarp_core::isohedral::{self, IH_FAMILIES, IH_TYPES};
use serde_json::{Value, json};

pub fn is_ih(id: &str) -> bool {
    isohedral::index_of(id).is_some()
}

/// The single entry for all isohedral types in a family combo. Returns `ih01` when picked while
/// another family is selected.
pub fn combo_entry(ui: &mut egui::Ui, current: &str) -> Option<&'static str> {
    ui.label(RichText::new("Isohedral (Kaplan)").weak().small());
    let on = is_ih(current);
    let r = ui
        .selectable_label(on, "Isohedral IH01–IH93 (81 types)")
        .on_hover_text("Every isohedral tile type tactile supports; pick the type below");
    (r.clicked() && !on).then_some(IH_FAMILIES[0].id)
}

pub struct ParamEvent {
    pub params: Value,
    /// A slider drag (or a one-click change) began: the editor records undo here.
    pub started: bool,
    /// The change is complete: the tiling panel rebuilds the simulation here.
    pub finished: bool,
}

pub enum IhChange {
    Type(&'static str),
    Params(ParamEvent),
}

/// Type picker (with previous / next) and one slider per shape parameter.
pub fn controls(ui: &mut egui::Ui, id: &str, params: Option<&Value>) -> Option<IhChange> {
    let idx = isohedral::index_of(id)?;
    let t = &IH_TYPES[idx];
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label("Type");
        if ui
            .add_enabled(idx > 0, egui::Button::new("<").small())
            .clicked()
        {
            out = Some(IhChange::Type(IH_FAMILIES[idx - 1].id));
        }
        egui::ComboBox::from_id_salt(ui.make_persistent_id("ih-type"))
            .selected_text(format!("IH{:02}  {}", t.num, IH_FAMILIES[idx].sym))
            .height(360.0)
            .show_ui(ui, |ui| {
                for (f, tt) in IH_FAMILIES.iter().zip(&IH_TYPES) {
                    let label = format!(
                        "IH{:02}  {:5} {} corners, {} param{}",
                        tt.num,
                        f.sym,
                        tt.nv,
                        tt.np,
                        if tt.np == 1 { "" } else { "s" }
                    );
                    if ui.selectable_label(f.id == id, label).clicked() && f.id != id {
                        out = Some(IhChange::Type(f.id));
                    }
                }
            });
        if ui
            .add_enabled(idx + 1 < IH_FAMILIES.len(), egui::Button::new(">").small())
            .clicked()
        {
            out = Some(IhChange::Type(IH_FAMILIES[idx + 1].id));
        }
    });
    if out.is_some() {
        return out;
    }
    if t.np == 0 {
        ui.label(
            RichText::new(
                "No shape parameters: the corners are fixed. Edges can still be reshaped.",
            )
            .small()
            .weak(),
        );
        return None;
    }
    let applied = params
        .and_then(|v| isohedral::params_of(t, v).ok())
        .unwrap_or_else(|| t.defaults.to_vec());
    // While a slider is held, its value lives in egui's memory so the caller can apply it only
    // when the drag ends.
    let key = ui.make_persistent_id(("ih-params", id));
    let mut p: Vec<f64> = ui
        .data(|d| d.get_temp::<Vec<f64>>(key))
        .filter(|v| v.len() == t.np)
        .unwrap_or_else(|| applied.clone());
    let (mut started, mut finished, mut changed, mut dragging) = (false, false, false, false);
    for (k, v) in p.iter_mut().enumerate() {
        let d = t.defaults[k];
        let range = (d - 0.5).min(0.0)..=(d + 0.5).max(1.0);
        let r = ui.add(egui::Slider::new(v, range).text(format!("p{}", k + 1)));
        dragging |= r.dragged();
        changed |= r.changed();
        started |= r.drag_started() || (r.changed() && !r.dragged());
        finished |= r.drag_stopped() || (r.changed() && !r.dragged());
    }
    if ui
        .small_button("Default shape")
        .on_hover_text("tactile's default parameters for this type")
        .clicked()
    {
        p = t.defaults.to_vec();
        (started, finished, changed) = (true, true, true);
    }
    ui.data_mut(|d| {
        if dragging {
            d.insert_temp(key, p.clone());
        } else {
            d.remove::<Vec<f64>>(key);
        }
    });
    (changed || finished).then(|| {
        IhChange::Params(ParamEvent {
            params: json!({ "p": p }),
            started,
            finished,
        })
    })
}
