//! `scene.*`: load, save, verify, and run, with their error kinds.

mod common;

use common::{fails, keys, scratch};
use jetcarp_api::op::{Op, Quiet, Report, to_value};
use jetcarp_api::ops::scene::*;
use jetcarp_api::{Ctx, ErrorKind, call, call_json};
use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
use serde_json::json;

fn life_scene() -> Scene {
    let mut sc = Scene::new(TilingSpec::rect("square", 32, 24, Boundary::Wrap));
    sc.rng_state = 3;
    sc.name = "soup".into();
    sc.cas
        .push(CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }));
    sc
}

fn run_in(scene: &std::path::Path) -> RunIn {
    serde_json::from_value(json!({"scene": scene.display().to_string()})).unwrap()
}

#[test]
fn save_load_and_their_errors() {
    let dir = scratch("scene-save");
    let cx = Ctx::new();
    let path = dir.join("a/b/soup.json");
    let saved = SceneSave::run(
        &cx,
        SaveIn {
            scene: serde_json::to_value(life_scene()).unwrap(),
            path: path.display().to_string(),
        },
        &Quiet,
    )
    .unwrap();
    assert!(path.is_file() && saved.bytes > 100);
    assert!(
        saved.text().starts_with("Saved the scene ("),
        "{}",
        saved.text()
    );
    let from_file = SceneLoad::run(
        &cx,
        SceneSource {
            scene: None,
            path: Some(path.display().to_string()),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(from_file.scene.name, "soup");
    assert_eq!(from_file.recorded, None);
    // The same scene as JSON text and as an object.
    let text = from_file.text();
    for v in [json!(text), serde_json::from_str(&text).unwrap()] {
        let s = SceneLoad::run(
            &cx,
            SceneSource {
                scene: Some(v),
                path: None,
            },
            &Quiet,
        )
        .unwrap();
        assert_eq!(s.scene.cas, from_file.scene.cas);
    }
    let load = |scene: Option<serde_json::Value>, path: Option<&str>| {
        SceneLoad::run(
            &cx,
            SceneSource {
                scene,
                path: path.map(String::from),
            },
            &Quiet,
        )
    };
    fails(load(Some(json!("{}")), Some("x.json")), ErrorKind::Invalid);
    fails(load(None, None), ErrorKind::Invalid);
    let e = fails(load(Some(json!({"tiling": 3})), None), ErrorKind::Invalid);
    assert!(e.message.starts_with("scene JSON: "), "{e}");
    fails(load(Some(json!("not json")), None), ErrorKind::Invalid);
    let missing = dir.join("missing.json");
    let e = fails(
        load(None, Some(missing.to_str().unwrap())),
        ErrorKind::NotFound,
    );
    assert!(e.message.starts_with("could not read "), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_saves_renders_and_verifies() {
    let dir = scratch("scene-run");
    let cx = Ctx::new();
    let input = dir.join("soup.json");
    std::fs::write(&input, life_scene().to_json()).unwrap();
    let mut a = run_in(&input);
    a.gens = 20;
    a.threads = Some(2);
    a.save = Some(dir.join("out/after.json").display().to_string());
    a.png = Some(dir.join("out/after.png").display().to_string());
    a.max_side = 200;
    let r = SceneRun::run(&cx, a, &Quiet).unwrap();
    assert_eq!((r.generation, r.threads), (20, 2));
    assert_eq!(r.backend, "cpu (2 threads)");
    let (w, h) = r.png_size.unwrap();
    assert!(w.max(h) <= 200 && w > 0);
    assert!(dir.join("out/after.png").is_file());
    let t = r.text();
    for line in [
        format!("scene:      {}", input.display()),
        "generation: 20".into(),
        format!("hash:       {}", r.hash),
        "time:       ".into(),
        format!("saved:      {}", dir.join("out/after.json").display()),
    ] {
        assert!(t.contains(&line), "{line} in\n{t}");
    }
    assert!(r.check().is_ok());
    // `jetcarp run --json` printed these keys before the operations existed.
    assert_eq!(
        keys(&to_value(&r)),
        [
            "backend",
            "cells",
            "dead_boundary",
            "done",
            "done_reason",
            "fields",
            "generation",
            "hash",
            "jetcarp_version",
            "png",
            "png_size",
            "reseed",
            "scene",
            "seconds",
            "status",
            "threads",
            "tiling"
        ]
    );
    // The saved scene replays to the run's hash; a changed record does not.
    let saved = dir.join("out/after.json");
    let v = SceneVerify::run(
        &cx,
        SceneSource {
            scene: None,
            path: Some(saved.display().to_string()),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((v.generation, v.hash.clone()), (20, r.hash.clone()));
    assert_eq!(v.text(), format!("ok: generation 20 hash {}\n", r.hash));
    let mut tampered: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
    tampered["record"]["hash"] = json!("0000000000000000");
    let e = fails(
        SceneVerify::run(
            &cx,
            SceneSource {
                scene: Some(tampered),
                path: None,
            },
            &Quiet,
        ),
        ErrorKind::Conflict,
    );
    assert!(
        e.message.starts_with("MISMATCH: recorded gen 20 hash 0000"),
        "{e}"
    );
    let e = fails(
        SceneVerify::run(
            &cx,
            SceneSource {
                scene: None,
                path: Some(input.display().to_string()),
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    assert_eq!(e.message, "scene has no recorded hash");
    // The thread count does not change the result.
    let mut one = run_in(&input);
    one.gens = 20;
    one.threads = Some(1);
    assert_eq!(SceneRun::run(&cx, one, &Quiet).unwrap().hash, r.hash);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_checks_expectations_reseeds_and_falls_back_to_the_cpu() {
    let dir = scratch("scene-expect");
    let cx = Ctx::new();
    let input = dir.join("soup.json");
    std::fs::write(&input, life_scene().to_json()).unwrap();
    // A wrong --expect: the summary is still made, and its check fails.
    let mut a = run_in(&input);
    a.gens = 5;
    a.expect = Some("deadbeef".into());
    let r = SceneRun::run(&cx, a, &Quiet).unwrap();
    let e = fails(r.check(), ErrorKind::Conflict);
    assert_eq!(
        e.message,
        format!("hash mismatch: expected deadbeef, got {}", r.hash)
    );
    // By name, the failed check is the call's error, and the output still comes with it.
    let input_json = json!({"scene": input.display().to_string(), "gens": 5, "expect": "deadbeef"});
    fails(
        call_json(&cx, "scene.run", input_json.clone()),
        ErrorKind::Conflict,
    );
    let c = call(&cx, "scene.run", input_json, &Quiet).unwrap();
    assert_eq!(c.value["hash"], r.hash);
    assert!(c.check.is_err());
    let mut ok = run_in(&input);
    ok.gens = 5;
    ok.expect = Some(r.hash.clone());
    assert!(SceneRun::run(&cx, ok, &Quiet).unwrap().check().is_ok());
    // Reseed policies and a reseed now.
    let mut a = run_in(&input);
    a.gens = 30;
    a.reseed = Some(json!({"mode": "rain", "when": {"kind": "every", "gens": 10}}));
    a.reseed_now = Some(json!("blobs"));
    let r = SceneRun::run(&cx, a, &Quiet).unwrap();
    assert_eq!(r.reseed.as_array().unwrap().len(), 1);
    assert_eq!(r.reseed[0]["status"]["fired"], 3);
    assert!(
        r.text()
            .contains("reseed:     rain every 10: fired 3 times")
    );
    let mut bad = run_in(&input);
    bad.reseed = Some(json!({"mode": "hail"}));
    let e = fails(SceneRun::run(&cx, bad, &Quiet), ErrorKind::Invalid);
    assert!(e.message.starts_with("not a reseed policy: "), "{e}");
    // No GPU without a platform: the run says so and steps on the CPU.
    let mut g = run_in(&input);
    g.gens = 3;
    g.gpu = Some(true);
    let r = SceneRun::run(&cx, g, &Quiet).unwrap();
    assert!(r.backend.contains("; no GPU: "), "{}", r.backend);
    assert_eq!(r.generation, 3);
    // A missing scene.
    let e = fails(
        SceneRun::run(&cx, run_in(&dir.join("nope.json")), &Quiet),
        ErrorKind::NotFound,
    );
    assert!(e.message.contains("nope.json"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}
