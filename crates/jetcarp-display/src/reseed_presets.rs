//! Built-in reseed presets, sized to a grid, and the small helpers they share with the policy
//! editor. No UI here: a profile or the host can use them directly.

use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};

/// The grid size the presets' drop counts are tuned for (200 x 140 squares).
const PRESET_CELLS: f64 = 28_000.0;

/// A built-in preset.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub hint: &'static str,
}

/// Built-in presets, in menu order. Drop counts scale with the grid's cell count.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "screensaver",
        name: "Screensaver",
        hint: "Blobs when the world settles (or after 2,400 generations at the latest), and a light rain every 400 generations: keeps any rule lively without turning it to noise",
    },
    Preset {
        id: "drizzle",
        name: "Gentle drizzle",
        hint: "A few small rain drops every 30 generations",
    },
    Preset {
        id: "continuous",
        name: "Continuous rain",
        hint: "One or two tiny drops every 3 generations",
    },
    Preset {
        id: "storm",
        name: "Storm",
        hint: "Many large drops every 8 generations",
    },
    Preset {
        id: "blobs",
        name: "Blobs when settled",
        hint: "Two large patches at 35% live when the world settles, or after 4,000 generations at the latest (cooldown 150)",
    },
    Preset {
        id: "refresh",
        name: "Full refresh when settled",
        hint: "Flip 25% of all cells when the world settles (cooldown 300)",
    },
];

/// Drops for a grid of `cells` cells, `base` at the preset size (at least 1).
fn drops(base: f64, cells: usize) -> u32 {
    (base * cells as f64 / PRESET_CELLS)
        .round()
        .clamp(1.0, 1_000_000.0) as u32
}

fn rain(drops: u32, min_size: f64, max_size: f64, density: f64) -> ReseedMode {
    ReseedMode::Rain {
        drops,
        min_size,
        max_size,
        density,
        sign: Default::default(),
    }
}

pub fn blobs_mode() -> ReseedMode {
    ReseedMode::Blobs {
        count: 2,
        min_radius: 0.06,
        max_radius: 0.16,
        live: 0.35,
        sign: Default::default(),
    }
}

/// A built-in preset's policies for a grid of `cells` cells.
pub fn preset_policies(id: &str, cells: usize) -> Option<Vec<AutoReseed>> {
    let every = |gens| ReseedWhen::Every { gens };
    Some(match id {
        "screensaver" => vec![
            AutoReseed::new(blobs_mode(), settled_or_after(2400)).cooldown(150),
            AutoReseed::new(rain(drops(3.0, cells), 2.0, 8.0, 0.45), every(400)),
        ],
        "drizzle" => vec![AutoReseed::new(
            rain(drops(2.0, cells), 2.0, 6.0, 0.45),
            every(30),
        )],
        "continuous" => vec![AutoReseed::new(
            rain(drops(0.4, cells), 1.5, 4.0, 0.45),
            every(3),
        )],
        "storm" => vec![AutoReseed::new(
            rain(drops(4.0, cells), 3.0, 12.0, 0.5),
            every(8),
        )],
        "blobs" => vec![AutoReseed::new(blobs_mode(), settled_or_after(4000)).cooldown(150)],
        "refresh" => vec![
            AutoReseed::new(
                ReseedMode::Random {
                    flip: 0.25,
                    sign: Default::default(),
                },
                ReseedWhen::settled(),
            )
            .cooldown(300),
        ],
        _ => return None,
    })
}

/// Settled with the default measures, or at the latest after `gens` generations.
fn settled_or_after(gens: u64) -> ReseedWhen {
    let mut w = ReseedWhen::settled();
    if let ReseedWhen::Settled { latest, .. } = &mut w {
        *latest = Some(gens);
    }
    w
}

/// Check every draft; the first error names its policy.
pub fn validate_all(drafts: &[AutoReseed]) -> Result<(), String> {
    for (k, p) in drafts.iter().enumerate() {
        p.validate().map_err(|e| format!("policy {}: {e}", k + 1))?;
    }
    Ok(())
}

/// "0.25%" with sensible precision.
pub fn pct(f: f64) -> String {
    let p = f * 100.0;
    if p == 0.0 {
        "0%".into()
    } else if p < 0.01 {
        "<0.01%".into()
    } else if p < 1.0 {
        format!("{p:.2}%")
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}
