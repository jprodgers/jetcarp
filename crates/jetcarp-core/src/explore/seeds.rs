//! Choosing a starting pattern for a candidate.

use super::space::{Fam, Grid};
use crate::rng::Rng;
use crate::seeding::{SeedFill, SeedShape, SeedStates};
use crate::world::SeedSpec;

fn pct(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    ((lo + (hi - lo) * rng.next_f64()) * 100.0).round() / 100.0
}

/// A starting pattern for this family on this grid and a short kind name (for coverage).
pub fn seed_for(fam: Fam, grid: &Grid, rng: &mut Rng) -> (SeedSpec, &'static str) {
    let _ = grid;
    match fam {
        // People preferred one starting cell, with random top rows as the occasional surprise.
        Fam::Wolfram | Fam::HexWolfram | Fam::HexTri => {
            if rng.below(4) != 0 {
                (SeedSpec::TopCenter, "point")
            } else {
                (
                    SeedSpec::TopRandom {
                        density: pct(rng, 0.1, 0.6),
                    },
                    "line",
                )
            }
        }
        // Layers grow outward from what they start with; patterned starts were nearly all dropped.
        Fam::Layered if rng.below(4) != 0 => (SeedSpec::Center, "point"),
        Fam::Turmite => (
            SeedSpec::Ants {
                count: 1 + rng.below(8) as u32,
            },
            "ants",
        ),
        Fam::Sandpile => {
            if rng.below(2) == 0 {
                (
                    SeedSpec::Pile {
                        grains: 10_000 * (1 + rng.below(40)),
                    },
                    "pile",
                )
            } else {
                pattern(fam, rng)
            }
        }
        Fam::Lenia => match rng.below(4) {
            3 => pattern(fam, rng),
            0 => (
                SeedSpec::Gaussian {
                    count: 1 + rng.below(8) as u32,
                    radius: 5.0 + rng.below(12) as f64,
                    peak: 0.7 + 0.3 * rng.next_f64(),
                },
                "bumps",
            ),
            1 => (
                SeedSpec::Blob {
                    radius: pct(rng, 0.1, 0.5),
                    density: pct(rng, 0.3, 0.9),
                },
                "disc",
            ),
            _ => (
                SeedSpec::Lenia {
                    name: "orbium".into(),
                    scale: 1.0,
                },
                "orbium",
            ),
        },
        Fam::VonNeumann => (SeedSpec::Empty, "empty"),
        // Random WireWorld is a dead soup; wiring gives the signals somewhere to go.
        Fam::WireWorld if rng.below(8) != 0 => {
            // Rated circuits: every keep had 50+ regions and 4+ signals per region.
            let sites = 40 + rng.below(40) as u32;
            (
                SeedSpec::Circuit {
                    sites,
                    electrons: sites * (3 + rng.below(4) as u32),
                },
                "circuit",
            )
        }
        Fam::SpectreGlider => (SeedSpec::GliderPair, "pair"),
        _ => pattern(fam, rng),
    }
}

/// What goes into seeded cells, for a family.
fn states_for(fam: Fam, rng: &mut Rng) -> SeedStates {
    match fam {
        Fam::Jaca => match rng.below(3) {
            0 => SeedStates::Signed {
                negative: pct(rng, 0.2, 0.8),
            },
            1 => SeedStates::ByRegion,
            _ => SeedStates::Signed { negative: 0.5 },
        },
        Fam::Cyclic | Fam::Rps | Fam::Hodgepodge | Fam::GreenbergHastings | Fam::Priority | Fam::WireWorld => {
            match rng.below(4) {
                0 => SeedStates::ByRegion,
                1 => SeedStates::One,
                _ => SeedStates::Uniform,
            }
        }
        _ => SeedStates::One,
    }
}

/// A random shape with a random fill. Most weight goes to soups and symmetric shapes, which rated
/// best by eye, then blobs and small clusters, which is where gliders and growth come from.
fn pattern(fam: Fam, rng: &mut Rng) -> (SeedSpec, &'static str) {
    let states = states_for(fam, rng);
    let r = rng.below(100);
    let (shape, kind, density): (SeedShape, &'static str, f64) = match r {
        0..=27 => (SeedShape::All, "soup", pct(rng, 0.08, 0.65)),
        28..=35 => (SeedShape::All, "sparse", pct(rng, 0.01, 0.08)),
        36..=44 => (
            SeedShape::Blobs {
                count: 1 + rng.below(7) as u32,
                min_radius: pct(rng, 0.03, 0.08),
                max_radius: pct(rng, 0.08, 0.2),
                wobble: pct(rng, 0.0, 0.6),
                min_gap: pct(rng, 0.0, 0.1),
            },
            "blobs",
            pct(rng, 0.2, 0.75),
        ),
        45..=52 => (
            SeedShape::Clusters {
                count: 4 + rng.below(90) as u32,
                radius_tiles: 1.5 + 5.0 * rng.next_f64(),
            },
            "clusters",
            pct(rng, 0.3, 0.9),
        ),
        53..=58 => (
            SeedShape::Dots {
                spacing_tiles: 8.0 + 32.0 * rng.next_f64(),
                jitter: pct(rng, 0.0, 0.9),
            },
            "dots",
            pct(rng, 0.5, 1.0),
        ),
        59..=65 => (
            SeedShape::Disc {
                radius: pct(rng, 0.05, 0.45),
            },
            "disc",
            pct(rng, 0.15, 0.75),
        ),
        66..=69 => (
            SeedShape::Ring {
                radius: pct(rng, 0.1, 0.42),
                width: pct(rng, 0.01, 0.1),
            },
            "ring",
            pct(rng, 0.3, 1.0),
        ),
        70..=72 => (
            SeedShape::Rings {
                count: 2 + rng.below(6) as u32,
                width: pct(rng, 0.01, 0.05),
            },
            "rings",
            pct(rng, 0.3, 1.0),
        ),
        73..=74 => (
            SeedShape::Stripes {
                period: pct(rng, 0.04, 0.3),
                duty: pct(rng, 0.1, 0.6),
                angle: rng.below(180) as f64,
            },
            "stripes",
            pct(rng, 0.2, 1.0),
        ),
        75 => (
            SeedShape::Checker {
                size: pct(rng, 0.03, 0.2),
            },
            "checker",
            pct(rng, 0.3, 1.0),
        ),
        76..=78 => (
            SeedShape::Gradient {
                angle: rng.below(360) as f64,
                low: pct(rng, 0.0, 0.2),
                high: pct(rng, 0.6, 1.0),
            },
            "gradient",
            pct(rng, 0.4, 1.0),
        ),
        79..=80 => (
            SeedShape::RadialGradient {
                centre: pct(rng, 0.5, 1.0),
                edge: pct(rng, 0.0, 0.3),
            },
            "radial",
            pct(rng, 0.4, 1.0),
        ),
        81..=84 => (
            SeedShape::Noise {
                scale: pct(rng, 0.03, 0.25),
                threshold: pct(rng, 0.35, 0.7),
                octaves: 1 + rng.below(4) as u32,
            },
            "noise",
            pct(rng, 0.3, 1.0),
        ),
        85..=88 => (
            SeedShape::Voronoi {
                sites: 4 + rng.below(60) as u32,
                keep: pct(rng, 0.2, 1.0),
            },
            "voronoi",
            pct(rng, 0.3, 1.0),
        ),
        _ => (
            SeedShape::Symmetric {
                order: [2, 3, 4, 6, 8][rng.below(5) as usize],
                mirror: rng.below(2) == 0,
                radius: pct(rng, 0.08, 0.4),
            },
            "symmetric",
            pct(rng, 0.2, 0.8),
        ),
    };
    (
        SeedSpec::Pattern {
            shape,
            fill: SeedFill { density, states },
        },
        kind,
    )
}
