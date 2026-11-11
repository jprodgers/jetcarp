//! Small libraries for the pack tests, and helpers.
#![allow(dead_code)]

use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{DiscoveredBy, PatternRecord, Provenance, record_from_object};
use jetcarp_core::*;
use jetcarp_pack::publish::{self, Options, Publisher, RaterBook, RaterEntry};
use jetcarp_pack::sign::Signer;
use jetcarp_store::Library;
use std::path::PathBuf;

pub const GLIDER: &str = ".o./..o/ooo";
pub const BLINKER: &str = "ooo";
pub const BLOCK: &str = "oo/oo";

/// A fresh folder for a test, under Cargo's scratch folder (never the user's).
pub fn temp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-pack-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn prov(date: &str) -> Provenance {
    Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: date.into(),
        ..Default::default()
    }
}

/// The one periodic object of `rows` on a wrapped square grid running `rule`.
pub fn one(rule: &str, rows: &str) -> PatternRecord {
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
        .map(|o| {
            record_from_object(
                &w,
                w.cas[0].id,
                o,
                prov("2026-11-01T10:00:00Z"),
                DiscoveredBy::User,
            )
            .unwrap()
        })
        .collect();
    assert_eq!(v.len(), 1, "{rows}");
    v.into_iter().next().unwrap()
}

/// An ant or turmite run on the square grid from the empty start, as an ants search stores it
/// (`RL` is Langton's ant, a highway from step 9,977, found by 20,000 steps).
pub fn ant(rule: &str, steps: u64) -> jetcarp_store::AntRow {
    let m = jetcarp_core::ants::search::parse_rule(rule).unwrap();
    let t = jetcarp_core::pattern::tiling_list("square").unwrap();
    let lim = jetcarp_core::ants::run::Limits {
        steps,
        max_period: 2000,
        symmetry: true,
    };
    jetcarp_search::ants::run_one(&m, &t[0], &jetcarp_core::ants::run::Start::Empty, &lim).unwrap()
}

/// The fixed publisher of the tests: a key from a seed, named.
pub fn publisher(seed: u8, name: &str) -> Publisher {
    Publisher {
        signer: Signer::from_seed(&[seed; 32], name),
        name: name.into(),
        orcid: String::new(),
        github: String::new(),
    }
}

/// A rater book with fixed ids.
pub fn book(names: &[&str]) -> RaterBook {
    RaterBook {
        raters: names
            .iter()
            .enumerate()
            .map(|(i, n)| RaterEntry {
                name: n.to_string(),
                id: format!("rater-{:016x}", 0x1000 + i),
                consent_date: "2026-11-01T00:00:00Z".into(),
                show_name: true,
                orcid: String::new(),
                notes: true,
                members: vec![],
            })
            .collect(),
    }
}

/// Publish options for a test store.
pub fn options(feed: &str, to: &std::path::Path) -> Options {
    Options {
        feed: feed.into(),
        to: to.to_path_buf(),
        title: format!("Test pack {feed}"),
        description: "A pack made by JETCARP's tests.".into(),
        version_label: "test".into(),
        created: Some("2026-11-11T12:00:00Z".into()),
        ..Default::default()
    }
}

/// A small library: a glider, a blinker and a block in Life, a sighting in a job, a rating
/// and a name.
pub fn small_library(dir: &std::path::Path) -> Library {
    let lib = Library::open(dir.join("lib"));
    let mut g = one("B3/S23", GLIDER);
    g.name = Some("glider".into());
    g.tags = vec!["classic".into()];
    let b = one("B3/S23", BLINKER);
    let k = one("B3/S23", BLOCK);
    let job = lib
        .create_job("hunt", &serde_json::json!({"label": "test hunt"}))
        .unwrap();
    lib.add_many(vec![g.clone(), b.clone()], Some(job)).unwrap();
    lib.add(k).unwrap();
    let rid = lib
        .rule_id("B3/S23", jetcarp_core::topology::Nb::Touch, 1)
        .unwrap();
    let ids: Vec<String> = lib.list().unwrap().into_iter().map(|r| r.id).collect();
    lib.add_sightings(
        job,
        &[jetcarp_store::Sighting {
            pattern_id: ids[0].clone(),
            rule_id: rid,
            count: 7,
            soups: 3,
        }],
    )
    .unwrap();
    lib.rate("pattern", &ids[1], "user", 4.0, "nice").unwrap();
    lib
}

pub fn run(lib: &Library, p: &Publisher, b: &mut RaterBook, o: &Options) -> publish::Published {
    publish::publish(lib, p, b, o, &|_| {}).unwrap()
}

/// Every file under a folder, relative path and bytes, sorted.
pub fn files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    std::fs::read(&p).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}
