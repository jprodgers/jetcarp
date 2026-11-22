//! The strobe guard's GPU pass against the processor's: the same tile maps, the same half image,
//! the same softened frames, from frames uploaded as textures, on whatever adapter runs the tests
//! (CI runs them on Mesa's llvmpipe, `JETCARP_GPU=software`).
//!
//! The GPU decodes pixels with the processor's own tables and makes the red measure's saturation
//! test in whole numbers, so every pixel's values are the processor's exactly; what is left is the
//! order of the sums. A tile's mean of 256 values in single precision can differ by order of
//! summation by at most about 255 x 2^-24 of the largest value: 1.5e-5 for luminance (0 to 1),
//! 5e-3 for the red measure (0 to 320). The bounds below are those, far under the 0.1 and 20 a
//! transition needs. (A texture unit's sRGB decoding, used before, differed by driver: llvmpipe's
//! moved a gradient pixel across the saturation test, a step of over 100 in its red measure, and a
//! tile's mean by about 1.)
//!
//! Each test skips itself when the machine has no usable GPU adapter, and fails instead under
//! `JETCARP_REQUIRE_GPU=1` ([`jetcarp_gpu::no_gpu`]). `gpu_cost` (ignored; run with
//! `-- --ignored --nocapture`) times the pass at common screen sizes.

use jetcarp_core::strobe::frame::{TileMap, reduce, reduce_both};
use jetcarp_core::strobe::soften::{Plan, Softener};
use jetcarp_display::strobe_gpu::{Shown, StrobeGpu};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    timestamps: bool,
}

/// The GPU, or None after [`jetcarp_gpu::no_gpu`] (which fails the test instead under
/// `JETCARP_REQUIRE_GPU=1`).
fn gpu() -> Option<Gpu> {
    const WHAT: &str = "the strobe guard's GPU pass tests";
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
    if let Err(e) = &choice.usable {
        jetcarp_gpu::no_gpu(WHAT, e);
        return None;
    }
    let name = choice.info.name.clone();
    let adapter = choice.adapter;
    let ts = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let timestamps = adapter.features().contains(ts);
    let mut desc = jetcarp_display::painter::device_descriptor(&adapter);
    if timestamps {
        desc.required_features |= ts;
    }
    let (device, queue) = match pollster::block_on(adapter.request_device(&desc)) {
        Ok(d) => d,
        Err(e) => {
            jetcarp_gpu::no_gpu(WHAT, &format!("{name} refused a device: {e}"));
            return None;
        }
    };
    eprintln!("adapter: {}", adapter.get_info().name);
    Some(Gpu {
        device,
        queue,
        timestamps,
    })
}

impl Gpu {
    /// An RGBA frame as a texture the pass can read through an sRGB view.
    fn texture(&self, rgba: &[u8], w: u32, h: u32) -> wgpu::Texture {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        tex
    }

    fn wait(&self) {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
    }

    /// The pixels of an Rgba8Unorm texture.
    fn read(&self, tex: &wgpu::Texture) -> Vec<u8> {
        let (w, h) = (tex.width(), tex.height());
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size: row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let v = buf.slice(..).get_mapped_range().expect("mapped");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            out.extend_from_slice(&v[y * row as usize..y * row as usize + w as usize * 4]);
        }
        out
    }

    /// One frame through the pass; its reading, waited for.
    fn frame(
        &self,
        s: &mut StrobeGpu,
        tex: &wgpu::Texture,
        plan: &Plan,
        dt: f32,
        t: f64,
        half: bool,
    ) -> (Shown, jetcarp_display::strobe_gpu::GpuReading) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let shown = s.process(
            &self.device,
            &self.queue,
            &mut enc,
            Some(tex),
            plan,
            dt,
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

/// A frame with every kind of pixel: gradients, saturated reds, black and white, partial tiles.
fn busy(w: u32, h: u32, k: u32) -> Vec<u8> {
    // The gradient pixels sweep red against green and blue through the saturation test.
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = match (x / 7 + y / 5 + k) % 5 {
                0 => [255, 0, 0],
                1 => [(x * 3) as u8, (y * 2) as u8, ((x + y + k) * 5) as u8],
                2 => [0, 0, 0],
                3 => [255, 255, 255],
                _ => [200, 30, 10],
            };
            v.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    v
}

/// A tile mean of luminance may differ by order of summation by this much (see the top).
const LUM_SUMS: f32 = 2e-5;
/// And of the red measure.
const RED_SUMS: f32 = 1e-2;

/// The fine patterns of two maps (#269) agree: the light and dark parts to `lum` and `red` (the
/// order of their sums), and every window's bits, clear and above the tile's mean. A window whose
/// mean is exactly the threshold away from the tile's (in exact arithmetic, as in a frame made of
/// a few colours it can be) is clear or not by rounding: where `frame` (the RGBA frame measured,
/// and its width) is given, a bit may differ only for such a window; else every bit must agree.
/// The tiles' mean colours (#270) agree to `lum` too, each channel being in linear light, 0 to 1.
fn same_fine(
    gpu: &TileMap,
    cpu: &TileMap,
    frame: Option<(&[u8], u32)>,
    lum: f32,
    red: f32,
    what: &str,
) {
    use jetcarp_core::strobe::frame::{FINE_SIG, window_devs};
    use jetcarp_core::strobe::{TILE, luminance, red_measure};
    let mut ties = 0;
    for t in 0..cpu.len() {
        for k in 0..3 {
            assert!(
                (gpu.rgb[t][k] - cpu.rgb[t][k]).abs() < lum,
                "{what} tile {t}, colour: {:?} vs {:?}",
                gpu.rgb[t],
                cpu.rgb[t]
            );
        }
        for (g, c, tol, which) in [
            (gpu.fine[t], cpu.fine[t], lum, 0),
            (gpu.red_fine[t], cpu.red_fine[t], red, 1),
        ] {
            let name = ["luminance", "red"][which];
            assert!(
                (g.light - c.light).abs() < tol && (g.dark - c.dark).abs() < tol,
                "{what} tile {t}, {name}: {g:?} vs {c:?}"
            );
            let differ = (g.sig ^ c.sig) | (g.sign ^ c.sign);
            if differ == 0 {
                continue;
            }
            let Some((rgba, w)) = frame else {
                panic!("{what} tile {t}, {name}: {g:?} vs {c:?}");
            };
            let (x0, y0) = ((t as u32 % cpu.cols) * TILE, (t as u32 / cpu.cols) * TILE);
            let (tw, th) = ((w - x0).min(TILE), (cpu.height - y0).min(TILE));
            let value = |x: usize, y: usize| {
                let i = (((y0 as usize + y) * w as usize) + x0 as usize + x) * 4;
                let p = [rgba[i], rgba[i + 1], rgba[i + 2]];
                if which == 0 {
                    luminance(p)
                } else {
                    red_measure(p)
                }
            };
            let mean = if which == 0 { cpu.lum[t] } else { cpu.red[t] };
            let devs = window_devs(tw as usize, th as usize, mean, value);
            let threshold = FINE_SIG * c.contrast();
            for (k, dev) in devs.iter().enumerate() {
                if differ & (1 << k) != 0 {
                    let d = dev.expect("a window inside the tile");
                    assert!(
                        (d.abs() - threshold).abs() <= 1e-4 * (1.0 + threshold),
                        "{what} tile {t}, {name}, window {k}: {d} from the mean, threshold \
                         {threshold}: {g:?} vs {c:?}"
                    );
                    ties += 1;
                }
            }
        }
    }
    if ties > 0 {
        eprintln!("{what}: {ties} windows exactly at the threshold, clear by rounding");
    }
}

#[test]
fn the_gpu_measures_what_the_processor_measures() {
    let Some(g) = gpu() else { return };
    let mut s = StrobeGpu::new(&g.device);
    for (w, h) in [(64, 48), (100, 37), (257, 131)] {
        let f = busy(w, h, 1);
        let tex = g.texture(&f, w, h);
        let (shown, r) = g.frame(&mut s, &tex, &Plan::none_for(w, h), 0.016, 1.0, true);
        assert_eq!(shown, Shown::Drawn);
        let (cpu, half) = reduce_both(&f, w, h);
        assert_eq!((r.input.cols, r.input.rows), (cpu.cols, cpu.rows));
        let (mut wl, mut wr) = (0f32, 0f32);
        for t in 0..cpu.len() {
            let (dl, dr) = (
                (r.input.lum[t] - cpu.lum[t]).abs(),
                (r.input.red[t] - cpu.red[t]).abs(),
            );
            (wl, wr) = (wl.max(dl), wr.max(dr));
            assert!(
                dl < LUM_SUMS,
                "{w}x{h} tile {t}: {} vs {}",
                r.input.lum[t],
                cpu.lum[t]
            );
            assert!(
                dr < RED_SUMS,
                "{w}x{h} tile {t}: red {} vs {}",
                r.input.red[t],
                cpu.red[t]
            );
        }
        eprintln!("{w}x{h}: tiles differ by up to {wl:e} in luminance, {wr:e} in red");
        same_fine(
            &r.input,
            &cpu,
            Some((&f, w)),
            LUM_SUMS,
            RED_SUMS,
            &format!("{w}x{h}"),
        );
        assert_eq!(r.input, r.output);
        let gh = r.half.expect("asked for");
        assert_eq!((gh.width, gh.height), (half.width, half.height));
        // Each value a mean of four.
        for (a, b) in gh.lum.iter().zip(&half.lum) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
        // Without the half image.
        let (_, r) = g.frame(&mut s, &tex, &Plan::none_for(w, h), 0.016, 2.0, false);
        assert!(r.half.is_none());
        assert!((r.input.lum[0] - reduce(&f, w, h).lum[0]).abs() < LUM_SUMS);
    }
}

#[test]
fn the_gpu_softens_as_the_processor_does() {
    let Some(g) = gpu() else { return };
    let (w, h) = (80, 50);
    let mut plan = Plan::none_for(w, h);
    // Soften the left half, veil a band across the bottom.
    for r in 0..plan.rows {
        for c in 0..plan.cols {
            let i = (r * plan.cols + c) as usize;
            plan.soften[i] = c < plan.cols / 2;
            if r == plan.rows - 1 {
                plan.veil[i] = [0.7, 0.4];
            }
        }
    }
    let mut s = StrobeGpu::new(&g.device);
    let mut cpu = Softener::new();
    let frames: Vec<Vec<u8>> = (0..6).map(|k| busy(w, h, k * 2)).collect();
    for (k, f) in frames.iter().enumerate() {
        let tex = g.texture(f, w, h);
        let (shown, r) = g.frame(&mut s, &tex, &plan, 1.0 / 60.0, k as f64 / 60.0, false);
        assert_eq!(shown, Shown::Softened);
        let c = cpu.apply(f, w, h, &plan, 1.0 / 60.0);
        // The softened frame: the same 16-bit state, veil and bytes as the processor's, but a
        // driver may fuse a multiply and an add the processor rounds apart, which can move a
        // 16-bit value by a step: a byte by at most one, and a tile's mean luminance by at most
        // a 65535th. Such a step at the saturation test moves a pixel's red measure across it, by
        // at most 320 (a 256th of that in a tile's mean, so 1.25 for one pixel; it takes sixteen
        // in one tile, all one way, to make the 20 of a red transition).
        let (mut wo, mut wr) = (0f32, 0f32);
        for t in 0..c.input.len() {
            assert!(
                (r.input.lum[t] - c.input.lum[t]).abs() < LUM_SUMS,
                "frame {k}"
            );
            let (dl, dr) = (
                (r.output.lum[t] - c.output.lum[t]).abs(),
                (r.output.red[t] - c.output.red[t]).abs(),
            );
            (wo, wr) = (wo.max(dl), wr.max(dr));
            assert!(
                dl < LUM_SUMS + 1.0 / 65535.0,
                "frame {k} tile {t}: {} vs {}",
                r.output.lum[t],
                c.output.lum[t]
            );
            assert!(
                dr < 1.0,
                "frame {k} tile {t}: red {} vs {}",
                r.output.red[t],
                c.output.red[t]
            );
        }
        same_fine(
            &r.input,
            &c.input,
            Some((f, w)),
            LUM_SUMS,
            RED_SUMS,
            &format!("frame {k} as drawn"),
        );
        // As shown: a 16-bit step of the softened state moves a pixel's luminance by a 65535th,
        // and its red measure by up to 320 at the saturation test (see above).
        same_fine(
            &r.output,
            &c.output,
            None,
            LUM_SUMS + 2.0 / 65535.0,
            2.0,
            &format!("frame {k} as shown"),
        );
        let pixels = g.read(s.softened_texture().expect("softened"));
        let want = c.shown.expect("softened");
        let worst = pixels
            .iter()
            .zip(&want)
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        eprintln!(
            "frame {k}: shown tiles differ by up to {wo:e} in luminance, {wr:e} in red; bytes by {worst}"
        );
        assert!(worst <= 1, "frame {k}: pixels differ by up to {worst}");
    }
    // With nothing to soften, the frame is shown as drawn, and softening starts afresh.
    let tex = g.texture(&frames[0], w, h);
    let (shown, r) = g.frame(&mut s, &tex, &Plan::none_for(w, h), 0.016, 1.0, false);
    assert_eq!(shown, Shown::Drawn);
    assert_eq!(r.input, r.output);
}

/// Times the pass at common screen sizes: measuring alone (every frame), with the half image
/// (the stripe check's frames), and softening. GPU timestamps where the adapter has them, else
/// the wall clock around a waited submit (an upper bound).
#[test]
#[ignore]
fn gpu_cost() {
    let Some(g) = gpu() else { return };
    let period = g.queue.get_timestamp_period() as f64;
    for (w, h) in [(1920, 1080), (2560, 1440), (3840, 2160)] {
        let f = busy(w, h, 3);
        let tex = g.texture(&f, w, h);
        let mut s = StrobeGpu::new(&g.device);
        let mut soft = Plan::none_for(w, h);
        soft.soften.fill(true);
        for (name, plan, half) in [
            ("measure", Plan::none_for(w, h), false),
            ("measure + half image", Plan::none_for(w, h), true),
            ("soften", soft.clone(), false),
        ] {
            let mut times = vec![];
            // The processor's side: mapping the readings and reading them into tile maps.
            let mut reads = vec![];
            for k in 0..60 {
                let qs = g.timestamps.then(|| {
                    g.device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: None,
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    })
                });
                let qbuf = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let rb = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut enc = g.device.create_command_encoder(&Default::default());
                if let Some(q) = &qs {
                    enc.write_timestamp(q, 0);
                }
                s.process(
                    &g.device,
                    &g.queue,
                    &mut enc,
                    Some(&tex),
                    &plan,
                    0.016,
                    k as f64,
                    half,
                );
                if let Some(q) = &qs {
                    enc.write_timestamp(q, 1);
                    enc.resolve_query_set(q, 0..2, &qbuf, 0);
                    enc.copy_buffer_to_buffer(&qbuf, 0, &rb, 0, 16);
                }
                let t0 = std::time::Instant::now();
                g.queue.submit([enc.finish()]);
                s.after_submit();
                g.wait();
                let wall = t0.elapsed().as_secs_f64() * 1e3;
                let t1 = std::time::Instant::now();
                let got = s.take(&g.device);
                let read = t1.elapsed().as_secs_f64() * 1e3;
                assert_eq!(got.len(), 1);
                let ms = if qs.is_some() {
                    rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                    g.wait();
                    let v = rb.slice(..).get_mapped_range().expect("mapped");
                    let t: &[u64] = bytemuck::cast_slice(&v[..]);
                    (t[1].saturating_sub(t[0])) as f64 * period / 1e6
                } else {
                    wall
                };
                if k >= 10 {
                    times.push(ms);
                    reads.push(read);
                }
            }
            times.sort_by(f64::total_cmp);
            reads.sort_by(f64::total_cmp);
            let med = times[times.len() / 2];
            let bytes = (w * h * 4) as f64;
            println!(
                "{w} x {h}, {name}: median {med:.3} ms ({:.1}% of a 60 Hz frame), {:.1} GB/s of the frame read{}; reading back {:.3} ms",
                med / (1000.0 / 60.0) * 100.0,
                bytes / (med / 1e3) / 1e9,
                if g.timestamps { "" } else { " (wall clock)" },
                reads[reads.len() / 2]
            );
        }
    }
}

/// A checkerboard of squares smaller than a tile, reversing (#269): its tiles' means do not change,
/// but WCAG counts it as flashing (its squares are over 0.1°), so the GPU's measure must show it
/// as the processor's does. Drawn and measured offscreen only. Squares of 2 pixels, under 0.1°, are
/// exempt.
#[test]
fn the_gpu_measure_counts_a_small_reversing_checkerboard() {
    use jetcarp_core::strobe::Viewing;
    use jetcarp_core::strobe::flash::{FlashMeter, Level};
    let Some(g) = gpu() else { return };
    let (w, h) = (640u32, 480u32);
    let board = |sq: u32, phase: u32| {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let inside = (200..360).contains(&x) && (150..310).contains(&y);
                let on = inside && ((x + 1) / sq + (y + 3) / sq + phase) % 2 == 1;
                let c = if on { 255 } else { 0 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    };
    let mut s = StrobeGpu::new(&g.device);
    let mut wrong = vec![];
    for (sq, fails) in [(4u32, true), (6, true), (12, true), (2, false)] {
        let maps: Vec<_> = (0..2)
            .map(|p| {
                let f = board(sq, p);
                let tex = g.texture(&f, w, h);
                let (_, r) = g.frame(&mut s, &tex, &Plan::none_for(w, h), 0.016, p as f64, false);
                let what = format!("{sq}-pixel squares, phase {p}");
                same_fine(
                    &r.input,
                    &reduce(&f, w, h),
                    Some((&f, w)),
                    LUM_SUMS,
                    RED_SUMS,
                    &what,
                );
                r.input
            })
            .collect();
        let mut m = FlashMeter::new(Viewing::screen(1.0), None);
        let mut worst = Level::Pass;
        for f in 0..120u32 {
            // Ten reversals a second.
            let r = m.update(&maps[((f / 6) % 2) as usize], f as f64 / 60.0);
            worst = worst.max(r.level);
        }
        if worst.fails() != fails {
            wrong.push(format!(
                "{sq}-pixel squares: {worst:?}, should fail: {fails}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The guard on the GPU, as every GPU output runs it ([`GuardedGpu`]), keeps a small reversing
/// checkerboard (#269) under the limit on screen: what it shows, read back from the GPU and
/// measured pixel by pixel as WCAG defines a flash (no tiles), never fails, and it is the guard
/// that keeps it so (unguarded, the same board fails). The board reverses every generation, its
/// generations held back by the guard's pace cap; drawn and measured offscreen only.
#[test]
fn the_gpu_guard_keeps_a_small_reversing_checkerboard_under_the_limit() {
    use jetcarp_core::strobe::flash::{COUNT_CAP, EPSILON, Tracker};
    use jetcarp_core::strobe::guard::Pace;
    use jetcarp_core::strobe::{DARK_LIMIT, FLASH_DELTA, Viewing, luminance};
    use jetcarp_display::strobe_gpu::GuardedGpu;
    let Some(g) = gpu() else { return };
    let (w, h) = (640u32, 480u32);
    let (x0, y0, side) = (240u32, 160u32, 160u32);
    let board = |sq: u32, phase: u32| {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let inside = (x0..x0 + side).contains(&x) && (y0..y0 + side).contains(&y);
                let on = inside && ((x + 1) / sq + (y + 3) / sq + phase) % 2 == 1;
                let c = if on { 255 } else { 0 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    };
    // The most transitions in the last second that more than a quarter of a 10° field made,
    // pixel by pixel over the board and a margin (the board is smaller than the field).
    let allowed = Viewing::screen(1.0).flash_area();
    let (rx, ry, rw) = (x0 - 8, y0 - 8, side + 16);
    let measure = |px: &mut Vec<Tracker>, rgba: &[u8], t: f64| {
        let mut at_least = [0usize; COUNT_CAP as usize + 2];
        for (i, tr) in px.iter_mut().enumerate() {
            let (x, y) = (rx + i as u32 % rw, ry + i as u32 / rw);
            let p = &rgba[((y * w + x) * 4) as usize..];
            tr.update(
                luminance([p[0], p[1], p[2]]),
                t,
                FLASH_DELTA,
                EPSILON,
                Some(DARK_LIMIT),
            );
            at_least[tr.count(t) as usize] += 1;
        }
        for k in (0..=COUNT_CAP as usize).rev() {
            at_least[k] += at_least[k + 1];
        }
        (1..=COUNT_CAP)
            .rev()
            .find(|&k| at_least[k as usize] as f64 > allowed)
            .unwrap_or(0)
    };
    for (sq, speed) in [(4u32, 10.0f64), (8, 30.0)] {
        let frames = [0, 1].map(|p| board(sq, p));
        let tex = [0, 1].map(|p| g.texture(&frames[p], w, h));
        for on in [false, true] {
            let mut gg = GuardedGpu::new(&g.device, Viewing::screen(1.0), None, on);
            let mut px = vec![Tracker::default(); (rw * rw) as usize];
            let (mut due, mut gen_no, mut most) = (0.0f64, 0u64, 0);
            for k in 0..(60 * 6) as u32 {
                let t = k as f64 / 60.0;
                let pace = gg.guard.cap().map_or(speed, |c| (c as f64).min(speed));
                gg.update(
                    &g.device,
                    Pace {
                        speed: speed as f32,
                        reached: Some(pace as f32),
                        frame_rate: 60.0,
                    },
                );
                if k > 0 && gg.guard.paused().is_none() {
                    due += pace / 60.0;
                    gen_no = (due + 1e-9).floor() as u64;
                }
                let phase = (gen_no % 2) as usize;
                let mut enc = g.device.create_command_encoder(&Default::default());
                let shown = gg.frame(&g.device, &g.queue, &mut enc, Some(&tex[phase]), t);
                g.queue.submit([enc.finish()]);
                gg.gpu.after_submit();
                g.wait();
                let out = match shown {
                    Shown::Softened => g.read(gg.gpu.softened_texture().expect("softened")),
                    Shown::Drawn => frames[phase].clone(),
                };
                most = most.max(measure(&mut px, &out, t));
            }
            eprintln!(
                "{sq}-pixel squares at {speed} gen/s, guard {}: {most} transitions a second \
                 shown, pixel by pixel",
                if on { "on" } else { "off" }
            );
            if on {
                // Under the limit with room: two flashes a second at most.
                assert!(
                    most <= 4,
                    "{sq}-pixel squares at {speed} generations a second: {most} transitions a \
                     second shown ({:?})",
                    gg.guard.report()
                );
            } else {
                assert!(most > 6, "{sq}-pixel squares unguarded: {most}");
            }
        }
    }
}

/// Red flashes by either definition (#270): one only WCAG 2.0's working definition catches (two
/// reds of one chromaticity), one only WCAG 2.2's (a dark red and a dark green, more than 0.2
/// apart in CIE 1976 UCS chromaticity, under 20 apart in the 2.0 measure), and one both. Each,
/// over the whole frame ten times a second, fails on the GPU's measure as on the processor's.
/// Drawn and measured offscreen only.
#[test]
fn the_gpu_measure_counts_red_flashes_by_either_definition() {
    use jetcarp_core::strobe::Viewing;
    use jetcarp_core::strobe::flash::{FlashMeter, Level};
    let Some(g) = gpu() else { return };
    let (w, h) = (320u32, 240u32);
    let solid = |c: [u8; 3]| {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            v.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        v
    };
    let mut s = StrobeGpu::new(&g.device);
    let mut wrong = vec![];
    for (name, a, b) in [
        ("only WCAG 2.0's", [255u8, 0, 0], [200u8, 0, 0]),
        ("only WCAG 2.2's", [60, 0, 0], [0, 40, 0]),
        ("both", [255, 0, 0], [128, 128, 128]),
    ] {
        let maps: Vec<_> = [a, b]
            .iter()
            .enumerate()
            .map(|(p, c)| {
                let tex = g.texture(&solid(*c), w, h);
                let plan = Plan::none_for(w, h);
                g.frame(&mut s, &tex, &plan, 0.016, p as f64, false).1.input
            })
            .collect();
        let mut m = FlashMeter::new(Viewing::screen(1.0), None);
        let (mut red, mut general) = (Level::Pass, Level::Pass);
        for f in 0..120u32 {
            let r = m.update(&maps[((f / 6) % 2) as usize], f as f64 / 60.0);
            red = red.max(r.red_level);
            general = general.max(r.level);
        }
        if red != Level::Fail || general != Level::Pass {
            wrong.push(format!("{name}: red {red:?}, general {general:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Write-only features are missing on some adapters: the pass itself must build on whatever runs
/// the tests (a software renderer in CI).
#[test]
fn the_pass_builds_and_runs_on_this_adapter() {
    let Some(g) = gpu() else { return };
    let mut s = StrobeGpu::new(&g.device);
    let f = busy(16, 16, 0);
    let tex = g.texture(&f, 16, 16);
    let mut plan = Plan::none_for(16, 16);
    plan.soften[0] = true;
    let (shown, r) = g.frame(&mut s, &tex, &plan, 0.016, 0.0, true);
    assert_eq!(shown, Shown::Softened);
    assert_eq!(r.input.len(), 1);
    assert!(s.can_soften());
}
