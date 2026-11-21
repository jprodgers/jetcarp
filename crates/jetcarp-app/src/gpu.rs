//! The tile renderer (`jetcarp_display::gpu`) as an egui paint callback, drawn through the strobe
//! guard (#97).
//!
//! A view's tiles are drawn first into a texture of the view's own (`StrobeGpu::scene_target`),
//! in the callback's `prepare`, on egui's encoder. The guard's pass measures that frame there,
//! and softens it when the guard says so; `paint` then copies the frame to show into egui's pass,
//! filling the callback's viewport. Every view the app draws (the grid tile, full screen, each
//! output window, the grid mirror) is its own guarded view, with its own readings.
//!
//! The guard's decisions are made here, at render time, from the readings of frames the GPU has
//! finished; [`GuardFeed`] carries in what they need from the app (the setting, the pace, a new
//! scene, a resumed one) and [`GuardOut`] carries back what each view's guard asks (a pace cap, a
//! pause) and what it did, for the app's next frame.

use eframe::egui;
use egui_wgpu::wgpu;
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::guard::{Pace, Report};
pub use jetcarp_display::gpu::*;
use jetcarp_display::strobe_gpu::GuardedGpu;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// What the app's GPU paint callbacks keep between frames.
pub struct AppGpu {
    pub tiles: TileRenderer,
    /// The format egui draws into.
    pub format: wgpu::TextureFormat,
    views: HashMap<u64, ViewGuard>,
}

struct ViewGuard {
    g: GuardedGpu,
    scene: u64,
    resume: u64,
}

impl AppGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> AppGpu {
        AppGpu {
            tiles: TileRenderer::new(device, format),
            format,
            views: HashMap::new(),
        }
    }
}

/// What a view's guard needs from the app this frame.
#[derive(Clone)]
pub struct GuardFeed {
    /// The setting for this view: the app's, or the desktop's in screensaver mode.
    pub on: bool,
    /// The saved answer is *at risk*: the guard's stricter stripe rule.
    pub at_risk: bool,
    /// The app's clock, in seconds.
    pub now: f64,
    pub viewing: Viewing,
    /// The screen's pixels, when the view is a part of it (the grid tile in the window).
    pub screen_px: Option<f64>,
    pub pace: Pace,
    /// Changes with each new world: a new scene for the guard.
    pub scene: u64,
    /// Changes each time the person runs a world the guard paused.
    pub resume: u64,
    /// The colour behind the grid.
    pub backdrop: [u8; 3],
    pub out: Arc<Mutex<GuardOut>>,
}

/// What each view's guard said last, by view.
#[derive(Default)]
pub struct GuardOut {
    pub views: HashMap<u64, ViewOut>,
}

/// One view's guard, as of its last frame.
#[derive(Clone, Debug, Default)]
pub struct ViewOut {
    /// The app's clock when it said this.
    pub at: f64,
    pub cap: Option<f32>,
    pub paused: Option<String>,
    pub report: Report,
    /// What it did since the app last looked.
    pub notes: Vec<String>,
}

/// One view's tiles.
#[derive(Clone)]
pub struct TileCallback {
    pub draw: TileDraw,
    /// The strobe guard's feed (None: drawn straight into egui's pass, unmeasured).
    pub guard: Option<GuardFeed>,
}

impl egui_wgpu::CallbackTrait for TileCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        enc: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(app) = res.get_mut::<AppGpu>() else {
            return vec![];
        };
        app.tiles.prepare(device, queue, &self.draw);
        let Some(feed) = &self.guard else {
            return vec![];
        };
        let view = app
            .views
            .entry(self.draw.view)
            .or_insert_with(|| ViewGuard {
                g: GuardedGpu::new(device, feed.viewing, feed.screen_px, feed.on),
                scene: feed.scene,
                resume: feed.resume,
            });
        // Last frame's work has been submitted by now: its readbacks can be mapped.
        view.g.gpu.after_submit();
        view.g.guard.set_on(feed.on);
        view.g.guard.set_viewing(feed.viewing, feed.screen_px);
        view.g.guard.set_at_risk(feed.at_risk);
        if view.scene != feed.scene {
            view.scene = feed.scene;
            view.g.guard.new_scene();
        }
        if view.resume != feed.resume {
            view.resume = feed.resume;
            view.g.guard.resume();
        }
        view.g.update(device, feed.pace);
        let size = [
            self.draw.rect_size[0].round().max(1.0) as u32,
            self.draw.rect_size[1].round().max(1.0) as u32,
        ];
        let target = view.g.gpu.scene_target(device, size, app.format);
        let [r, g, b] = feed.backdrop;
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("jetcarp view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64 / 255.0,
                            g: g as f64 / 255.0,
                            b: b as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            app.tiles.paint(&mut pass, &self.draw);
        }
        view.g.frame(device, queue, enc, None, feed.now);
        view.g.gpu.prepare_blit(device, app.format, None);
        if let Ok(mut out) = feed.out.lock() {
            let o = out.views.entry(self.draw.view).or_default();
            o.at = feed.now;
            o.cap = view.g.guard.cap();
            o.paused = view.g.guard.paused().map(str::to_string);
            o.report = view.g.guard.report();
            o.notes.extend(view.g.guard.take_notes());
        }
        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        let Some(app) = res.get::<AppGpu>() else {
            return;
        };
        if self.guard.is_none() {
            app.tiles.paint(pass, &self.draw);
            return;
        }
        if let Some(v) = app.views.get(&self.draw.view) {
            v.g.gpu.blit(pass);
        }
    }
}
