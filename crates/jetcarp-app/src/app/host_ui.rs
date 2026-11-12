//! The background host, from the Screensaver pane: start and stop it, see what it is showing, and
//! change its settings. The host is its own program, so all of this works over the link and the
//! settings file; closing this window leaves it running.

use super::*;
use jetcarp_display::autostart;
use jetcarp_display::caption::{CaptionSettings, Corner};
use jetcarp_display::gnome::{self, ExtensionState};
use jetcarp_display::host_settings::{HostSettings, LockScreen, MAX_AWAKE_MINUTES, Place};
use jetcarp_mcp::link::host_call;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};

const POLL_EVERY: f64 = 2.0;
/// Settings edits are told to the host this long after the last one.
const RELOAD_AFTER: f64 = 0.3;
const FPS_CHOICES: [f32; 5] = [15.0, 24.0, 30.0, 60.0, 120.0];

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

    fn running(&self) -> Option<&Value> {
        self.status.as_ref()?.as_ref().ok()
    }
}

fn summary(v: &Value) -> String {
    let s = &v["screens"][0]["stage"];
    let mut t = format!(
        "{}: {} ({} of {})",
        s["rotation"].as_str().unwrap_or("?"),
        s["label"].as_str().unwrap_or("starting"),
        s["number"],
        s["count"]
    );
    if s["paused"] == true {
        t.push_str(", paused");
    }
    t
}

impl JetcarpApp {
    pub(super) fn host_section(&mut self, ui: &mut egui::Ui, now: f64) {
        egui::CollapsingHeader::new("Background host")
            .id_salt("host-section")
            .default_open(std::env::var_os("JETCARP_HOST_PANEL").is_some())
            .show(ui, |ui| self.host_ui(ui, now));
    }

    fn host_ui(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.label(
            RichText::new(
                "Runs scenes as its own program, so they keep going when this window closes.",
            )
            .small()
            .weak(),
        );
        let rotations = self.rotation_store().names();
        let h = &mut self.display.host;
        h.poll(now);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(POLL_EVERY));
        if cfg!(target_os = "linux") {
            h.poll_extension();
        }
        let settings = h.settings.get_or_insert_with(|| HostSettings::load().0);
        let mut changed = false;
        let running = h.status.as_ref().and_then(|r| r.as_ref().ok()).cloned();
        let mut ask: Option<Value> = None;
        let mut start = false;
        ui.horizontal(|ui| {
            match &running {
                Some(_) => {
                    if ui.button("Stop").clicked() {
                        ask = Some(json!({"action": "quit"}));
                    }
                    if ui
                        .small_button("◀")
                        .on_hover_text("Previous item")
                        .clicked()
                    {
                        ask = Some(json!({"action": "previous"}));
                    }
                    if ui.small_button("▶").on_hover_text("Next item").clicked() {
                        ask = Some(json!({"action": "next"}));
                    }
                }
                None => start = ui.button("Start").clicked(),
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
        let screen = &mut settings.screens[0];
        ui.horizontal(|ui| {
            ui.label("Rotation");
            egui::ComboBox::from_id_salt("host-rotation")
                .selected_text(screen.rotation.as_str())
                .show_ui(ui, |ui| {
                    for n in &rotations {
                        changed |= ui
                            .selectable_value(&mut screen.rotation, n.clone(), n)
                            .changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Frames per second");
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
        changed |= caption_ui(ui, &mut settings.caption);
        changed |= ui
            .checkbox(&mut settings.pause_on_battery, "Pause on battery")
            .changed();
        ui.horizontal(|ui| {
            let mut on = settings.busy_percent > 0;
            if ui
                .checkbox(&mut on, "Pause while the processor is busier than")
                .on_hover_text("For 15 seconds, as during research runs. Carries on once it is 20 points lower")
                .changed()
            {
                settings.busy_percent = if on { 80 } else { 0 };
                changed = true;
            }
            ui.add_enabled_ui(on, |ui| {
                let mut p = settings.busy_percent.max(1);
                if ui
                    .add(egui::DragValue::new(&mut p).range(30..=100).suffix(" %"))
                    .changed()
                    && on
                {
                    settings.busy_percent = p;
                    changed = true;
                }
            });
        });
        let mut install = false;
        let mut install_saver = false;
        let wallpaper = settings.screens.iter().all(|s| s.place == Place::Wallpaper);
        let mut on = wallpaper;
        if ui
            .checkbox(&mut on, "Show on the desktop")
            .on_hover_text("Behind your windows, as the desktop background")
            .changed()
        {
            let place = if on { Place::Wallpaper } else { Place::Window };
            settings.screens.iter_mut().for_each(|s| s.place = place);
            changed = true;
        }
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
        if jetcarp_display::saver::AVAILABLE {
            ui.horizontal(|ui| {
                ui.label("Screen saver runs");
                let shown = settings
                    .saver_rotation
                    .clone()
                    .unwrap_or_else(|| "the first screen's rotation".into());
                egui::ComboBox::from_id_salt("host-saver")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                &mut settings.saver_rotation,
                                None,
                                "the first screen's rotation",
                            )
                            .changed();
                        for n in &rotations {
                            changed |= ui
                                .selectable_value(&mut settings.saver_rotation, Some(n.clone()), n)
                                .changed();
                        }
                    });
                let tip = if jetcarp_display::saver::BAKED {
                    "Add JETCARP to macOS's screen savers. The rotation is copied in, so install again after changing it"
                } else {
                    "Add JETCARP to Windows' list of screen savers"
                };
                install_saver = ui.small_button("Install").on_hover_text(tip).clicked();
            });
        }
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
            ui.horizontal(|ui| {
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
        let mut login = *h.login.get_or_insert_with(autostart::enabled);
        if ui.checkbox(&mut login, "Start at login").changed() {
            let result = autostart::login_command().and_then(|cmd| autostart::set(login, &cmd));
            match result {
                Ok(()) => h.login = Some(login),
                Err(e) => h.note = Some(e),
            }
        }
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
        if install {
            h.note = None;
            h.check_extension(gnome::install);
        }
        if install_saver {
            h.note = match jetcarp_display::saver::install() {
                Ok((_, warnings)) if !warnings.is_empty() => Some(warnings.join("; ")),
                Ok(_) => None,
                Err(e) => Some(e),
            };
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
            ui.colored_label(Color32::from_rgb(255, 170, 90), n);
        }
    }
}
