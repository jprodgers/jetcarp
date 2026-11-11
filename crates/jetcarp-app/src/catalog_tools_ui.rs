//! The *Catalog tools…* window: the catalog actions that were CLI- or script-only before
//! (Phase 3 reachability gaps). Collide two patterns, export and import JSON records, remove a
//! record, and run the evolutionary glider search.
//!
//! Collisions and evolve searches are catalog jobs: *Collide* and *Evolve* call the operation
//! `search.start` (strategies `collide` and `evolve`, in the background), the evolve search's
//! *Stop* calls `jobs.stop` and *Resume* `jobs.resume` (`crate::window_job`), so they are in the
//! process's job registry, where the dashboard and `jobs.status` see them. The window draws
//! their live progress (placements run, then products verified; each generation's best), and at
//! the end what their progress lists: a collision's outcomes and products, an evolve search's
//! champions, verified movers, and folder.

use crate::window_job::{Hooks, WindowJob};
use eframe::egui::{self, Color32, RichText};
use jetcarp_api::ops::jobs::ResumeIn;
use jetcarp_api::ops::search::SearchStartIn;
use jetcarp_core::library::{AddOutcome, Library};
use jetcarp_core::pattern;
use jetcarp_core::world::Scene;
use serde_json::{Value, json};
use std::path::PathBuf;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "catalog-tools";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Collide,
    Transfer,
    Remove,
    Evolve,
}

impl Tab {
    pub fn parse(s: &str) -> Tab {
        match s {
            "export" | "import" | "transfer" => Tab::Transfer,
            "remove" => Tab::Remove,
            "evolve" => Tab::Evolve,
            _ => Tab::Collide,
        }
    }
}

/// What the window asks the app to do.
pub enum Action {
    Open(Box<Scene>),
    /// Records were added or removed: the Library window should reload.
    LibraryChanged,
    Message(String, bool),
}

/// A finished collision, as its job's progress lists it.
#[derive(Clone)]
struct CollideDone {
    a: String,
    b: String,
    rule: String,
    counts: Vec<(String, usize)>,
    /// Products: id, label, new to the catalog (the first the progress lists).
    products: Vec<(String, String, bool)>,
    /// Products beyond those.
    more: usize,
}

impl CollideDone {
    fn of(p: &Value) -> CollideDone {
        let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let listed = p["found"].as_array().map_or(0, |a| a.len());
        CollideDone {
            more: (p["products"].as_u64().unwrap_or(0) as usize).saturating_sub(listed),
            a: text("a"),
            b: text("b"),
            rule: text("rule"),
            counts: p["outcomes"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0) as usize))
                        .collect()
                })
                .unwrap_or_default(),
            products: p["found"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| {
                    (
                        x["id"].as_str().unwrap_or_default().to_string(),
                        x["label"].as_str().unwrap_or_default().to_string(),
                        x["new"].as_bool().unwrap_or(false),
                    )
                })
                .collect(),
        }
    }
}

/// What a finished evolve search's progress lists: its best champions, the verified movers (or
/// that there were none), and its folder.
fn evolve_lines(p: &Value) -> Vec<String> {
    let mut lines: Vec<String> = p["champions"]
        .as_array()
        .into_iter()
        .flatten()
        .take(5)
        .map(|c| {
            format!(
                "{:.2}  {}",
                c["fitness"].as_f64().unwrap_or(0.0),
                c["rule"].as_str().unwrap_or("")
            )
        })
        .collect();
    let movers = p["movers"].as_array().cloned().unwrap_or_default();
    lines.extend(movers.iter().map(|m| {
        format!(
            "{}: {} ({} new)",
            m["kind"].as_str().unwrap_or("mover"),
            m["summary"].as_str().unwrap_or(""),
            m["added"].as_array().map_or(0, |a| a.len())
        )
    }));
    if movers.is_empty() {
        lines.push(format!(
            "{} generations, no verified glider",
            p["generation"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(f) = p["folder"].as_str().filter(|f| !f.is_empty()) {
        lines.push(format!("Files in {f}"));
    }
    lines
}

/// The end of a job a tab started: its last progress, or why it failed.
fn ended(j: &WindowJob) -> Result<Value, String> {
    let p = j.progress();
    match (j.error(), p["status"].as_str()) {
        (Some(e), _) => Err(e),
        (None, Some("failed")) => Err("the job failed".into()),
        _ => Ok(p),
    }
}

pub struct CatalogTools {
    pub open: bool,
    pub tab: Tab,
    lib: Library,
    catalog: PathBuf,
    // Collide
    pub a: String,
    pub b: String,
    offsets: i32,
    gens: u64,
    store: bool,
    collide_job: Option<WindowJob>,
    collide_last: Option<Result<CollideDone, String>>,
    // Export and import
    export_dir: String,
    export_ids: String,
    import_path: String,
    transfer_msg: Option<(String, bool)>,
    // Remove
    pub remove_id: String,
    confirm: bool,
    // Evolve
    ev_tiling: String,
    ev_states: u8,
    ev_pop: usize,
    ev_gens: usize,
    ev_radius: u32,
    ev_seed: u64,
    /// The evolve search started or resumed here (kept once stopped, for *Resume*).
    evolve: Option<WindowJob>,
    evolve_last: Option<Result<Vec<String>, String>>,
    pub autorun: bool,
}

impl Default for CatalogTools {
    fn default() -> Self {
        let out = jetcarp_api::out::out_root(None);
        CatalogTools {
            open: false,
            tab: Tab::Collide,
            lib: Library::open_default(),
            catalog: Library::default_path(),
            a: "block".into(),
            b: "glider".into(),
            offsets: 4,
            gens: 1024,
            store: true,
            collide_job: None,
            collide_last: None,
            export_dir: out.join("export").display().to_string(),
            export_ids: String::new(),
            import_path: String::new(),
            transfer_msg: None,
            remove_id: String::new(),
            confirm: false,
            ev_tiling: "spectre".into(),
            ev_states: 4,
            ev_pop: 32,
            ev_gens: 40,
            ev_radius: 16,
            ev_seed: 1,
            evolve: None,
            evolve_last: None,
            autorun: false,
        }
    }
}

impl CatalogTools {
    /// `JETCARP_CATALOG_TOOLS=collide|export|remove|evolve[:run]` (headless checks).
    pub fn open_env(&mut self, s: &str) {
        let (tab, run) = s.split_once(':').unwrap_or((s, ""));
        self.tab = Tab::parse(tab);
        self.open = true;
        self.autorun = run == "run";
    }

    fn busy(&self) -> bool {
        self.collide_job.is_some() || self.evolving()
    }

    /// The evolve search started or resumed here is still at work.
    fn evolving(&self) -> bool {
        self.evolve.as_ref().is_some_and(|j| j.busy())
    }

    /// *Collide*: the collision as a job (`search.start`, strategy `collide`).
    fn start_collide(&mut self) {
        let a = SearchStartIn {
            strategy: Some("collide".into()),
            spec: json!({
                "a": self.a.trim(),
                "b": self.b.trim(),
                "offsets": self.offsets,
                "gens": self.gens,
                "store": self.store,
            }),
            ..Default::default()
        };
        self.collide_last = None;
        self.collide_job = Some(WindowJob::start(&self.catalog, a, Hooks::default()));
    }

    /// *Evolve*: the search as a job (`search.start`, strategy `evolve`), its files in a new
    /// `evolve-<tiling>` folder in the output folder.
    fn start_evolve(&mut self) {
        if let Err(e) = pattern::tiling_from_key(&self.ev_tiling) {
            self.evolve_last = Some(Err(e));
            return;
        }
        let a = SearchStartIn {
            strategy: Some("evolve".into()),
            spec: json!({
                "tiling": self.ev_tiling.trim(),
                "states": self.ev_states,
                "pop": self.ev_pop,
                "gens": self.ev_gens,
                "radius": self.ev_radius,
                "seed": self.ev_seed,
            }),
            ..Default::default()
        };
        self.evolve_last = None;
        self.evolve = Some(WindowJob::start(&self.catalog, a, Hooks::default()));
    }

    /// *Resume*: carry the stopped evolve search on from its last generation (`jobs.resume`).
    fn resume_evolve(&mut self, job: i64) {
        let a = ResumeIn {
            job,
            ..Default::default()
        };
        self.evolve_last = None;
        self.evolve = Some(WindowJob::resume(&self.catalog, a, Hooks::default()));
    }

    /// The stopped evolve search that *Resume* carries on.
    fn evolve_resumable(&self) -> Option<i64> {
        self.evolve
            .as_ref()
            .filter(|j| !j.busy() && j.status() == "paused")
            .and_then(|j| j.job())
    }

    /// Collect finished jobs.
    fn poll(&mut self, out: &mut Vec<Action>) {
        if let Some(j) = self.collide_job.as_ref().filter(|j| !j.busy()) {
            let r = ended(j).map(|p| CollideDone::of(&p));
            if r.as_ref().is_ok_and(|d| d.products.iter().any(|p| p.2)) {
                out.push(Action::LibraryChanged);
            }
            self.collide_last = Some(r);
            self.collide_job = None;
        }
        if self.evolve_last.is_none()
            && let Some(j) = self.evolve.as_ref().filter(|j| !j.busy())
        {
            match ended(j) {
                Ok(p) if p["status"] == "done" => {
                    out.push(Action::LibraryChanged);
                    self.evolve_last = Some(Ok(evolve_lines(&p)));
                }
                Ok(p) => {
                    self.evolve_last = Some(Ok(vec![format!(
                        "Stopped after generation {} of {}. Resume carries it on from there.",
                        p["generation"].as_u64().unwrap_or(0),
                        p["generations"].as_u64().unwrap_or(self.ev_gens as u64)
                    )]));
                }
                Err(e) => self.evolve_last = Some(Err(e)),
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, selected: Option<String>) -> Vec<Action> {
        let mut out = vec![];
        self.poll(&mut out);
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if !self.open {
            return out;
        }
        if self.autorun && !self.busy() {
            self.autorun = false;
            match self.tab {
                Tab::Collide => self.start_collide(),
                Tab::Evolve => self.start_evolve(),
                _ => {}
            }
        }
        let mut open = true;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Catalog tools")
                .size(520.0, 480.0)
                .configure(|w| w.default_width(520.0)),
            &mut open,
            |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Collide, "Collide");
                    ui.selectable_value(&mut self.tab, Tab::Transfer, "Export / import");
                    ui.selectable_value(&mut self.tab, Tab::Remove, "Remove");
                    ui.selectable_value(&mut self.tab, Tab::Evolve, "Evolve");
                });
                ui.separator();
                match self.tab {
                    Tab::Collide => self.collide_ui(ui, &selected, &mut out),
                    Tab::Transfer => self.transfer_ui(ui, &mut out),
                    Tab::Remove => self.remove_ui(ui, &selected, &mut out),
                    Tab::Evolve => self.evolve_ui(ui),
                }
            },
        );
        self.open = open;
        out
    }

    fn collide_ui(&mut self, ui: &mut egui::Ui, selected: &Option<String>, out: &mut Vec<Action>) {
        ui.label(
            RichText::new(
                "Collide pattern B into pattern A at every offset and every phase of B, in a rule \
                 both work in; new objects are verified and stored (as `jetcarp search collide`).",
            )
            .small()
            .weak(),
        );
        let busy = self.collide_job.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("collide-form")
                .num_columns(3)
                .show(ui, |ui| {
                    for (label, v) in [("A (target)", &mut self.a), ("B (moved)", &mut self.b)] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(v).desired_width(200.0))
                            .on_hover_text("Pattern id, prefix, or name (glider, block, …)");
                        if let Some(s) = selected
                            && ui.small_button("use selected").clicked()
                        {
                            *v = s.clone();
                        }
                        ui.end_row();
                    }
                    ui.label("Offsets");
                    ui.add(egui::Slider::new(&mut self.offsets, 0..=20).text("± cells"));
                    ui.end_row();
                    ui.label("Generations");
                    ui.add(egui::DragValue::new(&mut self.gens).range(16..=8192));
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut self.store, "Store new objects in the catalog");
                    ui.end_row();
                });
            if ui.button("Collide").clicked() {
                self.start_collide();
            }
        });
        if let Some(j) = &self.collide_job {
            let p = j.progress();
            let n = |k: &str| p[k].as_u64().unwrap_or(0);
            let (d, t) = (n("runs_done"), n("runs"));
            let (frac, text) = match p.get("verified") {
                Some(v) if t > 0 && d == t => {
                    let (v, all) = (v.as_u64().unwrap_or(0), n("products"));
                    (
                        if all > 0 { v as f32 / all as f32 } else { 1.0 },
                        format!("{t} placements run; verifying {v} of {all} products"),
                    )
                }
                _ => (
                    if t > 0 { d as f32 / t as f32 } else { 0.0 },
                    format!("{d} of {t} placements"),
                ),
            };
            ui.add(egui::ProgressBar::new(frac).text(text));
        }
        match &self.collide_last {
            Some(Ok(d)) => {
                ui.separator();
                ui.label(format!("{} hit by {} in {}", d.a, d.b, d.rule));
                egui::Grid::new("collide-counts")
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, v) in &d.counts {
                            ui.label(k.replace('_', " "));
                            ui.label(v.to_string());
                            ui.end_row();
                        }
                    });
                if !d.products.is_empty() {
                    ui.label(RichText::new("Products").strong());
                }
                let mut open_id = None;
                for (id, label, new) in &d.products {
                    ui.horizontal(|ui| {
                        if ui.small_button("Open").clicked() {
                            open_id = Some(id.clone());
                        }
                        ui.label(format!("{label} ({})", &id[..id.len().min(10)]));
                        if *new {
                            ui.colored_label(Color32::LIGHT_GREEN, "new");
                        }
                    });
                }
                if d.more > 0 {
                    ui.label(RichText::new(format!("and {} more", d.more)).weak());
                }
                if let Some(id) = open_id {
                    match self
                        .lib
                        .get(&id)
                        .and_then(|r| pattern::promote::showcase_scene(&r, None, None))
                    {
                        Ok(sc) => out.push(Action::Open(Box::new(sc))),
                        Err(e) => out.push(Action::Message(e, true)),
                    }
                }
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            None => {}
        }
    }

    fn transfer_ui(&mut self, ui: &mut egui::Ui, out: &mut Vec<Action>) {
        ui.label(
            RichText::new(
                "JSON stays the exchange format: export writes one <id>.json per record; import \
                 reads record files, a folder of them, another catalog file, or a Phase 3 library \
                 folder, and verifies every record again (as `jetcarp library export` / `import`).",
            )
            .small()
            .weak(),
        );
        egui::Grid::new("transfer-form")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Export to");
                ui.add(egui::TextEdit::singleline(&mut self.export_dir).desired_width(320.0));
                ui.end_row();
                ui.label("Records");
                ui.add(
                    egui::TextEdit::singleline(&mut self.export_ids)
                        .hint_text("ids or names, comma-separated (empty: all)")
                        .desired_width(320.0),
                );
                ui.end_row();
            });
        if ui.button("Export").clicked() {
            let keys: Vec<String> = self
                .export_ids
                .split([',', ' '])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect();
            self.transfer_msg = Some(
                match self
                    .lib
                    .export(&keys, std::path::Path::new(self.export_dir.trim()))
                {
                    Ok(f) => (
                        format!("Wrote {} record file(s) to {}", f.len(), self.export_dir),
                        false,
                    ),
                    Err(e) => (e, true),
                },
            );
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Import from");
            ui.add(egui::TextEdit::singleline(&mut self.import_path).desired_width(260.0));
            if ui.button("Choose…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.import_path = p.display().to_string();
            }
        });
        if ui.button("Import").clicked() {
            self.transfer_msg = Some(
                match self
                    .lib
                    .import(std::path::Path::new(self.import_path.trim()))
                {
                    Ok(v) => {
                        let added = v.iter().filter(|(_, o)| *o == AddOutcome::Added).count();
                        out.push(Action::LibraryChanged);
                        (
                            format!(
                                "Imported {} record(s): {added} new, {} merged",
                                v.len(),
                                v.len() - added
                            ),
                            false,
                        )
                    }
                    Err(e) => (e, true),
                },
            );
        }
        if let Some((m, err)) = &self.transfer_msg {
            ui.colored_label(
                if *err {
                    Color32::LIGHT_RED
                } else {
                    Color32::LIGHT_GREEN
                },
                m,
            );
        }
    }

    fn remove_ui(&mut self, ui: &mut egui::Ui, selected: &Option<String>, out: &mut Vec<Action>) {
        ui.label(
            RichText::new(
                "Delete a pattern (with its rules and sightings) or an ant run (ids ant-…) from \
                 the catalog. This cannot be undone; export it first to keep a copy.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            ui.label("Record");
            ui.add(egui::TextEdit::singleline(&mut self.remove_id).desired_width(240.0));
            if let Some(s) = selected
                && ui.small_button("use selected").clicked()
            {
                self.remove_id = s.clone();
            }
        });
        let label = if self.remove_id.starts_with("ant-") {
            self.lib.ant(&self.remove_id).map(|a| a.line())
        } else {
            self.lib.get(&self.remove_id).map(|r| r.label())
        };
        match &label {
            Ok(l) => {
                ui.label(format!("Found: {l}"));
            }
            Err(_) if self.remove_id.trim().is_empty() => {}
            Err(e) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
        }
        ui.checkbox(&mut self.confirm, "Yes, delete it");
        if ui
            .add_enabled(
                self.confirm && label.is_ok(),
                egui::Button::new(RichText::new("Remove").color(Color32::LIGHT_RED)),
            )
            .clicked()
        {
            let r = if self.remove_id.starts_with("ant-") {
                self.lib.remove_ant(&self.remove_id).map(|a| a.id)
            } else {
                self.lib.remove(&self.remove_id).map(|r| r.id)
            };
            match r {
                Ok(id) => {
                    out.push(Action::Message(
                        format!("Removed {id} from the catalog."),
                        false,
                    ));
                    out.push(Action::LibraryChanged);
                    self.remove_id.clear();
                }
                Err(e) => out.push(Action::Message(e, true)),
            }
            self.confirm = false;
        }
    }

    fn evolve_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "Edmond's evolutionary glider search (as `jetcarp search evolve`): multi-state priority \
                 rules evolve towards seeds that travel far; the best are verified and the movers \
                 stored, and the run's files go to a new evolve folder in the output folder. It \
                 runs as a job: Stop keeps its last generation, and Resume carries it on. The \
                 published settings are population 256, 800 generations, radius 48.",
            )
            .small()
            .weak(),
        );
        let busy = self.evolving();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("evolve-form")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Tiling");
                    ui.add(egui::TextEdit::singleline(&mut self.ev_tiling).desired_width(140.0));
                    ui.end_row();
                    ui.label("States");
                    ui.add(egui::DragValue::new(&mut self.ev_states).range(2..=8));
                    ui.end_row();
                    ui.label("Population");
                    ui.add(egui::DragValue::new(&mut self.ev_pop).range(4..=4096));
                    ui.end_row();
                    ui.label("Generations");
                    ui.add(egui::DragValue::new(&mut self.ev_gens).range(1..=100_000));
                    ui.end_row();
                    ui.label("Arena radius");
                    ui.add(egui::DragValue::new(&mut self.ev_radius).range(4..=256));
                    ui.end_row();
                    ui.label("Seed");
                    ui.add(egui::DragValue::new(&mut self.ev_seed));
                    ui.end_row();
                });
        });
        let mut resume = None;
        ui.horizontal(|ui| {
            if !busy && ui.button("Evolve").clicked() {
                self.start_evolve();
            }
            if busy
                && let Some(j) = &self.evolve
                && ui
                    .button("Stop")
                    .on_hover_text("Stop after this generation; Resume carries it on")
                    .clicked()
            {
                j.stop();
            }
            if let Some(job) = self.evolve_resumable()
                && ui
                    .button("Resume")
                    .on_hover_text(format!(
                        "Carry search {job} on from its last generation (or `jetcarp jobs resume {job}`)"
                    ))
                    .clicked()
            {
                resume = Some(job);
            }
        });
        if let Some(job) = resume {
            self.resume_evolve(job);
        }
        if let Some(j) = self.evolve.as_ref().filter(|j| j.busy()) {
            let p = j.progress();
            match (p["best"].as_f64(), p["generation"].as_u64()) {
                (Some(best), Some(g)) => {
                    let total = p["generations"].as_u64().unwrap_or(self.ev_gens as u64);
                    ui.add(
                        egui::ProgressBar::new(g as f32 / total.max(1) as f32).text(format!(
                            "generation {g}/{total}: best {best:.2}, best ever {:.2}, {:.0} s",
                            p["best_ever"].as_f64().unwrap_or(best),
                            p["elapsed_s"].as_f64().unwrap_or(0.0)
                        )),
                    );
                }
                _ if p["status"] == "done" => {
                    ui.label("verifying the champions…");
                }
                _ => {
                    ui.label("building the arena…");
                }
            }
        }
        match &self.evolve_last {
            Some(Ok(lines)) => {
                ui.separator();
                for l in lines {
                    ui.monospace(l);
                }
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            None => {}
        }
    }
}

#[cfg(test)]
impl CatalogTools {
    /// Start an evolve search from the Evolve tab, as *Evolve* does (offscreen checks).
    pub fn test_evolve(&mut self, tiling: &str, pop: usize, gens: usize, radius: u32) {
        self.open = true;
        self.tab = Tab::Evolve;
        self.ev_tiling = tiling.into();
        (self.ev_pop, self.ev_gens, self.ev_radius) = (pop, gens, radius);
        self.start_evolve();
    }
    /// An evolve search started here still runs.
    pub fn evolve_running(&self) -> bool {
        self.evolving()
    }
    /// *Stop* on the Evolve tab.
    pub fn test_stop_evolve(&mut self) {
        if let Some(j) = &self.evolve {
            j.stop();
        }
    }
    /// The evolve search's job.
    pub fn test_evolve_job(&self) -> Option<i64> {
        self.evolve.as_ref().and_then(|j| j.job())
    }
    /// The evolve search has stopped (paused, resumable).
    pub fn test_evolve_stopped(&self) -> bool {
        self.evolve_resumable().is_some()
    }
    /// *Resume* a stopped evolve search; false when there is none.
    pub fn test_resume_evolve(&mut self) -> bool {
        match self.evolve_resumable() {
            Some(j) => {
                self.resume_evolve(j);
                true
            }
            None => false,
        }
    }
    /// Start a collision from the Collide tab, as *Collide* does.
    pub fn test_collide(&mut self, a: &str, b: &str, offsets: i32) {
        self.open = true;
        self.tab = Tab::Collide;
        (self.a, self.b, self.offsets) = (a.into(), b.into(), offsets);
        self.start_collide();
    }
    /// A collision started here still runs.
    pub fn collide_running(&self) -> bool {
        self.collide_job.is_some()
    }
}
