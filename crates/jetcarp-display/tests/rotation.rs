//! Rotations: the player's decisions from made-up observations, then real worlds running through
//! it (the Wolfram series to the bottom of the screen, the settle watch).

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

/// The clock a rotation reads is the moment's local date and time in this system's time zone,
/// whatever it is: its minutes and day are the moment's, moved by a whole number of minutes
/// (the zone's offset), and its weekday is its day's.
#[test]
fn the_clock_is_the_local_date_and_time() {
    for secs in [
        1_000_000_000,
        1_772_953_200,
        1_793_512_800,
        1_795_523_400,
        4_102_444_799,
    ] {
        let c = Clock::at(secs);
        let local = c.day * 86_400 + c.minute as i64 * 60 + secs.rem_euclid(60);
        let offset = local - secs;
        assert!(
            offset.abs() <= 18 * 3600 && offset % 60 == 0,
            "{secs}: {c:?}"
        );
        assert_eq!(c.weekday as i64, (c.day + 3).rem_euclid(7), "{secs}: {c:?}");
        assert!(c.minute < 24 * 60, "{secs}: {c:?}");
    }
    let before = Clock::at(now_secs());
    let now = Clock::local();
    let after = Clock::at(now_secs());
    assert!(now == before || now == after, "{now:?}");
}

/// Seconds since 1970-01-01 UTC now.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Set in a twin's process by [`the_clock_in_these_time_zones`].
#[cfg(unix)]
const TZ_CHILD: &str = "JETCARP_ROTATION_TIME_ZONE_TEST";

/// Moments, and the clock a rotation reads at each in time zone `TZ`: the weekday (0 is Monday),
/// the minutes since local midnight and the days since 1970-01-01, local.
#[cfg(unix)]
const CLOCKS: &[(&str, i64, u8, u32, i64)] = &[
    // Sunday 8 March 2026: 01:59:59 EST, then 03:00 EDT (two to three o'clock never comes).
    ("America/New_York", 1_772_953_199, 6, 119, 20_520),
    ("America/New_York", 1_772_953_200, 6, 180, 20_520),
    // Sunday 1 November 2026: 01:59:59 EDT, then 01:00 EST (one o'clock comes twice).
    ("America/New_York", 1_793_512_799, 6, 119, 20_758),
    ("America/New_York", 1_793_512_800, 6, 60, 20_758),
    // Tuesday 24 November 2026, either side of 07:30 and of 22:00.
    ("America/New_York", 1_795_523_399, 1, 449, 20_781),
    ("America/New_York", 1_795_523_400, 1, 450, 20_781),
    ("America/New_York", 1_795_575_599, 1, 1319, 20_781),
    ("America/New_York", 1_795_575_600, 1, 1320, 20_781),
    // Midnight at the new year, Thursday to Friday; and the last second before 1970 (UTC).
    ("America/New_York", 1_798_779_599, 3, 1439, 20_818),
    ("America/New_York", 1_798_779_600, 4, 0, 20_819),
    ("America/New_York", -1, 2, 1139, -1),
    // Lord Howe Island puts its clocks back half an hour: 01:59:59, then 01:30.
    ("Australia/Lord_Howe", 1_775_314_799, 6, 119, 20_548),
    ("Australia/Lord_Howe", 1_775_314_800, 6, 90, 20_548),
    ("Asia/Kathmandu", 1_793_907_002, 4, 75, 20_763),
    ("UTC", 1_793_907_002, 3, 1170, 20_762),
];

/// The clock in each zone of [`CLOCKS`], each set with `TZ` in a process of its own
/// ([`twin_the_clock_in_one_zone`]), is as it was read before; and in New York, the rotation's
/// time-of-day decisions at those moments: a schedule's window opening and closing on the minute,
/// a time-of-day end firing when its time is skipped and again when it comes twice, and one an
/// hour keeping its item through the hour that comes twice.
#[cfg(unix)]
#[test]
fn the_clock_in_these_time_zones() {
    let mut zones: Vec<&str> = CLOCKS.iter().map(|z| z.0).collect();
    zones.dedup();
    for tz in zones {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "twin_the_clock_in_one_zone",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(TZ_CHILD, "1")
            .env("TZ", tz)
            .output()
            .expect("start the test's own process");
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success() && text.contains("1 passed"),
            "TZ={tz}:\n{text}"
        );
    }
}

#[cfg(unix)]
#[test]
#[ignore = "a twin: the_clock_in_these_time_zones runs it with TZ set"]
fn twin_the_clock_in_one_zone() {
    if std::env::var_os(TZ_CHILD).is_none() {
        return;
    }
    let tz = std::env::var("TZ").unwrap();
    let mut seen = 0;
    for &(_, secs, weekday, minute, day) in CLOCKS.iter().filter(|z| z.0 == tz) {
        let want = Clock {
            weekday,
            minute,
            day,
        };
        assert_eq!(Clock::at(secs), want, "{secs} in {tz}");
        seen += 1;
    }
    assert!(seen > 0, "{tz}");
    if tz != "America/New_York" {
        return;
    }
    // A window from 07:30 to 22:00 opens and closes on the minute; one from 22:00 to 06:00
    // crosses midnight.
    let day = Schedule {
        from: Some("07:30".into()),
        to: Some("22:00".into()),
        ..Default::default()
    };
    let night = Schedule {
        from: Some("22:00".into()),
        to: Some("06:00".into()),
        ..Default::default()
    };
    let open = |s: &Schedule, secs| s.contains(Clock::at(secs));
    assert!(!open(&day, 1_795_523_399) && open(&day, 1_795_523_400));
    assert!(open(&day, 1_795_575_599) && !open(&day, 1_795_575_600));
    assert!(!open(&night, 1_795_575_599) && open(&night, 1_795_575_600));
    assert!(open(&night, 1_798_779_599) && open(&night, 1_798_779_600));
    assert!(open(&night, 1_795_517_999) && !open(&night, 1_795_518_000));

    // An item that ends at a time of day.
    let ends_at = |at: &str, start: i64, ticks: &[(i64, Action)]| {
        let r = rotation(vec![
            scene_item("a").end(End::Clock { at: at.into() }),
            scene_item("b"),
        ]);
        let mut p = player(&r, 1);
        assert_eq!(p.start(Clock::at(start)), Some(0));
        p.on_loaded(0.0, 0, Clock::at(start));
        for (k, &(secs, want)) in ticks.iter().enumerate() {
            let mut o = obs(1.0 + k as f64, 1);
            o.clock = Clock::at(secs);
            assert_eq!(p.tick(&o), want, "{at}, at {secs}");
        }
    };
    // 02:30 never comes on 8 March: the end fires as the clock jumps from 01:59 to 03:00.
    ends_at(
        "02:30",
        1_772_949_600,
        &[
            (1_772_953_199, Action::Stay),
            (1_772_953_200, Action::Load(1)),
        ],
    );
    // 01:30 comes twice on 1 November: the end fires the first time, and, for an item started
    // in between, again the second.
    ends_at(
        "01:30",
        1_793_509_200,
        &[
            (1_793_510_999, Action::Stay),
            (1_793_511_000, Action::Load(1)),
        ],
    );
    ends_at(
        "01:30",
        1_793_512_799,
        &[
            (1_793_512_800, Action::Stay),
            (1_793_514_599, Action::Stay),
            (1_793_514_600, Action::Load(1)),
        ],
    );

    // One an hour: the hour that comes twice is one hour of the clock, and the hour skipped is
    // none.
    let hour = |secs| Clock::at(secs).hour();
    assert_eq!(hour(1_793_512_799), hour(1_793_512_800));
    assert_eq!(hour(1_772_953_200) - hour(1_772_953_199), 2);
    assert_eq!(hour(1_798_779_600) - hour(1_798_779_599), 1);
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
fn one_a_day_starts_a_finished_item_again_instead_of_moving_on() {
    // "A classic a day" has no endings: an ant that leaves the grid, or a world that dies, is
    // done, and starts again once it has run a minute; it never gives way to the next item.
    let mut r = rotation(vec![scene_item("a"), scene_item("b"), scene_item("c")]);
    r.order = Order::Daily;
    let mut p = player(&r, 1);
    let today = begin(&mut p, 0.0);
    assert_eq!(today, 20_000 % 3, "chosen by the date");
    assert!(!p.wants_done(30.0), "not before it has run a minute");
    let mut o = obs(30.0, 10);
    o.done = true;
    assert_eq!(p.tick(&o), Action::Stay);
    assert!(p.wants_done(61.0));
    o.now = 61.0;
    assert_eq!(p.tick(&o), Action::Load(today), "the same item again");
    p.on_loaded(61.0, 0, MONDAY_NOON);
    o.now = 62.0;
    o.done = false;
    assert_eq!(p.tick(&o), Action::Stay);
    // A restarted program lands on the same item.
    assert_eq!(player(&r, 99).start(MONDAY_NOON), Some(today));
}

#[test]
fn one_an_hour_follows_the_clock_and_starts_a_finished_item_again() {
    let n = 7;
    // Each pass of n hours shows every item once, in a new order, and never repeats an item
    // across the boundary between passes.
    let base = 20_000 * 24 / n as i64 * n as i64;
    let slots: Vec<usize> = (base..base + 10 * n as i64)
        .map(|h| hourly_slot(h, n))
        .collect();
    for pass in slots.chunks(n) {
        assert_eq!(pass.iter().collect::<HashSet<_>>().len(), n, "{pass:?}");
    }
    assert!(slots.windows(2).all(|w| w[0] != w[1]), "{slots:?}");
    assert_ne!(slots[..n], slots[n..2 * n], "a new order each pass");
    assert_eq!(
        hourly_slot(base + 3, n),
        slots[3],
        "the clock alone decides"
    );
    assert_eq!(hourly_slot(5, 2), 1);
    assert_eq!(hourly_slot(-1, 1), 0);

    let mut r = rotation((0..n).map(|k| scene_item(&format!("i{k}"))).collect());
    r.order = Order::Hourly;
    let mut p = player(&r, 1);
    let noon = MONDAY_NOON.hour();
    let first = begin(&mut p, 0.0);
    assert_eq!(first, hourly_slot(noon, n));
    assert_eq!(
        player(&r, 42).start(MONDAY_NOON),
        Some(first),
        "after a restart"
    );
    // Finished within the hour: the same item again.
    let mut o = obs(90.0, 10);
    o.done = true;
    assert_eq!(p.tick(&o), Action::Load(first));
    p.on_loaded(90.0, 0, MONDAY_NOON);
    // The next hour: that hour's item.
    let mut o = obs(100.0, 10);
    o.clock.minute = 13 * 60 + 1;
    assert_eq!(p.tick(&o), Action::Load(hourly_slot(noon + 1, n)));
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
    // Quick Start entries by handle, in a library of the tests' own: the built-in rows every
    // library has.
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-rotation-handles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = jetcarp_store::Library::open(&dir);
    let known = |handle: &str| jetcarp_store::quickstart::scene(&lib, handle).is_ok();
    let names: HashSet<String> = profiles::builtins().into_iter().map(|p| p.name).collect();
    let all = builtins();
    assert!(all.len() >= 4);
    for r in all {
        r.validate().unwrap_or_else(|e| panic!("{}: {e}", r.name));
        assert_eq!(Rotation::from_json(&r.to_json()).unwrap(), r, "{}", r.name);
        for it in &r.items {
            match &it.source {
                Source::Profile { name } => assert!(names.contains(name), "{name}"),
                Source::QuickStart { id } => assert!(known(id), "{id}"),
                _ => {}
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
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

/// *Show just this scene*: a rotation of one profile, named after it, made once and found again
/// when pressed again; a name taken by a built-in or by a rotation of your own that shows something
/// else is passed over, and that rotation left alone.
#[test]
fn show_just_this_scene_makes_one_rotation_and_finds_it_again() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-rotations-just-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = RotationStore::at(Some(dir.clone()));
    let builtin_count = store.names().len();
    let name = "JACA rainbow fronts (Cairo)";

    let (r, path) = store.save_just(name, name).unwrap();
    assert_eq!(r.name, name);
    assert_eq!(r.just_profile(), Some(name));
    assert_eq!(
        r.default_minutes, None,
        "it runs until something else moves on"
    );
    assert_eq!(path, dir.join("jaca-rainbow-fronts-cairo.json"));
    assert!(path.exists());
    assert_eq!(
        RotationStore::at(Some(dir.clone())).get(name).as_ref(),
        Some(&r),
        "on disk"
    );

    // Again: the same rotation, no "2".
    let (again, path2) = store.save_just(name, name).unwrap();
    assert_eq!((again.name.as_str(), &path2), (name, &path));
    assert_eq!(store.names().len(), builtin_count + 1);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

    // Edits made to it since are kept.
    let mut edited = again.clone();
    edited.items[0].look.cell_px = Some(2.0);
    store.save(&edited).unwrap();
    let (kept, _) = store.save_just(name, name).unwrap();
    assert_eq!(kept.items[0].look.cell_px, Some(2.0));

    // A rotation of your own of that name that shows more is not touched.
    let mut mine = rotation(vec![
        Item::new(Source::Profile {
            name: "Mine".into(),
        }),
        Item::new(Source::Profile {
            name: "Life".into(),
        }),
    ]);
    mine.name = "Mine".into();
    store.save(&mine).unwrap();
    let (other, _) = store.save_just("Mine", "Mine").unwrap();
    assert_eq!(other.name, "Mine 2");
    assert_eq!(store.get("Mine").unwrap(), mine);
    let (other_again, _) = store.save_just("Mine", "Mine").unwrap();
    assert_eq!(other_again.name, "Mine 2", "found again");

    // Nor is a built-in, and a blank name has a stand-in.
    let (w, _) = store.save_just("Wolfram series", "Wolfram series").unwrap();
    assert_eq!(w.name, "Wolfram series 2");
    assert!(store.listed("Wolfram series").unwrap().file.is_none());
    assert_eq!(store.save_just(" ", "Life").unwrap().0.name, "My scene");

    // Without a settings folder nothing is saved.
    assert!(RotationStore::at(None).save_just(name, name).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
