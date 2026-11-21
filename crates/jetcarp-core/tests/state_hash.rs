//! The state hash (version 2) is the hash of the present state, however the world got there: a
//! world saved as a scene and reopened (its command log replayed) at any generation hashes as the
//! world that ran straight through, and so does every generation after (#173).
use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};
use jetcarp_core::script::{CaRef, Script, ScriptStep, Trigger};
use jetcarp_core::*;

fn scene(tiling: TilingSpec, cas: Vec<CaSpec>) -> Scene {
    let mut s = Scene::new(tiling);
    s.rng_state = 41;
    s.cas = cas;
    s
}

/// Scenes with every kind of state a restore has to bring back.
fn scenes() -> Vec<(&'static str, Scene)> {
    let rect = |f: &str, w, h, b| TilingSpec::rect(f, w, h, b);
    let patch = |f: &str, l| TilingSpec::new(f).with_size(SizeSpec::Level(l));
    let mut script = scene(
        rect("square", 24, 24, Boundary::Wrap),
        vec![
            CaSpec::new("B3/S23").private(),
            CaSpec::new("B36/S23").private(),
        ],
    );
    script.script = Some(Script {
        steps: vec![
            ScriptStep {
                name: "one".into(),
                run: vec![CaRef::Id(1)],
                until: Trigger::Generations { n: 9 },
                max_gens: None,
            },
            ScriptStep {
                name: "two".into(),
                run: vec![CaRef::Id(2)],
                until: Trigger::ActivityBelow {
                    fraction: 0.02,
                    for_gens: 3,
                },
                max_gens: Some(25),
            },
        ],
        repeat: true,
    });
    // A settled policy (it measures every generation's changes while it may fire) on a small
    // torus that settles quickly, and a policy that fires every 11 generations on another CA.
    let mut reseed = scene(
        rect("square", 12, 12, Boundary::Wrap),
        vec![
            CaSpec::new("B3/S23").private(),
            CaSpec::new("B36/S23").private(),
        ],
    );
    reseed.reseed = vec![
        AutoReseed {
            ca: Some(2),
            ..AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 11 })
        },
        AutoReseed {
            ca: Some(1),
            ..AutoReseed::new(
                ReseedMode::rain(),
                ReseedWhen::Settled {
                    activity: 0.05,
                    for_gens: 4,
                    cycle: Some(4),
                    period: Some(2),
                    latest: Some(40),
                },
            )
        },
    ];
    vec![
        ("a script", script),
        ("reseed policies", reseed),
        ("a settled policy with a cooldown", settled_scene()),
        (
            "a turmite",
            scene(
                rect("square", 24, 24, Boundary::Wrap),
                vec![CaSpec::new("Turmite LLRR").seed(SeedSpec::Ants { count: 3 })],
            ),
        ),
        (
            "a B0 rule on a patch",
            scene(patch("hat", 3), vec![CaSpec::new("B0/S6")]),
        ),
        (
            "a B0 rule on a patch whose background does not settle",
            scene(patch("hat", 3), vec![CaSpec::new("B03/S238")]),
        ),
        (
            "a layered rule",
            scene(
                rect("hex", 20, 20, Boundary::Dead),
                vec![CaSpec::new("Layer 3/012")],
            ),
        ),
        (
            "a forest fire",
            scene(
                rect("square", 24, 24, Boundary::Wrap),
                vec![CaSpec::new("ForestFire 0.05/0.001")],
            ),
        ),
    ]
}

const AT: [u64; 7] = [1, 7, 30, 64, 101, 200, 333];
const END: u64 = 400;

/// The world's version 2 hash after each generation up to `END`.
fn straight(sc: &Scene, track_changes: bool) -> Vec<String> {
    let mut w = World::from_scene(sc).unwrap();
    w.set_track_changes(track_changes);
    let mut out = vec![w.hash_hex()];
    for _ in 0..END {
        w.apply(Command::Step { gens: 1 }).unwrap();
        out.push(w.hash_hex());
    }
    out
}

// Saved and reopened at several generations, each world hashes as the one that ran straight
// through, then and at every generation after. A front end that counts every generation's
// changes (as the app does) changes nothing either.
#[test]
fn a_reopened_world_hashes_as_the_unbroken_one() {
    let mut bad = vec![];
    for (what, sc) in scenes() {
        let want = straight(&sc, false);
        if straight(&sc, true) != want {
            bad.push(format!(
                "{what}: counting every generation's changes changes the hash"
            ));
        }
        for g in AT {
            let mut w = World::from_scene(&sc).unwrap();
            w.apply(Command::Step { gens: g }).unwrap();
            let saved = Scene::from_json(&w.to_scene().to_json()).unwrap();
            let mut again = World::from_scene(&saved).unwrap();
            if again.hash_hex() != want[g as usize] {
                bad.push(format!(
                    "{what}: reopened at generation {g}, the hash differs"
                ));
                continue;
            }
            let rec = saved.record.clone().unwrap();
            assert_eq!(rec.hash_version, 2);
            assert!(rec.matches(&again).unwrap(), "{what}: the record at {g}");
            for t in g + 1..=END {
                again.apply(Command::Step { gens: 1 }).unwrap();
                if again.hash_hex() != want[t as usize] {
                    bad.push(format!(
                        "{what}: reopened at generation {g}, the hash differs at {t}"
                    ));
                    break;
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    // The scenes do what they are there for: both policies fire, the script moves on, the ants
    // and the fire are alive.
    let all = scenes();
    let mut w = World::from_scene(&all[1].1).unwrap();
    w.apply(Command::Step { gens: END }).unwrap();
    let fired: Vec<u64> = w.reseed_status().iter().map(|s| s.fired).collect();
    assert!(fired.iter().all(|&f| f > 0), "policies fired {fired:?}");
    let mut w = World::from_scene(&all[0].1).unwrap();
    w.apply(Command::Step { gens: END }).unwrap();
    assert!(w.script_state.started > 0, "the script never moved on");
}

/// A settled policy with a cooldown that looks for a steady cycle of changed counts, on a small
/// torus that settles soon after each reseed.
pub fn settled_scene() -> Scene {
    let mut sc = scene(
        TilingSpec::rect("square", 16, 16, Boundary::Wrap),
        vec![CaSpec::new("B3/S23")],
    );
    sc.reseed = vec![
        AutoReseed::new(
            ReseedMode::rain(),
            ReseedWhen::Settled {
                activity: 0.0,
                for_gens: 6,
                cycle: Some(6),
                period: None,
                latest: Some(400),
            },
        )
        .cooldown(40),
    ];
    sc
}

/// The generations at which the world's policies fired, and its version 2 hash after each
/// generation, over `gens` generations stepped one at a time.
fn reseeds(w: &mut World, gens: u64) -> (Vec<u64>, Vec<String>) {
    let (mut at, mut hashes) = (vec![], vec![]);
    let mut fired = 0;
    for _ in 0..gens {
        w.apply(Command::Step { gens: 1 }).unwrap();
        let f: u64 = w.reseed_status().iter().map(|s| s.fired).sum();
        if f != fired {
            at.push(w.generation);
            fired = f;
        }
        hashes.push(w.hash_hex());
    }
    (at, hashes)
}

// Whether a front end counts every generation's changes must not change when a settled policy
// fires: its decision reads only what the world measures for it.
#[test]
fn settled_policies_fire_alike_whether_changes_are_tracked() {
    let sc = settled_scene();
    let mut off = World::from_scene(&sc).unwrap();
    let mut on = World::from_scene(&sc).unwrap();
    on.set_track_changes(true);
    let (a, ha) = reseeds(&mut off, 600);
    let (b, hb) = reseeds(&mut on, 600);
    assert!(a.len() >= 3, "the policy fired at {a:?}");
    assert_eq!(a, b, "reseeded at {a:?} untracked, at {b:?} tracked");
    assert_eq!(ha, hb);
}

// An old record (no version: version 1, 16 digits) still verifies against its replay, and a
// version 2 record against its own.
#[test]
fn records_verify_in_the_version_they_name() {
    let sc = scene(
        TilingSpec::rect("square", 16, 16, Boundary::Wrap),
        vec![CaSpec::new("B3/S23")],
    );
    let mut w = World::from_scene(&sc).unwrap();
    w.apply(Command::Step { gens: 20 }).unwrap();
    let mut old = w.to_scene();
    let mut rec = old.record.clone().unwrap();
    rec.hash = w.hash_hex_v1();
    rec.hash_version = 1;
    old.record = Some(rec);
    // As a 0.4.2 file has it: no `hash_version` at all.
    let mut v: serde_json::Value = serde_json::from_str(&old.to_json()).unwrap();
    v["record"].as_object_mut().unwrap().remove("hash_version");
    let text = v.to_string();
    assert!(!text.contains("hash_version"), "{text}");
    let back = Scene::from_json(&text).unwrap();
    let rec = back.record.clone().unwrap();
    assert_eq!(rec.hash_version, 1);
    let replayed = World::from_scene(&back).unwrap();
    assert!(rec.matches(&replayed).unwrap());
    assert_eq!(replayed.hash_hex().len(), 64);
    let v2 = w.to_scene().record.unwrap();
    assert_eq!(v2.hash.len(), 64);
    assert!(v2.matches(&replayed).unwrap());
}
