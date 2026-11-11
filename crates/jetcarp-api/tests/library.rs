//! `library.*` on a small catalog (a glider, a blinker, an ant highway): happy paths, texts, and
//! error kinds.

mod common;

use common::{ant_id, catalog, fails, scratch};
use jetcarp_api::op::{NoInput, Op, Quiet, Report, to_value};
use jetcarp_api::ops::library::*;
use jetcarp_api::{ErrorKind, call_json};
use jetcarp_core::pattern::{CurationState, PatternClass};
use serde_json::json;

fn query(cx: &jetcarp_api::Ctx, q: QueryIn) -> QueryOut {
    LibraryQuery::run(cx, q, &Quiet).unwrap()
}

#[test]
fn info_query_and_count() {
    let dir = scratch("lib-query");
    let (lib, cx) = catalog(&dir);
    let i = LibraryInfoOp::run(&cx, NoInput {}, &Quiet).unwrap();
    assert_eq!((i.0.patterns, i.0.ants), (2, 1));
    assert!(i.text().starts_with("catalog:        "), "{}", i.text());
    assert!(i.text().contains("patterns 2, rules 1,"), "{}", i.text());
    // Every pattern, best first; the text is `jetcarp library list`'s.
    let all = query(&cx, QueryIn::default());
    assert_eq!((all.kind.as_str(), all.matching), ("pattern", Some(2)));
    assert_eq!(all.patterns[0].class, PatternClass::Spaceship);
    let t = all.text();
    assert!(
        t.starts_with(&format!(
            "catalog: {} (2 shown of 2 matching)\n",
            lib.path().display()
        )),
        "{t}"
    );
    assert_eq!(t.lines().count(), 3);
    // Filters and paging.
    let osc = query(
        &cx,
        QueryIn {
            class: Some("oscillator".into()),
            ..Default::default()
        },
    );
    assert_eq!(osc.patterns.len(), 1);
    assert_eq!(osc.patterns[0].period, 2);
    let paged = query(
        &cx,
        QueryIn {
            works_in: Some("B3/S23".into()),
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        },
    );
    assert_eq!((paged.patterns.len(), paged.matching), (1, Some(2)));
    let counted = query(
        &cx,
        QueryIn {
            count_only: true,
            min_period: Some(3),
            ..Default::default()
        },
    );
    assert_eq!((counted.patterns.len(), counted.matching), (0, Some(1)));
    // From a catalog Query, as Rhai's lib.query reads one.
    let q: jetcarp_core::library::Query =
        serde_json::from_value(json!({"class": "spaceship", "state": "new"})).unwrap();
    assert_eq!(query(&cx, QueryIn::from_query(&q)).patterns.len(), 1);
    // Ant runs, with the outcome census.
    let ants = query(
        &cx,
        QueryIn {
            kind: Some("ant".into()),
            outcome: Some("highway".into()),
            ..Default::default()
        },
    );
    assert_eq!((ants.ants.len(), ants.census.len()), (1, 1));
    assert!(ants.text().starts_with("ant-"), "{}", ants.text());
    // Bad words are invalid input, with the catalog's own messages.
    for (q, msg) in [
        (
            QueryIn {
                class: Some("bogus".into()),
                ..Default::default()
            },
            "unknown class 'bogus'",
        ),
        (
            QueryIn {
                state: Some("bogus".into()),
                ..Default::default()
            },
            "unknown state 'bogus'",
        ),
        (
            QueryIn {
                discovered_by: Some("bogus".into()),
                ..Default::default()
            },
            "unknown discoverer 'bogus'",
        ),
        (
            QueryIn {
                kind: Some("bogus".into()),
                ..Default::default()
            },
            "`kind` is pattern or ant",
        ),
        (
            QueryIn {
                neighborhood: Some("diagonal".into()),
                ..Default::default()
            },
            "neighborhood is touch or edge",
        ),
    ] {
        let e = fails(LibraryQuery::run(&cx, q, &Quiet), ErrorKind::Invalid);
        assert!(e.message.starts_with(msg), "{e}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_a_pattern_or_an_ant() {
    let dir = scratch("lib-get");
    let (lib, cx) = catalog(&dir);
    let get = |id: &str, phases: Option<u64>| {
        LibraryGet::run(
            &cx,
            GetIn {
                id: id.into(),
                kind: None,
                phases,
                bare: false,
            },
            &Quiet,
        )
    };
    let v = get("glider", Some(4)).unwrap();
    assert_eq!((v.kind.as_str(), v.label.as_str()), ("pattern", "glider"));
    assert_eq!(v.phases.len(), 4);
    assert!(v.phases[0].ascii.is_some());
    // A spaceship moves differently in each paste orientation (eight on squares).
    assert_eq!(v.orientations.len(), 8);
    let t = v.text();
    assert!(t.starts_with("id:          "), "{t}");
    assert!(t.contains("\norientations: 0:("), "{t}");
    assert!(t.contains("\nphase 3:\n"), "{t}");
    // By prefix; bare is the record alone.
    let id = v.pattern.as_ref().unwrap().id.clone();
    let b = LibraryGet::run(
        &cx,
        GetIn {
            id: id[..6].into(),
            kind: Some("pattern".into()),
            phases: None,
            bare: true,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(b.pattern.unwrap().id, id);
    assert!(b.phases.is_empty() && b.orientations.is_empty());
    // An ant run by its id.
    let a = get(&ant_id(&lib), None).unwrap();
    assert_eq!(a.kind, "ant");
    assert!(a.ant.is_some() && a.pattern.is_none());
    assert_eq!(a.text(), format!("{}\n", a.label));
    // JSON: a pattern view has no `ant` key and the other way round.
    let j = to_value(&v);
    assert!(j.get("ant").is_none() && j["pattern"]["id"] == id);
    fails(get("nonesuch", None), ErrorKind::NotFound);
    fails(get("ant-000000000000", None), ErrorKind::NotFound);
    let e = fails(
        call_json(&cx, "library.get", json!({"id": "glider", "kind": "bogus"})),
        ErrorKind::Invalid,
    );
    assert!(e.message.contains("pattern or ant"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn update_and_remove() {
    let dir = scratch("lib-update");
    let (lib, cx) = catalog(&dir);
    let u = LibraryUpdate::run(
        &cx,
        UpdateIn {
            id: "glider".into(),
            rating: Some(4.0),
            add_tags: vec!["nice".into()],
            state: Some("reviewed".into()),
            notes: Some("flies".into()),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    let r = u.pattern.as_ref().unwrap();
    assert_eq!((r.rating, r.state), (Some(4), CurationState::Reviewed));
    assert_eq!(r.tags, ["nice"]);
    assert_eq!(
        u.text(),
        format!("updated {} (glider), state reviewed\n", r.id)
    );
    // As Rhai's lib.update reads its map.
    let up: jetcarp_core::library::Update =
        serde_json::from_value(json!({"rating": 2, "remove_tags": ["nice"]})).unwrap();
    let u2 = LibraryUpdate::run(&cx, UpdateIn::from_update("glider", &up), &Quiet).unwrap();
    assert_eq!(u2.pattern.as_ref().unwrap().rating, Some(2));
    assert!(u2.pattern.unwrap().tags.is_empty());
    // An ant run: name, notes, rating (rounded), state.
    let ant = ant_id(&lib);
    let a = LibraryUpdate::run(
        &cx,
        UpdateIn {
            id: ant.clone(),
            name: Some("Highway".into()),
            rating: Some(3.6),
            state: Some("reviewed".into()),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    let row = a.ant.unwrap();
    assert_eq!((row.rating, row.state.as_str()), (Some(4), "reviewed"));
    // Errors.
    fails(
        LibraryUpdate::run(
            &cx,
            UpdateIn {
                id: "glider".into(),
                state: Some("bogus".into()),
                ..Default::default()
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    fails(
        LibraryUpdate::run(
            &cx,
            UpdateIn {
                id: ant.clone(),
                add_tags: vec!["x".into()],
                ..Default::default()
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    fails(
        LibraryUpdate::run(
            &cx,
            UpdateIn {
                id: "nonesuch".into(),
                ..Default::default()
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    // Remove: the blinker, then the ant run.
    let rm = |id: &str| {
        LibraryRemove::run(
            &cx,
            RemoveIn {
                id: id.into(),
                kind: None,
            },
            &Quiet,
        )
    };
    let gone = rm("blinker").unwrap();
    assert_eq!(gone.text(), format!("removed {} (blinker)\n", gone.id));
    assert_eq!(lib.count(&Default::default()).unwrap(), 1);
    let gone = rm(&ant).unwrap();
    assert!(gone.text().starts_with(&format!("removed {ant} ")));
    fails(rm("blinker"), ErrorKind::NotFound);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn render_export_import_and_paste() {
    let dir = scratch("lib-render");
    let (_lib, cx) = catalog(&dir);
    // Text drawings without a PNG.
    let r = LibraryRender::run(
        &cx,
        serde_json::from_value(json!({"id": "glider", "max_phases": 2})).unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!(r.phases.len(), 2);
    assert!(r.text().starts_with("phase 0:\n"), "{}", r.text());
    // A PNG (its folder is made), in a palette.
    let png = dir.join("pics").join("glider.png");
    let r = LibraryRender::run(
        &cx,
        serde_json::from_value(json!({"id": "glider", "png": png, "px": 40, "palette": "Paper"}))
            .unwrap(),
        &Quiet,
    )
    .unwrap();
    assert!(png.exists() && r.phases.is_empty());
    let (w, h) = r.png_size.unwrap();
    assert!(r.text().ends_with(&format!("({w}x{h})\n")), "{}", r.text());
    fails(
        LibraryRender::run(
            &cx,
            serde_json::from_value(json!({"id": "glider", "palette": "Bogus"})).unwrap(),
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    // Export every record, import them into another catalog.
    let to = dir.join("records");
    let ex = LibraryExport::run(
        &cx,
        ExportIn {
            ids: vec![],
            to: to.display().to_string(),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(ex.files.len(), 2);
    assert!(ex.text().starts_with("2 pattern(s) exported to "));
    let other = jetcarp_api::Ctx::new().with_library(Some(dir.join("other")));
    let im = LibraryImport::run(
        &other,
        ImportIn {
            path: to.display().to_string(),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((im.patterns, im.pairs.len()), (2, 2));
    assert!(im.pairs.iter().all(|p| p.outcome == "added"));
    assert!(im.text().contains("  added  "), "{}", im.text());
    // Importing again merges.
    let again = LibraryImport::run(
        &other,
        ImportIn {
            path: to.display().to_string(),
        },
        &Quiet,
    )
    .unwrap();
    assert!(again.pairs.iter().all(|p| p.seen.is_some()));
    fails(
        LibraryImport::run(
            &other,
            ImportIn {
                path: dir.join("nonesuch").display().to_string(),
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    fails(
        LibraryExport::run(
            &cx,
            ExportIn {
                ids: vec!["nonesuch".into()],
                to: to.display().to_string(),
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    // A scene of two patterns, and a PNG of it after a few generations.
    let scene = dir.join("two.json");
    let p = LibraryPaste::run(
        &cx,
        serde_json::from_value(json!({
            "placements": ["glider@5,5,o1", "blinker@20,20"],
            "out": scene, "png": dir.join("two.png"), "gens": 4, "size": "40x30"
        }))
        .unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!((p.patterns, p.cas, p.generation), (2, 1, 4));
    assert!(scene.exists() && dir.join("two.png").exists());
    assert!(p.text().starts_with("scene: "), "{}", p.text());
    let w = jetcarp_core::World::from_scene(&p.scene).unwrap();
    assert_eq!(w.fields[0].population(), 8);
    for (placement, kind) in [
        ("glider@x,1", ErrorKind::Invalid),
        ("glider@5,5,q1", ErrorKind::Invalid),
        ("glider@5", ErrorKind::Invalid),
        ("nonesuch", ErrorKind::NotFound),
    ] {
        fails(
            LibraryPaste::run(
                &cx,
                serde_json::from_value(json!({"placements": [placement], "out": scene})).unwrap(),
                &Quiet,
            ),
            kind,
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn verify_and_prune() {
    let dir = scratch("lib-verify");
    let (lib, cx) = catalog(&dir);
    let v = LibraryVerify::run(
        &cx,
        VerifyIn {
            all: true,
            sample: 0,
            periods: Some(2),
            drop: false,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((v.checked, v.failed.len(), v.removed), (2, 0, 0));
    assert_eq!(v.by_tiling["Square"].checked, 2);
    assert!(
        v.text().starts_with("2 patterns verified (0 of them"),
        "{}",
        v.text()
    );
    // The default: square patterns only as a sample.
    let s = LibraryVerify::run(
        &cx,
        serde_json::from_value(json!({"sample": 1})).unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!((s.checked, s.sampled), (1, 1));
    // Prune with a policy that keeps everything: nothing deleted, the file still there.
    let p = LibraryPrune::run(
        &cx,
        serde_json::from_value(json!({"no_vacuum": true})).unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!(p.deleted, 0);
    assert!(p.vacuum_seconds.is_none());
    assert!(p.text().starts_with("pruned 0 of "), "{}", p.text());
    assert_eq!(lib.count(&Default::default()).unwrap(), 2);
    let _ = std::fs::remove_dir_all(dir);
}
