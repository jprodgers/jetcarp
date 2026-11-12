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
    let q: jetcarp_store::Query =
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
    let up: jetcarp_store::Update =
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
    // The grid in the placements' coordinates (one tile to a unit cell on squares), and `uX,Y`
    // naming the same spots in unit cells.
    let g = p.grid.unwrap();
    assert_eq!((g.width, g.height, g.tiles_per_unit), (40, 30, 1));
    assert!(p.text().contains("grid:  40 x 30 tiles"), "{}", p.text());
    let u = LibraryPaste::run(
        &cx,
        serde_json::from_value(json!({
            "placements": ["glider@u5,5,o1", "blinker@u20,20"],
            "out": dir.join("two-u.json"), "size": "40x30"
        }))
        .unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!(u.hash, p.hash.clone());
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

// `library.reverify` checks records again as a job: with nothing flagged it checks nothing;
// with `all` it checks every record, each passes on this engine, and nothing stays flagged.
// (A geometry change, simulated in the file, is tested in jetcarp-store's library_v1.)
#[test]
fn reverify_checks_flagged_or_all_records_as_a_job() {
    let dir = scratch("lib-reverify");
    let (lib, cx) = catalog(&dir);
    let r = LibraryReverify::run(&cx, ReverifyIn::default(), &Quiet).unwrap();
    assert_eq!(
        (r.status.as_str(), r.progress.total, r.progress.checked),
        ("done", 0, 0)
    );
    assert_eq!(r.flagged.values().sum::<u64>(), 0);
    assert_eq!(lib.job(r.job).unwrap().kind, "reverify");
    let v = call_json(&cx, "library.reverify", json!({"all": true, "threads": 2})).unwrap();
    assert_eq!(v["status"], "done");
    assert_eq!(
        (
            v["progress"]["checked"].as_u64(),
            v["progress"]["passed"].as_u64()
        ),
        (Some(3), Some(3))
    );
    assert_eq!(v["progress"]["failed"], 0);
    // Resuming a finished job runs nothing.
    let again = LibraryReverify::run(
        &cx,
        ReverifyIn {
            job: Some(r.job),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert!(again.text().contains("0 of 0 checked"), "{}", again.text());
    // A pattern's history has its row 0 and a `reverified` row now.
    let id = &lib.list().unwrap()[0].id;
    let h = lib.provenance("pattern", id).unwrap();
    assert_eq!(
        (h[0].kind.as_str(), h.last().unwrap().kind.as_str()),
        ("found", "reverified")
    );
    let e = fails(
        call_json(&cx, "library.reverify", json!({"job": 0})),
        ErrorKind::Invalid,
    );
    assert!(e.message.contains("not a re-verification"), "{e}");
    let i = LibraryInfoOp::run(&cx, NoInput {}, &Quiet).unwrap();
    assert_eq!(i.0.needs_reverify, 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// The record of the one object the finder sees in `cells` (`i,j,k,s` tuples on a wrapped Life
/// grid) when it does not split compounds, in the first generation where it sees one: what
/// earlier builds stored.
fn unsplit_record(cells: &str) -> jetcarp_core::pattern::PatternRecord {
    use jetcarp_core::objects::{ObjectOptions, find_objects};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, decode_cells, record_from_object};
    use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
    let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let cells = decode_cells(cells)
        .unwrap()
        .iter()
        .map(|c| [g.resolve(c.i + 20, c.j + 20, c.k).unwrap() as u32, 1])
        .collect();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let opts = ObjectOptions {
        separate: false,
        ..ObjectOptions::default()
    };
    for _ in 0..8 {
        let r = find_objects(&w, &opts).unwrap();
        if r.objects.len() == 1 {
            let prov = Provenance {
                source: "an old build".into(),
                jetcarp_version: jetcarp_core::VERSION.into(),
                date: jetcarp_core::pattern::now_utc(),
                ..Default::default()
            };
            return record_from_object(&w, w.cas[0].id, &r.objects[0], prov, DiscoveredBy::Search)
                .unwrap();
        }
        w.apply(Command::Step { gens: 1 }).unwrap();
    }
    panic!("never one group");
}

// `library.compounds` finds the compounds an earlier build stored (two gliders flying together,
// two blocks a cell apart, a pulsar with blinkers) and lists them without changing anything;
// with `apply` it stores their parts, credited with the compounds' sightings, removes the
// uncurated compounds, and keeps the curated one tagged. A second pass finds nothing new.
#[test]
fn compounds_are_listed_then_split_into_their_parts() {
    let dir = scratch("lib-compounds");
    let (lib, cx) = catalog(&dir);
    let glider = lib.get("glider").unwrap();
    // Two gliders side by side, as a 20,000-soup hunt stored them.
    let pair = unsplit_record(
        "0,0,0,1 1,0,0,1 2,0,0,1 0,1,0,1 1,2,0,1 4,3,0,1 5,3,0,1 6,3,0,1 4,4,0,1 5,5,0,1",
    );
    assert_eq!((pair.class, pair.cells), (PatternClass::Spaceship, 10));
    let biblock = unsplit_record("0,0,0,1 1,0,0,1 3,0,0,1 4,0,0,1 0,1,0,1 1,1,0,1 3,1,0,1 4,1,0,1");
    assert_eq!((biblock.class, biblock.cells), (PatternClass::StillLife, 8));
    // A pulsar with four blinkers beside it, as the same hunt stored it.
    let mut favourite = unsplit_record(
        "2,0,0,1 3,0,0,1 4,0,0,1 8,0,0,1 9,0,0,1 10,0,0,1 0,2,0,1 5,2,0,1 7,2,0,1 12,2,0,1 0,3,0,1 \
         5,3,0,1 7,3,0,1 12,3,0,1 0,4,0,1 5,4,0,1 7,4,0,1 12,4,0,1 2,5,0,1 3,5,0,1 4,5,0,1 8,5,0,1 \
         9,5,0,1 10,5,0,1 2,7,0,1 3,7,0,1 4,7,0,1 8,7,0,1 9,7,0,1 10,7,0,1 0,8,0,1 5,8,0,1 7,8,0,1 \
         12,8,0,1 0,9,0,1 5,9,0,1 7,9,0,1 12,9,0,1 0,10,0,1 5,10,0,1 7,10,0,1 12,10,0,1 15,11,0,1 \
         2,12,0,1 3,12,0,1 4,12,0,1 8,12,0,1 9,12,0,1 10,12,0,1 15,12,0,1 15,13,0,1 11,15,0,1 \
         12,15,0,1 13,15,0,1 17,15,0,1 18,15,0,1 19,15,0,1 15,17,0,1 15,18,0,1 15,19,0,1",
    );
    assert_eq!(
        (favourite.class, favourite.period),
        (PatternClass::Oscillator, 6)
    );
    favourite.name = Some("pulsar with blinkers".into());
    for r in [pair.clone(), biblock.clone(), favourite.clone()] {
        lib.add(r).unwrap();
    }
    // A second sighting of the pair, so its glider gains four copies.
    lib.add(pair.clone()).unwrap();
    let before = lib.count(&Default::default()).unwrap();
    assert_eq!(before, 5);
    let report = dir.join("compounds.jsonl");
    let dry = LibraryCompounds::run(
        &cx,
        CompoundsIn {
            report: Some(report.display().to_string()),
            threads: 2,
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    let p = &dry.progress;
    assert_eq!(dry.status, "done");
    assert_eq!((p.checked, p.compounds, p.removed, p.kept), (5, 3, 2, 1));
    assert_eq!(p.by_class["spaceship"], 1);
    assert!(
        dry.text().contains("nothing changed (dry run)"),
        "{}",
        dry.text()
    );
    let row = p.examples.iter().find(|c| c.id == pair.id).unwrap();
    assert_eq!(row.action, "would remove");
    assert_eq!(row.parts.len(), 1);
    assert_eq!(
        (row.parts[0].id.as_str(), row.parts[0].copies),
        (glider.id.as_str(), 2)
    );
    assert!(row.parts[0].stored);
    let fav = p.examples.iter().find(|c| c.id == favourite.id).unwrap();
    assert_eq!(fav.action, "would keep");
    // Nothing changed; the report holds every compound with its whole record and history.
    assert_eq!(lib.count(&Default::default()).unwrap(), before);
    assert_eq!(lib.get(&glider.id).unwrap().seen, glider.seen);
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&report)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l["record"]["phases"].is_array()));
    assert!(lines.iter().all(|l| l["provenance"][0]["kind"] == "found"));
    // Apply: the parts are stored and credited, the uncurated compounds removed.
    let done = LibraryCompounds::run(
        &cx,
        CompoundsIn {
            apply: true,
            threads: 2,
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    let p = &done.progress;
    assert_eq!((p.compounds, p.removed, p.kept), (3, 2, 1));
    assert!(done.text().contains("removed 2"), "{}", done.text());
    assert!(std::path::Path::new(&p.report).exists());
    assert!(lib.get(&pair.id).is_err());
    assert!(lib.get(&biblock.id).is_err());
    let kept = lib.get(&favourite.id).unwrap();
    assert!(kept.tags.iter().any(|t| t == "compound"), "{:?}", kept.tags);
    assert!(kept.notes.contains("Compound: "), "{}", kept.notes);
    // The pair was seen twice: four more gliders.
    let g = lib.get(&glider.id).unwrap();
    assert_eq!(g.seen, glider.seen + 4);
    let h = lib.provenance("pattern", &glider.id).unwrap();
    let last = h.last().unwrap();
    assert_eq!(last.kind, "reverified");
    assert!(
        last.source
            .starts_with("compound-split: 2 copies in compound"),
        "{}",
        last.source
    );
    // The blocks are new: two copies of the bi-block's one sighting, with its history.
    let blocks = lib
        .query(&jetcarp_store::Query {
            class: Some(PatternClass::StillLife),
            ..Default::default()
        })
        .unwrap();
    let block = blocks.iter().find(|r| r.cells == 4).unwrap();
    assert_eq!(block.seen, 2);
    let h = lib.provenance("pattern", &block.id).unwrap();
    assert_eq!(
        (h[0].kind.as_str(), h[0].source.as_str()),
        ("found", "an old build")
    );
    assert!(h.last().unwrap().source.contains("which was removed"));
    // The kept compound gave a pulsar, and its blinkers joined the stored one.
    let all = lib.list().unwrap();
    assert!(all.iter().any(|r| r.cells == 48 && r.period == 3));
    // A second pass changes nothing: the kept compound is tagged.
    let again = LibraryCompounds::run(
        &cx,
        CompoundsIn {
            apply: true,
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!((again.progress.compounds, again.progress.already), (0, 1));
    let e = fails(
        call_json(&cx, "library.compounds", json!({"job": 0})),
        ErrorKind::Invalid,
    );
    assert!(e.message.contains("not a compounds pass"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A record a pack retracted stays in the library and shows as retracted: `library show` says by
/// whom, when and why, and `library list` marks it (the golden packs `minimal`, then
/// `retraction`, which retracts the blinker).
#[test]
fn retracted_records_show_as_retracted() {
    use jetcarp_api::ops::pack::{ImportIn, PackImport};
    let dir = scratch("retracted");
    let cx = jetcarp_api::Ctx::new().with_library(Some(dir.join("lib")));
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jetcarp-pack/tests/golden");
    for pack in ["minimal", "retraction"] {
        PackImport::run(
            &cx,
            ImportIn {
                path: golden.join(pack).display().to_string(),
                ..Default::default()
            },
            &Quiet,
        )
        .unwrap();
    }
    let q = query(
        &cx,
        QueryIn {
            class: Some("oscillator".into()),
            ..Default::default()
        },
    );
    assert_eq!(q.patterns.len(), 1);
    let blinker = q.patterns[0].id.clone();
    assert_eq!(q.retracted, vec![blinker.clone()]);
    assert!(q.text().contains("(retracted)"), "{}", q.text());
    let v = LibraryGet::run(
        &cx,
        GetIn {
            id: blinker.clone(),
            kind: None,
            phases: None,
            bare: false,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(v.retracted.len(), 1);
    let line = &v.retracted[0];
    assert!(
        line.starts_with("retracted by Golden Publisher A (SHA256:")
            && line.ends_with(" on 2026-11-11: a test retraction"),
        "{line}"
    );
    assert!(
        v.text().contains("retracted:   by Golden Publisher A"),
        "{}",
        v.text()
    );
    // The glider is not retracted.
    let all = query(&cx, QueryIn::default());
    assert_eq!(all.retracted, vec![blinker]);
    let _ = std::fs::remove_dir_all(dir);
}
