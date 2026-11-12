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
    pub fn frame(&mut self, dt: f64, ui: impl FnMut(&egui::Context)) -> Vec<u8> {
        self.frame_with(dt, vec![], ui)
    }

    /// [`frame`](Self::frame) with input events (pointer moves and clicks).
    pub fn frame_with(
        &mut self,
        dt: f64,
        events: Vec<egui::Event>,
        mut ui: impl FnMut(&egui::Context),
    ) -> Vec<u8> {
        self.time += dt;
        let [w, h] = self.size;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            time: Some(self.time),
            events,
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

    /// Piles topple on the GPU through the toppler the app registers at start (`crate::run`).
    fn gpu_piles() {
        jetcarp_core::pile::set_gpu_factory(|| {
            jetcarp_gpu::pile::PileGpu::headless()
                .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
        });
    }

    /// The Sandpile window in a light and a dark palette (needs `JETCARP_OUT` set to a scratch
    /// folder: the pile is written there).
    #[test]
    #[ignore]
    fn offscreen_sandpile_window() {
        gpu_piles();
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

    /// The Sandpile window through a stop and a resume: a larger pile drawn live, stopped part
    /// way (paused, its finished steps kept), resumed, and done (needs `JETCARP_OUT` and
    /// `JETCARP_LIBRARY`: the pile is a job in that catalog).
    #[test]
    #[ignore]
    fn offscreen_sandpile_stop_resume() {
        gpu_piles();
        use crate::pile_ui::PileWindow;
        let Some(dir) = out_dir() else { return };
        assert!(std::env::var_os("JETCARP_OUT").is_some(), "set JETCARP_OUT");
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(900, 860) else {
            return;
        };
        let show = |w: &mut PileWindow, ctx: &egui::Context| w.show(ctx);
        let mut w = PileWindow::default();
        w.palette = "Tide".into();
        w.open_env("square:4e6");
        let ok = frames_until(&mut o, 60.0, &mut w, show, |w| {
            let (st, step, steps) = w.test_progress();
            st == "running" && steps > 0 && step + 2 >= steps
        });
        assert!(ok, "the pile never got far: {:?}", w.test_progress());
        // A second or two into a long step, so the drawing is the step in flight.
        frames_until(&mut o, 1.5, &mut w, show, |_| false);
        shot(&mut o, &mut w, show, &dir, "sandpile-running");
        seen_running(w.test_job().unwrap());
        w.test_stop();
        let ok = frames_until(&mut o, 30.0, &mut w, show, |w| w.finished());
        assert!(ok, "the pile never stopped: {:?}", w.test_progress());
        shot(&mut o, &mut w, show, &dir, "sandpile-stopped");
        seen(w.test_job().unwrap());
        if w.test_resume() {
            frames_until(&mut o, 2.0, &mut w, show, |_| false);
            shot(&mut o, &mut w, show, &dir, "sandpile-resumed");
            let ok = frames_until(&mut o, 300.0, &mut w, show, |w| w.finished());
            assert!(ok, "the pile never finished: {:?}", w.test_progress());
            assert_eq!(w.test_progress().0, "done");
        } else {
            eprintln!("the pile ended before the stop: {:?}", w.test_progress());
        }
        shot(&mut o, &mut w, show, &dir, "sandpile-resumed-done");
    }

    /// The Ants window through a stop and a resume (the scratch catalog `JETCARP_LIBRARY`).
    #[test]
    #[ignore]
    fn offscreen_ants_stop_resume() {
        use crate::ants_ui::AntsWindow;
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(900, 980) else {
            return;
        };
        let show = |w: &mut AntsWindow, ctx: &egui::Context| {
            w.show(ctx);
        };
        let mut w = AntsWindow::default();
        w.open_env("square,hex:2-7");
        frames_until(&mut o, 3.0, &mut w, show, |_| false);
        shot(&mut o, &mut w, show, &dir, "ants-running");
        seen_running(w.test_job().unwrap());
        w.test_stop();
        let ok = frames_until(&mut o, 30.0, &mut w, show, |w| w.search_done());
        assert!(ok, "the search never stopped");
        shot(&mut o, &mut w, show, &dir, "ants-stopped");
        if w.test_resume() {
            frames_until(&mut o, 2.0, &mut w, show, |_| false);
            shot(&mut o, &mut w, show, &dir, "ants-resumed");
            // Stopped again (the whole search is long); its runs so far are stored.
            w.test_stop();
            let ok = frames_until(&mut o, 30.0, &mut w, show, |w| w.search_done());
            assert!(ok, "the resumed search never stopped");
            assert_eq!(w.test_status(), "paused");
        } else {
            eprintln!("the search ended before the stop: {}", w.test_status());
        }
        frames_until(&mut o, 2.0, &mut w, show, |_| false);
        shot(&mut o, &mut w, show, &dir, "ants-stopped-again");
    }

    /// Frames of `w`'s window until `done(w)` or `secs`; whether `done` came.
    fn frames_until<W>(
        o: &mut Offscreen,
        secs: f64,
        w: &mut W,
        mut ui: impl FnMut(&mut W, &egui::Context),
        mut done: impl FnMut(&W) -> bool,
    ) -> bool {
        let t = Instant::now();
        while t.elapsed().as_secs_f64() < secs {
            o.frame(0.1, |ctx| ui(w, ctx));
            if done(w) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// How the other front ends see job `job`: its status from `jobs.list` on an MCP context
    /// in this process (whether it runs here), from the MCP tool `job_status` (as over the
    /// app's link), and from `jetcarp jobs list` in a process of its own (when the binary is
    /// built). Printed for the record, and returned.
    fn seen(job: i64) -> ((String, bool), String, Option<String>) {
        use jetcarp_api::op::{Op, Quiet};
        use jetcarp_api::ops::jobs::{JobsList, ListIn};
        let l = JobsList::run(&jetcarp_api::Ctx::interactive(), ListIn::default(), &Quiet).unwrap();
        let line = l
            .jobs
            .iter()
            .find(|j| j.job == job)
            .expect("jobs.list lists it");
        let listed = (line.status.clone(), line.here);
        let out =
            jetcarp_mcp::job_tools::call("job_status", &serde_json::json!({ "job": job })).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out.joined_text()).unwrap();
        let mcp = v["status"].as_str().unwrap_or_default().to_string();
        let bin =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release/jetcarp");
        let cli = bin.exists().then(|| {
            let o = std::process::Command::new(&bin)
                .args(["jobs", "list", "--json"])
                .output()
                .unwrap();
            let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_default();
            v["jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|j| j["job"] == job)
                .and_then(|j| j["status"].as_str())
                .unwrap_or("not listed")
                .to_string()
        });
        eprintln!(
            "job {job}: jobs.list {listed:?}, MCP job_status {mcp}, jetcarp jobs list {cli:?}"
        );
        (listed, mcp, cli)
    }

    /// Job `job` runs here, and every front end says so.
    fn seen_running(job: i64) {
        let (listed, mcp, cli) = seen(job);
        assert_eq!(listed, ("running".to_string(), true));
        assert_eq!(mcp, "running");
        if let Some(c) = cli {
            assert_eq!(c, "running");
        }
    }

    /// A settled frame of `w`'s window saved as `<dir>/<name>.png`.
    fn shot<W>(
        o: &mut Offscreen,
        w: &mut W,
        mut ui: impl FnMut(&mut W, &egui::Context),
        dir: &std::path::Path,
        name: &str,
    ) {
        let px = settle(o, 0.0, |ctx| ui(w, ctx), || true);
        let p = dir.join(format!("{name}.png"));
        o.save(&px, &p).unwrap();
        eprintln!("wrote {}", p.display());
    }

    /// The Quick Start screen over the scratch catalog `JETCARP_LIBRARY` (which should hold
    /// entries of every source: an atlas, a pack's entry, a promotion; the built-ins come by
    /// themselves): every source's cards, a card's menu opened by a right click, and a rating and
    /// a hide from that menu reaching the library.
    #[test]
    #[ignore]
    fn offscreen_start_screen() {
        use crate::start::Menu;
        use jetcarp_store::{Library, QsSource};
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(1400, 900) else {
            return;
        };
        // As the app draws it each frame, noticing changes to the library's entries.
        let show = |m: &mut Menu, ctx: &egui::Context| {
            m.refresh_if_changed();
            ctx.root_ui(|ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = m.show(ui);
                });
            });
        };
        // A click of a mouse button at a point (moved there first).
        let click = |o: &mut Offscreen, m: &mut Menu, at: egui::Pos2, button| {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            o.frame_with(0.05, vec![egui::Event::PointerMoved(at)], |ctx| {
                show(m, ctx)
            });
            o.frame_with(0.05, vec![press(true)], |ctx| show(m, ctx));
            o.frame_with(0.05, vec![press(false)], |ctx| show(m, ctx));
        };
        let mut m = Menu::new();
        let loaded = frames_until(&mut o, 120.0, &mut m, show, |m| m.test_loaded());
        assert!(loaded, "the menu never loaded");
        // Time for the cards on screen to build and run.
        frames_until(&mut o, 5.0, &mut m, show, |_| false);
        shot(&mut o, &mut m, show, &dir, "start");
        let mut counts = vec![];
        for s in QsSource::ALL {
            m.view.filter.source = Some(s);
            frames_until(&mut o, 4.0, &mut m, show, |_| false);
            shot(&mut o, &mut m, show, &dir, &format!("start-{}", s.name()));
            counts.push((s, m.items.iter().filter(|i| i.source == s).count()));
        }
        eprintln!("entries by source: {counts:?}");
        assert!(counts.iter().all(|c| c.1 > 0), "{counts:?}");
        // A card's menu, by a right click on the first atlas card.
        m.view.filter.source = Some(QsSource::Atlas);
        frames_until(&mut o, 2.0, &mut m, show, |_| false);
        let (handle, rect) = m.test_first_card().expect("a card on screen");
        click(
            &mut o,
            &mut m,
            rect.center(),
            egui::PointerButton::Secondary,
        );
        shot(&mut o, &mut m, show, &dir, "start-card-menu");
        let places = m.test_menu_places();
        assert_eq!(places.len(), 6, "five stars and Hide in the card's menu");
        // Its fourth star clicked: the card shows the rating at once, and the library has it.
        click(
            &mut o,
            &mut m,
            places[3].center(),
            egui::PointerButton::Primary,
        );
        let lib = Library::open_default();
        let row = lib.get_quickstart(&handle).unwrap();
        let rated = frames_until(&mut o, 10.0, &mut m, show, |_| {
            lib.user_ratings()
                .unwrap()
                .get(&("quickstart".to_string(), row.id.clone()))
                == Some(&4.0)
        });
        assert!(rated, "the rating never reached the library");
        shot(&mut o, &mut m, show, &dir, "start-rated");
        // Hidden from it: gone from the menu after its reload, and hidden in the library.
        click(
            &mut o,
            &mut m,
            rect.center(),
            egui::PointerButton::Secondary,
        );
        let places = m.test_menu_places();
        click(
            &mut o,
            &mut m,
            places[5].center(),
            egui::PointerButton::Primary,
        );
        let gone = frames_until(&mut o, 20.0, &mut m, show, |m| {
            m.test_loaded() && m.items.iter().all(|i| i.handle != handle)
        });
        assert!(gone, "the hidden entry stayed in the menu");
        assert!(lib.get_quickstart(&handle).unwrap().hidden);
        lib.set_quickstart_hidden(&handle, false).unwrap();
        eprintln!("rated and hid {handle}");
    }

    /// The Hunts dashboard (the scratch catalog `JETCARP_LIBRARY`): a hunt started from its
    /// form, seen running, paused, resumed, and done with its heat map and finds.
    #[test]
    #[ignore]
    fn offscreen_dashboard_window() {
        use crate::dash_ui::DashWindow;
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(1300, 900) else {
            return;
        };
        let show = |w: &mut DashWindow, ctx: &egui::Context| w.show(ctx);
        let status = |w: &DashWindow| {
            w.test_jobs()
                .first()
                .map(|j| (j.id, j.status.clone(), j.here, j.done))
        };
        let mut w = DashWindow::default();
        w.test_start("B3/S23, B36/S23", "square", 20000);
        let ok = frames_until(&mut o, 30.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.1 == "running" && s.3 > 0)
        });
        assert!(ok, "the hunt never showed as running: {:?}", status(&w));
        shot(&mut o, &mut w, show, &dir, "dashboard-running");
        let job = status(&w).unwrap().0;
        seen_running(job);
        w.test_pause(job);
        let ok = frames_until(&mut o, 30.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.1 == "paused" && !s.2)
        });
        assert!(ok, "the hunt never showed as paused: {:?}", status(&w));
        shot(&mut o, &mut w, show, &dir, "dashboard-paused");
        seen(job);
        w.test_resume(job);
        let ok = frames_until(&mut o, 30.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.1 == "running" && s.2)
        });
        assert!(ok, "the hunt never showed as resumed: {:?}", status(&w));
        shot(&mut o, &mut w, show, &dir, "dashboard-resumed");
        let ok = frames_until(&mut o, 240.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.1 == "done")
        });
        assert!(ok, "the hunt never finished: {:?}", status(&w));
        // Time for the heat map, the finds, and their animations.
        frames_until(&mut o, 15.0, &mut w, show, |_| false);
        shot(&mut o, &mut w, show, &dir, "dashboard-done");
        w.shutdown(Duration::from_secs(20));
    }

    /// The Hunts dashboard with an explore search (the scratch catalog `JETCARP_LIBRARY`) started
    /// through `search.start` in the background: seen running (by every front end), paused with
    /// its *Continue here* button, and continued from its archive.
    #[test]
    #[ignore]
    fn offscreen_dashboard_explore() {
        use crate::dash_ui::DashWindow;
        use jetcarp_api::op::{Op, Quiet};
        use jetcarp_api::ops::search::{SearchStart, SearchStartIn};
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        let Some(mut o) = Offscreen::new(1300, 900) else {
            return;
        };
        let cx = crate::window_job::ctx(&jetcarp_store::Library::default_path());
        let spec = serde_json::json!({
            "tilings": "square, hex, trihex, cairo",
            "families": "life_like, generations, cyclic, hodgepodge, turmite",
            "screen_cells": 3000, "screen_gens": 160, "confirm_cells": 6000, "confirm_gens": 320,
        });
        let r = SearchStart::run(
            &cx,
            SearchStartIn {
                strategy: Some("explore".into()),
                spec,
                label: Some("offscreen explore".into()),
                threads: Some(4),
                background: true,
                ..Default::default()
            },
            &Quiet,
        )
        .unwrap();
        let job = r.job;
        let show = |w: &mut DashWindow, ctx: &egui::Context| w.show(ctx);
        let status = |w: &DashWindow| {
            w.test_jobs()
                .into_iter()
                .find(|j| j.id == job)
                .map(|j| (j.status.clone(), j.here, j.done, j.deterministic))
        };
        let mut w = DashWindow::default();
        w.test_open();
        let ok = frames_until(&mut o, 60.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.0 == "running" && s.2 > 500)
        });
        assert!(ok, "the search never showed as running: {:?}", status(&w));
        assert!(!status(&w).unwrap().3, "explore is not deterministic");
        shot(&mut o, &mut w, show, &dir, "dashboard-explore-running");
        seen_running(job);
        w.test_pause(job);
        let ok = frames_until(&mut o, 60.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.0 == "paused" && !s.1)
        });
        assert!(ok, "the search never showed as paused: {:?}", status(&w));
        shot(&mut o, &mut w, show, &dir, "dashboard-explore-paused");
        let before = status(&w).unwrap().2;
        w.test_resume(job);
        let ok = frames_until(&mut o, 60.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.0 == "running" && s.1 && s.2 > before + 200)
        });
        assert!(ok, "the search never continued: {:?}", status(&w));
        shot(&mut o, &mut w, show, &dir, "dashboard-explore-continued");
        w.test_pause(job);
        frames_until(&mut o, 60.0, &mut w, show, |w| {
            status(w).is_some_and(|s| s.0 == "paused" && !s.1)
        });
        w.shutdown(Duration::from_secs(20));
    }

    /// The Catalog tools window (the scratch catalog `JETCARP_LIBRARY`, where a small soup search
    /// first stores a block and a glider): a collision, then an evolve search seen running,
    /// stopped, and (where it can be) resumed to its end.
    #[test]
    #[ignore]
    fn offscreen_catalog_tools_window() {
        use crate::catalog_tools_ui::CatalogTools;
        use jetcarp_api::op::{Op, Quiet};
        use jetcarp_api::ops::search::{SearchStart, SearchStartIn};
        let Some(dir) = out_dir() else { return };
        assert!(
            std::env::var_os("JETCARP_LIBRARY").is_some(),
            "set JETCARP_LIBRARY"
        );
        assert!(std::env::var_os("JETCARP_OUT").is_some(), "set JETCARP_OUT");
        let seed = SearchStartIn {
            strategy: Some("search".into()),
            spec: serde_json::json!({"rules": ["B3/S23"], "soups": 24, "gens": 1500}),
            ..Default::default()
        };
        SearchStart::run(&jetcarp_api::Ctx::new(), seed, &Quiet).unwrap();
        let Some(mut o) = Offscreen::new(760, 900) else {
            return;
        };
        let show = |w: &mut CatalogTools, ctx: &egui::Context| {
            w.show(ctx, None);
        };
        let mut w = CatalogTools::default();
        w.test_collide("block", "glider", 8);
        let ok = frames_until(&mut o, 120.0, &mut w, show, |w| !w.collide_running());
        assert!(ok, "the collision never finished");
        shot(&mut o, &mut w, show, &dir, "catalog-tools-collide");
        w.test_evolve("spectre", 48, 2000, 16);
        frames_until(&mut o, 1.5, &mut w, show, |_| false);
        shot(&mut o, &mut w, show, &dir, "catalog-tools-evolve-running");
        if let Some(job) = w.test_evolve_job() {
            seen_running(job);
        }
        w.test_stop_evolve();
        frames_until(&mut o, 60.0, &mut w, show, |w| w.test_evolve_stopped());
        shot(&mut o, &mut w, show, &dir, "catalog-tools-evolve-stopped");
        if w.test_resume_evolve() {
            frames_until(&mut o, 4.0, &mut w, show, |_| false);
            shot(&mut o, &mut w, show, &dir, "catalog-tools-evolve-resumed");
            let ok = frames_until(&mut o, 300.0, &mut w, show, |w| !w.evolve_running());
            assert!(ok, "the evolve search never finished");
        }
        shot(&mut o, &mut w, show, &dir, "catalog-tools-evolve-done");
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
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../jetcarp-core/data/scenes/life-not.json"),
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
