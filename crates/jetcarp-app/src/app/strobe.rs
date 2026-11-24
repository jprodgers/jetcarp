//! The strobe guard in the app (#97, #148): the settings it reads, what each view's guard asks of
//! the running world (a pace cap, a pause), the status line, and Settings → Safety, where someone
//! who answered *no disorder* can turn a guard off through a deliberate confirmation.
//!
//! Each view's guard runs in its paint callback (`crate::gpu`); this side gathers what they said
//! ([`GuardOut`]) once a frame: the strictest cap holds the world's speed down, a pause stops it
//! with a note, and their notes go to the status line. The setting is read from the warning's
//! answer (`jetcarp_core::photosensitivity`) every couple of seconds, so a change made elsewhere
//! (another JETCARP window) shows. The app's screensaver mode follows the desktop's setting, as
//! the background host and the screen savers do: people who pass by never answered the warning.
//!
//! An AI working through AI control acts through the same world but has no way to these switches:
//! no tool, command or script turns a guard off.

use super::menu::ToolWin;
use super::*;
use crate::gpu::{GuardFeed, GuardOut};
use jetcarp_core::photosensitivity::{self as ps, Answer, GuardPlace};
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::guard::{self, Action, Pace, Report};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// How often the setting is read again, in seconds.
const READ_EVERY: f64 = 2.0;
/// A view's word counts for this long: one that stopped drawing no longer holds the world back.
const FRESH: f64 = 2.0;
/// Notes kept for the Safety window.
const NOTES: usize = 30;

/// The confirmation open in Settings → Safety: for which switch, and whether its box is ticked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Confirm {
    pub place: GuardPlace,
    pub ticked: bool,
}

pub(crate) struct AppStrobe {
    pub on_app: bool,
    pub on_desktop: bool,
    pub answer: Option<Answer>,
    read_at: f64,
    pub out: Arc<Mutex<GuardOut>>,
    /// Bumped when the person runs a world the guard paused.
    resume: u64,
    /// The strictest cap and the pause the views ask for now.
    pub cap: Option<f32>,
    pub paused: Option<String>,
    /// The most acting view's report.
    pub report: Option<Report>,
    /// What the guards did lately: (the app's clock, the note).
    pub notes: VecDeque<(f64, String)>,
    /// The world was running when the guard paused it.
    was_playing: bool,
    /// Generations a second reached: where the count started (seconds, generation).
    mark: Option<(f64, u64)>,
    pub reached: Option<f32>,
    /// The person's steps in the last second (the app's clock), the pace the guard is told while
    /// the world is stepped rather than run (#293), and when it last said it held a step back.
    steps: VecDeque<f64>,
    held_note_at: f64,
    pub window_open: bool,
    pub confirm: Option<Confirm>,
    pub error: Option<String>,
}

impl Default for AppStrobe {
    fn default() -> Self {
        let rec = ps::read();
        AppStrobe {
            on_app: rec.as_ref().is_none_or(|r| r.guard_on(GuardPlace::App)),
            on_desktop: rec.as_ref().is_none_or(|r| r.guard_on(GuardPlace::Desktop)),
            answer: rec.map(|r| r.answer),
            read_at: f64::NEG_INFINITY,
            out: Arc::new(Mutex::new(GuardOut::default())),
            resume: 0,
            cap: None,
            paused: None,
            report: None,
            notes: VecDeque::new(),
            was_playing: false,
            mark: None,
            reached: None,
            steps: VecDeque::new(),
            held_note_at: f64::NEG_INFINITY,
            window_open: false,
            confirm: None,
            error: None,
        }
    }
}

impl AppStrobe {
    fn read(&mut self) {
        let rec = ps::read();
        self.on_app = rec.as_ref().is_none_or(|r| r.guard_on(GuardPlace::App));
        self.on_desktop = rec.as_ref().is_none_or(|r| r.guard_on(GuardPlace::Desktop));
        self.answer = rec.map(|r| r.answer);
    }

    /// Read the setting again now (tests; the app reads it every couple of seconds).
    #[cfg(test)]
    pub fn reread(&mut self) {
        self.read();
    }

    fn note(&mut self, now: f64, n: String) {
        if self.notes.len() == NOTES {
            self.notes.pop_front();
        }
        self.notes.push_back((now, n));
    }
}

impl JetcarpApp {
    /// The setting the main view follows now: the desktop's in screensaver mode.
    pub(super) fn guard_on_here(&self) -> bool {
        if self.saver_active() {
            self.strobe.on_desktop
        } else {
            self.strobe.on_app
        }
    }

    /// Once a frame, before the world steps: read the setting now and then, gather what the
    /// views' guards asked, stop a world the guard paused, and count the speed reached.
    pub(super) fn strobe_frame(&mut self) {
        let now = self.started.elapsed().as_secs_f64();
        if now - self.strobe.read_at >= READ_EVERY {
            self.strobe.read_at = now;
            self.strobe.read();
        }
        // The speed reached, over a second or so of running.
        let g = self.world.generation;
        match self.strobe.mark {
            Some((t, g0)) if self.playing && g >= g0 => {
                if now - t >= 1.0 {
                    self.strobe.reached = Some(((g - g0) as f64 / (now - t)) as f32);
                    self.strobe.mark = Some((now, g));
                }
            }
            _ => {
                self.strobe.mark = self.playing.then_some((now, g));
                if !self.playing {
                    self.strobe.reached = None;
                }
            }
        }
        // Stepped by hand (#293): the steps of the last second are the pace the guard is told, as
        // a run's generations are, so it caps them from the rate they really come at.
        while self.strobe.steps.front().is_some_and(|t| now - t > 1.0) {
            self.strobe.steps.pop_front();
        }
        if !self.playing && !self.strobe.steps.is_empty() {
            self.strobe.reached = Some(self.strobe.steps.len() as f32);
        }
        let (mut cap, mut paused, mut report, mut notes) =
            (None::<f32>, None::<String>, None::<Report>, vec![]);
        if let Ok(mut out) = self.strobe.out.lock() {
            out.views.retain(|_, v| now - v.at < FRESH);
            for v in out.views.values_mut() {
                if let Some(c) = v.cap {
                    cap = Some(cap.map_or(c, |m: f32| m.min(c)));
                }
                if paused.is_none() {
                    paused = v.paused.clone();
                }
                if report.as_ref().is_none_or(|r| v.report.action > r.action) {
                    report = Some(v.report.clone());
                }
                notes.append(&mut v.notes);
            }
        }
        for n in notes {
            self.strobe.note(now, n.clone());
            let mut s = n;
            if let Some(c) = s.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            // A new message shows for a few seconds over the grid when the status bar is off.
            self.message = Some((format!("Strobe guard: {s}."), false));
        }
        // Paused by the guard: the world stops; running it again resumes the guard's watch.
        match (&paused, &self.strobe.paused) {
            (Some(_), None) => {
                self.strobe.was_playing = self.playing;
                self.playing = false;
            }
            (Some(_), Some(_)) if self.playing => {
                self.strobe.resume += 1;
                paused = None;
                if let Ok(mut out) = self.strobe.out.lock() {
                    for v in out.views.values_mut() {
                        v.paused = None;
                    }
                }
            }
            _ => {}
        }
        self.strobe.cap = cap;
        self.strobe.paused = paused;
        self.strobe.report = report;
    }

    /// The speed the world runs at under the guard's cap (generations a second), and whether it
    /// runs as fast as it can.
    pub(super) fn guarded_speed(&self) -> (f32, bool) {
        match self.strobe.cap {
            Some(c) if self.max_speed || self.speed > c => (c, false),
            _ => (self.speed, self.max_speed),
        }
    }

    /// Whether a step the person (or AI control) asks for now may go (#293). The guard's cap holds
    /// steps as it holds a run: a step sooner than one over the cap after the last is dropped, so
    /// a key held down builds no backlog. While the guard holds a pause, steps come no faster than
    /// its slowest pace. A step that goes is counted for the pace the guard is told.
    pub(super) fn step_allowed(&mut self) -> bool {
        let now = self.started.elapsed().as_secs_f64();
        let limit = if !self.guard_on_here() {
            None
        } else if self.strobe.paused.is_some() {
            Some(
                self.strobe
                    .cap
                    .map_or(guard::MIN_PACE, |c| c.min(guard::MIN_PACE)),
            )
        } else {
            self.strobe.cap
        };
        if let (Some(limit), Some(last)) = (limit, self.strobe.steps.back())
            && now - last < 1.0 / limit.max(1e-3) as f64 - 1e-6
        {
            if now - self.strobe.held_note_at >= 1.0 {
                self.strobe.held_note_at = now;
                self.message = Some((
                    format!(
                        "Strobe guard: steps held to {} while this scene flashes.",
                        steps_words(limit)
                    ),
                    false,
                ));
            }
            return false;
        }
        self.strobe.steps.push_back(now);
        true
    }

    /// What view `view`'s guard needs this frame, for a view of `size` physical pixels.
    pub(super) fn guard_feed(&self, view: u64, size: egui::Vec2) -> GuardFeed {
        let ppp = self.ppp() as f64;
        let ctx = self.ctx.as_ref();
        // The grid tile in the window is a part of its monitor's screen; outputs and full screen
        // fill theirs.
        let part = view == super::view::TILE_VIEW && self.want_fullscreen().is_none();
        let screen_px = part
            .then(|| ctx.and_then(|c| c.input(|i| i.viewport().monitor_size)))
            .flatten()
            .map(|m| (m.x as f64 * ppp) * (m.y as f64 * ppp))
            .filter(|px| *px > size.x as f64 * size.y as f64);
        let (_, back) = jetcarp_core::render::backdrop(&self.world);
        GuardFeed {
            on: self.guard_on_here(),
            at_risk: self.strobe.answer == Some(ps::Answer::AtRisk),
            now: self.started.elapsed().as_secs_f64(),
            viewing: Viewing::screen(ppp),
            screen_px,
            pace: Pace {
                speed: if self.max_speed { 0.0 } else { self.speed },
                reached: self.strobe.reached,
                frame_rate: self.fps,
            },
            scene: self.geom_counter,
            resume: self.strobe.resume,
            backdrop: [back[0], back[1], back[2]],
            out: self.strobe.out.clone(),
        }
    }

    /// The guard's word for the status bar, while it acts (or, switched off, while a scene flashes).
    pub(super) fn guard_status(&self) -> Option<(String, bool)> {
        let r = self.strobe.report.as_ref()?;
        let note = r.note.clone()?;
        Some((note, r.action == Action::Paused || !r.on))
    }

    /// Settings → Safety: the strobe guard's switches.
    pub(super) fn safety_window(&mut self, ctx: &egui::Context) {
        let mut open = self.strobe.window_open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(ToolWin::Safety.key(), "Safety").size(520.0, 420.0),
            &mut open,
            |ui| self.safety_ui(ui),
        );
        self.strobe.window_open = open;
        if let Some(c) = self.strobe.confirm {
            self.confirm_window(ctx, c);
        }
    }

    fn safety_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Strobe guard");
        ui.label(
            "The strobe guard watches what JETCARP draws, and softens, slows or pauses a scene \
             before it flashes more than three times a second or shows strong flickering \
             stripes. It lowers the risk of a seizure; it does not remove it, and the \
             photosensitive seizure warning still applies.",
        );
        ui.add_space(6.0);
        let at_risk = self.strobe.answer == Some(Answer::AtRisk);
        for place in [GuardPlace::App, GuardPlace::Desktop] {
            let on = match place {
                GuardPlace::App => self.strobe.on_app,
                GuardPlace::Desktop => self.strobe.on_desktop,
            };
            let what = match place {
                GuardPlace::App => "In the app (its view, full screen, outputs and exports)",
                GuardPlace::Desktop => "On the desktop background and the screen saver",
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).strong());
                ui.label(if on {
                    RichText::new("on")
                        .color(crate::theme::ink(ui, Color32::from_rgb(0x8c, 0xd8, 0x8c)))
                } else {
                    RichText::new("off")
                        .color(crate::theme::ink(ui, Color32::from_rgb(0xff, 0xb0, 0x60)))
                });
            });
            ui.horizontal(|ui| {
                if at_risk {
                    ui.label(RichText::new(ps::GUARD_ALWAYS).weak());
                } else if self.strobe.answer.is_none() {
                    ui.label(
                        RichText::new("On until the photosensitive seizure warning is answered.")
                            .weak(),
                    );
                } else if on {
                    let label = match place {
                        GuardPlace::App => "Turn off the strobe guard…",
                        GuardPlace::Desktop => "Turn off the strobe guard for the desktop…",
                    };
                    if ui.button(label).clicked() {
                        self.strobe.confirm = Some(Confirm {
                            place,
                            ticked: false,
                        });
                    }
                } else {
                    let label = match place {
                        GuardPlace::App => "Turn the strobe guard back on",
                        GuardPlace::Desktop => "Turn the strobe guard back on for the desktop",
                    };
                    if ui.button(label).clicked() {
                        self.set_guard(place, true);
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(e) = &self.strobe.error {
            ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), e);
        }
        ui.separator();
        ui.label(RichText::new("Now").strong());
        match &self.strobe.report {
            Some(r) => {
                ui.label(r.note.clone().unwrap_or_else(|| "Nothing to do.".into()));
                ui.label(
                    RichText::new(format!(
                        "As drawn: {} transitions in the last second ({} red); as shown: {}. \
                         Stripes: {:.0}% of the screen.",
                        r.drawn.transitions,
                        r.drawn.red_transitions,
                        r.shown.transitions,
                        r.stripes.share * 100.0
                    ))
                    .small()
                    .weak(),
                );
            }
            None => {
                ui.label("No view is being drawn.");
            }
        }
        if !self.strobe.notes.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("What it did lately").strong());
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for (t, n) in self.strobe.notes.iter().rev() {
                        ui.label(
                            RichText::new(format!("{:>7.1} s  {n}", t))
                                .small()
                                .monospace(),
                        );
                    }
                });
        }
    }

    fn set_guard(&mut self, place: GuardPlace, on: bool) {
        match ps::set_guard(place, on) {
            Ok(_) => {
                self.strobe.error = None;
                self.strobe.read();
                let now = self.started.elapsed().as_secs_f64();
                self.strobe.note(
                    now,
                    format!(
                        "turned {} for {}",
                        if on { "on" } else { "off" },
                        place.words()
                    ),
                );
            }
            Err(e) => self.strobe.error = Some(e),
        }
    }

    /// The deliberate confirmation: its box starts unticked, and the default button keeps the
    /// guard on.
    fn confirm_window(&mut self, ctx: &egui::Context, mut c: Confirm) {
        let mut keep = false;
        let mut off = false;
        egui::Modal::new(egui::Id::unique("strobe-guard-off")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading(ps::GUARD_OFF_TITLE);
            ui.add_space(4.0);
            ui.label(ps::GUARD_OFF_BODY);
            if c.place == GuardPlace::Desktop {
                ui.add_space(4.0);
                ui.label(ps::GUARD_OFF_ROOM);
            }
            ui.add_space(8.0);
            ui.checkbox(&mut c.ticked, ps::GUARD_OFF_CHECK);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let k = ui.add(egui::Button::new(RichText::new(ps::GUARD_KEEP).strong()));
                // The default: Enter and Escape keep the guard on.
                if !k.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                    k.request_focus();
                }
                keep |= k.clicked();
                off |= ui
                    .add_enabled(c.ticked, egui::Button::new(ps::GUARD_TURN_OFF))
                    .clicked();
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = true;
        }
        if off && c.ticked {
            self.strobe.confirm = None;
            self.set_guard(c.place, false);
        } else if keep {
            self.strobe.confirm = None;
        } else {
            self.strobe.confirm = Some(c);
        }
    }
}

/// A pace of steps in words: "2 steps a second", "a step every 2 seconds".
fn steps_words(per_s: f32) -> String {
    if per_s >= 1.0 {
        let n = (per_s * 10.0).round() / 10.0;
        if n == 1.0 {
            "1 step a second".into()
        } else {
            format!("{n} steps a second")
        }
    } else {
        let every = (1.0 / per_s.max(1e-3) * 10.0).round() / 10.0;
        format!("a step every {every} seconds")
    }
}
