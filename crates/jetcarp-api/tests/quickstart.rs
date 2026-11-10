//! `quickstart.*`: entries added from a pattern, an ant run, and a sweep run; listed, read,
//! hidden, removed; and the built-in examples checked.

mod common;

use common::{ant_id, catalog, fails, keys, scratch};
use jetcarp_api::op::{Op, Quiet, Report, to_value};
use jetcarp_api::ops::quickstart::*;
use jetcarp_api::sweeps::{Favourites, Manifest};
use jetcarp_api::{ErrorKind, call_json};
use jetcarp_core::library::{CurationState, QsStatus};
use serde_json::json;
use std::path::{Path, PathBuf};

/// A sweep folder as `sweep(#{ rules: "W", from: 28, to: 31, ... })` leaves it: run.json with
/// the settings, and a manifest of four runs (no pictures).
fn sweep_folder(dir: &Path) -> PathBuf {
    let f = dir.join("2026-11-10_12-00-00_W");
    std::fs::create_dir_all(&f).unwrap();
    let run = json!({"sweep": {"tiling": "square", "size": [40, 24], "boundary": "dead",
                                "seed": "top_center", "until": "done"}});
    std::fs::write(f.join("run.json"), run.to_string()).unwrap();
    let rows: Vec<_> = (28..32)
        .map(|n| {
            json!({"index": n - 28, "n": n, "rule": format!("W{n}"), "file": "",
                        "row_class": "chaotic"})
        })
        .collect();
    let m = json!({"columns": ["index", "n", "rule", "file", "row_class"], "rows": rows});
    std::fs::write(f.join("manifest.json"), m.to_string()).unwrap();
    f
}

fn add(cx: &jetcarp_api::Ctx, a: AddIn) -> Result<Added, jetcarp_api::ApiError> {
    QuickstartAdd::run(cx, a, &Quiet)
}

#[test]
fn add_from_a_pattern_an_ant_and_a_sweep_run() {
    let dir = scratch("qs-add");
    let (lib, cx) = catalog(&dir);
    // A pattern: centred on a grid of its own, marked promoted.
    let p = add(
        &cx,
        AddIn {
            pattern: Some("glider".into()),
            title: Some("My glider".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let gid = lib.get("glider").unwrap().id;
    assert_eq!(
        (p.source.as_str(), p.entry.clone(), p.title.as_str()),
        ("pattern", format!("find:{gid}"), "My glider")
    );
    assert_eq!(lib.get("glider").unwrap().state, CurationState::Promoted);
    assert!(
        p.text().starts_with("promoted: My glider\nentry:    find:"),
        "{}",
        p.text()
    );
    // An ant run: its build-up, in a palette.
    let ant = ant_id(&lib);
    let a = add(
        &cx,
        AddIn {
            ant: Some(ant.clone()),
            palette: Some("Paper".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((a.source.as_str(), a.id.as_str()), ("ant", ant.as_str()));
    assert_eq!(a.text(), format!("Quick Start entry {ant} ({})\n", a.title));
    assert_eq!(lib.ant(&ant).unwrap().state, "promoted");
    // A sweep run, starred in the folder's favourites with the entry and the note.
    let sw = sweep_folder(&dir);
    let s = add(
        &cx,
        AddIn {
            folder: Some(sw.display().to_string()),
            run: Some("W30".into()),
            title: Some("Rule thirty".into()),
            note: Some("chaos".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        (s.source.as_str(), s.rule.as_deref(), s.run),
        ("sweep", Some("W30"), Some(2))
    );
    assert!(s.id.starts_with("sweep-"));
    assert!(
        s.description.starts_with("chaos (W30 from the sweep "),
        "{}",
        s.description
    );
    assert!(
        s.text()
            .starts_with("promoted: W30 as \"Rule thirty\" (run #2)\n"),
        "{}",
        s.text()
    );
    let favs = Favourites::load(&sw);
    assert_eq!(favs.items[&2]["promoted"], s.entry.as_str());
    // Without the favourite: the folder is left alone.
    let s2 = add(
        &cx,
        AddIn {
            folder: Some(sw.display().to_string()),
            run: Some("#3".into()),
            favourite: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s2.title, "W31");
    assert_eq!(Favourites::load(&sw).items.len(), 1);
    // The entry replays the run.
    let row = lib.get_quickstart(&s.id).unwrap();
    let m = Manifest::load(&sw).unwrap();
    let w = jetcarp_api::sweeps::run_world(&m.run, "W30").unwrap();
    assert_eq!(
        jetcarp_core::World::from_scene(&row.scene)
            .unwrap()
            .hash_hex(),
        w.hash_hex()
    );
    // Errors.
    fails(add(&cx, AddIn::default()), ErrorKind::Invalid);
    fails(
        add(
            &cx,
            AddIn {
                pattern: Some("glider".into()),
                ant: Some(ant.clone()),
                ..Default::default()
            },
        ),
        ErrorKind::Invalid,
    );
    fails(
        add(
            &cx,
            AddIn {
                pattern: Some("nonesuch".into()),
                ..Default::default()
            },
        ),
        ErrorKind::NotFound,
    );
    fails(
        add(
            &cx,
            AddIn {
                folder: Some(sw.display().to_string()),
                ..Default::default()
            },
        ),
        ErrorKind::Invalid,
    );
    for (folder, run) in [(sw.clone(), "W99"), (dir.join("nofolder"), "W30")] {
        fails(
            add(
                &cx,
                AddIn {
                    folder: Some(folder.display().to_string()),
                    run: Some(run.into()),
                    ..Default::default()
                },
            ),
            ErrorKind::NotFound,
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn list_get_hide_and_remove() {
    let dir = scratch("qs-list");
    let (lib, cx) = catalog(&dir);
    for id in ["glider", "blinker"] {
        add(
            &cx,
            AddIn {
                pattern: Some(id.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let gid = format!("find:{}", lib.get("glider").unwrap().id);
    let l = QuickstartList::run(&cx, ListIn::default(), &Quiet).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert_eq!(
        keys(&to_value(&l.entries[0])),
        [
            "behaviour",
            "created",
            "description",
            "grid",
            "id",
            "rule",
            "rule_family",
            "score",
            "source_id",
            "source_kind",
            "status",
            "title"
        ]
    );
    assert!(l.text().starts_with("catalog: "), "{}", l.text());
    assert!(l.text().contains(&format!("\n{gid} ")), "{}", l.text());
    // Read one, with its scene.
    let e = QuickstartGet::run(&cx, IdIn { id: gid.clone() }, &Quiet).unwrap();
    assert_eq!(e.0.source_kind, "pattern");
    assert!(!e.0.scene.cas.is_empty());
    fails(
        QuickstartGet::run(
            &cx,
            IdIn {
                id: "find:nonesuch".into(),
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    // Hide it (rejected); the text echoes what was given.
    let s = QuickstartUpdate::run(
        &cx,
        StatusIn {
            id: gid.clone(),
            status: "rejected".into(),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((s.status, s.shown), (QsStatus::Rejected, false));
    assert_eq!(s.text(), format!("{gid}: rejected\n"));
    let hidden = QuickstartList::run(
        &cx,
        ListIn {
            status: Some("rejected".into()),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(hidden.entries.len(), 1);
    // By name, with JSON.
    let v = call_json(
        &cx,
        "quickstart.update",
        json!({"id": gid.trim_start_matches("find:"), "status": "approved"}),
    )
    .unwrap();
    assert_eq!(v, json!({"id": gid, "status": "approved", "shown": true}));
    fails(
        QuickstartUpdate::run(
            &cx,
            StatusIn {
                id: gid.clone(),
                status: "bogus".into(),
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    fails(
        QuickstartUpdate::run(
            &cx,
            StatusIn {
                id: "find:nonesuch".into(),
                status: "approved".into(),
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    fails(
        QuickstartList::run(
            &cx,
            ListIn {
                status: Some("bogus".into()),
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    // Remove it: the pattern stays.
    let r = QuickstartRemove::run(&cx, IdIn { id: gid.clone() }, &Quiet).unwrap();
    assert_eq!(r.text(), format!("removed {gid}\n"));
    let l = QuickstartList::run(&cx, ListIn::default(), &Quiet).unwrap();
    assert_eq!(l.entries.len(), 1);
    assert!(lib.get("glider").is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_built_in_examples() {
    let dir = scratch("qs-check");
    let cx = jetcarp_api::Ctx::new();
    let check = |v: serde_json::Value| {
        QuickstartCheck::run(&cx, serde_json::from_value(v).unwrap(), &Quiet).unwrap()
    };
    // The list: numbered as `jetcarp quickstart` numbers them.
    let all = check(json!({}));
    assert!(all.examples.len() > 20);
    assert_eq!(all.examples[0].id, "life");
    assert!(all.not_working.is_none() && all.examples[0].health.is_none());
    assert!(
        all.text().starts_with("  0 Life on the square grid "),
        "{}",
        all.text()
    );
    // Checked, filtered, with a sheet.
    let sheet = dir.join("sheets").join("langton.png");
    let c = check(json!({"filter": "langton", "check": true, "gens": 30,
                         "sheet": sheet, "thumb": 40, "cols": 2}));
    let at: Vec<usize> = c.examples.iter().map(|e| e.index).collect();
    assert_eq!(at, [7, 76]);
    assert_eq!(c.not_working, Some(0));
    assert!(c.examples[0].health.as_ref().unwrap().works);
    assert!(sheet.exists());
    let t = c.text();
    assert!(t.contains("  ok   pop "), "{t}");
    assert!(t.contains("\n2 examples, 0 not working\nwrote "), "{t}");
    // A survey of starting patterns.
    let s = check(json!({"filter": "langton", "survey": true, "gens": 10}));
    assert_eq!(s.examples[0].survey.len(), 16);
    assert!(
        s.text()
            .starts_with("  7 Langton's ant Ant RL\n      Random"),
        "{}",
        s.text()
    );
    let _ = std::fs::remove_dir_all(dir);
}
