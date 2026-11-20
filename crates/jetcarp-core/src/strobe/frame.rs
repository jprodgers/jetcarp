//! A drawn frame reduced to what the strobe guard measures: a map of tiles ([`TileMap`]) with
//! each tile's mean relative luminance and mean red measure, for the flash checks, and a
//! half-resolution luminance image ([`LumImage`]) for the stripe check.
//!
//! This is the processor's reduction, from RGBA bytes (sRGB, as JETCARP draws them). The GPU's
//! (`jetcarp_display::strobe_gpu`) gives the same numbers from a frame it drew; both sum in linear
//! light, since a mean of sRGB values is not the mean of the light. Each tile's sums run in pixel
//! order, so the result does not depend on the thread count.

use super::{TILE, linear, luminance_linear, red_measure_linear};
use rayon::prelude::*;

/// Each tile's means, for one frame. Tiles are [`TILE`] pixels square, row by row from the top
/// left; those at the right and bottom edges may be smaller.
#[derive(Clone, Debug, PartialEq)]
pub struct TileMap {
    /// The frame's size in pixels.
    pub width: u32,
    pub height: u32,
    /// Tiles across and down.
    pub cols: u32,
    pub rows: u32,
    /// Mean relative luminance of each tile (0 to 1).
    pub lum: Vec<f32>,
    /// Mean red measure of each tile (0 to 320).
    pub red: Vec<f32>,
}

/// Tiles across and down a frame of this size.
pub fn tiles_for(width: u32, height: u32) -> (u32, u32) {
    (width.div_ceil(TILE).max(1), height.div_ceil(TILE).max(1))
}

impl TileMap {
    /// A map of a frame of this size, every tile black.
    pub fn new(width: u32, height: u32) -> TileMap {
        let (cols, rows) = tiles_for(width, height);
        let n = (cols * rows) as usize;
        TileMap {
            width,
            height,
            cols,
            rows,
            lum: vec![0.0; n],
            red: vec![0.0; n],
        }
    }

    /// The same map as a frame of one colour.
    pub fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> TileMap {
        let mut m = TileMap::new(width, height);
        let (l, r) = (super::luminance(rgb), super::red_measure(rgb));
        m.lum.fill(l);
        m.red.fill(r);
        m
    }

    pub fn len(&self) -> usize {
        self.lum.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lum.is_empty()
    }

    /// The pixels tile `i` covers (fewer at the right and bottom edges).
    pub fn tile_px(&self, i: usize) -> f32 {
        let (c, r) = (i as u32 % self.cols, i as u32 / self.cols);
        let w = (self.width.saturating_sub(c * TILE)).min(TILE);
        let h = (self.height.saturating_sub(r * TILE)).min(TILE);
        (w * h) as f32
    }

    /// The frame's mean luminance and mean red measure (each tile weighted by its pixels).
    pub fn mean(&self) -> (f64, f64) {
        let (mut l, mut r, mut a) = (0f64, 0f64, 0f64);
        for i in 0..self.len() {
            let px = self.tile_px(i) as f64;
            l += self.lum[i] as f64 * px;
            r += self.red[i] as f64 * px;
            a += px;
        }
        if a > 0.0 { (l / a, r / a) } else { (0.0, 0.0) }
    }
}

/// A luminance image at half the frame's resolution: each value is the mean relative luminance of
/// a 2 by 2 block of pixels (fewer at an odd edge). The stripe check reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LumImage {
    pub width: u32,
    pub height: u32,
    pub lum: Vec<f32>,
}

impl LumImage {
    /// The value at `(x, y)`.
    pub fn at(&self, x: u32, y: u32) -> f32 {
        self.lum[(y * self.width + x) as usize]
    }
}

/// One pixel's luminance and red measure.
#[inline]
fn pixel(px: &[u8]) -> (f32, f32) {
    let (r, g, b) = (linear(px[0]), linear(px[1]), linear(px[2]));
    (luminance_linear(r, g, b), red_measure_linear(r, g, b))
}

/// The tile map of an RGBA frame (`width * height * 4` bytes, sRGB; alpha is ignored).
pub fn reduce(rgba: &[u8], width: u32, height: u32) -> TileMap {
    reduce_with(rgba, width, height, false).0
}

/// The tile map and the half-resolution luminance image, in one pass over the frame.
pub fn reduce_both(rgba: &[u8], width: u32, height: u32) -> (TileMap, LumImage) {
    let (m, h) = reduce_with(rgba, width, height, true);
    (m, h.expect("asked for"))
}

fn reduce_with(rgba: &[u8], width: u32, height: u32, half: bool) -> (TileMap, Option<LumImage>) {
    assert_eq!(
        rgba.len(),
        width as usize * height as usize * 4,
        "an RGBA frame of {width} x {height}"
    );
    let mut map = TileMap::new(width, height);
    let (cols, rows) = (map.cols as usize, map.rows as usize);
    let (hw, hh) = (width.div_ceil(2).max(1), height.div_ceil(2).max(1));
    let mut himg = half.then(|| LumImage {
        width: hw,
        height: hh,
        lum: vec![0.0; (hw * hh) as usize],
    });
    if width == 0 || height == 0 {
        return (map, himg);
    }
    let w = width as usize;
    // One band of TILE pixel rows at a time: its tiles, and its half of the half image.
    let band_rows = TILE as usize;
    let half_band = band_rows / 2;
    let mut lum_rows: Vec<&mut [f32]> = map.lum.chunks_mut(cols).collect();
    let mut red_rows: Vec<&mut [f32]> = map.red.chunks_mut(cols).collect();
    let mut half_rows: Vec<Option<&mut [f32]>> = match &mut himg {
        Some(h) => h
            .lum
            .chunks_mut(hw as usize * half_band)
            .map(Some)
            .collect(),
        None => (0..rows).map(|_| None).collect(),
    };
    half_rows.resize_with(rows, || None);
    lum_rows
        .par_iter_mut()
        .zip(red_rows.par_iter_mut())
        .zip(half_rows.par_iter_mut())
        .enumerate()
        .for_each(|(tr, ((lrow, rrow), hrow))| {
            let y0 = tr * band_rows;
            let y1 = (y0 + band_rows).min(height as usize);
            let mut lsum = vec![0f32; cols];
            let mut rsum = vec![0f32; cols];
            // Half image: sums of the band's 2x2 blocks.
            let mut hsum = vec![
                0f32;
                if hrow.is_some() {
                    hw as usize * half_band
                } else {
                    0
                }
            ];
            let mut hcnt = vec![0u8; hsum.len()];
            for y in y0..y1 {
                let row = &rgba[y * w * 4..(y + 1) * w * 4];
                for (x, px) in row.chunks_exact(4).enumerate() {
                    let (l, r) = pixel(px);
                    lsum[x / TILE as usize] += l;
                    rsum[x / TILE as usize] += r;
                    if !hsum.is_empty() {
                        let k = ((y - y0) / 2) * hw as usize + x / 2;
                        hsum[k] += l;
                        hcnt[k] += 1;
                    }
                }
            }
            for c in 0..cols {
                let tw = (w - c * TILE as usize).min(TILE as usize);
                let n = (tw * (y1 - y0)) as f32;
                lrow[c] = lsum[c] / n;
                rrow[c] = rsum[c] / n;
            }
            if let Some(h) = hrow {
                for (k, v) in h.iter_mut().enumerate() {
                    if hcnt[k] > 0 {
                        *v = hsum[k] / hcnt[k] as f32;
                    }
                }
            }
        });
    (map, himg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strobe::{luminance, red_measure};

    fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }

    #[test]
    fn a_uniform_frame_gives_every_tile_its_colour() {
        let (w, h) = (100, 37);
        let m = reduce(&frame(w, h, |_, _| [200, 30, 40]), w, h);
        assert_eq!((m.cols, m.rows), (7, 3));
        for i in 0..m.len() {
            assert!((m.lum[i] - luminance([200, 30, 40])).abs() < 1e-6);
            assert!((m.red[i] - red_measure([200, 30, 40])).abs() < 1e-3);
        }
        assert_eq!(m, {
            let mut u = TileMap::uniform(w, h, [200, 30, 40]);
            u.lum.iter_mut().zip(&m.lum).for_each(|(a, b)| *a = *b);
            u.red.iter_mut().zip(&m.red).for_each(|(a, b)| *a = *b);
            u
        });
        // Edge tiles are partial: 100 = 6 * 16 + 4, 37 = 2 * 16 + 5.
        assert_eq!(m.tile_px(0), 256.0);
        assert_eq!(m.tile_px(6), 4.0 * 16.0);
        assert_eq!(m.tile_px(20), 4.0 * 5.0);
    }

    #[test]
    fn tiles_average_in_linear_light() {
        // Half black, half white columns in every tile: the mean light is a half, though the
        // mean sRGB value would decode to about a fifth.
        let m = reduce(
            &frame(32, 32, |x, _| [if x % 2 == 0 { 0 } else { 255 }; 3]),
            32,
            32,
        );
        for l in &m.lum {
            assert!((l - 0.5).abs() < 1e-6, "{l}");
        }
        // The frame's mean weights partial tiles by their pixels.
        let m = reduce(
            &frame(20, 16, |x, _| [if x < 16 { 0 } else { 255 }; 3]),
            20,
            16,
        );
        assert_eq!(m.lum, vec![0.0, 1.0]);
        assert!((m.mean().0 - 4.0 / 20.0).abs() < 1e-9);
    }

    #[test]
    fn the_half_image_is_two_by_two_means() {
        let (w, h) = (5, 3);
        let f = frame(w, h, |x, y| [((x + y * w) * 10) as u8; 3]);
        let (m, hi) = reduce_both(&f, w, h);
        assert_eq!(m, reduce(&f, w, h));
        assert_eq!((hi.width, hi.height), (3, 2));
        let l = |x: u32, y: u32| luminance([((x + y * w) * 10) as u8; 3]);
        assert!((hi.at(0, 0) - (l(0, 0) + l(1, 0) + l(0, 1) + l(1, 1)) / 4.0).abs() < 1e-6);
        // The odd column and row average what they have.
        assert!((hi.at(2, 0) - (l(4, 0) + l(4, 1)) / 2.0).abs() < 1e-6);
        assert!((hi.at(2, 1) - l(4, 2)).abs() < 1e-6);
        assert!((hi.at(1, 1) - (l(2, 2) + l(3, 2)) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_reduction_does_not_depend_on_the_thread_count() {
        let (w, h) = (333, 257);
        let f = frame(w, h, |x, y| {
            [(x * 7 + y * 3) as u8, (x * y) as u8, (x ^ y) as u8]
        });
        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| reduce_both(&f, w, h));
        let many = rayon::ThreadPoolBuilder::new()
            .num_threads(7)
            .build()
            .unwrap()
            .install(|| reduce_both(&f, w, h));
        assert_eq!(one, many);
    }

    #[test]
    fn an_empty_frame_has_one_black_tile() {
        let m = reduce(&[], 0, 0);
        assert_eq!((m.cols, m.rows, m.len()), (1, 1, 1));
        assert_eq!(m.mean(), (0.0, 0.0));
    }
}
