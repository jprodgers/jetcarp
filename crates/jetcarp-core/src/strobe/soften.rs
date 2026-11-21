//! Softening: what the strobe guard does to frames before they are shown, as a [`Plan`] it makes
//! from its readings, and the processor's way of carrying a plan out ([`Softener`]). The GPU's
//! (`jetcarp_display::strobe_gpu`) does exactly the same arithmetic in one compute pass; this one
//! serves frames drawn on the processor (exported animations, the app's small previews) and is
//! what the tests check against.
//!
//! **Flashes.** Each tile the plan marks may change its mean luminance by at most
//! [`Plan::lum_rate`] a second, and its red measure by at most [`Plan::red_rate`]: the frame shown
//! is the last frame shown moved towards the new one by the fraction `beta` of the way that keeps
//! the tile's mean within that step. A transition needs a change of 0.1 in one direction, so at
//! the default rate ([`LUM_RATE`]) a softened tile can make at most three transitions a second, half
//! the limit, whatever is drawn; a flip becomes a fade. Tiles the plan leaves alone are shown as
//! drawn. The last frame shown is kept in linear light at 16 bits a channel, so that a slow fade
//! does not stall on 8-bit steps. While nothing is softened only each tile's mean is kept, so the
//! first softened frame moves from a grey at that mean: even that frame is within the step.
//!
//! **Stripes.** A plan can also *veil* tiles: blend them towards a grey of a given luminance, which
//! lowers the contrast of strong stripes there. The amount and the grey are interpolated between
//! tile centres, so a veil has no hard edges.

use super::frame::{TileMap, tiles_for};
use super::{TILE, encode, linear, luminance_linear, red_measure_linear};

/// The most a softened tile's mean luminance changes in a second: three transitions of 0.1 at
/// most, half of what fails.
pub const LUM_RATE: f32 = 0.3;

/// The same for the red measure (a red transition is a change of 20).
pub const RED_RATE: f32 = 60.0;

/// Frames further apart than this are softened as if they were this far apart (a frame shown
/// after a long pause may change by no more than a tenth of a second's worth).
pub const MAX_DT: f32 = 0.1;

/// What the guard asks of the frames drawn next.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Tiles across and down, as the frames' [`TileMap`]s have them.
    pub cols: u32,
    pub rows: u32,
    /// Each tile whose brightness change is limited.
    pub soften: Vec<bool>,
    /// Each tile's veil: how far towards grey (0: none, 1: all grey), and the grey's luminance.
    pub veil: Vec<[f32; 2]>,
    /// The most a softened tile's mean luminance may change in a second.
    pub lum_rate: f32,
    /// The most its red measure may change in a second.
    pub red_rate: f32,
}

impl Default for Plan {
    fn default() -> Self {
        Plan::none(0, 0)
    }
}

impl Plan {
    /// A plan that leaves every tile of a `cols` by `rows` map alone.
    pub fn none(cols: u32, rows: u32) -> Plan {
        let n = (cols * rows) as usize;
        Plan {
            cols,
            rows,
            soften: vec![false; n],
            veil: vec![[0.0, 0.0]; n],
            lum_rate: LUM_RATE,
            red_rate: RED_RATE,
        }
    }

    /// The same for a frame of this size.
    pub fn none_for(width: u32, height: u32) -> Plan {
        let (c, r) = tiles_for(width, height);
        Plan::none(c, r)
    }

    /// Whether it changes anything.
    pub fn active(&self) -> bool {
        self.soften.iter().any(|&s| s) || self.veil.iter().any(|v| v[0] > 0.0)
    }

    /// Whether it is for frames with this many tiles.
    pub fn fits(&self, cols: u32, rows: u32) -> bool {
        self.cols == cols && self.rows == rows
    }

    /// The share of tiles softened or veiled.
    pub fn share(&self) -> f32 {
        if self.soften.is_empty() {
            return 0.0;
        }
        let n = self
            .soften
            .iter()
            .zip(&self.veil)
            .filter(|(s, v)| **s || v[0] > 0.0)
            .count();
        n as f32 / self.soften.len() as f32
    }

    /// The most a softened tile may change this frame, `dt` seconds after the last: luminance,
    /// and the red measure.
    pub fn steps(&self, dt: f32) -> (f32, f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, MAX_DT)
        } else {
            MAX_DT
        };
        (self.lum_rate * dt, self.red_rate * dt)
    }

    /// The veil at pixel `(x, y)`: the amount, and the amount times the grey, each interpolated
    /// between tile centres (clamped at the edges) as the GPU's linear sampler gives them. A pixel
    /// `c` is shown as `c * (1 - amount) + amount_grey`: interpolating the grey already weighted
    /// by its amount keeps the edge of a veil from darkening.
    pub fn veil_at(&self, x: u32, y: u32) -> [f32; 2] {
        if self.cols == 0 || self.rows == 0 {
            return [0.0, 0.0];
        }
        let t = TILE as f32;
        let u = ((x as f32 + 0.5) / t - 0.5).clamp(0.0, (self.cols - 1) as f32);
        let v = ((y as f32 + 0.5) / t - 0.5).clamp(0.0, (self.rows - 1) as f32);
        let (c0, r0) = (u.floor() as u32, v.floor() as u32);
        let (c1, r1) = ((c0 + 1).min(self.cols - 1), (r0 + 1).min(self.rows - 1));
        let (fu, fv) = (u - c0 as f32, v - r0 as f32);
        let at = |c: u32, r: u32| {
            let [a, g] = self.veil[(r * self.cols + c) as usize];
            [a, a * g]
        };
        let (a, b, c, d) = (at(c0, r0), at(c1, r0), at(c0, r1), at(c1, r1));
        let mut out = [0f32; 2];
        for k in 0..2 {
            let top = a[k] + (b[k] - a[k]) * fu;
            let bottom = c[k] + (d[k] - c[k]) * fu;
            out[k] = top + (bottom - top) * fv;
        }
        out
    }
}

/// A frame through the softener: what to show, and the tile maps of what was drawn and of what
/// is shown.
#[derive(Clone, Debug)]
pub struct Softened {
    /// The frame to show (RGBA, sRGB), or None when the plan changed nothing: show the frame as
    /// drawn.
    pub shown: Option<Vec<u8>>,
    /// The frame as drawn.
    pub input: TileMap,
    /// The frame as shown.
    pub output: TileMap,
}

/// The processor's softener: keeps the last frame shown, in linear light.
#[derive(Clone, Debug, Default)]
pub struct Softener {
    width: u32,
    height: u32,
    /// The last frame shown before any veil, linear light, 16 bits a channel.
    state: Vec<[u16; 3]>,
    /// Each tile's mean luminance and red measure in the last frame shown (kept whether or not
    /// it was softened).
    prev: Vec<[f32; 2]>,
    /// `prev` is for frames of this size.
    prev_valid: bool,
    /// `state` holds nothing: the last frame was not softened.
    empty: bool,
}

fn unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

fn from16(v: u16) -> f32 {
    v as f32 / 65535.0
}

/// sRGB bytes of every 16-bit linear value (65,536 of them): how the softener writes the frame
/// it shows, on the processor and on the GPU alike.
pub fn encode_table() -> &'static [u8] {
    static T: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        (0..=65535u32)
            .map(|i| (encode(i as f32 / 65535.0) * 255.0 + 0.5) as u8)
            .collect()
    })
}

impl Softener {
    pub fn new() -> Softener {
        Softener {
            empty: true,
            ..Default::default()
        }
    }

    /// Forget the last frame shown: the next is shown as drawn.
    pub fn reset(&mut self) {
        self.empty = true;
        self.prev_valid = false;
    }

    /// Carries out `plan` on `rgba` (`width` by `height`, sRGB), drawn `dt` seconds after the last
    /// frame.
    pub fn apply(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        plan: &Plan,
        dt: f32,
    ) -> Softened {
        let input = super::frame::reduce(rgba, width, height);
        let n = (width * height) as usize;
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.empty = true;
            self.prev_valid = false;
        }
        let active = plan.active() && plan.fits(input.cols, input.rows);
        if !active {
            // Nothing to soften: remember the tiles shown, so that softening can start from them.
            self.prev = input
                .lum
                .iter()
                .zip(&input.red)
                .map(|(l, r)| [*l, *r])
                .collect();
            self.prev_valid = true;
            self.empty = true;
            return Softened {
                shown: None,
                output: input.clone(),
                input,
            };
        }
        // The first softened frame has no last frame to move from, only its tiles' means: it
        // moves from a grey at each tile's last mean, so that even that frame is within the step.
        let fresh = self.empty || self.state.len() != n;
        if fresh {
            self.state = vec![[0; 3]; n];
        }
        if self.prev.len() != input.len() {
            self.prev = vec![[0.0; 2]; input.len()];
            self.prev_valid = false;
        }
        let (lum_step, red_step) = plan.steps(dt);
        // Each tile's fraction of the way to the new frame.
        let beta: Vec<f32> = (0..input.len())
            .map(|t| {
                if !self.prev_valid || !plan.soften[t] {
                    return 1.0;
                }
                let (dl, dr) = (
                    (input.lum[t] - self.prev[t][0]).abs(),
                    (input.red[t] - self.prev[t][1]).abs(),
                );
                let mut b = 1f32;
                if dl > lum_step {
                    b = b.min(lum_step / dl);
                }
                if dr > red_step {
                    b = b.min(red_step / dr);
                }
                b
            })
            .collect();
        let cols = input.cols as usize;
        let mut ema = vec![[0f32; 2]; input.len()];
        let mut out_sum = vec![[0f32; 2]; input.len()];
        let mut shown = vec![0u8; n * 4];
        let table = encode_table();
        for y in 0..height as usize {
            let tr = y / TILE as usize;
            for x in 0..width as usize {
                let i = y * width as usize + x;
                let t = tr * cols + x / TILE as usize;
                let px = &rgba[i * 4..i * 4 + 3];
                let new = [linear(px[0]), linear(px[1]), linear(px[2])];
                let b = beta[t];
                let s = if b < 1.0 {
                    let was = if fresh {
                        [self.prev[t][0]; 3]
                    } else {
                        self.state[i].map(from16)
                    };
                    [0, 1, 2].map(|k| was[k] + (new[k] - was[k]) * b)
                } else {
                    new
                };
                let q = s.map(unorm16);
                self.state[i] = q;
                // Sums of what is kept, as the GPU keeps it.
                let s = q.map(from16);
                ema[t][0] += luminance_linear(s[0], s[1], s[2]);
                ema[t][1] += red_measure_linear(s[0], s[1], s[2]);
                let [amount, grey] = plan.veil_at(x as u32, y as u32);
                let o = s.map(|c| c * (1.0 - amount) + grey);
                out_sum[t][0] += luminance_linear(o[0], o[1], o[2]);
                out_sum[t][1] += red_measure_linear(o[0], o[1], o[2]);
                for k in 0..3 {
                    shown[i * 4 + k] = table[unorm16(o[k]) as usize];
                }
                shown[i * 4 + 3] = 255;
            }
        }
        let mut output = input.clone();
        for t in 0..input.len() {
            let px = input.tile_px(t);
            self.prev[t] = [ema[t][0] / px, ema[t][1] / px];
            output.lum[t] = out_sum[t][0] / px;
            output.red[t] = out_sum[t][1] / px;
        }
        self.empty = false;
        self.prev_valid = true;
        Softened {
            shown: Some(shown),
            input,
            output,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strobe::flash::{FlashMeter, Level};
    use crate::strobe::{FLASH_DELTA, Viewing};

    fn solid(w: u32, h: u32, c: u8) -> Vec<u8> {
        let mut v = vec![c; (w * h * 4) as usize];
        v.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
        v
    }

    #[test]
    fn a_plan_that_does_nothing_shows_the_frame_as_drawn() {
        let mut s = Softener::new();
        let f = solid(40, 20, 90);
        let r = s.apply(&f, 40, 20, &Plan::none_for(40, 20), 1.0 / 60.0);
        assert!(r.shown.is_none());
        assert_eq!(r.input, r.output);
        // A plan for another size is not carried out.
        let mut p = Plan::none(1, 1);
        p.soften[0] = true;
        assert!(s.apply(&f, 40, 20, &p, 0.016).shown.is_none());
    }

    /// The whole frame flipping black and white every frame, softened: what is shown never
    /// makes more than three transitions a second, at any frame rate, and a softened frame that
    /// does not change is shown unchanged.
    #[test]
    fn softening_turns_a_flip_into_a_fade_under_the_limit() {
        let (w, h) = (64, 48);
        let (black, white) = (solid(w, h, 0), solid(w, h, 255));
        for fps in [30.0f32, 60.0, 120.0, 144.0] {
            let mut plan = Plan::none_for(w, h);
            plan.soften.fill(true);
            let mut s = Softener::new();
            let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
            let mut most = 0;
            for f in 0..(fps as u32 * 4) {
                let frame = if f % 2 == 0 { &black } else { &white };
                let r = s.apply(frame, w, h, &plan, 1.0 / fps);
                let read = meter.update(&r.output, f as f64 / fps as f64);
                most = most.max(read.transitions);
                assert!(
                    read.level < Level::Warning,
                    "{fps} fps, frame {f}: {read:?}"
                );
            }
            assert!(most <= 3, "{fps} fps: {most}");
        }
        // Black then white for good: a fade of 0.3 a second, reaching white in a few seconds.
        let mut plan = Plan::none_for(w, h);
        plan.soften.fill(true);
        let mut s = Softener::new();
        s.apply(&black, w, h, &plan, 1.0 / 60.0);
        let mut last = 0.0;
        for k in 1..=60 {
            let r = s.apply(&white, w, h, &plan, 1.0 / 60.0);
            let l = r.output.lum[0];
            assert!(
                l - last <= 0.3 / 60.0 + 1e-4 && l > last,
                "frame {k}: {l} after {last}"
            );
            last = l;
        }
        assert!((last - 0.3).abs() < 0.01, "{last}");
    }

    #[test]
    fn only_the_tiles_marked_are_softened() {
        let (w, h) = (32, 16);
        let mut plan = Plan::none_for(w, h);
        plan.soften[0] = true;
        let mut s = Softener::new();
        s.apply(&solid(w, h, 0), w, h, &plan, 0.016);
        let r = s.apply(&solid(w, h, 255), w, h, &plan, 0.016);
        let out = r.shown.unwrap();
        // The left tile moved a little; the right tile is white.
        assert!(out[0] < 40, "{}", out[0]);
        assert_eq!(out[(16 * 4) as usize], 255);
        assert!(r.output.lum[0] < FLASH_DELTA);
        assert!((r.output.lum[1] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_veil_greys_stripes_smoothly() {
        let (w, h) = (64, 32);
        // Black and white columns.
        let mut f = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let c = if (x / 2) % 2 == 0 { 0 } else { 255 };
                f[i..i + 4].copy_from_slice(&[c, c, c, 255]);
            }
        }
        let mut plan = Plan::none_for(w, h);
        plan.veil.fill([0.9, 0.5]);
        let mut s = Softener::new();
        let r = s.apply(&f, w, h, &plan, 0.016);
        let out = r.shown.unwrap();
        // Black becomes 0.45 light, white 0.55: a tenth of the contrast.
        let l = |i: usize| crate::strobe::luminance([out[i], out[i + 1], out[i + 2]]);
        assert!((l(0) - 0.45).abs() < 0.01, "{}", l(0));
        assert!((l(8) - 0.55).abs() < 0.01, "{}", l(8));
        // The tile means are unchanged by a veil towards their own mean.
        for t in 0..r.output.len() {
            assert!((r.output.lum[t] - r.input.lum[t]).abs() < 0.01);
        }
        // Between a veiled tile and a clear one, the veil fades across the tile centres.
        let mut plan = Plan::none_for(w, h);
        plan.veil[0] = [1.0, 0.5];
        assert_eq!(plan.veil_at(0, 0), [1.0, 0.5]);
        assert_eq!(plan.veil_at(7, 0), [1.0, 0.5]);
        // Half a pixel past the middle of the way to the next tile's centre.
        let mid = plan.veil_at(16, 0);
        let f = 1.0 - 8.5 / 16.0;
        assert!(
            (mid[0] - f).abs() < 1e-6 && (mid[1] - 0.5 * f).abs() < 1e-6,
            "{mid:?}"
        );
        assert_eq!(plan.veil_at(30, 20), [0.0, 0.0]);
    }

    #[test]
    fn the_step_follows_the_time_between_frames() {
        let p = Plan::none(1, 1);
        assert_eq!(p.steps(0.5), (LUM_RATE * MAX_DT, RED_RATE * MAX_DT));
        let (l, r) = p.steps(1.0 / 60.0);
        assert!((l - 0.005).abs() < 1e-7 && (r - 1.0).abs() < 1e-5);
        assert_eq!(p.steps(f32::NAN), p.steps(1.0));
        assert_eq!(p.steps(-1.0), (0.0, 0.0));
    }
}
