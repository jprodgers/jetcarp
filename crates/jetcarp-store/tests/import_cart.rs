//! The CART importer (`examples/import_cart`, a development tool for the one-time import of
//! CART's catalog) on a small catalog in CART's format made here: a glider stored in two
//! orientations under two CART ids, a named block that works in one of its two rules, a "still
//! life" that dies, sightings in two jobs, census rows and unstored objects. The import is run
//! twice, after another importer has written a search and a rating into the same library.

#[path = "../examples/import_cart/import.rs"]
mod import;

use flate2::Compression;
use flate2::write::DeflateEncoder;
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{
    self, DiscoveredBy, PatternRecord, Provenance, decode_cells, encode_cells, record_from_object,
};
use jetcarp_core::*;
use jetcarp_store::Library;
use jetcarp_store::library::{Dedupe, RunReview};
use rusqlite::{Connection, params};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-import-cart-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
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
    let prov = Provenance {
        source: "hunt".into(),
        jetcarp_version: "0.1.0".into(),
        date: "2026-11-06T15:00:00Z".into(),
        ..Default::default()
    };
    let v: Vec<PatternRecord> = r
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| {
            record_from_object(&w, w.cas[0].id, o, prov.clone(), DiscoveredBy::Search).unwrap()
        })
        .collect();
    assert_eq!(v.len(), 1, "{rows}");
    v.into_iter().next().unwrap()
}

/// The record turned a quarter turn: every cell (i, j) to (-j, i), the displacement with it.
fn turned(r: &PatternRecord) -> PatternRecord {
    let mut t = r.clone();
    t.phases = r
        .phases
        .iter()
        .map(|p| {
            let mut v = decode_cells(p).unwrap();
            for c in &mut v {
                (c.i, c.j) = (-c.j, c.i);
            }
            v.sort_unstable_by_key(|c| (c.j, c.i, c.k, c.s));
            encode_cells(&v)
        })
        .collect();
    t.displacement = r.displacement.map(|[a, b]| [-b, a]);
    t.displacement_xy = r.displacement_xy.map(|[a, b]| [-b, a]);
    t
}

const SCHEMA: &str = "
CREATE TABLE rules (id INTEGER PRIMARY KEY, notation TEXT NOT NULL, neighborhood TEXT NOT NULL,
    steps_per_gen INTEGER NOT NULL DEFAULT 1, family TEXT NOT NULL, states INTEGER NOT NULL,
    birth INTEGER, survive INTEGER, created TEXT NOT NULL);
CREATE TABLE patterns (id TEXT PRIMARY KEY, model TEXT NOT NULL, tiling TEXT NOT NULL,
    tiling_id TEXT NOT NULL, tiling_name TEXT NOT NULL, tile_types INTEGER NOT NULL,
    neighborhood TEXT NOT NULL, steps_per_gen INTEGER NOT NULL, class TEXT NOT NULL,
    period INTEGER NOT NULL, dx INTEGER, dy INTEGER, speed TEXT, cells INTEGER NOT NULL,
    min_cells INTEGER NOT NULL, max_cells INTEGER NOT NULL, canonical TEXT NOT NULL,
    rule TEXT NOT NULL, range_bits INTEGER, birth_min INTEGER, birth_max INTEGER,
    survive_min INTEGER, survive_max INTEGER, states_min INTEGER, states_max INTEGER,
    seen INTEGER NOT NULL DEFAULT 0, score REAL NOT NULL DEFAULT 0, rating INTEGER,
    state TEXT NOT NULL DEFAULT 'new', name TEXT, label TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]', notes TEXT NOT NULL DEFAULT '', discovered_by TEXT NOT NULL,
    created TEXT NOT NULL, updated TEXT NOT NULL, body TEXT NOT NULL, emits TEXT);
CREATE TABLE pattern_rules (pattern_id TEXT NOT NULL, rule_id INTEGER NOT NULL,
    verified TEXT NOT NULL, periods INTEGER NOT NULL, first_seen TEXT NOT NULL,
    PRIMARY KEY (pattern_id, rule_id)) WITHOUT ROWID;
CREATE TABLE jobs (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, spec TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'running', progress TEXT NOT NULL DEFAULT '{}',
    checkpoint BLOB, created TEXT NOT NULL, updated TEXT NOT NULL, finished TEXT);
CREATE TABLE sightings (pattern_id TEXT NOT NULL, rule_id INTEGER NOT NULL,
    job_id INTEGER NOT NULL, count INTEGER NOT NULL, soups INTEGER NOT NULL DEFAULT 0,
    first_seen TEXT NOT NULL, last_seen TEXT NOT NULL,
    PRIMARY KEY (pattern_id, rule_id, job_id)) WITHOUT ROWID;
CREATE TABLE unstored (job_id INTEGER NOT NULL, rule_id INTEGER NOT NULL,
    pattern_id TEXT NOT NULL, class TEXT NOT NULL, period INTEGER NOT NULL,
    cells INTEGER NOT NULL, count INTEGER NOT NULL, soups INTEGER NOT NULL,
    first_soup INTEGER NOT NULL, rng_state INTEGER NOT NULL, generation INTEGER NOT NULL,
    PRIMARY KEY (job_id, rule_id, pattern_id)) WITHOUT ROWID;
CREATE TABLE census_rows (job_id INTEGER NOT NULL, idx INTEGER NOT NULL, rule TEXT NOT NULL,
    summary TEXT NOT NULL, PRIMARY KEY (job_id, idx)) WITHOUT ROWID;
PRAGMA user_version = 6;
";

/// Who wrote a catalog: CART (`cart_version` 0.1.0) or a JETCARP build from before content ids
/// (`jetcarp_version` 0.4.0).
#[derive(Clone, Copy, PartialEq)]
enum Maker {
    Cart,
    Jetcarp,
}

/// A catalog row for a record, its body deflated as CART stored it: found in its run at `found`,
/// stored at `created`.
#[allow(clippy::too_many_arguments)]
fn put(
    c: &Connection,
    maker: Maker,
    id: &str,
    r: &PatternRecord,
    name: Option<&str>,
    notes: &str,
    found: &str,
    created: &str,
) {
    let (key, version) = match maker {
        Maker::Cart => ("cart_version", "0.1.0"),
        Maker::Jetcarp => ("jetcarp_version", "0.4.0"),
    };
    let mut verified = json!({"periods": 16, "generations": 16 * r.period, "field": "frame",
        "date": created});
    verified[key] = json!(version);
    let mut provenance = json!({"source": "hunt", "rng_state": 7, "soup": 3, "density": 0.5,
        "generation": 120, "date": found});
    provenance[key] = json!(version);
    let body = json!({
        "format": 2,
        "tiling": r.tiling,
        "displacement_xy": r.displacement_xy,
        "bbox": r.bbox,
        "phases": r.phases,
        "verified": verified,
        "provenance": provenance,
    });
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(body.to_string().as_bytes()).unwrap();
    let mut blob = b"Z1".to_vec();
    blob.extend(enc.finish().unwrap());
    c.execute(
        "INSERT INTO patterns (id, model, tiling, tiling_id, tiling_name, tile_types,
            neighborhood, steps_per_gen, class, period, dx, dy, speed, cells, min_cells,
            max_cells, canonical, rule, seen, score, name, label, notes, discovered_by, created,
            updated, body)
         VALUES (?1, 'lifelike', ?2, 'square', 'Square', 1, 'touch', 1, ?3, ?4, ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, 'B3/S23', 0, 10.0, ?12, ?13, ?14, 'search', ?15, ?15, ?16)",
        params![
            id,
            pattern::tiling_key(&r.tiling),
            pattern::PatternClass::key(r.class),
            r.period as i64,
            r.displacement.map(|d| d[0]),
            r.displacement.map(|d| d[1]),
            r.speed,
            r.cells as i64,
            r.min_cells as i64,
            r.max_cells as i64,
            r.canonical,
            name,
            format!("{} #{}", name.unwrap_or("x"), &id[..6]),
            notes,
            created,
            blob,
        ],
    )
    .unwrap();
}

/// The CART catalog: rules 1 B3/S23, 2 B36/S23, 3 B3/S0; jobs 0 (by hand), 1 and 2 (hunts), 3
/// (a census).
fn cart_catalog(path: &std::path::Path, maker: Maker) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(SCHEMA).unwrap();
    for (id, n) in [(1, "B3/S23"), (2, "B36/S23"), (3, "B3/S0")] {
        c.execute(
            "INSERT INTO rules VALUES (?1, ?2, 'touch', 1, 'lifelike', 2, 0, 0, '2026-11-06T14:00:00Z')",
            params![id, n],
        )
        .unwrap();
    }
    c.execute_batch(
        r#"INSERT INTO jobs VALUES (0, 'manual', '{}', 'running', '{}', NULL,
               '2026-11-06T14:00:00Z', '2026-11-06T14:00:00Z', NULL);
           INSERT INTO jobs VALUES (1, 'hunt',
               '{"label":"first","out":"/home/someone/cart/runs/first-run"}', 'done',
               '{"job":1,"new_patterns":2,"newest":[{"id":"aaaa000000000001","label":"c/4 spaceship 5c #aaaa00"},{"id":"dddd000000000004","label":"still life 1c #dddd00"}]}',
               x'00', '2026-11-06T15:00:00Z', '2026-11-06T17:30:00Z', '2026-11-06T17:30:00Z');
           INSERT INTO jobs VALUES (2, 'hunt', '{"label":"second"}', 'done', '{}', NULL,
               '2026-11-06T15:30:00Z', '2026-11-06T18:00:00Z', '2026-11-06T18:00:00Z');
           INSERT INTO jobs VALUES (3, 'census', '{"label":"census"}', 'done', '{}', NULL,
               '2026-11-06T19:00:00Z', '2026-11-06T19:05:00Z', '2026-11-06T19:05:00Z');
           INSERT INTO census_rows VALUES (3, 0, 'B3/S23', '{"majority":"active"}');
           INSERT INTO census_rows VALUES (3, 1, 'B2/S', '{"majority":"exploded"}');
           INSERT INTO unstored VALUES (1, 1, 'eeee000000000005', 'still_life', 1, 70, 1, 1, 4, 9, 300);
           INSERT INTO unstored VALUES (1, 1, 'ffff000000000006', 'oscillator', 2, 80, 2, 2, 5, 9, 310);"#,
    )
    .unwrap();
    let glider = one("B3/S23", ".o./..o/ooo");
    let block = one("B3/S23", "oo/oo");
    let mut dot = block.clone();
    dot.phases = vec!["0,0,0,1".into()];
    dot.canonical = "o".into();
    dot.cells = 1;
    dot.min_cells = 1;
    dot.max_cells = 1;
    dot.bbox = [0, 0, 0, 0];
    // Found first (15:50), though stored last.
    put(
        &c,
        maker,
        "aaaa000000000001",
        &glider,
        None,
        "",
        "2026-11-06T15:50:00Z",
        "2026-11-06T17:00:00Z",
    );
    // The same glider turned a quarter turn, found later but stored first, with a note.
    put(
        &c,
        maker,
        "bbbb000000000002",
        &turned(&glider),
        None,
        "turned copy",
        "2026-11-06T15:55:00Z",
        "2026-11-06T16:00:00Z",
    );
    put(
        &c,
        maker,
        "cccc000000000003",
        &block,
        Some("block"),
        "",
        "2026-11-06T16:29:00Z",
        "2026-11-06T16:30:00Z",
    );
    put(
        &c,
        maker,
        "dddd000000000004",
        &dot,
        None,
        "",
        "2026-11-06T16:44:00Z",
        "2026-11-06T16:45:00Z",
    );
    c.execute_batch(
        "UPDATE patterns SET seen = 10 WHERE id = 'aaaa000000000001';
         UPDATE patterns SET seen = 5 WHERE id = 'bbbb000000000002';
         UPDATE patterns SET seen = 7 WHERE id = 'cccc000000000003';
         UPDATE patterns SET seen = 3 WHERE id = 'dddd000000000004';
         INSERT INTO pattern_rules VALUES ('aaaa000000000001', 1, '{}', 16, '2026-11-06T17:01:00Z');
         INSERT INTO pattern_rules VALUES ('aaaa000000000001', 2, '{}', 16, '2026-11-06T17:02:00Z');
         INSERT INTO pattern_rules VALUES ('bbbb000000000002', 1, '{}', 16, '2026-11-06T16:01:00Z');
         INSERT INTO pattern_rules VALUES ('cccc000000000003', 1, '{}', 16, '2026-11-06T16:31:00Z');
         INSERT INTO pattern_rules VALUES ('cccc000000000003', 3, '{}', 16, '2026-11-06T16:32:00Z');
         INSERT INTO pattern_rules VALUES ('dddd000000000004', 1, '{}', 16, '2026-11-06T16:46:00Z');
         INSERT INTO sightings VALUES ('aaaa000000000001', 1, 1, 6, 6, '2026-11-06T17:01:00Z', '2026-11-06T17:20:00Z');
         INSERT INTO sightings VALUES ('aaaa000000000001', 1, 2, 4, 3, '2026-11-06T17:10:00Z', '2026-11-06T17:50:00Z');
         INSERT INTO sightings VALUES ('bbbb000000000002', 1, 2, 5, 5, '2026-11-06T16:01:00Z', '2026-11-06T16:40:00Z');
         INSERT INTO sightings VALUES ('cccc000000000003', 1, 1, 5, 5, '2026-11-06T16:31:00Z', '2026-11-06T16:50:00Z');
         INSERT INTO sightings VALUES ('cccc000000000003', 3, 2, 2, 2, '2026-11-06T16:32:00Z', '2026-11-06T16:33:00Z');
         INSERT INTO sightings VALUES ('dddd000000000004', 1, 1, 3, 3, '2026-11-06T16:46:00Z', '2026-11-06T16:47:00Z');",
    )
    .unwrap();
}

#[test]
fn a_cart_catalog_comes_in_once_verified_with_its_history() {
    let d = dir("small");
    let catalog = d.join("catalog.sqlite");
    cart_catalog(&catalog, Maker::Cart);
    let lib = Library::open(d.join("library"));
    // Another importer got there first: a search and a rating of one of its runs.
    let search = lib
        .create_job(
            "explore",
            &json!({"uuid": "c0ffee00-0000-4000-8000-000000000001"}),
        )
        .unwrap();
    lib.add_run_reviews(
        &[RunReview {
            search: "c0ffee00-0000-4000-8000-000000000001".into(),
            run: 3,
            verdict: "keep".into(),
            note: "a note".into(),
            rater: "user".into(),
            source: "import:explore.sqlite".into(),
            created: "2026-11-09T10:00:00Z".into(),
        }],
        Dedupe::None,
    )
    .unwrap();
    let o = import::Options {
        cart: catalog.clone(),
        library: d.join("library"),
        report: Some(d.join("report")),
        threads: 2,
    };
    let r = import::run(&o).unwrap();
    assert_eq!(r.records, 4);
    assert_eq!(
        (r.failed, r.partial, r.rules_dropped),
        (1, 1, 1),
        "{}",
        r.text
    );
    assert_eq!(
        (r.merged_groups, r.merged_ids, r.ids),
        (1, 2, 2),
        "{}",
        r.text
    );
    assert_eq!((r.added, r.present), (2, 0));
    assert_eq!((r.jobs, r.jobs_added, r.census_rows_written), (3, 3, 2));
    assert_eq!((r.unstored_objects, r.unstored_copies), (2, 3));
    assert!(r.add_errors.is_empty(), "{:?}", r.add_errors);

    let glider = one("B3/S23", ".o./..o/ooo");
    let mut g = glider.clone();
    g.canonicalize().unwrap();
    let recs = lib.list().unwrap();
    assert_eq!(recs.len(), 2);
    let gl = lib.get(&g.id).unwrap();
    // Seen counts add up; the earliest record's date and note come along; its rules are both.
    assert_eq!(gl.seen, 15);
    // Stored first at 16:00; found first at 15:50, which its own provenance and row 0 both say.
    assert_eq!(gl.created, "2026-11-06T16:00:00Z");
    assert_eq!(gl.provenance.date, "2026-11-06T15:50:00Z");
    assert_eq!(gl.notes, "turned copy");
    assert_eq!(gl.rules, vec!["B3/S23".to_string(), "B36/S23".to_string()]);
    assert_eq!(gl.provenance.jetcarp_version, "CART 0.1.0");
    // Sightings per (rule, job), summed, with CART's dates.
    let mut s = lib.sightings(&g.id).unwrap();
    s.sort_by_key(|x| x.job);
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].count, s[0].soups), (6, 6));
    assert_eq!((s[1].count, s[1].soups), (9, 8));
    assert_eq!(s[1].first_seen, "2026-11-06T16:01:00Z");
    assert_eq!(s[1].last_seen, "2026-11-06T17:50:00Z");
    // Provenance: found by Jimmie Rodgers on CART's date, then imported.
    let h = lib.provenance("pattern", &g.id).unwrap();
    assert_eq!(h.len(), 2, "{h:?}");
    assert_eq!(h[0].kind, "found");
    assert_eq!(h[0].finder, "Jimmie Rodgers");
    assert_eq!(h[0].orcid, "0009-0003-8775-6483");
    assert_eq!(h[0].date, "2026-11-06T15:50:00Z");
    assert_eq!(h[0].engine_version, "CART 0.1.0");
    assert_eq!(h[0].source, "hunt");
    assert_eq!(h[0].geometry_pin, gl.geometry_pin);
    assert_eq!(h[0].machine, "");
    let jobs = lib.jobs().unwrap();
    let second = jobs.iter().find(|j| j.label() == Some("second")).unwrap();
    // The job whose sighting came first (16:01, the turned copy's).
    assert_eq!(h[0].search_kind, "hunt");
    assert_eq!(h[0].job_uuid, second.uuid);
    assert_eq!(h[1].kind, "imported");
    assert_eq!(h[1].source, "CART catalog catalog.sqlite");
    assert_eq!(h[1].engine_version, VERSION);
    // The named block works in B3/S23 only: its sightings in B3/S0 are dropped, and so are
    // the copies there from its seen count.
    let b = lib.get("block").unwrap();
    assert_eq!(b.rules, vec!["B3/S23".to_string()]);
    assert_eq!(b.seen, 5);
    assert_eq!(lib.sightings(&b.id).unwrap().len(), 1);
    // CART ids are not kept.
    assert!(lib.get("aaaa000000000001").is_err());
    assert!(lib.aliases("pattern", "").unwrap().is_empty());
    // Jobs: finished, with CART's dates, paths cut to file names, unstored objects as counts,
    // the CART ids in the progress made new ids (the failed find left out).
    let first = jobs.iter().find(|j| j.label() == Some("first")).unwrap();
    assert_eq!(first.kind, "hunt");
    assert_eq!(first.created, "2026-11-06T15:00:00Z");
    assert_eq!(first.finished.as_deref(), Some("2026-11-06T17:30:00Z"));
    assert_eq!(first.spec["out"], "first-run");
    assert_eq!(first.progress["unstored"]["still life"], 1);
    assert_eq!(first.progress["unstored"]["oscillator"], 2);
    assert_eq!(first.progress["unstored_distinct"], 2);
    assert_eq!(first.progress["job"], first.id);
    let newest = first.progress["newest"].as_array().unwrap();
    assert_eq!(newest.len(), 1);
    assert_eq!(newest[0]["id"], g.id.as_str());
    assert!(newest[0]["label"].as_str().unwrap().ends_with(&g.id[..6]));
    assert!(!lib.job_checkpoint(first.id).unwrap().is_some());
    let census = jobs.iter().find(|j| j.kind == "census").unwrap();
    assert_eq!(lib.census_rows(census.id).unwrap().len(), 2);
    // The mapping and the failures.
    let map = std::fs::read_to_string(d.join("report/cart-to-new-ids.tsv")).unwrap();
    assert_eq!(map.lines().count(), 5);
    assert!(map.contains(&format!(
        "aaaa000000000001\t{}\tsquare\tspaceship\tmerged",
        g.id
    )));
    assert!(map.contains(&format!(
        "bbbb000000000002\t{}\tsquare\tspaceship\tmerged",
        g.id
    )));
    assert!(map.contains(&format!(
        "cccc000000000003\t{}\tsquare\tstill_life\timported",
        b.id
    )));
    assert!(map.contains("dddd000000000004\t-\tsquare\tstill_life\tfailed"));
    let fails = std::fs::read_to_string(d.join("report/cart-failures.tsv")).unwrap();
    assert_eq!(fails.lines().count(), 2);
    assert!(fails.contains("dddd000000000004\tsquare\tstill_life\t1\tB3/S23\tin B3/S23: "));

    // Again: nothing new.
    let before = lib.info().unwrap();
    let again = import::run(&o).unwrap();
    assert_eq!((again.added, again.present), (0, 2));
    assert_eq!(again.rules_added_to_present, 0);
    assert_eq!(
        (
            again.jobs_added,
            again.sightings_written,
            again.census_rows_written
        ),
        (0, 0, 0)
    );
    let after = lib.info().unwrap();
    assert_eq!(
        (
            after.patterns,
            after.pattern_rules,
            after.sightings,
            after.jobs,
            after.seen
        ),
        (
            before.patterns,
            before.pattern_rules,
            before.sightings,
            before.jobs,
            before.seen
        )
    );
    assert_eq!(lib.provenance("pattern", &g.id).unwrap(), h);
    assert_eq!(lib.get(&g.id).unwrap().seen, 15);
    // The other importer's search and rating are untouched.
    assert_eq!(lib.job(search).unwrap().kind, "explore");
    assert_eq!(
        lib.run_reviews("c0ffee00-0000-4000-8000-000000000001")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_jetcarp_catalog_keeps_the_engine_version_its_records_state() {
    let d = dir("jetcarp");
    let catalog = d.join("hunts.sqlite");
    cart_catalog(&catalog, Maker::Jetcarp);
    let o = import::Options {
        cart: catalog,
        library: d.join("library"),
        report: Some(d.join("report")),
        threads: 2,
    };
    let r = import::run(&o).unwrap();
    assert_eq!((r.records, r.failed, r.ids), (4, 1, 2), "{}", r.text);
    let lib = Library::open(d.join("library"));
    let b = lib.get("block").unwrap();
    assert_eq!(b.provenance.jetcarp_version, "0.4.0");
    let h = lib.provenance("pattern", &b.id).unwrap();
    assert_eq!(
        (h[0].kind.as_str(), h[0].engine_version.as_str()),
        ("found", "0.4.0")
    );
    assert_eq!(h[0].date, "2026-11-06T16:29:00Z");
    assert_eq!(h[1].source, "JETCARP catalog hunts.sqlite");
    let c = Connection::open(lib.path()).unwrap();
    let versions: Vec<String> = c
        .prepare("SELECT DISTINCT engine_version FROM jobs WHERE id > 0")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(versions, vec!["0.4.0".to_string()]);
}
