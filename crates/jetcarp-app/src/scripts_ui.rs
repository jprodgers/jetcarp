//! The Scripts window: a click-to-build sweep form, a Rhai script editor, the function
//! reference, and one background job at a time with progress, ETA, and a log. Scripts run on
//! their own threads (`jetcarp_script::spawn`), so the app keeps simulating while a sweep runs.

use crate::rhai_edit;
use crate::sweep_form::{self, ScreenInfo, SweepForm, thousands};
use jetcarp_core::World;
use jetcarp_script::{Event, Job, Progress, RunOptions, Screen, ScriptError, Summary};
use eframe::egui::{self, Color32, RichText, text::LayoutJob};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "scripts";

/// Example scripts (shipped in `jetcarp-script`).
pub const EXAMPLES: &[(&str, &str)] = jetcarp_script::EXAMPLES;

const NEW_SCRIPT: &str = "// A JETCARP script (Rhai). Ctrl+Enter runs it; the Function reference tab lists what you can call.\n\
let w = world(#{ tiling: \"hex\", size: \"screen\", boundary: \"dead\" });\n\
w.add_ca(\"HexW3 8229\", #{ seed: \"top_center\" });\n\
let r = w.run(#{ until: \"done\", max_gens: 5000 });\n\
print(`${w.rule()}: ${r.generations} generations, ${w.population()} live`);\n\
show(w);   // shows it in the main view\n";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sweep,
    Script,
    Help,
}

/// What the window asks the app to do.
pub enum Action {
    /// `show(w)` from a script.
    ShowWorld(Box<World>, String),
    /// Open the results browser on a sweep folder.
    Browse(PathBuf),
    Message(String, bool),
}

/// What the app tells the window.
pub struct Host<'a> {
    pub screen: Screen,
    pub world: &'a World,
}

enum Estimating {
    Running {
        rx: Receiver<Result<jetcarp_script::Estimate, String>>,
        stop: Arc<AtomicBool>,
        spec: Value,
    },
    Done {
        spec: Value,
        result: Result<jetcarp_script::Estimate, String>,
    },
}

struct Running {
    job: Job,
    what: String,
    from_editor: bool,
    started: Instant,
    /// The sweep folder, as soon as the engine names it.
    folder: Option<PathBuf>,
}

pub struct ScriptsWindow {
    pub open: bool,
    pub tab: Tab,
    pub form: SweepForm,
    text: String,
    path: Option<PathBuf>,
    saved: String,
    check: Option<ScriptError>,
    checked: String,
    running: Option<Running>,
    progress: Option<Progress>,
    log: Vec<String>,
    last: Option<Summary>,
    last_folder: Option<PathBuf>,
    pub threads: usize,
    cores: usize,
    estimate: Option<Estimating>,
    /// Waiting for the user to confirm a large sweep.
    confirm: bool,
    help_query: String,
    /// Headless checks: press Run after this many frames (`JETCARP_SCRIPTS_RUN`).
    pub autorun: Option<u32>,
}

impl Default for ScriptsWindow {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        ScriptsWindow {
            open: false,
            tab: Tab::Sweep,
            form: SweepForm::default(),
            text: NEW_SCRIPT.into(),
            path: None,
            saved: NEW_SCRIPT.into(),
            check: None,
            checked: String::new(),
            running: None,
            progress: None,
            log: vec![],
            last: None,
            last_folder: None,
            // Leave a few cores for the app's own simulation and drawing.
            threads: (cores - (cores / 8).max(1)).max(1),
            cores,
            estimate: None,
            confirm: false,
            help_query: String::new(),
            autorun: None,
        }
    }
}

/// Runs above this many ask for confirmation, with an estimate, before starting.
const CONFIRM_RUNS: usize = 300;

impl ScriptsWindow {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// One line for the app's status bar while a job runs.
    pub fn status(&self) -> Option<String> {
        let r = self.running.as_ref()?;
        Some(match &self.progress {
            Some(p) => {
                let eta = p
                    .eta
                    .map(|e| format!(", ~{} left", jetcarp_script::duration(e)))
                    .unwrap_or_default();
                format!(
                    "{}: {}/{}{eta}",
                    r.what,
                    thousands(p.done),
                    thousands(p.total)
                )
            }
            None => format!("{}: running", r.what),
        })
    }

    /// Put an example script in the editor.
    pub fn load_example(&mut self, name: &str) {
        if let Some((_, src)) = EXAMPLES.iter().find(|e| e.0 == name) {
            self.text = src.to_string();
            self.saved = self.text.clone();
            self.path = None;
            self.tab = Tab::Script;
        }
    }

    /// Start the form's sweep from the world on screen.
    pub fn sweep_from_world(&mut self, world: &World) {
        self.form = SweepForm::from_world(world);
        self.tab = Tab::Sweep;
        self.open = true;
        self.estimate = None;
    }

    fn options(&self, host: &Host) -> RunOptions {
        RunOptions {
            screen: host.screen,
            threads: Some(self.threads.clamp(1, self.cores)),
            live_view: true,
            script_path: self.path.clone(),
            ..Default::default()
        }
    }

    fn start(&mut self, src: String, what: String, from_editor: bool, host: &Host) {
        if self.running.is_some() {
            return;
        }
        self.log.clear();
        self.progress = None;
        self.last = None;
        self.confirm = false;
        self.log.push(format!(
            "{what}: screen {} x {} px, {:.0} px tiles, {} threads",
            host.screen.width, host.screen.height, host.screen.cell_px, self.threads
        ));
        let job = jetcarp_script::spawn(src, self.options(host));
        self.running = Some(Running {
            job,
            what,
            from_editor,
            started: Instant::now(),
            folder: None,
        });
    }

    fn run_sweep(&mut self, host: &Host) {
        let spec = self.form.to_json();
        let src = jetcarp_script::sweep_to_script(&spec);
        let what = format!("sweep {}", spec["out"].as_str().unwrap_or(""));
        self.start(src, what, false, host);
    }

    fn run_script(&mut self, host: &Host) {
        let what = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "script".into());
        self.start(self.text.clone(), what, true, host);
    }

    fn start_estimate(&mut self, host: &Host) {
        if let Some(Estimating::Running { stop, .. }) = &self.estimate {
            stop.store(true, Ordering::Relaxed);
        }
        let spec = self.form.to_json();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (s, sp, screen) = (stop.clone(), spec.clone(), host.screen);
        std::thread::spawn(move || {
            let _ = tx.send(jetcarp_script::estimate(&sp, screen, 3, &|| {
                s.load(Ordering::Relaxed)
            }));
        });
        self.estimate = Some(Estimating::Running { rx, stop, spec });
    }

    /// Poll the running job (call every frame, open or not).
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = vec![];
        if let Some(Estimating::Running { rx, spec, .. }) = &self.estimate
            && let Ok(result) = rx.try_recv()
        {
            self.estimate = Some(Estimating::Done {
                spec: spec.clone(),
                result,
            });
        }
        if matches!(self.estimate, Some(Estimating::Running { .. })) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(r) = &mut self.running else {
            return out;
        };
        for e in r.job.events() {
            match e {
                Event::Log(line) => {
                    if r.folder.is_none()
                        && line.starts_with("sweep: ")
                        && let Some(rest) = line.split(" -> ").nth(1)
                    {
                        let f = rest.split(" (resuming").next().unwrap_or(rest);
                        r.folder = Some(PathBuf::from(f.trim()));
                    }
                    self.log.push(line);
                }
                Event::Progress(p) => self.progress = Some(p),
                Event::Show(s) => {
                    if let Some(w) = s.take_world() {
                        let title = if s.label.is_empty() {
                            w.cas
                                .first()
                                .map(|c| c.notation())
                                .unwrap_or_else(|| "Script".into())
                        } else {
                            s.label.clone()
                        };
                        out.push(Action::ShowWorld(Box::new(w), title));
                    }
                }
            }
        }
        if self.log.len() > 5000 {
            self.log.drain(..self.log.len() - 4000);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if r.job.is_finished() {
            let r = self.running.take().unwrap();
            for e in r.job.events() {
                if let Event::Log(l) = e {
                    self.log.push(l);
                }
            }
            let s = r.job.wait();
            let secs = r.started.elapsed().as_secs_f64();
            if let Some(e) = &s.error {
                if r.from_editor && e.line.is_some() {
                    self.check = Some(e.clone());
                    self.checked = self.text.clone();
                }
                self.log.push(format!(
                    "{}: {}",
                    if s.cancelled { "stopped" } else { "error" },
                    e
                ));
            } else {
                self.log
                    .push(format!("finished in {}", jetcarp_script::duration(secs)));
            }
            let folder = s
                .sweeps
                .last()
                .and_then(|x| x["folder"].as_str())
                .map(PathBuf::from)
                .or(r.folder.clone());
            self.last_folder = folder.clone().or_else(|| s.output_dirs.last().cloned());
            if let Some(f) = folder
                && f.exists()
            {
                out.push(Action::Browse(f));
            }
            out.push(Action::Message(
                format!(
                    "{} {} in {}",
                    r.what,
                    if s.ok {
                        "finished"
                    } else if s.cancelled {
                        "stopped"
                    } else {
                        "failed"
                    },
                    jetcarp_script::duration(secs)
                ),
                !s.ok && !s.cancelled,
            ));
            self.last = Some(s);
        }
        out
    }

    /// Draw the window.
    pub fn show(&mut self, ctx: &egui::Context, host: &Host) -> Vec<Action> {
        let mut out = vec![];
        match self.autorun {
            Some(0) => {
                self.autorun = None;
                match self.tab {
                    Tab::Script => self.run_script(host),
                    _ => self.request_sweep(host),
                }
            }
            Some(k) => self.autorun = Some(k - 1),
            None => {}
        }
        if !self.open {
            return out;
        }
        // Live syntax check.
        if self.checked != self.text {
            self.check = jetcarp_script::check(&self.text).err();
            self.checked = self.text.clone();
        }
        let mut open = self.open;
        let screen = ctx.content_rect();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Scripts and sweeps")
                .size(800.0, 860.0)
                .configure(|w| {
                    w.default_pos(screen.min + egui::vec2(345.0, 60.0))
                        .default_size([
                            (screen.width() * 0.6).clamp(420.0, 900.0),
                            (screen.height() * 0.85).clamp(360.0, 940.0),
                        ])
                        .min_size([360.0, 320.0])
                        .resizable(true)
                }),
            &mut open,
            |ui| self.contents(ui, host, &mut out),
        );
        self.open = open;
        out
    }

    fn contents(&mut self, ui: &mut egui::Ui, host: &Host, out: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.tab,
                Tab::Sweep,
                RichText::new("Sweep builder").strong(),
            );
            let dirty = if self.text != self.saved { " *" } else { "" };
            let name = self
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| format!("Script: {}{dirty}", n.to_string_lossy()))
                .unwrap_or_else(|| format!("Script editor{dirty}"));
            ui.selectable_value(&mut self.tab, Tab::Script, RichText::new(name).strong());
            ui.selectable_value(
                &mut self.tab,
                Tab::Help,
                RichText::new("Function reference").strong(),
            );
        });
        // Keyboard: Ctrl+Enter runs.
        if self.running.is_none()
            && self.tab != Tab::Help
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
        {
            match self.tab {
                Tab::Sweep => self.request_sweep(host),
                _ => {
                    if self.check.is_none() {
                        self.run_script(host);
                    }
                }
            }
        }
        // The run area at the bottom.
        egui::Panel::bottom("scripts-run")
            .resizable(false)
            .show(ui, |ui| self.run_area(ui, host, out));
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Sweep => self.sweep_tab(ui, host),
            Tab::Script => self.script_tab(ui),
            Tab::Help => self.help_tab(ui),
        });
    }

    /// Run the form's sweep, after an estimate and confirmation when it is large.
    fn request_sweep(&mut self, host: &Host) {
        let runs = self.form.runs().unwrap_or(0);
        if runs > CONFIRM_RUNS {
            let spec = self.form.to_json();
            let have = match &self.estimate {
                Some(Estimating::Done { spec: s, .. })
                | Some(Estimating::Running { spec: s, .. }) => *s == spec,
                None => false,
            };
            if !have {
                self.start_estimate(host);
            }
            self.confirm = true;
        } else {
            self.run_sweep(host);
        }
    }

    fn sweep_tab(&mut self, ui: &mut egui::Ui, host: &Host) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Run one world per rule and save a picture of each.").weak());
            if ui
                .button("Start from the current world")
                .on_hover_text("Take the tiling, size, edges, CAs, seeds and drawn cells from the main view, and sweep one CA's rule")
                .clicked()
            {
                self.form = SweepForm::from_world(host.world);
                self.estimate = None;
            }
            if ui.button("Reset form").clicked() {
                self.form = SweepForm::default();
                self.estimate = None;
            }
        });
        ui.separator();
        let info = ScreenInfo {
            screen: host.screen,
            world_size: host.world.topo.spec.size,
        };
        egui::ScrollArea::vertical()
            .id_salt("sweep-form-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if sweep_form::ui(ui, &mut self.form, &info) {
                    self.confirm = false;
                }
            });
    }

    fn script_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Examples", |ui| {
                for (name, src) in EXAMPLES {
                    let first = src
                        .lines()
                        .find(|l| l.starts_with("//") && l.len() > 3)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .trim();
                    if ui.button(*name).on_hover_text(first).clicked() {
                        self.text = src.to_string();
                        self.saved = self.text.clone();
                        self.path = None;
                        ui.close();
                    }
                }
            });
            if ui.button("New").clicked() {
                self.text = NEW_SCRIPT.into();
                self.saved = self.text.clone();
                self.path = None;
            }
            if ui.button("Open…").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("Rhai script", &["rhai"])
                    .pick_file()
            {
                match std::fs::read_to_string(&p) {
                    Ok(t) => {
                        self.text = t;
                        self.saved = self.text.clone();
                        self.path = Some(p);
                    }
                    Err(e) => self
                        .log
                        .push(format!("could not open {}: {e}", p.display())),
                }
            }
            let save_as = ui.button("Save as…").clicked();
            if (ui.button("Save").clicked() && self.path.is_some()) || save_as || {
                let s = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
                s && self.path.is_some()
            } {
                let path = if save_as || self.path.is_none() {
                    rfd::FileDialog::new()
                        .add_filter("Rhai script", &["rhai"])
                        .set_file_name("script.rhai")
                        .save_file()
                } else {
                    self.path.clone()
                };
                if let Some(p) = path {
                    match std::fs::write(&p, &self.text) {
                        Ok(()) => {
                            self.saved = self.text.clone();
                            self.path = Some(p);
                        }
                        Err(e) => self.log.push(format!("could not save: {e}")),
                    }
                }
            }
            ui.separator();
            match &self.check {
                None => ui.colored_label(Color32::LIGHT_GREEN, "No syntax errors"),
                Some(e) => ui.colored_label(Color32::from_rgb(0xff, 0x80, 0x80), e.to_string()),
            };
        });
        let h = ui.available_height() - 4.0;
        let err = self.check.as_ref().and_then(|e| e.line);
        rhai_edit::editor(ui, &mut self.text, err, h);
    }

    fn help_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.add(
                egui::TextEdit::singleline(&mut self.help_query)
                    .hint_text("sweep, save_png, until…")
                    .desired_width(200.0),
            );
            if !self.help_query.is_empty() && ui.small_button("x").clicked() {
                self.help_query.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("script-help")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                markdown(ui, jetcarp_script::help_text(), &self.help_query)
            });
    }

    fn run_area(&mut self, ui: &mut egui::Ui, host: &Host, out: &mut Vec<Action>) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let running = self.running.is_some();
            match self.tab {
                Tab::Sweep | Tab::Help => {
                    let runs = self.form.runs();
                    let label = match &runs {
                        Ok(n) => format!("Run sweep ({} runs)", thousands(*n as u64)),
                        Err(_) => "Run sweep".into(),
                    };
                    if ui
                        .add_enabled(!running && runs.is_ok(), egui::Button::new(RichText::new(label).strong()))
                        .on_hover_text("Ctrl+Enter")
                        .clicked()
                    {
                        self.request_sweep(host);
                    }
                    if ui
                        .add_enabled(runs.is_ok(), egui::Button::new("Estimate time"))
                        .on_hover_text("Time three runs now, without saving anything")
                        .clicked()
                    {
                        self.start_estimate(host);
                    }
                    if ui
                        .button("Edit as script")
                        .on_hover_text("Put this sweep in the script editor, to read, change, or save")
                        .clicked()
                    {
                        self.text = jetcarp_script::sweep_to_script(&self.form.to_json());
                        self.path = None;
                        self.saved.clear();
                        self.tab = Tab::Script;
                    }
                }
                Tab::Script => {
                    let ok = self.check.is_none();
                    if ui
                        .add_enabled(!running && ok, egui::Button::new(RichText::new("Run script").strong()))
                        .on_hover_text("Ctrl+Enter")
                        .clicked()
                    {
                        self.run_script(host);
                    }
                }
            }
            if running && ui.button("Cancel").clicked()
                && let Some(r) = &self.running
            {
                r.job.cancel();
                self.log.push("cancelling (runs in progress finish first)…".into());
            }
            ui.separator();
            ui.label("Threads");
            ui.add_enabled(
                !running,
                egui::DragValue::new(&mut self.threads).range(1..=self.cores),
            )
            .on_hover_text(format!(
                "Worker threads for scripts and sweeps (this computer has {}). Results do not depend on it; fewer leaves the app smoother.",
                self.cores
            ));
            ui.label(RichText::new(format!("of {}", self.cores)).weak());
        });
        // Estimate and confirmation.
        let current = self.form.to_json();
        match &self.estimate {
            Some(Estimating::Running { spec, .. }) if *spec == current || self.confirm => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Timing a few runs…");
                });
            }
            Some(Estimating::Done { spec, result }) if *spec == current => match result {
                Ok(e) => {
                    let line = estimate_line(e, self.threads);
                    if self.confirm {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new(line).strong());
                            ui.horizontal(|ui| {
                                if ui
                                    .button(RichText::new("Start the sweep").strong())
                                    .clicked()
                                {
                                    self.run_sweep(host);
                                }
                                if ui.button("Not now").clicked() {
                                    self.confirm = false;
                                }
                            });
                        });
                    } else {
                        ui.label(line);
                    }
                }
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("This sweep would fail: {e}"));
                    self.confirm = false;
                }
            },
            _ => {}
        }
        // Progress.
        if let Some(r) = &self.running {
            match &self.progress {
                Some(p) => {
                    let frac = if p.total > 0 {
                        p.done as f32 / p.total as f32
                    } else {
                        0.0
                    };
                    let eta = p
                        .eta
                        .map(|e| format!(" · about {} left", jetcarp_script::duration(e)))
                        .unwrap_or_default();
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!(
                                "{} / {} ({:.1}%){eta} · {}",
                                thousands(p.done),
                                thousands(p.total),
                                frac * 100.0,
                                p.item
                            ))
                            .animate(true),
                    );
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!(
                            "{} running for {}",
                            r.what,
                            jetcarp_script::duration(r.started.elapsed().as_secs_f64())
                        ));
                    });
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            let folder = self
                .running
                .as_ref()
                .and_then(|r| r.folder.clone())
                .or(self.last_folder.clone());
            if let Some(f) = folder {
                if ui
                    .button("Open output folder")
                    .on_hover_text(f.display().to_string())
                    .clicked()
                {
                    crate::results::open_in_file_manager(&f);
                }
                if (f.join("manifest.json").exists() || f.join("manifest.partial.jsonl").exists())
                    && ui.button("Browse results").clicked()
                {
                    out.push(Action::Browse(f.clone()));
                }
                ui.label(RichText::new(f.display().to_string()).small().weak());
            }
        });
        // Log.
        if self.log.is_empty() {
            ui.add_space(2.0);
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("scripts-log")
            .stick_to_bottom(true)
            .max_height(150.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for l in &self.log {
                    let c = if l.starts_with("error") {
                        Color32::from_rgb(0xff, 0x80, 0x80)
                    } else {
                        Color32::from_gray(190)
                    };
                    ui.label(RichText::new(l).monospace().size(12.0).color(c));
                }
            });
    }
}

fn estimate_line(e: &jetcarp_script::Estimate, threads: usize) -> String {
    let per = if e.seconds_per_run < 1.0 {
        format!("{:.0} ms", (e.seconds_per_run * 1000.0).max(1.0))
    } else {
        jetcarp_script::duration(e.seconds_per_run)
    };
    let tiling = e.tiling.split(':').next().unwrap_or(&e.tiling).trim();
    let mut s = format!(
        "{} runs of {} cells ({tiling}): about {per} per run, so about {} on {threads} threads",
        thousands(e.runs as u64),
        thousands(e.cells as u64),
        jetcarp_script::duration(e.seconds(threads).max(0.1)),
    );
    if let (Some(b), Some((w, h))) = (e.bytes_per_image, e.image) {
        let total = b * e.runs as f64;
        let size = if total > 1e9 {
            format!("{:.1} GB", total / 1e9)
        } else {
            format!("{:.0} MB", (total / 1e6).max(1.0))
        };
        s.push_str(&format!("; {w} x {h} images, roughly {size} in all"));
    }
    s
}

/// The function reference (Markdown with tables) as egui widgets. `query` keeps only matching
/// table rows and paragraphs.
fn markdown(ui: &mut egui::Ui, md: &str, query: &str) {
    let q = query.trim().to_ascii_lowercase();
    let hit = |s: &str| q.is_empty() || s.to_ascii_lowercase().contains(&q);
    let mut code = false;
    let mut rows: Vec<Vec<String>> = vec![];
    let flush = |ui: &mut egui::Ui, rows: &mut Vec<Vec<String>>| {
        if rows.is_empty() {
            return;
        }
        // One entry per row: the function (or option) on its own line, the text indented below,
        // so long signatures stay readable in a narrow window.
        for (k, r) in rows.iter().enumerate() {
            let first = r.first().cloned().unwrap_or_default();
            let rest = r[1..].join(" · ");
            let fill = if k % 2 == 0 {
                Color32::from_gray(32)
            } else {
                Color32::TRANSPARENT
            };
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(inline(ui, &first, true)).wrap());
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.add(egui::Label::new(inline(ui, &rest, false)).wrap());
                    });
                });
        }
        rows.clear();
    };
    let mut header = true;
    for line in md.lines() {
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            if hit(line) {
                ui.label(RichText::new(line).monospace().size(12.5));
            }
            continue;
        }
        if let Some(t) = line.strip_prefix('|') {
            if t.trim_start().starts_with("---") {
                continue;
            }
            let cells = split_row(t);
            if header {
                // The table's header row.
                header = false;
                continue;
            }
            if hit(line) {
                rows.push(cells);
            }
            continue;
        }
        flush(ui, &mut rows);
        header = true;
        if let Some(h) = line.strip_prefix("# ") {
            ui.label(RichText::new(h).size(20.0).strong());
        } else if let Some(h) = line.strip_prefix("## ") {
            ui.add_space(8.0);
            ui.label(RichText::new(h).size(16.0).strong());
        } else if let Some(h) = line.strip_prefix("### ") {
            ui.label(RichText::new(h).strong());
        } else if line.trim().is_empty() {
            ui.add_space(4.0);
        } else if hit(line) {
            ui.add(egui::Label::new(inline(ui, line, false)).wrap());
        }
    }
    flush(ui, &mut rows);
}

/// Split a table row at `|`, except inside backticks.
fn split_row(t: &str) -> Vec<String> {
    let mut cells = vec![];
    let mut cur = String::new();
    let mut tick = false;
    for c in t.chars() {
        match c {
            '`' => {
                tick = !tick;
                cur.push(c);
            }
            '|' if !tick => {
                cells.push(cur.trim().to_string());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        cells.push(cur.trim().to_string());
    }
    cells
}

/// Text with `code` spans in monospace.
fn inline(ui: &egui::Ui, s: &str, strong: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::FontId::monospace(body.size - 0.5);
    let text_c = if strong {
        Color32::from_gray(235)
    } else {
        Color32::from_gray(200)
    };
    let code_c = Color32::from_rgb(0xff, 0xd8, 0x9b);
    for (i, part) in s.split('`').enumerate() {
        let part = part.replace("**", "");
        if part.is_empty() {
            continue;
        }
        let (font, c) = if i % 2 == 1 {
            (mono.clone(), code_c)
        } else {
            (body.clone(), text_c)
        };
        job.append(&part, 0.0, egui::TextFormat::simple(font, c));
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_pass_the_syntax_check() {
        for (name, src) in EXAMPLES {
            assert!(jetcarp_script::check(src).is_ok(), "{name}");
        }
        assert!(jetcarp_script::check(NEW_SCRIPT).is_ok());
    }

    #[test]
    fn table_rows_keep_closures() {
        let c = split_row(" `keep` | Optional `|w, n| ...` called | x |");
        assert_eq!(c, vec!["`keep`", "Optional `|w, n| ...` called", "x"]);
    }
}
