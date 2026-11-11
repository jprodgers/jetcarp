//! Library schema 1: the file's marks, pins checked on opening, re-verification, provenance,
//! id aliases, and the importer's canonical ids.

use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{self, DiscoveredBy, PatternRecord, Provenance, record_from_object};
use jetcarp_core::*;
use jetcarp_search::jobs::reverify::{ReverifyProgress, ReverifySpec};
use jetcarp_search::jobs::{self, JobSpec, Runner};
use jetcarp_store::{AddOutcome, Library};
use rusqlite::{Connection, params};

const GLIDER: &str = ".o./..o/ooo";

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-library-v1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

fn prov() -> Provenance {
    Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: pattern::now_utc(),
        ..Default::default()
    }
}

/// The one periodic object of `rows` on a wrapped square grid running `rule`.
fn one(rule: &str, rows: &str) -> PatternRecord {
    let mut w = World::new(TilingSpec::rect("square", 48, 48, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let cells = place_rows(&g, rows, 20, 20);
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let v: Vec<PatternRecord> = r
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| record_from_object(&w, w.cas[0].id, o, prov(), DiscoveredBy::User).unwrap())
        .collect();
    assert_eq!(v.len(), 1, "{rows}");
    v.into_iter().next().unwrap()
}

/// The library file opened directly (no JETCARP checks, foreign keys off), to tamper with it.
fn raw(lib: &Library) -> Connection {
    let c = Connection::open(lib.path()).unwrap();
    c.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    c
}

/// Run a re-verification job to its end; its final progress.
fn reverify(lib: &Library, all: bool, drop: bool) -> ReverifyProgress {
    let js = JobSpec {
        strategy: "reverify".into(),
        spec: serde_json::to_value(ReverifySpec {
            all,
            drop,
            ..Default::default()
        })
        .unwrap(),
        ..Default::default()
    };
    let job = jobs::create(lib, &js).unwrap();
    let out = Runner::new(lib.clone())
        .run(job, &Default::default())
        .unwrap();
    assert_eq!(out.status, jetcarp_store::JobStatus::Done);
    serde_json::from_value(out.progress).unwrap()
}

#[test]
fn schema_1_is_marked_and_other_files_are_refused() {
    let lib = temp_lib("schema");
    assert!(lib.path().ends_with("library.sqlite"));
    lib.add(one("B3/S23", "oo/oo")).unwrap();
    let info = lib.info().unwrap();
    assert_eq!(info.schema_version, 1);
    let c = raw(&lib);
    let app: i64 = c
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app, jetcarp_store::library::schema::APPLICATION_ID);
    assert_eq!(lib.meta("schema").unwrap().as_deref(), Some("1"));
    let uuid = lib.meta("library_uuid").unwrap().unwrap();
    assert_eq!((uuid.len(), uuid.chars().nth(14)), (36, Some('4')));
    // The old stage 1 tables are gone.
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('settings', 'score_models')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    // A library from a newer JETCARP is refused, with what to do.
    c.execute_batch("PRAGMA user_version = 2").unwrap();
    drop(c);
    let e = Library::open(lib.path()).info().unwrap_err();
    assert!(
        e.contains("newer than this JETCARP") && e.contains("update"),
        "{e}"
    );
    // A catalog from before schema 1 (CART's, user version 6, no JETCARP mark) is refused.
    let old = lib.folder().join("catalog.sqlite");
    let c = Connection::open(&old).unwrap();
    c.execute_batch("CREATE TABLE patterns (id TEXT PRIMARY KEY); PRAGMA user_version = 6;")
        .unwrap();
    drop(c);
    let e = Library::open(&old).info().unwrap_err();
    assert!(e.contains("not a JETCARP library"), "{e}");
}

#[test]
fn a_record_whose_pin_differs_is_flagged_on_opening_and_reverify_clears_it() {
    let lib = temp_lib("flag");
    let g = one("B3/S23", GLIDER);
    let b = one("B3/S23", "oo/oo");
    lib.add(g.clone()).unwrap();
    lib.add(b.clone()).unwrap();
    let pin = Topology::geometry_pin(&TilingSpec::new("square"))
        .unwrap()
        .pin;
    assert_eq!(lib.get(&g.id).unwrap().geometry_pin, pin);
    assert!(!lib.get(&g.id).unwrap().needs_reverify);
    // A geometry change, simulated: the glider's stored pin is not the engine's.
    raw(&lib)
        .execute(
            "UPDATE patterns SET geometry_pin = ?2 WHERE id = ?1",
            params![g.id, "00000000000000000000000000000000"],
        )
        .unwrap();
    let again = Library::open(lib.path());
    let r = again.get(&g.id).unwrap();
    assert!(r.needs_reverify, "flagged on opening");
    assert!(!again.get(&b.id).unwrap().needs_reverify);
    assert_eq!(again.info().unwrap().needs_reverify, 1);
    // Checked again: passes, flag cleared, the engine's pin, a provenance row.
    let p = reverify(&again, false, false);
    assert_eq!((p.total, p.checked, p.passed, p.failed), (1, 1, 1, 0));
    let r = again.get(&g.id).unwrap();
    assert!(!r.needs_reverify);
    assert_eq!(r.geometry_pin, pin);
    let h = again.provenance("pattern", &g.id).unwrap();
    assert_eq!(
        h.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(),
        ["found", "reverified"]
    );
    assert_eq!(h[1].geometry_pin, pin);
    assert_eq!(
        h[0].machine,
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );
    // Opening again flags nothing.
    assert_eq!(Library::open(lib.path()).info().unwrap().needs_reverify, 0);
    // With `all`, every record is checked.
    let p = reverify(&again, true, false);
    assert_eq!((p.checked, p.passed), (2, 2));
}

#[test]
fn a_record_from_another_geometry_moves_to_its_new_id_with_an_alias() {
    let lib = temp_lib("rekey");
    let g = one("B3/S23", GLIDER);
    lib.add(g.clone()).unwrap();
    let job = lib
        .create_job("manual-test", &serde_json::json!({}))
        .unwrap();
    let rid = lib.rule_id("B3/S23", Nb::Touch, 1).unwrap();
    lib.add_sightings(
        job,
        &[jetcarp_store::Sighting {
            pattern_id: g.id.clone(),
            rule_id: rid,
            count: 3,
            soups: 2,
        }],
    )
    .unwrap();
    jetcarp_store::quickstart::promote(&lib, &g.id, Some("Glider"), None).unwrap();
    let copies = |lib: &Library, id: &str| -> u64 {
        lib.sightings(id).unwrap().iter().map(|s| s.count).sum()
    };
    let seen = copies(&lib, &g.id);
    assert_eq!(seen, 1 + 3);
    lib.rate("pattern", &g.id, "user", 4.0, "").unwrap();
    // The glider as an engine with another square grid would have stored it: another pin, so
    // another id (every table that names it renamed, as that library would hold it).
    let old_pin = "ffffffffffffffffffffffffffffffff";
    let old = pattern::content_id(&pattern::identity(&g).unwrap(), old_pin);
    let c = raw(&lib);
    for sql in [
        "UPDATE patterns SET id = ?2, geometry_pin = ?3 WHERE id = ?1",
        "UPDATE pattern_rules SET pattern_id = ?2 WHERE pattern_id = ?1",
        "UPDATE sightings SET pattern_id = ?2 WHERE pattern_id = ?1",
        "UPDATE quickstart SET id = ?2, source_id = ?2 WHERE id = ?1",
        "UPDATE ratings SET target_id = ?2 WHERE target_id = ?1",
        "UPDATE provenance SET target_id = ?2 WHERE target_id = ?1",
    ] {
        let n = if sql.contains("?3") {
            c.execute(sql, params![g.id, old, old_pin])
        } else {
            c.execute(sql, params![g.id, old])
        };
        n.unwrap();
    }
    drop(c);
    let lib = Library::open(lib.path());
    assert!(lib.get(&old).unwrap().needs_reverify);
    let p = reverify(&lib, false, false);
    assert_eq!((p.passed, p.rekeyed), (1, 1));
    assert_eq!(p.moved[0].old, old);
    assert_eq!(p.moved[0].new, g.id);
    // The record is at its new id; the old one is an alias.
    let r = lib.get(&old).unwrap();
    assert_eq!(r.id, g.id);
    assert_eq!(lib.get(&old[..8]).unwrap().id, g.id);
    assert!(!lib.contains(&old).unwrap());
    let a = lib.aliases("pattern", &g.id).unwrap();
    assert_eq!(
        (a[0].alias.as_str(), a[0].reason.as_str()),
        (old.as_str(), "geometry-change")
    );
    // Its sightings, Quick Start entry, rating and history moved with it.
    assert_eq!(copies(&lib, &g.id), seen);
    assert_eq!(lib.get_quickstart(&g.id).unwrap().title, "Glider");
    assert!(
        lib.ratings(Some("pattern"))
            .unwrap()
            .iter()
            .all(|x| x.target_id == g.id)
    );
    let h = lib.provenance("pattern", &g.id).unwrap();
    assert_eq!(h.first().unwrap().kind, "found");
    assert_eq!(h.last().unwrap().kind, "reverified");
    assert!(lib.provenance("pattern", &old).unwrap().is_empty());
}

#[test]
fn records_that_fail_stay_flagged_and_drop_deletes_them() {
    let lib = temp_lib("fail");
    let b = one("B3/S23", "oo/oo");
    lib.add(b.clone()).unwrap();
    // A geometry change under which the block no longer holds (simulated: another pin, and a
    // rule in which it dies).
    raw(&lib)
        .execute(
            "UPDATE patterns SET geometry_pin = 'x', rule = 'B2/S' WHERE id = ?1",
            [&b.id],
        )
        .unwrap();
    let lib = Library::open(lib.path());
    let p = reverify(&lib, false, false);
    assert_eq!((p.checked, p.passed, p.failed, p.removed), (1, 0, 1, 0));
    assert_eq!(p.failures[0].id, b.id);
    assert!(!p.failures[0].error.is_empty());
    // Kept, still flagged, and its history unchanged.
    let r = lib.get(&b.id).unwrap();
    assert!(r.needs_reverify);
    assert_eq!(lib.provenance("pattern", &b.id).unwrap().len(), 1);
    // Only `drop` deletes it.
    let p = reverify(&lib, false, true);
    assert_eq!((p.failed, p.removed), (1, 1));
    assert!(lib.get(&b.id).is_err());
    assert!(lib.provenance("pattern", &b.id).unwrap().is_empty());
}

// A record whose pin names a tiling this engine cannot build (a family removed or renamed,
// simulated) still lets the library open: the key gets no engine pin, and the record is flagged.
#[test]
fn a_record_on_a_tiling_the_engine_cannot_build_is_flagged() {
    let lib = temp_lib("nofamily");
    let b = one("B3/S23", "oo/oo");
    lib.add(b.clone()).unwrap();
    let gone = r#"{"family":"no-such-family","boundary":"dead"}"#;
    let c = raw(&lib);
    c.execute(
        "UPDATE patterns SET pin_key = ?2 WHERE id = ?1",
        params![b.id, gone],
    )
    .unwrap();
    // As an older engine would have cached it.
    c.execute(
        "INSERT INTO engine_pins (pin_key, pin, polygon_hash, neighbour_hash, engine_version,
            checked) VALUES (?1, ?2, '', '', 'an older engine', '')",
        params![gone, b.geometry_pin],
    )
    .unwrap();
    drop(c);
    let lib = Library::open(lib.path());
    assert!(lib.get(&b.id).unwrap().needs_reverify);
    let pin: String = raw(&lib)
        .query_row(
            "SELECT pin FROM engine_pins WHERE pin_key = ?1",
            [gone],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pin, jetcarp_store::library::schema::NO_PIN);
}

#[test]
fn aliases_resolve_ids_and_short_ids() {
    let lib = temp_lib("alias");
    let g = one("B3/S23", GLIDER);
    let b = one("B3/S23", "oo/oo");
    lib.add(g.clone()).unwrap();
    lib.add(b.clone()).unwrap();
    // A short id works when it is unique.
    assert_eq!(lib.get(&g.id[..8]).unwrap().id, g.id);
    // CART's 64-bit ids, kept as aliases (the CART importer fills these).
    lib.add_alias("25e004f4f640c35a", "pattern", &g.id, "cart-fnv")
        .unwrap();
    lib.add_alias("aaaa1111aaaa1111", "pattern", &g.id, "cart-fnv")
        .unwrap();
    lib.add_alias("aaaa2222aaaa2222", "pattern", &b.id, "canonicalised")
        .unwrap();
    assert_eq!(lib.get("25e004f4f640c35a").unwrap().id, g.id);
    assert_eq!(lib.get("25e004f4").unwrap().id, g.id);
    let e = lib.get("aaaa").unwrap_err();
    assert!(
        e.contains("matches") && e.contains(&g.id) && e.contains(&b.id),
        "{e}"
    );
    assert!(lib.add_alias("bbbb", "pattern", &g.id, "because").is_err());
    assert!(
        lib.add_alias("bbbb", "pattern", "no-such-id", "cart-fnv")
            .is_err()
    );
    // Removing the record removes its aliases.
    lib.remove(&g.id).unwrap();
    assert!(lib.get("25e004f4").is_err());
    assert!(lib.aliases("pattern", &g.id).unwrap().is_empty());
}

#[test]
fn provenance_row_0_is_written_once_and_never_rewritten() {
    let lib = temp_lib("prov");
    let g = one("B3/S23", GLIDER);
    lib.add(g.clone()).unwrap();
    let h = lib.provenance("pattern", &g.id).unwrap();
    assert_eq!(h.len(), 1);
    let h0 = &h[0];
    assert_eq!(
        (
            h0.seq,
            h0.kind.as_str(),
            h0.source.as_str(),
            h0.engine_version.as_str()
        ),
        (0, "found", "test", VERSION)
    );
    assert_eq!(h0.geometry_pin, g.geometry_pin);
    assert_eq!(h0.date, g.provenance.date);
    // Adding it again (another find) merges and leaves row 0 alone.
    let mut again = g.clone();
    again.provenance.source = "later".into();
    lib.add(again).unwrap();
    assert_eq!(lib.provenance("pattern", &g.id).unwrap(), h);
    // The file refuses to rewrite row 0.
    let e = raw(&lib)
        .execute(
            "UPDATE provenance SET finder = 'someone else' WHERE target_id = ?1 AND seq = 0",
            [&g.id],
        )
        .unwrap_err();
    assert!(e.to_string().contains("never rewritten"), "{e}");
    // A search's finds name the job: its kind and uuid.
    let spec = jetcarp_search::bounded::SearchSpec {
        rules: vec!["B3/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        size: SizeSpec::Rect { w: 32, h: 32 },
        boundary: Boundary::Wrap,
        soups: 3,
        gens: 400,
        seed: 3,
        ..Default::default()
    };
    let js = JobSpec {
        strategy: "search".into(),
        spec: serde_json::to_value(jetcarp_search::jobs::search::SearchJobSpec {
            search: spec,
            store: true,
        })
        .unwrap(),
        ..Default::default()
    };
    let job = jobs::create(&lib, &js).unwrap();
    Runner::new(lib.clone())
        .run(job, &Default::default())
        .unwrap();
    let uuid = raw(&lib)
        .query_row("SELECT uuid FROM jobs WHERE id = ?1", [job], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    let found: Vec<_> = lib
        .list()
        .unwrap()
        .into_iter()
        .filter(|r| r.id != g.id)
        .map(|r| lib.provenance("pattern", &r.id).unwrap().remove(0))
        .collect();
    assert!(!found.is_empty());
    for x in &found {
        assert_eq!(
            (x.kind.as_str(), x.search_kind.as_str(), x.job_uuid.as_str()),
            ("found", "search", uuid.as_str())
        );
    }
}

// The identity bug: one record written from each generation of its period imports as one
// pattern (it used to give one ship as many ids as it has phases).
#[test]
fn import_merges_a_record_stored_from_any_phase() {
    let lib = temp_lib("import");
    let g = one("B3/S23", GLIDER);
    let dir = lib.folder().join("records");
    std::fs::create_dir_all(&dir).unwrap();
    for k in 0..4u64 {
        let mut x = g.clone();
        x.phases = (0..4)
            .map(|t| {
                let v: Vec<_> = x
                    .phase(t + k)
                    .unwrap()
                    .into_iter()
                    .map(|mut c| {
                        c.i += 5;
                        c
                    })
                    .collect();
                pattern::encode_cells(&v)
            })
            .collect();
        x.id = format!("{k:016x}");
        x.canonical = "stored as found".into();
        std::fs::write(dir.join(format!("{k}.json")), x.to_json()).unwrap();
    }
    let done = lib.import(&dir).unwrap();
    assert_eq!(done.len(), 4);
    assert!(done.iter().all(|(id, _)| *id == g.id), "{done:?}");
    assert_eq!(done[0].1, AddOutcome::Added);
    assert_eq!(lib.list().unwrap().len(), 1);
    let r = lib.get(&g.id).unwrap();
    assert_eq!(
        (r.phases.clone(), r.canonical.clone()),
        (g.phases.clone(), g.canonical.clone())
    );
    let h = lib.provenance("pattern", &g.id).unwrap();
    assert_eq!(h[0].kind, "imported");
    assert!(h[0].source.starts_with("import "), "{}", h[0].source);
}

#[test]
fn ant_ids_are_content_ids_on_their_pin() {
    let lib = temp_lib("ants");
    let m = jetcarp_core::ants::search::parse_rule("RL").unwrap();
    let lim = jetcarp_core::ants::run::Limits {
        steps: 20_000,
        max_period: 2000,
        symmetry: true,
    };
    let sq = TilingSpec::new("square");
    let row = jetcarp_search::ants::run_one(&m, &sq, &jetcarp_core::ants::run::Start::Empty, &lim)
        .unwrap();
    let pin = Topology::geometry_pin(&sq).unwrap().pin;
    assert_eq!(row.geometry_pin, pin);
    let hex = row.id.strip_prefix("ant-").unwrap();
    assert!(pattern::range::is_content_id(hex), "{}", row.id);
    assert_eq!(
        hex,
        pattern::content_id(
            &jetcarp_store::library::ant_identity(&row.rule, &sq, &row.start),
            &pin
        )
    );
    lib.ants_flush(0, std::slice::from_ref(&row), None, None)
        .unwrap();
    let a = lib.ant(&row.id[..12]).unwrap();
    assert_eq!(
        (a.id.as_str(), a.geometry_pin.as_str()),
        (row.id.as_str(), pin.as_str())
    );
    assert_eq!(lib.provenance("ant", &a.id).unwrap()[0].kind, "found");
    // Checked again, the ant passes.
    let p = reverify(&lib, true, false);
    assert_eq!((p.checked, p.passed), (1, 1));
}
