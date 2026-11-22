//! Red flashes by either definition (#270). WCAG 2.0's working definition, which PEAT and IRIS
//! use: a change of at least 20 in `(R - G - B) × 320`, where red is at least 80% of `R + G + B`.
//! WCAG 2.2's (its glossary, *general flash and red flash thresholds*, note 3): a pair of opposing
//! transitions where one transition is to or from a state with `R / (R + G + B)` of 0.8 or more,
//! and the difference between the states is more than 0.2 in the CIE 1976 UCS chromaticity
//! diagram. The guard counts a red flash when either says so: if any tool says it is a flash, it
//! is one.
//!
//! Every frame is drawn and measured in memory: nothing is shown on a screen.

use jetcarp_core::strobe::flash::{FlashMeter, FlashReading, Level};
use jetcarp_core::strobe::frame::{TileMap, reduce};
use jetcarp_core::strobe::{FLASH_DELTA, RED_DELTA, Viewing, linear, luminance, red_measure};

const W: u32 = 320;
const H: u32 = 240;

fn solid(c: [u8; 3]) -> Vec<u8> {
    let mut v = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..W * H {
        v.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    v
}

/// CIE 1976 UCS chromaticity of an sRGB colour, from first principles (sRGB's primaries, D65).
fn uv(c: [u8; 3]) -> [f64; 2] {
    let (r, g, b) = (
        linear(c[0]) as f64,
        linear(c[1]) as f64,
        linear(c[2]) as f64,
    );
    let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
    let d = x + 15.0 * y + 3.0 * z;
    [4.0 * x / d, 9.0 * y / d]
}

fn chroma_change(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (p, q) = (uv(a), uv(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

fn saturated(c: [u8; 3]) -> bool {
    let (r, g, b) = (linear(c[0]), linear(c[1]), linear(c[2]));
    r >= 0.8 * (r + g + b)
}

/// The worst reading of the whole frame switching between `a` and `b` ten times a second, for
/// two seconds at 60 frames a second.
fn run(maps: &[TileMap; 2]) -> FlashReading {
    let mut m = FlashMeter::new(Viewing::screen(1.0), None);
    let mut worst = FlashReading::default();
    for f in 0..120u32 {
        let r = m.update(&maps[((f / 6) % 2) as usize], f as f64 / 60.0);
        if (r.red_level, r.red_transitions) > (worst.red_level, worst.red_transitions) {
            worst = r;
        }
    }
    worst
}

/// The three pairs of colours: one only WCAG 2.0's definition counts as a red flash, one only
/// WCAG 2.2's, one both; none of them a general flash.
pub const ONLY_2_0: ([u8; 3], [u8; 3]) = ([255, 0, 0], [200, 0, 0]);
pub const ONLY_2_2: ([u8; 3], [u8; 3]) = ([60, 0, 0], [0, 40, 0]);
pub const BOTH: ([u8; 3], [u8; 3]) = ([255, 0, 0], [128, 128, 128]);

#[test]
fn the_three_pairs_are_what_they_say() {
    let red20 = |(a, b): ([u8; 3], [u8; 3])| (red_measure(a) - red_measure(b)).abs() >= RED_DELTA;
    let red22 =
        |(a, b): ([u8; 3], [u8; 3])| chroma_change(a, b) > 0.2 && (saturated(a) || saturated(b));
    let general = |(a, b): ([u8; 3], [u8; 3])| (luminance(a) - luminance(b)).abs() >= FLASH_DELTA;
    assert!(red20(ONLY_2_0) && !red22(ONLY_2_0) && !general(ONLY_2_0));
    assert!(!red20(ONLY_2_2) && red22(ONLY_2_2) && !general(ONLY_2_2));
    assert!(red20(BOTH) && red22(BOTH) && !general(BOTH));
    // Pure reds of any brightness have one chromaticity.
    assert!(chroma_change([255, 0, 0], [200, 0, 0]) < 1e-9);
    eprintln!(
        "only 2.0: red measure {:.1} apart; only 2.2: {:.1} apart, chromaticity {:.3}; both: {:.1}, {:.3}",
        red_measure(ONLY_2_0.0) - red_measure(ONLY_2_0.1),
        red_measure(ONLY_2_2.0) - red_measure(ONLY_2_2.1),
        chroma_change(ONLY_2_2.0, ONLY_2_2.1),
        red_measure(BOTH.0) - red_measure(BOTH.1),
        chroma_change(BOTH.0, BOTH.1)
    );
}

/// Ten red flashes a second over the whole screen fail, whichever definition catches them, on
/// the processor's measure. None of them is a general flash.
#[test]
fn a_red_flash_by_either_definition_fails() {
    let mut missed = vec![];
    for (name, (a, b)) in [
        ("only WCAG 2.0's", ONLY_2_0),
        ("only WCAG 2.2's", ONLY_2_2),
        ("both", BOTH),
    ] {
        let maps = [reduce(&solid(a), W, H), reduce(&solid(b), W, H)];
        let r = run(&maps);
        if r.red_level != Level::Fail || r.level != Level::Pass {
            missed.push(format!("{name} ({a:?} and {b:?}): {r:?}"));
        }
    }
    assert!(
        missed.is_empty(),
        "red flashes not counted:\n{}",
        missed.join("\n")
    );
}

/// The same red flashes over a square a little more than a quarter of the 10° field fail, and
/// over one a little less pass: WCAG 2.2's red flashes keep the same area rule.
#[test]
fn red_flashes_by_wcag_2_2_keep_the_area_rule() {
    let allowed = Viewing::screen(1.0).flash_area();
    let side_under = ((allowed.sqrt() / 16.0).floor() as u32) * 16;
    for (side, fails) in [(side_under, false), (side_under + 16, true)] {
        let (a, b) = ONLY_2_2;
        let frame = |c: [u8; 3]| {
            let mut v = solid([0, 0, 0]);
            for y in 32..32 + side {
                for x in 32..32 + side {
                    let i = ((y * W + x) * 4) as usize;
                    v[i..i + 3].copy_from_slice(&c);
                }
            }
            reduce(&v, W, H)
        };
        let r = run(&[frame(a), frame(b)]);
        assert_eq!(r.red_level.fails(), fails, "{side} pixels square: {r:?}");
    }
}
