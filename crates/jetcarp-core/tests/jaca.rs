//! JACA (balanced ternary, annihilating) rules.

use jetcarp_core::jaca::{JacaRule, NEG, POS};
use jetcarp_core::render::{ColorBy, Palette};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};

fn world(tiling: &str, w: u32, h: u32, rule: &str, seed: SeedSpec, rng: u64) -> World {
    let mut wd = World::new(TilingSpec::rect(tiling, w, h, Boundary::Wrap), rng).unwrap();
    wd.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(seed),
    })
    .unwrap();
    wd
}

fn cells(w: &World) -> &[u8] {
    &w.fields[w.cas[0].field].state
}

fn set(w: &mut World, cs: &[(u32, u8)]) {
    let id = w.cas[0].id;
    w.apply(Command::Seed {
        id,
        seed: SeedSpec::Cells {
            cells: cs.iter().map(|&(c, s)| [c, s as u32]).collect(),
        },
    })
    .unwrap();
}

#[test]
fn notation_round_trips() {
    for (text, want) in [
        ("JACA B3/S23", "JACA B3/S23"),
        ("jaca b3/s23 b3/s23", "JACA B3/S23"),
        ("JACA +B3/S23 -B36/S23", "JACA +B3/S23 -B36/S23"),
        ("JACA B3/S23, B36/S23", "JACA +B3/S23 -B36/S23"),
        ("JACA -B36/S23 +B3/S23", "JACA +B3/S23 -B36/S23"),
        ("JACA B3/S23 blast 2", "JACA B3/S23 blast 2"),
        ("JACA B3/S23 blast=3", "JACA B3/S23 blast 3"),
        ("JACA B3/S23 x1", "JACA B3/S23 blast 1"),
        ("JACA B2/S", "JACA B2/S"),
        // Counts above 9 (triangles touch 12 cells) use commas.
        ("JACA +B2,10/S3,11 -B2/S", "JACA +B2,10/S3,11 -B2/S"),
        ("JACA B2,10/S3, blast 1", "JACA B2,10/S3 blast 1"),
        ("JACA +B10,/S1,5,11 -B4/S3", "JACA +B10,/S1,5,11 -B4/S3"),
    ] {
        let (r, _) = Rule::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(r.notation(), want, "{text}");
        assert_eq!(Rule::parse(want).unwrap().0, r, "{want}");
    }
    let (r, nb) = Rule::parse("JACA B2/S34V").unwrap();
    assert_eq!(nb, Some(Nb::Edge));
    assert!(matches!(r, Rule::Jaca(_)));
    for bad in [
        "JACA",
        "JACA B3/S23 B3/S23 B3/S23",
        "JACA B3",
        "JACA B3/S23 blast",
        "JACA B3/S23 blast 99",
        "JACA B03/S23",
    ] {
        assert!(Rule::parse(bad).is_err(), "{bad} should not parse");
    }
}

#[test]
fn edge_neighbourhood_notation_keeps_the_v() {
    let w = world(
        "square",
        10,
        10,
        "JACA +B2/S34V -B3/S23V",
        SeedSpec::Empty,
        1,
    );
    assert_eq!(w.cas[0].nb, Nb::Edge);
    let n = w.cas[0].notation();
    assert_eq!(n, "JACA +B2/S34V -B3/S23V");
    assert_eq!(Rule::parse(&n).unwrap().1, Some(Nb::Edge));
}

/// Seeded with one sign only, JACA is the plain B/S rule.
#[test]
fn one_sign_runs_like_the_bs_rule() {
    for (tiling, bs) in [
        ("square", "B3/S23"),
        ("hex", "B2/S34"),
        ("triangle", "B45/S34"),
    ] {
        let life = world(tiling, 40, 30, bs, SeedSpec::Random { density: 0.35 }, 7);
        let seed: Vec<u8> = cells(&life).to_vec();
        for sign in [POS, NEG] {
            let mut j = world(tiling, 40, 30, &format!("JACA {bs}"), SeedSpec::Empty, 7);
            let cs: Vec<(u32, u8)> = seed
                .iter()
                .enumerate()
                .filter(|(_, s)| **s == 1)
                .map(|(i, _)| (i as u32, sign))
                .collect();
            set(&mut j, &cs);
            let mut l2 = world(tiling, 40, 30, bs, SeedSpec::Random { density: 0.35 }, 7);
            for g in 0..60 {
                let want: Vec<u8> = cells(&l2).iter().map(|&s| s * sign).collect();
                assert_eq!(cells(&j), &want[..], "{tiling} {bs} sign {sign} gen {g}");
                l2.step();
                j.step();
            }
            assert_eq!(j.cas[0].heat.as_ref().unwrap().total_contacts, 0);
        }
    }
}

#[test]
fn touching_opposites_annihilate() {
    // Two blinkers side by side would live forever in Life; opposite signs one cell apart touch.
    let mut w = world("square", 20, 20, "JACA B3/S23", SeedSpec::Empty, 1);
    let g = w.topo.rows().unwrap().clone();
    let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
    set(
        &mut w,
        &[
            (at(5, 4), POS),
            (at(5, 5), POS),
            (at(5, 6), POS),
            (at(6, 4), NEG),
            (at(6, 5), NEG),
            (at(6, 6), NEG),
        ],
    );
    w.step();
    let heat = w.cas[0].heat.as_ref().unwrap();
    assert_eq!(heat.last.contacts, 6);
    assert_eq!(heat.last.destroyed, 6);
    for c in [at(5, 4), at(5, 5), at(5, 6), at(6, 4), at(6, 5), at(6, 6)] {
        assert_eq!(cells(&w)[c as usize], 0);
        assert_eq!(heat.glow[c as usize], 255);
    }
    // The births of the two halves are unaffected: each blinker still tries to turn.
    // (+ births at x=4 need 3 positive neighbours: (4,5) has (5,4),(5,5),(5,6).)
    assert_eq!(cells(&w)[at(4, 5) as usize], POS);
    assert_eq!(cells(&w)[at(7, 5) as usize], NEG);
}

#[test]
fn far_apart_signs_do_not_interact() {
    // A positive and a negative block with a gap of one cell: never touching, both still lifes.
    let mut w = world("square", 20, 20, "JACA B3/S23 blast 3", SeedSpec::Empty, 1);
    let g = w.topo.rows().unwrap().clone();
    let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
    let block = |x: i32, y: i32, s: u8| {
        [
            (at(x, y), s),
            (at(x + 1, y), s),
            (at(x, y + 1), s),
            (at(x + 1, y + 1), s),
        ]
    };
    let mut cs = block(4, 4, POS).to_vec();
    cs.extend(block(7, 4, NEG));
    set(&mut w, &cs);
    let start = cells(&w).to_vec();
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(cells(&w), &start[..]);
}

#[test]
fn blast_radius_grows_through_the_neighbourhood() {
    // A +/- pair next to two +1 blocks (still lifes) 2 and 5 Moore steps from the pair:
    // radius r clears every cell within r steps of the pair.
    for blast in 0..4u64 {
        let rule = format!("JACA B3/S23 blast {blast}");
        let mut w = world("square", 30, 30, &rule, SeedSpec::Empty, 1);
        let g = w.topo.rows().unwrap().clone();
        let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
        let mut cs = vec![(at(14, 15), POS), (at(15, 15), NEG)];
        for x in [17, 20] {
            cs.extend([
                (at(x, 15), POS),
                (at(x + 1, 15), POS),
                (at(x, 16), POS),
                (at(x + 1, 16), POS),
            ]);
        }
        set(&mut w, &cs);
        w.step();
        let h = w.cas[0].heat.as_ref().unwrap();
        assert_eq!(h.last.contacts, 2);
        assert_eq!(
            cells(&w)[at(17, 15) as usize] == 0,
            blast >= 2,
            "blast {blast}"
        );
        assert_eq!(cells(&w)[at(20, 15) as usize], POS, "blast {blast}");
        // The zone: two overlapping Chebyshev balls of radius `blast`, side by side.
        let side = 2 * blast + 1;
        assert_eq!(h.last.destroyed, side * (side + 1), "blast {blast}");
    }
}

#[test]
fn double_births_cancel() {
    // An empty cell with three +1 and three -1 neighbours (none touching the other sign).
    let mut w = world("square", 20, 20, "JACA B3/S23", SeedSpec::Empty, 1);
    let g = w.topo.rows().unwrap().clone();
    let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
    // Centre (10,10): +1 along the top row, -1 along the bottom row; they are two rows apart.
    set(
        &mut w,
        &[
            (at(9, 9), POS),
            (at(10, 9), POS),
            (at(11, 9), POS),
            (at(9, 11), NEG),
            (at(10, 11), NEG),
            (at(11, 11), NEG),
        ],
    );
    w.step();
    let h = w.cas[0].heat.as_ref().unwrap();
    assert_eq!(h.last.contacts, 0);
    assert_eq!(h.last.cancelled, 1);
    assert_eq!(cells(&w)[at(10, 10) as usize], 0);
    // The outer births happen: (10,8) for +1 and (10,12) for -1.
    assert_eq!(cells(&w)[at(10, 8) as usize], POS);
    assert_eq!(cells(&w)[at(10, 12) as usize], NEG);
}

#[test]
fn different_rules_per_sign() {
    // Seeds (B2/S) for -1, Life for +1: a lone pair of -1 cells explodes, a +1 pair dies.
    let mut w = world("square", 30, 30, "JACA +B3/S23 -B2/S", SeedSpec::Empty, 1);
    let g = w.topo.rows().unwrap().clone();
    let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
    set(
        &mut w,
        &[
            (at(5, 5), POS),
            (at(6, 5), POS),
            (at(20, 20), NEG),
            (at(21, 20), NEG),
        ],
    );
    w.step();
    let pos = cells(&w).iter().filter(|&&s| s == POS).count();
    let neg = cells(&w).iter().filter(|&&s| s == NEG).count();
    assert_eq!(pos, 0);
    assert_eq!(neg, 4);
}

#[test]
fn seeds_and_defaults() {
    let w = world(
        "square",
        60,
        40,
        "JACA B3/S23",
        SeedSpec::Halves { density: 0.4 },
        3,
    );
    let g = w.topo.rows().unwrap().clone();
    let s = cells(&w);
    let mut left = [0usize; 3];
    let mut right = [0usize; 3];
    for y in 0..40 {
        for x in 0..60 {
            let v = s[g.index(x, y, 0)] as usize;
            if x < 30 { left[v] += 1 } else { right[v] += 1 }
        }
    }
    assert_eq!(left[NEG as usize], 0);
    assert_eq!(right[POS as usize], 0);
    assert!(left[POS as usize] > 400 && right[NEG as usize] > 400);
    let w = world(
        "hex",
        60,
        40,
        "JACA B2/S34",
        SeedSpec::Signed {
            density: 0.5,
            negative: 0.25,
        },
        3,
    );
    let s = cells(&w);
    let pos = s.iter().filter(|&&v| v == POS).count() as f64;
    let neg = s.iter().filter(|&&v| v == NEG).count() as f64;
    assert!((neg / (pos + neg) - 0.25).abs() < 0.05);
    // Defaults: palettes, glow, and a mixed soup.
    let mut w = World::new(TilingSpec::rect("square", 20, 20, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("JACA B3/S23"),
    })
    .unwrap();
    let ca = &w.cas[0];
    assert_eq!(ca.palette, "Tide");
    assert_eq!(ca.palette_neg, "Ember");
    assert!(ca.glow);
    assert_eq!(
        ca.seed,
        SeedSpec::Signed {
            density: 0.3,
            negative: 0.5
        }
    );
    assert_eq!(ca.name, "JACA Life");
}

#[test]
fn scenes_keep_the_negative_palette() {
    let mut w = World::new(TilingSpec::rect("square", 20, 20, Boundary::Wrap), 1).unwrap();
    let mut spec = CaSpec::new("JACA +B3/S23 -B36/S23 blast 1");
    spec.palette = Some("#4fc3ff".into());
    spec.palette_neg = Some("#401010-#ff4040".into());
    spec.glow = false;
    w.apply(Command::AddCa { ca: spec }).unwrap();
    let sc = w.to_scene();
    let json = serde_json::to_string(&sc).unwrap();
    assert!(json.contains("palette_neg"), "{json}");
    let back = World::from_scene(&serde_json::from_str(&json).unwrap()).unwrap();
    let ca = &back.cas[0];
    assert_eq!(ca.palette_neg, "#401010-#ff4040");
    assert!(!ca.glow);
    assert_eq!(ca.rule.notation(), "JACA +B3/S23 -B36/S23 blast 1");
    // Non-JACA CAs do not write the field.
    let mut w = World::new(TilingSpec::rect("square", 20, 20, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23"),
    })
    .unwrap();
    let json = serde_json::to_string(&w.to_scene()).unwrap();
    assert!(!json.contains("palette_neg") && !json.contains("glow"));
}

#[test]
fn hex_palettes() {
    let p = Palette::from_hex("#4fc3ff").unwrap();
    assert_eq!(p.stops, vec![[0x4f, 0xc3, 0xff]]);
    let p = Palette::from_hex("#000-#fff, #808080").unwrap();
    assert_eq!(p.stops.len(), 3);
    assert_eq!(p.stops[1], [255, 255, 255]);
    assert!(Palette::from_hex("Ember").is_none());
    assert!(Palette::from_hex("#12345").is_none());
    assert!(jetcarp_core::render::builtin_palette("#ff0000").is_ok());
}

#[test]
fn signs_render_in_their_own_colours() {
    let mut w = World::new(TilingSpec::rect("square", 10, 10, Boundary::Wrap), 1).unwrap();
    let mut spec = CaSpec::new("JACA B3/S23");
    spec.palette = Some("#0000ff".into());
    spec.palette_neg = Some("#ff0000".into());
    spec.seed = Some(SeedSpec::Empty);
    w.apply(Command::AddCa { ca: spec }).unwrap();
    let g = w.topo.rows().unwrap().clone();
    let at = |x: i32, y: i32| g.index(x, y, 0) as u32;
    set(&mut w, &[(at(1, 1), POS), (at(5, 5), NEG), (at(6, 5), POS)]);
    let c = jetcarp_core::render::cell_colors(&w);
    assert_eq!(c[at(1, 1) as usize], [0, 0, 255, 255]);
    assert_eq!(c[at(5, 5) as usize], [255, 0, 0, 255]);
    let bg = c[at(8, 8) as usize];
    w.step();
    let c = jetcarp_core::render::cell_colors(&w);
    // The annihilated pair glows; untouched empty cells keep the background.
    assert_eq!(c[at(5, 5) as usize], jetcarp_core::render::GLOW);
    assert_eq!(c[at(8, 8) as usize], bg);
    let id = w.cas[0].id;
    w.apply(Command::UpdateCa {
        id,
        change: jetcarp_core::world::CaChange {
            glow: Some(false),
            color_by: Some(ColorBy::State),
            ..Default::default()
        },
    })
    .unwrap();
    let c = jetcarp_core::render::cell_colors(&w);
    assert_eq!(c[at(5, 5) as usize], bg);
}

#[test]
fn rule_struct_helpers() {
    let r = JacaRule::symmetric(1 << 3, 1 << 2 | 1 << 3, 2);
    assert!(r.is_symmetric());
    assert_eq!(r.notation(), "JACA B3/S23 blast 2");
    assert_eq!(jetcarp_core::jaca::value(NEG), -1);
    assert_eq!(jetcarp_core::jaca::state_of(-5), NEG);
}

fn counts(w: &World) -> (usize, usize) {
    let s = cells(w);
    (
        s.iter().filter(|&&c| c == POS).count(),
        s.iter().filter(|&&c| c == NEG).count(),
    )
}

fn reseed(w: &mut World, mode: &str) {
    let mode: jetcarp_core::reseed::ReseedMode =
        serde_json::from_value(jetcarp_core::reseed::mode_json(mode)).unwrap();
    w.apply(Command::Reseed { ca: None, mode }).unwrap();
}

#[test]
fn reseed_signs_on_jaca() {
    let empty = SeedSpec::Random { density: 0.0 };
    let big =
        r#"{"kind":"blobs","count":6,"min_radius":0.1,"max_radius":0.2,"live":0.5,"sign":"S"}"#;
    let blobs = |s: &str| -> jetcarp_core::reseed::ReseedMode {
        serde_json::from_str(&big.replace('S', s)).unwrap()
    };
    for (sign, want_pos, want_neg) in [
        ("pos", true, false),
        ("neg", false, true),
        ("mixed", true, true),
    ] {
        let mut w = world("square", 120, 120, "JACA B3/S23", empty.clone(), 5);
        w.apply(Command::Reseed {
            ca: None,
            mode: blobs(sign),
        })
        .unwrap();
        let (p, n) = counts(&w);
        assert_eq!((p > 0, n > 0), (want_pos, want_neg), "{sign}: {p} {n}");
        if sign == "mixed" {
            let r = p as f64 / (p + n) as f64;
            assert!((0.45..0.55).contains(&r), "mixed ratio {r}");
        }
    }
    // Either: each blob takes one sign, so with many blobs both appear, but a single blob is
    // all one sign.
    let mut w = world("square", 120, 120, "JACA B3/S23", empty.clone(), 5);
    reseed(&mut w, "rain:either");
    let (p, n) = counts(&w);
    assert!(p > 0 && n > 0, "either rain: {p} {n}");
    for rng in 0..6 {
        let mut w = world("square", 120, 120, "JACA B3/S23", empty.clone(), rng);
        w.apply(Command::Reseed {
            ca: None,
            mode: blobs("either").with_count(1),
        })
        .unwrap();
        let (p, n) = counts(&w);
        assert!(p == 0 || n == 0, "one blob, both signs: {p} {n}");
    }
    // Alternate: one blob per reseed flips sign every time.
    let mut w = world("square", 120, 120, "JACA B3/S23", empty.clone(), 9);
    let mut signs = vec![];
    for _ in 0..4 {
        w.apply(Command::Clear { id: w.cas[0].id }).unwrap();
        w.apply(Command::Reseed {
            ca: None,
            mode: blobs("alternate").with_count(1),
        })
        .unwrap();
        let (p, n) = counts(&w);
        assert!(p == 0 || n == 0);
        signs.push(p > 0);
    }
    assert_eq!(signs, [true, false, true, false]);
    // Binary rules ignore the sign.
    let mut w = world("square", 120, 120, "B3/S23", empty, 5);
    w.apply(Command::Reseed {
        ca: None,
        mode: blobs("neg"),
    })
    .unwrap();
    assert!(cells(&w).iter().all(|&c| c <= 1));
    assert!(cells(&w).contains(&1));
}

#[test]
fn reseed_sign_keeps_binary_worlds_unchanged() {
    // The sign draws nothing from the world RNG: a Life world reseeds identically whatever the
    // sign says.
    let mut hashes = vec![];
    for sign in ["pos", "neg", "mixed", "either", "alternate"] {
        let mut w = world(
            "hex",
            80,
            80,
            "B2/S34",
            SeedSpec::Random { density: 0.3 },
            3,
        );
        for m in ["rain", "blobs", "random"] {
            reseed(&mut w, &format!("{m}:{sign}"));
            w.apply(Command::Step { gens: 5 }).unwrap();
        }
        hashes.push(w.hash_hex());
    }
    assert!(hashes.windows(2).all(|p| p[0] == p[1]));
}

#[test]
fn reseed_policies_alternate_on_jaca() {
    let mut w = world(
        "square",
        100,
        100,
        "JACA B3/S23",
        SeedSpec::Random { density: 0.0 },
        4,
    );
    let p: jetcarp_core::reseed::AutoReseed =
        serde_json::from_str(r#"{"mode":"blobs:alternate","when":{"kind":"every","gens":10}}"#)
            .unwrap();
    assert_eq!(p.mode.sign(), jetcarp_core::reseed::Sign::Alternate);
    w.apply(Command::SetReseed { policies: vec![p] }).unwrap();
    w.apply(Command::Step { gens: 10 }).unwrap();
    let (p1, n1) = counts(&w);
    assert!(p1 > 0 && n1 == 0, "first firing +1: {p1} {n1}");
    w.apply(Command::Clear { id: w.cas[0].id }).unwrap();
    w.apply(Command::Step { gens: 10 }).unwrap();
    let (p2, n2) = counts(&w);
    assert!(n2 > 0 && p2 == 0, "second firing -1: {p2} {n2}");
}

#[test]
fn presets_load_and_start_with_both_signs() {
    let presets = jetcarp_core::quickstart::jaca_presets();
    assert!(presets.len() >= 20, "{} presets", presets.len());
    let mut ids = std::collections::HashSet::new();
    for p in &presets {
        assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
        assert_eq!(p.family, "JACA");
        let ca = &p.scene.cas[0];
        assert!(
            matches!(Rule::parse(&ca.rule), Ok((Rule::Jaca(_), _))),
            "{}: {}",
            p.id,
            ca.rule
        );
        assert!(!p.scene.name.is_empty() && !p.scene.description.is_empty());
    }
    // A few, started for real: both signs present at the start and after 50 generations.
    for p in presets.iter().step_by(9) {
        let mut w = World::from_scene(&p.scene).unwrap();
        let (a, b) = counts(&w);
        assert!(a > 0 && b > 0, "{}: {a} {b}", p.id);
        w.apply(Command::Step { gens: 50 }).unwrap();
        let (a, b) = counts(&w);
        assert!(a > 0 && b > 0, "{} after 50: {a} {b}", p.id);
    }
}
