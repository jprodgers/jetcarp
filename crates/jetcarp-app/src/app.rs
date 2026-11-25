//! App state and UI. Every change to the world goes through `jetcarp_core::Command`, the same layer the
//! CLI and scenes use, so anything done here can be saved and replayed.
//!
//! `JetcarpApp` owns all state; its methods are split by area into the modules below:
//! `shell` (window layout, menu bar placement, status bar), `menu` (actions, menus, shortcuts),
//! `panes` (the control pane registry and the docked layout), `settings` (the settings file),
//! `view` (the grid view), the panes' contents (`tiling_ui`, `ca_ui`, `sequence_ui`, `draw_ui`,
//! `objects_ui`), `tools` (tool windows), `scene` (files), `start_screen`, `autoshot`, `build`,
//! and `ai_control`.

use crate::gpu::{Geometry, TileCallback};
use crate::start;
use eframe::egui::{self, Color32, RichText, Sense};
use jetcarp_core::families::{FAMILIES, FamilyKind};
use jetcarp_core::render::{self, ColorBy, Palette, RenderOptions};
use jetcarp_core::rules::{FieldKind, Rule, catalog};
use jetcarp_core::world::Applied;
use jetcarp_core::*;
use std::sync::Arc;

mod accessibility;
/// Opt-in local endpoint for AI control (MCP). A child module so it can use `try_exec`.
mod ai_control;
mod anim_export;
mod autoshot;
mod build;
mod ca_ui;
mod display;
mod display_api;
mod display_ui;
mod draw_ui;
mod host_ui;
mod keyboard;
mod machine_ui;
pub use keyboard::{Shortcut, ShortcutGroup, table as shortcut_table};
#[cfg(test)]
mod keyboard_tests;
mod menu;
mod objects_ui;
mod palette_ui;
mod panes;
mod reseed_ui;
mod rotation_ui;
mod saver_ui;
mod scene;
mod screensaver;
mod sequence_ui;
mod settings;
pub(crate) use settings::saved_gpu_choice;
mod shell;
#[cfg(test)]
mod shots;
#[cfg(test)]
mod size_tests;
mod start_screen;
mod strobe;
mod tiling_ui;
mod tools;
mod view;
mod wallpaper_ui;
mod window_place;

use ca_ui::rule_menu;
use jetcarp_display::profiles;
use tiling_ui::level_label;

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Start,
    Sim,
}

#[derive(PartialEq, Clone, Copy)]
enum Tool {
    Pan,
    Draw,
    Erase,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum SizeChoice {
    Cells(u32),
    Fill,
    Level(u32),
    /// Exactly this many lattice unit cells across and down (#58): what HashLife's jump in place
    /// needs (sides that are powers of two), and what `--size WxH` gives on the command line.
    Rect(u32, u32),
}

const CELL_PRESETS: [(u32, &str); 10] = [
    (400, "About 400"),
    (900, "About 900"),
    (2500, "About 2,500"),
    (10000, "About 10,000"),
    (40000, "About 40,000"),
    (160_000, "About 160,000"),
    (1_000_000, "About 1 million"),
    (4_000_000, "About 4 million"),
    (16_000_000, "About 16 million"),
    (64_000_000, "About 64 million"),
];

pub struct JetcarpApp {
    screen: Screen,
    /// The Quick Start menu (built-in entries, My finds, search picks).
    menu: start::Menu,
    /// The *Hunts…* dashboard.
    dash: crate::dash_ui::DashWindow,
    world: World,
    scene_name: String,
    scene_desc: String,
    geom: Arc<Geometry>,
    geom_counter: u64,
    degrees: (Vec<usize>, Vec<usize>),
    /// Something changed the colours (taken into `color_epoch` when views draw).
    colors_dirty: bool,
    /// Counts colour changes; each view remembers the epoch it last coloured.
    color_epoch: u64,
    /// What each view (grid tile, outputs, mirror) has coloured.
    views: jetcarp_display::view::Views,
    gpu: bool,
    sim: crate::simgpu::SimBackend,
    // Running
    playing: bool,
    /// The world was running when JETCARP started measuring this computer, and runs again once it
    /// has ended (#301).
    measure_paused: bool,
    speed: f32,
    max_speed: bool,
    acc: f64,
    last_time: f64,
    /// The running world's clock: seconds from it, not egui's input time, which stands still
    /// while the window is hidden and only [`eframe::App::logic`] runs (#87).
    started: std::time::Instant,
    /// Whether the window had the keyboard last frame (#90).
    window_focused: bool,
    // Camera: world point at the view centre and points per world unit.
    cam: [f64; 2],
    zoom: f64,
    fit_pending: bool,
    view_size: egui::Vec2,
    /// The grid view could not draw every cell in view last frame (too many, under a pixel):
    /// how it thinned them, said in the status line (#300).
    thinned: Option<jetcarp_display::view::Thinned>,
    // Tools
    tool: Tool,
    draw_target: Option<u32>,
    draw_value: u32,
    last_painted: Option<usize>,
    // Tiling controls
    t_family: String,
    t_size: SizeChoice,
    /// The graphics adapters found, listed once for Run → GPU adapter (#59).
    gpu_adapters: Option<Vec<(String, &'static str)>>,
    t_boundary: Boundary,
    tile_json: String,
    // CA controls
    add_choice: usize,
    add_text: String,
    rule_text: std::collections::HashMap<u32, String>,
    /// JACA rule boxes being edited: (CA id, positive?) to text.
    jaca_text: std::collections::HashMap<(u32, bool), String>,
    message: Option<(String, bool)>,
    fps: f32,
    /// Script being edited; applied with SetScript.
    script_edit: Option<jetcarp_core::script::Script>,
    script_dirty: bool,
    editor: crate::editor::TileEditor,
    cell_editor: crate::cell_editor::CellEditor,
    pal_open: bool,
    /// PNG export: pixels per tile (0 = fit 2400 px).
    export_px: f32,
    /// The Jump menu's generation count (HashLife).
    jump_gens: String,
    random_grains: u64,
    pal_name: String,
    pal_stops: Vec<[u8; 3]>,
    pending_sheet: Option<scene::PendingSheet>,
    /// *File → Export animation…* (#62).
    anim: anim_export::AnimExport,
    autoshot: Option<autoshot::Autoshot>,
    ai: ai_control::AiControl,
    /// Rhai scripts and sweeps (runs in the background).
    scripts: crate::scripts_ui::ScriptsWindow,
    /// Sweep results browser.
    results: crate::results::ResultsBrowser,
    /// Where scripts write (`$JETCARP_OUT` or `<Pictures>/JETCARP`).
    out_root: std::path::PathBuf,
    /// Pattern library window.
    library: crate::library_ui::LibraryWindow,
    pile: crate::pile_ui::PileWindow,
    hlview: crate::hlview_ui::HlView,
    /// `JETCARP_HLVIEW`, applied once the scene has loaded.
    hlview_env: Option<String>,
    catalog_tools: crate::catalog_tools_ui::CatalogTools,
    ants: crate::ants_ui::AntsWindow,
    /// A library pattern waiting for clicks in the main view.
    paste: Option<crate::library_ui::PasteRequest>,
    /// Status, populations, and state hash of the world at one revision (scanning a
    /// 17-million-cell world every frame cost tens of milliseconds).
    stats: Option<Stats>,
    /// A large tiling being built on a background thread.
    building: Option<build::Building>,
    /// Scenes start running when loaded (`JETCARP_PAUSED=1` turns this off).
    autoplay: bool,
    /// `JETCARP_FRAMETIME=1`: frame-time statistics, printed at exit.
    frametime: Option<crate::frametime::FrameStats>,
    /// `JETCARP_ZOOM=f`: zoom in by f after fitting a new tiling (headless culling checks).
    zoom_mul: f64,
    /// Menu bar, panes, settings, monitors.
    shell: shell::ShellState,
    /// Outputs, full screen, screensaver, keep-awake (what is not saved).
    display: display::DisplayRt,
    /// For saving settings at exit.
    ctx: Option<egui::Context>,
    /// The strobe guard: its settings, what the views' guards ask, Settings → Safety (#97).
    strobe: strobe::AppStrobe,
    /// Tools → This computer… and the question a world over the machine's limit gets (#213).
    machine: machine_ui::MachineWin,
    /// The Reseed pane (`reseed_ui.rs`).
    reseed: reseed_ui::ReseedUi,
    /// The palette window's browser and editor state (`palette_ui.rs`).
    pal_ui: palette_ui::PaletteUi,
    /// The wallpaper export dialog (`wallpaper_ui.rs`).
    wallpaper: wallpaper_ui::WallpaperUi,
    /// The setup wizard (`crate::setup_ui`): at the first launch, and Help → Setup….
    setup: crate::setup_ui::Wizard,
    /// The photosensitive seizure warning (`crate::warning_ui`, #96): until it has been
    /// answered it fills the window, and nothing steps, shows or starts.
    warning: crate::warning_ui::Warning,
    /// The command line's launch flags, held back while the warning shows (screensaver mode and
    /// full-screen outputs among them).
    pending_launch: Option<crate::cli::LaunchArgs>,
    /// Settings → Accessibility, and the system's settings it follows (`accessibility.rs`).
    a11y: accessibility::A11yRt,
    /// The keyboard: the grid's cell cursor, F6's parts of the window, the menu bar (`keyboard.rs`).
    kb: keyboard::KeyboardRt,
    /// Panes open as windows over the Quick Start: only those not about a world
    /// ([`panes::WORLD_FREE`], #331). A world opening takes them into the layout.
    start_panes: Vec<&'static str>,
}

/// What the status and menu bars show about the world, kept until its revision changes.
struct Stats {
    rev: u64,
    status: Status,
    populations: Vec<u64>,
    /// None while a large world runs (hashing 17 million cells takes about 50 ms); computed
    /// once it pauses.
    hash: Option<String>,
}

/// Worlds with more cells than this hash their state only when paused.
const HASH_WHILE_RUNNING: usize = 1 << 20;

impl JetcarpApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: crate::cli::LaunchArgs) -> JetcarpApp {
        let mut slot_cap = crate::gpu::TEX_W as usize * 2048;
        let gpu = if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let r = crate::gpu::AppGpu::new(&rs.device, rs.target_format);
            slot_cap = crate::gpu::slot_capacity(&rs.device);
            rs.renderer.write().callback_resources.insert(r);
            true
        } else {
            false
        };
        // The theme (standard or high contrast, light or dark) is put in place with the settings
        // below (`a11y_frame`).
        // Atkinson Hyperlegible Next and Mono, egui's fonts behind them (`crate::fonts`); the
        // font chosen in Settings → Accessibility is put in place with the settings below.
        crate::fonts::install_default(&cc.egui_ctx);
        let world = World::new(TilingSpec::rect("square", 120, 80, Boundary::Wrap), 1)
            .expect("blank world");
        let geom = Arc::new(Geometry::build(&world.topo, 1));
        let degrees = world.topo.degree_sets();
        let mut app = JetcarpApp {
            screen: Screen::Start,
            menu: start::Menu::new(),
            dash: Default::default(),
            world,
            scene_name: String::new(),
            scene_desc: String::new(),
            geom,
            geom_counter: 1,
            degrees,
            colors_dirty: true,
            color_epoch: 1,
            views: jetcarp_display::view::Views::new(slot_cap),
            gpu,
            sim: crate::simgpu::backend(cc),
            playing: false,
            measure_paused: false,
            speed: 15.0,
            max_speed: false,
            acc: 0.0,
            last_time: 0.0,
            started: std::time::Instant::now(),
            window_focused: true,
            cam: [0.0, 0.0],
            zoom: 8.0,
            fit_pending: true,
            view_size: egui::vec2(1000.0, 800.0),
            thinned: None,
            tool: Tool::Pan,
            draw_target: None,
            draw_value: 1,
            last_painted: None,
            t_family: "square".into(),
            t_size: SizeChoice::Fill,
            gpu_adapters: None,
            t_boundary: Boundary::Wrap,
            tile_json: String::new(),
            add_choice: 0,
            add_text: String::new(),
            rule_text: Default::default(),
            jaca_text: Default::default(),
            message: None,
            fps: 0.0,
            script_edit: None,
            script_dirty: false,
            pending_sheet: None,
            anim: Default::default(),
            pal_open: false,
            export_px: 0.0,
            jump_gens: "1000".into(),
            random_grains: 1000,
            pal_name: String::new(),
            pal_stops: vec![],
            editor: crate::editor::TileEditor::new(),
            cell_editor: crate::cell_editor::CellEditor::new(),
            autoshot: None,
            ai: Default::default(),
            scripts: Default::default(),
            results: Default::default(),
            out_root: jetcarp_api::out::out_root(None),
            library: Default::default(),
            pile: Default::default(),
            hlview: Default::default(),
            hlview_env: std::env::var("JETCARP_HLVIEW").ok(),
            catalog_tools: Default::default(),
            ants: Default::default(),
            paste: None,
            stats: None,
            building: None,
            autoplay: true,
            frametime: crate::frametime::FrameStats::from_env(),
            zoom_mul: std::env::var("JETCARP_ZOOM")
                .ok()
                .and_then(|z| z.parse().ok())
                .unwrap_or(1.0),
            shell: shell::ShellState::new(),
            display: Default::default(),
            ctx: Some(cc.egui_ctx.clone()),
            strobe: Default::default(),
            machine: Default::default(),
            reseed: Default::default(),
            pal_ui: Default::default(),
            wallpaper: Default::default(),
            setup: crate::setup_ui::Wizard::for_app(),
            warning: crate::warning_ui::Warning::for_app(),
            pending_launch: None,
            a11y: Default::default(),
            kb: Default::default(),
            start_panes: Vec::new(),
        };
        let settings = app.shell.store.load();
        app.apply_settings(&cc.egui_ctx, settings);
        // A changed machine suggests measuring it again, once (#213).
        app.machine_suggestion();
        // The app holds still while JETCARP measures this computer (#301).
        jetcarp_core::machine::quiet::honour();
        // Reduced motion and the theme, before the first scene opens or the first frame draws.
        app.a11y_frame(&cc.egui_ctx);
        // The installed fonts' character maps, for characters no font in place has, read on a
        // thread of their own so the first frame doesn't wait (`crate::fonts::FallbackScan`).
        app.a11y.fonts.fallback.start(&cc.egui_ctx);
        let mut app = app.with_autoshot(&cc.egui_ctx);
        // The photosensitive seizure warning comes first (#96): until it has been answered, the
        // launch waits.
        if app.warning.showing {
            app.pending_launch = Some(launch);
        } else {
            app.launch(&cc.egui_ctx, launch);
        }
        app
    }

    /// The launch, once the photosensitive seizure warning has been answered: the command line's
    /// flags (screensaver mode among them), then, at the first launch (setup never ran), the
    /// setup wizard; screenshot runs and a launch into screensaver mode never open it.
    fn launch(&mut self, ctx: &egui::Context, launch: crate::cli::LaunchArgs) {
        let saver = launch.screensaver;
        self.apply_launch(ctx, launch);
        if !saver
            && std::env::var_os("JETCARP_SCREENSHOT").is_none()
            && crate::setup_ui::Wizard::first_run()
        {
            self.setup.show_again();
        }
    }

    /// The photosensitive seizure warning, filling the window. An answer is saved and the launch
    /// goes on; Quit closes the app.
    fn warning_screen(&mut self, ui: &mut egui::Ui) {
        use crate::warning_ui::Choice;
        let ctx = ui.ctx().clone();
        match self.warning.show(ui) {
            None => {}
            Some(Choice::Quit) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Choice::Answered(a)) => {
                if let Err(e) = jetcarp_core::photosensitivity::record(a, "app") {
                    self.message = Some((
                        format!(
                            "Your answer to the photosensitive seizure warning could not be \
                             saved, so it will be asked again next time: {e}"
                        ),
                        true,
                    ));
                }
                self.warning.showing = false;
                if let Some(l) = self.pending_launch.take() {
                    self.launch(&ctx, l);
                }
            }
        }
    }

    // ---------------------------------------------------------------- command layer

    /// Apply a command to the world. All UI actions come through here.
    fn exec(&mut self, cmd: Command) -> Option<Applied> {
        match self.try_exec(cmd) {
            Ok(a) => Some(a),
            Err(e) => {
                self.message = Some((e, true));
                None
            }
        }
    }

    /// `exec` that returns the error instead of showing it (AI control reports errors itself).
    fn try_exec(&mut self, cmd: Command) -> Result<Applied, String> {
        if let Some(busy) = self.build_busy() {
            return Err(busy);
        }
        let old = self.world.topo.clone();
        let a = self.world.apply(cmd)?;
        if !Arc::ptr_eq(&old, &self.world.topo) {
            self.on_new_topology();
        }
        self.after_exec();
        Ok(a)
    }

    fn after_exec(&mut self) {
        self.colors_dirty = true;
        if self
            .draw_target
            .is_none_or(|id| self.world.ca(id).is_none())
        {
            self.draw_target = self.world.cas.first().map(|c| c.id);
        }
    }

    fn on_new_topology(&mut self) {
        self.on_new_topology_with(None);
    }

    /// A new tiling is in place; `geom` is its drawing geometry if already built.
    fn on_new_topology_with(&mut self, geom: Option<Geometry>) {
        self.geom = Arc::new(geom.unwrap_or_else(|| {
            self.geom_counter += 1;
            Geometry::build(&self.world.topo, self.geom_counter)
        }));
        self.degrees = self.world.topo.degree_sets();
        self.fit_pending = true;
        self.colors_dirty = true;
        self.rule_text.clear();
        self.jaca_text.clear();
        self.script_edit = self.world.script.clone();
        self.script_dirty = false;
        self.tiling_ui_from_world();
    }

    /// The Tiling pane's family, edges and size, as the world on screen has them (a new tiling,
    /// or a change of them not built).
    fn tiling_ui_from_world(&mut self) {
        let spec = &self.world.topo.spec;
        self.t_family = spec.family.clone();
        self.t_boundary = if self.world.topo.wraps() {
            Boundary::Wrap
        } else if self.world.topo.live_edges() {
            Boundary::Live
        } else {
            Boundary::Dead
        };
        self.t_size = match spec.size {
            Some(SizeSpec::Level(l)) => SizeChoice::Level(l),
            Some(SizeSpec::Cells(c)) => SizeChoice::Cells(c),
            // A world built from Width × height keeps showing its sides; other rectangles come
            // from Fill (or a scene) and read as Fill, as before.
            Some(SizeSpec::Rect { w, h }) if matches!(self.t_size, SizeChoice::Rect(..)) => {
                SizeChoice::Rect(w, h)
            }
            Some(SizeSpec::Rect { .. }) => SizeChoice::Fill,
            None if self.world.topo.family.kind == FamilyKind::Aperiodic => {
                SizeChoice::Level(aperiodic::default_level(&spec.family))
            }
            None => SizeChoice::Cells(900),
        };
    }

    /// Recompute the status, populations, and hash when the world changed.
    fn refresh_stats(&mut self) {
        let rev = self.world.revision();
        let hash_now = !self.playing || self.world.topo.len() <= HASH_WHILE_RUNNING;
        match &mut self.stats {
            Some(s) if s.rev == rev => {
                if s.hash.is_none() && hash_now {
                    s.hash = Some(self.world.hash_hex());
                }
            }
            _ => {
                self.stats = Some(Stats {
                    rev,
                    status: self.world.status(),
                    populations: self.world.fields.iter().map(|f| f.population()).collect(),
                    hash: hash_now.then(|| self.world.hash_hex()),
                });
            }
        }
    }
}

impl eframe::App for JetcarpApp {
    fn on_exit(&mut self) {
        if let Some(ctx) = self.ctx.clone() {
            // Save the layout the screensaver put aside, not its bare screen.
            self.saver_leave(&ctx);
            self.save_settings(&ctx, true);
        }
        // Hunts and searches running here stop at a checkpoint and are left paused: the
        // dashboard's, then those the other windows started (when the dashboard never opened).
        self.dash.shutdown(std::time::Duration::from_secs(20));
        crate::window_job::shutdown(
            &jetcarp_store::Library::default_path(),
            std::time::Duration::from_secs(20),
        );
        // The rest of the app's own: the AI control link closes (its endpoint file goes) and
        // keep-awake is let go. Then the process ends at once, touching nothing on the GPU: the
        // simulation's buffers, eframe's device and its instance all end with the process.
        // Destroying the last instance unloads the graphics driver, and a driver thread still
        // running then crashes the process; dropping buffers or waiting on the device crashed
        // Windows' software renderer (see `jetcarp_display::exit`).
        self.ai = Default::default();
        self.display.keep.set(false, "");
        // Windows: the main window's taskbar identity goes before the window does (#347).
        jetcarp_display::launcher::clear_window_identity();
        // The library's connections aren't closed either, so fold its write-ahead log into the
        // file first: a copy of library.sqlite alone then holds everything.
        let _ = jetcarp_store::Library::checkpoint(&jetcarp_store::Library::default_path());
        // A login item being registered with launchd (macOS, #99) finishes first.
        crate::desktop_offers::finish_login_item();
        jetcarp_display::exit::end_process(0);
    }

    /// Before every frame, and on its own while the window is hidden (minimised, covered, or on
    /// another desktop: macOS, Wayland and Windows give such a window no frames): AI control is
    /// answered and the running world steps here, so both go on behind a hidden window (#87).
    /// eframe runs these passes when a repaint is asked for: the AI control link asks for one
    /// with each request, and a running world with each pass. While the window is hidden a
    /// running world asks for the next pass 50 ms on, not at once: with no frame to present there
    /// is nothing to wait on, and on macOS a covered window's passes then ran back to back, a
    /// whole core busy. A pass steps that long's worth of generations (`run_steps` takes up to a
    /// quarter of a second at a time), so the world keeps its speed. Nothing here draws.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Nothing runs, AI control included, until the photosensitive seizure warning has been
        // answered (#96).
        if self.warning.showing {
            return;
        }
        // While a file dialog is open the world and AI control wait, as they did when the dialog
        // held up the frame loop (#136): what is saved is what was showing when it was asked for.
        if crate::file_dialog::any_open() {
            return;
        }
        self.measuring_hold(ctx);
        self.ai_poll(ctx);
        if self.screen == Screen::Sim {
            let now = self.started.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            // What the strobe guard's views asked for after the last frame: a cap, a pause.
            self.strobe_frame();
            self.run_steps(now);
            if let Some(f) = &mut self.frametime {
                f.add_step(t0.elapsed().as_secs_f64() * 1e3);
            }
            if self.playing {
                if ctx.input(|i| i.viewport().visible() == Some(false)) {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                } else {
                    ctx.request_repaint();
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt).max(1e-3);
        self.fps = self.fps * 0.9 + 0.1 / dt;
        if let Some(f) = &mut self.frametime {
            f.frame(self.world.generation);
        }
        self.shell.monitors.update(frame);
        self.window_place(&ctx, frame);
        // The system's reduced motion and contrast, as Settings → Accessibility follows them.
        self.a11y_frame(&ctx);
        // F6, F10, announcements, scrolling to what the keyboard reached (`keyboard.rs`).
        self.keyboard_begin(&ctx);
        // The photosensitive seizure warning, alone, until it has been answered (#96): no outputs,
        // screensaver, tool windows or setup. A screenshot run still ends, picturing it.
        if self.warning.showing {
            self.warning_screen(ui);
            self.autoshot_tick(&ctx);
            self.keyboard_end(&ctx);
            return;
        }
        self.poll_build(&ctx);
        self.saver_zoom();
        self.autoshot_tick(&ctx);
        self.display_frame(&ctx);
        self.saver_tick(&ctx);
        if self.building.is_none()
            && let Some(s) = self.hlview_env.take()
        {
            self.screen = Screen::Sim;
            self.hlview.open_env(&self.world, &s);
        }
        // Leaving the window ends a text edit: a rule typed in a Rule box then applies as it does
        // on a click elsewhere, rather than waiting, unapplied, while the window is away (#90).
        let window_focused = ctx.input(|i| i.viewport().focused) != Some(false);
        if self.window_focused
            && !window_focused
            && let Some(f) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
        self.window_focused = window_focused;
        self.shortcuts(&ctx);
        // AI control and the world's steps ran in `logic`, just before this.
        if self.screen == Screen::Sim {
            self.refresh_stats();
        }
        self.shell_ui(ui);
        self.show_outputs(&ctx);
        if !self.windows_hidden() {
            self.tool_windows(&ctx);
        }
        // The guard's confirmation shows even with the panels hidden.
        self.safety_window(&ctx);
        self.export_sheet();
        self.file_dialog_answers();
        if self.screen == Screen::Sim && self.playing {
            ctx.request_repaint();
        }
        // The focus ring, the live status line, the accessibility tree's last tidy.
        self.keyboard_end(&ctx);
        self.save_settings(&ctx, false);
    }
}
