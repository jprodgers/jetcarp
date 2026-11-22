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
//! Each verdict is pinned twice for stripes: by the usual rule, and by the stricter rule for
//! someone who answered the seizure warning *at risk*, which counts similar parts regular or not
//! ([`jetcarp_core::strobe::similar`]). The guarded runs use each rule in turn.
//!
//! `JETCARP_STROBE_PRINT=1` prints every case's readings (to see why a pin moved).

use jetcarp_core::render::{self, RenderOptions, ViewOptions};
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
    /// Drawn this many pixels a cell around the world's middle, as a desktop picture or an
    /// exported animation draws it (with cell outlines from 7 pixels a cell), rather than fitted.
    cell_px: Option<f64>,
    flash: Level,
    red: Level,
    stripes: PatternLevel,
    /// The stripe verdict by the at-risk rule.
    at_risk: PatternLevel,
}

impl Case {
    /// The at-risk rule's verdict, where it differs from the usual one.
    const fn risk(mut self, at_risk: PatternLevel) -> Case {
        self.at_risk = at_risk;
        self
    }
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
        cell_px: None,
        flash,
        red,
        stripes,
        at_risk: stripes,
    }
}

/// A still scene drawn `cell_px` pixels a cell.
const fn sized(scene: &'static str, cell_px: f64, stripes: PatternLevel) -> Case {
    Case {
        scene,
        speed: 30.0,
        secs: 2.0,
        inset: None,
        cell_px: Some(cell_px),
        flash: Pass,
        red: Pass,
        stripes,
        at_risk: stripes,
    }
}

use Level::{Extended, Fail, Pass, Warning};
use PatternLevel::{Fail as SFail, Harmful as SHarmful, Pass as SPass};

/// JETCARP's verdicts, pinned.
const CASES: &[Case] = &[
    // Made to flash.
    case("tron-field", 60.0, 3.0, None, Fail, Pass, SPass),
    case("tron-red", 30.0, 3.0, None, Fail, Fail, SPass),
    // Hodgepodge as Quick Start shows it (the warning's example): from random states the whole
    // field swings between healthy and ill every ten generations or so as it settles, so at 60
    // generations a second its worst second has 8 transitions (four flashes), at 30 it has 6, and
    // at 15, 3.
    // By the at-risk rule its waves are similar parts at any speed.
    case("hodgepodge", 60.0, 4.0, None, Fail, Pass, SPass).risk(SFail),
    case("hodgepodge", 30.0, 4.0, None, Warning, Pass, SPass).risk(SFail),
    case("hodgepodge", 15.0, 4.0, None, Pass, Pass, SPass).risk(SFail),
    // Tron's rectilinear pattern, grown from the blob, is regular stripes too.
    case("tron-blob", 30.0, 3.0, None, Fail, Pass, SFail),
    // A soup of square cells: no stripes, but similar parts by the at-risk rule (IRIS fails it).
    case("seeds-soup", 30.0, 3.0, None, Fail, Pass, SPass).risk(SFail),
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
    // A checkerboard of 8-pixel squares reversing 15 times a second: every tile's mean stays put,
    // but every pixel flashes, and the squares are over 0.1°, so WCAG counts it (#269), as the
    // tiles' fine patterns do. IRIS, which takes the frame's mean, does not.
    case("tron-checkerboard", 30.0, 2.0, None, Fail, Pass, SFail),
    // Ordinary scenes.
    // Ordinary scenes, which the at-risk rule counts as similar parts: the soups and the waves
    // (IRIS fails the waves), Brian's Brain and the maze's corridors for less than half a second at
    // a time.
    case("life-soup", 30.0, 4.0, None, Pass, Pass, SPass).risk(SFail),
    case("brians-brain", 30.0, 4.0, None, Pass, Pass, SPass).risk(SHarmful),
    case("maze", 30.0, 4.0, None, Pass, Pass, SPass).risk(SHarmful),
    case("cyclic-waves", 15.0, 4.0, None, Pass, Pass, SPass).risk(SFail),
    // Still fields with thin lines over them: live cells at a fractional size (no seams), and
    // with cell outlines as a desktop picture or an exported animation draws them, which over
    // bright cells are drawn faint enough to make no strong stripes; and outlines over a light
    // ground with scattered cells on it.
    sized("live-field", 4.37, SPass),
    sized("live-field", 8.0, SPass),
    sized("live-field", 16.0, SPass),
    // Paper's scattered dark cells are similar parts by the at-risk rule (IRIS fails them at 16).
    sized("paper-field", 8.0, SPass).risk(SFail),
    sized("paper-field", 16.0, SPass).risk(SFail),
];

fn scene(name: &str) -> Scene {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/scenes/strobe")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The frame of `world` as the case draws it.
fn draw(world: &World, c: &Case) -> Vec<u8> {
    if let Some(px) = c.cell_px {
        let b = world.topo.bounds;
        let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        return render::render_view(world, W, H, center, px, &ViewOptions::default()).rgba;
    }
    let opts = RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    match c.inset {
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
fn verdict(c: &Case) -> (FlashReading, FlashReading, PatternReading, PatternReading) {
    let mut world = World::from_scene(&scene(c.scene)).unwrap();
    let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
    let mut pattern = PatternMeter::new(None);
    let mut strict = PatternMeter::new(None);
    strict.set_at_risk(true);
    let mut at_risk = PatternReading::default();
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
        let px = draw(&world, c);
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
            let p = strict.update(&half, t);
            if p.level > at_risk.level {
                at_risk = p;
            }
        }
    }
    (seen.flash, red, seen.stripes, at_risk)
}

/// Runs a case through the guard; the most transitions a second that reached the screen, the
/// worst level, and the worst stripe level of what was shown.
fn guarded(c: &Case, at_risk: bool) -> (u32, Level, PatternLevel) {
    let mut world = World::from_scene(&scene(c.scene)).unwrap();
    let mut guard = Guard::new(Viewing::screen(1.0), None, true);
    guard.set_at_risk(at_risk);
    let mut soft = Softener::new();
    let mut shown = FlashMeter::new(Viewing::screen(1.0), None);
    let mut shown_pattern = PatternMeter::new(None);
    shown_pattern.set_at_risk(at_risk);
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
        let px = draw(&world, c);
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

/// How often each scene's tiles' fine patterns reverse (#269), as drawn (ignored; run with
/// `--ignored --nocapture`): the most tiles at once with a counted reversal in the last second,
/// with two, and the most transitions the fine patterns made by the area rule.
#[test]
#[ignore]
fn fine_patterns_in_the_corpus() {
    for c in CASES {
        let mut world = World::from_scene(&scene(c.scene)).unwrap();
        let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
        let (mut one, mut two, mut area) = (0, 0, 0);
        for k in 0..(c.secs * FPS) as u32 {
            let due = (k as f64 * c.speed / FPS + 1e-9).floor();
            while (world.generation as f64) < due {
                world.step();
            }
            let (map, _) = reduce_both(&draw(&world, c), W, H);
            let r = meter.update(&map, k as f64 / FPS);
            let f = meter.tile_fine_counts();
            one = one.max(f.iter().filter(|&&n| n >= 1).count());
            two = two.max(f.iter().filter(|&&n| n >= 2).count());
            area = area.max(r.fine_transitions);
        }
        println!(
            "{} at {} gen/s{}: tiles reversing {one}, twice {two}, by the area rule {area} (of {} tiles)",
            c.scene,
            c.speed,
            c.cell_px
                .map_or(String::new(), |p| format!(", {p} px a cell")),
            (W / 16) * (H / 16)
        );
    }
}

/// The same through the guard as a screen shows it (ignored; `--ignored --nocapture`): the most
/// tiles with a counted fine-pattern reversal, as drawn and as shown, and the most transitions in
/// a second shown.
#[test]
#[ignore]
fn fine_patterns_in_the_corpus_guarded() {
    for c in CASES {
        let mut world = World::from_scene(&scene(c.scene)).unwrap();
        let mut guard = Guard::new(Viewing::screen(1.0), None, true);
        let mut soft = Softener::new();
        let mut drawn = FlashMeter::new(Viewing::screen(1.0), None);
        let mut shown = FlashMeter::new(Viewing::screen(1.0), None);
        let mut queue = VecDeque::new();
        let (mut due, mut d1, mut s1, mut most, mut veiled) = (0.0f64, 0, 0, 0, 0);
        for k in 0..(c.secs * FPS) as u32 {
            let t = k as f64 / FPS;
            let pace = guard.cap().map_or(c.speed, |cap| (cap as f64).min(c.speed));
            if k > 0 && guard.paused().is_none() {
                due += pace / FPS;
                while (world.generation as f64) < (due + 1e-9).floor() {
                    world.step();
                }
            }
            let px = draw(&world, c);
            let s = soft.apply(&px, W, H, &guard.plan().clone(), (1.0 / FPS) as f32);
            drawn.update(&s.input, t);
            let r = shown.update(&s.output, t);
            most = most.max(r.most());
            d1 = d1.max(drawn.tile_fine_counts().iter().filter(|&&n| n > 0).count());
            s1 = s1.max(shown.tile_fine_counts().iter().filter(|&&n| n > 0).count());
            veiled = veiled.max(guard.plan().veil.iter().filter(|v| v[0] > 0.0).count());
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
        println!(
            "{} at {} gen/s: reversing tiles drawn {d1}, shown {s1}; tiles veiled {veiled}; most \
             shown {most}; cap {:?}",
            c.scene,
            c.speed,
            guard.cap()
        );
    }
}

#[test]
fn the_corpus_verdicts_are_pinned() {
    let print = std::env::var_os("JETCARP_STROBE_PRINT").is_some();
    let mut wrong = vec![];
    for c in CASES {
        let (flash, red, stripes, at_risk) = verdict(c);
        let label = format!(
            "{} at {} gen/s for {} s{}",
            c.scene,
            c.speed,
            c.secs,
            c.inset.map_or(String::new(), |s| format!(", {s} px inset"))
                + &c.cell_px
                    .map_or(String::new(), |p| format!(", {p} px a cell"))
        );
        if print {
            eprintln!(
                "{label}: flash {:?} ({} transitions: area {}, screen {}, fine patterns {}, share {:.2}); red {:?} ({}, by chromaticity {}); stripes {:?} (share {:.2}, contrast {:.2}, period {:.1} px); at risk {:?} ({} similar parts)",
                flash.level,
                flash.transitions,
                flash.area_transitions,
                flash.screen_transitions,
                flash.fine_transitions,
                flash.share,
                red.red_level,
                red.red_transitions,
                red.chroma_transitions,
                stripes.level,
                stripes.share,
                stripes.contrast,
                stripes.period_px,
                at_risk.level,
                at_risk.parts
            );
        }
        let got = (flash.level, red.red_level, stripes.level, at_risk.level);
        let want = (c.flash, c.red, c.stripes, c.at_risk);
        if got != want {
            wrong.push(format!("{label}: got {got:?}, pinned {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "verdicts moved:\n{}", wrong.join("\n"));
}

/// Exported animations of the still fields, saved through the at-risk guard, for the IRIS
/// comparison (ignored, and needs `JETCARP_STROBE_RECORD=DIR`): each field drawn as an export
/// draws it, held for 60 frames at 10 a second, saved as WebP through the guard with its stricter
/// stripe rule, and read back: `DIR/export-risk/<scene>-<px>px/00000.png`, and
/// `DIR/exports.json` with the check of each file as saved.
#[test]
#[ignore]
fn record_at_risk_exports_for_iris() {
    use jetcarp_core::media::encode;
    let Some(dir) = std::env::var_os("JETCARP_STROBE_RECORD").map(PathBuf::from) else {
        eprintln!("skipped: set JETCARP_STROBE_RECORD to a folder");
        return;
    };
    let mut out = serde_json::Map::new();
    for c in CASES.iter().filter(|c| c.cell_px.is_some_and(|p| p >= 8.0)) {
        let world = World::from_scene(&scene(c.scene)).unwrap();
        let px = c.cell_px.unwrap();
        let img = jetcarp_core::render::Image {
            width: W,
            height: H,
            rgba: draw(&world, c),
        };
        let frames = vec![img; 60];
        let bg = [frames[0].rgba[0], frames[0].rgba[1], frames[0].rgba[2], 255];
        let enc = |f: &[&jetcarp_core::render::Image], d: u32| encode::webp(f, d, bg);
        let g = jetcarp_core::strobe::export::guard(
            &frames,
            100,
            true,
            true,
            &enc,
            &encode::decode_webp,
        )
        .unwrap();
        let name = format!("{}-{px}px", c.scene);
        let sub = dir.join("export-risk").join(&name);
        std::fs::create_dir_all(&sub).unwrap();
        for (k, f) in encode::decode_webp(&g.bytes).unwrap().iter().enumerate() {
            render::write_png(&sub.join(format!("{k:05}.png")), f, &[]).unwrap();
        }
        out.insert(name, g.record());
    }
    std::fs::write(
        dir.join("exports.json"),
        serde_json::to_string_pretty(&out).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn with_the_guard_on_nothing_in_the_corpus_fails_on_screen() {
    let mut wrong = vec![];
    for (c, at_risk) in CASES.iter().flat_map(|c| [(c, false), (c, true)]) {
        let (most, level, stripes) = guarded(c, at_risk);
        if level.fails() || stripes == SFail {
            wrong.push(format!(
                "{} at {} gen/s{}{}: {most} transitions a second on screen, {level:?}, stripes {stripes:?}",
                c.scene,
                c.speed,
                c.inset.map_or(String::new(), |s| format!(", {s} px inset"))
                    + &c.cell_px.map_or(String::new(), |p| format!(", {p} px a cell")),
                if at_risk { ", at risk" } else { "" }
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the guard let through:\n{}",
        wrong.join("\n")
    );
}
