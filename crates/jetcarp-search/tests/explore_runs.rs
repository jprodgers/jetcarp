//! An explore search's runs across sealed Parquet segments and its working database
//! (`jetcarp_search::explore::runs`): sealing, recovery from a crash at every step of a seal,
//! queries over segments matching the same runs unsealed, rescoring into sidecars without
//! rewriting segments, a database from before segments sealed on open, the manifest's record of
//! the search imported from CART kept, and reviews kept in the library when the search's folder
//! is deleted.

use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::{Fate, Features};
use jetcarp_core::explore::space::Fam;
use jetcarp_core::topology::Nb;
use jetcarp_core::world::SeedSpec;
use jetcarp_search::explore::archive::{self, Eval};
use jetcarp_search::explore::atlas;
use jetcarp_search::explore::reviews;
use jetcarp_search::explore::runs::{Manifest, RunFilter, Runs, SealStop};
use jetcarp_search::explore::segments::{ALL, RunRec, Tuning};
use jetcarp_search::explore::store::{self, Delta, RunOut};
use jetcarp_store::Library;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("explore-runs-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const FAMS: [Fam; 4] = [Fam::LifeLike, Fam::Generations, Fam::Cyclic, Fam::Lenia];
const TILINGS: [&str; 3] = ["square", "hex", "trihex"];
const FATES: [Fate; 4] = [Fate::Died, Fate::Froze, Fate::Cycle, Fate::Running];

/// Run `id`, made up: families, tilings, fates and scores vary (with ties), every fifth run a
/// confirmation run of the one before. `true_score`: the score [`archive::score`] gives it, else
/// -1 (to be rescored).
fn run(id: i64, true_score: bool) -> RunOut {
    let h = (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let k = (h >> 40) as usize;
    let fam = FAMS[k % 4];
    let fate = FATES[(k / 4) % 4];
    let feat = Features {
        settle: (h % 60) as u32,
        period: (h % 5) as u32,
        density: ((h >> 8) % 100) as f32 / 100.0,
        density_late: ((h >> 16) % 100) as f32 / 100.0,
        activity_late: ((h >> 24) % 50) as f32 / 1000.0,
        activity_early: ((h >> 32) % 50) as f32 / 500.0,
        smooth: ((h >> 12) % 10) as f32 / 10.0,
        entropy: ((h >> 20) % 10) as f32 / 10.0,
        largest: ((h >> 28) % 10) as f32 / 10.0,
        movers: (h % 4) as u32,
        mover_speed: 0.5,
        ..Default::default()
    };
    let cand = Cand {
        tiling: TILINGS[(k / 16) % 3].into(),
        nb: if k % 2 == 0 { Nb::Touch } else { Nb::Edge },
        fam,
        rule: format!("B3/S2{}", k % 7),
        seed: SeedSpec::Random {
            density: 0.1 * (1 + k % 5) as f64,
        },
        cells: 400,
        gens: 64,
        steps: 1,
        rng: h,
        origin: "random".into(),
    };
    let score = if true_score {
        archive::score(fate, &feat, cand.gens, fam)
    } else {
        -1.0
    };
    let stage = if id % 5 == 0 { 2 } else { 1 };
    RunOut {
        id,
        stage,
        parent: (stage == 2).then_some(id - 1),
        seed_kind: ["random", "point", "blob"][k % 3].into(),
        ev: Eval {
            cand,
            fate,
            feat,
            score,
            bin: archive::behaviour_bin(fate, &feat, 64),
        },
    }
}

/// A search folder holding runs `1..=n` written `step` at a time, sealing every `seal` (0: never),
/// in segments of row groups of 32 runs and pages of 8 (so that a query meets many of them).
fn make(dir: &Path, n: i64, step: i64, seal: u64, true_score: bool) -> Runs {
    let mut r = Runs::open(&dir.join("search.sqlite")).unwrap();
    r.tuning = Tuning {
        group_rows: 32,
        page_rows: 8,
        ..Default::default()
    };
    let mut id = 1;
    while id <= n {
        let rows: Vec<RunOut> = (id..(id + step).min(n + 1))
            .map(|i| run(i, true_score))
            .collect();
        id += rows.len() as i64;
        r.write(&Delta {
            rows,
            ..Default::default()
        })
        .unwrap();
        if seal > 0 {
            r.seal_full(seal).unwrap();
        }
    }
    r
}

/// Every run, in id order, without its write time.
fn all(r: &Runs) -> Vec<RunRec> {
    let ids: Vec<i64> = (1..=r.max_id().unwrap()).collect();
    r.get(&ids)
        .unwrap()
        .into_iter()
        .map(|mut x| {
            x.ts = 0;
            x
        })
        .collect()
}

fn files(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for sub in ["", "runs"] {
        for e in std::fs::read_dir(dir.join(sub))
            .into_iter()
            .flatten()
            .flatten()
        {
            // The working database's own files change as SQLite pleases.
            if e.path().is_file() && !e.file_name().to_string_lossy().starts_with("search.sqlite") {
                let name = e.file_name().to_string_lossy().into_owned();
                out.push((
                    if sub.is_empty() {
                        name
                    } else {
                        format!("{sub}/{name}")
                    },
                    e.metadata().unwrap().len(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_search_seals_a_segment_per_seal_rows_and_none_on_stop() {
    let dir = scratch("seal");
    let r = make(&dir, 1000, 37, 100, true);
    // Ten segments of 100 runs; every segment's runs are gone from the working database.
    assert_eq!(r.manifest.segments.len(), 10);
    assert!(r.manifest.segments.iter().all(|s| s.rows == 100));
    assert_eq!(r.unsealed(), 0);
    let r = make(&scratch("seal-tail"), 1050, 37, 100, true);
    assert_eq!((r.manifest.segments.len(), r.unsealed()), (10, 50));
    for (k, s) in r.manifest.segments.iter().enumerate() {
        assert_eq!(s.file, format!("runs/part-{:06}.parquet", k + 1));
        assert_eq!(
            (s.first_id, s.last_id),
            (k as i64 * 100 + 1, k as i64 * 100 + 100)
        );
        let p = r.dir.join(&s.file);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), s.bytes);
        assert_eq!(
            jetcarp_search::explore::segments::file_sha256(&p).unwrap(),
            s.sha256
        );
    }
    // Opening again (as a stopped search continued) seals nothing more.
    let dir = r.dir.clone();
    drop(r);
    let r = Runs::open(&dir.join("search.sqlite")).unwrap();
    assert_eq!((r.manifest.segments.len(), r.unsealed()), (10, 50));
    assert_eq!(r.count(None).unwrap(), 1050);
    assert_eq!(r.max_id().unwrap(), 1050);
}

#[test]
fn a_crash_at_any_step_of_a_seal_is_recovered_on_open() {
    let base = scratch("crash");
    let reference = {
        let r = make(&base.join("ref"), 260, 260, 0, true);
        all(&r)
    };
    for stop in [
        SealStop::BeforeFooter,
        SealStop::BeforeSync,
        SealStop::BeforeVerify,
        SealStop::BeforeRename,
        SealStop::BeforeManifest,
        SealStop::BeforeDelete,
    ] {
        let dir = base.join(format!("{stop:?}"));
        let mut r = make(&dir, 260, 260, 0, true);
        r.seal_stopping(100, None).unwrap();
        let e = r.seal_stopping(100, Some(stop)).unwrap_err();
        assert!(e.contains("a test"), "{stop:?}: {e}");
        drop(r);
        // A manifest being written when the crash came.
        std::fs::write(dir.join("manifest.json.tmp"), "{ half").unwrap();
        let r = Runs::open(&dir.join("search.sqlite")).unwrap();
        let segs = r.manifest.segments.len();
        let expect = if stop == SealStop::BeforeDelete { 2 } else { 1 };
        assert_eq!(segs, expect, "{stop:?}");
        assert_eq!(r.unsealed(), 260 - 100 * segs as u64, "{stop:?}");
        assert_eq!(all(&r), reference, "{stop:?}: every run once");
        let left: Vec<String> = files(&dir).into_iter().map(|f| f.0).collect();
        assert!(
            left.iter().all(|f| !f.ends_with(".tmp")),
            "{stop:?}: {left:?}"
        );
        assert_eq!(
            left.iter().filter(|f| f.ends_with(".parquet")).count(),
            segs,
            "{stop:?}: {left:?}"
        );
        // Again: nothing changes.
        let (m, before) = (r.manifest.clone(), files(&dir));
        drop(r);
        let mut r = Runs::open(&dir.join("search.sqlite")).unwrap();
        assert_eq!((r.manifest.clone(), files(&dir)), (m, before), "{stop:?}");
        // And sealing goes on.
        r.seal_full(100).unwrap();
        r.seal_all().unwrap();
        assert_eq!(
            (r.manifest.segments.len(), r.unsealed()),
            (3, 0),
            "{stop:?}"
        );
        assert_eq!(all(&r), reference, "{stop:?}");
    }
}

#[test]
fn a_segment_file_the_manifest_lost_is_kept_when_its_runs_are_nowhere_else() {
    let dir = scratch("adopt");
    let r = make(&dir, 300, 300, 100, true);
    let reference = all(&r);
    drop(r);
    std::fs::remove_file(dir.join("manifest.json")).unwrap();
    let r = Runs::open(&dir.join("search.sqlite")).unwrap();
    assert_eq!(r.manifest.segments.len(), 3);
    assert_eq!(all(&r), reference);
}

#[test]
fn queries_over_segments_match_the_same_runs_unsealed() {
    let base = scratch("queries");
    let a = make(&base.join("unsealed"), 1300, 61, 0, true);
    let b = make(&base.join("sealed"), 1300, 61, 97, true);
    assert_eq!(a.manifest.segments.len(), 0);
    assert_eq!((b.manifest.segments.len(), b.unsealed()), (13, 39));
    assert_eq!(all(&a), all(&b));
    let r = Runs::open_read(&base.join("sealed").join("search.sqlite")).unwrap();
    assert_eq!(all(&r), all(&a), "read only");
    for stage in [None, Some(1), Some(2)] {
        assert_eq!(a.count(stage).unwrap(), b.count(stage).unwrap());
    }
    let within: Vec<i64> = (1..1300).filter(|i| i % 7 == 3).collect();
    let mut filters = Vec::new();
    for order in [None, Some("newest"), Some("id"), Some("score")] {
        let o = order.map(String::from);
        filters.push(RunFilter {
            order: o.clone(),
            ..Default::default()
        });
        for f in FAMS {
            filters.push(RunFilter {
                family: Some(f),
                order: o.clone(),
                ..Default::default()
            });
        }
        filters.push(RunFilter {
            tiling: Some("hex".into()),
            fate: Some("cycle".into()),
            order: o.clone(),
            ..Default::default()
        });
        filters.push(RunFilter {
            min_score: Some(1.0),
            stage: Some(1),
            order: o.clone(),
            ..Default::default()
        });
        filters.push(RunFilter {
            stage: Some(2),
            family: Some(Fam::Cyclic),
            order: o.clone(),
            ..Default::default()
        });
        filters.push(RunFilter {
            reviewed: Some("any".into()),
            within: Some(within.clone()),
            order: o.clone(),
            ..Default::default()
        });
        filters.push(RunFilter {
            min_score: Some(9.0),
            order: o.clone(),
            ..Default::default()
        });
    }
    filters.push(RunFilter {
        ids: Some(vec![1300, 5, 99_999, 1, 5, 640, 1251]),
        ..Default::default()
    });
    for f in &filters {
        for limit in [1, 7, 40, 5000] {
            assert_eq!(
                a.ids(f, limit).unwrap(),
                b.ids(f, limit).unwrap(),
                "{f:?} limit {limit}"
            );
        }
    }
    // By ids, in the order asked, every column.
    let ids = vec![1291, 3, 97, 98, 1300, 2_000, 500, 3];
    let strip = |v: Vec<RunRec>| -> Vec<RunRec> {
        v.into_iter()
            .map(|mut x| {
                x.ts = 0;
                x
            })
            .collect()
    };
    assert_eq!(strip(a.get(&ids).unwrap()), strip(b.get(&ids).unwrap()));
    assert_eq!(
        b.get(&ids)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![1291, 3, 97, 98, 1300, 500]
    );
    assert!(b.has(1300).unwrap() && b.has(1).unwrap() && !b.has(1301).unwrap());
    assert_eq!(a.fetch(&ids, ALL).unwrap().len(), 6);
    // The atlas's queries.
    let ta: Vec<i64> = atlas::taste_sample(&a, 24, 5)
        .unwrap()
        .iter()
        .map(|x| x.0.id)
        .collect();
    let tb: Vec<i64> = atlas::taste_sample(&b, 24, 5)
        .unwrap()
        .iter()
        .map(|x| x.0.id)
        .collect();
    assert!(!ta.is_empty());
    assert_eq!(ta, tb);
    let opts = atlas::AtlasOpts {
        min_score: 0.0,
        ..Default::default()
    };
    let sa: Vec<(i64, f64)> = atlas::select(&a, &opts)
        .unwrap()
        .iter()
        .map(|x| (x.0.id, x.1))
        .collect();
    let sb: Vec<(i64, f64)> = atlas::select(&b, &opts)
        .unwrap()
        .iter()
        .map(|x| (x.0.id, x.1))
        .collect();
    assert!(!sa.is_empty());
    assert_eq!(sa, sb);
}

#[test]
fn rescoring_writes_sidecars_and_never_rewrites_a_segment() {
    let base = scratch("rescore");
    let groups = jetcarp_search::explore::job::tiling_groups();
    let g = |k: &str| groups.get(k).copied().unwrap_or(0);
    // The same runs scored right from the start, and scored -1 then rescored.
    let good = make(&base.join("good"), 450, 50, 100, true);
    let mut r = make(&base.join("bad"), 450, 50, 100, false);
    assert_eq!((r.manifest.segments.len(), r.unsealed()), (4, 50));
    let before = files(&r.dir);
    let segs: Vec<_> = r.manifest.segments.clone();
    let out = r.rescore(&g, &|_| true).unwrap();
    assert_eq!((out.runs, out.changed), (450, 450));
    assert_eq!((out.sidecars, out.version), (4, 1));
    // The segments are as they were; the sidecars are new files beside them.
    assert_eq!(r.manifest.segments, segs);
    for (f, n) in &before {
        if f.ends_with(".parquet") {
            let now = std::fs::metadata(r.dir.join(f)).unwrap().len();
            assert_eq!(now, *n, "{f}");
        }
    }
    for s in &segs {
        assert_eq!(
            jetcarp_search::explore::segments::file_sha256(&r.dir.join(&s.file)).unwrap(),
            s.sha256
        );
    }
    let set = &r.manifest.scores[0];
    assert_eq!(set.version, 1);
    assert_eq!(set.files.len(), 4);
    assert!(set.files[0].file == "scores-1/part-000001.parquet");
    assert!(r.dir.join("scores-1/part-000004.parquet").is_file());
    // Readers see the new scores, as if they had been there all along.
    let ga = all(&good);
    assert_eq!(all(&r), ga);
    let dir = r.dir.clone();
    drop(r);
    let ro = Runs::open_read(&dir.join("search.sqlite")).unwrap();
    assert_eq!(all(&ro), ga);
    for f in [
        RunFilter::default(),
        RunFilter {
            min_score: Some(1.2),
            ..Default::default()
        },
    ] {
        assert_eq!(good.ids(&f, 30).unwrap(), ro.ids(&f, 30).unwrap());
    }
    // The archive is the one the right scores give.
    let elites = |r: &Runs| -> Vec<(i64, i64)> {
        r.db.prepare("SELECT id, grp FROM elites ORDER BY id")
            .unwrap()
            .query_map([], |x| Ok((x.get(0)?, x.get(1)?)))
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    };
    let mut gm = Runs::open(&base.join("good").join("search.sqlite")).unwrap();
    let again = gm.rescore(&g, &|_| true).unwrap();
    assert_eq!(
        (again.changed, again.sidecars),
        (0, 0),
        "nothing to rescore"
    );
    assert_eq!(elites(&ro), elites(&gm));
    drop(ro);
    // A second rescore with nothing to change writes nothing.
    let mut r = Runs::open(&dir.join("search.sqlite")).unwrap();
    let out = r.rescore(&g, &|_| true).unwrap();
    assert_eq!((out.changed, out.sidecars), (0, 0));
    assert_eq!(r.manifest.scores.len(), 1);
    assert_eq!(all(&r), ga);
}

#[test]
fn a_database_from_before_segments_is_sealed_on_open() {
    let dir = scratch("old-format");
    let lib = Library::open(dir.join("library.sqlite"));
    let folder = dir
        .join("searches")
        .join("5d0c6a41-0000-4000-8000-000000000001");
    let r = make(&folder, 2300, 2300, 0, true);
    let reference = all(&r);
    // As a search from before sealed segments left it: every run in `runs`, only ids in
    // `elites`, its reviews in `reviews`, format 1, no manifest.
    r.db.execute_batch(
        "INSERT INTO elites(id, grp) VALUES (2, 1), (7, 3);
         CREATE TABLE e2(id INTEGER PRIMARY KEY, grp INT NOT NULL);
         INSERT INTO e2 SELECT id, grp FROM elites; DROP TABLE elites;
         ALTER TABLE e2 RENAME TO elites;
         CREATE TABLE reviews(run_id INT, verdict TEXT, note TEXT, ts INT, rater TEXT,
           source TEXT);
         INSERT INTO reviews VALUES (7, 'keep', 'lovely', 1794168214, 'Emily', 'review');
         UPDATE meta SET v = 'jetcarp-explore 1' WHERE k = 'format';",
    )
    .unwrap();
    drop(r);
    std::fs::remove_file(folder.join("manifest.json")).unwrap();
    let mut r = Runs::open(&folder.join("search.sqlite")).unwrap();
    // Sealed, a million at a time: one segment here.
    assert_eq!((r.manifest.segments.len(), r.unsealed()), (1, 0));
    assert_eq!(all(&r), reference);
    assert_eq!(
        store::meta_get(&r.db, "format").unwrap().as_deref(),
        Some(store::FORMAT)
    );
    // The elites carry their runs now, so loading reads them alone.
    let saved = store::load(&mut r.db).unwrap();
    assert_eq!(saved.stats.elites, 2);
    assert_eq!(
        saved.archive.runs[&7].cand.rule, reference[6].rule,
        "elite 7's run"
    );
    // The reviews move to the library, once.
    assert_eq!(reviews::move_old(&lib, &r).unwrap(), 1);
    assert!(!store::has_table(&r.db, "reviews").unwrap());
    assert_eq!(reviews::move_old(&lib, &r).unwrap(), 0);
    let rv = lib.run_reviews(&r.uuid()).unwrap();
    assert_eq!(rv.len(), 1);
    assert_eq!(
        (rv[0].run, rv[0].verdict.as_str(), rv[0].rater.as_str()),
        (7, "keep", "Emily")
    );
}

#[test]
fn the_remeasure_record_of_the_search_imported_from_cart_is_read_and_kept() {
    let dir = scratch("remeasure-record");
    drop(make(&dir, 250, 50, 0, true));
    // As the import of CART's search left its manifest: what it checked and re-measured.
    let path = dir.join("manifest.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(m.get("remeasure").is_none(), "{m}");
    let rec = serde_json::json!({
        "mode": "auto", "engine_version": "0.4.0", "checked": "2026-11-11T12:00:00Z",
        "sample_per_tiling": 32, "tilings": 3, "sampled": 96, "sampled_differ": 4,
        "backend": "cpu",
        "remeasured": [
            {"tiling": "hex", "why": "3 of 32 sampled runs measured differently",
             "runs": 80, "changed": 75, "failed": 1},
            {"tiling": "square", "family": "cyclic", "why": "1 of 32, all cyclic",
             "runs": 20, "changed": 20, "failed": 0},
        ],
        "to_remeasure": 100, "runs": 100, "changed": 95,
        "check_seconds": 1.5, "remeasure_seconds": 2.25, "done": true,
    });
    m["remeasure"] = rec.clone();
    std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    // Read with the rest of the manifest, every field.
    let mut r = Runs::open(&dir.join("search.sqlite")).unwrap();
    let got = r.manifest.remeasure.clone().unwrap();
    assert_eq!(
        (
            got.remeasured[1].tiling.as_str(),
            got.remeasured[1].family.as_deref()
        ),
        ("square", Some("cyclic"))
    );
    assert_eq!(serde_json::to_value(&got).unwrap(), rec);
    // Kept as it is when the manifest is written again: a new spec, a seal.
    r.set_spec(&serde_json::json!({"seed": 9})).unwrap();
    assert_eq!(r.seal_all().unwrap().len(), 1);
    drop(r);
    let again = Manifest::read(&dir).unwrap().unwrap();
    assert_eq!((again.segments.len(), again.remeasure), (1, Some(got)));
    let text: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(text["remeasure"], rec);
}

#[test]
fn reviews_outlive_the_search_folder() {
    let dir = scratch("reviews");
    let lib = Library::open(dir.join("library.sqlite"));
    let folder = dir
        .join("searches")
        .join("1b9b0e2a-0000-4000-8000-000000000002");
    let r = make(&folder, 250, 50, 100, true);
    let uuid = r.uuid();
    reviews::add_review(&lib, &r, 42, "keep", "a busy one", "user", "review").unwrap();
    reviews::add_review(&lib, &r, 249, "drop", "", "Emily", "review").unwrap();
    assert!(reviews::add_review(&lib, &r, 251, "keep", "", "user", "review").is_err());
    // A reviewed filter reads them.
    let mut f = RunFilter {
        reviewed: Some("any".into()),
        order: Some("id".into()),
        ..Default::default()
    };
    reviews::resolve(&lib, &uuid, &mut f).unwrap();
    assert_eq!(r.ids(&f, 10).unwrap(), vec![42, 249]);
    drop(r);
    std::fs::remove_dir_all(&folder).unwrap();
    let lib = Library::open(dir.join("library.sqlite"));
    assert_eq!(
        reviews::review_counts(&lib, &uuid).unwrap(),
        vec![("drop".into(), 1), ("keep".into(), 1)]
    );
    let rv = lib.run_reviews(&uuid).unwrap();
    assert_eq!(
        rv.iter()
            .map(|x| (x.run, x.verdict.as_str(), x.note.as_str()))
            .collect::<Vec<_>>(),
        vec![(42, "keep", "a busy one"), (249, "drop", "")]
    );
    let all = lib.ratings(Some("search_run")).unwrap();
    assert_eq!(all[0].target_id, format!("{uuid}:42"));
    assert_eq!((all[0].rating, all[1].rating), (5.0, 1.0));
}
