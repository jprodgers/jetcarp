//! The tile renderer (`jetcarp_display::gpu`) as an egui paint callback.

use eframe::egui;
use egui_wgpu::wgpu;
pub use jetcarp_display::gpu::*;

/// One view's tiles, drawn inside egui's render pass.
#[derive(Clone)]
pub struct TileCallback(pub TileDraw);

impl egui_wgpu::CallbackTrait for TileCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = res.get_mut::<TileRenderer>() {
            r.prepare(device, queue, &self.0);
        }
        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        if let Some(r) = res.get::<TileRenderer>() {
            r.paint(pass, &self.0);
        }
    }
}
