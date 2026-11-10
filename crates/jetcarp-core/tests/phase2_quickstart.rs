//! Quick Start: a working example of every named rule.
use jetcarp_core::quickstart::{entries, health, preview_cells};
use jetcarp_core::rules::catalog;
use rayon::prelude::*;

#[test]
fn every_named_rule_has_an_example() {
    let all = entries();
    for e in catalog() {
        assert!(
            all.iter().any(|q| q.id == format!("rule:{}", e.notation)),
            "no Quick Start example for {} ({})",
            e.name,
            e.notation
        );
    }
    assert!(all.len() >= 190, "only {} examples", all.len());
}

// Every example, at the size its live preview uses, is alive and still doing something after 300
// generations (or settled only after a while, or cycling as waves). Two are checked differently:
// the sandpile identity is a still picture by design, and the Spectre glider flies off the
// smaller preview patch, so it is checked at full size.
#[test]
fn every_example_works() {
    let all = entries();
    let bad: Vec<String> = all
        .par_iter()
        .filter_map(|e| {
            let full = e.rule == "SpectreGlider";
            let h = if e.id == "sandpile-identity-cairo" {
                let h = health(&e.scene, 1, None).ok()?;
                return (h.population == 0).then(|| format!("{}: empty", e.scene.name));
            } else if full {
                health(&e.scene, 100, None)
            } else {
                health(&e.scene, 300, preview_cells(&e.scene, 6000))
            };
            match h {
                Ok(h) if h.works() => None,
                Ok(h) => Some(format!(
                    "{} ({}): {} pop {}",
                    e.scene.name, e.rule, h.status, h.population
                )),
                Err(err) => Some(format!("{}: {err}", e.scene.name)),
            }
        })
        .collect();
    assert!(bad.is_empty(), "examples not working:\n{}", bad.join("\n"));
}

// Examples are reproducible: the same entry gives the same hash twice.
#[test]
fn examples_are_reproducible() {
    for e in entries().iter().step_by(17) {
        let run = || {
            let mut w = jetcarp_core::World::from_scene(&jetcarp_core::quickstart::preview_scene(
                &e.scene, 3000,
            ))
            .unwrap();
            w.apply(jetcarp_core::Command::Step { gens: 40 }).unwrap();
            w.hash_hex()
        };
        assert_eq!(run(), run(), "{}", e.scene.name);
    }
}
