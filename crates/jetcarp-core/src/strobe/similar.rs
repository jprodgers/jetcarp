//! The stricter stripe rule for someone who answered the seizure warning *at risk* (whose guard
//! can never be turned off): IRIS's pattern check, which counts **similar parts**, regular or not.
//!
//! A frame shows a harmful pattern by this rule when at least [`MIN_PARTS`] parts of it that are
//! alike in shape cover, together, at least [`AREA_SHARE`] of the frame, its light parts are at
//! least [`LIGHT_MIN`] in luminance, and they differ from its dark parts by at least
//! [`MIN_CONTRAST`]. Held for half a second it fails, as stripes do
//! ([`super::pattern::FAIL_SECONDS`]). Everyone else keeps the regular-stripes rule
//! ([`super::pattern`]), so an ordinary soup of cells is not dimmed for them; for someone at risk
//! it is, as IRIS would fail it.
//!
//! **How it is measured**, on the half-resolution luminance image ([`LumImage`]), halved again
//! until it is no wider than 480 samples (IRIS halves frames wider than 480 pixels; a part is at
//! least a share of the image, so it stays well resolved, and a check of a 4K frame costs what a
//! check of a 1080p frame does, a millisecond or two): the image is cut
//! into light and dark at Otsu's threshold, as IRIS cuts it; the light parts' mean luminance is
//! the light level. Each connected light or dark part (touching along an edge or a corner) of at
//! least [`MIN_PART_SHARE`] of the image, and at most a sixth of it, is a part, taken as its convex
//! hull, as IRIS fills its parts convex before it compares them. Two parts are alike when their
//! hulls' Hu moments differ by less than [`ALIKE`] by OpenCV's `CONTOURS_MATCH_I1`, IRIS's measure
//! and threshold. The largest set of parts alike to one of them, with the rectangle
//! around them, is the pattern. IRIS finds its parts in what is left of the frame after taking
//! out its strongest repeats; this takes every part, so it finds at least as much, but for parts
//! barely lighter than the rest: IRIS stretches every frame to full contrast, then takes out the
//! frame's strongest repeats, so a faint even grid (cell outlines drawn faint over a bright
//! field, which no longer make stripes) leaves it nothing, and the contrast floor leaves it out
//! here.
//!
//! **Where it is veiled**: a harmful pattern is veiled over the rectangle around every part found,
//! and far enough that its light parts drop under [`LIGHT_MIN`].

use super::AREA_SHARE;
use super::frame::LumImage;
use crate::strobe::pattern::halve;

/// At least this many parts alike (IRIS's six).
pub const MIN_PARTS: usize = 6;

/// The light parts' mean luminance must be at least this (IRIS's 0.25).
pub const LIGHT_MIN: f32 = 0.25;

/// The light parts must differ from the dark ones by at least this in mean luminance: the change a
/// flash needs ([`super::FLASH_DELTA`]).
pub const MIN_CONTRAST: f32 = super::FLASH_DELTA;

/// Parts whose shapes differ by less than this are alike (IRIS's `matchShapes` threshold).
pub const ALIKE: f64 = 0.7;

/// A part is at least this share of the image (IRIS leaves out contours smaller than this).
pub const MIN_PART_SHARE: f64 = 0.00155;

/// Images are halved until they are no wider than this (IRIS halves frames wider than 480
/// pixels).
pub const MAX_WIDTH: u32 = 480;

/// What the check found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Parts {
    /// The most parts alike.
    pub count: usize,
    /// The share of the image the rectangle around them covers.
    pub share: f32,
    /// The light parts' mean luminance, and the dark parts'.
    pub light: f32,
    pub dark: f32,
    /// The rectangle around every part found, `[x0, y0, x1, y1)` as shares of the image's width
    /// and height (where to veil).
    pub region: [f32; 4],
}

impl Parts {
    /// A harmful pattern by this rule.
    pub fn harmful(&self) -> bool {
        self.count >= MIN_PARTS
            && self.share as f64 >= AREA_SHARE
            && self.light >= LIGHT_MIN
            && self.light - self.dark >= MIN_CONTRAST
    }
}

/// One connected part.
struct Part {
    bbox: [u32; 4],
    hu: [f64; 7],
}

/// Otsu's threshold of `v` (values from `lo` to `hi`), in 256 levels.
fn otsu(v: &[f32], lo: f32, hi: f32) -> f32 {
    let mut hist = [0f64; 256];
    let span = (hi - lo).max(1e-9);
    for &x in v {
        hist[(((x - lo) / span) * 255.0).round().clamp(0.0, 255.0) as usize] += 1.0;
    }
    let total = v.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, h)| i as f64 * h).sum();
    let (mut wb, mut sb, mut best, mut at) = (0f64, 0f64, -1f64, 0usize);
    for (i, &h) in hist.iter().enumerate() {
        wb += h;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += i as f64 * h;
        let (mb, mf) = (sb / wb, (sum - sb) / wf);
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            at = i;
        }
    }
    lo + (at as f32 + 0.5) / 255.0 * span
}

/// The convex hull of points (Andrew's monotone chain), counter-clockwise.
fn hull(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f64, f64)> = vec![];
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = vec![];
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The raw moments (to the third order) of the polygon `p`, as OpenCV takes a contour's.
fn polygon_moments(p: &[(f64, f64)]) -> [f64; 10] {
    let mut m = [0f64; 10];
    for i in 0..p.len() {
        let (x0, y0) = p[i];
        let (x1, y1) = p[(i + 1) % p.len()];
        let a = x0 * y1 - x1 * y0;
        m[0] += a;
        m[1] += (x0 + x1) * a;
        m[2] += (y0 + y1) * a;
        m[3] += (x0 * x0 + x0 * x1 + x1 * x1) * a;
        m[4] += (x0 * y1 + 2.0 * x0 * y0 + 2.0 * x1 * y1 + x1 * y0) * a;
        m[5] += (y0 * y0 + y0 * y1 + y1 * y1) * a;
        m[6] += (x0 * x0 * x0 + x0 * x0 * x1 + x0 * x1 * x1 + x1 * x1 * x1) * a;
        m[7] +=
            (x0 * x0 * (3.0 * y0 + y1) + 2.0 * x0 * x1 * (y0 + y1) + x1 * x1 * (y0 + 3.0 * y1)) * a;
        m[8] +=
            (y0 * y0 * (3.0 * x0 + x1) + 2.0 * y0 * y1 * (x0 + x1) + y1 * y1 * (x0 + 3.0 * x1)) * a;
        m[9] += (y0 * y0 * y0 + y0 * y0 * y1 + y0 * y1 * y1 + y1 * y1 * y1) * a;
    }
    let scale = [2.0, 6.0, 6.0, 12.0, 24.0, 12.0, 20.0, 60.0, 60.0, 20.0];
    for (v, d) in m.iter_mut().zip(scale) {
        *v /= d;
    }
    if m[0] < 0.0 {
        m.iter_mut().for_each(|v| *v = -*v);
    }
    m
}

/// Hu's seven moment invariants of a part, from its raw moments.
fn hu(m: &[f64; 10]) -> [f64; 7] {
    let [m00, m10, m01, m20, m11, m02, m30, m21, m12, m03] = *m;
    let (cx, cy) = (m10 / m00, m01 / m00);
    let mu20 = m20 - cx * m10;
    let mu02 = m02 - cy * m01;
    let mu11 = m11 - cx * m01;
    let mu30 = m30 - 3.0 * cx * m20 + 2.0 * cx * cx * m10;
    let mu03 = m03 - 3.0 * cy * m02 + 2.0 * cy * cy * m01;
    let mu21 = m21 - 2.0 * cx * m11 - cy * m20 + 2.0 * cx * cx * m01;
    let mu12 = m12 - 2.0 * cy * m11 - cx * m02 + 2.0 * cy * cy * m10;
    let n2 = m00 * m00;
    let n3 = m00.powf(2.5);
    let (n20, n02, n11) = (mu20 / n2, mu02 / n2, mu11 / n2);
    let (n30, n03, n21, n12) = (mu30 / n3, mu03 / n3, mu21 / n3, mu12 / n3);
    let (a, b) = (n30 + n12, n21 + n03);
    [
        n20 + n02,
        (n20 - n02).powi(2) + 4.0 * n11 * n11,
        (n30 - 3.0 * n12).powi(2) + (3.0 * n21 - n03).powi(2),
        a * a + b * b,
        (n30 - 3.0 * n12) * a * (a * a - 3.0 * b * b)
            + (3.0 * n21 - n03) * b * (3.0 * a * a - b * b),
        (n20 - n02) * (a * a - b * b) + 4.0 * n11 * a * b,
        (3.0 * n21 - n03) * a * (a * a - 3.0 * b * b)
            - (n30 - 3.0 * n12) * b * (3.0 * a * a - b * b),
    ]
}

/// How different two parts' shapes are: OpenCV's `CONTOURS_MATCH_I1`.
fn unlike(a: &[f64; 7], b: &[f64; 7]) -> f64 {
    const EPS: f64 = 1e-5;
    let mut d = 0.0;
    for k in 0..7 {
        let (x, y) = (a[k], b[k]);
        if x.abs() > EPS && y.abs() > EPS {
            let mx = x.signum() * x.abs().log10();
            let my = y.signum() * y.abs().log10();
            d += (1.0 / mx - 1.0 / my).abs();
        }
    }
    d
}

/// The connected parts (along edges and corners) of `light` and of its opposite, with at least
/// `min` and at most `max` samples.
fn parts(light: &[bool], w: usize, h: usize, min: f64, max: f64) -> Vec<Part> {
    let mut seen = vec![false; light.len()];
    let mut out = vec![];
    let mut stack = vec![];
    for start in 0..light.len() {
        if seen[start] {
            continue;
        }
        let side = light[start];
        seen[start] = true;
        stack.push(start);
        let mut area = 0f64;
        // Each row's leftmost and rightmost sample, for the hull.
        let mut rows: std::collections::HashMap<usize, (usize, usize)> = Default::default();
        let mut bbox = [u32::MAX, u32::MAX, 0, 0];
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            area += 1.0;
            let r = rows.entry(y).or_insert((x, x));
            *r = (r.0.min(x), r.1.max(x));
            bbox = [
                bbox[0].min(x as u32),
                bbox[1].min(y as u32),
                bbox[2].max(x as u32 + 1),
                bbox[3].max(y as u32 + 1),
            ];
            for yy in y.saturating_sub(1)..(y + 2).min(h) {
                for xx in x.saturating_sub(1)..(x + 2).min(w) {
                    let q = yy * w + xx;
                    if !seen[q] && light[q] == side {
                        seen[q] = true;
                        stack.push(q);
                    }
                }
            }
        }
        if area >= min && area <= max {
            let corners: Vec<(f64, f64)> = rows
                .iter()
                .flat_map(|(&y, &(x0, x1))| {
                    let (y, x0, x1) = (y as f64, x0 as f64, x1 as f64 + 1.0);
                    [(x0, y), (x0, y + 1.0), (x1, y), (x1, y + 1.0)]
                })
                .collect();
            let m = polygon_moments(&hull(corners));
            if m[0] > 0.0 {
                out.push(Part { bbox, hu: hu(&m) });
            }
        }
    }
    out
}

/// The similar parts of the frame whose half image is `img`.
pub fn find(img: &LumImage) -> Parts {
    let mut small = None;
    while small.as_ref().unwrap_or(img).width > MAX_WIDTH {
        small = Some(halve(small.as_ref().unwrap_or(img)));
    }
    let img = small.as_ref().unwrap_or(img);
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return Parts::default();
    }
    let (lo, hi) = img
        .lum
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    if hi - lo < 1e-4 {
        return Parts::default();
    }
    let cut = otsu(&img.lum, lo, hi);
    let light: Vec<bool> = img.lum.iter().map(|&x| x > cut).collect();
    let (mut ls, mut ln, mut ds, mut dn) = (0f64, 0f64, 0f64, 0f64);
    for (&x, &l) in img.lum.iter().zip(&light) {
        if l {
            ls += x as f64;
            ln += 1.0;
        } else {
            ds += x as f64;
            dn += 1.0;
        }
    }
    let total = (w * h) as f64;
    let found = parts(&light, w, h, (MIN_PART_SHARE * total).max(2.0), total / 6.0);
    let mut out = Parts {
        light: (ls / ln.max(1.0)) as f32,
        dark: (ds / dn.max(1.0)) as f32,
        ..Default::default()
    };
    if found.len() < MIN_PARTS {
        return out;
    }
    // The largest set alike to one part (IRIS's grouping), and the rectangle around it.
    let mut best: Vec<usize> = vec![];
    for (i, a) in found.iter().enumerate() {
        let group: Vec<usize> = found
            .iter()
            .enumerate()
            .filter(|(j, b)| *j == i || unlike(&a.hu, &b.hu) < ALIKE)
            .map(|(j, _)| j)
            .collect();
        if group.len() > best.len() {
            best = group;
        }
    }
    let rect = |idx: &mut dyn Iterator<Item = &Part>| {
        idx.fold([u32::MAX, u32::MAX, 0, 0], |r, p| {
            [
                r[0].min(p.bbox[0]),
                r[1].min(p.bbox[1]),
                r[2].max(p.bbox[2]),
                r[3].max(p.bbox[3]),
            ]
        })
    };
    let g = rect(&mut best.iter().map(|&i| &found[i]));
    let all = rect(&mut found.iter());
    out.count = best.len();
    out.share = ((g[2] - g[0]) as f64 * (g[3] - g[1]) as f64 / total) as f32;
    out.region = [
        all[0] as f32 / w as f32,
        all[1] as f32 / h as f32,
        all[2] as f32 / w as f32,
        all[3] as f32 / h as f32,
    ];
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::strobe::frame::reduce_both;

    fn half(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> LumImage {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        reduce_both(&v, w, h).1
    }

    #[test]
    fn a_soup_of_square_cells_is_a_pattern_of_similar_parts() {
        // Random 8-pixel cells, half of them white: many parts alike, all over the frame.
        let mut rng = Rng::new(5);
        let cells: Vec<bool> = (0..40 * 30).map(|_| rng.next_f64() < 0.3).collect();
        let soup = half(320, 240, |x, y| {
            if cells[(y / 8 * 40 + x / 8) as usize] {
                255
            } else {
                20
            }
        });
        let p = find(&soup);
        assert!(p.harmful(), "{p:?}");
        assert!(p.region[0] < 0.1 && p.region[2] > 0.9, "{p:?}");
    }

    #[test]
    fn regular_stripes_are_similar_parts_too() {
        let s = half(320, 240, |x, _| if (x / 8) % 2 == 0 { 0 } else { 255 });
        assert!(find(&s).harmful());
    }

    #[test]
    fn dim_light_parts_a_few_parts_or_a_small_area_are_not() {
        // The same soup, its light cells dim (under 0.25 in luminance).
        let mut rng = Rng::new(5);
        let cells: Vec<bool> = (0..40 * 30).map(|_| rng.next_f64() < 0.3).collect();
        let dim = half(320, 240, |x, y| {
            if cells[(y / 8 * 40 + x / 8) as usize] {
                120
            } else {
                20
            }
        });
        let p = find(&dim);
        assert!(p.light < LIGHT_MIN && !p.harmful(), "{p:?}");
        // Five squares.
        let five = half(320, 240, |x, y| {
            if y > 100 && y < 140 && x % 64 < 32 && x < 300 {
                255
            } else {
                0
            }
        });
        assert!(!find(&five).harmful(), "{:?}", find(&five));
        // Squares in a corner, under a quarter of the frame.
        let corner = half(320, 240, |x, y| {
            if x < 120 && y < 120 && (x / 10 + y / 10) % 2 == 0 {
                255
            } else {
                0
            }
        });
        assert!(!find(&corner).harmful(), "{:?}", find(&corner));
        // Squares barely lighter than the lines between them (faint outlines over a bright field).
        let faint = half(
            320,
            240,
            |x, y| if x % 16 == 0 || y % 16 == 0 { 225 } else { 240 },
        );
        let f = find(&faint);
        assert!(f.count >= MIN_PARTS && !f.harmful(), "{f:?}");
        // Plain, and noise too fine to make parts.
        assert!(!find(&half(320, 240, |_, _| 128)).harmful());
        let mut rng = Rng::new(9);
        let noise: Vec<u8> = (0..320 * 240)
            .map(|_| (rng.next_f64() * 255.0) as u8)
            .collect();
        let n = half(320, 240, |x, y| noise[(y * 320 + x) as usize]);
        assert!(!find(&n).harmful(), "{:?}", find(&n));
    }

    #[test]
    fn shapes_alike_are_matched_whatever_their_size() {
        let square = |s: u32| {
            let mut m = [0f64; 10];
            for y in 0..s {
                for x in 0..s {
                    let (fx, fy) = (x as f64, y as f64);
                    m[0] += 1.0;
                    m[1] += fx;
                    m[2] += fy;
                    m[3] += fx * fx;
                    m[4] += fx * fy;
                    m[5] += fy * fy;
                    m[6] += fx * fx * fx;
                    m[7] += fx * fx * fy;
                    m[8] += fx * fy * fy;
                    m[9] += fy * fy * fy;
                }
            }
            hu(&m)
        };
        assert!(unlike(&square(6), &square(12)) < ALIKE);
    }
}
