//! The Library window: browse the pattern library (`jetcarp_store::library`) as a thumbnail grid,
//! filter and sort it, curate records (name, tags, notes, rating, state), paste a pattern into
//! the world on screen (through `Command::PastePattern`), open it alone in a new world, and
//! promote it to the Quick Start ("My finds").

use crate::sweep_form::thousands;
use crate::thumbs::{self, Thumb, ThumbCache};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use jetcarp_core::Scene;
use jetcarp_core::pattern::promote::{render_phases_in, showcase_scene};
use jetcarp_core::pattern::{
    CurationState, PatternClass, PatternRecord, PatternSource, orientation_moves,
};
use jetcarp_store::{Library, Query, Update};
use std::sync::Arc;
use std::time::Instant;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "library";

/// A pattern ready to paste with a click in the main view.
#[derive(Clone)]
pub struct PasteRequest {
    /// Inline cells (resolved from the library).
    pub source: PatternSource,
    pub label: String,
    pub rule: String,
    pub orientation: u32,
}

pub enum Action {
    /// Paste with the next clicks in the main view.
    Arm(Box<PasteRequest>),
    /// Open this scene in the main view.
    Open(Box<Scene>),
    /// A scene was promoted: the Quick Start should reload "My finds".
    QuickStartChanged,
    Message(String, bool),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Score,
    Seen,
    Updated,
    Created,
    Period,
    Cells,
}

impl Sort {
    const ALL: [(Sort, &'static str); 6] = [
        (Sort::Score, "score"),
        (Sort::Seen, "times seen"),
        (Sort::Updated, "last changed"),
        (Sort::Created, "date found"),
        (Sort::Period, "period"),
        (Sort::Cells, "cells"),
    ];
    fn label(self) -> &'static str {
        Self::ALL.iter().find(|x| x.0 == self).map_or("", |x| x.1)
    }
}

/// The filter fields of the window.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Filters {
    pub text: String,
    pub rule: String,
    pub tiling: String,
    pub class: Option<PatternClass>,
    pub state: Option<CurationState>,
    pub min_period: u64,
    /// 0 = no upper limit.
    pub max_period: u64,
    pub min_score: f64,
}

impl Filters {
    pub fn query(&self) -> Query {
        let s = |t: &String| (!t.trim().is_empty()).then(|| t.trim().to_string());
        Query {
            rule: s(&self.rule),
            tiling: s(&self.tiling),
            class: self.class,
            min_period: (self.min_period > 0).then_some(self.min_period),
            max_period: (self.max_period > 0).then_some(self.max_period),
            min_score: (self.min_score > 0.0).then_some(self.min_score),
            state: self.state,
            text: s(&self.text),
            ..Default::default()
        }
    }
}

/// Records to show, in order.
pub fn view(records: &[PatternRecord], f: &Filters, sort: Sort, descending: bool) -> Vec<usize> {
    let q = f.query();
    let mut v: Vec<usize> = (0..records.len())
        .filter(|&i| q.matches(&records[i]))
        .collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (&records[a], &records[b]);
        let o = match sort {
            Sort::Score => x
                .score
                .partial_cmp(&y.score)
                .unwrap_or(std::cmp::Ordering::Equal),
            Sort::Seen => x.seen.cmp(&y.seen),
            Sort::Updated => x.updated.cmp(&y.updated),
            Sort::Created => x.created.cmp(&y.created),
            Sort::Period => x.period.cmp(&y.period),
            Sort::Cells => x.cells.cmp(&y.cells),
        };
        (if descending { o.reverse() } else { o }).then(x.id.cmp(&y.id))
    });
    v
}

/// Curation fields being edited.
#[derive(Clone, PartialEq)]
struct Edit {
    id: String,
    name: String,
    tags: String,
    notes: String,
    rating: u8,
    state: CurationState,
}

impl Edit {
    fn of(r: &PatternRecord) -> Edit {
        Edit {
            id: r.id.clone(),
            name: r.name.clone().unwrap_or_default(),
            tags: r.tags.join(", "),
            notes: r.notes.clone(),
            rating: r.rating.unwrap_or(0),
            state: r.state,
        }
    }
    fn update(&self) -> Update {
        Update {
            name: Some(self.name.clone()),
            tags: Some(
                self.tags
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect(),
            ),
            notes: Some(self.notes.clone()),
            rating: Some(self.rating),
            state: Some(self.state),
            ..Default::default()
        }
    }
}

pub struct LibraryWindow {
    pub open: bool,
    lib: Library,
    records: Vec<PatternRecord>,
    error: Option<String>,
    loaded: Option<Instant>,
    filters: Filters,
    sort: Sort,
    descending: bool,
    order: Vec<usize>,
    dirty: bool,
    cache: ThumbCache,
    cell_w: f32,
    selected: Option<String>,
    edit: Option<Edit>,
    strip: Option<(String, Result<TextureHandle, String>)>,
    orientation: u32,
    phase: u32,
    moves: Vec<[i32; 2]>,
    /// Palette of the thumbnails and the phase strip (`None`: the rule's colours; saved in the
    /// settings).
    pub palette: Option<String>,
    /// Records a pack retracted, by id: `retracted by <publisher> on <date>: <reason>`.
    retracted: std::collections::HashMap<String, Vec<String>>,
}

impl Default for LibraryWindow {
    fn default() -> Self {
        LibraryWindow {
            open: false,
            lib: Library::open_default(),
            records: vec![],
            error: None,
            loaded: None,
            filters: Filters::default(),
            sort: Sort::Score,
            descending: true,
            order: vec![],
            dirty: true,
            cache: ThumbCache::new(2),
            cell_w: 170.0,
            selected: None,
            edit: None,
            strip: None,
            orientation: 0,
            phase: 0,
            palette: None,
            moves: vec![],
            retracted: Default::default(),
        }
    }
}

const STATES: [CurationState; 4] = [
    CurationState::New,
    CurationState::Reviewed,
    CurationState::Promoted,
    CurationState::Rejected,
];

const CLASSES: [PatternClass; 6] = [
    PatternClass::StillLife,
    PatternClass::Oscillator,
    PatternClass::Spaceship,
    PatternClass::Puffer,
    PatternClass::Gun,
    PatternClass::Mover,
];

impl LibraryWindow {
    /// Read the records again (they are files other programs may change).
    pub fn reload(&mut self) {
        match self.lib.list() {
            Ok(v) => {
                self.records = v;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        let names = self.lib.publisher_names().unwrap_or_default();
        self.retracted = self
            .lib
            .retractions_by_id("pattern")
            .unwrap_or_default()
            .into_iter()
            .map(|(id, rows)| (id, rows.iter().map(|r| r.describe(&names)).collect()))
            .collect();
        self.loaded = Some(Instant::now());
        self.dirty = true;
    }

    /// The selected record's id (the Catalog tools window offers it).
    pub fn selected_id(&self) -> Option<String> {
        self.selected.clone()
    }

    /// Select a record by id, prefix, or name (headless checks).
    pub fn select_key(&mut self, key: &str) {
        if let Ok(r) = self.lib.get(key) {
            self.select(&r.id);
        }
    }

    fn record(&self, id: &str) -> Option<&PatternRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    fn select(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_string());
        self.edit = self.record(id).map(Edit::of);
        self.strip = None;
        self.orientation = 0;
        self.phase = 0;
        self.moves = self
            .record(id)
            .and_then(|r| orientation_moves(r).ok())
            .unwrap_or_default();
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        if self.loaded.is_none_or(|t| t.elapsed().as_secs() > 15) {
            self.reload();
        }
        if self.cache.begin_frame(ctx) {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let mut open = self.open;
        let screen = ctx.content_rect();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Pattern library")
                .size(1100.0, 800.0)
                .configure(|w| {
                    w.default_pos(screen.min + egui::vec2(345.0, 60.0))
                        .default_size([
                            (screen.width() * 0.75).min(1300.0),
                            (screen.height() * 0.8).min(900.0),
                        ])
                        .min_size([420.0, 320.0])
                        .resizable(true)
                }),
            &mut open,
            |ui| self.contents(ui, &mut out),
        );
        self.open = open;
        out
    }

    fn contents(&mut self, ui: &mut egui::Ui, out: &mut Vec<Action>) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} patterns in {}",
                    thousands(self.records.len() as u64),
                    self.lib.path().display()
                ))
                .weak(),
            );
            if ui.button("Reload").clicked() {
                self.reload();
            }
            if ui.button("Show in file manager").clicked() {
                let dir = self.lib.folder();
                let _ = std::fs::create_dir_all(&dir);
                crate::results::open_in_file_manager(&dir);
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let before = (self.filters.clone(), self.sort, self.descending);
        let f = &mut self.filters;
        ui.horizontal_wrapped(|ui| {
            ui.label("Find");
            ui.add(
                egui::TextEdit::singleline(&mut f.text)
                    .hint_text("name or label")
                    .desired_width(110.0),
            );
            ui.label("Rule");
            ui.add(
                egui::TextEdit::singleline(&mut f.rule)
                    .hint_text("B3/S23")
                    .desired_width(80.0),
            );
            ui.label("Tiling");
            ui.add(
                egui::TextEdit::singleline(&mut f.tiling)
                    .hint_text("hex, k2-001")
                    .desired_width(80.0),
            );
            ui.separator();
            ui.label("Sort");
            egui::ComboBox::from_id_salt("lib-sort")
                .selected_text(self.sort.label())
                .show_ui(ui, |ui| {
                    for (s, l) in Sort::ALL {
                        ui.selectable_value(&mut self.sort, s, l);
                    }
                });
            if ui
                .button(if self.descending {
                    "high to low"
                } else {
                    "low to high"
                })
                .clicked()
            {
                self.descending = !self.descending;
            }
            ui.separator();
            ui.label("Size");
            ui.add(egui::Slider::new(&mut self.cell_w, 110.0..=320.0).show_value(false));
        });
        let f = &mut self.filters;
        ui.horizontal_wrapped(|ui| {
            ui.label("Class");
            egui::ComboBox::from_id_salt("lib-class")
                .selected_text(f.class.map_or("any", |c| c.name()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.class, None, "any");
                    for c in CLASSES {
                        ui.selectable_value(&mut f.class, Some(c), c.name());
                    }
                });
            ui.label("State");
            egui::ComboBox::from_id_salt("lib-state")
                .selected_text(f.state.map_or("any", |c| c.name()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.state, None, "any");
                    for s in STATES {
                        ui.selectable_value(&mut f.state, Some(s), s.name());
                    }
                });
            ui.label("Period");
            ui.add(egui::DragValue::new(&mut f.min_period).range(0..=100_000));
            ui.label("to");
            ui.add(
                egui::DragValue::new(&mut f.max_period)
                    .range(0..=100_000)
                    .custom_formatter(|v, _| {
                        if v <= 0.0 {
                            "any".into()
                        } else {
                            format!("{v:.0}")
                        }
                    }),
            );
            ui.label("Score at least");
            ui.add(
                egui::DragValue::new(&mut f.min_score)
                    .range(0.0..=100.0)
                    .speed(0.5),
            );
            if *f != Filters::default() && ui.small_button("Clear filters").clicked() {
                *f = Filters::default();
            }
        });
        if before != (self.filters.clone(), self.sort, self.descending) {
            self.dirty = true;
        }
        if self.dirty {
            self.order = view(&self.records, &self.filters, self.sort, self.descending);
            self.dirty = false;
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Showing {} · click a pattern for its details, paste, and promotion",
                    thousands(self.order.len() as u64)
                ))
                .small()
                .weak(),
            );
            ui.separator();
            ui.label("Colours");
            if crate::palette_pick::picker_opt(
                ui,
                "lib-palette",
                &mut self.palette,
                &[],
                "Rule's own",
            ) {
                self.strip = None;
            }
        });
        ui.separator();
        if let Some(id) = self.selected.clone()
            && self.record(&id).is_some()
        {
            egui::Panel::right("lib-detail")
                .resizable(true)
                .default_size(360.0)
                .min_size(260.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("lib-detail-scroll")
                        .show(ui, |ui| self.detail(ui, &id, out));
                });
        }
        let order = self.order.clone();
        let mut clicked = None;
        egui::CentralPanel::default().show(ui, |ui| {
            if order.is_empty() {
                thumbs::empty_note(
                    ui,
                    if self.records.is_empty() {
                        "The library is empty. Find patterns with `jetcarp search hunt`, the catalog-hunt script, or Add favourites to library in sweep results."
                    } else {
                        "Nothing matches these filters."
                    },
                );
                return;
            }
            let recs = &self.records;
            let retracted = &self.retracted;
            let sel = self.selected.clone();
            let res = thumbs::grid(
                ui,
                &mut self.cache,
                thumbs::GridSpec {
                    id: "lib-grid",
                    n: order.len(),
                    cell_w: self.cell_w,
                    aspect: 0.75,
                    scroll_to: None,
                },
                &mut |k| {
                    let r = &recs[order[k]];
                    thumbs::Cell {
                        image: Some(thumb(r, self.palette.as_deref())),
                        placeholder: String::new(),
                        title: if retracted.contains_key(&r.id) {
                            format!("{} (retracted)", r.label())
                        } else {
                            r.label()
                        },
                        subtitle: format!(
                            "score {:.0} · seen {} · {} · {}",
                            r.score,
                            r.seen,
                            r.rule,
                            short_tiling(&r.tiling_name),
                        ),
                        star: None,
                        selected: sel.as_deref() == Some(r.id.as_str()),
                    }
                },
                &mut |ui, k| {
                    let r = &recs[order[k]];
                    ui.label(RichText::new(r.label()).strong());
                    ui.label(jetcarp_core::pattern::promote::describe(r));
                    ui.label(RichText::new(format!("state: {}", r.state.name())).weak());
                },
            );
            clicked = res.clicked.or(res.double_clicked);
        });
        if let Some(k) = clicked {
            let id = self.records[order[k]].id.clone();
            self.select(&id);
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, id: &str, out: &mut Vec<Action>) {
        let Some(r) = self.record(id).cloned() else {
            return;
        };
        ui.label(RichText::new(r.label()).strong().size(16.0));
        ui.horizontal(|ui| {
            // The short id (the first 8 digits, which commands accept); the whole id on hover
            // and when copied.
            ui.label(
                RichText::new(&r.id[..r.id.len().min(8)])
                    .monospace()
                    .small()
                    .weak(),
            )
            .on_hover_text(&r.id);
            if ui.small_button("copy id").clicked() {
                ui.ctx().copy_text(r.id.clone());
            }
        });
        for x in self.retracted.get(&r.id).into_iter().flatten() {
            // A retracted record stays here, shown as retracted, with who did and why.
            let mut t = x.clone();
            if let Some(f) = t.get_mut(..1) {
                f.make_ascii_uppercase();
            }
            ui.colored_label(Color32::from_rgb(230, 110, 90), t);
        }
        if r.needs_reverify {
            ui.colored_label(
                Color32::from_rgb(230, 160, 60),
                "Needs re-verification: the engine's geometry for this tiling changed since it \
                 was verified (jetcarp library reverify)",
            );
        }
        // Every phase, side by side.
        if self.strip.as_ref().is_none_or(|s| s.0 != r.id) {
            let tex = render_phases_in(&r, 120, 8, self.palette.as_deref()).map(|img| {
                ui.ctx().load_texture(
                    format!("lib-strip-{}", r.id),
                    thumbs::color_image(&img, 1200),
                    egui::TextureOptions::LINEAR,
                )
            });
            self.strip = Some((r.id.clone(), tex));
        }
        if let Some((_, s)) = &self.strip {
            match s {
                Ok(t) => {
                    let sz = t.size_vec2();
                    let k = (ui.available_width() / sz.x).min(1.0);
                    ui.image((t.id(), sz * k));
                }
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            }
        }
        egui::Grid::new("lib-facts")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(RichText::new(k).weak());
                    ui.label(v);
                    ui.end_row();
                };
                row("class", r.class.name().into());
                row("period", r.period.to_string());
                if let Some(s) = &r.speed {
                    row(
                        "speed",
                        format!("{s} ({:?} per period)", r.displacement.unwrap_or([0, 0])),
                    );
                }
                row(
                    "cells",
                    format!(
                        "{} (from {} to {} over a period)",
                        r.cells, r.min_cells, r.max_cells
                    ),
                );
                row("rule", format!("{} ({:?})", r.rule, r.neighborhood));
                if r.rules.len() > 1 {
                    row("verified in", r.rules.join(", "));
                }
                if let Some(x) = &r.range {
                    row("works in", x.describe());
                }
                row("tiling", r.tiling_name.clone());
                row("seen", r.seen.to_string());
                row("score", format!("{:.1}", r.score));
                row(
                    "verified",
                    format!(
                        "{} periods ({} generations) on {}",
                        r.verified.periods, r.verified.generations, r.verified.field
                    ),
                );
                row("found by", format!("{:?}", r.discovered_by).to_lowercase());
            });
        egui::CollapsingHeader::new("Provenance")
            .default_open(false)
            .show(ui, |ui| {
                let p = &r.provenance;
                ui.label(format!("source: {}", p.source));
                if let Some(t) = &p.tiling {
                    ui.label(format!(
                        "tiling: {}",
                        serde_json::to_string(t).unwrap_or_default()
                    ));
                }
                if let Some(x) = p.rng_state {
                    ui.label(format!("RNG state: {x}"));
                }
                if let Some(x) = p.soup {
                    ui.label(format!("soup: {x}"));
                }
                if let Some(x) = p.density {
                    ui.label(format!("density: {x}"));
                }
                if let Some(x) = p.generation {
                    ui.label(format!("generation: {x}"));
                }
                ui.label(format!("JETCARP {} on {}", p.jetcarp_version, p.date));
            });
        ui.separator();
        // Curation.
        let mut save = false;
        if let Some(e) = &mut self.edit {
            egui::Grid::new("lib-edit")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(200.0));
                    ui.end_row();
                    ui.label("Tags");
                    ui.add(
                        egui::TextEdit::singleline(&mut e.tags)
                            .hint_text("comma separated")
                            .desired_width(200.0),
                    );
                    ui.end_row();
                    ui.label("State");
                    ui.horizontal_wrapped(|ui| {
                        for s in STATES {
                            ui.radio_value(&mut e.state, s, s.name());
                        }
                    });
                    ui.end_row();
                    ui.label("Rating");
                    ui.horizontal(|ui| {
                        for k in 0..=5u8 {
                            ui.radio_value(
                                &mut e.rating,
                                k,
                                if k == 0 {
                                    "none".to_string()
                                } else {
                                    k.to_string()
                                },
                            );
                        }
                    });
                    ui.end_row();
                });
            ui.label("Notes");
            ui.add(
                egui::TextEdit::multiline(&mut e.notes)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            let changed = *e != Edit::of(&r);
            ui.horizontal(|ui| {
                save = ui
                    .add_enabled(changed, egui::Button::new("Save changes"))
                    .clicked();
                if changed && ui.button("Undo").clicked() {
                    *e = Edit::of(&r);
                }
            });
        }
        if save && let Some(e) = self.edit.clone() {
            match self.lib.update(&e.id, &e.update()) {
                Ok(_) => {
                    self.reload();
                    out.push(Action::Message(format!("Saved {}", e.id), false));
                }
                Err(err) => out.push(Action::Message(err, true)),
            }
        }
        ui.separator();
        // Use it.
        ui.horizontal_wrapped(|ui| {
            if r.phases.len() > 1 {
                ui.label("Phase");
                ui.add(
                    egui::DragValue::new(&mut self.phase)
                        .range(0..=r.period.saturating_sub(1) as u32),
                );
            }
            if !self.moves.is_empty() || !r.is_aperiodic() {
                ui.label("Orientation");
                let n = self.moves.len().max(1) as u32;
                ui.add(
                    egui::DragValue::new(&mut self.orientation)
                        .range(0..=n.saturating_sub(1).max(7)),
                );
                if let Some(m) = self.moves.get(self.orientation as usize) {
                    ui.label(RichText::new(format!("moves {m:?}")).weak());
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(RichText::new("Paste into world").strong())
                .on_hover_text("Then click a cell in the main view (the pattern's corner goes there). Esc stops.")
                .clicked()
            {
                let src = PatternSource {
                    library: Some(r.id.clone()),
                    ..Default::default()
                };
                match jetcarp_core::pattern::paste::resolve(&src, self.phase, Some(&self.lib)) {
                    Ok(source) => out.push(Action::Arm(Box::new(PasteRequest {
                        source,
                        label: r.label(),
                        rule: r.rule.clone(),
                        orientation: self.orientation,
                    }))),
                    Err(e) => out.push(Action::Message(e, true)),
                }
            }
            if ui
                .button("Open in a new world")
                .on_hover_text("The pattern alone on a grid sized for it (the Quick Start scene)")
                .clicked()
            {
                match showcase_scene(&r, None, None) {
                    Ok(sc) => out.push(Action::Open(Box::new(sc))),
                    Err(e) => out.push(Action::Message(e, true)),
                }
            }
            if ui
                .button("Promote to Quick Start")
                .on_hover_text("Adds it to the Quick Start under My finds and marks it promoted")
                .clicked()
            {
                match promote(&self.lib, &r) {
                    Ok(entry) => {
                        self.reload();
                        self.edit = self.record(&r.id).map(Edit::of);
                        out.push(Action::QuickStartChanged);
                        out.push(Action::Message(
                            format!("Promoted {} to the Quick Start ({entry})", r.label()),
                            false,
                        ));
                    }
                    Err(e) => out.push(Action::Message(e, true)),
                }
            }
        });
    }
}

/// Add a pattern to the Quick Start ("My finds") with `quickstart.add`, titled with its name, and
/// mark it promoted. Returns the entry (`find:<id>`).
fn promote(lib: &Library, r: &PatternRecord) -> Result<String, String> {
    use jetcarp_api::op::{Op, Quiet};
    use jetcarp_api::ops::quickstart::{AddIn, QuickstartAdd};
    let cx = jetcarp_api::Ctx::new().with_store(lib.clone());
    let a = AddIn {
        pattern: Some(r.id.clone()),
        title: r.name.clone(),
        ..Default::default()
    };
    QuickstartAdd::run(&cx, a, &Quiet)
        .map(|p| p.entry)
        .map_err(|e| e.to_string())
}

/// A record's thumbnail: phase 0, rendered on a worker thread (keyed by id, last change, and
/// palette).
fn thumb(r: &PatternRecord, palette: Option<&str>) -> Thumb {
    let rec = Arc::new(r.clone());
    let pal = palette.map(String::from);
    Thumb::render(
        format!("lib:{}:{}:{}", r.id, r.updated, palette.unwrap_or("")),
        Arc::new(move || {
            render_phases_in(&rec, 240, 1, pal.as_deref())
                .map(|img| thumbs::color_image(&img, thumbs::THUMB_SIDE))
        }),
    )
}

fn short_tiling(name: &str) -> String {
    let s: String = name.chars().take(22).collect();
    if s.len() < name.len() {
        format!("{s}…")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib_with_patterns() -> (Library, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("jetcarp-app-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib = Library::open(&root);
        let mut w = jetcarp_core::World::new(
            jetcarp_core::TilingSpec::rect("square", 40, 40, jetcarp_core::Boundary::Wrap),
            1,
        )
        .unwrap();
        let id = w
            .apply(jetcarp_core::Command::AddCa {
                ca: jetcarp_core::CaSpec::new("B3/S23").seed(jetcarp_core::SeedSpec::Empty),
            })
            .unwrap()
            .new_id
            .unwrap();
        // A glider, a block, and a blinker.
        let g = w.topo.lattice().unwrap().clone();
        let mut cells = vec![];
        for (x, y) in [
            (6, 5),
            (7, 6),
            (5, 7),
            (6, 7),
            (7, 7),
            (25, 25),
            (26, 25),
            (25, 26),
            (26, 26),
            (10, 30),
            (11, 30),
            (12, 30),
        ] {
            cells.push([g.index(x, y, 0) as u32, 1]);
        }
        w.apply(jetcarp_core::Command::SetCells { id, cells })
            .unwrap();
        let rep = jetcarp_script::library::catalog_world(
            &lib,
            &w,
            None,
            None,
            "test",
            jetcarp_core::pattern::DiscoveredBy::User,
        )
        .unwrap();
        assert_eq!(rep.added.len(), 3);
        (lib, root)
    }

    #[test]
    fn filter_sort_edit_promote() {
        let (lib, root) = lib_with_patterns();
        let recs = lib.list().unwrap();
        let all = view(&recs, &Filters::default(), Sort::Score, true);
        assert_eq!(all.len(), 3);
        assert_eq!(recs[all[0]].class, PatternClass::Spaceship);
        let f = Filters {
            class: Some(PatternClass::Oscillator),
            ..Default::default()
        };
        let v = view(&recs, &f, Sort::Score, true);
        assert_eq!(v.len(), 1);
        assert_eq!(recs[v[0]].period, 2);
        let f = Filters {
            min_period: 2,
            ..Default::default()
        };
        assert_eq!(view(&recs, &f, Sort::Period, false).len(), 2);
        // Edit: name, tags, state, rating, notes.
        let ship = &recs[all[0]];
        let mut e = Edit::of(ship);
        e.name = "my glider".into();
        e.tags = "a, b".into();
        e.state = CurationState::Reviewed;
        e.rating = 4;
        e.notes = "nice".into();
        let r = lib.update(&ship.id, &e.update()).unwrap();
        assert_eq!(r.name.as_deref(), Some("my glider"));
        assert_eq!(r.tags, vec!["a", "b"]);
        assert_eq!((r.state, r.rating), (CurationState::Reviewed, Some(4)));
        // Promote: the scene appears in the Quick Start's finds.
        assert_eq!(promote(&lib, ship).unwrap(), format!("find:{}", ship.id));
        let finds = lib
            .quickstart(&[jetcarp_store::QsSource::Promotion])
            .unwrap();
        assert_eq!(finds.len(), 1);
        assert_eq!(lib.get(&ship.id).unwrap().state, CurationState::Promoted);
        // Thumbnails render.
        let t = thumb(&recs[all[0]], Some("Paper"));
        let thumbs::Loader::Render(f) = &t.load else {
            panic!()
        };
        assert!(f().is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
