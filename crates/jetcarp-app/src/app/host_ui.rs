//! The Desktop background pane (*View → Desktop background settings…*): the background host,
//! which runs scenes as the desktop background (or in windows) as its own program. Start and stop
//! it, choose each screen's rotation and monitor, the lock screen, the still picture, the frame
//! rate and the GPU, the pauses, start at login, GNOME's extension, and *Fit to this computer*,
//! which finds each scene's cell size within the processor and GPU limits set here, in about ten
//! seconds (#113): each screen's most demanding scene is measured, its cards scroll sideways and
//! open at the chosen one (#108), and the rest of the rotation takes its size from it. With
//! [`jetcarp_core::fit::FIT`] off (0.4.2, #112) the pane shows neither it nor its limits and cards.
//!
//! The host is its own program, so all of this works over the link and the settings file
//! (`host.json`); closing this window leaves it running. Rotations and profiles are shared with
//! the screensaver. Profiles are edited in its pane; rotations there or here, under each screen
//! (*Edit*, *New*, and *Show just this scene*: see `rotation_ui.rs`).

use super::*;
use egui_wgpu::wgpu;
use jetcarp_display::autostart;
use jetcarp_display::caption::{CaptionSettings, Corner};
use jetcarp_display::gnome::{self, ExtensionState};
use jetcarp_display::host_settings::{GPU_UNMEASURED, Rung, Size, crops_dir};
use jetcarp_display::host_settings::{
    HostSettings, LockScreen, MAX_AWAKE_MINUTES, Pacing, Place, ScreenSettings, TuneLimits,
    TunedScene, Tuning,
};
use jetcarp_display::pace::DEFAULT_MAX_FPS;
use jetcarp_display::tuning::{self, TuneChild, TuneEvent, TuneRequest};
use jetcarp_gpu::select::{self, Mode, Own};
use jetcarp_mcp::link::host_call;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};

const POLL_EVERY: f64 = 2.0;
/// Settings edits are told to the host this long after the last one.
const RELOAD_AFTER: f64 = 0.3;
const FPS_CHOICES: [f32; 5] = [15.0, 24.0, 30.0, 60.0, 120.0];
const WARN: Color32 = Color32::from_rgb(255, 170, 90);

#[derive(Default)]
pub(super) struct HostPanel {
    /// The last answer (Err: not running, or why not).
    status: Option<Result<Value, String>>,
    rx: Option<Receiver<Result<Value, String>>>,
    asked: f64,
    reload_at: Option<f64>,
    settings: Option<HostSettings>,
    login: Option<bool>,
    note: Option<String>,
    /// GNOME's extension, asked for on a thread because it runs a program.
    extension: Option<ExtensionState>,
    extension_rx: Option<Receiver<Result<ExtensionState, String>>>,
    /// *Fit to this computer*: the sweep under way, the host's or one of its own.
    feed: Option<Feed>,
    /// The cards' pictures, by file, with when the file was written.
    crops: std::collections::HashMap<String, (std::time::SystemTime, egui::TextureHandle)>,
    /// How the last tune ended.
    tune_note: Option<(String, bool)>,
    /// The adapter the GPU policy picks for a display, found on a thread (with no host running
    /// to say which it uses).
    adapter: Option<String>,
    adapter_rx: Option<Receiver<String>>,
    /// Each rotation's scenes (its items' labels) by name, with the rotation they were expanded
    /// from, for Fit's cards; emptied whenever the settings are read again.
    scene_labels: std::collections::HashMap<String, (String, Vec<String>)>,
    /// The rows of cards already scrolled to their chosen card, so each is scrolled once (#108).
    scrolled: std::collections::HashSet<String>,
}

/// The caption's options. True when one changed.
pub(super) fn caption_ui(ui: &mut egui::Ui, c: &mut CaptionSettings) -> bool {
    let before = *c;
    ui.checkbox(&mut c.show, "Show what is running (the rule and the grid)")
        .on_hover_text("A small label in a corner of the screen");
    ui.add_enabled_ui(c.show, |ui| {
        ui.horizontal(|ui| {
            ui.label("Corner");
            egui::ComboBox::from_id_salt(ui.scope_id().with("caption-corner"))
                .selected_text(c.corner.label())
                .show_ui(ui, |ui| {
                    for k in Corner::ALL {
                        ui.selectable_value(&mut c.corner, k, k.label());
                    }
                });
            ui.label("Size");
            ui.add(
                egui::DragValue::new(&mut c.size)
                    .range(8.0..=96.0)
                    .speed(0.5),
            );
        });
        ui.checkbox(&mut c.generation, "Include the generation count");
        ui.horizontal(|ui| {
            ui.label("Stay up for");
            ui.add(
                egui::DragValue::new(&mut c.seconds)
                    .range(0.0..=3600.0)
                    .speed(0.5)
                    .suffix(" s"),
            );
            ui.label(
                RichText::new("after each new item (0: always)")
                    .small()
                    .weak(),
            );
        });
    });
    *c != before
}

impl HostPanel {
    /// The GNOME extension's state as shown, without asking GNOME (the book's pictures, drawn
    /// where no GNOME session can be reached).
    #[cfg(test)]
    pub(super) fn test_extension(&mut self, s: ExtensionState) {
        self.extension = Some(s);
    }

    fn ask(&mut self, args: Value, now: f64) {
        let (tx, rx) = channel();
        self.asked = now;
        self.rx = Some(rx);
        let _ = std::thread::Builder::new()
            .name("jetcarp-host-ask".into())
            .spawn(move || {
                let _ = tx.send(host_call(&args));
            });
    }

    fn poll(&mut self, now: f64) {
        if let Some(rx) = &self.rx
            && let Ok(r) = rx.try_recv()
        {
            self.rx = None;
            if let Ok(v) = &r {
                self.login = v["run_at_login"].as_bool();
            }
            self.status = Some(r);
            self.settings = None;
        }
        if self.rx.is_none() && now - self.asked >= POLL_EVERY {
            self.ask(json!({"action": "status"}), now);
        }
    }

    /// Finds the extension's state, doing `work` first.
    fn check_extension(&mut self, work: fn() -> Result<(), String>) {
        let (tx, rx) = channel();
        self.extension_rx = Some(rx);
        let _ = std::thread::Builder::new()
            .name("jetcarp-gnome-state".into())
            .spawn(move || {
                let _ = tx.send(work().map(|()| gnome::state()));
            });
    }

    fn poll_extension(&mut self) {
        if self.extension.is_none() && self.extension_rx.is_none() {
            self.check_extension(|| Ok(()));
        }
        if let Some(rx) = &self.extension_rx
            && let Ok(r) = rx.try_recv()
        {
            self.extension_rx = None;
            match r {
                Ok(s) => self.extension = Some(s),
                Err(e) => self.note = Some(e),
            }
        }
    }

    pub(super) fn running(&self) -> Option<&Value> {
        self.status.as_ref()?.as_ref().ok()
    }

    /// The host's settings as the pane shows them (read from `host.json` when not yet).
    pub(super) fn settings_mut(&mut self) -> &mut HostSettings {
        self.settings.get_or_insert_with(|| HostSettings::load().0)
    }

    /// The GPU the desktop background draws and steps on: the running host's, else the one the
    /// GPU policy picks for a display.
    fn adapter(&mut self) -> Option<String> {
        if let Some(name) = self.running().and_then(|v| v["gpu"].as_str()) {
            return Some(name.to_string());
        }
        if self.adapter.is_none() && self.adapter_rx.is_none() {
            let (tx, rx) = channel();
            self.adapter_rx = Some(rx);
            let _ = std::thread::Builder::new()
                .name("jetcarp-gpu-pick".into())
                .spawn(move || {
                    let instance = wgpu::Instance::new(
                        wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
                    );
                    let name = match select::pick(&instance, select::Purpose::Display, None) {
                        Ok(c) => c.label(),
                        Err(e) => format!("none ({e})"),
                    };
                    let _ = tx.send(name);
                });
        }
        if let Some(rx) = &self.adapter_rx
            && let Ok(name) = rx.try_recv()
        {
            self.adapter = Some(name);
            self.adapter_rx = None;
        }
        self.adapter.clone()
    }
}

fn summary(v: &Value) -> String {
    let screens = v["screens"].as_array().map_or(0, Vec::len);
    let mut lines = vec![];
    for (i, sc) in v["screens"].as_array().into_iter().flatten().enumerate() {
        let s = &sc["stage"];
        let mut t = format!(
            "{}{}: {} ({} of {})",
            if screens > 1 {
                format!("Screen {}: ", i + 1)
            } else {
                String::new()
            },
            s["rotation"].as_str().unwrap_or("?"),
            s["label"].as_str().unwrap_or("starting"),
            s["number"],
            s["count"]
        );
        if let Some(px) = s["cell_px"].as_f64().filter(|p| *p > 0.0) {
            t.push_str(&format!(
                ", {} px cells",
                jetcarp_display::host_settings::round_px(px as f32)
            ));
            if s["tuned"] == true {
                t.push_str(" (tuned)");
            }
            if let Some(fps) = s["frame_rate"].as_f64() {
                t.push_str(&format!(", {} fps", (fps * 100.0).round() / 100.0));
            }
        }
        if s["paused"] == true {
            // Frozen by a hold (its reason is the pane's Held line) or paused by hand (#76).
            let held = v["held"].as_array().is_some_and(|h| !h.is_empty());
            t.push_str(if held && v["paused"] != true {
                ", held"
            } else {
                ", paused"
            });
        }
        lines.push(t);
    }
    let words = |key: &str| -> Vec<&str> {
        v[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    let why = words("paused_why");
    if !why.is_empty() {
        lines.push(format!("Paused: {}.", why.join("; ")));
    }
    // A hold passes by itself; Resume is for a pause (#76).
    let held = words("held_why");
    if !held.is_empty() {
        lines.push(format!("Held: {} (resumes by itself).", held.join("; ")));
    }
    lines.join("\n")
}

/// A monitor choice's words.
fn monitor_label(names: &[String], m: Option<usize>) -> String {
    match m {
        None => "Every monitor".into(),
        Some(i) => match names.get(i) {
            Some(n) => format!("{i}: {n}"),
            None => format!("{i} (not connected)"),
        },
    }
}

impl JetcarpApp {
    /// The scenes rotation `name` shows (its items' labels, as the host and the tuner name them),
    /// from the rotation and profile stores, expanded again only when the rotation changes. None
    /// for a rotation that can't be found here.
    fn rotation_labels(&mut self, name: &str) -> Option<Vec<String>> {
        let r = self.rotation_store().get(name)?;
        let key = r.to_json();
        if let Some((k, labels)) = self.display.host.scene_labels.get(name)
            && *k == key
        {
            return Some(labels.clone());
        }
        let store = self.profile_store();
        let (slots, _) = r.expand(&jetcarp_display::rotation::default_base(), &|n| {
            store.get(n)
        });
        let labels: Vec<String> = slots.into_iter().map(|s| s.label).collect();
        self.display
            .host
            .scene_labels
            .insert(name.to_string(), (key, labels.clone()));
        Some(labels)
    }

    /// The monitors' names, from the host (whose numbering the settings use), else this window's.
    fn host_monitor_names(&self) -> Vec<String> {
        let from_host: Vec<String> = self
            .display
            .host
            .running()
            .and_then(|v| v["monitors"].as_array())
            .map(|a| {
                a.iter()
                    .map(|n| n.as_str().unwrap_or("?").to_string())
                    .collect()
            })
            .unwrap_or_default();
        if !from_host.is_empty() {
            return from_host;
        }
        self.shell
            .monitors
            .list()
            .iter()
            .map(|m| m.describe())
            .collect()
    }

    pub(super) fn desktop_pane(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.label(
            RichText::new(
                "The background host runs scenes as its own program, behind your windows or in a \
                 window, and keeps going when this window closes.",
            )
            .small()
            .weak(),
        );
        let rotations = self.rotation_store().names();
        let profile_names = match self.display.host_rotation_ui.host_screen() {
            Some(_) => self.profile_store().names(),
            None => vec![],
        };
        let scene = (self.screen == Screen::Sim && !self.world.cas.is_empty())
            .then(|| self.profile_base_name());
        let monitors = self.host_monitor_names();
        let shared_was = crate::simgpu::shared_mode();
        let mut shared = shared_was;
        let h = &mut self.display.host;
        h.poll(now);
        h.poll_tune(now);
        let view = h_tune_view(h);
        let adapter = h.adapter();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(if h.feed.is_some() {
                0.25
            } else {
                POLL_EVERY
            }));
        if cfg!(target_os = "linux") {
            h.poll_extension();
        }
        if h.settings.is_none() {
            // Read again below, as are the rotations' scenes (a profile may have gone since).
            h.scene_labels.clear();
        }
        let settings = h.settings.get_or_insert_with(|| HostSettings::load().0);
        let mut changed = false;
        let running = h.status.as_ref().and_then(|r| r.as_ref().ok()).cloned();
        let mut ask: Option<Value> = None;
        let mut start = false;
        // ---------------------------------------------- the host
        ui.horizontal(|ui| {
            match &running {
                Some(_) => {
                    if ui.button("Stop").clicked() {
                        ask = Some(json!({"action": "quit"}));
                    }
                    if ui
                        .small_button("◀")
                        .on_hover_text("Previous item")
                        .accessible_name("Previous item")
                        .clicked()
                    {
                        ask = Some(json!({"action": "previous"}));
                    }
                    if ui
                        .small_button("▶")
                        .on_hover_text("Next item")
                        .accessible_name("Next item")
                        .clicked()
                    {
                        ask = Some(json!({"action": "next"}));
                    }
                }
                None => start = ui.button(RichText::new("Start").strong()).clicked(),
            }
            if let Some(v) = &running {
                let paused = v["paused"] == true;
                if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                    ask = Some(json!({"action": if paused { "resume" } else { "pause" }}));
                }
            }
        });
        ui.label(
            RichText::new(
                running
                    .as_ref()
                    .map_or_else(|| "Not running.".to_string(), summary),
            )
            .small(),
        );
        // ---------------------------------------------- where
        ui.separator();
        let wallpaper = settings.screens.iter().all(|s| s.place == Place::Wallpaper);
        let mut on = wallpaper;
        if ui
            .checkbox(&mut on, RichText::new("Show on the desktop").strong())
            .on_hover_text("Behind your windows, as the desktop background (else in a window)")
            .changed()
        {
            let place = if on { Place::Wallpaper } else { Place::Window };
            settings.screens.iter_mut().for_each(|s| s.place = place);
            changed = true;
        }
        let mut remove = None;
        let count = settings.screens.len();
        // The screen whose rotation is open in the editor, which shows under it.
        let editing = self
            .display
            .host_rotation_ui
            .host_screen()
            .filter(|&i| i < count);
        let mut rotation_do = None;
        for (i, screen) in settings.screens.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if count > 1 {
                    ui.label(RichText::new(format!("Screen {}", i + 1)).strong());
                }
                ui.label("Rotation");
                egui::ComboBox::from_id_salt(("host-rotation", i))
                    .selected_text(screen.rotation.as_str())
                    .show_ui(ui, |ui| {
                        for n in &rotations {
                            changed |= ui
                                .selectable_value(&mut screen.rotation, n.clone(), n)
                                .changed();
                        }
                    });
                ui.label("on");
                let picked = egui::ComboBox::from_id_salt(("host-monitor", i))
                    .selected_text(monitor_label(&monitors, screen.monitor))
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                &mut screen.monitor,
                                None,
                                monitor_label(&monitors, None),
                            )
                            .on_hover_text(
                                "As a desktop background: every monitor no other screen claims. \
                                 In a window: a window of its own",
                            )
                            .changed();
                        for k in 0..monitors.len().max(screen.monitor.map_or(0, |m| m + 1)) {
                            changed |= ui
                                .selectable_value(
                                    &mut screen.monitor,
                                    Some(k),
                                    monitor_label(&monitors, Some(k)),
                                )
                                .changed();
                        }
                    });
                crate::a11y::name(&picked.response, format!("Screen {}'s monitor", i + 1));
                if count > 1
                    && ui
                        .small_button("×")
                        .on_hover_text("Remove this screen")
                        .accessible_name("Remove this screen")
                        .clicked()
                {
                    remove = Some(i);
                }
            });
            let listed = rotations.contains(&screen.rotation);
            let rot = &mut self.display.host_rotation_ui;
            if let Some(a) = super::rotation_ui::host_rotation_buttons(
                ui,
                listed,
                scene.as_deref(),
                editing.is_some(),
            ) {
                rotation_do = Some((i, a));
            }
            if editing == Some(i) {
                let tip = "Save it, and show it on this screen";
                if let Some(a) = super::rotation_ui::editor_block(ui, rot, &profile_names, tip) {
                    rotation_do = Some((i, a));
                }
                ui.separator();
            }
        }
        self.display.host_rotation_ui.note_ui(ui);
        if let Some(i) = remove {
            settings.screens.remove(i);
            self.display.host_rotation_ui.screen_removed(i);
            changed = true;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .small_button("Add a screen")
                .on_hover_text("Another scene, on another monitor, with a rotation of its own")
                .clicked()
            {
                let place = if on { Place::Wallpaper } else { Place::Window };
                let next = (0..monitors.len())
                    .find(|m| !settings.screens.iter().any(|s| s.monitor == Some(*m)));
                settings.screens.push(ScreenSettings {
                    monitor: next,
                    place,
                    ..Default::default()
                });
                changed = true;
            }
        });
        let mut edit_rotations = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(
                    "Rotations and profiles are shared with the screensaver. Profiles are made \
                     and edited in its pane:",
                )
                .small()
                .weak(),
            );
            edit_rotations = ui
                .small_button("Screensaver pane…")
                .on_hover_text("Make and edit profiles (View → Screensaver settings…)")
                .clicked();
        });
        // A rotation edited, made or chosen here: saved, and chosen for its screen.
        if let Some((i, a)) = rotation_do {
            let ctx = ui.ctx().clone();
            changed |= self.rotation_do(super::rotation_ui::Target::Host(i), a, &ctx);
        }
        // The scenes each screen's rotation shows now: only theirs get rows in Fit's cards (#73).
        let screen_rotations: Vec<String> =
            self.display.host.settings.as_ref().map_or(vec![], |s| {
                s.screens.iter().map(|x| x.rotation.clone()).collect()
            });
        let shown: Vec<Option<Vec<String>>> = screen_rotations
            .iter()
            .map(|r| self.rotation_labels(r))
            .collect();
        let h = &mut self.display.host;
        let settings = h.settings.as_mut().expect("loaded above");
        // ---------------------------------------------- the desktop
        ui.add_enabled_ui(on, |ui| {
            changed |= ui
                .checkbox(
                    &mut settings.still_picture,
                    "Leave a still picture when it stops",
                )
                .on_hover_text(
                    "Sets a picture of the scene as the desktop background when the host stops, \
                     or while the live one can't be shown. Your own picture comes back when this \
                     or Show on the desktop is turned off",
                )
                .changed();
        });
        let mut install = false;
        if cfg!(target_os = "linux") {
            ui.add_enabled_ui(on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("On the lock screen");
                    egui::ComboBox::from_id_salt("host-lock")
                        .selected_text(settings.lock_screen.label())
                        .show_ui(ui, |ui| {
                            for k in LockScreen::ALL {
                                changed |= ui
                                    .selectable_value(&mut settings.lock_screen, k, k.label())
                                    .changed();
                            }
                        });
                });
                if settings.lock_screen == LockScreen::Screensaver {
                    ui.horizontal(|ui| {
                        ui.label("Let the screen sleep after");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut settings.lock_awake_minutes)
                                    .range(0..=MAX_AWAKE_MINUTES)
                                    .suffix(" min"),
                            )
                            .changed();
                        ui.label(RichText::new("(0: never)").small().weak());
                    });
                    ui.label(
                        RichText::new(
                            "Until then the screen and the computer stay awake while locked.",
                        )
                        .small()
                        .weak(),
                    );
                }
            });
            ui.horizontal_wrapped(|ui| {
                if let Some(state) = &h.extension {
                    ui.label(RichText::new(state.describe()).small());
                    if matches!(
                        state,
                        ExtensionState::NotInstalled | ExtensionState::Inactive(_)
                    ) {
                        install = ui.small_button("Install").clicked();
                    }
                }
            });
        }
        // ---------------------------------------------- how it runs
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Frames per second").on_hover_text(
                "Every screen draws at least this often; how a faster scene draws is under \
                 Performance below",
            );
            egui::ComboBox::from_id_salt("host-fps")
                .selected_text(format!("{}", settings.fps))
                .show_ui(ui, |ui| {
                    for f in FPS_CHOICES {
                        changed |= ui
                            .selectable_value(&mut settings.fps, f, format!("{f}"))
                            .changed();
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("GPU simulation");
            let label = crate::simgpu::mode_label;
            egui::ComboBox::from_id_salt("host-gpu")
                .selected_text(match settings.gpu {
                    Own::Follow => format!("Follow the shared setting ({})", label(shared)),
                    Own::On => "On".into(),
                    Own::Off => "Off".into(),
                })
                .show_ui(ui, |ui| {
                    for (o, text, tip) in [
                        (
                            Own::Follow,
                            format!("Follow the shared setting ({})", label(shared)),
                            "Whatever the shared setting below says",
                        ),
                        (
                            Own::On,
                            "On".into(),
                            "Every world the GPU can run steps there, whatever the shared setting says",
                        ),
                        (
                            Own::Off,
                            "Off".into(),
                            "Every world steps on the processor, whatever the shared setting says",
                        ),
                    ] {
                        changed |= ui
                            .selectable_value(&mut settings.gpu, o, text)
                            .on_hover_text(tip)
                            .changed();
                    }
                })
                .response
                .on_hover_text(
                    "The desktop background's own choice. The results are the processor's to the \
                     bit either way",
                );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Shared setting").small());
            egui::ComboBox::from_id_salt("host-gpu-shared")
                .selected_text(crate::simgpu::mode_label(shared))
                .show_ui(ui, |ui| {
                    for (m, tip) in [
                        (
                            Mode::Auto,
                            "Large worlds step on the GPU, small ones on the processor",
                        ),
                        (Mode::On, "Every world the GPU can run steps there"),
                        (Mode::Off, "Every world steps on the processor"),
                    ] {
                        ui.selectable_value(&mut shared, m, crate::simgpu::mode_label(m))
                            .on_hover_text(tip);
                    }
                });
            ui.label(
                RichText::new(
                    "for the app, the desktop background, the command line and the screen savers, \
                     unless they have a choice of their own (JETCARP_GPU comes first)",
                )
                .small()
                .weak(),
            );
        });
        egui::CollapsingHeader::new("Caption")
            .id_salt("host-caption")
            .show(ui, |ui| {
                changed |= caption_ui(ui, &mut settings.caption);
            });
        changed |= ui
            .checkbox(&mut settings.pause_on_battery, "Pause on battery")
            .changed();
        ui.horizontal_wrapped(|ui| {
            let mut on = settings.busy_percent > 0;
            if ui
                .checkbox(&mut on, "Pause while the processor is busier than")
                .on_hover_text("For 15 seconds, as during research runs. Carries on once it is 20 points lower")
                .changed()
            {
                settings.busy_percent = if on { 80 } else { 0 };
                changed = true;
            }
            // Added to the wrapping row itself, so in a narrow pane it goes to the next line rather
            // than past the pane's edge, out of sight.
            let mut p = settings.busy_percent.max(1);
            let busy = ui.add_enabled(
                on,
                egui::DragValue::new(&mut p).range(30..=100).suffix(" %"),
            );
            crate::a11y::name(&busy, "Pause while the processor is busier than, in per cent");
            if busy.changed() && on {
                settings.busy_percent = p;
                changed = true;
            }
        });
        let mut login = *h.login.get_or_insert_with(autostart::enabled);
        if ui.checkbox(&mut login, "Start at login").changed() {
            let result = autostart::login_command().and_then(|cmd| autostart::set(login, &cmd));
            match result {
                Ok(()) => {
                    h.login = Some(login);
                    // And registered with launchd now, on macOS, as `host login on` does.
                    if login {
                        crate::desktop_offers::refresh_login_item();
                    }
                }
                Err(e) => h.note = Some(e),
            }
        }
        // ---------------------------------------------- performance
        ui.separator();
        let crops = &mut h.crops;
        let scrolled = &mut h.scrolled;
        let mut crop = |ctx: &egui::Context, name: &str| crop_texture(crops, ctx, name);
        let mut first_seen = |key: &str| scrolled.insert(key.to_string());
        let (start_tune, stop_tune, forget) = performance_ui(
            ui,
            view,
            adapter.as_deref(),
            settings,
            &shown,
            &mut changed,
            &mut crop,
            &mut first_seen,
        );
        // ---------------------------------------------- apply
        if forget {
            settings.screens.iter_mut().for_each(|s| s.tuning = None);
            changed = true;
        }
        let h = &mut self.display.host;
        let settings = h.settings.as_mut().expect("loaded above");
        if changed {
            match settings.save() {
                Ok(()) => h.reload_at = Some(now + RELOAD_AFTER),
                Err(e) => h.note = Some(e),
            }
            // The host puts the desktop's own picture back itself; with it stopped, this does.
            if running.is_none()
                && let Err(e) = jetcarp_display::desktop_picture::settle(settings)
            {
                h.note = Some(e);
            }
        }
        if start_tune {
            let l = settings.tune;
            h.tune_note = None;
            let req = TuneRequest {
                cpu_percent: Some(l.cpu_percent),
                gpu_percent: Some(l.gpu_percent),
                fps: l.fps,
                ..Default::default()
            };
            match Feed::start(&req, running.is_some(), now) {
                Ok(f) => h.feed = Some(f),
                Err(e) => h.tune_note = Some((e, true)),
            }
        }
        if stop_tune && let Some(mut f) = h.feed.take() {
            f.stop();
            h.tune_note = Some(("Stopped; nothing was saved.".into(), false));
            h.settings = None;
        }
        if install {
            h.note = None;
            h.check_extension(gnome::install);
        }
        if h.reload_at.is_some_and(|t| now >= t) && h.rx.is_none() && h.running().is_some() {
            h.reload_at = None;
            ask = ask.or(Some(json!({"action": "reload"})));
        }
        if start {
            let result = autostart::host_command().and_then(|cmd| cmd.spawn_detached());
            match result {
                Ok(()) => h.note = None,
                Err(e) => h.note = Some(e),
            }
            h.asked = now - POLL_EVERY + 0.5;
        }
        if let Some(a) = ask {
            h.rx = None;
            h.ask(a, now);
        }
        if let Some(n) = &h.note {
            ui.colored_label(crate::theme::ink(ui, WARN), n);
        }
        if edit_rotations {
            self.shell.layout.show_pane("screensaver");
        }
        if shared != shared_was {
            self.set_shared_gpu(shared);
            // The host reads the shared setting again.
            self.display.host.reload_at = Some(now + RELOAD_AFTER);
        }
    }

    /// The system's screen saver (Windows and macOS), which the host's program runs: its rotation
    /// and the button that installs it. In the Screensaver pane.
    pub(super) fn system_saver_ui(&mut self, ui: &mut egui::Ui) {
        if !jetcarp_display::saver::AVAILABLE {
            return;
        }
        let rotations = self.rotation_store().names();
        let h = &mut self.display.host;
        let settings = h.settings.get_or_insert_with(|| HostSettings::load().0);
        let mut changed = false;
        let mut install_saver = false;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("System screen saver runs");
            let shown = settings
                .saver_rotation
                .clone()
                .unwrap_or_else(|| "the desktop background's rotation".into());
            // Named by its label for screen readers, even when a narrow pane wraps it onto a line
            // of its own (#219).
            let combo = egui::ComboBox::from_id_salt("host-saver")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut settings.saver_rotation,
                            None,
                            "the desktop background's rotation",
                        )
                        .changed();
                    for n in &rotations {
                        changed |= ui
                            .selectable_value(&mut settings.saver_rotation, Some(n.clone()), n)
                            .changed();
                    }
                });
            combo.response.labelled_by(label.id);
            let tip = if jetcarp_display::saver::BAKED {
                "Add JETCARP to macOS's screen savers. The rotation is copied in, so install again after changing it"
            } else {
                "Add JETCARP to Windows' list of screen savers"
            };
            install_saver = ui.small_button("Install").on_hover_text(tip).clicked();
        });
        if changed && let Err(e) = settings.save() {
            h.note = Some(e);
        }
        if install_saver {
            h.note = match jetcarp_display::saver::install() {
                Ok((_, warnings)) if !warnings.is_empty() => Some(warnings.join("; ")),
                Ok(_) => None,
                Err(e) => Some(e),
            };
        }
        if let Some(n) = &h.note {
            ui.colored_label(crate::theme::ink(ui, WARN), n);
        }
    }
}

/// A sweep the pane follows: the host's, on the real screen, or with no host running, one off
/// screen in a program of its own.
pub(super) enum Feed {
    Child(TuneChild),
    Host {
        events: Vec<TuneEvent>,
        rx: Option<Receiver<Result<Value, String>>>,
        asked: f64,
        ended: Option<Result<(), String>>,
    },
}

/// How often the host's sweep is asked for news.
const FEED_EVERY: f64 = 0.3;

impl Feed {
    /// Starts a sweep: the host's when it runs, else one of its own.
    fn start(req: &TuneRequest, host: bool, now: f64) -> Result<Feed, String> {
        if !host {
            return TuneChild::spawn(req).map(Feed::Child);
        }
        let mut ask = json!({"action": "tune"});
        for (k, v) in [
            ("cpu_limit", req.cpu_percent),
            ("gpu_limit", req.gpu_percent),
            ("fps", req.fps),
        ] {
            if let Some(v) = v {
                ask[k] = json!(v);
            }
        }
        host_call(&ask)?;
        Ok(Feed::Host {
            events: vec![],
            rx: None,
            asked: now,
            ended: None,
        })
    }

    fn events(&self) -> &[TuneEvent] {
        match self {
            Feed::Child(c) => &c.events,
            Feed::Host { events, .. } => events,
        }
    }

    /// New news; true when a scene finished (its sizes are saved).
    fn poll(&mut self, now: f64) -> bool {
        let new: Vec<TuneEvent> = match self {
            Feed::Child(c) => c.poll(),
            Feed::Host {
                events,
                rx,
                asked,
                ended,
            } => {
                let mut new = vec![];
                if let Some(r) = rx
                    && let Ok(reply) = r.try_recv()
                {
                    *rx = None;
                    match reply {
                        Ok(v) => {
                            new = serde_json::from_value(v["events"].clone()).unwrap_or_default();
                            events.extend(new.iter().cloned());
                            if v["running"] == false && ended.is_none() {
                                *ended = Some(Ok(()));
                            }
                        }
                        Err(e) => *ended = Some(Err(e)),
                    }
                }
                for e in &new {
                    match e {
                        TuneEvent::Done { .. } => *ended = Some(Ok(())),
                        TuneEvent::Failed { error } => *ended = Some(Err(error.clone())),
                        _ => {}
                    }
                }
                if rx.is_none() && ended.is_none() && now - *asked >= FEED_EVERY {
                    let (tx, r) = channel();
                    let since = events.len();
                    *rx = Some(r);
                    *asked = now;
                    let _ = std::thread::Builder::new()
                        .name("jetcarp-host-sweep".into())
                        .spawn(move || {
                            let _ = tx
                                .send(host_call(&json!({"action": "tune_events", "since": since})));
                        });
                }
                new
            }
        };
        new.iter().any(|e| matches!(e, TuneEvent::Scene { .. }))
    }

    /// How it ended, once it has.
    fn outcome(&self) -> Option<Result<(), String>> {
        match self {
            Feed::Child(c) => c.outcome(),
            Feed::Host { ended, .. } => ended.clone(),
        }
    }

    fn stop(&mut self) {
        match self {
            Feed::Child(c) => c.stop(),
            Feed::Host { .. } => {
                let _ = std::thread::Builder::new()
                    .name("jetcarp-host-ask".into())
                    .spawn(|| host_call(&json!({"action": "tune", "on": false})));
            }
        }
    }

    /// Whether the sweep shows on the desktop (the host's), as opposed to off screen.
    fn on_screen(&self) -> bool {
        matches!(self, Feed::Host { .. })
    }
}

impl HostPanel {
    /// Follows a sweep under way; once it ends, says how and reads the sizes it saved.
    fn poll_tune(&mut self, now: f64) {
        let Some(f) = &mut self.feed else { return };
        if f.poll(now) {
            // A scene's cards are saved: read them.
            self.settings = None;
        }
        if let Some(outcome) = f.outcome() {
            let done = f.events().iter().rev().find_map(|e| match e {
                TuneEvent::Done {
                    scenes,
                    trials,
                    seconds,
                    ..
                } => Some((*scenes, *trials, *seconds)),
                _ => None,
            });
            self.tune_note = Some(match (outcome, done) {
                (Ok(()), Some((scenes, trials, Some(secs)))) if trials > 0 => (
                    format!(
                        "Fitted {scenes} scene(s) in {secs:.0} s, {trials} size(s) tried. Each \
                         shows at its size unless you choose another."
                    ),
                    false,
                ),
                (Ok(()), Some((scenes, _, _))) => (
                    format!(
                        "Placed the sizes the last fit found for {scenes} scene(s): nothing has \
                         changed since."
                    ),
                    false,
                ),
                (Ok(()), None) => ("Fitted.".into(), false),
                (Err(e), _) => (format!("The fit stopped: {e}. Nothing was saved."), true),
            });
            self.feed = None;
            self.settings = None;
            // A new fit's cards are scrolled into view again (#108).
            self.scrolled.clear();
        }
    }
}

/// What the Performance group shows of a fit under way.
struct TuneView {
    running: bool,
    on_screen: bool,
    progress: Option<String>,
    /// Each screen being measured, its scene, and its cards so far: (screen, scene, cards).
    live: Vec<(usize, String, Vec<Rung>)>,
    note: Option<(String, bool)>,
}

fn h_tune_view(h: &HostPanel) -> TuneView {
    let Some(f) = &h.feed else {
        return TuneView {
            running: false,
            on_screen: false,
            progress: None,
            live: vec![],
            note: h.tune_note.clone(),
        };
    };
    let events = f.events();
    let progress = events
        .iter()
        .rev()
        .find(|e| matches!(e, TuneEvent::Rung { .. } | TuneEvent::Start { .. }))
        .map(TuneEvent::describe);
    // Each measured screen's cards so far: every screen is measured at once (#113).
    let mut live: Vec<(usize, String, Vec<Rung>)> = vec![];
    for e in events {
        match e {
            TuneEvent::Rung { screen, scene, .. } => {
                if !live.iter().any(|(s, _, _)| s == screen) {
                    live.push((*screen, scene.clone(), vec![]));
                }
            }
            TuneEvent::Measured { screen, rung, .. } => {
                if let Some((_, _, cards)) = live.iter_mut().find(|(s, _, _)| s == screen) {
                    cards.push(rung.clone());
                }
            }
            TuneEvent::Scene { screen, .. } => live.retain(|(s, _, _)| s != screen),
            _ => {}
        }
    }
    for (_, _, cards) in &mut live {
        cards.sort_by(|a, b| a.size.px.total_cmp(&b.size.px));
    }
    TuneView {
        running: true,
        on_screen: f.on_screen(),
        progress,
        live,
        note: None,
    }
}

/// What a click in the cards asks for.
enum CardDo {
    /// Show screen `0`'s scene `1` at this size (the measured scene's: the rest follow it).
    Choose(usize, String, Size),
    /// Show every scene on this tiling (screen, tiling) at this cell size.
    AllOnTiling(usize, String, f32),
    /// Every scene nobody chose shows at its recommended size, now as a choice.
    RecommendedForRest,
}

/// How the limits are shared between the screens, for the pane (#107): a line for each screen
/// fitted, once there is more than one.
fn share_lines(settings: &HostSettings) -> Vec<String> {
    let fitted: Vec<(usize, &Tuning)> = settings
        .screens
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.tuning
                .as_ref()
                .filter(|t| t.rotation == s.rotation && t.share > 0.0)
                .map(|t| (i, t))
        })
        .collect();
    if fitted.len() < 2 {
        return vec![];
    }
    let l = &settings.tune;
    fitted
        .iter()
        .map(|(i, t)| {
            format!(
                "Screen {}: {} × {}, {:.0}% of the area: processor {}, GPU {}",
                i + 1,
                t.canvas[0],
                t.canvas[1],
                t.share * 100.0,
                tuning::percent(l.cpu_percent as f64 / 100.0 * t.share),
                tuning::percent(l.gpu_percent as f64 / 100.0 * t.share),
            )
        })
        .collect()
}

/// The pacing, then the limits, *Fit to this computer*, and the cards (these three not while Fit
/// is switched off, #112). Returns whether to start a fit, stop the one running, or forget the
/// sizes found; a card or size chosen is put in `settings` (`changed`).
/// `shown` gives the scenes each screen's rotation shows now: only they get rows (#73).
/// `first_seen` says whether a row of cards is new, so it is scrolled to its chosen card (#108).
#[allow(clippy::too_many_arguments)]
fn performance_ui(
    ui: &mut egui::Ui,
    view: TuneView,
    adapter: Option<&str>,
    settings: &mut HostSettings,
    shown: &[Option<Vec<String>>],
    changed: &mut bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
    first_seen: &mut dyn FnMut(&str) -> bool,
) -> (bool, bool, bool) {
    let (mut start, mut stop, mut forget) = (false, false, false);
    ui.label(RichText::new("Performance").strong());
    ui.label(
        RichText::new(format!(
            "GPU: {}",
            adapter.unwrap_or("finding the GPU the desktop background uses…")
        ))
        .small()
        .weak(),
    )
    .on_hover_text(
        "The adapter JETCARP's GPU setting picks for a display (jetcarp doctor shows why)",
    );
    // How often each screen draws against its scene's speed (#42), and the most it draws at.
    for p in Pacing::ALL {
        *changed |= ui
            .radio_value(&mut settings.pacing, p, p.label())
            .on_hover_text(p.hint())
            .changed();
    }
    ui.add_enabled_ui(settings.pacing == Pacing::EveryGeneration, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.label("Up to");
            let words = |m: Option<f32>| match m {
                Some(r) => format!("{} frames a second", r.round()),
                None => "the monitor's refresh rate".to_string(),
            };
            egui::ComboBox::from_id_salt("host-max-fps")
                .selected_text(words(settings.max_fps))
                .show_ui(ui, |ui| {
                    let mut choices = vec![Some(DEFAULT_MAX_FPS), None];
                    if !choices.contains(&settings.max_fps) {
                        choices.insert(1, settings.max_fps);
                    }
                    for c in choices {
                        *changed |= ui
                            .selectable_value(&mut settings.max_fps, c, words(c))
                            .changed();
                    }
                })
                .response
                .on_hover_text(
                    "The most frames a second a screen draws at. 60 shows every generation of a \
                     scene up to 60 generations a second; the monitor's refresh rate (144 on some \
                     laptops) draws faster scenes faster, and works the GPU harder \
                     (jetcarp host max-fps sets any rate)",
                );
        });
    });
    // Fit to this computer is switched off in 0.4.2 (#112): no limits, no button, no cards. The
    // sizes an earlier Fit saved stay in the settings, unused.
    if !jetcarp_core::fit::FIT {
        ui.label(
            RichText::new(
                "Each scene shows at its own cell size, which its profile or its rotation sets.",
            )
            .small()
            .weak(),
        );
        return (false, false, false);
    }
    let l = &mut settings.tune;
    egui::Grid::new("host-tune-limits")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Processor limit");
            *changed |= ui
                .add(
                    egui::DragValue::new(&mut l.cpu_percent)
                        .range(TuneLimits::CPU_RANGE)
                        .speed(0.2)
                        .max_decimals(1)
                        .suffix(" %"),
                )
                .on_hover_text(
                    "JETCARP's own processor time, as a share of the whole machine (every core)",
                )
                .changed();
            ui.end_row();
            ui.label("GPU limit");
            *changed |= ui
                .add(
                    egui::DragValue::new(&mut l.gpu_percent)
                        .range(TuneLimits::GPU_RANGE)
                        .speed(0.2)
                        .max_decimals(1)
                        .suffix(" %"),
                )
                .on_hover_text("JETCARP's own share of the graphics card's time")
                .changed();
            ui.end_row();
            ui.label("Smooth at");
            ui.horizontal(|ui| {
                let mut auto = l.fps.is_none();
                if ui
                    .checkbox(&mut auto, "the monitor's rate")
                    .on_hover_text(
                        "The monitor's refresh rate (or 60), at most the rate each scene draws at",
                    )
                    .changed()
                {
                    l.fps = if auto { None } else { Some(settings.fps) };
                    *changed = true;
                }
                if let Some(f) = &mut l.fps {
                    *changed |= ui
                        .add(
                            egui::DragValue::new(f)
                                .range(TuneLimits::FPS_RANGE)
                                .speed(0.5)
                                .suffix(" fps"),
                        )
                        .changed();
                }
            });
            ui.end_row();
        });
    // One set of limits for the whole desktop, shared by pixel area (#107).
    ui.label(
        RichText::new(
            "The limits are for the whole desktop: with several screens, each gets a share by its \
             pixel area.",
        )
        .small()
        .weak(),
    );
    for line in share_lines(settings) {
        ui.label(RichText::new(line).small());
    }
    ui.horizontal_wrapped(|ui| {
        if view.running {
            crate::a11y::spinner(ui);
            if ui.button("Cancel").clicked() {
                stop = true;
            }
        } else if ui
            .button(RichText::new("Fit to this computer…").strong())
            .on_hover_text(
                "Measure the most demanding scene of each screen's rotation at a few sizes, about \
                 a second each and every screen at once (a few seconds), within the processor \
                 and GPU limits at a smooth frame rate. The size found serves the whole rotation: \
                 a cheaper scene goes finer. The rules and colours stay as they are",
            )
            .clicked()
        {
            start = true;
        }
        let tuned = settings.screens.iter().any(|s| s.tuning.is_some());
        if !view.running
            && tuned
            && ui
                .small_button("Use the scenes' own sizes")
                .on_hover_text("Forget the fit; each scene shows at its own cell size")
                .clicked()
        {
            forget = true;
        }
    });
    *changed |= ui
        .checkbox(
            &mut settings.tune.auto,
            "Measure again by itself when a monitor changes",
        )
        .on_hover_text(
            "Once fitted: only the screens whose monitor changed (its size, scale or refresh rate) \
             are measured again; a size you chose is kept while it is still within the limits, \
             else the recommended one shows. Not while on battery or busy",
        )
        .changed();
    if view.running {
        if let Some(p) = &view.progress {
            ui.label(RichText::new(p).small());
        }
        ui.label(
            RichText::new(if view.on_screen {
                "Each size shows on the desktop as it is measured."
            } else {
                "With the host stopped, the sizes are measured off screen."
            })
            .small()
            .weak(),
        );
    }
    if let Some((t, err)) = &view.note {
        let c = if *err {
            Color32::from_rgb(255, 110, 110)
        } else {
            ui.visuals().weak_text_color()
        };
        ui.label(RichText::new(t).small().color(crate::theme::ink(ui, c)));
    }
    // The cards: each screen's measured scene (choosable; the rest follow it), the other scenes'
    // sizes, and the screens being measured (not yet).
    let ppp = ui.ctx().pixels_per_point();
    let ctx = ui.ctx().clone();
    let mut todo: Option<CardDo> = None;
    let mut any = false;
    let screens = settings.screens.len();
    for (i, s) in settings.screens.iter().enumerate() {
        let Some(t) = s.tuning.as_ref().filter(|t| t.rotation == s.rotation) else {
            continue;
        };
        // A screen being measured shows its cards so far, below.
        if view.live.iter().any(|(sc, _, _)| *sc == i) {
            continue;
        }
        // Only the scenes of the rotation shown: a size fitted while another was shown stays
        // saved, but is not one of these (#73).
        let labels = shown.get(i).and_then(Option::as_deref);
        let scenes: Vec<&TunedScene> = t.scenes_shown(labels).collect();
        if scenes.is_empty() || scenes.iter().all(|x| x.rungs.is_empty() && x.cost == 0.0) {
            continue;
        }
        any = true;
        ui.horizontal_wrapped(|ui| {
            if screens > 1 {
                ui.label(RichText::new(format!("Screen {}", i + 1)).strong());
            }
            if !view.running
                && scenes.iter().any(|x| !x.chosen)
                && ui
                    .small_button("Use the recommended size for the rest")
                    .on_hover_text(
                        "Every scene you have not chosen a size for keeps its recommended size, \
                         as your choice",
                    )
                    .clicked()
            {
                todo = Some(CardDo::RecommendedForRest);
            }
        });
        // Said once for the screen; each card shows the share as unknown (#72).
        if scenes.iter().any(|x| gpu_unmeasured(x)) {
            ui.label(
                RichText::new(GPU_UNMEASURED)
                    .small()
                    .color(crate::theme::ink(ui, WARN)),
            );
        }
        // The measured scene's cards (and a fit's from before 0.4.3: every scene's).
        let measured = scenes.iter().any(|x| x.measured);
        for scene in scenes.iter().filter(|x| !x.rungs.is_empty()) {
            if let Some(d) = scene_row(ui, &ctx, ppp, i, scene, !view.running, crop, first_seen) {
                todo = Some(d);
            }
        }
        let rest: Vec<&TunedScene> = scenes
            .iter()
            .filter(|x| x.rungs.is_empty())
            .copied()
            .collect();
        if !rest.is_empty() {
            ui.label(
                RichText::new(if measured {
                    "The other scenes take their sizes from the scene measured on their device, \
                     by their cost, at most twice its cells unless you choose:"
                } else {
                    "The other scenes:"
                })
                .small()
                .weak(),
            );
            if let Some(d) = other_scenes(ui, i, t, &rest, !view.running) {
                todo = Some(d);
            }
        }
    }
    for (i, name, cards) in &view.live {
        any = true;
        let scene = TunedScene {
            scene: name.clone(),
            rungs: cards.clone(),
            ..Default::default()
        };
        ui.label(
            RichText::new(if screens > 1 {
                format!("Screen {}: measuring…", i + 1)
            } else {
                "Measuring…".into()
            })
            .small()
            .weak(),
        );
        if gpu_unmeasured(&scene) {
            ui.label(
                RichText::new(GPU_UNMEASURED)
                    .small()
                    .color(crate::theme::ink(ui, WARN)),
            );
        }
        let _ = scene_row(ui, &ctx, ppp, *i, &scene, false, crop, &mut |_| false);
    }
    if !any && !view.running {
        ui.label(
            RichText::new("Not fitted: each scene shows at its own cell size.")
                .small()
                .weak(),
        );
    }
    match todo {
        Some(CardDo::Choose(i, name, size)) => {
            if let Some(t) = settings.screens[i].tuning.as_mut()
                && let Some(scene) = t.scenes.iter_mut().find(|x| x.scene == name)
            {
                let own = scene.rungs.iter().any(|r| r.own && r.size.same(&size))
                    || (size.level.is_none() && (size.px - scene.design_px).abs() < 1e-3);
                scene.set_size(if own { Size::px(scene.design_px) } else { size });
                scene.chosen = true;
                scene.note = None;
                // The measured scene's size serves the rest of the rotation.
                if scene.measured {
                    tuning::rederive(t);
                }
                *changed = true;
            }
        }
        Some(CardDo::AllOnTiling(i, tiling, px)) => {
            let labels = shown.get(i).and_then(Option::as_deref);
            if let Some(t) = settings.screens[i].tuning.as_mut() {
                for scene in t.scenes.iter_mut().filter(|x| {
                    x.tiling == tiling
                        && x.level.is_none()
                        && x.recommended.level.is_none()
                        && Tuning::shows(&x.scene, labels)
                }) {
                    scene.set_size(Size::px(px));
                    scene.chosen = true;
                    scene.note = None;
                }
                *changed = true;
            }
        }
        Some(CardDo::RecommendedForRest) => {
            // The scenes with rows: those of the rotation each screen shows.
            for (i, s) in settings.screens.iter_mut().enumerate() {
                let labels = shown.get(i).and_then(Option::as_deref);
                let rotation = &s.rotation;
                let scenes = s
                    .tuning
                    .iter_mut()
                    .filter(|t| t.rotation == *rotation)
                    .flat_map(|t| t.scenes.iter_mut());
                for scene in scenes {
                    if !scene.chosen && Tuning::shows(&scene.scene, labels) {
                        let r = scene.recommended;
                        let own = scene.rungs.iter().any(|x| x.own && x.size.same(&r));
                        scene.set_size(if own { Size::px(scene.design_px) } else { r });
                        scene.chosen = true;
                    }
                }
            }
            *changed = true;
        }
        None => {}
    }
    (start, stop, forget)
}

/// The key a row of cards is scrolled into view by once (#108): a new fit, or another size
/// chosen, scrolls it again.
fn row_key(screen: usize, scene: &TunedScene) -> String {
    format!(
        "{screen}|{}|{}|{:?}|{:?}",
        scene.scene,
        scene.rungs.len(),
        scene.size(),
        scene.recommended
    )
}

/// One scene's cards in a row, its name and note above. `choosable`: a click chooses a card. The
/// row scrolls sideways (a trackpad's sideways swipe, or shift and the wheel), and the first time
/// it is seen (`first_seen`) it is scrolled to the chosen card, else the recommended one (#108).
#[allow(clippy::too_many_arguments)]
fn scene_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ppp: f32,
    screen: usize,
    scene: &TunedScene,
    choosable: bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
    first_seen: &mut dyn FnMut(&str) -> bool,
) -> Option<CardDo> {
    let mut todo = None;
    let showing = scene.size();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&scene.scene).strong());
        if !scene.also.is_empty() {
            ui.label(
                RichText::new(format!(
                    "and the {} other rules of its series",
                    scene.also.len()
                ))
                .small(),
            );
        }
        if scene.measured && choosable {
            ui.label(
                RichText::new(if scene.gpu {
                    "measured: the most demanding scene that steps on the GPU"
                } else {
                    "measured: the most demanding scene that steps on the processor"
                })
                .small()
                .weak(),
            )
            .on_hover_text(
                "Fit measures the scene with the most work a cell of those that step on the GPU, \
                 and of those that step on the processor; the size chosen here serves the other \
                 scenes on its device, each cheaper scene going finer by its cost",
            );
        }
        if scene.placed {
            ui.label(
                RichText::new("a placed pattern: its own size unless you choose")
                    .small()
                    .weak(),
            );
        }
        if choosable && scene.chosen {
            ui.label(RichText::new("your choice").small().weak());
        }
        let lattice = scene.rungs.iter().all(|r| r.size.level.is_none());
        if choosable
            && lattice
            && scene.chosen
            && !scene.tiling.is_empty()
            && ui
                .small_button("Use this size for every scene on this tiling")
                .on_hover_text(format!(
                    "Every scene on {} on this screen shows at {}",
                    jetcarp_core::families::family(&scene.tiling)
                        .map(|f| f.name.to_string())
                        .unwrap_or_else(|| scene.tiling.clone()),
                    showing.describe()
                ))
                .clicked()
        {
            todo = Some(CardDo::AllOnTiling(
                screen,
                scene.tiling.clone(),
                showing.px,
            ));
        }
    });
    if let Some(n) = &scene.note {
        ui.label(RichText::new(n).small().color(crate::theme::ink(ui, WARN)));
    }
    let scroll_now = choosable && first_seen(&row_key(screen, scene));
    egui::ScrollArea::horizontal()
        .id_salt(("tune-cards", screen, &scene.scene))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut target: Option<(bool, egui::Response)> = None;
                for r in &scene.rungs {
                    let picked = choosable
                        && (r.size.same(&showing)
                            || (r.own && showing.level.is_none() && showing.px == scene.design_px));
                    let recommended = choosable && r.size.same(&scene.recommended);
                    // A sweep from before 0.4.2 kept the unjudged share as a number.
                    let unknown = r.gpu_unmeasured || scene.gpu_timing.unmeasured();
                    let (clicked, resp) = card(
                        ui,
                        ctx,
                        ppp,
                        r,
                        picked,
                        recommended,
                        unknown,
                        choosable,
                        crop,
                    );
                    if clicked {
                        todo = Some(CardDo::Choose(screen, scene.scene.clone(), r.size));
                    }
                    // The chosen card, else the recommended one.
                    if picked || (recommended && target.as_ref().is_none_or(|t| !t.0)) {
                        target = Some((picked, resp));
                    }
                }
                // Only this row scrolls: a scroll area takes both of a target's directions, so
                // the pane itself stays where it is.
                if scroll_now && let Some((_, resp)) = target {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
            });
        });
    todo
}

/// The scenes a fit sized from the measured one, each with its size and a choice of the others
/// (`choosable`); a placed pattern, or a scene that is not rebuilt to fill the screen, at its
/// own size.
fn other_scenes(
    ui: &mut egui::Ui,
    screen: usize,
    t: &Tuning,
    rest: &[&TunedScene],
    choosable: bool,
) -> Option<CardDo> {
    let mut todo = None;
    egui::Grid::new(("tune-others", screen))
        .num_columns(3)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            for x in rest {
                let name = if x.also.is_empty() {
                    x.scene.clone()
                } else {
                    format!("{} and its series' {} other rules", x.scene, x.also.len())
                };
                ui.label(RichText::new(name).small());
                let size = x.size();
                let sizes = tuning::scene_sizes(t, x);
                let words = |s: &Size| {
                    let mut w = s.describe();
                    if s.same(&x.recommended) {
                        w.push_str(", recommended");
                    }
                    if s.level.is_none() && (s.px - x.design_px).abs() < 1e-3 {
                        w.push_str(", its own");
                    }
                    w
                };
                if choosable && !sizes.is_empty() {
                    egui::ComboBox::from_id_salt(("tune-size", screen, &x.scene))
                        .selected_text(words(&size))
                        .show_ui(ui, |ui| {
                            for s in &sizes {
                                if ui.selectable_label(s.same(&size), words(s)).clicked() {
                                    todo = Some(CardDo::Choose(screen, x.scene.clone(), *s));
                                }
                            }
                        });
                } else {
                    ui.label(RichText::new(size.describe()).small());
                }
                let why = if x.chosen {
                    "your choice".to_string()
                } else if x.placed {
                    "a placed pattern: its own size unless you choose".to_string()
                } else if sizes.is_empty() {
                    "its own size".to_string()
                } else if (x.cost - 1.0).abs() < 0.005 {
                    "the same work a cell as the measured scene".to_string()
                } else if x.cost < 1.0 {
                    format!("{:.0}% of the measured scene's work a cell", x.cost * 100.0)
                } else {
                    format!("{:.1} times the measured scene's work a cell", x.cost)
                };
                // The soft cap on the estimate: said where the size is shown.
                let why = if x.capped && !x.chosen {
                    format!("{why}; held to twice the measured scene's cells: choose a finer size to go further")
                } else {
                    why
                };
                let l = ui.label(RichText::new(why).small().weak());
                if let Some(n) = &x.note {
                    l.on_hover_text(n);
                }
                ui.end_row();
            }
        });
    todo
}

/// One card: the picture at the screen's own pixels, the size and cells, the numbers, and whether
/// it is within the limits. `gpu_unknown`: its GPU share wasn't measured, so it shows as unknown
/// (#72). Whether it was clicked, and its response.
#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ppp: f32,
    r: &Rung,
    picked: bool,
    recommended: bool,
    gpu_unknown: bool,
    choosable: bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
) -> (bool, egui::Response) {
    let accent = ui.visuals().selection.stroke.color;
    let stroke = if picked {
        egui::Stroke::new(2.0, accent)
    } else {
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    let resp = egui::Frame::new()
        .stroke(stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                match r.crop.as_deref().and_then(|n| crop(ctx, n)) {
                    // Shown at one picture pixel to one screen pixel, as on the screen it is for.
                    Some(tex) => {
                        let [w, h] = tex.size();
                        ui.image((tex.id(), egui::vec2(w as f32 / ppp, h as f32 / ppp)));
                    }
                    None => {
                        ui.add_space(4.0);
                    }
                }
                let size = match r.size.level {
                    Some(l) => format!(
                        "level {l} · {} px",
                        jetcarp_display::host_settings::round_px(r.size.px)
                    ),
                    None => r.size.describe(),
                };
                ui.label(
                    RichText::new(format!("{size} · {}", tuning::thousands(r.cells)))
                        .small()
                        .strong(),
                );
                let gpu = match r.gpu_percent {
                    _ if gpu_unknown => " · GPU unknown".to_string(),
                    Some(g) => format!(" · GPU {}", tuning::percent(g as f64 / 100.0)),
                    None => String::new(),
                };
                let numbers = ui.label(
                    RichText::new(format!(
                        "CPU {}{gpu} · {:.0} fps",
                        tuning::percent(r.cpu_percent as f64 / 100.0),
                        r.fps
                    ))
                    .small(),
                );
                if gpu_unknown {
                    numbers.on_hover_text(GPU_UNMEASURED);
                }
                ui.horizontal(|ui| {
                    if r.at_limit {
                        // Within the measurement's noise of a limit: a fit repeated may find it
                        // either side, so the size recommended last time is kept (#94).
                        ui.label(
                            RichText::new("at the limit")
                                .small()
                                .color(crate::theme::ink(ui, WARN)),
                        )
                        .on_hover_text(format!(
                            "Within the measurement's noise of a limit ({}): measured again it \
                             may come out either side, so a fit keeps the size it recommended \
                             before{}",
                            tuning::percent(tuning::NOISE),
                            r.over
                                .as_deref()
                                .map_or(String::new(), |o| format!("; this time {o}"))
                        ));
                    } else if r.fits {
                        ui.label(
                            RichText::new("within the limits")
                                .small()
                                .color(crate::theme::ink(ui, Color32::from_rgb(110, 200, 120))),
                        );
                    } else {
                        ui.label(
                            RichText::new("over the limits")
                                .small()
                                .color(crate::theme::ink(ui, WARN)),
                        )
                        .on_hover_text(r.over.as_deref().unwrap_or(""));
                    }
                });
                ui.horizontal(|ui| {
                    if recommended {
                        ui.label(
                            RichText::new("Recommended")
                                .small()
                                .strong()
                                .color(crate::theme::ink(ui, accent)),
                        );
                    }
                    if r.own {
                        ui.label(RichText::new("its own size").small().weak());
                    }
                });
            });
        })
        .response;
    if !choosable {
        return (false, resp);
    }
    let resp = resp.interact(egui::Sense::click()).on_hover_text(format!(
        "{}; frames {:.1} ms (95th percentile); stepped on the {}{}. Click to choose",
        r.size.describe(),
        r.p95_ms,
        if r.on_gpu { "GPU" } else { "processor" },
        if gpu_unknown {
            format!("; {GPU_UNMEASURED}")
        } else {
            String::new()
        }
    ));
    (resp.clicked(), resp)
}

/// Whether a scene's GPU share wasn't measured: its sweep's clock, or any of its cards (a scene
/// still being measured has no clock yet) (#72).
fn gpu_unmeasured(scene: &TunedScene) -> bool {
    scene.gpu_timing.unmeasured() || scene.rungs.iter().any(|r| r.gpu_unmeasured)
}

/// A card's picture from the cache, read from the tuning folder (again when the file changes).
fn crop_texture(
    cache: &mut std::collections::HashMap<String, (std::time::SystemTime, egui::TextureHandle)>,
    ctx: &egui::Context,
    name: &str,
) -> Option<egui::TextureHandle> {
    let path = crops_dir()?.join(name);
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    if let Some((t, tex)) = cache.get(name)
        && *t == modified
    {
        return Some(tex.clone());
    }
    let bytes = std::fs::read(&path).ok()?;
    let img = jetcarp_core::media::encode::decode_png(&bytes).ok()?;
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.rgba,
    );
    // Nearest: a 1-pixel cell stays one pixel.
    let tex = ctx.load_texture(format!("crop-{name}"), color, egui::TextureOptions::NEAREST);
    cache.insert(name.to_string(), (modified, tex.clone()));
    Some(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pane tells a hold from a pause, as `host status` does: a busy processor holds the host,
    /// which resumes by itself, and Resume is for a pause (#76).
    #[test]
    fn the_summary_tells_a_hold_from_a_pause() {
        let held = json!({"screens": [], "paused": false, "paused_why": [],
            "held": ["busy"], "held_why": ["the processor is busy"]});
        assert_eq!(
            summary(&held),
            "Held: the processor is busy (resumes by itself)."
        );
        let paused = json!({"screens": [], "paused": true, "paused_why": ["by hand"],
            "held": [], "held_why": []});
        assert_eq!(summary(&paused), "Paused: by hand.");
    }

    /// The cards say when a scene's GPU share wasn't measured: from its sweep's clock (a file
    /// from rc.2 kept the unjudged share as a number), or from a card while it is measured (#72).
    #[test]
    fn the_cards_know_an_unmeasured_gpu_share() {
        use jetcarp_display::host_settings::GpuTiming;
        let scene = |gpu_timing, gpu_unmeasured| TunedScene {
            gpu_timing,
            rungs: vec![Rung {
                gpu_unmeasured,
                ..Rung::default()
            }],
            ..Default::default()
        };
        assert!(gpu_unmeasured(&scene(GpuTiming::Submission, false)));
        assert!(gpu_unmeasured(&scene(GpuTiming::None, true)), "measuring");
        assert!(!gpu_unmeasured(&scene(GpuTiming::Kernel, false)));
        assert!(!gpu_unmeasured(&scene(GpuTiming::Counters, false)));
    }

    /// Where a text starting `start` is drawn and can be seen on a 400 x 300 screen: its rect,
    /// within its clip and the screen (None: not drawn, or out of sight).
    fn seen(shapes: &[egui::epaint::ClippedShape], start: &str) -> Option<egui::Rect> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        shapes.iter().find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with(start) => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                let shown = r.intersect(c.clip_rect).intersect(screen);
                (shown.width() > 1.0 && shown.height() > 1.0).then_some(r)
            }
            _ => None,
        })
    }

    /// A scene's row of cards runs past the pane's edge, so it scrolls sideways: with shift and
    /// the wheel, or a trackpad's sideways swipe, inside the pane's own up and down scrolling. It
    /// opens scrolled to the chosen card, and the pane itself stays where it was (#108): on the
    /// Mac, none of the chosen cards could be seen, and the row would not scroll.
    #[test]
    fn the_cards_scroll_sideways_and_open_at_the_chosen_card() {
        let rungs: Vec<Rung> = (1..=12)
            .map(|k| Rung {
                size: Size::px(k as f32),
                cells: 1000 + k,
                fits: true,
                ..Rung::default()
            })
            .collect();
        let mut scene = TunedScene {
            scene: "Life".into(),
            design_px: 3.0,
            recommended: Size::px(11.0),
            rungs,
            measured: true,
            ..Default::default()
        };
        // The coarsest card chosen: the far end of the row.
        scene.set_size(Size::px(12.0));
        scene.chosen = true;
        let ctx = egui::Context::default();
        let mut first = std::collections::HashSet::new();
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>, _modifiers: egui::Modifiers| {
            t += 0.1;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                time: Some(t),
                predicted_dt: 0.1,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let c = ui.ctx().clone();
                // The pane's own scrolling, as `pane_scroll` has it.
                egui::ScrollArea::vertical()
                    .id_salt("pane")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label("Performance");
                        let _ =
                            scene_row(ui, &c, 1.0, 0, &scene, true, &mut |_, _| None, &mut |k| {
                                first.insert(k.to_string())
                            });
                        for _ in 0..40 {
                            ui.label("more of the pane");
                        }
                    });
            });
            // Nothing draws the fonts' textures here.
            out.textures_delta.clear();
            out.shapes
        };
        let mut shapes = vec![];
        for _ in 0..8 {
            shapes = frame(vec![], egui::Modifiers::NONE);
        }
        let chosen = seen(&shapes, "12 px · 1,012");
        assert!(
            chosen.is_some(),
            "the chosen card, at the far end, is in view"
        );
        assert!(
            seen(&shapes, "1 px · 1,001").is_none(),
            "the first is scrolled away"
        );
        let top = seen(&shapes, "Performance").expect("the pane did not scroll");
        assert!(top.min.y < 20.0, "{top:?}");
        // Shift and the wheel, over the row, scroll it back to the first card.
        let over = chosen.unwrap().center();
        frame(vec![egui::Event::PointerMoved(over)], egui::Modifiers::NONE);
        let wheel = |delta: egui::Vec2, modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        };
        for _ in 0..12 {
            frame(
                vec![wheel(egui::vec2(0.0, 400.0), egui::Modifiers::SHIFT)],
                egui::Modifiers::SHIFT,
            );
        }
        for _ in 0..4 {
            shapes = frame(vec![], egui::Modifiers::NONE);
        }
        assert!(
            seen(&shapes, "1 px · 1,001").is_some(),
            "shift and the wheel"
        );
        assert!(seen(&shapes, "Performance").is_some_and(|r| r.min.y < 20.0));
        // A sideways swipe takes it back along, without being scrolled back to the chosen card.
        for _ in 0..12 {
            frame(
                vec![wheel(egui::vec2(-400.0, 0.0), egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
        }
        for _ in 0..4 {
            shapes = frame(vec![], egui::Modifiers::NONE);
        }
        assert!(seen(&shapes, "1 px · 1,001").is_none(), "a sideways swipe");
        assert!(seen(&shapes, "12 px · 1,012").is_some());
    }
}
