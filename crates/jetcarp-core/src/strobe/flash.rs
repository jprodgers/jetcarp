//! Counting flashes and red flashes in a stream of [`TileMap`]s, as WCAG 2.3.1 defines them.
//!
//! **A transition** ([`Tracker`]): a value (a tile's mean luminance, or its red measure) moving in
//! one direction by at least the threshold since the run began, counted once per run; a change of
//! direction starts a new run. For luminance, the darker of the run's two ends must be below
//! [`DARK_LIMIT`]. This is how IRIS counts a recording's frame mean, with two differences that
//! can only count more, never fewer: a run is not cut off after one second, and a reversal
//! smaller than [`EPSILON`] (dither, rounding) does not end it.
//!
//! **Each frame** ([`FlashMeter::update`]) updates every tile's trackers, then reads:
//!
//! - *the area rule*: for each count `k`, the largest share of a 10° field ([`Viewing`]) covered
//!   by tiles that made at least `k` transitions in the last second; the reading's count is the
//!   largest `k` whose share is over [`AREA_SHARE`]. The field slides over the screen a tile at a
//!   time, and a field reaching past the edge of the view counts nothing there.
//! - *the whole screen*: the screen's mean, tracked like a tile's. This is IRIS's own measure of a
//!   recording, so a flash IRIS would count on a recording of the screen is counted here too,
//!   even in the rare case where it is spread too thinly for any tile to count it. Where the view
//!   is a part of a larger screen (the app's window), its change is weighted by its share of the
//!   screen.
//!
//! The larger of the two is the reading's transition count ([`FlashReading`]); more than
//! [`MAX_TRANSITIONS`] fails.
//!
//! **Coherence.** A flash is an area changing together, so a tile's transition counts only when at
//! least [`COHERENT`] tiles of the 3 by 3 around it (itself included) made the same transition
//! within a few frames. Small cells flickering at random (a soup of 4-pixel cells) change
//! neighbouring tiles independently, and are not a flash; a field, a large square or a band
//! flipping is. Areas are still counted a tile at a time.

use super::frame::TileMap;
use super::{
    AREA_SHARE, DARK_LIMIT, EXTENDED_SECONDS, EXTENDED_WINDOW, FLASH_DELTA, MAX_TRANSITIONS,
    RED_DELTA, Viewing, WARN_TRANSITIONS,
};
use serde::Serialize;
use std::collections::VecDeque;

/// Changes smaller than this (in luminance) neither add to a run nor end it.
pub const EPSILON: f32 = 0.002;

/// The same for the red measure (0 to 320).
pub const RED_EPSILON: f32 = 0.5;

/// Transition times a tracker keeps: more than [`MAX_TRANSITIONS`], so it can say a second held
/// more than that.
const RING: usize = 8;

/// Counts are kept up to this (one more than a failure needs).
pub const COUNT_CAP: u32 = RING as u32;

/// A tile's transition counts only when at least this many tiles of the 3 by 3 around it, itself
/// included, made the same transition within [`COHERENT_SECONDS`]: a flash is an area changing
/// together. Small cells flickering at random (a Life soup) change neighbouring tiles' means
/// independently, and do not count.
pub const COHERENT: u32 = 3;

/// How close together the agreeing transitions must be.
pub const COHERENT_SECONDS: f64 = 0.05;

/// One value's transitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tracker {
    primed: bool,
    last: f32,
    /// Where the current run began.
    start: f32,
    /// The run's direction: 1 up, -1 down, 0 none yet.
    dir: i8,
    /// The run has made its transition.
    counted: bool,
    /// When the most recent transitions were made (seconds), oldest first.
    times: [f64; RING],
    n: u8,
}

impl Tracker {
    /// Takes the value at time `t` (seconds, never decreasing) and counts the transition it makes,
    /// if it makes one. `delta` is the change a transition needs, `eps` the change that counts at
    /// all, and `dark` the limit the darker end must be below (None for the red measure). True
    /// when this made a transition.
    pub fn update(&mut self, v: f32, t: f64, delta: f32, eps: f32, dark: Option<f32>) -> bool {
        let dir = self.observe(v, delta, eps, dark);
        if dir != 0 {
            self.commit(t);
        }
        dir != 0
    }

    /// The same, without counting the transition: its direction (1 up, -1 down), or 0 for none.
    /// [`commit`](Self::commit) counts it.
    pub fn observe(&mut self, v: f32, delta: f32, eps: f32, dark: Option<f32>) -> i8 {
        if !self.primed {
            *self = Tracker {
                primed: true,
                last: v,
                start: v,
                ..Default::default()
            };
            return 0;
        }
        let d = v - self.last;
        self.last = v;
        let sign = |x: f32| if x > 0.0 { 1 } else { -1 };
        let dir = if d.abs() >= eps {
            sign(d)
        } else if self.dir != 0 {
            // A small step continues the run, whichever way it goes.
            self.dir
        } else if (v - self.start).abs() >= eps {
            // Small steps since the first value have added up to a direction.
            sign(v - self.start)
        } else {
            return 0;
        };
        if dir != self.dir {
            // The first run starts at the first value; a reversal, where the last run ended.
            if self.dir != 0 {
                self.start = v - d;
            }
            self.dir = dir;
            self.counted = false;
        }
        if self.counted {
            return 0;
        }
        let darker = self.start.min(v);
        // A hair under the threshold counts, so rounding never hides a transition.
        if (v - self.start).abs() >= delta - 1e-5 && dark.is_none_or(|l| darker < l) {
            self.counted = true;
            return dir;
        }
        0
    }

    /// Counts a transition made at `t`.
    pub fn commit(&mut self, t: f64) {
        if (self.n as usize) < RING {
            self.times[self.n as usize] = t;
            self.n += 1;
        } else {
            self.times.rotate_left(1);
            self.times[RING - 1] = t;
        }
    }

    /// Transitions in the second up to `t`: made after `t - 1` (at most [`COUNT_CAP`]). One made
    /// exactly a second before `t` is out, as IRIS counts the last `fps` frames.
    pub fn count(&self, t: f64) -> u32 {
        self.times[..self.n as usize]
            .iter()
            .filter(|&&s| s > t - 1.0 + 1e-6)
            .count() as u32
    }

    /// Forget everything (a new scene).
    pub fn reset(&mut self) {
        *self = Tracker::default();
    }
}

/// How a reading stands against the limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Under [`WARN_TRANSITIONS`] a second.
    #[default]
    Pass,
    /// [`WARN_TRANSITIONS`] to [`MAX_TRANSITIONS`] a second: two or three flashes.
    Warning,
    /// Warnings for [`EXTENDED_SECONDS`] out of the last [`EXTENDED_WINDOW`].
    Extended,
    /// More than [`MAX_TRANSITIONS`] a second: more than three flashes.
    Fail,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Pass => "pass",
            Level::Warning => "warning",
            Level::Extended => "extended",
            Level::Fail => "fail",
        }
    }
    /// A failure, sudden or sustained.
    pub fn fails(self) -> bool {
        self >= Level::Extended
    }
}

/// One frame's flash reading.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Default)]
pub struct FlashReading {
    /// The frame's time, in seconds.
    pub time: f64,
    /// General flashes: transitions in the last second, by the area rule or the whole screen,
    /// whichever is more (at most [`COUNT_CAP`]).
    pub transitions: u32,
    /// The area rule's count alone.
    pub area_transitions: u32,
    /// The whole screen's count alone.
    pub screen_transitions: u32,
    /// Red flashes: the same for the red measure.
    pub red_transitions: u32,
    /// The largest share of a 10° field covered by tiles that made at least two transitions (a
    /// flash) in the last second.
    pub share: f32,
    /// The same for red.
    pub red_share: f32,
    pub level: Level,
    pub red_level: Level,
}

impl FlashReading {
    /// The worse of the general and red levels.
    pub fn worst(&self) -> Level {
        self.level.max(self.red_level)
    }
    /// The larger of the general and red counts.
    pub fn most(&self) -> u32 {
        self.transitions.max(self.red_transitions)
    }
}

/// When a count has been at the warning level, for the extended failure.
#[derive(Clone, Debug, Default)]
struct Sustained {
    /// (start, end) of the stretches at warning or above within the last window.
    spans: VecDeque<(f64, f64)>,
}

impl Sustained {
    fn update(&mut self, t: f64, dt: f64, warning: bool) -> bool {
        if warning {
            match self.spans.back_mut() {
                Some(s) if s.1 >= t - dt - 1e-9 => s.1 = t,
                _ => self.spans.push_back(((t - dt).max(0.0), t)),
            }
        }
        while self
            .spans
            .front()
            .is_some_and(|s| s.1 < t - EXTENDED_WINDOW)
        {
            self.spans.pop_front();
        }
        let from = t - EXTENDED_WINDOW;
        let total: f64 = self.spans.iter().map(|s| s.1 - s.0.max(from)).sum();
        warning && total >= EXTENDED_SECONDS - 1e-9
    }
}

/// The flash reading of a stream of frames.
#[derive(Clone, Debug)]
pub struct FlashMeter {
    viewing: Viewing,
    /// The screen's area in pixels, for the whole screen's mean.
    screen_px: f64,
    cols: u32,
    rows: u32,
    lum: Vec<Tracker>,
    red: Vec<Tracker>,
    screen_lum: Tracker,
    screen_red: Tracker,
    sustained: Sustained,
    red_sustained: Sustained,
    last_t: Option<f64>,
    /// Each tile's count of luminance and red transitions in the last second.
    counts: Vec<u8>,
    red_counts: Vec<u8>,
    /// Each tile's latest transition, counted or not (time, direction), for coherence.
    raw: Vec<(f64, i8)>,
    red_raw: Vec<(f64, i8)>,
    last: FlashReading,
}

impl FlashMeter {
    /// A meter for a view watched as `viewing` says, which is all of a screen of `screen_px`
    /// pixels or part of it (None: the view is the whole screen).
    pub fn new(viewing: Viewing, screen_px: Option<f64>) -> FlashMeter {
        FlashMeter {
            viewing,
            screen_px: screen_px.unwrap_or(0.0),
            cols: 0,
            rows: 0,
            lum: vec![],
            red: vec![],
            screen_lum: Tracker::default(),
            screen_red: Tracker::default(),
            sustained: Sustained::default(),
            red_sustained: Sustained::default(),
            last_t: None,
            counts: vec![],
            red_counts: vec![],
            raw: vec![],
            red_raw: vec![],
            last: FlashReading::default(),
        }
    }

    pub fn viewing(&self) -> Viewing {
        self.viewing
    }

    /// Watch the view as `viewing` says from now on (another monitor, another scale).
    pub fn set_viewing(&mut self, viewing: Viewing, screen_px: Option<f64>) {
        self.viewing = viewing;
        self.screen_px = screen_px.unwrap_or(0.0);
    }

    /// Forget every frame so far.
    pub fn reset(&mut self) {
        let (v, s) = (self.viewing, self.screen_px);
        *self = FlashMeter::new(v, Some(s).filter(|s| *s > 0.0));
    }

    /// The last reading.
    pub fn reading(&self) -> FlashReading {
        self.last
    }

    /// Each tile's luminance transitions in the last second, as of the last frame.
    pub fn tile_counts(&self) -> &[u8] {
        &self.counts
    }

    /// Each tile's red transitions in the last second.
    pub fn tile_red_counts(&self) -> &[u8] {
        &self.red_counts
    }

    /// Tiles across and down, as of the last frame.
    pub fn tiles(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }

    /// Takes the frame shown at time `t` (seconds, never decreasing) and reads it.
    pub fn update(&mut self, map: &TileMap, t: f64) -> FlashReading {
        if (map.cols, map.rows) != (self.cols, self.rows) {
            // A new size: the tiles are not the same places any more.
            self.cols = map.cols;
            self.rows = map.rows;
            let n = map.len();
            self.lum = vec![Tracker::default(); n];
            self.red = vec![Tracker::default(); n];
            self.counts = vec![0; n];
            self.red_counts = vec![0; n];
            self.raw = vec![(f64::NEG_INFINITY, 0); n];
            self.red_raw = vec![(f64::NEG_INFINITY, 0); n];
        }
        let dt = self.last_t.map_or(0.0, |l| (t - l).clamp(0.0, 1.0));
        self.last_t = Some(t);
        let n = map.len();
        let mut now = vec![0i8; n];
        let mut now_red = vec![0i8; n];
        for i in 0..n {
            now[i] = self.lum[i].observe(map.lum[i], FLASH_DELTA, EPSILON, Some(DARK_LIMIT));
            now_red[i] = self.red[i].observe(map.red[i], RED_DELTA, RED_EPSILON, None);
            if now[i] != 0 {
                self.raw[i] = (t, now[i]);
            }
            if now_red[i] != 0 {
                self.red_raw[i] = (t, now_red[i]);
            }
        }
        // A transition counts when the area changes together: at least COHERENT tiles of the 3 by
        // 3 around it (itself included) made the same one within the last few frames.
        let (cols, rows) = (map.cols as i64, map.rows as i64);
        let agree = |raw: &[(f64, i8)], i: usize, dir: i8| {
            let (c, r) = (i as i64 % cols, i as i64 / cols);
            let mut k = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (c + dx, r + dy);
                    if x >= 0 && y >= 0 && x < cols && y < rows {
                        let (tt, d) = raw[(y * cols + x) as usize];
                        if d == dir && t - tt <= COHERENT_SECONDS + 1e-9 {
                            k += 1;
                        }
                    }
                }
            }
            k
        };
        for i in 0..n {
            if now[i] != 0 && agree(&self.raw, i, now[i]) >= COHERENT {
                self.lum[i].commit(t);
            }
            if now_red[i] != 0 && agree(&self.red_raw, i, now_red[i]) >= COHERENT {
                self.red[i].commit(t);
            }
        }
        let (mut any, mut any_red) = (false, false);
        for i in 0..n {
            let c = self.lum[i].count(t) as u8;
            let r = self.red[i].count(t) as u8;
            self.counts[i] = c;
            self.red_counts[i] = r;
            any |= c > 0;
            any_red |= r > 0;
        }
        // The whole screen: the view's mean, weighted by its share of the screen.
        let view_px = map.width as f64 * map.height as f64;
        let share = if self.screen_px > view_px {
            view_px / self.screen_px
        } else {
            1.0
        };
        let (ml, mr) = map.mean();
        self.screen_lum.update(
            (ml * share) as f32,
            t,
            FLASH_DELTA,
            EPSILON,
            Some(DARK_LIMIT),
        );
        self.screen_red
            .update((mr * share) as f32, t, RED_DELTA, RED_EPSILON, None);
        let screen_transitions = self.screen_lum.count(t);
        let screen_red = self.screen_red.count(t);
        let (area, flash_share) = if any {
            self.area_rule(map, &self.counts)
        } else {
            (0, 0.0)
        };
        let (red_area, red_share) = if any_red {
            self.area_rule(map, &self.red_counts)
        } else {
            (0, 0.0)
        };
        let transitions = area.max(screen_transitions);
        let red_transitions = red_area.max(screen_red);
        let level = Self::level(
            transitions,
            self.sustained
                .update(t, dt, transitions >= WARN_TRANSITIONS),
        );
        let red_level = Self::level(
            red_transitions,
            self.red_sustained
                .update(t, dt, red_transitions >= WARN_TRANSITIONS),
        );
        self.last = FlashReading {
            time: t,
            transitions,
            area_transitions: area,
            screen_transitions,
            red_transitions,
            share: flash_share,
            red_share,
            level,
            red_level,
        };
        self.last
    }

    fn level(transitions: u32, sustained: bool) -> Level {
        if transitions > MAX_TRANSITIONS {
            Level::Fail
        } else if sustained {
            Level::Extended
        } else if transitions >= WARN_TRANSITIONS {
            Level::Warning
        } else {
            Level::Pass
        }
    }

    /// The area rule over these per-tile counts: the largest `k` for which tiles with at least
    /// `k` transitions cover more than [`AREA_SHARE`] of some field, and the largest share
    /// covered by tiles with at least two.
    fn area_rule(&self, map: &TileMap, counts: &[u8]) -> (u32, f32) {
        let side = self.viewing.field_tiles() as i64;
        let field_px = self.viewing.field_area();
        let mut best = 0;
        let mut flash_share = 0.0;
        for k in 1..=COUNT_CAP as u8 {
            let w: Vec<f32> = counts
                .iter()
                .enumerate()
                .map(|(i, &c)| if c >= k { map.tile_px(i) } else { 0.0 })
                .collect();
            let most = max_window(&w, map.cols as i64, map.rows as i64, side);
            let share = (most as f64 / field_px) as f32;
            if k == 2 {
                flash_share = share;
            }
            if (share as f64) > AREA_SHARE {
                best = k as u32;
            } else if k >= 2 {
                break;
            }
        }
        (best, flash_share)
    }
}

/// The largest sum of `w` (a `cols` by `rows` grid) inside a `side` by `side` square of cells,
/// over every placement that overlaps the grid (cells outside count 0).
fn max_window(w: &[f32], cols: i64, rows: i64, side: i64) -> f64 {
    // Summed-area table with a zero row and column in front.
    let (c1, r1) = (cols + 1, rows + 1);
    let mut sat = vec![0f64; (c1 * r1) as usize];
    for y in 0..rows {
        let mut run = 0f64;
        for x in 0..cols {
            run += w[(y * cols + x) as usize] as f64;
            sat[((y + 1) * c1 + x + 1) as usize] = sat[(y * c1 + x + 1) as usize] + run;
        }
    }
    let at = |x: i64, y: i64| sat[(y.clamp(0, rows) * c1 + x.clamp(0, cols)) as usize];
    let mut best = 0f64;
    // Placements from reaching off the top left to reaching off the bottom right.
    for y0 in (1 - side)..rows {
        for x0 in (1 - side)..cols {
            let (x1, y1) = (x0 + side, y0 + side);
            let s = at(x1, y1) - at(x0, y1) - at(x1, y0) + at(x0, y0);
            if s > best {
                best = s;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strobe::{TILE, luminance};

    const W: u32 = 640;
    const H: u32 = 480;

    /// A meter for a 640 x 480 screen at a scale of 1.
    fn meter() -> FlashMeter {
        FlashMeter::new(Viewing::screen(1.0), None)
    }

    /// Frame `f` of a sequence at `fps`, with the tiles inside `rect` (pixels, tile-aligned)
    /// coloured `on(f)` and the rest `bg`.
    fn map_with(rect: [u32; 4], colour: [u8; 3], bg: [u8; 3]) -> TileMap {
        let mut m = TileMap::uniform(W, H, bg);
        let (on_l, on_r) = (luminance(colour), crate::strobe::red_measure(colour));
        for r in 0..m.rows {
            for c in 0..m.cols {
                let (x, y) = (c * TILE, r * TILE);
                if x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3] {
                    let i = (r * m.cols + c) as usize;
                    m.lum[i] = on_l;
                    m.red[i] = on_r;
                }
            }
        }
        m
    }

    /// Runs `secs` seconds at `fps`, with the area `rect` switching between `a` and `b` every
    /// `every` frames, over a background `bg`; returns the worst reading.
    fn run(
        fps: f64,
        secs: f64,
        every: u32,
        rect: [u32; 4],
        a: [u8; 3],
        b: [u8; 3],
        bg: [u8; 3],
    ) -> FlashReading {
        let mut m = meter();
        let frames = (fps * secs).round() as u32;
        let mut worst = FlashReading::default();
        let (ma, mb) = (map_with(rect, a, bg), map_with(rect, b, bg));
        for f in 0..frames {
            let map = if (f / every) % 2 == 0 { &ma } else { &mb };
            let r = m.update(map, f as f64 / fps);
            let key = |r: &FlashReading| (r.worst(), r.most(), (r.share * 1e6) as u64);
            if key(&r) > key(&worst) {
                worst = r;
            }
        }
        worst
    }

    const FULL: [u32; 4] = [0, 0, W, H];
    const BLACK: [u8; 3] = [0, 0, 0];
    const WHITE: [u8; 3] = [255, 255, 255];

    #[test]
    fn a_transition_needs_a_tenth_of_the_range_in_one_direction() {
        let mut t = Tracker::default();
        assert!(!t.update(0.3, 0.0, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        assert!(!t.update(0.399, 0.1, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        // In two steps, still one run.
        assert!(t.update(0.4001, 0.2, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        // Further in the same direction: the same transition.
        assert!(!t.update(0.9, 0.3, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        // Back down: a new run, and its own transition.
        assert!(t.update(0.6, 0.4, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        assert_eq!(t.count(0.4), 2);
        // A second later the first has dropped out of the window.
        assert_eq!(t.count(1.2), 1);
        assert_eq!(t.count(1.4), 0);
        // A reversal smaller than EPSILON does not end a run.
        let mut t = Tracker::default();
        t.update(0.3, 0.0, FLASH_DELTA, EPSILON, None);
        t.update(0.36, 0.1, FLASH_DELTA, EPSILON, None);
        t.update(0.359, 0.2, FLASH_DELTA, EPSILON, None);
        assert!(t.update(0.4, 0.3, FLASH_DELTA, EPSILON, None));
        // A real reversal does: 0.3 up to 0.36, down to 0.34, up to 0.42 is no transition.
        let mut t = Tracker::default();
        for (k, v) in [0.3f32, 0.36, 0.34, 0.42].into_iter().enumerate() {
            assert!(!t.update(v, k as f64, FLASH_DELTA, EPSILON, None), "{v}");
        }
        // A slow drift of small steps adds up.
        let mut t = Tracker::default();
        let made: Vec<bool> = (0..=120)
            .map(|k| {
                t.update(
                    0.2 + k as f32 * 0.001,
                    k as f64 / 60.0,
                    FLASH_DELTA,
                    EPSILON,
                    None,
                )
            })
            .collect();
        assert_eq!(made.iter().position(|&m| m), Some(100));
        assert_eq!(made.iter().filter(|&&m| m).count(), 1);
    }

    #[test]
    fn the_darker_state_must_be_below_point_eight() {
        // 0.80 to 0.95 and back: never a transition.
        let mut t = Tracker::default();
        for (k, v) in [0.8f32, 0.95, 0.8, 0.95, 0.8].into_iter().enumerate() {
            assert!(!t.update(v, k as f64, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        }
        // 0.79 to 0.95: each a transition.
        let mut t = Tracker::default();
        t.update(0.79, 0.0, FLASH_DELTA, EPSILON, Some(DARK_LIMIT));
        assert!(t.update(0.95, 0.1, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        assert!(t.update(0.79, 0.2, FLASH_DELTA, EPSILON, Some(DARK_LIMIT)));
        // Red has no such limit.
        let mut t = Tracker::default();
        t.update(300.0, 0.0, RED_DELTA, RED_EPSILON, None);
        assert!(t.update(320.0, 0.1, RED_DELTA, RED_EPSILON, None));
    }

    /// The full screen switching black and white every `every` frames at each frame rate: three
    /// flashes a second (six transitions) is a warning, more fails.
    #[test]
    fn three_flashes_a_second_pass_and_more_fail_at_every_frame_rate() {
        for fps in [30.0, 60.0, 120.0, 144.0] {
            // Every fps/6 frames: exactly six transitions a second.
            let at = (fps / 6.0) as u32;
            let r = run(fps, 3.0, at, FULL, BLACK, WHITE, BLACK);
            assert_eq!(r.transitions, 6, "{fps} fps, every {at}");
            assert_eq!(r.level, Level::Warning, "{fps} fps, every {at}");
            // One frame sooner: more than six in some second.
            let r = run(fps, 3.0, at - 1, FULL, BLACK, WHITE, BLACK);
            assert!(r.transitions > 6, "{fps} fps, every {}", at - 1);
            assert_eq!(r.level, Level::Fail);
            // Every frame: as many as a tracker keeps.
            let r = run(fps, 2.0, 1, FULL, BLACK, WHITE, BLACK);
            assert_eq!(r.transitions, COUNT_CAP, "{fps} fps");
            // Two flashes a second for a long time: the extended failure.
            let r = run(fps, 6.0, (fps / 4.0) as u32, FULL, BLACK, WHITE, BLACK);
            assert_eq!(r.level, Level::Extended, "{fps} fps");
            assert!((4..=5).contains(&r.transitions), "{fps} fps: {r:?}");
            // But not for three seconds.
            let r = run(fps, 3.0, (fps / 4.0) as u32, FULL, BLACK, WHITE, BLACK);
            assert_eq!(r.level, Level::Warning, "{fps} fps");
            // One flash a second never warns.
            let r = run(fps, 5.0, (fps / 2.0) as u32, FULL, BLACK, WHITE, BLACK);
            assert_eq!((r.transitions, r.level), (2, Level::Pass), "{fps} fps");
        }
    }

    #[test]
    fn a_flash_needs_a_tenth_of_the_luminance() {
        // The grey levels whose luminances are just over and just under 0.1 above grey 100.
        let a = [100u8; 3];
        let over = (101..=255u8)
            .find(|&g| luminance([g; 3]) - luminance(a) >= FLASH_DELTA)
            .unwrap();
        let (b_over, b_under) = ([over; 3], [over - 1; 3]);
        assert!(luminance(b_under) - luminance(a) < FLASH_DELTA);
        let r = run(60.0, 2.0, 1, FULL, a, b_over, a);
        assert_eq!(r.level, Level::Fail);
        let r = run(60.0, 2.0, 1, FULL, a, b_under, a);
        assert_eq!((r.transitions, r.level), (0, Level::Pass));
        // Bright flicker whose darker state is at 0.8 or above never counts.
        let (light, lighter) = ([232u8; 3], [255u8; 3]);
        assert!(luminance(light) >= 0.8);
        let r = run(60.0, 2.0, 1, FULL, light, lighter, light);
        assert_eq!(r.transitions, 0);
    }

    /// The area rule: a flashing square just under a quarter of the field passes, just over
    /// fails, and the screen's mean alone (IRIS's measure) is far from counting either.
    #[test]
    fn flashes_count_over_a_quarter_of_the_field() {
        let v = Viewing::screen(1.0);
        let allowed = v.flash_area();
        // Tile-aligned squares just under and over the allowed area.
        let side_under = ((allowed.sqrt() / TILE as f64).floor() as u32) * TILE;
        let side_over = side_under + TILE;
        assert!(((side_under * side_under) as f64) < allowed);
        assert!(((side_over * side_over) as f64) > allowed);
        let r = run(
            60.0,
            2.0,
            2,
            [64, 64, 64 + side_under, 64 + side_under],
            BLACK,
            WHITE,
            BLACK,
        );
        assert_eq!(r.level, Level::Pass, "{r:?}");
        assert_eq!(r.screen_transitions, 0);
        assert!(r.share < 0.25 && r.share > 0.2, "{}", r.share);
        let r = run(
            60.0,
            2.0,
            2,
            [64, 64, 64 + side_over, 64 + side_over],
            BLACK,
            WHITE,
            BLACK,
        );
        assert_eq!(r.level, Level::Fail, "{r:?}");
        assert_eq!(r.screen_transitions, 0, "the mean changes by under 0.1");
        assert!(r.share > 0.25, "{}", r.share);
        // A thin strip larger than that does not fill a quarter of any field, so it passes.
        let strip = [0, 0, W, 2 * TILE];
        assert!((W * 2 * TILE) as f64 > allowed);
        let r = run(60.0, 2.0, 2, strip, BLACK, WHITE, BLACK);
        assert_eq!(r.level, Level::Pass, "{r:?}");
    }

    #[test]
    fn a_view_smaller_than_the_field_counts_against_the_whole_field() {
        // A 96 x 96 view flashing all over is under a quarter of a 265-pixel field.
        let mut m = FlashMeter::new(Viewing::screen(1.0), Some(1920.0 * 1080.0));
        let (a, b) = (
            TileMap::uniform(96, 96, BLACK),
            TileMap::uniform(96, 96, WHITE),
        );
        let mut r = FlashReading::default();
        for f in 0..120 {
            r = m.update(if f % 2 == 0 { &a } else { &b }, f as f64 / 60.0);
        }
        assert_eq!(r.level, Level::Pass, "{r:?}");
        // A 160 x 160 view is over.
        let mut m = FlashMeter::new(Viewing::screen(1.0), Some(1920.0 * 1080.0));
        let (a, b) = (
            TileMap::uniform(160, 160, BLACK),
            TileMap::uniform(160, 160, WHITE),
        );
        for f in 0..120 {
            r = m.update(if f % 2 == 0 { &a } else { &b }, f as f64 / 60.0);
        }
        assert_eq!(r.level, Level::Fail, "{r:?}");
        assert_eq!(r.screen_transitions, 0, "1% of the screen");
    }

    #[test]
    fn the_screen_mean_counts_a_flash_spread_too_thin_for_the_area_rule() {
        // Every tile changes by 0.099, just under a tile's threshold: nothing anywhere.
        let lo = TileMap::uniform(W, H, [60; 3]);
        let mut hi = lo.clone();
        hi.lum.iter_mut().for_each(|l| *l += 0.099);
        let mut m = meter();
        let mut r = FlashReading::default();
        for f in 0..60 {
            r = m.update(if f % 2 == 0 { &lo } else { &hi }, f as f64 / 60.0);
        }
        assert_eq!(r.transitions, 0);
        // One tile in seven changes by 0.9 as well: those tiles flash, but they cover under a
        // quarter of any field. The screen's mean changes by 0.21, which IRIS would count.
        let mut hi = lo.clone();
        for (i, l) in hi.lum.iter_mut().enumerate() {
            *l += if i % 7 == 0 { 0.9 } else { 0.099 };
        }
        let mut m = meter();
        for f in 0..60 {
            r = m.update(if f % 2 == 0 { &lo } else { &hi }, f as f64 / 60.0);
        }
        assert_eq!(r.area_transitions, 0, "{r:?}");
        assert!(r.screen_transitions > 6, "{r:?}");
        assert_eq!(r.level, Level::Fail);
    }

    #[test]
    fn red_flashes_are_counted_apart() {
        let red = [255u8, 0, 0];
        let dark_red = [200u8, 0, 0];
        // Red and black: a red flash, and a general one (red is 0.21 luminance).
        let r = run(60.0, 2.0, 1, FULL, BLACK, red, BLACK);
        assert_eq!((r.red_level, r.level), (Level::Fail, Level::Fail));
        // Red and a dark red 0.07 darker: under the luminance threshold, over the red one.
        assert!(luminance(red) - luminance(dark_red) < 0.1);
        let r = run(60.0, 2.0, 1, FULL, dark_red, red, BLACK);
        assert_eq!((r.red_level, r.level), (Level::Fail, Level::Pass), "{r:?}");
        // Two reds 19 apart in the measure: nothing.
        let (a, b) = ([250u8, 0, 0], [255u8, 0, 0]);
        assert!(crate::strobe::red_measure(b) - crate::strobe::red_measure(a) < RED_DELTA);
        let r = run(60.0, 2.0, 1, FULL, a, b, BLACK);
        assert_eq!(r.red_transitions, 0);
        // Pink is not saturated: no red flash, though its luminance flashes.
        let r = run(60.0, 2.0, 1, FULL, BLACK, [255, 140, 140], BLACK);
        assert_eq!((r.red_transitions, r.level), (0, Level::Fail));
    }

    #[test]
    fn the_window_slides_a_second_at_a_time() {
        // Seven transitions spread over a little more than a second: never seven in one second.
        let mut m = meter();
        let (a, b) = (TileMap::uniform(W, H, BLACK), TileMap::uniform(W, H, WHITE));
        let mut r = FlashReading::default();
        let step = 1.02 / 6.0;
        for k in 0..=7 {
            r = r.max_by(m.update(if k % 2 == 0 { &b } else { &a }, k as f64 * step));
        }
        // The first frame primes; transitions at frames 1..=6.
        assert_eq!(r.transitions, 6, "{r:?}");
        // Packed into under a second: seven.
        let mut m = meter();
        let mut r = FlashReading::default();
        m.update(&a, 0.0);
        for k in 1..=7 {
            r = r.max_by(m.update(if k % 2 == 0 { &a } else { &b }, k as f64 * 0.98 / 7.0));
        }
        assert_eq!((r.transitions, r.level), (7, Level::Fail));
    }

    impl FlashReading {
        fn max_by(self, other: FlashReading) -> FlashReading {
            if other.transitions >= self.transitions {
                other
            } else {
                self
            }
        }
    }

    #[test]
    fn the_same_flashing_reads_the_same_at_any_frame_rate() {
        // 2.5 flashes a second for 4 seconds: a warning at every rate, never a failure.
        for fps in [30.0, 60.0, 120.0, 144.0] {
            let mut m = meter();
            let (a, b) = (TileMap::uniform(W, H, BLACK), TileMap::uniform(W, H, WHITE));
            let mut worst = Level::Pass;
            let mut most = 0;
            for f in 0..(fps as u32 * 4) {
                let t = f as f64 / fps;
                let on = ((t * 5.0).floor() as u32) % 2 == 1;
                let r = m.update(if on { &b } else { &a }, t);
                worst = worst.max(r.level);
                most = most.max(r.transitions);
            }
            assert_eq!(worst, Level::Warning, "{fps} fps");
            assert_eq!(most, 5, "{fps} fps");
        }
    }

    /// From RGBA frames through the processor's reduction: a checkerboard of squares larger than a
    /// tile reversing every frame flashes everywhere; one of 4-pixel squares keeps every tile's
    /// mean, so it is no flash (the stripe check's business).
    #[test]
    fn a_reversing_checkerboard_flashes_when_its_squares_are_large() {
        let board = |sq: u32, phase: u32| {
            let mut v = Vec::with_capacity((W * H * 4) as usize);
            for y in 0..H {
                for x in 0..W {
                    let on = ((x / sq + y / sq + phase) % 2) == 1;
                    let c = if on { 255 } else { 0 };
                    v.extend_from_slice(&[c, c, c, 255]);
                }
            }
            crate::strobe::frame::reduce(&v, W, H)
        };
        for (sq, fails) in [(32, true), (64, true), (4, false), (8, false)] {
            let (a, b) = (board(sq, 0), board(sq, 1));
            let mut m = meter();
            let mut r = FlashReading::default();
            for f in 0..60 {
                r = m.update(if f % 2 == 0 { &a } else { &b }, f as f64 / 60.0);
            }
            assert_eq!(r.level.fails(), fails, "{sq}-pixel squares: {r:?}");
            assert_eq!(r.screen_transitions, 0, "the mean never changes");
        }
    }

    /// Tiles flickering each on its own (a soup of small cells) are not a flash, though each tile
    /// makes transitions; the same tiles flipping together are.
    #[test]
    fn only_areas_changing_together_flash() {
        let mut rng = crate::rng::Rng::new(9);
        let base = TileMap::uniform(W, H, [30; 3]);
        let mut lit = vec![false; base.len()];
        let mut m = meter();
        let mut r = FlashReading::default();
        let mut worst = 0;
        for f in 0..120 {
            // Each tile, independently, switches between dark and bright now and then, as tiles of
            // a soup of small cells do.
            for l in lit.iter_mut() {
                if rng.below(20) == 0 {
                    *l = !*l;
                }
            }
            let mut map = base.clone();
            for (l, on) in map.lum.iter_mut().zip(&lit) {
                *l += if *on { 0.3 } else { 0.0 };
            }
            r = m.update(&map, f as f64 / 60.0);
            worst = worst.max(r.area_transitions);
        }
        assert!(worst <= 2, "independent flicker: {worst} ({r:?})");
        // The same quarter of the tiles, a block of them, flipping together.
        let mut m = meter();
        for f in 0..60 {
            let mut map = base.clone();
            if f % 2 == 1 {
                for (i, l) in map.lum.iter_mut().enumerate() {
                    let (c, rr) = (i as u32 % map.cols, i as u32 / map.cols);
                    if c < 20 && rr < 15 {
                        *l += 0.3;
                    }
                }
            }
            r = m.update(&map, f as f64 / 60.0);
        }
        assert_eq!(r.level, Level::Fail, "{r:?}");
    }

    #[test]
    fn a_new_size_starts_the_tiles_again_and_reset_forgets() {
        let mut m = meter();
        let (a, b) = (TileMap::uniform(W, H, BLACK), TileMap::uniform(W, H, WHITE));
        for f in 0..30 {
            m.update(if f % 2 == 0 { &a } else { &b }, f as f64 / 60.0);
        }
        assert!(m.reading().level.fails());
        assert_eq!(m.tiles(), (40, 30));
        m.reset();
        assert_eq!(m.update(&a, 1.0).transitions, 0);
        assert_eq!(max_window(&[1.0; 6], 3, 2, 2), 4.0);
        assert_eq!(max_window(&[1.0; 6], 3, 2, 5), 6.0);
    }
}
