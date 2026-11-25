//! A record of what the strobe guard's GPU pass reads and what the guard decides, frame by frame,
//! on the whole corpus (`docs/scenes/strobe/`), for comparing two builds (#343): a change to how
//! the guard meters must leave both exactly as they were, the readings to the bit.
//!
//! Ignored; needs `JETCARP_STROBE_VERDICTS=DIR` and writes two files there:
//! - `readings.txt`: each frame's readings, as a digest of every bit of its tile maps (as drawn
//!   and as shown), its half image, and the frame shown when the pass softened it; for every
//!   recording of the corpus drawn at 333 by 251 (partial tiles at two edges), read in both ways
//!   the pass reads (8-bit frames by table, others decoded), measured alone and with every tile
//!   softened and veiled;
//! - `verdicts.jsonl`: the guard's report and notes at each frame, on the corpus as the desktop
//!   painter draws it (read in place and copied) and as the app's view draws it, for the guard's
//!   rules for everyone and for someone at risk.
//!
//! Run it on both builds, on each adapter (`JETCARP_GPU`), and compare the files:
//! `cargo test --release -p jetcarp-display --test strobe_record -- --ignored --nocapture`.

use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::frame::TileMap;
use jetcarp_core::strobe::guard::Pace;
use jetcarp_core::strobe::soften::Plan;
use jetcarp_core::{Scene, World};
use jetcarp_display::caption::CaptionSettings;
use jetcarp_display::gpu::{Geometry, TileRenderer, slot_capacity};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{Profile, Show};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Stage, no_lookup};
use jetcarp_display::strobe_gpu::{GpuReading, GuardedGpu, Shown, StrobeGpu};
use jetcarp_display::view::{ViewRequest, Views, place};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Time enough to step every generation due at each frame, so each run steps the same ones.
const STEP_BUDGET: Duration = Duration::from_secs(10);

/// The corpus's recordings, as `strobe_painter.rs` lists them: name, scene, speed, pixels a cell,
/// outlines (drawn as the app's view draws them).
const CORPUS: &[(&str, &str, f32, f32, bool)] = &[
    ("tron-field", "tron-field", 60.0, 4.0, false),
    ("tron-red", "tron-red", 30.0, 4.0, false),
    ("tron-blob", "tron-blob", 30.0, 4.0, false),
    ("seeds-soup", "seeds-soup", 30.0, 4.0, false),
    ("hodgepodge", "hodgepodge", 60.0, 4.0, false),
    ("stripes-still", "stripes-still", 10.0, 4.0, false),
    ("tron-checkerboard", "tron-checkerboard", 30.0, 4.0, false),
    ("life-soup", "life-soup", 30.0, 4.0, false),
    ("brians-brain", "brians-brain", 30.0, 4.0, false),
    ("maze", "maze", 30.0, 4.0, false),
    ("cyclic-waves", "cyclic-waves", 15.0, 4.0, false),
    ("live-field-4.37px", "live-field", 30.0, 4.37, false),
    ("live-field-7.7px", "live-field", 30.0, 7.7, false),
    ("live-field-outlined-8px", "live-field", 30.0, 8.0, true),
    ("live-field-outlined-12px", "live-field", 30.0, 12.0, true),
    ("live-field-outlined-16px", "live-field", 30.0, 16.0, true),
    ("paper-field-outlined-8px", "paper-field", 30.0, 8.0, true),
    ("paper-field-outlined-16px", "paper-field", 30.0, 16.0, true),
];

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> Option<Gpu> {
    const WHAT: &str = "the strobe guard's record";
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let choice =
        match jetcarp_gpu::select::pick(&instance, jetcarp_gpu::select::Purpose::Display, None) {
            Ok(c) => c,
            Err(e) => {
                jetcarp_gpu::no_gpu(WHAT, &e);
                return None;
            }
        };
    let desc = jetcarp_display::painter::device_descriptor(&choice.adapter);
    let (device, queue) = pollster::block_on(choice.adapter.request_device(&desc)).ok()?;
    println!("adapter: {}", choice.adapter.get_info().name);
    Some(Gpu { device, queue })
}

impl Gpu {
    fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    fn texture(
        &self,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
        readable: bool,
    ) -> wgpu::Texture {
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        if readable {
            usage |= wgpu::TextureUsages::TEXTURE_BINDING;
        }
        let views = jetcarp_display::strobe_gpu::view_formats(format);
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: if readable { &views } else { &[] },
        })
    }

    fn upload(&self, tex: &wgpu::Texture, bytes: &[u8], bytes_per_px: u32) {
        let (w, h) = (tex.width(), tex.height());
        self.queue.write_texture(
            tex.as_image_copy(),
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * bytes_per_px),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// One frame through the pass, waited for.
    fn frame(
        &self,
        s: &mut StrobeGpu,
        tex: &wgpu::Texture,
        plan: &Plan,
        t: f64,
        half: bool,
    ) -> (Shown, GpuReading) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let shown = s.process(
            &self.device,
            &self.queue,
            &mut enc,
            Some(tex),
            plan,
            1.0 / 60.0,
            t,
            half,
        );
        self.queue.submit([enc.finish()]);
        s.after_submit();
        self.wait();
        let mut r = s.take(&self.device);
        assert_eq!(r.len(), 1, "one reading");
        (shown, r.remove(0))
    }
}

/// FNV-1a, 64 bits.
#[derive(Clone, Copy)]
struct Digest(u64);

impl Digest {
    fn new() -> Digest {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= *x as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.bytes(&x.to_bits().to_le_bytes());
        }
    }
    fn map(&mut self, m: &TileMap) {
        self.f32s(&m.lum);
        self.f32s(&m.red);
        for f in m.fine.iter().chain(&m.red_fine) {
            self.f32s(&[f.light, f.dark]);
            self.bytes(&f.sig.to_le_bytes());
            self.bytes(&f.sign.to_le_bytes());
        }
        for c in &m.rgb {
            self.f32s(c);
        }
    }
}

fn stage_at(sc: Scene, speed: f32, cell_px: f32, w: u32, h: u32) -> Stage {
    let profile = Profile {
        name: "record".into(),
        show: Show::Scene {
            scene: Box::new(sc),
        },
        cell_px,
        refill: false,
        speed,
        gpu: false,
        ..Default::default()
    };
    let slot = Slot {
        label: "record".into(),
        profile,
        stored: false,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
        series: None,
    };
    let rotation = Rotation {
        name: "record".into(),
        ..Default::default()
    };
    let mut stage = Stage::new(
        &rotation,
        vec![slot],
        1,
        [w as f64, h as f64],
        no_lookup(),
        1 << 24,
    )
    .unwrap();
    let mut sim = SimBackend::unavailable("a test".into());
    let t0 = Instant::now();
    while stage.world().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never built");
        stage.tick(0.0, &mut sim, Duration::from_millis(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    stage
}

fn corpus_scene(name: &str) -> Scene {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/scenes/strobe")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// A world drawn as the app's view draws it, with outlines.
struct ViewDraw {
    world: World,
    geometry: Arc<Geometry>,
    views: Views,
    renderer: TileRenderer,
    center: [f64; 2],
    zoom: f64,
    speed: f32,
    size: [f32; 2],
}

impl ViewDraw {
    fn new(device: &wgpu::Device, sc: &Scene, speed: f32, cell_px: f32, w: u32, h: u32) -> Self {
        let world = World::from_scene(sc).unwrap();
        let geometry = Arc::new(Geometry::build(&world.topo, 1));
        let (center, zoom) = place(&world, [w as f64, h as f64], cell_px as f64);
        ViewDraw {
            world,
            geometry,
            views: Views::new(slot_capacity(device)),
            renderer: TileRenderer::new(device, FORMAT),
            center,
            zoom,
            speed,
            size: [w as f32, h as f32],
        }
    }

    /// Steps to the generation due at `t` and draws into `enc`'s pass on `view`.
    fn draw(&mut self, g: &Gpu, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, t: f64) {
        let due = ((t - 1.0) * self.speed as f64 + 1e-9).floor();
        while (self.world.generation as f64) < due {
            self.world.step();
        }
        let req = ViewRequest {
            view: 0,
            center: self.center,
            zoom: self.zoom,
            size: self.size,
            outlines: true,
        };
        let epoch = self.world.generation;
        let (d, _) = self.views.draw(&self.world, &self.geometry, epoch, req);
        self.renderer.prepare(&g.device, &g.queue, &d);
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.renderer.paint(&mut p, &d);
    }
}

/// The frames of one recording, `n` of them at 60 a second, as RGBA bytes: drawn by the painter
/// with no guard, or as the app's view draws them.
fn frames(g: &Gpu, rec: &(&str, &str, f32, f32, bool), w: u32, h: u32, n: u32) -> Vec<Vec<u8>> {
    let (_, scene, speed, px, outlines) = *rec;
    let sc = corpus_scene(scene);
    let tex = g.texture(FORMAT, w, h, false);
    let view = tex.create_view(&Default::default());
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    let mut painted: Result<(Stage, Painter), ViewDraw> = if outlines {
        Err(ViewDraw::new(&g.device, &sc, speed, px, w, h))
    } else {
        Ok((
            stage_at(sc, speed, px, w, h),
            Painter::new(&g.device, FORMAT),
        ))
    };
    let mut sim = SimBackend::unavailable("a test".into());
    let mut out = vec![];
    for k in 0..n {
        let t = 1.0 + k as f64 / 60.0;
        match &mut painted {
            Ok((stage, painter)) => {
                stage.tick(t, &mut sim, STEP_BUDGET);
                painter.paint(&g.device, &g.queue, &tex, [w, h], 1.0, stage, &caption, t);
            }
            Err(v) => {
                let mut enc = g.device.create_command_encoder(&Default::default());
                v.draw(g, &mut enc, &view, t);
                g.queue.submit([enc.finish()]);
            }
        }
        out.push(
            jetcarp_display::crop::read(&g.device, &g.queue, &tex, [0, 0], [w, h])
                .unwrap()
                .rgba,
        );
    }
    out
}

/// An 8-bit frame as 32-bit floats, each channel its byte over 255 (read by decoding, not by table).
fn as_floats(rgba: &[u8]) -> Vec<u8> {
    rgba.iter()
        .flat_map(|b| (*b as f32 / 255.0).to_le_bytes())
        .collect()
}

/// The readings of every frame of the corpus, in every way the pass reads and softens.
fn readings(g: &Gpu, out: &mut String) {
    let (w, h) = (333, 251);
    for rec in CORPUS {
        let fr = frames(g, rec, w, h, 120);
        for (format, name) in [
            (wgpu::TextureFormat::Rgba8Unorm, "table"),
            (wgpu::TextureFormat::Rgba32Float, "decoded"),
        ] {
            for soften in [false, true] {
                let tex = g.texture(format, w, h, true);
                let mut s = StrobeGpu::new(&g.device);
                let mut plan = Plan::none_for(w, h);
                for (k, f) in fr.iter().enumerate() {
                    if format == wgpu::TextureFormat::Rgba8Unorm {
                        g.upload(&tex, f, 4);
                    } else {
                        g.upload(&tex, &as_floats(f), 16);
                    }
                    if soften {
                        // Softened tiles and veils that move about from frame to frame.
                        for (i, (s, v)) in plan.soften.iter_mut().zip(&mut plan.veil).enumerate() {
                            *s = (i + k / 7) % 3 != 0;
                            *v = if (i + k) % 5 == 0 {
                                [0.0, 0.0]
                            } else {
                                [
                                    ((i * 7 + k) % 10) as f32 / 10.0,
                                    ((i + 3 * k) % 9) as f32 / 9.0,
                                ]
                            };
                        }
                        plan.lum_rate = 1.0 + (k % 4) as f32;
                        plan.red_rate = 50.0 + (k % 3) as f32 * 40.0;
                    }
                    let half = k % 15 == 0 || k % 15 == 7;
                    let (shown, r) = g.frame(&mut s, &tex, &plan, k as f64 / 60.0, half);
                    let mut d = Digest::new();
                    d.map(&r.input);
                    d.map(&r.output);
                    if let Some(hf) = &r.half {
                        d.bytes(&hf.width.to_le_bytes());
                        d.f32s(&hf.lum);
                    }
                    if shown == Shown::Softened {
                        let t = s.softened_texture().expect("softened");
                        let img =
                            jetcarp_display::crop::read(&g.device, &g.queue, t, [0, 0], [w, h])
                                .unwrap();
                        d.bytes(&img.rgba);
                    }
                    let _ = writeln!(
                        out,
                        "{} {name} {} {k} {:?} {:016x}",
                        rec.0,
                        if soften { "softened" } else { "measured" },
                        shown,
                        d.0
                    );
                }
            }
        }
    }
}

/// The guard's report and notes at each frame of the corpus, as the desktop painter and the app's
/// view show it, for everyone and for someone at risk.
fn verdicts(g: &Gpu, out: &mut String) {
    let (w, h) = (320, 240);
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    for rec in CORPUS {
        let (name, scene, speed, px, outlines) = *rec;
        let sc = corpus_scene(scene);
        let ways: &[&str] = if outlines {
            &["view"]
        } else {
            &["read in place", "copied"]
        };
        for way in ways {
            for at_risk in [false, true] {
                let readable = *way != "copied";
                let tex = g.texture(FORMAT, w, h, readable);
                let view = tex.create_view(&Default::default());
                let mut sim = SimBackend::unavailable("a test".into());
                let mut drawn: Result<(Stage, Painter), (ViewDraw, GuardedGpu)> = if outlines {
                    let mut gg = GuardedGpu::new(&g.device, Viewing::screen(1.0), None, true);
                    gg.guard.set_at_risk(at_risk);
                    Err((ViewDraw::new(&g.device, &sc, speed, px, w, h), gg))
                } else {
                    let mut p = Painter::guarded(&g.device, FORMAT, true);
                    p.set_guard_at_risk(at_risk);
                    p.set_readable(readable);
                    Ok((stage_at(sc.clone(), speed, px, w, h), p))
                };
                for k in 0..360u32 {
                    let t = 1.0 + k as f64 / 60.0;
                    let (report, notes, generation) = match &mut drawn {
                        Ok((stage, painter)) => {
                            stage.tick(t, &mut sim, STEP_BUDGET);
                            painter.paint(
                                &g.device,
                                &g.queue,
                                &tex,
                                [w, h],
                                1.0,
                                stage,
                                &caption,
                                t,
                            );
                            g.wait();
                            let (cap, paused) = painter.guard_hold();
                            stage.set_guard(painter.id(), t, cap, paused);
                            (
                                painter.guard_report(),
                                painter.take_guard_notes(),
                                stage.world().map_or(0, |w| w.generation),
                            )
                        }
                        Err((v, gg)) => {
                            gg.update(
                                &g.device,
                                Pace {
                                    speed: v.speed,
                                    reached: Some(v.speed),
                                    frame_rate: 60.0,
                                },
                            );
                            let mut enc = g.device.create_command_encoder(&Default::default());
                            v.draw(g, &mut enc, &view, t);
                            if gg.frame(&g.device, &g.queue, &mut enc, Some(&tex), t)
                                == Shown::Softened
                            {
                                gg.gpu.prepare_blit(&g.device, FORMAT, Some(&tex));
                            }
                            g.queue.submit([enc.finish()]);
                            gg.gpu.after_submit();
                            g.wait();
                            (
                                Some(gg.guard.report()),
                                gg.guard.take_notes(),
                                v.world.generation,
                            )
                        }
                    };
                    let line = serde_json::json!({
                        "rec": name,
                        "way": way,
                        "at_risk": at_risk,
                        "frame": k,
                        "generation": generation,
                        "report": report,
                        "notes": notes,
                    });
                    let _ = writeln!(out, "{line}");
                }
            }
        }
    }
}

#[test]
#[ignore]
fn record_readings_and_verdicts() {
    let Some(dir) = std::env::var_os("JETCARP_STROBE_VERDICTS").map(PathBuf::from) else {
        eprintln!("skipped: set JETCARP_STROBE_VERDICTS to a folder");
        return;
    };
    let Some(g) = gpu() else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = String::new();
    readings(&g, &mut r);
    std::fs::write(dir.join("readings.txt"), &r).unwrap();
    let mut v = String::new();
    verdicts(&g, &mut v);
    std::fs::write(dir.join("verdicts.jsonl"), &v).unwrap();
    println!(
        "wrote {} readings and {} verdicts to {}",
        r.lines().count(),
        v.lines().count(),
        dir.display()
    );
}
