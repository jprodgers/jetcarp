//! What a desktop background's frame costs the GPU, pass by pass (#343), offscreen: a scene drawn
//! at 1920 by 1080 as the host draws it, through the strobe guard as the shared painter
//! ([`Painter`]) does, each pass timed on its own. The passes: the simulation's step, the drawing,
//! the guard's metering (its compute pass and the copy of its readings), its softening and veil
//! (every tile softened and veiled, the most it can cost), and the frame put on the target with
//! the caption (the guard's copy of the frame where the target can't be read, as on DirectX 12's
//! surfaces). Then the painter itself, timed whole: guarded, the way each target is drawn, and
//! without the guard (as 0.4.2 drew). A present can't be timed off screen.
//!
//! GPU timestamps where the adapter has them; else the wall clock around a waited submit, an
//! upper bound. Not run by the suite: run it by hand, choosing the adapter with `JETCARP_GPU`, and
//! with scratch `JETCARP_*` folders:
//!
//! `cargo test --release -p jetcarp-display --test frame_cost -- --ignored --nocapture`

use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::guard::Pace;
use jetcarp_core::strobe::soften::Plan;
use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
use jetcarp_display::caption::CaptionSettings;
use jetcarp_display::gpu::{TileRenderer, slot_capacity};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{Profile, Show};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::{SimBackend, SimStatus};
use jetcarp_display::stage::{Stage, no_lookup};
use jetcarp_display::strobe_gpu::{GuardedGpu, Shown};
use jetcarp_display::view::Views;
use std::time::{Duration, Instant};

const W: u32 = 1920;
const H: u32 = 1080;
/// A plain format, as the host's surfaces are configured (DirectX 12 and Vulkan both offer it).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const FPS: f64 = 30.0;
/// Frames drawn before measuring, and measured.
const WARM: usize = 20;
const FRAMES: usize = 90;

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    timestamps: bool,
    period: f64,
}

fn gpu() -> Option<Gpu> {
    const WHAT: &str = "the frame cost harness";
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
    let adapter = choice.adapter;
    let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let mut desc = jetcarp_display::painter::device_descriptor(&adapter);
    if timestamps {
        desc.required_features |= wgpu::Features::TIMESTAMP_QUERY;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}), {}",
        info.name,
        info.backend,
        if timestamps {
            "GPU timestamps"
        } else {
            "no timestamps: wall clock around waited submits (upper bounds)"
        }
    );
    let period = queue.get_timestamp_period() as f64;
    Some(Gpu {
        adapter,
        device,
        queue,
        timestamps,
        period,
    })
}

impl Gpu {
    fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    fn target(&self, readable: bool) -> wgpu::Texture {
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let twin = [FORMAT.add_srgb_suffix()];
        if readable {
            usage |= wgpu::TextureUsages::TEXTURE_BINDING;
        }
        self.device.create_texture(&wgpu::TextureDescriptor {
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
            view_formats: if readable { &twin } else { &[] },
        })
    }
}

/// Timestamps: a query set, written by passes (or by empty compute passes, [`Stamps::mark`]), read
/// back once a frame's work is done.
struct Stamps {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

const STAMPS: u32 = 8;

impl Stamps {
    fn new(g: &Gpu) -> Stamps {
        let buf = |usage| {
            g.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: STAMPS as u64 * 8,
                usage,
                mapped_at_creation: false,
            })
        };
        Stamps {
            set: g.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count: STAMPS,
            }),
            resolve: buf(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buf(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
        }
    }

    /// Timestamp `i` once the GPU gets here in `enc`.
    fn mark(&self, enc: &mut wgpu::CommandEncoder, i: u32) {
        drop(enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("mark"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.set,
                beginning_of_pass_write_index: Some(i),
                end_of_pass_write_index: None,
            }),
        }));
    }

    /// Timestamp `i` in a submit of its own.
    fn mark_alone(&self, g: &Gpu, i: u32) {
        let mut enc = g.device.create_command_encoder(&Default::default());
        self.mark(&mut enc, i);
        g.queue.submit([enc.finish()]);
    }

    fn render(&self, begin: u32, end: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(end),
        }
    }

    /// Waits for the work so far, and gives the timestamps in milliseconds from the first.
    fn read(&self, g: &Gpu, n: u32) -> Vec<f64> {
        let mut enc = g.device.create_command_encoder(&Default::default());
        enc.resolve_query_set(&self.set, 0..n, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, n as u64 * 8);
        g.queue.submit([enc.finish()]);
        self.read.map_async(wgpu::MapMode::Read, .., |_| {});
        g.wait();
        let t: Vec<u64> = {
            let v = self.read.get_mapped_range(..).expect("mapped");
            bytemuck::cast_slice::<u8, u64>(&v[..n as usize * 8]).to_vec()
        };
        self.read.unmap();
        t.iter()
            .map(|x| x.saturating_sub(t[0]) as f64 * g.period / 1e6)
            .collect()
    }
}

/// A stage showing `scene` at `cell_px` pixels a cell and `speed` generations a second, built.
fn stage(scene: Scene, cell_px: f32, speed: f32, sim: &mut SimBackend) -> Stage {
    let profile = Profile {
        name: "frame cost".into(),
        show: Show::Scene {
            scene: Box::new(scene),
        },
        cell_px,
        refill: true,
        speed,
        gpu: true,
        ..Default::default()
    };
    let slot = Slot {
        label: "frame cost".into(),
        profile,
        stored: false,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
        series: None,
    };
    let rotation = Rotation {
        name: "frame cost".into(),
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
    stage.resize([W as f64, H as f64], 1.0, 0.0);
    let t0 = Instant::now();
    while stage.world().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(60), "never built");
        stage.tick(0.0, sim, Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
    }
    // On the GPU where the host would step it there.
    let paced = speed > 0.0;
    let on = stage
        .world()
        .is_some_and(|w| sim.available() && jetcarp_gpu::GpuSim::worth(w, paced));
    stage.set_world_gpu(on);
    stage
}

fn hodgepodge() -> Scene {
    let mut sc = Scene::new(TilingSpec::rect("square", 200, 140, Boundary::Wrap));
    sc.cas = vec![CaSpec::new("Hodgepodge 200/3/3/28").seed(SeedSpec::RandomStates)];
    sc
}

/// The Quick Start's four-colour turmite, as the rotation the issue was found on shows it.
fn turmite() -> Scene {
    Scene::from_json(
        r#"{"name": "Four-colour turmite LLRR", "tiling": {"family": "square", "size": {"rect":
        {"w": 160, "h": 160}}, "boundary": "wrap"}, "rng_state": 1, "cas": [{"rule":
        "Turmite LLRR", "steps_per_gen": 200, "seed": {"ants": {"count": 1}}, "palette":
        "Prism"}]}"#,
    )
    .unwrap()
}

/// How a frame reaches its target.
#[derive(Clone, Copy, PartialEq)]
enum Path {
    /// Drawn on the target, read in place (Vulkan's and Metal's surfaces).
    Readable,
    /// Drawn into the guard's own texture and copied to the target (DirectX 12's surfaces).
    Copied,
}

/// Each pass's median milliseconds over the measured frames.
#[derive(Default)]
struct Passes {
    step_gpu: Vec<f64>,
    step_wall: Vec<f64>,
    draw: Vec<f64>,
    meter: Vec<f64>,
    put: Vec<f64>,
    total: Vec<f64>,
    cpu: Vec<f64>,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The frames of `scene` at `cell_px`, drawn as the shared painter draws them, pass by pass: the
/// guard on, along `path`; `soften` forces every tile softened and veiled.
fn by_pass(g: &Gpu, scene: Scene, cell_px: f32, speed: f32, path: Path, soften: bool) -> String {
    let mut sim = SimBackend::from_device(&g.adapter, g.device.clone(), g.queue.clone());
    let mut stage = stage(scene, cell_px, speed, &mut sim);
    let target = g.target(path == Path::Readable);
    let target_view = target.create_view(&Default::default());
    let mut renderer = TileRenderer::new(&g.device, FORMAT);
    let mut views = Views::new(slot_capacity(&g.device));
    let mut guard = GuardedGpu::new(&g.device, Viewing::screen(1.0), None, true);
    let stamps = g.timestamps.then(|| Stamps::new(g));
    let mut all = Plan::none_for(W, H);
    all.soften.fill(true);
    all.veil.fill([0.5, 0.2]);
    let mut p = Passes::default();
    let mut softened = 0usize;
    let cells = stage.world().map_or(0, |w| w.topo.len());
    for k in 0..WARM + FRAMES {
        let now = 1.0 + k as f64 / FPS;
        let measured = k >= WARM;
        // The step.
        let s0 = Instant::now();
        if let Some(s) = &stamps {
            s.mark_alone(g, 0);
        }
        stage.tick(now, &mut sim, Duration::from_secs_f64(0.5 / FPS));
        if let Some(s) = &stamps {
            s.mark_alone(g, 1);
        }
        let step_wall = s0.elapsed().as_secs_f64() * 1e3;
        // The painter's work, as `Painter::paint` does it (no caption).
        let c0 = Instant::now();
        let draw = stage.draw(&mut views, 0, [W as f32, H as f32]);
        if let Some(d) = &draw {
            renderer.prepare(&g.device, &g.queue, d);
        }
        let [r, gr, b] = stage.backdrop();
        let clear = wgpu::Color {
            r: r as f64 / 255.0,
            g: gr as f64 / 255.0,
            b: b as f64 / 255.0,
            a: 1.0,
        };
        let pace = stage.pace();
        guard.update(
            &g.device,
            Pace {
                speed: stage.scene_speed(),
                reached: stage.measured_speed(),
                frame_rate: pace.fps,
            },
        );
        let readable = path == Path::Readable;
        let scene_view = if readable {
            target_view.clone()
        } else {
            guard.gpu.scene_target(&g.device, [W, H], FORMAT)
        };
        let source = readable.then_some(&target);
        // Each part in its own submit when there are no timestamps, waited and timed.
        let mut walls = [0.0f64; 3];
        let mut enc = g.device.create_command_encoder(&Default::default());
        let flush = |enc: wgpu::CommandEncoder, at: &mut f64| {
            if stamps.is_none() {
                let t = Instant::now();
                g.queue.submit([enc.finish()]);
                g.wait();
                *at = t.elapsed().as_secs_f64() * 1e3;
                g.device.create_command_encoder(&Default::default())
            } else {
                enc
            }
        };
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stage"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scene_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: stamps.as_ref().map(|s| s.render(2, 3)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(d) = &draw {
                renderer.paint(&mut pass, d);
            }
        }
        enc = flush(enc, &mut walls[0]);
        if let Some(s) = &stamps {
            s.mark(&mut enc, 4);
        }
        let shown = if soften {
            guard.gpu.process(
                &g.device,
                &g.queue,
                &mut enc,
                source,
                &all,
                1.0 / 30.0,
                now,
                false,
            )
        } else {
            guard.frame(&g.device, &g.queue, &mut enc, source, now)
        };
        if let Some(s) = &stamps {
            s.mark(&mut enc, 5);
        }
        enc = flush(enc, &mut walls[1]);
        let copy = shown == Shown::Softened || !readable;
        if measured && shown == Shown::Softened {
            softened += 1;
        }
        if copy {
            guard.gpu.prepare_blit(&g.device, FORMAT, source);
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stage caption"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: stamps.as_ref().map(|s| s.render(6, 7)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if copy {
                guard.gpu.blit(&mut pass);
            }
        }
        let cpu = c0.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        g.queue.submit([enc.finish()]);
        guard.gpu.after_submit();
        if stamps.is_none() {
            g.wait();
            walls[2] = t.elapsed().as_secs_f64() * 1e3;
        }
        let (cap, paused) = (guard.guard.cap(), guard.guard.paused().map(str::to_string));
        stage.set_guard(1, now, cap, paused);
        let times = stamps.as_ref().map(|s| s.read(g, STAMPS));
        if !measured {
            continue;
        }
        p.step_wall.push(step_wall);
        p.cpu.push(cpu);
        match times {
            Some(t) => {
                p.step_gpu.push(t[1] - t[0]);
                p.draw.push(t[3] - t[2]);
                p.meter.push(t[5] - t[4]);
                p.put.push(t[7] - t[6]);
                p.total.push((t[3] - t[2]) + (t[7] - t[4]));
            }
            None => {
                p.draw.push(walls[0]);
                p.meter.push(walls[1]);
                p.put.push(walls[2]);
                p.total.push(walls.iter().sum());
            }
        }
    }
    let on_gpu = matches!(sim.status(), SimStatus::Gpu(_));
    let total = median(&mut p.total);
    format!(
        "{cells:>7} cells at {cell_px:>2} px, {}{}: step {} (wall {:.3}), draw {:.3}, metering \
         {:.3}{}, frame to target {:.3}; GPU {total:.3} ms a frame = {:.1}% at 30 fps; \
         processor {:.3} ms; {softened} of {FRAMES} frames softened or veiled (the guard: {:?}, {})",
        match path {
            Path::Readable => "read in place",
            Path::Copied => "copied",
        },
        if soften { ", every tile softened" } else { "" },
        if on_gpu {
            format!("{:.3} on the GPU", median(&mut p.step_gpu))
        } else {
            "on the processor".into()
        },
        median(&mut p.step_wall),
        median(&mut p.draw),
        median(&mut p.meter),
        if soften { " (with the veil)" } else { "" },
        median(&mut p.put),
        total * FPS / 10.0,
        median(&mut p.cpu),
        guard.guard.report().action,
        guard.guard.report().note.unwrap_or_default(),
    )
}

/// The painter itself, timed whole: guarded (along `path`) or not.
fn whole(g: &Gpu, scene: Scene, cell_px: f32, speed: f32, guarded: Option<Path>) -> String {
    let mut sim = SimBackend::from_device(&g.adapter, g.device.clone(), g.queue.clone());
    let mut stage = stage(scene, cell_px, speed, &mut sim);
    let readable = guarded == Some(Path::Readable);
    let target = g.target(readable);
    let mut painter = match guarded {
        Some(_) => Painter::guarded(&g.device, FORMAT, true),
        None => Painter::new(&g.device, FORMAT),
    };
    painter.set_readable(readable);
    let caption = CaptionSettings::default();
    let stamps = g.timestamps.then(|| Stamps::new(g));
    let (mut gpu, mut cpu) = (vec![], vec![]);
    for k in 0..WARM + FRAMES {
        let now = 1.0 + k as f64 / FPS;
        stage.tick(now, &mut sim, Duration::from_secs_f64(0.5 / FPS));
        g.wait();
        let c0 = Instant::now();
        painter.paint_timed(
            &g.device,
            &g.queue,
            &target,
            [W, H],
            1.0,
            &stage,
            &caption,
            now,
            stamps.as_ref().map(|s| s.render(0, 1)),
        );
        let c = c0.elapsed().as_secs_f64() * 1e3;
        let ms = match &stamps {
            Some(s) => {
                s.mark_alone(g, 2);
                s.read(g, 3)[2]
            }
            None => {
                g.wait();
                c0.elapsed().as_secs_f64() * 1e3
            }
        };
        let (cap, paused) = painter.guard_hold();
        stage.set_guard(painter.id(), now, cap, paused);
        if k >= WARM {
            gpu.push(ms);
            cpu.push(c);
        }
    }
    let ms = median(&mut gpu);
    format!(
        "{:>7} cells at {cell_px:>2} px, the painter {}: GPU {ms:.3} ms a frame = {:.1}% at 30 \
         fps; processor {:.3} ms",
        stage.world().map_or(0, |w| w.topo.len()),
        match guarded {
            Some(Path::Readable) => "guarded, read in place",
            Some(Path::Copied) => "guarded, copied",
            None => "with no guard (as 0.4.2)",
        },
        ms * FPS / 10.0,
        median(&mut cpu),
    )
}

#[test]
#[ignore]
fn frame_cost() {
    let Some(g) = gpu() else { return };
    println!("{W} x {H}, {FPS} frames a second; median of {FRAMES} frames");
    for (name, scene, speed) in [
        // Softened and slowed by the guard most frames.
        ("Hodgepodge", hodgepodge as fn() -> Scene, 30.0),
        // Calm: nothing to soften.
        ("Turmite LLRR", turmite as fn() -> Scene, 240.0),
    ] {
        println!("{name}:");
        for px in [16.0, 6.0] {
            for path in [Path::Copied, Path::Readable] {
                println!("  {}", by_pass(&g, scene(), px, speed, path, false));
            }
        }
        println!(
            "  {}",
            by_pass(&g, scene(), 16.0, speed, Path::Copied, true)
        );
        for px in [16.0, 6.0] {
            for guarded in [None, Some(Path::Copied), Some(Path::Readable)] {
                println!("  {}", whole(&g, scene(), px, speed, guarded));
            }
        }
    }
}
