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
    let p = publisher(1, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    let out = run(&lib, &p, &mut b, &options("test", &store));
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    assert_eq!(out.seq, 1);
    assert_eq!(out.tables["patterns"].0, 3, "{:?}", out.tables);
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
        (3, 0, 0),
        "{r:?}"
    );
    assert_eq!(r.sample.checked, 3, "{:?}", r.sample);
    assert_eq!(r.sample.passed, 3, "{:?}", r.sample);
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
    assert_eq!((r2.patterns_added, r2.patterns_merged), (0, 3));
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
