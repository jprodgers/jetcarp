//! Rule-semantics versions: what each rule family means, as a version number.
//!
//! Geometry pins ([`crate::pin`]) catch a changed tiling. These catch a changed rule: a parser
//! fix, or a change in how a family counts neighbours or states, steps, or draws its randomness,
//! anything that makes the same rule, start and seed reach another state on the same tiles.
//!
//! - [`RULE_SEMANTICS`] gives every rule family the engine runs a version, a positive integer
//!   written as a string. Every family is at `"1"` in JETCARP 0.4.0. A family's version goes up
//!   by one when the meaning of any rule in it changes; a change that keeps every rule's meaning
//!   (a faster step, a new notation for an old rule) keeps it.
//! - Every pattern record carries its rule's family's version as it was verified
//!   ([`crate::pattern::PatternRecord::semantics`]). A library flags (`needs_reverify`) the
//!   records whose version is not the running engine's, as it flags records whose geometry pin
//!   is not the engine's, and `jetcarp library reverify` checks them again. Packs carry the table
//!   as their engine's `rule_semantics`.
//! - `tests/semantics_pins.rs` runs one small fixed scene per family and pins the state it
//!   reaches. A change in a family's meaning fails that test, and the test refuses to pin the new
//!   state until the family's version here has been raised.
//!
//! The families ([`family`]) are the engine's rule kinds, one per kind of [`Rule`], with two
//! splits: B/S rules (`lifelike`) and Generations (`generations`) are apart, though a pattern's
//! identity calls both `lifelike`; and turmites are `ant` (a turn string: one state, each colour
//! repainted the next, such as `Ant RL` and `Turmite LLRR`) or `turmite` (a state table, such as
//! `Turmite 1R1,1L0/1R1,0N0`). The names of the families that a pattern identity's `model` line
//! also names (`lifelike`, `wireworld`, `table`, `priority`, `spectre_glider`, `ltl`,
//! `margolus`, `lenia`) are the same; the identity's `other` is split here into its families.

use crate::rules::Rule;

/// The version every family started at (JETCARP 0.4.0), and so the version of a record written
/// before versions existed.
pub const FIRST: &str = "1";

/// Every rule family and its semantics version: the one table. Raise a family's version by one
/// when the meaning of any of its rules changes (see the module notes).
pub const RULE_SEMANTICS: [(&str, &str); 23] = [
    // B/S rules: `B3/S23`, `B3/S23V`.
    ("lifelike", "1"),
    // Generations: `B2/S/C3`.
    ("generations", "1"),
    // Larger than Life: `R5,C2,S33-57,B34-45`.
    ("ltl", "1"),
    // Elementary rules on square rows: `W30`.
    ("elementary", "1"),
    // Two-parent hexagon rows: `HexW 6`, `HexW3 1234`.
    ("hexw", "1"),
    // Three-parent flat-top hexagon rows: `HexT 30`, `HexT3 sum 777`.
    ("hext", "1"),
    // Layered rules: `Layer 2/01`.
    ("layered", "1"),
    // JACA: `JACA +B3/S23 -B36/S23 blast 2`.
    ("jaca", "1"),
    // Turn-string ants: `Ant RL`, `Turmite LLRR`.
    ("ant", "1"),
    // Turmite state tables: `Turmite 1R1,1L0/1R1,0N0`.
    ("turmite", "1"),
    ("wireworld", "1"),
    // `Cyclic 14/1`.
    ("cyclic", "1"),
    // `GH 8/1`.
    ("greenberg_hastings", "1"),
    // Rock-paper-scissors: `RPS 3/3`.
    ("rps", "1"),
    // `ForestFire 0.01/0.0001`.
    ("forest_fire", "1"),
    // `Hodgepodge 100/2/3/28`.
    ("hodgepodge", "1"),
    // `Margolus BBM`.
    ("margolus", "1"),
    // `Lenia R=13,T=10,b=1,m=0.15,s=0.015`.
    ("lenia", "1"),
    // `Sandpile`.
    ("sandpile", "1"),
    // Von Neumann's 29 states: `JvN29`.
    ("jvn29", "1"),
    // Priority tables: `Priority 4: ...`.
    ("priority", "1"),
    // `SpectreGlider`.
    ("spectre_glider", "1"),
    // Rule tables by name: `Table parity`.
    ("table", "1"),
];

/// The family of a rule (a name in [`RULE_SEMANTICS`]).
pub fn family(rule: &Rule) -> &'static str {
    // No catch-all arm: a new kind of rule must be given its family here.
    match rule {
        Rule::LifeLike { .. } => "lifelike",
        Rule::Generations { .. } => "generations",
        Rule::Ltl(_) => "ltl",
        Rule::Wolfram(_) => "elementary",
        Rule::HexWolfram { .. } => "hexw",
        Rule::HexTri { .. } => "hext",
        Rule::Layered { .. } => "layered",
        Rule::Jaca(_) => "jaca",
        Rule::Turmite(m) if m.turns().is_some() => "ant",
        Rule::Turmite(_) => "turmite",
        Rule::WireWorld => "wireworld",
        Rule::Cyclic { .. } => "cyclic",
        Rule::GreenbergHastings { .. } => "greenberg_hastings",
        Rule::Rps { .. } => "rps",
        Rule::ForestFire { .. } => "forest_fire",
        Rule::Hodgepodge { .. } => "hodgepodge",
        Rule::Margolus(_) => "margolus",
        Rule::Lenia(_) => "lenia",
        Rule::Sandpile => "sandpile",
        Rule::VonNeumann => "jvn29",
        Rule::Priority(_) => "priority",
        Rule::SpectreGlider => "spectre_glider",
        Rule::Table(_) => "table",
    }
}

/// The running engine's version of a family, if it has the family.
pub fn version(family: &str) -> Option<&'static str> {
    RULE_SEMANTICS
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, v)| *v)
}

/// The running engine's version of a rule's family.
pub fn rule_version(rule: &Rule) -> &'static str {
    // Every family has a version (the tests check it).
    version(family(rule)).unwrap_or(FIRST)
}

/// The family and the running engine's version of a rule given as notation, if it parses.
pub fn of_notation(notation: &str) -> Option<(&'static str, &'static str)> {
    let rule = Rule::parse(notation).ok()?.0;
    Some((family(&rule), rule_version(&rule)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// One rule of every kind, the two kinds of turmite apart.
    const EXAMPLES: [&str; 23] = [
        "B3/S23",
        "B2/S/C3",
        "R5,C2,S33-57,B34-45",
        "W30",
        "HexW 6",
        "HexT3 sum 777",
        "Layer 2/01",
        "JACA +B3/S23 -B36/S23 blast 2",
        "Turmite LLRR",
        "Turmite 1R1,1L0/1R1,0N0",
        "WireWorld",
        "Cyclic 14/1",
        "GH 8/1",
        "RPS 3/3",
        "ForestFire 0.01/0.0001",
        "Hodgepodge 100/2/3/28",
        "Margolus BBM",
        "Lenia R=13,T=10,b=1,m=0.15,s=0.015",
        "Sandpile",
        "JvN29",
        "Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1",
        "SpectreGlider",
        "Table parity",
    ];

    /// Which kind of [`Rule`] this is. No catch-all arm, so a new kind fails to compile here
    /// until it is counted, and the test below then asks for an example of it.
    fn kind(rule: &Rule) -> usize {
        match rule {
            Rule::LifeLike { .. } => 0,
            Rule::Generations { .. } => 1,
            Rule::Ltl(_) => 2,
            Rule::Wolfram(_) => 3,
            Rule::HexWolfram { .. } => 4,
            Rule::HexTri { .. } => 5,
            Rule::Layered { .. } => 6,
            Rule::Jaca(_) => 7,
            Rule::Turmite(_) => 8,
            Rule::WireWorld => 9,
            Rule::Cyclic { .. } => 10,
            Rule::GreenbergHastings { .. } => 11,
            Rule::Rps { .. } => 12,
            Rule::ForestFire { .. } => 13,
            Rule::Hodgepodge { .. } => 14,
            Rule::Margolus(_) => 15,
            Rule::Lenia(_) => 16,
            Rule::Sandpile => 17,
            Rule::VonNeumann => 18,
            Rule::Priority(_) => 19,
            Rule::SpectreGlider => 20,
            Rule::Table(_) => 21,
        }
    }
    const KINDS: usize = 22;

    #[test]
    fn the_table_covers_every_family_once() {
        let names: BTreeSet<&str> = RULE_SEMANTICS.iter().map(|(f, _)| *f).collect();
        assert_eq!(
            names.len(),
            RULE_SEMANTICS.len(),
            "a family is listed twice"
        );
        let mut kinds = BTreeSet::new();
        let mut reached = BTreeSet::new();
        for n in EXAMPLES {
            let rule = Rule::parse(n).unwrap_or_else(|e| panic!("{n}: {e}")).0;
            kinds.insert(kind(&rule));
            let f = family(&rule);
            assert!(
                version(f).is_some(),
                "{n}: family {f} is not in RULE_SEMANTICS"
            );
            assert_eq!(of_notation(n), Some((f, rule_version(&rule))));
            reached.insert(f);
        }
        assert_eq!(kinds.len(), KINDS, "EXAMPLES needs a rule of every kind");
        assert_eq!(
            reached, names,
            "every family in RULE_SEMANTICS is some rule's"
        );
        // `Ant RL` is a turn string like `Turmite LLRR`.
        assert_eq!(of_notation("Ant RL").unwrap().0, "ant");
        // Versions are positive integers written as strings; every family started at 1.
        for (f, v) in RULE_SEMANTICS {
            assert!(
                v.parse::<u32>().is_ok_and(|x| x >= 1) && !v.starts_with('0'),
                "{f}: version {v}"
            );
            assert!(
                f.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit()),
                "{f}"
            );
        }
        assert_eq!(FIRST, "1");
    }
}
