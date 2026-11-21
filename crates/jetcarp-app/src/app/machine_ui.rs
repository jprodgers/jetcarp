//! *Tools → This computer…* (#213): the hardware, the JETCARP Hardware Score with its parts and
//! the date of the last run, *Run again* and *Save a report*, and the limits, each with its
//! suggestion, your own value, its ceiling and *Back to the suggestion*.
//!
//! Also the question a world over this machine's memory limit gets before it is built: what it
//! needs, the limit and where the limit came from, with *Build anyway* (up to the code's own
//! ceilings) or *Cancel*. The command line, scripts and the MCP tools refuse instead.
//!
//! The benchmark runs on a thread of its own, through `machine.score` as `jetcarp score` runs
//! it; while it runs nothing is drawn but a progress bar (the world is paused, so it does not
//! slow the measuring either).

use super::*;
use jetcarp_api::Ctx;
use jetcarp_api::op::{NoInput, Op, Progress};
use jetcarp_api::ops::machine::{
    LimitResetIn, LimitSetIn, MachineInfo, MachineLimitsReset, MachineLimitsSet, MachineOut,
    MachineScore, ScoreIn, ScoreOut, save_report,
};
use jetcarp_core::machine::{self, Id, Source, Unit, limits};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

/// The window's key (popout registry and settings).
pub(super) const KEY: &str = "this-computer";

/// What the window holds between frames.
#[derive(Default)]
pub(super) struct MachineWin {
    pub open: bool,
    /// The machine, its score and limits, read when the window opens and after each change.
    pub info: Option<MachineOut>,
    /// What is typed in each limit's own-value field.
    typed: Vec<(Id, String)>,
    run: Option<Run>,
    /// The last thing done, in a line (an error in red).
    note: Option<(String, bool)>,
    /// A world over the limit, waiting for *Build anyway* or *Cancel*.
    pub ask: Option<OverLimitAsk>,
    /// *Build anyway* was pressed: the next build is not asked about again.
    pub anyway: bool,
}

/// The benchmark running.
struct Run {
    progress: Arc<Mutex<(f32, String)>>,
    stop: Arc<AtomicBool>,
    rx: Receiver<Result<ScoreOut, String>>,
}

struct RunProgress {
    progress: Arc<Mutex<(f32, String)>>,
    stop: Arc<AtomicBool>,
}

impl Progress for RunProgress {
    fn report(&self, done: u64, total: Option<u64>, msg: &str) {
        if let Ok(mut p) = self.progress.lock() {
            *p = (
                total.map_or(0.0, |t| done as f32 / t.max(1) as f32),
                msg.to_string(),
            );
        }
    }
    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// What waits on the answer to a world over the limit.
pub(super) enum Then {
    Scene(Box<Scene>, bool),
    Tiling(TilingSpec),
}

pub(super) struct OverLimitAsk {
    pub message: String,
    pub then: Then,
}

/// The operations' context in the app: its platform, the settings folder it reads.
fn op_ctx() -> Ctx {
    Ctx::new().with_platform(Arc::new(crate::window_job::AppPlatform))
}

/// A world of `cas` on `spec` against this machine's memory limit: the refusal's text, if it
/// is over.
pub(super) fn over_limit(spec: &TilingSpec, cas: &[jetcarp_core::CaSpec]) -> Option<String> {
    jetcarp_api::ops::world::check_world_memory(spec, cas, &jetcarp_api::Limits::for_this_machine())
        .err()
        .map(|e| e.message)
}

impl MachineWin {
    /// Read the machine, its score and its limits again, and the fields from them.
    pub fn reload(&mut self) {
        let info = MachineInfo::run(&op_ctx(), NoInput {}, &jetcarp_api::op::Quiet).ok();
        self.typed = info
            .as_ref()
            .map(|i| {
                i.limits
                    .entries
                    .iter()
                    .map(|e| (e.id, own_text(e)))
                    .collect()
            })
            .unwrap_or_default();
        self.info = info;
    }

    /// Show `info` (the book's pictures show an example machine, never the one running them).
    #[cfg(test)]
    pub fn show_info(&mut self, info: MachineOut) {
        self.typed = info
            .limits
            .entries
            .iter()
            .map(|e| (e.id, own_text(e)))
            .collect();
        self.info = Some(info);
    }

    fn typed_mut(&mut self, id: Id) -> &mut String {
        if !self.typed.iter().any(|t| t.0 == id) {
            self.typed.push((id, String::new()));
        }
        &mut self.typed.iter_mut().find(|t| t.0 == id).unwrap().1
    }

    fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Start the benchmark (about a minute).
    fn start(&mut self) {
        let progress = Arc::new(Mutex::new((0.0, "Starting".to_string())));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let p = RunProgress {
            progress: progress.clone(),
            stop: stop.clone(),
        };
        std::thread::Builder::new()
            .name("jetcarp-score".into())
            .spawn(move || {
                let r = MachineScore::run(&op_ctx(), ScoreIn::default(), &p).map_err(|e| e.message);
                let _ = tx.send(r);
            })
            .ok();
        self.note = None;
        self.run = Some(Run { progress, stop, rx });
    }

    /// Take the benchmark's result when it ends.
    fn poll(&mut self) {
        let Some(r) = &self.run else { return };
        let Ok(out) = r.rx.try_recv() else { return };
        self.run = None;
        match out {
            Ok(s) => {
                self.note = Some((
                    match s.score.wrong().count() {
                        0 => format!("Measured: JETCARP Hardware Score {}.", s.score.text()),
                        n => format!(
                            "Measured, with {n} wrong answers: the devices that gave them are not used for those workloads."
                        ),
                    },
                    s.score.wrong().next().is_some(),
                ));
                machine::reload();
            }
            Err(e) => self.note = Some((e, true)),
        }
        self.reload();
    }

    /// A running desktop background reads its limits again (none running: nothing to tell).
    fn tell_host() {
        let _ = jetcarp_mcp::link::host_call(&serde_json::json!({"action": "reload"}));
    }

    fn set(&mut self, id: Id, text: &str) {
        let r = MachineLimitsSet::run(
            &op_ctx(),
            LimitSetIn {
                name: id.name().into(),
                value: text.into(),
            },
            &jetcarp_api::op::Quiet,
        );
        self.note = match r {
            Ok(_) => {
                Self::tell_host();
                None
            }
            Err(e) => Some((e.message, true)),
        };
        self.reload();
    }

    fn reset(&mut self, id: Option<Id>) {
        let r = MachineLimitsReset::run(
            &op_ctx(),
            LimitResetIn {
                name: id.map(|i| i.name().to_string()),
            },
            &jetcarp_api::op::Quiet,
        );
        match r {
            Ok(_) => Self::tell_host(),
            Err(e) => self.note = Some((e.message, true)),
        }
        self.reload();
    }

    fn save_report(&mut self) {
        let Some(info) = &self.info else { return };
        let Some(score) = &info.score else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name("jetcarp-report.md")
            .save_file()
        else {
            return;
        };
        let l = machine::Limits::compute(
            &info.hardware,
            &machine::Own::load(jetcarp_core::paths::config_dir().as_deref()).0,
        );
        self.note = Some(
            match save_report(
                &op_ctx(),
                &path.display().to_string(),
                score,
                &info.hardware,
                &l,
            ) {
                Ok(files) => (
                    format!(
                        "Saved {}",
                        files
                            .iter()
                            .map(|f| f.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                    false,
                ),
                Err(e) => (e.message, true),
            },
        );
    }
}

/// An own value as its field shows it (empty: none).
fn own_text(e: &machine::Entry) -> String {
    e.own
        .map(|v| limits::value_text(e.unit, v))
        .unwrap_or_default()
}

impl JetcarpApp {
    /// Tools → This computer…, and the question a world over the limit gets.
    pub(super) fn machine_window(&mut self, ctx: &egui::Context) {
        self.machine.poll();
        if self.machine.running() {
            self.playing = false;
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.machine.open && self.machine.info.is_none() {
            self.machine.reload();
        }
        let mut open = self.machine.open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "This computer").size(760.0, 640.0),
            &mut open,
            |ui| self.machine_ui(ui),
        );
        self.machine.open = open;
        self.over_limit_window(ctx);
    }

    fn machine_ui(&mut self, ui: &mut egui::Ui) {
        let m = &mut self.machine;
        let Some(info) = m.info.clone() else {
            ui.label("Reading this computer…");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("The machine");
            egui::Grid::new("machine-hw")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    let hw = &info.hardware;
                    ui.label(RichText::new("System").strong());
                    ui.label(format!("{} {}", hw.os, hw.arch));
                    ui.end_row();
                    ui.label(RichText::new("Processor").strong());
                    ui.label(format!(
                        "{} ({} threads{})",
                        hw.cpu,
                        hw.threads,
                        hw.cores.map_or(String::new(), |c| format!(", {c} cores"))
                    ));
                    ui.end_row();
                    ui.label(RichText::new("Memory").strong());
                    ui.label(format!(
                        "{}{}",
                        machine::bytes_text(hw.memory_total),
                        hw.memory_available.map_or(String::new(), |a| format!(
                            " ({} free now)",
                            machine::bytes_text(a)
                        ))
                    ));
                    ui.end_row();
                    for a in &info.adapters {
                        ui.label(RichText::new("GPU").strong());
                        ui.label(format!("{} ({}, {})", a.name, a.kind, a.backend));
                        ui.end_row();
                    }
                    for g in &hw.gpus {
                        ui.label(RichText::new("GPU memory").strong());
                        ui.label(match (g.dedicated, g.shared, g.unified) {
                            (_, Some(sh), true) => {
                                format!("shares the system's {}", machine::bytes_text(sh))
                            }
                            (Some(d), Some(sh), _) => format!(
                                "{} of its own, may borrow {} ({})",
                                machine::bytes_text(d),
                                machine::bytes_text(sh),
                                g.source
                            ),
                            (Some(d), None, _) => {
                                format!("{} of its own ({})", machine::bytes_text(d), g.source)
                            }
                            _ => "not reported".into(),
                        });
                        ui.end_row();
                    }
                });
            ui.add_space(10.0);
            ui.heading("JETCARP Hardware Score");
            match &info.score {
                Some(s) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.text()).size(22.0).strong());
                        ui.label(
                            RichText::new(format!(
                                "score v{}, {}, JETCARP {}{}",
                                s.benchmark,
                                if s.date.is_empty() { "undated" } else { &s.date },
                                s.jetcarp,
                                if s.quick { ", a short run" } else { "" }
                            ))
                            .weak(),
                        );
                    });
                    if let Some(why) = &info.stale {
                        ui.colored_label(crate::theme::ink(ui, Color32::from_rgb(0xff, 0xb0, 0x60)), format!("{why}: run it again."));
                    }
                    egui::CollapsingHeader::new("Its parts")
                        .id_salt("machine-parts")
                        .show(ui, |ui| {
                            egui::Grid::new("machine-parts-grid")
                                .num_columns(3)
                                .striped(true)
                                .spacing([12.0, 2.0])
                                .show(ui, |ui| {
                                    for p in &s.parts {
                                        ui.label(if p.scored {
                                            RichText::new(&p.label).strong()
                                        } else {
                                            RichText::new(&p.label)
                                        });
                                        ui.label(RichText::new(&p.device).weak());
                                        let v = format!(
                                            "{} {}",
                                            machine::score::rate_text(p.value),
                                            p.unit
                                        );
                                        if p.correct == Some(false) {
                                            ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), v + ", wrong answer");
                                        } else {
                                            ui.label(v);
                                        }
                                        ui.end_row();
                                    }
                                });
                            for p in &s.problems {
                                ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), p);
                            }
                        });
                }
                None => {
                    ui.label("Not measured yet.");
                }
            }
            ui.label(
                RichText::new(
                    "About a minute of JETCARP's own work: Life soups on five tilings (the score), \
                     one core, a hunt, the GPU, memory and the disk, each answer checked. Nothing \
                     is drawn but the progress bar, and nothing is sent anywhere.",
                )
                .weak(),
            );
            match &m.run {
                Some(r) => {
                    let (f, what) = r.progress.lock().map(|p| p.clone()).unwrap_or_default();
                    ui.add(egui::ProgressBar::new(f).text(what));
                    if ui.button("Stop").clicked() {
                        r.stop.store(true, Ordering::Relaxed);
                    }
                }
                None => {
                    ui.horizontal(|ui| {
                        let label = if info.score.is_some() {
                            "Run again"
                        } else {
                            "Measure this computer"
                        };
                        if ui.button(label).clicked() {
                            m.start();
                        }
                        if ui
                            .add_enabled(info.score.is_some(), egui::Button::new("Save a report…"))
                            .on_hover_text("The machine, its parts, its score and its limits, in Markdown and JSON, for comparing machines")
                            .clicked()
                        {
                            m.save_report();
                        }
                    });
                }
            }
            if let Some((n, err)) = &m.note {
                if *err {
                    ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), n);
                } else {
                    ui.label(n);
                }
            }
            ui.add_space(10.0);
            ui.heading("Limits");
            ui.label(
                RichText::new(
                    "Each is your own value if you give one, else this machine's suggestion, \
                     never above its ceiling. They only allow or refuse: a world too big is \
                     refused with the reason, never made smaller, so a result never depends on \
                     the machine.",
                )
                .weak(),
            );
            ui.add_space(4.0);
            let mut set: Option<(Id, String)> = None;
            let mut reset: Option<Option<Id>> = None;
            egui::Grid::new("machine-limits")
                .num_columns(5)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Limit", "Now", "Suggested", "Your own", "Ceiling"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for e in &info.limits.entries {
                        ui.label(&e.label).on_hover_text(e.id.what());
                        let now = RichText::new(e.text());
                        ui.label(match e.source {
                            Source::Own => now.strong(),
                            Source::Ceiling => now.color(crate::theme::ink(ui, Color32::from_rgb(0xff, 0xb0, 0x60))),
                            Source::Suggested => now,
                        });
                        ui.label(limits::value_text(e.unit, e.suggestion))
                            .on_hover_text(&e.why);
                        ui.horizontal(|ui| {
                            if e.unit == Unit::Share {
                                let mut v = e.value as f32;
                                let r = ui.add(
                                    egui::Slider::new(&mut v, 0.0..=1.0)
                                        .custom_formatter(|v, _| {
                                            limits::value_text(Unit::Share, v)
                                        })
                                        .show_value(true),
                                );
                                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                    set = Some((e.id, format!("{v}")));
                                }
                            } else {
                                let t = m.typed_mut(e.id);
                                let r = ui.add(
                                    egui::TextEdit::singleline(t)
                                        .desired_width(110.0)
                                        .hint_text("your own"),
                                );
                                if r.lost_focus() && t.trim() != own_text(e) {
                                    if t.trim().is_empty() {
                                        reset = Some(Some(e.id));
                                    } else {
                                        set = Some((e.id, t.trim().to_string()));
                                    }
                                }
                            }
                            if e.own.is_some()
                                && ui
                                    .small_button("Back to the suggestion")
                                    .clicked()
                            {
                                reset = Some(Some(e.id));
                            }
                        });
                        ui.label(limits::value_text(e.unit, e.ceiling))
                            .on_hover_text(&e.ceiling_why);
                        ui.end_row();
                    }
                });
            if info.limits.entries.iter().any(|e| e.own.is_some())
                && ui.button("All back to the suggestions").clicked()
            {
                reset = Some(None);
            }
            if let Some((id, t)) = set {
                m.set(id, &t);
            }
            if let Some(id) = reset {
                m.reset(id);
            }
            ui.label(
                RichText::new(
                    "Memory as 24GB or 512MB, counts as 64000000 or 64M. Reserved for the System \
                     and Reserved for JETCARP are one split: setting one sets the other. Kept in \
                     limits.json in the settings folder; measuring again never undoes them.",
                )
                .small()
                .weak(),
            );
        });
    }

    /// A world over the machine's memory limit: say so, and build it only if asked.
    fn over_limit_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = &self.machine.ask else { return };
        let message = ask.message.clone();
        let mut answer = None;
        egui::Window::new("Over this machine's limit")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(capital(&message));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Building it anyway may make this computer slow, or stop JETCARP if the \
                         memory runs out. Nothing about the world changes either way.",
                    )
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Build anyway").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                    if ui.button("This computer…").clicked() {
                        self.machine.open = true;
                        self.machine.info = None;
                        answer = Some(false);
                    }
                });
            });
        let Some(yes) = answer else { return };
        let Some(ask) = self.machine.ask.take() else {
            return;
        };
        if !yes {
            self.message = Some(("Not built: over this machine's limit.".into(), false));
            return;
        }
        self.machine.anyway = true;
        match ask.then {
            Then::Scene(s, play) => self.load_scene_bg_as(&s, play),
            Then::Tiling(t) => self.set_tiling_bg(t),
        }
        self.machine.anyway = false;
    }

    /// Before a world is built: over the limit, ask (and build nothing yet); else go on.
    pub(super) fn ask_over_limit(&mut self, then: Then) -> Option<Then> {
        if self.machine.anyway {
            return Some(then);
        }
        let over = match &then {
            Then::Scene(s, _) => over_limit(&s.tiling, &s.cas),
            Then::Tiling(t) => {
                let cas: Vec<_> = self.world.cas.iter().map(|c| c.spec()).collect();
                over_limit(t, &cas)
            }
        };
        match over {
            Some(message) => {
                self.machine.ask = Some(OverLimitAsk { message, then });
                None
            }
            None => Some(then),
        }
    }

    /// Once at start: a changed machine (or a newer benchmark) suggests measuring again, in the
    /// status line.
    pub(super) fn machine_suggestion(&mut self) {
        let hw = machine::hardware()
            .map(|h| (*h).clone())
            .unwrap_or_else(machine::Hardware::read);
        if let Some(s) =
            machine::suggest_measuring(jetcarp_core::paths::config_dir().as_deref(), &hw)
        {
            self.message = Some((s, false));
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world over the machine's memory limit gets the question, with what it needs and the
    /// limit; one within it does not (the reference machine's 4 GB in tests).
    #[test]
    fn worlds_over_the_limit_are_asked_about() {
        let life = [jetcarp_core::CaSpec::new("B3/S23")];
        let mut big = TilingSpec::new("square");
        big.size = Some(jetcarp_core::SizeSpec::Rect { w: 30000, h: 30000 });
        let m = over_limit(&big, &life).expect("over the limit");
        assert!(
            m.starts_with("this world needs 6.7 GB; this machine's limit is 4 GB"),
            "{m}"
        );
        assert_eq!(capital(&m)[..10], *"This world");
        let mut small = TilingSpec::new("square");
        small.size = Some(jetcarp_core::SizeSpec::Rect { w: 300, h: 300 });
        assert_eq!(over_limit(&small, &life), None);
    }
}
