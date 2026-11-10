//! Where the grid shows: the main window (the default), the main window full screen on a
//! chosen monitor, or *outputs*: one chrome-less full-screen OS window per chosen monitor,
//! spanning one canvas or mirroring one view (`crate::span` lays them out).
//!
//! The settings ([`DisplaySettings`]) say what is wanted; every frame [`JetcarpApp::display_frame`]
//! makes the windows match (full screen on or off, output windows shown) and passes input from
//! the output windows back to the canvas camera. Output windows are deferred viewports: each
//! draws the tile callback the main frame prepared for it, and records its input in a shared
//! slot (pan, zoom, keys) that the main frame applies at its next frame. eframe 0.37 can
//! screenshot deferred viewports, so the outputs can be checked headless
//! (`JETCARP_SCREENSHOT_VIEWPORT=output-0,output-1`).

use super::*;
use crate::span::{self, Arrangement, Screen as SpanScreen, Units};
use jetcarp_display::caption::CaptionSettings;
pub(super) use jetcarp_display::profiles::{CELL_PX_RANGE, fill_size};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Where the grid shows.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum OutputMode {
    /// The grid tile in the main window.
    #[default]
    Window,
    /// The main window full screen (on `fullscreen_monitor`).
    FullScreen,
    /// Output windows on the chosen monitors; the main window keeps the controls.
    Outputs,
}

impl OutputMode {
    pub fn id(self) -> &'static str {
        match self {
            OutputMode::Window => "window",
            OutputMode::FullScreen => "fullscreen",
            OutputMode::Outputs => "outputs",
        }
    }
    pub fn from_id(s: &str) -> Option<OutputMode> {
        [
            OutputMode::Window,
            OutputMode::FullScreen,
            OutputMode::Outputs,
        ]
        .into_iter()
        .find(|m| m.id() == s)
    }
}

/// One output: a monitor (winit's index, as `ViewportCommand::SetMonitor` takes it).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub(super) struct OutputSpec {
    pub monitor: usize,
    /// A manual position on the canvas, in canvas units (Wayland positions can be missing or
    /// wrong). None: the position winit reports.
    pub pos: Option<[i32; 2]>,
    /// A plain window of this many physical pixels instead of full screen (several outputs on
    /// one monitor, for checks).
    pub window: Option<[u32; 2]>,
}

/// What a screensaver shows.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum SaverSource {
    /// The profile named `profile`.
    #[default]
    Profile,
    /// The rotation named `rotation`.
    #[serde(alias = "Playlist")]
    Rotation,
    /// The world in the app now.
    Current,
    /// `scene` (a scene file).
    Scene,
    /// `quickstart` (a Quick Start id).
    QuickStart,
}

impl SaverSource {
    pub const ALL: [SaverSource; 5] = [
        SaverSource::Profile,
        SaverSource::Rotation,
        SaverSource::Current,
        SaverSource::Scene,
        SaverSource::QuickStart,
    ];
    pub fn id(self) -> &'static str {
        match self {
            SaverSource::Profile => "profile",
            SaverSource::Rotation => "rotation",
            SaverSource::Current => "current",
            SaverSource::Scene => "scene",
            SaverSource::QuickStart => "quickstart",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SaverSource::Profile => "One profile",
            SaverSource::Rotation => "A rotation",
            SaverSource::Current => "The current world",
            SaverSource::Scene => "A scene file",
            SaverSource::QuickStart => "A Quick Start entry",
        }
    }
    pub fn from_id(s: &str) -> Option<SaverSource> {
        SaverSource::ALL.into_iter().find(|m| m.id() == s)
    }
}

/// Screensaver mode's configuration: what it shows (a profile, a rotation, ...) and how it runs.
/// The cell size, refill, speed, and GPU here are for the current world, scene files, and Quick
/// Start entries; profiles carry their own.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub(super) struct SaverSettings {
    pub source: SaverSource,
    /// The profile shown with source `profile`.
    pub profile: String,
    pub scene: String,
    pub quickstart: String,
    /// The rotation shown with source `rotation`.
    pub rotation: String,
    /// Cell size in physical pixels (zoom; with `refill`, the tiling is rebuilt at this size).
    pub cell_px: f32,
    /// Rebuild lattice tilings to fill the canvas at `cell_px` on start and each item.
    pub refill: bool,
    /// Generations per second (0: max speed).
    pub speed: f32,
    /// Step on the GPU where the rules allow it.
    pub gpu: bool,
    pub keep_awake: bool,
    /// Leave on any key, click, or mouse movement (else on Esc only).
    pub exit_any_input: bool,
    /// Start the AI-control endpoint (CLI and MCP control).
    pub control: bool,
    /// Hide floating and popped-out windows while it runs.
    pub hide_windows: bool,
    /// Hide the pointer after this many seconds without input.
    pub cursor_hide_s: f32,
    /// A label saying what is running.
    pub caption: CaptionSettings,
}

impl Default for SaverSettings {
    fn default() -> Self {
        SaverSettings {
            source: SaverSource::Profile,
            profile: "Life".into(),
            scene: String::new(),
            quickstart: String::new(),
            rotation: "A classic a day".into(),
            cell_px: 4.0,
            refill: true,
            speed: 30.0,
            gpu: true,
            keep_awake: true,
            exit_any_input: false,
            control: false,
            hide_windows: true,
            caption: CaptionSettings::default(),
            cursor_hide_s: 2.0,
        }
    }
}

impl SaverSettings {
    /// A profile for a scene file, a Quick Start id, or the current world, with these settings'
    /// cell size, refill, speed, and GPU (and the scene's own palette and policies).
    pub fn adhoc(&self, name: String, show: profiles::Show) -> profiles::Profile {
        profiles::Profile {
            name,
            show,
            cell_px: self.cell_px,
            refill: self.refill,
            speed: self.speed,
            gpu: self.gpu,
            ..Default::default()
        }
    }
}

/// Outputs, full screen, cell size, keep-awake, and the screensaver (saved in the settings).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub(super) struct DisplaySettings {
    pub mode: OutputMode,
    /// Full screen on this monitor (winit index); None: the one the window is on.
    pub fullscreen_monitor: Option<usize>,
    pub outputs: Vec<OutputSpec>,
    pub arrangement: Arrangement,
    pub units: Units,
    /// Outputs on: draw no grid in the main window (saves the GPU work), only a note.
    pub hide_preview: bool,
    /// Cell size in physical pixels for *Fill* and the pixel-exact zoom (1 to 64).
    pub cell_px: f32,
    /// Keep the computer awake while the app is open.
    pub keep_awake: bool,
    pub screensaver: SaverSettings,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        DisplaySettings {
            mode: OutputMode::Window,
            fullscreen_monitor: None,
            outputs: vec![],
            arrangement: Arrangement::Span,
            units: Units::Pixels,
            hide_preview: false,
            cell_px: 8.0,
            keep_awake: false,
            screensaver: SaverSettings::default(),
        }
    }
}

/// What an output window saw since the main frame last looked.
#[derive(Default, Debug)]
pub(super) struct OutInput {
    /// Drag in physical pixels.
    pub drag: [f64; 2],
    /// Zoom factors and the pixel they are about.
    pub zooms: Vec<(f64, [f64; 2])>,
    /// Keys pressed (with modifiers).
    pub keys: Vec<(egui::Key, egui::Modifiers)>,
    /// Any key, click, or pointer movement.
    pub any: bool,
    pub moved: bool,
    pub close: bool,
}

/// Shared between the main frame and one output window.
pub(super) struct OutSlot {
    /// The drawing the main frame prepared.
    pub cb: Option<TileCallback>,
    /// The window's content size in physical pixels, once it has drawn.
    pub size_px: Option<[f32; 2]>,
    pub input: OutInput,
    pub hide_cursor: bool,
    /// Text drawn in the middle (identify).
    pub label: Option<String>,
    pub bg: Color32,
    /// The caption, on the output that carries it.
    pub caption: Option<screensaver::Caption>,
}

impl Default for OutSlot {
    fn default() -> Self {
        OutSlot {
            cb: None,
            size_px: None,
            input: Default::default(),
            hide_cursor: false,
            label: None,
            bg: shell::GRID_BG,
            caption: None,
        }
    }
}

/// A live output window.
pub(super) struct OutputRt {
    pub slot: Arc<Mutex<OutSlot>>,
    /// The spec it was opened for (a change reopens it).
    pub spec: OutputSpec,
}

/// Display state that is not saved.
#[derive(Default)]
pub(super) struct DisplayRt {
    pub outputs: Vec<OutputRt>,
    /// The main window's full screen as last asked for: None windowed, Some(monitor).
    pub fs_applied: Option<Option<usize>>,
    pub saver: Option<super::screensaver::Saver>,
    pub keep: crate::keepawake::KeepAwake,
    /// Why keep-awake is held: the settings' switch, the screensaver.
    pub keep_wanted: bool,
    /// Identify monitors: until when, and the monitors' labels.
    pub identify: Option<(f64, Vec<(usize, String)>)>,
    /// Last input anywhere (pointer or key), for hiding the pointer.
    pub last_input: f64,
    /// The canvas size last frame and when it last changed (fits wait for it to settle).
    pub canvas_seen: [f64; 2],
    pub canvas_changed: f64,
    /// Quit the app when the screensaver ends (`--quit-on-exit`).
    pub quit_after_saver: bool,
    /// A profile (or the screensaver's current world) was loaded: once it is built, scale its
    /// reseed drops, zoom to its cell size, and run.
    pub after_load: Option<super::saver_ui::AfterLoad>,
    /// `JETCARP_FILL`, `JETCARP_PIXEL_ZOOM`: once the canvas has settled.
    pub fill_pending: bool,
    pub pixel_zoom_pending: bool,
    /// Screensaver profiles: the files and the built-ins (read on first use).
    pub profiles: profiles::ProfileStore,
    /// Rotations: the files and the built-ins (read on first use).
    pub rotations: jetcarp_display::rotation::RotationStore,
    pub rotation_ui: super::rotation_ui::RotationUi,
    /// The Screensaver pane's state.
    pub saver_ui: super::saver_ui::SaverUi,
    pub host: super::host_ui::HostPanel,
}

/// Viewport key of output `k` (`JETCARP_SCREENSHOT_VIEWPORT=output-0`).
pub(super) fn output_key(k: usize) -> String {
    format!("output-{k}")
}

impl JetcarpApp {
    /// Output windows are wanted and configured.
    pub(super) fn outputs_active(&self) -> bool {
        self.shell.display.mode == OutputMode::Outputs && !self.shell.display.outputs.is_empty()
    }

    /// The monitors winit lists, without the duplicates some compositors report.
    pub(super) fn monitors_deduped(&self) -> Vec<crate::monitors::MonitorInfo> {
        self.shell
            .monitors
            .list()
            .iter()
            .filter(|m| m.same_as.is_none())
            .cloned()
            .collect()
    }

    /// The outputs' places on the canvas, in order.
    pub(super) fn screens(&self) -> Vec<SpanScreen> {
        let list = self.shell.monitors.list();
        self.shell
            .display
            .outputs
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let m = list.iter().find(|m| m.index == o.monitor);
                let seen = self
                    .display
                    .outputs
                    .get(k)
                    .filter(|r| r.spec == *o)
                    .and_then(|r| r.slot.lock().unwrap().size_px);
                let size = match (seen, o.window, m) {
                    (Some(s), _, _) => [s[0] as f64, s[1] as f64],
                    (None, Some(w), _) => [w[0] as f64, w[1] as f64],
                    (None, None, Some(m)) => [m.size_px[0] as f64, m.size_px[1] as f64],
                    _ => [1920.0, 1080.0],
                };
                let pos = match (o.pos, m) {
                    (Some(p), _) => [p[0] as f64, p[1] as f64],
                    (None, Some(m)) => [m.position_px[0] as f64, m.position_px[1] as f64],
                    _ => [0.0, 0.0],
                };
                SpanScreen {
                    pos,
                    size_px: size,
                    scale: m.map_or(1.0, |m| m.scale_factor),
                }
            })
            .collect()
    }

    /// The outputs' canvas size (canvas units), when outputs are on.
    pub(super) fn output_canvas(&self) -> Option<[f64; 2]> {
        if !self.outputs_active() {
            return None;
        }
        let d = &self.shell.display;
        span::canvas_size(&self.screens(), d.arrangement, d.units)
    }

    /// The canvas in canvas units (physical pixels unless the outputs use logical units):
    /// the outputs' span, or the grid tile (the whole screen when full screen and bare).
    pub(super) fn canvas_px(&self) -> [f64; 2] {
        if let Some(c) = self.output_canvas() {
            return c;
        }
        let ppp = self.ppp() as f64;
        [self.view_size.x as f64 * ppp, self.view_size.y as f64 * ppp]
    }

    /// The canvas centre relative to a whole-pixel origin (for snapping the camera).
    pub(super) fn canvas_center_px(&self) -> [f64; 2] {
        if self.outputs_active() {
            let d = &self.shell.display;
            let s = self.screens();
            match d.arrangement {
                Arrangement::Span => {
                    if let Some(c) = span::canvas(&s, d.units) {
                        return c.center();
                    }
                }
                Arrangement::Mirror => {
                    if let Some(f) = s.first() {
                        return [f.size_px[0] / 2.0, f.size_px[1] / 2.0];
                    }
                }
            }
        }
        let c = self.canvas_px();
        [c[0] / 2.0, c[1] / 2.0]
    }

    /// The canvas has kept its size for a moment (windows going full screen have settled).
    /// In the main window (no full screen, outputs, or screensaver) it is always ready.
    pub(super) fn canvas_ready(&self) -> bool {
        let settling =
            self.display.fs_applied.is_some() || self.outputs_active() || self.saver_active();
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        !settling || now - self.display.canvas_changed > 0.35
    }

    /// The main window should be full screen now (and on which monitor).
    fn want_fullscreen(&self) -> Option<Option<usize>> {
        let d = &self.shell.display;
        let saver_fs = self.display.saver.is_some() && !self.outputs_active();
        (d.mode == OutputMode::FullScreen || saver_fs).then_some(d.fullscreen_monitor)
    }

    /// Every frame, before the panels: make the windows match the settings, apply the outputs'
    /// input, and keep keep-awake in step.
    pub(super) fn display_frame(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        // Full screen.
        let want = self.want_fullscreen();
        if want != self.display.fs_applied {
            match want {
                Some(Some(m)) => ctx.send_viewport_cmd(egui::ViewportCommand::SetMonitor(m)),
                Some(None) => ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true)),
                None => ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false)),
            }
            self.display.fs_applied = want;
            self.display.canvas_changed = now;
        }
        // The canvas size, to wait for it to settle before fitting.
        let c = self.canvas_px();
        if c != self.display.canvas_seen {
            self.display.canvas_seen = c;
            self.display.canvas_changed = now;
        }
        // Input in the output windows.
        self.output_input(ctx, now);
        self.sync_keep_awake();
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO || !i.events.is_empty()) {
            self.display.last_input = now;
        }
        // Screenshot hooks: fill or zoom once the canvas has its size.
        let settled = self.screen == Screen::Sim
            && self.building.is_none()
            && !self.fit_pending
            && self.canvas_ready();
        if settled && self.display.fill_pending {
            self.display.fill_pending = false;
            self.fill_canvas();
            // Frame times measure the filled view, not the frames before it.
            if let Some(f) = &mut self.frametime {
                f.restart();
            }
        } else if settled && self.display.pixel_zoom_pending {
            self.display.pixel_zoom_pending = false;
            self.pixel_zoom(self.shell.display.cell_px as f64);
        }
    }

    /// Keep-awake follows the settings' switch, or a screensaver that asks for it.
    pub(super) fn sync_keep_awake(&mut self) {
        let saver_keep = self.saver_active() && self.shell.display.screensaver.keep_awake;
        let wanted = self.shell.display.keep_awake || saver_keep;
        if wanted == self.display.keep_wanted {
            return;
        }
        self.display.keep_wanted = wanted;
        self.display
            .keep
            .set(wanted, "JETCARP is showing cellular automata");
        if wanted && !self.display.keep.active() {
            self.message = Some((
                format!("Keep-awake: {}", self.display.keep.describe()),
                true,
            ));
        }
    }

    /// Apply what the output windows recorded: pan, zoom, keys, closing.
    fn output_input(&mut self, ctx: &egui::Context, now: f64) {
        if self.display.outputs.is_empty() {
            return;
        }
        let d = &self.shell.display;
        let views = span::views(
            &self.screens(),
            d.arrangement,
            d.units,
            span::Camera {
                center: self.cam,
                zoom: self.zoom,
            },
        );
        let mut keys = vec![];
        let mut any = false;
        let mut close = false;
        let inputs: Vec<OutInput> = self
            .display
            .outputs
            .iter()
            .map(|o| std::mem::take(&mut o.slot.lock().unwrap().input))
            .collect();
        for (k, inp) in inputs.into_iter().enumerate() {
            let Some(v) = views.get(k) else { continue };
            if inp.moved || inp.any {
                self.display.last_input = now;
            }
            any |= inp.any;
            close |= inp.close;
            keys.extend(inp.keys);
            if self.display.saver.is_some() {
                continue;
            }
            if inp.drag != [0.0, 0.0] {
                self.cam[0] -= inp.drag[0] / v.zoom_px;
                self.cam[1] -= inp.drag[1] / v.zoom_px;
            }
            for (f, px) in inp.zooms {
                self.zoom_at(v.world_at(px), f);
            }
        }
        if self.display.saver.is_some() {
            let esc = keys.iter().any(|(k, _)| *k == egui::Key::Escape);
            if esc || close || (any && self.saver_exits_on_any(now)) {
                self.saver_leave(ctx);
            }
            return;
        }
        if close {
            self.shell.display.mode = OutputMode::Window;
            self.message = Some((
                "Outputs closed; the grid is back in this window.".into(),
                false,
            ));
        }
        // Shortcuts pressed in an output window act as in the main window.
        for (key, mods) in keys {
            for a in menu::Action::all() {
                if a.keys()
                    .iter()
                    .any(|k| k.logical_key == key && mods.matches_logically(k.modifiers))
                    && self.action_enabled(a)
                {
                    self.run_action(ctx, a);
                    break;
                }
            }
        }
    }

    /// Every frame, after the panels: show the output windows (and the identify labels).
    pub(super) fn show_outputs(&mut self, ctx: &egui::Context) {
        self.show_identify(ctx);
        if !self.outputs_active() || self.screen != Screen::Sim || !self.gpu {
            self.display.outputs.clear();
            return;
        }
        let specs = self.shell.display.outputs.clone();
        // Keep the slots of outputs whose spec did not change.
        self.display.outputs.truncate(specs.len());
        for (k, s) in specs.iter().enumerate() {
            match self.display.outputs.get_mut(k) {
                Some(o) if o.spec == *s => {}
                Some(o) => {
                    *o = OutputRt {
                        slot: Default::default(),
                        spec: s.clone(),
                    }
                }
                None => self.display.outputs.push(OutputRt {
                    slot: Default::default(),
                    spec: s.clone(),
                }),
            }
        }
        let d = self.shell.display.clone();
        let views = span::views(
            &self.screens(),
            d.arrangement,
            d.units,
            span::Camera {
                center: self.cam,
                zoom: self.zoom,
            },
        );
        let hide_cursor = self.cursor_hidden(ctx);
        let caption = self.saver_caption(ctx.input(|i| i.time));
        for (k, (spec, v)) in specs.iter().zip(views).enumerate() {
            let slot = self.display.outputs[k].slot.clone();
            let size = egui::vec2(v.size_px[0] as f32, v.size_px[1] as f32);
            let cb = self.tile_callback(view::OUTPUT_VIEW0 + k as u64, v.center, v.zoom_px, size);
            let bg = self.stage_color();
            {
                let mut s = slot.lock().unwrap();
                s.cb = Some(cb);
                s.bg = bg;
                s.hide_cursor = hide_cursor;
                s.label = None;
                s.caption = caption.clone().filter(|_| k == 0);
            }
            let title = crate::popout::os_title(&format!("Output {}", k + 1));
            let mut builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_app_id("jetcarp");
            builder = match spec.window {
                Some(w) => {
                    let ppp = ctx.pixels_per_point();
                    builder
                        .with_inner_size([w[0] as f32 / ppp, w[1] as f32 / ppp])
                        .with_decorations(true)
                }
                None => builder.with_decorations(false).with_monitor(spec.monitor),
            };
            let vid = crate::popout::viewport_id(&output_key(k));
            ctx.show_viewport_deferred(vid, builder, move |ui, _class| {
                output_ui(ui, &slot);
            });
            ctx.request_repaint_of(vid);
        }
    }

    /// The pointer is hidden (screensaver, after a while without input).
    pub(super) fn cursor_hidden(&self, ctx: &egui::Context) -> bool {
        let Some(_) = &self.display.saver else {
            return false;
        };
        let now = ctx.input(|i| i.time);
        let after = self.shell.display.screensaver.cursor_hide_s as f64;
        if now - self.display.last_input > after {
            true
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(after.max(0.1)));
            false
        }
    }

    /// Identify monitors: a full-screen label on each for a few seconds.
    pub(super) fn identify_monitors(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let labels = self
            .monitors_deduped()
            .iter()
            .map(|m| (m.index, format!("Monitor {}\n{}", m.index, m.describe())))
            .collect::<Vec<_>>();
        if labels.is_empty() {
            self.message = Some(("No monitors reported.".into(), true));
            return;
        }
        self.display.identify = Some((now + 3.0, labels));
    }

    fn show_identify(&mut self, ctx: &egui::Context) {
        let Some((until, labels)) = &self.display.identify else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if now > *until {
            self.display.identify = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        for (index, label) in labels.clone() {
            let builder = egui::ViewportBuilder::default()
                .with_title(crate::popout::os_title(&format!("Monitor {index}")))
                .with_app_id("jetcarp")
                .with_decorations(false)
                .with_monitor(index);
            let slot = Arc::new(Mutex::new(OutSlot {
                label: Some(label),
                bg: Color32::from_rgb(0x20, 0x30, 0x60),
                ..Default::default()
            }));
            let vid = crate::popout::viewport_id(&format!("identify-{index}"));
            ctx.show_viewport_deferred(vid, builder, move |ui, _| output_ui(ui, &slot));
        }
    }

    /// The main window's grid tile when outputs are on and the preview is hidden.
    pub(super) fn preview_placeholder(&self, ui: &egui::Ui, rect: egui::Rect) {
        let d = &self.shell.display;
        let n = d.outputs.len();
        let how = match d.arrangement {
            Arrangement::Span => "spanning",
            Arrangement::Mirror => "mirrored on",
        };
        let what: Vec<String> = d
            .outputs
            .iter()
            .map(|o| match o.window {
                Some(w) => format!("a {} × {} window", w[0], w[1]),
                None => format!("monitor {}", o.monitor),
            })
            .collect();
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "The grid is on {n} output{} ({how} {}).\nView → Display pane to change it; Esc there or Window mode brings it back.",
                if n == 1 { "" } else { "s" },
                what.join(", ")
            ),
            egui::FontId::proportional(15.0),
            Color32::from_gray(170),
        );
    }

    /// Outputs on: outline each output's slice on the main window's preview.
    pub(super) fn outputs_overlay(&self, ui: &egui::Ui, xf: &view::TileXf) {
        if !self.outputs_active() {
            return;
        }
        let d = &self.shell.display;
        let views = span::views(
            &self.screens(),
            d.arrangement,
            d.units,
            span::Camera {
                center: self.cam,
                zoom: self.zoom,
            },
        );
        let p = ui.painter_at(xf.rect);
        for (k, v) in views.iter().enumerate() {
            let r = v.world_rect();
            let rect = egui::Rect::from_two_pos(xf.to_screen(r.min), xf.to_screen(r.max));
            let col = Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x80, 0xc0);
            p.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, col),
                egui::StrokeKind::Inside,
            );
            p.text(
                rect.left_top() + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                format!("{}", k + 1),
                egui::FontId::proportional(14.0),
                col,
            );
        }
    }

    /// Rebuild the tiling to fill the canvas at the cell size, then zoom pixel-exact.
    pub(super) fn fill_canvas(&mut self) {
        if self.aperiodic_now() {
            self.message = Some((
                "Fill needs a periodic tiling (aperiodic patches have levels).".into(),
                true,
            ));
            return;
        }
        self.t_family = self.world.topo.spec.family.clone();
        self.t_size = SizeChoice::Fill;
        self.apply_tiling_ui(true);
    }

    fn aperiodic_now(&self) -> bool {
        self.world.topo.family.kind == FamilyKind::Aperiodic
    }

    /// Floating and popped-out windows are hidden (screensaver mode, as configured).
    pub(super) fn windows_hidden(&self) -> bool {
        self.saver_active() && self.shell.display.screensaver.hide_windows
    }

    /// Main window full screen on or off (F11).
    pub(super) fn toggle_fullscreen(&mut self) {
        let d = &mut self.shell.display;
        d.mode = if d.mode == OutputMode::FullScreen {
            OutputMode::Window
        } else {
            OutputMode::FullScreen
        };
    }

    /// Outputs on or off (View menu).
    pub(super) fn toggle_outputs(&mut self) {
        let d = &mut self.shell.display;
        if d.mode == OutputMode::Outputs {
            d.mode = OutputMode::Window;
        } else if d.outputs.is_empty() {
            self.message = Some((
                "Choose output monitors in the Display pane first.".into(),
                true,
            ));
            self.shell.panes_hidden = false;
            self.shell.layout.show_pane("display");
        } else {
            d.mode = OutputMode::Outputs;
        }
    }

    /// `jetcarp-app` flags (after the settings and the screenshot hooks).
    pub(super) fn apply_launch(&mut self, ctx: &egui::Context, a: crate::cli::LaunchArgs) {
        let d = &mut self.shell.display;
        if let Some(c) = a.cell_px {
            d.cell_px = c;
            d.screensaver.cell_px = c;
        }
        if !a.monitors.is_empty() {
            d.outputs = a
                .monitors
                .iter()
                .map(|&m| OutputSpec {
                    monitor: m,
                    pos: None,
                    window: None,
                })
                .collect();
            d.mode = OutputMode::Outputs;
        }
        if a.span {
            d.arrangement = Arrangement::Span;
        }
        if a.mirror {
            d.arrangement = Arrangement::Mirror;
        }
        if a.fullscreen {
            d.mode = OutputMode::FullScreen;
        }
        if a.keep_awake {
            d.keep_awake = true;
        }
        if a.control {
            self.ai.start_at_launch = true;
        }
        self.display.quit_after_saver = a.quit_on_exit;
        if a.caption {
            self.shell.display.screensaver.caption.show = true;
        }
        let scene = a.scene.as_ref().map(|p| p.display().to_string());
        if let Some(name) = &a.rotation {
            let Some(r) = self.rotation_store().get(name) else {
                let names = self.rotation_store().names().join(", ");
                self.message = Some((format!("No rotation {name:?} (rotations: {names})"), true));
                return;
            };
            let s = &mut self.shell.display.screensaver;
            s.source = SaverSource::Rotation;
            s.rotation = r.name;
            if a.screensaver {
                self.saver_enter(ctx);
            }
            return;
        }
        if let Some(name) = &a.profile {
            let Some(p) = self.profile_store().get(name) else {
                let names = self.profile_store().names().join(", ");
                self.message = Some((
                    format!("No screensaver profile {name:?} (profiles: {names})"),
                    true,
                ));
                return;
            };
            if a.screensaver {
                let s = &mut self.shell.display.screensaver;
                s.source = SaverSource::Profile;
                s.profile = p.name.clone();
                self.saver_enter(ctx);
            } else {
                self.profile_try(&p);
            }
            return;
        }
        if a.screensaver {
            let s = &mut self.shell.display.screensaver;
            if let Some(p) = scene {
                s.source = SaverSource::Scene;
                s.scene = p;
            } else if let Some(q) = &a.quickstart {
                s.source = SaverSource::QuickStart;
                s.quickstart = q.clone();
            }
            self.saver_enter(ctx);
            return;
        }
        let loaded = match (&scene, &a.quickstart) {
            (Some(p), _) => std::fs::read_to_string(p)
                .map_err(|e| e.to_string())
                .and_then(|j| Scene::from_json(&j).map_err(|e| e.to_string()))
                .map_err(|e| format!("{p}: {e}")),
            (None, Some(q)) => self
                .menu
                .scene_of(q)
                .ok_or(format!("no Quick Start entry {q:?}")),
            _ => return,
        };
        match loaded {
            Ok(s) => self.load_scene_bg(&s),
            Err(e) => self.message = Some((e, true)),
        }
    }
}

/// One output window's frame (runs in the output's own viewport).
fn output_ui(ui: &mut egui::Ui, slot: &Arc<Mutex<OutSlot>>) {
    let ctx = ui.ctx().clone();
    let mut s = slot.lock().unwrap();
    let bg = s.bg;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(bg))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let ppp = ctx.pixels_per_point();
            s.size_px = Some([rect.width() * ppp, rect.height() * ppp]);
            let resp = ui.allocate_rect(rect, Sense::click_and_drag());
            if let Some(cb) = s.cb.clone() {
                ui.painter()
                    .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
            }
            if let Some(c) = &s.caption {
                screensaver::paint_caption(ui.painter(), rect, c);
            }
            if let Some(l) = &s.label {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    l,
                    egui::FontId::proportional(48.0),
                    Color32::WHITE,
                );
            }
            let pan = resp.dragged_by(egui::PointerButton::Primary)
                || resp.dragged_by(egui::PointerButton::Secondary)
                || resp.dragged_by(egui::PointerButton::Middle);
            if pan {
                let d = resp.drag_delta() * ppp;
                s.input.drag[0] += d.x as f64;
                s.input.drag[1] += d.y as f64;
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
                if scroll != 0.0
                    && let Some(p) = resp.hover_pos()
                {
                    let px = (p - rect.min) * ppp;
                    s.input
                        .zooms
                        .push(((scroll as f64 * 0.0025).exp(), [px.x as f64, px.y as f64]));
                }
            }
        });
    let (keys, any, moved, close) = ctx.input(|i| {
        let keys: Vec<_> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect();
        let click = i.pointer.any_pressed();
        let moved = i.pointer.delta().length() > 0.0;
        (keys, click || moved, moved, i.viewport().close_requested())
    });
    let any = any || !keys.is_empty();
    s.input.keys.extend(keys);
    s.input.any |= any;
    s.input.moved |= moved;
    s.input.close |= close;
    if s.hide_cursor {
        ctx.set_cursor_icon(egui::CursorIcon::None);
    }
    if any || close || s.input.drag != [0.0, 0.0] || !s.input.zooms.is_empty() {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saver_settings_keep_the_chosen_rotation() {
        let s = SaverSettings {
            source: SaverSource::Rotation,
            rotation: "Wolfram series".into(),
            ..Default::default()
        };
        let back: SaverSettings =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(
            SaverSource::from_id("rotation"),
            Some(SaverSource::Rotation)
        );
        // Settings saved before rotations still load; their playlist is ignored.
        let old: SaverSettings = serde_json::from_str(
            r#"{"source": "Playlist", "playlist": [{"scene": "a.json"}], "minutes": 2, "shuffle": true}"#,
        )
        .unwrap();
        assert_eq!(old.source, SaverSource::Rotation);
    }

    #[test]
    fn fill_covers_the_canvas_at_one_pixel() {
        let spec = TilingSpec::new("square");
        assert_eq!(
            fill_size(&spec, 2560.0, 1440.0, 1.0),
            SizeSpec::Rect { w: 2560, h: 1440 }
        );
        // A three-monitor span is wider than the old 4000-cell limit.
        assert_eq!(
            fill_size(&spec, 7680.0, 1440.0, 1.0),
            SizeSpec::Rect { w: 7680, h: 1440 }
        );
        assert_eq!(
            fill_size(&spec, 2560.0, 1440.0, 8.0),
            SizeSpec::Rect { w: 320, h: 180 }
        );
    }
}
