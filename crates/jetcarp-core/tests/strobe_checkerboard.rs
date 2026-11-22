//! A reversing checkerboard of squares smaller than the strobe guard's tiles (#269): WCAG 2.3.1
//! counts it as flashing unless its squares are under 0.1° of the visual field, under 3 pixels at
//! the distance the guard assumes (`Viewing::screen(1.0)`: a 10° field 265 pixels across, so 0.1°
//! is 2.65 pixels). A tile's mean does not change when such a board reverses, so the guard must see
//! it another way.
//!
//! Every frame here is drawn and measured in memory: nothing is shown on a screen.
//!
//! - **The meter** must count each reversal of a board of squares from 3 to 32 pixels, at any
//!   offset from the tiles, reversing 4 to 10 times a second, over an area larger than WCAG's share
//!   of a 10° field (a quarter of it) and well under a quarter of the screen (where the stripe
//!   check would veil it anyway). Squares of 1 and 2 pixels stay exempt, as WCAG says.
//! - **The guard** must keep what is shown under the limit, measured pixel by pixel, as WCAG
//!   defines a flash, by a meter that knows nothing of tiles.

use jetcarp_core::strobe::flash::{COUNT_CAP, EPSILON, FlashMeter, FlashReading, Level, Tracker};
use jetcarp_core::strobe::frame::{TileMap, reduce};
use jetcarp_core::strobe::guard::{Frame, Guard, Pace};
use jetcarp_core::strobe::soften::Softener;
use jetcarp_core::strobe::{DARK_LIMIT, FLASH_DELTA, Viewing, linear, luminance_linear};
use std::collections::VecDeque;

/// Half a 1080p screen, so a quarter of the screen (129,600 pixels) is far more than a quarter of
/// the 10° field (about 13,700).
const W: u32 = 960;
const H: u32 = 540;

/// The board: 176 pixels square, 31,000 pixels, 6% of the screen and over twice WCAG's share of a
/// 10° field.
const SIDE: u32 = 176;
const X0: u32 = 300;
const Y0: u32 = 180;

/// A frame of `w` by `h` with a board of `sq`-pixel squares over `[x0, x0 + side)` and the same
/// down, its squares offset `off` pixels from the frame's grid, in phase `phase`, light `hi` and
/// dark `lo`, on a ground of `bg`.
#[allow(clippy::too_many_arguments)]
fn board(
    w: u32,
    h: u32,
    [x0, y0, side]: [u32; 3],
    sq: u32,
    off: [u32; 2],
    phase: u32,
    [lo, hi, bg]: [u8; 3],
) -> Vec<u8> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let inside = x >= x0 && x < x0 + side && y >= y0 && y < y0 + side;
            let c = if !inside {
                bg
            } else if ((x + off[0]) / sq + (y + off[1]) / sq + phase) % 2 == 1 {
                hi
            } else {
                lo
            };
            v.extend_from_slice(&[c, c, c, 255]);
        }
    }
    v
}

/// The two phases of the board, as tile maps.
fn phases(sq: u32, off: [u32; 2], colours: [u8; 3]) -> [TileMap; 2] {
    [0, 1].map(|p| {
        reduce(
            &board(W, H, [X0, Y0, SIDE], sq, off, p, colours),
            W,
            H,
        )
    })
}

/// The worst reading of `secs` seconds at 60 frames a second, the board reversing `rate` times a
/// second.
fn run(maps: &[TileMap; 2], rate: u32, secs: f64) -> FlashReading {
    let mut m = FlashMeter::new(Viewing::screen(1.0), None);
    let every = 60 / rate;
    let mut worst = FlashReading::default();
    for f in 0..(60.0 * secs) as u32 {
        let r = m.update(&maps[((f / every) % 2) as usize], f as f64 / 60.0);
        if (r.level, r.transitions) > (worst.level, worst.transitions) {
            worst = r;
        }
    }
    worst
}

const BW: [u8; 3] = [0, 255, 0];

/// The level `rate` reversals a second should read for 3 seconds: 4 to 6 transitions (two or
/// three flashes) are a warning, more fail.
fn level_for(rate: u32) -> Level {
    if rate > 6 { Level::Fail } else { Level::Warning }
}

#[test]
fn a_small_reversing_checkerboard_flashes() {
    // The area: more than WCAG's share of a 10° field, much less than a quarter of the screen.
    let v = Viewing::screen(1.0);
    assert!((SIDE * SIDE) as f64 > 2.0 * v.flash_area());
    assert!(((SIDE * SIDE) as f64) < 0.1 * (W * H) as f64);
    let mut missed = vec![];
    // Squares from just over 0.1° to twice a tile, either side of the tile's 16 pixels, at offsets
    // from the tiles' grid that line the squares up with it, put them half a square off, and at
    // odd pixels.
    for sq in [3u32, 4, 5, 6, 8, 10, 12, 15, 16, 17, 24, 32] {
        for off in [[0, 0], [1, 2], [sq / 2, sq / 2], [3, 1], [8, 8]] {
            let maps = phases(sq, off, BW);
            for rate in [4u32, 6, 10] {
                let r = run(&maps, rate, 3.0);
                let want = rate.min(COUNT_CAP);
                if r.transitions < want || r.level != level_for(rate) {
                    missed.push(format!(
                        "{sq}-pixel squares, offset {off:?}, {rate} reversals a second: {} \
                         transitions, {:?} (area {}, screen {})",
                        r.transitions, r.level, r.area_transitions, r.screen_transitions
                    ));
                }
            }
        }
    }
    assert!(
        missed.is_empty(),
        "reversing checkerboards not counted:\n{}",
        missed.join("\n")
    );
}

/// Lower contrast, grey squares on grey, still a flash: a change of 0.1 or more, the darker state
/// under 0.8. And one just under 0.1 is not.
#[test]
fn a_grey_checkerboard_flashes_by_its_contrast() {
    let lum = |b: u8| luminance_linear(linear(b), linear(b), linear(b));
    let lo = 90u8;
    let over = (lo..=255).find(|&g| lum(g) - lum(lo) >= FLASH_DELTA + 0.01).unwrap();
    let under = (lo..=255)
        .rev()
        .find(|&g| lum(g) - lum(lo) < FLASH_DELTA - 0.01)
        .unwrap();
    for (hi, flashes) in [(over, true), (under, false)] {
        for sq in [4u32, 8] {
            let maps = phases(sq, [1, 1], [lo, hi, lo]);
            let r = run(&maps, 10, 2.0);
            assert_eq!(
                r.level.fails(),
                flashes,
                "grey {lo} and {hi} ({:.3} apart), {sq}-pixel squares: {r:?}",
                lum(hi) - lum(lo)
            );
        }
    }
    // Light squares on lighter ones, the darker at 0.8 or more: never a flash.
    let maps = phases(6, [0, 0], [235, 255, 235]);
    assert!(lum(235) >= DARK_LIMIT);
    assert_eq!(run(&maps, 10, 2.0).transitions, 0);
}

/// Squares under 0.1° (1 and 2 pixels at the distance the guard assumes) are exempt, as WCAG
/// says: a fine, balanced pattern, at any offset.
#[test]
fn squares_under_a_tenth_of_a_degree_are_exempt() {
    assert!(2.0 < Viewing::screen(1.0).field_px / 100.0);
    for sq in [1u32, 2] {
        for off in [[0, 0], [1, 0], [0, 1], [1, 1]] {
            let maps = phases(sq, off, BW);
            let r = run(&maps, 10, 2.0);
            assert_eq!(
                (r.transitions, r.level),
                (0, Level::Pass),
                "{sq}-pixel squares, offset {off:?}: {r:?}"
            );
        }
    }
}

/// Each pixel's own transitions in the last second, and how much of the screen made at least `k`:
/// WCAG's definition with no tiles, for what is shown (the board is smaller than the 10° field,
/// so all of it is in one field).
struct PixelMeter {
    px: Vec<Tracker>,
    w: u32,
    rect: [u32; 4],
}

impl PixelMeter {
    fn new(w: u32, rect: [u32; 4]) -> PixelMeter {
        let n = ((rect[2] - rect[0]) * (rect[3] - rect[1])) as usize;
        PixelMeter {
            px: vec![Tracker::default(); n],
            w,
            rect,
        }
    }

    /// The most transitions in the last second that more than a quarter of a 10° field made.
    fn update(&mut self, rgba: &[u8], t: f64) -> u32 {
        let [x0, y0, x1, _] = self.rect;
        let rw = x1 - x0;
        for (i, tr) in self.px.iter_mut().enumerate() {
            let (x, y) = (x0 + i as u32 % rw, y0 + i as u32 / rw);
            let p = &rgba[((y * self.w + x) * 4) as usize..];
            let l = luminance_linear(linear(p[0]), linear(p[1]), linear(p[2]));
            tr.update(l, t, FLASH_DELTA, EPSILON, Some(DARK_LIMIT));
        }
        let allowed = Viewing::screen(1.0).flash_area();
        let mut at_least = [0usize; COUNT_CAP as usize + 2];
        for p in &self.px {
            at_least[p.count(t) as usize] += 1;
        }
        for k in (0..=COUNT_CAP as usize).rev() {
            at_least[k] += at_least[k + 1];
        }
        (1..=COUNT_CAP)
            .rev()
            .find(|&k| at_least[k as usize] as f64 > allowed)
            .unwrap_or(0)
    }
}

/// The board through the guard as a screen shows it: drawn at 60 frames a second, reversing every
/// generation, its generations held back by the guard's pace cap; softened by the processor's
/// softener as the guard's plan says, read back two frames late. What is shown, measured pixel by
/// pixel, never fails, and it is the guard that keeps it so: unguarded, the same board fails.
#[test]
fn the_guard_keeps_a_small_reversing_checkerboard_under_the_limit() {
    let (w, h) = (640u32, 480u32);
    let at = [240, 160, 160];
    let rect = [at[0] - 8, at[1] - 8, at[0] + at[2] + 8, at[1] + at[2] + 8];
    assert!((at[2] * at[2]) as f64 > Viewing::screen(1.0).flash_area());
    for (sq, speed) in [(4u32, 10.0f64), (6, 8.0), (8, 30.0), (12, 7.0)] {
        let frames = [0, 1].map(|p| board(w, h, at, sq, [1, 3], p, BW));
        // Unguarded, what is drawn fails.
        let mut raw = PixelMeter::new(w, rect);
        let mut most_raw = 0;
        for k in 0..120u32 {
            let g = (k as f64 * speed / 60.0 + 1e-9).floor() as usize;
            most_raw = most_raw.max(raw.update(&frames[g % 2], k as f64 / 60.0));
        }
        assert!(most_raw > 6, "{sq}-pixel squares unguarded: {most_raw}");
        // Guarded.
        let mut guard = Guard::new(Viewing::screen(1.0), None, true);
        let mut soft = Softener::new();
        let mut shown = PixelMeter::new(w, rect);
        let mut queue = VecDeque::new();
        let (mut due, mut gen_no, mut most) = (0.0f64, 0u64, 0);
        for k in 0..(60 * 8) as u32 {
            let t = k as f64 / 60.0;
            let pace = guard.cap().map_or(speed, |c| (c as f64).min(speed));
            if k > 0 && guard.paused().is_none() {
                due += pace / 60.0;
                gen_no = (due + 1e-9).floor() as u64;
            }
            let px = &frames[(gen_no % 2) as usize];
            let s = soft.apply(px, w, h, &guard.plan().clone(), 1.0 / 60.0);
            most = most.max(shown.update(s.shown.as_deref().unwrap_or(px), t));
            queue.push_back((t, s.input, s.output));
            while queue.len() > 2 {
                let (t, i, o) = queue.pop_front().unwrap();
                guard.observe(
                    Frame {
                        time: t,
                        input: &i,
                        output: &o,
                        half: None,
                    },
                    Pace {
                        speed: speed as f32,
                        reached: Some(pace as f32),
                        frame_rate: 60.0,
                    },
                );
            }
        }
        assert!(
            most <= 6,
            "{sq}-pixel squares at {speed} generations a second: {most} transitions a second \
             shown ({:?})",
            guard.report()
        );
    }
}
