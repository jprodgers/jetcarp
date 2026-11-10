//! The GPU simulation toggle (`jetcarp_display::simgpu`), on the app's own device, and its status.

use eframe::egui::{self, Color32};
pub use jetcarp_display::simgpu::{SimBackend, SimStatus};

/// The simulator for the app's wgpu device.
pub fn backend(cc: &eframe::CreationContext<'_>) -> SimBackend {
    match cc.wgpu_render_state.as_ref() {
        Some(rs) => SimBackend::from_device(&rs.adapter, rs.device.clone(), rs.queue.clone()),
        None => SimBackend::unavailable("no wgpu device".into()),
    }
}

pub fn status_ui(sim: &SimBackend, ui: &mut egui::Ui) {
    match sim.status() {
        SimStatus::Cpu => {
            ui.label("simulation: CPU");
        }
        SimStatus::Gpu(name) => {
            ui.colored_label(Color32::LIGHT_GREEN, "simulation: GPU")
                .on_hover_text(name);
        }
        SimStatus::Fallback(why) => {
            ui.colored_label(Color32::YELLOW, "simulation: CPU (fallback)")
                .on_hover_text(why);
            ui.label(egui::RichText::new(why).small());
        }
        SimStatus::Waiting => {
            ui.label("simulation: GPU when running");
        }
    }
}
