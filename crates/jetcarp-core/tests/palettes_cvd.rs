//! The palettes chosen for colour-vision deficiency, checked (#217), and how every other
//! built-in palette fares in the same check.
//!
//! Each stop is seen as with normal vision and as simulated for protanopia, deuteranopia and
//! tritanopia (Machado, Oliveira and Fernandes 2009, severity 1) and their milder forms
//! (severity 0.5), then compared in CIELAB with CIEDE2000 (`jetcarp_core::cvd`).
//!
//! The distance asked for is [`cvd::MIN_DIFFERENCE`], 10. CIEDE2000 is scaled so that about 1 is
//! just noticeable between two large patches side by side. Cells are small and the colours to
//! tell apart are scattered, not side by side, which needs differences several times larger
//! (Szafir 2018), and the simulation models an average observer. Ten just-noticeable differences
//! leave that margin, and a five-stop ramp on a dark background can still reach it under the
//! strongest deficiencies, where lightness has to carry it alone.

use jetcarp_core::cvd::{self, CHOSEN, MIN_DIFFERENCE, Vision};
use jetcarp_core::render::{GLOW, PALETTES, Palette};

fn stops(name: &str) -> (Vec<[u8; 3]>, [u8; 3]) {
    let p = Palette::builtin(name).unwrap_or_else(|| panic!("{name} is built in"));
    let bg = p.background_rgba();
    (p.stops, [bg[0], bg[1], bg[2]])
}

/// The smallest step in CIELAB lightness the chosen palettes keep from stop to stop.
const LIGHTNESS_STEP: f64 = 12.0;

#[test]
fn the_stated_distance_is_ten() {
    // The justification above is for this number; change both together.
    assert_eq!(MIN_DIFFERENCE, 10.0);
}

#[test]
fn the_chosen_palettes_pass_for_every_kind_of_deficiency() {
    for name in [CHOSEN.palette, CHOSEN.jaca_pos, CHOSEN.jaca_neg] {
        let (s, bg) = stops(name);
        let c = cvd::check(&s, bg);
        assert!(c.passes(), "{name}: {}", c.failure().unwrap_or_default());
        assert!(
            cvd::lightness_steady(&s, LIGHTNESS_STEP),
            "{name}: lightness should fall steadily along the ramp"
        );
        // Every kind was looked at: the check's worst cases come from simulated vision, not
        // only normal vision.
        for v in cvd::VISIONS {
            for w in s.windows(2) {
                assert!(
                    cvd::difference(w[0], w[1], v) >= MIN_DIFFERENCE,
                    "{name} {v:?}"
                );
            }
        }
    }
}

#[test]
fn jaca_s_two_sides_stay_apart_at_every_age_and_from_the_glow() {
    assert_eq!(
        cvd::JACA_PAIRS[0],
        (CHOSEN.jaca_pos, CHOSEN.jaca_neg),
        "the chosen pair comes first"
    );
    for &(pos_name, neg_name) in cvd::JACA_PAIRS {
        let (pos, bg_pos) = stops(pos_name);
        let (neg, bg_neg) = stops(neg_name);
        // One backdrop whichever side's palette sets it.
        assert_eq!(bg_pos, bg_neg, "{pos_name} and {neg_name}");
        // Each side passes on its own.
        for (name, side, bg) in [(pos_name, &pos, bg_pos), (neg_name, &neg, bg_neg)] {
            let c = cvd::check(side, bg);
            assert!(c.passes(), "{name}: {:?}", c.failure());
        }
        // Cells of either sign, of any ages, meet: every stop of one side against every stop of
        // the other.
        let w = cvd::closest_between(&pos, &neg);
        assert!(
            w.difference >= MIN_DIFFERENCE,
            "{pos_name} stop {} and {neg_name} stop {} under {}: {:.1}",
            w.stops.0 + 1,
            w.stops.1 + 1,
            w.vision.name(),
            w.difference
        );
        // Annihilated cells glow pale yellow, fading; at its brightest the glow is told from both.
        let glow = [GLOW[0], GLOW[1], GLOW[2]];
        for (name, side) in [(pos_name, &pos), (neg_name, &neg)] {
            let w = cvd::closest_between(side, &[glow]);
            assert!(
                w.difference >= MIN_DIFFERENCE,
                "{name} stop {} and the glow under {}: {:.1}",
                w.stops.0 + 1,
                w.vision.name(),
                w.difference
            );
        }
    }
}

#[test]
fn the_former_jaca_defaults_are_why_there_is_a_new_pair() {
    // Tide and Koi stay apart for normal vision, but under deuteranopia some of their stops come
    // within a couple of units of each other.
    let (tide, _) = stops("Tide");
    let (koi, _) = stops("Koi");
    let w = cvd::closest_between(&tide, &koi);
    assert!(w.difference < MIN_DIFFERENCE / 2.0, "{w:?}");
    assert_ne!(w.vision, Vision::Normal);
}

/// The built-in palettes that pass, in the table's order. The book's palettes reference marks
/// them (its table comes from `jetcarp palettes --json`, which runs the same check); a change here
/// is a change there.
const PASSING: &[&str] = &[
    "Tide",
    "Phosphor",
    "Koi",
    "Verdigris",
    "Lantern",
    "Cobalt",
    "Saffron",
    "Dusk",
    "Furnace",
    "Kelp",
    "Abyss",
    "Rosewood",
    "Harbour",
    "Lagoon",
    "Heather",
    "Graphite",
    "Honey",
    "Spruce",
    "Fjord",
    "Copper",
    "Peony",
    "Sulphur",
    "Frost",
    "Sepia",
    "Midnight Garden",
    "Lava Flow",
    "Nebula",
    "Vellum",
    "Linen",
    "Ink Wash",
    "Bluebell",
    "Fern",
    "Brick",
    "Mulberry",
    "Driftwood",
    "Tidewheel",
    "Compass Rose",
    "Indigo",
    "Marmalade",
    "Sky",
    "Rust",
    "Iris",
    "Gold",
    "Lapis",
    "Amber",
    "Ice",
    "Ember Red",
    "Delft",
    "Ochre",
];

#[test]
fn how_every_built_in_palette_fares() {
    let mut passing = vec![];
    eprintln!("palette        least  what holds it back");
    for b in PALETTES {
        let (s, bg) = stops(b.name);
        let c = cvd::check(&s, bg);
        eprintln!(
            "{:14} {:5.1}  {}",
            b.name,
            c.least(),
            c.failure().unwrap_or_else(|| "passes".into())
        );
        if c.passes() {
            passing.push(b.name);
        }
    }
    assert_eq!(passing, PASSING);
}
