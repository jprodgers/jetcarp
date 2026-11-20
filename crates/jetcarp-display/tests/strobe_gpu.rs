//! The strobe guard's GPU pass against the processor's: the same tile maps, the same half image,
//! the same softened frames, from frames uploaded as textures. The GPU's sRGB decoding is the
//! texture unit's, which may differ from the exact curve by a few parts in ten thousand, so the
//! numbers agree to that. Each test skips itself when the
//! machine has no GPU adapter. `gpu_cost` (ignored; run with `-- --ignored --nocapture`) times the
//! pass at common screen sizes.

use jetcarp_core::strobe::frame::{reduce, reduce_both};
use jetcarp_core::strobe::soften::{Plan, Softener};
use jetcarp_display::strobe_gpu::{Shown, StrobeGpu};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    timestamps: bool,
}

fn gpu() -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let choice =
        jetcarp_gpu::select::pick(&instance, jetcarp_gpu::select::Purpose::Display, None).ok()?;
    if let Err(e) = &choice.usable {
        eprintln!("skipped: {e}");
        return None;
    }
    let adapter = choice.adapter;
    let ts = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let timestamps = adapter.features().contains(ts);
    let mut desc = jetcarp_display::painter::device_descriptor(&adapter);
    if timestamps {
        desc.required_features |= ts;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
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
        for t in 0..cpu.len() {
            assert!(
                (r.input.lum[t] - cpu.lum[t]).abs() < 5e-4,
                "{w}x{h} tile {t}: {} vs {}",
                r.input.lum[t],
                cpu.lum[t]
            );
            assert!(
                (r.input.red[t] - cpu.red[t]).abs() < 0.2,
                "{w}x{h} tile {t}: red {} vs {}",
                r.input.red[t],
                cpu.red[t]
            );
        }
        assert_eq!(r.input, r.output);
        let gh = r.half.expect("asked for");
        assert_eq!((gh.width, gh.height), (half.width, half.height));
        for (a, b) in gh.lum.iter().zip(&half.lum) {
            assert!((a - b).abs() < 2e-3, "{a} vs {b}");
        }
        // Without the half image.
        let (_, r) = g.frame(&mut s, &tex, &Plan::none_for(w, h), 0.016, 2.0, false);
        assert!(r.half.is_none());
        assert!((r.input.lum[0] - reduce(&f, w, h).lum[0]).abs() < 5e-4);
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
        for t in 0..c.input.len() {
            assert!((r.input.lum[t] - c.input.lum[t]).abs() < 5e-4, "frame {k}");
            assert!(
                (r.output.lum[t] - c.output.lum[t]).abs() < 2e-3,
                "frame {k} tile {t}: {} vs {}",
                r.output.lum[t],
                c.output.lum[t]
            );
            assert!(
                (r.output.red[t] - c.output.red[t]).abs() < 1.0,
                "frame {k} tile {t}: red {} vs {}",
                r.output.red[t],
                c.output.red[t]
            );
        }
        let pixels = g.read(s.softened_texture().expect("softened"));
        let want = c.shown.expect("softened");
        let worst = pixels
            .iter()
            .zip(&want)
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        assert!(worst <= 3, "frame {k}: pixels differ by up to {worst}");
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
                let _ = s.take(&g.device);
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
                }
            }
            times.sort_by(f64::total_cmp);
            let med = times[times.len() / 2];
            let bytes = (w * h * 4) as f64;
            println!(
                "{w} x {h}, {name}: median {med:.3} ms ({:.1}% of a 60 Hz frame), {:.1} GB/s of the frame read{}",
                med / (1000.0 / 60.0) * 100.0,
                bytes / (med / 1e3) / 1e9,
                if g.timestamps { "" } else { " (wall clock)" }
            );
        }
    }
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
