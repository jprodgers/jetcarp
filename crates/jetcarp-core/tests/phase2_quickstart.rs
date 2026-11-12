//! Quick Start: a working example of every named rule, the featured entries, and previews of
//! scenes that place cells by index.
use jetcarp_core::quickstart::{
    BUNDLED, COMPUTING_SCENES, FEATURED, entries, health, places_cells, preview_cells,
    preview_scene,
};
use jetcarp_core::rules::catalog;
use jetcarp_core::topology::{Boundary, TilingSpec};
use jetcarp_core::world::{CaSpec, Command, Scene, SeedSpec, World};
use rayon::prelude::*;
use std::collections::HashSet;

// A scene that starts an ant on a cell index (an ant run's build-up, as `jetcarp library ants
// promote` makes it, or a pack's highway) only works at its own size: its card's preview keeps
// it, where a smaller grid would have no such cell (or put the ant somewhere else).
#[test]
fn ant_scenes_keep_their_size_in_previews() {
    let mut sc = Scene::new(TilingSpec::rect("hex", 400, 300, Boundary::Dead));
    let ant = CaSpec::new("Turmite RNL").seed(SeedSpec::AntAt {
        cell: 100_000,
        heading: 0,
    });
    sc.cas = vec![ant.clone()];
    assert!(places_cells(&sc));
    let p = preview_scene(&sc, 6000);
    assert_eq!(p.tiling.size, sc.tiling.size);
    World::from_scene(&p).unwrap();
    // The same ant added by a command.
    sc.cas.clear();
    sc.commands = vec![Command::AddCa { ca: ant }];
    assert!(places_cells(&sc));
    // A random soup of ants is made smaller, as before.
    sc.commands.clear();
    sc.cas = vec![CaSpec::new("Turmite RNL").seed(SeedSpec::Random { density: 0.001 })];
    assert!(!places_cells(&sc));
}

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

// The featured entries are the hand-made scenes and the starter set's built-ins (`FEATURED`), and
// every listed id names an entry, once: a renamed built-in fails here instead of dropping out of
// the starter set. 71 = 25 hand-made scenes + 46 more kept in the curation of 2026-11-11 (the
// list's other 17 are hand-made); a later curation changes this count with the list.
#[test]
fn featured_entries_are_the_hand_made_scenes_and_the_list() {
    let all = entries();
    let mut listed = HashSet::new();
    for id in FEATURED {
        assert!(listed.insert(*id), "FEATURED lists {id} twice");
        let e = all
            .iter()
            .find(|q| q.id == *id)
            .unwrap_or_else(|| panic!("FEATURED lists {id}, which is not a built-in entry"));
        assert!(e.featured, "{id}");
    }
    let hand_made: HashSet<&str> = BUNDLED
        .iter()
        .chain(COMPUTING_SCENES)
        .map(|(id, _)| *id)
        .collect();
    for e in &all {
        let want = hand_made.contains(e.id.as_str()) || listed.contains(e.id.as_str());
        assert_eq!(e.featured, want, "{}", e.id);
    }
    assert_eq!(hand_made.len(), 25);
    assert_eq!(all.iter().filter(|q| q.featured).count(), 71);
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
