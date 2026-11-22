//! The size a world is asked for reaches the build as it is (#300), tested on the whole app with no
//! window and no GPU, as the keyboard tests are ([`super::keyboard_tests`]): the sides are typed
//! into the Tiling pane's *Width × height* fields as key events, and each test runs in a process
//! of its own with scratch folders, where this machine's largest world is lowered to 1 GB. No
//! world here is larger than about 150 MB.
//!
//! Limits only allow or refuse: a world over the limit gets the over-limit question with what it
//! needs and the limit, and nothing is built unless asked; a world within it is built at the size
//! asked, even when the view cannot draw every cell of it at once, and the view says so.

use super::keyboard_tests::{Kb, child, done, in_scratch};
use super::*;
use egui::Key;
use jetcarp_core::machine::{self, Id, Limits, Own};

/// This machine's largest world, lowered to `bytes` in the test's own settings folder (a scratch
/// one) and read again, as Tools → This computer… does.
fn largest_world(bytes: u64) {
    let settings = jetcarp_core::paths::config_dir().expect("the test's settings folder");
    Limits::reference()
        .set(&Own::default(), Id::World, bytes as f64)
        .unwrap()
        .save(&settings)
        .unwrap();
    machine::reload();
}

/// Life on 40 × 30 squares, the Tiling pane shown with its *Width × height* fields.
fn sides_shown() -> Kb {
    let mut k = Kb::with_life();
    k.app
        .run_action(&k.ctx.clone(), menu::Action::Pane("tiling"));
    k.app.t_size = SizeChoice::Rect(40, 30);
    k.frames(3);
    k
}

/// `text` typed into the side field named `field` in place of what it held, and Enter.
fn type_side(k: &mut Kb, field: &str, text: &str) {
    k.focus_named(field);
    k.key_with(Key::A, egui::Modifiers::COMMAND);
    k.typed(text);
    k.key(Key::Enter);
    k.frames(2);
}

/// Frames until a build in the background is in place (at most a minute).
fn wait_built(k: &mut Kb) {
    let t = std::time::Instant::now();
    while k.app.building.is_some() && t.elapsed().as_secs() < 60 {
        k.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(k.app.building.is_none(), "the build finished");
    k.frames(3);
}

/// The world's sides in unit cells.
fn sides(k: &Kb) -> (u32, u32) {
    let g = k.app.world.topo.lattice().expect("a lattice");
    (g.w, g.h)
}

/// Every text the last frame's tree holds (labels and values), for finding a line.
fn texts(k: &Kb) -> Vec<String> {
    k.nodes
        .values()
        .flat_map(|n| [n.label.clone(), n.value.clone()])
        .flatten()
        .collect()
}

#[test]
fn sides_typed_over_the_limit_are_asked_about_never_made_smaller() {
    in_scratch(
        "app::size_tests::child_sides_typed_over_the_limit_are_asked_about_never_made_smaller",
    );
}

/// 100,000 × 100,000 squares typed into the Tiling pane, on a machine whose largest world is
/// 1 GB: each side stays as typed. 100,000 × 30 fits, and is built at that width; 100,000 ×
/// 100,000 (about 75 GB) gets the over-limit question with what it needs and the limit, and
/// builds nothing; Cancel leaves the world as it was.
#[test]
#[ignore]
fn child_sides_typed_over_the_limit_are_asked_about_never_made_smaller() {
    if !child() {
        return;
    }
    largest_world(1 << 30);
    let mut k = sides_shown();
    type_side(&mut k, "Width in unit cells", "100000");
    wait_built(&mut k);
    assert_eq!(sides(&k), (100_000, 30), "built at the width typed");
    type_side(&mut k, "Height in unit cells", "100000");
    assert_eq!(
        k.app.t_size,
        SizeChoice::Rect(100_000, 100_000),
        "the fields keep the sides typed"
    );
    let ask = k.app.machine.ask.as_ref().expect("the over-limit question");
    assert!(
        ask.message
            .starts_with("this world needs 74.5 GB; this machine's limit is 1 GB (your own value"),
        "{}",
        ask.message
    );
    assert!(k.app.building.is_none(), "nothing is built while it asks");
    k.frames(2);
    assert!(
        texts(&k)
            .iter()
            .any(|t| t.starts_with("This world needs 74.5 GB")),
        "the question shows what the world needs"
    );
    k.focus_named("Cancel");
    k.key(Key::Enter);
    k.frames(2);
    assert!(k.app.machine.ask.is_none(), "Cancel answered it");
    assert!(k.app.building.is_none());
    assert_eq!(sides(&k), (100_000, 30), "nothing built on Cancel");
    done();
}

#[test]
fn a_world_too_big_to_draw_whole_is_built_as_asked_and_the_view_says_so() {
    in_scratch(
        "app::size_tests::child_a_world_too_big_to_draw_whole_is_built_as_asked_and_the_view_says_so",
    );
}

/// 20,000 × 1,000 squares (20 million cells, about 150 MB) typed in: within the limit, so built at
/// that size with no question, though the view, fitted to it, cannot draw each cell (they are
/// under a pixel); the status line says so, and that zooming in shows them. Zoomed in to a few
/// pixels a cell, every cell in view is drawn and the line goes.
#[test]
#[ignore]
fn child_a_world_too_big_to_draw_whole_is_built_as_asked_and_the_view_says_so() {
    if !child() {
        return;
    }
    largest_world(1 << 30);
    let mut k = sides_shown();
    type_side(&mut k, "Width in unit cells", "20000");
    wait_built(&mut k);
    type_side(&mut k, "Height in unit cells", "1000");
    wait_built(&mut k);
    let mut wrong = vec![];
    if let Some(a) = &k.app.machine.ask {
        wrong.push(format!(
            "asked about a world within the limit: {}",
            a.message
        ));
    }
    if sides(&k) != (20_000, 1_000) {
        wrong.push(format!("built {:?}, not the 20000 x 1000 typed", sides(&k)));
    }
    let says = |k: &Kb| texts(k).into_iter().find(|t| t.contains("Zoom in to see"));
    match says(&k) {
        Some(t) if t.contains("cannot draw every cell") => {}
        other => wrong.push(format!(
            "the status line does not say the view cannot show every cell: {other:?}"
        )),
    }
    // Zoomed in: every cell in view is drawn.
    k.app.pixel_zoom(4.0);
    k.frames(3);
    if let Some(t) = says(&k) {
        wrong.push(format!("still says so at 4 px a cell: {t}"));
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    done();
}
