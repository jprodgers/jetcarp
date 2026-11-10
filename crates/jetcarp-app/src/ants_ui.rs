//! The *Ants…* window: ant and turmite searches (`jetcarp_core::ants`) as background catalog jobs,
//! with live progress (the same `AntProgress` the CLI, scripts, and MCP see), the outcome census
//! per tiling, and the stored runs, each of which opens as its build-up scene in the main view.
//! A selected run can be named, rated (1-5, the catalog's `ratings` too), and curated (new,
//! reviewed, promoted, rejected: promoted and reviewed runs go to the website export), and its
//! media are rendered into the catalog on a background thread and played here: a highway's
//! seamless loop in a window moving with the ant, other outcomes their build-up from step 0.
//! *Colours* picks a palette for the media and for runs opened in the main view: the house
//! style (stored in the catalog, Prism when opened) or any built-in palette (rendered in
//! memory for this window, so the catalog's media stay in the house style).
//! `JETCARP_ANTS_SELECT=<k>` selects row k (headless checks).

use jetcarp_core::ants::job::{self, AntProgress, AntSpec};
use jetcarp_core::ants::run::Outcome;
use jetcarp_core::ants::search::Strings;
use jetcarp_core::jobs::{JobHandle, RunOptions, Runner};
use jetcarp_core::library::{AntCensus, AntQuery, AntRow, AntUpdate, Library};
use jetcarp_core::media::{self, MediaOptions, MediaStyle, MediaTarget};
use jetcarp_core::render::Image;
use jetcarp_core::world::Scene;
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::sync::mpsc;
use std::time::Instant;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "ants";

/// Decoded animation frames and their delay, or why there are none.
type MediaResult = Result<(Vec<Image>, u32, String), String>;

/// The selected run's curation and media.
struct Selected {
    id: String,
    name: String,
    media: Option<mpsc::Receiver<MediaResult>>,
    frames: Vec<TextureHandle>,
    delay_ms: u32,
    started: Instant,
    note: String,
}

const STATES: [&str; 4] = ["new", "reviewed", "promoted", "rejected"];

pub enum Action {
    Open(Box<Scene>),
    Message(String, bool),
}

pub struct AntsWindow {
    pub open: bool,
    lib: Library,
    tilings: String,
    min_len: usize,
    max_len: usize,
    steps: u64,
    handle: Option<JobHandle>,
    last: Option<AntProgress>,
    outcome: Option<Outcome>,
    tiling_filter: String,
    sort: &'static str,
    rows: Vec<AntRow>,
    census: Vec<AntCensus>,
    loaded: Option<Instant>,
    error: Option<String>,
    selected: Option<Selected>,
    select_env: Option<usize>,
    /// Palette for media and opened runs (`None`: the house style; saved in the settings).
    pub palette: Option<String>,
}

impl Default for AntsWindow {
    fn default() -> Self {
        AntsWindow {
            open: false,
            lib: Library::open_default(),
            tilings: "square".into(),
            min_len: 2,
            max_len: 5,
            steps: 1_000_000,
            handle: None,
            last: None,
            outcome: Some(Outcome::Highway),
            tiling_filter: String::new(),
            sort: "score",
            rows: vec![],
            census: vec![],
            loaded: None,
            error: None,
            selected: None,
            select_env: std::env::var("JETCARP_ANTS_SELECT")
                .ok()
                .and_then(|v| v.parse().ok()),
            palette: None,
        }
    }
}

impl AntsWindow {
    /// `JETCARP_ANTS=<tilings>:<min>-<max>` starts a search; `JETCARP_ANTS=list` only opens the window.
    pub fn open_env(&mut self, s: &str) {
        self.open = true;
        if s != "list" && s != "1" {
            let (t, r) = s.split_once(':').unwrap_or((s, "2-4"));
            self.tilings = t.to_string();
            if let Some((a, b)) = r.split_once('-')
                && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
            {
                (self.min_len, self.max_len) = (a, b);
            }
            self.start();
        }
        self.reload();
    }

    fn running(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    fn start(&mut self) {
        self.error = None;
        let tilings = match jetcarp_core::library::tiling_list(&self.tilings) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let spec = AntSpec {
            tilings,
            strings: Some(Strings {
                min: self.min_len.min(self.max_len),
                max: self.max_len.max(self.min_len),
                alphabet: None,
            }),
            steps: self.steps,
            ..Default::default()
        };
        match job::start(&self.lib, &spec) {
            Ok(jid) => match Runner::new(self.lib.clone()).spawn(jid, RunOptions::default()) {
                Ok(h) => self.handle = Some(h),
                Err(e) => self.error = Some(e),
            },
            Err(e) => self.error = Some(e),
        }
    }

    fn reload(&mut self) {
        let q = AntQuery {
            outcome: self.outcome,
            tiling: (!self.tiling_filter.trim().is_empty()).then(|| self.tiling_filter.clone()),
            sort: Some(self.sort.into()),
            limit: Some(200),
            ..Default::default()
        };
        match self.lib.ants(&q) {
            Ok(r) => self.rows = r,
            Err(e) => self.error = Some(e),
        }
        self.census = self.lib.ant_census(None).unwrap_or_default();
        self.loaded = Some(Instant::now());
    }

    /// Select a run: its name for editing, and its media rendered (or read) in the background.
    fn select(&mut self, r: &AntRow) {
        let (tx, rx) = mpsc::channel();
        let lib = self.lib.clone();
        let id = r.id.clone();
        let palette = self.palette.clone();
        std::thread::spawn(move || {
            let res = (|| -> MediaResult {
                if let Some(p) = &palette {
                    // Another palette: in memory only, the catalog keeps the house style.
                    let o = MediaOptions {
                        style: MediaStyle::with_palette(Some(p))?,
                        ..Default::default()
                    };
                    let a = media::target_animation(&lib, &MediaTarget::Ant(id.clone()), &o)?;
                    let note = format!(
                        "{} in {p} (not stored), {} frames, {} × {}",
                        if a.mode == media::MediaMode::Alone {
                            "the highway's loop, alone"
                        } else {
                            "the build-up from step 0"
                        },
                        a.frames.len(),
                        a.width(),
                        a.height()
                    );
                    let mut frames = a.frames;
                    frames.truncate(240);
                    return Ok((frames, a.delay_ms.max(20), note));
                }
                let o = MediaOptions::default();
                let done = media::render(&lib, &MediaTarget::Ant(id.clone()), &o)?;
                let a = done.animation().ok_or("no animation was rendered")?.clone();
                let bytes = std::fs::read(&a.abs_path)
                    .map_err(|e| format!("{}: {e}", a.abs_path.display()))?;
                let mut frames = if a.kind == "gif" {
                    media::encode::decode_gif(&bytes)?
                } else {
                    media::encode::decode_webp(&bytes)?
                };
                frames.truncate(240);
                let delay = a.render["delay_ms"].as_u64().unwrap_or(80) as u32;
                let note = format!(
                    "{} ({}), {} frames, {} × {}{}",
                    if done.mode == media::MediaMode::Alone {
                        "the highway's loop, alone"
                    } else {
                        "the build-up from step 0"
                    },
                    a.kind,
                    a.frames,
                    a.width,
                    a.height,
                    if done.written { ", rendered now" } else { "" }
                );
                Ok((frames, delay.max(20), note))
            })();
            let _ = tx.send(res);
        });
        self.selected = Some(Selected {
            id: r.id.clone(),
            name: r.name.clone().unwrap_or_default(),
            media: Some(rx),
            frames: vec![],
            delay_ms: 80,
            started: Instant::now(),
            note: "Rendering media…".into(),
        });
    }

    fn update(&mut self, id: &str, u: AntUpdate) {
        match self.lib.update_ant(id, &u) {
            Ok(_) => self.reload(),
            Err(e) => self.error = Some(e),
        }
    }

    /// The selected run's panel: name, rating, state, media.
    fn detail(&mut self, ui: &mut egui::Ui) {
        let Some(sel) = &mut self.selected else {
            return;
        };
        if let Some(rx) = &sel.media {
            match rx.try_recv() {
                Ok(Ok((frames, delay, note))) => {
                    sel.frames = frames
                        .iter()
                        .enumerate()
                        .map(|(i, im)| {
                            let ci = egui::ColorImage::from_rgba_unmultiplied(
                                [im.width as usize, im.height as usize],
                                &im.rgba,
                            );
                            ui.ctx().load_texture(
                                format!("ant-{}-{i}", sel.id),
                                ci,
                                egui::TextureOptions::NEAREST,
                            )
                        })
                        .collect();
                    sel.delay_ms = delay;
                    sel.started = Instant::now();
                    sel.note = note;
                    sel.media = None;
                }
                Ok(Err(e)) => {
                    sel.note = format!("No media: {e}");
                    sel.media = None;
                }
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint(),
                Err(mpsc::TryRecvError::Disconnected) => sel.media = None,
            }
        }
        let Some(r) = self.rows.iter().find(|r| r.id == sel.id).cloned() else {
            return;
        };
        let id = sel.id.clone();
        let mut upd: Option<AntUpdate> = None;
        ui.separator();
        ui.label(RichText::new(r.line()).strong());
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut sel.name).desired_width(180.0));
            if ui.button("Save name").clicked() {
                upd = Some(AntUpdate {
                    name: Some(sel.name.trim().to_string()),
                    ..Default::default()
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Rating");
            ui.spacing_mut().item_spacing.x = 0.0;
            for k in 1..=5i64 {
                let on = r.rating.is_some_and(|x| x >= k);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(17.0, 17.0), egui::Sense::click());
                let lit = on || resp.hovered();
                crate::thumbs::star(
                    ui.painter(),
                    rect.center(),
                    7.0,
                    on,
                    if lit {
                        Color32::from_rgb(0xff, 0xc8, 0x3c)
                    } else {
                        Color32::from_gray(90)
                    },
                );
                if resp.on_hover_text(format!("Rate {k} of 5")).clicked() {
                    upd = Some(AntUpdate {
                        rating: Some(k),
                        ..Default::default()
                    });
                }
            }
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label("State");
            let mut st = r.state.clone();
            egui::ComboBox::from_id_salt("ant-state")
                .selected_text(st.clone())
                .show_ui(ui, |ui| {
                    for s in STATES {
                        ui.selectable_value(&mut st, s.to_string(), s);
                    }
                });
            if st != r.state {
                upd = Some(AntUpdate {
                    state: Some(st),
                    ..Default::default()
                });
            }
            ui.label(
                RichText::new("promoted and reviewed runs are public (website export)")
                    .small()
                    .weak(),
            );
        });
        ui.label(RichText::new(&sel.note).small());
        if !sel.frames.is_empty() {
            let n = sel.frames.len();
            let k = (sel.started.elapsed().as_millis() / sel.delay_ms as u128) as usize % n;
            let t = &sel.frames[k];
            let sz = t.size_vec2();
            let s = (300.0 / sz.x.max(sz.y)).min(4.0);
            ui.image((t.id(), sz * s));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(sel.delay_ms as u64));
        }
        if let Some(u) = upd {
            self.update(&id, u);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = vec![];
        if let Some(k) = self.select_env
            && let Some(r) = self.rows.get(k).cloned()
        {
            self.select_env = None;
            self.select(&r);
        }
        if let Some(h) = &self.handle {
            self.last = Some(h.progress_as::<AntProgress>());
            if h.is_finished() {
                self.handle = None;
                self.reload();
            }
        }
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
            if self.open && self.loaded.is_none_or(|t| t.elapsed().as_secs_f64() > 1.0) {
                self.reload();
            }
        }
        if !self.open {
            return out;
        }
        let mut open = true;
        let running = self.running();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Ant and turmite search")
                .size(620.0, 560.0)
                .configure(|w| w.default_width(620.0).default_height(560.0)),
            &mut open,
            |ui| {
                ui.label(
                    RichText::new(
                        "Turn strings (Langton's ant is RL) on unbounded tilings: each run is a \
                         highway (proved exactly: period, displacement, onset), periodic, \
                         symmetric growth, bounded, or chaotic, and goes to the catalog.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Tilings");
                        ui.add(egui::TextEdit::singleline(&mut self.tilings).desired_width(150.0))
                            .on_hover_text("square, hex, triangle, 4.8.8, archimedean, …");
                        ui.label("Lengths");
                        ui.add(egui::DragValue::new(&mut self.min_len).range(2..=12));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.max_len).range(2..=12));
                        ui.label("Steps");
                        ui.add(
                            egui::DragValue::new(&mut self.steps)
                                .range(1000..=100_000_000)
                                .speed(10_000),
                        );
                        if ui.button("Search").clicked() {
                            self.start();
                        }
                    });
                });
                if let Some(h) = &self.handle
                    && ui.button("Stop (resumable)").clicked()
                {
                    h.stop();
                }
                if let Some(p) = &self.last {
                    let frac = if p.runs_total > 0 {
                        p.runs_done as f32 / p.runs_total as f32
                    } else {
                        0.0
                    };
                    ui.add(egui::ProgressBar::new(frac).text(p.line()));
                }
                if let Some(e) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                ui.separator();
                ui.label(RichText::new("Outcomes per tiling (every stored run)").strong());
                egui::Grid::new("ant-census").striped(true).show(ui, |ui| {
                    ui.label("tiling");
                    for o in Outcome::ALL {
                        ui.label(o.name());
                    }
                    ui.end_row();
                    for c in &self.census {
                        ui.label(&c.tiling_name);
                        for o in Outcome::ALL {
                            ui.label(c.by_outcome.get(o.name()).copied().unwrap_or(0).to_string());
                        }
                        ui.end_row();
                    }
                });
                ui.separator();
                let mut changed = false;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("ant-outcome")
                        .selected_text(self.outcome.map(|o| o.name()).unwrap_or("any outcome"))
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut self.outcome, None, "any outcome")
                                .clicked();
                            for o in Outcome::ALL {
                                changed |= ui
                                    .selectable_value(&mut self.outcome, Some(o), o.name())
                                    .clicked();
                            }
                        });
                    ui.label("tiling");
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.tiling_filter).desired_width(90.0),
                        )
                        .lost_focus();
                    egui::ComboBox::from_id_salt("ant-sort")
                        .selected_text(format!("by {}", self.sort))
                        .show_ui(ui, |ui| {
                            for s in ["score", "onset", "period", "speed", "rule"] {
                                changed |= ui.selectable_value(&mut self.sort, s, s).clicked();
                            }
                        });
                    if ui.button("Refresh").clicked() {
                        changed = true;
                    }
                });
                if changed {
                    self.reload();
                }
                ui.horizontal(|ui| {
                    ui.label("Colours");
                    if crate::palette_pick::picker_opt(
                        ui,
                        "ants-palette",
                        &mut self.palette,
                        &[],
                        "House style",
                    ) {
                        // Play the selected run's media in the new colours.
                        if let Some(r) = self
                            .selected
                            .as_ref()
                            .and_then(|s| self.rows.iter().find(|r| r.id == s.id).cloned())
                        {
                            self.select(&r);
                        }
                    }
                    ui.label(
                        RichText::new("media, and runs opened in the main view")
                            .small()
                            .weak(),
                    );
                });
                let mut pick: Option<AntRow> = None;
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        egui::Grid::new("ant-rows").striped(true).show(ui, |ui| {
                            for r in &self.rows {
                                let sel = self.selected.as_ref().is_some_and(|s| s.id == r.id);
                                if ui
                                    .selectable_label(sel, "Select")
                                    .on_hover_text("Name, rate, curate, and play its media")
                                    .clicked()
                                {
                                    pick = Some(r.clone());
                                }
                                if ui
                                    .small_button("Open")
                                    .on_hover_text("The run from step 0 in the main view")
                                    .clicked()
                                {
                                    match jetcarp_core::ants::media::build_scene_in(
                                        r,
                                        self.palette.as_deref(),
                                    ) {
                                        Ok(sc) => out.push(Action::Open(Box::new(sc))),
                                        Err(e) => out.push(Action::Message(e, true)),
                                    }
                                }
                                ui.monospace(
                                    r.rule
                                        .trim_start_matches("Turmite ")
                                        .trim_start_matches("Ant "),
                                );
                                ui.label(&r.tiling_name);
                                ui.label(r.outcome.name());
                                ui.label(match (r.period, r.d, r.onset) {
                                    (Some(p), Some(d), Some(o)) => {
                                        format!("p{p}  d({}, {})  from step {o}", d[0], d[1])
                                    }
                                    _ => format!("{} cells at {} steps", r.cells, r.steps),
                                });
                                ui.label(match (&r.name, r.rating) {
                                    (Some(n), Some(k)) => format!("{n} ({k}/5)"),
                                    (Some(n), None) => n.clone(),
                                    (None, Some(k)) => format!("{k}/5"),
                                    _ => String::new(),
                                });
                                ui.end_row();
                            }
                        });
                    });
                if let Some(r) = pick {
                    self.select(&r);
                }
                self.detail(ui);
            },
        );
        self.open = open;
        out
    }
}

#[cfg(test)]
impl AntsWindow {
    /// The search started by `open_env` has finished (offscreen checks).
    pub fn search_done(&self) -> bool {
        self.handle.is_none()
    }
    /// Select the first stored run (offscreen checks).
    pub fn select_first(&mut self) {
        self.reload();
        if let Some(r) = self.rows.first().cloned() {
            self.select(&r);
        }
    }
}
