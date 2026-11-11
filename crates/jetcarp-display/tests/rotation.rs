//! Rotations: the player's decisions from made-up observations, then real worlds running through
//! it (the Wolfram series to the bottom of the screen, the settle watch).

use jetcarp_core::quickstart::all_entries;
use jetcarp_core::{Command, World};
use jetcarp_display::profiles::{self, Profile, Show};
use jetcarp_display::rotation::*;
use std::collections::HashSet;

const MONDAY_NOON: Clock = Clock {
    weekday: 0,
    minute: 12 * 60,
    day: 20_000,
};

fn obs(now: f64, generation: u64) -> Observation {
    Observation {
        now,
        clock: MONDAY_NOON,
        generation,
        population: 100,
        cells: 1000,
        activity: None,
        done: false,
        settled: false,
    }
}

fn rotation(items: Vec<Item>) -> Rotation {
    Rotation {
        name: "test".into(),
        default_minutes: None,
        items,
        ..Default::default()
    }
}

fn scene_item(name: &str) -> Item {
    let mut it = Item::new(Source::Scene {
        path: format!("{name}.json"),
    });
    it.label = Some(name.to_string());
    it
}

fn player(r: &Rotation, seed: u64) -> Player {
    let (slots, warn) = r.expand(&default_base(), &|n| {
        profiles::builtins().into_iter().find(|p| p.name == n)
    });
    assert!(warn.is_empty(), "{warn:?}");
    Player::new(r, slots, seed).expect("player")
}

/// Starts `p` and marks its first item loaded at `t`.
fn begin(p: &mut Player, t: f64) -> usize {
    let first = p.start(MONDAY_NOON).expect("something to show");
    p.on_loaded(t, 0, MONDAY_NOON);
    first
}

#[test]
fn items_end_by_time_and_generations_counted_from_their_start() {
    let r = rotation(vec![
        scene_item("a").end(End::Minutes { minutes: 1.0 }),
        scene_item("b").end(End::Generations { gens: 100 }),
    ]);
    let mut p = player(&r, 1);
    assert_eq!(begin(&mut p, 10.0), 0);
    assert_eq!(p.tick(&obs(69.0, 5)), Action::Stay);
    assert_eq!(p.seconds_left(69.0), Some(1.0));
    assert_eq!(p.tick(&obs(70.0, 5)), Action::Load(1));
    assert_eq!(p.tick(&obs(71.0, 5)), Action::Stay, "waits for the world");
    p.on_loaded(71.0, 1000, MONDAY_NOON);
    assert_eq!(p.tick(&obs(72.0, 1099)), Action::Stay);
    assert_eq!(p.tick(&obs(72.0, 1100)), Action::Load(0), "wraps around");
}

#[test]
fn finished_worlds_hold_their_last_frame_before_moving_on() {
    let r = rotation(vec![
        scene_item("a").end(End::Done).hold(3.0),
        scene_item("b"),
    ]);
    let mut p = player(&r, 1);
    begin(&mut p, 0.0);
    let mut o = obs(5.0, 10);
    assert_eq!(p.tick(&o), Action::Stay);
    o.done = true;
    assert_eq!(p.tick(&o), Action::Stay, "holding");
    o.now = 7.9;
    assert_eq!(p.tick(&o), Action::Stay);
    o.now = 8.0;
    assert_eq!(p.tick(&o), Action::Load(1));
}

#[test]
fn settled_ends_an_item_and_a_single_item_starts_again() {
    let r = rotation(vec![scene_item("a").end(End::Settled { latest: None })]);
    let mut p = player(&r, 1);
    begin(&mut p, 0.0);
    let mut o = obs(5.0, 10);
    assert_eq!(p.tick(&o), Action::Stay);
    o.settled = true;
    assert_eq!(p.tick(&o), Action::Load(0));
}

#[test]
fn schedules_choose_days_and_hours_and_end_items() {
    let weekend = Schedule {
        days: vec!["sat".into(), "sun".into()],
        ..Default::default()
    };
    let night = Schedule {
        from: Some("22:00".into()),
        to: Some("06:00".into()),
        ..Default::default()
    };
    let saturday = Clock {
        weekday: 5,
        ..MONDAY_NOON
    };
    assert!(weekend.contains(saturday) && !weekend.contains(MONDAY_NOON));
    let at = |minute| Clock {
        minute,
        ..MONDAY_NOON
    };
    assert!(night.contains(at(23 * 60)) && night.contains(at(60)) && !night.contains(at(720)));

    let mut a = scene_item("a");
    a.schedule = Some(weekend);
    let b = scene_item("b").end(End::Minutes { minutes: 1.0 });
    let mut p = player(&rotation(vec![a, b]), 1);
    assert_eq!(p.start(MONDAY_NOON), Some(1), "a is for weekends");
    p.on_loaded(0.0, 0, MONDAY_NOON);
    // b restarts while a may not run.
    assert_eq!(p.tick(&obs(60.0, 1)), Action::Load(1));
    p.on_loaded(60.0, 0, MONDAY_NOON);
    // On Saturday, b's time ends and a comes up.
    let mut o = obs(120.0, 1);
    o.clock = saturday;
    assert_eq!(p.tick(&o), Action::Load(0));
    p.on_loaded(120.0, 0, saturday);
    // Monday again: a has no time limit, but its schedule is over.
    assert_eq!(p.tick(&obs(130.0, 1)), Action::Load(1));
}

#[test]
fn nothing_eligible_keeps_the_current_item_running() {
    let mut a = scene_item("a").end(End::Minutes { minutes: 1.0 });
    a.schedule = Some(Schedule {
        days: vec!["sun".into()],
        ..Default::default()
    });
    let mut p = player(&rotation(vec![a]), 1);
    assert_eq!(begin(&mut p, 0.0), 0);
    assert_eq!(p.tick(&obs(120.0, 1)), Action::Stay);
}

#[test]
fn a_clock_end_fires_as_the_time_passes() {
    let r = rotation(vec![
        scene_item("a").end(End::Clock { at: "12:05".into() }),
        scene_item("b"),
    ]);
    let mut p = player(&r, 1);
    begin(&mut p, 0.0);
    let at = |now, minute| {
        let mut o = obs(now, 1);
        o.clock.minute = minute;
        o
    };
    assert_eq!(p.tick(&at(60.0, 12 * 60 + 4)), Action::Stay);
    assert_eq!(p.tick(&at(120.0, 12 * 60 + 5)), Action::Load(1));
}

#[test]
fn shuffle_visits_everything_then_reorders_without_repeating() {
    let items = (0..12)
        .map(|k| scene_item(&format!("s{k}")).end(End::Generations { gens: 1 }))
        .collect();
    let mut r = rotation(items);
    r.order = Order::Shuffle;
    let mut p = player(&r, 7);
    let mut last = begin(&mut p, 0.0);
    let mut seen = vec![last];
    for k in 0..40 {
        let Action::Load(n) = p.tick(&obs(k as f64, 1)) else {
            panic!("should move on");
        };
        assert_ne!(n, last, "a repeat at step {k}");
        last = n;
        seen.push(n);
        p.on_loaded(k as f64, 0, MONDAY_NOON);
    }
    assert_eq!(seen[..12].iter().collect::<HashSet<_>>().len(), 12);
    assert_ne!(seen[..12], (0..12).collect::<Vec<_>>());
    assert_ne!(seen[..12], seen[12..24], "a new order the second time");
}

#[test]
fn one_a_day_changes_at_midnight_and_restarts_within_the_day() {
    let mut r = rotation(vec![
        scene_item("a").end(End::Minutes { minutes: 1.0 }),
        scene_item("b"),
        scene_item("c"),
    ]);
    r.order = Order::Daily;
    let mut p = player(&r, 1);
    let first = p.start(MONDAY_NOON).unwrap();
    assert_eq!(first, 20_000 % 3);
    p.on_loaded(0.0, 0, MONDAY_NOON);
    let mut o = obs(5.0, 1);
    o.clock.day += 1;
    o.clock.weekday = 1;
    assert_eq!(p.tick(&o), Action::Load(20_001 % 3));
    assert_eq!(20_001 % 3, 0, "item a has the time limit");
    p.on_loaded(10.0, 0, o.clock);
    o.now = 70.0;
    assert_eq!(p.tick(&o), Action::Load(0), "restarts the same item");
}

#[test]
fn scripts_move_on_jump_reseed_and_remember() {
    let mut r = rotation(vec![scene_item("a"), scene_item("b"), scene_item("c")]);
    r.script = r#"
        state.runs = (state.runs ?? 0) + 1;
        if s.label == "a" && s.generation >= 10 { "next" }
        else if s.label == "b" && state.runs == 3 { #{item: "c"} }
        else if s.label == "c" && s.population < 50 { "reseed" }
        else if s.label == "c" && s.hour == 12 && s.generation > 500 { 0 }
    "#
    .into();
    let mut p = player(&r, 1);
    begin(&mut p, 0.0);
    assert_eq!(p.tick(&obs(1.0, 5)), Action::Stay);
    assert_eq!(p.tick(&obs(1.5, 50)), Action::Stay, "at most once a second");
    assert_eq!(p.tick(&obs(2.0, 50)), Action::Load(1));
    p.on_loaded(2.0, 50, MONDAY_NOON);
    assert_eq!(
        p.tick(&obs(3.0, 51)),
        Action::Load(2),
        "third run, kept state"
    );
    p.on_loaded(3.0, 0, MONDAY_NOON);
    let mut o = obs(4.0, 1);
    o.population = 10;
    assert_eq!(p.tick(&o), Action::Reseed);
    assert_eq!(p.tick(&obs(5.0, 501)), Action::Load(0));
    assert!(p.take_warning().is_none());
}

#[test]
fn script_problems_are_reported_and_cannot_hang() {
    let mut r = rotation(vec![scene_item("a")]);
    r.script = "1 +".into();
    assert!(r.validate().unwrap_err().contains("script"));

    r.script = "loop { }".into();
    let mut p = player(&r, 1);
    begin(&mut p, 0.0);
    let t0 = std::time::Instant::now();
    assert_eq!(p.tick(&obs(1.0, 1)), Action::Stay);
    assert!(t0.elapsed().as_secs_f64() < 2.0);
    assert!(p.take_warning().unwrap().contains("script"));

    for (script, want) in [("\"sideways\"", "not an answer"), ("99", "out of range")] {
        r.script = script.into();
        let mut p = player(&r, 1);
        begin(&mut p, 0.0);
        p.tick(&obs(1.0, 1));
        assert!(p.take_warning().unwrap().contains(want), "{script}");
    }
}

#[test]
fn bad_rotations_are_refused() {
    let good = || rotation(vec![scene_item("a")]);
    assert!(good().validate().is_ok());
    let mut r = good();
    r.name = " ".into();
    assert!(r.validate().is_err());
    let mut r = good();
    r.items.clear();
    assert!(r.validate().is_err());
    for bad in [
        End::Minutes { minutes: 0.0 },
        End::Generations { gens: 0 },
        End::LowActivity {
            below: 2.0,
            for_gens: 1,
        },
        End::Clock { at: "25:00".into() },
    ] {
        let r = rotation(vec![scene_item("a").end(bad.clone())]);
        assert!(r.validate().is_err(), "{bad:?}");
    }
    let mut it = scene_item("a");
    it.schedule = Some(Schedule {
        from: Some("09:00".into()),
        ..Default::default()
    });
    assert!(rotation(vec![it]).validate().is_err());
}

#[test]
fn built_ins_are_valid_and_refer_to_things_that_exist() {
    let known: HashSet<String> = all_entries().into_iter().map(|q| q.id).collect();
    let names: HashSet<String> = profiles::builtins().into_iter().map(|p| p.name).collect();
    let all = builtins();
    assert!(all.len() >= 4);
    for r in all {
        r.validate().unwrap_or_else(|e| panic!("{}: {e}", r.name));
        assert_eq!(Rotation::from_json(&r.to_json()).unwrap(), r, "{}", r.name);
        for it in &r.items {
            match &it.source {
                Source::Profile { name } => assert!(names.contains(name), "{name}"),
                Source::QuickStart { id } => assert!(known.contains(id), "{id}"),
                _ => {}
            }
        }
    }
}

#[test]
fn series_expand_to_one_item_per_rule() {
    let r = builtins()
        .into_iter()
        .find(|r| r.name == "Wolfram series")
        .unwrap();
    let (slots, warn) = r.expand(&default_base(), &|_| None);
    assert!(warn.is_empty());
    assert_eq!(slots.len(), 256);
    assert_eq!(slots[30].label, "W30");
    assert_eq!(slots[30].n, Some(30));
    assert_eq!(slots[30].end, vec![End::Done]);
    assert_eq!(slots[30].profile.palette.as_deref(), Some("Ink"));

    let too_many = rotation(vec![Item::new(Source::Series {
        rules: "HexT3".into(),
        from: None,
        to: None,
        family: "hex".into(),
        boundary: jetcarp_core::Boundary::Dead,
        seed: String::new(),
    })]);
    let (slots, warn) = too_many.expand(&default_base(), &|_| None);
    assert!(slots.is_empty());
    assert_eq!(warn.len(), 1, "{warn:?}");
}

/// The world a slot shows on a canvas of this size.
fn world_of(slot: &Slot, canvas: [f64; 2]) -> World {
    let Show::Scene { scene } = &slot.profile.show else {
        panic!("expected a scene");
    };
    let sc = profiles::apply_profile(&slot.profile, (**scene).clone(), Some(canvas));
    World::from_scene(&sc).expect("world")
}

#[test]
fn the_wolfram_series_draws_to_the_bottom_then_moves_to_the_next_rule() {
    let r = builtins()
        .into_iter()
        .find(|r| r.name == "Wolfram series")
        .unwrap();
    let mut p = player(&r, 1);
    let mut slot = p.start(MONDAY_NOON).unwrap();
    for expected in 0..4 {
        assert_eq!(slot, expected);
        let s = p.slots()[slot].clone();
        assert_eq!(s.label, format!("W{expected}"));
        let mut w = world_of(&s, [400.0, 240.0]);
        let rows = 240 / 4;
        p.on_loaded(1000.0 * expected as f64, w.generation, MONDAY_NOON);
        let t = 1000.0 * expected as f64 + 1.0;
        let mut gens = 0;
        loop {
            let mut o = obs(t, w.generation);
            o.done = w.done();
            let a = p.tick(&o);
            assert_eq!(a, Action::Stay, "ended early at generation {gens}");
            if o.done {
                break;
            }
            w.apply(Command::Step { gens: 1 }).unwrap();
            gens += 1;
            assert!(gens <= rows + 2, "never reached the bottom");
        }
        assert!(gens >= rows - 2, "stopped after {gens} of {rows} rows");
        // The last frame holds for three seconds, then the next rule.
        let mut o = obs(t + 2.9, w.generation);
        o.done = true;
        assert_eq!(p.tick(&o), Action::Stay);
        o.now = t + 3.0;
        let Action::Load(n) = p.tick(&o) else {
            panic!("should move on");
        };
        slot = n;
    }
    assert_eq!(slot, 4);
}

#[test]
fn the_settle_watch_fires_without_touching_the_world() {
    let life = profiles::builtins().remove(0);
    let Show::Scene { scene } = life.show.clone() else {
        panic!("Life keeps its scene");
    };
    let plain = Profile {
        reseed: Some(vec![]),
        cell_px: 4.0,
        ..life
    };
    let sc = profiles::apply_profile(&plain, *scene, Some([240.0, 160.0]));
    let mut watched = World::from_scene(&sc).unwrap();
    let mut control = World::from_scene(&sc).unwrap();
    let watch = End::Settled {
        latest: Some(20_000),
    }
    .watch()
    .unwrap();
    watched
        .apply(Command::SetReseed {
            policies: vec![watch],
        })
        .unwrap();
    let mut gens = 0;
    while watched.reseed_status()[0].fired == 0 {
        watched.apply(Command::Step { gens: 16 }).unwrap();
        gens += 16;
        assert!(gens <= 20_000 + 16, "never settled");
    }
    assert!(
        gens < 20_000,
        "Life on a small grid settles before the limit"
    );
    control.apply(Command::Step { gens }).unwrap();
    let all: Vec<u32> = (0..watched.topo.len() as u32).collect();
    assert_eq!(
        jetcarp_core::render::colors_for(&watched, &all),
        jetcarp_core::render::colors_for(&control, &all),
        "a watch must change no cells"
    );
}

#[test]
fn the_store_saves_overrides_built_ins_and_follows_renamed_profiles() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-rotations-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = RotationStore::at(Some(dir.clone()));
    let builtin_count = store.names().len();

    let mut mine = rotation(vec![Item::new(Source::Profile { name: "Old".into() })]);
    mine.name = "Mine".into();
    store.save(&mine).unwrap();
    assert_eq!(store.names().len(), builtin_count + 1);
    assert!(store.listed("Mine").unwrap().file.is_some());

    store.rename_profile("Old", "New").unwrap();
    let mut again = RotationStore::at(Some(dir.clone()));
    assert!(again.errors.is_empty(), "{:?}", again.errors);
    assert_eq!(
        again.get("mine").unwrap().items[0].source,
        Source::Profile { name: "New".into() },
        "found case-insensitively, renamed on disk"
    );

    let mut edited = again.get("Wolfram series").unwrap();
    edited.description = "edited".into();
    again.save(&edited).unwrap();
    assert_eq!(
        again.names().len(),
        builtin_count + 1,
        "a copy, not a new one"
    );
    assert!(again.listed("Wolfram series").unwrap().builtin);
    assert_eq!(again.get("Wolfram series").unwrap().description, "edited");
    again.delete("Wolfram series").unwrap();
    assert_ne!(again.get("Wolfram series").unwrap().description, "edited");
    assert!(again.delete("Wolfram series").is_err());

    again.delete("Mine").unwrap();
    std::fs::remove_dir(&dir).unwrap();
}
