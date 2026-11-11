//! The hunt's storage policy (large one-off still lifes and oscillators kept as `unstored`
//! rows), compressed pattern bodies, and pruning a catalog to the policy.

use jetcarp_core::hunt::job::{self, HuntSpec, StorePolicy};
use jetcarp_core::jobs::hunt::HuntRun;
use jetcarp_core::jobs::{Budget, RunOptions, Runner};
use jetcarp_core::library::{Library, PatternRecord, Query};
use jetcarp_core::topology::TilingSpec;
use std::collections::BTreeSet;

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("store-policy-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

/// A crystal-growing rule (many large still lifes and oscillators) and Life.
fn spec(store: StorePolicy) -> HuntSpec {
    HuntSpec {
        rules: vec!["B478/S124567".into(), "B3/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: Some(240),
        store,
        ..Default::default()
    }
}

type Snap = (Vec<String>, Vec<(String, String, u64, u64)>, Vec<String>);

fn snapshot(lib: &Library, job: i64) -> Snap {
    let mut recs: Vec<PatternRecord> = lib.query(&Query::default()).unwrap();
    recs.sort_by(|a, b| a.id.cmp(&b.id));
    let rows = recs
        .iter()
        .map(|r| {
            format!(
                "{} {} {} p{} {}c seen {} soup {:?} rng {:?} gen {:?}",
                r.id,
                r.rule,
                r.class.name(),
                r.period,
                r.cells,
                r.seen,
                r.provenance.soup,
                r.provenance.rng_state,
                r.provenance.generation
            )
        })
        .collect();
    let mut unstored = Vec::new();
    for notation in ["B478/S124567", "B3/S23"] {
        let rid = lib.rule_id(notation, jetcarp_core::Nb::Touch, 1).unwrap();
        for u in lib.unstored(job, rid).unwrap() {
            unstored.push(format!(
                "{} {} {} p{} {}c x{} in {} first {} {} {}",
                notation,
                u.pattern_id,
                u.class.name(),
                u.period,
                u.cells,
                u.count,
                u.soups,
                u.first_soup,
                u.rng_state,
                u.generation
            ));
        }
    }
    unstored.sort();
    (rows, lib.job_sightings(job).unwrap(), unstored)
}

/// Run (or resume) a hunt here on the job runner: on `threads` threads (0: all), `batch` soups
/// per batch (0: adapt), stopping (paused) after `limit` soups per group in this session.
fn run_hunt(
    lib: &Library,
    jid: i64,
    threads: usize,
    batch: u64,
    limit: Option<u64>,
) -> job::HuntProgress {
    let o = RunOptions {
        threads,
        budget: Budget {
            items: limit,
            ..Default::default()
        },
        ..Default::default()
    };
    Runner::new(lib.clone())
        .run_with(jid, &mut HuntRun::with_batch(batch), &o)
        .unwrap()
        .progress_as()
}

fn run(lib: &Library, s: &HuntSpec, threads: usize, batch: u64) -> (i64, job::HuntProgress) {
    let jid = job::start(lib, s).unwrap();
    (jid, run_hunt(lib, jid, threads, batch, None))
}

#[test]
fn policy_keeps_large_one_offs_unstored_independent_of_threads_and_batches() {
    let s = spec(StorePolicy::default());
    let mut snaps = Vec::new();
    let mut totals = Vec::new();
    for (name, threads, batch) in [("p-t1", 1, 7), ("p-tall", 0, 0), ("p-t3", 3, 50)] {
        let lib = temp_lib(name);
        let (jid, p) = run(&lib, &s, threads, batch);
        assert_eq!(p.status, "done");
        snaps.push(snapshot(&lib, jid));
        totals.push(p.groups.clone());
    }
    assert_eq!(snaps[0], snaps[1]);
    assert_eq!(snaps[0], snaps[2]);
    assert_eq!(totals[0], totals[1]);
    assert_eq!(totals[0], totals[2]);
    let (recs, _, unstored) = &snaps[0];
    assert!(
        !unstored.is_empty(),
        "the crystal rule leaves large objects"
    );
    // Every stored still life or oscillator is small, or was seen at least twice.
    let lib = temp_lib("p-check");
    let (jid, p) = run(&lib, &s, 0, 0);
    for r in lib.query(&Query::default()).unwrap() {
        if matches!(r.class.name(), "still life" | "oscillator") {
            assert!(
                (r.cells <= 64 && r.period * r.cells as u64 <= 4000) || r.seen >= 2,
                "{} {}c p{} seen {}",
                r.id,
                r.cells,
                r.period,
                r.seen
            );
        }
    }
    // The unstored rows are what the progress reports.
    let counts = lib.unstored_counts(jid).unwrap();
    let rows: u64 = counts.values().map(|x| x.0).sum();
    let groups: u64 = p.groups.iter().map(|g| g.unstored_distinct).sum();
    assert_eq!(rows, groups);
    assert_eq!(rows as usize, unstored.len());
    // Storing everything gives exactly the stored patterns plus the unstored ones.
    let all = temp_lib("p-all");
    let (ja, pa) = run(&all, &spec(StorePolicy::all()), 0, 0);
    let ids_all: BTreeSet<String> = all
        .query(&Query::default())
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    let ids: BTreeSet<String> = recs.iter().map(|r| r[..16].to_string()).collect();
    let un: BTreeSet<String> = unstored
        .iter()
        .map(|u| u.split(' ').nth(1).unwrap().to_string())
        .collect();
    assert!(ids.is_disjoint(&un));
    assert_eq!(&ids | &un, ids_all);
    assert!(all.unstored_counts(ja).unwrap().is_empty());
    // Distinct counts do not depend on the policy.
    let d = |p: &job::HuntProgress| p.groups.iter().map(|g| g.distinct).sum::<u64>();
    assert_eq!(d(&p), d(&pa));
    // Pruning the full catalog to the policy removes exactly the unstored objects and keeps
    // their copies as unstored rows.
    let rep = all.prune_patterns(64, 4000, 2).unwrap();
    assert_eq!(rep.deleted as usize, un.len());
    let left: BTreeSet<String> = all
        .query(&Query::default())
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(left, ids);
    let moved: u64 = all.unstored_counts(ja).unwrap().values().map(|x| x.0).sum();
    assert_eq!(moved as usize, un.len());
    all.vacuum().unwrap();
}

#[test]
fn policy_hunt_resumed_equals_uninterrupted() {
    let s = spec(StorePolicy::default());
    let a = temp_lib("r-whole");
    let (ja, pa) = run(&a, &s, 0, 0);
    let b = temp_lib("r-halves");
    let jb = job::start(&b, &s).unwrap();
    let half = run_hunt(&b, jb, 0, 40, Some(120));
    assert_eq!(half.status, "paused");
    let b2 = Library::open(b.path());
    let pb = run_hunt(&b2, jb, 0, 0, None);
    assert_eq!(pb.status, "done");
    assert_eq!(pa.groups, pb.groups);
    assert_eq!(pa.newest, pb.newest);
    assert_eq!(snapshot(&a, ja), snapshot(&b2, jb));
}

#[test]
fn bodies_are_compressed_and_read_back() {
    let lib = temp_lib("bodies");
    let (_, p) = run(&lib, &spec(StorePolicy::default()), 0, 0);
    assert!(p.distinct > 0);
    let before: Vec<PatternRecord> = lib.query(&Query::default()).unwrap();
    // Written compressed; compacting finds nothing to do.
    let (n, _, _) = lib.compact_bodies().unwrap();
    assert_eq!(n, 0);
    let again: Vec<PatternRecord> = lib.query(&Query::default()).unwrap();
    assert_eq!(before, again);
    for r in before.iter().take(5) {
        assert_eq!(&lib.get(&r.id).unwrap(), r);
    }
}
