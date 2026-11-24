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

use jetcarp_core::render::{ViewOptions, render_view};
use jetcarp_core::strobe::flash::{COUNT_CAP, EPSILON, FlashMeter, FlashReading, Level, Tracker};
use jetcarp_core::strobe::frame::{TileMap, reduce};
use jetcarp_core::strobe::guard::{Frame, Guard, Pace};
use jetcarp_core::strobe::soften::Softener;
use jetcarp_core::strobe::{DARK_LIMIT, FLASH_DELTA, Viewing, linear, luminance_linear};
use jetcarp_core::{Scene, World};
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
    [0, 1].map(|p| reduce(&board(W, H, [X0, Y0, SIDE], sq, off, p, colours), W, H))
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
    if rate > 6 {
        Level::Fail
    } else {
        Level::Warning
    }
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
    let over = (lo..=255)
        .find(|&g| lum(g) - lum(lo) >= FLASH_DELTA + 0.01)
        .unwrap();
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
        eprintln!(
            "{sq}-pixel squares at {speed} gen/s: unguarded {most_raw}, guarded {most} \
             transitions a second, pixel by pixel; {:?}, cap {:?}",
            guard.report().action,
            guard.cap()
        );
        // Under the limit with room: two flashes a second at most, before the pace cap holds.
        assert!(
            most <= 4,
            "{sq}-pixel squares at {speed} generations a second: {most} transitions a second \
             shown ({:?})",
            guard.report()
        );
    }
}

/// An ordinary scene: its name, its world, its generations a second, and how many pixels a cell.
struct Ordinary {
    name: String,
    world: World,
    speed: f64,
    cell_px: f64,
}

/// A square world `w` by `h` cells, wrapping, of `rule` with `seed` (a seed's JSON).
fn world(w: u32, h: u32, rule: &str, seed: &str, rng: u64) -> World {
    let json = format!(
        r#"{{"name": "ordinary", "tiling": {{"family": "square", "size": {{"rect": {{"w": {w}, "h": {h}}}}}, "boundary": "wrap"}}, "rng_state": {rng}, "cas": [{{"rule": "{rule}", "seed": {seed}}}]}}"#
    );
    let scene: Scene = serde_json::from_str(&json).unwrap();
    World::from_scene(&scene).unwrap()
}

/// Cells at the `(x, y)` offsets of `shape`, repeated every `every` cells over a `w` by `h` world.
fn tiled(w: u32, h: u32, every: u32, shape: &[(u32, u32)]) -> String {
    let mut cells = vec![];
    for by in (0..h.saturating_sub(every)).step_by(every as usize) {
        for bx in (0..w.saturating_sub(every)).step_by(every as usize) {
            for &(x, y) in shape {
                cells.push(format!("[{}, 1]", (by + y) * w + bx + x));
            }
        }
    }
    format!(r#"{{"cells": {{"cells": [{}]}}}}"#, cells.join(", "))
}

/// Ordinary scenes for a `vw` by `vh` view: Life soups at common cell sizes, at 30 generations a
/// second; a soup left to settle into its ash (still lifes and blinkers), slow and fast; and
/// fields of still lifes, of gliders, and of blinkers, slow and fast.
fn ordinary(vw: u32, vh: u32, sizes: &[f64]) -> Vec<Ordinary> {
    let mut out = vec![];
    let cells = |px: f64| {
        (
            (vw as f64 / px).ceil() as u32 + 2,
            (vh as f64 / px).ceil() as u32 + 2,
        )
    };
    for &px in sizes {
        let (w, h) = cells(px);
        out.push(Ordinary {
            name: format!("Life soup, {px} px a cell"),
            world: world(w, h, "B3/S23", r#"{"random": {"density": 0.35}}"#, 7),
            speed: 30.0,
            cell_px: px,
        });
    }
    let px = 6.0;
    let (w, h) = cells(px);
    let mut ash = world(w, h, "B3/S23", r#"{"random": {"density": 0.35}}"#, 11);
    for _ in 0..1500 {
        ash.step();
    }
    for speed in [4.0, 30.0] {
        out.push(Ordinary {
            name: format!("Life ash (still lifes, blinkers), {px} px a cell, {speed} gen/s"),
            world: ash.clone(),
            speed,
            cell_px: px,
        });
    }
    let block = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let blinker = [(0, 1), (1, 1), (2, 1)];
    for (what, shape, every, speed) in [
        ("blocks", &block[..], 4, 30.0),
        ("gliders", &glider[..], 8, 30.0),
        ("blinkers", &blinker[..], 5, 2.0),
        ("blinkers", &blinker[..], 5, 30.0),
    ] {
        out.push(Ordinary {
            name: format!("a field of {what}, {px} px a cell, {speed} gen/s"),
            world: world(w, h, "B3/S23", &tiled(w, h, every, shape), 1),
            speed,
            cell_px: px,
        });
    }
    out
}

/// The scene's frames, `secs` seconds at 60 frames a second.
fn frames(o: &Ordinary, vw: u32, vh: u32, secs: f64) -> Vec<Vec<u8>> {
    let mut world = o.world.clone();
    let b = world.topo.bounds;
    let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let start = world.generation;
    (0..(secs * 60.0) as u32)
        .map(|k| {
            let due = (k as f64 * o.speed / 60.0 + 1e-9).floor() as u64;
            while world.generation - start < due {
                world.step();
            }
            render_view(&world, vw, vh, center, o.cell_px, &ViewOptions::default()).rgba
        })
        .collect()
}

/// The worst reading of the maps, with the tiles' fine patterns and without them (the meter as it
/// was before #269), and the most transitions the fine patterns made over the area rule.
fn readings(maps: &[TileMap]) -> (FlashReading, FlashReading, u32) {
    let mut with = FlashMeter::new(Viewing::screen(1.0), None);
    let mut without = FlashMeter::new(Viewing::screen(1.0), None);
    let (mut a, mut b, mut fine) = (FlashReading::default(), FlashReading::default(), 0);
    let worse = |x: &FlashReading, y: &FlashReading| (y.worst(), y.most()) > (x.worst(), x.most());
    for (k, m) in maps.iter().enumerate() {
        let t = k as f64 / 60.0;
        let r = with.update(m, t);
        fine = fine.max(r.fine_transitions);
        if worse(&a, &r) {
            a = r;
        }
        let mut plain = m.clone();
        plain.fine.clear();
        plain.red_fine.clear();
        let r = without.update(&plain, t);
        if worse(&b, &r) {
            b = r;
        }
    }
    (a, b, fine)
}

/// Ordinary scenes do not flash by their fine patterns: a soup's cells change a few at a time,
/// still lifes and gliders keep their shapes, and a blinker's two phases are not each other's
/// negative. The meter reads every one as it did before #269, except a field of blinkers
/// flipping 30 times a second: since #275 the tiles' windows that flip back and forth count as
/// flashing (`strobe_period2.rs`), and over a field of small cells, a blinker every 5 of them
/// (16% of the area flipping, under WCAG's quarter), they count more than flips: stricter than
/// WCAG.
#[test]
fn ordinary_scenes_are_read_as_before() {
    let (vw, vh) = (320, 240);
    let mut moved = vec![];
    for o in ordinary(vw, vh, &[2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0])
        .into_iter()
        .filter(|o| !(o.name.contains("blinkers") && o.speed > 7.0))
    {
        let maps: Vec<TileMap> = frames(&o, vw, vh, 3.0)
            .iter()
            .map(|f| reduce(f, vw, vh))
            .collect();
        let (with, without, fine) = readings(&maps);
        if fine > 0 || (with.worst(), with.most()) != (without.worst(), without.most()) {
            moved.push(format!(
                "{}: {:?} ({} transitions, {fine} by fine patterns), before {:?} ({})",
                o.name,
                with.worst(),
                with.most(),
                without.worst(),
                without.most()
            ));
        }
    }
    assert!(moved.is_empty(), "readings moved:\n{}", moved.join("\n"));
}

/// The numbers behind the ordinary scenes, on half a 1080p screen (ignored; run with `--ignored
/// --nocapture`): each scene's worst reading with the fine patterns and without; and, by the
/// letter of WCAG, pixel by pixel (each pixel's own transitions: no tiles, no coherence, no
/// exemption for fine patterns), the most transitions in a second that more than a quarter of
/// some 10° field made. That last is where random soups are a grey area.
#[test]
#[ignore]
fn ordinary_scenes_by_the_numbers() {
    let (vw, vh) = (W, H);
    let side = Viewing::screen(1.0).field_area().sqrt().round() as usize;
    let allowed = Viewing::screen(1.0).flash_area();
    for o in ordinary(vw, vh, &[1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0]) {
        let fr = frames(&o, vw, vh, 4.0);
        let maps: Vec<TileMap> = fr.iter().map(|f| reduce(f, vw, vh)).collect();
        let (with, without, fine) = readings(&maps);
        // Pixel by pixel.
        let mut px = vec![Tracker::default(); (vw * vh) as usize];
        let mut letter = 0;
        let (w, h) = (vw as usize, vh as usize);
        for (k, f) in fr.iter().enumerate() {
            let t = k as f64 / 60.0;
            for (i, p) in px.iter_mut().enumerate() {
                let c = &f[i * 4..i * 4 + 3];
                let l = luminance_linear(linear(c[0]), linear(c[1]), linear(c[2]));
                p.update(l, t, FLASH_DELTA, EPSILON, Some(DARK_LIMIT));
            }
            if k % 6 != 5 {
                continue;
            }
            let counts: Vec<u32> = px.iter().map(|p| p.count(t)).collect();
            for kk in (letter + 1)..=COUNT_CAP {
                // The most pixels with at least `kk` in any field-sized square, every 4 pixels.
                let mut sat = vec![0u32; (w + 1) * (h + 1)];
                for y in 0..h {
                    let mut run = 0;
                    for x in 0..w {
                        run += (counts[y * w + x] >= kk) as u32;
                        sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + run;
                    }
                }
                let s = side.min(w).min(h);
                let at = |x: usize, y: usize| sat[y * (w + 1) + x];
                let mut most = 0;
                for y0 in (0..=h - s).step_by(4) {
                    for x0 in (0..=w - s).step_by(4) {
                        let v = at(x0 + s, y0 + s) + at(x0, y0) - at(x0, y0 + s) - at(x0 + s, y0);
                        most = most.max(v);
                    }
                }
                if most as f64 > allowed {
                    letter = kk;
                } else {
                    break;
                }
            }
        }
        println!(
            "{}: with fine patterns {:?} ({} transitions, {fine} by fine patterns); before {:?} \
             ({}); pixel by pixel {letter}",
            o.name,
            with.worst(),
            with.most(),
            without.worst(),
            without.most()
        );
    }
}
