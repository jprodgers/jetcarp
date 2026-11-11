//! Rule-semantics pins: one small fixed scene per rule family, run a fixed number of generations,
//! must reach its pinned state.
//!
//! `dynamics_pins` pins what the Quick Start scenes do; this pins what each rule family means,
//! one scene per family of [`RULE_SEMANTICS`] (`jetcarp_core::semantics`), so a change in a
//! family's meaning cannot go unnoticed: the family's version must then go up, and records
//! verified under the old version are flagged for re-verification when a library is opened.
//!
//! - Each scene starts from cells this test chooses with its own random-number generator (or,
//!   for ants, sandpiles, Lenia and the row rules, a fixed start of the engine's), so only the
//!   rule, the tiles and the forest fire's own randomness decide where it goes.
//! - The hash covers what a rule decides: each field's states, grain heights, continuous values
//!   (by their bits) and lost grains, and every ant's cell, heading and state; not ages or
//!   bookkeeping.
//! - `tests/data/semantics_pins.txt` holds, per family, the version it was pinned at and the hash.
//!   `JETCARP_PIN_WRITE=1` writes it again, but refuses a family whose hash changed while its
//!   version did not: raise the version in `RULE_SEMANTICS` first.
use jetcarp_core::semantics::{RULE_SEMANTICS, family};
use jetcarp_core::sha256::{Sha256, hex};
use jetcarp_core::*;
use rayon::prelude::*;
use std::collections::BTreeMap;

/// How a scene starts.
enum Start {
    /// Each cell, with probability `density`, gets a state from `lo` to `hi` (this test's
    /// generator).
    Soup { density: f64, lo: u32, hi: u32 },
    /// One ant on the centre cell, facing its first edge.
    Ant,
    /// A start of the engine's own that uses no randomness, or the row rules' random top row.
    Engine(SeedSpec),
}

/// One family's scene: tiling, rule (in its default neighbourhood), start, generations.
struct Case {
    family: &'static str,
    tiling: TilingSpec,
    rule: &'static str,
    start: Start,
    gens: u64,
}

fn rect(f: &str, w: u32, h: u32, b: Boundary) -> TilingSpec {
    TilingSpec::rect(f, w, h, b)
}

fn soup(density: f64, lo: u32, hi: u32) -> Start {
    Start::Soup { density, lo, hi }
}

fn cases() -> Vec<Case> {
    use Boundary::{Dead, Wrap};
    let c = |family, tiling, rule, start, gens| Case {
        family,
        tiling,
        rule,
        start,
        gens,
    };
    vec![
        c(
            "lifelike",
            rect("square", 32, 32, Wrap),
            "B3/S23",
            soup(0.35, 1, 1),
            48,
        ),
        c(
            "generations",
            rect("square", 32, 32, Wrap),
            "B2/S/C3",
            soup(0.3, 1, 2),
            48,
        ),
        c(
            "ltl",
            rect("square", 48, 48, Wrap),
            "R5,C2,S33-57,B34-45",
            soup(0.5, 1, 1),
            24,
        ),
        c(
            "elementary",
            rect("square", 64, 48, Dead),
            "W110",
            Start::Engine(SeedSpec::TopRandom { density: 0.5 }),
            40,
        ),
        c(
            "hexw",
            rect("hex", 48, 32, Dead),
            "HexW3 1234",
            Start::Engine(SeedSpec::TopRandom { density: 0.5 }),
            28,
        ),
        c(
            "hext",
            rect("hex_flat", 32, 24, Dead),
            "HexT3 sum 777",
            Start::Engine(SeedSpec::TopRandom { density: 0.5 }),
            20,
        ),
        c(
            "layered",
            rect("trihex", 16, 16, Dead),
            "Layer 3/012",
            soup(0.02, 1, 2),
            24,
        ),
        c(
            "jaca",
            rect("square", 32, 32, Wrap),
            "JACA +B3/S23 -B36/S23 blast 1",
            soup(0.35, 1, 2),
            24,
        ),
        c(
            "ant",
            rect("square", 32, 32, Wrap),
            "Turmite LLRR",
            Start::Ant,
            600,
        ),
        c(
            "turmite",
            rect("hex", 24, 24, Wrap),
            "Turmite 1R1,1L0/1R1,0N0",
            Start::Ant,
            600,
        ),
        c(
            "wireworld",
            rect("square", 32, 32, Dead),
            "WireWorld",
            soup(0.5, 1, 3),
            48,
        ),
        c(
            "cyclic",
            rect("square", 32, 32, Wrap),
            "Cyclic 14/1",
            soup(1.0, 0, 13),
            48,
        ),
        c(
            "greenberg_hastings",
            rect("hex", 32, 32, Wrap),
            "GH 8/1",
            soup(0.2, 1, 7),
            48,
        ),
        c(
            "rps",
            rect("square", 32, 32, Wrap),
            "RPS 3/3",
            soup(1.0, 0, 2),
            48,
        ),
        c(
            "forest_fire",
            rect("square", 32, 32, Wrap),
            "ForestFire 0.05/0.001",
            soup(0.5, 1, 2),
            48,
        ),
        c(
            "hodgepodge",
            rect("square", 32, 32, Wrap),
            "Hodgepodge 100/2/3/28",
            soup(1.0, 0, 100),
            48,
        ),
        c(
            "margolus",
            rect("square", 32, 32, Wrap),
            "Margolus Critters",
            soup(0.3, 1, 1),
            48,
        ),
        c(
            "lenia",
            rect("square", 64, 64, Wrap),
            "Lenia R=13,T=10,b=1,m=0.15,s=0.015",
            Start::Engine(SeedSpec::Lenia {
                name: "orbium".into(),
                scale: 1.0,
            }),
            12,
        ),
        c(
            "sandpile",
            rect("square", 31, 31, Dead),
            "Sandpile",
            Start::Engine(SeedSpec::Pile { grains: 3000 }),
            96,
        ),
        c(
            "jvn29",
            rect("square", 32, 32, Dead),
            "JvN29",
            soup(0.3, 1, 28),
            48,
        ),
        c(
            "priority",
            rect("square", 32, 32, Wrap),
            "Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1",
            soup(0.5, 1, 3),
            48,
        ),
        c(
            "spectre_glider",
            TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
            "SpectreGlider",
            soup(0.3, 1, 3),
            48,
        ),
        c(
            "table",
            rect("square", 30, 30, Wrap),
            "Table parity",
            soup(0.3, 1, 1),
            32,
        ),
    ]
}

/// This test's own generator (xorshift64*), seeded from the family's name.
struct Gen(u64);
impl Gen {
    fn new(name: &str) -> Gen {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in name.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
        Gen(h | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// What a rule decides, hashed: states, heights, values, lost grains, and the ants.
fn state_hash(w: &World) -> String {
    let mut h = Sha256::new();
    for f in &w.fields {
        h.text(&f.kind.name());
        h.update(&f.state);
        for x in &f.height {
            h.update(&x.to_le_bytes());
        }
        for x in &f.value {
            h.update(&x.to_bits().to_le_bytes());
        }
        h.update(&f.lost.to_le_bytes());
    }
    for c in &w.cas {
        for a in &c.agents {
            h.update(&a.cell.to_le_bytes());
            h.update(&a.heading.to_le_bytes());
            h.update(&[a.state]);
        }
    }
    hex(&h.finish()[..8])
}

/// Is anything left: a live cell, a grain, a value, or an ant?
fn alive(w: &World) -> bool {
    w.fields.iter().any(|f| {
        f.state.iter().any(|&s| s != 0)
            || f.height.iter().any(|&h| h != 0)
            || f.value.iter().any(|&v| v != 0.0)
    }) || w.cas.iter().any(|c| !c.agents.is_empty())
}

/// Run a case: the state hash at its start and after its generations, and whether anything is
/// left.
fn run(c: &Case) -> Result<(String, String, bool), String> {
    let mut w = World::new(c.tiling.clone(), 1)?;
    let n = w.topo.len();
    let seed = match &c.start {
        Start::Soup { density, lo, hi } => {
            let mut g = Gen::new(c.family);
            let cells = (0..n as u32)
                .filter_map(|i| {
                    let pick = g.unit() < *density;
                    let s = lo + (g.next() % (hi - lo + 1) as u64) as u32;
                    (pick && s > 0).then_some([i, s])
                })
                .collect();
            SeedSpec::Cells { cells }
        }
        Start::Ant => SeedSpec::AntAt {
            cell: w.topo.center_cell() as u32,
            heading: 0,
        },
        Start::Engine(s) => s.clone(),
    };
    let ca = CaSpec::new(c.rule).seed(seed);
    w.apply(Command::AddCa { ca })?;
    let first = state_hash(&w);
    w.apply(Command::Step { gens: c.gens })?;
    Ok((first, state_hash(&w), alive(&w)))
}

fn pins_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/semantics_pins.txt")
}

/// The pinned `(version, hash)` of each family.
fn read_pins() -> BTreeMap<String, (String, String)> {
    std::fs::read_to_string(pins_path())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 3).then(|| (f[0].to_string(), (f[1].to_string(), f[2].to_string())))
        })
        .collect()
}

fn bump(f: &str, v: &str, h: &str, got: &str) -> String {
    format!(
        "{f}: its scene now reaches {got}, but it was pinned at {h} with {f}'s semantics \
         version {v}. The meaning of a {f} rule changed: raise {f}'s version in RULE_SEMANTICS \
         (crates/jetcarp-core/src/semantics.rs), then pin it again with JETCARP_PIN_WRITE=1. \
         (If no rule's meaning changed, only this test's scene or hash, delete the family's \
         line from tests/data/semantics_pins.txt and write it again.)"
    )
}

#[test]
fn every_rule_family_has_a_pinned_scene() {
    let table: BTreeMap<&str, &str> = RULE_SEMANTICS.iter().copied().collect();
    let cases = cases();
    // One scene per family of the table, each in a rule of its family.
    let names: Vec<&str> = cases.iter().map(|c| c.family).collect();
    assert_eq!(
        names
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        table.keys().copied().collect(),
        "a scene for every family in RULE_SEMANTICS, and only those"
    );
    assert_eq!(names.len(), table.len(), "one scene per family");
    for c in &cases {
        let rule = jetcarp_core::rules::Rule::parse(c.rule).unwrap().0;
        assert_eq!(family(&rule), c.family, "{}", c.rule);
    }
    let got: BTreeMap<&str, String> = cases
        .par_iter()
        .map(|c| {
            let (first, last, alive) = run(c).unwrap_or_else(|e| panic!("{}: {e}", c.family));
            // A scene that does nothing, or dies out, would pin little.
            assert_ne!(first, last, "{}: the scene does not change", c.family);
            assert!(alive, "{}: the scene dies out", c.family);
            (c.family, last)
        })
        .collect();
    let pinned = read_pins();
    if std::env::var_os("JETCARP_PIN_WRITE").is_some() {
        let refused: Vec<String> = got
            .iter()
            .filter_map(|(f, h)| match pinned.get(*f) {
                Some((v, old)) if old != h && v == table[f] => Some(bump(f, v, old, h)),
                _ => None,
            })
            .collect();
        assert!(refused.is_empty(), "not written:\n{}", refused.join("\n"));
        let mut text = String::from(
            "# Pinned rule semantics (tests/semantics_pins.rs): rule family, its semantics version\n\
             # in RULE_SEMANTICS when pinned, and the state hash its scene reaches.\n",
        );
        for (f, h) in &got {
            text.push_str(&format!("{f}\t{}\t{h}\n", table[f]));
        }
        std::fs::write(pins_path(), text).unwrap();
        return;
    }
    let mut bad = vec![];
    for (f, h) in &got {
        match pinned.get(*f) {
            None => bad.push(format!("{f}: not pinned (run with JETCARP_PIN_WRITE=1)")),
            Some((v, old)) if old != h => bad.push(bump(f, v, old, h)),
            Some((v, _)) if v != table[f] => bad.push(format!(
                "{f}: pinned at version {v}, but RULE_SEMANTICS has {}: pin it again with \
                 JETCARP_PIN_WRITE=1",
                table[f]
            )),
            Some(_) => {}
        }
    }
    for f in pinned.keys().filter(|f| !table.contains_key(f.as_str())) {
        bad.push(format!("{f}: pinned, but not a family in RULE_SEMANTICS"));
    }
    assert!(
        bad.is_empty(),
        "{} of {} families differ from tests/data/semantics_pins.txt:\n{}",
        bad.len(),
        got.len(),
        bad.join("\n")
    );
}
