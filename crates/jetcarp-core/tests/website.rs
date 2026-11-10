//! Website export (Phase 4 catalog deliverable 10): what is public, the manifest, and
//! incremental updates.

use jetcarp_core::library::{
    self, CurationState, DiscoveredBy, Library, PatternClass, PatternRecord, Provenance,
    QuickStartRow, Update, record_from_object,
};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::website::{self, ExportFilter, ExportOptions, Manifest};
use jetcarp_core::*;
use std::path::PathBuf;

fn temp(name: &str) -> (Library, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("jetcarp-site-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    (Library::open(root.join("catalog")), root.join("site"))
}

fn one(rule: &str, rows: &str) -> PatternRecord {
    let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells {
            cells: place_rows(&g, rows, 20, 20),
        }),
    })
    .unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let prov = Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: library::now_utc(),
        ..Default::default()
    };
    r.objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| record_from_object(&w, w.cas[0].id, o, prov.clone(), DiscoveredBy::User).unwrap())
        .next()
        .unwrap()
}

fn set_state(lib: &Library, id: &str, s: CurationState) {
    lib.update(
        id,
        &Update {
            state: Some(s),
            ..Default::default()
        },
    )
    .unwrap();
}

/// A catalog with a promoted glider, a reviewed blinker, a rejected block, a new beehive, and
/// one Quick Start soup.
fn catalog(lib: &Library) -> [String; 4] {
    let ids = [
        one("B3/S23", ".o./..o/ooo"),
        one("B3/S23", "ooo"),
        one("B3/S23", "oo/oo"),
        one("B3/S23", ".oo./o..o/.oo."),
    ]
    .map(|r| {
        let id = r.id.clone();
        lib.add(r).unwrap();
        id
    });
    set_state(lib, &ids[0], CurationState::Promoted);
    set_state(lib, &ids[1], CurationState::Reviewed);
    set_state(lib, &ids[2], CurationState::Rejected);
    let mut sc = Scene::new(TilingSpec::rect("square", 48, 32, Boundary::Wrap));
    sc.name = "Small soup".into();
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 })];
    lib.put_quickstart(&QuickStartRow::for_scene("soup", "scene", sc))
        .unwrap();
    ids
}

fn opts() -> ExportOptions {
    let mut o = ExportOptions::default();
    o.media.gens = Some(24);
    o
}

#[test]
fn export_writes_public_items_then_only_changes() {
    let (lib, dir) = temp("incr");
    let ids = catalog(&lib);
    let r1 = website::export(&lib, &dir, &opts()).unwrap();
    assert_eq!(
        (r1.patterns, r1.quickstart),
        (2, 1),
        "promoted + reviewed, one entry"
    );
    assert_eq!(r1.rendered, 3);
    assert!(r1.media_failed.is_empty(), "{:?}", r1.media_failed);
    // Items, media, and the manifest.
    let m = Manifest::read(&dir).unwrap();
    assert_eq!(m.format, website::FORMAT);
    assert_eq!(m.items.len(), 3);
    for it in &m.items {
        assert!(dir.join(&it.path).exists(), "{}", it.path);
        for k in ["anim", "poster", "thumb"] {
            let md = it
                .media
                .get(k)
                .unwrap_or_else(|| panic!("{} has no {k}", it.id));
            let bytes = std::fs::read(dir.join(&md.path)).unwrap();
            assert_eq!(media_hash(&bytes), md.hash, "{}", md.path);
        }
    }
    let glider = m.items.iter().find(|i| i.id == ids[0]).unwrap();
    assert_eq!(glider.media["anim"].mime, "image/gif");
    assert_eq!(glider.class, "spaceship");
    let soup = m.items.iter().find(|i| i.kind == "quickstart").unwrap();
    assert_eq!(soup.media["anim"].mode, "seed");
    // The pattern JSON has the rule-free record and its rule range.
    let pj: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(&glider.path)).unwrap()).unwrap();
    assert_eq!(pj["range"]["min_rule"], "B3/S23");
    assert_eq!(pj["rules"][0], "B3/S23");
    assert_eq!(pj["seen_by_rule"]["B3/S23"], 1);
    assert_eq!(pj["media"]["anim"]["path"], glider.media["anim"].path);
    assert!(
        !dir.join(format!("patterns/{}.json", ids[2])).exists(),
        "rejected"
    );
    assert!(
        !dir.join(format!("patterns/{}.json", ids[3])).exists(),
        "new"
    );

    // Nothing changed: nothing written, the manifest untouched.
    let before = std::fs::read(dir.join("manifest.json")).unwrap();
    let r2 = website::export(&lib, &dir, &opts()).unwrap();
    assert!(r2.written.is_empty(), "{:?}", r2.written);
    assert!(r2.deleted.is_empty());
    assert_eq!((r2.unchanged, r2.rendered), (3, 0));
    assert_eq!(std::fs::read(dir.join("manifest.json")).unwrap(), before);

    // The viewer page is written once, and exports leave it alone (it is not in the manifest).
    assert!(website::write_page(&dir).unwrap());
    assert!(!website::write_page(&dir).unwrap());
    assert!(website::PAGE.contains("manifest.json"));

    // One rating: exactly that item and the index.
    lib.update(
        &ids[1],
        &Update {
            rating: Some(5),
            ..Default::default()
        },
    )
    .unwrap();
    let r3 = website::export(&lib, &dir, &opts()).unwrap();
    assert_eq!(
        r3.written,
        vec![
            format!("patterns/{}.json", ids[1]),
            "manifest.json".to_string()
        ]
    );
    let m3 = Manifest::read(&dir).unwrap();
    assert_eq!(
        m3.items.iter().find(|i| i.id == ids[1]).unwrap().rating,
        Some(5)
    );

    // Rejecting an item deletes its JSON and media from the export.
    set_state(&lib, &ids[1], CurationState::Rejected);
    let r4 = website::export(&lib, &dir, &opts()).unwrap();
    assert_eq!(r4.written, vec!["manifest.json".to_string()]);
    assert_eq!(r4.deleted.len(), 4, "{:?}", r4.deleted);
    assert!(!dir.join(format!("patterns/{}.json", ids[1])).exists());
    assert!(!dir.join(format!("media/pattern/{}", ids[1])).exists());
    assert_eq!(Manifest::read(&dir).unwrap().items.len(), 2);
    assert!(dir.join("index.html").exists());
}

fn media_hash(b: &[u8]) -> String {
    jetcarp_core::media::content_hash(b)
}

#[test]
fn filters_and_frames() {
    let (lib, dir) = temp("filter");
    let ids = catalog(&lib);
    let mut o = opts();
    o.filter = ExportFilter {
        states: vec![
            CurationState::Promoted,
            CurationState::Reviewed,
            CurationState::New,
        ],
        classes: vec![PatternClass::Spaceship, PatternClass::StillLife],
        quickstart: vec![],
        ..Default::default()
    };
    o.frames = true;
    let r = website::export(&lib, &dir, &o).unwrap();
    // The glider (promoted ship) and the beehive (new still life); not the blinker (class),
    // not the block (rejected), no Quick Start entries.
    assert_eq!((r.patterns, r.quickstart), (2, 0));
    let m = Manifest::read(&dir).unwrap();
    let got: Vec<&str> = m.items.iter().map(|i| i.id.as_str()).collect();
    assert!(got.contains(&ids[0].as_str()) && got.contains(&ids[3].as_str()));
    let g = m.items.iter().find(|i| i.id == ids[0]).unwrap();
    let fr = &g.media["frames"];
    assert_eq!((fr.mime.as_str(), fr.frames), ("directory", 4));
    assert!(dir.join(&fr.path).join("0003.png").exists());
    // A minimum score drops the beehive (score below a ship's).
    o.filter.min_score = Some(50.0);
    let r = website::export(&lib, &dir, &o).unwrap();
    assert_eq!(r.patterns, 1);
    assert!(r.deleted.iter().any(|p| p.contains(&ids[3])));
}

/// Public ant and turmite runs are exported with their media: a highway's loop, a symmetric
/// rule's build-up; new and rejected ants are not; a second export writes nothing; rejecting an
/// ant deletes its files; an outcome filter keeps only highways.
#[test]
fn ants_are_exported_with_their_media() {
    use jetcarp_core::ants::job::{parse_rule, run_one};
    use jetcarp_core::ants::run::{Limits, Start};
    use jetcarp_core::library::AntUpdate;
    let (lib, site) = temp("ants");
    let lim = |steps: u64| Limits {
        steps,
        ..Limits::default()
    };
    let mut rows = vec![];
    for (rule, steps) in [("RL", 100_000u64), ("LLRR", 50_000), ("RLR", 20_000)] {
        rows.push(
            run_one(
                &parse_rule(rule).unwrap(),
                &jetcarp_core::library::tiling_list("square")
                    .unwrap()
                    .remove(0),
                &Start::Empty,
                &lim(steps),
            )
            .unwrap(),
        );
    }
    lib.ants_flush(0, &rows, None, None).unwrap();
    let state = |id: &str, s: &str| {
        lib.update_ant(
            id,
            &AntUpdate {
                state: Some(s.into()),
                ..Default::default()
            },
        )
        .unwrap();
    };
    state(&rows[0].id, "promoted");
    state(&rows[1].id, "reviewed");
    // rows[2] stays new.
    let o = ExportOptions {
        filter: ExportFilter {
            quickstart: vec![],
            ..Default::default()
        },
        ..opts()
    };
    let r = website::export(&lib, &site, &o).unwrap();
    assert_eq!(r.ants, 2, "{:?}", r.media_failed);
    let m = Manifest::read(&site).unwrap();
    let ants: Vec<_> = m.items.iter().filter(|i| i.kind == "ant").collect();
    assert_eq!(ants.len(), 2);
    let hw = ants.iter().find(|i| i.class == "highway").unwrap();
    assert_eq!(hw.media["anim"].mode, "alone", "a highway loops alone");
    assert!(site.join(&hw.media["anim"].path).exists());
    let sym = ants.iter().find(|i| i.class != "highway").unwrap();
    assert_eq!(sym.media["poster"].mode, "seed", "no loop: the build-up");
    let j: serde_json::Value =
        serde_json::from_slice(&std::fs::read(site.join(&hw.path)).unwrap()).unwrap();
    assert_eq!(j["kind"], "ant");
    assert_eq!(j["period"], 104);
    assert_eq!(m.counts["ants"], 2);

    let again = website::export(&lib, &site, &o).unwrap();
    assert!(again.written.is_empty(), "{:?}", again.written);

    state(&rows[1].id, "rejected");
    let r = website::export(&lib, &site, &o).unwrap();
    assert_eq!(r.ants, 1);
    assert!(r.deleted.iter().any(|d| d.contains("ants/")));
    assert!(!site.join(&sym.path).exists());

    state(&rows[1].id, "reviewed");
    let only = ExportOptions {
        filter: ExportFilter {
            quickstart: vec![],
            ant_outcomes: vec!["highway".into()],
            ..Default::default()
        },
        ..opts()
    };
    assert_eq!(website::export(&lib, &site, &only).unwrap().ants, 1);
}
