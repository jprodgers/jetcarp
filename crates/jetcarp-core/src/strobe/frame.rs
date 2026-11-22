//! A drawn frame reduced to what the strobe guard measures: a map of tiles ([`TileMap`]) with
//! each tile's mean relative luminance and mean red measure, and its fine pattern ([`Fine`]), for
//! the flash checks; and a half-resolution luminance image ([`LumImage`]) for the stripe check.
//!
//! This is the processor's reduction, from RGBA bytes (sRGB, as JETCARP draws them). The GPU's
//! (`jetcarp_display::strobe_gpu`) gives the same numbers from a frame it drew; both sum in linear
//! light, since a mean of sRGB values is not the mean of the light. Each tile's sums run in pixel
//! order, so the result does not depend on the thread count.
//!
//! **The fine pattern** of a tile is what its mean cannot show: a checkerboard of squares smaller
//! than a tile keeps the tile's mean when it reverses, though every pixel of it changes. WCAG
//! 2.3.1 counts such a checkerboard as flashing unless its squares are smaller than 0.1° of the
//! visual field (under 3 pixels at the distance JETCARP assumes, [`super::Viewing`]). So each tile
//! also gives the means of its pixels above and below its mean (its light and dark parts, which a
//! reversal swaps), and the pattern of its *windows*: squares of [`FINE_PX`] pixels, one every
//! [`FINE_STEP`] pixels across and down inside it, each above or below the tile's mean, and whether
//! clearly so (by at least [`FINE_SIG`] of the light parts' difference from the dark ones). A window
//! of 4 pixels holds a whole number of periods of a checkerboard of 1- or 2-pixel squares, so such
//! a pattern leaves every window at the tile's mean and shows nothing, while squares of 3 pixels and
//! more leave windows clearly above and below it, at any offset. The flash meter
//! ([`super::flash`]) counts a tile's fine pattern turning into its own negative as a transition of
//! every pixel in it.

use super::{TILE, linear, luminance_linear, red_measure};
use rayon::prelude::*;

/// A fine pattern's windows are this many pixels on a side: a whole number of periods of a
/// checkerboard of squares of 1 or 2 pixels, both under 0.1° at the distance JETCARP assumes.
pub const FINE_PX: u32 = 4;

/// Windows start every this many pixels inside a tile, so a pattern is seen at any offset.
pub const FINE_STEP: u32 = 2;

/// Windows across (and down) a whole tile: 7, so 49 in all.
pub const FINE_SIDE: u32 = (TILE - FINE_PX) / FINE_STEP + 1;

/// A window is clearly above or below its tile's mean when it is at least this share of the light
/// parts' difference from the dark ones away from it.
pub const FINE_SIG: f32 = 0.1;

/// One tile's fine pattern, of luminance or of the red measure (see the module's notes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fine {
    /// The mean of the tile's pixels above its mean, and of those at or below it (each the
    /// tile's mean when there are none).
    pub light: f32,
    pub dark: f32,
    /// Each window clearly above or below the tile's mean (at least [`FINE_SIG`] of `light -
    /// dark` away from it), a bit each, row by row ([`FINE_SIDE`] a row).
    pub sig: u64,
    /// Each of those above it.
    pub sign: u64,
}

impl Fine {
    /// A tile of one value: no pattern.
    pub fn flat(v: f32) -> Fine {
        Fine {
            light: v,
            dark: v,
            sign: 0,
            sig: 0,
        }
    }

    /// The light parts' difference from the dark ones.
    pub fn contrast(&self) -> f32 {
        self.light - self.dark
    }
}

/// The fine pattern of a tile `tw` by `th` pixels whose mean is `mean`, from its pixels' values
/// `at(x, y)`. The GPU's pass works it out the same way: the light and dark parts as sums over the
/// tile, and each window as the sum of its four 2 by 2 blocks, each block summed in the order
/// (0, 0), (1, 0), (0, 1), (1, 1), so the two differ only in the order of the light and dark sums.
pub fn fine_of(tw: usize, th: usize, mean: f32, at: impl Fn(usize, usize) -> f32) -> Fine {
    let (mut ls, mut ln, mut ds, mut dn) = (0f32, 0f32, 0f32, 0f32);
    for y in 0..th {
        for x in 0..tw {
            let v = at(x, y);
            if v > mean {
                ls += v;
                ln += 1.0;
            } else {
                ds += v;
                dn += 1.0;
            }
        }
    }
    let light = if ln > 0.0 { ls / ln } else { mean };
    let dark = if dn > 0.0 { ds / dn } else { mean };
    let mut f = Fine {
        light,
        dark,
        sign: 0,
        sig: 0,
    };
    let threshold = FINE_SIG * (light - dark);
    for (k, dev) in window_devs(tw, th, mean, &at).into_iter().enumerate() {
        if let Some(dev) = dev.filter(|d| light > dark && d.abs() >= threshold) {
            f.sig |= 1 << k;
            if dev > 0.0 {
                f.sign |= 1 << k;
            }
        }
    }
    f
}

/// Each window's mean less the tile's `mean`, row by row, for a tile `tw` by `th` pixels whose
/// pixels' values are `at(x, y)`; None for a window the tile ends inside.
pub fn window_devs(
    tw: usize,
    th: usize,
    mean: f32,
    at: impl Fn(usize, usize) -> f32,
) -> [Option<f32>; (FINE_SIDE * FINE_SIDE) as usize] {
    const B: usize = (TILE / 2) as usize;
    // Each whole 2 by 2 block's sum (None where the tile ends inside it).
    let mut blk = [[None::<f32>; B]; B];
    for (by, row) in blk.iter_mut().enumerate() {
        for (bx, b) in row.iter_mut().enumerate() {
            let (x, y) = (bx * 2, by * 2);
            if x + 2 <= tw && y + 2 <= th {
                *b = Some(((at(x, y) + at(x + 1, y)) + at(x, y + 1)) + at(x + 1, y + 1));
            }
        }
    }
    let side = FINE_SIDE as usize;
    let mut out = [None; (FINE_SIDE * FINE_SIDE) as usize];
    for wy in 0..side {
        for wx in 0..side {
            if let (Some(a), Some(b), Some(c), Some(d)) = (
                blk[wy][wx],
                blk[wy][wx + 1],
                blk[wy + 1][wx],
                blk[wy + 1][wx + 1],
            ) {
                out[wy * side + wx] = Some(((a + b) + (c + d)) / 16.0 - mean);
            }
        }
    }
    out
}

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
    /// Each tile's fine pattern of luminance, and of the red measure.
    pub fine: Vec<Fine>,
    pub red_fine: Vec<Fine>,
    /// Each tile's mean colour in linear light (red, green, blue, each 0 to 1), for WCAG 2.2's
    /// red flashes, a change of chromaticity (#270; [`super::chromaticity`]).
    pub rgb: Vec<[f32; 3]>,
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
            fine: vec![Fine::flat(0.0); n],
            red_fine: vec![Fine::flat(0.0); n],
            rgb: vec![[0.0; 3]; n],
        }
    }

    /// The same map as a frame of one colour.
    pub fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> TileMap {
        let mut m = TileMap::new(width, height);
        let (l, r) = (super::luminance(rgb), super::red_measure(rgb));
        m.lum.fill(l);
        m.red.fill(r);
        m.fine.fill(Fine::flat(l));
        m.red_fine.fill(Fine::flat(r));
        m.rgb.fill(rgb.map(linear));
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

/// One pixel's luminance and red measure (worked out exactly, [`super::red_measure_fixed`]).
#[inline]
fn pixel(px: &[u8]) -> (f32, f32) {
    let (r, g, b) = (linear(px[0]), linear(px[1]), linear(px[2]));
    (
        luminance_linear(r, g, b),
        red_measure([px[0], px[1], px[2]]),
    )
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
    let mut fine_rows: Vec<&mut [Fine]> = map.fine.chunks_mut(cols).collect();
    let mut red_fine_rows: Vec<&mut [Fine]> = map.red_fine.chunks_mut(cols).collect();
    let mut rgb_rows: Vec<&mut [[f32; 3]]> = map.rgb.chunks_mut(cols).collect();
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
        .zip(fine_rows.par_iter_mut())
        .zip(red_fine_rows.par_iter_mut())
        .zip(rgb_rows.par_iter_mut())
        .zip(half_rows.par_iter_mut())
        .enumerate()
        .for_each(|(tr, (((((lrow, rrow), frow), rfrow), crow), hrow))| {
            let y0 = tr * band_rows;
            let y1 = (y0 + band_rows).min(height as usize);
            let mut lsum = vec![0f32; cols];
            let mut rsum = vec![0f32; cols];
            let mut csum = vec![[0f32; 3]; cols];
            // The band's pixels' values, for the fine patterns.
            let mut vals = vec![(0f32, 0f32); w * (y1 - y0)];
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
                    vals[(y - y0) * w + x] = (l, r);
                    lsum[x / TILE as usize] += l;
                    rsum[x / TILE as usize] += r;
                    let c = &mut csum[x / TILE as usize];
                    for k in 0..3 {
                        c[k] += linear(px[k]);
                    }
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
                crow[c] = csum[c].map(|s| s / n);
                let x0 = c * TILE as usize;
                let at = |x: usize, y: usize| vals[y * w + x0 + x];
                frow[c] = fine_of(tw, y1 - y0, lrow[c], |x, y| at(x, y).0);
                rfrow[c] = fine_of(tw, y1 - y0, rrow[c], |x, y| at(x, y).1);
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
            for (a, b) in u.rgb.iter().zip(&m.rgb) {
                for k in 0..3 {
                    assert!((a[k] - b[k]).abs() < 1e-6, "{a:?} {b:?}");
                }
            }
            u.rgb.clone_from(&m.rgb);
            // No pattern: light and dark at the mean, no window clear of it.
            u.fine = u.lum.iter().map(|l| Fine::flat(*l)).collect();
            u.red_fine = u.red.iter().map(|r| Fine::flat(*r)).collect();
            u
        });
        // Edge tiles are partial: 100 = 6 * 16 + 4, 37 = 2 * 16 + 5.
        assert_eq!(m.tile_px(0), 256.0);
        assert_eq!(m.tile_px(6), 4.0 * 16.0);
        assert_eq!(m.tile_px(20), 4.0 * 5.0);
    }

    /// A checkerboard's fine pattern: squares of 1 and 2 pixels leave every window at the tile's
    /// mean, at any offset; squares of 3 pixels and more leave windows clear of it at any offset,
    /// and reversed, every one of those changes sides. The light and dark parts are its squares.
    #[test]
    fn a_tile_s_fine_pattern_shows_squares_from_three_pixels() {
        let board = |sq: u32, off: [u32; 2], phase: u32| {
            frame(48, 48, move |x, y| {
                let on = ((x + off[0]) / sq + (y + off[1]) / sq + phase) % 2 == 1;
                [if on { 255 } else { 0 }; 3]
            })
        };
        for sq in [1u32, 2] {
            for off in [[0, 0], [1, 0], [0, 1], [1, 1]] {
                let m = reduce(&board(sq, off, 0), 48, 48);
                assert!(m.fine.iter().all(|f| f.sig == 0), "{sq} at {off:?}");
            }
        }
        for sq in [3u32, 4, 5, 6, 8, 12, 16, 24] {
            for ox in 0..sq.min(16) {
                let off = [ox, (ox * 3 + 1) % sq];
                let (a, b) = (
                    reduce(&board(sq, off, 0), 48, 48),
                    reduce(&board(sq, off, 1), 48, 48),
                );
                for t in 0..a.len() {
                    let (fa, fb) = (a.fine[t], b.fine[t]);
                    let both = fa.sig & fb.sig;
                    let flipped = ((fa.sign ^ fb.sign) & both).count_ones();
                    let what = format!("{sq} at {off:?}, tile {t}: {fa:?} {fb:?}");
                    if (a.lum[t] - b.lum[t]).abs() >= 0.1 {
                        // Inside a square, or mostly: the tile's mean shows it.
                        continue;
                    }
                    assert!(both.count_ones() >= 8, "{what}");
                    assert_eq!(flipped, both.count_ones(), "{what}");
                    assert!(fa.light > 0.99 && fa.dark < 0.01, "{what}");
                }
            }
        }
        // Red: a red and black board's red measure has its pattern too.
        let red = |phase: u32| {
            frame(16, 16, move |x, y| {
                if (x / 4 + y / 4 + phase) % 2 == 1 {
                    [255, 0, 0]
                } else {
                    [0; 3]
                }
            })
        };
        let (a, b) = (reduce(&red(0), 16, 16), reduce(&red(1), 16, 16));
        assert!((a.red_fine[0].contrast() - 320.0).abs() < 0.01);
        assert_eq!(a.red_fine[0].sig, b.red_fine[0].sig);
        assert_eq!(a.red_fine[0].sign ^ b.red_fine[0].sign, a.red_fine[0].sig);
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
