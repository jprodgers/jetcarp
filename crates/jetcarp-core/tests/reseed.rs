//! Reseeding: manual and automatic, every mode, several tilings, determinism, and scenes.

use jetcarp_core::reseed::{
    self, AutoReseed, EDIT_BIRTH, EDIT_FLIP, EDIT_KILL, EDIT_LIVE, ReseedMode, ReseedWhen,
};
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::FieldKind;
use jetcarp_core::world::cell_size;
use jetcarp_core::*;

fn life_world(tiling: TilingSpec, density: f64) -> World {
    let mut s = Scene::new(tiling);
    s.rng_state = 11;
    s.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density })];
    World::from_scene(&s).unwrap()
}

fn settled(mode: ReseedMode) -> AutoReseed {
    AutoReseed::new(
        mode,
        ReseedWhen::Settled {
            activity: 0.003,
            for_gens: 40,
            cycle: Some(12),
            period: Some(6),
            latest: None,
        },
    )
    .cooldown(60)
}

#[test]
fn settled_life_soup_keeps_running() {
    let mut w = life_world(TilingSpec::rect("square", 128, 128, Boundary::Wrap), 0.3);
    w.apply(Command::SetReseed {
        policies: vec![settled(ReseedMode::Blobs {
            count: 2,
            min_radius: 0.08,
            max_radius: 0.2,
            live: 0.4,
            sign: Default::default(),
        })],
    })
    .unwrap();
    let mut dead_chunks = 0;
    let mut last_fired = 0;
    for _ in 0..40 {
        w.apply(Command::Step { gens: 400 }).unwrap();
        let st = &w.reseed_status()[0];
        if w.fields[0].population() == 0 && st.fired == last_fired {
            dead_chunks += 1;
        }
        last_fired = st.fired;
    }
    let st = &w.reseed_status()[0];
    eprintln!(
        "fired {} times, last at {:?}, population {}",
        st.fired,
        st.last_fired,
        w.fields[0].population()
    );
    assert!(
        st.fired >= 3,
        "only {} reseeds in 16,000 generations",
        st.fired
    );
    assert_eq!(dead_chunks, 0, "the world stayed dead");
    assert!(st.error.is_none());
    // The activity history fills while the settled policy measures.
    assert!(!w.activity_history().is_empty());
    assert!(w.activity_history().len() <= jetcarp_core::world::ACTIVITY_HISTORY);
}

#[test]
fn same_hash_at_one_and_all_threads_and_after_reload() {
    let run = || {
        let mut w = life_world(TilingSpec::rect("square", 96, 80, Boundary::Wrap), 0.3);
        w.apply(Command::SetReseed {
            policies: vec![
                settled(ReseedMode::blobs()),
                AutoReseed::new(
                    ReseedMode::Rain {
                        drops: 3,
                        min_size: 2.0,
                        max_size: 6.0,
                        density: 0.5,
                        sign: Default::default(),
                    },
                    ReseedWhen::Every { gens: 25 },
                ),
            ],
        })
        .unwrap();
        w.apply(Command::Step { gens: 300 }).unwrap();
        w.apply(Command::Reseed {
            ca: None,
            mode: ReseedMode::Random {
                flip: 0.1,
                sign: Default::default(),
            },
        })
        .unwrap();
        w.apply(Command::Step { gens: 900 }).unwrap();
        w
    };
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(run);
    let all = run();
    assert_eq!(one.hash_hex(), all.hash_hex());
    assert!(all.reseed_status()[1].fired >= 30);
    // The scene (base policies plus the logged commands) replays to the same state.
    let json = all.to_scene().to_json();
    let back = World::from_scene(&Scene::from_json(&json).unwrap()).unwrap();
    assert_eq!(back.hash_hex(), all.hash_hex());
    assert_eq!(back.reseed_status(), all.reseed_status());
    // Policies installed in the scene itself (not by a command) round-trip too.
    let mut sc = Scene::new(TilingSpec::rect("square", 40, 40, Boundary::Wrap));
    sc.cas = vec![CaSpec::new("B3/S23")];
    sc.reseed = vec![settled(ReseedMode::rain())];
    let text = sc.to_json();
    assert!(text.contains("\"reseed\""));
    let sc2 = Scene::from_json(&text).unwrap();
    assert_eq!(sc2.reseed, sc.reseed);
    let mut a = World::from_scene(&sc2).unwrap();
    a.apply(Command::Step { gens: 500 }).unwrap();
    let saved = a.to_scene();
    assert_eq!(saved.reseed, sc.reseed);
    let b = World::from_scene(&saved).unwrap();
    assert_eq!(a.hash_hex(), b.hash_hex());
    // Reset keeps the policies.
    a.apply(Command::Reset).unwrap();
    assert_eq!(a.reseed_policies().len(), 1);
}

#[test]
fn random_flip_rate() {
    let topo = Topology::build(&TilingSpec::rect("square", 200, 200, Boundary::Wrap)).unwrap();
    let mut rng = Rng::new(5);
    for flip in [0.01, 0.1, 0.5] {
        let e = reseed::plan(
            &topo,
            1.0,
            FieldKind::Binary,
            &ReseedMode::Random {
                flip,
                sign: Default::default(),
            },
            &mut rng,
        )
        .unwrap();
        let rate = e.len() as f64 / topo.len() as f64;
        assert!(
            (rate - flip).abs() < 0.01 + flip * 0.05,
            "flip {flip}: {rate}"
        );
        assert!(e.cells().all(|(_, op)| op == EDIT_FLIP));
    }
    // Flipping a field: dead becomes live, live (and dying) becomes dead.
    let mut w = life_world(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 0.5);
    let before = w.fields[0].state.clone();
    w.apply(Command::Reseed {
        ca: Some(1),
        mode: ReseedMode::Random {
            flip: 1.0,
            sign: Default::default(),
        },
    })
    .unwrap();
    for (a, b) in before.iter().zip(&w.fields[0].state) {
        assert_eq!(*a == 0, *b == 1);
    }
}

/// Births per drop stay inside a disc `max_size` tiles across, and drops only add cells.
fn rain_checks(tiling: TilingSpec, what: &str) {
    let topo = Topology::build(&tiling).unwrap();
    let tile = cell_size(&topo);
    let mut rng = Rng::new(9);
    let (min_size, max_size) = (3.0, 9.0);
    let mut counts = vec![];
    for _ in 0..40 {
        let e = reseed::plan(
            &topo,
            tile,
            FieldKind::Binary,
            &ReseedMode::Rain {
                drops: 1,
                min_size,
                max_size,
                density: 1.0,
                sign: Default::default(),
            },
            &mut rng,
        )
        .unwrap();
        assert!(
            e.cells().all(|(_, op)| op == EDIT_BIRTH),
            "{what}: births only"
        );
        let pts: Vec<_> = e.cells().map(|(c, _)| topo.centroid(c)).collect();
        if pts.is_empty() {
            continue;
        }
        // Skip drops that wrap across an edge (their centroids are far apart in the plane).
        let b = topo.bounds;
        let wraps = pts.iter().any(|p| p[0] - b[0] < max_size * tile)
            || pts.iter().any(|p| b[2] - p[0] < max_size * tile)
            || pts.iter().any(|p| p[1] - b[1] < max_size * tile)
            || pts.iter().any(|p| b[3] - p[1] < max_size * tile);
        if !wraps {
            let mut far: f64 = 0.0;
            for p in &pts {
                for q in &pts {
                    far = far.max(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt());
                }
            }
            assert!(far <= max_size * tile + 1e-6, "{what}: drop {far} across");
        }
        counts.push(pts.len() as f64);
    }
    // Areas: between a min-size disc and a max-size disc, give or take a ring of tiles.
    let disc = |s: f64| std::f64::consts::PI * (s / 2.0) * (s / 2.0);
    let mean = counts.iter().sum::<f64>() / counts.len() as f64;
    assert!(
        mean > disc(min_size) * 0.6 && mean < disc(max_size) * 1.3,
        "{what}: mean drop {mean} cells"
    );
    // Rain never kills: every live cell before is live after.
    let mut s = Scene::new(tiling);
    s.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 })];
    let mut w = World::from_scene(&s).unwrap();
    let before = w.fields[0].state.clone();
    w.apply(Command::Reseed {
        ca: None,
        mode: ReseedMode::rain(),
    })
    .unwrap();
    let after = &w.fields[0].state;
    assert!(before.iter().zip(after).all(|(a, b)| *a == 0 || *b == 1));
    assert!(w.fields[0].population() > before.iter().filter(|&&x| x != 0).count() as u64);
}

/// The live fraction inside blobs is near `live`, and the outline is not a disc.
fn blob_checks(tiling: TilingSpec, what: &str) {
    let topo = Topology::build(&tiling).unwrap();
    let tile = cell_size(&topo);
    let mut rng = Rng::new(21);
    let mut lumpy = 0;
    for live in [0.2, 0.5] {
        for _ in 0..6 {
            let e = reseed::plan(
                &topo,
                tile,
                FieldKind::Binary,
                &ReseedMode::Blobs {
                    count: 1,
                    min_radius: 0.15,
                    max_radius: 0.25,
                    live,
                    sign: Default::default(),
                },
                &mut rng,
            )
            .unwrap();
            assert!(e.len() > 50, "{what}: blob of {} cells", e.len());
            let on = e.cells().filter(|&(_, op)| op == EDIT_LIVE).count() as f64;
            assert!(e.cells().all(|(_, op)| op == EDIT_LIVE || op == EDIT_KILL));
            let frac = on / e.len() as f64;
            assert!((frac - live).abs() < 0.08, "{what}: live {frac} for {live}");
            // Outline: the farthest cell in each of 16 directions from the blob's mean.
            let pts: Vec<_> = e.cells().map(|(c, _)| topo.centroid(c)).collect();
            let n = pts.len() as f64;
            let m = [
                pts.iter().map(|p| p[0]).sum::<f64>() / n,
                pts.iter().map(|p| p[1]).sum::<f64>() / n,
            ];
            let mut reach = [0f64; 16];
            for p in &pts {
                let (dx, dy) = (p[0] - m[0], p[1] - m[1]);
                let a = dy.atan2(dx) + std::f64::consts::PI;
                let k = ((a / std::f64::consts::TAU * 16.0) as usize).min(15);
                reach[k] = reach[k].max((dx * dx + dy * dy).sqrt());
            }
            let (lo, hi) = reach
                .iter()
                .fold((f64::MAX, 0f64), |(a, b), &x| (a.min(x), b.max(x)));
            if hi / lo.max(1e-9) > 1.15 {
                lumpy += 1;
            }
        }
    }
    assert!(lumpy >= 8, "{what}: only {lumpy} of 12 blobs are lumpy");
}

#[test]
fn modes_on_square_hex_archimedean_and_aperiodic() {
    let tilings = [
        (
            TilingSpec::rect("square", 160, 120, Boundary::Wrap),
            "square",
        ),
        (TilingSpec::rect("hex", 120, 100, Boundary::Dead), "hex"),
        (TilingSpec::rect("trihex", 60, 60, Boundary::Wrap), "trihex"),
        (TilingSpec::rect("snubsq", 40, 40, Boundary::Dead), "snubsq"),
        (TilingSpec::new("hat").with_size(SizeSpec::Level(4)), "hat"),
    ];
    for (t, what) in tilings {
        rain_checks(t.clone(), what);
        blob_checks(t.clone(), what);
        // An automatic policy runs on each.
        let mut w = life_world(t, 0.25);
        w.apply(Command::SetReseed {
            policies: vec![AutoReseed::new(
                ReseedMode::rain(),
                ReseedWhen::Every { gens: 10 },
            )],
        })
        .unwrap();
        w.apply(Command::Step { gens: 55 }).unwrap();
        assert_eq!(w.reseed_status()[0].fired, 5, "{what}");
    }
}

#[test]
fn blobs_wrap_across_edges() {
    // A blob centred near a wrap edge covers cells on both sides.
    let topo = Topology::build(&TilingSpec::rect("square", 100, 100, Boundary::Wrap)).unwrap();
    let mut rng = Rng::new(1);
    let mut both = false;
    for _ in 0..40 {
        let e = reseed::plan(
            &topo,
            1.0,
            FieldKind::Binary,
            &ReseedMode::Blobs {
                count: 1,
                min_radius: 0.2,
                max_radius: 0.2,
                live: 1.0,
                sign: Default::default(),
            },
            &mut rng,
        )
        .unwrap();
        let xs: Vec<f64> = e.cells().map(|(c, _)| topo.centroid(c)[0]).collect();
        if xs.iter().any(|&x| x < 3.0) && xs.iter().any(|&x| x > 97.0) {
            both = true;
            // Wrapped, not a band across the whole grid.
            assert!(e.len() < 100 * 100 / 4);
        }
    }
    assert!(both, "no blob wrapped across the left and right edges");
}

#[test]
fn generations_and_other_fields() {
    // Brian's Brain: flips take dying cells (state 2) to dead, dead to state 1.
    let mut s = Scene::new(TilingSpec::rect("square", 80, 80, Boundary::Wrap));
    s.cas = vec![CaSpec::new("B2/S/C3").seed(SeedSpec::Random { density: 0.2 })];
    let mut w = World::from_scene(&s).unwrap();
    w.apply(Command::Step { gens: 5 }).unwrap();
    let before = w.fields[0].state.clone();
    assert!(before.contains(&2));
    w.apply(Command::Reseed {
        ca: Some(1),
        mode: ReseedMode::Random {
            flip: 1.0,
            sign: Default::default(),
        },
    })
    .unwrap();
    for (a, b) in before.iter().zip(&w.fields[0].state) {
        assert_eq!(*b, (*a == 0) as u8);
    }
    // Brian's Brain never settles by itself in a small torus? Either way the policy keeps it alive.
    w.apply(Command::SetReseed {
        policies: vec![settled(ReseedMode::blobs())],
    })
    .unwrap();
    w.apply(Command::Step { gens: 1500 }).unwrap();
    assert!(w.fields[0].population() > 0 || w.reseed_status()[0].fired > 0);
    // Sandpiles are refused; Lenia takes values in (0, 1].
    let mut s = Scene::new(TilingSpec::rect("square", 40, 40, Boundary::Dead));
    s.cas = vec![CaSpec::new("Sandpile")];
    let mut w = World::from_scene(&s).unwrap();
    let e = w
        .apply(Command::Reseed {
            ca: Some(1),
            mode: ReseedMode::rain(),
        })
        .unwrap_err();
    assert!(e.contains("sandpile"), "{e}");
    let mut s = Scene::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap));
    s.cas = vec![CaSpec::new("Lenia").seed(SeedSpec::Empty)];
    let mut w = World::from_scene(&s).unwrap();
    w.apply(Command::Reseed {
        ca: Some(1),
        mode: ReseedMode::Blobs {
            count: 1,
            min_radius: 0.2,
            max_radius: 0.3,
            live: 0.5,
            sign: Default::default(),
        },
    })
    .unwrap();
    let f = &w.fields[0];
    assert!(f.value.iter().any(|&v| v > 0.0));
    assert!(f.value.iter().all(|&v| (0.0..=1.0).contains(&v)));
}

#[test]
fn every_and_cooldown_timing() {
    let mut w = life_world(TilingSpec::rect("square", 32, 32, Boundary::Wrap), 0.3);
    w.apply(Command::SetReseed {
        policies: vec![
            AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 7 }),
            AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 3 }).cooldown(10),
            AutoReseed {
                enabled: false,
                ..AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 1 })
            },
        ],
    })
    .unwrap();
    w.apply(Command::Step { gens: 70 }).unwrap();
    let st = w.reseed_status();
    assert_eq!(st[0].fired, 10);
    assert_eq!(st[0].last_fired, Some(70));
    assert_eq!(st[1].fired, 7);
    assert_eq!(st[2].fired, 0);
    // A manual reseed restarts every policy's count.
    w.apply(Command::Reseed {
        ca: None,
        mode: ReseedMode::Random {
            flip: 0.01,
            sign: Default::default(),
        },
    })
    .unwrap();
    w.apply(Command::Step { gens: 6 }).unwrap();
    assert_eq!(w.reseed_status()[0].fired, 10);
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert_eq!(w.reseed_status()[0].fired, 11);
    // A bad policy is refused.
    let bad = AutoReseed::new(
        ReseedMode::Random {
            flip: 2.0,
            sign: Default::default(),
        },
        ReseedWhen::Every { gens: 1 },
    );
    assert!(
        w.apply(Command::SetReseed {
            policies: vec![bad]
        })
        .is_err()
    );
    // Policies targeting a missing CA record an error and keep their cooldown.
    w.apply(Command::SetReseed {
        policies: vec![
            AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 2 }).target(99),
        ],
    })
    .unwrap();
    w.apply(Command::Step { gens: 4 }).unwrap();
    let st = &w.reseed_status()[0];
    assert_eq!(st.fired, 2);
    assert!(st.error.as_deref().unwrap_or("").contains("99"));
}

#[test]
fn json_forms() {
    let c: Command =
        serde_json::from_str(r#"{"op":"reseed","mode":{"kind":"blobs","count":2,"live":0.3}}"#)
            .unwrap();
    assert!(matches!(
        c,
        Command::Reseed {
            ca: None,
            mode: ReseedMode::Blobs { count: 2, .. }
        }
    ));
    let c: Command = serde_json::from_str(
        r#"{"op":"set_reseed","policies":[{"mode":{"kind":"rain"},"when":{"kind":"settled"},"cooldown":100}]}"#,
    )
    .unwrap();
    let Command::SetReseed { policies } = c else {
        panic!()
    };
    assert_eq!(policies[0].cooldown, 100);
    assert!(policies[0].enabled);
    assert!(matches!(
        policies[0].when,
        ReseedWhen::Settled {
            for_gens: 60,
            period: None,
            cycle: Some(12),
            ..
        }
    ));
    // Bare names, and every field surviving a round trip (cycle switched off included).
    let p: AutoReseed =
        serde_json::from_str(r#"{"mode":"blobs","when":"settled","ca":2}"#).unwrap();
    assert_eq!(p.mode, ReseedMode::blobs());
    assert_eq!(p.when, ReseedWhen::settled());
    let mut p = p;
    p.when = ReseedWhen::Settled {
        activity: 0.01,
        for_gens: 30,
        cycle: None,
        period: Some(2),
        latest: Some(900),
    };
    let back: AutoReseed = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
}

/// Timing on a 25-million-cell grid (run with `--ignored --nocapture`).
#[test]
#[ignore]
fn big_world_reseed_timing() {
    let mut s = Scene::new(TilingSpec::rect("square", 5000, 5000, Boundary::Wrap));
    s.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 })];
    let mut w = World::from_scene(&s).unwrap();
    for mode in [
        ReseedMode::Random {
            flip: 0.05,
            sign: Default::default(),
        },
        ReseedMode::rain(),
        ReseedMode::Blobs {
            count: 2,
            min_radius: 0.15,
            max_radius: 0.2,
            live: 0.4,
            sign: Default::default(),
        },
    ] {
        let t = std::time::Instant::now();
        w.apply(Command::Reseed {
            ca: None,
            mode: mode.clone(),
        })
        .unwrap();
        eprintln!("{}: {:?}", mode.name(), t.elapsed());
    }
    let t = std::time::Instant::now();
    let p = Topology::build(&TilingSpec::new("hat").with_size(SizeSpec::Level(6))).unwrap();
    eprintln!("hat level 6: {} cells, built in {:?}", p.len(), t.elapsed());
    let mut rng = Rng::new(1);
    let tile = cell_size(&p);
    for mode in [ReseedMode::rain(), ReseedMode::blobs()] {
        let t = std::time::Instant::now();
        let e = reseed::plan(&p, tile, FieldKind::Binary, &mode, &mut rng).unwrap();
        eprintln!(
            "hat {}: {} edits in {:?}",
            mode.name(),
            e.len(),
            t.elapsed()
        );
    }
}

/// `latest`: a settled policy fires at a checkpoint once that many generations have passed, even
/// while frequent rain keeps the settled measures from ever holding (the screensaver's guarantee).
#[test]
fn settled_policies_fire_by_latest_under_rain() {
    let mut w = life_world(TilingSpec::rect("square", 96, 80, Boundary::Wrap), 0.3);
    let blobs = AutoReseed::new(
        ReseedMode::blobs(),
        ReseedWhen::Settled {
            activity: 0.0,
            for_gens: 60,
            cycle: None,
            period: None,
            latest: Some(400),
        },
    )
    .cooldown(100);
    let rain = AutoReseed::new(
        ReseedMode::Rain {
            drops: 4,
            min_size: 2.0,
            max_size: 6.0,
            density: 0.5,
            sign: Default::default(),
        },
        ReseedWhen::Every { gens: 10 },
    );
    w.apply(Command::SetReseed {
        policies: vec![blobs, rain],
    })
    .unwrap();
    w.apply(Command::Step { gens: 4000 }).unwrap();
    let st = &w.reseed_status()[0];
    // Every 400 generations, rounded up to the 16-generation checkpoint (400 is a multiple).
    assert_eq!(st.fired, 10, "{st:?}");
    assert_eq!(st.last_fired, Some(4000));
    assert!(st.last_fired.unwrap().is_multiple_of(reseed::SETTLE_CHECK));
    // Zero is refused.
    let bad = AutoReseed::new(
        ReseedMode::blobs(),
        ReseedWhen::Settled {
            activity: 0.002,
            for_gens: 60,
            cycle: Some(12),
            period: None,
            latest: Some(0),
        },
    );
    assert!(bad.validate().is_err());
}
