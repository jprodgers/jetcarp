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

/// Who the app's own GPU choice belongs to, in reports.
pub const OWNER: &str = "the app";

/// The policy the app's world steps by: `JETCARP_GPU`, else the app's own choice, else the shared
/// setting.
pub fn app_policy(own: jetcarp_gpu::select::Own) -> jetcarp_gpu::select::Policy {
    jetcarp_gpu::select::Policy::load_for(own, OWNER)
}

/// Saves the shared GPU setting (`gpu.json` in the settings folder: automatic, on or off, which
/// the app, the host, the command line and the screen savers follow unless they have a choice of
/// their own), unless this is a screenshot run without a settings folder of its own.
pub fn save_shared(mode: jetcarp_gpu::select::Mode) -> Result<(), String> {
    use jetcarp_gpu::select::Setting;
    if std::env::var_os("JETCARP_CONFIG_DIR").is_none()
        && std::env::var_os("JETCARP_SCREENSHOT").is_some()
    {
        return Ok(());
    }
    let mut s = Setting::load().ok().flatten().unwrap_or_default();
    s.gpu = mode;
    s.save().map(|_| ())
}

/// The shared setting's mode now (automatic when it is not saved).
pub fn shared_mode() -> jetcarp_gpu::select::Mode {
    jetcarp_gpu::select::Setting::load()
        .ok()
        .flatten()
        .map(|s| s.gpu)
        .unwrap_or_default()
}

/// The next of follow, on, off (the GPU simulation action).
pub fn next_choice(o: jetcarp_gpu::select::Own) -> jetcarp_gpu::select::Own {
    use jetcarp_gpu::select::Own;
    match o {
        Own::Follow => Own::On,
        Own::On => Own::Off,
        Own::Off => Own::Follow,
    }
}

/// `Automatic`, `On`, `Off`.
pub fn mode_label(m: jetcarp_gpu::select::Mode) -> &'static str {
    use jetcarp_gpu::select::Mode;
    match m {
        Mode::Auto => "Automatic",
        Mode::On => "On",
        Mode::Off => "Off",
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
