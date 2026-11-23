//! Fields of period-2 oscillators whose two phases are not each other's negative (#275): blinkers,
//! toads, clocks and denser agars change only part of each tile, so a tile's mean may not move and
//! its fine pattern does not reverse. WCAG 2.3.1 counts such a field pixel by pixel: where more
//! than a quarter of a 10° field flips more than three times a second, it flashes, whether or not
//! any tile's mean changes.
//!
//! Every frame here is drawn and measured in memory, with JETCARP's own stepping and renderer:
//! nothing is shown on a screen.
//!
//! - **The reference** ([`Wcag`]) is WCAG 2.x's general flash and red flash, applied literally to
//!   each pixel: no tiles, no coherence, no exemption. Its field is WCAG's own estimate, a rectangle
//!   of 341 by 256 pixels at 1024 by 768, scaled to the 10° the strobe guard assumes
//!   (`Viewing::screen(1.0)`: 265 pixels across), so 265 by 199, landscape or portrait, anywhere in
//!   the frame; it flashes where more than a quarter of one is covered by pixels each of which made
//!   more than six transitions in the last second. Its colour arithmetic is its own, not the
//!   guard's.
//! - **The fields** are Life's (B3/S23) densest period-2 lattices of blinkers, toads, clocks and
//!   beacons on a torus, the densest period-2 agars a search of small tori found, a reversing one
//!   (Venetian blinds), and soups of JETCARP's named rules left to settle into their ash. Each is
//!   checked to be period 2 under JETCARP's own stepping before it is measured.
//! - **The pins** ([`the_densest_period_2_fields_are_pinned`]) keep the verdicts of the densest
//!   fields: the reference's, the guard's as drawn, and what the guard lets through to the screen.
//!
//! `study_table` (ignored; `-- --ignored --nocapture`) prints the study's table.

use jetcarp_core::render::{ViewOptions, render_view};
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::flash::{FlashMeter, FlashReading, Level};
use jetcarp_core::strobe::frame::{reduce, reduce_both};
use jetcarp_core::strobe::guard::{Frame, Guard, Pace};
use jetcarp_core::strobe::soften::Softener;
use jetcarp_core::{Scene, World};
use rayon::prelude::*;
use std::collections::VecDeque;

/// The view: two and a half 10° fields across, so the area rule's field fits anywhere in it.
const VW: u32 = 640;
const VH: u32 = 480;
const FPS: f64 = 60.0;

// ---------------------------------------------------------------------------------------------
// The reference: WCAG 2.x, pixel by pixel.
// ---------------------------------------------------------------------------------------------

/// An sRGB byte in linear light, by the sRGB standard (worked out here, not by the guard).
fn lin(b: u8) -> f64 {
    let c = b as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One pixel's values: relative luminance; WCAG 2.0's red measure (`(R - G - B) * 320` for a
/// saturated red, `R / (R + G + B) >= 0.8`, else 0); its CIE 1976 UCS chromaticity (None for black);
/// and whether it is a saturated red.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Values {
    lum: f32,
    red: f32,
    uv: Option<[f32; 2]>,
    sat: bool,
}

fn values(t: &[f64; 256], p: &[u8]) -> Values {
    let (r, g, b) = (t[p[0] as usize], t[p[1] as usize], t[p[2] as usize]);
    let s = r + g + b;
    let sat = s > 0.0 && r / s >= 0.8;
    let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
    let d = x + 15.0 * y + 3.0 * z;
    Values {
        lum: y as f32,
        red: if sat {
            ((r - g - b) * 320.0) as f32
        } else {
            0.0
        },
        uv: (d > 1e-9).then(|| [(4.0 * x / d) as f32, (9.0 * y / d) as f32]),
        sat,
    }
}

/// A run of change in one direction: a transition is a change of at least `delta` since the run
/// began (the darker end below `dark`, for luminance), counted once a run; a change of direction
/// begins a new run.
#[derive(Clone, Copy, Default)]
struct Run {
    primed: bool,
    start: f32,
    last: f32,
    dir: i8,
    counted: bool,
}

impl Run {
    fn update(&mut self, v: f32, delta: f32, dark: Option<f32>) -> bool {
        if !self.primed {
            *self = Run {
                primed: true,
                start: v,
                last: v,
                ..Default::default()
            };
            return false;
        }
        let d = v - self.last;
        self.last = v;
        if d == 0.0 {
            return false;
        }
        let dir = if d > 0.0 { 1 } else { -1 };
        if dir != self.dir {
            if self.dir != 0 {
                self.start = v - d;
            }
            self.dir = dir;
            self.counted = false;
        }
        if !self.counted
            && (v - self.start).abs() >= delta - 1e-6
            && dark.is_none_or(|l| self.start.min(v) < l)
        {
            self.counted = true;
            return true;
        }
        false
    }
}

/// WCAG 2.2's red transition: a move of more than 0.2 in chromaticity from where the run began,
/// to or from a saturated red; a run turns back when the colour comes back towards its start.
#[derive(Clone, Copy, Default)]
struct ChromaRun {
    start: Option<([f32; 2], bool)>,
    far: ([f32; 2], bool),
    far_d: f32,
    counted: bool,
}

impl ChromaRun {
    fn update(&mut self, v: &Values) -> bool {
        let Some(c) = v.uv else { return false };
        let now = (c, v.sat);
        let dist =
            |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let Some(start) = self.start else {
            self.start = Some(now);
            self.far = now;
            return false;
        };
        let d = dist(start.0, c);
        if d >= self.far_d {
            self.far = now;
            self.far_d = d;
        } else if self.far_d - d > 0.005 {
            self.start = Some(self.far);
            self.far = now;
            self.far_d = dist(self.start.unwrap().0, c);
            self.counted = false;
        }
        let s = self.start.unwrap();
        if !self.counted && self.far_d > 0.2 && (s.1 || self.far.1) {
            self.counted = true;
            return true;
        }
        false
    }
}

/// When the last few transitions were made.
#[derive(Clone, Copy, Default)]
struct Times {
    t: [f32; 8],
    n: u8,
}

impl Times {
    fn push(&mut self, t: f32) {
        if (self.n as usize) < self.t.len() {
            self.t[self.n as usize] = t;
            self.n += 1;
        } else {
            self.t.rotate_left(1);
            self.t[7] = t;
        }
    }
    /// Transitions after `t - 1` (a second up to `t`).
    fn count(&self, t: f32) -> u8 {
        self.t[..self.n as usize]
            .iter()
            .filter(|&&s| s > t - 1.0 + 1e-4)
            .count() as u8
    }
}

#[derive(Clone, Copy, Default)]
struct Pixel {
    lum: Run,
    red: Run,
    chroma: ChromaRun,
    lum_t: Times,
    red_t: Times,
}

/// One frame's reading by the reference.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Letter {
    /// The most transitions in the last second that more than a quarter of some field made,
    /// pixel by pixel (general flashes, and red).
    general: u8,
    red: u8,
    /// The largest share of a field covered by pixels with more than six transitions in the last
    /// second (what fails), general or red.
    fail_share: f32,
    /// The same with two or more (a flash a second).
    flash_share: f32,
    /// The most transitions in the last second that more than a quarter of some field made, among
    /// the pixels that made one in this frame: WCAG's "flashes occurring concurrently" read
    /// strictly (general or red). The verdict is the stricter reading above; for a period-2 field
    /// the two agree, since all of its flipping pixels flip together.
    concurrent: u8,
}

impl Letter {
    fn worse(self, o: Letter) -> Letter {
        Letter {
            general: self.general.max(o.general),
            red: self.red.max(o.red),
            fail_share: self.fail_share.max(o.fail_share),
            flash_share: self.flash_share.max(o.flash_share),
            concurrent: self.concurrent.max(o.concurrent),
        }
    }
    fn most(&self) -> u8 {
        self.general.max(self.red)
    }
    fn fails(&self) -> bool {
        self.most() > 6
    }
}

/// WCAG 2.x pixel by pixel over a `w` by `h` frame.
struct Wcag {
    w: usize,
    h: usize,
    lin: [f64; 256],
    px: Vec<Pixel>,
    /// The field's rectangle, landscape (and portrait, transposed).
    rect: (usize, usize),
}

impl Wcag {
    fn new(w: u32, h: u32) -> Wcag {
        Wcag {
            w: w as usize,
            h: h as usize,
            lin: std::array::from_fn(|i| lin(i as u8)),
            px: vec![Pixel::default(); (w * h) as usize],
            rect: field_rect(),
        }
    }

    /// Takes the frame shown at `t` (seconds).
    fn update(&mut self, rgba: &[u8], t: f64) -> Letter {
        let t = t as f32;
        let lin = self.lin;
        let mut lum_c = vec![0u8; self.px.len()];
        let mut red_c = vec![0u8; self.px.len()];
        let mut now_c = vec![0u8; self.px.len()];
        let (any_lum, any_red) = self
            .px
            .par_iter_mut()
            .zip(rgba.par_chunks_exact(4))
            .zip(lum_c.par_iter_mut().zip(red_c.par_iter_mut()))
            .zip(now_c.par_iter_mut())
            .map(|(((p, c), (lc, rc)), nc)| {
                let v = values(&lin, c);
                let l = p.lum.update(v.lum, 0.1, Some(0.8));
                if l {
                    p.lum_t.push(t);
                }
                // A red transition by either definition, counted once a frame.
                let r20 = p.red.update(v.red, 20.0, None);
                let r22 = p.chroma.update(&v);
                if r20 || r22 {
                    p.red_t.push(t);
                }
                *lc = p.lum_t.count(t);
                *rc = p.red_t.count(t);
                // Concurrent: the pixels that made a transition in this frame.
                *nc = (if l { *lc } else { 0 }).max(if r20 || r22 { *rc } else { 0 });
                (l, r20 || r22)
            })
            .reduce(|| (false, false), |a, b| (a.0 || b.0, a.1 || b.1));
        // Counts only fall between transitions, so the most is read where some pixel made one.
        let none = (0, 0.0, 0.0);
        let (general, gf, g2) = if any_lum { self.area(&lum_c) } else { none };
        let (red, rf, r2) = if any_red { self.area(&red_c) } else { none };
        let concurrent = if any_lum || any_red {
            self.area(&now_c).0
        } else {
            0
        };
        Letter {
            general,
            red,
            fail_share: gf.max(rf),
            flash_share: g2.max(r2),
            concurrent,
        }
    }

    /// The area rule over per-pixel counts: the largest `k` such that pixels with at least `k`
    /// transitions cover more than a quarter of some field; the largest share covered by pixels
    /// with seven or more, and with two or more.
    fn area(&self, counts: &[u8]) -> (u8, f32, f32) {
        let mut hist = [0usize; 9];
        for &c in counts {
            hist[c.min(8) as usize] += 1;
        }
        let (rw, rh) = self.rect;
        let field = (rw * rh) as f64;
        let share = |k: u8| -> f32 {
            let at_least: usize = hist[k as usize..].iter().sum();
            if at_least == 0 {
                return 0.0;
            }
            let (w, h) = (self.w, self.h);
            let mut sat = vec![0u32; (w + 1) * (h + 1)];
            for y in 0..h {
                let mut run = 0u32;
                for x in 0..w {
                    run += (counts[y * w + x] >= k) as u32;
                    sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + run;
                }
            }
            let at = |x: usize, y: usize| sat[y * (w + 1) + x] as i64;
            let mut most = 0i64;
            for (fw, fh) in [(rw, rh), (rh, rw)] {
                if fw > w || fh > h {
                    continue;
                }
                for y0 in 0..=h - fh {
                    for x0 in 0..=w - fw {
                        let v =
                            at(x0 + fw, y0 + fh) + at(x0, y0) - at(x0, y0 + fh) - at(x0 + fw, y0);
                        most = most.max(v);
                    }
                }
            }
            (most as f64 / field) as f32
        };
        let mut memo = [None::<f32>; 9];
        let mut get = |k: u8| -> f32 { *memo[k as usize].get_or_insert_with(|| share(k)) };
        // The share only falls as `k` rises, so the largest `k` over the limit is found by halving;
        // where too few pixels have `k` to cover a quarter of any field, there is no need to look.
        let (mut lo, mut hi) = (0u8, 8u8);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let enough = hist[mid as usize..].iter().sum::<usize>() as f64 > 0.25 * field;
            if enough && get(mid) as f64 > 0.25 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        (lo, get(7), get(2))
    }
}

/// WCAG's estimate of a 10° field, a rectangle of 341 by 256 pixels at 1024 by 768, scaled to the
/// 10° the guard assumes on a screen at a scale of 1 (265 pixels across).
fn field_rect() -> (usize, usize) {
    let s = Viewing::screen(1.0).field_px / 341.0;
    ((341.0 * s).round() as usize, (256.0 * s).round() as usize)
}

// ---------------------------------------------------------------------------------------------
// The fields.
// ---------------------------------------------------------------------------------------------

/// A period-2 pattern of Life, as one period of it on a torus `w` by `h` whose rows wrap `shift`
/// cells along (the row below the last is the first, `shift` cells to the right: the lattice of
/// `(w, 0)` and `(-shift, h)`): its live cells, row by row, `o` live and `.` dead, rows split by
/// `/`.
#[derive(Clone, Copy, Debug)]
struct Motif {
    name: &'static str,
    w: usize,
    h: usize,
    shift: usize,
    cells: &'static str,
}

const fn motif(name: &'static str, w: usize, h: usize, shift: usize, cells: &'static str) -> Motif {
    Motif {
        name,
        w,
        h,
        shift,
        cells,
    }
}

/// The fields studied: Life's densest lattices of each common period-2 oscillator (a search of
/// every lattice of one copy on tori up to 12 by 12), the densest agars an exhaustive search of
/// tori up to 24 cells found, and a reversing one.
const MOTIFS: &[Motif] = &[
    // A blinker every 5 cells, the field #274 tested: 4 of 25 cells change (16%).
    motif(
        "blinkers, every 5 cells",
        5,
        5,
        0,
        "...../.ooo./...../...../.....",
    ),
    // The densest blinker lattices: one every 16 cells, 4 of 16 change (25%).
    motif("blinkers, 4 by 4", 4, 4, 0, "..../ooo./..../...."),
    motif("blinkers, 4 by 4 staggered", 4, 4, 2, "..../ooo./..../...."),
    // Pairs of blinkers in opposite phases: 8 of 32 (25%).
    motif(
        "blinker pairs, opposite phases",
        8,
        4,
        4,
        ".....o../ooo..o../.....o../........",
    ),
    // Toads: 8 of 24 (33%).
    motif("toads", 6, 4, 4, "....../.ooo../ooo.../......"),
    // Clocks: 8 of 25 (32%).
    motif("clocks", 5, 5, 2, "..o../o.o../.o.o./.o.../....."),
    // Beacons: 2 of 24 (8%).
    motif("beacons", 12, 2, 7, "oo.......oo./oo.......oo."),
    // The densest agars of tori up to 24 cells whose phases are not each other's negative or a
    // shift of each other: 8 of 16 (50%) and 12 of 22 (55%).
    motif("agar, 50% flipping", 4, 4, 0, "ooo./oo.o/.o../o..."),
    motif("agar, 55% flipping", 11, 2, 6, "oooo.....o./oooo.o....."),
    // An agar whose phases are shifts of each other (stripes stepping along): 16 of 22 (73%).
    motif(
        "agar, 73% flipping, shifting",
        11,
        2,
        7,
        "oo..oo..o../oo..o..oo..",
    ),
    // Venetian blinds: two rows on, two off, each phase the other's negative (100%).
    motif("Venetian blinds (reversing)", 1, 4, 0, "./o/o/."),
];

impl Motif {
    /// The share of its cells that change, as its name says.
    fn name_share(&self) -> f64 {
        match self.name {
            n if n.starts_with("blinkers, every 5") => 4.0 / 25.0,
            n if n.starts_with("blinker") => 0.25,
            "toads" => 1.0 / 3.0,
            "clocks" => 8.0 / 25.0,
            "beacons" => 2.0 / 24.0,
            n if n.contains("50%") => 0.5,
            n if n.contains("55%") => 12.0 / 22.0,
            n if n.contains("73%") => 16.0 / 22.0,
            n if n.contains("reversing") => 1.0,
            n => panic!("{n}"),
        }
    }

    fn alive(&self) -> Vec<bool> {
        let rows: Vec<&str> = self.cells.split('/').collect();
        assert_eq!(rows.len(), self.h, "{}", self.name);
        let mut v = vec![false; self.w * self.h];
        for (y, r) in rows.iter().enumerate() {
            assert_eq!(r.len(), self.w, "{}", self.name);
            for (x, c) in r.chars().enumerate() {
                v[y * self.w + x] = c == 'o';
            }
        }
        v
    }

    /// Whether cell `(x, y)` of the plane is live.
    fn at(&self, alive: &[bool], x: i64, y: i64) -> bool {
        let (w, h) = (self.w as i64, self.h as i64);
        let q = y.div_euclid(h);
        let (xx, yy) = ((x + q * self.shift as i64).rem_euclid(w), y.rem_euclid(h));
        alive[(yy * w + xx) as usize]
    }

    /// A world size of at least `min_w` by `min_h` cells on which the pattern wraps exactly.
    fn world_size(&self, min_w: u32, min_h: u32) -> (u32, u32) {
        let (w, h, s) = (
            self.w as u32,
            self.h as u32,
            self.shift as u32 % self.w as u32,
        );
        // Down the world, the rows' shifts must add up to whole periods across.
        let ph = if s == 0 { h } else { h * (w / gcd(w, s)) };
        (min_w.div_ceil(w) * w, min_h.div_ceil(ph) * ph)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// A square world `w` by `h` cells, wrapping, of `rule`, `palette`, with `seed` (a seed's JSON).
fn world(w: u32, h: u32, rule: &str, palette: &str, seed: &str, rng: u64) -> World {
    let json = format!(
        r#"{{"name": "period 2", "tiling": {{"family": "square", "size": {{"rect": {{"w": {w}, "h": {h}}}}}, "boundary": "wrap"}}, "rng_state": {rng}, "cas": [{{"rule": "{rule}", "palette": "{palette}", "seed": {seed}}}]}}"#
    );
    let scene: Scene = serde_json::from_str(&json).unwrap();
    World::from_scene(&scene).unwrap()
}

/// The motif's field over a world large enough for a view of `VW` by `VH` at `cell_px`, checked
/// to be period 2 (and not 1) under JETCARP's stepping; the share of cells that change.
fn field(m: &Motif, cell_px: f64, palette: &str) -> (World, f64) {
    let need = |v: u32| (v as f64 / cell_px).ceil() as u32 + 2;
    let (w, h) = m.world_size(need(VW), need(VH));
    let alive = m.alive();
    let mut cells = vec![];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if m.at(&alive, x, y) {
                cells.push(format!("[{}, 1]", y * w as i64 + x));
            }
        }
    }
    let seed = format!(r#"{{"cells": {{"cells": [{}]}}}}"#, cells.join(", "));
    let mut wd = world(w, h, "B3/S23", palette, &seed, 1);
    let s0 = wd.fields[0].state.clone();
    wd.step();
    let s1 = wd.fields[0].state.clone();
    wd.step();
    assert_eq!(wd.fields[0].state, s0, "{}: not period 2", m.name);
    assert_ne!(s1, s0, "{}: still", m.name);
    let changed = s0.iter().zip(&s1).filter(|(a, b)| a != b).count();
    // Back to where it was drawn, with ages as they would be.
    (wd, changed as f64 / s0.len() as f64)
}

/// A soup of `rule` left to settle for `gens` generations on a world for `cell_px`; the share of
/// cells that change every generation and come back every other (its period-2 part).
fn ash(rule: &str, cell_px: f64, palette: &str, gens: u32, rng: u64) -> (World, f64) {
    let need = |v: u32| (v as f64 / cell_px).ceil() as u32 + 2;
    let mut wd = world(
        need(VW),
        need(VH),
        rule,
        palette,
        r#"{"random": {"density": 0.35}}"#,
        rng,
    );
    for _ in 0..gens {
        wd.step();
    }
    (wd.clone(), p2_share(&wd))
}

/// The share of a world's cells that change in the next generation and are back in the one after.
fn p2_share(w: &World) -> f64 {
    let mut a = w.clone();
    let s0 = a.fields[0].state.clone();
    a.step();
    let s1 = a.fields[0].state.clone();
    a.step();
    let s2 = &a.fields[0].state;
    let n = s0
        .iter()
        .zip(&s1)
        .zip(s2)
        .filter(|((x, y), z)| x != y && *x == *z)
        .count();
    n as f64 / s0.len() as f64
}

// ---------------------------------------------------------------------------------------------
// Running a field.
// ---------------------------------------------------------------------------------------------

/// One case: a world, drawn `cell_px` pixels a cell around its middle (moved `offset` pixels), at
/// `speed` generations a second, in a view that is the whole screen or a window on a larger one
/// ([`SCREEN`]).
#[derive(Clone)]
struct Case {
    name: String,
    world: World,
    cell_px: f64,
    palette: &'static str,
    speed: f64,
    offset: f64,
    screen: Option<f64>,
    /// The share of its cells that change every generation, period 2.
    p2: f64,
}

fn draw(world: &World, c: &Case) -> Vec<u8> {
    let b = world.topo.bounds;
    let center = [
        (b[0] + b[2]) / 2.0 + c.offset / c.cell_px,
        (b[1] + b[3]) / 2.0 + c.offset / c.cell_px,
    ];
    render_view(world, VW, VH, center, c.cell_px, &ViewOptions::default()).rgba
}

/// A window's screen: 1920 by 1080. The view (640 by 480) is 15% of it, so stripes in the view
/// never cover the quarter of the screen that makes them harmful (the stripe check's rule), and
/// only the flash measure can act on a field.
const SCREEN: Option<f64> = Some(1920.0 * 1080.0);

/// What a case reads: by the reference, by the guard as drawn, and through the guard as a screen
/// shows it.
#[derive(Clone, Debug)]
struct Row {
    name: String,
    cell_px: f64,
    palette: &'static str,
    speed: f64,
    window: bool,
    /// The stripe check found a harmful pattern in what was drawn.
    stripes: bool,
    p2: f64,
    /// The reference on the frames as drawn.
    letter: Letter,
    /// The guard's worst reading of the frames as drawn.
    drawn: FlashReading,
    /// Through the guard: the reference on what was shown, and the most the guard's own meter saw
    /// shown.
    shown_letter: Letter,
    shown: u32,
    /// The guard's largest share of tiles veiled, and softened, its action and cap.
    veiled: f32,
    softened: f32,
    action: &'static str,
    cap: Option<f32>,
    /// Frames of what the guard showed in which the reference failed: by the strictest reading
    /// (each pixel's own transitions in the last second), and concurrently (#289).
    shown_fail_frames: u32,
    shown_concurrent_frames: u32,
    /// The most transitions the guard's meter read, as drawn, over its tiles' restless change
    /// (#289).
    restless: u32,
}

impl Row {
    /// A gap: the reference counts the drawn frames as failing, and the guard lets a failure
    /// through to the screen.
    fn gap(&self) -> bool {
        self.letter.fails() && self.shown_letter.fails()
    }
}

fn worse(a: &FlashReading, b: &FlashReading) -> bool {
    (b.worst(), b.most()) > (a.worst(), a.most())
}

/// Runs a case for `secs` seconds at 60 frames a second, unguarded and through the guard.
fn run(c: &Case, secs: f64) -> Row {
    run_recorded(c, secs, None)
}

/// The same, saving the frames as drawn to `rec/off` and as the guard showed them to `rec/on`, as
/// PNGs (`00000.png` on), for IRIS (#289). Nothing is shown on a screen.
fn run_recorded(c: &Case, secs: f64, rec: Option<&std::path::Path>) -> Row {
    let save = |sub: &str, k: u32, px: &[u8]| {
        if let Some(dir) = rec {
            let d = dir.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            let img = jetcarp_core::render::Image {
                width: VW,
                height: VH,
                rgba: px.to_vec(),
            };
            jetcarp_core::render::write_png(&d.join(format!("{k:05}.png")), &img, &[]).unwrap();
        }
    };
    let frames = (secs * FPS) as u32;
    // Unguarded: the reference and the guard's meter on the frames as drawn.
    let mut world = c.world.clone();
    let start = world.generation;
    let mut wcag = Wcag::new(VW, VH);
    let mut meter = FlashMeter::new(Viewing::screen(1.0), c.screen);
    let (mut letter, mut drawn) = (Letter::default(), FlashReading::default());
    let mut restless = 0;
    for k in 0..frames {
        let t = k as f64 / FPS;
        let due = (k as f64 * c.speed / FPS + 1e-9).floor() as u64;
        while world.generation - start < due {
            world.step();
        }
        let px = draw(&world, c);
        save("off", k, &px);
        letter = letter.worse(wcag.update(&px, t));
        let r = meter.update(&reduce(&px, VW, VH), t);
        restless = restless.max(r.restless_transitions);
        if worse(&drawn, &r) {
            drawn = r;
        }
    }
    // Through the guard: the processor's softener carrying out its plan, its readings two
    // frames late, its pace cap holding the generations back, its stripe check four times a
    // second.
    let mut world = c.world.clone();
    let mut guard = Guard::new(Viewing::screen(1.0), c.screen, true);
    // For the IRIS comparison, the guard for someone at risk (`JETCARP_STROBE_AT_RISK=1`).
    guard.set_at_risk(std::env::var_os("JETCARP_STROBE_AT_RISK").is_some());
    let mut stripes = false;
    let mut soft = Softener::new();
    let mut wcag = Wcag::new(VW, VH);
    let mut queue = VecDeque::new();
    let (mut due, mut shown_letter, mut shown, mut veiled, mut softened) =
        (0.0f64, Letter::default(), 0u32, 0f32, 0f32);
    let (mut shown_fail_frames, mut shown_concurrent_frames) = (0u32, 0u32);
    // Each frame shown, for one case at a time (`JETCARP_P2_TRACE=1`).
    let trace = std::env::var_os("JETCARP_P2_TRACE").is_some();
    for k in 0..frames {
        let t = k as f64 / FPS;
        let pace = guard.cap().map_or(c.speed, |cap| (cap as f64).min(c.speed));
        if k > 0 && guard.paused().is_none() {
            due += pace / FPS;
            while ((world.generation - start) as f64) < (due + 1e-9).floor() {
                world.step();
            }
        }
        let px = draw(&world, c);
        let plan = guard.plan().clone();
        let s = soft.apply(&px, VW, VH, &plan, (1.0 / FPS) as f32);
        let out = s.shown.as_deref().unwrap_or(&px);
        save("on", k, out);
        let now = wcag.update(out, t);
        if trace {
            eprintln!(
                "{:.3}: shown {} (concurrently {}), veiled {:.0}%, cap {:?}, generation {}",
                t,
                now.most(),
                now.concurrent,
                100.0 * plan.veil.iter().filter(|v| v[0] > 0.0).count() as f32
                    / plan.veil.len().max(1) as f32,
                guard.cap(),
                world.generation - start
            );
        }
        shown_fail_frames += now.fails() as u32;
        shown_concurrent_frames += (now.concurrent > 6) as u32;
        shown_letter = shown_letter.worse(now);
        let n = plan.veil.len().max(1) as f32;
        veiled = veiled.max(plan.veil.iter().filter(|v| v[0] > 0.0).count() as f32 / n);
        softened = softened.max(plan.soften.iter().filter(|s| **s).count() as f32 / n);
        let half = guard.wants_half(t).then(|| reduce_both(&px, VW, VH).1);
        queue.push_back((t, s.input, s.output, half));
        while queue.len() > 2 {
            let (t, i, o, h) = queue.pop_front().unwrap();
            guard.observe(
                Frame {
                    time: t,
                    input: &i,
                    output: &o,
                    half: h.as_ref(),
                },
                Pace {
                    speed: c.speed as f32,
                    reached: Some(pace as f32),
                    frame_rate: FPS as f32,
                },
            );
            let r = guard.report();
            shown = shown.max(r.shown.most());
            stripes |= r.stripes.level >= jetcarp_core::strobe::pattern::PatternLevel::Harmful;
        }
    }
    let report = guard.report();
    Row {
        name: c.name.clone(),
        cell_px: c.cell_px,
        palette: c.palette,
        speed: c.speed,
        window: c.screen.is_some(),
        stripes,
        p2: c.p2,
        letter,
        drawn,
        shown_letter,
        shown,
        veiled,
        softened,
        action: report.action.name(),
        cap: report.cap,
        shown_fail_frames,
        shown_concurrent_frames,
        restless,
    }
}

fn level_name(l: Level) -> &'static str {
    l.name()
}

fn letter_verdict(l: &Letter, cell_px: f64) -> String {
    let v = if l.fails() {
        "fail"
    } else if l.most() >= 4 {
        "warning"
    } else {
        "pass"
    };
    // WCAG's exception for a fine, balanced pattern: squares under 0.1° (2.65 pixels at the
    // distance the guard assumes).
    if cell_px < Viewing::screen(1.0).field_px / 100.0 && l.fails() {
        format!("{v} (cells under 0.1°)")
    } else {
        v.to_string()
    }
}

fn print_row(r: &Row) {
    println!(
        "| {} | {} | {} | {} | {} | {:.0}% | {:.0}% | {} ({}) | {} ({}: area {}, fine {}, partial {}, screen {}) | {} (guard {}) | {:.0}%{} | {:.0}% | {}{} |",
        r.name,
        if r.window { "window" } else { "screen" },
        r.cell_px,
        r.palette,
        r.speed,
        r.p2 * 100.0,
        r.letter.fail_share * 100.0,
        letter_verdict(&r.letter, r.cell_px),
        format!("{}, concurrent {}", r.letter.most(), r.letter.concurrent),
        level_name(r.drawn.worst()),
        r.drawn.most(),
        r.drawn.area_transitions.max(r.drawn.red_transitions),
        r.drawn.fine_transitions,
        r.drawn.partial_transitions,
        r.drawn.screen_transitions,
        r.shown_letter.most(),
        r.shown,
        r.veiled * 100.0,
        if r.stripes { " (stripes)" } else { "" },
        r.softened * 100.0,
        r.action,
        r.cap.map_or(String::new(), |c| format!(", cap {c}"))
    );
}

const SIZES: &[f64] = &[
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0, 16.0, 24.0, 32.0,
];
const PALETTES: &[&str] = &["Ember", "Thermal", "Paper", "Autumn"];

/// Every field is period 2 under JETCARP's stepping, and changes the share of its cells it should.
#[test]
fn the_fields_are_period_2() {
    for m in MOTIFS {
        let rotor = m.name_share();
        let (_, p2) = field(m, 4.0, "Ember");
        assert!(
            (p2 - rotor).abs() < 1e-9,
            "{}: {p2} of its cells change, not {rotor}",
            m.name
        );
    }
}

/// The case of the field `name` (a motif's name, as far as it is unique), `px` pixels a cell.
fn motif_case(name: &str, px: f64, palette: &'static str, speed: f64) -> Case {
    let m = MOTIFS
        .iter()
        .find(|m| m.name.starts_with(name))
        .unwrap_or_else(|| panic!("{name}"));
    let (world, p2) = field(m, px, palette);
    Case {
        name: m.name.to_string(),
        world,
        cell_px: px,
        palette,
        speed,
        offset: 0.0,
        screen: SCREEN,
        p2,
    }
}

/// Ordinary scenes at `px` pixels a cell: a field of still lifes, just drawn and once its cells'
/// colours have settled (a cell's colour follows its age, to 400 generations), of gliders slow and
/// fast, a Life soup, and its ash (still lifes and a few blinkers), slow and fast.
fn ordinary(px: f64, palette: &'static str) -> Vec<Case> {
    let need = |v: u32| (v as f64 / px).ceil() as u32 + 2;
    let (w, h) = (need(VW), need(VH));
    let tiled = |every: u32, shape: &[(u32, u32)]| {
        let mut cells = vec![];
        for by in (0..h.saturating_sub(every)).step_by(every as usize) {
            for bx in (0..w.saturating_sub(every)).step_by(every as usize) {
                for &(x, y) in shape {
                    cells.push(format!("[{}, 1]", (by + y) * w + bx + x));
                }
            }
        }
        format!(r#"{{"cells": {{"cells": [{}]}}}}"#, cells.join(", "))
    };
    let case = |name: &str, world: World, speed: f64| Case {
        name: name.to_string(),
        p2: p2_share(&world),
        world,
        cell_px: px,
        palette,
        speed,
        offset: 0.0,
        screen: SCREEN,
    };
    let block = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let soup = r#"{"random": {"density": 0.35}}"#;
    let (ash, _) = ash("B3/S23", px, palette, 1500, 11);
    let blocks = world(w, h, "B3/S23", palette, &tiled(4, &block), 1);
    let mut settled = blocks.clone();
    for _ in 0..400 {
        settled.step();
    }
    vec![
        case(
            "still lifes (blocks, every 4 cells), just drawn",
            blocks,
            30.0,
        ),
        case(
            "still lifes (blocks, every 4 cells), settled",
            settled,
            30.0,
        ),
        case(
            "gliders (every 8 cells)",
            world(w, h, "B3/S23", palette, &tiled(8, &glider), 1),
            4.0,
        ),
        case(
            "gliders (every 8 cells)",
            world(w, h, "B3/S23", palette, &tiled(8, &glider), 1),
            30.0,
        ),
        case("Life soup", world(w, h, "B3/S23", palette, soup, 7), 30.0),
        case("Life ash", ash.clone(), 4.0),
        case("Life ash", ash, 30.0),
    ]
}

/// Dense period-2 fields that WCAG counts as flashing, pixel by pixel, and that the guard's tiles
/// missed (#275): squares of 3 pixels and more (over 0.1°), the default palette and the strongest,
/// in a window (where the stripe check cannot act on them).
fn dense_cases() -> Vec<Case> {
    [
        ("blinkers, 4 by 4", 4.0, "Ember"),
        ("blinkers, 4 by 4", 8.0, "Paper"),
        ("blinker pairs", 5.0, "Thermal"),
        ("toads", 4.0, "Ember"),
        ("toads", 8.0, "Ember"),
        ("toads", 16.0, "Thermal"),
        ("clocks", 3.0, "Paper"),
        ("clocks", 8.0, "Ember"),
        ("agar, 50%", 3.0, "Ember"),
        ("agar, 50%", 4.0, "Ember"),
        ("agar, 50%", 8.0, "Autumn"),
        ("agar, 55%", 4.0, "Paper"),
        ("agar, 73%", 4.0, "Paper"),
        ("agar, 73%", 6.0, "Autumn"),
    ]
    .into_iter()
    .map(|(n, px, pal)| motif_case(n, px, pal, 30.0))
    .collect()
}

/// The guard's meter counts dense period-2 fields as flashing, as the letter of WCAG does, though
/// their tiles' means barely move and their fine patterns do not reverse (#275): every case fails
/// by the reference, and by the guard as drawn.
#[test]
fn the_guard_counts_dense_period_2_fields() {
    let rows: Vec<Row> = dense_cases().par_iter().map(|c| run(c, 1.5)).collect();
    let missed: Vec<String> = rows
        .iter()
        .filter(|r| !(r.letter.fails() && r.drawn.worst().fails()))
        .map(|r| {
            format!(
                "{}, {} px a cell, {}: reference {} ({:.0}% of a field), guard {:?} ({} transitions: \
                 area {}, partial {})",
                r.name,
                r.cell_px,
                r.palette,
                r.letter.most(),
                r.letter.fail_share * 100.0,
                r.drawn.worst(),
                r.drawn.most(),
                r.drawn.area_transitions,
                r.drawn.partial_transitions
            )
        })
        .collect();
    assert!(
        missed.is_empty(),
        "dense period-2 fields not counted:\n{}",
        missed.join("\n")
    );
}

/// Through the guard, as a screen shows it, no dense period-2 field flashes, measured pixel by
/// pixel by the reference: two flashes a second at most, before the pace cap holds. In a window,
/// and over the whole screen, where the stripe check finds the regular ones too.
#[test]
fn through_the_guard_dense_period_2_fields_stay_under_the_limit() {
    let mut cases = dense_cases();
    for (n, px) in [
        ("blinkers, 4 by 4", 4.0),
        ("toads", 4.0),
        ("agar, 50%", 4.0),
    ] {
        cases.push(Case {
            screen: None,
            ..motif_case(n, px, "Ember", 30.0)
        });
    }
    let rows: Vec<Row> = cases.par_iter().map(|c| run(c, 4.0)).collect();
    let through: Vec<String> = rows
        .iter()
        .filter(|r| r.shown_letter.most() > 4)
        .map(|r| {
            format!(
                "{}, {} px a cell, {}, {}: {} transitions a second shown, pixel by pixel ({:?}, \
                 {:.0}% veiled, cap {:?})",
                r.name,
                r.cell_px,
                r.palette,
                if r.window { "window" } else { "whole screen" },
                r.shown_letter.most(),
                r.action,
                r.veiled * 100.0,
                r.cap
            )
        })
        .collect();
    assert!(
        through.is_empty(),
        "the guard let through:\n{}",
        through.join("\n")
    );
}

/// What must be left alone: sparser period-2 fields, which the reference passes (a blinker every
/// 5 cells, 16% of the area, at 7 pixels a cell and more; beacons, 8%), and ordinary scenes (still
/// lifes, gliders, a Life soup and its ash). The guard never veils one where the reference counts
/// no flash, and never counts a slow scene, or still lifes whose colours have settled, as flashing
/// at all. (Still lifes just drawn, in a palette whose colours pass through a saturated red as the
/// cells age, flash red once together, which the reference counts: the guard may act on that.)
#[test]
fn sparse_period_2_fields_and_ordinary_scenes_are_left_alone() {
    let mut cases = vec![];
    for (px, pal) in [(4.0, "Ember"), (6.0, "Paper"), (8.0, "Thermal")] {
        cases.push(motif_case("beacons", px, pal, 30.0));
        cases.extend(ordinary(px, pal));
    }
    for (px, pal) in [(8.0, "Ember"), (8.0, "Thermal"), (12.0, "Paper")] {
        cases.push(motif_case("blinkers, every 5", px, pal, 30.0));
    }
    let rows: Vec<Row> = cases.par_iter().map(|c| run(c, 3.0)).collect();
    let wrong: Vec<String> = rows
        .iter()
        .filter(|r| {
            let slow_or_still = r.speed < 7.0 || r.name.contains("settled");
            (r.veiled > 0.0 && r.letter.most() < 2)
                || (slow_or_still && (r.drawn.worst() != Level::Pass || r.veiled > 0.0))
        })
        .map(|r| {
            format!(
                "{} at {} gen/s, {} px a cell, {}: veiled {:.0}%, reference {}, guard {:?} ({}: \
                 area {}, partial {})",
                r.name,
                r.speed,
                r.cell_px,
                r.palette,
                r.veiled * 100.0,
                r.letter.most(),
                r.drawn.worst(),
                r.drawn.most(),
                r.drawn.area_transitions,
                r.drawn.partial_transitions
            )
        })
        .collect();
    assert!(wrong.is_empty(), "left alone, but:\n{}", wrong.join("\n"));
}

/// The densest field of each kind, pinned (#275): at 4 pixels a cell (the app's display's
/// default) and 8 (the command line's), in the default palette, in a window, 30 generations a
/// second. For each, whether the reference fails it, pixel by pixel, and whether the guard's
/// meter does, as drawn. Blinkers 4 cells apart flip a quarter of the area, which a field of
/// 4-pixel cells is over by a hair wherever a field of vision falls across more of their rows; at
/// 8 pixels a cell the cells' outlines keep part of each cell still, and the field flips a fifth,
/// which the guard, counting its windows a little larger than they are
/// ([`jetcarp_core::strobe::flash::WINDOW_MARGIN`]), counts as flashing: stricter than WCAG. They
/// agree on every other one. A regression either way moves a pin.
const PINS: &[(&str, f64, bool, bool)] = &[
    ("blinkers, every 5", 4.0, false, false),
    ("blinkers, every 5", 8.0, false, false),
    ("beacons", 4.0, false, false),
    ("beacons", 8.0, false, false),
    ("blinkers, 4 by 4", 4.0, true, true),
    ("blinkers, 4 by 4", 8.0, false, true),
    ("blinker pairs", 4.0, true, true),
    ("blinker pairs", 8.0, false, true),
    ("toads", 4.0, true, true),
    ("toads", 8.0, true, true),
    ("clocks", 4.0, true, true),
    ("clocks", 8.0, true, true),
    ("agar, 50%", 4.0, true, true),
    ("agar, 50%", 8.0, true, true),
    ("agar, 55%", 4.0, true, true),
    ("agar, 55%", 8.0, true, true),
    ("agar, 73%", 4.0, true, true),
    ("agar, 73%", 8.0, true, true),
    ("Venetian blinds", 4.0, true, true),
    ("Venetian blinds", 8.0, true, true),
];

#[test]
fn the_densest_period_2_fields_are_pinned() {
    let rows: Vec<(Row, bool, bool)> = PINS
        .par_iter()
        .map(|&(name, px, letter, guard)| {
            (
                run(&motif_case(name, px, "Ember", 30.0), 1.5),
                letter,
                guard,
            )
        })
        .collect();
    let moved: Vec<String> = rows
        .iter()
        .filter(|(r, letter, guard)| {
            (r.letter.fails(), r.drawn.worst().fails()) != (*letter, *guard)
        })
        .map(|(r, letter, guard)| {
            format!(
                "{}, {} px a cell: reference fails {} ({}, {:.0}% of a field), guard fails {} \
                 ({:?}, {}); pinned {letter}, {guard}",
                r.name,
                r.cell_px,
                r.letter.fails(),
                r.letter.most(),
                r.letter.fail_share * 100.0,
                r.drawn.worst().fails(),
                r.drawn.worst(),
                r.drawn.most()
            )
        })
        .collect();
    assert!(moved.is_empty(), "pins moved:\n{}", moved.join("\n"));
}

/// The study's table (ignored; run with `--ignored --nocapture`): every field at every cell size
/// and palette, 30 generations a second, 3 seconds at 60 frames a second; then the ash of the
/// named rules that settle into the most period-2 change, and ordinary scenes.
#[test]
#[ignore]
fn study_table() {
    let only = std::env::var("JETCARP_P2_ONLY").ok();
    let mut cases = vec![];
    for m in MOTIFS {
        if only.as_deref().is_some_and(|o| !m.name.contains(o)) {
            continue;
        }
        for &px in SIZES {
            for &pal in PALETTES {
                let c = motif_case(m.name, px, pal, 30.0);
                cases.push(Case {
                    screen: None,
                    ..c.clone()
                });
                cases.push(c);
            }
        }
    }
    // Soups of named rules left to settle 1,000 generations (`ash_of_the_named_rules`): Long
    // Life, Walled Cities, 3-4 Life and DotLife settle with much of their area flipping; Life's
    // ash, for comparison.
    if only.as_deref().is_none_or(|o| o == "ash") {
        for (rule, name) in [
            ("B345/S5", "Long Life ash"),
            ("B45678/S2345", "Walled Cities ash"),
            ("B34/S34", "3-4 Life ash"),
            ("B3/S023", "DotLife ash"),
            ("B3/S23", "Life ash"),
        ] {
            for &px in &[2.0, 4.0, 8.0, 16.0] {
                for &pal in &["Ember", "Thermal", "Paper"] {
                    let (world, p2) = ash(rule, px, pal, 1000, 7);
                    cases.push(Case {
                        name: name.into(),
                        world,
                        cell_px: px,
                        palette: pal,
                        speed: 30.0,
                        offset: 0.0,
                        screen: SCREEN,
                        p2,
                    });
                }
            }
        }
    }
    if only.as_deref().is_none_or(|o| o == "ordinary") {
        for &px in &[2.0, 4.0, 8.0, 16.0] {
            for &pal in &["Ember", "Paper"] {
                cases.extend(ordinary(px, pal));
            }
        }
    }
    let rows: Vec<Row> = cases.par_iter().map(|c| run(c, 3.0)).collect();
    println!(
        "| Field | View | Cell px | Palette | Gen/s | Cells flipping | Area flashing (reference) | Reference | Guard as drawn | Reference on screen | Veiled | Softened | Guard action |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in &rows {
        print_row(r);
    }
    let gaps: Vec<&Row> = rows.iter().filter(|r| r.gap()).collect();
    println!(
        "\n{} of {} cases: the reference fails what is drawn, and what the guard shows still fails",
        gaps.len(),
        rows.len()
    );
}

/// Named rules' soups left to settle (ignored; `--ignored --nocapture`): each two-state
/// Life-like rule's period-2 share after 1,000 generations, most first.
#[test]
#[ignore]
fn ash_of_the_named_rules() {
    let rules: Vec<&str> = jetcarp_core::rules::CATALOG
        .iter()
        .map(|e| e.notation)
        .filter(|n| {
            n.starts_with('B') && n.contains("/S") && !n.contains("/C") && !n.ends_with('V')
        })
        .collect();
    let mut out: Vec<(f64, &str)> = rules
        .par_iter()
        .map(|r| (ash(r, 4.0, "Ember", 1000, 7).1, *r))
        .collect();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (s, r) in out {
        println!("{r}: {:.1}% of cells period 2", s * 100.0);
    }
}

/// How well the tiles' windows stand for the pixels that change (ignored; `--ignored
/// --nocapture`, cases in `JETCARP_P2_CASES` as `name|cell px|palette;...`, `soup` and
/// `gliders` for ordinary scenes): over the frames that change, the share of the view's pixels
/// that changed by 0.1 or more since the frame before, and the share the guard counts for its
/// tiles' changes ([`Change`]), and its windows' alone.
#[test]
#[ignore]
fn windows_against_pixels() {
    use jetcarp_core::strobe::flash::{Change, window_share};
    use jetcarp_core::strobe::{DARK_LIMIT, FLASH_DELTA};
    let specs = std::env::var("JETCARP_P2_CASES").unwrap_or("toads|16|Thermal".into());
    for spec in specs.split(';') {
        let p: Vec<&str> = spec.split('|').collect();
        let pal: &'static str = Box::leak(p[2].to_string().into_boxed_str());
        let cell: f64 = p[1].parse().unwrap();
        let c = match p[0] {
            "soup" => ordinary(cell, pal)
                .into_iter()
                .find(|c| c.name == "Life soup")
                .unwrap(),
            "gliders" => ordinary(cell, pal)
                .into_iter()
                .find(|c| c.name.starts_with("gliders") && c.speed > 10.0)
                .unwrap(),
            name => motif_case(name, cell, pal, 30.0),
        };
        let mut world = c.world.clone();
        let start = world.generation;
        let mut change = vec![];
        let mut last: Option<Vec<f32>> = None;
        let lin: [f64; 256] = std::array::from_fn(|i| lin(i as u8));
        let (mut sum, mut est, mut frames, mut most, mut most_est) =
            (0f64, [0f64; 4], 0, 0f64, [0f64; 4]);
        for k in 0..90u32 {
            let due = (k as f64 * c.speed / FPS + 1e-9).floor() as u64;
            while world.generation - start < due {
                world.step();
            }
            let px = draw(&world, &c);
            let map = reduce(&px, VW, VH);
            if change.len() != map.len() {
                change = vec![Change::default(); map.len()];
            }
            let lum: Vec<f32> = px.chunks_exact(4).map(|q| values(&lin, q).lum).collect();
            let changed = last.as_ref().map_or(0, |l| {
                l.iter()
                    .zip(&lum)
                    .filter(|(a, b)| (*a - *b).abs() >= 0.1)
                    .count()
            });
            let mut e = [0f64; 4];
            for i in 0..map.len() {
                let c = change[i].compare(map.lum[i], &map.fine[i], FLASH_DELTA, Some(DARK_LIMIT));
                let px = map.tile_px(i) as f64;
                // The share the guard counts, and the windows' alone.
                e[0] += c.share as f64 * px;
                e[1] += window_share(c.mask) as f64 * px;
            }
            last = Some(lum);
            let n = (VW * VH) as f64;
            if changed > 0 {
                frames += 1;
                sum += changed as f64 / n;
                most = most.max(changed as f64 / n);
                for j in 0..4 {
                    est[j] += e[j] / n;
                    most_est[j] = most_est[j].max(e[j] / n);
                }
            }
        }
        let f = frames.max(1) as f64;
        println!(
            "{spec}: pixels changed {:.1}% (most {:.1}%); the guard's share {:.1}% ({:.1}%), the \
             windows' alone {:.1}% ({:.1}%)",
            100.0 * sum / f,
            100.0 * most,
            100.0 * est[0] / f,
            100.0 * most_est[0],
            100.0 * est[1] / f,
            100.0 * most_est[1]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Restless ash (#289): change that does not come back.
// ---------------------------------------------------------------------------------------------

/// The share of a world's cells that change in the next generation, and of those not back in the
/// one after: change that does not come back within two generations.
fn change_shares(w: &World) -> (f64, f64) {
    let mut a = w.clone();
    let s0 = a.fields[0].state.clone();
    a.step();
    let s1 = a.fields[0].state.clone();
    a.step();
    let s2 = &a.fields[0].state;
    let n = s0.len() as f64;
    let changed = s0.iter().zip(&s1).filter(|(x, y)| x != y).count() as f64;
    let restless = s0
        .iter()
        .zip(&s1)
        .zip(s2)
        .filter(|((x, y), z)| x != y && x != z)
        .count() as f64;
    (changed / n, restless / n)
}

/// Every named two-state Life-like rule and Generations rule of the Moore neighbourhood.
fn life_like_and_generations() -> Vec<(&'static str, &'static str)> {
    jetcarp_core::rules::CATALOG
        .iter()
        .filter(|e| e.notation.starts_with('B') && e.notation.contains("/S"))
        .filter(|e| !e.notation.ends_with('V'))
        .map(|e| (e.notation, e.name))
        .collect()
}

/// A soup of `rule` (density 0.35) left to settle `gens` generations, drawn `px` pixels a cell at
/// 30 generations a second.
fn ash_case(rule: &str, name: &str, px: f64, palette: &'static str, screen: Option<f64>) -> Case {
    let (world, p2) = ash(rule, px, palette, 1000, 7);
    Case {
        name: name.to_string(),
        world,
        cell_px: px,
        palette,
        speed: 30.0,
        offset: 0.0,
        screen,
        p2,
    }
}

fn print_restless(r: &Row, changed: f64, restless: f64) {
    println!(
        "| {} | {} | {} | {} | {:.0}% | {:.0}% | {:.0}% | {}, {} ({:.0}%) | {} ({}: area {}, fine {}, partial {}, restless {}, screen {}) | {}, {} ({} / {} frames) | {:.0}% | {:.0}% | {}{} |",
        r.name,
        if r.window { "window" } else { "screen" },
        r.cell_px,
        r.palette,
        changed * 100.0,
        restless * 100.0,
        r.p2 * 100.0,
        r.letter.most(),
        r.letter.concurrent,
        r.letter.fail_share * 100.0,
        level_name(r.drawn.worst()),
        r.drawn.most(),
        r.drawn.area_transitions.max(r.drawn.red_transitions),
        r.drawn.fine_transitions,
        r.drawn.partial_transitions,
        r.restless,
        r.drawn.screen_transitions,
        r.shown_letter.most(),
        r.shown_letter.concurrent,
        r.shown_fail_frames,
        r.shown_concurrent_frames,
        r.veiled * 100.0,
        r.softened * 100.0,
        r.action,
        r.cap.map_or(String::new(), |c| format!(", cap {c}"))
    );
}

const RESTLESS_HEADER: &str = "| Ash | View | Cell px | Palette | Cells changing | Not back in 2 | Period 2 | Reference: strict, concurrent (area failing) | Guard as drawn | Reference on screen: strict, concurrent (frames failing) | Veiled | Softened | Guard action |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|";

/// Dense restless ash (#289): the settled soups of Walled Cities, 34 Life and Long Life, whose
/// cells keep changing without coming back, at cell sizes over 0.1°, in a window (where the stripe
/// check cannot act on them).
fn restless_cases() -> Vec<Case> {
    [
        ("B45678/S2345", "Walled Cities", 4.0, "Ember"),
        ("B45678/S2345", "Walled Cities", 4.0, "Paper"),
        ("B45678/S2345", "Walled Cities", 8.0, "Thermal"),
        ("B34/S34", "34 Life", 4.0, "Ember"),
        ("B34/S34", "34 Life", 8.0, "Paper"),
        ("B345/S5", "Long Life", 4.0, "Thermal"),
        ("B345/S5", "Long Life", 4.0, "Paper"),
    ]
    .into_iter()
    .map(|(rule, name, px, pal)| ash_case(rule, name, px, pal, SCREEN))
    .collect()
}

/// WCAG counted pixel by pixel says dense restless ash flashes, by the strictest reading and
/// concurrently (the pixels that make a transition in one frame, over a quarter of a field, each
/// made more than six in the last second), and the guard's meter counts it as flashing as drawn
/// (#289).
#[test]
fn restless_ash_flashes_by_the_reference_and_the_guard() {
    let rows: Vec<Row> = restless_cases().par_iter().map(|c| run(c, 1.5)).collect();
    let wrong: Vec<String> = rows
        .iter()
        .filter(|r| !(r.letter.fails() && r.letter.concurrent > 6 && r.drawn.worst().fails()))
        .map(|r| {
            format!(
                "{} ash, {} px a cell, {}: reference {} (concurrently {}), guard {:?} ({})",
                r.name,
                r.cell_px,
                r.palette,
                r.letter.most(),
                r.letter.concurrent,
                r.drawn.worst(),
                r.drawn.most()
            )
        })
        .collect();
    assert!(wrong.is_empty(), "not counted:\n{}", wrong.join("\n"));
}

/// Through the guard, as a screen shows it, dense restless ash never flashes, measured pixel by
/// pixel by the reference by its strictest reading: three flashes a second at most (six
/// transitions, the most WCAG allows), from the start, before the pace cap holds as after (#289).
/// In a window, and over the whole screen; and a Seeds soup, which is restless too.
#[test]
fn through_the_guard_restless_ash_stays_under_the_limit() {
    let mut cases = restless_cases();
    for (rule, name) in [("B45678/S2345", "Walled Cities"), ("B34/S34", "34 Life")] {
        cases.push(ash_case(rule, name, 4.0, "Ember", None));
    }
    cases.push(ash_case("B2/S", "Seeds", 4.0, "Ember", SCREEN));
    let rows: Vec<Row> = cases.par_iter().map(|c| run(c, 4.0)).collect();
    let through: Vec<String> = rows
        .iter()
        .filter(|r| r.shown_letter.most() > 6)
        .map(|r| {
            format!(
                "{} ash, {} px a cell, {}, {}: {} transitions a second shown, pixel by pixel \
                 (concurrently {}; {} frames failing; {:?}, {:.0}% veiled, cap {:?})",
                r.name,
                r.cell_px,
                r.palette,
                if r.window { "window" } else { "whole screen" },
                r.shown_letter.most(),
                r.shown_letter.concurrent,
                r.shown_fail_frames,
                r.action,
                r.veiled * 100.0,
                r.cap
            )
        })
        .collect();
    assert!(
        through.is_empty(),
        "the guard let through:\n{}",
        through.join("\n")
    );
}

/// Busy scenes that WCAG counted pixel by pixel does not count as flashing concurrently are not
/// veiled (#289): Brian's Brain, Lava and a Life soup at 30 generations a second. Their cells
/// change all over the field, but a pixel seldom flashes more than three times a second while
/// those around it do.
#[test]
fn busy_scenes_that_do_not_flash_concurrently_are_not_veiled() {
    let mut cases = vec![];
    for (px, pal) in [(4.0, "Ember"), (8.0, "Paper"), (16.0, "Thermal")] {
        cases.push(ash_case("B2/S/C3", "Brian's Brain", px, pal, SCREEN));
        cases.push(ash_case("B2/S/C3", "Brian's Brain", px, pal, None));
    }
    for (px, pal) in [(4.0, "Ember"), (8.0, "Paper")] {
        cases.push(ash_case("B45678/S12345/C8", "Lava", px, pal, SCREEN));
    }
    cases.extend(
        ordinary(4.0, "Ember")
            .into_iter()
            .filter(|c| c.name == "Life soup"),
    );
    let rows: Vec<Row> = cases.par_iter().map(|c| run(c, 3.0)).collect();
    let wrong: Vec<String> = rows
        .iter()
        .filter(|r| r.letter.concurrent > 6 || r.veiled > 0.0)
        .map(|r| {
            format!(
                "{}, {} px a cell, {}, {}: reference concurrently {} (strictly {}), veiled {:.0}%",
                r.name,
                r.cell_px,
                r.palette,
                if r.window { "window" } else { "whole screen" },
                r.letter.concurrent,
                r.letter.most(),
                r.veiled * 100.0
            )
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every named Life-like and Generations rule's soup left to settle 1,000 generations, drawn 4
/// pixels a cell in Ember in a window at 30 generations a second (ignored; `--ignored
/// --nocapture`): how much of it changes, and what the reference (strictly and concurrently) and
/// the guard read, as drawn and as shown.
#[test]
#[ignore]
fn restless_ash_of_the_named_rules() {
    let px: f64 = std::env::var("JETCARP_RESTLESS_PX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0);
    let rows: Vec<(Row, f64, f64)> = life_like_and_generations()
        .par_iter()
        .map(|&(rule, name)| {
            let c = ash_case(rule, &format!("{name} ({rule})"), px, "Ember", SCREEN);
            let (changed, restless) = change_shares(&c.world);
            (run(&c, 3.0), changed, restless)
        })
        .collect();
    println!("{RESTLESS_HEADER}");
    for (r, changed, restless) in &rows {
        print_restless(r, *changed, *restless);
    }
}

/// The restless ash studied (#289): the Life-like rules that settle with the most change that does
/// not come back, and the Generations rules, Brian's Brain among them.
const RESTLESS: &[(&str, &str)] = &[
    // Settled, with change that does not come back.
    ("B45678/S2345", "Walled Cities"),
    ("B34/S34", "34 Life"),
    ("B345/S5", "Long Life"),
    ("B3/S023", "DotLife"),
    // Still chaotic after 1,000 generations.
    ("B234/S", "Serviettes"),
    ("B2/S", "Seeds"),
    ("B1/S1", "Gnarl"),
    ("B1357/S1357", "Replicator"),
    ("B1357/S02468", "Fredkin"),
    // Generations rules: those whose soups change most, and Brian's Brain.
    ("B2/S/C3", "Brian's Brain"),
    ("B45678/S12345/C8", "Lava"),
    ("B23/S2/C8", "RainZha"),
    ("B23/S23/C8", "BelZhab"),
    ("B2/S23/C8", "Cooties"),
    ("B2356/S1456/C16", "Xtasy"),
];

/// The study's table (ignored; `--ignored --nocapture`, rules by name in `JETCARP_RESTLESS_RULES`,
/// split by commas): each restless ash at 1 to 16 pixels a cell, in a dark palette, the strongest
/// and a light one, in a window and over the whole screen, 30 generations a second for 3 seconds.
/// With `JETCARP_STROBE_RECORD=DIR`, the whole-screen cases at 2, 4, 8 and 16 pixels in Ember and
/// Paper are saved as drawn and as the guard showed them, `DIR/<case>/<off|on>/00000.png`, for
/// IRIS (and with `JETCARP_RESTLESS_RECORD_ONLY=1` only those cases are run; with
/// `JETCARP_STROBE_AT_RISK=1`, through the guard for someone at risk).
#[test]
#[ignore]
fn restless_table() {
    let only = std::env::var("JETCARP_RESTLESS_RULES").ok();
    let rec = std::env::var_os("JETCARP_STROBE_RECORD").map(std::path::PathBuf::from);
    let rec_only = std::env::var_os("JETCARP_RESTLESS_RECORD_ONLY").is_some();
    let recorded = |c: &Case| {
        c.screen.is_none() && [2.0, 4.0, 8.0, 16.0].contains(&c.cell_px) && c.palette != "Thermal"
    };
    let mut cases = vec![];
    for &(rule, name) in RESTLESS {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|n| n == name))
        {
            continue;
        }
        let sizes: Vec<f64> = std::env::var("JETCARP_RESTLESS_PX").map_or_else(
            |_| vec![1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0],
            |v| v.split(',').map(|x| x.parse().unwrap()).collect(),
        );
        let palettes: Vec<String> = std::env::var("JETCARP_RESTLESS_PALETTES").map_or_else(
            |_| vec!["Ember".into(), "Thermal".into(), "Paper".into()],
            |v| v.split(',').map(String::from).collect(),
        );
        for &px in &sizes {
            for pal in &palettes {
                let pal: &'static str = Box::leak(pal.clone().into_boxed_str());
                for screen in [SCREEN, None] {
                    let c = ash_case(rule, name, px, pal, screen);
                    if !rec_only || recorded(&c) {
                        cases.push(c);
                    }
                }
            }
        }
    }
    let rows: Vec<(Row, f64, f64)> = cases
        .par_iter()
        .map(|c| {
            let dir = rec.as_ref().filter(|_| recorded(c)).map(|d| {
                d.join(format!(
                    "{}-{}px-{}",
                    c.name.replace([' ', '\''], "-"),
                    c.cell_px,
                    c.palette
                ))
            });
            let (changed, restless) = change_shares(&c.world);
            (run_recorded(c, 3.0, dir.as_deref()), changed, restless)
        })
        .collect();
    println!("{RESTLESS_HEADER}");
    for (r, changed, restless) in &rows {
        print_restless(r, *changed, *restless);
    }
    let gaps = rows
        .iter()
        .filter(|(r, _, _)| r.cell_px >= 3.0 && r.letter.fails() && r.shown_letter.fails())
        .count();
    let conc = rows
        .iter()
        .filter(|(r, _, _)| {
            r.cell_px >= 3.0 && r.letter.concurrent > 6 && r.shown_letter.concurrent > 6
        })
        .count();
    println!(
        "\nOf {} cases, at 3 pixels a cell and more: {gaps} fail on screen by the strictest reading \
         where the reference fails what is drawn; {conc} concurrently",
        rows.len()
    );
}

/// The reference on recordings (ignored; `JETCARP_WCAG_DIRS=DIR,DIR --ignored --nocapture`): each
/// a folder of PNGs at 60 frames a second, such as the IRIS comparison's: the most transitions in
/// a second over a quarter of a field, strictly and concurrently, the share failing, and how many
/// frames fail.
#[test]
#[ignore]
fn the_reference_on_recordings() {
    let Some(dirs) = std::env::var("JETCARP_WCAG_DIRS").ok() else {
        eprintln!("skipped: set JETCARP_WCAG_DIRS");
        return;
    };
    let dirs: Vec<&str> = dirs.split(',').collect();
    let out: Vec<String> = dirs
        .par_iter()
        .map(|dir| {
            let mut files: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "png"))
                .collect();
            files.sort();
            let mut wcag: Option<Wcag> = None;
            let (mut worst, mut fail, mut conc) = (Letter::default(), 0, 0);
            for (k, f) in files.iter().enumerate() {
                let img =
                    jetcarp_core::media::encode::decode_png(&std::fs::read(f).unwrap()).unwrap();
                let w = wcag.get_or_insert_with(|| Wcag::new(img.width, img.height));
                let now = w.update(&img.rgba, k as f64 / FPS);
                fail += now.fails() as u32;
                conc += (now.concurrent > 6) as u32;
                worst = worst.worse(now);
            }
            format!(
                "{dir}: {} frames; strict {} (general {}, red {}; {:.0}% of a field failing), \
                 concurrent {}; frames failing {fail}, concurrently {conc}",
                files.len(),
                worst.most(),
                worst.general,
                worst.red,
                worst.fail_share * 100.0,
                worst.concurrent
            )
        })
        .collect();
    for l in out {
        println!("{l}");
    }
}

/// Busy scenes by the numbers (ignored; `--ignored --nocapture`): a Life soup, Brian's Brain and
/// Lava at 3 to 16 pixels a cell in three palettes, 30 generations a second, in a window, with
/// the study's columns: what the reference and the guard's restless count read, and whether the
/// guard veils them (#289).
#[test]
#[ignore]
fn busy_scenes_by_the_numbers() {
    let mut cases = vec![];
    for &px in &[3.0, 4.0, 6.0, 8.0, 12.0, 16.0] {
        for &pal in &["Ember", "Thermal", "Paper"] {
            cases.extend(
                ordinary(px, pal)
                    .into_iter()
                    .filter(|c| c.name == "Life soup"),
            );
            cases.push(ash_case("B2/S/C3", "Brian's Brain", px, pal, SCREEN));
            cases.push(ash_case("B45678/S12345/C8", "Lava", px, pal, SCREEN));
        }
    }
    let rows: Vec<(Row, f64, f64)> = cases
        .par_iter()
        .map(|c| {
            let (changed, restless) = change_shares(&c.world);
            (run(c, 3.0), changed, restless)
        })
        .collect();
    println!("{RESTLESS_HEADER}");
    for (r, changed, restless) in &rows {
        print_restless(r, *changed, *restless);
    }
}
