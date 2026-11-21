//! Colour-vision deficiency: whether a palette's colours stay apart for people with the common
//! kinds of it (#217).
//!
//! Each colour is seen as a person with protanopia, deuteranopia or tritanopia would see it, or
//! with the milder protanomaly, deuteranomaly or tritanomaly, through the matrices of Machado,
//! Oliveira and Fernandes (2009), applied to linear sRGB. The colours are then compared in CIELAB
//! with the CIEDE2000 colour difference (Sharma, Wu and Dalal 2005). A palette passes when, for
//! normal vision and every one of the six simulated kinds, every pair of neighbouring stops and
//! every stop against the palette's background stay at least [`MIN_DIFFERENCE`] apart.
//!
//! The model is an approximation: it simulates the average of each kind, and people differ. The
//! book's palettes pages mark the palettes that pass; nothing in JETCARP's drawing depends on it.

/// How a colour is seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vision {
    Normal,
    /// No long-wavelength (L) cones: reds look dark, and red and green alike.
    Protanopia,
    /// No medium-wavelength (M) cones: red and green alike. The commonest kind, with deuteranomaly.
    Deuteranopia,
    /// No short-wavelength (S) cones: blue and green alike, and yellow and pink. Rare.
    Tritanopia,
    /// Protanomaly, deuteranomaly and tritanomaly: the cones are there but shifted. Machado's
    /// severity 0.5, halfway to the full deficiency.
    Protanomaly,
    Deuteranomaly,
    Tritanomaly,
}

/// Every way of seeing a palette is checked under.
pub const VISIONS: [Vision; 7] = [
    Vision::Normal,
    Vision::Protanopia,
    Vision::Deuteranopia,
    Vision::Tritanopia,
    Vision::Protanomaly,
    Vision::Deuteranomaly,
    Vision::Tritanomaly,
];

impl Vision {
    pub fn name(self) -> &'static str {
        match self {
            Vision::Normal => "normal vision",
            Vision::Protanopia => "protanopia",
            Vision::Deuteranopia => "deuteranopia",
            Vision::Tritanopia => "tritanopia",
            Vision::Protanomaly => "protanomaly",
            Vision::Deuteranomaly => "deuteranomaly",
            Vision::Tritanomaly => "tritanomaly",
        }
    }

    /// Machado, Oliveira and Fernandes (2009), table of their supplementary material: severity
    /// 1.0 for the -opias, 0.5 for the -omalies. On linear RGB.
    fn matrix(self) -> Option<[[f64; 3]; 3]> {
        Some(match self {
            Vision::Normal => return None,
            Vision::Protanopia => [
                [0.152286, 1.052583, -0.204868],
                [0.114503, 0.786281, 0.099216],
                [-0.003882, -0.048116, 1.051998],
            ],
            Vision::Deuteranopia => [
                [0.367322, 0.860646, -0.227968],
                [0.280085, 0.672501, 0.047413],
                [-0.011820, 0.042940, 0.968881],
            ],
            Vision::Tritanopia => [
                [1.255528, -0.076749, -0.178779],
                [-0.078411, 0.930809, 0.147602],
                [0.004733, 0.691367, 0.303900],
            ],
            Vision::Protanomaly => [
                [0.458064, 0.679578, -0.137642],
                [0.092785, 0.846313, 0.060902],
                [-0.007494, -0.016807, 1.024301],
            ],
            Vision::Deuteranomaly => [
                [0.547494, 0.607765, -0.155259],
                [0.181692, 0.781742, 0.036566],
                [-0.010410, 0.027275, 0.983136],
            ],
            Vision::Tritanomaly => [
                [1.017277, 0.027029, -0.044306],
                [-0.006113, 0.958479, 0.047634],
                [0.006379, 0.248708, 0.744913],
            ],
        })
    }
}

/// The smallest CIEDE2000 difference a palette keeps between neighbouring stops, and between
/// each stop and its background, under every [`Vision`].
///
/// Why 10: CIEDE2000 is scaled so that a difference of about 1 is just noticeable between two
/// large patches side by side. JETCARP's cells are small, often a few pixels, and the colours to be
/// told apart are scattered over the grid rather than side by side; small marks need differences
/// several times larger than large patches to be told apart as reliably (Szafir 2018). The
/// simulation is also an average, and people with the same kind of deficiency differ. So the bar
/// is ten just-noticeable differences: high enough that the colours of a pattern's ages separate
/// at a glance, and low enough that a five-stop ramp, whose lightness alone has to carry it under
/// the strongest deficiencies, can reach it on a dark background.
pub const MIN_DIFFERENCE: f64 = 10.0;

/// A colour in CIELAB (D65).
pub type Lab = [f64; 3];

fn linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear sRGB to CIELAB, D65 white.
fn lab_of_linear([r, g, b]: [f64; 3]) -> Lab {
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    let f = |t: f64| {
        const D: f64 = 6.0 / 29.0;
        if t > D * D * D {
            t.cbrt()
        } else {
            t / (3.0 * D * D) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// An sRGB colour as `vision` sees it, in CIELAB.
pub fn seen(rgb: [u8; 3], vision: Vision) -> Lab {
    let lin = rgb.map(linear);
    let v = match vision.matrix() {
        None => lin,
        Some(m) => {
            let row =
                |k: usize| (m[k][0] * lin[0] + m[k][1] * lin[1] + m[k][2] * lin[2]).clamp(0.0, 1.0);
            [row(0), row(1), row(2)]
        }
    };
    lab_of_linear(v)
}

/// CIELAB lightness L* of an sRGB colour (0 black to 100 white).
pub fn lightness(rgb: [u8; 3]) -> f64 {
    seen(rgb, Vision::Normal)[0]
}

/// The CIEDE2000 colour difference of two CIELAB colours (kL = kC = kH = 1), as Sharma, Wu and
/// Dalal (2005) give it.
pub fn ciede2000(a: Lab, b: Lab) -> f64 {
    use std::f64::consts::PI;
    let deg = |r: f64| r * 180.0 / PI;
    let rad = |d: f64| d * PI / 180.0;
    let [l1, a1, b1] = a;
    let [l2, a2, b2] = b;
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cb7 = ((c1 + c2) / 2.0).powi(7);
    let g = 0.5 * (1.0 - (cb7 / (cb7 + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |ap: f64, b: f64| {
        if ap == 0.0 && b == 0.0 {
            0.0
        } else {
            deg(b.atan2(ap)).rem_euclid(360.0)
        }
    };
    let (h1p, h2p) = (hue(a1p, b1), hue(a2p, b2));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d > 180.0 {
            d - 360.0
        } else if d < -180.0 {
            d + 360.0
        } else {
            d
        }
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * rad(dh / 2.0).sin();
    let lb = (l1 + l2) / 2.0;
    let cbp = (c1p + c2p) / 2.0;
    let hb = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * rad(hb - 30.0).cos()
        + 0.24 * rad(2.0 * hb).cos()
        + 0.32 * rad(3.0 * hb + 6.0).cos()
        - 0.20 * rad(4.0 * hb - 63.0).cos();
    let dtheta = 30.0 * (-((hb - 275.0) / 25.0).powi(2)).exp();
    let cbp7 = cbp.powi(7);
    let rc = 2.0 * (cbp7 / (cbp7 + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lb - 50.0).powi(2) / (20.0 + (lb - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbp;
    let sh = 1.0 + 0.015 * cbp * t;
    let rt = -rad(2.0 * dtheta).sin() * rc;
    let (x, y, z) = (dl / sl, dc / sc, dhh / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt()
}

/// The difference between two sRGB colours as `vision` sees them.
pub fn difference(a: [u8; 3], b: [u8; 3], vision: Vision) -> f64 {
    ciede2000(seen(a, vision), seen(b, vision))
}

/// The smallest difference found, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Worst {
    pub difference: f64,
    pub vision: Vision,
    /// The stops (indices) concerned; for the background, the stop's index twice.
    pub stops: (usize, usize),
}

/// How a palette fares: its closest neighbouring stops and its stop closest to the background,
/// over every [`Vision`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Check {
    pub neighbours: Option<Worst>,
    pub background: Worst,
}

impl Check {
    /// The smallest of the two differences.
    pub fn least(&self) -> f64 {
        self.neighbours
            .map_or(f64::INFINITY, |w| w.difference)
            .min(self.background.difference)
    }

    /// Every difference is at least [`MIN_DIFFERENCE`].
    pub fn passes(&self) -> bool {
        self.least() >= MIN_DIFFERENCE
    }

    /// What holds it back, in words ("stops 2 and 3 under deuteranopia: 5.8"), or None when it
    /// passes.
    pub fn failure(&self) -> Option<String> {
        if self.passes() {
            return None;
        }
        let n = self.neighbours.filter(|w| w.difference < MIN_DIFFERENCE);
        let b = Some(self.background).filter(|w| w.difference < MIN_DIFFERENCE);
        let mut parts = vec![];
        if let Some(w) = n {
            parts.push(format!(
                "stops {} and {} under {}: {:.1}",
                w.stops.0 + 1,
                w.stops.1 + 1,
                w.vision.name(),
                w.difference
            ));
        }
        if let Some(w) = b {
            parts.push(format!(
                "stop {} against the background under {}: {:.1}",
                w.stops.0 + 1,
                w.vision.name(),
                w.difference
            ));
        }
        Some(parts.join("; "))
    }
}

/// Check a palette's stops (young or low first) and its background.
pub fn check(stops: &[[u8; 3]], background: [u8; 3]) -> Check {
    let mut neighbours: Option<Worst> = None;
    let mut back: Option<Worst> = None;
    let keep = |slot: &mut Option<Worst>, w: Worst| {
        if slot.is_none_or(|s| w.difference < s.difference) {
            *slot = Some(w);
        }
    };
    for v in VISIONS {
        let labs: Vec<Lab> = stops.iter().map(|&s| seen(s, v)).collect();
        let bg = seen(background, v);
        for (i, w) in labs.windows(2).enumerate() {
            let d = ciede2000(w[0], w[1]);
            keep(
                &mut neighbours,
                Worst {
                    difference: d,
                    vision: v,
                    stops: (i, i + 1),
                },
            );
        }
        for (i, &l) in labs.iter().enumerate() {
            keep(
                &mut back,
                Worst {
                    difference: ciede2000(l, bg),
                    vision: v,
                    stops: (i, i),
                },
            );
        }
    }
    Check {
        neighbours,
        background: back.unwrap_or(Worst {
            difference: f64::INFINITY,
            vision: Vision::Normal,
            stops: (0, 0),
        }),
    }
}

/// The closest pair of stops between two palettes, over every [`Vision`]: for JACA's two sides,
/// whose cells of any ages meet. `stops` are (index in `a`, index in `b`).
pub fn closest_between(a: &[[u8; 3]], b: &[[u8; 3]]) -> Worst {
    let mut worst = Worst {
        difference: f64::INFINITY,
        vision: Vision::Normal,
        stops: (0, 0),
    };
    for v in VISIONS {
        let lb: Vec<Lab> = b.iter().map(|&s| seen(s, v)).collect();
        for (i, &x) in a.iter().enumerate() {
            let lx = seen(x, v);
            for (j, &ly) in lb.iter().enumerate() {
                let d = ciede2000(lx, ly);
                if d < worst.difference {
                    worst = Worst {
                        difference: d,
                        vision: v,
                        stops: (i, j),
                    };
                }
            }
        }
    }
    worst
}

/// Lightness runs one way along the stops, by at least `step` (CIELAB L*) from each stop to the
/// next: lightness alone tells the stops apart, whatever else a deficiency takes away.
pub fn lightness_steady(stops: &[[u8; 3]], step: f64) -> bool {
    let l: Vec<f64> = stops.iter().map(|&s| lightness(s)).collect();
    let falls = l.windows(2).all(|w| w[0] - w[1] >= step);
    let rises = l.windows(2).all(|w| w[1] - w[0] >= step);
    falls || rises
}

/// The built-in palettes chosen for colour-vision deficiency: one for any rule, and JACA's two
/// sides (+1, −1). The app's Settings → Accessibility makes them the defaults in one click.
pub const CHOSEN: Chosen = Chosen {
    palette: "Lantern",
    jaca_pos: "Cobalt",
    jaca_neg: "Saffron",
};

/// A set of default palettes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chosen {
    pub palette: &'static str,
    pub jaca_pos: &'static str,
    pub jaca_neg: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs from Sharma, Wu and Dalal's test data for CIEDE2000 (2005), with the differences
    /// they give.
    #[test]
    fn ciede2000_matches_sharma_wu_and_dalal() {
        let pairs: &[(Lab, Lab, f64)] = &[
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, -1.3802, -84.2814], [50.0, 0.0, -82.7485], 1.0000),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0009], 7.1792),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
            ([50.0, -0.001, 2.49], [50.0, 0.0009, -2.49], 4.8045),
            ([50.0, -0.001, 2.49], [50.0, 0.0011, -2.49], 4.7461),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            ([50.0, 2.5, 0.0], [61.0, -5.0, 29.0], 22.8977),
            ([50.0, 2.5, 0.0], [56.0, -27.0, -3.0], 31.9030),
            ([50.0, 2.5, 0.0], [58.0, 24.0, 15.0], 19.4535),
            (
                [60.2574, -34.0099, 36.2677],
                [60.4626, -34.1751, 39.4387],
                1.2644,
            ),
            (
                [63.0109, -31.0961, -5.8663],
                [62.8187, -29.7946, -4.0864],
                1.2630,
            ),
            (
                [22.7233, 20.0904, -46.694],
                [23.0331, 14.973, -42.5619],
                2.0373,
            ),
            (
                [90.9257, -0.5406, -0.9208],
                [88.6381, -0.8985, -0.7239],
                1.5381,
            ),
            (
                [6.7747, -0.2908, -2.4247],
                [5.8714, -0.0985, -2.2286],
                0.6377,
            ),
            ([2.0776, 0.0795, -1.135], [0.9033, -0.0636, -0.5514], 0.9082),
        ];
        for (a, b, want) in pairs {
            let got = ciede2000(*a, *b);
            assert!((got - want).abs() < 1e-4, "{a:?} {b:?}: {got} not {want}");
            assert!((ciede2000(*b, *a) - want).abs() < 1e-4, "symmetric");
        }
    }

    #[test]
    fn simulation_keeps_greys_and_merges_what_each_kind_merges() {
        // Every Machado matrix keeps white, black and greys (its rows sum to 1).
        for v in VISIONS {
            for g in [0u8, 60, 128, 200, 255] {
                let n = seen([g, g, g], Vision::Normal);
                let s = seen([g, g, g], v);
                assert!(ciede2000(n, s) < 0.05, "{v:?} {g}: {s:?}");
            }
        }
        // Red and green, far apart in normal vision, come together without M cones, and red
        // with a darker green without L cones (reds look dark to protanopes); blue and green stay
        // apart for them but come closer without S cones.
        let (red, green) = ([0xd0, 0x40, 0x30], [0x60, 0x90, 0x30]);
        assert!(difference(red, green, Vision::Normal) > 50.0);
        assert!(difference(red, green, Vision::Deuteranopia) < 3.0);
        let dark_green = [0x40, 0x60, 0x30];
        assert!(difference(red, dark_green, Vision::Normal) > 50.0);
        assert!(difference(red, dark_green, Vision::Protanopia) < 3.0);
        let (blue, teal) = ([0x30, 0x60, 0xd0], [0x30, 0x90, 0x90]);
        assert!(difference(blue, teal, Vision::Deuteranopia) > 15.0);
        assert!(
            difference(blue, teal, Vision::Tritanopia) < difference(blue, teal, Vision::Normal)
        );
        // The milder forms lie between.
        let n = difference(red, green, Vision::Normal);
        let m = difference(red, green, Vision::Deuteranomaly);
        let d = difference(red, green, Vision::Deuteranopia);
        assert!(d < m && m < n, "{d} {m} {n}");
    }

    #[test]
    fn a_check_finds_the_closest_stops_and_names_them() {
        let grey = check(&[[0xf0; 3], [0xa0; 3], [0x60; 3]], [0x10; 3]);
        assert!(grey.passes(), "{grey:?}");
        let rg = check(&[[0xd0, 0x40, 0x30], [0x60, 0x90, 0x30]], [0x10; 3]);
        assert!(!rg.passes());
        let w = rg.neighbours.unwrap();
        assert_eq!(w.stops, (0, 1));
        assert!(matches!(
            w.vision,
            Vision::Deuteranopia | Vision::Protanopia
        ));
        assert!(rg.failure().unwrap().starts_with("stops 1 and 2 under "));
        let dark = check(&[[0xff; 3], [0x18; 3]], [0x10; 3]);
        assert_eq!(dark.background.stops, (1, 1));
        assert!(
            dark.failure()
                .unwrap()
                .contains("stop 2 against the background")
        );
        assert!(lightness_steady(&[[0xf0; 3], [0xa0; 3], [0x60; 3]], 10.0));
        assert!(lightness_steady(&[[0x60; 3], [0xa0; 3], [0xf0; 3]], 10.0));
        assert!(!lightness_steady(&[[0xf0; 3], [0x60; 3], [0xa0; 3]], 1.0));
    }
}
