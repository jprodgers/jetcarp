//! JETCARP desktop app: egui UI over the jetcarp-core engine, tiles drawn with wgpu. The `jetcarp`
//! program opens it with [`run`] when no subcommand is given.

mod ants_ui;
mod app;
mod catalog_picker;
mod catalog_tools_ui;
mod cell_editor;
mod cli;
mod dash_ui;
mod editor;
mod frametime;
mod gpu;
mod hlview_ui;
mod isohedral_ui;
mod jump_ui;
mod keepawake;
mod library_ui;
mod monitors;
#[cfg(test)]
mod offscreen;
mod palette_pick;
mod pile_ui;
mod popout;
mod previews;
mod queue_ui;
mod results;
mod rhai_edit;
mod scripts_ui;
mod simgpu;
mod span;
mod start;
mod sweep_form;
mod thumbs;

pub use cli::LaunchArgs;

/// Opens the app window and runs until it closes.
pub fn run(launch: LaunchArgs) -> Result<(), String> {
    // Scripts run from the app may ask for GPU toppling of large sandpiles.
    jetcarp_core::pile::set_gpu_factory(|| {
        jetcarp_gpu::pile::PileGpu::headless()
            .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
    });
    // `JETCARP_WINDOW=1920x1080` sets the starting window size (headless screenshot checks).
    let size = std::env::var("JETCARP_WINDOW")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.trim().parse().ok()?, h.trim().parse().ok()?])
        })
        .unwrap_or([1400.0, 900.0]);
    // Level-8 aperiodic patches (17 to 18 million tiles) need buffers over wgpu's default 256 MB
    // (instances) and storage bindings over 128 MB (the GPU simulation's neighbour lists): ask for
    // what the adapter allows.
    let mut wgpu_options = eframe::NativeOptions::default().wgpu_options;
    if let egui_wgpu::WgpuSetup::CreateNew(c) = &mut wgpu_options.wgpu_setup {
        let base = c.device_descriptor.clone();
        c.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            let (al, l) = (adapter.limits(), &mut d.required_limits);
            l.max_buffer_size = l.max_buffer_size.max(al.max_buffer_size);
            l.max_storage_buffer_binding_size = l
                .max_storage_buffer_binding_size
                .max(al.max_storage_buffer_binding_size);
            d
        });
    }
    // `JETCARP_PRESENT=fifo|mailbox|immediate|novsync`: the surfaces' present mode (frame pacing
    // checks with several windows; the default waits for vsync on each).
    if let Ok(p) = std::env::var("JETCARP_PRESENT") {
        use eframe::wgpu::PresentMode as P;
        wgpu_options.surface.present_mode = match p.as_str() {
            "fifo" => P::Fifo,
            "mailbox" => P::Mailbox,
            "immediate" => P::Immediate,
            "novsync" => P::AutoNoVsync,
            _ => P::AutoVsync,
        };
    }
    let opts = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("JETCARP")
            .with_app_id("jetcarp")
            .with_inner_size(size)
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "JETCARP",
        opts,
        Box::new(move |cc| Ok(Box::new(app::JetcarpApp::new(cc, launch)))),
    )
    .map_err(|e| e.to_string())
}
