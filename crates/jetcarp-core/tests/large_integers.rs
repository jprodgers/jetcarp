//! Integers beyond 2^53 in JSON: a scene's random state, a jump's generations and a find's soup
//! state are decimal strings when they are that large, read back either way, and give the same
//! world. Canonical JSON refuses them as numbers.

use jetcarp_core::pattern::Provenance;
use jetcarp_core::{Boundary, CaSpec, Command, Scene, TilingSpec, World, jcs};

const BIG: u64 = 17705299566207515059;

fn scene(rng: u64) -> Scene {
    let mut s = Scene::new(TilingSpec::rect("hex", 30, 30, Boundary::Wrap));
    s.rng_state = rng;
    s.cas.push(CaSpec::new("B2/S34H"));
    s.commands.push(Command::Step { gens: 5 });
    s
}

fn world_after(s: &Scene, gens: u32) -> String {
    let mut w = World::from_scene(s).unwrap();
    for _ in 0..gens {
        w.step();
    }
    w.hash_hex()
}

#[test]
fn a_scenes_random_state_is_a_string_beyond_2_53() {
    let s = scene(BIG);
    let text = s.to_json();
    assert!(
        text.contains(r#""rng_state": "17705299566207515059""#),
        "{text}"
    );
    // A small one stays a number, so scenes as they have always been written read the same.
    assert!(scene(42).to_json().contains(r#""rng_state": 42"#));
    // Read back, as written and as older files wrote it (a number).
    let back = Scene::from_json(&text).unwrap();
    assert_eq!(back, s);
    let old = text.replace(r#""17705299566207515059""#, "17705299566207515059");
    assert_eq!(Scene::from_json(&old).unwrap(), s);
    // The same world and state hash, whichever way it was read.
    assert_eq!(world_after(&back, 30), world_after(&s, 30));
    // Its canonical JSON holds the digits exactly.
    let canon = jcs::to_string(&serde_json::to_value(&s).unwrap()).unwrap();
    assert!(
        canon.contains(r#""rng_state":"17705299566207515059""#),
        "{canon}"
    );
}

#[test]
fn canonical_json_refuses_a_large_integer_number() {
    // What a scene was before (its state a number in a JSON value): refused, naming it.
    let mut v = serde_json::to_value(scene(BIG)).unwrap();
    v["rng_state"] = serde_json::json!(BIG);
    let e = jcs::to_string(&v).unwrap_err();
    assert!(e.to_string().contains("17705299566207515059"), "{e}");
}

#[test]
fn every_large_integer_in_a_scene_reads_back() {
    let mut s = scene(u64::MAX);
    s.commands = vec![
        Command::SetRngState { state: BIG },
        Command::Jump { gens: 1 << 60 },
        Command::Step {
            gens: (1 << 53) + 1,
        },
    ];
    s.record = Some(jetcarp_core::world::Record {
        generation: (1 << 60) + 7,
        hash: "x".into(),
        jetcarp_version: "0".into(),
        dead_boundary: false,
    });
    let text = s.to_json();
    for n in [
        "18446744073709551615",
        "17705299566207515059",
        "1152921504606846976",
        "9007199254740993",
        "1152921504606846983",
    ] {
        assert!(
            text.contains(&format!("\"{n}\"")),
            "{n} not a string in {text}"
        );
    }
    assert_eq!(Scene::from_json(&text).unwrap(), s);
    jcs::to_string(&serde_json::to_value(&s).unwrap()).unwrap();
    // A find's provenance too.
    let p = Provenance {
        source: "search".into(),
        rng_state: Some(BIG),
        soup: Some(3),
        ..Default::default()
    };
    let t = serde_json::to_string(&p).unwrap();
    assert!(t.contains(r#""rng_state":"17705299566207515059""#), "{t}");
    assert_eq!(serde_json::from_str::<Provenance>(&t).unwrap(), p);
    let old = t.replace(r#""17705299566207515059""#, "17705299566207515059");
    assert_eq!(serde_json::from_str::<Provenance>(&old).unwrap(), p);
}
