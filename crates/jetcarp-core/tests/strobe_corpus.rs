//! The strobe guard's test corpus (#150): the scenes in `docs/scenes/strobe/`, made to flash, made to
//! sit just under the limits, ordinary, and striped, each drawn frame by frame on the processor at a
//! fixed speed and size, 60 frames a second. JETCARP's verdict on each is pinned below; the IRIS
//! comparison (`docs/src/about/testing.md`) checks these verdicts against IRIS's on recordings of
//! the same scenes.
//!
//! Each scene is also run through the guard, as a screen would show it: the processor's softener
//! carrying out the guard's plan, its readings two frames late, and its pace cap holding the
//! generations back. What that shows must never fail.
//!
//! `JETCARP_STROBE_PRINT=1` prints every case's readings (to see why a pin moved).

use jetcarp_core::render::{self, RenderOptions};
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::flash::{FlashMeter, FlashReading, Level};
use jetcarp_core::strobe::frame::reduce_both;
use jetcarp_core::strobe::guard::{Frame, Guard, PATTERN_EVERY, Pace};
use jetcarp_core::strobe::pattern::{PatternLevel, PatternMeter, PatternReading};
use jetcarp_core::strobe::soften::Softener;
use jetcarp_core::{Scene, World};
use std::collections::VecDeque;
use std::path::PathBuf;

const FPS: f64 = 60.0;
const W: u32 = 320;
const H: u32 = 240;

/// One case: a scene, how fast and how long it runs, where it is drawn, and the verdict.
struct Case {
    scene: &'static str,
    speed: f64,
    secs: f64,
    /// Drawn in a square this many pixels across in the middle of a black frame (a small view),
    /// or over the whole frame.
    inset: Option<u32>,
    flash: Level,
    red: Level,
    stripes: PatternLevel,
}

const fn case(
    scene: &'static str,
    speed: f64,
    secs: f64,
    inset: Option<u32>,
    flash: Level,
    red: Level,
    stripes: PatternLevel,
) -> Case {
    Case {
        scene,
        speed,
        secs,
        inset,
        flash,
        red,
        stripes,
    }
}

use Level::{Extended, Fail, Pass, Warning};
use PatternLevel::{Fail as SFail, Pass as SPass};

/// JETCARP's verdicts, pinned.
const CASES: &[Case] = &[
    // Made to flash.
    case("tron-field", 60.0, 3.0, None, Fail, Pass, SPass),
    case("tron-red", 30.0, 3.0, None, Fail, Fail, SPass),
    // Tron's rectilinear pattern, grown from the blob, is regular stripes too.
    case("tron-blob", 30.0, 3.0, None, Fail, Pass, SFail),
    case("seeds-soup", 30.0, 3.0, None, Fail, Pass, SPass),
    // Just over and just under the rate: six generations a second flip the field six times (three
    // flashes, a warning), seven fail; six a second for longer is the extended failure.
    case("tron-field", 7.0, 3.0, None, Fail, Pass, SPass),
    case("tron-field", 6.0, 3.0, None, Warning, Pass, SPass),
    case("tron-field", 6.0, 6.0, None, Extended, Pass, SPass),
    case("tron-field", 2.0, 6.0, None, Pass, Pass, SPass),
    // Just under and just over the area: a field 112 pixels across flashing in a 320 by 240 frame
    // is under a quarter of a 10-degree field (and moves the frame's mean by under 0.1); 128
    // pixels across is over.
    case("tron-field", 60.0, 3.0, Some(112), Pass, Pass, SPass),
    case("tron-field", 60.0, 3.0, Some(128), Fail, Pass, SPass),
    // Stripes.
    case("stripes-still", 10.0, 2.0, None, Pass, Pass, SFail),
    case("tron-checkerboard", 30.0, 2.0, None, Pass, Pass, SFail),
    // Ordinary scenes.
    case("life-soup", 30.0, 4.0, None, Pass, Pass, SPass),
    case("brians-brain", 30.0, 4.0, None, Pass, Pass, SPass),
    case("maze", 30.0, 4.0, None, Pass, Pass, SPass),
    case("cyclic-waves", 15.0, 4.0, None, Pass, Pass, SPass),
];

fn scene(name: &str) -> Scene {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/scenes/strobe")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The frame of `world` as the case draws it.
fn draw(world: &World, inset: Option<u32>) -> Vec<u8> {
    let opts = RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    match inset {
        None => render::render_fitted(world, W, H, &opts).rgba,
        Some(s) => {
            let img = render::render_fitted(world, s, s, &opts);
            let mut v = vec![0u8; (W * H * 4) as usize];
            v.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
            let (x0, y0) = ((W - img.width) / 2, (H - img.height) / 2);
            for y in 0..img.height {
                let src =
                    &img.rgba[(y * img.width * 4) as usize..((y + 1) * img.width * 4) as usize];
                let at = (((y0 + y) * W + x0) * 4) as usize;
                v[at..at + src.len()].copy_from_slice(src);
            }
            v
        }
    }
}

struct Seen {
    flash: FlashReading,
    stripes: PatternReading,
}

fn worse(a: &FlashReading, b: &FlashReading) -> bool {
    (b.level, b.transitions) > (a.level, a.transitions)
}

fn worse_red(a: &FlashReading, b: &FlashReading) -> bool {
    (b.red_level, b.red_transitions) > (a.red_level, a.red_transitions)
}

/// Runs a case unguarded; the worst general, red and stripe readings.
fn verdict(c: &Case) -> (FlashReading, FlashReading, PatternReading) {
    let mut world = World::from_scene(&scene(c.scene)).unwrap();
    let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
    let mut pattern = PatternMeter::new(None);
    let mut last_pattern = f64::NEG_INFINITY;
    let mut seen = Seen {
        flash: FlashReading::default(),
        stripes: PatternReading::default(),
    };
    let mut red = FlashReading::default();
    for k in 0..(c.secs * FPS) as u32 {
        let t = k as f64 / FPS;
        // Generations due by this frame, exactly: six a second are every tenth frame.
        let due = (k as f64 * c.speed / FPS + 1e-9).floor();
        while (world.generation as f64) < due {
            world.step();
        }
        let px = draw(&world, c.inset);
        let (map, half) = reduce_both(&px, W, H);
        let r = meter.update(&map, t);
        if worse(&seen.flash, &r) {
            seen.flash = r;
        }
        if worse_red(&red, &r) {
            red = r;
        }
        if t - last_pattern >= PATTERN_EVERY - 1e-9 {
            last_pattern = t;
            let p = pattern.update(&half, t);
            if p.level > seen.stripes.level {
                seen.stripes = p;
            }
        }
    }
    (seen.flash, red, seen.stripes)
}

/// Runs a case through the guard; the most transitions a second that reached the screen, the
/// worst level, and the worst stripe level of what was shown.
fn guarded(c: &Case) -> (u32, Level, PatternLevel) {
    let mut world = World::from_scene(&scene(c.scene)).unwrap();
    let mut guard = Guard::new(Viewing::screen(1.0), None, true);
    let mut soft = Softener::new();
    let mut shown = FlashMeter::new(Viewing::screen(1.0), None);
    let mut shown_pattern = PatternMeter::new(None);
    let mut queue = VecDeque::new();
    let (mut due, mut most, mut level, mut stripes) = (0.0f64, 0, Pass, SPass);
    let mut last_pattern = f64::NEG_INFINITY;
    for k in 0..(c.secs * FPS) as u32 {
        let t = k as f64 / FPS;
        let pace = guard.cap().map_or(c.speed, |cap| (cap as f64).min(c.speed));
        if k > 0 && guard.paused().is_none() {
            due += pace / FPS;
            while (world.generation as f64) < (due + 1e-9).floor() {
                world.step();
            }
        }
        let px = draw(&world, c.inset);
        let s = soft.apply(&px, W, H, &guard.plan().clone(), (1.0 / FPS) as f32);
        let out = s.shown.as_deref().unwrap_or(&px);
        let r = shown.update(&s.output, t);
        most = most.max(r.transitions.max(r.red_transitions));
        level = level.max(r.worst());
        if t - last_pattern >= PATTERN_EVERY - 1e-9 {
            last_pattern = t;
            stripes = stripes.max(shown_pattern.update(&reduce_both(out, W, H).1, t).level);
        }
        let half = guard.wants_half(t).then(|| reduce_both(&px, W, H).1);
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
        }
    }
    (most, level, stripes)
}

#[test]
fn the_corpus_verdicts_are_pinned() {
    let print = std::env::var_os("JETCARP_STROBE_PRINT").is_some();
    let mut wrong = vec![];
    for c in CASES {
        let (flash, red, stripes) = verdict(c);
        let label = format!(
            "{} at {} gen/s for {} s{}",
            c.scene,
            c.speed,
            c.secs,
            c.inset.map_or(String::new(), |s| format!(", {s} px inset"))
        );
        if print {
            eprintln!(
                "{label}: flash {:?} ({} transitions: area {}, screen {}, share {:.2}); red {:?} ({}); stripes {:?} (share {:.2}, contrast {:.2}, period {:.1} px)",
                flash.level,
                flash.transitions,
                flash.area_transitions,
                flash.screen_transitions,
                flash.share,
                red.red_level,
                red.red_transitions,
                stripes.level,
                stripes.share,
                stripes.contrast,
                stripes.period_px
            );
        }
        let got = (flash.level, red.red_level, stripes.level);
        let want = (c.flash, c.red, c.stripes);
        if got != want {
            wrong.push(format!("{label}: got {got:?}, pinned {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "verdicts moved:\n{}", wrong.join("\n"));
}

#[test]
fn with_the_guard_on_nothing_in_the_corpus_fails_on_screen() {
    let mut wrong = vec![];
    for c in CASES {
        let (most, level, stripes) = guarded(c);
        if level.fails() || stripes == SFail {
            wrong.push(format!(
                "{} at {} gen/s{}: {most} transitions a second on screen, {level:?}, stripes {stripes:?}",
                c.scene,
                c.speed,
                c.inset.map_or(String::new(), |s| format!(", {s} px inset"))
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the guard let through:\n{}",
        wrong.join("\n")
    );
}
