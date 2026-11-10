//! Draws a stage, with its caption, onto a surface: what every place JETCARP shows a scene shares
//! (the host's windows and wallpapers, and the macOS screen saver bundle).

use crate::caption::{CaptionPainter, CaptionSettings};
use crate::gpu::{TileRenderer, slot_capacity};
use crate::stage::Stage;
use crate::view::Views;

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

/// Everything needed to draw one stage onto one surface.
pub struct Painter {
    renderer: TileRenderer,
    views: Views,
    caption: CaptionPainter,
    words: Vec<String>,
    words_at: f64,
}

impl Painter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        Painter {
            renderer: TileRenderer::new(device, format),
            views: Views::new(slot_capacity(device)),
            caption: CaptionPainter::new(device, format),
            words: vec![],
            words_at: f64::NEG_INFINITY,
        }
    }

    /// Draws `stage` onto `target`, `size` pixels with `scale` pixels per point, at time `now`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scale: f32,
        stage: &Stage,
        cap: &CaptionSettings,
        now: f64,
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
        self.caption.prepare(
            device,
            queue,
            &self.words,
            cap,
            size,
            scale,
            cap.opacity(stage.item_age(now)),
        );
        let [r, g, b] = stage.backdrop();
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stage"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            if let Some(d) = &draw {
                self.renderer.paint(&mut pass, d);
            }
            self.caption.paint(&mut pass);
        }
        queue.submit([enc.finish()]);
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
