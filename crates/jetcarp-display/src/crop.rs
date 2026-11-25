//! A sweep card's picture: the middle of what a stage shows, drawn by the same painter the screen
//! uses, at the screen's own pixels (a 1-pixel cell is one pixel of the picture). A scene drawn
//! row by row from the top of the screen (a rows rule, such as the Wolfram series' rules) is
//! pictured where its rows are drawn ([`rows_origin`]): a fit shows it for a second or so, too
//! short for its rows to reach the middle of the screen (#346).

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
    let caption = CaptionSettings {
        show: false,
        ..CaptionSettings::default()
    };
    painter.paint(device, queue, &tex, size, scale, stage, &caption, 0.0);
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

/// Whether `world` is drawn row by row down the screen: a rows rule runs in it.
pub fn draws_rows(world: &jetcarp_core::World) -> bool {
    world.cas.iter().any(|c| c.enabled && c.rule.is_rows())
}

/// Where a picture `size` pixels of a frame drawn row by row starts: in the frame's middle
/// columns, at the rows holding the most pixels drawn (any colour but the `backdrop`), the topmost
/// of equals; the middle when nothing there is drawn. The rows such a scene has drawn so far, from
/// its first, rather than the middle of the screen, which they may not have reached.
pub fn rows_origin(frame: &Image, size: [u32; 2], backdrop: [u8; 3]) -> [u32; 2] {
    let canvas = [frame.width, frame.height];
    let mid = middle(canvas, size);
    let (x0, x1) = (mid[0], (mid[0] + size[0]).min(frame.width));
    let h = size[1].min(frame.height) as usize;
    // Drawn pixels in each row of the middle columns.
    let drawn: Vec<usize> = (0..frame.height as usize)
        .map(|y| {
            (x0..x1)
                .filter(|&x| {
                    let i = (y * frame.width as usize + x as usize) * 4;
                    frame.rgba[i..i + 3] != backdrop
                })
                .count()
        })
        .collect();
    let mut sum: usize = drawn[..h].iter().sum();
    let (mut best, mut at) = (sum, 0);
    for y in 1..=drawn.len() - h {
        sum = sum + drawn[y + h - 1] - drawn[y - 1];
        if sum > best {
            (best, at) = (sum, y);
        }
    }
    if best == 0 {
        return mid;
    }
    [mid[0], at as u32]
}

/// `size` pixels of `img` from `origin`.
pub fn part(img: &Image, origin: [u32; 2], size: [u32; 2]) -> Image {
    let w = size[0].min(img.width.saturating_sub(origin[0]));
    let h = size[1].min(img.height.saturating_sub(origin[1]));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in origin[1]..origin[1] + h {
        let start = ((y * img.width + origin[0]) * 4) as usize;
        rgba.extend_from_slice(&img.rgba[start..start + (w * 4) as usize]);
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// A card's picture from a frame drawn on `tex` (`canvas` pixels, made with `COPY_SRC`) of
/// `stage`: its middle `size` pixels, or where its rows are drawn ([`rows_origin`]).
pub fn from_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    stage: &Stage,
    canvas: [u32; 2],
    size: [u32; 2],
) -> Option<Image> {
    if !stage.world().is_some_and(draws_rows) {
        return read(device, queue, tex, middle(canvas, size), size);
    }
    let frame = read(device, queue, tex, [0, 0], canvas)?;
    let origin = rows_origin(&frame, size, stage.backdrop());
    Some(part(&frame, origin, size))
}

/// A card's picture of what `stage` shows on a screen of `canvas` pixels: its middle `size`
/// pixels ([`render`]), or where its rows are drawn, from a frame of the whole screen
/// ([`rows_origin`]).
pub fn picture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut Painter,
    stage: &Stage,
    canvas: [u32; 2],
    size: [u32; 2],
    scale: f32,
) -> Option<Image> {
    if !stage.world().is_some_and(draws_rows) {
        return render(device, queue, painter, stage, size, scale);
    }
    let frame = render(device, queue, painter, stage, canvas, scale)?;
    let origin = rows_origin(&frame, size, stage.backdrop());
    Some(part(&frame, origin, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows drawn from the top of a frame, a few of them: the picture is taken from the rows
    /// drawn, not from the middle, where nothing is yet; and the middle where nothing is drawn.
    #[test]
    fn a_frame_drawn_row_by_row_is_pictured_where_its_rows_are() {
        let (w, h) = (64u32, 48u32);
        let back = [10, 20, 30];
        let mut f = Image {
            width: w,
            height: h,
            rgba: [back[0], back[1], back[2], 255].repeat((w * h) as usize),
        };
        assert_eq!(rows_origin(&f, [16, 8], back), middle([w, h], [16, 8]));
        // Rows 2 to 13 drawn, every other one, and a dot far down at the left, outside the
        // middle columns.
        for y in (2..14).step_by(2) {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                f.rgba[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let i = ((40 * w + 1) * 4) as usize;
        f.rgba[i..i + 3].copy_from_slice(&[255, 0, 0]);
        // Eight rows from row 1 hold four drawn ones, as many as any eight: the topmost such.
        let at = rows_origin(&f, [16, 8], back);
        assert_eq!(at, [24, 1], "the first rows drawn");
        let p = part(&f, at, [16, 8]);
        assert_eq!((p.width, p.height), (16, 8));
        assert_eq!(
            &p.rgba[16 * 4..16 * 4 + 3],
            &[255, 255, 255],
            "its second row is row 2"
        );
        // A picture as tall as the frame starts at its top.
        assert_eq!(rows_origin(&f, [16, 48], back), [24, 0]);
    }
}
