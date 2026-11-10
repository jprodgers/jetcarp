//! The *Catalog tools…* window: the catalog actions that were CLI- or script-only before
//! (Phase 3 reachability gaps). Collide two patterns, export and import JSON records, remove a
//! record, and run the evolutionary glider search, each on a background thread.

use jetcarp_core::evolve::{EvolveSpec, GenStats, evolve};
use jetcarp_core::library::collide::{CollideSpec, collide};
use jetcarp_core::library::{self, AddOutcome, JobStatus, Library};
use jetcarp_core::world::Scene;
use eframe::egui::{self, Color32, RichText};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
struct CollideDone {
    a: String,
    b: String,
    rule: String,
    counts: Vec<(String, usize)>,
    products: Vec<(String, String, bool)>,
}

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// A collision in flight: its result slot and (placements done, placements).
type CollideJob = (Slot<CollideDone>, Arc<Mutex<(usize, usize)>>);

struct EvolveRun {
    stop: Arc<AtomicBool>,
    live: Arc<Mutex<Option<GenStats>>>,
    done: Slot<Vec<String>>,
}

pub struct CatalogTools {
    pub open: bool,
    pub tab: Tab,
    lib: Library,
    // Collide
    pub a: String,
    pub b: String,
    offsets: i32,
    gens: u64,
    store: bool,
    collide_job: Option<CollideJob>,
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
    evolve: Option<EvolveRun>,
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

fn run_collide(
    lib: &Library,
    a: &str,
    b: &str,
    spec: &CollideSpec,
    store: bool,
    prog: &Mutex<(usize, usize)>,
) -> Result<CollideDone, String> {
    let ra = lib.get(a)?;
    let rb = lib.get(b)?;
    let p = |d: usize, t: usize| *prog.lock().unwrap() = (d, t);
    let out = collide(&ra, &rb, spec, 0, Some(&p))?;
    let mut new_ids = Vec::new();
    if store && !out.records.is_empty() {
        let job = lib.create_job(
            "collide",
            &serde_json::to_value(&out.report.spec).unwrap_or_default(),
        )?;
        for (rec, o) in out
            .records
            .iter()
            .zip(lib.add_many(out.records.clone(), Some(job))?)
        {
            if o? == AddOutcome::Added {
                new_ids.push(rec.id.clone());
            }
        }
        lib.update_job(job, Some(JobStatus::Done), None, None)?;
    }
    Ok(CollideDone {
        a: out.report.a_label.clone(),
        b: out.report.b_label.clone(),
        rule: out.report.rule.clone(),
        counts: out
            .report
            .counts
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
        products: out
            .records
            .iter()
            .map(|r| (r.id.clone(), r.label(), new_ids.contains(&r.id)))
            .collect(),
    })
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
        self.collide_job.is_some() || self.evolve.is_some()
    }

    fn start_collide(&mut self) {
        let spec = CollideSpec {
            dx: [-self.offsets, self.offsets],
            dy: [-self.offsets, self.offsets],
            gens: self.gens,
            ..Default::default()
        };
        let slot: Slot<CollideDone> = Default::default();
        let prog = Arc::new(Mutex::new((0, 0)));
        let (l, a, b, st, s2, p2) = (
            self.lib.clone(),
            self.a.clone(),
            self.b.clone(),
            self.store,
            slot.clone(),
            prog.clone(),
        );
        std::thread::spawn(move || {
            let r = run_collide(&l, &a, &b, &spec, st, &p2);
            *s2.lock().unwrap() = Some(r);
        });
        self.collide_job = Some((slot, prog));
    }

    fn start_evolve(&mut self) {
        let tiling = match library::tiling_from_key(&self.ev_tiling) {
            Ok(t) => t,
            Err(e) => {
                self.evolve_last = Some(Err(e));
                return;
            }
        };
        let spec = EvolveSpec {
            tiling,
            states: self.ev_states,
            pop: self.ev_pop,
            gens: self.ev_gens,
            radius: self.ev_radius,
            seed: self.ev_seed,
            ..Default::default()
        };
        let run = EvolveRun {
            stop: Arc::new(AtomicBool::new(false)),
            live: Default::default(),
            done: Default::default(),
        };
        let (stop, live, done, lib) = (
            run.stop.clone(),
            run.live.clone(),
            run.done.clone(),
            self.lib.clone(),
        );
        std::thread::spawn(move || {
            let r = evolve(&spec, &|g| {
                *live.lock().unwrap() = Some(g.clone());
                !stop.load(Ordering::SeqCst)
            })
            .and_then(|rep| {
                let found = jetcarp_core::evolve::catalog::catalog_gliders(&rep, Some(&lib), 20_000)?;
                let mut lines: Vec<String> = rep
                    .champions
                    .iter()
                    .take(5)
                    .map(|c| format!("{:.2}  {}", c.fitness, c.rule))
                    .collect();
                lines.extend(
                    found
                        .iter()
                        .map(|m| format!("{}: {} ({} new)", m.kind, m.summary, m.added.len())),
                );
                if found.is_empty() {
                    lines.push(format!(
                        "{} generations, no verified glider",
                        rep.generations.len()
                    ));
                }
                Ok(lines)
            });
            *done.lock().unwrap() = Some(r);
        });
        self.evolve = Some(run);
    }

    /// Collect finished background work.
    fn poll(&mut self, out: &mut Vec<Action>) {
        let collided = self
            .collide_job
            .as_ref()
            .and_then(|(slot, _)| slot.lock().unwrap().take());
        if let Some(r) = collided {
            if r.as_ref().is_ok_and(|d| d.products.iter().any(|p| p.2)) {
                out.push(Action::LibraryChanged);
            }
            self.collide_last = Some(r);
            self.collide_job = None;
        }
        let evolved = self
            .evolve
            .as_ref()
            .and_then(|run| run.done.lock().unwrap().take());
        if let Some(r) = evolved {
            out.push(Action::LibraryChanged);
            self.evolve_last = Some(r);
            self.evolve = None;
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
                 both work in; new objects are verified and stored (as `jetcarp library collide`).",
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
        if let Some((_, p)) = &self.collide_job {
            let (d, t) = *p.lock().unwrap();
            ui.add(
                egui::ProgressBar::new(if t > 0 { d as f32 / t as f32 } else { 0.0 })
                    .text(format!("{d} of {t} placements")),
            );
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
                if let Some(id) = open_id {
                    match self
                        .lib
                        .get(&id)
                        .and_then(|r| library::promote::showcase_scene(&r, None, None))
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
                "Edmond's evolutionary glider search (as `jetcarp evolve`): multi-state priority \
                 rules evolve towards seeds that travel far; the best are verified and the movers \
                 stored. The published settings are population 256, 800 generations, radius 48.",
            )
            .small()
            .weak(),
        );
        let busy = self.evolve.is_some();
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
        ui.horizontal(|ui| {
            if !busy && ui.button("Evolve").clicked() {
                self.start_evolve();
            }
            if let Some(r) = &self.evolve
                && ui.button("Stop").clicked()
            {
                r.stop.store(true, Ordering::SeqCst);
            }
        });
        if let Some(r) = &self.evolve {
            match r.live.lock().unwrap().as_ref() {
                Some(g) => {
                    ui.add(
                        egui::ProgressBar::new(g.generation as f32 / self.ev_gens.max(1) as f32)
                            .text(format!(
                                "generation {}/{}: best {:.2}, best ever {:.2}, {:.0} s",
                                g.generation, self.ev_gens, g.best, g.best_ever, g.elapsed_s
                            )),
                    );
                }
                None => {
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
