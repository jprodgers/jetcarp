//! App state and UI. Every change to the world goes through `jetcarp_core::Command`, the same layer the
//! CLI and scenes use, so anything done here can be saved and replayed.
//!
//! `JetcarpApp` owns all state; its methods are split by area into the modules below:
//! `shell` (window layout, menu bar placement, status bar), `menu` (actions, menus, shortcuts),
//! `panes` (the control pane registry and the docked layout), `settings` (the settings file),
//! `view` (the grid view), the panes' contents (`tiling_ui`, `ca_ui`, `sequence_ui`, `draw_ui`,
//! `objects_ui`), `tools` (tool windows), `scene` (files), `start_screen`, `autoshot`, `build`,
//! and `ai_control`.

use crate::gpu::{Geometry, TileCallback, TileRenderer};
use crate::start;
use eframe::egui::{self, Color32, RichText, Sense};
use jetcarp_core::families::{FAMILIES, FamilyKind};
use jetcarp_core::render::{self, ColorBy, Palette, RenderOptions};
use jetcarp_core::rules::{FieldKind, Rule, catalog};
use jetcarp_core::world::Applied;
use jetcarp_core::*;
use std::sync::Arc;

/// Opt-in local endpoint for AI control (MCP). A child module so it can use `try_exec`.
mod ai_control;
mod autoshot;
mod build;
mod ca_ui;
mod display;
mod display_api;
mod display_ui;
mod draw_ui;
mod host_ui;
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
mod shell;
mod start_screen;
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
    /// eframe's device, waited on at exit.
    wgpu_device: Option<eframe::wgpu::Device>,
    sim: crate::simgpu::SimBackend,
    // Running
    playing: bool,
    speed: f32,
    max_speed: bool,
    acc: f64,
    last_time: f64,
    // Camera: world point at the view centre and points per world unit.
    cam: [f64; 2],
    zoom: f64,
    fit_pending: bool,
    view_size: egui::Vec2,
    // Tools
    tool: Tool,
    draw_target: Option<u32>,
    draw_value: u32,
    last_painted: Option<usize>,
    // Tiling controls
    t_family: String,
    t_size: SizeChoice,
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
    pending_sheet: Option<(jetcarp_core::sheet::SheetKind, u64, String, bool)>,
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
    /// The Reseed pane (`reseed_ui.rs`).
    reseed: reseed_ui::ReseedUi,
    /// The palette window's browser and editor state (`palette_ui.rs`).
    pal_ui: palette_ui::PaletteUi,
    /// The wallpaper export dialog (`wallpaper_ui.rs`).
    wallpaper: wallpaper_ui::WallpaperUi,
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
            let r = TileRenderer::new(&rs.device, rs.target_format);
            slot_cap = crate::gpu::slot_capacity(&rs.device);
            rs.renderer.write().callback_resources.insert(r);
            true
        } else {
            false
        };
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        // Ubuntu Light (the default proportional font) has few symbols; fall back to Hack for
        // arrows, boxes, and dots in labels and buttons.
        let mut fonts = egui::FontDefinitions::default();
        if let Some(p) = fonts.families.get_mut(&egui::FontFamily::Proportional)
            && !p.iter().any(|f| f == "Hack")
        {
            p.push("Hack".to_owned());
        }
        cc.egui_ctx.set_fonts(fonts);
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
            wgpu_device: cc.wgpu_render_state.as_ref().map(|rs| rs.device.clone()),
            sim: crate::simgpu::backend(cc),
            playing: false,
            speed: 15.0,
            max_speed: false,
            acc: 0.0,
            last_time: 0.0,
            cam: [0.0, 0.0],
            zoom: 8.0,
            fit_pending: true,
            view_size: egui::vec2(1000.0, 800.0),
            tool: Tool::Pan,
            draw_target: None,
            draw_value: 1,
            last_painted: None,
            t_family: "square".into(),
            t_size: SizeChoice::Fill,
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
            reseed: Default::default(),
            pal_ui: Default::default(),
            wallpaper: Default::default(),
        };
        let settings = app.shell.store.load();
        app.apply_settings(&cc.egui_ctx, settings);
        let mut app = app.with_autoshot(&cc.egui_ctx);
        app.apply_launch(&cc.egui_ctx, launch);
        app
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
        // The rest of the app's own, while the device lives: the AI control link closes (its
        // endpoint file goes), keep-awake is let go, the GPU simulation's buffers are dropped,
        // and the GPU is waited on until it is idle. Then the process ends at once, leaving
        // eframe's device and instance to the system: destroying the last instance unloads the
        // graphics driver, and a driver thread still running then crashes the process (see
        // `jetcarp_display::exit`).
        self.ai = Default::default();
        self.display.keep.set(false, "");
        self.sim = crate::simgpu::SimBackend::unavailable("closing".into());
        if let Some(device) = &self.wgpu_device {
            jetcarp_display::exit::wait_idle(device);
        }
        jetcarp_display::exit::end_process(0);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt).max(1e-3);
        self.fps = self.fps * 0.9 + 0.1 / dt;
        if let Some(f) = &mut self.frametime {
            f.frame(self.world.generation);
        }
        self.shell.monitors.update(frame);
        self.window_place(frame);
        self.poll_build(&ctx);
        self.saver_zoom();
        self.autoshot_tick(&ctx);
        self.ai_poll(&ctx);
        self.display_frame(&ctx);
        self.saver_tick(&ctx);
        if self.building.is_none()
            && let Some(s) = self.hlview_env.take()
        {
            self.screen = Screen::Sim;
            self.hlview.open_env(&self.world, &s);
        }
        self.shortcuts(&ctx);
        if self.screen == Screen::Sim {
            let now = ctx.input(|i| i.time);
            let t0 = std::time::Instant::now();
            self.run_steps(now);
            if let Some(f) = &mut self.frametime {
                f.add_step(t0.elapsed().as_secs_f64() * 1e3);
            }
            self.refresh_stats();
        }
        self.shell_ui(ui);
        self.show_outputs(&ctx);
        if !self.windows_hidden() {
            self.tool_windows(&ctx);
        }
        self.export_sheet();
        if self.screen == Screen::Sim && self.playing {
            ctx.request_repaint();
        }
        self.save_settings(&ctx, false);
    }
}
