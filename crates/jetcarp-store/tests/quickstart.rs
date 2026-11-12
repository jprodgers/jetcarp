//! The Quick Start table and its menu: the built-in rows written again when the build changes,
//! an atlas published into the table, hidden entries, entries read by handle (cards with their
//! mature states, scenes), the menu's sources, categories, filters, counts and search, the
//! Featured shuffle, every named rule staying reachable, the opening rows, ratings and opens,
//! and a schema 2 library's entries kept.

use jetcarp_core::rules::catalog;
use jetcarp_core::topology::TilingSpec;
use jetcarp_core::{CaSpec, Scene, World};
use jetcarp_store::library::MenuRow;
use jetcarp_store::quickstart::menu::{self, Filter, Sort};
use jetcarp_store::quickstart::{self as qs, canonical_rule};
use jetcarp_store::{Library, QsSource, QuickStartRow, UsageRow};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-qs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn temp_lib(name: &str) -> (Library, PathBuf) {
    let dir = temp_dir(name);
    (Library::open(dir.join("lib")), dir)
}

fn raw(lib: &Library) -> Connection {
    Connection::open(lib.path()).unwrap()
}

fn life_scene(rule: &str, w: u32, h: u32) -> Scene {
    let mut s = Scene::new(TilingSpec::rect(
        "square",
        w,
        h,
        jetcarp_core::Boundary::Wrap,
    ));
    let mut ca = CaSpec::new(rule);
    ca.palette = Some("Ember".into());
    ca.palette_neg = Some("Ink".into());
    s.cas = vec![ca];
    s
}

/// An atlas run, as the search names it.
struct Run<'a> {
    id: i64,
    category: &'a str,
    family: &'a str,
    tiling: &'a str,
    group: &'a str,
    rule: &'a str,
    seed: &'a str,
    score: f64,
}

/// An atlas row of a run, made as `jetcarp_search::explore::atlas` makes them (the second of a
/// list has a mature state).
fn atlas_row(uuid: &str, r: &Run, k: usize) -> QuickStartRow {
    let short: String = uuid.chars().filter(|c| *c != '-').take(8).collect();
    let id = format!("atlas-{short}-{}", r.id);
    let mut q = QuickStartRow::for_scene(&id, "search_run", life_scene(r.rule, 240, 240));
    q.handle = format!("atlas:{short}-{}", r.id);
    q.source = QsSource::Atlas;
    q.source_ref = uuid.into();
    q.target_id = Some(format!("{uuid}:{}", r.id));
    let family = qs::family_label_named(r.family);
    q.title = qs::atlas_title(r.rule, r.category, &family);
    q.description = format!(
        "{} on {}\nSeed: {}\nScore {:.2}",
        r.rule,
        r.tiling,
        qs::describe_seed(r.seed),
        r.score
    );
    q.category = r.category.into();
    q.behaviour = r.category.into();
    q.grid_group = qs::group_label(r.group).into();
    q.family = family;
    q.tiling_key = r.tiling.into();
    q.tiling_name = qs::tiling_label(r.tiling);
    q.rule = r.rule.into();
    q.preview = Some(life_scene(r.rule, 32, 24));
    if k == 1 {
        q.mature_gen = 300;
        q.mature = Some(qs::deflate_state(&vec![1u8; 32 * 24]));
    }
    q.score = Some(r.score);
    q.rank = k as i64;
    q
}

/// Publish runs as an explore job's atlas; the job's uuid and its short form.
fn publish(lib: &Library, runs: &[Run]) -> (i64, String, String) {
    let job = lib.create_job("explore", &serde_json::json!({})).unwrap();
    let uuid = lib.job(job).unwrap().uuid;
    let rows: Vec<QuickStartRow> = runs
        .iter()
        .enumerate()
        .map(|(k, r)| atlas_row(&uuid, r, k))
        .collect();
    lib.publish_atlas(job, &uuid, &rows).unwrap();
    let short = uuid.chars().filter(|c| *c != '-').take(8).collect();
    (job, uuid, short)
}

fn sample() -> Vec<Run<'static>> {
    vec![
        Run {
            id: 1,
            category: "Gliders & ships",
            family: "life_like",
            tiling: "square",
            group: "square",
            rule: "B3/S23",
            seed: r#"{"random":{"density":0.3}}"#,
            score: 3.0,
        },
        Run {
            id: 2,
            category: "Turbulence",
            family: "generations",
            tiling: "catalog:k2-001",
            group: "k-uniform",
            rule: "B2/S/C3",
            seed: r#"{"pattern":{"shape":{"kind":"noise"},"fill":{"density":0.85}}}"#,
            score: 2.0,
        },
        Run {
            id: 3,
            category: "Turbulence",
            family: "life_like",
            tiling: "trihex",
            group: "archimedean",
            rule: "B3/S23",
            seed: r#"{"blob":{"radius":0.2}}"#,
            score: 1.0,
        },
    ]
}

#[test]
fn builtins_are_written_again_when_the_build_changes() {
    let (lib, dir) = temp_lib("builtins");
    let n = qs::builtin_rows().len();
    assert!(n > 300, "{n}");
    // Making the library writes the built-ins (so asking again finds them up to date).
    assert_eq!(lib.sync_builtins().unwrap(), None);
    assert_eq!(lib.quickstart(&[QsSource::Builtin]).unwrap().len(), n);
    assert_eq!(
        lib.meta("builtin_version").unwrap().as_deref(),
        Some(qs::builtin_version())
    );
    // Under today's ids as handles; with no provenance (every JETCARP has them).
    let life = lib.get_quickstart("life").unwrap();
    assert_eq!(
        (life.source, life.handle.as_str(), life.id.as_str()),
        (QsSource::Builtin, "life", "life")
    );
    assert!(life.featured);
    assert!(lib.provenance("quickstart", "life").unwrap().is_empty());
    let rule = lib.get_quickstart("rule:B3/S23").unwrap();
    assert_eq!((rule.rule.as_str(), rule.featured), ("B3/S23", false));
    // A library that has them is not written when opened again.
    let stamp = lib.quickstart_stamp();
    assert_eq!(Library::open(lib.path()).quickstart_stamp(), stamp);

    // Another build's built-ins: one changed, one this build lacks, one missing, and one the
    // user hid.
    lib.set_quickstart_hidden("langtons-ant", true).unwrap();
    raw(&lib)
        .execute_batch(
            "UPDATE meta SET value = 'an older build' WHERE key = 'builtin_version';
             UPDATE quickstart SET title = 'Stale', created = '2026-01-01T00:00:00Z'
                WHERE handle = 'rule:B3/S23';
             DELETE FROM quickstart WHERE handle = 'wireworld';
             INSERT INTO quickstart (id, handle, source, target_kind, title, tiling_key,
                tiling_name, rule, rule_family, scene, pin_key, geometry_pin, engine_version,
                created, updated)
             SELECT 'rule:gone', 'rule:gone', 'builtin', 'scene', 'Gone', tiling_key,
                tiling_name, rule, rule_family, scene, pin_key, geometry_pin, engine_version,
                created, updated FROM quickstart WHERE handle = 'life';",
        )
        .unwrap();
    // Opening brings them up to date: the changed one written again (its creation kept), the
    // missing one back, the lacking one gone, the hidden one still hidden.
    let lib = Library::open(lib.path());
    assert_eq!(lib.quickstart(&[QsSource::Builtin]).unwrap().len(), n);
    let again = lib.get_quickstart("rule:B3/S23").unwrap();
    assert_eq!(again.title, rule.title);
    assert_eq!(again.created, "2026-01-01T00:00:00Z");
    assert!(lib.get_quickstart("wireworld").is_ok());
    assert!(lib.get_quickstart("rule:gone").is_err());
    assert!(lib.get_quickstart("langtons-ant").unwrap().hidden);
    assert_eq!(
        lib.meta("builtin_version").unwrap().as_deref(),
        Some(qs::builtin_version())
    );
    // The version names this engine and the rows' contents.
    assert!(qs::builtin_version().starts_with(jetcarp_core::pin::engine_geometry()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_atlas_is_published_into_the_table() {
    let (lib, dir) = temp_lib("publish");
    let (job, uuid, short) = publish(&lib, &sample());
    let atlas = lib.quickstart(&[QsSource::Atlas]).unwrap();
    assert_eq!(atlas.len(), 3);
    let first = lib.get_quickstart(&format!("atlas:{short}-1")).unwrap();
    assert_eq!(first.id, format!("atlas-{short}-1"));
    assert_eq!(first.source_ref, uuid);
    assert_eq!(
        (first.target_kind.as_str(), first.target_id.as_deref()),
        ("search_run", Some(format!("{uuid}:1").as_str()))
    );
    assert_eq!(first.title, "Life", "the rule's catalog name");
    assert_eq!(first.category, "Gliders & ships");
    assert!(first.preview.is_some());
    // Found by the search: its provenance names it and the run.
    let h = lib.provenance("quickstart", &first.id).unwrap();
    assert_eq!(
        (
            h[0].kind.as_str(),
            h[0].search_kind.as_str(),
            h[0].job_uuid.as_str(),
            h[0].source.clone()
        ),
        (
            "found",
            "explore",
            uuid.as_str(),
            format!("search_run:{uuid}:1")
        )
    );
    // Published again with one run chosen, after one of the others was hidden: the chosen one
    // written again, the hidden one kept (hidden), the third deleted.
    lib.set_quickstart_hidden(&format!("atlas:{short}-2"), true)
        .unwrap();
    let rows = vec![atlas_row(&uuid, &sample()[0], 0)];
    let w = lib.publish_atlas(job, &uuid, &rows).unwrap();
    assert_eq!((w.added, w.updated, w.removed), (0, 1, 1));
    let left: Vec<(String, bool)> = lib
        .quickstart(&[QsSource::Atlas])
        .unwrap()
        .into_iter()
        .map(|q| (q.handle, q.hidden))
        .collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(left.contains(&(format!("atlas:{short}-2"), true)));
    assert!(left.contains(&(format!("atlas:{short}-1"), false)));
    // Another search's atlas is its own: publishing it leaves this one alone, and a row of
    // another search is refused.
    let (_, other, _) = publish(&lib, &sample()[..1]);
    assert_eq!(lib.quickstart(&[QsSource::Atlas]).unwrap().len(), 3);
    assert!(
        lib.publish_atlas(job, &uuid, &[atlas_row(&other, &sample()[0], 0)])
            .is_err()
    );
    // Writing the built-ins again leaves the atlas alone.
    raw(&lib)
        .execute(
            "UPDATE meta SET value = '' WHERE key = 'builtin_version'",
            [],
        )
        .unwrap();
    let lib = Library::open(lib.path());
    assert_eq!(lib.quickstart(&[QsSource::Atlas]).unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cards_and_scenes_are_read_by_handle() {
    let (lib, dir) = temp_lib("cards");
    let (_, _, short) = publish(&lib, &sample());
    // An atlas card: its preview (small) and its mature state; the full scene is what opens.
    let h2 = format!("atlas:{short}-2");
    let card = qs::card(&lib, &h2).unwrap();
    assert!(card.own_preview);
    assert_eq!(card.preview.tiling.family, "square");
    assert_eq!(card.mature_gen, 300);
    let state = card.mature.clone().unwrap();
    assert_eq!(state.len(), 32 * 24);
    let mut w = World::from_scene(&card.preview).unwrap();
    qs::apply_mature(&mut w, &state, card.mature_gen);
    assert_eq!(w.generation, 300);
    assert!(w.fields[0].state.iter().all(|&s| s == 1));
    assert_eq!(
        w.cas[0].palette_neg, "Ink",
        "the entry's second palette is respected"
    );
    let full = qs::scene(&lib, &h2).unwrap();
    assert!(full.tiling.size != card.preview.tiling.size);
    assert!(
        qs::card(&lib, &format!("atlas:{short}-1"))
            .unwrap()
            .mature
            .is_none()
    );
    assert!(qs::scene(&lib, "atlas:nonesuch").is_err());
    // A built-in by its handle: its scene as built; its card the scene made smaller.
    let built = jetcarp_core::quickstart::entries()
        .into_iter()
        .find(|e| e.id == "life")
        .unwrap();
    assert_eq!(
        qs::scene(&lib, "life").unwrap().to_json(),
        built.scene.to_json()
    );
    let c = qs::card(&lib, "life").unwrap();
    assert!(!c.own_preview && c.mature.is_none());
    // A promotion by its handle, its id, or find:<id>; opened and removed by its handle.
    lib.put_quickstart(&QuickStartRow::for_scene(
        "my-soup",
        "scene",
        life_scene("B3/S23", 40, 40),
    ))
    .unwrap();
    for k in ["find:my-soup", "my-soup"] {
        assert_eq!(lib.get_quickstart(k).unwrap().handle, "find:my-soup");
    }
    lib.note_quickstart_open("find:my-soup").unwrap();
    let u = lib.quickstart_usage().unwrap();
    assert_eq!((u[0].handle.as_str(), u[0].opens), ("find:my-soup", 1));
    lib.remove_quickstart("find:my-soup").unwrap();
    assert!(lib.get_quickstart("my-soup").is_err());
    // A library with no file yet: its built-ins are found by handle, its menu lists them, and
    // reading makes no file; the first write makes it, with the built-ins.
    let fresh = Library::open(dir.join("fresh"));
    assert!(!fresh.path().exists());
    assert!(qs::scene(&fresh, "langtons-ant").is_ok());
    assert!(qs::card(&fresh, "langtons-ant").is_ok());
    assert!(qs::scene(&fresh, "atlas:nonesuch").is_err());
    assert_eq!(menu::load(&fresh).len(), qs::builtin_rows().len());
    assert!(!fresh.path().exists());
    fresh.note_quickstart_open("langtons-ant").unwrap();
    assert!(fresh.path().exists());
    assert_eq!(
        fresh.quickstart(&[QsSource::Builtin]).unwrap().len(),
        qs::builtin_rows().len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hidden_entries_leave_the_menu_and_packs() {
    let (lib, dir) = temp_lib("hidden");
    let (_, _, short) = publish(&lib, &sample());
    for id in ["keep", "drop"] {
        lib.put_quickstart(&QuickStartRow::for_scene(
            id,
            "scene",
            life_scene("B36/S23", 40, 40),
        ))
        .unwrap();
    }
    let gone = [
        "find:drop".to_string(),
        format!("atlas:{short}-3"),
        "life".to_string(),
    ];
    for h in &gone {
        assert!(lib.set_quickstart_hidden(h, true).unwrap().hidden);
    }
    let shown: Vec<String> = menu::load(&lib).into_iter().map(|i| i.handle).collect();
    for h in &gone {
        assert!(!shown.contains(h), "{h} is hidden");
    }
    assert!(shown.contains(&"find:keep".to_string()));
    assert!(shown.contains(&format!("atlas:{short}-1")));
    // A pack takes what is shown, but no built-in.
    let packed: Vec<String> = lib
        .pack_quickstart()
        .unwrap()
        .into_iter()
        .map(|q| q.row.handle)
        .collect();
    let mut want = vec![
        "find:keep".to_string(),
        format!("atlas:{short}-1"),
        format!("atlas:{short}-2"),
    ];
    want.sort();
    let mut got = packed.clone();
    got.sort();
    assert_eq!(got, want);
    // Shown again.
    lib.set_quickstart_hidden("find:drop", false).unwrap();
    assert!(menu::load(&lib).iter().any(|i| i.handle == "find:drop"));
    assert!(lib.set_quickstart_hidden("find:nonesuch", true).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn seed_descriptions_read_well() {
    assert_eq!(
        qs::describe_seed(r#"{"pattern":{"shape":{"kind":"noise"},"fill":{"density":0.85}}}"#),
        "noise pattern, density 0.85"
    );
    assert_eq!(
        qs::describe_seed(r#"{"random":{"density":0.123}}"#),
        "random, density 0.12"
    );
    assert_eq!(
        qs::describe_seed(r#"{"ants":{"count":8}}"#),
        "ants, count 8"
    );
    assert_eq!(qs::describe_seed("not json"), "not json");
}

#[test]
fn menu_filters_and_counts_atlas_items() {
    let (lib, dir) = temp_lib("menu-atlas");
    let (_, _, short) = publish(&lib, &sample());
    let items = menu::load(&lib);
    let atlas_n = items.iter().filter(|i| i.source == QsSource::Atlas).count();
    assert_eq!(atlas_n, 3);
    let h = |n: i64| format!("atlas:{short}-{n}");
    let glider = items.iter().find(|i| i.handle == h(1)).unwrap();
    assert_eq!(glider.behaviour, "Gliders & ships");
    assert_eq!(glider.family, "Life-like");
    assert_eq!(glider.grid_group, "Squares");
    assert_eq!(glider.tiling, "Square");
    assert!(glider.has_preview);
    let k = items.iter().find(|i| i.handle == h(2)).unwrap();
    assert_eq!(k.grid_group, "k-uniform");
    assert_eq!(k.family, "Generations");
    // Text search reaches rule, tiling, family, category, and seed description.
    let text = |t: &str| Filter {
        text: t.into(),
        ..Default::default()
    };
    let hits = |t: &str| menu::select(&items, &text(t), Sort::Score);
    let atlas_hits = |t: &str| -> Vec<&str> {
        hits(t)
            .iter()
            .filter(|&&i| items[i].source == QsSource::Atlas)
            .map(|&i| items[i].handle.as_str())
            .collect()
    };
    assert_eq!(atlas_hits("noise"), [h(2)]);
    assert_eq!(atlas_hits("gliders"), [h(1)]);
    assert_eq!(atlas_hits("generations"), [h(2)]);
    assert_eq!(atlas_hits("k2-001"), [h(2)]);
    assert_eq!(atlas_hits("turbulence b3/s23"), [h(3)]);
    // Facet counts: the category row counts over the other filters.
    let tf = Filter {
        behaviour: "Turbulence".into(),
        ..Default::default()
    };
    let fc = menu::facets_for(&items, &tf);
    assert_eq!(
        fc.categories
            .iter()
            .find(|(c, _)| c == "Turbulence")
            .unwrap()
            .1,
        2
    );
    assert_eq!(
        fc.grids.iter().find(|(g, _)| g == "Archimedean").unwrap().1,
        1,
        "other dimensions are counted within the selected category"
    );
    assert!(
        !fc.sources
            .iter()
            .any(|(o, n)| *o == QsSource::Atlas && *n == 3),
        "the source row counts only within the category"
    );
    let total: usize = fc.categories.iter().map(|x| x.1).sum();
    assert_eq!(fc.all, total);
    // Sort by rule, by tiling.
    let by_rule = menu::select(&items, &text("atlas"), Sort::Rule);
    let by_rule: Vec<&str> = by_rule.iter().map(|&i| items[i].rule.as_str()).collect();
    assert!(by_rule.windows(2).all(|w| w[0] <= w[1]));
    let by_tiling = menu::select(&items, &text("atlas"), Sort::Tiling);
    let by_t: Vec<&str> = by_tiling
        .iter()
        .map(|&i| items[i].tiling.as_str())
        .collect();
    assert!(by_t.windows(2).all(|w| w[0] <= w[1]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A named rule's notation is reachable when a menu item has that canonical rule.
fn reachable(items: &[menu::Item], notation: &str) -> bool {
    let want = canonical_rule(notation);
    items.iter().any(|i| canonical_rule(&i.rule) == want)
}

#[test]
fn every_named_rule_is_reachable_with_and_without_the_atlas() {
    let (lib, dir) = temp_lib("reach");
    // No atlas: every built-in is there, so every named rule is.
    let items = menu::load(&lib);
    assert_eq!(items.len(), qs::builtin_rows().len());
    let unreachable: Vec<&str> = catalog()
        .iter()
        .filter(|e| !reachable(&items, e.notation))
        .map(|e| e.notation)
        .collect();
    assert!(unreachable.is_empty(), "not reachable: {unreachable:?}");
    let built_in_life = items.iter().filter(|i| i.handle == "rule:B3/S23").count();
    assert_eq!(built_in_life, 1);

    // An atlas covering Life: Life's plain catalog entry goes, the featured scene stays, and
    // Life is reachable through the atlas.
    publish(&lib, &sample());
    let items = menu::load(&lib);
    assert_eq!(
        items.iter().filter(|i| i.handle == "rule:B3/S23").count(),
        0,
        "the atlas covers Life"
    );
    assert!(
        items
            .iter()
            .any(|i| i.source == QsSource::Builtin && i.featured && i.handle == "life"),
        "hand-made featured scenes stay"
    );
    let unreachable: Vec<&str> = catalog()
        .iter()
        .filter(|e| !reachable(&items, e.notation))
        .map(|e| e.notation)
        .collect();
    assert!(unreachable.is_empty(), "not reachable: {unreachable:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn featured_shuffle_mixes_the_best_of_every_category() {
    let (lib, dir) = temp_lib("shuffle");
    // Three categories of 60 entries each, scores decreasing with id.
    let cats = ["Gliders & ships", "Turbulence", "Quick deaths"];
    let mut runs = Vec::new();
    for c in cats {
        for k in 0..60 {
            let rule: &'static str = Box::leak(format!("B3{k}/S23").into_boxed_str());
            let id = runs.len() as i64 + 1;
            runs.push(Run {
                id,
                category: c,
                family: "life_like",
                tiling: "square",
                group: "square",
                rule,
                seed: "{}",
                score: 100.0 - id as f64,
            });
        }
    }
    let (_, _, short) = publish(&lib, &runs);
    let mut items = menu::load(&lib);
    items.retain(|i| i.source == QsSource::Atlas);
    assert_eq!(items.len(), 180);
    let order_with = |items: &mut Vec<menu::Item>, seed: u64| -> Vec<usize> {
        menu::shuffle(items, seed);
        menu::select(items, &Filter::default(), Sort::Featured)
    };
    let a = order_with(&mut items, 11);
    assert_eq!(a, order_with(&mut items, 11), "same seed, same order");
    assert_ne!(a, order_with(&mut items, 12), "another seed, another order");
    let a = order_with(&mut items, 11);
    let cat_of =
        |o: &[usize]| -> Vec<&str> { o.iter().map(|&i| items[i].behaviour.as_str()).collect() };
    let top80: Vec<&str> = cat_of(&a[..80]);
    // The top is a mix of the non-dull categories' best; the dull category waits its turn.
    assert!(top80.iter().filter(|c| **c == "Quick deaths").count() == 0);
    assert!(top80.contains(&"Gliders & ships") && top80.contains(&"Turbulence"));
    // Highest-scoring: every entry of the first band (the best 40) of Gliders outranks the
    // category's weakest.
    let pos_of = |h: &str| -> usize { a.iter().position(|&i| items[i].handle == h).unwrap() };
    let best = (1..=40)
        .map(|k| pos_of(&format!("atlas:{short}-{k}")))
        .max()
        .unwrap();
    assert!(
        best < pos_of(&format!("atlas:{short}-60")),
        "the band's entries come first"
    );
    // Selecting a category keeps the same order as the shuffle.
    let turb = menu::select(
        &items,
        &Filter {
            behaviour: "Turbulence".into(),
            ..Default::default()
        },
        Sort::Featured,
    );
    assert!(
        turb.windows(2)
            .all(|w| items[w[0]].shuffle_rank < items[w[1]].shuffle_rank)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_featured_order_survives_a_reload() {
    let (lib, dir) = temp_lib("keep-order");
    let mut runs = Vec::new();
    for c in ["Gliders & ships", "Turbulence"] {
        for k in 0..50 {
            let rule: &'static str = Box::leak(format!("B3{k}/S23").into_boxed_str());
            let id = runs.len() as i64 + 1;
            runs.push(Run {
                id,
                category: c,
                family: "life_like",
                tiling: "square",
                group: "square",
                rule,
                seed: "{}",
                score: 100.0 - id as f64,
            });
        }
    }
    publish(&lib, &runs);
    let featured = |items: &[menu::Item]| -> Vec<String> {
        menu::select(items, &Filter::default(), Sort::Featured)
            .into_iter()
            .map(|i| items[i].handle.clone())
            .collect()
    };
    let load = || {
        let mut items = menu::load(&lib);
        menu::shuffle(&mut items, 11);
        items
    };
    let before = load();
    let order = featured(&before);
    // Hiding an entry reloads the menu: the same seed alone moves the cards after it.
    let hidden = order[3].clone();
    lib.set_quickstart_hidden(&hidden, true).unwrap();
    let mut after = load();
    let unkept: Vec<String> = featured(&after);
    let expected: Vec<String> = order.iter().filter(|h| **h != hidden).cloned().collect();
    assert_ne!(unkept, expected, "a reload reshuffles without keep_order");
    menu::keep_order(&mut after, &before);
    assert_eq!(
        featured(&after),
        expected,
        "every other card keeps its place"
    );
    // An entry that comes back takes a place of its own, and the rest still keep theirs.
    lib.set_quickstart_hidden(&hidden, false).unwrap();
    let mut again = load();
    menu::keep_order(&mut again, &after);
    let now: Vec<String> = featured(&again)
        .into_iter()
        .filter(|h| *h != hidden)
        .collect();
    assert_eq!(now, expected);
    let _ = std::fs::remove_dir_all(&dir);
}

fn scene_row(id: &str, title: &str) -> QuickStartRow {
    let e = jetcarp_core::quickstart::entries()
        .into_iter()
        .find(|e| e.id == "rule:B3/S23")
        .unwrap();
    let mut q = QuickStartRow::for_scene(id, "scene", e.scene);
    q.title = title.into();
    q.behaviour = "soup".into();
    q
}

#[test]
fn menu_has_every_source_categories_and_search() {
    let (lib, dir) = temp_lib("menu");
    lib.put_quickstart(&scene_row("my-life", "My Life soup"))
        .unwrap();
    let mut waiting = scene_row("waiting", "Waiting");
    waiting.hidden = true;
    lib.put_quickstart(&waiting).unwrap();
    // An entry from a pack: its behaviour, a menu category, is its category.
    let mut packed = scene_row("qs-00112233445566778899aabbccddeeff", "From a pack");
    packed.source = QsSource::Pack;
    packed.handle = packed.id.clone();
    packed.source_ref = "sha256:0011".into();
    packed.behaviour = "Gliders & ships".into();
    packed.category.clear();
    lib.put_quickstart(&packed).unwrap();
    let mut items = menu::load(&lib);
    // Every built-in (only an atlas entry stands in for a plain built-in of its rule) and the two
    // shown entries; the hidden one is left out.
    assert_eq!(items.len(), qs::builtin_rows().len() + 2);
    assert!(items.iter().any(|i| i.handle == "rule:B3/S23"));
    assert!(items.iter().all(|i| i.handle != "find:waiting"));
    let mine = items.iter().find(|i| i.handle == "find:my-life").unwrap();
    assert_eq!(
        (
            mine.source,
            mine.family.as_str(),
            mine.grid_group.as_str(),
            mine.behaviour.as_str()
        ),
        (QsSource::Promotion, "Life-like", "Squares", "My finds")
    );
    let pack = items.iter().find(|i| i.source == QsSource::Pack).unwrap();
    assert_eq!(pack.behaviour, "Gliders & ships");
    // Built-ins get a category and a grid group.
    let ant = items.iter().find(|i| i.handle == "langtons-ant").unwrap();
    assert_eq!(ant.behaviour, "Ants & turmites");
    let r30 = items.iter().find(|i| i.handle == "rule:W30").unwrap();
    assert_eq!(r30.behaviour, "Fractals & rows");
    assert!(items.iter().any(|i| i.grid_group == "Aperiodic"));
    assert!(items.iter().any(|i| i.grid_group == "Archimedean"));
    assert!(items.iter().all(|i| i.source != QsSource::Atlas));

    // Search: every word must match, in any field.
    let f = |text: &str| Filter {
        text: text.into(),
        ..Default::default()
    };
    let hits = menu::select(&items, &f("my life"), Sort::Featured);
    assert_eq!(hits.len(), 1);
    assert_eq!(items[hits[0]].handle, "find:my-life");
    assert!(menu::select(&items, &f("zzz-nothing"), Sort::Featured).is_empty());
    // Categories.
    let fam = Filter {
        family: "Life-like".into(),
        grid: "Squares".into(),
        source: Some(QsSource::Promotion),
        ..Default::default()
    };
    let v = menu::select(&items, &fam, Sort::Rule);
    assert_eq!(v.len(), 1);
    assert_eq!(items[v[0]].handle, "find:my-life");
    let beh = menu::facets(&items, |i| &i.behaviour);
    assert!(beh.iter().any(|(b, _)| b == "Classics"));
    let fc = menu::facets_for(&items, &Filter::default());
    let sources: Vec<QsSource> = fc.sources.iter().map(|s| s.0).collect();
    assert_eq!(
        sources,
        [QsSource::Promotion, QsSource::Pack, QsSource::Builtin]
    );
    // The built-in source is not called after the "Classics" category.
    assert_eq!(QsSource::Builtin.label(), "Built-in");
    assert!(
        items
            .iter()
            .filter(|i| i.source == QsSource::Builtin)
            .any(|i| i.matches("built-in"))
    );
    // Sorts: rules ascend; a shuffle is reproducible and keeps every entry.
    let all = Filter::default();
    let rules: Vec<String> = menu::select(&items, &all, Sort::Rule)
        .iter()
        .map(|&i| items[i].rule.clone())
        .collect();
    assert!(rules.windows(2).all(|w| w[0] <= w[1]));
    menu::shuffle(&mut items, 7);
    let feat = menu::select(&items, &all, Sort::Featured);
    assert_eq!(feat.len(), items.len());
    menu::shuffle(&mut items, 7);
    assert_eq!(feat, menu::select(&items, &all, Sort::Featured));

    // A user rating of an entry and its opens show in the next load; a built-in is rated by
    // its handle too.
    let mine = items.iter().find(|i| i.handle == "find:my-life").unwrap();
    assert_eq!(
        menu::rate(&lib, mine, 5).unwrap(),
        ("quickstart".to_string(), "my-life".to_string())
    );
    assert!(menu::rate(&lib, mine, 6).is_err());
    let r = lib.ratings(Some("quickstart")).unwrap();
    assert_eq!(
        (
            r.len(),
            r[0].target_id.as_str(),
            r[0].rating,
            r[0].rater.as_str()
        ),
        (1, "my-life", 5.0, "user")
    );
    menu::rate_handle(&lib, "wireworld", 3).unwrap();
    lib.note_quickstart_open("find:my-life").unwrap();
    lib.note_quickstart_open("find:my-life").unwrap();
    let items = menu::load(&lib);
    let top = &items[menu::popular(&items, 1)[0]];
    assert_eq!(
        (top.handle.as_str(), top.opens, top.rating),
        ("find:my-life", 2, Some(5.0))
    );
    let ww = items.iter().find(|i| i.handle == "wireworld").unwrap();
    assert_eq!(ww.rating, Some(3.0));
    let _ = std::fs::remove_dir_all(&dir);
}

fn usage(handle: &str, opens: u64, last: &str) -> UsageRow {
    UsageRow {
        handle: handle.into(),
        opens,
        first_opened: Some("2026-01-01T00:00:00Z".into()),
        last_opened: Some(last.into()),
    }
}

/// The built-in entries as menu items, without a library.
fn built_in_items() -> Vec<menu::Item> {
    menu::items_of(qs::builtin_rows().iter().map(MenuRow::of).collect())
}

#[test]
fn popular_recent_and_random_rows() {
    let mut items = built_in_items();
    let usage = vec![
        usage("life", 3, "2026-11-01T10:00:00Z"),
        usage("rule:B3/S23", 3, "2026-11-02T10:00:00Z"),
        usage("wireworld", 5, "2026-10-01T10:00:00Z"),
        usage("brians-brain", 1, "2026-11-05T10:00:00Z"),
    ];
    // A rating breaks the tie between the two entries opened three times.
    let mut ratings = HashMap::new();
    ratings.insert(("quickstart".to_string(), "life".to_string()), 5.0);
    menu::apply_usage(&mut items, &usage, &ratings);
    fn handles_of(items: &[menu::Item], v: Vec<usize>) -> Vec<String> {
        v.into_iter().map(|i| items[i].handle.clone()).collect()
    }
    let ids = |v: Vec<usize>| handles_of(&items, v);
    assert_eq!(
        ids(menu::popular(&items, 10)),
        ["wireworld", "life", "rule:B3/S23", "brians-brain"]
    );
    assert_eq!(
        ids(menu::recent(&items, 2)),
        ["brians-brain", "rule:B3/S23"]
    );
    // Without the rating, the later open wins the tie.
    let mut plain = built_in_items();
    menu::apply_usage(&mut plain, &usage, &HashMap::new());
    let p: Vec<String> = menu::popular(&plain, 3)
        .into_iter()
        .map(|i| plain[i].handle.clone())
        .collect();
    assert_eq!(p, ["wireworld", "rule:B3/S23", "life"]);
    // Opening counts at once.
    menu::note_open(&mut items, "hex-wolfram", "2026-11-06T00:00:00Z");
    assert_eq!(items[menu::recent(&items, 1)[0]].handle, "hex-wolfram");
    let ids = |v: Vec<usize>| handles_of(&items, v);

    // Random: the same seed gives the same draw, another seed another; no repeats; the order
    // of the menu does not matter.
    let a = ids(menu::random(&items, 42, 12));
    let b = ids(menu::random(&items, 42, 12));
    let c = ids(menu::random(&items, 43, 12));
    assert_eq!(a, b);
    assert_ne!(a, c);
    let mut d = a.clone();
    d.sort();
    d.dedup();
    assert_eq!(d.len(), 12);
    let mut rev = items.clone();
    rev.reverse();
    let r: Vec<String> = menu::random(&rev, 42, 12)
        .into_iter()
        .map(|i| rev[i].handle.clone())
        .collect();
    assert_eq!(r, a);
    assert_eq!(menu::random(&items, 1, 100_000).len(), items.len());
}

/// Schema 2's `quickstart` table, as the libraries of development builds before the 0.4.0
/// release have it.
const QUICKSTART_2: &str = "
CREATE TABLE quickstart (
    id TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('pattern', 'sweep', 'scene')),
    source_id TEXT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    grid TEXT NOT NULL,
    tiling TEXT,
    rule TEXT NOT NULL,
    rule_family TEXT NOT NULL,
    behaviour TEXT NOT NULL DEFAULT '',
    scene TEXT NOT NULL,
    score REAL,
    status TEXT NOT NULL DEFAULT 'approved'
        CHECK (status IN ('auto', 'candidate', 'approved', 'rejected')),
    pin_key TEXT NOT NULL,
    geometry_pin TEXT NOT NULL,
    needs_reverify INTEGER NOT NULL DEFAULT 0,
    engine_version TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);
CREATE INDEX quickstart_status ON quickstart (status, score DESC);
CREATE INDEX quickstart_source ON quickstart (source_kind, source_id);
CREATE INDEX quickstart_pin ON quickstart (pin_key, geometry_pin);
";

#[test]
fn a_schema_2_library_keeps_its_quick_start_entries() {
    let (lib, dir) = temp_lib("schema2");
    lib.put_quickstart(&scene_row("pinned", "Pinned")).unwrap();
    let path = lib.path().to_path_buf();
    drop(lib);
    // The same library as schema 2 had it: a pattern's entry, a sweep run's, an ant's scene, a
    // pack's (with the pack its provenance names), and a rejected one; opens by entry id.
    {
        let c = Connection::open(&path).unwrap();
        let (scene, pin_key, pin): (String, String, String) = c
            .query_row(
                "SELECT scene, pin_key, geometry_pin FROM quickstart WHERE id = 'pinned'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        c.execute_batch(&format!(
            "DROP TABLE quickstart; {QUICKSTART_2}
             DROP TABLE quickstart_usage;
             CREATE TABLE quickstart_usage (entry_id TEXT PRIMARY KEY,
                opens INTEGER NOT NULL DEFAULT 0, first_opened TEXT, last_opened TEXT);
             INSERT INTO quickstart_usage VALUES ('find:abcd', 2, 'x', 'y');
             INSERT INTO provenance (target_kind, target_id, seq, kind, pack_hash,
                engine_version, geometry_pin, date)
                VALUES ('quickstart', 'qs-1234', 0, 'imported', 'sha256:feed', '0.4.0', '', 'x');
             UPDATE meta SET value = '2' WHERE key = 'schema';
             PRAGMA user_version = 2;"
        ))
        .unwrap();
        let rows = [
            ("abcd", "pattern", Some("abcd"), "spaceship", "approved"),
            ("sweep-99", "sweep", Some("W:1"), "", "auto"),
            ("ant-77", "scene", None, "highway", "approved"),
            ("qs-1234", "scene", None, "Gliders & ships", "approved"),
            ("nope", "scene", None, "", "rejected"),
        ];
        for (id, kind, src, behaviour, status) in rows {
            c.execute(
                "INSERT INTO quickstart (id, source_kind, source_id, title, grid, tiling, rule,
                    rule_family, behaviour, scene, status, pin_key, geometry_pin,
                    engine_version, created, updated)
                 VALUES (?1, ?2, ?3, ?1, 'Square', 'square', 'B3/S23', 'lifelike', ?4, ?5, ?6,
                    ?7, ?8, '0.4.0', 'c', 'u')",
                rusqlite::params![id, kind, src, behaviour, scene, status, pin_key, pin],
            )
            .unwrap();
        }
    }
    let lib = Library::open(&path);
    assert_eq!(lib.info().unwrap().schema_version, 3);
    let got = |id: &str| lib.get_quickstart(id).unwrap();
    let a = got("abcd");
    assert_eq!(
        (
            a.handle.as_str(),
            a.source,
            a.target_kind.as_str(),
            a.target_id.as_deref(),
            a.category.as_str()
        ),
        (
            "find:abcd",
            QsSource::Promotion,
            "pattern",
            Some("abcd"),
            "Gliders & ships"
        )
    );
    assert_eq!(
        (got("sweep-99").target_kind.as_str(), got("sweep-99").hidden),
        ("sweep_run", false)
    );
    assert_eq!(
        (
            got("ant-77").target_kind.as_str(),
            got("ant-77").target_id.as_deref()
        ),
        ("ant", Some("ant-77"))
    );
    let p = got("qs-1234");
    assert_eq!(
        (
            p.handle.as_str(),
            p.source,
            p.source_ref.as_str(),
            p.category.as_str()
        ),
        ("qs-1234", QsSource::Pack, "sha256:feed", "Gliders & ships")
    );
    assert!(got("find:nope").hidden);
    // Dates kept; opens counted by handle; the built-ins written.
    assert_eq!((a.created.as_str(), a.updated.as_str()), ("c", "u"));
    let u = lib.quickstart_usage().unwrap();
    assert_eq!((u[0].handle.as_str(), u[0].opens), ("find:abcd", 2));
    assert_eq!(
        lib.quickstart(&[QsSource::Builtin]).unwrap().len(),
        qs::builtin_rows().len()
    );
    assert_eq!(lib.meta("schema").unwrap().as_deref(), Some("3"));
    let _ = std::fs::remove_dir_all(&dir);
}
