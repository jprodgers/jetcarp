//! *File → Export animation…* (#62): the view as it is now, then every `every` generations for
//! `gens` more, as an animated GIF or WebP, rendered on a copy of the world in the background.
//!
//! - **What it shows:** the grid as the view frames it (the camera's centre and zoom), scaled so
//!   the longer side is `side` pixels. The world steps on a copy on the processor, so the app keeps
//!   running meanwhile; the frames are what stepping gives, exactly as on the GPU.
//! - **The format** is the user's choice; the hunts' *Render media* picks it by size instead.
//! - **Memory:** every frame is held until encoding, so the frame count is capped
//!   ([`MAX_FRAMES`]).
//! - **The strobe guard** (#149): the frames are checked at their own rate before anything is
//!   saved (`jetcarp_core::strobe::export`). An animation that flashes or shows strong stripes is
//!   held, with what was found and a choice: save the softened version (the default), or cancel;
//!   with the app's guard turned off, saving it as it is is offered too.

use super::*;
use jetcarp_core::media::encode;
use jetcarp_core::render::Image;
use std::sync::{Arc, Mutex};

/// Frames an animation may have (each held until the end: 2,000 frames of 640 x 400 is about
/// 2 GB).
pub const MAX_FRAMES: u32 = 2000;

/// The window's settings, kept between exports.
pub(crate) struct AnimExport {
    pub open: bool,
    pub gens: u32,
    pub every: u32,
    pub side: u32,
    pub webp: bool,
    /// The export under way: frames done, frames in all, and its outcome once finished.
    pub job: Option<Arc<Mutex<AnimJob>>>,
}

impl Default for AnimExport {
    fn default() -> Self {
        AnimExport {
            open: false,
            gens: 200,
            every: 1,
            side: 640,
            webp: false,
            job: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct AnimJob {
    pub done: u32,
    pub total: u32,
    pub outcome: Option<Result<String, String>>,
    /// Frames the strobe guard held back, waiting for the person's choice.
    pub held: Option<Held>,
}

/// An animation the strobe guard found flashing, before it is saved.
pub(crate) struct Held {
    pub frames: Vec<Image>,
    pub delay_ms: u32,
    pub webp: bool,
    pub path: std::path::PathBuf,
    pub check: jetcarp_core::strobe::export::Check,
    /// For the message once saved.
    pub summary: String,
}

/// What the person chose for a held animation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HeldChoice {
    Softened,
    AsIs,
    Cancel,
}

impl JetcarpApp {
    pub(super) fn anim_export_window(&mut self, ctx: &egui::Context) {
        // The strobe guard held the frames back: ask what to save.
        let held = self
            .anim
            .job
            .as_ref()
            .and_then(|j| j.lock().unwrap().held.as_ref().map(|h| (h.check, h.webp)));
        if let Some((check, webp)) = held {
            if let Some(choice) = self.held_dialog(ctx, &check, webp) {
                self.save_held(choice);
            }
            return;
        }
        if let Some(job) = &self.anim.job {
            let outcome = job.lock().unwrap().outcome.take();
            if let Some(o) = outcome {
                self.message = Some(match o {
                    Ok(m) => (m, false),
                    Err(e) => (format!("Could not export the animation: {e}"), true),
                });
                self.anim.job = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        if !self.anim.open {
            return;
        }
        let mut open = true;
        let mut go = false;
        egui::Window::new("Export animation")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("The view as it is now, then the generations after it.");
                egui::Grid::new("anim-export").num_columns(2).show(ui, |ui| {
                    ui.label("Generations");
                    ui.add(egui::DragValue::new(&mut self.anim.gens).range(1..=100_000));
                    ui.end_row();
                    ui.label("A frame every");
                    ui.add(
                        egui::DragValue::new(&mut self.anim.every)
                            .range(1..=10_000)
                            .suffix(" gen"),
                    );
                    ui.end_row();
                    ui.label("Longer side");
                    ui.add(
                        egui::DragValue::new(&mut self.anim.side)
                            .range(64..=4096)
                            .suffix(" px"),
                    );
                    ui.end_row();
                    ui.label("Format");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.anim.webp, false, "GIF");
                        ui.radio_value(&mut self.anim.webp, true, "WebP (lossless)");
                    });
                    ui.end_row();
                });
                let frames = self.anim.gens / self.anim.every.max(1) + 1;
                if frames > MAX_FRAMES {
                    ui.colored_label(
                        crate::theme::ink(ui, egui::Color32::LIGHT_RED),
                        format!(
                            "{frames} frames is over the limit of {MAX_FRAMES}: fewer generations, or a frame every more"
                        ),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!(
                            "{frames} frames, rendered on the processor in the background"
                        ))
                        .small()
                        .weak(),
                    );
                }
                ui.separator();
                match &self.anim.job {
                    Some(job) => {
                        let j = job.lock().unwrap();
                        ui.add(
                            egui::ProgressBar::new(j.done as f32 / j.total.max(1) as f32)
                                .text(format!("frame {} of {}", j.done, j.total)),
                        );
                    }
                    None => {
                        go = ui
                            .add_enabled(frames <= MAX_FRAMES, egui::Button::new("Export…"))
                            .clicked();
                    }
                }
            });
        self.anim.open = open;
        if go {
            self.start_anim_export();
        }
    }

    fn start_anim_export(&mut self) {
        let (ext, filter) = if self.anim.webp {
            ("webp", "Animated WebP")
        } else {
            ("gif", "Animated GIF")
        };
        crate::file_dialog::open(&self.dialog_ctx(), "anim.export", move || {
            rfd::FileDialog::new()
                .add_filter(filter, &[ext])
                .set_file_name(format!("jetcarp.{ext}"))
                .save_file()
        });
    }

    /// Called every frame: starts the animation export once its file has been chosen (#136).
    pub(super) fn anim_export_answer(&mut self) {
        if let Some(path) = crate::file_dialog::take::<std::path::PathBuf>("anim.export") {
            self.anim_export_to(path);
        }
    }

    fn anim_export_to(&mut self, path: std::path::PathBuf) {
        let [cw, ch] = self.canvas_px();
        let f = self.anim.side as f64 / cw.max(ch).max(1.0);
        let (w, h) = (
            (cw * f).round().max(1.0) as u32,
            (ch * f).round().max(1.0) as u32,
        );
        let (center, scale) = (self.cam, self.zoom * f);
        let (gens, every, webp) = (self.anim.gens, self.anim.every.max(1), self.anim.webp);
        // The frames play at the speed the world runs at here (30 a second at Max speed).
        let per_s = if self.max_speed {
            30.0
        } else {
            (self.speed as f64 / every as f64).clamp(1.0, 50.0)
        };
        let delay_ms = (1000.0 / per_s).round() as u32;
        let mut world = self.world.clone();
        let total = gens / every + 1;
        let job = Arc::new(Mutex::new(AnimJob {
            total,
            ..Default::default()
        }));
        self.anim.job = Some(job.clone());
        std::thread::spawn(move || {
            let start = world.generation;
            let frame = Frame {
                width: w,
                height: h,
                center,
                scale,
            };
            let frames = match render_frames(&mut world, &frame, gens, every, &|k| {
                job.lock().unwrap().done = k;
            }) {
                Ok(f) => f,
                Err(e) => {
                    job.lock().unwrap().outcome = Some(Err(e));
                    return;
                }
            };
            let summary = format!(
                "{} ({w}x{h}, {total} frames, generations {start} to {})",
                path.display(),
                world.generation
            );
            // The strobe guard: checked before anything is saved.
            let refs: Vec<&Image> = frames.iter().collect();
            let check = jetcarp_core::strobe::export::check(
                &refs,
                delay_ms,
                jetcarp_core::photosensitivity::at_risk(),
            );
            if check.harmful {
                job.lock().unwrap().held = Some(Held {
                    frames,
                    delay_ms,
                    webp,
                    path,
                    check,
                    summary,
                });
                return;
            }
            let outcome = encode_frames(&frames, webp, delay_ms).and_then(|b| {
                std::fs::write(&path, &b).map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(format!("Exported {summary}"))
            });
            job.lock().unwrap().outcome = Some(outcome);
        });
    }

    /// The choice for a held animation: what was found, *Save the softened version* (the
    /// default), *Save it as it is* (only with the app's guard off), *Cancel*.
    fn held_dialog(
        &mut self,
        ctx: &egui::Context,
        check: &jetcarp_core::strobe::export::Check,
        webp: bool,
    ) -> Option<HeldChoice> {
        let mut choice = None;
        let guard_on = self.strobe.on_app;
        egui::Modal::new(egui::Id::unique("strobe-guard-export")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading("This animation flashes");
            ui.add_space(4.0);
            ui.label(format!(
                "The {} {}. Anyone who opens it would see that, and flashing like this can trigger \
                 seizures in people with photosensitive epilepsy.",
                if webp { "WebP" } else { "GIF" },
                check.words().unwrap_or_else(|| "flashes".into())
            ));
            ui.label(
                "The strobe guard can save a version that does not flash: shown more slowly, when \
                 a little slower is enough, or else with its flashing softened and strong stripes \
                 veiled.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let soft = ui.add(egui::Button::new(
                    RichText::new("Save the softened version").strong(),
                ));
                if !soft.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                    soft.request_focus();
                }
                if soft.clicked() {
                    choice = Some(HeldChoice::Softened);
                }
                if !guard_on && ui.button("Save it as it is").clicked() {
                    choice = Some(HeldChoice::AsIs);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(HeldChoice::Cancel);
                }
            });
            if guard_on {
                ui.label(
                    RichText::new(
                        "Saving it as it is needs the strobe guard turned off (Settings → Safety).",
                    )
                    .small()
                    .weak(),
                );
            }
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(HeldChoice::Cancel);
        }
        choice
    }

    /// Saves (or drops) the held animation as chosen, in the background.
    fn save_held(&mut self, choice: HeldChoice) {
        let Some(job) = self.anim.job.clone() else {
            return;
        };
        let Some(held) = job.lock().unwrap().held.take() else {
            return;
        };
        if choice == HeldChoice::Cancel {
            self.anim.job = None;
            self.message = Some((
                "Not exported: the animation flashes, and it was not saved.".into(),
                false,
            ));
            return;
        }
        // As it is only with the app's guard off, whatever asked.
        let soften = choice == HeldChoice::Softened || self.strobe.on_app;
        std::thread::spawn(move || {
            let Held {
                frames,
                delay_ms,
                webp,
                path,
                summary,
                ..
            } = held;
            let outcome = guarded_bytes(&frames, webp, delay_ms, soften).and_then(|g| {
                std::fs::write(&path, &g.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(match g.note() {
                    Some(n) => format!("Exported {summary}: {n}."),
                    None => format!("Exported {summary}"),
                })
            });
            job.lock().unwrap().outcome = Some(outcome);
        });
    }
}

/// Renders `world` now and after every `every` generations up to `gens` more (stepping `world`).
/// `done(k)` hears each frame finished.
pub(crate) fn render_frames(
    world: &mut World,
    frame: &Frame,
    gens: u32,
    every: u32,
    done: &dyn Fn(u32),
) -> Result<Vec<Image>, String> {
    let every = every.max(1);
    let total = gens / every + 1;
    if total > MAX_FRAMES {
        return Err(format!("{total} frames is over the limit of {MAX_FRAMES}"));
    }
    let opts = render::ViewOptions {
        outline_min_px: 0.0,
        repeat: false,
        colors: None,
    };
    let mut frames = Vec::with_capacity(total as usize);
    for k in 0..total {
        if k > 0 {
            for _ in 0..every {
                world.step();
            }
        }
        frames.push(render::render_view(
            world,
            frame.width,
            frame.height,
            frame.center,
            frame.scale,
            &opts,
        ));
        done(k + 1);
    }
    Ok(frames)
}

/// The frames as a GIF or a lossless WebP, `delay_ms` each.
pub(crate) fn encode_frames(
    frames: &[Image],
    webp: bool,
    delay_ms: u32,
) -> Result<Vec<u8>, String> {
    let refs: Vec<&Image> = frames.iter().collect();
    if webp {
        let p = &frames[0].rgba;
        encode::webp(&refs, delay_ms, [p[0], p[1], p[2], 255])
    } else {
        encode::gif(&refs, delay_ms)
    }
}

/// The frames through the strobe guard: softened if they flash and `soften`, checked again as
/// encoded.
pub(crate) fn guarded_bytes(
    frames: &[Image],
    webp: bool,
    delay_ms: u32,
    soften: bool,
) -> Result<jetcarp_core::strobe::export::Guarded, String> {
    let background = frames
        .first()
        .map_or([0, 0, 0, 255], |f| [f.rgba[0], f.rgba[1], f.rgba[2], 255]);
    let enc = |f: &[&Image], d: u32| {
        if webp {
            encode::webp(f, d, background)
        } else {
            encode::gif(f, d)
        }
    };
    let dec = |b: &[u8]| {
        if webp {
            encode::decode_webp(b)
        } else {
            encode::decode_gif(b)
        }
    };
    jetcarp_core::strobe::export::guard(
        frames,
        delay_ms,
        soften,
        jetcarp_core::photosensitivity::at_risk(),
        &enc,
        &dec,
    )
}

/// The part of the world a frame shows: `width` x `height` pixels, world point `center` in the
/// middle, `scale` pixels per world unit.
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    pub center: [f64; 2],
    pub scale: f64,
}

/// Renders `world` now and after every `every` generations up to `gens` more (stepping `world`),
/// and encodes the frames as a GIF or a lossless WebP, `delay_ms` each, with no strobe guard (the
/// tests' check of the frames). `done(k)` hears each frame finished.
#[cfg(test)]
pub(crate) fn animate(
    world: &mut World,
    frame: &Frame,
    gens: u32,
    every: u32,
    webp: bool,
    delay_ms: u32,
    done: &dyn Fn(u32),
) -> Result<Vec<u8>, String> {
    let frames = render_frames(world, frame, gens, every, done)?;
    encode_frames(&frames, webp, delay_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strobe guard on an export (#149): a Margolus Tron field, which flips every generation,
    /// exported at 20 frames a second, is found harmful; its softened version, as saved in either
    /// format and read back, is not.
    #[test]
    fn a_flashing_export_is_caught_and_its_softened_version_passes() {
        let scene: Scene = serde_json::from_str(
            r#"{"name": "Tron flip", "tiling": {"family": "square", "size": {"rect": {"w": 40, "h": 30}}, "boundary": "wrap"}, "rng_state": 1, "cas": [{"rule": "Margolus Tron", "seed": "empty"}]}"#,
        )
        .unwrap();
        let mut w = World::from_scene(&scene).unwrap();
        let b = w.topo.bounds;
        let frame = Frame {
            width: 160,
            height: 120,
            center: [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0],
            scale: 160.0 / (b[2] - b[0]),
        };
        let frames = render_frames(&mut w, &frame, 40, 1, &|_| {}).unwrap();
        let refs: Vec<&Image> = frames.iter().collect();
        let check = jetcarp_core::strobe::export::check(&refs, 50, false);
        assert!(check.harmful, "{check:?}");
        for webp in [false, true] {
            let g = guarded_bytes(&frames, webp, 50, true).unwrap();
            assert!(!g.after.harmful, "webp {webp}: {:?}", g.after);
            let back = if webp {
                encode::decode_webp(&g.bytes).unwrap()
            } else {
                encode::decode_gif(&g.bytes).unwrap()
            };
            let back: Vec<&Image> = back.iter().collect();
            assert!(
                !jetcarp_core::strobe::export::check(
                    &back,
                    g.delay_ms,
                    jetcarp_core::photosensitivity::at_risk()
                )
                .harmful
            );
            assert!(g.note().unwrap().contains("strobe guard saved"));
            // Unguarded, the same frames are saved as drawn.
            let raw = guarded_bytes(&frames, webp, 50, false).unwrap();
            assert_eq!(raw.bytes, encode_frames(&frames, webp, 50).unwrap());
        }
    }

    /// A GIF and a WebP of a Life world have a frame now and one every `every` generations, the
    /// last one the world as stepping leaves it, and the world is that many generations on.
    #[test]
    fn an_animation_has_every_frame_and_ends_where_stepping_does() {
        use jetcarp_core::topology::{SizeSpec, TilingSpec};
        let spec = TilingSpec::new("square")
            .with_size(SizeSpec::Rect { w: 32, h: 32 })
            .with_boundary(Boundary::Wrap);
        let mut base = World::new(spec, 7).unwrap();
        let id = base
            .apply(Command::AddCa {
                ca: jetcarp_core::world::CaSpec::new("B3/S23"),
            })
            .unwrap()
            .new_id
            .unwrap();
        base.apply(Command::Seed {
            id,
            seed: SeedSpec::Random { density: 0.4 },
        })
        .unwrap();
        let b = base.topo.bounds;
        let frame = Frame {
            width: 64,
            height: 64,
            center: [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0],
            scale: 64.0 / (b[2] - b[0]).max(b[3] - b[1]),
        };
        for webp in [false, true] {
            let mut w = base.clone();
            let start = w.generation;
            let bytes = animate(&mut w, &frame, 10, 2, webp, 50, &|_| {}).unwrap();
            assert_eq!(w.generation, start + 10);
            let frames = if webp {
                encode::decode_webp(&bytes).unwrap()
            } else {
                encode::decode_gif(&bytes).unwrap()
            };
            assert_eq!(frames.len(), 6);
            let last = render::render_view(
                &w,
                64,
                64,
                frame.center,
                frame.scale,
                &render::ViewOptions {
                    outline_min_px: 0.0,
                    repeat: false,
                    colors: None,
                },
            );
            assert_eq!(frames[5].rgba, last.rgba, "the last frame (webp: {webp})");
        }
        base.step();
        assert!(animate(&mut base, &frame, 5000, 1, false, 50, &|_| {}).is_err());
    }
}
