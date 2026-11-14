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
use jetcarp_display::tuning::{self, TuneChild, TuneRequest};
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
    /// *Fit to this computer*, running as a program of its own.
    tune: Option<TuneChild>,
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

    /// Follows a tune under way; once it ends, says how and reads the sizes it saved.
    fn poll_tune(&mut self) {
        let Some(t) = &mut self.tune else { return };
        t.poll();
        if let Some(outcome) = t.outcome() {
            let scenes = t.scenes().len();
            self.tune_note = Some(match outcome {
                Ok(()) => (format!("Tuned {scenes} scene(s) for this computer."), false),
                Err(e) => (format!("Tuning stopped: {e}"), true),
            });
            self.tune = None;
            self.settings = None;
        }
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
        h.poll_tune();
        let view = h_tune_view(h);
        let adapter = h.adapter();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(if h.tune.is_some() {
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
        let (start_tune, stop_tune, forget) =
            performance_ui(ui, view, adapter.as_deref(), settings, &mut changed);
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
            match TuneChild::spawn(&TuneRequest {
                cpu_percent: Some(l.cpu_percent),
                gpu_percent: Some(l.gpu_percent),
                fps: l.fps,
                ..Default::default()
            }) {
                Ok(t) => h.tune = Some(t),
                Err(e) => h.tune_note = Some((e, true)),
            }
        }
        if stop_tune && let Some(mut t) = h.tune.take() {
            t.stop();
            // The host lets go of its hold now rather than in 30 s.
            if running.is_some() {
                let _ = std::thread::Builder::new()
                    .name("jetcarp-host-ask".into())
                    .spawn(|| host_call(&json!({"action": "tuning", "on": false})));
            }
            h.tune_note = Some((
                "Stopped; the sizes found so far were not saved.".into(),
                false,
            ));
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

/// What the Performance group shows of a tune under way.
struct TuneView {
    running: bool,
    progress: Option<String>,
    counts: Option<(usize, usize)>,
    /// Each scene done so far: (screen, result).
    live: Vec<(usize, TunedScene)>,
    note: Option<(String, bool)>,
}

fn h_tune_view(h: &HostPanel) -> TuneView {
    match &h.tune {
        Some(t) => TuneView {
            running: true,
            progress: t.progress(),
            counts: t.counts(),
            live: t.scenes(),
            note: None,
        },
        None => TuneView {
            running: false,
            progress: None,
            counts: None,
            live: vec![],
            note: h.tune_note.clone(),
        },
    }
}

/// The limits, *Fit to this computer*, and what it found. Returns whether to start a tune, stop
/// the one running, or forget the sizes found.
fn performance_ui(
    ui: &mut egui::Ui,
    view: TuneView,
    adapter: Option<&str>,
    settings: &mut HostSettings,
    changed: &mut bool,
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
                "Run each scene of the desktop background's rotations for a few seconds at a time, \
                 at its screen's size, and keep the most cells (the same rules and colours) that \
                 stay within these limits. Takes a few seconds a scene; the host pauses meanwhile",
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
                .on_hover_text("Forget the sizes found; each scene shows at its own cell size")
                .clicked()
        {
            forget = true;
        }
    });
    *changed |= ui
        .checkbox(
            &mut settings.tune.auto,
            "Tune again by itself when the monitors change",
        )
        .on_hover_text("Once it has been tuned, and not while on battery or busy")
        .changed();
    if view.running {
        let text = match (view.counts, &view.progress) {
            (Some((n, of)), _) => format!("Trying scene {n} of {of}…"),
            (None, Some(p)) => p.clone(),
            _ => "Starting…".into(),
        };
        ui.label(RichText::new(text).small());
        if let Some(p) = &view.progress {
            ui.label(RichText::new(p).small().weak());
        }
    }
    if let Some((t, err)) = &view.note {
        let c = if *err {
            Color32::from_rgb(255, 110, 110)
        } else {
            ui.visuals().weak_text_color()
        };
        ui.label(RichText::new(t).small().color(c));
    }
    // What it found: live while it runs, else what is saved.
    let saved: Vec<(usize, TunedScene, bool)> = settings
        .screens
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            let current = s.tuning.as_ref().is_some_and(|t| t.rotation == s.rotation);
            s.tuning
                .iter()
                .flat_map(move |t| t.scenes.iter().map(move |r| (i, r.clone(), current)))
        })
        .collect();
    let rows: Vec<(usize, TunedScene, bool)> = if view.running {
        view.live.into_iter().map(|(i, r)| (i, r, true)).collect()
    } else {
        saved
    };
    if rows.is_empty() {
        if !view.running {
            ui.label(
                RichText::new("Not tuned: each scene shows at its own cell size.")
                    .small()
                    .weak(),
            );
        }
        return (start, stop, forget);
    }
    let screens = settings.screens.len();
    egui::Grid::new("host-tune-results")
        .num_columns(5)
        .striped(true)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for h in ["Scene", "Cell size, cells", "Processor", "GPU", "Frames"] {
                ui.label(RichText::new(h).small().strong());
            }
            ui.end_row();
            for (i, r, current) in &rows {
                let name = if screens > 1 {
                    format!("{}: {}", i + 1, r.scene)
                } else {
                    r.scene.clone()
                };
                let mut name = RichText::new(name).small();
                if !current {
                    name = name.weak();
                }
                let resp = ui.label(name);
                if !current {
                    resp.on_hover_text("Found for another rotation: tune again to use it");
                }
                let size = if r.cell_px > r.design_px {
                    format!("{} px (own {} px)", r.cell_px, r.design_px)
                } else {
                    format!("{} px, own size", r.cell_px)
                };
                ui.label(RichText::new(format!("{size}, {}", tuning::thousands(r.cells))).small());
                ui.label(RichText::new(tuning::percent(r.cpu_percent as f64 / 100.0)).small());
                ui.label(
                    RichText::new(
                        r.gpu_percent
                            .map_or("–".into(), |g| tuning::percent(g as f64 / 100.0)),
                    )
                    .small(),
                )
                .on_hover_text(format!(
                    "{}; measured with {}",
                    if r.on_gpu {
                        "Stepped on the GPU"
                    } else {
                        "Stepped on the processor"
                    },
                    r.gpu_timing.describe()
                ));
                let frames = RichText::new(format!("{:.1} ms", r.p95_ms)).small();
                let resp = match &r.note {
                    Some(_) if !r.fits => ui.label(frames.color(WARN)),
                    _ => ui.label(frames),
                };
                let resp = resp.on_hover_text("95th percentile of a frame's time");
                if let Some(n) = &r.note {
                    resp.on_hover_text(n.as_str());
                }
                ui.end_row();
            }
        });
    if rows.iter().any(|(_, r, _)| r.note.is_some() && !r.fits) {
        ui.label(
            RichText::new("Orange: over a limit even at the coarsest size (hover for why).")
                .small()
                .weak(),
        );
    }
    (start, stop, forget)
}
