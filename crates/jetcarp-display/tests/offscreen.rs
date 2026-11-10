//! The display engine with no window toolkit at all: build a world from a profile, step it, draw it
//! to an off-screen texture, and check the pixels against the simulation's own colours. Each test
//! skips itself when the machine has no GPU adapter.

use jetcarp_core::render::colors_for;
use jetcarp_core::{Command, Scene, SizeSpec, TilingSpec, World, geom};
use jetcarp_display::gpu::{Geometry, TileDraw, TileRenderer, slot_capacity};
use jetcarp_display::profiles::{self, Show};
use jetcarp_display::simgpu::{SimBackend, SimStatus};
use jetcarp_display::view::{ViewRequest, Views};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: TileRenderer,
}

fn gpu() -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    let renderer = TileRenderer::new(&device, FORMAT);
    Some(Gpu {
        adapter,
        device,
        queue,
        renderer,
    })
}

impl Gpu {
    /// Draws one frame and returns its RGBA pixels.
    fn frame(&mut self, draw: &TileDraw, w: u32, h: u32) -> Vec<u8> {
        self.renderer.prepare(&self.device, &self.queue, draw);
        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tiles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            self.renderer.paint(&mut pass, draw);
        }
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            extent,
        );
        self.queue.submit([enc.finish()]);
        buf.map_async(wgpu::MapMode::Read, .., |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let data = buf.get_mapped_range(..).expect("mapped");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            let start = y * row as usize;
            out.extend_from_slice(&data[start..start + w as usize * 4]);
        }
        out
    }
}

/// A black target of `w` by `h`, painted by `paint`, as RGBA pixels.
fn target_pixels(
    g: &Gpu,
    w: u32,
    h: u32,
    paint: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let extent = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let tex = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: row as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("target"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        paint(&mut pass);
    }
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        extent,
    );
    g.queue.submit([enc.finish()]);
    buf.map_async(wgpu::MapMode::Read, .., |_| {});
    g.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = buf.get_mapped_range(..).expect("mapped");
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        out.extend_from_slice(&data[start..start + w as usize * 4]);
    }
    out
}

/// The built-in Life profile's scene.
fn life_scene() -> Scene {
    let p = profiles::builtins().remove(0);
    let Show::Scene { scene } = p.show else {
        panic!("the Life profile keeps its scene");
    };
    *scene
}

fn mid(b: [f64; 4]) -> [f64; 2] {
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
}

/// Asserts each cell with its centre `margin` pixels inside the frame shows its own colour.
/// Returns how many cells were checked.
fn check_cells(
    world: &World,
    px: &[u8],
    (w, h): (u32, u32),
    center: [f64; 2],
    zoom: f64,
    margin: f64,
) -> usize {
    let mut checked = 0;
    let mut distinct = std::collections::HashSet::new();
    for i in 0..world.topo.len() {
        let c = geom::centroid(&world.topo.polygon(i));
        let x = (c[0] - center[0]) * zoom + w as f64 / 2.0;
        let y = (c[1] - center[1]) * zoom + h as f64 / 2.0;
        if x < margin || y < margin || x >= w as f64 - margin || y >= h as f64 - margin {
            continue;
        }
        let at = (y.floor() as usize * w as usize + x.floor() as usize) * 4;
        let want = colors_for(world, &[i as u32])[0];
        assert_eq!(&px[at..at + 3], &want[..3], "cell {i} at ({x}, {y})");
        distinct.insert([want[0], want[1], want[2]]);
        checked += 1;
    }
    assert!(
        distinct.len() > 1,
        "the frame should not be one flat colour"
    );
    checked
}

#[test]
fn a_profile_draws_pixel_exact_without_a_window_toolkit() {
    let Some(mut g) = gpu() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (w, h) = (160u32, 120u32);
    let mut p = profiles::builtins().remove(0);
    p.cell_px = 1.0;
    let scene = profiles::apply_profile(&p, life_scene(), Some([w as f64, h as f64]));
    assert!(profiles::is_lattice(&scene));
    let mut world = World::from_scene(&scene).expect("world");
    assert_eq!(world.topo.len(), (w * h) as usize, "one cell per pixel");
    world.apply(Command::Step { gens: 3 }).expect("step");

    let geometry = Arc::new(Geometry::build(&world.topo, 1));
    let mut views = Views::new(slot_capacity(&g.device));
    let center = mid(world.topo.bounds);
    let req = ViewRequest {
        view: 0,
        center,
        zoom: 1.0,
        size: [w as f32, h as f32],
        outlines: false,
    };
    let (draw, work) = views.draw(&world, &geometry, 1, req);
    assert_eq!(
        work.cells,
        (w * h) as usize,
        "the first frame colours every cell"
    );
    let px = g.frame(&draw, w, h);
    assert_eq!(
        check_cells(&world, &px, (w, h), center, 1.0, 0.0),
        (w * h) as usize
    );

    // Nothing changed: nothing is coloured again.
    let (draw, work) = views.draw(&world, &geometry, 1, req);
    assert_eq!(work.cells, 0);
    assert!(draw.colors.is_none());

    // A new generation is coloured and shown.
    world.apply(Command::Step { gens: 2 }).expect("step");
    let (draw, _) = views.draw(&world, &geometry, 2, req);
    let px = g.frame(&draw, w, h);
    check_cells(&world, &px, (w, h), center, 1.0, 0.0);
}

#[test]
fn an_aperiodic_patch_draws_and_pans_without_a_window_toolkit() {
    let Some(mut g) = gpu() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    // Large enough that one view holds only part of the patch.
    let mut scene = Scene::new(TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(7)));
    scene.cas = life_scene().cas;
    let world = World::from_scene(&scene).expect("world");
    assert!(world.topo.lattice().is_none());
    let geometry = Arc::new(Geometry::build(&world.topo, 1));
    let mut views = Views::new(slot_capacity(&g.device));
    let (w, h) = (400u32, 300u32);
    let zoom = 16.0 / jetcarp_display::view::cell_unit(&world);
    let center = mid(world.topo.bounds);
    let mut req = ViewRequest {
        view: 0,
        center,
        zoom,
        size: [w as f32, h as f32],
        outlines: false,
    };
    let (draw, work) = views.draw(&world, &geometry, 1, req);
    assert!(work.cells > 0 && work.cells < world.topo.len());
    let px = g.frame(&draw, w, h);
    let first = check_cells(&world, &px, (w, h), center, zoom, 2.0);
    assert!(first > 50, "only {first} cells in view");

    // Panning colours just what came into view. Instances are binned by row, so move a full view
    // down to bring new rows in.
    let moved = [center[0], center[1] + h as f64 / zoom];
    req.center = moved;
    let (draw, panned) = views.draw(&world, &geometry, 1, req);
    assert!(panned.cells > 0 && panned.cells < world.topo.len());
    let px = g.frame(&draw, w, h);
    check_cells(&world, &px, (w, h), moved, zoom, 2.0);
}

#[test]
fn a_second_renderer_draws_the_same_aperiodic_patch() {
    // The host and the saver give each screen its own painter, with its own renderer, all on one
    // device and drawing one stage. The second renderer used to find the patch's instances gone.
    let Some(mut g) = gpu() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let mut scene = Scene::new(TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(5)));
    scene.cas = life_scene().cas;
    let world = World::from_scene(&scene).expect("world");
    let geometry = Arc::new(Geometry::build(&world.topo, 1));
    let (w, h) = (200u32, 150u32);
    let zoom = 12.0 / jetcarp_display::view::cell_unit(&world);
    let center = mid(world.topo.bounds);
    let req = ViewRequest {
        view: 0,
        center,
        zoom,
        size: [w as f32, h as f32],
        outlines: false,
    };
    for screen in 0..2 {
        let mut views = Views::new(slot_capacity(&g.device));
        let (draw, _) = views.draw(&world, &geometry, 1, req);
        let px = g.frame(&draw, w, h);
        let checked = check_cells(&world, &px, (w, h), center, zoom, 2.0);
        assert!(checked > 20, "screen {screen}: only {checked} cells");
        g.renderer = TileRenderer::new(&g.device, FORMAT);
    }
}

#[test]
fn a_caption_draws_in_its_corner_and_nowhere_else() {
    use jetcarp_display::caption::{CaptionPainter, CaptionSettings, Corner};
    let Some(g) = gpu() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (w, h) = (400u32, 200u32);
    let mut painter = CaptionPainter::new(&g.device, FORMAT);
    let lines = vec!["Life · B3/S23".to_string(), "Square grid".to_string()];
    let mut settings = CaptionSettings {
        show: true,
        ..Default::default()
    };
    let shot = |painter: &mut CaptionPainter, settings: &CaptionSettings, opacity: f32| {
        painter.prepare(&g.device, &g.queue, &lines, settings, [w, h], 1.0, opacity);
        target_pixels(&g, w, h, |pass| painter.paint(pass))
    };
    let lit = |px: &[u8], left: bool, top: bool| {
        (0..h as usize / 2)
            .flat_map(|y| (0..w as usize / 2).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let x = if left { x } else { w as usize / 2 + x };
                let y = if top { y } else { h as usize / 2 + y };
                px[(y * w as usize + x) * 4] > 128
            })
            .count()
    };
    let px = shot(&mut painter, &settings, 1.0);
    assert!(lit(&px, true, false) > 100, "text in the bottom left");
    assert_eq!(lit(&px, false, false), 0);
    assert_eq!(lit(&px, true, true), 0);
    assert_eq!(lit(&px, false, true), 0);

    settings.corner = Corner::TopRight;
    let px = shot(&mut painter, &settings, 1.0);
    assert!(lit(&px, false, true) > 100, "text in the top right");
    assert_eq!(lit(&px, true, false), 0);

    let px = shot(&mut painter, &settings, 0.0);
    assert!(px.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]), "invisible");
}

#[test]
fn gpu_stepping_matches_the_cpu_without_a_window_toolkit() {
    let Some(g) = gpu() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let mut p = profiles::builtins().remove(0);
    p.cell_px = 1.0;
    let scene = profiles::apply_profile(&p, life_scene(), Some([96.0, 64.0]));
    let mut on_gpu = World::from_scene(&scene).expect("world");
    let mut on_cpu = World::from_scene(&scene).expect("world");

    let mut sim = SimBackend::from_device(&g.adapter, g.device.clone(), g.queue.clone());
    assert!(sim.available(), "{}", sim.hint());
    sim.on = true;
    assert_eq!(sim.status(), SimStatus::Waiting);
    let mut done = 0;
    while done < 20 {
        let deadline = Instant::now() + Duration::from_secs(5);
        let n = sim.step(&mut on_gpu, 20 - done, deadline);
        assert!(n > 0, "{:?}", sim.status());
        done += n;
    }
    assert!(
        matches!(sim.status(), SimStatus::Gpu(_)),
        "{:?}",
        sim.status()
    );
    on_cpu.apply(Command::Step { gens: 20 }).expect("step");
    let all: Vec<u32> = (0..on_cpu.topo.len() as u32).collect();
    assert_eq!(colors_for(&on_gpu, &all), colors_for(&on_cpu, &all));
}
