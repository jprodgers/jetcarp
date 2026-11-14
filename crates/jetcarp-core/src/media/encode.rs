//! Encoders for media files, all pure Rust: indexed PNG (the `png` crate), animated GIF (the
//! `gif` crate), and animated WebP: lossless VP8L frames (JETCARP's palette encoder in `vp8l` for
//! up to 256 colours, `image-webp` beyond) inside an animation container written here
//! (`RIFF`/`WEBP` with `VP8X`, `ANIM`, and one `ANMF` per frame).
//!
//! Every encoder is a pure function of its frames, so the bytes are the same on any machine
//! and thread count. Both animation writers store only the rectangle that changed since the
//! previous frame (WebP rectangles start on even pixels, as the format requires), drawn over
//! the previous frame without blending, and loop forever.

use crate::render::Image;
use std::borrow::Cow;

/// The colours of the frames in first-seen order (frame by frame, row by row), if there are at
/// most 256 of them.
pub fn palette(frames: &[&Image]) -> Option<Vec<[u8; 4]>> {
    let mut pal: Vec<[u8; 4]> = Vec::new();
    let mut last = None;
    for f in frames {
        for px in f.rgba.as_chunks::<4>().0 {
            if last == Some(*px) {
                continue;
            }
            last = Some(*px);
            if !pal.contains(px) {
                if pal.len() == 256 {
                    return None;
                }
                pal.push(*px);
            }
        }
    }
    Some(pal)
}

/// Palette indices of one frame. Colours not in the palette take the nearest entry.
fn indices(img: &Image, pal: &[[u8; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width * img.height) as usize);
    let (mut last, mut last_i) = (None, 0u8);
    for px in img.rgba.as_chunks::<4>().0 {
        if last != Some(*px) {
            last = Some(*px);
            last_i = match pal.iter().position(|c| c == px) {
                Some(i) => i as u8,
                None => nearest(pal, px),
            };
        }
        out.push(last_i);
    }
    out
}

fn nearest(pal: &[[u8; 4]], px: &[u8; 4]) -> u8 {
    let d = |c: &[u8; 4]| {
        (0..3)
            .map(|k| (c[k] as i32 - px[k] as i32).pow(2))
            .sum::<i32>()
    };
    (0..pal.len()).min_by_key(|&i| d(&pal[i])).unwrap_or(0) as u8
}

/// Up to 256 colours for frames with more: the most frequent ones (ties by first appearance).
fn reduced_palette(frames: &[&Image]) -> Vec<[u8; 4]> {
    let mut counts: Vec<([u8; 4], u64, usize)> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for f in frames {
        for px in f.rgba.as_chunks::<4>().0 {
            let n = counts.len();
            let k = *index.entry(*px).or_insert(n);
            if k == n {
                counts.push((*px, 0, n));
            }
            counts[k].1 += 1;
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    counts.into_iter().take(256).map(|c| c.0).collect()
}

/// The smallest rectangle `[x0, y0, x1, y1)` where `b` differs from `a` (same size), or `None`.
fn changed(a: &Image, b: &Image) -> Option<[u32; 4]> {
    let w = a.width as usize;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..a.height as usize {
        let ra = &a.rgba[y * w * 4..(y + 1) * w * 4];
        let rb = &b.rgba[y * w * 4..(y + 1) * w * 4];
        if ra == rb {
            continue;
        }
        let first = (0..w).find(|&x| ra[x * 4..x * 4 + 4] != rb[x * 4..x * 4 + 4]);
        let last = (0..w)
            .rev()
            .find(|&x| ra[x * 4..x * 4 + 4] != rb[x * 4..x * 4 + 4]);
        if let (Some(f), Some(l)) = (first, last) {
            x0 = x0.min(f);
            x1 = x1.max(l + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    (x0 != usize::MAX).then_some([x0 as u32, y0 as u32, x1 as u32, y1 as u32])
}

fn crop(img: &Image, r: [u32; 4]) -> Image {
    let (w, h) = (r[2] - r[0], r[3] - r[1]);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in r[1]..r[3] {
        let o = ((y * img.width + r[0]) * 4) as usize;
        rgba.extend_from_slice(&img.rgba[o..o + (w * 4) as usize]);
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// The rectangle each frame stores: the whole first frame, then what changed (one pixel when
/// nothing did). `even` rounds the corner down to even coordinates (WebP).
fn rects(frames: &[&Image], even: bool) -> Vec<[u32; 4]> {
    let mut out = Vec::with_capacity(frames.len());
    for (k, f) in frames.iter().enumerate() {
        if k == 0 {
            out.push([0, 0, f.width, f.height]);
            continue;
        }
        let mut r = changed(frames[k - 1], f).unwrap_or([0, 0, 1, 1]);
        if even {
            r[0] &= !1;
            r[1] &= !1;
        }
        out.push(r);
    }
    out
}

/// Check that frames are non-empty and all one size.
fn same_size(frames: &[&Image]) -> Result<(u32, u32), String> {
    let f = frames.first().ok_or("no frames to encode")?;
    if frames
        .iter()
        .any(|g| g.width != f.width || g.height != f.height)
    {
        return Err("frames differ in size".into());
    }
    if f.width == 0 || f.height == 0 {
        return Err("empty frame".into());
    }
    Ok((f.width, f.height))
}

/// An animated GIF that loops forever, `delay_ms` per frame (GIF stores hundredths of a
/// second; at least 2). One global palette: the frames' own colours when there are at most
/// 256, else the 256 most frequent with every other colour mapped to the nearest.
pub fn gif(frames: &[&Image], delay_ms: u32) -> Result<Vec<u8>, String> {
    let (w, h) = same_size(frames)?;
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(format!("{w} x {h} is too large for a GIF"));
    }
    let pal = palette(frames).unwrap_or_else(|| reduced_palette(frames));
    let mut flat: Vec<u8> = pal.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
    // GIF palettes have a power-of-two size; pad with black.
    let size = pal.len().next_power_of_two().max(2);
    flat.resize(size * 3, 0);
    let mut out = Vec::new();
    {
        let mut enc =
            gif::Encoder::new(&mut out, w as u16, h as u16, &flat).map_err(|e| e.to_string())?;
        enc.set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        let delay = (delay_ms.div_ceil(10)).clamp(2, u16::MAX as u32) as u16;
        for (f, r) in frames.iter().zip(rects(frames, false)) {
            let part = crop(f, r);
            let frame = gif::Frame {
                delay,
                dispose: gif::DisposalMethod::Keep,
                left: r[0] as u16,
                top: r[1] as u16,
                width: part.width as u16,
                height: part.height as u16,
                buffer: Cow::Owned(indices(&part, &pal)),
                ..gif::Frame::default()
            };
            enc.write_frame(&frame).map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

/// A lossless VP8L bitstream for one opaque frame (the payload of a `VP8L` chunk): JETCARP's
/// palette encoder for up to 256 colours, else `image-webp`'s general one.
fn vp8l(img: &Image) -> Result<Vec<u8>, String> {
    if let Some(b) = super::vp8l::encode(img) {
        return Ok(b);
    }
    let rgb: Vec<u8> = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut file = Vec::new();
    image_webp::WebPEncoder::new(&mut file)
        .encode(&rgb, img.width, img.height, image_webp::ColorType::Rgb8)
        .map_err(|e| format!("WebP: {e}"))?;
    // The simple container: RIFF size WEBP, then one VP8L chunk.
    if file.len() < 20 || &file[0..4] != b"RIFF" || &file[12..16] != b"VP8L" {
        return Err("WebP: unexpected encoder output".into());
    }
    let n = u32::from_le_bytes(file[16..20].try_into().unwrap()) as usize;
    Ok(file[20..20 + n].to_vec())
}

fn chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(name);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

fn u24(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes()[..3]);
}

/// An animated, lossless WebP that loops forever, `delay_ms` per frame. Opaque frames up to
/// 16,384 pixels on a side; `background` is the canvas colour players may show before the
/// first frame.
pub fn webp(frames: &[&Image], delay_ms: u32, background: [u8; 4]) -> Result<Vec<u8>, String> {
    let (w, h) = same_size(frames)?;
    if w > 16384 || h > 16384 {
        return Err(format!("{w} x {h} is too large for a WebP"));
    }
    let mut body = Vec::new();
    // VP8X: flags (animation), 3 reserved bytes, canvas width - 1 and height - 1 (24 bits each).
    let mut vp8x = vec![0x02, 0, 0, 0];
    u24(&mut vp8x, w - 1);
    u24(&mut vp8x, h - 1);
    chunk(&mut body, b"VP8X", &vp8x);
    // ANIM: background colour (B, G, R, A) and loop count (0 = forever).
    let bg = [background[2], background[1], background[0], background[3]];
    let mut anim = bg.to_vec();
    anim.extend_from_slice(&0u16.to_le_bytes());
    chunk(&mut body, b"ANIM", &anim);
    let delay = delay_ms.min((1 << 24) - 1);
    for (f, r) in frames.iter().zip(rects(frames, true)) {
        let part = crop(f, r);
        let mut anmf = Vec::new();
        u24(&mut anmf, r[0] / 2);
        u24(&mut anmf, r[1] / 2);
        u24(&mut anmf, part.width - 1);
        u24(&mut anmf, part.height - 1);
        u24(&mut anmf, delay);
        // Flags: do not blend (bit 1), keep the canvas (disposal bit 0 clear).
        anmf.push(0x02);
        chunk(&mut anmf, b"VP8L", &vp8l(&part)?);
        chunk(&mut body, b"ANMF", &anmf);
    }
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Ok(out)
}

/// A PNG: indexed when the image has at most 256 colours, else RGBA.
pub fn png(img: &Image) -> Result<Vec<u8>, String> {
    png_with_text(img, &[])
}

/// The same, with text chunks (such as [`crate::render::png_metadata`]'s scene JSON).
pub fn png_with_text(img: &Image, text: &[(&str, String)]) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut enc = png::Encoder::new(&mut buf, img.width, img.height);
    for (k, v) in text {
        enc.add_itxt_chunk(k.to_string(), v.clone())
            .map_err(|e| e.to_string())?;
    }
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Balanced);
    enc.set_filter(png::Filter::Adaptive);
    match palette(&[img]) {
        Some(pal) => {
            enc.set_color(png::ColorType::Indexed);
            enc.set_palette(
                pal.iter()
                    .flat_map(|c| [c[0], c[1], c[2]])
                    .collect::<Vec<_>>(),
            );
            let idx = indices(img, &pal);
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(&idx).map_err(|e| e.to_string())?;
            wr.finish().map_err(|e| e.to_string())?;
        }
        None => {
            enc.set_color(png::ColorType::Rgba);
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
            wr.finish().map_err(|e| e.to_string())?;
        }
    }
    Ok(buf)
}

/// Decode a PNG written by [`png`] (or any 8-bit PNG) to RGBA.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().ok_or("PNG too large")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        c => return Err(format!("unexpected PNG colour type {c:?}")),
    };
    Ok(Image {
        width: w,
        height: h,
        rgba,
    })
}

/// Decode every frame of a GIF to full RGBA canvases (for tests and checks).
pub fn decode_gif(bytes: &[u8]) -> Result<Vec<Image>, String> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut d = opts
        .read_info(std::io::Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let (w, h) = (d.width() as u32, d.height() as u32);
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let mut out = Vec::new();
    while let Some(f) = d.read_next_frame().map_err(|e| e.to_string())? {
        for y in 0..f.height as u32 {
            for x in 0..f.width as u32 {
                let s = ((y * f.width as u32 + x) * 4) as usize;
                let px = &f.buffer[s..s + 4];
                if px[3] == 0 {
                    continue;
                }
                let o = (((y + f.top as u32) * w + x + f.left as u32) * 4) as usize;
                canvas[o..o + 4].copy_from_slice(px);
            }
        }
        out.push(Image {
            width: w,
            height: h,
            rgba: canvas.clone(),
        });
    }
    Ok(out)
}

/// Decode every frame of an animated WebP with `image-webp` (pure Rust) to RGBA.
pub fn decode_webp(bytes: &[u8]) -> Result<Vec<Image>, String> {
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("WebP: {e}"))?;
    let (w, h) = d.dimensions();
    let bpp = if d.has_alpha() { 4 } else { 3 };
    let n = if d.is_animated() { d.num_frames() } else { 1 };
    let mut out = Vec::new();
    for _ in 0..n {
        let mut buf = vec![0u8; (w * h) as usize * bpp];
        if d.is_animated() {
            d.read_frame(&mut buf).map_err(|e| format!("WebP: {e}"))?;
        } else {
            d.read_image(&mut buf).map_err(|e| format!("WebP: {e}"))?;
        }
        let rgba = if bpp == 4 {
            buf
        } else {
            buf.as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        out.push(Image {
            width: w,
            height: h,
            rgba,
        });
    }
    Ok(out)
}

/// Shrink an image to fit in `side` x `side` (never enlarges) by averaging the source pixels
/// each target pixel covers (integer arithmetic, so the result is the same everywhere).
pub fn shrink(img: &Image, side: u32) -> Image {
    let (w, h) = (img.width, img.height);
    if w <= side && h <= side {
        return img.clone();
    }
    let k = (w.max(h) as f64 / side as f64).max(1.0);
    let nw = ((w as f64 / k).round() as u32).clamp(1, side);
    let nh = ((h as f64 / k).round() as u32).clamp(1, side);
    let mut rgba = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        let (y0, y1) = (
            (y as u64 * h as u64 / nh as u64) as u32,
            (((y + 1) as u64 * h as u64).div_ceil(nh as u64) as u32).min(h),
        );
        for x in 0..nw {
            let (x0, x1) = (
                (x as u64 * w as u64 / nw as u64) as u32,
                (((x + 1) as u64 * w as u64).div_ceil(nw as u64) as u32).min(w),
            );
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let o = ((sy * w + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += img.rgba[o + c] as u64;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)).max(1) as u64;
            rgba.extend(sum.iter().map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Image {
        width: nw,
        height: nh,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend(f(x, y));
            }
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn gif_and_webp_round_trip() {
        let frames: Vec<Image> = (0..5)
            .map(|t| {
                img(37, 21, |x, y| {
                    if (x + y + t) % 7 == 0 {
                        [255, 200, 100, 255]
                    } else {
                        [20, 30, 40, 255]
                    }
                })
            })
            .collect();
        let refs: Vec<&Image> = frames.iter().collect();
        let g = decode_gif(&gif(&refs, 100).unwrap()).unwrap();
        let w = decode_webp(&webp(&refs, 100, [0, 0, 0, 255]).unwrap()).unwrap();
        assert_eq!(g.len(), 5);
        assert_eq!(w.len(), 5);
        for k in 0..5 {
            assert_eq!(g[k].rgba, frames[k].rgba, "gif frame {k}");
            assert_eq!(w[k].rgba, frames[k].rgba, "webp frame {k}");
        }
        let p = decode_png(&png(&frames[2]).unwrap()).unwrap();
        assert_eq!(p.rgba, frames[2].rgba);
    }

    #[test]
    fn shrink_averages() {
        let a = img(4, 2, |x, _| if x < 2 { [0; 4] } else { [200; 4] });
        let s = shrink(&a, 2);
        assert_eq!((s.width, s.height), (2, 1));
        assert_eq!(&s.rgba[..4], &[0; 4]);
        assert_eq!(&s.rgba[4..], &[200; 4]);
    }
}
