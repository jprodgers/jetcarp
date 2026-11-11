//! A real explore search through a pack: run, sealed, published, imported into an empty
//! library, where its working database is rebuilt from its segments, so it is a stopped search
//! like the original (the same counts, archive, coverage, seen set and confirmed elites) that can
//! be resumed.

mod common;

use common::*;
use jetcarp_pack::import::{self, VerifyMode};
use jetcarp_search::explore::runs::Runs;
use jetcarp_search::jobs::explore::{ExploreProgress, db_path};
use jetcarp_search::jobs::{self, Budget, JobSpec, RunOptions, Runner};
use jetcarp_store::{JobStatus, Library};
use serde_json::json;
use std::collections::BTreeSet;

fn small_search(lib: &Library) -> i64 {
    let spec = json!({
        "tilings": ["square", "hex"],
        "families": ["life_like", "generations", "cyclic"],
        "screen_cells": 400, "screen_gens": 48,
        "confirm_cells": 900, "confirm_gens": 64, "confirm_seeds": 2,
        "batch": 48, "seed": 7, "seal_rows": 150, "max_rss_gb": 64.5,
    });
    jobs::create(
        lib,
        &JobSpec {
            strategy: "explore".into(),
            spec,
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap()
}

fn run_for(lib: &Library, job: i64, items: u64) {
    let o = RunOptions {
        threads: 2,
        budget: Budget {
            items: Some(items),
            ..Default::default()
        },
        ..Default::default()
    };
    Runner::new(lib.clone()).run(job, &o).unwrap();
}

/// What a working database holds: (elites, confirmed, coverage rows, family rows, seen count).
type State = (
    BTreeSet<(i64, String)>,
    BTreeSet<i64>,
    BTreeSet<(String, String, String, i64)>,
    BTreeSet<(String, i64)>,
    usize,
);

fn state(lib: &Library, job: i64) -> State {
    let r = Runs::open_read(&db_path(lib, &lib.job(job).unwrap()).unwrap()).unwrap();
    let c = &r.db;
    let q = |sql: &str| -> Vec<Vec<String>> {
        let mut st = c.prepare(sql).unwrap();
        let n = st.column_count();
        st.query_map([], |row| {
            Ok((0..n)
                .map(|i| {
                    let v: rusqlite::types::Value = row.get(i).unwrap();
                    match v {
                        rusqlite::types::Value::Integer(x) => x.to_string(),
                        rusqlite::types::Value::Text(t) => t,
                        rusqlite::types::Value::Real(f) => f.to_string(),
                        _ => String::new(),
                    }
                })
                .collect())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    };
    let elites = q("SELECT id, rule FROM elites")
        .into_iter()
        .map(|v| (v[0].parse().unwrap(), v[1].clone()))
        .collect();
    let confirmed = q("SELECT id FROM confirmed")
        .into_iter()
        .map(|v| v[0].parse().unwrap())
        .collect();
    let coverage = q("SELECT tiling, fam, seed_kind, n FROM coverage")
        .into_iter()
        .map(|v| {
            (
                v[0].clone(),
                v[1].clone(),
                v[2].clone(),
                v[3].parse().unwrap(),
            )
        })
        .collect();
    let families = q("SELECT fam, runs FROM families")
        .into_iter()
        .map(|v| (v[0].clone(), v[1].parse().unwrap()))
        .collect();
    let seen: usize = q("SELECT length(data) FROM seen")
        .iter()
        .map(|v| v[0].parse::<usize>().unwrap() / 8)
        .sum();
    (elites, confirmed, coverage, families, seen)
}

#[test]
fn an_imported_search_is_a_stopped_search_that_resumes() {
    let dir = temp("searches");
    let lib = Library::open(dir.join("lib"));
    let job = small_search(&lib);
    run_for(&lib, job, 400);
    // Seal what the working database still holds, as `jetcarp jobs seal` does.
    let path = db_path(&lib, &lib.job(job).unwrap()).unwrap();
    Runs::open(&path).unwrap().seal_all().unwrap();
    let before: ExploreProgress = serde_json::from_value(lib.job(job).unwrap().progress).unwrap();
    assert!(before.screened >= 400, "{}", before.screened);
    let p = publisher(41, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    let out = run(&lib, &p, &mut b, &options("search", &store));
    assert_eq!(out.searches.len(), 1);

    let lib2 = Library::open(dir.join("lib2"));
    let o = import::Options {
        path: store.clone(),
        verify: VerifyMode::None,
        ..Default::default()
    };
    let r = import::import(&lib2, &o, &|_| {}).unwrap();
    assert_eq!(r.rebuilt.len(), 1, "{r:?}");
    let uuid = &out.searches[0];
    let j2 = lib2
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.spec["uuid"] == json!(uuid))
        .unwrap();
    assert_eq!(
        (j2.kind.as_str(), j2.status),
        ("explore", JobStatus::Paused)
    );
    // Its settings came with it (the decimal written as a string, read back as a number).
    assert_eq!(j2.spec["screen_cells"], json!(400));
    assert_eq!(j2.spec["max_rss_gb"], json!(64.5));
    let after: ExploreProgress = serde_json::from_value(j2.progress.clone()).unwrap();
    assert_eq!(
        (
            after.screened,
            after.confirmed,
            after.elites,
            after.niches,
            after.seen
        ),
        (
            before.screened,
            before.confirmed,
            before.elites,
            before.niches,
            before.seen
        )
    );
    assert_eq!(after.confirmed_elites, before.confirmed_elites);
    // The same archive, confirmed elites, coverage, family totals and seen set.
    assert_eq!(state(&lib2, j2.id), state(&lib, job));
    // It resumes: more runs, with ids after the imported ones.
    let last = Runs::open_read(&db_path(&lib2, &j2).unwrap())
        .unwrap()
        .max_id()
        .unwrap();
    run_for(&lib2, j2.id, 60);
    let resumed: ExploreProgress =
        serde_json::from_value(lib2.job(j2.id).unwrap().progress).unwrap();
    assert!(
        resumed.screened >= before.screened + 60,
        "{}",
        resumed.screened
    );
    let r2 = Runs::open_read(&db_path(&lib2, &lib2.job(j2.id).unwrap()).unwrap()).unwrap();
    assert!(r2.max_id().unwrap() > last);
    // Importing again leaves the search alone.
    let again = import::import(&lib2, &o, &|_| {}).unwrap();
    assert!(again.rebuilt.is_empty());
}
