//! Portable, deterministic maths for anything that feeds a cell's state.
//!
//! The platform's `exp`, `hypot`, `sin`, ... come from the C maths library (or the compiler's
//! intrinsics), which may round differently in the last bit on another OS or CPU. Lenia stores
//! exact f32 values and hashes their bits, so one different bit changes the hash. These functions
//! use only IEEE-754 addition, multiplication, division, and square root (each correctly rounded
//! everywhere) in a fixed order, so they give the same bits on every platform. Rust never fuses
//! `a * b + c` into an FMA unless asked (`mul_add`), and none is used here.
//!
//! Also `sin_cos`, `atan2`, `hypot` and `powi` for the geometry that builds tilings.
//!
//! Accuracy: [`exp`] is within about 2 units in the last place (checked against the platform's
//! `exp` in the tests); that is plenty for a CA, and the point is that it is the same everywhere.

/// ln 2 split into a high part with its low 32 bits zero (so `k * LN2_HI` is exact for
/// |k| < 2^20) and the rest, ln 2 − LN2_HI rounded to f64 (computed to 60 digits; subtracting
/// in f64 would lose the bits beyond `LN_2`).
const LN2_HI: f64 = f64::from_bits(0x3FE6_2E42_FEE0_0000);
const LN2_LO: f64 = f64::from_bits(0x3DEA_39EF_3579_3C76);

/// e^x, deterministic on every platform.
///
/// Range reduction `x = k ln 2 + r` with |r| <= ln 2 / 2 (Cody and Waite's two-part ln 2), then
/// the Taylor series of e^r to degree 13 by Horner's rule (the truncation error is below
/// 4e-18 relative, under half an f64 ulp), then the exact scaling by 2^k built from its bits.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_1 {
        return 0.0;
    }
    let k = (x / std::f64::consts::LN_2).round();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    // 1/n! for n = 13 down to 1.
    const C: [f64; 13] = [
        1.0 / 6_227_020_800.0,
        1.0 / 479_001_600.0,
        1.0 / 39_916_800.0,
        1.0 / 3_628_800.0,
        1.0 / 362_880.0,
        1.0 / 40_320.0,
        1.0 / 5_040.0,
        1.0 / 720.0,
        1.0 / 120.0,
        1.0 / 24.0,
        1.0 / 6.0,
        1.0 / 2.0,
        1.0,
    ];
    let mut p = 0.0;
    for c in C {
        p = p * r + c;
    }
    let er = p * r + 1.0;
    scale2(er, k as i32)
}

/// `v * 2^k` for v in about [0.7, 1.5], exact (in two steps so subnormal results round once,
/// as a single multiplication would).
fn scale2(v: f64, k: i32) -> f64 {
    let pow2 = |e: i32| f64::from_bits(((e + 1023) as u64) << 52);
    if k > 1023 {
        v * pow2(1023) * pow2(k - 1023)
    } else if k < -1022 {
        // The result may be subnormal: the first step is exact, the second rounds once.
        v * pow2(k + 1000) * pow2(-1000)
    } else {
        v * pow2(k)
    }
}

/// Distance between two points as `sqrt(dx² + dy²)`: square root is correctly rounded
/// everywhere, unlike `hypot`.
pub fn dist(p: [f64; 2], q: [f64; 2]) -> f64 {
    let (dx, dy) = (p[0] - q[0], p[1] - q[1]);
    (dx * dx + dy * dy).sqrt()
}

/// x⁴ as (x²)², the same bits as the `powi(4)` it replaces on platforms where `powi` multiplies
/// by repeated squaring, and the same everywhere.
pub fn pow4(x: f64) -> f64 {
    let y = x * x;
    y * y
}

/// π/2 in three parts of 33 significant bits (so `k * PIO2_n` is exact for |k| < 2^20) plus a
/// tail: Cody and Waite's range reduction. The values are the bits of π/2, a mathematical
/// constant.
const PIO2_1: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
const PIO2_2: f64 = f64::from_bits(0x3DD0_B461_1A60_0000);
const PIO2_3: f64 = f64::from_bits(0x3BA3_198A_2E00_0000);
const PIO2_3T: f64 = f64::from_bits(0x397B_839A_2520_49C1);

/// `(sin x, cos x)`, deterministic on every platform.
///
/// `x = k π/2 + r` with |r| ≤ π/4 (Cody and Waite, π/2 in four parts; exact enough for the
/// |x| < 10^5 that geometry uses, still deterministic beyond), then the Taylor series of sin r to
/// degree 21 and cos r to degree 22 by Horner's rule in r² (truncation below 10^-19), then the
/// quadrant. Within a few units in the last place of the platform's `sin_cos`.
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    let k = (x * std::f64::consts::FRAC_2_PI).round();
    let r = (((x - k * PIO2_1) - k * PIO2_2) - k * PIO2_3) - k * PIO2_3T;
    let r2 = r * r;
    // (-1)^n / (2n+1)! for n = 10 down to 1, and (-1)^n / (2n)! for n = 11 down to 1.
    const S: [f64; 10] = [
        1.0 / 51_090_942_171_709_440_000.0,
        -1.0 / 121_645_100_408_832_000.0,
        1.0 / 355_687_428_096_000.0,
        -1.0 / 1_307_674_368_000.0,
        1.0 / 6_227_020_800.0,
        -1.0 / 39_916_800.0,
        1.0 / 362_880.0,
        -1.0 / 5_040.0,
        1.0 / 120.0,
        -1.0 / 6.0,
    ];
    const C: [f64; 11] = [
        -1.0 / 1_124_000_727_777_607_680_000.0,
        1.0 / 2_432_902_008_176_640_000.0,
        -1.0 / 6_402_373_705_728_000.0,
        1.0 / 20_922_789_888_000.0,
        -1.0 / 87_178_291_200.0,
        1.0 / 479_001_600.0,
        -1.0 / 3_628_800.0,
        1.0 / 40_320.0,
        -1.0 / 720.0,
        1.0 / 24.0,
        -1.0 / 2.0,
    ];
    let mut ps = 0.0;
    for c in S {
        ps = ps * r2 + c;
    }
    let s = r + r * (r2 * ps);
    let mut pc = 0.0;
    for c in C {
        pc = pc * r2 + c;
    }
    let c = 1.0 + r2 * pc;
    match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// sin x, deterministic ([`sin_cos`]).
pub fn sin(x: f64) -> f64 {
    sin_cos(x).0
}
/// cos x, deterministic ([`sin_cos`]).
pub fn cos(x: f64) -> f64 {
    sin_cos(x).1
}

/// atan t for 0 ≤ t ≤ 1: two argument halvings `t → t / (1 + sqrt(1 + t²))` bring t below
/// tan(π/16), then the series to degree 25.
fn atan01(t: f64) -> f64 {
    let h = |t: f64| t / (1.0 + (1.0 + t * t).sqrt());
    let u = h(h(t));
    let u2 = u * u;
    let mut p = 0.0;
    for n in (1..=12).rev() {
        let sign = if n % 2 == 0 { 1.0 } else { -1.0 };
        let c = sign / (2 * n + 1) as f64;
        p = p * u2 + c;
    }
    4.0 * (u + u * (u2 * p))
}

/// atan2(y, x) in (-π, π], deterministic on every platform (only +, −, ×, ÷ and square root).
pub fn atan2(y: f64, x: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return if x.is_sign_negative() {
            PI.copysign(y)
        } else {
            0.0f64.copysign(y)
        };
    }
    if x == 0.0 {
        return FRAC_PI_2.copysign(y);
    }
    let (ax, ay) = (x.abs(), y.abs());
    let a = if ax.is_infinite() || ay.is_infinite() {
        match (ax.is_infinite(), ay.is_infinite()) {
            (true, true) => std::f64::consts::FRAC_PI_4,
            (true, false) => 0.0,
            _ => FRAC_PI_2,
        }
    } else if ay <= ax {
        atan01(ay / ax)
    } else {
        FRAC_PI_2 - atan01(ax / ay)
    };
    let a = if x < 0.0 { PI - a } else { a };
    a.copysign(y)
}

/// `sqrt(x² + y²)`: square root is correctly rounded everywhere, unlike `hypot`.
pub fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// `x^n` by repeated squaring, the same bits everywhere (unlike `powi`, whose expansion is up
/// to the compiler and its runtime library).
pub fn powi(x: f64, n: i32) -> f64 {
    let mut b = if n < 0 { 1.0 / x } else { x };
    let mut e = n.unsigned_abs();
    let mut r = 1.0;
    while e > 0 {
        if e & 1 == 1 {
            r *= b;
        }
        b *= b;
        e >>= 1;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const PINNED_TRIG: u64 = 0xaac7_8542_bbf7_5354;

    fn ulps(a: f64, b: f64) -> u64 {
        (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
    }

    #[test]
    fn exp_matches_the_platform_to_a_few_ulps() {
        let mut worst = 0;
        let mut x = -740.0;
        while x < 709.0 {
            let (a, b) = (exp(x), x.exp());
            if b.is_normal() {
                worst = worst.max(ulps(a, b));
            }
            x += 0.013_7;
        }
        // Lenia's range: growth exponents in [-50, 0], kernel cores in [-inf, 0].
        let mut x = -50.0;
        while x <= 0.0 {
            worst = worst.max(ulps(exp(x), x.exp()));
            x += 1.0e-4;
        }
        assert!(worst <= 2, "worst {worst} ulps");
        assert_eq!(exp(0.0), 1.0);
        assert!(ulps(exp(1.0), std::f64::consts::E) <= 1);
        assert_eq!(exp(-1000.0), 0.0);
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert!(exp(f64::NAN).is_nan());
        assert!(exp(-744.0) > 0.0);
    }

    #[test]
    fn exp_bits_are_pinned() {
        // These bit patterns are what every platform must produce.
        for (x, bits) in [
            (-0.5, 0x3FE3_68B2_FC6F_960Au64),
            (-3.0, 0x3FA9_7DB0_CCCE_B0AF),
            (-12.345, 0x3ED2_4057_2F47_CB82),
            (0.7, 0x4000_1C2A_6126_8987),
        ] {
            assert_eq!(exp(x).to_bits(), bits, "exp({x}) = {:e}", exp(x));
        }
    }

    #[test]
    fn trig_matches_the_platform_to_a_few_ulps() {
        let mut worst = (0, 0, 0);
        let mut x = -40.0;
        while x < 40.0 {
            let (s, c) = sin_cos(x);
            let (ps, pc) = x.sin_cos();
            // Measured relative to the larger of the two (near a zero, ulps of 1).
            let rel = |a: f64, b: f64| ((a - b).abs() / f64::EPSILON).round() as u64;
            worst.0 = worst.0.max(rel(s, ps));
            worst.1 = worst.1.max(rel(c, pc));
            let y = x * 0.37 + 0.11;
            worst.2 = worst.2.max(rel(atan2(y, x), y.atan2(x)));
            x += 0.000_731;
        }
        assert!(
            worst.0 <= 4 && worst.1 <= 4 && worst.2 <= 8,
            "worst {worst:?}"
        );
        assert_eq!(sin_cos(0.0), (0.0, 1.0));
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert!(atan2(-1e-300, -1.0) < -3.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(powi(1.5, 3), 3.375);
        assert_eq!(powi(2.0, -2), 0.25);
    }

    #[test]
    fn trig_bits_are_pinned() {
        // What every platform must produce.
        let pins: Vec<u64> = [0.5, 1.0, 2.0943951023931953, -0.6283185307179586, 7.5]
            .iter()
            .flat_map(|&x| {
                let (s, c) = sin_cos(x);
                [s.to_bits(), c.to_bits(), atan2(s, c).to_bits()]
            })
            .collect();
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for v in pins {
            h = (h ^ v).wrapping_mul(0x100_0000_01b3);
        }
        assert_eq!(h, PINNED_TRIG, "trig hash {h:#x}");
    }

    #[test]
    fn dist_and_pow4() {
        assert_eq!(dist([0.0, 0.0], [3.0, 4.0]), 5.0);
        assert_eq!(pow4(0.5), 0.0625);
        for x in [0.1, 0.37, 0.999_999, 1e-3] {
            let y = std::hint::black_box(x) * std::hint::black_box(x);
            assert_eq!(pow4(x), y * y);
        }
    }
}
