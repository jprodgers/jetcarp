//! Quick Start's atlas source: the file lookup, cards and full scenes read by id, the menu's
//! filters and counts with atlas items, the Featured shuffle, and that every named rule stays
//! reachable when the atlas covers some of them.

use jetcarp_core::explore::atlas::{self, AtlasEntry};
use jetcarp_core::library::Library;
use jetcarp_core::qsmenu::{self, Filter, Origin, Sort, canonical_rule};
use jetcarp_core::rules::catalog;
use jetcarp_core::topology::TilingSpec;
use jetcarp_core::{CaSpec, Scene, World};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jetcarp-qsa-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"sqlite").unwrap();
}

#[test]
fn atlas_lookup_order() {
    let dir = temp_dir("locate");
    let env_file = dir.join("mine.sqlite");
    let docs = dir.join("docs");
    let exe = dir
        .join("repo")
        .join("target")
        .join("release")
        .join("jetcarp-app");
    // Nothing anywhere.
    assert_eq!(
        atlas::locate_from(None, Some(&docs), Some(&exe)),
        None,
        "no atlas in any place"
    );
    // The repo's copy, found walking up from the executable.
    let repo_copy = dir.join("repo").join("scenes").join("atlas.sqlite");
    touch(&repo_copy);
    assert_eq!(
        atlas::locate_from(None, Some(&docs), Some(&exe)),
        Some(repo_copy.clone())
    );
    // The Documents copy wins over the repo's.
    let doc_copy = docs.join("jetcarp-out").join("explore").join("atlas.sqlite");
    touch(&doc_copy);
    assert_eq!(
        atlas::locate_from(None, Some(&docs), Some(&exe)),
        Some(doc_copy.clone())
    );
    // $JETCARP_ATLAS wins over both, when it names a file (a missing file falls through).
    touch(&env_file);
    assert_eq!(
        atlas::locate_from(Some(env_file.as_os_str()), Some(&docs), Some(&exe)),
        Some(env_file.clone())
    );
    let missing = dir.join("nope.sqlite");
    assert_eq!(
        atlas::locate_from(Some(missing.as_os_str()), Some(&docs), Some(&exe)),
        Some(doc_copy)
    );
}

fn life_scene(rule: &str, w: u32, h: u32) -> Scene {
    let mut s = Scene::new(TilingSpec::rect("square", w, h, jetcarp_core::Boundary::Wrap));
    let mut ca = CaSpec::new(rule);
    ca.palette = Some("Ember".into());
    ca.palette_neg = Some("Ink".into());
    s.cas = vec![ca];
    s
}

fn deflate(state: &[u8]) -> Vec<u8> {
    let mut e = DeflateEncoder::new(Vec::new(), Compression::fast());
    e.write_all(state).unwrap();
    e.finish().unwrap()
}

struct Row<'a> {
    id: i64,
    category: &'a str,
    family: &'a str,
    tiling: &'a str,
    group: &'a str,
    rule: &'a str,
    seed: &'a str,
    score: f64,
}

/// A small atlas with one mature entry (the second).
fn write_atlas(path: &Path, rows: &[Row]) {
    let conn = atlas::create(path).unwrap();
    let mut st = conn
        .prepare(
            "INSERT INTO entries(id,title,category,family,tiling,tiling_group,rule,seed,score,
             preview,full,mature_gen,mature) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .unwrap();
    for (k, r) in rows.iter().enumerate() {
        let preview = life_scene(r.rule, 32, 24);
        let full = life_scene(r.rule, 240, 240);
        let mature = (k == 1).then(|| deflate(&vec![1u8; 32 * 24]));
        st.execute(rusqlite::params![
            r.id,
            format!("{} on {}", r.rule, r.tiling),
            r.category,
            r.family,
            r.tiling,
            r.group,
            r.rule,
            r.seed,
            r.score,
            serde_json::to_string(&preview).unwrap(),
            serde_json::to_string(&full).unwrap(),
            if k == 1 { 300 } else { 0 },
            mature,
        ])
        .unwrap();
    }
}

fn sample() -> Vec<Row<'static>> {
    vec![
        Row {
            id: 1,
            category: "Gliders & ships",
            family: "life_like",
            tiling: "square",
            group: "square",
            rule: "B3/S23",
            seed: r#"{"random":{"density":0.3}}"#,
            score: 3.0,
        },
        Row {
            id: 2,
            category: "Turbulence",
            family: "generations",
            tiling: "catalog:k2-001",
            group: "k-uniform",
            rule: "B2/S/C3",
            seed: r#"{"pattern":{"shape":{"kind":"noise"},"fill":{"density":0.85}}}"#,
            score: 2.0,
        },
        Row {
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
fn cards_and_full_scenes_read_by_id() {
    let dir = temp_dir("cards");
    let file = dir.join("atlas.sqlite");
    write_atlas(&file, &sample());
    let metas = atlas::load_meta(&file).unwrap();
    assert_eq!(metas.len(), 3);
    assert_eq!(metas[0].score, 3.0, "best score first");
    assert!(metas[1].has_mature && !metas[0].has_mature);
    // A card: the preview (small) and the mature state; no full scene needed.
    let conn = atlas::open(&file).unwrap();
    let card = atlas::load_card(&conn, 2).unwrap();
    assert_eq!(card.preview.tiling.family, "square");
    assert_eq!(card.mature_gen, 300);
    let state = atlas::inflate_state(card.mature.as_deref().unwrap()).unwrap();
    assert_eq!(state.len(), 32 * 24);
    let mut w = World::from_scene(&card.preview).unwrap();
    atlas::apply_mature(&mut w, &state, card.mature_gen);
    assert_eq!(w.generation, 300);
    assert!(w.fields[0].state.iter().all(|&s| s == 1));
    assert_eq!(
        w.cas[0].palette_neg, "Ink",
        "the entry's second palette is respected"
    );
    // The full scene is what opening loads: the large one.
    let full = atlas::load_full(&file, 2).unwrap();
    assert!(
        full.tiling.size != card.preview.tiling.size,
        "full and preview differ"
    );
    assert!(atlas::load_full(&file, 99).is_err());
    // The old all-in-one loader still works.
    let all: Vec<AtlasEntry> = atlas::load(&file).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(atlas::preview_world(&all[1]).unwrap().generation, 300);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn seed_descriptions_read_well() {
    assert_eq!(
        atlas::describe_seed(r#"{"pattern":{"shape":{"kind":"noise"},"fill":{"density":0.85}}}"#),
        "noise pattern, density 0.85"
    );
    assert_eq!(
        atlas::describe_seed(r#"{"random":{"density":0.123}}"#),
        "random, density 0.12"
    );
    assert_eq!(
        atlas::describe_seed(r#"{"ants":{"count":8}}"#),
        "ants, count 8"
    );
    assert_eq!(atlas::describe_seed("not json"), "not json");
}

fn temp_lib(name: &str) -> (Library, PathBuf) {
    let dir = temp_dir(name);
    (Library::open(dir.join("catalog")), dir)
}

#[test]
fn menu_filters_and_counts_atlas_items() {
    let (lib, dir) = temp_lib("menu");
    let file = dir.join("atlas.sqlite");
    write_atlas(&file, &sample());
    let items = qsmenu::load_with(&lib, Some(&file));
    let atlas_n = items.iter().filter(|i| i.origin == Origin::Atlas).count();
    assert_eq!(atlas_n, 3);
    let glider = items.iter().find(|i| i.q.id == "atlas:1").unwrap();
    assert_eq!(glider.behaviour, "Gliders & ships");
    assert_eq!(glider.family, "Life-like");
    assert_eq!(glider.grid_group, "Squares");
    assert_eq!(glider.q.tiling, "Square");
    assert_eq!(glider.atlas.as_ref().unwrap().id, 1);
    let k = items.iter().find(|i| i.q.id == "atlas:2").unwrap();
    assert_eq!(k.grid_group, "k-uniform");
    assert_eq!(k.family, "Generations");
    // Text search reaches rule, tiling, family, category, and seed description.
    let text = |t: &str| Filter {
        text: t.into(),
        ..Default::default()
    };
    let hits = |t: &str| qsmenu::select(&items, &text(t), Sort::Score);
    let atlas_hits = |t: &str| -> Vec<&str> {
        hits(t)
            .iter()
            .filter(|&&i| items[i].origin == Origin::Atlas)
            .map(|&i| items[i].q.id.as_str())
            .collect()
    };
    assert_eq!(atlas_hits("noise"), ["atlas:2"]);
    assert_eq!(atlas_hits("gliders"), ["atlas:1"]);
    assert_eq!(atlas_hits("generations"), ["atlas:2"]);
    assert_eq!(atlas_hits("k2-001"), ["atlas:2"]);
    assert_eq!(atlas_hits("turbulence b3/s23"), ["atlas:3"]);
    // Facet counts: the category row counts over the other filters.
    let tf = Filter {
        behaviour: "Turbulence".into(),
        ..Default::default()
    };
    let fc = qsmenu::facets_for(&items, &tf);
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
        !fc.origins
            .iter()
            .any(|(o, n)| *o == Origin::Atlas && *n == 3),
        "the source row counts only within the category"
    );
    let total: usize = fc.categories.iter().map(|x| x.1).sum();
    assert_eq!(fc.all, total);
    // Sort by rule, by tiling.
    let by_rule = qsmenu::select(&items, &text("atlas"), Sort::Rule);
    let by_rule: Vec<&str> = by_rule.iter().map(|&i| items[i].q.rule.as_str()).collect();
    assert!(by_rule.windows(2).all(|w| w[0] <= w[1]));
    let by_tiling = qsmenu::select(&items, &text("atlas"), Sort::Tiling);
    let by_t: Vec<&str> = by_tiling
        .iter()
        .map(|&i| items[i].q.tiling.as_str())
        .collect();
    assert!(by_t.windows(2).all(|w| w[0] <= w[1]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A named rule's notation is reachable when a menu item has that canonical rule.
fn reachable(items: &[qsmenu::Item], notation: &str) -> bool {
    let want = canonical_rule(notation);
    items.iter().any(|i| canonical_rule(&i.q.rule) == want)
}

#[test]
fn every_named_rule_is_reachable_with_and_without_the_atlas() {
    let (lib, dir) = temp_lib("reach");
    // No atlas: every built-in is there, so every named rule is.
    let items = qsmenu::load_with(&lib, None);
    let unreachable: Vec<&str> = catalog()
        .iter()
        .filter(|e| !reachable(&items, e.notation))
        .map(|e| e.notation)
        .collect();
    assert!(unreachable.is_empty(), "not reachable: {unreachable:?}");
    let built_in_life = items.iter().filter(|i| i.q.id == "rule:B3/S23").count();
    assert_eq!(built_in_life, 1);

    // An atlas covering Life: Life's plain catalog entry goes, the featured scene stays, and
    // Life is reachable through the atlas.
    let file = dir.join("atlas.sqlite");
    write_atlas(&file, &sample());
    let items = qsmenu::load_with(&lib, Some(&file));
    assert_eq!(
        items.iter().filter(|i| i.q.id == "rule:B3/S23").count(),
        0,
        "the atlas covers Life"
    );
    assert!(
        items
            .iter()
            .any(|i| i.origin == Origin::Classics && i.q.featured && i.q.id == "life"),
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
    let file = dir.join("atlas.sqlite");
    // Three categories of 60 entries each, scores decreasing with id.
    let cats = ["Gliders & ships", "Turbulence", "Quick deaths"];
    let mut rows = Vec::new();
    for c in cats {
        for k in 0..60 {
            let rule: &'static str = Box::leak(format!("B3{k}/S23").into_boxed_str());
            let id = rows.len() as i64 + 1;
            rows.push(Row {
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
    write_atlas(&file, &rows);
    let mut items = qsmenu::load_with(&lib, Some(&file));
    items.retain(|i| i.origin == Origin::Atlas);
    let order_with = |items: &mut Vec<qsmenu::Item>, seed: u64| -> Vec<usize> {
        qsmenu::shuffle(items, seed);
        qsmenu::select(items, &Filter::default(), Sort::Featured)
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
    let pos_of = |id: &str| -> usize { a.iter().position(|&i| items[i].q.id == id).unwrap() };
    let best = (1..=40)
        .map(|k| pos_of(&format!("atlas:{k}")))
        .max()
        .unwrap();
    assert!(best < pos_of("atlas:60"), "the band's entries come first");
    // Selecting a category keeps the same order as the shuffle.
    let turb = qsmenu::select(
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
