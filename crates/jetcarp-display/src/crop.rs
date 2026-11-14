//! A sweep card's picture: the middle of what a stage shows, drawn by the same painter the screen
//! uses, at the screen's own pixels (a 1-pixel cell is one pixel of the picture).

use crate::caption::CaptionSettings;
use crate::painter::Painter;
use crate::stage::Stage;
use jetcarp_core::render::Image;

/// The format pictures are drawn in: plain RGBA, as the host's surfaces are.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Draws the middle `size` pixels of what `stage` shows (no caption) and reads them back. The
/// stage's own view is used, so with each side of `size` as even or odd as the canvas's
/// ([`crate::tuning::crop_size`]), the picture's pixels are the screen's.
pub fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut Painter,
    stage: &Stage,
    size: [u32; 2],
    scale: f32,
) -> Option<Image> {
    stage.world()?;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("jetcarp-crop"),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
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
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    painter.paint(device, queue, &view, size, scale, stage, &caption, 0.0);
    read(device, queue, &tex, [0, 0], size)
}

/// Reads `size` pixels of an RGBA texture (made with `COPY_SRC`) from `origin`.
pub fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
) -> Option<Image> {
    let [w, h] = [size[0].max(1), size[1].max(1)];
    let row = (w * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("jetcarp-crop-read"),
        size: row as u64 * h as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
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
    queue.submit([enc.finish()]);
    buf.map_async(wgpu::MapMode::Read, .., |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let data = buf.get_mapped_range(..).ok()?;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        rgba.extend_from_slice(&data[start..start + w as usize * 4]);
    }
    drop(data);
    buf.unmap();
    Some(Image {
        width: w,
        height: h,
        rgba,
    })
}

/// Where in a frame of `canvas` pixels its middle `size` pixels start.
pub fn middle(canvas: [u32; 2], size: [u32; 2]) -> [u32; 2] {
    [
        canvas[0].saturating_sub(size[0]) / 2,
        canvas[1].saturating_sub(size[1]) / 2,
    ]
}
