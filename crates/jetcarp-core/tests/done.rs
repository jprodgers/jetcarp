//! Completion detection (`World::done`) and the `done` script trigger.

use jetcarp_core::script::{CaRef, Script, ScriptStep, Trigger};
use jetcarp_core::*;

fn world(family: &str, w: u32, h: u32, b: Boundary, cas: Vec<CaSpec>) -> World {
    let mut world = World::new(TilingSpec::rect(family, w, h, b), 1).unwrap();
    for c in cas {
        world.add_ca(c).unwrap();
    }
    world
}

/// Steps until done (at most `limit`), returning the generation it became done at.
fn run_until_done(w: &mut World, limit: u64) -> Option<u64> {
    if w.done() {
        return Some(w.generation);
    }
    for _ in 0..limit {
        w.step();
        if w.done() {
            return Some(w.generation);
        }
    }
    None
}

fn rows(w: &World) -> u32 {
    w.topo.rows().or_else(|| w.topo.hex_rows()).unwrap().h
}

#[test]
fn elementary_rules_stop_at_the_bottom_row() {
    for (family, rule) in [
        ("square", "W30"),
        ("square", "W90"),
        ("hex", "HexW3 8229"),
        ("hex", "HexW 6"),
    ] {
        for track in [false, true] {
            // Dead edges: the run cannot continue past the last row.
            let mut w = world(family, 61, 40, Boundary::Dead, vec![CaSpec::new(rule)]);
            w.set_track_changes(track);
            let h = rows(&w);
            assert_eq!(h, 40);
            let g = run_until_done(&mut w, 500);
            assert_eq!(g, Some(h as u64 - 1), "{rule} dead edges, tracking {track}");
            assert!(
                w.done_reason().unwrap().contains("bottom row"),
                "{:?}",
                w.done_reason()
            );
            // Nothing changes afterwards.
            let before = w.fields[0].state.clone();
            w.step();
            assert_eq!(before, w.fields[0].state);
            assert!(w.done());

            // Wrap edges: one full pass from the top row to the bottom row, then it stops too.
            let mut w = world(family, 61, 40, Boundary::Wrap, vec![CaSpec::new(rule)]);
            w.set_track_changes(track);
            assert_eq!(run_until_done(&mut w, 500), Some(39), "{rule} wrap");
            assert!(w.done_reason().unwrap().contains("full pass"));
            let before = w.fields[0].state.clone();
            w.step();
            assert_eq!(before, w.fields[0].state, "{rule}: the picture stays");
            assert!(w.done(), "{rule}: it stops at the bottom on wrap edges too");
        }
    }
}

#[test]
fn rule_zero_still_runs_to_the_bottom() {
    // W0 writes only empty rows, which leaves the cells unchanged, but the cursor still moves.
    let mut w = world("square", 30, 25, Boundary::Dead, vec![CaSpec::new("W0")]);
    w.set_track_changes(true);
    assert_eq!(run_until_done(&mut w, 100), Some(24));
}

#[test]
fn done_is_not_part_of_the_hash() {
    let mut a = world("square", 40, 30, Boundary::Dead, vec![CaSpec::new("W30")]);
    let mut b = world("square", 40, 30, Boundary::Dead, vec![CaSpec::new("W30")]);
    b.set_track_changes(true);
    for _ in 0..50 {
        a.step();
        b.step();
        let _ = b.done();
    }
    assert_eq!(a.hash_hex(), b.hash_hex());
}

#[test]
fn field_rules_done_when_empty_or_frozen() {
    // A single Life cell dies at generation 1.
    let one = SeedSpec::Cells {
        cells: vec![[205, 1]],
    };
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Dead,
        vec![CaSpec::new("B3/S23").seed(one.clone())],
    );
    assert!(!w.done());
    assert_eq!(run_until_done(&mut w, 10), Some(1));
    assert_eq!(w.done_reason().as_deref(), Some("empty"));

    // Live edges: an empty Life field is not finished (cells are born along the edge).
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Live,
        vec![CaSpec::new("B3/S23").seed(one.clone())],
    );
    w.step();
    assert!(w.fields[0].is_empty() || !w.done());

    // B0 rules are never "done" because their field is empty.
    let w = world(
        "square",
        20,
        20,
        Boundary::Wrap,
        vec![CaSpec::new("B0123/S").seed(SeedSpec::Empty)],
    );
    assert!(!w.done());

    // A block is frozen: seen at once when changes are tracked, else by the exact period watch.
    let block = SeedSpec::Cells {
        cells: vec![[105, 1], [106, 1], [125, 1], [126, 1]],
    };
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Wrap,
        vec![CaSpec::new("B3/S23").seed(block.clone())],
    );
    w.set_track_changes(true);
    assert_eq!(run_until_done(&mut w, 10), Some(1));
    assert!(w.done_reason().unwrap().contains("frozen"));
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Wrap,
        vec![CaSpec::new("B3/S23").seed(block)],
    );
    let g = run_until_done(&mut w, 10).unwrap();
    assert!((1..=3).contains(&g), "period watch saw the block at {g}");

    // A blinker is never done.
    let blinker = SeedSpec::Cells {
        cells: vec![[105, 1], [106, 1], [107, 1]],
    };
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Wrap,
        vec![CaSpec::new("B3/S23").seed(blinker)],
    );
    w.set_track_changes(true);
    assert_eq!(run_until_done(&mut w, 100), None);

    // Forest fire is never frozen by a quiet generation.
    let mut w = world(
        "square",
        20,
        20,
        Boundary::Wrap,
        vec![CaSpec::new("ForestFire 0.0/0.0").seed(SeedSpec::Empty)],
    );
    w.set_track_changes(true);
    w.step();
    assert!(!w.done());
}

#[test]
fn sandpiles_ants_and_layers() {
    // A pile of 200 grains on squares stabilises.
    let mut w = world(
        "square",
        31,
        31,
        Boundary::Dead,
        vec![CaSpec::new("Sandpile").seed(SeedSpec::Pile { grains: 200 })],
    );
    assert!(!w.done());
    let g = run_until_done(&mut w, 10_000).expect("stabilises");
    assert!(g > 5);
    assert!(w.done_reason().unwrap().contains("stable"));

    // An ant that falls off a dead edge leaves nothing running.
    let mut ant = CaSpec::new("Ant RL");
    ant.agent_edge = world::AgentEdge::FallOff;
    let mut w = world("square", 12, 12, Boundary::Dead, vec![ant]);
    assert!(!w.done());
    let g = run_until_done(&mut w, 100_000).expect("the ant leaves");
    assert!(w.cas[0].agents.is_empty() && g > 10);

    // Layered rules finish at the farthest layer from the seed.
    let mut w = world(
        "square",
        21,
        21,
        Boundary::Dead,
        vec![CaSpec::new("Layer 2/01")],
    );
    let g = run_until_done(&mut w, 100).unwrap();
    // Centre to corner is 10 touching steps.
    assert_eq!(g, 10);
    assert!(w.done_reason().unwrap().contains("last layer"));

    // Two CAs: done only when both are.
    let mut w = world(
        "square",
        30,
        20,
        Boundary::Dead,
        vec![CaSpec::new("W30").private(), CaSpec::new("W90").private()],
    );
    w.apply(Command::UpdateCa {
        id: 2,
        change: CaChange {
            steps_per_gen: Some(2),
            ..Default::default()
        },
    })
    .unwrap();
    // W90 at two rows per generation reaches the bottom first; W30 needs 19 generations.
    assert_eq!(run_until_done(&mut w, 100), Some(19));
}

#[test]
fn done_trigger_in_scripts() {
    let json = r#"{
      "tiling": {"family": "square", "size": {"rect": {"w": 80, "h": 50}}, "boundary": "dead"},
      "rng_state": 3,
      "cas": [{"rule": "W30", "seed": "top_center"}, {"rule": "B3/S23", "seed": "empty"}],
      "script": {"steps": [
        {"name": "Rule 30 to the bottom", "run": ["Rule 30"], "until": {"when": "done"}},
        {"name": "Life until done", "run": ["Life"], "until": {"when": "done"}, "max_gens": 3000}
      ]}
    }"#;
    let sc = Scene::from_json(json).unwrap();
    assert_eq!(sc.script.as_ref().unwrap().steps[0].until, Trigger::Done);
    let mut w = World::from_scene(&sc).unwrap();
    w.apply(Command::Step { gens: 48 }).unwrap();
    assert_eq!(w.script_state.step, 0);
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert_eq!(w.generation, 49);
    assert_eq!(w.script_state.step, 1, "rule 30 reached row 49 of 50");
    assert_eq!(w.script_state.started, 49);
    w.apply(Command::Step { gens: 3000 }).unwrap();
    assert!(w.script_state.finished);
    // Replaying the scene gives the same state.
    let again = World::from_scene(&w.to_scene()).unwrap();
    assert_eq!(again.hash_hex(), w.hash_hex());
    // Serialised as {"when": "done"}.
    let s = Script {
        steps: vec![ScriptStep {
            name: String::new(),
            run: vec![CaRef::Id(1)],
            until: Trigger::Done,
            max_gens: None,
        }],
        repeat: false,
    };
    assert!(
        serde_json::to_string(&s)
            .unwrap()
            .contains(r#"{"when":"done"}"#)
    );
}

#[test]
fn fill_size_matches_the_screen() {
    // Squares of 8 px on 1920 x 1080: 240 x 135 cells.
    let spec = TilingSpec::new("square");
    assert_eq!(
        topology::fill_size(&spec, 1920.0, 1080.0, 8.0),
        SizeSpec::Rect { w: 240, h: 135 }
    );
    // Hexagons: the grid's pixel size at that scale is at most the screen.
    let spec = TilingSpec::new("hex").with_boundary(Boundary::Dead);
    let SizeSpec::Rect { w, h } = topology::fill_size(&spec, 1920.0, 1080.0, 8.0) else {
        panic!()
    };
    let t = Topology::build(&spec.clone().with_size(SizeSpec::Rect { w, h })).unwrap();
    let g = t.lattice().unwrap();
    let area = (g.a1[0] * g.a2[1] - g.a1[1] * g.a2[0]).abs();
    let zoom = 8.0 / area.sqrt();
    assert!(w as f64 * g.a1[0] * zoom <= 1920.0 && h as f64 * g.a2[1] * zoom <= 1080.0);
    assert!(w > 200 && h > 100, "{w} x {h}");
}
