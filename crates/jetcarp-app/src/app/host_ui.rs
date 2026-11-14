//! The Desktop background pane (*View → Desktop background settings…*): the background host,
//! which runs scenes as the desktop background (or in windows) as its own program. Start and stop
//! it, choose each screen's rotation and monitor, the lock screen, the still picture, the frame
//! rate and the GPU, the pauses, start at login, GNOME's extension, and *Fit to this computer*,
//! which tunes each scene's cell size to the processor and GPU limits set here.
//!
//! The host is its own program, so all of this works over the link and the settings file
//! (`host.json`); closing this window leaves it running. Rotations and profiles are shared with
//! the screensaver and edited in its pane.

use super::*;
use egui_wgpu::wgpu;
use jetcarp_display::autostart;
use jetcarp_display::caption::{CaptionSettings, Corner};
use jetcarp_display::gnome::{self, ExtensionState};
use jetcarp_display::host_settings::{
    HostSettings, LockScreen, MAX_AWAKE_MINUTES, Place, ScreenSettings, TuneLimits, TunedScene,
};
use jetcarp_display::host_settings::{Rung, Size, crops_dir};
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
                        Ok(c) => jetcarp_gpu::describe(&c.info),
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
            t.push_str(&format!(", {px} px cells"));
            if s["tuned"] == true {
                t.push_str(" (tuned)");
            }
        }
        if s["paused"] == true {
            t.push_str(", paused");
        }
        lines.push(t);
    }
    let why: Vec<&str> = v["paused_why"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !why.is_empty() {
        lines.push(format!("Paused: {}.", why.join("; ")));
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
                        .clicked()
                    {
                        ask = Some(json!({"action": "previous"}));
                    }
                    if ui.small_button("▶").on_hover_text("Next item").clicked() {
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
                egui::ComboBox::from_id_salt(("host-monitor", i))
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
                if count > 1
                    && ui
                        .small_button("×")
                        .on_hover_text("Remove this screen")
                        .clicked()
                {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            settings.screens.remove(i);
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
                RichText::new("Rotations and profiles are shared with the screensaver:")
                    .small()
                    .weak(),
            );
            edit_rotations = ui
                .small_button("Screensaver pane…")
                .on_hover_text("Edit rotations and profiles (View → Screensaver settings…)")
                .clicked();
        });
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
        let mut login = *h.login.get_or_insert_with(autostart::enabled);
        if ui.checkbox(&mut login, "Start at login").changed() {
            let result = autostart::login_command().and_then(|cmd| autostart::set(login, &cmd));
            match result {
                Ok(()) => h.login = Some(login),
                Err(e) => h.note = Some(e),
            }
        }
        // ---------------------------------------------- performance
        ui.separator();
        let crops = &mut h.crops;
        let mut crop = |ctx: &egui::Context, name: &str| crop_texture(crops, ctx, name);
        let (start_tune, stop_tune, forget) = performance_ui(
            ui,
            view,
            adapter.as_deref(),
            settings,
            &mut changed,
            &mut crop,
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
            h.tune_note = Some((
                "Stopped; the scenes already swept keep their cards.".into(),
                false,
            ));
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
            ui.colored_label(WARN, n);
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
            ui.label("System screen saver runs");
            let shown = settings
                .saver_rotation
                .clone()
                .unwrap_or_else(|| "the desktop background's rotation".into());
            egui::ComboBox::from_id_salt("host-saver")
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
            ui.colored_label(WARN, n);
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
            let scenes = f
                .events()
                .iter()
                .filter(|e| matches!(e, TuneEvent::Scene { .. }))
                .count();
            self.tune_note = Some(match outcome {
                Ok(()) => (
                    format!(
                        "Swept {scenes} scene(s). Each shows at the recommended size unless you choose another card."
                    ),
                    false,
                ),
                Err(e) => (
                    format!("The sweep stopped: {e}. The scenes done are saved."),
                    true,
                ),
            });
            self.feed = None;
            self.settings = None;
        }
    }
}

/// What the Performance group shows of a sweep under way.
struct TuneView {
    running: bool,
    on_screen: bool,
    progress: Option<String>,
    /// The scene on show, and its cards so far: (screen, scene, cards).
    live: Option<(usize, String, Vec<Rung>)>,
    note: Option<(String, bool)>,
}

fn h_tune_view(h: &HostPanel) -> TuneView {
    let Some(f) = &h.feed else {
        return TuneView {
            running: false,
            on_screen: false,
            progress: None,
            live: None,
            note: h.tune_note.clone(),
        };
    };
    let events = f.events();
    let progress = events
        .iter()
        .rev()
        .find(|e| matches!(e, TuneEvent::Rung { .. } | TuneEvent::Start { .. }))
        .map(TuneEvent::describe);
    // The cards of the scene on show: those measured since it started.
    let mut live: Option<(usize, String, Vec<Rung>)> = None;
    for e in events {
        match e {
            TuneEvent::Rung { screen, scene, .. } => {
                if live
                    .as_ref()
                    .is_none_or(|(s, n, _)| s != screen || n != scene)
                {
                    live = Some((*screen, scene.clone(), vec![]));
                }
            }
            TuneEvent::Measured { rung, .. } => {
                if let Some((_, _, cards)) = &mut live {
                    cards.push(rung.clone());
                }
            }
            TuneEvent::Scene { .. } => live = None,
            _ => {}
        }
    }
    if let Some((_, _, cards)) = &mut live {
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
    /// Show screen `0`'s scene `1` at this card's size.
    Choose(usize, String, Size),
    /// Show every scene on this tiling (screen, tiling) at this cell size.
    AllOnTiling(usize, String, f32),
    /// Every scene nobody chose shows at its recommended size, now as a choice.
    RecommendedForRest,
}

/// The limits, *Fit to this computer*, and the cards. Returns whether to start a sweep, stop the
/// one running, or forget the sizes found; a card chosen is put in `settings` (`changed`).
#[allow(clippy::too_many_arguments)]
fn performance_ui(
    ui: &mut egui::Ui,
    view: TuneView,
    adapter: Option<&str>,
    settings: &mut HostSettings,
    changed: &mut bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
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
                        "The monitor's refresh rate (or 60), at most the frames per second above",
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
    ui.horizontal_wrapped(|ui| {
        if view.running {
            ui.spinner();
            if ui.button("Cancel").clicked() {
                stop = true;
            }
        } else if ui
            .button(RichText::new("Fit to this computer…").strong())
            .on_hover_text(
                "Show each scene of the desktop background at each size from 1 pixel a cell up, \
                 a couple of seconds each, measuring the processor, the GPU and the frame rate; \
                 then choose a card for each scene. The rules and colours stay as they are",
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
                .on_hover_text("Forget the sweep; each scene shows at its own cell size")
                .clicked()
        {
            forget = true;
        }
    });
    *changed |= ui
        .checkbox(
            &mut settings.tune.auto,
            "Measure again by itself when the monitors change",
        )
        .on_hover_text(
            "Once swept: a size you chose is kept while it is still within the limits, else the \
             recommended one shows. Not while on battery or busy",
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
        ui.label(RichText::new(t).small().color(c));
    }
    // The cards: each scene's saved sweep (choosable), and the scene on show (not yet).
    let ppp = ui.ctx().pixels_per_point();
    let ctx = ui.ctx().clone();
    let mut todo: Option<CardDo> = None;
    let mut any = false;
    let screens = settings.screens.len();
    for (i, s) in settings.screens.iter().enumerate() {
        let Some(t) = s.tuning.as_ref().filter(|t| t.rotation == s.rotation) else {
            continue;
        };
        if t.scenes.iter().all(|x| x.rungs.is_empty()) {
            continue;
        }
        any = true;
        ui.horizontal_wrapped(|ui| {
            if screens > 1 {
                ui.label(RichText::new(format!("Screen {}", i + 1)).strong());
            }
            if !view.running
                && t.scenes.iter().any(|x| !x.chosen)
                && ui
                    .small_button("Use the recommended size for the rest")
                    .on_hover_text(
                        "Every scene you have not chosen a card for keeps its recommended size, \
                         as your choice",
                    )
                    .clicked()
            {
                todo = Some(CardDo::RecommendedForRest);
            }
        });
        for scene in &t.scenes {
            if view
                .live
                .as_ref()
                .is_some_and(|(sc, n, _)| *sc == i && *n == scene.scene)
            {
                continue;
            }
            if let Some(d) = scene_row(ui, &ctx, ppp, i, scene, !view.running, crop) {
                todo = Some(d);
            }
        }
    }
    if let Some((i, name, cards)) = &view.live {
        any = true;
        let scene = TunedScene {
            scene: name.clone(),
            rungs: cards.clone(),
            ..Default::default()
        };
        ui.label(RichText::new("Measuring…").small().weak());
        let _ = scene_row(ui, &ctx, ppp, *i, &scene, false, crop);
    }
    if !any && !view.running {
        ui.label(
            RichText::new("Not swept: each scene shows at its own cell size.")
                .small()
                .weak(),
        );
    }
    match todo {
        Some(CardDo::Choose(i, name, size)) => {
            if let Some(scene) = settings.screens[i]
                .tuning
                .as_mut()
                .and_then(|t| t.scenes.iter_mut().find(|x| x.scene == name))
            {
                let own = scene.rungs.iter().any(|r| r.own && r.size.same(&size));
                scene.set_size(if own { Size::px(scene.design_px) } else { size });
                scene.chosen = true;
                scene.note = None;
                *changed = true;
            }
        }
        Some(CardDo::AllOnTiling(i, tiling, px)) => {
            if let Some(t) = settings.screens[i].tuning.as_mut() {
                for scene in t.scenes.iter_mut().filter(|x| {
                    x.tiling == tiling && x.level.is_none() && x.recommended.level.is_none()
                }) {
                    scene.set_size(Size::px(px));
                    scene.chosen = true;
                    scene.note = None;
                }
                *changed = true;
            }
        }
        Some(CardDo::RecommendedForRest) => {
            for s in &mut settings.screens {
                for scene in s.tuning.iter_mut().flat_map(|t| t.scenes.iter_mut()) {
                    if !scene.chosen {
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

/// One scene's cards in a row, its name and note above. `choosable`: a click chooses a card.
fn scene_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ppp: f32,
    screen: usize,
    scene: &TunedScene,
    choosable: bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
) -> Option<CardDo> {
    let mut todo = None;
    let showing = scene.size();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&scene.scene).strong());
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
        ui.label(RichText::new(n).small().color(WARN));
    }
    egui::ScrollArea::horizontal()
        .id_salt(("tune-cards", screen, &scene.scene))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for r in &scene.rungs {
                    let picked = choosable
                        && (r.size.same(&showing)
                            || (r.own && showing.level.is_none() && showing.px == scene.design_px));
                    let recommended = choosable && r.size.same(&scene.recommended);
                    if card(ui, ctx, ppp, r, picked, recommended, choosable, crop) {
                        todo = Some(CardDo::Choose(screen, scene.scene.clone(), r.size));
                    }
                }
            });
        });
    todo
}

/// One card: the picture at the screen's own pixels, the size and cells, the numbers, and whether
/// it is within the limits. True when it was clicked.
#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ppp: f32,
    r: &Rung,
    picked: bool,
    recommended: bool,
    choosable: bool,
    crop: &mut dyn FnMut(&egui::Context, &str) -> Option<egui::TextureHandle>,
) -> bool {
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
                let gpu = r.gpu_percent.map_or(String::new(), |g| {
                    format!(" · GPU {}", tuning::percent(g as f64 / 100.0))
                });
                ui.label(
                    RichText::new(format!(
                        "CPU {}{gpu} · {:.0} fps",
                        tuning::percent(r.cpu_percent as f64 / 100.0),
                        r.fps
                    ))
                    .small(),
                );
                ui.horizontal(|ui| {
                    if r.fits {
                        ui.label(
                            RichText::new("within the limits")
                                .small()
                                .color(Color32::from_rgb(110, 200, 120)),
                        );
                    } else {
                        ui.label(RichText::new("over the limits").small().color(WARN))
                            .on_hover_text(r.over.as_deref().unwrap_or(""));
                    }
                });
                ui.horizontal(|ui| {
                    if recommended {
                        ui.label(RichText::new("Recommended").small().strong().color(accent));
                    }
                    if r.own {
                        ui.label(RichText::new("its own size").small().weak());
                    }
                });
            });
        })
        .response;
    if !choosable {
        return false;
    }
    let resp = resp.interact(egui::Sense::click()).on_hover_text(format!(
        "{}; frames {:.1} ms (95th percentile); stepped on the {}. Click to choose",
        r.size.describe(),
        r.p95_ms,
        if r.on_gpu { "GPU" } else { "processor" }
    ));
    resp.clicked()
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
