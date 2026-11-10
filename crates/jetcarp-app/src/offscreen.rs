//! Offscreen pictures of tool windows (tests only): egui drawn by egui-wgpu into a texture on a
//! headless GPU device, for screenshot checks where no window can be shown (a compositor that
//! holds back frames, a machine with no display). The `#[ignore]`d tests below draw the
//! Sandpile, Ants, and HashLife windows in chosen palettes and write PNGs to
//! `JETCARP_OFFSCREEN_OUT` (run with `cargo test -p jetcarp-app offscreen -- --ignored`).

use eframe::egui;
use egui_wgpu::wgpu;

/// A headless egui context and renderer of a fixed size.
pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    pub ctx: egui::Context,
    size: [u32; 2],
    time: f64,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Offscreen {
    /// `None` when there is no GPU adapter.
    pub fn new(width: u32, height: u32) -> Option<Offscreen> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let renderer = egui_wgpu::Renderer::new(&device, FORMAT, Default::default());
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        Some(Offscreen {
            device,
            queue,
            renderer,
            ctx,
            size: [width, height],
            time: 0.0,
        })
    }

    /// Run one frame of `ui` (`dt` seconds after the last) and return its pixels (RGBA).
    pub fn frame(&mut self, dt: f64, mut ui: impl FnMut(&egui::Context)) -> Vec<u8> {
        self.time += dt;
        let [w, h] = self.size;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            time: Some(self.time),
            ..Default::default()
        };
        let mut out = self.ctx.run_pass(input, |ctx| ui(ctx));
        let ppp = out.pixels_per_point;
        let prims = self.ctx.tessellate(out.shapes, ppp);
        for (id, ds) in &out.textures_delta.set {
            for d in ds {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, d);
            }
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let sd = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [w, h],
            pixels_per_point: ppp,
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        let extra = self
            .renderer
            .update_buffers(&self.device, &self.queue, &mut enc, &prims, &sd);
        {
            let bg = self.ctx.global_style().visuals.panel_fill;
            let c = |v: u8| v as f64 / 255.0;
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c(bg.r()),
                            g: c(bg.g()),
                            b: c(bg.b()),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &prims, &sd);
        }
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen-read"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            tex.size(),
        );
        self.queue
            .submit(extra.into_iter().chain(std::iter::once(enc.finish())));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        {
            let m = slice.get_mapped_range().expect("mapped readback");
            for r in m.chunks(row as usize) {
                px.extend_from_slice(&r[..(w * 4) as usize]);
            }
        }
        buf.unmap();
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        out.textures_delta.clear();
        px
    }

    /// Write pixels from [`frame`](Self::frame) to a PNG.
    pub fn save(&self, px: &[u8], path: &std::path::Path) -> Result<(), String> {
        let img = jetcarp_core::render::Image {
            width: self.size[0],
            height: self.size[1],
            rgba: px.to_vec(),
        };
        jetcarp_core::render::write_png(path, &img, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn out_dir() -> Option<PathBuf> {
        let d = PathBuf::from(std::env::var_os("JETCARP_OFFSCREEN_OUT")?);
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }

    /// Frames of `ui` until `done` (or `secs`), then one more; returns the last frame.
    fn settle(
        o: &mut Offscreen,
        secs: f64,
        mut ui: impl FnMut(&egui::Context),
        mut done: impl FnMut() -> bool,
    ) -> Vec<u8> {
        let t = Instant::now();
        for _ in 0..4 {
            o.frame(0.05, &mut ui);
        }
        while !done() && t.elapsed().as_secs_f64() < secs {
            o.frame(0.1, &mut ui);
            std::thread::sleep(Duration::from_millis(50));
        }
        for _ in 0..3 {
            o.frame(0.05, &mut ui);
        }
        o.frame(0.05, &mut ui)
    }

    /// The Sandpile window in a light and a dark palette (needs `JETCARP_OUT` set to a scratch
    /// folder: the pile is written there).
    #[test]
    #[ignore]
    fn offscreen_sandpile_window() {
        let Some(dir) = out_dir() else { return };
        assert!(std::env::var_os("JETCARP_OUT").is_some(), "set JETCARP_OUT");
        for (pal, rev, tiling) in [("Candy", false, "hex:1e5"), ("Aurora", true, "square:2e5")] {
            let Some(mut o) = Offscreen::new(900, 860) else {
                eprintln!("no GPU adapter");
                return;
            };
            let mut w = crate::pile_ui::PileWindow::default();
            w.palette = pal.into();
            w.reverse = rev;
            w.open_env(tiling);
            let t = Instant::now();
            while !w.finished() && t.elapsed().as_secs() < 60 {
                o.frame(0.1, |ctx| w.show(ctx));
                std::thread::sleep(Duration::from_millis(50));
            }
            let px = settle(&mut o, 0.0, |ctx| w.show(ctx), || true);
            let p = dir.join(format!("sandpile-{}.png", pal.to_lowercase()));
            o.save(&px, &p).unwrap();
            eprintln!("wrote {}", p.display());
            if pal == "Candy" {
                // Another palette after the run: the kept pile is redrawn at once, and an
                // export in the new colours is offered.
                w.set_look("Glacier", true);
                let px = settle(&mut o, 0.0, |ctx| w.show(ctx), || true);
                let p = dir.join("sandpile-candy-then-glacier.png");
                o.save(&px, &p).unwrap();
                eprintln!("wrote {}", p.display());
            }
        }
    }

    /// The Ants window (a small search into the scratch catalog `JETCARP_LIBRARY`, the first run
    /// selected) in Glacier.
    #[test]
    #[ignore]
    fn offscreen_ants_window() {
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(900, 980) else {
            return;
        };
        let mut w = crate::ants_ui::AntsWindow::default();
        w.palette = Some("Glacier".into());
        w.open_env("square:2-3");
        let t = Instant::now();
        // Let the search finish, then select the first (best) run.
        while t.elapsed().as_secs() < 60 {
            o.frame(0.1, |ctx| {
                w.show(ctx);
            });
            if w.search_done() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        w.select_first();
        let px = settle(
            &mut o,
            30.0,
            |ctx| {
                w.show(ctx);
            },
            || false,
        );
        let p = dir.join("ants-glacier.png");
        o.save(&px, &p).unwrap();
        eprintln!("wrote {}", p.display());
    }

    /// The HashLife view of the Life NOT gate after 2^10 generations, in Synthwave and Paper.
    #[test]
    #[ignore]
    fn offscreen_hashlife_window() {
        let Some(dir) = out_dir() else { return };
        let Some(mut o) = Offscreen::new(900, 700) else {
            return;
        };
        let sc = jetcarp_core::Scene::from_json(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jetcarp-core/data/scenes/life-not.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let world = jetcarp_core::World::from_scene(&sc).unwrap();
        for pal in [Some("Synthwave"), Some("Paper")] {
            let mut w = crate::hlview_ui::HlView::default();
            w.palette = pal.map(String::from);
            w.open_env(&world, "2^10");
            let px = settle(&mut o, 5.0, |ctx| w.show(ctx), || false);
            let p = dir.join(format!("hashlife-{}.png", pal.unwrap().to_lowercase()));
            o.save(&px, &p).unwrap();
            eprintln!("wrote {}", p.display());
        }
    }
}
