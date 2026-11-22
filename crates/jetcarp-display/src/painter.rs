//! Draws a stage, with its caption, onto a surface: what every place JETCARP shows a scene shares
//! (the host's windows and wallpapers, and the macOS screen saver bundle).
//!
//! A painter for a surface people see draws through the strobe guard ([`Painter::guarded`],
//! `jetcarp_core::strobe`): each frame is measured on the GPU, and softened there when the guard
//! says so, before the caption is drawn over it (the caption fades over seconds and is not
//! measured). Where the surface can be read ([`readable`]) the frame is drawn onto it and read in
//! place, so an unsoftened frame costs only the measuring; elsewhere it is drawn into a texture
//! and copied to the surface. The guard's pace cap and pause go to the stage through
//! [`Stage::set_guard`]. Offscreen painters (the tuner's, sweep cards) have no guard.

use crate::caption::{CaptionPainter, CaptionSettings, Insets};
use crate::gpu::{TileRenderer, slot_capacity};
use crate::stage::Stage;
use crate::strobe_gpu::{GuardedGpu, Shown};
use crate::view::Views;
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::guard::{Pace, Report};
use std::sync::atomic::{AtomicU64, Ordering};

/// How often the caption's words are refreshed (its generation count ticks this fast).
const CAPTION_EVERY: f64 = 0.25;

/// Adapter limits a very large world needs (the same as the app asks for).
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let al = adapter.limits();
    let mut d = wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().using_resolution(al.clone()),
        ..Default::default()
    };
    let l = &mut d.required_limits;
    l.max_buffer_size = l.max_buffer_size.max(al.max_buffer_size);
    l.max_storage_buffer_binding_size = l
        .max_storage_buffer_binding_size
        .max(al.max_storage_buffer_binding_size);
    d
}

/// A surface set-up of `size` pixels that keeps cell colours exact, and never blocks where the
/// platform allows it.
pub fn surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: (u32, u32),
) -> Result<wgpu::SurfaceConfiguration, String> {
    // Plain (not sRGB) formats keep cell colours exact.
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or(caps.formats.first().copied())
        .ok_or("the surface has no formats")?;
    // Mailbox never blocks when the compositor stops asking for frames; the caller paces itself.
    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.0.max(1),
        height: size.1.max(1),
        present_mode,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    })
}

/// Lets the strobe guard read the surface's frames in place, where the surface and the adapter
/// allow it: adds `TEXTURE_BINDING` to the configuration's usage and the sRGB twin of its format
/// to its view formats. True when it did (tell the painter with [`Painter::set_readable`]).
pub fn readable(
    config: &mut wgpu::SurfaceConfiguration,
    caps: &wgpu::SurfaceCapabilities,
    adapter: &wgpu::Adapter,
) -> bool {
    let views = crate::strobe_gpu::view_formats(config.format);
    let twin_ok = views.is_empty()
        || adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
    if !caps.usages.contains(wgpu::TextureUsages::TEXTURE_BINDING) || !twin_ok {
        return false;
    }
    config.usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    config.view_formats = views;
    true
}

/// Painters made so far, for [`Painter::id`].
static PAINTERS: AtomicU64 = AtomicU64::new(1);

/// Everything needed to draw one stage onto one surface.
pub struct Painter {
    renderer: TileRenderer,
    views: Views,
    caption: CaptionPainter,
    words: Vec<String>,
    words_at: f64,
    /// What the caption keeps clear of (a wallpaper's taskbar, menu bar or Dock).
    pub insets: Insets,
    /// The strobe guard, on surfaces people see.
    guard: Option<GuardedGpu>,
    /// The target can be read in place ([`readable`]).
    readable: bool,
    /// The stage's world count when last drawn: a new world is a new scene for the guard.
    builds: Option<u64>,
    id: u64,
}

impl Painter {
    /// A painter with no strobe guard, for pictures no one watches as they are drawn.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        Painter {
            renderer: TileRenderer::new(device, format),
            views: Views::new(slot_capacity(device)),
            caption: CaptionPainter::new(device, format),
            words: vec![],
            words_at: f64::NEG_INFINITY,
            insets: Insets::default(),
            guard: None,
            readable: false,
            builds: None,
            id: PAINTERS.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// A painter for a surface people see: it draws through the strobe guard, on or off as the
    /// setting for the desktop background and the screen saver says.
    pub fn guarded(device: &wgpu::Device, format: wgpu::TextureFormat, on: bool) -> Painter {
        let mut p = Painter::new(device, format);
        p.guard = Some(GuardedGpu::new(device, Viewing::screen(1.0), None, on));
        p
    }

    /// Draw the caption in `font` (the app's font setting, [`crate::caption::FontWatch`]).
    pub fn set_caption_font(&mut self, font: &std::sync::Arc<crate::caption::CaptionFont>) {
        self.caption.set_font(font);
    }

    /// Whether the target was configured to be read in place ([`readable`]).
    pub fn set_readable(&mut self, readable: bool) {
        self.readable = readable;
    }

    /// The setting changed.
    pub fn set_guard_on(&mut self, on: bool) {
        if let Some(g) = &mut self.guard {
            g.guard.set_on(on);
        }
    }

    /// The saved answer to the seizure warning is *at risk* (or no longer is): the guard's
    /// stricter stripe rule, similar parts regular or not ([`jetcarp_core::strobe::similar`]).
    pub fn set_guard_at_risk(&mut self, at_risk: bool) {
        if let Some(g) = &mut self.guard {
            g.guard.set_at_risk(at_risk);
        }
    }

    /// What the strobe guard is doing (None: this painter has none).
    pub fn guard_report(&self) -> Option<Report> {
        self.guard.as_ref().map(|g| g.guard.report())
    }

    /// The guard's pace cap and the reason it paused, if it did.
    pub fn guard_hold(&self) -> (Option<f32>, Option<String>) {
        self.guard.as_ref().map_or((None, None), |g| {
            (g.guard.cap(), g.guard.paused().map(str::to_string))
        })
    }

    /// What the guard did since the last call, for the log.
    pub fn take_guard_notes(&mut self) -> Vec<String> {
        self.guard.as_mut().map_or(vec![], |g| g.guard.take_notes())
    }

    /// This painter's number, for [`Stage::set_guard`].
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Draws `stage` onto `target`, `size` pixels with `scale` pixels per point, at time `now`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        size: [u32; 2],
        scale: f32,
        stage: &Stage,
        cap: &CaptionSettings,
        now: f64,
    ) {
        self.paint_timed(device, queue, target, size, scale, stage, cap, now, None);
    }

    /// The same, writing GPU timestamps at the start and end of the drawing (the tuner's).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_timed(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        size: [u32; 2],
        scale: f32,
        stage: &Stage,
        cap: &CaptionSettings,
        now: f64,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let [w, h] = size;
        let draw = stage.draw(&mut self.views, 0, [w as f32, h as f32]);
        if let Some(d) = &draw {
            self.renderer.prepare(device, queue, d);
        }
        if now - self.words_at >= CAPTION_EVERY {
            self.words = stage.caption(cap.generation);
            self.words_at = now;
        }
        self.caption.prepare_within(
            device,
            queue,
            &self.words,
            cap,
            size,
            scale,
            cap.opacity(stage.item_age(now)),
            self.insets,
        );
        let [r, g, b] = stage.backdrop();
        let clear = wgpu::Color {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
            a: 1.0,
        };
        let target_view = target.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        let pass_on =
            |enc: &'_ mut wgpu::CommandEncoder,
             view: &wgpu::TextureView,
             load: wgpu::LoadOp<wgpu::Color>,
             label: &'static str,
             timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>| {
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: timestamps,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime()
            };
        let Some(guard) = &mut self.guard else {
            let mut pass = pass_on(
                &mut enc,
                &target_view,
                wgpu::LoadOp::Clear(clear),
                "stage",
                timestamps,
            );
            if let Some(d) = &draw {
                self.renderer.paint(&mut pass, d);
            }
            self.caption.paint(&mut pass);
            drop(pass);
            queue.submit([enc.finish()]);
            return;
        };
        // The guard: its readings first (a new world is a new scene), then the frame, measured.
        if self.builds != Some(stage.builds()) {
            self.builds = Some(stage.builds());
            guard.guard.new_scene();
        }
        guard.guard.set_viewing(Viewing::screen(scale as f64), None);
        let pace = stage.pace();
        guard.update(
            device,
            Pace {
                speed: stage.scene_speed(),
                reached: stage.measured_speed(),
                frame_rate: pace.fps,
            },
        );
        let readable = self.readable;
        let scene = if readable {
            target_view.clone()
        } else {
            guard
                .gpu
                .scene_target(device, [w.max(1), h.max(1)], target.format())
        };
        {
            let mut pass = pass_on(
                &mut enc,
                &scene,
                wgpu::LoadOp::Clear(clear),
                "stage",
                timestamps,
            );
            if let Some(d) = &draw {
                self.renderer.paint(&mut pass, d);
            }
        }
        let source = readable.then_some(target);
        let shown = guard.frame(device, queue, &mut enc, source, now);
        let copy = shown == Shown::Softened || !readable;
        if copy {
            guard.gpu.prepare_blit(device, target.format(), source);
        }
        {
            let mut pass = pass_on(
                &mut enc,
                &target_view,
                wgpu::LoadOp::Load,
                "stage caption",
                None,
            );
            if copy {
                guard.gpu.blit(&mut pass);
            }
            self.caption.paint(&mut pass);
        }
        queue.submit([enc.finish()]);
        guard.gpu.after_submit();
    }
}

/// The canvas a screen's world is built for when it shows on surfaces of these sizes: the largest
/// width and height among them, so each draws pixel-exact, a larger monitor gets no border, and a
/// smaller one shows the middle.
pub fn shared_canvas(sizes: impl IntoIterator<Item = (u32, u32)>) -> Option<[f64; 2]> {
    sizes.into_iter().fold(None, |acc, (w, h)| {
        let [aw, ah] = acc.unwrap_or([0.0, 0.0]);
        Some([aw.max(w as f64), ah.max(h as f64)])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_world_fits_the_largest_monitor() {
        assert_eq!(shared_canvas([]), None);
        assert_eq!(
            shared_canvas([(1920, 1080), (2560, 1440)]),
            Some([2560.0, 1440.0])
        );
        // A tall monitor beside a wide one: wide enough for one and tall enough for the other.
        assert_eq!(
            shared_canvas([(2560, 1440), (1080, 1920)]),
            Some([2560.0, 1920.0])
        );
    }
}
