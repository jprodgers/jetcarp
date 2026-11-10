//! The *Sandpile…* window: build a large sandpile (`jetcarp_core::pile`) on a background thread,
//! see the pile drawn live, the doubling step (k of log2 N), and an ETA; cancel; the result is
//! written to `<Pictures>/JETCARP/piles/<time>_<tiling>-<N>/` like `jetcarp pile`.
//!
//! **Colours.** The palette (its stops for heights 1 to the largest degree, its background for
//! empty cells, its stage around the pile) and *Reverse* can change at any time: the live view
//! takes them at the next drawing (within a second while toppling, at once when the run has
//! ended, since the window keeps the pile), and the export uses the choice when the pile is
//! done. The folder's `look.json` keeps the choice, so `jetcarp pile --resume <folder>` exports
//! with it. After the run, *Export again* writes `pile.png` and `preview.png` in a new choice.

use jetcarp_core::pile::export::Look;
use jetcarp_core::pile::{self, Centre, Ctl, Pile, PileSpec, RunOptions, StepInfo, StepRecord, Tick};
use jetcarp_core::render::Image;
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "sandpile";

/// What the worker shares with the window.
#[derive(Default)]
struct Shared {
    info: StepInfo,
    tick: Tick,
    records: Vec<StepRecord>,
    preview: Option<Image>,
    preview_rev: u64,
    dir: Option<PathBuf>,
    /// Finished: Ok(summary line) or Err(message).
    done: Option<Result<String, String>>,
    backend: String,
    /// The colouring the worker draws with (the window changes it at any time).
    look: Look,
    /// The pile once the run has ended (finished or cancelled), to recolour and export again.
    pile: Option<Arc<Pile>>,
    /// The colouring `pile.png` was exported in.
    exported: Option<Look>,
}

struct Job {
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
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
    job: Option<Job>,
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
            job: None,
            tex: None,
            error: None,
            reexport: None,
            reexport_note: None,
        }
    }
}

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

impl PileWindow {
    /// `JETCARP_PILE=hex:1e6` (headless checks): open the window and start that pile.
    pub fn open_env(&mut self, s: &str) {
        let (t, g) = s.split_once(':').unwrap_or((s, "1e5"));
        self.tiling = t.to_string();
        self.grains = g.to_string();
        self.open = true;
        self.start();
    }

    fn running(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.shared.lock().unwrap().done.is_none())
    }

    fn look(&self) -> Look {
        Look::named(&self.palette, self.reverse)
    }

    /// The colouring changed: tell the worker, keep it in the folder, and redraw a pile whose
    /// run has ended at once.
    fn look_changed(&mut self) {
        let look = self.look();
        let Some(j) = &self.job else { return };
        let mut s = j.shared.lock().unwrap();
        s.look = look.clone();
        if let Some(d) = &s.dir {
            let _ = pile::export::write_look_file(d, &self.palette, self.reverse);
        }
        if let Some(p) = s.pile.clone() {
            s.preview = Some(pile::export::preview(&p.board.as_ref(), 480, &look));
            s.preview_rev += 1;
        }
    }

    /// Write pile.png, preview.png, and pile.json again in the current colours.
    fn export_again(&mut self) {
        let Some(j) = &self.job else { return };
        let (pile, dir) = {
            let s = j.shared.lock().unwrap();
            match (&s.pile, &s.dir) {
                (Some(p), Some(d)) => (p.clone(), d.clone()),
                _ => return,
            }
        };
        let look = self.look();
        let (palette, reverse, px, tiling) = (
            self.palette.clone(),
            self.reverse,
            self.px,
            self.tiling.clone(),
        );
        let shared = j.shared.clone();
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
        let spec = (|| -> Result<PileSpec, String> {
            Ok(PileSpec {
                tiling: jetcarp_script::spec::tiling_by_id(&self.tiling)?,
                nb: if self.nb == 0 {
                    jetcarp_core::Nb::Edge
                } else {
                    jetcarp_core::Nb::Touch
                },
                grains: pile::parse_grains(&self.grains)?,
                centre: [Centre::Cell, Centre::Vertex, Centre::Edge][self.centre],
            })
        })();
        let spec = match spec {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let shared = Arc::new(Mutex::new(Shared::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sh, cn) = (shared.clone(), cancel.clone());
        let tiling = self.tiling.clone();
        let use_gpu = self.gpu && !pile::is_patch(&spec.tiling);
        let px = self.px;
        shared.lock().unwrap().look = self.look();
        self.reexport = None;
        self.reexport_note = None;
        std::thread::spawn(move || {
            let mut kept = None;
            let r = run(spec, &tiling, use_gpu, px, &sh, &cn, &mut kept);
            let mut s = sh.lock().unwrap();
            s.pile = kept.map(Arc::new);
            s.done = Some(r);
        });
        self.job = Some(Job {
            shared,
            cancel,
            started: Instant::now(),
            tex_rev: 0,
        });
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
                    RichText::new(
                        "N grains on one spot of an unbounded grid, settled by doubling (N's binary \
                         digits). Writes pile.png, preview.png, pile.json, and a resumable file per \
                         step to <Pictures>/JETCARP/piles/.",
                    )
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
                        ui.checkbox(&mut self.gpu, "Topple on the GPU (periodic tilings)");
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
                ui.horizontal(|ui| {
                    if running {
                        if ui.button("Cancel").clicked()
                            && let Some(j) = &self.job
                        {
                            j.cancel.store(true, Ordering::SeqCst);
                        }
                    } else if ui.button(RichText::new("Start").strong()).clicked() {
                        self.start();
                    }
                });
                if let Some(e) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                self.progress(ui);
            },
        );
        if !open {
            self.open = false;
        }
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
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
        let mut again = false;
        let Some(job) = &mut self.job else { return };
        let sh = job.shared.lock().unwrap();
        ui.separator();
        let el = job.started.elapsed().as_secs_f64();
        match &sh.done {
            None => {
                let i = sh.info;
                let frac = if i.steps > 0 {
                    (i.step.saturating_sub(1)) as f32 / i.steps as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(frac).text(format!(
                    "step {} of {} (log2 N), {} grains",
                    i.step, i.steps, i.grains
                )));
                let eta = pile::eta_seconds(&sh.records, i.steps, sh.tick.seconds)
                    .map(|s| format!(", about {} left", pile::fmt_seconds(s)))
                    .unwrap_or_default();
                ui.label(format!(
                    "{} sweeps, {:.3e} topplings this step; {} elapsed{eta} ({})",
                    sh.tick.sweeps,
                    sh.tick.topplings as f64,
                    pile::fmt_seconds(el),
                    sh.backend
                ));
            }
            Some(Ok(s)) => {
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
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
        }
        if let Some(d) = &sh.dir {
            ui.label(RichText::new(d.display().to_string()).small().weak());
        }
        if let Some(r) = sh.records.last() {
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
        if sh.preview_rev != job.tex_rev
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
            job.tex_rev = sh.preview_rev;
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

/// The worker: build the pile step by step, publishing previews in the window's colours. The
/// pile is left in `kept` (also when cancelled or failed mid-run).
fn run(
    spec: PileSpec,
    tiling: &str,
    use_gpu: bool,
    px: f64,
    sh: &Arc<Mutex<Shared>>,
    cancel: &Arc<AtomicBool>,
    kept: &mut Option<Pile>,
) -> Result<String, String> {
    let p = kept.insert(Pile::new(spec.clone())?);
    let root = jetcarp_api::out::out_root(None).join("piles");
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dir = jetcarp_api::out::make_out_dir(&root, &format!("{tiling}-{}", spec.grains))?;
    {
        let mut s = sh.lock().unwrap();
        s.dir = Some(dir.clone());
        pile::export::write_look_file(&dir, &s.look.palette.name, s.look.reverse)?;
    }
    let mut gpu = if use_gpu {
        match jetcarp_gpu::pile::PileGpu::headless() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("pile: GPU unavailable ({e}); using the CPU");
                None
            }
        }
    } else {
        None
    };
    sh.lock().unwrap().backend = match &gpu {
        Some(g) => pile::Toppler::name(g),
        None => format!(
            "cpu, {} threads",
            std::thread::available_parallelism().map_or(1, |n| n.get())
        ),
    };
    let publish = |b: pile::BoardRef| {
        let look = sh.lock().unwrap().look.clone();
        let img = pile::export::preview(&b, 480, &look);
        let mut s = sh.lock().unwrap();
        s.preview = Some(img);
        s.preview_rev += 1;
    };
    // A tick every second draws the pile (the GPU copies its board back for it).
    let mut ctl = Ctl {
        cancel: Some(cancel),
        tick_every: 1.0,
        needs_board: gpu.is_some(),
        on_tick: Some(Box::new(|info, t, b| {
            {
                let mut s = sh.lock().unwrap();
                s.info = *info;
                s.tick = *t;
            }
            publish(b);
        })),
        ..Default::default()
    };
    std::fs::create_dir_all(dir.join("steps")).map_err(|e| e.to_string())?;
    while !p.finished() {
        {
            let mut s = sh.lock().unwrap();
            s.info = StepInfo {
                step: p.done + 1,
                steps: p.steps,
                grains: p.grains_after(p.done + 1),
            };
            s.tick = Tick::default();
        }
        let r = match gpu.as_mut() {
            Some(g) => p.next_step(Some(g as &mut dyn pile::Toppler), &mut ctl),
            None => p.next_step(None, &mut ctl),
        };
        let r = r.map_err(|e| {
            if cancel.load(Ordering::SeqCst) {
                format!(
                    "Cancelled in step {}. Resume with: jetcarp pile --resume {}",
                    p.done + 1,
                    dir.display()
                )
            } else {
                e
            }
        })?;
        pile::file::save(p, &dir.join(format!("steps/step-{:02}.pile", p.done)))?;
        pile::write_growth(p, &dir)?;
        sh.lock().unwrap().records.push(r);
        publish(p.board.as_ref());
    }
    drop(ctl);
    // Export in the colours chosen now.
    let look = sh.lock().unwrap().look.clone();
    pile::export::write_look_file(&dir, &look.palette.name, look.reverse)?;
    let opts = RunOptions {
        export: pile::export::ExportOptions {
            px_per_cell: px,
            palette: look.palette.name.clone(),
            reverse: look.reverse,
            ..Default::default()
        },
        save_steps: false,
        extra: serde_json::json!({ "tiling_id": tiling, "host": "jetcarp-app" }),
    };
    let s = pile::run_to_dir(p, &dir, None, &mut Ctl::default(), &opts)?;
    sh.lock().unwrap().exported = Some(look.clone());
    let (top, sweeps, secs) = p.totals();
    Ok(format!(
        "Done: {} grains, {} topplings, {} sweeps, {} of toppling{} in {}{}",
        p.spec.grains,
        top,
        sweeps,
        pile::fmt_seconds(secs),
        s.image_size
            .map(|(w, h)| format!("; pile.png {w} x {h}"))
            .unwrap_or_default(),
        look.palette.name,
        if look.reverse { ", reversed" } else { "" }
    ))
}

#[cfg(test)]
impl PileWindow {
    /// The run has ended (offscreen checks).
    pub fn finished(&self) -> bool {
        self.job.is_some() && !self.running()
    }
    /// Change the colours as the picker does (offscreen checks).
    pub fn set_look(&mut self, palette: &str, reverse: bool) {
        self.palette = palette.into();
        self.reverse = reverse;
        self.look_changed();
    }
}
