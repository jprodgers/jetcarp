//! A stage running a rotation on real worlds, on a clock the test controls.

use jetcarp_display::profiles;
use jetcarp_display::rotation::*;
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Lookup, Stage};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn life_item(seconds: f64) -> Item {
    Item::new(Source::Profile {
        name: "Life".into(),
    })
    .end(End::Minutes {
        minutes: seconds / 60.0,
    })
}

fn stage_of(items: Vec<Item>, lookup: Lookup, max_cells: u64) -> Stage {
    let r = Rotation {
        name: "test".into(),
        default_minutes: None,
        items,
        ..Default::default()
    };
    let (slots, warnings) = r.expand(&default_base(), &|n| {
        profiles::builtins().into_iter().find(|p| p.name == n)
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    Stage::new(&r, slots, 1, [120.0, 90.0], lookup, max_cells).expect("a stage")
}

fn no_lookup() -> Lookup {
    Arc::new(|_| None)
}

fn sim() -> SimBackend {
    SimBackend::unavailable("not in this test".into())
}

/// Ticks at `now`, until `ok` or a real minute passes.
fn until(st: &mut Stage, sim: &mut SimBackend, now: f64, ok: impl Fn(&Stage) -> bool) {
    let t0 = Instant::now();
    while !ok(st) {
        assert!(t0.elapsed() < Duration::from_secs(60), "{:?}", st.status());
        st.tick(now, sim, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_pause_does_not_use_up_an_items_time() {
    let mut st = stage_of(vec![life_item(10.0), life_item(10.0)], no_lookup(), 1 << 30);
    let mut s = sim();
    until(&mut st, &mut s, 0.0, |x| x.world().is_some());
    // The first tick with the world tells the rotation it started, at 0 s of play.
    st.tick(0.0, &mut s, Duration::from_millis(1));
    st.tick(5.0, &mut s, Duration::from_millis(1));
    assert_eq!(st.status().number, 1);
    st.set_paused(true);
    st.tick(500.0, &mut s, Duration::from_millis(1));
    let frozen = st.status();
    assert_eq!(frozen.number, 1);
    st.tick(510.0, &mut s, Duration::from_millis(1));
    assert_eq!(st.status().generation, frozen.generation, "paused");
    st.set_paused(false);
    st.tick(510.5, &mut s, Duration::from_millis(1));
    assert_eq!(st.status().number, 1, "5.5 s of play is not over");
    st.tick(516.0, &mut s, Duration::from_millis(1));
    assert_eq!(st.status().number, 2, "over 10 s of play: on to the next");
}

#[test]
fn an_unknown_quick_start_entry_shows_life_and_says_so() {
    let missing = Item::new(Source::QuickStart {
        id: "rule:nope".into(),
    })
    .end(End::Minutes { minutes: 1.0 });
    let mut st = stage_of(vec![missing], no_lookup(), 1 << 30);
    let mut s = sim();
    until(&mut st, &mut s, 0.0, |x| x.world().is_some());
    let notes = st.take_notes();
    assert!(
        notes.iter().any(|n| n.contains("showing Life")),
        "{notes:?}"
    );
    assert!(
        st.caption(false)[0].contains("Life"),
        "{:?}",
        st.caption(false)
    );
}

#[test]
fn giving_up_pauses_and_resuming_tries_again() {
    let mut st = stage_of(vec![life_item(10.0)], no_lookup(), 3);
    let mut s = sim();
    until(&mut st, &mut s, 0.0, |x| x.paused());
    assert!(st.world().is_none());
    let notes = st.take_notes();
    assert!(
        notes.iter().any(|n| n.contains("over the limit")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("can be shown")),
        "{notes:?}"
    );
    st.set_paused(false);
    until(&mut st, &mut s, 1.0, |x| x.paused());
    assert!(st.take_notes().iter().any(|n| n.contains("over the limit")));
}

#[test]
fn a_cell_is_a_point_so_a_2x_screen_builds_a_quarter_of_the_cells() {
    let mut s = sim();
    let mut one = stage_of(vec![life_item(60.0)], no_lookup(), 1 << 30);
    until(&mut one, &mut s, 0.0, |x| x.world().is_some());
    let mut two = stage_of(vec![life_item(60.0)], no_lookup(), 1 << 30);
    two.resize([120.0, 90.0], 2.0, 0.0);
    until(&mut two, &mut s, 0.0, |x| x.world().is_some());
    let (a, b) = (
        one.world().unwrap().topo.len() as f64,
        two.world().unwrap().topo.len() as f64,
    );
    assert!((3.5..4.5).contains(&(a / b)), "{a} cells at 1x, {b} at 2x");
}

#[test]
fn a_new_canvas_rebuilds_the_world_once_the_size_holds() {
    let mut st = stage_of(vec![life_item(60.0)], no_lookup(), 1 << 30);
    let mut s = sim();
    until(&mut st, &mut s, 0.0, |x| x.world().is_some());
    let cells = st.world().unwrap().topo.len();
    st.resize([240.0, 180.0], 1.0, 1.0);
    st.tick(1.2, &mut s, Duration::from_millis(1));
    assert_eq!(st.world().unwrap().topo.len(), cells, "still settling");
    st.tick(1.7, &mut s, Duration::from_millis(1));
    until(&mut st, &mut s, 1.8, |x| {
        x.world().is_some_and(|w| w.topo.len() > cells)
    });
}

#[test]
fn just_after_a_skip_the_status_is_the_new_items() {
    let mut st = stage_of(
        vec![life_item(600.0), life_item(600.0)],
        no_lookup(),
        1 << 30,
    );
    let mut s = sim();
    until(&mut st, &mut s, 0.0, |x| x.world().is_some());
    for k in 1..20 {
        st.tick(k as f64, &mut s, Duration::from_millis(1));
    }
    let before = st.status();
    assert!(before.generation > 0 && before.cells > 0, "{before:?}");
    st.skip(1).unwrap();
    // The old world is still on screen, but the reply to `next` is about the new item.
    assert!(st.world().is_some());
    let after = st.status();
    assert_eq!((after.number, after.generation, after.cells), (2, 0, 0));
    assert!(after.building && after.caption.is_empty(), "{after:?}");
    until(&mut st, &mut s, 30.0, |x| x.status().cells > 0);
    assert_eq!(st.status().number, 2);
}
