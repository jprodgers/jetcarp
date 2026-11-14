//! The *Sandpile…* window: build a large sandpile (`jetcarp_core::pile`) as a catalog job, see
//! the pile drawn live, the doubling step (k of log2 N), and the time left; stop it and resume
//! it; the result is written to `piles/<time>_<tiling>-<N>/` in the output folder
//! (`$JETCARP_OUT`, else `<Pictures>/JETCARP`), like `jetcarp pile`, and the window names it.
//!
//! **The job.** *Start* calls the operation `search.start` (strategy `pile`, in the background),
//! *Stop* calls `jobs.stop`, and *Resume* calls `jobs.resume` (`crate::window_job`), so the pile
//! is a job in the process's registry, which the dashboard and `jobs.status` see. The window
//! watches the run's live state: the job shows its board every second and after each step, and
//! the window draws the preview from it on the run's thread. A stopped pile keeps its finished
//! steps in its folder, and *Resume* carries it on from the last one.
//!
//! **Colours.** The palette (its stops for heights 1 to the largest degree, its background for
//! empty cells, its stage around the pile) and *Reverse* can change at any time: the live view
//! takes them at the next drawing (within a second while toppling, at once when the run has
//! ended, since the window keeps the pile), and the export uses the choice when the pile is
//! done: the window keeps it in the folder's `look.json`, which the job reads for its export (so
//! `jetcarp jobs resume JOB` exports with it too). After the run, *Export again* writes
//! `pile.png` and `preview.png` in a new choice.

use crate::window_job::{Hooks, WindowJob};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use jetcarp_api::ops::jobs::ResumeIn;
use jetcarp_api::ops::search::SearchStartIn;
use jetcarp_core::pile::export::Look;
use jetcarp_core::pile::{self, BoardRef, Pile, RunOptions};
use jetcarp_core::render::Image;
use jetcarp_search::pile::PileProgress;
use jetcarp_store::Library;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

/// Where piles are written: `piles` in the output folder the job's context uses
/// (`crate::window_job::ctx`: `$JETCARP_OUT`, else `<Pictures>/JETCARP`).
fn piles_folder() -> PathBuf {
    jetcarp_api::out::out_root(None).join("piles")
}

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "sandpile";

/// How a run ended.
#[derive(Clone, Debug)]
enum End {
    /// Done: the summary line.
    Done(String),
    /// Stopped part way, resumable: what to say.
    Paused(String),
    Failed(String),
}

/// What the run's thread and the window's worker share with the window.
#[derive(Default)]
struct Shared {
    preview: Option<Image>,
    preview_rev: u64,
    /// The colouring the live view draws with (the window changes it at any time).
    look: Look,
    /// The pile once the run has ended (finished or stopped), read from its folder, to
    /// recolour and export again.
    pile: Option<Arc<Pile>>,
    /// The colouring `pile.png` was exported in.
    exported: Option<Look>,
    end: Option<End>,
}

/// The pile started or resumed here.
struct Run {
    job: WindowJob,
    shared: Arc<Mutex<Shared>>,
    started: Instant,
    tex_rev: u64,
}

pub struct PileWindow {
    pub open: bool,
    tiling: String,
    grains: String,
    nb: usize,
    centre: usize,
    gpu: bool,
    px: f64,
    /// Palette (a built-in name) and direction of the height colours (saved in the settings).
    pub palette: String,
    pub reverse: bool,
    catalog: PathBuf,
    run: Option<Run>,
    tex: Option<TextureHandle>,
    error: Option<String>,
    /// An *Export again* in flight, and its last result.
    reexport: Option<mpsc::Receiver<Result<String, String>>>,
    reexport_note: Option<Result<String, String>>,
}

impl Default for PileWindow {
    fn default() -> Self {
        PileWindow {
            open: false,
            tiling: "square".into(),
            grains: "1e6".into(),
            nb: 0,
            centre: 0,
            gpu: true,
            px: 2.0,
            palette: "Ember".into(),
            reverse: false,
            catalog: Library::default_path(),
            run: None,
            tex: None,
            error: None,
            reexport: None,
            reexport_note: None,
        }
    }
}

/// The centre choices, as the pile spec names them.
const CENTRES: [&str; 3] = ["cell", "vertex", "edge"];

const TILINGS: &[&str] = &[
    "square",
    "hex",
    "triangle",
    "trihex",
    "snubsq",
    "oct",
    "rhombitrihex",
    "trunctrihex",
    "snubhex",
    "elongtri",
    "trunchex",
    "cairo",
    "spectre",
    "hat",
    "penrose-rhomb",
    "penrose-kd",
];

/// How the window watches a pile job: the board drawn into the preview in the window's colours
/// (on the run's thread), and at the end the pile read back from its folder and its export's
/// colours and size from `pile.json`.
fn hooks(shared: &Arc<Mutex<Shared>>) -> Hooks {
    let sh = shared.clone();
    let view = Arc::new(move |state: &dyn std::any::Any| {
        if let Some(b) = BoardRef::from_any(state) {
            let look = sh.lock().unwrap().look.clone();
            let img = pile::export::preview(&b, 480, &look);
            let mut s = sh.lock().unwrap();
            s.preview = Some(img);
            s.preview_rev += 1;
        }
    });
    let sh = shared.clone();
    let ended = Arc::new(
        move |_: &jetcarp_api::Ctx, job: i64, last: &serde_json::Value| {
            let p: PileProgress = serde_json::from_value(last.clone()).unwrap_or_default();
            let folder = PathBuf::from(&p.folder);
            // The pile as its folder keeps it, to recolour and export again.
            let kept = pile::latest_step_file(&folder).and_then(|f| pile::file::load(&f).ok());
            let look = sh.lock().unwrap().look.clone();
            let img = kept
                .as_ref()
                .map(|k| pile::export::preview(&k.board.as_ref(), 480, &look));
            let (end, exported) = match p.status.as_str() {
                "done" => {
                    let (line, exported) = done_line(&p, &folder);
                    (End::Done(line), exported)
                }
                "paused" => (
                    End::Paused(format!(
                        "Stopped after step {} of {}. Resume carries it on from there (or `jetcarp jobs resume {job}`).",
                        p.step, p.steps
                    )),
                    None,
                ),
                _ => (
                    End::Failed(p.error.unwrap_or_else(|| "the pile failed".into())),
                    None,
                ),
            };
            let mut s = sh.lock().unwrap();
            if let Some(img) = img {
                s.preview = Some(img);
                s.preview_rev += 1;
            }
            s.pile = kept.map(Arc::new);
            s.exported = exported;
            s.end = Some(end);
        },
    );
    Hooks {
        view: Some(view),
        ended: Some(ended),
    }
}

/// The summary of a finished pile, and the colouring its export used (`pile.json`).
fn done_line(p: &PileProgress, folder: &Path) -> (String, Option<Look>) {
    let meta: serde_json::Value = std::fs::read(folder.join("pile.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let img = &meta["image"];
    let palette = img["palette"].as_str().unwrap_or("Ember").to_string();
    let reverse = img["reverse"].as_bool().unwrap_or(false);
    let size = match (img["width"].as_u64(), img["height"].as_u64()) {
        (Some(w), Some(h)) => format!("; pile.png {w} x {h}"),
        _ => String::new(),
    };
    let line = format!(
        "Done: {} grains, {} topplings, {} sweeps, {} of toppling{size} in {palette}{}",
        p.grains,
        p.topplings,
        p.sweeps,
        pile::fmt_seconds(p.seconds),
        if reverse { ", reversed" } else { "" }
    );
    (line, Some(Look::named(&palette, reverse)))
}

impl PileWindow {
    /// `JETCARP_PILE=hex:1e6` (headless checks): open the window and start that pile.
    pub fn open_env(&mut self, s: &str) {
        let (t, g) = s.split_once(':').unwrap_or((s, "1e5"));
        self.tiling = t.to_string();
        self.grains = g.to_string();
        self.open = true;
        self.start();
    }

    /// The pile started or resumed here is still at work.
    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.job.busy())
    }

    fn look(&self) -> Look {
        Look::named(&self.palette, self.reverse)
    }

    /// The run's progress (the pile job's).
    fn progress(&self) -> PileProgress {
        self.run
            .as_ref()
            .map(|r| serde_json::from_value(r.job.progress()).unwrap_or_default())
            .unwrap_or_default()
    }

    /// The pile's folder, once the job has one.
    fn folder(&self) -> Option<PathBuf> {
        let f = self.progress().folder;
        (!f.is_empty()).then(|| PathBuf::from(f))
    }

    /// The colouring changed: tell the live view, keep it in the folder (the job's export reads
    /// it), and redraw a pile whose run has ended at once.
    fn look_changed(&mut self) {
        let look = self.look();
        let folder = self.folder();
        let Some(r) = &self.run else { return };
        let mut s = r.shared.lock().unwrap();
        s.look = look.clone();
        if let Some(d) = &folder {
            let _ = pile::export::write_look_file(d, &self.palette, self.reverse);
        }
        if let Some(p) = s.pile.clone() {
            s.preview = Some(pile::export::preview(&p.board.as_ref(), 480, &look));
            s.preview_rev += 1;
        }
    }

    /// Write pile.png, preview.png, and pile.json again in the current colours.
    fn export_again(&mut self) {
        let Some(dir) = self.folder() else { return };
        let Some(r) = &self.run else { return };
        let Some(pile) = r.shared.lock().unwrap().pile.clone() else {
            return;
        };
        let look = self.look();
        let (palette, reverse, px, tiling) = (
            self.palette.clone(),
            self.reverse,
            self.px,
            self.tiling.clone(),
        );
        let shared = r.shared.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = (|| -> Result<String, String> {
                pile::export::write_look_file(&dir, &palette, reverse)?;
                let opts = RunOptions {
                    export: pile::export::ExportOptions {
                        px_per_cell: px,
                        palette: palette.clone(),
                        reverse,
                        ..Default::default()
                    },
                    save_steps: false,
                    extra: serde_json::json!({ "tiling_id": tiling, "host": "jetcarp-app" }),
                };
                let s = pile::export::finish(&pile, &dir, &opts)?;
                shared.lock().unwrap().exported = Some(look);
                Ok(format!(
                    "Exported again in {palette}{}{}",
                    if reverse { ", reversed" } else { "" },
                    s.image_size
                        .map(|(w, h)| format!(": pile.png {w} x {h}"))
                        .unwrap_or_default()
                ))
            })();
            let _ = tx.send(r);
        });
        self.reexport = Some(rx);
        self.reexport_note = None;
    }

    fn start(&mut self) {
        self.error = None;
        // Checked here for a message at once; the operation reads the same text.
        let tiling = match jetcarp_api::args::tiling_by_id(&self.tiling) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        if let Err(e) = pile::parse_grains(&self.grains) {
            self.error = Some(e);
            return;
        }
        let a = SearchStartIn {
            strategy: Some("pile".into()),
            spec: serde_json::json!({
                "tiling": self.tiling.trim(),
                "grains": self.grains.trim(),
                "nb": if self.nb == 0 { "edge" } else { "touch" },
                "centre": CENTRES[self.centre],
                "px": self.px,
                "palette": self.palette,
                "reverse": self.reverse,
                // Ticked: as JETCARP's GPU setting says (automatic: from 100,000 grains on
                // periodic tilings); unticked: the CPU only.
                "gpu": if self.gpu && !pile::is_patch(&tiling) {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::Bool(false)
                },
            }),
            ..Default::default()
        };
        let shared = Arc::new(Mutex::new(Shared {
            look: self.look(),
            ..Default::default()
        }));
        self.reexport = None;
        self.reexport_note = None;
        self.run = Some(Run {
            job: WindowJob::start(&self.catalog, a, hooks(&shared)),
            shared,
            started: Instant::now(),
            tex_rev: 0,
        });
    }

    /// Carry the stopped pile on from its last finished step (`jobs.resume`).
    fn resume(&mut self, job: i64) {
        self.error = None;
        let Some(r) = self.run.take() else { return };
        {
            let mut s = r.shared.lock().unwrap();
            s.end = None;
            s.pile = None;
            s.look = self.look();
        }
        let a = ResumeIn {
            job,
            ..Default::default()
        };
        self.run = Some(Run {
            job: WindowJob::resume(&self.catalog, a, hooks(&r.shared)),
            shared: r.shared,
            started: Instant::now(),
            tex_rev: r.tex_rev,
        });
    }

    /// The job of a stopped pile that can be resumed here.
    fn resumable(&self) -> Option<i64> {
        let r = self.run.as_ref().filter(|r| !r.job.busy())?;
        matches!(r.shared.lock().unwrap().end, Some(End::Paused(_)))
            .then(|| r.job.job())
            .flatten()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            if self.running() {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
            return;
        }
        let mut open = true;
        let running = self.running();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Large sandpile")
                .size(560.0, 520.0)
                .configure(|w| w.default_width(560.0)),
            &mut open,
            |ui| {
                ui.label(
                    RichText::new(format!(
                        "N grains on one spot of an unbounded grid, settled by doubling (N's binary \
                         digits). Writes pile.png, preview.png, pile.json, and a resumable file per \
                         step to {}. The pile runs as a job: the dashboard lists it, and a stopped \
                         pile resumes from its last finished step.",
                        piles_folder().display()
                    ))
                    .small()
                    .weak(),
                );
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("pile-form").num_columns(2).show(ui, |ui| {
                        ui.label("Tiling");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.tiling).desired_width(120.0),
                            )
                            .on_hover_text(
                                "Any tiling id: square, hex, snubsq, k3-012, ih41, spectre, …",
                            );
                            egui::ComboBox::from_id_salt("pile-tiling")
                                .selected_text("pick…")
                                .show_ui(ui, |ui| {
                                    for t in TILINGS {
                                        if ui.selectable_label(self.tiling == *t, *t).clicked() {
                                            self.tiling = t.to_string();
                                        }
                                    }
                                });
                        });
                        ui.end_row();
                        ui.label("Grains");
                        ui.add(egui::TextEdit::singleline(&mut self.grains).desired_width(120.0))
                            .on_hover_text("1000000, 1e7, 2^30 (up to 2^64 − 1)");
                        ui.end_row();
                        ui.label("Neighbours");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.nb, 0, "edge (von Neumann)");
                            ui.selectable_value(&mut self.nb, 1, "touch (Moore)");
                        });
                        ui.end_row();
                        ui.label("Centre");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.centre, 0, "cell");
                            ui.selectable_value(&mut self.centre, 1, "vertex");
                            ui.selectable_value(&mut self.centre, 2, "edge midpoint");
                        });
                        ui.end_row();
                        ui.label("Image");
                        ui.add(egui::Slider::new(&mut self.px, 0.5..=8.0).text("px per cell"));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut self.gpu, "Topple on the GPU (periodic tilings)")
                            .on_hover_text(
                                "As JETCARP's GPU setting says (automatic: piles of 100,000 \
                                 grains or more); unticked, on the CPU only. The pile is the same.",
                            );
                        ui.end_row();
                    });
                });
                // The colours change at any time, while the pile builds too.
                ui.horizontal(|ui| {
                    ui.label("Colours");
                    let mut changed =
                        crate::palette_pick::picker(ui, "pile-palette", &mut self.palette, &[]);
                    changed |= ui
                        .checkbox(&mut self.reverse, "Reverse")
                        .on_hover_text("Height 1 at the palette's last stop, the top at its first")
                        .changed();
                    if changed {
                        self.look_changed();
                    }
                });
                ui.label(
                    RichText::new(
                        "Heights 1 and up run along the palette; empty cells take its background, \
                         the area around the pile its stage.",
                    )
                    .small()
                    .weak(),
                );
                let mut resume = None;
                ui.horizontal(|ui| {
                    if running {
                        if ui
                            .button("Stop (resumable)")
                            .on_hover_text(
                                "Stop now: the step in flight is saved as it stands (its grains \
                                 added, part-toppled), and Resume carries it on from there",
                            )
                            .clicked()
                            && let Some(r) = &self.run
                        {
                            r.job.stop();
                        }
                    } else {
                        if ui.button(RichText::new("Start").strong()).clicked() {
                            self.start();
                        }
                        if let Some(job) = self.resumable()
                            && ui
                                .button("Resume")
                                .on_hover_text(
                                    "Carry the pile on from where it stopped: its last finished \
                                     step, or the step it stopped in",
                                )
                                .clicked()
                        {
                            resume = Some(job);
                        }
                    }
                });
                if let Some(job) = resume {
                    self.resume(job);
                }
                if let Some(e) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                self.progress_ui(ui);
            },
        );
        if !open {
            self.open = false;
        }
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn progress_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.reexport {
            match rx.try_recv() {
                Ok(r) => {
                    self.reexport_note = Some(r);
                    self.reexport = None;
                }
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint(),
                Err(mpsc::TryRecvError::Disconnected) => self.reexport = None,
            }
        }
        let p = self.progress();
        let running = self.running();
        let mut again = false;
        let Some(run) = &mut self.run else { return };
        let start_error = run.job.error().filter(|_| run.job.job().is_none());
        let sh = run.shared.lock().unwrap();
        ui.separator();
        let el = run.started.elapsed().as_secs_f64();
        match (&sh.end, running) {
            (_, true) => {
                if p.steps == 0 {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Starting the pile…");
                    });
                } else {
                    let frac = p.step as f32 / p.steps as f32;
                    ui.add(egui::ProgressBar::new(frac).text(format!(
                        "step {} of {} (log2 N), {} grains settled",
                        (p.step + 1).min(p.steps),
                        p.steps,
                        p.grains_done
                    )));
                    let eta = p
                        .eta_s
                        .map(|s| format!(", about {} left", pile::fmt_seconds(s)))
                        .unwrap_or_default();
                    ui.label(format!(
                        "{} sweeps, {:.3e} topplings this step; {} elapsed{eta} ({})",
                        p.in_step.sweeps,
                        p.in_step.topplings as f64,
                        pile::fmt_seconds(el),
                        p.backend.as_deref().unwrap_or("starting")
                    ));
                }
            }
            (Some(End::Done(s)), false) => {
                ui.label(RichText::new(s).color(Color32::LIGHT_GREEN));
                // Offer an export in the current colours when they differ from pile.png's.
                let now = Look::named(&self.palette, self.reverse);
                if sh.pile.is_some() && sh.exported.as_ref().is_some_and(|e| *e != now) {
                    let busy = self.reexport.is_some();
                    let label = format!(
                        "Export again in {}{}",
                        self.palette,
                        if self.reverse { ", reversed" } else { "" }
                    );
                    if ui
                        .add_enabled(!busy, egui::Button::new(label))
                        .on_hover_text(
                            "Write pile.png, preview.png, and pile.json in these colours",
                        )
                        .clicked()
                    {
                        again = true;
                    }
                }
            }
            (Some(End::Paused(s)), false) => {
                ui.label(RichText::new(s).color(Color32::from_rgb(0xff, 0xc8, 0x3c)));
            }
            (Some(End::Failed(e)), false) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            (None, false) => {
                if let Some(e) = &start_error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            }
        }
        if !p.folder.is_empty() {
            ui.label(RichText::new(&p.folder).small().weak());
        }
        if let Some(r) = &p.last_step {
            ui.label(
                RichText::new(format!(
                    "last step: {} grains, {} sweeps, {} topplings, {}",
                    r.grains,
                    r.sweeps,
                    r.topplings,
                    pile::fmt_seconds(r.seconds)
                ))
                .small(),
            );
        }
        if sh.preview_rev != run.tex_rev
            && let Some(img) = &sh.preview
        {
            let ci = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.rgba,
            );
            self.tex = Some(
                ui.ctx()
                    .load_texture("pile-preview", ci, Default::default()),
            );
            run.tex_rev = sh.preview_rev;
        }
        drop(sh);
        if self.reexport.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Exporting…");
            });
        }
        match &self.reexport_note {
            Some(Ok(m)) => {
                ui.label(RichText::new(m).color(Color32::LIGHT_GREEN));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            None => {}
        }
        if let Some(t) = &self.tex {
            let size = t.size_vec2();
            let s = (ui.available_width() / size.x).min(480.0 / size.y).min(4.0);
            ui.image((t.id(), size * s));
        }
        if again {
            self.export_again();
        }
    }
}

#[cfg(test)]
impl PileWindow {
    /// The run has ended (offscreen checks).
    pub fn finished(&self) -> bool {
        self.run.is_some() && !self.running()
    }
    /// Change the colours as the picker does (offscreen checks).
    pub fn set_look(&mut self, palette: &str, reverse: bool) {
        self.palette = palette.into();
        self.reverse = reverse;
        self.look_changed();
    }
    /// *Stop (resumable)*.
    pub fn test_stop(&self) {
        if let Some(r) = &self.run {
            r.job.stop();
        }
    }
    /// *Resume*; false when there is no stopped pile.
    pub fn test_resume(&mut self) -> bool {
        match self.resumable() {
            Some(j) => {
                self.resume(j);
                true
            }
            None => false,
        }
    }
    /// The pile's job.
    pub fn test_job(&self) -> Option<i64> {
        self.run.as_ref().and_then(|r| r.job.job())
    }
    /// The run's progress: (status, steps done, steps).
    pub fn test_progress(&self) -> (String, u32, u32) {
        let p = self.progress();
        (p.status, p.step, p.steps)
    }
}
