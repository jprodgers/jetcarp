//! `world.*` on a headless session: happy paths, limits, and error kinds.

mod common;

use common::{fails, keys, scratch};
use jetcarp_api::op::{Callbacks, Quiet, Report, WorldOp, to_value};
use jetcarp_api::ops::world::*;
use jetcarp_api::world::{Host, Session};
use jetcarp_api::{Ctx, ErrorKind, Limits};
use jetcarp_core::library::Library;
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{DiscoveredBy, Provenance, record_from_object};
use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn new_world(cx: &Ctx, h: &mut Session, v: Value) -> NewWorldOut {
    WorldNew::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet).unwrap()
}

fn draw(cx: &Ctx, h: &mut Session, v: Value) -> Result<Drawn, jetcarp_api::ApiError> {
    WorldDraw::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet)
}

#[test]
fn glider_draw_step_read() {
    let cx = Ctx::interactive();
    let mut h = Session::new();
    let made = new_world(
        &cx,
        &mut h,
        json!({"tiling": "square", "width": 30, "height": 30,
               "cas": [{"rule": "Life", "seed": "empty"}]}),
    );
    assert!(made.text().starts_with("New world created.\n{"));
    assert_eq!(made.status.cas[0].rule, "B3/S23");
    assert_eq!(made.status.tiling.grid["width"], 30);
    let d = draw(
        &cx,
        &mut h,
        json!({"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}),
    )
    .unwrap();
    assert_eq!((d.count, d.pattern_box), (5, Some([5, 5, 7, 7])));
    let t = d.text();
    assert!(t.starts_with("Set 5 cell(s) in CA '"), "{t}");
    assert!(
        t.ends_with("(id 1). Pattern box: x 5..7, y 5..7. Field population now 5. Generation 0."),
        "{t}"
    );
    let s = WorldStep::run(
        &cx,
        &mut h,
        StepIn {
            generations: Some(8),
            stop_when_settled: false,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((s.ran, s.generation, s.stopped.clone()), (8, 8, None));
    assert!(
        s.text().starts_with("Ran 8 generation(s) in "),
        "{}",
        s.text()
    );
    let r = WorldRead::run(&cx, &mut h, ReadIn::default(), &Quiet).unwrap();
    assert_eq!(r.bounding_box, Some([7, 7, 9, 9]));
    assert!(
        r.text().contains("bounding box x 7..9, y 7..9"),
        "{}",
        r.text()
    );
    let list = WorldRead::run(
        &cx,
        &mut h,
        ReadIn {
            format: Some("list".into()),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(list.cells.as_ref().unwrap().len(), 5);
    // RLE in a fresh world reaches the same state.
    let hash = h.world().hash_hex();
    new_world(
        &cx,
        &mut h,
        json!({"width": 30, "height": 30, "cas": [{"rule": "B3/S23", "seed": "empty"}]}),
    );
    draw(&cx, &mut h, json!({"rle": "bo$2bo$3o!", "x": 5, "y": 5})).unwrap();
    h.exec(Command::Step { gens: 8 }).unwrap();
    assert_eq!(h.world().hash_hex(), hash);
}

#[test]
fn drawing_errors_name_their_kinds() {
    let cx = Ctx::interactive();
    let mut h = Session::new();
    // No CAs yet.
    fails(
        draw(&cx, &mut h, json!({"pattern": "O"})),
        ErrorKind::Invalid,
    );
    new_world(
        &cx,
        &mut h,
        json!({"width": 20, "height": 20, "boundary": "dead",
               "cas": [{"rule": "Life", "seed": "empty"}]}),
    );
    for (v, kind, needle) in [
        (
            json!({"cells": [[25, 3]]}),
            ErrorKind::Invalid,
            "outside the grid",
        ),
        (
            json!({"cells": [[1, 1, 2]]}),
            ErrorKind::Invalid,
            "too high",
        ),
        (
            json!({"pattern": "O?"}),
            ErrorKind::Invalid,
            "not understood",
        ),
        (
            json!({"ca": 9, "cells": [[1, 1]]}),
            ErrorKind::NotFound,
            "no CA 9",
        ),
        (json!({}), ErrorKind::Invalid, "nothing to draw"),
        (
            json!({"cells": [[1, 1]], "x": 2}),
            ErrorKind::Invalid,
            "x and y place",
        ),
    ] {
        let e = fails(draw(&cx, &mut h, v.clone()), kind);
        assert!(e.message.contains(needle), "{v}: {e}");
    }
    // A CA by name, and erasing.
    draw(
        &cx,
        &mut h,
        json!({"ca": "life", "cells": [[1, 1], [2, 2]]}),
    )
    .unwrap();
    let d = draw(&cx, &mut h, json!({"cells": [[1, 1]], "erase": true})).unwrap();
    assert_eq!(d.population, 1);
    // The limits are the context's.
    let small = Ctx::new().with_limits(Limits {
        max_set: 2,
        ..Limits::NONE
    });
    let e = fails(
        draw(&small, &mut h, json!({"cells": [[1, 1], [2, 2], [3, 3]]})),
        ErrorKind::Limit,
    );
    assert_eq!(e.message, "at most 2 cells per call");
    assert_eq!(h.world().generation, 0);
}

#[test]
fn new_world_checks_tiling_size_and_cas() {
    let cx = Ctx::interactive();
    let mut h = Session::new();
    let run = |h: &mut Session, v: Value| {
        WorldNew::run(&cx, h, serde_json::from_value(v).unwrap(), &Quiet)
    };
    for (v, kind, needle) in [
        (
            json!({"tiling": "pentagonal-nonsense"}),
            ErrorKind::NotFound,
            "unknown tiling",
        ),
        (
            json!({"width": 3, "height": 3}),
            ErrorKind::Invalid,
            "8 to 4096",
        ),
        (
            json!({"width": 30}),
            ErrorKind::Invalid,
            "both width and height",
        ),
        (
            json!({"cells": 10}),
            ErrorKind::Invalid,
            "cells must be 64 to 4000000",
        ),
        (
            json!({"tiling": "spectre", "level": 9}),
            ErrorKind::Invalid,
            "level must be 1 to 7",
        ),
        (
            json!({"tiling": "square", "level": 2}),
            ErrorKind::Invalid,
            "only for aperiodic",
        ),
        (
            json!({"cas": ["Q7 nonsense"]}),
            ErrorKind::Engine,
            "not a rule",
        ),
        (
            json!({"tiling": "hex", "cas": ["W30"]}),
            ErrorKind::Engine,
            "square grid",
        ),
        (json!({"cas": 3}), ErrorKind::Invalid, "cas must be a list"),
        (
            json!({"boundary": "nope"}),
            ErrorKind::Invalid,
            "boundary must be",
        ),
    ] {
        let e = fails(run(&mut h, v.clone()), kind);
        assert!(e.message.contains(needle), "{v}: {e}");
    }
    // The world survived all of that; aperiodic patches have no grid.
    assert_eq!(h.world().topo.len(), 64 * 64);
    let out = run(
        &mut h,
        json!({"tiling": "spectre", "level": 2, "cas": ["Life"], "rng_seed": 5}),
    )
    .unwrap();
    assert!(out.status.tiling.grid.is_string());
    assert_eq!(out.status.tiling.boundary, "dead");
    // A trusted caller has no world-size limit beyond the engine's; the MCP server's is 4M cells.
    let trusted = Ctx::new();
    WorldNew::run(
        &trusted,
        &mut h,
        serde_json::from_value(json!({"tiling": "spectre", "level": 2})).unwrap(),
        &Quiet,
    )
    .unwrap();
}

/// A scene file naming a palette this JETCARP does not know opens (and runs, and verifies) with a
/// warning, keeping the name; a palette typed in an operation's arguments is refused.
#[test]
fn unknown_palettes_warn_in_scenes_and_are_refused_when_typed() {
    use jetcarp_api::op::Op;
    use jetcarp_api::ops::scene::{RunIn, SceneRun, SceneSource, SceneVerify};
    let dir = scratch("unknown-palette");
    let cx = Ctx::interactive();
    let mut h = Session::new();
    let e = fails(
        WorldNew::run(
            &cx,
            &mut h,
            serde_json::from_value(json!({"tiling": "square", "width": 10, "height": 10,
                "cas": [{"rule": "Life", "palette": "Dusk"}]}))
            .unwrap(),
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    assert!(e.message.contains("no palette named 'Dusk'"), "{e}");
    // The same CA in a scene file.
    let mut sc = jetcarp_core::Scene::new(TilingSpec::rect("square", 12, 12, Boundary::Wrap));
    let mut ca = CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 });
    ca.palette = Some("Dusk".into());
    sc.cas = vec![ca];
    let w = World::from_scene(&sc).unwrap();
    sc.record = Some(w.record());
    let file = dir.join("dusk.json");
    std::fs::write(&file, sc.to_json()).unwrap();
    let warning = "palette 'Dusk' is unknown here; drawn in Ember";
    let open = WorldOpen::run(
        &cx,
        &mut h,
        OpenIn {
            path: Some(file.display().to_string()),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(open.warnings, [warning]);
    assert!(open.text().contains(&format!("Warning: {warning}.")));
    assert_eq!(open.status.cas[0].palette, "Dusk");
    let cli = Ctx::new();
    let run: RunIn = serde_json::from_value(json!({"scene": file, "gens": 2})).unwrap();
    let r = SceneRun::run(&cli, run, &Quiet).unwrap();
    assert!(
        r.text().contains(&format!("warning:    {warning}\n")),
        "{}",
        r.text()
    );
    assert_eq!(to_value(&r)["warnings"][0], warning);
    let v = SceneVerify::run(
        &cli,
        SceneSource {
            path: Some(file.display().to_string()),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(
        v.text(),
        format!("ok: generation 0 hash {}\nwarning: {warning}\n", v.hash)
    );
}

/// What `world_status` says of each CA: Larger than Life's own neighbourhood, and a B/S rule's
/// hexagonal name on the hexagon grid's neighbour graph (and only there).
#[test]
fn status_names_ltl_neighbourhoods_and_hexagonal_rules() {
    let cx = Ctx::interactive();
    let status = |tiling: &str, rule: &str| {
        let mut h = Session::new();
        new_world(
            &cx,
            &mut h,
            json!({"tiling": tiling, "width": 24, "height": 24, "cas": [rule]}),
        );
        let st = WorldStatusOp::run(&cx, &mut h, StatusIn::default(), &Quiet).unwrap();
        let c = &st.status.cas[0];
        (c.name.clone(), c.neighborhood.clone())
    };
    // NN (von Neumann) and N+ (the cross) reach over edge neighbours, NM over touching ones.
    assert_eq!(status("square", "R2,C2,S2-4,B3-4,NN").1, "edge");
    assert_eq!(status("square", "R2,C2,S2-4,B3-4,N+").1, "edge");
    assert_eq!(status("square", "R2,C2,S2-4,B3-4,NM").1, "touch");
    assert_eq!(status("square", "R2,C2,S2-4,B3-4").1, "touch");
    // B2/S34 is Hexagonal Life (B2/S34H) on hexagons and Escher's lizards, whose tiles have the
    // same six edge neighbours, but not on squares or triangles.
    assert_eq!(status("hex", "B2/S34").0, "Hexagonal Life");
    assert_eq!(status("lizard", "B2/S34").0, "Hexagonal Life");
    assert_eq!(status("square", "B2/S34").0, "B2/S34");
    assert_eq!(status("triangle", "B2/S34").0, "B2/S34");
    // Other names are as before.
    assert_eq!(status("hex", "B3/S23").0, "Life");
    assert_eq!(status("square", "B3/S23").0, "Life");
}

#[test]
fn status_inspect_and_apply() {
    let cx = Ctx::interactive();
    let mut h = Session::new();
    new_world(
        &cx,
        &mut h,
        json!({"tiling": "hex", "width": 40, "height": 30, "cas": ["Brian's Brain"], "rng_seed": 3}),
    );
    let st = WorldStatusOp::run(&cx, &mut h, StatusIn::default(), &Quiet).unwrap();
    assert!(st.scene.is_none());
    let v = to_value(&st.status);
    assert_eq!(
        keys(&v),
        [
            "cas",
            "cells",
            "cells_hash",
            "done",
            "fields",
            "generation",
            "hash",
            "hash_note",
            "host",
            "script",
            "status",
            "tiling"
        ]
    );
    assert!(v["script"].is_null());
    assert!(v["fields"][0].get("stable").is_none());
    let with_scene = WorldStatusOp::run(&cx, &mut h, StatusIn { scene: true }, &Quiet).unwrap();
    assert!(with_scene.text().contains("\"cas\""));
    let c = WorldInspect::run(
        &cx,
        &mut h,
        InspectIn {
            x: Some(3),
            y: Some(3),
            index: None,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(c.edge_neighbors_clockwise.len(), 6);
    assert_eq!((c.x, c.y), (Some(3), Some(3)));
    fails(
        WorldInspect::run(
            &cx,
            &mut h,
            InspectIn {
                index: Some(1_000_000),
                ..Default::default()
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    let apply =
        |h: &mut Session, v: Value| WorldApply::run(&cx, h, ApplyIn { commands: v }, &Quiet);
    let a = apply(
        &mut h,
        json!([{"op": "add_ca", "ca": {"rule": "Game of Life", "seed": "random 10%"}},
               {"op": "step", "gens": 3}]),
    )
    .unwrap();
    assert_eq!(a.new_ca_ids, vec![2]);
    assert!(
        a.text()
            .starts_with("1. add_ca: ok, new CA id 2\n2. step: ok\nGeneration 3.")
    );
    assert_eq!(h.world().cas[1].notation(), "B3/S23");
    let e = fails(apply(&mut h, json!([{"gens": 3}])), ErrorKind::Invalid);
    assert!(e.message.contains("no \"op\""), "{e}");
    let e = fails(
        apply(
            &mut h,
            json!([{"op": "step", "gens": 1}, {"op": "remove_ca", "id": 42}]),
        ),
        ErrorKind::Engine,
    );
    assert!(e.message.contains("2. remove_ca: failed"), "{e}");
    assert!(
        e.message
            .contains("Stopped; later commands were not applied.")
    );
    let e = fails(
        apply(&mut h, json!([{"op": "step", "gens": 99_999_999}])),
        ErrorKind::Limit,
    );
    assert!(
        e.message.contains("at most 1000000 generations per call"),
        "{e}"
    );
    fails(apply(&mut h, json!([])), ErrorKind::Invalid);
    fails(apply(&mut h, Value::Null), ErrorKind::Invalid);
    assert_eq!(h.world().generation, 4);
}

#[test]
fn seeds_from_commands_keep_the_ant_limit() {
    let w = World::new(TilingSpec::rect("square", 16, 16, Boundary::Wrap), 1).unwrap();
    let cmd = json!({"op": "add_ca", "ca": {"rule": "Ant RL", "seed": "ants:50000"}});
    let ants = |l: &Limits| match prepare_command(&cmd, &w, l).unwrap() {
        Command::AddCa { ca } => ca.seed,
        c => panic!("{c:?}"),
    };
    assert_eq!(
        ants(&Limits::INTERACTIVE),
        Some(SeedSpec::Ants { count: 10_000 })
    );
    assert_eq!(ants(&Limits::NONE), Some(SeedSpec::Ants { count: 50_000 }));
    let big = json!({"op": "set_tiling", "tiling": {"family": "hex",
        "size": {"rect": {"w": 3000, "h": 3000}}, "boundary": "wrap"}});
    assert_eq!(
        prepare_command(&big, &w, &Limits::INTERACTIVE)
            .unwrap_err()
            .kind,
        ErrorKind::Limit
    );
}

#[test]
fn step_settles_cancels_and_reports() {
    let cx = Ctx::interactive();
    let mut h = Session::new();
    new_world(
        &cx,
        &mut h,
        json!({"width": 21, "height": 21, "boundary": "dead",
               "cas": [{"rule": "Sandpile", "seed": "pile:400"}]}),
    );
    let s = WorldStep::run(
        &cx,
        &mut h,
        StepIn {
            generations: None,
            stop_when_settled: true,
        },
        &Quiet,
    )
    .unwrap();
    assert!(s.ran > 1 && s.ran < SETTLE_GENS, "{}", s.ran);
    assert_eq!(s.stopped.as_deref(), Some("settled"));
    assert!(s.text().contains("(stopped early: settled)"));
    // A cancelled call stops at once and says so.
    let cancel = Arc::new(AtomicBool::new(true));
    let p = Callbacks {
        on_report: None,
        cancel: Some(cancel),
    };
    let s = WorldStep::run(
        &cx,
        &mut h,
        StepIn {
            generations: Some(100),
            stop_when_settled: false,
        },
        &p,
    )
    .unwrap();
    assert_eq!((s.ran, s.stopped.as_deref()), (0, Some("cancelled")));
    let step = |h: &mut Session, g: i64| {
        WorldStep::run(
            &cx,
            h,
            StepIn {
                generations: Some(g),
                stop_when_settled: false,
            },
            &Quiet,
        )
    };
    fails(step(&mut h, 0), ErrorKind::Invalid);
    fails(step(&mut h, 2_000_000), ErrorKind::Limit);
}

#[test]
fn reseed_render_save_and_open() {
    let dir = scratch("world");
    let cx = Ctx::interactive();
    let mut h = Session::new();
    new_world(
        &cx,
        &mut h,
        json!({"width": 60, "height": 40, "cas": [{"rule": "Life", "seed": "empty"}]}),
    );
    let reseed = |h: &mut Session, v: Value| {
        WorldReseed::run(&cx, h, serde_json::from_value(v).unwrap(), &Quiet)
    };
    let r = reseed(
        &mut h,
        json!({"mode": {"kind": "blobs", "count": 2, "live": 0.5}}),
    )
    .unwrap();
    assert_eq!(r.done, vec!["reseeded (blobs)".to_string()]);
    assert!(h.world().fields[0].population() > 0);
    reseed(
        &mut h,
        json!({"policies": [{"mode": "rain", "when": {"kind": "every", "gens": 10}},
                            {"mode": "blobs", "when": "settled", "cooldown": 50}]}),
    )
    .unwrap();
    h.exec(Command::Step { gens: 100 }).unwrap();
    let r = reseed(&mut h, json!({"activity_gens": 4})).unwrap();
    assert_eq!(r.policies[0].status["fired"], 10);
    assert!(
        (1..=4).contains(&r.activity.rows.len()),
        "{:?}",
        r.activity.rows
    );
    for (v, needle) in [
        (json!({"mode": "hail"}), "`mode` is random, rain, blobs"),
        (json!({"ca": 1}), "`ca` goes with `mode`"),
        (json!({"policies": "x"}), "must be a list"),
        (
            json!({"policies": [{"mode": "x"}]}),
            "could not read policy",
        ),
    ] {
        let e = fails(reseed(&mut h, v.clone()), ErrorKind::Invalid);
        assert!(e.message.contains(needle), "{v}: {e}");
    }
    // A picture, also saved; the bytes are a PNG.
    let png = dir.join("pics/w.png");
    let pic = WorldRender::run(
        &cx,
        &mut h,
        RenderIn {
            max_side: Some(128),
            outlines: Some(true),
            save_path: Some(png.display().to_string()),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(&pic.png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.is_file());
    assert!(
        pic.text()
            .ends_with(&format!(" Saved to {}.", png.display()))
    );
    fails(
        WorldRender::run(
            &cx,
            &mut h,
            RenderIn {
                max_side: Some(5),
                ..Default::default()
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    // Save, then open in another session: the same state.
    let path = dir.join("scenes/w.json");
    let saved = WorldSave::run(
        &cx,
        &mut h,
        SaveIn {
            path: path.display().to_string(),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(saved.generation, 100);
    let mut h2 = Session::new();
    let open = |h: &mut Session, v: Value| {
        WorldOpen::run(&cx, h, serde_json::from_value(v).unwrap(), &Quiet)
    };
    let o = open(&mut h2, json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(o.replayed_to_record, Some(true));
    assert!(
        o.text()
            .starts_with("Scene loaded. Replayed to its recorded hash.\n")
    );
    assert_eq!(h2.world().hash_hex(), h.world().hash_hex());
    fails(open(&mut h2, json!({"scene": "{}"})), ErrorKind::Invalid);
    fails(open(&mut h2, json!({})), ErrorKind::Invalid);
    fails(
        open(
            &mut h2,
            json!({"path": dir.join("nope.json").display().to_string()}),
        ),
        ErrorKind::NotFound,
    );
    fails(
        WorldSave::run(&cx, &mut h, SaveIn { path: " ".into() }, &Quiet),
        ErrorKind::Invalid,
    );
    // A scene over the limit is refused before it is built.
    let small = Ctx::new().with_limits(Limits {
        max_cells: 1000,
        ..Limits::INTERACTIVE
    });
    let e = fails(
        WorldOpen::run(
            &small,
            &mut h2,
            serde_json::from_value(json!({"path": path.display().to_string()})).unwrap(),
            &Quiet,
        ),
        ErrorKind::Limit,
    );
    assert!(e.message.contains("60 x 40 is too large"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A catalog holding Life's glider.
fn glider_catalog(dir: &std::path::Path) -> Library {
    let mut w = World::new(TilingSpec::rect("square", 40, 40, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells {
            cells: place_rows(&g, ".o./..o/ooo", 10, 10),
        }),
    })
    .unwrap();
    let lib = Library::open(dir.join("catalog"));
    let found = find_objects(&w, &ObjectOptions::default()).unwrap();
    for o in found
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
    {
        let prov = Provenance {
            source: "api test".into(),
            jetcarp_version: jetcarp_core::VERSION.into(),
            date: jetcarp_core::pattern::now_utc(),
            ..Default::default()
        };
        let rec = record_from_object(&w, w.cas[0].id, o, prov, DiscoveredBy::User).unwrap();
        lib.add(rec).unwrap();
    }
    lib
}

#[test]
fn paste_from_the_catalog() {
    let dir = scratch("paste");
    let lib = glider_catalog(&dir);
    let mut cx = Ctx::interactive();
    cx.library = Some(lib.path().to_path_buf());
    let mut h = Session::new();
    new_world(
        &cx,
        &mut h,
        json!({"width": 30, "height": 30, "cas": [{"rule": "Life", "seed": "empty"}]}),
    );
    let paste = |h: &mut Session, v: Value| {
        WorldPaste::run(&cx, h, serde_json::from_value(v).unwrap(), &Quiet)
    };
    let p = paste(&mut h, json!({"id": "glider", "x": 4, "y": 4})).unwrap();
    assert_eq!(p.cells, 5);
    assert!(p.warning.is_none());
    assert!(p.text().starts_with("Pasted glider ("), "{}", p.text());
    assert_eq!(h.world().fields[0].population(), 5);
    assert!(
        h.world()
            .log()
            .iter()
            .any(|c| matches!(c, Command::PastePattern { .. }))
    );
    fails(
        paste(&mut h, json!({"id": "no-such-pattern"})),
        ErrorKind::NotFound,
    );
    fails(
        paste(&mut h, json!({"id": "glider", "x": 1})),
        ErrorKind::Invalid,
    );
    // A CA running another rule gets a note.
    new_world(
        &cx,
        &mut h,
        json!({"width": 30, "height": 30, "cas": [{"rule": "B2/S", "seed": "empty"}]}),
    );
    let p = paste(&mut h, json!({"id": "glider"})).unwrap();
    assert!(
        p.warning
            .unwrap()
            .starts_with("Note: the pattern runs as stored in")
    );
    let _ = std::fs::remove_dir_all(dir);
}
