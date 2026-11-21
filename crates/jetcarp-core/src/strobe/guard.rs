//! The strobe guard's decisions (#148): from the readings of each frame as drawn and as shown, what
//! to do about it, in order.
//!
//! 1. **Soften.** As soon as a quarter of a 10° field (or the screen's mean) has made
//!    [`ACT_TRANSITIONS`] transitions in the last second, the tiles that are changing, and the
//!    tiles around them, are softened ([`Plan`]): their brightness may change only so fast, so a
//!    flip becomes a fade and what is shown stays under three transitions a second whatever is
//!    drawn. The readings come back a frame or two after the frame was shown, so acting at two
//!    transitions leaves room for the few that reach the screen before the softening does. A tile
//!    stays softened for [`HOLD`] after it last changed.
//! 2. **Cap the pace.** A scene that is still flashing after [`SLOW_AFTER`] of softening is
//!    slowed, so that it can be seen again rather than as a wash: its generations a second are
//!    capped at what would bring it to [`PACE_GOAL`] transitions a second, and lowered again every
//!    [`RECAP_AFTER`] while it still flashes, down to [`MIN_PACE`]. Under the cap the scene's
//!    flashing is under the limit on its own, and the softening lets go once it is calm.
//! 3. **Pause**, with a note saying why, when neither is enough: the scene still flashes at the
//!    slowest pace for [`PAUSE_AFTER`] (its flashing does not come from its generations), or more
//!    than three flashes a second reached the screen.
//!
//! **Stripes.** A harmful pattern ([`super::pattern`]) in the frames as drawn is veiled where it
//! was found: blended towards a dim grey ([`veil_for`]) until its contrast is [`VEIL_CONTRAST`].
//! The veil comes on and goes at [`VEIL_RATE`] and stays [`HOLD`] after the pattern has gone, so it
//! never flashes itself. The stripe check runs every [`PATTERN_EVERY`].
//!
//! The guard changes only what is shown and how fast generations are shown; a world computes the
//! same generations either way. Switched off, it still measures, so the status can say a scene
//! flashes, but it does nothing.

use super::flash::{COUNT_CAP, FlashMeter, FlashReading, Level};
use super::frame::{LumImage, TileMap};
use super::pattern::{PatternLevel, PatternMeter, PatternReading};
use super::soften::Plan;
use super::{MAX_TRANSITIONS, Viewing, WARN_TRANSITIONS};
use serde::Serialize;

/// Transitions in a second (over a quarter of a field, or the screen's mean) that start the
/// softening.
pub const ACT_TRANSITIONS: u32 = 2;

/// A tile stays softened this many seconds after it last changed.
pub const HOLD: f64 = 1.5;

/// Seconds of softening a still-flashing scene gets before it is slowed.
pub const SLOW_AFTER: f64 = 1.0;

/// Transitions a second a capped pace aims at: one flash a second.
pub const PACE_GOAL: f32 = 2.0;

/// The slowest pace the guard slows a scene to, in generations a second, before pausing it.
pub const MIN_PACE: f32 = 0.5;

/// Seconds between one lowering of the cap and the next.
pub const RECAP_AFTER: f64 = 1.5;

/// Seconds a scene may go on flashing at [`MIN_PACE`] before it is paused.
pub const PAUSE_AFTER: f64 = 2.0;

/// The contrast a veil leaves strong stripes with: under the stripe check's threshold.
pub const VEIL_CONTRAST: f32 = 0.04;

/// How fast a veil comes on and goes, in its amount a second: over an eighth of a second, so that
/// stripes are veiled well within the half second they are allowed, and the change it makes to a
/// tile's mean is one transition, never a flash (the veil holds for [`HOLD`] after the stripes go).
pub const VEIL_RATE: f32 = 8.0;

/// A veil blends towards a grey no brighter than this: veiled stripes are dim as well as low in
/// contrast, so that a tool that stretches contrast before it looks (as IRIS does, counting a
/// pattern only when its light parts are at least 0.25) sees none either.
pub const VEIL_GREY_MAX: f32 = 0.18;

/// The veil for stripes of `contrast` around a mean luminance of `mean`: how far towards grey, and
/// the grey, so that what is left of them has [`VEIL_CONTRAST`] at most.
pub fn veil_for(contrast: f32, mean: f32) -> [f32; 2] {
    [
        1.0 - (VEIL_CONTRAST / contrast.max(1e-6)).min(1.0),
        mean.min(VEIL_GREY_MAX),
    ]
}

/// Seconds between stripe checks.
pub const PATTERN_EVERY: f64 = 0.25;

/// One frame's readings: the frame as drawn, as shown, and its half image when it was checked for
/// stripes.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// When it was shown, in seconds (never decreasing).
    pub time: f64,
    pub input: &'a TileMap,
    pub output: &'a TileMap,
    pub half: Option<&'a LumImage>,
}

/// How fast the scene runs, for capping it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pace {
    /// Generations a second it asks for (0: as fast as it can).
    pub speed: f32,
    /// Generations a second it reached lately, if known.
    pub reached: Option<f32>,
    /// Frames a second it is drawn at.
    pub frame_rate: f32,
}

/// The furthest the guard has gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Watching: nothing to do.
    #[default]
    None,
    /// Softening flashes or veiling stripes.
    Softening,
    /// Slowing the scene (and maybe softening).
    Slowed,
    /// Paused it.
    Paused,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Softening => "softening",
            Action::Slowed => "slowed",
            Action::Paused => "paused",
        }
    }
}

/// What the guard is doing, for status lines, reports and the tools.
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
pub struct Report {
    /// The guard is on.
    pub on: bool,
    pub action: Action,
    /// The share of the view softened or veiled.
    pub softened: f32,
    /// The pace cap, in generations a second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<f32>,
    /// Why it paused the scene.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
    /// The frames as drawn.
    pub drawn: FlashReading,
    /// The frames as shown.
    pub shown: FlashReading,
    pub stripes: PatternReading,
    /// A sentence for the status line, when there is something to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The guard's state for one view.
#[derive(Clone, Debug)]
pub struct Guard {
    on: bool,
    can_soften: bool,
    input: FlashMeter,
    output: FlashMeter,
    pattern: PatternMeter,
    plan: Plan,
    /// Each tile is softened until this time.
    hold: Vec<f64>,
    /// Each tile's veil target (amount, grey), and until when it holds.
    veil_target: Vec<([f32; 2], f64)>,
    last_t: Option<f64>,
    softening_since: Option<f64>,
    cap: Option<f32>,
    cap_at: f64,
    floor_since: Option<f64>,
    paused: Option<String>,
    last_pattern: Option<f64>,
    notes: Vec<String>,
    /// The worst the scene drew since the cap was last set (for the note).
    worst: u32,
}

/// "4 or more times a second", "3 times a second": flashes, from transitions.
fn flashes(transitions: u32) -> String {
    if transitions >= COUNT_CAP {
        format!("{} or more times a second", COUNT_CAP / 2)
    } else {
        let f = transitions as f32 / 2.0;
        if f.fract() == 0.0 {
            format!("{f:.0} times a second")
        } else {
            format!("{f:.1} times a second")
        }
    }
}

/// A pace in words: "2 generations a second", "0.5 generations a second".
fn pace_words(g: f32) -> String {
    if g >= 10.0 || g.fract() == 0.0 {
        format!("{g:.0} generations a second")
    } else {
        format!("{g:.1} generations a second")
    }
}

impl Guard {
    /// A guard for a view watched as `viewing` says, all of a screen or part of one of
    /// `screen_px` pixels; `on` as the person's setting says.
    pub fn new(viewing: Viewing, screen_px: Option<f64>, on: bool) -> Guard {
        Guard {
            on,
            can_soften: true,
            input: FlashMeter::new(viewing, screen_px),
            output: FlashMeter::new(viewing, screen_px),
            pattern: PatternMeter::for_viewing(viewing, screen_px),
            plan: Plan::default(),
            hold: vec![],
            veil_target: vec![],
            last_t: None,
            softening_since: None,
            cap: None,
            cap_at: f64::NEG_INFINITY,
            floor_since: None,
            paused: None,
            last_pattern: None,
            notes: vec![],
            worst: 0,
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Switch it on or off (the person's setting changed).
    pub fn set_on(&mut self, on: bool) {
        if on != self.on {
            self.on = on;
            if !on {
                self.lift();
                self.plan = Plan::none(self.plan.cols, self.plan.rows);
                self.hold.fill(f64::NEG_INFINITY);
                self.softening_since = None;
            }
        }
    }

    /// The view is watched another way (another monitor, another scale).
    pub fn set_viewing(&mut self, viewing: Viewing, screen_px: Option<f64>) {
        if viewing != self.input.viewing() {
            self.input.set_viewing(viewing, screen_px);
            self.output.set_viewing(viewing, screen_px);
            let at_risk = self.pattern.at_risk();
            self.pattern = PatternMeter::for_viewing(viewing, screen_px);
            self.pattern.set_at_risk(at_risk);
        }
    }

    /// The person watching answered the seizure warning *at risk*: patterns of similar parts are
    /// veiled as stripes are, as IRIS's pattern check counts them ([`super::similar`]).
    pub fn set_at_risk(&mut self, at_risk: bool) {
        self.pattern.set_at_risk(at_risk);
    }

    pub fn at_risk(&self) -> bool {
        self.pattern.at_risk()
    }

    /// Whether the view can carry out softening (a very large frame's state may not fit on the
    /// GPU). Without it the guard slows and pauses sooner.
    pub fn set_can_soften(&mut self, can: bool) {
        self.can_soften = can;
    }

    /// Whether the frame shown at `t` should be checked for stripes (its half image wanted).
    pub fn wants_half(&self, t: f64) -> bool {
        self.last_pattern
            .is_none_or(|l| t - l >= PATTERN_EVERY - 1e-6)
    }

    /// What to do to the next frames drawn.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The pace cap, in generations a second.
    pub fn cap(&self) -> Option<f32> {
        if self.on { self.cap } else { None }
    }

    /// Why the scene is paused, if the guard paused it.
    pub fn paused(&self) -> Option<&str> {
        self.paused.as_deref().filter(|_| self.on)
    }

    /// The person started a paused scene again: watch it afresh, keeping the cap.
    pub fn resume(&mut self) {
        self.paused = None;
        self.floor_since = None;
    }

    /// Another scene is on show: lift the cap and the pause. Softening stays until the tiles calm,
    /// so a new scene that flashes as well is softened at once.
    pub fn new_scene(&mut self) {
        self.lift();
    }

    fn lift(&mut self) {
        self.cap = None;
        self.cap_at = f64::NEG_INFINITY;
        self.floor_since = None;
        self.paused = None;
        self.worst = 0;
    }

    /// Notes of what the guard did since the last call, for logs and scene notes.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    fn note(&mut self, s: String) {
        if self.notes.len() < 16 {
            self.notes.push(s);
        }
    }

    /// Takes one frame's readings, and the pace the scene runs at.
    pub fn observe(&mut self, f: Frame<'_>, pace: Pace) {
        let t = f.time;
        let drawn = self.input.update(f.input, t);
        let shown = self.output.update(f.output, t);
        if let Some(h) = f.half {
            self.pattern.update(h, t);
            self.last_pattern = Some(t);
        }
        let (cols, rows) = (f.input.cols, f.input.rows);
        let dt = self.last_t.map_or(0.0, |l| (t - l).clamp(0.0, 0.25)) as f32;
        self.last_t = Some(t);
        if !self.plan.fits(cols, rows) {
            self.plan = Plan::none(cols, rows);
            self.hold = vec![f64::NEG_INFINITY; (cols * rows) as usize];
            self.veil_target = vec![([0.0, 0.0], f64::NEG_INFINITY); (cols * rows) as usize];
        }
        if !self.on {
            return;
        }
        let threshold = if self.cap.is_some() {
            ACT_TRANSITIONS + 1
        } else {
            ACT_TRANSITIONS
        };
        let most = drawn.transitions.max(drawn.red_transitions);
        let flashing = most >= threshold;
        // 1. Soften the tiles that change, and those around them.
        if flashing && self.can_soften {
            let n = self.hold.len();
            let spread_thin = drawn.area_transitions.max(drawn.red_transitions) < threshold;
            let counts = self.input.tile_counts();
            let reds = self.input.tile_red_counts();
            for i in 0..n {
                if spread_thin || counts[i] > 0 || reds[i] > 0 {
                    let (c, r) = ((i as u32 % cols) as i64, (i as u32 / cols) as i64);
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (x, y) = (c + dx, r + dy);
                            if x >= 0 && y >= 0 && (x as u32) < cols && (y as u32) < rows {
                                self.hold[(y as u32 * cols + x as u32) as usize] = t + HOLD;
                            }
                        }
                    }
                }
            }
        }
        let mut any = false;
        for (s, h) in self.plan.soften.iter_mut().zip(&self.hold) {
            *s = *h >= t;
            any |= *s;
        }
        match (any, self.softening_since) {
            (true, None) => {
                self.softening_since = Some(t);
                let share = self.plan.soften.iter().filter(|s| **s).count() as f32
                    / self.plan.soften.len().max(1) as f32;
                self.note(format!(
                    "softening: the scene flashed {} over {:.0}% of the view",
                    flashes(most),
                    share * 100.0
                ));
            }
            (false, Some(_)) => self.softening_since = None,
            _ => {}
        }
        // 2. Veil strong stripes, where the last check of the frames as drawn found them.
        let stripes = self.pattern.reading();
        let found = self.pattern.found();
        let harmful = stripes.level >= PatternLevel::Harmful && found.cols == cols;
        let was_veiled = self.plan.veil.iter().any(|v| v[0] > 0.0);
        if f.half.is_some() && harmful {
            for (i, vt) in self.veil_target.iter_mut().enumerate() {
                if let Some((contrast, mean)) = found.tiles.get(i).copied().flatten() {
                    *vt = (veil_for(contrast, mean), t + HOLD);
                }
            }
        }
        let step = VEIL_RATE * dt;
        for (v, (target, until)) in self.plan.veil.iter_mut().zip(&self.veil_target) {
            let want = if *until >= t { target[0] } else { 0.0 };
            if want > 0.0 {
                v[1] = target[1];
            }
            v[0] += (want - v[0]).clamp(-step, step);
            if v[0] <= 0.0 {
                *v = [0.0, 0.0];
            }
        }
        if harmful && !was_veiled && self.plan.veil.iter().any(|v| v[0] > 0.0) {
            self.note(if stripes.similar {
                format!(
                    "softening a pattern of {} similar parts over {:.0}% of the view (the \
                     stricter guard for someone at risk)",
                    stripes.parts,
                    stripes.share * 100.0
                )
            } else {
                format!(
                    "softening strong stripes over {:.0}% of the view",
                    stripes.share * 100.0
                )
            });
        }
        // 3. Cap the pace.
        self.worst = self.worst.max(most);
        let softened_long = self
            .softening_since
            .is_some_and(|s| t - s >= SLOW_AFTER - 1e-9);
        let shown_most = shown.transitions.max(shown.red_transitions);
        let shown_bad =
            shown_most > MAX_TRANSITIONS || (softened_long && shown_most >= WARN_TRANSITIONS);
        let needs_slowing = (flashing && (softened_long || !self.can_soften)) || shown_bad;
        if self.paused.is_none() && needs_slowing && t - self.cap_at >= RECAP_AFTER - 1e-9 {
            let reached = pace
                .reached
                .filter(|r| *r > 0.0)
                .or(Some(pace.speed).filter(|s| *s > 0.0))
                .unwrap_or(pace.frame_rate.max(1.0));
            let now = self.cap.map_or(reached, |c| c.min(reached));
            if now <= MIN_PACE + 1e-3 {
                self.floor_since.get_or_insert(t);
            } else {
                let aim = if most >= COUNT_CAP || most == 0 {
                    PACE_GOAL
                } else {
                    now * PACE_GOAL / most as f32
                };
                let next = aim.min(now * 0.5).max(MIN_PACE);
                self.cap = Some(next);
                self.cap_at = t;
                self.note(format!(
                    "slowed to {}: the scene flashed {}",
                    pace_words(next),
                    flashes(self.worst)
                ));
            }
        }
        if !flashing {
            self.floor_since = None;
        }
        // 4. Pause.
        let at_floor_long = self
            .floor_since
            .is_some_and(|s| t - s >= PAUSE_AFTER - 1e-9);
        let overrun = shown.level == Level::Fail || shown.red_level == Level::Fail;
        let stuck = overrun && (!self.can_soften || self.cap.is_some_and(|c| c <= MIN_PACE));
        if self.paused.is_none() && (at_floor_long || stuck) {
            let why = if at_floor_long {
                format!(
                    "the scene kept flashing {} even at {}",
                    flashes(most),
                    pace_words(MIN_PACE)
                )
            } else {
                format!(
                    "the scene flashed {} on screen, more than three times a second",
                    flashes(shown_most)
                )
            };
            self.note(format!("paused: {why}"));
            self.paused = Some(why);
        }
    }

    /// What the guard is doing.
    pub fn report(&self) -> Report {
        let drawn = self.input.reading();
        let shown = self.output.reading();
        let stripes = self.pattern.reading();
        let softened = if self.on { self.plan.share() } else { 0.0 };
        let action = if !self.on {
            Action::None
        } else if self.paused.is_some() {
            Action::Paused
        } else if self.cap.is_some() {
            Action::Slowed
        } else if self.plan.active() {
            Action::Softening
        } else {
            Action::None
        };
        let veiled = self.on && self.plan.veil.iter().any(|v| v[0] > 0.0);
        let note = match action {
            Action::Paused => Some(format!(
                "Strobe guard: paused, because {}.",
                self.paused.as_deref().unwrap_or("the scene flashed")
            )),
            Action::Slowed => Some(format!(
                "Strobe guard: slowed to {}, because the scene flashed {}.",
                pace_words(self.cap.unwrap_or(MIN_PACE)),
                flashes(self.worst.max(drawn.most()))
            )),
            Action::Softening if veiled && !self.plan.soften.iter().any(|s| *s) => {
                Some(if stripes.similar {
                    format!(
                        "Strobe guard: softening a pattern of similar parts over {:.0}% of the \
                         view (stricter for someone at risk).",
                        stripes.share * 100.0
                    )
                } else {
                    format!(
                        "Strobe guard: softening strong stripes over {:.0}% of the view.",
                        stripes.share * 100.0
                    )
                })
            }
            Action::Softening => Some(format!(
                "Strobe guard: softening flashes over {:.0}% of the view.",
                softened * 100.0
            )),
            Action::None if !self.on && drawn.worst().fails() => Some(format!(
                "Strobe guard off: this scene flashes {}.",
                flashes(drawn.most())
            )),
            Action::None if !self.on && stripes.level == PatternLevel::Fail => {
                Some("Strobe guard off: this scene shows strong stripes.".into())
            }
            Action::None => None,
        };
        Report {
            on: self.on,
            action,
            softened,
            cap: self.cap(),
            paused: self.paused().map(str::to_string),
            drawn,
            shown,
            stripes,
            note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strobe::frame::reduce_both;
    use crate::strobe::soften::Softener;
    use std::collections::VecDeque;

    /// A simulated screen: a scene whose frame depends on the generation shown, drawn at `fps`,
    /// with its generations running at `speed` (or the guard's cap), softened by the processor's
    /// softener, read back `latency` frames late. Returns the guard and the worst reading of what
    /// was shown.
    struct Sim {
        guard: Guard,
        soft: Softener,
        fps: f64,
        speed: f32,
        latency: usize,
        t: f64,
        gen_f: f64,
        queue: VecDeque<(f64, TileMap, TileMap, Option<LumImage>)>,
        worst_shown: u32,
        shown: FlashMeter,
        paused_at: Option<f64>,
    }

    const W: u32 = 320;
    const H: u32 = 240;

    impl Sim {
        fn new(fps: f64, speed: f32, latency: usize, on: bool) -> Sim {
            Sim {
                guard: Guard::new(Viewing::screen(1.0), None, on),
                soft: Softener::new(),
                fps,
                speed,
                latency,
                t: 0.0,
                gen_f: 0.0,
                queue: VecDeque::new(),
                worst_shown: 0,
                shown: FlashMeter::new(Viewing::screen(1.0), None),
                paused_at: None,
            }
        }

        /// Runs `secs` seconds of frames drawn by `draw(generation, frame)`.
        fn run(&mut self, secs: f64, draw: &dyn Fn(u64, u64) -> Vec<u8>) {
            let frames = (secs * self.fps).round() as u64;
            let dt = 1.0 / self.fps;
            for _ in 0..frames {
                let pace = self.guard.cap().map_or(self.speed, |c| c.min(self.speed));
                if self.guard.paused().is_none() {
                    self.gen_f += pace as f64 * dt;
                } else {
                    self.paused_at.get_or_insert(self.t);
                }
                let frame_no = (self.t * self.fps).round() as u64;
                let rgba = draw(self.gen_f as u64, frame_no);
                let plan = self.guard.plan().clone();
                let s = self.soft.apply(&rgba, W, H, &plan, dt as f32);
                // The stripe check reads the frame as drawn.
                let half = self
                    .guard
                    .wants_half(self.t)
                    .then(|| reduce_both(&rgba, W, H).1);
                // What reached the screen, measured on its own.
                let r = self.shown.update(&s.output, self.t);
                self.worst_shown = self.worst_shown.max(r.transitions.max(r.red_transitions));
                self.queue.push_back((self.t, s.input, s.output, half));
                while self.queue.len() > self.latency {
                    let (t, i, o, h) = self.queue.pop_front().unwrap();
                    self.guard.observe(
                        Frame {
                            time: t,
                            input: &i,
                            output: &o,
                            half: h.as_ref(),
                        },
                        Pace {
                            speed: self.speed,
                            reached: Some(pace),
                            frame_rate: self.fps as f32,
                        },
                    );
                }
                self.t += dt;
            }
        }
    }

    fn solid(c: [u8; 3]) -> Vec<u8> {
        let mut v = Vec::with_capacity((W * H * 4) as usize);
        for _ in 0..W * H {
            v.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        v
    }

    /// A B0 rule's field: black and white by generation.
    fn b0(g: u64, _f: u64) -> Vec<u8> {
        solid(if g % 2 == 0 { [0; 3] } else { [255; 3] })
    }

    #[test]
    fn twice_the_frame_rate_shows_one_phase_and_needs_nothing() {
        // A B0 field at 60 generations a second drawn at 30 frames: every frame the same phase.
        let mut sim = Sim::new(30.0, 60.0, 2, true);
        sim.run(3.0, &b0);
        assert_eq!(sim.guard.report().action, Action::None);
        assert_eq!(sim.worst_shown, 0);
    }

    #[test]
    fn a_whole_field_flipping_is_softened_then_slowed_and_never_flashes_on_screen() {
        for (fps, latency) in [(30.0, 2), (60.0, 2), (120.0, 3), (144.0, 4)] {
            // A generation a frame: every frame flips.
            let mut sim = Sim::new(fps, fps as f32, latency, true);
            sim.run(1.0, &b0);
            let r = sim.guard.report();
            assert!(r.action >= Action::Softening, "{fps}: {r:?}");
            assert!(
                sim.worst_shown <= MAX_TRANSITIONS,
                "{fps} fps: {}",
                sim.worst_shown
            );
            sim.run(9.0, &b0);
            let r = sim.guard.report();
            assert_eq!(r.action, Action::Slowed, "{fps}: {r:?}");
            let cap = r.cap.unwrap();
            assert!(cap <= 3.0 && cap >= MIN_PACE, "{fps}: cap {cap}");
            assert!(
                sim.worst_shown <= MAX_TRANSITIONS,
                "{fps} fps: {}",
                sim.worst_shown
            );
            assert!(sim.paused_at.is_none(), "{fps}: paused");
            // Slowed, the field flips at most PACE_GOAL... times a second: soft no longer needed.
            sim.run(5.0, &b0);
            let r = sim.guard.report();
            assert!(r.drawn.transitions <= 3, "{fps}: {r:?}");
            assert_eq!(r.softened, 0.0, "{fps}: softening let go: {r:?}");
            assert!(r.note.unwrap().starts_with("Strobe guard: slowed to"));
            let notes = sim.guard.take_notes();
            assert!(
                notes[0].starts_with("softening: the scene flashed"),
                "{notes:?}"
            );
            assert!(
                notes.iter().any(|n| n.starts_with("slowed to")),
                "{notes:?}"
            );
        }
    }

    #[test]
    fn flashing_that_slowing_cannot_stop_is_paused_with_a_note() {
        // The field flips every frame whatever the generation (a palette cycling, say).
        let mut sim = Sim::new(60.0, 30.0, 2, true);
        sim.run(20.0, &|_, f| {
            solid(if f % 2 == 0 { [0; 3] } else { [255; 3] })
        });
        let r = sim.guard.report();
        assert_eq!(r.action, Action::Paused, "{r:?}");
        assert!(
            r.paused
                .as_deref()
                .unwrap()
                .contains("even at 0.5 generations a second"),
            "{r:?}"
        );
        assert!(r.note.unwrap().starts_with("Strobe guard: paused, because"));
        assert!(sim.worst_shown <= MAX_TRANSITIONS);
        // Another scene lifts the pause and the cap.
        sim.guard.new_scene();
        assert_eq!(sim.guard.paused(), None);
        assert_eq!(sim.guard.cap(), None);
    }

    #[test]
    fn without_softening_a_flashing_scene_is_slowed_and_paused_at_once() {
        let mut sim = Sim::new(60.0, 60.0, 2, true);
        sim.guard.set_can_soften(false);
        sim.run(3.0, &|_, f| {
            solid(if f % 2 == 0 { [0; 3] } else { [255; 3] })
        });
        let r = sim.guard.report();
        assert!(r.cap.is_some(), "{r:?}");
        assert!(
            sim.paused_at.is_some_and(|t| t < 3.0),
            "{:?}",
            sim.paused_at
        );
        assert!(!sim.guard.plan().soften.iter().any(|s| *s));
    }

    #[test]
    fn small_flashing_and_ordinary_change_are_left_alone() {
        // A 48-pixel square blinking in a corner: under a quarter of a field.
        let blink = |g: u64, _f: u64| {
            let mut v = solid([10; 3]);
            if g % 2 == 1 {
                for y in 0..48 {
                    for x in 0..48 {
                        let i = ((y * W + x) * 4) as usize;
                        v[i..i + 3].copy_from_slice(&[255; 3]);
                    }
                }
            }
            v
        };
        let mut sim = Sim::new(60.0, 30.0, 2, true);
        sim.run(5.0, &blink);
        let r = sim.guard.report();
        assert_eq!((r.action, r.cap), (Action::None, None), "{r:?}");
        assert!(!sim.guard.plan().active());
        // A slow fade of the whole field: one transition, never a flash.
        let mut sim = Sim::new(60.0, 30.0, 2, true);
        sim.run(5.0, &|_, f| solid([(f.min(255)) as u8; 3]));
        assert_eq!(sim.guard.report().action, Action::None);
    }

    #[test]
    fn switched_off_it_measures_and_does_nothing() {
        let mut sim = Sim::new(60.0, 60.0, 2, false);
        sim.run(4.0, &b0);
        let r = sim.guard.report();
        assert!(!r.on);
        assert_eq!((r.action, r.cap, r.softened), (Action::None, None, 0.0));
        assert!(!sim.guard.plan().active());
        assert!(sim.worst_shown > MAX_TRANSITIONS);
        assert_eq!(
            r.note.as_deref(),
            Some("Strobe guard off: this scene flashes 4 or more times a second.")
        );
        // Switched on, it acts.
        sim.guard.set_on(true);
        sim.run(1.0, &b0);
        assert!(sim.guard.report().action >= Action::Softening);
        // Switched off again, it lets go at once.
        sim.guard.set_on(false);
        assert!(!sim.guard.plan().active() && sim.guard.cap().is_none());
    }

    #[test]
    fn red_flashing_is_softened_too() {
        let red = |g: u64, _f: u64| solid(if g % 2 == 0 { [255, 0, 0] } else { [120, 0, 0] });
        let mut sim = Sim::new(60.0, 60.0, 2, true);
        sim.run(1.0, &red);
        assert!(sim.guard.report().action >= Action::Softening);
        assert!(sim.worst_shown <= MAX_TRANSITIONS, "{}", sim.worst_shown);
    }

    #[test]
    fn strong_stripes_are_veiled_down_to_low_contrast() {
        let stripes = |_g: u64, _f: u64| {
            let mut v = Vec::with_capacity((W * H * 4) as usize);
            for _ in 0..H {
                for x in 0..W {
                    let c = if (x / 6) % 2 == 0 { 0 } else { 255 };
                    v.extend_from_slice(&[c, c, c, 255]);
                }
            }
            v
        };
        let mut sim = Sim::new(60.0, 10.0, 2, true);
        sim.run(2.0, &stripes);
        let r = sim.guard.report();
        assert!(
            sim.worst_shown < WARN_TRANSITIONS,
            "the veil came on gently: {}",
            sim.worst_shown
        );
        assert_eq!(r.action, Action::Softening, "{r:?}");
        assert!(
            r.note.as_deref().unwrap().contains("strong stripes"),
            "{r:?}"
        );
        let veiled: Vec<&[f32; 2]> = sim
            .guard
            .plan()
            .veil
            .iter()
            .filter(|v| v[0] > 0.0)
            .collect();
        assert!(veiled.len() * 2 > sim.guard.plan().veil.len());
        for v in veiled {
            // Black and white stripes, contrast about 1: veiled to 0.04 of it, towards a dim grey.
            assert!(v[0] > 0.9 && v[1] <= VEIL_GREY_MAX, "{v:?}");
        }
        // The softened frames' stripes no longer reach the threshold.
        let plan = sim.guard.plan().clone();
        let s = Softener::new().apply(&stripes(0, 0), W, H, &plan, 1.0 / 60.0);
        let half = reduce_both(&s.shown.unwrap(), W, H).1;
        let mut m = PatternMeter::new(None);
        assert_eq!(m.update(&half, 0.0).level, PatternLevel::Pass);
    }

    /// For someone at risk, a still soup of square cells (no stripes) is veiled as a pattern of
    /// similar parts, until its light parts are dim; for anyone else it is left alone.
    #[test]
    fn the_at_risk_guard_veils_similar_parts() {
        let mut rng = crate::rng::Rng::new(3);
        let cells: Vec<bool> = (0..(W / 8 + 1) * (H / 8 + 1))
            .map(|_| rng.next_f64() < 0.3)
            .collect();
        let soup = |_g: u64, _f: u64| {
            let mut v = Vec::with_capacity((W * H * 4) as usize);
            for y in 0..H {
                for x in 0..W {
                    let c = if cells[(y / 8 * (W / 8 + 1) + x / 8) as usize] {
                        240
                    } else {
                        20
                    };
                    v.extend_from_slice(&[c, c, c, 255]);
                }
            }
            v
        };
        for at_risk in [false, true] {
            let mut sim = Sim::new(60.0, 10.0, 2, true);
            sim.guard.set_at_risk(at_risk);
            sim.run(2.0, &soup);
            let veiled = sim.guard.plan().veil.iter().filter(|v| v[0] > 0.0).count();
            if !at_risk {
                assert_eq!(veiled, 0, "only for someone at risk");
                continue;
            }
            assert!(veiled * 2 > sim.guard.plan().veil.len(), "{veiled}");
            let r = sim.guard.report();
            assert!(
                r.note.as_deref().unwrap().contains("similar parts"),
                "{r:?}"
            );
            // What is shown: its light parts under the rule's light level, so it passes.
            let plan = sim.guard.plan().clone();
            let s = Softener::new().apply(&soup(0, 0), W, H, &plan, 1.0 / 60.0);
            let half = reduce_both(&s.shown.unwrap(), W, H).1;
            let p = crate::strobe::similar::find(&half);
            assert!(p.light < crate::strobe::similar::LIGHT_MIN, "{p:?}");
        }
    }

    #[test]
    fn words_for_the_notes() {
        assert_eq!(flashes(8), "4 or more times a second");
        assert_eq!(flashes(6), "3 times a second");
        assert_eq!(flashes(5), "2.5 times a second");
        assert_eq!(pace_words(0.5), "0.5 generations a second");
        assert_eq!(pace_words(2.0), "2 generations a second");
        assert_eq!(pace_words(15.3), "15 generations a second");
    }
}
