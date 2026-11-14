//! The Screensaver pane (*View → Screensaver settings…*): what the screensaver shows (one
//! profile, a rotation, the current world, a scene file, a Quick Start entry), how
//! it runs, and the profiles themselves: the list, an editor for the selected one, and the
//! workflow to tune one (*Try in this window*, then the Reseed pane, the palette browser, the
//! Run menu, then *Update from the current world*).
//!
//! Profile edits are saved to the profile's file a moment after the last change (once the
//! mouse button is up); editing a built-in saves a user copy. See `profiles.rs`.

use super::display::SaverSource;
use super::profiles::{self, Profile, ProfileStore, Show};
use super::*;
use std::path::PathBuf;

/// Seconds after the last edit before a profile is saved.
const SAVE_DELAY: f64 = 0.5;

/// What to do once a loaded profile's world is built: scale its reseed drops to the world's
/// cell count, zoom to the cell size, and run.
#[derive(Clone, Debug)]
pub(super) struct AfterLoad {
    pub px: f32,
    /// The cell count the world's policies were set for.
    pub reseed_cells: Option<u64>,
}

/// The pane's state.
#[derive(Default)]
pub(super) struct SaverUi {
    /// The profile being edited.
    pub selected: Option<String>,
    /// Its working copy.
    draft: Option<Profile>,
    /// Unsaved edits since (seconds).
    dirty_at: Option<f64>,
    /// The new name while renaming.
    rename: Option<String>,
    /// The profile last tried in this window or shown by the screensaver (what *Update from the
    /// current world* updates).
    pub shown: Option<String>,
    /// The last thing done, or an error.
    note: Option<(String, bool)>,
    /// *Tune for this rule*: running or done.
    tune: Option<TuneJob>,
    /// A profile to try once the grid has its size (`--profile` at launch).
    pending_try: Option<Profile>,
}

/// *Tune for this rule* on a worker thread.
struct TuneJob {
    /// The profile it was started for.
    profile: String,
    rx: std::sync::mpsc::Receiver<TuneMsg>,
    progress: (usize, usize),
    results: Option<Result<Vec<profiles::TuneResult>, String>>,
}

enum TuneMsg {
    Progress(usize, usize),
    Done(Result<Vec<profiles::TuneResult>, String>),
}

impl TuneJob {
    fn start(profile: String, sc: Scene) -> TuneJob {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t2 = tx.clone();
            let r = profiles::run_tune(&sc, profiles::TUNE_GENS, |k, n| {
                let _ = t2.send(TuneMsg::Progress(k, n));
            });
            let _ = tx.send(TuneMsg::Done(r));
        });
        TuneJob {
            profile,
            rx,
            progress: (0, 0),
            results: None,
        }
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                TuneMsg::Progress(k, n) => self.progress = (k, n),
                TuneMsg::Done(r) => self.results = Some(r),
            }
        }
    }
}

impl JetcarpApp {
    /// The profiles (read from disk on first use).
    pub(super) fn profile_store(&mut self) -> &mut ProfileStore {
        if !self.display.profiles.is_loaded() {
            let dir = ProfileStore::dir_for_settings(self.shell.store.path());
            self.display.profiles = ProfileStore::at(dir);
        }
        &mut self.display.profiles
    }

    /// The scene a profile's source gives (before its overrides).
    pub(super) fn profile_source_scene(&self, p: &Profile) -> Result<Scene, String> {
        profiles::source_scene(p, &|id| self.menu.scene_of(id))
    }

    /// Load a profile's world (in the background if large) at the current canvas, with its
    /// speed and GPU choice; once it is built its drops are scaled and the view zoomed
    /// ([`JetcarpApp::after_load_tick`]). A scene that cannot be read or built falls back to Life.
    pub(super) fn profile_load(&mut self, p: &Profile) -> Result<(), String> {
        let (p, scene, err) = profiles::resolve(p, &|id| self.menu.scene_of(id));
        let sc = profiles::apply_profile(&p, scene, Some(self.canvas_px()));
        let autoplay = self.autoplay;
        self.autoplay = true;
        self.load_scene_bg(&sc);
        self.autoplay = autoplay;
        if p.refill && profiles::is_lattice(&sc) {
            self.t_size = SizeChoice::Fill;
        }
        self.max_speed = p.speed <= 0.0;
        if p.speed > 0.0 {
            self.speed = p.speed;
        }
        self.sim.on = p.gpu && self.sim.available();
        self.screen = Screen::Sim;
        self.playing = true;
        self.fit_pending = false;
        self.display.after_load = Some(AfterLoad {
            px: p.cell_px,
            reseed_cells: p.reseed_cells,
        });
        err.map_or(Ok(()), Err)
    }

    /// *Try in this window*: load the profile here (the grid tile, not full screen) to tune it.
    pub(super) fn profile_try(&mut self, p: &Profile) {
        // At launch the grid has no size yet: wait for it, or the world would be tiny.
        if self.view_size.x < 32.0 || self.view_size.y < 32.0 {
            self.display.saver_ui.pending_try = Some(p.clone());
            self.screen = Screen::Sim;
            return;
        }
        let r = self.profile_load(p);
        self.display.saver_ui.shown = Some(p.name.clone());
        self.display.saver_ui.selected = Some(p.name.clone());
        self.display.saver_ui.draft = None;
        self.message = Some(match r {
            Ok(()) => (
                format!(
                    "Trying screensaver profile {}: tune it with the Reseed pane, the palettes, and the Run menu, then Update it in the Screensaver pane.",
                    p.name
                ),
                false,
            ),
            Err(e) => (format!("Profile {}: {e}", p.name), true),
        });
    }

    /// Every frame: once a loaded profile is built, scale its reseed drops to the world, zoom
    /// to its cell size, and run; and save pending profile edits.
    pub(super) fn after_load_tick(&mut self) {
        self.saver_ui_flush(false);
        if self.building.is_some() || self.screen != Screen::Sim {
            return;
        }
        if self.view_size.x >= 32.0
            && self.view_size.y >= 32.0
            && let Some(p) = self.display.saver_ui.pending_try.take()
        {
            self.profile_try(&p);
            return;
        }
        let Some(a) = self.display.after_load.take() else {
            return;
        };
        if let Some(from) = a.reseed_cells {
            let cur = self.world.reseed_policies().to_vec();
            let scaled = profiles::scale_policies(&cur, from, self.world.topo.len());
            if scaled != cur {
                self.exec(Command::SetReseed { policies: scaled });
            }
        }
        self.zoom_to_cells(a.px as f64);
        self.playing = true;
    }

    /// Centre the world and zoom so a cell is `px` pixels (pixel-exact); a world smaller than
    /// the canvas at that size (not rebuilt to fill it, or aperiodic) is fitted instead.
    pub(super) fn zoom_to_cells(&mut self, px: f64) {
        self.fit_pending = false;
        let b = self.world.topo.bounds;
        self.cam = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        self.pixel_zoom(px);
        let c = self.canvas_px();
        let (w, h) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
        let fit = (c[0] / w).min(c[1] / h);
        if fit > self.zoom {
            self.cam = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
            self.zoom = fit;
        }
    }

    /// The world now as a new profile: its setup (with palette and reseed policies), the cell
    /// size it shows at, the speed, and the GPU choice.
    pub(super) fn profile_from_world(&self, name: &str) -> Profile {
        let sc = profiles::capture_scene(&self.world, &self.scene_name, &self.scene_desc);
        let lattice = profiles::is_lattice(&sc);
        let px = (self.zoom * self.cell_unit()).round() as f32;
        Profile {
            name: name.to_string(),
            description: self.scene_desc.clone(),
            show: Show::Scene {
                scene: Box::new(sc),
            },
            palette: None,
            color_by: None,
            reseed: None,
            reseed_cells: Some(self.world.topo.len() as u64),
            cell_px: px.clamp(
                *display::CELL_PX_RANGE.start(),
                *display::CELL_PX_RANGE.end(),
            ),
            refill: lattice,
            speed: if self.max_speed { 0.0 } else { self.speed },
            gpu: self.sim.on,
            minutes: None,
        }
    }

    /// Replace a profile's scene, palette, colouring, policies, speed, and GPU with the world
    /// now (keeping its name, description, cell size, refill, and minutes) and save it.
    pub(super) fn profile_update_from_world(&mut self, name: &str) -> Result<PathBuf, String> {
        let old = self
            .profile_store()
            .get(name)
            .ok_or(format!("no profile named {name:?}"))?;
        let now = self.profile_from_world(name);
        let p = Profile {
            name: old.name.clone(),
            description: old.description.clone(),
            cell_px: old.cell_px,
            refill: old.refill && now.refill,
            minutes: old.minutes,
            ..now
        };
        let path = self.profile_store().save(&p)?;
        if self.display.saver_ui.selected.as_deref() == Some(name) {
            self.display.saver_ui.draft = Some(p);
            self.display.saver_ui.dirty_at = None;
        }
        Ok(path)
    }

    /// Start the screensaver on one profile (or switch the running one to it).
    pub(super) fn saver_show_profile(&mut self, name: &str, ctx: &egui::Context) {
        self.saver_ui_flush(true);
        let s = &mut self.shell.display.screensaver;
        s.source = SaverSource::Profile;
        s.profile = name.to_string();
        if self.saver_active() {
            self.saver_restart(ctx);
        } else {
            self.saver_enter(ctx);
        }
    }

    /// Save the draft if its edits are due (or now, with `force`).
    fn saver_ui_flush(&mut self, force: bool) {
        let Some(t) = self.display.saver_ui.dirty_at else {
            return;
        };
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        let down = self
            .ctx
            .as_ref()
            .is_some_and(|c| c.input(|i| i.pointer.any_down()));
        if !force && (now - t < SAVE_DELAY || down) {
            return;
        }
        self.display.saver_ui.dirty_at = None;
        let Some(p) = self.display.saver_ui.draft.clone() else {
            return;
        };
        let r = self.profile_store().save(&p);
        self.display.saver_ui.note = Some(match r {
            Ok(path) => (format!("Saved {}", path.display()), false),
            Err(e) => (e, true),
        });
    }

    /// The screensaver's canvas in pixels: the outputs' span, else this window's monitor.
    fn saver_canvas_estimate(&self) -> [f64; 2] {
        if let Some(c) = self.output_canvas() {
            return c;
        }
        let list = self.shell.monitors.list();
        let m = match self.shell.display.fullscreen_monitor {
            Some(i) => list.iter().find(|m| m.index == i),
            None => list.iter().find(|m| m.current).or(list.first()),
        };
        m.map_or([2560.0, 1440.0], |m| {
            [m.size_px[0] as f64, m.size_px[1] as f64]
        })
    }

    // ------------------------------------------------------------------ the pane

    pub(super) fn saver_pane(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = ui.input(|i| i.time);
        self.profile_store();
        // ---------------------------------------------- start
        let active = self.saver_active();
        ui.horizontal(|ui| {
            let label = if active {
                "Leave the screensaver"
            } else {
                "Start the screensaver"
            };
            if ui
                .button(RichText::new(label).strong())
                .on_hover_text("F12. Esc leaves it.")
                .clicked()
            {
                self.saver_ui_flush(true);
                self.saver_toggle(&ctx);
            }
            if active
                && self
                    .display
                    .saver
                    .as_ref()
                    .is_some_and(|s| s.items().len() > 1)
            {
                if ui
                    .small_button("◀")
                    .on_hover_text("Previous item")
                    .clicked()
                {
                    let _ = self.saver_skip(-1);
                }
                if ui.small_button("▶").on_hover_text("Next item").clicked() {
                    let _ = self.saver_skip(1);
                }
            }
        });
        let where_ = if self.outputs_active() {
            format!("on the {} outputs", self.shell.display.outputs.len())
        } else {
            "full screen on this window's monitor".into()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Runs {where_}.")).small().weak());
            if ui
                .small_button("Display pane…")
                .on_hover_text("Where the grid shows: outputs, full screen, which monitors")
                .clicked()
            {
                self.shell.layout.show_pane("display");
            }
        });
        if let Some(s) = &self.display.saver
            && let Some(it) = s.current()
        {
            ui.label(
                RichText::new(format!(
                    "Showing {} ({} of {}).",
                    it.label,
                    s.index() + 1,
                    s.items().len()
                ))
                .small(),
            );
        }
        // ---------------------------------------------- what it shows
        ui.separator();
        self.saver_show_ui(ui);
        egui::CollapsingHeader::new("While it runs")
            .id_salt("saver-options")
            .show(ui, |ui| self.saver_options_ui(ui));
        self.system_saver_ui(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("The desktop background has a pane of its own:")
                    .small()
                    .weak(),
            );
            if ui
                .small_button("Desktop background settings…")
                .on_hover_text(
                    "The background host: scenes behind your windows, the lock screen, and fitting them to this computer",
                )
                .clicked()
            {
                self.shell.layout.show_pane("desktop");
            }
        });
        // ---------------------------------------------- profiles
        ui.separator();
        self.profiles_list_ui(ui, &ctx);
        ui.separator();
        self.profile_editor_ui(ui, &ctx, now);
        if let Some((t, err)) = &self.display.saver_ui.note {
            let c = if *err {
                Color32::from_rgb(255, 110, 110)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(RichText::new(t).small().color(c));
        }
        if self.display.saver_ui.dirty_at.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }

    /// Show: one profile, a rotation, the current world, a scene file, a Quick Start entry.
    fn saver_show_ui(&mut self, ui: &mut egui::Ui) {
        let names = self.profile_store().names();
        let c = &mut self.shell.display.screensaver;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show").strong());
            egui::ComboBox::from_id_salt("saver-source")
                .selected_text(c.source.label())
                .show_ui(ui, |ui| {
                    for s in SaverSource::ALL {
                        ui.selectable_value(&mut c.source, s, s.label());
                    }
                });
        });
        let mut select = None;
        match c.source {
            SaverSource::Profile => {
                ui.horizontal(|ui| {
                    ui.label("Profile");
                    egui::ComboBox::from_id_salt("saver-profile")
                        .selected_text(c.profile.as_str())
                        .show_ui(ui, |ui| {
                            for n in &names {
                                if ui.selectable_value(&mut c.profile, n.clone(), n).clicked() {
                                    select = Some(n.clone());
                                }
                            }
                        });
                });
                if !names.contains(&c.profile) {
                    ui.colored_label(
                        Color32::from_rgb(255, 170, 90),
                        format!("No profile named {:?}: Life shows instead.", c.profile),
                    );
                }
            }
            SaverSource::Rotation => {}
            SaverSource::Current | SaverSource::Scene | SaverSource::QuickStart => {
                match c.source {
                    SaverSource::Scene => {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut c.scene)
                                    .hint_text("scene.json")
                                    .desired_width(180.0),
                            );
                            if ui.button("…").clicked()
                                && let Some(p) = rfd::FileDialog::new()
                                    .add_filter("JETCARP scene", &["json"])
                                    .pick_file()
                            {
                                c.scene = p.display().to_string();
                            }
                        });
                    }
                    SaverSource::QuickStart => {
                        ui.add(
                            egui::TextEdit::singleline(&mut c.quickstart)
                                .hint_text("Quick Start id, e.g. rule:B3/S23"),
                        );
                    }
                    _ => {}
                }
                ui.label(
                    RichText::new("With the scene's own palette and reseeding, and:")
                        .small()
                        .weak(),
                );
                defaults_ui(ui, c);
            }
        }
        if c.source == SaverSource::Rotation {
            let ctx = ui.ctx().clone();
            self.rotation_ui(ui, &ctx);
        }
        if let Some(n) = select {
            self.saver_select(&n);
        }
    }

    fn saver_options_ui(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.shell.display.screensaver;
        super::host_ui::caption_ui(ui, &mut c.caption);
        ui.label(
            RichText::new("Press I while it runs to show or hide it.")
                .small()
                .weak(),
        );
        ui.separator();
        ui.checkbox(&mut c.keep_awake, "Keep the computer awake while it runs");
        ui.checkbox(
            &mut c.exit_any_input,
            "Leave on any key or mouse movement (else Esc only)",
        );
        ui.checkbox(&mut c.hide_windows, "Hide floating and popped-out windows");
        ui.checkbox(
            &mut c.control,
            "Start AI control (CLI and MCP can steer it)",
        );
        ui.horizontal(|ui| {
            ui.label("Hide the pointer after");
            ui.add(
                egui::DragValue::new(&mut c.cursor_hide_s)
                    .range(0.0..=3600.0)
                    .speed(0.1)
                    .suffix(" s"),
            );
        });
        ui.label(
            RichText::new(
                "`jetcarp --screensaver --profile NAME` starts in it; `jetcarp app screensaver on|off`, `jetcarp app profiles`, and the MCP tool app_display control it.",
            )
            .small()
            .weak(),
        );
    }

    /// Select a profile for editing (saving the previous one's edits first).
    fn saver_select(&mut self, name: &str) {
        if self.display.saver_ui.selected.as_deref() == Some(name) {
            return;
        }
        self.saver_ui_flush(true);
        self.display.saver_ui.selected = Some(name.to_string());
        self.display.saver_ui.draft = None;
        self.display.saver_ui.rename = None;
    }

    /// The profile list and the actions on the list.
    fn profiles_list_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Profiles").strong());
            ui.label(
                RichText::new("a complete setup: scene, palette, reseeding, cells, speed")
                    .small()
                    .weak(),
            );
        });
        let list = self.profile_store().list();
        if self.display.saver_ui.selected.is_none() {
            let c = &self.shell.display.screensaver;
            let pick = list
                .iter()
                .find(|l| l.profile.name == c.profile)
                .or(list.first())
                .map(|l| l.profile.name.clone());
            self.display.saver_ui.selected = pick;
        }
        let selected = self.display.saver_ui.selected.clone();
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("saver-profile-list")
            .max_height(170.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for l in &list {
                    let on = selected.as_deref() == Some(l.profile.name.as_str());
                    let text = RichText::new(format!("{}  ", l.profile.name));
                    let r = ui
                        .horizontal(|ui| {
                            let r = ui.selectable_label(on, text);
                            ui.label(RichText::new(l.tag()).small().weak());
                            r
                        })
                        .inner;
                    let r = r.on_hover_text(if l.profile.description.is_empty() {
                        l.profile.show.describe()
                    } else {
                        format!("{}\n{}", l.profile.description, l.profile.show.describe())
                    });
                    if r.clicked() {
                        select = Some(l.profile.name.clone());
                    }
                    if r.double_clicked() {
                        let p = l.profile.clone();
                        self.saver_ui_flush(true);
                        self.profile_try(&p);
                    }
                }
            });
        if let Some(n) = select {
            self.saver_select(&n);
        }
        for e in self.display.profiles.errors.clone() {
            ui.colored_label(Color32::from_rgb(255, 110, 110), RichText::new(e).small());
        }
        ui.horizontal_wrapped(|ui| {
            let sim = self.screen == Screen::Sim && !self.world.cas.is_empty();
            if ui
                .add_enabled(sim, egui::Button::new("Save current world as profile"))
                .on_hover_text(
                    "A new profile of the world now: its tiling, rules, seeds, palette, and reseed policies, the cell size it shows at, and the speed",
                )
                .clicked()
            {
                let base = if self.scene_name.trim().is_empty() {
                    "My profile".to_string()
                } else {
                    self.scene_name.clone()
                };
                let name = self.profile_store().free_name(&base);
                let p = self.profile_from_world(&name);
                self.saver_ui_flush(true);
                let r = self.profile_store().save(&p);
                self.display.saver_ui.note = Some(match r {
                    Ok(f) => (format!("Saved {}", f.display()), false),
                    Err(e) => (e, true),
                });
                self.display.saver_ui.shown = Some(name.clone());
                self.saver_select(&name);
            }
            if ui
                .button("Import…")
                .on_hover_text("Add profile files (names already taken get a number)")
                .clicked()
                && let Some(files) = rfd::FileDialog::new()
                    .add_filter("Screensaver profile", &["json"])
                    .pick_files()
            {
                let mut done = vec![];
                let mut errs = vec![];
                for f in files {
                    match std::fs::read_to_string(&f)
                        .map_err(|e| e.to_string())
                        .and_then(|t| Profile::from_json(&t))
                    {
                        Ok(mut p) => {
                            p.name = self.profile_store().free_name(&p.name);
                            match self.profile_store().save(&p) {
                                Ok(_) => done.push(p.name),
                                Err(e) => errs.push(e),
                            }
                        }
                        Err(e) => errs.push(format!("{}: {e}", f.display())),
                    }
                }
                self.display.saver_ui.note = Some(if errs.is_empty() {
                    (format!("Imported {}", done.join(", ")), false)
                } else {
                    (errs.join("; "), true)
                });
                if let Some(n) = done.first() {
                    self.saver_select(&n.clone());
                }
            }
            if ui
                .small_button("Reload")
                .on_hover_text("Read the profile files again (after editing them by hand)")
                .clicked()
            {
                self.saver_ui_flush(true);
                self.display.profiles.reload();
                self.display.saver_ui.draft = None;
            }
        });
        if let Some(d) = self.display.profiles.dir() {
            let t = format!("Files: {}", d.display());
            ui.add(egui::Label::new(RichText::new(t).small().weak()).selectable(true));
        } else {
            ui.label(
                RichText::new("No settings folder for this run: profiles are not saved.")
                    .small()
                    .weak(),
            );
        }
        let _ = ctx;
    }

    /// The selected profile's settings and actions.
    fn profile_editor_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, now: f64) {
        let Some(name) = self.display.saver_ui.selected.clone() else {
            return;
        };
        let Some(listed) = self.profile_store().listed(&name) else {
            self.display.saver_ui.selected = None;
            return;
        };
        if self
            .display
            .saver_ui
            .draft
            .as_ref()
            .is_none_or(|d| d.name != name)
        {
            self.display.saver_ui.draft = Some(listed.profile.clone());
            self.display.saver_ui.dirty_at = None;
        }
        let saved_presets = self.reseed.settings.presets.clone();
        let world_signed = reseed_ui::signed_cas(&self.world);
        let canvas = self.saver_canvas_estimate();
        let sim = self.screen == Screen::Sim && !self.world.cas.is_empty();
        let mut draft = self.display.saver_ui.draft.clone().expect("draft");
        let mut changed = false;
        // ---------------------------------------------- name and file actions
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&name).strong().size(15.0));
            ui.label(RichText::new(listed.tag()).small().weak());
        });
        let mut action: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Rename…").clicked() {
                self.display.saver_ui.rename = Some(name.clone());
            }
            if ui.small_button("Duplicate").clicked() {
                action = Some("duplicate");
            }
            if listed.file.is_some() {
                let label = if listed.builtin {
                    "Revert to built-in"
                } else {
                    "Delete"
                };
                if ui.small_button(label).clicked() {
                    action = Some("delete");
                }
            }
            if ui
                .small_button("Export…")
                .on_hover_text("Save this profile as a file to share")
                .clicked()
            {
                action = Some("export");
            }
        });
        if let Some(r) = &mut self.display.saver_ui.rename {
            let mut go = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(r).desired_width(150.0));
                go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                go |= ui.small_button("OK").clicked();
                cancel = ui.small_button("Cancel").clicked();
            });
            if go {
                let new = r.trim().to_string();
                self.saver_ui_flush(true);
                match self.profile_store().rename(&name, &new) {
                    Ok(()) => {
                        let builtin = listed.builtin;
                        if !builtin {
                            self.rename_profile_refs(&name, &new);
                        }
                        self.display.saver_ui.selected = Some(new.clone());
                        self.display.saver_ui.draft = None;
                        self.display.saver_ui.note = Some((
                            if builtin {
                                format!("Copied the built-in {name} as {new}")
                            } else {
                                format!("Renamed {name} to {new}")
                            },
                            false,
                        ));
                    }
                    Err(e) => self.display.saver_ui.note = Some((e, true)),
                }
                self.display.saver_ui.rename = None;
                return;
            }
            if cancel {
                self.display.saver_ui.rename = None;
            }
        }
        // ---------------------------------------------- the settings
        ui.add(
            egui::TextEdit::multiline(&mut draft.description)
                .desired_rows(1)
                .desired_width(f32::INFINITY)
                .hint_text("description"),
        )
        .changed()
        .then(|| changed = true);
        egui::Grid::new("saver-profile-grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // What it shows.
                ui.label("Shows");
                ui.vertical(|ui| {
                    let kind = match &draft.show {
                        Show::Scene { .. } => 0,
                        Show::File { .. } => 1,
                        Show::QuickStart { .. } => 2,
                    };
                    ui.horizontal(|ui| {
                        for (k, label, tip) in [
                            (
                                0,
                                "Its scene",
                                "A scene kept in the profile: the file's or Quick Start entry's scene, else the current world's setup",
                            ),
                            (1, "File", "A scene file"),
                            (2, "Quick Start", "A Quick Start id"),
                        ] {
                            let enabled = k != 0 || kind == 0 || sim;
                            if ui
                                .add_enabled(enabled, egui::Button::selectable(kind == k, label))
                                .on_hover_text(tip)
                                .clicked()
                                && kind != k
                            {
                                draft.show = match k {
                                    0 => Show::Scene {
                                        scene: Box::new(
                                            self.profile_source_scene(&draft).unwrap_or_else(
                                                |_| {
                                                    profiles::capture_scene(
                                                        &self.world,
                                                        &self.scene_name,
                                                        &self.scene_desc,
                                                    )
                                                },
                                            ),
                                        ),
                                    },
                                    1 => Show::File {
                                        path: String::new(),
                                    },
                                    _ => Show::QuickStart {
                                        id: "rule:B3/S23".into(),
                                    },
                                };
                                changed = true;
                            }
                        }
                    });
                    match &mut draft.show {
                        Show::Scene { scene } => {
                            ui.label(
                                RichText::new(Show::Scene { scene: scene.clone() }.describe())
                                    .small(),
                            );
                        }
                        Show::File { path } => {
                            ui.horizontal(|ui| {
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(path)
                                            .hint_text("scene.json")
                                            .desired_width(150.0),
                                    )
                                    .changed();
                                if ui.small_button("…").clicked()
                                    && let Some(p) = rfd::FileDialog::new()
                                        .add_filter("JETCARP scene", &["json"])
                                        .pick_file()
                                {
                                    *path = p.display().to_string();
                                    changed = true;
                                }
                            });
                        }
                        Show::QuickStart { id } => {
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(id)
                                        .hint_text("rule:B3/S23")
                                        .desired_width(150.0),
                                )
                                .changed();
                        }
                    }
                });
                ui.end_row();
                // Palette and colouring.
                let extra: Vec<Palette> = match &draft.show {
                    Show::Scene { scene } => scene.palettes.clone(),
                    _ => vec![],
                };
                ui.label("Palette");
                changed |= crate::palette_pick::picker_opt(
                    ui,
                    "saver-profile-palette",
                    &mut draft.palette,
                    &extra,
                    "Scene's own",
                );
                ui.end_row();
                ui.label("Colour by");
                let text = match draft.color_by {
                    None => "Scene's own",
                    Some(ColorBy::Auto) => "Auto",
                    Some(ColorBy::Age) => "Age",
                    Some(ColorBy::State) => "State",
                    Some(ColorBy::Row) => "Row",
                };
                egui::ComboBox::from_id_salt("saver-profile-colour")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        for (v, l) in [
                            (None, "Scene's own"),
                            (Some(ColorBy::Auto), "Auto"),
                            (Some(ColorBy::Age), "Age"),
                            (Some(ColorBy::State), "State"),
                            (Some(ColorBy::Row), "Row"),
                        ] {
                            changed |= ui.selectable_value(&mut draft.color_by, v, l).changed();
                        }
                    });
                ui.end_row();
                // Cells and speed.
                ui.label("Cells");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut draft.cell_px)
                                .range(display::CELL_PX_RANGE)
                                .speed(0.1)
                                .max_decimals(1)
                                .suffix(" px"),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(&mut draft.refill, "Rebuild to fill")
                        .on_hover_text(
                            "Rebuild a periodic tiling to cover the screen at this cell size (aperiodic patches are fitted)",
                        )
                        .changed();
                });
                ui.end_row();
                ui.label("Speed");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut draft.speed)
                                .range(0.0..=10_000.0)
                                .speed(1.0)
                                .custom_formatter(|v, _| {
                                    if v <= 0.0 {
                                        "max".into()
                                    } else {
                                        format!("{v:.0} gen/s")
                                    }
                                }),
                        )
                        .on_hover_text("Generations per second; 0 is as fast as possible")
                        .changed();
                    changed |= ui.checkbox(&mut draft.gpu, "GPU simulation").changed();
                });
                ui.end_row();
                ui.label("In a rotation");
                ui.horizontal(|ui| {
                    let mut own = draft.minutes.is_some();
                    if ui.checkbox(&mut own, "own minutes").changed() {
                        draft.minutes = own.then_some(5.0);
                        changed = true;
                    }
                    if let Some(m) = &mut draft.minutes {
                        changed |= ui
                            .add(
                                egui::DragValue::new(m)
                                    .range(0.05..=1440.0)
                                    .speed(0.1)
                                    .suffix(" min"),
                            )
                            .changed();
                    }
                });
                ui.end_row();
            });
        // ---------------------------------------------- reseeding
        ui.add_space(2.0);
        ui.label(RichText::new("Reseeding").strong());
        let scene_policies = match &draft.show {
            Show::Scene { scene } => Some(scene.reseed.clone()),
            _ => None,
        };
        let mut own = draft.reseed.is_none();
        ui.horizontal_wrapped(|ui| {
            let n = scene_policies
                .as_ref()
                .map(|p| {
                    format!(
                        " ({} {})",
                        p.len(),
                        if p.len() == 1 { "policy" } else { "policies" }
                    )
                })
                .unwrap_or_default();
            if ui
                .radio_value(&mut own, true, format!("The scene's own{n}"))
                .changed()
            {
                draft.reseed = None;
                changed = true;
            }
            if ui
                .radio_value(&mut own, false, "These policies")
                .on_hover_text("Replace the scene's policies (none: no reseeding)")
                .changed()
            {
                draft.reseed = Some(scene_policies.clone().unwrap_or_default());
                changed = true;
            }
        });
        let cells = (canvas[0] * canvas[1] / (draft.cell_px as f64).powi(2)).max(1.0) as usize;
        if let Some(policies) = &mut draft.reseed {
            ui.label(
                RichText::new("Presets (for this screen at this cell size):")
                    .small()
                    .weak(),
            );
            if let Some(id) = reseed_ui::preset_buttons(ui, &saved_presets)
                && let Some(p) = reseed_ui::find_preset(&id, &saved_presets, cells)
            {
                *policies = p;
                draft.reseed_cells = Some(cells as u64);
                changed = true;
            }
            let cas: Vec<(u32, String)> = match &draft.show {
                Show::Scene { scene } => scene
                    .cas
                    .iter()
                    .enumerate()
                    .map(|(k, c)| {
                        (
                            c.id.unwrap_or(k as u32 + 1),
                            c.name.clone().unwrap_or_else(|| c.rule.clone()),
                        )
                    })
                    .collect(),
                _ => vec![],
            };
            // JACA CAs get a sign row: the scene's, or the running world's for other shows.
            let signed: Vec<u32> = match &draft.show {
                Show::Scene { scene } => scene
                    .cas
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        matches!(
                            jetcarp_core::rules::Rule::parse(&c.rule),
                            Ok((jetcarp_core::rules::Rule::Jaca(_), _))
                        )
                    })
                    .map(|(k, c)| c.id.unwrap_or(k as u32 + 1))
                    .collect(),
                _ => world_signed.clone(),
            };
            changed |= reseed_ui::policies_editor(
                ui,
                "saver-profile-policies",
                policies,
                &cas,
                &signed,
                None,
            );
            if policies.is_empty() {
                ui.label(RichText::new("No policies: no reseeding.").small().weak());
            }
            if let Err(e) = reseed_ui::validate_all(policies) {
                ui.colored_label(Color32::from_rgb(255, 110, 110), e);
            }
        }
        if let Some(c) = draft.reseed_cells {
            ui.label(
                RichText::new(format!(
                    "Rain drop counts are for {} cells and scale with the screen (here about {}).",
                    crate::sweep_form::thousands(c),
                    crate::sweep_form::thousands(cells as u64)
                ))
                .small()
                .weak(),
            );
        }
        // ---------------------------------------------- tune for this rule
        let mut start_tune = false;
        let mut use_result = None;
        let running = self
            .display
            .saver_ui
            .tune
            .as_ref()
            .is_some_and(|t| t.results.is_none());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Tune for this rule"))
                .on_hover_text(format!(
                    "Run a small copy of this profile's world (about {} cells, {} generations each) with no reseeding, the scene's own policies, the presets, and the built-in mixes, and rank how lively each stays",
                    profiles::TUNE_CELLS,
                    profiles::TUNE_GENS
                ))
                .clicked()
            {
                start_tune = true;
            }
            if let Some(t) = &mut self.display.saver_ui.tune {
                t.poll();
                if t.results.is_none() {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("trying {} of {}…", t.progress.0 + 1, t.progress.1.max(1)))
                            .small(),
                    );
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        });
        if let Some(t) = &self.display.saver_ui.tune
            && t.profile == name
        {
            match &t.results {
                Some(Ok(rs)) => {
                    egui::Grid::new("saver-tune-results")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Policies").small().strong());
                            ui.label(RichText::new("Score").small().strong());
                            ui.label(RichText::new("Activity").small().strong())
                                .on_hover_text("Mean share of cells changing per generation; quiet: generations below 0.05%");
                            ui.label("");
                            ui.end_row();
                            for (k, r) in rs.iter().enumerate() {
                                ui.label(RichText::new(&r.name).small());
                                ui.label(RichText::new(format!("{:.2}", r.rank)).small());
                                ui.label(
                                    RichText::new(format!(
                                        "{}, quiet {}",
                                        reseed_ui::pct(r.meter.mean_activity()),
                                        r.meter.quiet
                                    ))
                                    .small(),
                                )
                                .on_hover_text(format!("{}; fired {}", r.meter.summary(), r.fired));
                                if ui.small_button("Use").clicked() {
                                    use_result = Some(k);
                                }
                                ui.end_row();
                            }
                        });
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(255, 110, 110), e);
                }
                None => {}
            }
        }
        if let Some(k) = use_result
            && let Some(Some(Ok(rs))) = self.display.saver_ui.tune.as_ref().map(|t| &t.results)
            && let Some(r) = rs.get(k)
        {
            draft.reseed = Some(r.policies.clone());
            draft.reseed_cells = Some(r.cells);
            changed = true;
        }
        if start_tune {
            match self.profile_source_scene(&draft) {
                Ok(sc) => {
                    let sc = profiles::tune_scene(&draft, sc);
                    self.display.saver_ui.tune = Some(TuneJob::start(name.clone(), sc));
                }
                Err(e) => self.display.saver_ui.note = Some((e, true)),
            }
        }
        // ---------------------------------------------- actions
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Try in this window")
                .on_hover_text(
                    "Load it here to tune it with the Reseed pane, the palettes, and the Run menu; then Update from the current world",
                )
                .clicked()
            {
                action = Some("try");
            }
            if ui
                .button(RichText::new("Start screensaver").strong())
                .on_hover_text("Show this profile now")
                .clicked()
            {
                action = Some("start");
            }
        });
        let shown_here = self.display.saver_ui.shown.as_deref() == Some(name.as_str());
        ui.horizontal_wrapped(|ui| {
            let tip = if shown_here {
                "Take the world now (its scene, palette, reseed policies, speed) into this profile; keeps its name, cell size, refill, and minutes. While the screensaver runs, the panes edit its live world, so this works then too."
            } else {
                "This profile is not the one shown now; this replaces it with whatever world is showing"
            };
            if ui
                .add_enabled(sim, egui::Button::new("Update from the current world"))
                .on_hover_text(tip)
                .clicked()
            {
                action = Some("update");
            }
            if !shown_here && sim {
                ui.label(RichText::new("(not the world shown now)").small().weak());
            }
        });
        // ---------------------------------------------- apply
        if changed {
            if draft.validate().is_ok() {
                self.display.saver_ui.dirty_at = Some(now);
            }
            self.display.saver_ui.draft = Some(draft.clone());
        }
        match action {
            Some("try") => {
                self.saver_ui_flush(true);
                self.profile_try(&draft);
            }
            Some("start") => {
                self.saver_show_profile(&name, ctx);
            }
            Some("update") => {
                self.saver_ui_flush(true);
                let r = self.profile_update_from_world(&name);
                self.display.saver_ui.note = Some(match r {
                    Ok(f) => (
                        format!("Updated {name} from the current world ({})", f.display()),
                        false,
                    ),
                    Err(e) => (e, true),
                });
            }
            Some("duplicate") => {
                self.saver_ui_flush(true);
                let mut p = draft.clone();
                p.name = self.profile_store().free_name(&format!("{name} copy"));
                let r = self.profile_store().save(&p);
                match r {
                    Ok(_) => self.saver_select(&p.name),
                    Err(e) => self.display.saver_ui.note = Some((e, true)),
                }
            }
            Some("delete") => {
                self.display.saver_ui.dirty_at = None;
                let r = self.profile_store().delete(&name);
                self.display.saver_ui.draft = None;
                self.display.saver_ui.note = Some(match r {
                    Ok(()) if listed.builtin => (format!("{name} is the built-in again"), false),
                    Ok(()) => {
                        self.display.saver_ui.selected = None;
                        (format!("Deleted {name}"), false)
                    }
                    Err(e) => (e, true),
                });
            }
            Some("export") => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Screensaver profile", &["json"])
                    .set_file_name(format!("{}.json", profiles::slug(&name)))
                    .save_file()
                {
                    self.display.saver_ui.note =
                        Some(match std::fs::write(&path, draft.to_json()) {
                            Ok(()) => (format!("Exported {}", path.display()), false),
                            Err(e) => (format!("{}: {e}", path.display()), true),
                        });
                }
            }
            _ => {}
        }
    }

    /// A user profile was renamed: the screensaver's choice and the rotations follow.
    fn rename_profile_refs(&mut self, old: &str, new: &str) {
        let c = &mut self.shell.display.screensaver;
        if c.profile == old {
            c.profile = new.to_string();
        }
        if let Err(e) = self.rotation_store().rename_profile(old, new) {
            self.message = Some((format!("Rotations: {e}"), true));
        }
        if self.display.saver_ui.shown.as_deref() == Some(old) {
            self.display.saver_ui.shown = Some(new.to_string());
        }
    }
}

/// Cell size, refill, speed, and GPU for the current world, scene files, and Quick Start ids.
fn defaults_ui(ui: &mut egui::Ui, c: &mut display::SaverSettings) {
    ui.horizontal(|ui| {
        ui.label("Cells");
        ui.add(
            egui::DragValue::new(&mut c.cell_px)
                .range(display::CELL_PX_RANGE)
                .speed(0.2)
                .suffix(" px"),
        );
        ui.checkbox(&mut c.refill, "Rebuild to fill the screen")
            .on_hover_text("Periodic tilings are rebuilt to cover the canvas at this cell size");
    });
    ui.horizontal(|ui| {
        ui.label("Speed");
        ui.add(
            egui::DragValue::new(&mut c.speed)
                .range(0.0..=10_000.0)
                .speed(1.0)
                .custom_formatter(|v, _| {
                    if v <= 0.0 {
                        "max".into()
                    } else {
                        format!("{v:.0} gen/s")
                    }
                }),
        );
        ui.checkbox(&mut c.gpu, "GPU simulation");
    });
}
