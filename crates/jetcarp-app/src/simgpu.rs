//! GPU simulation (`jetcarp_display::simgpu`) on the app's own device: its status, and the
//! automatic, on and off setting, which is the GPU policy every part of JETCARP shares.

use eframe::egui::{self, Color32};
pub use jetcarp_display::simgpu::{SimBackend, SimStatus};

/// The simulator for the app's wgpu device.
pub fn backend(cc: &eframe::CreationContext<'_>) -> SimBackend {
    match cc.wgpu_render_state.as_ref() {
        Some(rs) => SimBackend::from_device(&rs.adapter, rs.device.clone(), rs.queue.clone()),
        None => SimBackend::unavailable("no wgpu device".into()),
    }
}

/// Sets the GPU simulation to off, automatic or on, and saves it as the shared setting
/// (`gpu.json` in the settings folder) unless this is a screenshot run without a settings folder
/// of its own. `JETCARP_GPU`, when set, still overrides the saved setting at the next start.
pub fn set_mode(sim: &mut SimBackend, mode: jetcarp_gpu::select::Mode) -> Result<(), String> {
    use jetcarp_gpu::select::Setting;
    sim.mode = mode;
    if std::env::var_os("JETCARP_CONFIG_DIR").is_none()
        && std::env::var_os("JETCARP_SCREENSHOT").is_some()
    {
        return Ok(());
    }
    let mut s = Setting::load().ok().flatten().unwrap_or_default();
    s.gpu = mode;
    s.save().map(|_| ())
}

/// The next of automatic, on, off (the GPU simulation action).
pub fn next_mode(m: jetcarp_gpu::select::Mode) -> jetcarp_gpu::select::Mode {
    use jetcarp_gpu::select::Mode;
    match m {
        Mode::Auto => Mode::On,
        Mode::On => Mode::Off,
        Mode::Off => Mode::Auto,
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
        SimStatus::Small => {
            ui.label("simulation: CPU (automatic: a small world)")
                .on_hover_text(sim.hint());
        }
        SimStatus::Waiting => {
            ui.label("simulation: GPU when running");
        }
    }
}
