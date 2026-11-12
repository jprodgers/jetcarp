//! Large tilings are built on a background thread: a level-8 aperiodic patch (17 to 18 million
//! tiles) takes several seconds to build, plus its seeding and drawing geometry, and the window
//! would otherwise freeze. While it builds, the old world stays on screen (paused), commands are
//! refused with a message, and the view shows what is being built and for how long. A newer
//! scene, tiling, or shown world replaces a build in progress (its result is dropped).

use super::{JetcarpApp, Screen};
use crate::gpu::Geometry;
use eframe::egui::{self, Color32};
use jetcarp_core::{Scene, SizeSpec, TilingSpec, Topology, World, aperiodic};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

/// Tilings with more cells than this are built in the background.
const BACKGROUND_CELLS: u64 = 1_000_000;

pub(super) struct Building {
    what: String,
    started: Instant,
    rx: Receiver<Result<Built, String>>,
    /// The scene being loaded (None: a tiling change of the current world).
    scene: Option<Box<Scene>>,
    /// Whether to run once it is in place: a tiling change resumes if it was running, a scene
    /// as it was asked to.
    play_after: bool,
}

enum Built {
    World(Box<World>, Geometry),
    Topology(Arc<Topology>, Geometry),
}

/// Roughly how many cells a tiling spec builds, if it is large enough to say.
fn estimated_cells(spec: &TilingSpec) -> u64 {
    match spec.size {
        Some(SizeSpec::Level(l)) => aperiodic::tile_count(&spec.family, l).unwrap_or(0) as u64,
        Some(SizeSpec::Cells(c)) => c as u64,
        Some(SizeSpec::Rect { w, h }) => w as u64 * h as u64,
        None => 0,
    }
}

/// Whether building this tiling should not block the window.
pub(super) fn heavy(spec: &TilingSpec) -> bool {
    estimated_cells(spec) >= BACKGROUND_CELLS
}

fn describe(spec: &TilingSpec) -> String {
    let fam = jetcarp_core::families::family(&spec.family)
        .map(|f| f.name)
        .unwrap_or(&spec.family);
    match spec.size {
        Some(SizeSpec::Level(l)) => format!("{fam}, {}", super::level_label(&spec.family, l)),
        _ => format!("{fam}, about {} cells", estimated_cells(spec)),
    }
}

impl JetcarpApp {
    fn start_build(
        &mut self,
        what: String,
        scene: Option<Box<Scene>>,
        play_after: bool,
        job: impl FnOnce(u64) -> Result<Built, String> + Send + 'static,
    ) {
        self.geom_counter += 1;
        let id = self.geom_counter;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("jetcarp-build".into())
            .spawn(move || {
                let _ = tx.send(job(id));
            })
            .expect("spawn build thread");
        // The overlay in the main view says what is being built.
        self.screen = Screen::Sim;
        self.playing = false;
        self.building = Some(Building {
            what,
            started: Instant::now(),
            rx,
            scene,
            play_after,
        });
    }

    /// Load a scene; a large tiling is built in the background.
    pub(super) fn load_scene_bg(&mut self, scene: &Scene) {
        self.load_scene_bg_as(scene, self.autoplay);
    }

    /// [`Self::load_scene_bg`], running once loaded or not (`play`).
    pub(super) fn load_scene_bg_as(&mut self, scene: &Scene, play: bool) {
        if !heavy(&scene.tiling) {
            return self.load_scene_as(scene, play);
        }
        let s = scene.clone();
        self.start_build(
            describe(&scene.tiling),
            Some(Box::new(scene.clone())),
            play,
            move |id| {
                let w = World::from_scene(&s)?;
                let g = Geometry::build(&w.topo, id);
                Ok(Built::World(Box::new(w), g))
            },
        );
    }

    /// Show a world built elsewhere (a fresh wallpaper grid) as `scene`'s world; a large one gets
    /// its drawing geometry in the background.
    pub(super) fn show_world_bg(&mut self, world: World, scene: Scene) {
        self.cancel_build();
        if !heavy(&world.topo.spec) && world.topo.len() < BACKGROUND_CELLS as usize {
            self.world = world;
            self.after_scene(&scene, None, self.autoplay);
            self.screen = Screen::Sim;
            return;
        }
        let what = describe(&world.topo.spec);
        let play = self.autoplay;
        self.start_build(what, Some(Box::new(scene)), play, move |id| {
            let g = Geometry::build(&world.topo, id);
            Ok(Built::World(Box::new(world), g))
        });
    }

    /// Change the current world's tiling; a large tiling is built in the background.
    pub(super) fn set_tiling_bg(&mut self, spec: TilingSpec) {
        if self.building.is_some() || !heavy(&spec) {
            self.exec(jetcarp_core::Command::SetTiling { tiling: spec });
            return;
        }
        let what = describe(&spec);
        let play = self.playing;
        self.start_build(what, None, play, move |id| {
            let topo = Arc::new(Topology::build(&spec)?);
            let g = Geometry::build(&topo, id);
            Ok(Built::Topology(topo, g))
        });
    }

    /// Forget a build in progress (something newer replaced the world).
    pub(super) fn cancel_build(&mut self) {
        self.building = None;
    }

    /// Install a finished build. Called every frame.
    pub(super) fn poll_build(&mut self, ctx: &egui::Context) {
        let Some(b) = &self.building else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let res = match b.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the build thread stopped".into()),
        };
        let b = self.building.take().expect("build in progress");
        let secs = b.started.elapsed().as_secs_f64();
        let ok = match res {
            Ok(Built::World(w, g)) => {
                self.world = *w;
                if let Some(scene) = &b.scene {
                    self.after_scene(scene, Some(g), b.play_after);
                }
                true
            }
            Ok(Built::Topology(topo, g)) => match self.world.set_tiling_built(topo) {
                Ok(_) => {
                    self.on_new_topology_with(Some(g));
                    self.after_exec();
                    self.playing = b.play_after;
                    true
                }
                Err(e) => {
                    self.message = Some((e, true));
                    false
                }
            },
            Err(e) if b.scene.is_some() => {
                self.message = Some((format!("Could not load scene: {e}"), true));
                false
            }
            Err(e) => {
                self.message = Some((e, true));
                false
            }
        };
        if ok {
            // After a scene's warnings, if it had any (a palette this JETCARP does not know).
            let built = format!("Built {} in {secs:.1} s.", b.what);
            let warnings = self.world.warnings();
            self.message = Some(if b.scene.is_some() && !warnings.is_empty() {
                (format!("{built} {}", warnings.join("; ")), false)
            } else {
                (built, false)
            });
        }
        if let Some(f) = &mut self.frametime {
            println!("frametime: built {} in {secs:.2} s", b.what);
            f.restart();
        }
        self.screen = Screen::Sim;
    }

    /// Commands wait while a build runs (they would apply to the world being replaced).
    pub(super) fn build_busy(&self) -> Option<String> {
        self.building
            .as_ref()
            .map(|b| format!("Building {}; wait for it to finish.", b.what))
    }

    /// The viewport's overlay while building.
    pub(super) fn build_overlay(&self, ui: &egui::Ui, rect: egui::Rect) {
        let Some(b) = &self.building else { return };
        let p = ui.painter();
        p.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
        let t = b.started.elapsed().as_secs_f64();
        let spin = ["|", "/", "-", "\\"][(t * 8.0) as usize % 4];
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{spin}  Building {}…  {t:.1} s", b.what),
            egui::FontId::proportional(18.0),
            Color32::from_rgb(0xff, 0xd0, 0x80),
        );
        p.text(
            rect.center() + egui::vec2(0.0, 26.0),
            egui::Align2::CENTER_CENTER,
            "Level 8 patches take a few seconds to build (and about 1.3 GB). The window stays usable.",
            egui::FontId::proportional(13.0),
            Color32::from_gray(170),
        );
    }
}
