//! The stripe check (#147): regular, high-contrast stripes, which can trigger seizures as flashes
//! can (ITU-R BT.1702; the Harding guidance; IRIS's pattern check).
//!
//! The guidance describes a harmful pattern as clearly discernible stripes, more than five
//! light-dark pairs of them, in any orientation, filling a large part of the screen: more than a
//! quarter when they move, flicker or reverse, more than 40% when still. IRIS uses a quarter
//! either way, and so does JETCARP, so a still pattern counts as soon as a moving one would. The
//! stripes may be straight or not, and made of rows of separate elements (a checkerboard, dots).
//!
//! **How it is measured.** The half-resolution luminance image of the frame ([`LumImage`]) is
//! looked at in windows of [`WINDOW`] by [`WINDOW`] samples, at four scales (each sample 2, 4, 8 and
//! 16 pixels), so stripes from 4 to about 128 pixels a pair are seen; finer stripes (under 4
//! pixels a pair, at the conservative viewing distance more than six cycles a degree) blur into
//! the half image. A window is *striped* when:
//!
//! - its light and dark samples (above and below its mean) differ by at least [`MIN_CONTRAST`],
//!   with the dark ones below [`DARK_LIMIT`], and neither makes up less than [`MIN_DUTY`] of it
//!   (a few lines on a plain ground are not stripes). Cell outlines over dead cells are too faint
//!   to count; strong lines in a grid do count, as rows of repeated elements, which the guidance
//!   includes;
//! - along its rows or its columns, the light and dark runs cross the mean the same number of
//!   times in every line, give or take one (noise and random cells do not), at least twice a
//!   period for two periods; and
//! - the window is periodic with that period: shifted by a period, all its lines together match
//!   themselves (correlation at least [`MIN_MATCH`]), and shifted by half a period it is its own negative (at most
//!   `-HALF_MATCH`). Stripes at any angle are periodic along the rows, the columns or both;
//!   checkerboards and dots along both.
//!
//! The windows found, at any scale, cover a share of the screen. Over [`AREA_SHARE`] with more than
//! five pairs of stripes across it is a harmful pattern; harmful for [`FAIL_SECONDS`] it fails, as
//! IRIS counts it. For someone who answered the seizure warning *at risk*, a pattern of similar
//! parts, regular or not, is harmful too ([`super::similar`], IRIS's own rule;
//! [`PatternMeter::set_at_risk`]). Whether the pattern moves or flickers (its samples change between checks) is
//! reported, not used: the quarter applies either way.

use super::frame::LumImage;
use super::{AREA_SHARE, DARK_LIMIT, TILE};
use serde::Serialize;

/// Windows are this many samples on a side.
pub const WINDOW: u32 = 16;

/// The least difference between a window's light and dark samples that makes stripes: half the
/// 0.1 of a flash, since a pattern's contrast is the larger half of its risk.
pub const MIN_CONTRAST: f32 = 0.05;

/// The least share of a window its light or its dark samples must make up.
pub const MIN_DUTY: f32 = 0.15;

/// The longest period a window can show, in samples: a period and a half in its 16.
pub const MAX_PERIOD: f32 = 10.5;

/// The shortest period a window looks for, in samples: 2 at the finest scale, and 4 at the others,
/// where shorter periods are the finer scale's and a box-filtered image near its limit aliases
/// (thin lines 10 pixels apart look like broad ones at 4 pixels a sample).
pub fn min_period(scale: u32) -> f32 {
    if scale == 0 { 2.0 } else { 4.0 }
}

/// The correlation of a window with itself a period along that counts as periodic.
pub const MIN_MATCH: f32 = 0.5;

/// Shifted by half a period, a periodic window's correlation with itself is at most minus this
/// (a square wave whose light part is [`MIN_DUTY`] of it reaches -0.18).
pub const HALF_MATCH: f32 = 0.15;

/// A harmful pattern shown this long fails (IRIS's half a second).
pub const FAIL_SECONDS: f64 = 0.5;

/// More than this many light-dark pairs across the pattern.
pub const MIN_PAIRS: f32 = 5.0;

/// Scales looked at: each sample of the half image is 2 pixels, then 4, 8 and 16.
pub const SCALES: u32 = 4;

/// What one window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stripes {
    /// Light samples' mean minus dark samples' mean.
    pub contrast: f32,
    /// The window's mean luminance.
    pub mean: f32,
    /// The period found, in samples of its scale (along the rows or the columns).
    pub period: f32,
    /// The rows that show it, a bit each (0 when the period is along the columns only).
    pub rows: u32,
    /// The columns that show it.
    pub cols: u32,
}

/// How a reading stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PatternLevel {
    /// No harmful pattern.
    #[default]
    Pass,
    /// A harmful pattern, for less than [`FAIL_SECONDS`].
    Harmful,
    /// A harmful pattern for [`FAIL_SECONDS`] or more.
    Fail,
}

impl PatternLevel {
    pub fn name(self) -> &'static str {
        match self {
            PatternLevel::Pass => "pass",
            PatternLevel::Harmful => "harmful",
            PatternLevel::Fail => "fail",
        }
    }
}

/// One check's reading.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Default)]
pub struct PatternReading {
    pub time: f64,
    /// The share of the screen covered by striped windows.
    pub share: f32,
    /// Their contrast, weighted by area.
    pub contrast: f32,
    /// Their period in pixels (the median of the windows').
    pub period_px: f32,
    /// Light-dark pairs across the striped area.
    pub pairs: f32,
    /// The striped area's samples changed since the last check: the stripes move or flicker.
    pub changing: bool,
    /// The most similar parts the at-risk rule found ([`super::similar`]; 0 by the usual rule).
    pub parts: u32,
    /// The pattern is harmful by the at-risk rule (similar parts), whether or not by stripes.
    pub similar: bool,
    pub level: PatternLevel,
}

/// The mean of `v` relative to which a window's runs are counted, and the window's light and dark
/// means and shares.
struct Split {
    mean: f32,
    light: f32,
    dark: f32,
    duty: f32,
}

fn split(v: &[f32]) -> Split {
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    let (mut ls, mut ln, mut ds, mut dn) = (0f32, 0u32, 0f32, 0u32);
    for &x in v {
        if x > mean {
            ls += x;
            ln += 1;
        } else {
            ds += x;
            dn += 1;
        }
    }
    let light = if ln > 0 { ls / ln as f32 } else { mean };
    let dark = if dn > 0 { ds / dn as f32 } else { mean };
    Split {
        mean,
        light,
        dark,
        duty: ln.min(dn) as f32 / v.len() as f32,
    }
}

/// A window's samples, row by row.
type Win = [f32; (WINDOW * WINDOW) as usize];

/// Sample `k` of line `i` of a window: its rows, or its columns.
#[inline]
fn sample(w: &Win, rows: bool, i: usize, k: usize) -> f32 {
    let n = WINDOW as usize;
    if rows { w[i * n + k] } else { w[k * n + i] }
}

/// The correlation of the window's lines with themselves shifted along by `lag` samples, each
/// line taken about its own mean (a plain line, such as one between rows of dots, adds nothing).
fn correlation(w: &Win, rows: bool, lag: f32) -> f32 {
    let n = WINDOW as usize;
    let (whole, frac) = (lag.floor() as usize, lag - lag.floor());
    let (mut sab, mut saa, mut sbb) = (0f32, 0f32, 0f32);
    for i in 0..n {
        let mean = (0..n).map(|k| sample(w, rows, i, k)).sum::<f32>() / n as f32;
        let mut x = 0;
        while x + whole + 1 < n || (frac == 0.0 && x + whole < n) {
            let a = sample(w, rows, i, x) - mean;
            let b0 = sample(w, rows, i, x + whole);
            let b = if frac > 0.0 {
                b0 + (sample(w, rows, i, x + whole + 1) - b0) * frac
            } else {
                b0
            } - mean;
            sab += a * b;
            saa += a * a;
            sbb += b * b;
            x += 1;
        }
    }
    if saa <= 0.0 || sbb <= 0.0 {
        return 0.0;
    }
    sab / (saa * sbb).sqrt()
}

/// The period of the window's lines (rows, or columns), and which lines show it (a bit each), if:
/// at least a quarter of them cross the window's mean three times or more (rows of dots leave
/// lines between them that never cross); those cross it about as often as each other; every
/// other crossing is a period apart, give or take a fifth; and shifted by that period the lines
/// together match themselves, while shifted by half of it they are their own negative. The
/// cheap tests come first: most windows of noise fail on the counts.
fn periodic(w: &Win, rows: bool, mean: f32, shortest: f32) -> Option<(f32, u32)> {
    let n = WINDOW as usize;
    let mut counts = [0u8; WINDOW as usize];
    let (mut used, mut sum, mut sum2) = (0u32, 0f32, 0f32);
    for (i, c) in counts.iter_mut().enumerate() {
        let mut last = sample(w, rows, i, 0) > mean;
        for k in 1..n {
            let now = sample(w, rows, i, k) > mean;
            *c += (now != last) as u8;
            last = now;
        }
        if *c >= 3 {
            used += 1;
            sum += *c as f32;
            sum2 += (*c as f32) * (*c as f32);
        }
    }
    if used * 4 < n as u32 {
        return None;
    }
    let m = sum / used as f32;
    if sum2 / used as f32 - m * m > 1.0 {
        return None;
    }
    // Every other crossing, at its position between samples.
    let (mut ps, mut ps2, mut pn) = (0f32, 0f32, 0f32);
    let mut which = 0u32;
    for (i, &c) in counts.iter().enumerate() {
        if c < 3 {
            continue;
        }
        which |= 1 << i;
        let mut at = [0f32; WINDOW as usize];
        let mut na = 0;
        for k in 0..n - 1 {
            let (a, b) = (sample(w, rows, i, k), sample(w, rows, i, k + 1));
            if (a > mean) != (b > mean) {
                at[na] = k as f32 + ((mean - a) / (b - a)).clamp(0.0, 1.0);
                na += 1;
            }
        }
        for k in 0..na.saturating_sub(2) {
            let p = at[k + 2] - at[k];
            ps += p;
            ps2 += p * p;
            pn += 1.0;
        }
    }
    let period = ps / pn;
    let spread = (ps2 / pn - period * period).max(0.0).sqrt();
    if spread > 0.2 * period || !(shortest..=MAX_PERIOD).contains(&period) {
        return None;
    }
    let full = correlation(w, rows, period);
    let half = correlation(w, rows, period / 2.0);
    (full >= MIN_MATCH && half <= -HALF_MATCH).then_some((period, which))
}

/// Whether the `WINDOW`-sample square at `(x0, y0)` of `img` is striped, with a period of at least
/// `shortest` samples ([`min_period`]).
pub fn window(img: &LumImage, x0: u32, y0: u32, shortest: f32) -> Option<Stripes> {
    let n = WINDOW;
    if x0 + n > img.width || y0 + n > img.height {
        return None;
    }
    let mut w: Win = [0.0; (WINDOW * WINDOW) as usize];
    for y in 0..n {
        let row = ((y0 + y) * img.width + x0) as usize;
        w[(y * n) as usize..((y + 1) * n) as usize]
            .copy_from_slice(&img.lum[row..row + n as usize]);
    }
    let s = split(&w);
    let contrast = s.light - s.dark;
    if contrast < MIN_CONTRAST || s.dark >= DARK_LIMIT || s.duty < MIN_DUTY {
        return None;
    }
    let (along_rows, along_cols) = (
        periodic(&w, true, s.mean, shortest),
        periodic(&w, false, s.mean, shortest),
    );
    let period = match (along_rows, along_cols) {
        (Some(a), Some(b)) => a.0.min(b.0),
        (Some(a), None) | (None, Some(a)) => a.0,
        (None, None) => return None,
    };
    Some(Stripes {
        contrast,
        mean: s.mean,
        period,
        rows: along_rows.map_or(0, |a| a.1),
        cols: along_cols.map_or(0, |a| a.1),
    })
}

/// `img` at half its resolution (each value the mean of a 2 by 2 block, or of what an odd edge
/// has).
pub fn halve(img: &LumImage) -> LumImage {
    let (w, h) = (img.width.div_ceil(2).max(1), img.height.div_ceil(2).max(1));
    let mut lum = vec![0f32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (mut s, mut k) = (0f32, 0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + dx, 2 * y + dy);
                if sx < img.width && sy < img.height {
                    s += img.at(sx, sy);
                    k += 1.0;
                }
            }
            lum[(y * w + x) as usize] = s / k.max(1.0);
        }
    }
    LumImage {
        width: w,
        height: h,
        lum,
    }
}

/// What a check found where: for each 16-pixel tile of the frame, the strongest stripes covering
/// it (their contrast and their window's mean luminance), for the guard's veil.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub cols: u32,
    pub rows: u32,
    pub tiles: Vec<Option<(f32, f32)>>,
}

/// Where windows start along a side of `n` samples: every [`WINDOW`] from 0, and one more against
/// the far edge when the side is not a whole number of windows, so stripes near the right and
/// bottom edges are seen (and veiled) as well as anywhere else.
fn origins(n: u32) -> impl Iterator<Item = u32> {
    let whole = n / WINDOW;
    let edge = (whole > 0 && !n.is_multiple_of(WINDOW)).then_some(n - WINDOW);
    (0..whole).map(|k| k * WINDOW).chain(edge)
}

/// Every striped window of the frame whose half image is `img`, at every scale from `first` on,
/// as the tiles they cover; with their area-weighted contrast and their median period in pixels.
pub fn find(img: &LumImage, first: u32) -> (Found, f32, f32) {
    let (fw, fh) = (img.width * 2, img.height * 2);
    let (cols, rows) = super::frame::tiles_for(fw, fh);
    let mut found = Found {
        cols,
        rows,
        tiles: vec![None; (cols * rows) as usize],
    };
    let mut periods = vec![];
    let (mut csum, mut cw) = (0f32, 0f32);
    let mut level = img.clone();
    for _ in 0..first.min(SCALES - 1) {
        level = halve(&level);
    }
    for scale in first.min(SCALES - 1)..SCALES {
        // Pixels of the frame each sample covers.
        let px = 2u32 << scale;
        for y0 in origins(level.height) {
            for x0 in origins(level.width) {
                if let Some(s) = window(&level, x0, y0, min_period(scale)) {
                    periods.push(s.period * px as f32);
                    let area = (WINDOW * px) as f32 * (WINDOW * px) as f32;
                    csum += s.contrast * area;
                    cw += area;
                    // The tiles its striped lines cover.
                    for k in 0..WINDOW {
                        for (bits, along_rows) in [(s.rows, true), (s.cols, false)] {
                            if bits & (1 << k) == 0 {
                                continue;
                            }
                            // Line k spans these frame pixels across, and the whole window along.
                            let (lo, hi) = if along_rows {
                                ((y0 + k) * px, (y0 + k + 1) * px)
                            } else {
                                ((x0 + k) * px, (x0 + k + 1) * px)
                            };
                            let (wlo, whi) = if along_rows {
                                (x0 * px, (x0 + WINDOW) * px)
                            } else {
                                (y0 * px, (y0 + WINDOW) * px)
                            };
                            for t1 in lo / TILE..hi.div_ceil(TILE) {
                                for t2 in wlo / TILE..whi.div_ceil(TILE) {
                                    let (tx, ty) = if along_rows { (t2, t1) } else { (t1, t2) };
                                    if tx >= cols || ty >= rows {
                                        continue;
                                    }
                                    let t = &mut found.tiles[(ty * cols + tx) as usize];
                                    if t.is_none_or(|(c, _)| s.contrast > c) {
                                        *t = Some((s.contrast, s.mean));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if scale + 1 < SCALES {
            level = halve(&level);
        }
    }
    periods.sort_by(f32::total_cmp);
    let period = periods.get(periods.len() / 2).copied().unwrap_or(0.0);
    let contrast = if cw > 0.0 { csum / cw } else { 0.0 };
    (found, contrast, period)
}

/// The stripe check over a stream of frames.
#[derive(Clone, Debug, Default)]
pub struct PatternMeter {
    /// The screen's area in pixels, when the frames are a part of it.
    screen_px: Option<f64>,
    /// The finest scale looked at: 1 on a screen with twice the reference density or more, where
    /// stripes under 8 pixels a pair are finer than six cycles a degree, and the check costs a
    /// quarter as much.
    first: u32,
    /// The at-risk rule is on too ([`super::similar`]).
    at_risk: bool,
    since: Option<f64>,
    last_img: Option<LumImage>,
    last_found: Found,
    last: PatternReading,
}

impl PatternMeter {
    /// A meter for frames that are all of a screen, or part of one of `screen_px` pixels.
    pub fn new(screen_px: Option<f64>) -> PatternMeter {
        PatternMeter {
            screen_px,
            ..Default::default()
        }
    }

    /// A meter for a screen watched as `viewing` says.
    pub fn for_viewing(viewing: super::Viewing, screen_px: Option<f64>) -> PatternMeter {
        let reference = super::Viewing::screen(1.0).field_px;
        PatternMeter {
            screen_px,
            first: if viewing.field_px >= 2.0 * reference - 1e-6 {
                1
            } else {
                0
            },
            ..Default::default()
        }
    }

    pub fn set_screen(&mut self, screen_px: Option<f64>) {
        self.screen_px = screen_px;
    }

    /// Someone who answered the seizure warning *at risk* is watching: a pattern of similar
    /// parts counts too ([`super::similar`]).
    pub fn set_at_risk(&mut self, at_risk: bool) {
        self.at_risk = at_risk;
    }

    pub fn at_risk(&self) -> bool {
        self.at_risk
    }

    pub fn reset(&mut self) {
        *self = PatternMeter {
            screen_px: self.screen_px,
            first: self.first,
            at_risk: self.at_risk,
            ..Default::default()
        };
    }

    /// The last reading.
    pub fn reading(&self) -> PatternReading {
        self.last
    }

    /// Where the last check found stripes (empty unless it was harmful).
    pub fn found(&self) -> &Found {
        &self.last_found
    }

    /// Checks the frame whose half image is `img`, shown at time `t`.
    pub fn update(&mut self, img: &LumImage, t: f64) -> PatternReading {
        let (mut found, contrast, period_px) = find(img, self.first);
        let (fw, fh) = (img.width * 2, img.height * 2);
        let frame_px = fw as f64 * fh as f64;
        let screen = self.screen_px.filter(|s| *s > frame_px).unwrap_or(frame_px);
        let mut covered = 0f64;
        for (i, t) in found.tiles.iter().enumerate() {
            if t.is_some() {
                let (c, r) = (i as u32 % found.cols, i as u32 / found.cols);
                let w = fw.saturating_sub(c * TILE).min(TILE);
                let h = fh.saturating_sub(r * TILE).min(TILE);
                covered += (w * h) as f64;
            }
        }
        let share = (covered / screen.max(1.0)) as f32;
        let pairs = if period_px > 0.0 {
            covered.sqrt() as f32 / period_px
        } else {
            0.0
        };
        // Moving or flickering: the striped samples changed by more than a quarter of the
        // contrast on average since the last check.
        let changing = match &self.last_img {
            Some(prev) if prev.width == img.width && prev.height == img.height && covered > 0.0 => {
                let (mut d, mut n) = (0f32, 0f32);
                for (i, tile) in found.tiles.iter().enumerate() {
                    if tile.is_none() {
                        continue;
                    }
                    let (c, r) = (i as u32 % found.cols, i as u32 / found.cols);
                    for y in (r * TILE / 2)..((r + 1) * TILE / 2).min(img.height) {
                        for x in (c * TILE / 2)..((c + 1) * TILE / 2).min(img.width) {
                            d += (img.at(x, y) - prev.at(x, y)).abs();
                            n += 1.0;
                        }
                    }
                }
                n > 0.0 && d / n > contrast * 0.25
            }
            _ => false,
        };
        let mut harmful = share as f64 > AREA_SHARE && pairs > MIN_PAIRS;
        let mut share = share;
        // The at-risk rule: similar parts, veiled over the rectangle around them, far enough to
        // bring their light parts under the rule's light level.
        let (mut parts, mut similar) = (0, false);
        if self.at_risk {
            let p = super::similar::find(img);
            parts = p.count as u32;
            if p.harmful() {
                similar = true;
                harmful = true;
                share = share.max(p.share);
                // The veil's grey, as [`super::guard::veil_for`] takes it from the mean.
                let mean = (p.light + p.dark) / 2.0;
                let grey = mean.min(super::guard::VEIL_GREY_MAX);
                let dim = if p.light > 0.2 {
                    (p.light - 0.2) / (p.light - grey).max(1e-3)
                } else {
                    0.0
                };
                let strong = 1.0 - super::guard::VEIL_CONTRAST / (p.light - p.dark).max(1e-3);
                let amount = dim.max(strong).clamp(0.0, 0.98);
                // As a contrast, so the veil ([`super::guard::veil_for`]) takes that amount.
                let as_contrast = super::guard::VEIL_CONTRAST / (1.0 - amount);
                let (x0, y0) = (p.region[0] * fw as f32, p.region[1] * fh as f32);
                let (x1, y1) = (p.region[2] * fw as f32, p.region[3] * fh as f32);
                for ty in (y0 as u32 / TILE)..(y1.ceil() as u32).div_ceil(TILE).min(found.rows) {
                    for tx in (x0 as u32 / TILE)..(x1.ceil() as u32).div_ceil(TILE).min(found.cols)
                    {
                        let t = &mut found.tiles[(ty * found.cols + tx) as usize];
                        if t.is_none_or(|(c, _)| as_contrast > c) {
                            *t = Some((as_contrast, mean));
                        }
                    }
                }
            }
        }
        if harmful {
            self.since.get_or_insert(t);
        } else {
            self.since = None;
        }
        let level = match self.since {
            Some(s) if t - s >= FAIL_SECONDS - 1e-9 => PatternLevel::Fail,
            Some(_) => PatternLevel::Harmful,
            None => PatternLevel::Pass,
        };
        self.last_found = if harmful {
            found
        } else {
            Found {
                cols: found.cols,
                rows: found.rows,
                tiles: vec![None; found.tiles.len()],
            }
        };
        self.last_img = Some(img.clone());
        self.last = PatternReading {
            time: t,
            share,
            contrast,
            period_px,
            pairs,
            changing,
            parts,
            similar,
            level,
        };
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::strobe::frame::reduce_both;

    const W: u32 = 640;
    const H: u32 = 480;

    /// The half image of a frame `f(x, y)` gives the grey level of.
    fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> LumImage {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        reduce_both(&v, w, h).1
    }

    fn level(img: &LumImage) -> PatternReading {
        let mut m = PatternMeter::new(None);
        m.update(img, 0.0)
    }

    fn harmful(img: &LumImage) -> bool {
        level(img).level >= PatternLevel::Harmful
    }

    #[test]
    fn stripes_of_every_orientation_and_size_are_found() {
        for period in [4u32, 8, 16, 32, 64, 128] {
            let half = period / 2;
            // Wide stripes need a wide screen to have more than five pairs.
            let (w, h) = if period > 64 { (1600, 1000) } else { (W, H) };
            let v = frame(w, h, |x, _| if (x / half) % 2 == 0 { 0 } else { 255 });
            let r = level(&v);
            assert!(r.share > 0.6, "vertical, {period}-pixel pairs: {r:?}");
            assert_eq!(r.level, PatternLevel::Harmful, "vertical {period}: {r:?}");
            assert!(
                (r.period_px - period as f32).abs() <= period as f32 * 0.2,
                "{period}: {}",
                r.period_px
            );
            let hz = frame(w, h, |_, y| if (y / half) % 2 == 0 { 0 } else { 255 });
            assert!(harmful(&hz), "horizontal {period}");
            let d = frame(
                w,
                h,
                |x, y| if ((x + y) / half) % 2 == 0 { 30 } else { 220 },
            );
            assert!(harmful(&d), "diagonal {period}");
        }
    }

    #[test]
    fn checkerboards_and_dots_are_patterns() {
        let board = frame(W, H, |x, y| if (x / 8 + y / 8) % 2 == 0 { 0 } else { 255 });
        assert!(harmful(&board));
        // Rows of round dots, 12 pixels apart.
        let dots = frame(W, H, |x, y| {
            let (dx, dy) = ((x % 12) as i32 - 6, (y % 12) as i32 - 6);
            if dx * dx + dy * dy < 16 { 255 } else { 0 }
        });
        assert!(harmful(&dots), "{:?}", level(&dots));
    }

    #[test]
    fn noise_and_random_cells_are_not_stripes() {
        let mut rng = Rng::new(7);
        let noise: Vec<u8> = (0..W * H)
            .map(|_| if rng.below(2) == 0 { 0 } else { 255 })
            .collect();
        let n = frame(W, H, |x, y| noise[(y * W + x) as usize]);
        assert_eq!(level(&n).level, PatternLevel::Pass, "{:?}", level(&n));
        for cell in [2u32, 4, 8, 16] {
            let mut rng = Rng::new(cell as u64);
            let (cw, ch) = (W / cell + 1, H / cell + 1);
            let cells: Vec<bool> = (0..cw * ch).map(|_| rng.below(2) == 0).collect();
            let f = frame(W, H, |x, y| {
                if cells[((y / cell) * cw + x / cell) as usize] {
                    255
                } else {
                    0
                }
            });
            let r = level(&f);
            assert!(r.share < 0.25, "random {cell}-pixel cells: {r:?}");
        }
    }

    #[test]
    fn faint_outlines_are_not_stripes_but_a_strong_grid_is() {
        // The outline colour over black cells, as the renderer draws it: too faint.
        for cell in [7u32, 8, 10, 16] {
            let g = frame(W, H, |x, y| {
                if x % cell == 0 || y % cell == 0 {
                    56
                } else {
                    0
                }
            });
            assert_eq!(
                level(&g).level,
                PatternLevel::Pass,
                "{cell}: {:?}",
                level(&g)
            );
        }
        // Dark lines over a field of white cells: rows of repeated squares, a pattern.
        let g = frame(
            W,
            H,
            |x, y| if x % 10 == 0 || y % 10 == 0 { 40 } else { 255 },
        );
        assert!(harmful(&g), "{:?}", level(&g));
    }

    #[test]
    fn stripes_against_the_far_edges_are_found_too() {
        // Bands 48 pixels high and 96 a pair: only windows 256 pixels on a side see them, and
        // neither 640 nor 480 is a whole number of those, so windows also sit against the right
        // and bottom edges. Every tile is found, so the veil covers the whole pattern.
        let bands = frame(W, H, |_, y| if (y / 48) % 2 == 0 { 0 } else { 255 });
        let mut m = PatternMeter::new(None);
        let r = m.update(&bands, 0.0);
        assert!(r.share > 0.99, "{r:?}");
        assert!(m.found().tiles.iter().all(|t| t.is_some()), "{r:?}");
    }

    /// How long a check of a 1080p frame's half image takes (ignored; run with `--ignored
    /// --nocapture`).
    #[test]
    #[ignore]
    fn pattern_cost() {
        let mut rng = Rng::new(3);
        let img = LumImage {
            width: 960,
            height: 540,
            lum: (0..960 * 540).map(|_| rng.next_f64() as f32).collect(),
        };
        let stripes = frame(1920, 1080, |x, _| if (x / 8) % 2 == 0 { 0 } else { 255 });
        for (name, i) in [("noise", &img), ("stripes everywhere", &stripes)] {
            let t = std::time::Instant::now();
            for _ in 0..10 {
                std::hint::black_box(find(i, 0));
            }
            println!(
                "{name}: {:.2} ms a check",
                t.elapsed().as_secs_f64() * 100.0
            );
        }
    }

    #[test]
    fn contrast_and_the_darker_bars_decide() {
        // Two greys MIN_CONTRAST apart in luminance, and just under.
        let base = 80u8;
        let lb = crate::strobe::luminance([base; 3]);
        let over = (base..=255)
            .find(|&g| crate::strobe::luminance([g; 3]) - lb >= MIN_CONTRAST + 0.005)
            .unwrap();
        let under = (base..=255)
            .rev()
            .find(|&g| crate::strobe::luminance([g; 3]) - lb < MIN_CONTRAST - 0.005)
            .unwrap();
        let s = |hi: u8| frame(W, H, move |x, _| if (x / 8) % 2 == 0 { base } else { hi });
        assert!(harmful(&s(over)));
        assert!(!harmful(&s(under)));
        // Bright stripes whose darker bars are at 0.8 or more.
        let bright = frame(W, H, |x, _| if (x / 8) % 2 == 0 { 235 } else { 255 });
        assert!(!harmful(&bright));
    }

    #[test]
    fn a_quarter_of_the_screen_with_more_than_five_pairs() {
        // Stripes over a band of the screen: 20% and 30%.
        for (rows, want) in [(96u32, false), (160, true)] {
            let f = frame(
                W,
                H,
                move |x, y| {
                    if y < rows && (x / 8) % 2 == 1 { 255 } else { 0 }
                },
            );
            let r = level(&f);
            assert_eq!(r.level >= PatternLevel::Harmful, want, "{rows} rows: {r:?}");
        }
        // A view that is part of a larger screen counts against the screen.
        let v = frame(W, H, |x, _| if (x / 8) % 2 == 0 { 0 } else { 255 });
        let mut m = PatternMeter::new(Some((W * H * 5) as f64));
        assert_eq!(m.update(&v, 0.0).level, PatternLevel::Pass);
        // Too few pairs: wide stripes over a small screen.
        let few = frame(96, 96, |x, _| if (x / 16) % 2 == 0 { 0 } else { 255 });
        let r = level(&few);
        assert!(r.pairs <= MIN_PAIRS, "{r:?}");
        assert_eq!(r.level, PatternLevel::Pass);
    }

    #[test]
    fn a_pattern_fails_after_half_a_second_and_moving_is_seen() {
        let still = frame(W, H, |x, _| if (x / 8) % 2 == 0 { 0 } else { 255 });
        let mut m = PatternMeter::new(None);
        assert_eq!(m.update(&still, 0.0).level, PatternLevel::Harmful);
        assert_eq!(m.update(&still, 0.3).level, PatternLevel::Harmful);
        let r = m.update(&still, 0.5);
        assert_eq!(r.level, PatternLevel::Fail);
        assert!(!r.changing);
        let strong = m
            .found()
            .tiles
            .iter()
            .filter(|t| t.is_some_and(|(c, _)| c > 0.9));
        assert!(strong.count() * 2 > m.found().tiles.len());
        // Moved by half a period: reversed.
        let moved = frame(W, H, |x, _| if ((x + 4) / 8) % 2 == 0 { 0 } else { 255 });
        assert!(m.update(&moved, 0.6).changing);
        // Gone: the clock starts again.
        let plain = frame(W, H, |_, _| 40);
        assert_eq!(m.update(&plain, 0.7).level, PatternLevel::Pass);
        assert!(m.found().tiles.iter().all(|t| t.is_none()));
        assert_eq!(m.update(&still, 0.8).level, PatternLevel::Harmful);
    }

    #[test]
    fn a_dense_screen_starts_a_scale_coarser() {
        let v4 = frame(W, H, |x, _| if (x / 2) % 2 == 0 { 0 } else { 255 });
        let v16 = frame(W, H, |x, _| if (x / 8) % 2 == 0 { 0 } else { 255 });
        let mut fine = PatternMeter::for_viewing(crate::strobe::Viewing::screen(1.0), None);
        let mut dense = PatternMeter::for_viewing(crate::strobe::Viewing::screen(2.0), None);
        assert_eq!(fine.update(&v4, 0.0).level, PatternLevel::Harmful);
        assert_eq!(dense.update(&v4, 0.0).level, PatternLevel::Pass);
        assert_eq!(dense.update(&v16, 0.1).level, PatternLevel::Harmful);
    }

    #[test]
    fn stripes_finer_than_the_half_image_blur_away() {
        // One-pixel stripes average to grey in every 2 by 2 block.
        let fine = frame(W, H, |x, _| if x % 2 == 0 { 0 } else { 255 });
        assert_eq!(level(&fine).level, PatternLevel::Pass);
    }
}
