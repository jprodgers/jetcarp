//! Browse a sweep folder: a thumbnail grid read from its manifest, sorted and filtered by the
//! manifest's columns, with favourites (`favourites.json` in the folder). Clicking a result
//! rebuilds that run from `run.json` and opens it in the main view.
//!
//! The model (loading, filtering, sorting, favourites) is plain Rust and tested below.

use crate::sweep_form::thousands;
use crate::thumbs::{self, ThumbCache};
use jetcarp_core::{Command, World};
use eframe::egui::{self, Color32, RichText};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "results";

pub use jetcarp_script::results::{
    Favourites, Manifest, Row, num, promote_run, subtitle, sweep_folders, text,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeriodFilter {
    Any,
    Has,
    None,
}

/// Row classes in the manifest (`row_class`), plus rows without one.
pub const CLASSES: [&str; 6] = [
    "empty",
    "uniform",
    "periodic",
    "shifting",
    "aperiodic",
    "(none)",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub search: String,
    pub classes: [bool; 6],
    pub period: PeriodFilter,
    /// Inclusive density range.
    pub density: (f64, f64),
    /// PNG size range in KB (`max` 0 = no upper limit).
    pub png_kb: (f64, f64),
    pub favourites_only: bool,
    pub images_only: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            search: String::new(),
            classes: [true; 6],
            period: PeriodFilter::Any,
            density: (0.0, 1.0),
            png_kb: (0.0, 0.0),
            favourites_only: false,
            images_only: false,
        }
    }
}

/// A search: series numbers (`8229`, `30, 90, 110`, `100-200`) or text in the rule.
pub fn matches_search(r: &Row, q: &str) -> bool {
    let q = q.trim();
    if q.is_empty() {
        return true;
    }
    let n = r.get("n").and_then(|v| v.as_u64());
    let rule = text(r, "rule").to_ascii_lowercase();
    q.split([',', ' ']).filter(|p| !p.is_empty()).any(|part| {
        if let Ok(k) = part.parse::<u64>() {
            return n == Some(k)
                || (n.is_none() && r.get("index").and_then(|v| v.as_u64()) == Some(k));
        }
        if let Some((a, b)) = part.split_once('-')
            && let (Ok(a), Ok(b)) = (a.trim().parse::<u64>(), b.trim().parse::<u64>())
        {
            return n.is_some_and(|n| n >= a && n <= b);
        }
        rule.contains(&part.to_ascii_lowercase())
    })
}

impl Filter {
    pub fn accepts(&self, r: &Row, fav: bool) -> bool {
        if self.favourites_only && !fav {
            return false;
        }
        if self.images_only && text(r, "file").is_empty() {
            return false;
        }
        let class = match r.get("row_class").and_then(|c| c.as_str()) {
            Some(c) => CLASSES
                .iter()
                .position(|x| *x == c)
                .unwrap_or(CLASSES.len() - 1),
            None => CLASSES.len() - 1,
        };
        if !self.classes[class] {
            return false;
        }
        let has_period = r.get("period").is_some_and(|p| !p.is_null());
        match self.period {
            PeriodFilter::Has if !has_period => return false,
            PeriodFilter::None if has_period => return false,
            _ => {}
        }
        if let Some(d) = num(r, "density")
            && (d < self.density.0 - 1e-12 || d > self.density.1 + 1e-12)
        {
            return false;
        }
        if self.png_kb != (0.0, 0.0)
            && let Some(b) = num(r, "png_bytes")
        {
            let kb = b / 1024.0;
            if kb < self.png_kb.0 || (self.png_kb.1 > 0.0 && kb > self.png_kb.1) {
                return false;
            }
        }
        matches_search(r, &self.search)
    }
}

/// Rows to show, in order. Missing values sort last either way.
pub fn view(
    m: &Manifest,
    f: &Filter,
    sort: &str,
    descending: bool,
    favs: &Favourites,
) -> Vec<usize> {
    let mut v: Vec<usize> = (0..m.rows.len())
        .filter(|&i| f.accepts(&m.rows[i], favs.has(&m.rows[i])))
        .collect();
    let key = |i: usize| -> (u8, f64, String) {
        let r = &m.rows[i];
        match r.get(sort) {
            Some(Value::Number(x)) => (0, x.as_f64().unwrap_or(0.0), String::new()),
            Some(Value::String(s)) => (0, 0.0, s.clone()),
            Some(Value::Bool(b)) => (0, *b as u8 as f64, String::new()),
            _ => (1, 0.0, String::new()),
        }
    };
    v.sort_by(|&a, &b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0)
            .then_with(|| {
                let o =
                    ka.1.partial_cmp(&kb.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| ka.2.cmp(&kb.2));
                if descending { o.reverse() } else { o }
            })
            .then(a.cmp(&b))
    });
    v
}

// ---------------------------------------------------------------------------------------------
// The browser window

/// What the browser asks the app to do.
pub enum Action {
    /// Show this world in the main view.
    Open {
        world: Box<World>,
        title: String,
        desc: String,
    },
    Message(String, bool),
    /// Patterns were added to the library.
    LibraryChanged,
    /// A scene was added to the Quick Start's "My finds".
    QuickStartChanged,
}

type Loaded = Result<(World, String, String), String>;

pub struct ResultsBrowser {
    pub open: bool,
    manifest: Option<Manifest>,
    error: Option<String>,
    favs: Favourites,
    filter: Filter,
    sort: String,
    descending: bool,
    order: Vec<usize>,
    dirty: bool,
    cache: ThumbCache,
    cell_w: f32,
    /// Selected row (manifest position).
    selected: Option<usize>,
    scroll_to: Option<usize>,
    recent: Vec<PathBuf>,
    recent_at: Option<Instant>,
    last_reload: Instant,
    loading: Option<(Receiver<Loaded>, String)>,
    /// Adding favourites to the library (a background job): its report.
    cataloging: Option<Receiver<(String, bool)>>,
}

impl Default for ResultsBrowser {
    fn default() -> Self {
        ResultsBrowser {
            open: false,
            manifest: None,
            error: None,
            favs: Favourites::default(),
            filter: Filter::default(),
            sort: "index".into(),
            descending: false,
            order: vec![],
            dirty: true,
            cache: ThumbCache::new(4),
            cell_w: 200.0,
            selected: None,
            scroll_to: None,
            recent: vec![],
            recent_at: None,
            last_reload: Instant::now(),
            loading: None,
            cataloging: None,
        }
    }
}

impl ResultsBrowser {
    /// Open a sweep folder (keeps the filters).
    pub fn open_folder(&mut self, folder: &Path) {
        self.open = true;
        match Manifest::load(folder) {
            Ok(m) => {
                let same = self.manifest.as_ref().is_some_and(|x| x.folder == m.folder);
                if !same {
                    self.favs = Favourites::load(folder);
                    self.selected = None;
                    self.cache.clear();
                }
                self.manifest = Some(m);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.dirty = true;
        self.last_reload = Instant::now();
        self.recent_at = None;
    }

    /// Select a row (manifest position) and open its run at the end, as a click does.
    pub fn open_row(&mut self, k: usize) {
        if self.manifest.as_ref().is_some_and(|m| k < m.rows.len()) {
            self.selected = Some(k);
            self.scroll_to = Some(k);
            self.start_load(k, true);
        }
    }

    pub fn folder(&self) -> Option<&Path> {
        self.manifest.as_ref().map(|m| m.folder.as_path())
    }

    fn start_load(&mut self, row: usize, at_end: bool) {
        let Some(m) = &self.manifest else { return };
        let r = m.rows[row].clone();
        let run = m.run.clone();
        let rule = text(&r, "rule").to_string();
        let gens = if at_end {
            num(&r, "generations").unwrap_or(0.0) as u64
        } else {
            0
        };
        let folder = m.name();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let res = jetcarp_script::run_world(&run, &rule).and_then(|mut w| {
                if gens > 0 {
                    w.apply(Command::Step { gens })?;
                }
                let mut desc = format!("From sweep {folder}, run #{}", r["index"]);
                let mut parts = vec![];
                for k in ["row_class", "period", "density", "generations"] {
                    if let Some(v) = r.get(k).filter(|v| !v.is_null()) {
                        parts.push(format!("{k} {v}"));
                    }
                }
                if !parts.is_empty() {
                    desc.push_str(&format!(" · {}", parts.join(", ")).replace('"', ""));
                }
                if let Some(h) = r.get("hash").and_then(|h| h.as_str())
                    && gens > 0
                    && h != w.hash_hex()
                {
                    desc.push_str(" · (hash differs from the manifest: JETCARP version changed?)");
                }
                Ok((w, rule.clone(), desc))
            });
            let _ = tx.send(res);
        });
        self.loading = Some((rx, text(&m.rows[row], "rule").to_string()));
    }

    /// Draw the window. `root` is the output root (for the list of sweep folders).
    pub fn show(&mut self, ctx: &egui::Context, root: &Path) -> Vec<Action> {
        let mut actions = vec![];
        if let Some(rx) = &self.cataloging {
            match rx.try_recv() {
                Ok((m, err)) => {
                    actions.push(Action::Message(m, err));
                    actions.push(Action::LibraryChanged);
                    self.cataloging = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200))
                }
                Err(_) => self.cataloging = None,
            }
        }
        if let Some((rx, _)) = &self.loading {
            match rx.try_recv() {
                Ok(Ok((world, title, desc))) => {
                    actions.push(Action::Open {
                        world: Box::new(world),
                        title,
                        desc,
                    });
                    self.loading = None;
                }
                Ok(Err(e)) => {
                    actions.push(Action::Message(
                        format!("Could not open the run: {e}"),
                        true,
                    ));
                    self.loading = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint(),
                Err(_) => self.loading = None,
            }
        }
        if !self.open {
            return actions;
        }
        // A running sweep: reload its manifest every two seconds.
        if self.manifest.as_ref().is_some_and(|m| m.partial)
            && self.last_reload.elapsed().as_secs_f64() > 2.0
            && let Some(f) = self.folder().map(Path::to_path_buf)
        {
            self.open_folder(&f);
        }
        if self.recent_at.is_none_or(|t| t.elapsed().as_secs() > 10) {
            self.recent = sweep_folders(root);
            self.recent_at = Some(Instant::now());
        }
        if self.cache.begin_frame(ctx) {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let mut open = self.open;
        let screen = ctx.content_rect();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Sweep results")
                .size(1100.0, 800.0)
                .configure(|w| {
                    w.default_pos(screen.min + egui::vec2(345.0, 60.0))
                        .default_size([
                            (screen.width() * 0.75).min(1250.0),
                            (screen.height() * 0.8).min(900.0),
                        ])
                        .min_size([360.0, 300.0])
                        .resizable(true)
                }),
            &mut open,
            |ui| self.contents(ui, root, &mut actions),
        );
        self.open = open;
        actions
    }

    fn contents(&mut self, ui: &mut egui::Ui, root: &Path, actions: &mut Vec<Action>) {
        // Folder row.
        ui.horizontal_wrapped(|ui| {
            let name = self
                .manifest
                .as_ref()
                .map(|m| m.name())
                .unwrap_or_else(|| "no folder".into());
            egui::ComboBox::from_id_salt("results-recent")
                .selected_text(RichText::new(name).strong())
                .width(330.0)
                .height(500.0)
                .show_ui(ui, |ui| {
                    if self.recent.is_empty() {
                        ui.label(RichText::new(format!("No sweeps in {}", root.display())).weak());
                    }
                    for p in self.recent.clone() {
                        let n = p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let on = self.folder() == Some(p.as_path());
                        if ui.selectable_label(on, n).clicked() {
                            self.open_folder(&p);
                        }
                    }
                })
                .response
                .on_hover_text("Sweep folders in the output folder, newest first");
            if ui.button("Open folder…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_directory(root).pick_folder()
            {
                self.open_folder(&p);
            }
            if !self.favs.items.is_empty() {
                let busy = self.cataloging.is_some();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(format!(
                            "Add {} favourites to library",
                            self.favs.items.len()
                        )),
                    )
                    .on_hover_text("Rebuild each favourite run, find its still lifes, oscillators, and spaceships, verify them, and store them in the pattern library. Row rules (W, HexW) make pictures, not objects: promote those to the Quick Start instead.")
                    .clicked()
                {
                    self.add_favourites_to_library();
                }
                if busy {
                    ui.spinner();
                }
            }
            if let Some(f) = self.folder().map(Path::to_path_buf) {
                if ui.button("Reload").clicked() {
                    self.open_folder(&f);
                }
                if ui
                    .button("Show in file manager")
                    .on_hover_text(f.display().to_string())
                    .clicked()
                {
                    open_in_file_manager(&f);
                }
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let Some(m) = &self.manifest else {
            thumbs::empty_note(
                ui,
                "Pick a sweep folder above, or run a sweep from the Scripts window.",
            );
            return;
        };
        // Summary line.
        let total = m
            .total_runs()
            .map(|t| format!(" of {}", thousands(t)))
            .unwrap_or_default();
        let images = m
            .rows
            .iter()
            .filter(|r| !text(r, "file").is_empty())
            .count();
        let mut line = format!(
            "{} runs{total}, {} images, {} favourites",
            thousands(m.rows.len() as u64),
            thousands(images as u64),
            self.favs.items.len()
        );
        if let Some(t) = m.run["tiling"].as_str() {
            line.push_str(&format!(" · {t}"));
        }
        if m.partial {
            line.push_str(" · still running or interrupted (reloading)");
        }
        ui.label(RichText::new(line).weak());
        let numeric = m.numeric_columns();
        let has = |c: &str| m.columns.iter().any(|x| x == c);
        let (has_density, has_png, has_class) =
            (has("density"), has("png_bytes"), has("row_class"));
        // Filters.
        let before = (self.filter.clone(), self.sort.clone(), self.descending);
        ui.horizontal_wrapped(|ui| {
            ui.label("Find");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter.search)
                    .hint_text("8229 · 30, 90 · 100-200 · S23")
                    .desired_width(150.0),
            )
            .on_hover_text("Rule numbers, a list, a range, or text in the rule");
            ui.separator();
            ui.label("Sort");
            egui::ComboBox::from_id_salt("results-sort")
                .selected_text(column_label(&self.sort))
                .show_ui(ui, |ui| {
                    for c in &numeric {
                        ui.selectable_value(&mut self.sort, c.clone(), column_label(c));
                    }
                    if has_class {
                        ui.selectable_value(
                            &mut self.sort,
                            "row_class".to_string(),
                            column_label("row_class"),
                        );
                    }
                    ui.selectable_value(&mut self.sort, "rule".to_string(), "rule");
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
            ui.checkbox(&mut self.filter.favourites_only, "favourites only");
            ui.checkbox(&mut self.filter.images_only, "with image");
            ui.separator();
            ui.label("Size");
            ui.add(egui::Slider::new(&mut self.cell_w, 110.0..=420.0).show_value(false));
        });
        ui.horizontal_wrapped(|ui| {
            if has_class {
                ui.label("Rows");
                for (k, c) in CLASSES.iter().enumerate() {
                    ui.checkbox(&mut self.filter.classes[k], *c)
                        .on_hover_text(match *c {
                            "empty" => "No live cells",
                            "uniform" => "At most two distinct rows",
                            "periodic" => "The bottom rows repeat (row_period)",
                            "aperiodic" => "No vertical period found in the bottom half",
                            _ => "No row class (not a lattice tiling, or an error)",
                        });
                }
                ui.separator();
            }
            ui.label("Period");
            egui::ComboBox::from_id_salt("results-period")
                .selected_text(match self.filter.period {
                    PeriodFilter::Any => "any",
                    PeriodFilter::Has => "found",
                    PeriodFilter::None => "none found",
                })
                .width(90.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter.period, PeriodFilter::Any, "any");
                    ui.selectable_value(&mut self.filter.period, PeriodFilter::Has, "found");
                    ui.selectable_value(&mut self.filter.period, PeriodFilter::None, "none found");
                })
                .response
                .on_hover_text("Whether an exact period was seen (the world repeats in time)");
            if has_density {
                ui.separator();
                ui.label("Density");
                let (lo, hi) = &mut self.filter.density;
                ui.add(
                    egui::DragValue::new(lo)
                        .range(0.0..=1.0)
                        .speed(0.005)
                        .max_decimals(3),
                );
                ui.label("to");
                ui.add(
                    egui::DragValue::new(hi)
                        .range(0.0..=1.0)
                        .speed(0.005)
                        .max_decimals(3),
                );
            }
            if has_png {
                ui.separator();
                ui.label("PNG KB");
                let (lo, hi) = &mut self.filter.png_kb;
                ui.add(
                    egui::DragValue::new(lo)
                        .range(0.0..=1e6)
                        .speed(1.0)
                        .max_decimals(0),
                );
                ui.label("to");
                ui.add(
                    egui::DragValue::new(hi)
                        .range(0.0..=1e6)
                        .speed(1.0)
                        .max_decimals(0)
                        .custom_formatter(|v, _| {
                            if v <= 0.0 {
                                "any".into()
                            } else {
                                format!("{v:.0}")
                            }
                        }),
                )
                .on_hover_text(
                    "Compressed image size: a cheap measure of how complex a picture is",
                );
            }
            if self.filter != Filter::default() && ui.small_button("Clear filters").clicked() {
                self.filter = Filter::default();
            }
        });
        if before != (self.filter.clone(), self.sort.clone(), self.descending) {
            self.dirty = true;
        }
        if self.dirty {
            self.order = view(m, &self.filter, &self.sort, self.descending, &self.favs);
            self.dirty = false;
        }
        ui.label(
            RichText::new(format!(
                "Showing {} · click a picture to open that run in the main view, click its star to mark a favourite",
                thousands(self.order.len() as u64)
            ))
            .small()
            .weak(),
        );
        ui.separator();
        // Details of the selected run, in a panel at the bottom; the grid fills the rest.
        if let Some(sel) = self.selected
            && sel < m.rows.len()
        {
            egui::Panel::bottom("results-detail")
                .resizable(false)
                .show(ui, |ui| self.detail(ui, sel, actions));
        }
        let mut res = thumbs::GridResponse::default();
        let order = self.order.clone();
        egui::CentralPanel::default().show(ui, |ui| {
            let m = self.manifest.as_ref().unwrap();
            if order.is_empty() {
                thumbs::empty_note(
                    ui,
                    if m.rows.is_empty() {
                        "No runs yet."
                    } else {
                        "Nothing matches these filters."
                    },
                );
                return;
            }
            let aspect = m.aspect();
            let scroll_to = self
                .scroll_to
                .take()
                .and_then(|r| order.iter().position(|&x| x == r));
            let favs = &self.favs;
            let selected = self.selected;
            res = thumbs::grid(
                ui,
                &mut self.cache,
                thumbs::GridSpec {
                    id: "results-grid",
                    n: order.len(),
                    cell_w: self.cell_w,
                    aspect,
                    scroll_to,
                },
                &mut |k| {
                    let r = &m.rows[order[k]];
                    thumbs::Cell {
                        image: m.image(r).map(thumbs::Thumb::png),
                        placeholder: match r.get("skipped").and_then(|s| s.as_str()) {
                            Some(s) => format!("skipped: {s}"),
                            None if r.get("error").is_some_and(|e| !e.is_null()) => "error".into(),
                            None => "no image".into(),
                        },
                        title: text(r, "rule").to_string(),
                        subtitle: subtitle(r),
                        star: Some(favs.has(r)),
                        selected: selected == Some(order[k]),
                    }
                },
                &mut |ui, k| {
                    let r = &m.rows[order[k]];
                    row_details(ui, r, 2);
                },
            );
        });
        if let Some(k) = res.star_toggled {
            let r = self.manifest.as_ref().unwrap().rows[order[k]].clone();
            let on = self.favs.toggle(&r);
            if let Err(e) = self.favs.save(&self.manifest.as_ref().unwrap().folder) {
                actions.push(Action::Message(e, true));
            }
            if self.filter.favourites_only && !on {
                self.dirty = true;
            }
        }
        if let Some(k) = res.clicked.or(res.double_clicked) {
            self.selected = Some(order[k]);
            self.start_load(order[k], true);
        }
    }

    /// The selected run: favourite, open at the end or the start, its columns.
    fn detail(&mut self, ui: &mut egui::Ui, sel: usize, actions: &mut Vec<Action>) {
        let r = self.manifest.as_ref().unwrap().rows[sel].clone();
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(text(&r, "rule")).strong().size(15.0));
            let fav = self.favs.has(&r);
            if thumbs::star_button(
                ui,
                fav,
                if fav {
                    "Favourite"
                } else {
                    "Add to favourites"
                },
            )
            .on_hover_text("Saved in favourites.json in the sweep folder")
            .clicked()
            {
                self.favs.toggle(&r);
                if let Some(m) = &self.manifest
                    && let Err(e) = self.favs.save(&m.folder)
                {
                    actions.push(Action::Message(e, true));
                }
                self.dirty |= self.filter.favourites_only;
            }
            let gens = num(&r, "generations").unwrap_or(0.0) as u64;
            if ui
                .button(format!("Open at the end (generation {gens})"))
                .on_hover_text("The state in the picture")
                .clicked()
            {
                self.start_load(sel, true);
            }
            if ui
                .button("Open at the start")
                .on_hover_text("Generation 0, to watch it run")
                .clicked()
            {
                self.start_load(sel, false);
            }
            if ui
                .add_enabled(self.favs.has(&r), egui::Button::new("Promote to Quick Start"))
                .on_hover_text("Favourites only: add this run (tiling, rule, start) to the Quick Start under My finds")
                .clicked()
            {
                match self.promote_row(sel) {
                    Ok(id) => {
                        actions.push(Action::QuickStartChanged);
                        actions.push(Action::Message(
                            format!(
                                "Added {} to the Quick Start (find:{id})",
                                text(&r, "rule"),
                            ),
                            false,
                        ));
                    }
                    Err(e) => actions.push(Action::Message(e, true)),
                }
            }
            if ui.button("Copy rule").clicked() {
                ui.ctx().copy_text(text(&r, "rule").to_string());
            }
            if let Some((_, what)) = &self.loading {
                ui.spinner();
                ui.label(RichText::new(format!("opening {what}…")).weak());
            }
        });
        let roomy = ui.ctx().content_rect().height() > 760.0;
        egui::CollapsingHeader::new(RichText::new("All columns").small())
            .id_salt("results-detail-columns")
            .default_open(roomy)
            .show(ui, |ui| row_details(ui, &r, 4));
        ui.add_space(2.0);
    }
}

impl ResultsBrowser {
    /// Catalog the objects of every favourite run into the pattern library (background).
    fn add_favourites_to_library(&mut self) {
        let Some(m) = &self.manifest else { return };
        let rows: Vec<Row> = m
            .rows
            .iter()
            .filter(|r| self.favs.has(r))
            .cloned()
            .collect();
        let run = m.run.clone();
        let folder = m.name();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let lib = jetcarp_core::library::Library::open_default();
            let _ = tx.send(catalog_rows(&lib, &run, &folder, &rows));
        });
        self.cataloging = Some(rx);
    }

    /// Add a Quick Start entry ("My finds") for a run. Returns its id.
    fn promote_row(&mut self, row: usize) -> Result<String, String> {
        let m = self.manifest.as_ref().ok_or("no sweep open")?;
        let lib = jetcarp_core::library::Library::open_default();
        let id = promote_run(m, row, &lib)?;
        let r = &m.rows[row];
        // Note it on the favourite.
        if let Some(i) = r.get("index").and_then(|i| i.as_u64())
            && let Some(f) = self.favs.items.get_mut(&i)
        {
            f["promoted"] = json!(format!("find:{id}"));
            let folder = m.folder.clone();
            self.favs.save(&folder)?;
        }
        Ok(id)
    }
}

/// Rebuild each run, find its objects, verify and store them in `lib`. Returns a one-line report
/// and whether it failed.
pub fn catalog_rows(
    lib: &jetcarp_core::library::Library,
    run: &Value,
    folder: &str,
    rows: &[Row],
) -> (String, bool) {
    let (mut objects, mut added, mut merged, mut none) = (0, 0, 0, vec![]);
    let mut errors = vec![];
    for r in rows {
        let rule = text(r, "rule").to_string();
        let res = jetcarp_script::run_world(run, &rule).and_then(|mut w| {
            let g = num(r, "generations").unwrap_or(0.0) as u64;
            if g > 0 {
                w.apply(Command::Step { gens: g })?;
            }
            jetcarp_script::library::catalog_world(
                lib,
                &w,
                None,
                None,
                &format!("sweep {folder} run #{} ({rule})", r["index"]),
                jetcarp_core::library::DiscoveredBy::User,
            )
        });
        match res {
            Ok(rep) => {
                objects += rep.objects;
                added += rep.added.len();
                merged += rep.merged.len();
                if rep.objects == 0 {
                    none.push(rule);
                }
                errors.extend(rep.failed.into_iter().map(|(id, e)| format!("{id}: {e}")));
            }
            Err(e) => errors.push(format!("{rule}: {e}")),
        }
    }
    let mut msg = format!(
        "Library: {} favourites, {objects} objects, {added} new patterns, {merged} already known",
        rows.len()
    );
    if !none.is_empty() {
        msg.push_str(&format!("; no objects in {}", none.join(", ")));
    }
    if !errors.is_empty() {
        msg.push_str(&format!(
            "; {} could not be cataloged ({})",
            errors.len(),
            errors[0]
        ));
    }
    (msg, added == 0 && merged == 0 && !errors.is_empty())
}

fn column_label(c: &str) -> String {
    match c {
        "index" => "run order".into(),
        "n" => "rule number".into(),
        "png_bytes" => "PNG size (complexity)".into(),
        "row_period" => "row period".into(),
        "distinct_rows" => "distinct rows".into(),
        "periodic_from" => "periodic from row".into(),
        "row_class" => "row class".into(),
        c => c.replace('_', " "),
    }
}

/// The row's columns as a small grid.
fn row_details(ui: &mut egui::Ui, r: &Row, cols: usize) {
    let skip = ["rule", "file", "kept", "index"];
    let items: Vec<(String, String)> = r
        .iter()
        .filter(|(k, v)| !skip.contains(&k.as_str()) && !v.is_null())
        .map(|(k, v)| {
            let v = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) if n.is_f64() => format!("{:.4}", n.as_f64().unwrap())
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string(),
                v => v.to_string(),
            };
            (column_label(k), v)
        })
        .collect();
    egui::Grid::new(ui.next_auto_id())
        .num_columns(cols * 2)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for (i, (k, v)) in items.iter().enumerate() {
                ui.label(RichText::new(k).small().weak());
                ui.label(RichText::new(v).small().monospace());
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
}

/// Open a folder in the system's file manager.
pub fn open_in_file_manager(p: &Path) {
    let cmd = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(i: u64, class: &str, density: f64, period: Option<u64>, png: u64) -> Row {
        let mut r = json!({
            "index": i, "n": i, "rule": format!("W{i}"), "file": format!("W{i:03}.png"),
            "density": density, "row_class": class, "png_bytes": png,
            "period": period,
        })
        .as_object()
        .unwrap()
        .clone();
        if class == "uniform" {
            r.insert("file".into(), json!(""));
            r.insert("skipped".into(), json!("uniform"));
        }
        r
    }

    fn manifest() -> Manifest {
        Manifest {
            folder: PathBuf::from("/tmp/x"),
            columns: vec![],
            rows: vec![
                row(0, "uniform", 0.0, Some(1), 100),
                row(30, "aperiodic", 0.5, None, 9000),
                row(90, "periodic", 0.25, Some(1), 3000),
                row(110, "aperiodic", 0.6, None, 7000),
            ],
            run: Value::Null,
            partial: false,
        }
    }

    #[test]
    fn filter_and_sort() {
        let m = manifest();
        let favs = Favourites::default();
        let mut f = Filter::default();
        assert_eq!(view(&m, &f, "index", false, &favs), vec![0, 1, 2, 3]);
        assert_eq!(view(&m, &f, "png_bytes", true, &favs), vec![1, 3, 2, 0]);
        assert_eq!(view(&m, &f, "density", false, &favs), vec![0, 2, 1, 3]);
        // Missing values sort last both ways.
        assert_eq!(view(&m, &f, "period", false, &favs), vec![0, 2, 1, 3]);
        assert_eq!(view(&m, &f, "period", true, &favs), vec![0, 2, 1, 3]);
        f.classes[1] = false; // no uniform
        assert_eq!(view(&m, &f, "index", false, &favs), vec![1, 2, 3]);
        f.period = PeriodFilter::None;
        assert_eq!(view(&m, &f, "index", false, &favs), vec![1, 3]);
        f.density = (0.55, 1.0);
        assert_eq!(view(&m, &f, "index", false, &favs), vec![3]);
        let mut f = Filter {
            png_kb: (4.0, 0.0),
            ..Default::default()
        };
        assert_eq!(view(&m, &f, "index", false, &favs), vec![1, 3]);
        f.png_kb = (0.0, 5.0);
        assert_eq!(view(&m, &f, "index", false, &favs), vec![0, 2]);
        let f = Filter {
            images_only: true,
            ..Default::default()
        };
        assert_eq!(view(&m, &f, "index", false, &favs), vec![1, 2, 3]);
    }

    #[test]
    fn search() {
        let m = manifest();
        let favs = Favourites::default();
        let q = |s: &str| {
            let f = Filter {
                search: s.into(),
                ..Default::default()
            };
            view(&m, &f, "index", false, &favs)
        };
        assert_eq!(q("30"), vec![1]);
        assert_eq!(q("30, 110"), vec![1, 3]);
        assert_eq!(q("20-100"), vec![1, 2]);
        assert_eq!(q("w9"), vec![2]);
        assert_eq!(q(""), vec![0, 1, 2, 3]);
    }

    #[test]
    fn favourites_round_trip() {
        let m = manifest();
        let dir = std::env::temp_dir().join(format!("jetcarp-favs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut favs = Favourites::default();
        assert!(favs.toggle(&m.rows[1]));
        assert!(favs.toggle(&m.rows[3]));
        assert!(!favs.toggle(&m.rows[3]));
        favs.save(&dir).unwrap();
        let back = Favourites::load(&dir);
        assert_eq!(back.items.len(), 1);
        assert!(back.has(&m.rows[1]) && !back.has(&m.rows[3]));
        assert_eq!(back.items[&30]["rule"], "W30");
        let f = Filter {
            favourites_only: true,
            ..Default::default()
        };
        assert_eq!(view(&m, &f, "index", false, &back), vec![1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_a_real_sweep_and_reopens_a_run() {
        let root = std::env::temp_dir().join(format!("jetcarp-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = jetcarp_script::run(
            r#"sweep(#{ rules: "W", from: 28, to: 33, tiling: "square", size: [31, 20],
                        boundary: "dead", seed: "top_center", until: "done",
                        png: #{ size: [62, 40] }, skip: ["uniform"] });"#,
            jetcarp_script::RunOptions {
                out_root: Some(root.clone()),
                ..Default::default()
            },
            |_| {},
            Default::default(),
        );
        assert!(s.ok, "{:?}", s.error);
        let dirs = sweep_folders(&root);
        assert_eq!(dirs.len(), 1);
        let m = Manifest::load(&dirs[0]).unwrap();
        assert_eq!(m.rows.len(), 6);
        assert!(!m.partial);
        assert!((m.aspect() - 40.0 / 62.0).abs() < 1e-6);
        assert!(m.numeric_columns().contains(&"png_bytes".to_string()));
        let r = &m.rows[2];
        assert_eq!(text(r, "rule"), "W30");
        assert!(m.image(r).unwrap().exists());
        let mut w = jetcarp_script::run_world(&m.run, "W30").unwrap();
        w.apply(Command::Step {
            gens: num(r, "generations").unwrap() as u64,
        })
        .unwrap();
        assert_eq!(text(r, "hash"), w.hash_hex());
        // Promote the run: a Quick Start entry the finds list reads, which runs to the same state.
        let lib = jetcarp_core::library::Library::open(root.join("lib"));
        let id = promote_run(&m, 2, &lib).unwrap();
        let finds = jetcarp_core::quickstart::user_finds(&lib);
        assert_eq!(finds.len(), 1);
        assert_eq!(finds[0].rule, "W30");
        let mut again = World::from_scene(&finds[0].scene).unwrap();
        again
            .apply(Command::Step {
                gens: num(r, "generations").unwrap() as u64,
            })
            .unwrap();
        assert_eq!(text(r, "hash"), again.hash_hex());
        assert_eq!(finds[0].id, format!("find:{id}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn favourites_go_to_the_library() {
        let root = std::env::temp_dir().join(format!("jetcarp-results-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = jetcarp_script::run(
            r#"sweep(#{ rules: ["B3/S23", "W30"], tiling: "square", size: [48, 48],
                        boundary: "wrap", seed: "random:0.35", until: "repeats",
                        max_period: 30, max_gens: 3000, png: false });"#,
            jetcarp_script::RunOptions {
                out_root: Some(root.clone()),
                ..Default::default()
            },
            |_| {},
            Default::default(),
        );
        assert!(s.ok, "{:?}", s.error);
        let m = Manifest::load(&sweep_folders(&root)[0]).unwrap();
        let lib = jetcarp_core::library::Library::open(root.join("lib"));
        let (msg, failed) = catalog_rows(&lib, &m.run, &m.name(), &m.rows);
        assert!(!failed, "{msg}");
        // Life leaves objects; W30 makes none (a row rule), and the report says so.
        assert!(!lib.list().unwrap().is_empty(), "{msg}");
        assert!(msg.contains("W30"), "{msg}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
