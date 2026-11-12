//! Publish a library, read the store back, import it into an empty library: the same records,
//! histories, sightings and ratings; byte-identical stores from the same library; no local
//! paths anywhere.

mod common;

use common::*;
use jetcarp_pack::import::{self, VerifyMode};
use jetcarp_pack::read::Source;
use jetcarp_store::Library;

#[test]
fn a_library_round_trips_through_a_pack() {
    let dir = temp("round-trip");
    let lib = small_library(&dir);
    // A block on edge neighbours: JETCARP spells its rule `B3/S23V`, which must come back so.
    let mut v = one("B3/S23V", BLOCK);
    v.provenance.date = "2026-11-02T10:00:00Z".into();
    lib.add(v).unwrap();
    assert!(lib.list().unwrap().iter().any(|r| r.rule == "B3/S23V"));
    let p = publisher(1, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    let out = run(&lib, &p, &mut b, &options("test", &store));
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    assert_eq!(out.seq, 1);
    assert_eq!(out.tables["patterns"].0, 4, "{:?}", out.tables);
    // The publisher was added to the rater book, with consent, on its first publish.
    assert_eq!(b.raters.len(), 1);

    // Read back: signed by its publisher, every table there.
    let src = Source::open(&store).unwrap();
    let h = src.default_manifest(None).unwrap();
    assert_eq!(h, out.manifest);
    let v = src.version(&h).unwrap();
    assert!(
        v.signed.by_publisher && v.signed.key_statement,
        "{:?}",
        v.signed
    );
    assert_eq!(v.signed.name, "Test Publisher");
    for t in [
        "tilings",
        "rules",
        "patterns",
        "verifications",
        "sightings",
        "provenance",
        "annotations",
        "ratings",
        "raters",
    ] {
        assert!(v.tables.contains_key(t), "{t}: {:?}", v.tables.keys());
    }

    // Import into an empty library.
    let lib2 = Library::open(dir.join("lib2"));
    let opts = import::Options {
        path: store.clone(),
        verify: VerifyMode::Full,
        ..Default::default()
    };
    let r = import::import(&lib2, &opts, &|_| {}).unwrap();
    assert_eq!(
        (r.patterns_added, r.patterns_merged, r.flagged),
        (4, 0, 0),
        "{r:?}"
    );
    assert_eq!(r.sample.checked, 4, "{:?}", r.sample);
    assert_eq!(r.sample.passed, 4, "{:?}", r.sample);
    assert_eq!(r.signed, "publisher");
    for a in lib.list().unwrap() {
        let c = lib2.get(&a.id).unwrap();
        assert_eq!(
            (
                &c.id,
                &c.rule,
                &c.phases,
                c.period,
                &c.canonical,
                &c.name,
                &c.tags,
                c.seen
            ),
            (
                &a.id,
                &a.rule,
                &a.phases,
                a.period,
                &a.canonical,
                &a.name,
                &a.tags,
                a.seen
            ),
            "{}",
            a.id
        );
        assert_eq!(c.rules, a.rules);
        assert_eq!(c.range, a.range);
        assert_eq!(c.verified, a.verified);
        // Provenance: the history as it was, then this import.
        let ha = lib.provenance("pattern", &a.id).unwrap();
        let hc = lib2.provenance("pattern", &a.id).unwrap();
        assert_eq!(hc.len(), ha.len() + 1, "{hc:?}");
        for (x, y) in ha.iter().zip(&hc) {
            assert_eq!(
                (
                    &x.kind,
                    &x.finder,
                    &x.job_uuid,
                    &x.search_kind,
                    &x.engine_version,
                    &x.date,
                    &x.source,
                    &x.machine
                ),
                (
                    &y.kind,
                    &y.finder,
                    &y.job_uuid,
                    &y.search_kind,
                    &y.engine_version,
                    &y.date,
                    &y.source,
                    &y.machine
                )
            );
        }
        let last = hc.last().unwrap();
        assert_eq!(
            (last.kind.as_str(), last.pack_hash.as_str()),
            ("imported", h.as_str())
        );
        // Sightings: per job uuid, the same.
        let sa = lib.pack_sightings(&a.id);
        let sc = lib2.pack_sightings(&a.id);
        assert_eq!(sa, sc, "{}", a.id);
    }
    let ra: Vec<_> = lib.ratings(None).unwrap();
    let rc: Vec<_> = lib2.ratings(None).unwrap();
    assert_eq!(ra.len(), rc.len());
    assert_eq!(rc[0].rater, "Test Publisher");
    assert_eq!((rc[0].rating, &rc[0].note), (ra[0].rating, &ra[0].note));

    // Importing again changes nothing.
    let r2 = import::import(&lib2, &opts, &|_| {}).unwrap();
    assert_eq!((r2.patterns_added, r2.patterns_merged), (0, 4));
    for a in lib.list().unwrap() {
        assert_eq!(lib2.get(&a.id).unwrap().seen, a.seen);
        assert_eq!(lib2.pack_sightings(&a.id), lib.pack_sightings(&a.id));
    }
    assert_eq!(lib2.ratings(None).unwrap().len(), ra.len());
    assert_eq!(lib2.pack_imports().unwrap().len(), 2);
}

trait Sightings {
    fn pack_sightings(&self, id: &str) -> Vec<(String, String, u64, u64, String, String)>;
}
impl Sightings for Library {
    fn pack_sightings(&self, id: &str) -> Vec<(String, String, u64, u64, String, String)> {
        self.sightings_by_uuid(id)
            .unwrap()
            .into_iter()
            .map(|s| {
                (
                    s.rule,
                    s.job_uuid,
                    s.count,
                    s.soups,
                    s.first_seen,
                    s.last_seen,
                )
            })
            .collect()
    }
}

/// The library keys a Quick Start entry by name; a pack only by its pack id (`qs-...`). Every
/// row about the entry names `quickstart:<pack id>`, and an import maps it back: to the entry
/// the library has, or to a new entry under the pack id.
#[test]
fn quick_start_entries_go_by_their_pack_ids() {
    let dir = temp("qs-ids");
    let lib = small_library(&dir);
    let mut scene = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
        "square",
        32,
        32,
        jetcarp_core::Boundary::Wrap,
    ));
    scene.name = "Glider".into();
    scene.cas.push(jetcarp_core::CaSpec::new("B3/S23"));
    lib.put_quickstart(&jetcarp_store::QuickStartRow::for_scene(
        "my-glider",
        "scene",
        scene,
    ))
    .unwrap();
    lib.rate("quickstart", "my-glider", "user", 5.0, "a good start")
        .unwrap();
    let p = publisher(4, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    run(&lib, &p, &mut b, &options("qs", &store));

    let src = Source::open(&store).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    let rows = |table: &str| -> Vec<serde_json::Value> {
        let mut out = vec![];
        src.table_rows(&v, table, |_, r| {
            out.extend(r);
            Ok(())
        })
        .unwrap();
        out
    };
    let qs = rows("quickstart");
    assert_eq!(qs.len(), 1);
    let id = qs[0]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("qs-"), "{id}");
    let tg = format!("quickstart:{id}");
    let prov: Vec<_> = rows("provenance")
        .into_iter()
        .filter(|r| r["target"].as_str().unwrap().starts_with("quickstart:"))
        .collect();
    assert!(!prov.is_empty());
    assert!(prov.iter().all(|r| r["target"] == tg.as_str()), "{prov:?}");
    let rated: Vec<_> = rows("ratings")
        .into_iter()
        .filter(|r| r["target"].as_str().unwrap().starts_with("quickstart:"))
        .collect();
    assert_eq!(rated.len(), 1);
    assert_eq!(rated[0]["target"], tg.as_str());

    // A fresh library keeps the entry under its pack id, with its history and rating.
    let lib2 = Library::open(dir.join("lib2"));
    let opts = import::Options {
        path: store.clone(),
        verify: VerifyMode::None,
        ..Default::default()
    };
    let r = import::import(&lib2, &opts, &|_| {}).unwrap();
    assert_eq!(r.quickstart, 1);
    lib2.get_quickstart(&id).unwrap();
    let h = lib2.provenance("quickstart", &id).unwrap();
    assert_eq!(h[0].kind, "found", "{h:?}");
    let rq = lib2.ratings(Some("quickstart")).unwrap();
    assert_eq!(rq.len(), 1);
    assert_eq!(rq[0].target_id, id);

    // The publisher's own library has the entry: no second copy, and its rating stays on it.
    let before = lib.quickstart(&[]).unwrap().len();
    let r = import::import(&lib, &opts, &|_| {}).unwrap();
    assert_eq!(r.quickstart, 0);
    assert_eq!(lib.quickstart(&[]).unwrap().len(), before);
    assert!(lib.get_quickstart(&id).is_err());
    assert!(
        lib.ratings(Some("quickstart"))
            .unwrap()
            .iter()
            .all(|x| x.target_id == "my-glider")
    );

    // A retraction by the library's id names the pack id.
    let mut o = options("qs", &store);
    o.retract = vec![("quickstart:my-glider".into(), "a test".into())];
    let out = run(&lib, &p, &mut b, &o);
    let v2 = src.version(&out.manifest).unwrap();
    assert_eq!(v2.manifest.retracts.len(), 1);
    assert_eq!(v2.manifest.retracts[0].target, tg);
}

/// Every time in a pack is an RFC 3339 UTC time with whole seconds: a rater book holding a bare
/// consent date publishes its midnight, and a version's time that is not one is refused.
#[test]
fn consent_dates_are_times() {
    let dir = temp("consent");
    let lib = small_library(&dir);
    let p = publisher(5, "Test Publisher");
    let mut b = book(&["Test Publisher"]);
    b.raters[0].consent_date = "2026-11-01".into();
    let store = dir.join("store");
    run(&lib, &p, &mut b, &options("consent", &store));
    let src = Source::open(&store).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    let mut dates = vec![];
    src.table_rows(&v, "raters", |_, rows| {
        dates.extend(rows.into_iter().map(|r| r["consent_date"].clone()));
        Ok(())
    })
    .unwrap();
    assert_eq!(dates, ["2026-11-01T00:00:00Z"]);
    let mut o = options("consent", &dir.join("bad"));
    o.created = Some("2026-11-11".into());
    let e = jetcarp_pack::publish::publish(&lib, &p, &mut b, &o, &|_| {}).unwrap_err();
    assert!(e.contains("RFC 3339"), "{e}");
}

/// A rater's rows are events: consent recorded again adds a row, and the latest stands. A rater
/// who stops being named is shown by their id from the next version on.
#[test]
fn a_raters_latest_consent_stands() {
    let dir = temp("raters");
    let lib = small_library(&dir);
    let id = lib.list().unwrap()[0].id.clone();
    lib.import_ratings(&[jetcarp_store::RatingRow {
        id: 0,
        target_kind: "pattern".into(),
        target_id: id,
        rater: "Test Rater B".into(),
        rating: 3.0,
        verdict: None,
        note: String::new(),
        source: "review".into(),
        created: "2026-11-11T12:00:00Z".into(),
    }])
    .unwrap();
    let p = publisher(6, "Test Publisher");
    let mut b = book(&["Test Publisher", "Test Rater B"]);
    let store = dir.join("store");
    run(&lib, &p, &mut b, &options("raters", &store));
    b.raters[1].show_name = false;
    b.raters[1].consent_date = "2026-11-05T00:00:00Z".into();
    let mut o = options("raters", &store);
    o.created = Some("2026-11-12T12:00:00Z".into());
    let out = run(&lib, &p, &mut b, &o);

    let src = Source::open(&store).unwrap();
    let v = src.version(&out.manifest).unwrap();
    let mut rows = vec![];
    src.table_rows(&v, "raters", |_, r| {
        rows.extend(r);
        Ok(())
    })
    .unwrap();
    let b_rows: Vec<_> = rows
        .iter()
        .filter(|r| r["rater"] == "rater-0000000000001001")
        .collect();
    assert_eq!(b_rows.len(), 2, "{rows:?}");

    let lib2 = Library::open(dir.join("lib2"));
    let opts = import::Options {
        path: store.clone(),
        verify: VerifyMode::None,
        ..Default::default()
    };
    import::import(&lib2, &opts, &|_| {}).unwrap();
    let raters: Vec<String> = lib2
        .ratings(None)
        .unwrap()
        .into_iter()
        .map(|r| r.rater)
        .collect();
    assert!(
        raters.contains(&"rater-0000000000001001".to_string()),
        "{raters:?}"
    );
    assert!(!raters.contains(&"Test Rater B".to_string()), "{raters:?}");
}

#[test]
fn the_same_library_gives_the_same_store() {
    let dir = temp("determinism");
    let lib = small_library(&dir);
    let p = publisher(2, "Test Publisher");
    let mut b = book(&["Test Publisher"]);
    let a = run(&lib, &p, &mut b, &options("det", &dir.join("a")));
    let c = run(&lib, &p, &mut b, &options("det", &dir.join("b")));
    assert_eq!(a.manifest, c.manifest);
    let fa = files(&dir.join("a"));
    let fb = files(&dir.join("b"));
    assert_eq!(fa.len(), fb.len());
    for (x, y) in fa.iter().zip(&fb) {
        assert_eq!(x.0, y.0);
        assert!(x.1 == y.1, "{} differs", x.0);
    }
    // Another created time: only the manifest (and what names it) changes.
    let mut o = options("det", &dir.join("c"));
    o.created = Some("2026-11-12T00:00:00Z".into());
    let d = run(&lib, &p, &mut b, &o);
    assert_ne!(d.manifest, a.manifest);
    let fc = files(&dir.join("c"));
    let parquet = |f: &[(String, Vec<u8>)]| -> Vec<(String, Vec<u8>)> {
        f.iter()
            .filter(|x| x.0.ends_with(".parquet"))
            .cloned()
            .collect()
    };
    assert_eq!(parquet(&fa), parquet(&fc));
}

#[test]
fn no_local_paths_reach_a_store() {
    let dir = temp("paths");
    let lib = small_library(&dir);
    // A record whose source and notes name local files.
    let mut s = one("B3/S23", "oo/o.o/.o");
    s.provenance.source = format!("{}/scenes/boat.json", dir.display());
    s.notes = "from /home/someone/work/boat.json".into();
    lib.add(s).unwrap();
    let p = publisher(3, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    run(&lib, &p, &mut b, &options("paths", &store));
    assert_no_paths(&store, &dir.display().to_string());
    // An export of it too (its README, citation, views and every file).
    let src = Source::open(&store).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    let ex = dir.join("export");
    jetcarp_pack::export::export(&src, &v, &ex).unwrap();
    let root = dir.display().to_string();
    for (name, bytes) in files(&ex) {
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(&root), "export {name} holds {root}");
        assert!(
            jetcarp_pack::paths::find_absolute(&bytes).is_none(),
            "export {name} holds a local path"
        );
    }
    // The export reads and checks as a store does.
    let e = Source::open(&ex).unwrap();
    let ev = e.version(&e.default_manifest(None).unwrap()).unwrap();
    assert!(ev.signed.by_publisher, "{:?}", ev.signed);
    assert_eq!(ev.hash, v.hash);
}

/// No file of a store, and no cell of any table in it, holds a local path.
pub fn assert_no_paths(store: &std::path::Path, root: &str) {
    for (name, bytes) in files(store) {
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(root), "{name} holds {root}");
        if let Some(x) = jetcarp_pack::paths::find_absolute(&bytes) {
            panic!("{name} holds a local path: {x}");
        }
    }
    // Table cells are compressed in their files: read them.
    let src = Source::open(store).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    let mut cells = 0;
    for t in v.tables.keys() {
        src.table_rows(&v, t, |_, rows| {
            for r in rows {
                let text = r.to_string();
                cells += 1;
                assert!(!text.contains(root), "{t} row holds {root}: {text}");
                if let Some(x) = jetcarp_pack::paths::find_absolute(text.as_bytes()) {
                    panic!("{t} row holds a local path: {x}: {text}");
                }
            }
            Ok(())
        })
        .unwrap();
    }
    assert!(cells > 0);
}

// An export hard-links the store's files where it can, and copies them when linking fails
// (another file system, EXDEV; Windows without links): forced here.
#[test]
fn an_export_copies_when_it_cannot_link() {
    let dir = temp("export-copy");
    let lib = small_library(&dir);
    let p = publisher(4, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    run(&lib, &p, &mut b, &options("copy", &store));
    let src = Source::open(&store).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    fn no_link(_: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::from_raw_os_error(18))
    }
    let copy = dir.join("copied");
    let e = jetcarp_pack::export::export_with(&src, &v, &copy, no_link).unwrap();
    assert_eq!(e.linked, 0);
    assert!(e.copied > 0);
    let linked = dir.join("linked");
    let l = jetcarp_pack::export::export(&src, &v, &linked).unwrap();
    assert_eq!(l.linked + l.copied, e.copied);
    // The same files either way, and both read as signed by their publisher.
    assert_eq!(files(&copy), files(&linked));
    for d in [&copy, &linked] {
        let s = Source::open(d).unwrap();
        assert!(s.version(&v.hash).unwrap().signed.by_publisher);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (_, path) = &v.tables["patterns"].parts[0];
        let rel = jetcarp_pack::read::export_path(
            "patterns",
            &v.tables["patterns"].parts[0].0.partition,
            1,
        );
        let ino = |p: &std::path::Path| std::fs::metadata(p).unwrap().ino();
        assert_ne!(ino(&copy.join(&rel)), ino(path), "a copy, not a link");
        if l.linked > 0 {
            assert_eq!(ino(&linked.join(&rel)), ino(path), "a link");
        }
    }
}

/// Atlas entries (an explore search's runs, published into the Quick Start table) go into a pack
/// like any other entry, under their pack ids, and a selection of a few makes a pack of them alone.
/// Imported, they are a pack's entries with their scenes, titles, descriptions and categories
/// (their previews and mature states stay behind), shown in the menu where they were; the
/// publisher's own library maps them back to its atlas entries.
#[test]
fn atlas_entries_travel_in_a_pack() {
    use jetcarp_core::{Boundary, CaSpec, Scene, TilingSpec};
    use jetcarp_store::quickstart::{self as qs, menu};
    use jetcarp_store::{QsSource, QuickStartRow};
    let dir = temp("qs-atlas");
    let lib = small_library(&dir);
    let job = lib.create_job("explore", &serde_json::json!({})).unwrap();
    let uuid = lib.job(job).unwrap().uuid;
    // Rows as the atlas makes them: the run on its large grid, a preview and a mature state.
    let row = |run: i64, rule: &str, cat: &str| -> QuickStartRow {
        let grid = |w, h| TilingSpec::rect("square", w, h, Boundary::Wrap);
        let mut scene = Scene::new(grid(240, 240));
        scene.name = format!("{rule} — square");
        let mut ca = CaSpec::new(rule);
        ca.palette = Some("Ember".into());
        scene.cas.push(ca);
        let (id, handle) = jetcarp_search::explore::atlas::row_ids(&uuid, run);
        let mut q = QuickStartRow::for_scene(&id, "search_run", scene.clone());
        q.handle = handle;
        q.source = QsSource::Atlas;
        q.source_ref = uuid.clone();
        q.target_id = Some(format!("{uuid}:{run}"));
        q.title = format!("Atlas run {run}");
        q.description = format!("{rule} on square\nSeed: random, density 0.3\nScore 2.00");
        q.category = cat.into();
        q.behaviour = cat.into();
        let mut preview = scene;
        preview.tiling = grid(32, 24);
        q.preview = Some(preview);
        q.mature_gen = 100;
        q.mature = Some(qs::deflate_state(&vec![1u8; 32 * 24]));
        q.score = Some(2.0);
        q
    };
    let rows = vec![
        row(7, "B36/S23", "Gliders & ships"),
        row(8, "B2/S/C3", "Turbulence"),
        row(9, "B3/S12345", "Labyrinths"),
    ];
    lib.publish_atlas(job, &uuid, &rows).unwrap();
    // A pack of two of them.
    let p = publisher(6, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    let mut o = options("atlas", &store);
    o.selection.ids = Some(rows[..2].iter().map(|q| q.id.clone()).collect());
    o.selection.searches = Some(vec![]);
    let out = run(&lib, &p, &mut b, &o);
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    let src = Source::open(&store).unwrap();
    let v = src.version(&out.manifest).unwrap();
    let table = |name: &str| -> Vec<serde_json::Value> {
        let mut out = vec![];
        src.table_rows(&v, name, |_, r| {
            out.extend(r);
            Ok(())
        })
        .unwrap();
        out
    };
    let packed = table("quickstart");
    assert_eq!(packed.len(), 2);
    assert!(table("patterns").is_empty());
    for q in &packed {
        assert!(q["id"].as_str().unwrap().starts_with("qs-"), "{q}");
        assert!(
            q["source"].is_null(),
            "a search run is no pattern or ant: {q}"
        );
        let orig = rows.iter().find(|r| r.title == q["title"]).unwrap();
        assert_eq!(q["behaviour"], orig.category.as_str());
        assert_eq!(q["description"], orig.description.as_str());
    }
    let prov: Vec<_> = table("provenance")
        .into_iter()
        .filter(|r| r["target"].as_str().unwrap().starts_with("quickstart:"))
        .collect();
    assert_eq!(prov.len(), 2);
    assert!(
        prov.iter()
            .all(|r| r["search"] == uuid.as_str() && r["kind"] == "found")
    );

    // Into a fresh library: a pack's entries, by their pack ids, shown in their categories.
    let lib2 = Library::open(dir.join("lib2"));
    let opts = import::Options {
        path: store.clone(),
        verify: VerifyMode::None,
        ..Default::default()
    };
    let r = import::import(&lib2, &opts, &|_| {}).unwrap();
    assert_eq!(r.quickstart, 2);
    let got = lib2.quickstart(&[QsSource::Pack]).unwrap();
    assert_eq!(got.len(), 2);
    for g in &got {
        let orig = rows.iter().find(|r| r.title == g.title).unwrap();
        assert_eq!(
            serde_json::to_value(&g.scene).unwrap(),
            serde_json::to_value(&orig.scene).unwrap()
        );
        assert_eq!(
            (g.handle.as_str(), g.category.as_str()),
            (g.id.as_str(), orig.category.as_str())
        );
        assert_eq!(g.source_ref, out.manifest);
        assert!(g.preview.is_none() && g.mature.is_none() && !g.hidden);
        let h = lib2.provenance("quickstart", &g.id).unwrap();
        assert_eq!(
            (h[0].kind.as_str(), h[0].job_uuid.as_str()),
            ("found", uuid.as_str())
        );
    }
    let items = menu::load(&lib2);
    for g in &got {
        let it = items.iter().find(|i| i.handle == g.handle).unwrap();
        assert_eq!(
            (it.source, it.behaviour.as_str()),
            (QsSource::Pack, g.category.as_str())
        );
    }
    // A card of an imported entry runs its scene made smaller, from the start.
    let card = qs::card(&lib2, &got[0].handle).unwrap();
    assert!(!card.own_preview && card.mature.is_none());
    // The publisher's library has them already, as its atlas entries.
    let r = import::import(&lib, &opts, &|_| {}).unwrap();
    assert_eq!(r.quickstart, 0);
    assert!(lib.quickstart(&[QsSource::Pack]).unwrap().is_empty());
    assert_eq!(lib.quickstart(&[QsSource::Atlas]).unwrap().len(), 3);
}

/// Random states beyond 2^53 travel exactly: a scene's `rng_state` and a find's soup state are
/// decimal strings in the pack (RFC 8785 refuses such an integer as a number, and a double would
/// round it), and come back as the same `u64`, so a scene starts from the same soup and gives
/// the same world.
#[test]
fn large_random_states_travel_exactly() {
    let dir = temp("big-rng");
    let lib = small_library(&dir);
    let states = [
        17705299566207515059u64,
        u64::MAX,
        (1 << 53) + 1,
        (1 << 53) - 1,
        3,
    ];
    for (k, s) in states.iter().enumerate() {
        let mut scene = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
            "square",
            24,
            24,
            jetcarp_core::Boundary::Wrap,
        ));
        scene.rng_state = *s;
        scene.cas.push(jetcarp_core::CaSpec::new("B3/S23"));
        lib.put_quickstart(&jetcarp_store::QuickStartRow::for_scene(
            &format!("soup-{k}"),
            "scene",
            scene,
        ))
        .unwrap();
    }
    // A find whose soup's state is beyond 2^53.
    let mut p = one("B3/S23", "oo/o.o/.o");
    p.provenance.rng_state = Some(17705299566207515059);
    p.provenance.soup = Some(12);
    lib.add(p.clone()).unwrap();
    let pid = p.id.clone();

    let store = dir.join("store");
    let out = run(
        &lib,
        &publisher(7, "Test Publisher"),
        &mut book(&[]),
        &options("big", &store),
    );
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    let src = Source::open(&store).unwrap();
    let v = src.version(&out.manifest).unwrap();
    let mut qs = vec![];
    let mut prov = vec![];
    src.table_rows(&v, "quickstart", |_, r| {
        qs.extend(r);
        Ok(())
    })
    .unwrap();
    src.table_rows(&v, "provenance", |_, r| {
        prov.extend(r);
        Ok(())
    })
    .unwrap();
    for s in states {
        let want = if s > (1 << 53) - 1 {
            format!("\"rng_state\":\"{s}\"")
        } else {
            format!("\"rng_state\":{s}")
        };
        assert!(
            qs.iter()
                .any(|q| q["scene"].as_str().unwrap().contains(&want)),
            "no scene cell has {want}"
        );
    }
    let run_cell = prov
        .iter()
        .find(|r| r["target"] == format!("pattern:{pid}").as_str() && !r["run"].is_null())
        .unwrap()["run"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        run_cell.contains("\"rng_state\":\"17705299566207515059\""),
        "{run_cell}"
    );

    // Into a fresh library: the same states, and the same world after 40 generations.
    let lib2 = Library::open(dir.join("lib2"));
    let opts = import::Options {
        path: store.clone(),
        verify: VerifyMode::None,
        ..Default::default()
    };
    import::import(&lib2, &opts, &|_| {}).unwrap();
    let got = lib2.quickstart(&[jetcarp_store::QsSource::Pack]).unwrap();
    assert_eq!(got.len(), states.len());
    let world = |sc: &jetcarp_core::Scene| {
        let mut w = jetcarp_core::World::from_scene(sc).unwrap();
        for _ in 0..40 {
            w.step();
        }
        w.hash_hex()
    };
    for k in 0..states.len() {
        let orig = lib.get_quickstart(&format!("soup-{k}")).unwrap();
        let back = got
            .iter()
            .find(|g| g.scene.rng_state == orig.scene.rng_state)
            .unwrap_or_else(|| panic!("no entry has rng_state {}", orig.scene.rng_state));
        assert_eq!(world(&back.scene), world(&orig.scene));
    }
    let back = lib2.get(&pid).unwrap();
    assert_eq!(back.provenance.rng_state, Some(17705299566207515059));
    assert_eq!(back.provenance.soup, Some(12));
}
