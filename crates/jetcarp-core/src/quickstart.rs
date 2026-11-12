//! Quick Start: the bundled example scenes plus a working example of every named rule, each on
//! the grid it is usually shown on. The app shows these with live previews; `jetcarp quickstart`
//! lists them, surveys starting patterns, and renders contact sheets, and the tests run them all.

use crate::rules::{CatalogEntry, FieldKind, Rule, catalog};
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec};
use crate::world::{CaSpec, Scene, SeedSpec};

/// Hand-made scenes, listed first and always featured.
pub static BUNDLED: &[(&str, &str)] = &[
    ("life", include_str!("../data/scenes/life.json")),
    (
        "rule30-life-ants",
        include_str!("../data/scenes/rule30-life-ants.json"),
    ),
    (
        "script-five",
        include_str!("../data/scenes/script-five.json"),
    ),
    (
        "hex-wolfram",
        include_str!("../data/scenes/hex-wolfram.json"),
    ),
    (
        "sandpile-identity-cairo",
        include_str!("../data/scenes/sandpile-identity-cairo.json"),
    ),
    (
        "b2s34-lizard",
        include_str!("../data/scenes/b2s34-lizard.json"),
    ),
    (
        "brians-brain",
        include_str!("../data/scenes/brians-brain.json"),
    ),
    (
        "langtons-ant",
        include_str!("../data/scenes/langtons-ant.json"),
    ),
    (
        "turmite-llrr",
        include_str!("../data/scenes/turmite-llrr.json"),
    ),
    ("wireworld", include_str!("../data/scenes/wireworld.json")),
    (
        "sandpile-hex",
        include_str!("../data/scenes/sandpile-hex.json"),
    ),
    (
        "sandpile-cairo",
        include_str!("../data/scenes/sandpile-cairo.json"),
    ),
    (
        "sandpile-spectre",
        include_str!("../data/scenes/sandpile-spectre.json"),
    ),
    (
        "spectre-glider",
        include_str!("../data/scenes/spectre-glider.json"),
    ),
    (
        "life-triangle",
        include_str!("../data/scenes/life-triangle.json"),
    ),
    (
        "layered-spectre",
        include_str!("../data/scenes/layered-spectre.json"),
    ),
    (
        "vonneumann-growing-channels",
        include_str!("../data/scenes/vonneumann-growing-channels.json"),
    ),
    ("hat-b2s34", include_str!("../data/scenes/hat-b2s34.json")),
    (
        "penrose-kd-sandpile",
        include_str!("../data/scenes/penrose-kd-sandpile.json"),
    ),
    (
        "penrose-rhomb-brain",
        include_str!("../data/scenes/penrose-rhomb-brain.json"),
    ),
    (
        "vonneumann-construct-withdraw",
        include_str!("../data/scenes/vonneumann-construct-withdraw.json"),
    ),
];

/// The built-in entries picked for the starter set, by id: every built-in kept in the curation
/// of 2026-11-11 (Jimmie Rodgers's keeps among the hand-made scenes, the named rules, the tiling
/// showcases and the JACA presets, plus the Spectre glider, kept among the top finds). These are
/// featured, as the hand-made scenes ([`BUNDLED`], [`COMPUTING_SCENES`]) always are; built-ins
/// never travel in a pack, so this list is how the starter set reaches them. A later curation
/// edits only this list. `tests/phase2_quickstart.rs` checks that each names an entry.
pub static FEATURED: &[&str] = &[
    // Hand-made scenes (featured anyway).
    "life",
    "rule30-life-ants",
    "script-five",
    "hex-wolfram",
    "sandpile-identity-cairo",
    "brians-brain",
    "langtons-ant",
    "turmite-llrr",
    "wireworld",
    "sandpile-hex",
    "spectre-glider",
    "life-triangle",
    "layered-spectre",
    "penrose-kd-sandpile",
    "penrose-rhomb-brain",
    "wireworld-adder",
    "life-and",
    // Named rules.
    "rule:B36/S23",
    "rule:B3678/S34678",
    "rule:B1357/S1357",
    "rule:B3/S12345",
    "rule:B2/S345/C4",
    "rule:R5,C2,S33-57,B34-45",
    "rule:Cyclic 16/1",
    "rule:ForestFire 0.01/0.0001",
    "rule:Hodgepodge 200/3/3/28",
    "rule:W110",
    "rule:HexT 90",
    "rule:Lenia R=13,T=10,b=1,m=0.15,s=0.015",
    "rule:Table parity",
    "rule:Layer 3/012",
    "rule:JACA B3/S23",
    // Tiling showcases.
    "tiling:quad",
    // JACA presets.
    "jaca:trihex-streaks",
    "jaca:trunchex-worms",
    "jaca:ih28-bands",
    "jaca:k2-curly-trenches",
    "jaca:k2-ropes",
    "jaca:square-beads",
    "jaca:square-rainbow",
    "jaca:penrose-kd-rainbow",
    "jaca:trihex-rainbow",
    "jaca:snubhex-rainbow",
    "jaca:trunchex-twins",
    "jaca:k2-rope-network",
    "jaca:snubsq-starry",
    "jaca:square-plasma",
    "jaca:penrose-rainbow-vs-sparkle",
    "jaca:square-pink-plasma",
    "jaca:k2-halves-bubbles",
    "jaca:trunctrihex-halves",
    "jaca:trunchex-halves-worms",
    "jaca:square-anneal-rainbow",
    "jaca:rhombitrihex-shards",
    "jaca:oct-city",
    "jaca:square-bubbles",
    "jaca:square-rainbow-bubbles",
    "jaca:trihex-dark-islands",
    "jaca:square-circuit",
    "jaca:triangle-edge-rainbow",
    "jaca:square-cracks",
    "jaca:penrose-pink-bands",
    "jaca:square-wavy-lines",
];

/// Family of the computing demos.
pub const COMPUTING: &str = "Computing";

/// Computing demos (`computing.rs`), saved by `jetcarp demo ... --save`; shown featured under the
/// family "Computing". `tests/computing.rs` checks each file equals the generator's output.
pub static COMPUTING_SCENES: &[(&str, &str)] = &[
    (
        "wireworld-adder",
        include_str!("../data/scenes/wireworld-adder.json"),
    ),
    ("life-not", include_str!("../data/scenes/life-not.json")),
    ("life-and", include_str!("../data/scenes/life-and.json")),
    ("life-or", include_str!("../data/scenes/life-or.json")),
];

#[derive(Clone, Debug)]
pub struct QuickStart {
    /// Stable key (bundled scene name, or `rule:<notation>`).
    pub id: String,
    pub scene: Scene,
    /// Rule family, for sorting and filtering.
    pub family: String,
    /// Tiling display name.
    pub tiling: String,
    /// States in the main CA's field (sandpiles: 0).
    pub states: u32,
    /// A hand-made scene, or one picked for the starter set ([`FEATURED`]).
    pub featured: bool,
    /// Notation of the main CA.
    pub rule: String,
    /// Where the rule's name and notation were checked, when known.
    pub source: Option<String>,
}

fn rect(family: &str, w: u32, h: u32, b: Boundary) -> TilingSpec {
    TilingSpec::rect(family, w, h, b)
}

/// Starting patterns chosen with `jetcarp quickstart --survey` and a look at the contact sheet:
/// explosive rules start from a small blob so the growth shows; others from a soup.
fn start_hint(notation: &str) -> Option<SeedSpec> {
    let soup = |d: f64| SeedSpec::Random { density: d };
    let blob = |r: f64, d: f64| SeedSpec::Blob {
        radius: r,
        density: d,
    };
    Some(match notation {
        // Grow from a small seed rather than fill the screen with noise at once.
        "B1/S1" | "B1/S0V" | "B1/S012345678" | "B1/S014567" | "B1/S134567" | "B2/S" | "B2/S0"
        | "B234/S" | "B1357/S1357" | "B1357/S02468" | "B13/S13V" | "B13/S024V" | "B1358/S0247"
        | "B12678/S15678" => blob(0.08, 0.5),
        "B3/S012345678" | "B37/S012345678" | "B38/S012345678" | "B368/S012345678"
        | "B378/S012345678" | "B3/S12345" | "B3/S1234" | "B37/S12345" | "B37/S1234"
        | "B45/S12345" | "B3/S245678" | "B3/S123678" | "B3/S124" => blob(0.15, 0.4),
        // LifeWiki: Lines wants a circle of radius 40 or more at 20%; SediMental a 30% soup.
        "B458/S012345/C3" => blob(0.8, 0.2),
        "B25678/S45678/C4" => soup(0.3),
        // Chosen with `jetcarp quickstart --survey` (300 generations at preview size).
        "B3/S45678" | "R5,C2,S33-57,B34-45" => soup(0.3),
        "B5678/S45678"
        | "B45678/S5678"
        | "B4678/S35678"
        | "B35678/S4678"
        | "B35678/S5678"
        | "R4,C2,S40-80,B41-80"
        | "R7,C2,S112-224,B113-224"
        | "R7,C2,S99-199,B75-170"
        | "R8,C2,S163-223,B74-252" => soup(0.5),
        "B56/S14568" => soup(0.45),
        // These fill a soup solid at once; a blob shows how they grow.
        "B3678/S235678" | "B34568/S15678" | "B3678/S135678" => blob(0.2, 0.4),
        "B345/S4567" | "R3,C2,S2,B3,N+" => soup(0.2),
        "B23/S356/C6" => soup(0.1),
        "R10,C2,S122-211,B123-170" => soup(0.4),
        "R10,C255,S1-2,B3" => soup(0.01),
        // A mixed Life soup blows itself away with blast 2; halves show the blasts at the front.
        "JACA B3/S23 blast 2" => SeedSpec::Halves { density: 0.3 },
        _ => return None,
    })
}

/// Rule families whose random soups do something only at a particular density.
fn soup_density(rule: &Rule) -> f64 {
    match rule {
        Rule::Ltl(_) => 0.45,
        Rule::Generations { .. } => 0.3,
        _ => 0.35,
    }
}

/// The scene shown for a named rule: its usual grid and a start that shows it working.
pub fn scene_for(e: &CatalogEntry) -> Result<Scene, String> {
    let (rule, nb) = Rule::parse(e.notation)?;
    let hex = e.notation.trim().to_ascii_uppercase().ends_with('H');
    let mut ca = CaSpec::new(e.notation);
    let hint = start_hint(e.notation);
    if rule == Rule::VonNeumann {
        // Its ground state does nothing on its own; show the hand-built demo instead.
        let mut sc = Scene::from_json(include_str!(
            "../data/scenes/vonneumann-growing-channels.json"
        ))?;
        sc.name = e.name.to_string();
        return Ok(sc);
    }
    let tiling = match &rule {
        Rule::LifeLike { .. } | Rule::Generations { .. } => {
            ca.seed = Some(hint.unwrap_or(SeedSpec::Random {
                density: soup_density(&rule),
            }));
            if hex {
                rect("hex", 150, 110, Boundary::Wrap)
            } else {
                if nb == Some(Nb::Edge) {
                    ca.neighborhood = Some(Nb::Edge);
                }
                rect("square", 180, 120, Boundary::Wrap)
            }
        }
        Rule::Jaca(_) => {
            ca.seed = Some(hint.unwrap_or(SeedSpec::Signed {
                density: 0.3,
                negative: 0.5,
            }));
            ca.palette = Some("Tide".into());
            ca.palette_neg = Some("Ember".into());
            rect("square", 180, 120, Boundary::Wrap)
        }
        Rule::Ltl(l) => {
            ca.seed = Some(hint.unwrap_or(SeedSpec::Random {
                density: soup_density(&rule),
            }));
            let s = (l.range as u32 * 24).clamp(160, 260);
            rect("square", s * 3 / 2, s, Boundary::Wrap)
        }
        Rule::Wolfram(n) => {
            // Particle and wall rules show their behaviour from a random row.
            ca.seed = Some(if matches!(n, 184 | 54 | 73) {
                SeedSpec::TopRandom { density: 0.5 }
            } else {
                SeedSpec::TopCenter
            });
            ca.palette = Some("Tide".into());
            rect("square", 241, 160, Boundary::Dead)
        }
        Rule::HexWolfram { .. } => {
            ca.palette = Some("Prism".into());
            rect("hex", 161, 81, Boundary::Dead)
        }
        Rule::HexTri { .. } => {
            ca.palette = Some("Prism".into());
            // 2 x 80 columns; the light cone spreads two columns per row and reaches the sides.
            rect("hex_flat", 80, 40, Boundary::Dead)
        }
        Rule::Turmite(t) => {
            ca.steps_per_gen = if t.colors <= 2 { 100 } else { 200 };
            ca.palette = Some("Prism".into());
            rect("square", 160, 160, Boundary::Wrap)
        }
        Rule::WireWorld => rect("square", 120, 60, Boundary::Dead),
        Rule::Sandpile => {
            ca.steps_per_gen = 20;
            ca.seed = Some(SeedSpec::Pile { grains: 30000 });
            rect("square", 140, 140, Boundary::Dead)
        }
        Rule::SpectreGlider => TilingSpec::new("spectre").with_size(SizeSpec::Level(5)),
        Rule::Priority(_) => TilingSpec::new("spectre").with_size(SizeSpec::Level(5)),
        Rule::Table(_) => {
            ca.seed = Some(SeedSpec::Center);
            rect("square", 161, 161, Boundary::Wrap)
        }
        Rule::ForestFire { .. } => rect("square", 200, 140, Boundary::Wrap),
        Rule::Cyclic { .. } | Rule::Rps { .. } | Rule::GreenbergHastings { .. } => {
            ca.palette = Some("Prism".into());
            rect("square", 180, 120, Boundary::Wrap)
        }
        Rule::Hodgepodge { .. } => {
            ca.palette = Some("Tide".into());
            rect("square", 180, 120, Boundary::Wrap)
        }
        Rule::VonNeumann => unreachable!(),
        Rule::Lenia(_) => {
            ca.palette = Some("Ember".into());
            ca.seed = Some(if e.name.starts_with("Orbium") {
                SeedSpec::Lenia {
                    name: "orbium".into(),
                    scale: 1.0,
                }
            } else {
                SeedSpec::Blob {
                    radius: 0.3,
                    density: 0.5,
                }
            });
            rect("square", 128, 96, Boundary::Wrap)
        }
        Rule::Margolus(_) => {
            ca.palette = Some("Tide".into());
            // Even sides, so the wrapped grid splits into whole blocks.
            rect("square", 160, 120, Boundary::Wrap)
        }
        Rule::Layered { .. } => {
            ca.palette = Some("Prism".into());
            ca.color_by = crate::render::ColorBy::State;
            TilingSpec::new("cairo")
                .with_size(SizeSpec::Cells(12000))
                .with_boundary(Boundary::Dead)
        }
    };
    if ca.palette.is_none() {
        ca.palette = Some(
            match e.family {
                f if f.starts_with("Generations") => "Prism",
                f if f.starts_with("Larger") => "Tide",
                f if f.contains("B0") => "Ink",
                _ => "Ember",
            }
            .into(),
        );
    }
    let mut sc = Scene::new(tiling);
    sc.name = e.name.to_string();
    sc.description = if e.blurb.is_empty() {
        format!("{} rule {}.", e.family, e.notation)
    } else {
        e.blurb.to_string()
    };
    // A stable, rule-specific RNG state so every example is reproducible.
    let mut h = crate::rng::Fnv::default();
    h.bytes(e.notation.as_bytes());
    sc.rng_state = h.0 | 1;
    sc.cas = vec![ca];
    Ok(sc)
}

/// Display name of a tiling's family. The unreshaped square grid is "Square" (its family, which
/// also holds every parallelogram, is "Square (parallelogram)").
pub(crate) fn tiling_name(t: &TilingSpec) -> String {
    if t.family == "square"
        && t.params
            .as_ref()
            .is_none_or(|p| p.as_object().is_none_or(|o| o.is_empty()))
    {
        return "Square".into();
    }
    crate::families::family(&t.family)
        .or_else(|| crate::isohedral::family(&t.family))
        .map(|f| f.name.to_string())
        .unwrap_or_else(|| t.family.clone())
}

pub fn main_rule(sc: &Scene) -> (String, u32, String) {
    let Some(ca) = sc.cas.first() else {
        return (String::new(), 0, "Other".into());
    };
    let (rule, _) = match Rule::parse(&ca.rule) {
        Ok(r) => r,
        Err(_) => return (ca.rule.clone(), 0, "Other".into()),
    };
    let states = match rule.field_kind() {
        FieldKind::Binary => 2,
        FieldKind::Multi(n) => n as u32,
        FieldKind::Height => 0,
        FieldKind::Continuous => 256,
    };
    let family = catalog()
        .iter()
        .find(|e| e.notation == rule.notation())
        .map(|e| e.family.to_string())
        .unwrap_or_else(|| family_of(&rule).into());
    (ca.rule.clone(), states, family)
}

fn family_of(rule: &Rule) -> &'static str {
    match rule {
        Rule::LifeLike { .. } => "Life-like",
        Rule::Generations { .. } => "Generations",
        Rule::Wolfram(_) => "Elementary",
        Rule::HexWolfram { .. } | Rule::HexTri { .. } => "Hexagonal elementary",
        Rule::Turmite(_) => "Agents",
        Rule::WireWorld => "Wire",
        Rule::Sandpile => "Sandpile",
        Rule::SpectreGlider | Rule::Table(_) => "Table",
        Rule::Ltl(_) => "Larger than Life",
        Rule::Layered { .. } => "Layered",
        Rule::VonNeumann => "Self-reproduction",
        Rule::Margolus(_) => "Margolus block",
        Rule::Lenia(_) => "Continuous",
        Rule::Jaca(_) => "JACA",
        _ => "Multi-state",
    }
}

/// Tiling showcases: per tiling, a rule that stays lively there, chosen with
/// `jetcarp quickstart --survey-tilings` (Life where it survives, otherwise the first candidate that
/// stays active), plus a sandpile on every tiling.
pub static SHOWCASES: &[(&str, &str)] = &[
    ("lizard", "B35/S234"),
    ("hex", "B35/S234"),
    ("brick", "B35/S234"),
    ("quad", "B3/S23"),
    ("triangle", "B2/S/C3"),
    ("glide", "B3/S23"),
    ("fourfold", "B3/S23"),
    ("cairo", "B2/S34"),
    ("spectre", "B2/S23V"),
    ("hat", "B35/S234"),
    ("penrose-kd", "B2/S/C3"),
    ("penrose-rhomb", "B2/S34"),
    ("oct", "B2/S23V"),
    ("trihex", "B2/S34"),
    ("elongtri", "B2/S/C3"),
    ("snubsq", "B3/S23"),
    ("snubhex", "B2/S/C3"),
    ("rhombitrihex", "B2/S34"),
    ("trunchex", "B2/S23V"),
    ("trunctrihex", "B2/S23V"),
    ("herring", "B35/S234"),
    ("pyth", "B2/S23V"),
    ("ih01", "B35/S234"),
    ("ih07", "B35/S234"),
    ("ih21", "B2/S34"),
    ("ih28", "B2/S34"),
    ("ih41", "B3/S23"),
    ("ih55", "B36/S23"),
    ("ih74", "B3/S23"),
    ("ih81", "B3/S23"),
    ("ih90", "B2/S/C3"),
    // k-uniform tilings from the catalog (`catalog:<id>` builds the periodic family).
    ("catalog:k2-001", "B2/S34"),
    ("catalog:k2-002", "B2/S34"),
    ("catalog:k3-011", "B2/S34"),
    ("catalog:k4-100", "B2/S34"),
    ("catalog:k5-058", "B2/S34"),
    ("catalog:k6-673", "B2/S34"),
];

/// The tiling spec and display name for a showcase key (family id or `catalog:<id>`).
fn showcase_tiling(key: &str) -> Option<(TilingSpec, String, String)> {
    if let Some(id) = key.strip_prefix("catalog:") {
        let t = crate::catalog::get(id)?;
        let mut spec = TilingSpec::new("periodic");
        spec.params = Some(serde_json::json!({ "catalog": id }));
        let note = format!(
            "A {}-uniform tiling from Galebach's catalog ({}). {}",
            t.k, t.id, t.note
        );
        return Some((spec, format!("{} ({})", t.name, t.id), note));
    }
    let f = crate::families::family(key).or_else(|| crate::isohedral::family(key))?;
    Some((TilingSpec::new(key), f.name.to_string(), f.note.to_string()))
}

fn showcase_size(fam: &str) -> SizeSpec {
    match crate::families::family(fam) {
        Some(f) if f.kind == crate::families::FamilyKind::Aperiodic => {
            SizeSpec::Level(crate::aperiodic::default_level(fam) + 1)
        }
        _ => SizeSpec::Cells(3000),
    }
}

fn showcases() -> Vec<QuickStart> {
    let mut out = Vec::new();
    let mut sand_done: Vec<&str> = vec!["hex", "cairo", "spectre", "square", "penrose-kd"];
    for (k, &(fam, rule)) in SHOWCASES.iter().enumerate() {
        let Some((base, tname, note)) = showcase_tiling(fam) else {
            continue;
        };
        let Ok((r, nb)) = Rule::parse(rule) else {
            continue;
        };
        let tiling = base
            .clone()
            .with_size(showcase_size(fam))
            .with_boundary(Boundary::Wrap);
        let mut sc = Scene::new(tiling.clone());
        let rname = crate::rules::catalog_name(&r, nb.unwrap_or(Nb::Touch))
            .map(String::from)
            .unwrap_or_else(|| rule.to_string());
        sc.name = format!("{rname} on {tname}");
        sc.description = format!(
            "{note} The rule was picked by surveying candidates on this tiling for one that stays active."
        );
        sc.rng_state = 100 + k as u64;
        let mut ca = CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 });
        ca.palette = Some(["Prism", "Ember", "Tide"][k % 3].into());
        sc.cas = vec![ca];
        let (rn, states, _) = main_rule(&sc);
        out.push(QuickStart {
            id: format!("tiling:{fam}"),
            tiling: tname.clone(),
            scene: sc,
            family: "Tiling showcase".into(),
            states,
            featured: false,
            rule: rn,
            source: None,
        });
        if !sand_done.contains(&fam) {
            sand_done.push(fam);
            let size = showcase_size(fam);
            let cells = match size {
                SizeSpec::Cells(c) => c as u64,
                _ => 30000,
            };
            let mut sc = Scene::new(base.clone().with_size(size).with_boundary(Boundary::Dead));
            sc.name = format!("Sandpile on {tname}");
            sc.description = format!("{note} Grains dropped on the centre topple outward.");
            sc.rng_state = 1;
            let mut ca = CaSpec::new("Sandpile")
                .seed(SeedSpec::Pile { grains: cells })
                .steps(4);
            ca.palette = Some(["Ember", "Tide", "Prism"][k % 3].into());
            sc.cas = vec![ca];
            out.push(QuickStart {
                id: format!("sandpile:{fam}"),
                tiling: tname.clone(),
                scene: sc,
                family: "Tiling showcase".into(),
                states: 0,
                featured: false,
                rule: "Sandpile".into(),
                source: None,
            });
        }
    }
    out
}

/// All Quick Start entries: the hand-made scenes first, then one per named rule in catalog order,
/// the JACA presets and the tiling showcases. The hand-made scenes and those in [`FEATURED`] are
/// featured.
pub fn entries() -> Vec<QuickStart> {
    let mut out = Vec::new();
    for (id, json) in BUNDLED {
        if let Ok(scene) = Scene::from_json(json) {
            let (rule, states, family) = main_rule(&scene);
            out.push(QuickStart {
                id: id.to_string(),
                tiling: tiling_name(&scene.tiling),
                scene,
                family,
                states,
                featured: true,
                rule,
                source: None,
            });
        }
    }
    for (id, json) in COMPUTING_SCENES {
        if let Ok(scene) = Scene::from_json(json) {
            let (rule, states, _) = main_rule(&scene);
            out.push(QuickStart {
                id: id.to_string(),
                tiling: tiling_name(&scene.tiling),
                scene,
                family: COMPUTING.into(),
                states,
                featured: true,
                rule,
                source: None,
            });
        }
    }
    for e in catalog() {
        let Ok(scene) = scene_for(e) else { continue };
        let (rule, states, _) = main_rule(&scene);
        out.push(QuickStart {
            id: format!("rule:{}", e.notation),
            tiling: tiling_name(&scene.tiling),
            scene,
            family: e.family.to_string(),
            states,
            featured: false,
            rule,
            source: crate::rules::source_of(e.notation).map(String::from),
        });
    }
    out.extend(jaca_presets());
    out.extend(showcases());
    for q in &mut out {
        q.featured |= FEATURED.contains(&q.id.as_str());
    }
    out
}

/// JACA presets: whole scenes (tiling, neighbourhood, rule pair, blast, start) chosen with the
/// `jaca_search` example and a look at each, in `scenes/presets/jaca.json` (a list of
/// `{"id": ..., "scene": {...}}`). Ids `jaca:<id>`, family "JACA".
pub static JACA_PRESETS: &str = include_str!("../data/scenes/presets/jaca.json");

pub fn jaca_presets() -> Vec<QuickStart> {
    #[derive(serde::Deserialize)]
    struct Preset {
        id: String,
        scene: Scene,
    }
    let list: Vec<Preset> = match serde_json::from_str(JACA_PRESETS) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("scenes/presets/jaca.json: {e}");
            return vec![];
        }
    };
    list.into_iter()
        .map(|p| {
            let (rule, states, _) = main_rule(&p.scene);
            QuickStart {
                id: format!("jaca:{}", p.id),
                tiling: tiling_name(&p.scene.tiling),
                scene: p.scene,
                family: "JACA".into(),
                states,
                featured: false,
                rule,
                source: Some("jaca_search (crates/jetcarp-search/examples/jaca_search.rs)".into()),
            }
        })
        .collect()
}

/// Family shown for promoted pattern-library finds.
pub const MY_FINDS: &str = "My finds";

/// Preview size for a scene: `base` cells, more for long-range rules whose structures are large.
/// `None` when the full scene is already small enough.
pub fn preview_cells(s: &Scene, base: u32) -> Option<u32> {
    let range = s
        .cas
        .iter()
        .filter_map(|c| match Rule::parse(&c.rule) {
            Ok((Rule::Ltl(l), _)) => Some(l.range as u32),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    Some(if range <= 2 {
        base
    } else {
        (base * range * range / 4).min(60_000)
    })
}

/// Whether a scene places cells by index (explicit seeds, painted cells, pasted patterns):
/// such scenes only work at their own size.
pub fn places_cells(s: &Scene) -> bool {
    use crate::world::Command;
    s.cas
        .iter()
        .any(|c| matches!(c.seed, Some(SeedSpec::Cells { .. })))
        || s.commands
            .iter()
            .any(|c| matches!(c, Command::SetCells { .. } | Command::PastePattern { .. }))
}

/// A smaller copy of a scene for live previews: about `cells` cells, sandpiles scaled to match.
/// Scenes that place cells by index (see [`places_cells`]) are kept at full size.
pub fn preview_scene(s: &Scene, cells: u32) -> Scene {
    let mut p = s.clone();
    if places_cells(s) {
        return p;
    }
    let mut ratio = 1.0;
    p.tiling.size = match p.tiling.size {
        Some(SizeSpec::Rect { w, h }) => {
            let k = ((w * h) as f64 / cells as f64).sqrt().max(1.0);
            ratio = 1.0 / (k * k);
            Some(SizeSpec::Rect {
                w: ((w as f64 / k) as u32).max(8),
                h: ((h as f64 / k) as u32).max(8),
            })
        }
        Some(SizeSpec::Cells(c)) => {
            ratio = c.min(cells / 2) as f64 / c as f64;
            Some(SizeSpec::Cells(c.min(cells / 2)))
        }
        Some(SizeSpec::Level(l)) => {
            // Largest listed level under the budget (unlisted families keep their level).
            let fam = &p.tiling.family;
            let fit = crate::aperiodic::levels(fam)
                .iter()
                .filter(|x| x.0 <= l && x.1 <= (cells as usize).max(5000))
                .max_by_key(|x| x.0)
                .copied();
            match fit {
                Some((nl, cnt)) => {
                    let full = crate::aperiodic::tile_count(fam, l).unwrap_or(cnt);
                    ratio = cnt as f64 / full as f64;
                    Some(SizeSpec::Level(nl))
                }
                None => Some(SizeSpec::Level(l)),
            }
        }
        None => None,
    };
    for ca in &mut p.cas {
        if let Some(SeedSpec::Pile { grains }) = &mut ca.seed {
            *grains = ((*grains as f64) * ratio) as u64;
        }
    }
    p
}

/// How an example is doing after a run, for the "every example works" check and the survey.
#[derive(Clone, Debug)]
pub struct Health {
    pub generation: u64,
    pub cells: usize,
    pub population: u64,
    /// Fraction of cells that changed in the last generation.
    pub activity: f64,
    pub status: crate::world::Status,
}

impl Health {
    /// Alive at the end, and either still changing, or settled only after a while, or cycling
    /// with a period above 2 (waves). Small blinking leftovers from a dying soup do not count.
    pub fn works(&self) -> bool {
        use crate::world::Status;
        match self.status {
            Status::Empty => false,
            Status::Repeats { period, seen_at } => {
                self.population > 0 && (seen_at >= 20 || period > 2)
            }
            Status::Running => self.population > 0,
        }
    }
}

/// Run a scene for `gens` generations (at preview size when `preview_cells` is given).
pub fn health(scene: &Scene, gens: u64, preview_cells: Option<u32>) -> Result<Health, String> {
    let sc = match preview_cells {
        Some(c) => preview_scene(scene, c),
        None => scene.clone(),
    };
    let mut w = crate::world::World::from_scene(&sc)?;
    let mut before = Vec::new();
    for g in 0..gens {
        if g + 1 == gens {
            before = w
                .fields
                .iter()
                .map(|f| (f.state.clone(), f.height.clone()))
                .collect();
        }
        w.step();
    }
    let mut changed = 0usize;
    for (f, (s, h)) in w.fields.iter().zip(&before) {
        changed += f.state.iter().zip(s).filter(|(a, b)| a != b).count();
        changed += f.height.iter().zip(h).filter(|(a, b)| a != b).count();
    }
    let population = w.fields.iter().map(|f| f.population()).sum::<u64>()
        + w.cas.iter().map(|c| c.agents.len() as u64).sum::<u64>();
    Ok(Health {
        generation: w.generation,
        cells: w.topo.len(),
        population,
        activity: changed as f64 / w.topo.len().max(1) as f64,
        status: w.status(),
    })
}

/// A labelled contact sheet: each scene at preview size after `gens` generations, numbered from
/// `first` in the order given.
pub fn contact_sheet(
    scenes: &[&Scene],
    first: u32,
    gens: u64,
    thumb: u32,
    cols: u32,
) -> crate::render::Image {
    use crate::render::{Image, RenderOptions, STAGE, render};
    use rayon::prelude::*;
    let thumbs: Vec<Image> = scenes
        .par_iter()
        .map(|s| {
            let sc = preview_scene(s, preview_cells(s, 9000).unwrap_or(9000));
            match crate::world::World::from_scene(&sc) {
                Ok(mut w) => {
                    for _ in 0..gens {
                        w.step();
                    }
                    render(
                        &w,
                        &RenderOptions {
                            max_side: thumb,
                            outline_min_px: 0.0,
                            ..Default::default()
                        },
                    )
                }
                Err(_) => Image {
                    width: thumb,
                    height: thumb / 2,
                    rgba: [0xff, 0x30, 0x30, 0xff].repeat((thumb * thumb / 2) as usize),
                },
            }
        })
        .collect();
    let (tw, th) = (thumb, thumb);
    let (label, pad) = (16, 6);
    let cols = cols.max(1).min(scenes.len().max(1) as u32);
    let rows = (scenes.len() as u32).div_ceil(cols);
    let (cw, ch) = (tw + pad, th + label + pad);
    let mut img = Image {
        width: cols * cw + pad,
        height: rows * ch + pad,
        rgba: STAGE
            .iter()
            .copied()
            .cycle()
            .take(((cols * cw + pad) * (rows * ch + pad) * 4) as usize)
            .collect(),
    };
    for (k, t) in thumbs.iter().enumerate() {
        let (cx, cy) = ((k as u32 % cols) * cw + pad, (k as u32 / cols) * ch + pad);
        crate::sheet::draw_number(
            &mut img,
            cx,
            cy + 2,
            (first + k as u32) as u64,
            2,
            [0xd0, 0xd4, 0xdc, 0xff],
        );
        for y in 0..t.height.min(th) {
            let w = t.width.min(tw);
            let src = (y * t.width * 4) as usize;
            let dst = (((cy + label + y) * img.width + cx) * 4) as usize;
            img.rgba[dst..dst + (w * 4) as usize]
                .copy_from_slice(&t.rgba[src..src + (w * 4) as usize]);
        }
    }
    img
}
