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
    let profile = Profile {
        name: "tron".into(),
        show: Show::Scene {
            scene: Box::new(sc),
        },
        cell_px: 4.0,
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

/// Every scene of the corpus (`docs/scenes/strobe/`), with the speed it is recorded at.
const CORPUS: &[(&str, f32)] = &[
    ("tron-field", 60.0),
    ("tron-red", 30.0),
    ("tron-blob", 30.0),
    ("seeds-soup", 30.0),
    ("stripes-still", 10.0),
    ("tron-checkerboard", 30.0),
    ("life-soup", 30.0),
    ("brians-brain", 30.0),
    ("maze", 30.0),
    ("cyclic-waves", 15.0),
];

/// Recordings for the IRIS comparison (#150; ignored, and needs `JETCARP_STROBE_RECORD=DIR`):
/// every scene of the corpus drawn as the desktop background and the screen savers draw it (the
/// shared painter, on the GPU), with the guard off (what the scene draws) and on (what the guard
/// shows), 6 seconds at 60 frames a second, 320 by 240. Each recording is a folder of PNGs,
/// `DIR/desktop-off/<scene>/00000.png` and `DIR/desktop-on/<scene>/...`, and `DIR/verdicts.json`
/// holds JETCARP's verdict on each, from the processor's meters on the frames recorded.
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
    for &(name, speed) in CORPUS {
        let text = std::fs::read_to_string(corpus.join(format!("{name}.json"))).unwrap();
        let sc: Scene = serde_json::from_str(&text).unwrap();
        for on in [false, true] {
            let mut stage = stage_of(sc.clone(), speed);
            let tex = target(&device, true);
            let mut painter = Painter::guarded(&device, FORMAT, on);
            painter.set_readable(true);
            let mut sim = SimBackend::unavailable("a test".into());
            let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
            let mut stripes = PatternMeter::new(None);
            let sub = dir
                .join(if on { "desktop-on" } else { "desktop-off" })
                .join(name);
            std::fs::create_dir_all(&sub).unwrap();
            let (mut worst, mut worst_red, mut worst_stripes) = (
                jetcarp_core::strobe::flash::FlashReading::default(),
                0,
                jetcarp_core::strobe::pattern::PatternLevel::Pass,
            );
            for k in 0..360u32 {
                let t = 1.0 + k as f64 / 60.0;
                stage.tick(t, &mut sim, Duration::from_millis(20));
                painter.paint(&device, &queue, &tex, [W, H], 1.0, &stage, &caption, t);
                let img =
                    jetcarp_display::crop::read(&device, &queue, &tex, [0, 0], [W, H]).unwrap();
                let (map, half) = reduce_both(&img.rgba, W, H);
                let r = meter.update(&map, t);
                if (r.level, r.transitions) > (worst.level, worst.transitions) {
                    worst = r;
                }
                worst_red = worst_red.max(r.red_transitions);
                if k % 15 == 0 {
                    worst_stripes = worst_stripes.max(stripes.update(&half, t).level);
                }
                let (cap, paused) = painter.guard_hold();
                stage.set_guard(painter.id(), t, cap, paused);
                jetcarp_core::render::write_png(&sub.join(format!("{k:05}.png")), &img, &[])
                    .unwrap();
            }
            verdicts.insert(
                format!("desktop-{}/{name}", if on { "on" } else { "off" }),
                serde_json::json!({
                    "frames": 360,
                    "fps": 60,
                    "size": [W, H],
                    "speed": speed,
                    "flash": worst.level.name(),
                    "transitions": worst.transitions,
                    "red_transitions": worst_red,
                    "stripes": worst_stripes.name(),
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
