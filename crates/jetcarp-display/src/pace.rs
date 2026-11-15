//! How often a desktop background screen draws, and how many generations each frame steps (#42).
//!
//! A scene asks for a speed in generations a second. A screen that drew at a fixed rate below it
//! would show every second or third generation, and an oscillator whose period divides that would
//! seem to stand still: Life's blinkers at 60 generations a second, drawn at 30 frames a second,
//! show the same phase in every frame. So by default a screen draws as fast as its scene steps
//! ([`Pacing::EveryGeneration`]): at the scene's speed, at least the host's frame rate, and at most
//! the monitor's refresh rate. [`Pacing::LowPower`] instead caps each scene's speed at the frame
//! rate, so a screen never steps more than one generation a frame.
//!
//! [`FrameClock`] says when each screen's next frame is due, and [`Pace::gens_due`] how many
//! generations a frame steps: exactly one a frame for a scene drawn at its own speed, though the
//! timer wakes each frame a little late.

use serde::{Deserialize, Serialize};

/// How a screen's frame rate follows its scene's speed (`pacing` in `host.json`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Pacing {
    /// Every generation is shown: a screen draws as fast as its scene steps, at least the host's
    /// frame rate and at most the monitor's refresh rate. A scene faster than the refresh draws
    /// several generations a frame.
    #[default]
    EveryGeneration,
    /// A scene's speed is capped at the host's frame rate, and its screen draws at that rate: at
    /// most one generation a frame, and the least work.
    LowPower,
}

impl Pacing {
    pub const ALL: [Pacing; 2] = [Pacing::EveryGeneration, Pacing::LowPower];

    /// The app's words for it.
    pub fn label(self) -> &'static str {
        match self {
            Pacing::EveryGeneration => "Show every generation (draws up to the monitor's refresh)",
            Pacing::LowPower => "Low power: cap the speed at the frame rate",
        }
    }

    /// What it does, for a hover.
    pub fn hint(self) -> &'static str {
        match self {
            Pacing::EveryGeneration => {
                "Each screen draws as fast as its scene steps: at least the frames per second \
                 above, at most the monitor's refresh rate. Life at 60 generations a second draws \
                 60 frames a second, one generation a frame; a scene faster than the refresh draws \
                 several generations a frame"
            }
            Pacing::LowPower => {
                "Each screen draws at the frames per second above, and no scene runs faster: at \
                 most one generation a frame. Life at 60 generations a second runs at 30 at 30 \
                 frames a second. Less work for the GPU"
            }
        }
    }

    /// The command line's word for it, and `host status`'s.
    pub fn word(self) -> &'static str {
        match self {
            Pacing::EveryGeneration => "every-generation",
            Pacing::LowPower => "low-power",
        }
    }
}

/// The refresh rate taken for a monitor whose own is not known (and for a host with no monitor).
pub const DEFAULT_REFRESH: f32 = 60.0;
/// The most frames a second showing every generation draws at, unless the settings say otherwise:
/// enough for one generation a frame at 60 generations a second, without drawing a fast scene at a
/// 144 Hz monitor's full rate. `max_fps: null` lets it go up to the refresh (Jimmie's call).
pub const DEFAULT_MAX_FPS: f32 = 60.0;
/// Generations owed when a world goes on show: half of one, so a frame that comes a little early
/// or late still steps the same whole number of generations (see [`Pace::gens_due`]).
pub const START_OWED: f64 = 0.5;
/// A scene up to this many generations a frame steps one a frame: Life's 60 a second on a monitor
/// of 59.94 Hz runs at 59.94 rather than skipping a generation every 17 seconds.
pub const ONE_A_FRAME: f32 = 1.01;
/// A frame due this soon is drawn now (the host's timer may wake a hair early).
pub const EARLY: f64 = 0.001;

/// What a screen's pace follows: the host's pacing and frame rate, and its monitor's refresh rate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pacer {
    pub pacing: Pacing,
    /// The host's frame rate (`fps` in `host.json`).
    pub fps: f32,
    /// The monitor's refresh rate (None: not known, taken as [`DEFAULT_REFRESH`]).
    pub refresh: Option<f32>,
    /// The most frames a second a screen showing every generation draws at (None: up to the
    /// refresh rate). The settings' default is [`DEFAULT_MAX_FPS`].
    pub max_fps: Option<f32>,
}

impl Default for Pacer {
    fn default() -> Pacer {
        Pacer::fixed(30.0)
    }
}

/// How fast a screen draws, and how fast its scene runs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pace {
    /// Frames a second.
    pub fps: f32,
    /// Generations a second (0: as fast as it can).
    pub speed: f32,
}

impl Pacer {
    /// A screen that draws at `fps` whatever its scene's speed (the macOS screen saver, whose
    /// frames the system times; a stage nobody paces).
    pub fn fixed(fps: f32) -> Pacer {
        Pacer {
            pacing: Pacing::EveryGeneration,
            fps,
            refresh: Some(fps),
            max_fps: None,
        }
    }

    /// The refresh rate a screen draws at most: the monitor's, or [`DEFAULT_REFRESH`].
    pub fn refresh(&self) -> f32 {
        self.refresh
            .filter(|r| r.is_finite() && *r >= 1.0)
            .unwrap_or(DEFAULT_REFRESH)
    }

    /// The most a screen showing every generation draws at: the refresh rate, or the settings'
    /// maximum when that is lower.
    pub fn top(&self) -> f32 {
        let r = self.refresh();
        self.max_fps
            .filter(|m| m.is_finite() && *m >= 1.0)
            .map_or(r, |m| m.min(r))
    }

    /// The pace of a scene that asks for `speed` generations a second (0 or less: as fast as it
    /// can). Showing every generation, the screen draws at the scene's speed, at least the frame
    /// rate and at most [`top`](Self::top) (a frame rate set above that still holds), and the
    /// scene keeps its speed. In low power it draws at the frame rate, and a scene runs no faster,
    /// one that runs as fast as it can included.
    pub fn pace(&self, speed: f32) -> Pace {
        let fps = if self.fps.is_finite() {
            self.fps.max(1.0)
        } else {
            30.0
        };
        let paced = speed.is_finite() && speed > 0.0;
        match self.pacing {
            Pacing::EveryGeneration => Pace {
                fps: if paced {
                    speed.min(self.top()).max(fps)
                } else {
                    fps
                },
                speed: if paced { speed } else { 0.0 },
            },
            Pacing::LowPower => Pace {
                fps,
                speed: if paced { speed.min(fps) } else { fps },
            },
        }
    }
}

impl Pace {
    /// Generations a frame (None: as fast as it can).
    pub fn per_frame(&self) -> Option<f32> {
        (self.speed > 0.0).then(|| self.speed / self.fps.max(1.0))
    }

    /// Whether a frame steps one generation at most: the scene runs no faster than its screen
    /// draws (to within [`ONE_A_FRAME`]).
    pub fn one_a_frame(&self) -> bool {
        self.per_frame().is_some_and(|g| g <= ONE_A_FRAME)
    }

    /// The generations due in a frame `dt` seconds after the last, of a scene that runs at a set
    /// speed. `owed` carries what is owed from frame to frame: start it at [`START_OWED`], and take
    /// off what the frame stepped. What is owed grows by the speed times `dt`, up to `backlog`
    /// seconds' worth; a frame steps the whole generations owed, or where [`one_a_frame`] one at
    /// most, so a frame that comes late never skips a generation (the scene falls behind its
    /// speed instead, and catches up a generation a frame).
    ///
    /// [`one_a_frame`]: Pace::one_a_frame
    pub fn gens_due(&self, owed: &mut f64, dt: f64, backlog: f64) -> u64 {
        let speed = self.speed.max(0.0) as f64;
        *owed = (*owed + dt.max(0.0) * speed).min(speed * backlog + 1.0);
        let due = owed.floor().max(0.0) as u64;
        if self.one_a_frame() { due.min(1) } else { due }
    }
}

/// When a screen's next frame is due, on the host's clock (seconds).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct FrameClock {
    next: Option<f64>,
    rate: f32,
}

impl FrameClock {
    /// Whether a frame of a screen drawing `rate` frames a second is due at `now`. When it is, the
    /// next is set a frame after this one was due, not after it came, so frames keep to the rate on
    /// average though the timer wakes each a little late. A frame more than a frame late, or a new
    /// rate, starts again from now.
    pub fn tick(&mut self, now: f64, rate: f32) -> bool {
        let rate = if rate.is_finite() { rate.max(1.0) } else { 1.0 };
        if rate != self.rate {
            self.rate = rate;
            self.next = None;
        }
        let due = match self.next {
            None => now,
            Some(t) if now + EARLY >= t => t,
            Some(_) => return false,
        };
        let interval = 1.0 / rate as f64;
        let next = due + interval;
        self.next = Some(if next <= now { now + interval } else { next });
        true
    }

    /// When the next frame is due (None: at once).
    pub fn next(&self) -> Option<f64> {
        self.next
    }

    /// The frame rate it keeps.
    pub fn rate(&self) -> f32 {
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKLOG: f64 = 0.25;

    /// Showing every generation draws at most the settings' maximum (60 by default) where the
    /// monitor refreshes faster; with none, up to the refresh: a 144 Hz monitor may draw Life at
    /// 144 frames a second if someone wants it.
    #[test]
    fn the_most_a_screen_draws_at_is_the_maximum_or_the_refresh() {
        let capped = Pacer {
            max_fps: Some(DEFAULT_MAX_FPS),
            ..pacer(Pacing::EveryGeneration, 30.0, Some(144.0))
        };
        assert_eq!(capped.pace(60.0).fps, 60.0, "one a frame at 60");
        assert_eq!(capped.pace(144.0).fps, 60.0, "capped at 60");
        assert_eq!(capped.pace(240.0).fps, 60.0);
        assert_eq!(capped.pace(240.0).speed, 240.0, "the speed is kept");
        assert_eq!(capped.pace(20.0).fps, 30.0, "at least the frame rate");
        let full = Pacer {
            max_fps: None,
            ..capped
        };
        assert_eq!(full.pace(144.0).fps, 144.0);
        assert_eq!(full.pace(240.0).fps, 144.0, "at most the refresh");
        // A maximum above the refresh is the refresh; on a 60 Hz monitor the cap changes nothing.
        let high = Pacer {
            max_fps: Some(240.0),
            ..capped
        };
        assert_eq!(high.pace(240.0).fps, 144.0);
        let slow = Pacer {
            refresh: Some(60.0),
            ..full
        };
        assert_eq!(slow.pace(144.0).fps, 60.0);
    }

    fn pacer(pacing: Pacing, fps: f32, refresh: Option<f32>) -> Pacer {
        Pacer {
            pacing,
            fps,
            refresh,
            max_fps: None,
        }
    }

    fn pace(fps: f32, speed: f32) -> Pace {
        Pace { fps, speed }
    }

    #[test]
    fn a_screen_draws_at_its_scene_s_speed_between_the_frame_rate_and_the_refresh() {
        let every = |speed, fps, refresh| pacer(Pacing::EveryGeneration, fps, refresh).pace(speed);
        // Life at 60 a second draws 60 frames a second on a 60 Hz or a 144 Hz monitor, and on a
        // monitor whose rate is not known.
        assert_eq!(every(60.0, 30.0, Some(60.0)), pace(60.0, 60.0));
        assert_eq!(every(60.0, 30.0, Some(144.0)), pace(60.0, 60.0));
        assert_eq!(every(60.0, 30.0, None), pace(60.0, 60.0));
        assert_eq!(every(60.0, 30.0, Some(59.94)), pace(59.94, 60.0));
        // Faster than the refresh: at the refresh, several generations a frame.
        assert_eq!(every(240.0, 30.0, Some(144.0)), pace(144.0, 240.0));
        assert_eq!(every(240.0, 30.0, None), pace(60.0, 240.0));
        assert_eq!(
            every(240.0, 30.0, Some(144.0)).per_frame(),
            Some(240.0 / 144.0)
        );
        // Slower than the frame rate, or as fast as it can: the frame rate.
        assert_eq!(every(20.0, 30.0, Some(60.0)), pace(30.0, 20.0));
        assert_eq!(every(0.0, 30.0, Some(60.0)), pace(30.0, 0.0));
        assert_eq!(every(-1.0, 30.0, Some(60.0)), pace(30.0, 0.0));
        // A frame rate set above the refresh still holds.
        assert_eq!(every(60.0, 120.0, Some(60.0)), pace(120.0, 60.0));
        // A refresh rate that makes no sense is taken as 60.
        assert_eq!(every(240.0, 30.0, Some(0.0)), pace(60.0, 240.0));
        // Low power: the frame rate, and no scene faster, one as fast as it can included.
        let low = |speed, fps, refresh| pacer(Pacing::LowPower, fps, refresh).pace(speed);
        assert_eq!(low(60.0, 30.0, Some(144.0)), pace(30.0, 30.0));
        assert_eq!(low(240.0, 30.0, Some(144.0)), pace(30.0, 30.0));
        assert_eq!(low(20.0, 30.0, Some(144.0)), pace(30.0, 20.0));
        assert_eq!(low(0.0, 30.0, Some(144.0)), pace(30.0, 30.0));
        // A fixed pacer draws at its rate whatever the speed.
        assert_eq!(Pacer::fixed(30.0).pace(60.0), pace(30.0, 60.0));
        assert_eq!(Pacer::fixed(30.0).pace(20.0), pace(30.0, 20.0));
        assert_eq!(Pacer::default(), Pacer::fixed(30.0));
        // One a frame: up to the speed, within a percent; never as fast as it can.
        assert!(pace(60.0, 60.0).one_a_frame() && pace(59.94, 60.0).one_a_frame());
        assert!(pace(30.0, 20.0).one_a_frame());
        assert!(!pace(144.0, 240.0).one_a_frame() && !pace(30.0, 0.0).one_a_frame());
        assert_eq!(Pacing::default(), Pacing::EveryGeneration);
        assert_eq!(Pacing::LowPower.word(), "low-power");
    }

    /// A tiny random number generator, so the jitter is the same on every run.
    struct Lcg(u64);
    impl Lcg {
        /// In [0, 1).
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Runs a screen at `pace` for `seconds` as the host does: the loop wakes when the next frame
    /// is due, late by `late(frame)` seconds, and the frame steps what is due. Each frame's
    /// generations.
    fn run(p: Pace, seconds: f64, mut late: impl FnMut(usize) -> f64) -> Vec<u64> {
        let mut clock = FrameClock::default();
        let mut owed = START_OWED;
        let mut last: Option<f64> = None;
        let mut frames = vec![];
        let mut t: f64 = 0.0;
        while t < seconds {
            t = clock.next().unwrap_or(0.0).max(t) + late(frames.len());
            if !clock.tick(t, p.fps) {
                continue;
            }
            let dt = last.map_or(0.0, |l| t - l);
            last = Some(t);
            let n = p.gens_due(&mut owed, dt, BACKLOG);
            owed -= n as f64;
            frames.push(n);
        }
        frames
    }

    #[test]
    fn a_scene_drawn_at_its_own_speed_steps_one_generation_every_frame() {
        // Woken up to 4 ms late, at random: every frame but the first (nothing owed yet) steps
        // one generation, at 60 frames a second and at 59.94.
        for p in [
            pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(60.0),
            pacer(Pacing::EveryGeneration, 30.0, Some(59.94)).pace(60.0),
            pacer(Pacing::EveryGeneration, 30.0, Some(144.0)).pace(60.0),
            pacer(Pacing::LowPower, 30.0, Some(144.0)).pace(60.0),
        ] {
            let mut rng = Lcg(42);
            let frames = run(p, 10.0, |_| rng.next() * 0.004);
            assert!(frames.len() as f64 >= p.fps as f64 * 9.9, "{p:?}");
            assert!(frames[1..].iter().all(|&n| n == 1), "{p:?}: {frames:?}");
        }
        // The old way, 60 generations a second drawn at 30 frames: two a frame, the blinkers'
        // same phase every time.
        let frames = run(Pacer::fixed(30.0).pace(60.0), 2.0, |_| 0.001);
        assert!(frames[1..].iter().all(|&n| n == 2), "{frames:?}");
    }

    #[test]
    fn a_late_frame_never_skips_a_generation_that_is_drawn_one_a_frame() {
        // A stall of a fifth of a second now and then: the frame after it still steps one, in
        // both pacings, and the scene catches up a generation a frame.
        for p in [
            pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(60.0),
            pacer(Pacing::LowPower, 30.0, Some(60.0)).pace(60.0),
            pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(20.0),
        ] {
            let frames = run(p, 5.0, |k| if k % 97 == 50 { 0.2 } else { 0.002 });
            assert!(frames.iter().all(|&n| n <= 1), "{p:?}: {frames:?}");
        }
        // A scene slower than the frame rate steps a generation every so many frames, at its
        // speed: 20 a second at 30 frames, two frames in three.
        let p = pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(20.0);
        let mut rng = Lcg(7);
        let frames = run(p, 9.0, |_| rng.next() * 0.004);
        let gens: u64 = frames.iter().sum();
        assert!((178..=181).contains(&gens), "{gens} of 180");
        // Faster than the refresh: by the clock, several a frame, and the speed kept.
        let p = pacer(Pacing::EveryGeneration, 30.0, Some(144.0)).pace(240.0);
        let mut rng = Lcg(9);
        let frames = run(p, 5.0, |_| rng.next() * 0.002);
        let gens: u64 = frames.iter().sum();
        assert!((1196..=1201).contains(&gens), "{gens} of 1200");
        assert!(
            frames[1..].iter().all(|&n| (1..=3).contains(&n)),
            "{frames:?}"
        );
    }

    /// The host's loop wakes for whichever screen is due soonest and draws only the screens that
    /// are due (`Core::tick` and `Core::next_frame`): a screen at 60 frames a second beside one at
    /// 30 and one at 144 each keeps its own rate, every frame of each stepping its generation.
    #[test]
    fn several_screens_keep_their_own_rates_in_one_loop() {
        let paces = [
            pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(60.0),
            pacer(Pacing::EveryGeneration, 30.0, Some(60.0)).pace(30.0),
            pacer(Pacing::EveryGeneration, 30.0, Some(144.0)).pace(144.0),
        ];
        let mut clocks = [FrameClock::default(); 3];
        let mut owed = [START_OWED; 3];
        let mut last: [Option<f64>; 3] = [None; 3];
        let mut frames = [0u32; 3];
        let mut gens = [0u64; 3];
        let mut rng = Lcg(3);
        let mut t: f64 = 0.0;
        while t < 10.0 {
            let soonest = clocks
                .iter()
                .map(|c| c.next().unwrap_or(t))
                .fold(t + 0.2, f64::min);
            t = soonest.max(t) + rng.next() * 0.002;
            for i in 0..3 {
                if !clocks[i].tick(t, paces[i].fps) {
                    continue;
                }
                let dt = last[i].map_or(0.0, |l| t - l);
                last[i] = Some(t);
                let n = paces[i].gens_due(&mut owed[i], dt, BACKLOG);
                assert!(frames[i] == 0 || n == 1, "screen {i}, frame {}", frames[i]);
                owed[i] -= n as f64;
                frames[i] += 1;
                gens[i] += n;
            }
        }
        assert!((599..=601).contains(&frames[0]), "{frames:?}");
        assert!((299..=301).contains(&frames[1]), "{frames:?}");
        assert!((1438..=1441).contains(&frames[2]), "{frames:?}");
        assert_eq!(gens.map(|g| g + 1), frames.map(u64::from));
    }

    #[test]
    fn the_frame_clock_keeps_to_its_rate_though_each_frame_comes_late() {
        let mut c = FrameClock::default();
        assert!(c.tick(0.0, 60.0), "the first frame is due at once");
        assert_eq!(c.next(), Some(1.0 / 60.0));
        assert!(!c.tick(0.010, 60.0), "not yet");
        assert!(
            c.tick(1.0 / 60.0 - 0.0005, 60.0),
            "a hair early is close enough"
        );
        // Late by 5 ms: the next is still due on the beat, not 5 ms after it.
        assert!(c.tick(2.0 / 60.0 + 0.005, 60.0));
        assert!((c.next().unwrap() - 3.0 / 60.0).abs() < 1e-9);
        // More than a frame late: starts again from now.
        assert!(c.tick(0.5, 60.0));
        assert!((c.next().unwrap() - (0.5 + 1.0 / 60.0)).abs() < 1e-9);
        // A new rate starts again too.
        assert!(c.tick(0.51, 30.0));
        assert_eq!(c.rate(), 30.0);
        assert!((c.next().unwrap() - (0.51 + 1.0 / 30.0)).abs() < 1e-9);
        // Over ten seconds of frames each woken up to 3 ms late, it draws its 600.
        let mut c = FrameClock::default();
        let mut rng = Lcg(1);
        let (mut t, mut n) = (0.0, 0);
        while t < 10.0 {
            t = c.next().unwrap_or(0.0) + rng.next() * 0.003;
            if c.tick(t, 60.0) {
                n += 1;
            }
        }
        assert!((600..=601).contains(&n), "{n}");
    }
}
