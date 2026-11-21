//! The shared painter through the strobe guard (#148, #149): what the desktop background and the
//! screen savers draw. A stage showing a Margolus Tron field, which flips every generation, run at
//! 60 generations a second and drawn offscreen at 60 frames a second of simulated time; every frame
//! read back and measured on the processor. Both ways the painter can draw: into a target it reads
//! in place, and into its own texture, copied to the target. Each test skips itself when the
//! machine has no GPU adapter. `JETCARP_STROBE_RECORD=DIR` also writes each frame as a PNG there,
//! for IRIS.

use jetcarp_core::strobe::flash::FlashMeter;
use jetcarp_core::strobe::frame::reduce;
use jetcarp_core::strobe::{MAX_TRANSITIONS, Viewing};
use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
use jetcarp_display::caption::CaptionSettings;
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{Profile, Show};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Stage, no_lookup};
use std::time::{Duration, Instant};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const W: u32 = 320;
const H: u32 = 240;

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let choice =
        jetcarp_gpu::select::pick(&instance, jetcarp_gpu::select::Purpose::Display, None).ok()?;
    if let Err(e) = &choice.usable {
        eprintln!("skipped: {e}");
        return None;
    }
    pollster::block_on(
        choice
            .adapter
            .request_device(&wgpu::DeviceDescriptor::default()),
    )
    .ok()
}

/// A stage showing a Margolus Tron field on an empty background (it flips every generation) at
/// `speed` generations a second.
fn tron(speed: f32) -> Stage {
    let mut sc = Scene::new(TilingSpec::rect("square", 80, 60, Boundary::Wrap));
    sc.cas = vec![CaSpec::new("Margolus Tron").seed(SeedSpec::Empty)];
    stage_of(sc, speed)
}

/// A stage showing `sc` at `speed` generations a second, at 4 pixels a cell.
fn stage_of(sc: Scene, speed: f32) -> Stage {
    stage_at(sc, speed, 4.0)
}

/// A stage showing `sc` at `speed` generations a second, at `cell_px` pixels a cell.
fn stage_at(sc: Scene, speed: f32, cell_px: f32) -> Stage {
    let profile = Profile {
        name: "tron".into(),
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
        label: "tron".into(),
        profile,
        stored: false,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
    };
    let rotation = Rotation {
        name: "test".into(),
        ..Default::default()
    };
    let mut stage = Stage::new(
        &rotation,
        vec![slot],
        1,
        [W as f64, H as f64],
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

fn target(device: &wgpu::Device, readable: bool) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    if readable {
        usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screen"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: if readable {
            &[wgpu::TextureFormat::Rgba8UnormSrgb]
        } else {
            &[]
        },
    })
}

/// Runs `secs` seconds of frames at 60 a second; returns the most transitions in a second on the
/// screen, and the stage after.
fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    readable: bool,
    on: bool,
    secs: f64,
) -> (u32, Stage) {
    let mut stage = tron(60.0);
    let tex = target(device, readable);
    let mut painter = Painter::guarded(device, FORMAT, on);
    painter.set_readable(readable);
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    let mut sim = SimBackend::unavailable("a test".into());
    let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
    let record = std::env::var_os("JETCARP_STROBE_RECORD").map(std::path::PathBuf::from);
    let mut worst = 0;
    let frames = (secs * 60.0) as u32;
    for k in 0..frames {
        let t = 1.0 + k as f64 / 60.0;
        stage.tick(t, &mut sim, Duration::from_millis(20));
        painter.paint(device, queue, &tex, [W, H], 1.0, &stage, &caption, t);
        let img = jetcarp_display::crop::read(device, queue, &tex, [0, 0], [W, H]).unwrap();
        let r = meter.update(&reduce(&img.rgba, W, H), t);
        worst = worst.max(r.transitions.max(r.red_transitions));
        let (cap, paused) = painter.guard_hold();
        stage.set_guard(painter.id(), t, cap, paused);
        stage.set_guard_report(painter.guard_report());
        if let Some(d) = &record {
            let name = format!(
                "painter-{}-{}-{k:05}.png",
                if readable { "readable" } else { "copied" },
                if on { "on" } else { "off" }
            );
            std::fs::create_dir_all(d).unwrap();
            jetcarp_core::render::write_png(&d.join(name), &img, &[]).unwrap();
        }
    }
    (worst, stage)
}

/// One recording of the corpus (`docs/scenes/strobe/`): the scene, the speed it runs at, the
/// pixels a cell is drawn, and whether cell outlines are drawn over it.
struct Rec {
    name: &'static str,
    scene: &'static str,
    speed: f32,
    cell_px: f32,
    outlines: bool,
}

const fn rec(name: &'static str, scene: &'static str, speed: f32, cell_px: f32) -> Rec {
    Rec {
        name,
        scene,
        speed,
        cell_px,
        outlines: false,
    }
}

const fn outlined(name: &'static str, scene: &'static str, cell_px: f32) -> Rec {
    Rec {
        name,
        scene,
        speed: 30.0,
        cell_px,
        outlines: true,
    }
}

/// Every recording of the corpus. Those without outlines are drawn by the shared painter (the
/// desktop background and the screen savers); those with outlines as the app's view draws them,
/// the outlines fading in from 6 pixels a cell and full from 16 (the painter draws none).
const CORPUS: &[Rec] = &[
    rec("tron-field", "tron-field", 60.0, 4.0),
    rec("tron-red", "tron-red", 30.0, 4.0),
    rec("tron-blob", "tron-blob", 30.0, 4.0),
    rec("seeds-soup", "seeds-soup", 30.0, 4.0),
    rec("hodgepodge", "hodgepodge", 60.0, 4.0),
    rec("stripes-still", "stripes-still", 10.0, 4.0),
    rec("tron-checkerboard", "tron-checkerboard", 30.0, 4.0),
    rec("life-soup", "life-soup", 30.0, 4.0),
    rec("brians-brain", "brians-brain", 30.0, 4.0),
    rec("maze", "maze", 30.0, 4.0),
    rec("cyclic-waves", "cyclic-waves", 15.0, 4.0),
    // A field of live cells at fractional sizes, where seams between cells would show.
    rec("live-field-4.37px", "live-field", 30.0, 4.37),
    rec("live-field-7.7px", "live-field", 30.0, 7.7),
    // The same field with the app's cell outlines: faint at 8 pixels, full at 16; and the
    // outlines over a light background.
    outlined("live-field-outlined-8px", "live-field", 8.0),
    outlined("live-field-outlined-12px", "live-field", 12.0),
    outlined("live-field-outlined-16px", "live-field", 16.0),
    outlined("paper-field-outlined-8px", "paper-field", 8.0),
    outlined("paper-field-outlined-16px", "paper-field", 16.0),
];

/// A world drawn as the app's view draws it (the tile renderer, with outlines), through the
/// strobe guard on the GPU, on or off.
struct AppView {
    world: jetcarp_core::World,
    geometry: std::sync::Arc<jetcarp_display::gpu::Geometry>,
    views: jetcarp_display::view::Views,
    renderer: jetcarp_display::gpu::TileRenderer,
    guard: jetcarp_display::strobe_gpu::GuardedGpu,
    center: [f64; 2],
    zoom: f64,
    speed: f32,
}

impl AppView {
    fn new(device: &wgpu::Device, sc: &Scene, speed: f32, cell_px: f32, on: bool) -> AppView {
        let world = jetcarp_core::World::from_scene(sc).unwrap();
        let geometry = std::sync::Arc::new(jetcarp_display::gpu::Geometry::build(&world.topo, 1));
        let (center, zoom) =
            jetcarp_display::view::place(&world, [W as f64, H as f64], cell_px as f64);
        AppView {
            world,
            geometry,
            views: jetcarp_display::view::Views::new(jetcarp_display::gpu::slot_capacity(device)),
            renderer: jetcarp_display::gpu::TileRenderer::new(device, FORMAT),
            guard: jetcarp_display::strobe_gpu::GuardedGpu::new(
                device,
                Viewing::screen(1.0),
                None,
                on,
            ),
            center,
            zoom,
            speed,
        }
    }

    /// Steps the world to the generation due at `t` and draws it into `tex` (readable).
    fn paint(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, t: f64) {
        use jetcarp_core::strobe::guard::Pace;
        use jetcarp_display::strobe_gpu::Shown;
        let due = ((t - 1.0) * self.speed as f64 + 1e-9).floor();
        while (self.world.generation as f64) < due {
            self.world.step();
        }
        self.guard.update(
            device,
            Pace {
                speed: self.speed,
                reached: Some(self.speed),
                frame_rate: 60.0,
            },
        );
        let req = jetcarp_display::view::ViewRequest {
            view: 0,
            center: self.center,
            zoom: self.zoom,
            size: [W as f32, H as f32],
            outlines: true,
        };
        let epoch = self.world.generation;
        let (d, _) = self.views.draw(&self.world, &self.geometry, epoch, req);
        self.renderer.prepare(device, queue, &d);
        let view = tex.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        let pass = |enc: &mut wgpu::CommandEncoder, load| {
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime()
        };
        {
            let mut p = pass(&mut enc, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            self.renderer.paint(&mut p, &d);
        }
        if self.guard.frame(device, queue, &mut enc, Some(tex), t) == Shown::Softened {
            self.guard.gpu.prepare_blit(device, FORMAT, Some(tex));
            let mut p = pass(&mut enc, wgpu::LoadOp::Load);
            self.guard.gpu.blit(&mut p);
        }
        queue.submit([enc.finish()]);
        self.guard.gpu.after_submit();
    }
}

/// Recordings for the IRIS comparison (#150; ignored, and needs `JETCARP_STROBE_RECORD=DIR`):
/// every recording of the corpus, with the guard off (what the scene draws) and on (what the
/// guard shows), 6 seconds at 60 frames a second, 320 by 240, on the GPU. Each recording is a
/// folder of PNGs, `DIR/desktop-off/<name>/00000.png` and `DIR/desktop-on/<name>/...` for the
/// shared painter (the desktop background and the screen savers) and `DIR/view-off/<name>/...`
/// and `DIR/view-on/<name>/...` for the app's view with outlines; `DIR/verdicts.json` holds
/// JETCARP's verdict on each, from the processor's meters on the frames recorded.
#[test]
#[ignore]
fn record_the_corpus_for_iris() {
    use jetcarp_core::strobe::frame::reduce_both;
    use jetcarp_core::strobe::pattern::PatternMeter;
    let Some(dir) = std::env::var_os("JETCARP_STROBE_RECORD").map(std::path::PathBuf::from) else {
        eprintln!("skipped: set JETCARP_STROBE_RECORD to a folder");
        return;
    };
    let Some((device, queue)) = gpu() else {
        return;
    };
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/scenes/strobe");
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    let mut verdicts = serde_json::Map::new();
    for r in CORPUS {
        let text = std::fs::read_to_string(corpus.join(format!("{}.json", r.scene))).unwrap();
        let sc: Scene = serde_json::from_str(&text).unwrap();
        for on in [false, true] {
            let path = if r.outlines { "view" } else { "desktop" };
            let key = format!("{path}-{}/{}", if on { "on" } else { "off" }, r.name);
            // The painter, with its stage, or the app's view.
            let mut painted = if r.outlines {
                Err(AppView::new(&device, &sc, r.speed, r.cell_px, on))
            } else {
                let mut painter = Painter::guarded(&device, FORMAT, on);
                painter.set_readable(true);
                Ok((stage_at(sc.clone(), r.speed, r.cell_px), painter))
            };
            let tex = target(&device, true);
            let mut sim = SimBackend::unavailable("a test".into());
            let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
            let mut stripes = PatternMeter::new(None);
            let sub = dir.join(&key);
            std::fs::create_dir_all(&sub).unwrap();
            let (mut worst, mut worst_red, mut worst_stripes) = (
                jetcarp_core::strobe::flash::FlashReading::default(),
                0,
                jetcarp_core::strobe::pattern::PatternReading::default(),
            );
            for k in 0..360u32 {
                let t = 1.0 + k as f64 / 60.0;
                match &mut painted {
                    Ok((stage, painter)) => {
                        stage.tick(t, &mut sim, Duration::from_millis(20));
                        painter.paint(&device, &queue, &tex, [W, H], 1.0, stage, &caption, t);
                        let (cap, paused) = painter.guard_hold();
                        stage.set_guard(painter.id(), t, cap, paused);
                    }
                    Err(view) => view.paint(&device, &queue, &tex, t),
                }
                let img =
                    jetcarp_display::crop::read(&device, &queue, &tex, [0, 0], [W, H]).unwrap();
                let (map, half) = reduce_both(&img.rgba, W, H);
                let m = meter.update(&map, t);
                if (m.level, m.transitions) > (worst.level, worst.transitions) {
                    worst = m;
                }
                worst_red = worst_red.max(m.red_transitions);
                if k % 15 == 0 {
                    let p = stripes.update(&half, t);
                    if p.level > worst_stripes.level
                        || (p.level == worst_stripes.level && p.share > worst_stripes.share)
                    {
                        worst_stripes = p;
                    }
                }
                jetcarp_core::render::write_png(&sub.join(format!("{k:05}.png")), &img, &[])
                    .unwrap();
            }
            verdicts.insert(
                key,
                serde_json::json!({
                    "frames": 360,
                    "fps": 60,
                    "size": [W, H],
                    "scene": r.scene,
                    "speed": r.speed,
                    "cell_px": r.cell_px,
                    "outlines": r.outlines,
                    "flash": worst.level.name(),
                    "transitions": worst.transitions,
                    "red_transitions": worst_red,
                    "stripes": worst_stripes.level.name(),
                    "stripe_share": worst_stripes.share,
                    "stripe_contrast": worst_stripes.contrast,
                    "stripe_period_px": worst_stripes.period_px,
                }),
            );
        }
    }
    let out = dir.join("verdicts.json");
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&verdicts).unwrap() + "\n",
    )
    .unwrap();
    eprintln!("wrote {}", out.display());
}

/// No seams: a field of live cells drawn by the painter at fractional sizes is one colour, with
/// nothing between the cells for the stripe check to find.
#[test]
fn a_live_field_at_a_fractional_size_draws_no_seams() {
    use jetcarp_core::strobe::frame::reduce_both;
    use jetcarp_core::strobe::pattern::{PatternLevel, PatternMeter};
    let Some((device, queue)) = gpu() else {
        return;
    };
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/scenes/strobe/live-field.json");
    let sc: Scene = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    for cell_px in [4.37, 5.5, 6.3, 7.7, 9.1, 12.4] {
        let stage = stage_at(sc.clone(), 30.0, cell_px);
        let tex = target(&device, true);
        let mut painter = Painter::guarded(&device, FORMAT, false);
        painter.set_readable(true);
        painter.paint(&device, &queue, &tex, [W, H], 1.0, &stage, &caption, 1.0);
        let img = jetcarp_display::crop::read(&device, &queue, &tex, [0, 0], [W, H]).unwrap();
        let first = &img.rgba[..4];
        let other = img.rgba.chunks(4).filter(|p| *p != first).count();
        assert_eq!(other, 0, "{cell_px} px: {other} pixels unlike the rest");
        let (_, half) = reduce_both(&img.rgba, W, H);
        let mut m = PatternMeter::new(None);
        assert_eq!(
            m.update(&half, 0.0).level,
            PatternLevel::Pass,
            "{cell_px} px"
        );
    }
}

/// The app's view with cell outlines, pinned: over a field of white live cells, and over a light
/// ground with scattered cells (the Paper field), the outlines are drawn faint enough over bright
/// cells to make no strong stripes, at 8 and at 16 pixels a cell, where they are drawn fully over
/// dark cells. With the guard on, nothing shown fails.
#[test]
fn outlines_in_the_app_s_view_are_pinned() {
    use jetcarp_core::strobe::frame::reduce_both;
    use jetcarp_core::strobe::pattern::{PatternLevel, PatternMeter};
    let Some((device, queue)) = gpu() else {
        return;
    };
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/scenes/strobe");
    for (scene, cell_px, want) in [
        ("live-field", 8.0, PatternLevel::Pass),
        ("live-field", 16.0, PatternLevel::Pass),
        ("paper-field", 8.0, PatternLevel::Pass),
        ("paper-field", 16.0, PatternLevel::Pass),
    ] {
        let text = std::fs::read_to_string(corpus.join(format!("{scene}.json"))).unwrap();
        let sc: Scene = serde_json::from_str(&text).unwrap();
        for on in [false, true] {
            let mut view = AppView::new(&device, &sc, 30.0, cell_px, on);
            let tex = target(&device, true);
            let mut stripes = PatternMeter::new(None);
            let mut worst = PatternLevel::Pass;
            for k in 0..90u32 {
                let t = 1.0 + k as f64 / 60.0;
                view.paint(&device, &queue, &tex, t);
                if k % 15 == 0 {
                    let img =
                        jetcarp_display::crop::read(&device, &queue, &tex, [0, 0], [W, H]).unwrap();
                    worst = worst.max(stripes.update(&reduce_both(&img.rgba, W, H).1, t).level);
                }
            }
            if on {
                assert!(
                    worst < PatternLevel::Fail,
                    "{scene} at {cell_px} px, guard on"
                );
            } else {
                assert_eq!(worst, want, "{scene} at {cell_px} px");
            }
        }
    }
}

#[test]
fn the_desktop_painter_never_shows_more_than_three_flashes_a_second() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    for readable in [true, false] {
        let (worst, stage) = run(&device, &queue, readable, true, 8.0);
        assert!(
            worst <= MAX_TRANSITIONS,
            "readable {readable}: {worst} transitions in a second on screen"
        );
        let st = stage.status();
        let report = st.strobe_guard.expect("the guard's report");
        assert!(report.on);
        assert!(stage.guard_cap().is_some(), "slowed: {report:?}");
        assert!(st.speed <= 3.0, "the stage runs slower: {}", st.speed);
        assert!(stage.guard_paused().is_none());
    }
}

#[test]
fn with_the_guard_off_the_painter_shows_the_flashing() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let (worst, stage) = run(&device, &queue, true, false, 2.0);
    assert!(worst > MAX_TRANSITIONS, "{worst}");
    assert!(stage.guard_cap().is_none());
    let report = stage.status().strobe_guard.expect("measured all the same");
    assert!(!report.on);
    assert!(
        report
            .note
            .unwrap()
            .starts_with("Strobe guard off: this scene flashes")
    );
}
