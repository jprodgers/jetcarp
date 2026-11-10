//! The approval queue (Phase 4 deliverable 8): Quick Start candidates (catalog rows with status
//! `candidate`, from stage 1 settings or anything else) with a large live preview, to approve,
//! reject, or rate one after another from the keyboard.
//!
//! Keys (when no text field has focus): `1`-`5` pick stars, `A` approves (the stars, or 4, count
//! as the user's rating), `X` rejects (rated only if stars were picked), `Enter` saves the stars
//! alone, `J` / Down and `K` / Up move, `O` opens the entry in the main view. The catalog writes
//! (`jetcarp_core::qsmenu::approve`, `reject`, `rate_entry`) run on a background thread, in order.
//!
//! **Score model.** The window also shows stage 1's active score model (version, source,
//! thresholds, ratings used, fit error) and the number of rated settings, and can set new
//! `auto` / `candidate` thresholds or refit the weights from the ratings (as `jetcarp stage1
//! thresholds` and `jetcarp stage1 refit`; each stores a new model version and rescores every
//! setting, so the queue and the Quick Start reload).

use crate::start::Preview;
use jetcarp_core::Scene;
use jetcarp_core::library::{Library, QuickStartRow};
use jetcarp_core::qsmenu;
use jetcarp_core::stage1::score::{MIN_RATED, ScoreModel};
use eframe::egui::{self, Color32, Key, RichText, Sense};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "queue";

enum Job {
    Approve(Box<QuickStartRow>, Option<u8>),
    Reject(Box<QuickStartRow>, Option<u8>),
    Rate(Box<QuickStartRow>, u8),
    /// Refit the score from the ratings (ridge strength).
    Refit(f64),
    /// New `auto` and `candidate` thresholds.
    Thresholds(f64, f64),
}

/// The active score model and how many settings are rated.
type ModelInfo = (ScoreModel, usize);

pub enum QueueAction {
    Open(Box<Scene>),
    /// Entries were approved or rejected: the menu should reload.
    Changed,
}

pub struct QueueWindow {
    pub open: bool,
    path: PathBuf,
    rows: Vec<QuickStartRow>,
    loading: Option<Receiver<Result<Vec<QuickStartRow>, String>>>,
    sel: usize,
    stars: Option<u8>,
    preview: Preview,
    preview_id: String,
    /// Palette of the preview (`None`: the entry's own; saved in the settings).
    pub palette: Option<String>,
    tx: Option<Sender<Job>>,
    done_rx: Option<Receiver<Result<String, String>>>,
    pending: usize,
    log: Vec<(String, bool)>,
    approved: usize,
    rejected: usize,
    changed: bool,
    model: Option<ModelInfo>,
    model_rx: Option<Receiver<Result<ModelInfo, String>>>,
    auto: f64,
    candidate: f64,
    lambda: f64,
}

impl Default for QueueWindow {
    fn default() -> Self {
        QueueWindow {
            open: false,
            path: Library::default_path(),
            rows: vec![],
            loading: None,
            sel: 0,
            stars: None,
            preview: Preview::default(),
            preview_id: String::new(),
            palette: None,
            tx: None,
            done_rx: None,
            pending: 0,
            log: vec![],
            approved: 0,
            rejected: 0,
            changed: false,
            model: None,
            model_rx: None,
            auto: 0.0,
            candidate: 0.0,
            lambda: 2.0,
        }
    }
}

impl QueueWindow {
    /// Read the candidates again (background thread).
    pub fn reload(&mut self) {
        let (tx, rx) = channel();
        let path = self.path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(qsmenu::candidates(&Library::open(path)));
        });
        self.loading = Some(rx);
        let (tx, rx) = channel();
        let path = self.path.clone();
        std::thread::spawn(move || {
            let lib = Library::open(path);
            let r = lib
                .score_model()
                .and_then(|m| lib.rating_samples().map(|(_, n)| (m, n)));
            let _ = tx.send(r);
        });
        self.model_rx = Some(rx);
    }

    pub fn open_window(&mut self) {
        self.open = true;
        self.reload();
    }

    fn worker(&mut self) -> &Sender<Job> {
        if self.tx.is_none() {
            let (tx, rx) = channel::<Job>();
            let (dtx, drx) = channel();
            let path = self.path.clone();
            std::thread::Builder::new()
                .name("jetcarp-queue".into())
                .spawn(move || {
                    let lib = Library::open(path);
                    for j in rx {
                        let r = match j {
                            Job::Approve(row, s) => qsmenu::approve(&lib, &row, s)
                                .map(|_| format!("Approved {}", row.title)),
                            Job::Reject(row, s) => qsmenu::reject(&lib, &row, s)
                                .map(|_| format!("Rejected {}", row.title)),
                            Job::Rate(row, s) => qsmenu::rate_entry(&lib, &row, s)
                                .map(|_| format!("Rated {} {s} of 5", row.title)),
                            Job::Refit(lambda) => lib.refit_score(lambda).map(|(m, r)| {
                                format!(
                                    "Score model {} refit from {} ratings of {} settings: rmse {} -> {}; rescored {} settings {:?}",
                                    m.id,
                                    m.ratings,
                                    m.rated_settings,
                                    m.rmse_before.map(|x| format!("{x:.3}")).unwrap_or("-".into()),
                                    m.rmse_after.map(|x| format!("{x:.3}")).unwrap_or("-".into()),
                                    r.settings,
                                    r.statuses
                                )
                            }),
                            Job::Thresholds(a, c) => lib.set_thresholds(a, c).map(|(m, r)| {
                                format!(
                                    "Score model {}: auto {a}, candidate {c}; rescored {} settings {:?}",
                                    m.id, r.settings, r.statuses
                                )
                            }),
                        };
                        if dtx.send(r).is_err() {
                            return;
                        }
                    }
                })
                .ok();
            self.tx = Some(tx);
            self.done_rx = Some(drx);
        }
        self.tx.as_ref().unwrap()
    }

    fn act(&mut self, approve: Option<bool>) {
        let Some(row) = self.rows.get(self.sel).cloned() else {
            return;
        };
        let stars = self.stars.take();
        let job = match approve {
            Some(true) => {
                self.approved += 1;
                Job::Approve(Box::new(row), stars)
            }
            Some(false) => {
                self.rejected += 1;
                Job::Reject(Box::new(row), stars)
            }
            None => match stars {
                Some(s) => Job::Rate(Box::new(row), s),
                None => return,
            },
        };
        let decided = !matches!(job, Job::Rate(..));
        let _ = self.worker().send(job);
        self.pending += 1;
        if decided {
            self.rows.remove(self.sel);
            if self.sel >= self.rows.len() {
                self.sel = self.rows.len().saturating_sub(1);
            }
            self.changed = true;
        }
    }

    fn keys(&mut self, ctx: &egui::Context) -> Option<QueueAction> {
        if ctx.egui_wants_keyboard_input() {
            return None;
        }
        let mut out: Option<Option<bool>> = None;
        ctx.input_mut(|i| {
            for (k, n) in [
                (Key::Num1, 1),
                (Key::Num2, 2),
                (Key::Num3, 3),
                (Key::Num4, 4),
                (Key::Num5, 5),
            ] {
                if i.consume_key(egui::Modifiers::NONE, k) {
                    self.stars = Some(n);
                }
            }
            if i.consume_key(egui::Modifiers::NONE, Key::A) {
                out = Some(Some(true));
            } else if i.consume_key(egui::Modifiers::NONE, Key::X) {
                out = Some(Some(false));
            } else if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                out = Some(None);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::J)
                || i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
            {
                self.move_sel(1);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::K)
                || i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
            {
                self.move_sel(-1);
            }
        });
        if let Some(a) = out {
            self.act(a);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::O))
            && let Some(r) = self.rows.get(self.sel)
        {
            return Some(QueueAction::Open(Box::new(r.scene.clone())));
        }
        None
    }

    /// Stage 1's score model: thresholds and refit.
    fn model_panel(&mut self, ui: &mut egui::Ui) {
        let Some((m, rated)) = self.model.clone() else {
            return;
        };
        egui::CollapsingHeader::new(format!(
            "Score model {} ({}): auto ≥ {:.1}, candidate ≥ {:.1} · {rated} rated settings",
            m.id, m.source, m.auto, m.candidate
        ))
        .id_salt("queue-score-model")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "Settings scoring at least `auto` go to the Quick Start, at least `candidate` \
                     to this queue. Each change is a new model version and rescores every \
                     setting. {}",
                    match (m.rmse_before, m.rmse_after) {
                        (Some(a), Some(b)) =>
                            format!("Last fit: {} ratings, rmse {a:.3} -> {b:.3}.", m.ratings),
                        _ => String::new(),
                    }
                ))
                .small()
                .weak(),
            );
            let busy = self.pending > 0;
            ui.horizontal(|ui| {
                ui.label("auto");
                ui.add(
                    egui::DragValue::new(&mut self.auto)
                        .speed(0.5)
                        .range(0.0..=100.0),
                );
                ui.label("candidate");
                ui.add(
                    egui::DragValue::new(&mut self.candidate)
                        .speed(0.5)
                        .range(0.0..=100.0),
                );
                let ok = self.candidate <= self.auto
                    && (self.auto != m.auto || self.candidate != m.candidate);
                if ui
                    .add_enabled(ok && !busy, egui::Button::new("Apply thresholds"))
                    .on_disabled_hover_text("Change a threshold (candidate at most auto)")
                    .clicked()
                {
                    let job = Job::Thresholds(self.auto, self.candidate);
                    let _ = self.worker().send(job);
                    self.pending += 1;
                    self.changed = true;
                    self.model = None;
                }
            });
            ui.horizontal(|ui| {
                ui.label("ridge λ");
                ui.add(
                    egui::DragValue::new(&mut self.lambda)
                        .speed(0.1)
                        .range(0.0..=100.0),
                )
                .on_hover_text("Pull towards the current weights");
                if ui
                    .add_enabled(
                        rated >= MIN_RATED && !busy,
                        egui::Button::new("Refit from ratings"),
                    )
                    .on_hover_text("Fit the weights to the user's and the model's ratings")
                    .on_disabled_hover_text(format!(
                        "A refit needs at least {MIN_RATED} rated settings ({rated} so far)"
                    ))
                    .clicked()
                {
                    let job = Job::Refit(self.lambda);
                    let _ = self.worker().send(job);
                    self.pending += 1;
                    self.changed = true;
                    self.model = None;
                }
            });
        });
    }

    fn move_sel(&mut self, d: i64) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as i64;
        self.sel = (self.sel as i64 + d).clamp(0, n - 1) as usize;
        self.stars = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Vec<QueueAction> {
        let mut out = vec![];
        if let Some(rx) = &self.done_rx {
            while let Ok(r) = rx.try_recv() {
                self.pending = self.pending.saturating_sub(1);
                match r {
                    Ok(m) => self.log.push((m, false)),
                    Err(e) => self.log.push((e, true)),
                }
                if self.log.len() > 6 {
                    self.log.remove(0);
                }
            }
        }
        if let Some(rx) = &self.model_rx
            && let Ok(r) = rx.try_recv()
        {
            self.model_rx = None;
            match r {
                Ok((m, n)) => {
                    self.auto = m.auto;
                    self.candidate = m.candidate;
                    self.model = Some((m, n));
                }
                Err(e) => self.log.push((e, true)),
            }
        }
        if self.changed && self.pending == 0 {
            self.changed = false;
            out.push(QueueAction::Changed);
            if self.model.is_none() && self.model_rx.is_none() {
                self.reload();
            }
        }
        if !self.open {
            return out;
        }
        if let Some(rx) = &self.loading
            && let Ok(r) = rx.try_recv()
        {
            self.loading = None;
            match r {
                Ok(v) => {
                    self.rows = v;
                    self.sel = self.sel.min(self.rows.len().saturating_sub(1));
                }
                Err(e) => self.log.push((e, true)),
            }
        }
        if let Some(a) = self.keys(ctx) {
            out.push(a);
        }
        let now = ctx.input(|i| i.time);
        if let Some(r) = self.rows.get(self.sel) {
            if self.preview_id != r.id {
                self.preview.clear();
                self.preview_id = r.id.clone();
            }
            let (id, scene) = (r.id.clone(), r.scene.clone());
            self.preview.tick_in(
                ctx,
                &format!("queue-{id}"),
                &scene,
                480,
                now,
                self.palette.as_deref(),
            );
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        let mut open = self.open;
        let screen = ctx.content_rect();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Approval queue")
                .size(1000.0, 720.0)
                .configure(|w| {
                    w.default_pos(screen.min + egui::vec2(120.0, 80.0))
                        .default_size([
                            (screen.width() * 0.7).min(1100.0),
                            (screen.height() * 0.75).min(760.0),
                        ])
                        .resizable(true)
                }),
            &mut open,
            |ui| {
                if let Some(a) = self.contents(ui) {
                    out.push(a);
                }
            },
        );
        self.open = open;
        out
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> Option<QueueAction> {
        let mut act = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} waiting · {} approved, {} rejected this session",
                    self.rows.len(),
                    self.approved,
                    self.rejected
                ))
                .strong(),
            );
            if ui.button("Reload").clicked() {
                self.reload();
            }
            ui.label(
                RichText::new("Keys: 1-5 stars · A approve · X reject · Enter rate only · J/K next/previous · O open")
                    .small()
                    .weak(),
            );
        });
        self.model_panel(ui);
        for (m, err) in &self.log {
            ui.colored_label(
                if *err {
                    Color32::LIGHT_RED
                } else {
                    Color32::from_gray(150)
                },
                RichText::new(m).small(),
            );
        }
        ui.separator();
        if self.loading.is_some() && self.rows.is_empty() {
            ui.label("Reading the candidates…");
            return None;
        }
        if self.rows.is_empty() {
            crate::thumbs::empty_note(
                ui,
                "Nothing waits for approval. Stage 1 searches (`jetcarp stage1`) add candidates below the auto threshold.",
            );
            return None;
        }
        let list_w = 300.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(list_w);
                let row_h = 34.0;
                let n = self.rows.len();
                let mut pick = None;
                egui::ScrollArea::vertical()
                    .id_salt("queue-list")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show_rows(ui, row_h, n, |ui, range| {
                        for i in range {
                            let r = &self.rows[i];
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(list_w - 14.0, row_h),
                                Sense::click(),
                            );
                            let p = ui.painter_at(rect);
                            if i == self.sel {
                                p.rect_filled(rect, 3.0, Color32::from_rgb(0x3a, 0x2c, 0x1c));
                            } else if resp.hovered() {
                                p.rect_filled(rect, 3.0, Color32::from_gray(40));
                            }
                            p.text(
                                rect.min + egui::vec2(6.0, 3.0),
                                egui::Align2::LEFT_TOP,
                                &r.title,
                                egui::FontId::proportional(13.0),
                                Color32::from_gray(225),
                            );
                            p.text(
                                rect.min + egui::vec2(6.0, 19.0),
                                egui::Align2::LEFT_TOP,
                                format!(
                                    "{} · {}{}",
                                    qsmenu::row_grid(r),
                                    r.behaviour,
                                    r.score
                                        .map(|s| format!(" · score {s:.1}"))
                                        .unwrap_or_default()
                                ),
                                egui::FontId::proportional(11.0),
                                Color32::from_gray(140),
                            );
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                if let Some(i) = pick {
                    self.sel = i;
                    self.stars = None;
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(r) = self.rows.get(self.sel).cloned() else {
                    return;
                };
                let side = (ui.available_width() - 10.0)
                    .min(ui.available_height() - 150.0)
                    .max(200.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(side, side * 0.75), Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 3.0, Color32::from_gray(18));
                if let Some(t) = self.preview.texture() {
                    let s = t.size_vec2();
                    let k = (rect.width() / s.x).min(rect.height() / s.y);
                    p.image(
                        t.id(),
                        egui::Rect::from_center_size(rect.center(), s * k),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.horizontal(|ui| {
                    ui.label("Colours");
                    crate::palette_pick::picker_opt(
                        ui,
                        "queue-palette",
                        &mut self.palette,
                        &[],
                        "Entry's own",
                    );
                    ui.label(RichText::new("preview only").small().weak());
                });
                ui.label(RichText::new(&r.title).strong().size(16.0));
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {} · {}",
                        r.rule,
                        qsmenu::row_grid(&r),
                        r.rule_family,
                        r.behaviour
                    ))
                    .monospace()
                    .small(),
                );
                if let Some(s) = r.score {
                    ui.label(format!("Score {s:.1} (below the auto threshold)"));
                }
                ui.label(RichText::new(&r.description).small().weak());
                ui.horizontal(|ui| {
                    let cur = self.stars.unwrap_or(0);
                    for k in 1..=5u8 {
                        let (sr, resp) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
                        crate::thumbs::star(
                            ui.painter(),
                            sr.center(),
                            9.0,
                            k <= cur,
                            if k <= cur || resp.hovered() {
                                Color32::from_rgb(0xff, 0xc8, 0x3c)
                            } else {
                                Color32::from_gray(90)
                            },
                        );
                        if resp.on_hover_text(format!("{k} of 5 (key {k})")).clicked() {
                            self.stars = Some(k);
                        }
                    }
                    ui.add_space(12.0);
                    if ui
                        .button(RichText::new("Approve (A)").strong())
                        .on_hover_text(
                            "Show it in the Quick Start; counts as a rating (the stars, or 4)",
                        )
                        .clicked()
                    {
                        self.act(Some(true));
                    }
                    if ui.button("Reject (X)").clicked() {
                        self.act(Some(false));
                    }
                    if ui
                        .add_enabled(self.stars.is_some(), egui::Button::new("Rate only (Enter)"))
                        .clicked()
                    {
                        self.act(None);
                    }
                    if ui.button("Skip (J)").clicked() {
                        self.move_sel(1);
                    }
                    if ui.button("Open (O)").clicked() {
                        act = Some(QueueAction::Open(Box::new(r.scene.clone())));
                    }
                });
            });
        });
        act
    }
}
