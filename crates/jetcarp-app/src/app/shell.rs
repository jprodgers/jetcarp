//! The window's layout: the menu bar (auto-hiding if asked), the status bar, the docked panes and
//! grid, floating and popped-out panes, and the tool windows. Also the Scene and Command log
//! panes.

use super::*;

/// State of the app's chrome (everything but the world and the tools).
pub(super) struct ShellState {
    pub layout: panes::Layout,
    pub menu_autohide: bool,
    pub status_bar: bool,
    pub cell_outlines: bool,
    /// Tab: only the grid (docked and floating panes hidden).
    pub panes_hidden: bool,
    pub shortcuts_open: bool,
    pub about_open: bool,
    pub ai_at_launch: bool,
    pub display: display::DisplaySettings,
    pub monitors: crate::monitors::Monitors,
    pub store: settings::Store,
    /// `JETCARP_MENU=<title>`: hold that menu open (screenshots).
    pub open_menu: Option<String>,
    /// `JETCARP_MENU_AUTOHIDE=shown`: keep an auto-hidden menu bar revealed (screenshots).
    pub force_reveal: bool,
    /// `JETCARP_GRID_POPOUT=1|deferred`: mirror the grid in a second OS window (wave 2 experiment).
    pub grid_mirror: Mirror,
    /// The deferred mirror's next draw and its last size.
    pub mirror_slot: Arc<std::sync::Mutex<(Option<TileCallback>, egui::Vec2)>>,
    /// Auto-hide: whether the bar shows now, and when the pointer last needed it.
    reveal: bool,
    reveal_at: f64,
    /// Height of the menu bar last frame (the reveal zone while it shows).
    bar_h: f32,
    /// A message, and when it appeared (the grid shows it a while when the status bar is off).
    seen_message: Option<String>,
    message_at: f64,
}

impl ShellState {
    pub fn new() -> ShellState {
        ShellState {
            layout: panes::Layout::default(),
            menu_autohide: false,
            status_bar: true,
            cell_outlines: true,
            panes_hidden: false,
            shortcuts_open: false,
            about_open: false,
            ai_at_launch: false,
            display: Default::default(),
            monitors: Default::default(),
            store: settings::Store::from_env(),
            open_menu: None,
            force_reveal: false,
            grid_mirror: Mirror::Off,
            mirror_slot: Arc::new(std::sync::Mutex::new((None, egui::vec2(640.0, 420.0)))),
            reveal: false,
            reveal_at: 0.0,
            bar_h: 24.0,
            seen_message: None,
            message_at: 0.0,
        }
    }
}

/// The grid mirror experiment (`JETCARP_GRID_POPOUT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mirror {
    Off,
    /// An immediate viewport: drawn in the root frame, from `&mut JetcarpApp`.
    Immediate,
    /// A deferred viewport: its own frames, drawing a callback the root frame prepared.
    Deferred,
}

/// Pixels from the top edge that reveal an auto-hidden menu bar.
const REVEAL_EDGE: f32 = 4.0;
/// Seconds an auto-hidden menu stays after the pointer leaves it.
const HIDE_AFTER: f64 = 1.0;

impl JetcarpApp {
    /// Everything in the root window, every frame.
    pub(super) fn shell_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if self.message.as_ref().map(|m| &m.0) != self.shell.seen_message.as_ref() {
            self.shell.seen_message = self.message.as_ref().map(|m| m.0.clone());
            self.shell.message_at = now;
        }
        let saver = self.saver_active();
        if !saver {
            self.menu_bar(ui, now);
        }
        let sim = self.screen == Screen::Sim;
        if self.shell.status_bar && !saver && (sim || self.message.is_some()) {
            self.status_bar(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(GRID_BG))
            .show(ui, |ui| {
                if !sim {
                    self.start_screen(ui);
                } else if self.shell.panes_hidden {
                    self.grid_view(ui);
                } else {
                    self.docked_ui(ui);
                }
            });
        if sim {
            if !self.windows_hidden() {
                self.loose_panes(&ctx);
            }
            self.grid_mirror(&ctx);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        let ctx = ui.ctx().clone();
        if !self.shell.menu_autohide {
            let r = egui::Panel::top("menu-bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    self.menu_bar_contents(ui);
                    self.bar_status(ui);
                });
            });
            self.shell.bar_h = r.response.rect.height();
            return;
        }
        // Auto-hide: shown over the grid while the pointer is at the top edge or on the bar,
        // a menu is open, or Alt is held; hidden a second after none of these.
        let (pointer, alt) = ctx.input(|i| (i.pointer.hover_pos(), i.modifiers.alt));
        let zone = if self.shell.reveal {
            self.shell.bar_h + REVEAL_EDGE
        } else {
            REVEAL_EDGE
        };
        let wanted = self.shell.force_reveal
            || pointer.is_some_and(|p| p.y <= zone)
            || alt
            || (self.shell.reveal && egui::Popup::is_any_open(&ctx));
        if wanted {
            self.shell.reveal = true;
            self.shell.reveal_at = now;
        } else if self.shell.reveal && now - self.shell.reveal_at > HIDE_AFTER {
            self.shell.reveal = false;
        }
        if !self.shell.reveal {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let screen = ctx.content_rect();
        let r = egui::Area::new(egui::Id::unique("menu-bar-overlay"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                egui::Frame::side_top_panel(ui.style()).show(ui, |ui| {
                    ui.set_width(screen.width() - 16.0);
                    egui::MenuBar::new().ui(ui, |ui| {
                        self.menu_bar_contents(ui);
                        self.bar_status(ui);
                    });
                });
            });
        self.shell.bar_h = r.response.rect.height();
    }

    /// Generation, hash, and speed at the right end of the menu bar.
    fn bar_status(&mut self, ui: &mut egui::Ui) {
        if self.screen != Screen::Sim {
            return;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{:.0} fps", self.fps))
                    .monospace()
                    .weak(),
            );
            match self.stats.as_ref().and_then(|s| s.hash.clone()) {
                Some(h) => {
                    if ui
                        .link(RichText::new(format!("hash {}", &h[..8])).monospace())
                        .on_hover_text(format!("State hash {h}. Click to copy."))
                        .clicked()
                    {
                        ui.ctx().copy_text(h);
                    }
                }
                None => {
                    ui.label(RichText::new("hash …").monospace().weak())
                        .on_hover_text("Large worlds compute their state hash when paused.");
                }
            }
            ui.label(RichText::new(format!("gen {}", self.world.generation)).monospace());
            let (txt, col) = if self.playing {
                ("▶ running", Color32::LIGHT_GREEN)
            } else {
                ("‖ paused", Color32::GRAY)
            };
            if ui
                .add(egui::Button::new(RichText::new(txt).color(col)).frame(false))
                .on_hover_text("Space")
                .clicked()
            {
                self.playing = !self.playing;
            }
            if !self.scene_name.is_empty() {
                ui.add(egui::Label::new(RichText::new(&self.scene_name).strong()).truncate())
                    .on_hover_text(&self.scene_desc);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if let Some((m, err)) = self.message.clone() {
                    ui.colored_label(message_color(err), m);
                    if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                        self.message = None;
                    }
                    ui.separator();
                }
                if self.screen != Screen::Sim {
                    return;
                }
                let (status, pops) = match &self.stats {
                    Some(s) => (s.status.clone(), s.populations.clone()),
                    None => (String::new(), vec![]),
                };
                ui.label(status);
                ui.separator();
                for (i, f) in self.world.fields.iter().enumerate() {
                    let what = if f.kind == FieldKind::Height {
                        "grains"
                    } else {
                        "live"
                    };
                    let pop = pops.get(i).copied().unwrap_or(0);
                    ui.label(format!("field {i}: {pop} {what}"));
                    if f.kind == FieldKind::Height && f.lost > 0 {
                        ui.label(format!("({} lost)", f.lost));
                    }
                }
                ui.separator();
                ui.label(format!(
                    "{} cells, {} storage",
                    self.world.topo.len(),
                    self.world.topo.storage_name()
                ));
                ui.separator();
                if let Some(s) = self.scripts.status() {
                    ui.label(RichText::new(s).color(Color32::from_rgb(0xff, 0xc0, 0x80)));
                    ui.separator();
                }
                crate::simgpu::status_ui(&self.sim, ui);
                if !self.gpu {
                    ui.colored_label(Color32::LIGHT_RED, "no GPU renderer");
                }
            });
        });
    }

    /// With the status bar off, the grid shows a new message for a few seconds.
    pub(super) fn message_overlay(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.shell.status_bar || self.saver_active() {
            return;
        }
        let Some((m, err)) = &self.message else {
            return;
        };
        let age = ui.input(|i| i.time) - self.shell.message_at;
        if age > 6.0 {
            return;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
        let font = egui::FontId::proportional(14.0);
        let galley = ui
            .painter()
            .layout(m.clone(), font, message_color(*err), rect.width() - 40.0);
        let at = rect.left_top() + egui::vec2(12.0, 12.0);
        let bg = egui::Rect::from_min_size(at, galley.size()).expand(6.0);
        ui.painter()
            .rect_filled(bg, 4.0, Color32::from_black_alpha(180));
        ui.painter().galley(at, galley, Color32::WHITE);
    }

    /// The Scene pane: name, description, and the latest message.
    pub(super) fn scene_pane(&mut self, ui: &mut egui::Ui) {
        if self.scene_name.is_empty() {
            ui.label(RichText::new("Untitled world").weak());
        } else {
            ui.label(RichText::new(&self.scene_name).strong().size(16.0));
        }
        if !self.scene_desc.is_empty() {
            ui.label(RichText::new(&self.scene_desc).small());
        }
        if let Some((m, err)) = self.message.clone() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(message_color(err), m);
                if ui.small_button("×").clicked() {
                    self.message = None;
                }
            });
        }
        ui.separator();
        ui.label(
            RichText::new(format!(
                "{} · {} cells · generation {}",
                jetcarp_core::families::family(&self.world.topo.spec.family)
                    .map(|f| f.name)
                    .unwrap_or(&self.world.topo.spec.family),
                self.world.topo.len(),
                self.world.generation
            ))
            .small()
            .weak(),
        );
    }

    /// The Command log pane.
    pub(super) fn log_pane(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(format!(
                "{} commands since the last tiling change or reset. Saved in the scene.",
                self.world.log().len()
            ))
            .small(),
        );
        let log = self.world.log();
        for c in log.iter().skip(log.len().saturating_sub(40)) {
            let mut s = serde_json::to_string(c).unwrap_or_default();
            if s.len() > 160 {
                s.truncate(157);
                s.push('…');
            }
            ui.label(RichText::new(s).monospace().small());
        }
    }
}

pub(super) const GRID_BG: Color32 = Color32::from_rgb(0x10, 0x12, 0x17);

fn message_color(err: bool) -> Color32 {
    if err {
        Color32::LIGHT_RED
    } else {
        Color32::LIGHT_GREEN
    }
}
