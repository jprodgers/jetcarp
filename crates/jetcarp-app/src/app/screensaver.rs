//! Screensaver mode: every control hidden, the grid on the configured outputs (or the main
//! window full screen), running, the pointer hidden after a moment, optionally a playlist on a
//! rotation and keep-awake. Leaving restores the layout (menu, status bar, panes, windows, where
//! the grid shows, running state, camera) and the world shown before, if the screensaver
//! replaced it.
//!
//! Everything it shows is a screensaver profile (`profiles.rs`): a named profile, or one made
//! on the spot for a scene file, a Quick Start id, or the current world rebuilt to fill the
//! screen, with the screensaver's default cell size and speed.

use super::display::SaverSource;
use super::profiles::{Profile, Show};
use super::*;
use jetcarp_display::caption::{self, CaptionSettings, LATER_LINES, Place};
use jetcarp_display::reseed_presets::blobs_mode;
use jetcarp_display::rotation::{Action, Clock, Observation, Player, Rotation, Slot};

/// What screensaver mode put aside, to restore when it ends.
struct Saved {
    screen: Screen,
    menu_autohide: bool,
    status_bar: bool,
    panes_hidden: bool,
    playing: bool,
    speed: f32,
    max_speed: bool,
    gpu_on: bool,
    cam: [f64; 2],
    zoom: f64,
    t_size: SizeChoice,
    /// The world, if the screensaver replaced it.
    world: Option<(World, String, String)>,
    ai_was_on: bool,
}

/// A running screensaver.
pub(super) struct Saver {
    saved: Saved,
    started: f64,
    /// What it shows (None: the current world as it is).
    player: Option<Player>,
    /// Load the current item once the canvas has settled.
    load_pending: bool,
    /// The current item's world is built and the player is watching it.
    running: bool,
    /// Where the current item's settle watches start in the world's reseed policies.
    watch_from: usize,
    /// When the current item's world started running.
    item_since: f64,
}

/// A caption ready to paint.
#[derive(Clone, Debug)]
pub(super) struct Caption {
    lines: Vec<String>,
    settings: CaptionSettings,
    opacity: f32,
}

/// Paints a caption in a corner of `rect`, the same way the host's surfaces do.
pub(super) fn paint_caption(painter: &egui::Painter, rect: egui::Rect, c: &Caption) {
    if c.opacity <= 0.0 || c.lines.is_empty() {
        return;
    }
    let size = c.settings.size;
    let (pad, margin) = ((size * 0.6).ceil(), (size * 1.2).ceil());
    let text = Color32::WHITE.gamma_multiply(c.opacity);
    let wrap = (rect.width() - 2.0 * (margin + pad)).max(32.0);
    let galleys: Vec<_> = c
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let s = if i == 0 { size } else { size * LATER_LINES };
            painter.layout(l.clone(), egui::FontId::proportional(s), text, wrap)
        })
        .collect();
    let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let h: f32 = galleys.iter().map(|g| g.size().y).sum();
    let box_size = egui::vec2(w + 2.0 * pad, h + 2.0 * pad);
    let corner = c.settings.corner;
    let x = if corner.right() {
        rect.right() - margin - box_size.x
    } else {
        rect.left() + margin
    };
    let y = if corner.bottom() {
        rect.bottom() - margin - box_size.y
    } else {
        rect.top() + margin
    };
    let at = egui::pos2(x, y);
    painter.rect_filled(
        egui::Rect::from_min_size(at, box_size),
        4.0,
        Color32::from_black_alpha((170.0 * c.opacity) as u8),
    );
    let mut line_y = y + pad;
    for g in galleys {
        let h = g.size().y;
        painter.galley(egui::pos2(x + pad, line_y), g, text);
        line_y += h;
    }
}

impl Saver {
    pub fn items(&self) -> &[Slot] {
        self.player.as_ref().map_or(&[], |p| p.slots())
    }
    pub fn index(&self) -> usize {
        self.player.as_ref().map_or(0, |p| p.current_index())
    }
    /// The item showing now.
    pub fn current(&self) -> Option<&Slot> {
        self.player.as_ref()?.current()
    }
    /// Seconds until the current item's time limit, if it has one.
    pub fn seconds_left(&self, now: f64) -> Option<f64> {
        self.player.as_ref()?.seconds_left(now)
    }
}

fn single(label: String, profile: Profile, stored: bool) -> Slot {
    Slot {
        label,
        profile,
        stored,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
    }
}

impl JetcarpApp {
    pub(super) fn saver_active(&self) -> bool {
        self.display.saver.is_some()
    }

    /// The caption to paint now, if there is one. With no rotation (the current world as it is),
    /// it describes that world from the start.
    pub(super) fn saver_caption(&self, now: f64) -> Option<Caption> {
        let s = self.display.saver.as_ref()?;
        let cfg = &self.shell.display.screensaver;
        let settings = cfg.caption;
        if !settings.show || (s.player.is_some() && !s.running) {
            return None;
        }
        let place = (cfg.source == SaverSource::Rotation).then(|| Place {
            rotation: &cfg.rotation,
            number: s.index() + 1,
            count: s.items().len(),
        });
        Some(Caption {
            lines: caption::lines(&self.world, place, settings.generation),
            settings,
            opacity: settings.opacity(now - s.item_since),
        })
    }

    /// A player for what the settings ask for (None: the current world as it is).
    fn saver_player(&mut self) -> Option<Player> {
        let cfg = self.shell.display.screensaver.clone();
        let mut warn = vec![];
        let adhoc = |label: String, show: Show| {
            let p = cfg.adhoc(label.clone(), show);
            single(label, p, false)
        };
        let mut rotation = Rotation {
            name: "one item".into(),
            default_minutes: None,
            ..Default::default()
        };
        let mut slots = vec![];
        match cfg.source {
            SaverSource::Current => {
                // Rebuilt to fill the screen from its setup; else shown as it is (no items).
                if cfg.refill && self.screen == Screen::Sim && !self.world.cas.is_empty() {
                    let sc =
                        profiles::capture_scene(&self.world, &self.scene_name, &self.scene_desc);
                    let mut it = adhoc(
                        "the current world".into(),
                        Show::Scene {
                            scene: Box::new(sc),
                        },
                    );
                    it.profile.reseed_cells = Some(self.world.topo.len() as u64);
                    slots.push(it);
                }
            }
            SaverSource::Profile => match self.profile_store().get(&cfg.profile) {
                Some(p) => slots.push(single(format!("profile {}", p.name), p, true)),
                None => warn.push(format!("no profile named {:?}", cfg.profile)),
            },
            SaverSource::Scene if !cfg.scene.trim().is_empty() => slots.push(adhoc(
                format!("scene {}", cfg.scene),
                Show::File {
                    path: cfg.scene.clone(),
                },
            )),
            SaverSource::QuickStart if !cfg.quickstart.trim().is_empty() => slots.push(adhoc(
                format!("Quick Start {}", cfg.quickstart),
                Show::QuickStart {
                    id: cfg.quickstart.clone(),
                },
            )),
            SaverSource::Scene | SaverSource::QuickStart => {}
            SaverSource::Rotation => match self.rotation_store().get(&cfg.rotation) {
                Some(r) => {
                    let base = cfg.adhoc("rotation item".into(), Show::default());
                    let store = self.profile_store();
                    let (s, w) = r.expand(&base, &|n| store.get(n));
                    slots = s;
                    warn.extend(w);
                    rotation = r;
                }
                None => warn.push(format!("no rotation named {:?}", cfg.rotation)),
            },
        }
        // Nothing to run (the Quick Start screen, a blank slate, a missing profile): Life.
        let nothing = self.screen != Screen::Sim || self.world.cas.is_empty();
        if slots.is_empty() && (nothing || cfg.source != SaverSource::Current) {
            let life = self
                .profile_store()
                .get("Life")
                .unwrap_or_else(|| profiles::builtins().remove(0));
            slots.push(single(format!("profile {}", life.name), life, true));
            rotation.script.clear();
            rotation.order = Default::default();
        }
        if !warn.is_empty() {
            self.message = Some((format!("Screensaver: {}", warn.join("; ")), true));
        }
        if slots.is_empty() {
            return None;
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        match Player::new(&rotation, slots.clone(), seed) {
            Ok(p) => Some(p),
            Err(e) => {
                self.message = Some((format!("Screensaver: {e}"), true));
                rotation.script.clear();
                Player::new(&rotation, slots, seed).ok()
            }
        }
    }

    /// Start screensaver mode (nothing if it runs).
    pub(super) fn saver_enter(&mut self, ctx: &egui::Context) {
        if self.display.saver.is_some() {
            return;
        }
        let cfg = self.shell.display.screensaver.clone();
        let now = ctx.input(|i| i.time);
        let saved = Saved {
            screen: self.screen,
            menu_autohide: self.shell.menu_autohide,
            status_bar: self.shell.status_bar,
            panes_hidden: self.shell.panes_hidden,
            playing: self.playing,
            speed: self.speed,
            max_speed: self.max_speed,
            gpu_on: self.sim.on,
            cam: self.cam,
            zoom: self.zoom,
            t_size: self.t_size,
            world: None,
            ai_was_on: self.ai_port().is_some(),
        };
        let mut player = self.saver_player();
        if let Some(p) = &mut player {
            p.start(Clock::local());
        }
        self.display.saver = Some(Saver {
            saved,
            started: now,
            player,
            load_pending: true,
            running: false,
            watch_from: 0,
            item_since: now,
        });
        self.shell.panes_hidden = true;
        self.shell.status_bar = false;
        self.screen = Screen::Sim;
        self.playing = true;
        // The current world as it is runs at the screensaver's speed; profiles set their own.
        self.max_speed = cfg.speed <= 0.0;
        if cfg.speed > 0.0 {
            self.speed = cfg.speed;
        }
        self.sim.on = cfg.gpu && self.sim.available();
        if cfg.control && self.ai_port().is_none() {
            self.ai_start(ctx);
        }
        self.display.last_input = now;
        self.display.canvas_changed = now;
        ctx.request_repaint();
    }

    /// The settings changed while it runs (another profile chosen): start over with them,
    /// keeping the world put aside at the start.
    pub(super) fn saver_restart(&mut self, ctx: &egui::Context) {
        if self.display.saver.is_none() {
            return self.saver_enter(ctx);
        }
        let mut player = self.saver_player();
        if let Some(p) = &mut player {
            p.start(Clock::local());
        }
        if let Some(s) = &mut self.display.saver {
            s.player = player;
            s.load_pending = true;
            s.running = false;
        }
    }

    /// End screensaver mode and put everything back.
    pub(super) fn saver_leave(&mut self, ctx: &egui::Context) {
        let Some(s) = self.display.saver.take() else {
            return;
        };
        self.display.after_load = None;
        let v = s.saved;
        if let Some((w, name, desc)) = v.world {
            self.show_world(w, name, desc);
        }
        self.t_size = v.t_size;
        self.cam = v.cam;
        self.zoom = v.zoom;
        self.fit_pending = false;
        self.screen = v.screen;
        self.shell.menu_autohide = v.menu_autohide;
        self.shell.status_bar = v.status_bar;
        self.shell.panes_hidden = v.panes_hidden;
        self.playing = v.playing;
        self.speed = v.speed;
        self.max_speed = v.max_speed;
        self.sim.on = v.gpu_on;
        if !v.ai_was_on && self.shell.display.screensaver.control {
            self.ai_stop();
        }
        ctx.set_cursor_icon(egui::CursorIcon::Default);
        if self.display.quit_after_saver {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn saver_toggle(&mut self, ctx: &egui::Context) {
        if self.saver_active() {
            self.saver_leave(ctx);
        } else {
            self.saver_enter(ctx);
        }
    }

    /// Any input ends it (when configured), after a second's grace for the windows to settle.
    pub(super) fn saver_exits_on_any(&self, now: f64) -> bool {
        self.shell.display.screensaver.exit_any_input
            && self
                .display
                .saver
                .as_ref()
                .is_some_and(|s| now - s.started > 1.0)
    }

    /// Every frame while it runs (before the shortcuts): exit keys, the rotation, loading
    /// items once the canvas has settled, the pointer.
    pub(super) fn saver_tick(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.display.saver else { return };
        let now = ctx.input(|i| i.time);
        let started = s.started;
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        // I shows or hides the caption, without counting as input that ends the screensaver.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::I)) {
            let c = &mut self.shell.display.screensaver.caption;
            c.show = !c.show;
        }
        let any = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.pointer.delta().length() > 2.0
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        if esc || (any && self.saver_exits_on_any(now)) {
            self.saver_leave(ctx);
            return;
        }
        if self.cursor_hidden(ctx) {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
        let s = self.display.saver.as_mut().expect("running");
        if s.load_pending && self.canvas_ready() && now - started > 0.3 && self.building.is_none() {
            let s = self.display.saver.as_mut().expect("running");
            s.load_pending = false;
            let item = s.current().cloned();
            self.saver_load(item);
        }
        self.rotation_tick(now);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    /// Watch the running item and move on when the rotation says so.
    fn rotation_tick(&mut self, now: f64) {
        let world_ready = self.building.is_none() && self.display.after_load.is_none();
        let Some(s) = self.display.saver.as_mut() else {
            return;
        };
        if s.load_pending || !world_ready || s.player.is_none() {
            return;
        }
        let clock = Clock::local();
        if !s.running {
            let watches = s.current().map(Slot::watches).unwrap_or_default();
            let from = self.world.reseed_policies().len();
            let generation = self.world.generation;
            s.watch_from = from;
            s.running = true;
            s.item_since = now;
            if let Some(p) = &mut s.player {
                p.on_loaded(now, generation, clock);
            }
            if !watches.is_empty() {
                let mut all = self.world.reseed_policies().to_vec();
                all.extend(watches);
                self.exec(Command::SetReseed { policies: all });
            }
            return;
        }
        let Some(p) = &s.player else { return };
        let due = p.script_due(now);
        let wants_done = due || p.watches_done();
        let from = s.watch_from;
        let obs = Observation::of(&self.world, from, now, clock, due, wants_done);
        let s = self.display.saver.as_mut().expect("running");
        let p = s.player.as_mut().expect("a player");
        let action = p.tick(&obs);
        let warning = p.take_warning();
        match action {
            Action::Stay => {}
            Action::Load(_) => {
                s.load_pending = true;
                s.running = false;
            }
            Action::Reseed => {
                self.exec(Command::Reseed {
                    ca: None,
                    mode: blobs_mode(),
                });
            }
        }
        if let Some(w) = warning {
            self.message = Some((format!("Rotation: {w}"), true));
        }
    }

    /// The next (`+1`) or previous (`-1`) item of the rotation.
    pub(super) fn saver_skip(&mut self, by: i64) -> Result<(), String> {
        let Some(s) = &mut self.display.saver else {
            return Err("the screensaver is not running".into());
        };
        if s.items().len() < 2 {
            return Err("the screensaver shows one thing (no rotation)".into());
        }
        let p = s.player.as_mut().expect("items come from a player");
        if p.advance(by, Clock::local()) != Action::Stay {
            s.load_pending = true;
            s.running = false;
        }
        Ok(())
    }

    /// Show an item (None: the current world as it is, zoomed to the cell size): load its
    /// profile's world at the canvas size, with its speed, palette, and policies.
    fn saver_load(&mut self, item: Option<Slot>) {
        let Some(item) = item else {
            self.display.after_load = Some(saver_ui::AfterLoad {
                px: self.shell.display.screensaver.cell_px,
                reseed_cells: None,
            });
            self.playing = true;
            return;
        };
        // Keep the user's world to give back at the end (once).
        if let Some(s) = &mut self.display.saver
            && s.saved.world.is_none()
        {
            let tiny = World::new(TilingSpec::rect("square", 8, 8, Boundary::Wrap), 1)
                .expect("small world");
            let w = std::mem::replace(&mut self.world, tiny);
            s.saved.world = Some((w, self.scene_name.clone(), self.scene_desc.clone()));
        }
        if let Err(e) = self.profile_load(&item.profile) {
            self.message = Some((format!("Screensaver: {}: {e}", item.label), true));
        }
        if item.stored {
            self.display.saver_ui.shown = Some(item.profile.name.clone());
        }
    }

    /// Once an item's world is built: scale its drops, zoom to its cell size, run (the same step
    /// as *Try in this window*).
    pub(super) fn saver_zoom(&mut self) {
        self.after_load_tick();
    }
}
