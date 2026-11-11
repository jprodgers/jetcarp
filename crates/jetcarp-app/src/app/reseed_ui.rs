//! The Reseed pane: a live activity graph, the automatic reseed policies (mode, trigger, target,
//! cooldown), Reseed now, and presets.
//!
//! Policies are edited as a list of [`AutoReseed`] drafts and applied with `Command::SetReseed`
//! (logged, saved in scenes) a moment after the last change and only once the mouse button is up,
//! so dragging a slider does not flood the command log. Changes made elsewhere (a scene, a
//! script, MCP) replace the drafts when there are no unapplied edits.
//!
//! While the pane is drawn the world counts changed cells every generation
//! (`World::set_track_changes`), so the graph fills; it is switched off again when the pane is
//! hidden (counting costs a field copy per generation). Settled policies measure activity
//! themselves, so their graph fills either way.

use super::*;
use jetcarp_core::reseed::{AutoReseed, PolicyStatus, ReseedMode, ReseedWhen, Sign};
pub(super) use jetcarp_display::reseed_presets::{
    PRESETS, blobs_mode, pct, preset_policies, validate_all,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Generations shown in the activity graph (the length of the world's activity history).
pub(super) const GRAPH_GENS: u64 = 512;
/// The graph's log scale: fractions from 10^-LOG_DECADES (0.01%) to 1 (100%).
const LOG_DECADES: f64 = 4.0;
/// Seconds after the last edit before the drafts are applied.
const DEBOUNCE: f64 = 0.4;
/// A named list of policies (a user preset, saved in the app settings).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct NamedPolicies {
    pub name: String,
    pub policies: Vec<AutoReseed>,
}

/// What the Reseed pane remembers between runs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(super) struct ReseedSettings {
    /// Presets saved with *Save as preset*.
    pub presets: Vec<NamedPolicies>,
    /// The Reseed now modes (random, rain, blobs).
    pub now: Vec<ReseedMode>,
    /// The preset *Seed → Auto reseed* installs when there are no policies.
    pub auto_preset: String,
}

impl Default for ReseedSettings {
    fn default() -> Self {
        ReseedSettings {
            presets: vec![],
            now: default_now().to_vec(),
            auto_preset: "screensaver".into(),
        }
    }
}

fn default_now() -> [ReseedMode; 3] {
    [
        ReseedMode::Random {
            flip: 0.2,
            sign: Default::default(),
        },
        ReseedMode::rain(),
        ReseedMode::Blobs {
            count: 2,
            min_radius: 0.06,
            max_radius: 0.16,
            live: 0.35,
            sign: Default::default(),
        },
    ]
}

/// A mode of another kind, with that kind's defaults (or the current values when unchanged).
pub(super) fn switch_mode(m: &ReseedMode, kind: &str) -> ReseedMode {
    if m.name() == kind {
        return m.clone();
    }
    match kind {
        "random" => ReseedMode::random(),
        "rain" => ReseedMode::rain(),
        _ => ReseedMode::blobs(),
    }
    .with_sign(m.sign())
}

/// A trigger of another kind, with its defaults.
pub(super) fn switch_when(w: &ReseedWhen, settled: bool) -> ReseedWhen {
    match (w, settled) {
        (ReseedWhen::Settled { .. }, true) | (ReseedWhen::Every { .. }, false) => w.clone(),
        (_, true) => ReseedWhen::settled(),
        (_, false) => ReseedWhen::Every { gens: 100 },
    }
}

/// Height in the graph (0 bottom, 1 top) of a fraction of cells changed: a log scale from 0.01%
/// to 100%, so 0.1% and 50% are both readable. Zero and anything below 0.01% sit on the bottom.
pub(super) fn graph_y(fraction: f64) -> f32 {
    if fraction.is_nan() || fraction <= 0.0 {
        return 0.0;
    }
    ((fraction.log10() + LOG_DECADES) / LOG_DECADES).clamp(0.0, 1.0) as f32
}

/// One line on what a policy is doing at generation `gen`.
pub(super) fn status_text(p: &AutoReseed, st: &PolicyStatus, generation: u64) -> String {
    use crate::sweep_form::thousands;
    let mut s = if !p.enabled {
        "off".to_string()
    } else {
        let ready_at = st.since.saturating_add(p.cooldown);
        match &p.when {
            _ if generation < ready_at => {
                format!(
                    "cooling down, {} gens left",
                    thousands(ready_at - generation)
                )
            }
            ReseedWhen::Settled { for_gens, .. } => format!(
                "quiet for {}/{} gens",
                thousands(st.streak.min(*for_gens)),
                thousands(*for_gens)
            ),
            ReseedWhen::Every { gens } => {
                let due = st.since.saturating_add(*gens).max(ready_at);
                format!("next in {} gens", thousands(due.saturating_sub(generation)))
            }
        }
    };
    match (st.fired, st.last_fired) {
        (0, _) => s.push_str(" · not fired yet"),
        (1, Some(g)) => s.push_str(&format!(" · fired once, at gen {}", thousands(g))),
        (n, Some(g)) => s.push_str(&format!(
            " · fired {} times, last at gen {}",
            thousands(n),
            thousands(g)
        )),
        (n, None) => s.push_str(&format!(" · fired {} times", thousands(n))),
    }
    s
}

/// Where a reseed mark in the graph came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    Policy(usize),
    Manual,
}

/// The pane's state.
pub(super) struct ReseedUi {
    /// The policies being edited.
    pub drafts: Vec<AutoReseed>,
    /// When the drafts last changed without being applied (seconds).
    dirty_at: Option<f64>,
    /// The world's policies when last seen, to notice changes from elsewhere.
    synced: Vec<AutoReseed>,
    /// Modes for Reseed now: random, rain, blobs.
    pub now: [ReseedMode; 3],
    /// Reseed now target (None: every running CA).
    now_target: Option<u32>,
    /// When the pane was last drawn (seconds).
    seen_at: f64,
    /// Change tracking was switched on here.
    tracking: bool,
    /// Generations reseeds fired at, oldest first.
    marks: VecDeque<(u64, Mark)>,
    /// Each policy's fired count last frame.
    fired: Vec<u64>,
    last_gen: u64,
    /// `JETCARP_RESEED=<preset>`: install once the scene has loaded.
    pub env: Option<String>,
    /// Saved presets and choices.
    pub settings: ReseedSettings,
    preset_name: String,
    error: Option<String>,
}

impl Default for ReseedUi {
    fn default() -> Self {
        ReseedUi {
            drafts: vec![],
            dirty_at: None,
            synced: vec![],
            now: default_now(),
            now_target: None,
            seen_at: f64::NEG_INFINITY,
            tracking: false,
            marks: VecDeque::new(),
            fired: vec![],
            last_gen: 0,
            env: std::env::var("JETCARP_RESEED").ok(),
            settings: ReseedSettings::default(),
            preset_name: String::new(),
            error: None,
        }
    }
}

impl ReseedUi {
    /// Settings as they are now.
    pub fn snapshot(&self) -> ReseedSettings {
        ReseedSettings {
            now: self.now.to_vec(),
            ..self.settings.clone()
        }
    }

    pub fn restore(&mut self, s: ReseedSettings) {
        for (k, m) in s.now.iter().enumerate().take(3) {
            if m.validate().is_ok() && m.name() == default_now()[k].name() {
                self.now[k] = m.clone();
            }
        }
        self.settings = s;
    }

    fn mark_dirty(&mut self, now: f64) {
        self.dirty_at = Some(now);
    }
}

/// Edit a fraction as a percentage. Returns whether it changed.
fn pct_drag(ui: &mut egui::Ui, x: &mut f64, max_pct: f64) -> egui::Response {
    let mut v = *x * 100.0;
    let r = ui.add(
        egui::DragValue::new(&mut v)
            .range(0.0..=max_pct)
            .speed(0.05)
            .max_decimals(3)
            .suffix("%"),
    );
    if r.changed() {
        *x = (v / 100.0).clamp(0.0, max_pct / 100.0);
    }
    r
}

/// The mode's parameters. Returns whether anything changed.
fn mode_editor(ui: &mut egui::Ui, m: &mut ReseedMode, signed: bool) -> bool {
    let mut changed = false;
    match m {
        ReseedMode::Random { flip, .. } => {
            ui.horizontal(|ui| {
                ui.label("Flip");
                changed |= pct_drag(ui, flip, 100.0)
                    .on_hover_text(
                        "Each cell flips (dead to live, live to dead) with this probability",
                    )
                    .changed();
                ui.label("of cells");
            });
        }
        ReseedMode::Rain {
            drops,
            min_size,
            max_size,
            density,
            ..
        } => {
            ui.horizontal(|ui| {
                ui.label("Drops");
                changed |= ui
                    .add(egui::DragValue::new(drops).range(1..=1_000_000).speed(0.5))
                    .changed();
                ui.label("size");
                changed |= ui
                    .add(
                        egui::DragValue::new(min_size)
                            .range(0.5..=10_000.0)
                            .speed(0.1)
                            .max_decimals(1),
                    )
                    .on_hover_text("Smallest drop, in tiles across")
                    .changed();
                ui.label("to");
                changed |= ui
                    .add(
                        egui::DragValue::new(max_size)
                            .range(0.5..=10_000.0)
                            .speed(0.1)
                            .max_decimals(1),
                    )
                    .on_hover_text("Largest drop, in tiles across (small drops are more likely)")
                    .changed();
                ui.label("tiles");
            });
            ui.horizontal(|ui| {
                ui.label("Density");
                changed |= pct_drag(ui, density, 100.0)
                    .on_hover_text("Each dead cell in a drop comes alive with this probability")
                    .changed();
            });
            if *max_size < *min_size {
                *max_size = *min_size;
            }
        }
        ReseedMode::Blobs {
            count,
            min_radius,
            max_radius,
            live,
            ..
        } => {
            ui.horizontal(|ui| {
                ui.label("Blobs");
                changed |= ui
                    .add(egui::DragValue::new(count).range(1..=10_000).speed(0.1))
                    .changed();
                ui.label("radius");
                changed |= pct_drag(ui, min_radius, 200.0)
                    .on_hover_text("Smallest radius, as a share of the grid's shorter side")
                    .changed();
                ui.label("to");
                changed |= pct_drag(ui, max_radius, 200.0)
                    .on_hover_text("Largest radius, as a share of the grid's shorter side")
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Live");
                changed |= pct_drag(ui, live, 100.0)
                    .on_hover_text("Every cell inside a blob is set live with this probability, dead otherwise")
                    .changed();
            });
            if *max_radius < *min_radius {
                *max_radius = *min_radius;
            }
        }
    }
    if signed || !m.sign().is_default() {
        changed |= sign_editor(ui, m);
    }
    changed
}

/// A sign's button label and tooltip.
pub(super) fn sign_label(s: Sign) -> (&'static str, &'static str) {
    match s {
        Sign::Either => (
            "± each",
            "Each drop or blob is all +1 or all −1, at random (Random: each cell)",
        ),
        Sign::Mixed => ("Mixed", "Each new cell is +1 or −1 at random"),
        Sign::Alternate => (
            "Alternate",
            "Drops and blobs alternate +1, −1, …, and the first sign flips with every reseed (Random: the whole reseed takes one sign)",
        ),
        Sign::Pos => ("+1", "Every new cell is +1"),
        Sign::Neg => ("−1", "Every new cell is −1"),
    }
}

/// The sign of new cells, for JACA (signed) fields. Returns whether it changed.
fn sign_editor(ui: &mut egui::Ui, m: &mut ReseedMode) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("Sign").on_hover_text(
            "JACA cells are +1 or −1: the sign new live cells take. Other rules always get live (+1) cells",
        );
        let cur = m.sign();
        for s in Sign::ALL {
            let (label, tip) = sign_label(s);
            if ui.selectable_label(cur == s, label).on_hover_text(tip).clicked() && cur != s {
                *m.sign_mut() = s;
                changed = true;
            }
        }
    });
    changed
}

/// Does a reseed aimed at `target` reach a JACA CA (`signed`: their ids)?
pub(super) fn targets_signed(signed: &[u32], target: Option<u32>) -> bool {
    match target {
        None => !signed.is_empty(),
        Some(id) => signed.contains(&id),
    }
}

/// The ids of the world's JACA CAs.
pub(super) fn signed_cas(w: &jetcarp_core::World) -> Vec<u32> {
    w.cas
        .iter()
        .filter(|c| matches!(c.rule, jetcarp_core::rules::Rule::Jaca(_)))
        .map(|c| c.id)
        .collect()
}

/// The trigger's parameters and the cooldown. Returns whether anything changed.
fn when_editor(ui: &mut egui::Ui, p: &mut AutoReseed, id: usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("When");
        let settled = matches!(p.when, ReseedWhen::Settled { .. });
        for (s, label) in [(true, "Settled"), (false, "Every N gens")] {
            if ui.selectable_label(settled == s, label).clicked() && settled != s {
                p.when = switch_when(&p.when, s);
                changed = true;
            }
        }
        if ui
            .small_button("Continuous")
            .on_hover_text("Every 3 generations")
            .clicked()
        {
            p.when = ReseedWhen::Every { gens: 3 };
            changed = true;
        }
    });
    match &mut p.when {
        ReseedWhen::Settled {
            activity,
            for_gens,
            cycle,
            period,
            latest,
        } => {
            ui.horizontal(|ui| {
                ui.label("Activity below");
                changed |= pct_drag(ui, activity, 100.0)
                    .on_hover_text("Fraction of cells changing per generation")
                    .changed();
                ui.label("for");
                changed |= ui
                    .add(egui::DragValue::new(for_gens).range(1..=436).speed(0.5))
                    .changed();
                ui.label("gens");
            });
            ui.horizontal(|ui| {
                let mut on = cycle.is_some();
                if ui
                    .checkbox(&mut on, "Repeating up to")
                    .on_hover_text("Also settled when the changed count repeats with a short period (only oscillators and gliders left; catches Life ash)")
                    .changed()
                {
                    *cycle = on.then_some(12);
                    changed = true;
                }
                if let Some(c) = cycle {
                    changed |= ui.add(egui::DragValue::new(c).range(1..=64)).changed();
                }
                let mut on = period.is_some();
                if ui
                    .checkbox(&mut on, "Exact period up to")
                    .on_hover_text("Also settled when the whole state repeats with a period this short")
                    .changed()
                {
                    *period = on.then_some(6);
                    changed = true;
                }
                if let Some(c) = period {
                    changed |= ui.add(egui::DragValue::new(c).range(1..=10_000)).changed();
                }
            });
            ui.horizontal(|ui| {
                let mut on = latest.is_some();
                if ui
                    .checkbox(&mut on, "Anyway after")
                    .on_hover_text("Fire anyway once this many generations have passed since it last fired, so a screensaver never stalls (rules that never look settled, or rain from other policies keeping the world busy)")
                    .changed()
                {
                    *latest = on.then_some(2400);
                    changed = true;
                }
                if let Some(l) = latest {
                    changed |= ui
                        .add(egui::DragValue::new(l).range(16..=10_000_000).speed(4.0))
                        .changed();
                    ui.label("gens");
                }
            });
            if let Some(c) = cycle {
                *for_gens = (*for_gens).min(500 - *c);
            }
        }
        ReseedWhen::Every { gens } => {
            ui.horizontal(|ui| {
                ui.label("Every");
                changed |= ui
                    .add(egui::DragValue::new(gens).range(1..=10_000_000).speed(1.0))
                    .changed();
                ui.label("generations");
            });
        }
    }
    ui.horizontal(|ui| {
        ui.label("Cooldown");
        changed |= ui
            .add(
                egui::DragValue::new(&mut p.cooldown)
                    .range(0..=10_000_000)
                    .speed(1.0),
            )
            .on_hover_text(
                "Generations after it fires (or a manual reseed) before it can fire again",
            )
            .changed();
        ui.label("gens");
    });
    let _ = id;
    changed
}

/// Choose a target CA (None: every running CA). Returns whether it changed.
fn target_combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    cas: &[(u32, String)],
    t: &mut Option<u32>,
) -> bool {
    let text = match t {
        None => "every running CA".to_string(),
        Some(id) => cas
            .iter()
            .find(|c| c.0 == *id)
            .map(|c| c.1.clone())
            .unwrap_or_else(|| format!("CA {id} (missing)")),
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .width(140.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(t.is_none(), "every running CA")
                .clicked()
            {
                changed |= t.is_some();
                *t = None;
            }
            for (id, name) in cas {
                if ui.selectable_label(*t == Some(*id), name).clicked() {
                    changed |= *t != Some(*id);
                    *t = Some(*id);
                }
            }
        });
    changed
}

/// The activity graph: fraction changed per generation (log scale), the settled thresholds,
/// and where reseeds fired.
fn activity_graph(
    ui: &mut egui::Ui,
    hist: &VecDeque<jetcarp_core::world::Activity>,
    generation: u64,
    thresholds: &[f64],
    marks: &VecDeque<(u64, Mark)>,
) {
    let w = ui.available_width().max(120.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 110.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Color32::from_gray(18));
    let plot = rect.shrink2(egui::vec2(4.0, 6.0));
    let plot = egui::Rect::from_min_max(plot.min + egui::vec2(38.0, 0.0), plot.max);
    let x0 = generation.saturating_sub(GRAPH_GENS - 1);
    let gx = |g: u64| {
        plot.left() + plot.width() * (g.saturating_sub(x0)) as f32 / (GRAPH_GENS - 1) as f32
    };
    let gy = |f: f64| plot.bottom() - plot.height() * graph_y(f);
    // Gridlines at each decade.
    let font = egui::FontId::monospace(9.0);
    for (f, label) in [
        (1e-4, "0.01%"),
        (1e-3, "0.1%"),
        (1e-2, "1%"),
        (1e-1, "10%"),
        (1.0, "100%"),
    ] {
        let y = gy(f);
        p.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0, Color32::from_gray(40)),
        );
        p.text(
            egui::pos2(plot.left() - 3.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            font.clone(),
            Color32::from_gray(120),
        );
    }
    for &t in thresholds {
        let y = gy(t);
        p.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0, Color32::from_rgb(230, 170, 60)),
        );
    }
    for (g, m) in marks {
        if *g < x0 {
            continue;
        }
        let x = gx(*g);
        let c = match m {
            Mark::Manual => Color32::from_rgb(120, 180, 255),
            Mark::Policy(_) => Color32::from_rgb(255, 90, 140),
        };
        p.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.5, c),
        );
    }
    // The activity line, broken at gaps (generations not counted).
    let mut run: Vec<egui::Pos2> = vec![];
    let mut last: Option<u64> = None;
    let stroke = egui::Stroke::new(1.5, Color32::from_rgb(110, 220, 160));
    for a in hist.iter().filter(|a| a.generation >= x0) {
        if last.is_some_and(|l| a.generation > l + 8) && !run.is_empty() {
            p.add(egui::Shape::line(std::mem::take(&mut run), stroke));
        }
        run.push(egui::pos2(gx(a.generation), gy(a.fraction as f64)));
        last = Some(a.generation);
    }
    if run.len() == 1 {
        p.circle_filled(run[0], 1.5, stroke.color);
    } else if !run.is_empty() {
        p.add(egui::Shape::line(run, stroke));
    }
    if hist.is_empty() {
        p.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "No activity counted yet: run the world",
            egui::FontId::proportional(12.0),
            Color32::from_gray(140),
        );
    }
    if let Some(pos) = resp.hover_pos()
        && plot.contains(pos)
    {
        let g =
            x0 + (((pos.x - plot.left()) / plot.width()) * (GRAPH_GENS - 1) as f32).round() as u64;
        if let Some(a) = hist.iter().min_by_key(|a| a.generation.abs_diff(g)) {
            resp.on_hover_text(format!(
                "gen {}: {} cells changed ({})",
                crate::sweep_form::thousands(a.generation),
                crate::sweep_form::thousands(a.changed),
                pct(a.fraction as f64)
            ));
        }
    }
}

impl JetcarpApp {
    /// Every frame: apply debounced edits, follow outside changes, note firings, and switch
    /// change tracking on while the pane is visible.
    pub(super) fn reseed_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.screen == Screen::Sim
            && self.building.is_none()
            && let Some(id) = self.reseed.env.take()
        {
            // Screenshots: install the preset and show the pane.
            self.install_preset(&id);
            self.shell.layout.show_pane("reseed");
        }
        // Outside changes replace the drafts unless there are unapplied edits.
        if self.world.reseed_policies() != self.reseed.synced.as_slice() {
            self.reseed.synced = self.world.reseed_policies().to_vec();
            if self.reseed.dirty_at.is_none() {
                self.reseed.drafts = self.reseed.synced.clone();
            }
        }
        if let Some(t) = self.reseed.dirty_at {
            let down = ctx.input(|i| i.pointer.any_down());
            if now - t >= DEBOUNCE && !down {
                self.reseed.dirty_at = None;
                self.apply_drafts();
            }
        }
        // Firings.
        if self.world.generation < self.reseed.last_gen {
            self.reseed.marks.clear();
        }
        self.reseed.last_gen = self.world.generation;
        let status = self.world.reseed_status();
        if status.len() != self.reseed.fired.len() {
            self.reseed.fired = status.iter().map(|s| s.fired).collect();
        }
        for (k, st) in status.iter().enumerate() {
            if st.fired > self.reseed.fired[k] {
                let g = st.last_fired.unwrap_or(self.world.generation);
                self.reseed.marks.push_back((g, Mark::Policy(k)));
                self.reseed.fired[k] = st.fired;
            }
        }
        let floor = self.world.generation.saturating_sub(GRAPH_GENS);
        while self.reseed.marks.front().is_some_and(|m| m.0 < floor) {
            self.reseed.marks.pop_front();
        }
        // Count changes while the pane shows.
        let want = self.screen == Screen::Sim && now - self.reseed.seen_at < 0.5;
        if want && !self.world.track_changes() {
            self.world.set_track_changes(true);
            self.reseed.tracking = true;
        } else if !want && self.reseed.tracking {
            self.world.set_track_changes(false);
            self.reseed.tracking = false;
        }
    }

    fn apply_drafts(&mut self) {
        match validate_all(&self.reseed.drafts) {
            Ok(()) => {
                self.reseed.error = None;
                let policies = self.reseed.drafts.clone();
                if self.exec(Command::SetReseed { policies }).is_some() {
                    self.reseed.synced = self.world.reseed_policies().to_vec();
                }
            }
            Err(e) => self.reseed.error = Some(e),
        }
    }

    /// Replace the policies with a preset (built in or saved). Returns false if unknown.
    pub(super) fn install_preset(&mut self, id: &str) -> bool {
        let cells = self.world.topo.len();
        let policies = find_preset(id, &self.reseed.settings.presets, cells);
        let Some(policies) = policies else {
            self.message = Some((format!("No reseed preset named {id:?}"), true));
            return false;
        };
        self.reseed.drafts = policies;
        self.reseed.dirty_at = None;
        self.apply_drafts();
        true
    }

    /// Reseed now with mode `k` (0 random, 1 rain, 2 blobs) of the pane's settings.
    pub(super) fn reseed_now(&mut self, k: usize) {
        let mode = self.reseed.now[k.min(2)].clone();
        let ca = self.reseed.now_target;
        if self.exec(Command::Reseed { ca, mode }).is_some() {
            let g = self.world.generation;
            self.reseed.marks.push_back((g, Mark::Manual));
        }
    }

    /// Any automatic policy is on.
    pub(super) fn auto_reseed_on(&self) -> bool {
        self.world.reseed_policies().iter().any(|p| p.enabled)
    }

    /// *Seed → Auto reseed*: switch every policy off or on; with none, install the preset.
    pub(super) fn toggle_auto_reseed(&mut self) {
        let mut policies = self.world.reseed_policies().to_vec();
        if policies.is_empty() {
            let id = self.reseed.settings.auto_preset.clone();
            if !self.install_preset(&id) {
                self.install_preset("screensaver");
            }
            return;
        }
        let on = !self.auto_reseed_on();
        for p in &mut policies {
            p.enabled = on;
        }
        self.reseed.drafts = policies;
        self.reseed.dirty_at = None;
        self.apply_drafts();
    }

    /// The *Seed → Reseed now* submenu.
    pub(super) fn reseed_now_menu(&mut self, ui: &mut egui::Ui) {
        let ok = self.screen == Screen::Sim && !self.world.cas.is_empty();
        ui.add_enabled_ui(ok, |ui| {
            ui.menu_button("Reseed now", |ui| {
                for (k, label) in ["Random", "Rain", "Blobs"].iter().enumerate() {
                    if ui
                        .button(*label)
                        .on_hover_text(mode_summary(&self.reseed.now[k]))
                        .clicked()
                    {
                        self.reseed_now(k);
                        ui.close();
                    }
                }
            });
        });
    }

    /// The Reseed pane.
    pub(super) fn reseed_pane(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.reseed.seen_at = now;
        let generation = self.world.generation;
        // Activity and population.
        let last = self.world.last_activity();
        let pop: u64 = self
            .stats
            .as_ref()
            .map(|s| s.populations.iter().sum())
            .unwrap_or(0);
        let cells = self.world.topo.len() as u64;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Activity {}",
                last.map(pct).unwrap_or_else(|| "–".into())
            ))
            .on_hover_text(
                "Cells that changed in the last generation, as a share of the running fields",
            );
            ui.separator();
            ui.label(format!(
                "Population {} ({})",
                crate::sweep_form::thousands(pop),
                pct(pop as f64 / cells.max(1) as f64)
            ));
            ui.separator();
            ui.label(format!("Gen {}", crate::sweep_form::thousands(generation)));
        });
        let thresholds: Vec<f64> = self
            .world
            .reseed_policies()
            .iter()
            .filter(|p| p.enabled)
            .filter_map(|p| match p.when {
                ReseedWhen::Settled { activity, .. } => Some(activity),
                _ => None,
            })
            .collect();
        activity_graph(
            ui,
            self.world.activity_history(),
            generation,
            &thresholds,
            &self.reseed.marks,
        );
        ui.label(
            RichText::new("Changed cells per generation (log scale). Orange: settled thresholds; pink: automatic reseeds; blue: by hand.")
                .small()
                .weak(),
        );
        let cas: Vec<(u32, String)> = self
            .world
            .cas
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();

        // Reseed now.
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Reseed now");
            ui.add_enabled_ui(!cas.is_empty(), |ui| {
                for (k, label) in ["Random", "Rain", "Blobs"].iter().enumerate() {
                    if ui
                        .button(*label)
                        .on_hover_text(mode_summary(&self.reseed.now[k]))
                        .clicked()
                    {
                        self.reseed_now(k);
                    }
                }
            });
        });
        egui::CollapsingHeader::new("Reseed now settings")
            .id_salt("reseed-now-settings")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Target");
                    target_combo(ui, "reseed-now-target", &cas, &mut self.reseed.now_target);
                });
                let signed = targets_signed(&signed_cas(&self.world), self.reseed.now_target);
                for (k, label) in ["Random", "Rain", "Blobs"].iter().enumerate() {
                    ui.label(RichText::new(*label).small().weak());
                    mode_editor(ui, &mut self.reseed.now[k], signed);
                }
                if ui.small_button("Defaults").clicked() {
                    self.reseed.now = default_now();
                }
            });

        // Presets.
        ui.separator();
        ui.strong("Presets");
        if let Some(id) = preset_buttons(ui, &self.reseed.settings.presets) {
            self.install_preset(&id);
        }
        auto_preset_combo(ui, &mut self.reseed.settings);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.reseed.preset_name)
                    .hint_text("preset name")
                    .desired_width(120.0),
            );
            let name = self.reseed.preset_name.trim().to_string();
            if ui
                .add_enabled(
                    !name.is_empty() && !self.reseed.drafts.is_empty(),
                    egui::Button::new("Save as preset"),
                )
                .on_hover_text("Keep these policies as a preset (saved in the app settings)")
                .clicked()
            {
                let presets = &mut self.reseed.settings.presets;
                presets.retain(|p| p.name != name);
                presets.push(NamedPolicies {
                    name,
                    policies: self.reseed.drafts.clone(),
                });
                self.reseed.preset_name.clear();
            }
            if !self.reseed.settings.presets.is_empty() {
                ui.menu_button("Delete…", |ui| {
                    let mut del = None;
                    for p in &self.reseed.settings.presets {
                        if ui.button(&p.name).clicked() {
                            del = Some(p.name.clone());
                        }
                    }
                    if let Some(d) = del {
                        let s = &mut self.reseed.settings;
                        s.presets.retain(|p| p.name != d);
                        if s.auto_preset == d && preset_policies(&d, 1).is_none() {
                            s.auto_preset = ReseedSettings::default().auto_preset;
                        }
                        ui.close();
                    }
                });
            }
        });

        // Policies.
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Automatic policies");
            if self.reseed.dirty_at.is_some() {
                ui.label(RichText::new("(applying…)").small().weak());
            }
        });
        if let Some(e) = &self.reseed.error {
            ui.colored_label(Color32::from_rgb(255, 110, 110), e);
        }
        if self.reseed.drafts.is_empty() {
            ui.label(RichText::new("None. Add one, or pick a preset.").weak());
        }
        let status = self.world.reseed_status().to_vec();
        let applied = self.reseed.dirty_at.is_none() && self.reseed.error.is_none();
        let live = applied.then_some((status.as_slice(), generation));
        let signed = signed_cas(&self.world);
        let changed = policies_editor(
            ui,
            "reseed-pane",
            &mut self.reseed.drafts,
            &cas,
            &signed,
            live,
        );
        if changed {
            self.reseed.mark_dirty(now);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(450));
        }
        if self.reseed.dirty_at.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// The policy list editor (the Reseed pane, screensaver profiles): one frame per policy with
/// its mode, trigger, cooldown, and target, then *Add policy* and *Remove all*. `live`: the
/// world's status per policy and the generation, shown under each policy when given. Returns
/// whether anything changed. `signed`: the JACA CAs' ids (policies reaching one get a sign).
pub(super) fn policies_editor(
    ui: &mut egui::Ui,
    salt: &str,
    drafts: &mut Vec<AutoReseed>,
    cas: &[(u32, String)],
    signed: &[u32],
    live: Option<(&[PolicyStatus], u64)>,
) -> bool {
    ui.push_id(salt, |ui| {
        let mut changed = false;
        let mut remove = None;
        for (k, p) in drafts.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut p.enabled, format!("Policy {}", k + 1))
                        .changed();
                    for kind in ["random", "rain", "blobs"] {
                        let label = match kind {
                            "random" => "Random",
                            "rain" => "Rain",
                            _ => "Blobs",
                        };
                        if ui.selectable_label(p.mode.name() == kind, label).clicked()
                            && p.mode.name() != kind
                        {
                            p.mode = switch_mode(&p.mode, kind);
                            changed = true;
                        }
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(k);
                    }
                });
                changed |= mode_editor(ui, &mut p.mode, targets_signed(signed, p.ca));
                changed |= when_editor(ui, p, k);
                ui.horizontal(|ui| {
                    ui.label("Reseed");
                    changed |= target_combo(ui, ("reseed-target", salt, k), cas, &mut p.ca);
                });
                if let Some((status, generation)) = live
                    && let Some(st) = status.get(k)
                {
                    ui.label(RichText::new(status_text(p, st, generation)).small());
                    if let Some(e) = &st.error {
                        ui.colored_label(Color32::from_rgb(255, 110, 110), e);
                    }
                }
            });
        }
        if let Some(k) = remove {
            drafts.remove(k);
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add policy").clicked() {
                drafts.push(AutoReseed::new(blobs_mode(), ReseedWhen::settled()).cooldown(150));
                changed = true;
            }
            if !drafts.is_empty() && ui.button("Remove all").clicked() {
                drafts.clear();
                changed = true;
            }
        });
        changed
    })
    .inner
}

/// The built-in presets as buttons, then `saved` presets (starred). Returns the one clicked:
/// a built-in's id or a saved preset's name.
pub(super) fn preset_buttons(ui: &mut egui::Ui, saved: &[NamedPolicies]) -> Option<String> {
    let mut install = None;
    ui.horizontal_wrapped(|ui| {
        for p in PRESETS {
            if ui.button(p.name).on_hover_text(p.hint).clicked() {
                install = Some(p.id.to_string());
            }
        }
        for p in saved {
            if ui
                .button(format!("★ {}", p.name))
                .on_hover_text("Saved preset")
                .clicked()
            {
                install = Some(p.name.clone());
            }
        }
    });
    install
}

/// The preset *Seed → Auto reseed* installs when a world has no policies: a built-in one or one
/// of the saved ones (starred), kept in the app settings.
fn auto_preset_combo(ui: &mut egui::Ui, s: &mut ReseedSettings) {
    let label = auto_preset_label(&s.auto_preset, &s.presets);
    ui.horizontal(|ui| {
        ui.label("Seed → Auto reseed installs")
            .on_hover_text("The preset that Seed → Auto reseed puts in when the world has no policies (with policies, it switches them all off or on)");
        egui::ComboBox::from_id_salt("reseed-auto-preset")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for p in PRESETS {
                    ui.selectable_value(&mut s.auto_preset, p.id.to_string(), p.name)
                        .on_hover_text(p.hint);
                }
                for p in &s.presets {
                    ui.selectable_value(&mut s.auto_preset, p.name.clone(), format!("★ {}", p.name));
                }
            });
    });
}

/// How the combo names the auto-reseed preset `id`: a built-in's name, a saved preset starred,
/// or the screensaver preset's name for one that no longer exists (it falls back to that).
fn auto_preset_label(id: &str, saved: &[NamedPolicies]) -> String {
    if let Some(p) = PRESETS.iter().find(|p| p.id == id) {
        return p.name.to_string();
    }
    if saved.iter().any(|p| p.name == id) {
        return format!("★ {id}");
    }
    PRESETS
        .iter()
        .find(|p| p.id == ReseedSettings::default().auto_preset)
        .map_or_else(|| id.to_string(), |p| p.name.to_string())
}

/// A preset's policies by id (built in) or name (saved), for a grid of `cells` cells.
pub(super) fn find_preset(
    id: &str,
    saved: &[NamedPolicies],
    cells: usize,
) -> Option<Vec<AutoReseed>> {
    preset_policies(id, cells).or_else(|| {
        saved
            .iter()
            .find(|p| p.name == id)
            .map(|p| p.policies.clone())
    })
}

/// A mode in a few words, for tooltips.
pub(super) fn mode_summary(m: &ReseedMode) -> String {
    let mut s = mode_summary_unsigned(m);
    if !m.sign().is_default() {
        s.push_str(&format!(" (JACA sign: {})", sign_label(m.sign()).0));
    }
    s
}

fn mode_summary_unsigned(m: &ReseedMode) -> String {
    match *m {
        ReseedMode::Random { flip, .. } => format!("Flip {} of all cells", pct(flip)),
        ReseedMode::Rain {
            drops,
            min_size,
            max_size,
            density,
            ..
        } => format!(
            "{drops} drops, {min_size}–{max_size} tiles across, {} live",
            pct(density)
        ),
        ReseedMode::Blobs {
            count,
            min_radius,
            max_radius,
            live,
            ..
        } => format!(
            "{count} blob(s), radius {}–{} of the shorter side, {} live",
            pct(min_radius),
            pct(max_radius),
            pct(live)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_at_every_size() {
        for cells in [100, 9_600, 28_000, 400_000, 3_686_400, 64_000_000] {
            for p in PRESETS {
                let v = preset_policies(p.id, cells).unwrap();
                assert!(!v.is_empty());
                validate_all(&v).unwrap_or_else(|e| panic!("{} at {cells}: {e}", p.id));
            }
        }
        assert!(preset_policies("nope", 100).is_none());
        // Drops scale with the grid.
        let d = |cells| match &preset_policies("storm", cells).unwrap()[0].mode {
            ReseedMode::Rain { drops, .. } => *drops,
            _ => unreachable!(),
        };
        assert_eq!(d(28_000), 4);
        assert_eq!(d(280_000), 40);
        assert_eq!(d(10), 1);
    }

    #[test]
    fn switching_kinds_keeps_values_or_takes_defaults() {
        let m = ReseedMode::Rain {
            drops: 7,
            min_size: 1.0,
            max_size: 3.0,
            density: 0.2,
            sign: Default::default(),
        };
        assert_eq!(switch_mode(&m, "rain"), m);
        assert_eq!(switch_mode(&m, "blobs"), ReseedMode::blobs());
        assert_eq!(switch_mode(&m, "random"), ReseedMode::random());
        let w = ReseedWhen::Every { gens: 9 };
        assert_eq!(switch_when(&w, false), w);
        assert_eq!(switch_when(&w, true), ReseedWhen::settled());
        assert_eq!(
            switch_when(&ReseedWhen::settled(), false),
            ReseedWhen::Every { gens: 100 }
        );
        for kind in ["random", "rain", "blobs"] {
            for settled in [true, false] {
                let p = AutoReseed::new(switch_mode(&m, kind), switch_when(&w, settled));
                p.validate().unwrap();
            }
        }
    }

    #[test]
    fn validation_names_the_policy() {
        let good = AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 10 });
        let bad = AutoReseed::new(
            ReseedMode::Random {
                flip: 1.5,
                sign: Default::default(),
            },
            ReseedWhen::settled(),
        );
        assert!(validate_all(std::slice::from_ref(&good)).is_ok());
        let e = validate_all(&[good.clone(), bad]).unwrap_err();
        assert!(e.starts_with("policy 2:"), "{e}");
        let zero = AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 0 });
        assert!(validate_all(&[zero]).is_err());
        // What the editor writes as percentages comes out as fractions.
        let mut flip = 0.0;
        let v: f64 = 12.5;
        flip += v / 100.0;
        let p = AutoReseed::new(
            ReseedMode::Random {
                flip,
                sign: Default::default(),
            },
            ReseedWhen::settled(),
        );
        assert!(p.validate().is_ok());
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["mode"]["flip"], 0.125);
    }

    #[test]
    fn graph_scale_is_logarithmic_and_clamped() {
        assert_eq!(graph_y(0.0), 0.0);
        assert_eq!(graph_y(-1.0), 0.0);
        assert_eq!(graph_y(f64::NAN), 0.0);
        assert_eq!(graph_y(1e-6), 0.0);
        assert!((graph_y(1e-4) - 0.0).abs() < 1e-6);
        assert!((graph_y(1e-3) - 0.25).abs() < 1e-6);
        assert!((graph_y(1e-2) - 0.5).abs() < 1e-6);
        assert!((graph_y(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(graph_y(3.0), 1.0);
        // 0.1% and 50% are far apart, both well inside the graph.
        let (lo, hi) = (graph_y(0.001), graph_y(0.5));
        assert!(lo > 0.2 && hi < 0.95 && hi - lo > 0.6);
        let mut prev = -1.0;
        for k in 1..=100 {
            let y = graph_y(k as f64 / 100.0);
            assert!(y > prev);
            prev = y;
        }
    }

    #[test]
    fn status_lines() {
        let p = AutoReseed::new(ReseedMode::blobs(), ReseedWhen::settled()).cooldown(100);
        let st = PolicyStatus {
            since: 1000,
            streak: 40,
            fired: 5,
            last_fired: Some(12_345),
            error: None,
        };
        assert_eq!(
            status_text(&p, &st, 1050),
            "cooling down, 50 gens left · fired 5 times, last at gen 12,345"
        );
        assert_eq!(
            status_text(&p, &st, 1200),
            "quiet for 40/60 gens · fired 5 times, last at gen 12,345"
        );
        let e = AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 500 });
        let st = PolicyStatus {
            since: 1000,
            ..Default::default()
        };
        assert_eq!(
            status_text(&e, &st, 1100),
            "next in 400 gens · not fired yet"
        );
        let mut off = e.clone();
        off.enabled = false;
        assert!(status_text(&off, &st, 1100).starts_with("off"));
        assert_eq!(pct(0.0025), "0.25%");
        assert_eq!(pct(0.5), "50%");
        assert_eq!(pct(0.00001), "<0.01%");
    }

    #[test]
    fn the_auto_reseed_preset_is_named_in_its_combo() {
        let saved = [NamedPolicies {
            name: "mine".into(),
            policies: preset_policies("storm", 1000).unwrap(),
        }];
        assert_eq!(auto_preset_label("screensaver", &saved), "Screensaver");
        assert_eq!(auto_preset_label("storm", &saved), "Storm");
        assert_eq!(auto_preset_label("mine", &saved), "★ mine");
        // A saved preset deleted since: Seed → Auto reseed falls back to the screensaver preset.
        assert_eq!(auto_preset_label("gone", &saved), "Screensaver");
        assert!(find_preset("gone", &saved, 1000).is_none());
        assert!(find_preset("mine", &saved, 1000).is_some());
    }

    #[test]
    fn settings_round_trip() {
        let mut ui = ReseedUi::default();
        ui.now[1] = ReseedMode::Rain {
            drops: 3,
            min_size: 1.0,
            max_size: 2.0,
            density: 0.9,
            sign: Default::default(),
        };
        ui.settings.presets.push(NamedPolicies {
            name: "mine".into(),
            policies: preset_policies("storm", 1000).unwrap(),
        });
        let s = ui.snapshot();
        let json = serde_json::to_string(&s).unwrap();
        let back: ReseedSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        let mut ui2 = ReseedUi::default();
        ui2.restore(back);
        assert_eq!(ui2.now[1], ui.now[1]);
        assert_eq!(ui2.settings.presets.len(), 1);
        // Missing fields take defaults; a bad mode is ignored.
        let s: ReseedSettings =
            serde_json::from_str(r#"{"now": [{"kind": "random", "flip": 7}]}"#).unwrap();
        let mut ui3 = ReseedUi::default();
        ui3.restore(s);
        assert_eq!(ui3.now[0], default_now()[0]);
        assert_eq!(ui3.settings.auto_preset, "screensaver");
    }

    /// Runs every preset on Life, a hex rule, and two Generations rules and prints activity and
    /// population, and writes snapshots to `$JETCARP_PRESET_OUT` if set. For tuning the presets:
    /// `cargo test -p jetcarp-app --release preset_survey -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn preset_survey() {
        let out = std::env::var("JETCARP_PRESET_OUT")
            .ok()
            .map(std::path::PathBuf::from);
        let worlds: &[(&str, &str, &str, f64)] = &[
            ("life", "square", "B3/S23", 0.35),
            ("hex", "hex", "B2/S34H", 0.35),
            ("brain", "square", "B2/S/C3", 0.2),
            ("starwars", "square", "B2/S345/C4", 0.35),
        ];
        let mut ids: Vec<&str> = vec!["none"];
        ids.extend(PRESETS.iter().map(|p| p.id));
        let only = std::env::var("JETCARP_PRESET_ONLY").ok();
        for (name, family, rule, density) in worlds {
            for id in &ids {
                if only
                    .as_deref()
                    .is_some_and(|o| !o.split(',').any(|x| x == *id))
                {
                    continue;
                }
                let mut w =
                    World::new(TilingSpec::rect(family, 200, 140, Boundary::Wrap), 7).unwrap();
                let mut ca = CaSpec::new(rule);
                ca.seed = Some(SeedSpec::Random { density: *density });
                w.apply(Command::AddCa { ca }).unwrap();
                w.apply(Command::Reset).unwrap();
                let cells = w.topo.len();
                if let Some(p) = preset_policies(id, cells) {
                    w.apply(Command::SetReseed { policies: p }).unwrap();
                }
                w.set_track_changes(true);
                let mut line = format!("{name:9} {id:12}");
                for chunk in 0..6 {
                    let mut act = 0.0;
                    let mut n = 0;
                    for _ in 0..1000 {
                        w.apply(Command::Step { gens: 1 }).unwrap();
                        act += w.last_activity().unwrap_or(0.0);
                        n += 1;
                    }
                    let pop = w.fields[0].population() as f64 / cells as f64;
                    line.push_str(&format!(
                        " | act {:>6} pop {:>5}",
                        pct(act / n as f64),
                        pct(pop)
                    ));
                    if let Some(o) = &out
                        && chunk == 5
                    {
                        std::fs::create_dir_all(o).unwrap();
                        let img = render::render(
                            &w,
                            &RenderOptions {
                                max_side: 600,
                                ..Default::default()
                            },
                        );
                        render::write_png(&o.join(format!("{name}-{id}.png")), &img, &[]).unwrap();
                    }
                }
                let fired: u64 = w.reseed_status().iter().map(|s| s.fired).sum();
                println!("{line} | fired {fired}");
            }
        }
    }
}
