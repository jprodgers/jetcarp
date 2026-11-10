//! A contact sheet of every pattern seed shape on several tilings, labelled with each seed's
//! `describe()`. Usage: `cargo run --release -p jetcarp-core --example seed_sheet -- out.png`.
//!
//! Row groups: square, hexagon, trihexagonal, and Spectre tilings (a binary field), then fills:
//! a multi-state field by region, JACA by region, Lenia values, and sandpile grains.

use jetcarp_core::render::{Image, RenderOptions, render, write_png};
use jetcarp_core::sheet::{Label, labelled_sheet};
use jetcarp_core::*;
use rayon::prelude::*;

const LABEL: [u8; 4] = [0xd0, 0xd4, 0xdc, 0xff];
const THUMB: u32 = 220;

fn shapes() -> Vec<SeedShape> {
    vec![
        SeedShape::All,
        SeedShape::Disc { radius: 0.3 },
        SeedShape::Blobs {
            count: 4,
            min_radius: 0.1,
            max_radius: 0.18,
            wobble: 1.0,
            min_gap: 0.04,
        },
        SeedShape::Clusters {
            count: 50,
            radius_tiles: 2.0,
        },
        SeedShape::Dots {
            spacing_tiles: 12.0,
            jitter: 0.5,
        },
        SeedShape::Ring {
            radius: 0.32,
            width: 0.08,
        },
        SeedShape::Rings {
            count: 4,
            width: 0.05,
        },
        SeedShape::Stripes {
            period: 0.2,
            duty: 0.4,
            angle: 30.0,
        },
        SeedShape::Checker { size: 0.18 },
        SeedShape::Gradient {
            angle: 0.0,
            low: 0.0,
            high: 1.0,
        },
        SeedShape::RadialGradient {
            centre: 1.0,
            edge: 0.0,
        },
        SeedShape::Noise {
            scale: 0.15,
            threshold: 0.5,
            octaves: 3,
        },
        SeedShape::Voronoi {
            sites: 24,
            keep: 0.5,
        },
        SeedShape::Symmetric {
            order: 6,
            mirror: true,
            radius: 0.42,
        },
    ]
}

/// The tilings of the shape rows.
fn tilings() -> Vec<(&'static str, TilingSpec)> {
    vec![
        (
            "square",
            TilingSpec::rect("square", 120, 90, Boundary::Dead),
        ),
        ("hex", TilingSpec::rect("hex", 110, 90, Boundary::Dead)),
        ("trihex", TilingSpec::rect("trihex", 50, 50, Boundary::Dead)),
        (
            "spectre",
            TilingSpec::new("spectre").with_size(SizeSpec::Level(5)),
        ),
    ]
}

/// A thumbnail of one seed on one tiling.
fn thumb(
    spec: &TilingSpec,
    rule: &str,
    seed: SeedSpec,
    palette: &str,
    name: &str,
) -> (Label, Image) {
    let mut ca = CaSpec::new(rule).seed(seed.clone());
    ca.palette = Some(palette.into());
    let mut scene = Scene::new(spec.clone());
    scene.rng_state = 17;
    scene.cas = vec![ca];
    let world = World::from_scene(&scene).unwrap_or_else(|e| panic!("{name}: {e}"));
    let img = render(
        &world,
        &RenderOptions {
            max_side: THUMB,
            outline_min_px: 0.0,
            ..Default::default()
        },
    );
    (
        vec![
            (seed.describe(), LABEL),
            (format!("{name} {} cells", world.topo.len()), LABEL),
        ],
        img,
    )
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: seed_sheet <output.png>");
    let mut jobs: Vec<(TilingSpec, String, SeedSpec, &str, String)> = Vec::new();
    for (name, spec) in tilings() {
        for shape in shapes() {
            let seed = SeedSpec::Pattern {
                shape,
                fill: SeedFill::density(0.85),
            };
            jobs.push((spec.clone(), "B3/S23".into(), seed, "Ember", name.into()));
        }
    }

    // Fills: states and field kinds.
    let hex = TilingSpec::rect("hex", 110, 90, Boundary::Wrap);
    let spectre = TilingSpec::new("spectre").with_size(SizeSpec::Level(5));
    let by_region = |s: SeedShape| SeedSpec::Pattern {
        shape: s,
        fill: SeedFill::density(1.0).states(SeedStates::ByRegion),
    };
    let voronoi = SeedShape::Voronoi {
        sites: 30,
        keep: 1.0,
    };
    let blobs = SeedShape::Blobs {
        count: 6,
        min_radius: 0.08,
        max_radius: 0.16,
        wobble: 1.3,
        min_gap: 0.03,
    };
    let clusters = SeedShape::Clusters {
        count: 80,
        radius_tiles: 3.0,
    };
    let noise = SeedShape::Noise {
        scale: 0.12,
        threshold: 0.5,
        octaves: 3,
    };
    for (spec, tag) in [(hex.clone(), "hex wrap"), (spectre.clone(), "spectre")] {
        for s in [
            voronoi.clone(),
            blobs.clone(),
            clusters.clone(),
            noise.clone(),
        ] {
            jobs.push((
                spec.clone(),
                "Cyclic 14/1".into(),
                by_region(s.clone()),
                "Prism",
                format!("{tag} cyclic"),
            ));
            jobs.push((
                spec.clone(),
                "JACA B3/S23".into(),
                by_region(s),
                "Tide",
                format!("{tag} jaca"),
            ));
        }
    }
    let sym_uniform = SeedSpec::Pattern {
        shape: SeedShape::Symmetric {
            order: 6,
            mirror: false,
            radius: 0.45,
        },
        fill: SeedFill::density(0.5).states(SeedStates::Uniform),
    };
    jobs.push((
        hex.clone(),
        "Cyclic 14/1".into(),
        sym_uniform,
        "Prism",
        "hex states".into(),
    ));
    let lenia = "Lenia R=13,T=10,b=1,m=0.15,s=0.015";
    let sand = |s: SeedShape, d: f64| SeedSpec::Pattern {
        shape: s,
        fill: SeedFill::density(d),
    };
    for (spec, tag) in [(hex.clone(), "hex"), (spectre.clone(), "spectre")] {
        for s in [
            SeedShape::Disc { radius: 0.32 },
            blobs.clone(),
            SeedShape::Rings {
                count: 3,
                width: 0.09,
            },
            noise.clone(),
        ] {
            jobs.push((
                spec.clone(),
                lenia.into(),
                sand(s.clone(), 0.9),
                "Aurora",
                format!("{tag} lenia"),
            ));
            jobs.push((
                spec.clone(),
                "Sandpile".into(),
                sand(s, 0.5),
                "Ember",
                format!("{tag} pile"),
            ));
        }
    }

    let thumbs: Vec<(Label, Image)> = jobs
        .par_iter()
        .map(|(spec, rule, seed, palette, name)| thumb(spec, rule, seed.clone(), palette, name))
        .collect();
    let items: Vec<(Label, &Image)> = thumbs.iter().map(|(l, t)| (l.clone(), t)).collect();
    let sheet = labelled_sheet(&items, 7);
    write_png(std::path::Path::new(&path), &sheet, &[])
        .unwrap_or_else(|e| panic!("write {path}: {e}"));
    println!(
        "{path}: {} x {}, {} seeds",
        sheet.width,
        sheet.height,
        thumbs.len()
    );
}
