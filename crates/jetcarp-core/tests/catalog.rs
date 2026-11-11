//! Phase 4: the SQLite catalog, rule-free pattern identity, and rule ranges.

use jetcarp_core::jobs::search::{SearchJobSpec, SearchResult, SearchRun};
use jetcarp_core::library::search::{SearchSpec, search};
use jetcarp_core::library::{self, AddOutcome, Library, Query, Sighting};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{
    self, DiscoveredBy, PatternClass, PatternRecord, Provenance, RuleRange, record_from_object,
    verify,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::*;
use std::collections::HashSet;

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "jetcarp-catalog-test-{name}-{}",
        std::process::id()
    ));
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

/// Records for every periodic object of `rows` placed on a wrapped square grid running `rule`.
fn records(rule: &str, pats: &[(&str, i32, i32)]) -> Vec<PatternRecord> {
    let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = Vec::new();
    for &(rows, u, j) in pats {
        cells.extend(place_rows(&g, rows, u, j));
    }
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    r.objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| record_from_object(&w, w.cas[0].id, o, prov(), DiscoveredBy::User).unwrap())
        .collect()
}

fn one(rule: &str, rows: &str) -> PatternRecord {
    let v = records(rule, &[(rows, 20, 20)]);
    assert_eq!(v.len(), 1, "{rule} {rows}");
    v.into_iter().next().unwrap()
}

fn in_rule(rec: &PatternRecord, rule: &str) -> PatternRecord {
    let mut r = rec.clone();
    r.rule = Rule::parse(rule).unwrap().0.notation();
    r
}

/// An independent, minimal Life-like stepper on the unbounded square grid (Moore neighbours).
fn step(cells: &HashSet<(i32, i32)>, b: u32, s: u32) -> HashSet<(i32, i32)> {
    let mut cand: HashSet<(i32, i32)> = HashSet::new();
    for &(x, y) in cells {
        for dx in -1..=1 {
            for dy in -1..=1 {
                cand.insert((x + dx, y + dy));
            }
        }
    }
    cand.into_iter()
        .filter(|&(x, y)| {
            let mut n = 0;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if (dx, dy) != (0, 0) && cells.contains(&(x + dx, y + dy)) {
                        n += 1;
                    }
                }
            }
            let mask = if cells.contains(&(x, y)) { s } else { b };
            mask >> n & 1 == 1
        })
        .collect()
}

const GLIDER: &str = ".o./..o/ooo";

#[test]
fn glider_has_one_id_across_rules_and_its_hand_derived_range() {
    let life = one("B3/S23", GLIDER);
    let high = one("B36/S23", GLIDER);
    assert_eq!(life.class, PatternClass::Spaceship);
    assert_eq!(life.id, high.id, "same evolution, same pattern");
    assert_eq!(life.rule, "B3/S23");
    assert_eq!(high.rule, "B36/S23");
    // Worked by hand over the four phases: births happen only at 3 live
    // neighbours, and empty cells next to the glider see 1, 2, 4, and 5 without being born; live
    // cells survive with 2 and 3 and die with 1 and 4; 0, 5-8 never occur for live cells and
    // 6-8 never for empty ones. Deaths go straight to 0, so it is plain B/S (2 states).
    let r = life.range.clone().expect("a B/S spaceship has a range");
    assert_eq!(
        r,
        RuleRange {
            bits: 9,
            birth_min: 1 << 3,
            birth_max: (1 << 3) | (1 << 6) | (1 << 7) | (1 << 8),
            survive_min: (1 << 2) | (1 << 3),
            survive_max: 1 | (1 << 2) | (1 << 3) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8),
            states_min: 2,
            states_max: 2,
            min_rule: "B3/S23".into(),
            max_rule: "B3678/S0235678".into(),
        }
    );
    assert_eq!(high.range.as_ref(), Some(&r));
    assert_eq!(r.rule_count(), 256.0);

    // Every B/S rule without B0 on the independent stepper: the glider comes back moved by one
    // cell diagonally after 4 generations exactly in the rules of the range.
    let start: HashSet<(i32, i32)> = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)].into();
    let moved: HashSet<(i32, i32)> = start.iter().map(|&(x, y)| (x + 1, y + 1)).collect();
    let mut works = 0;
    for b in (0..512u32).filter(|b| b & 1 == 0) {
        for s in 0..512u32 {
            let mut c = start.clone();
            for _ in 0..4 {
                c = step(&c, b, s);
            }
            let ok = c == moved;
            let rule = Rule::LifeLike {
                birth: b,
                survive: s,
            };
            assert_eq!(ok, r.contains(&rule), "{}", rule.notation());
            works += ok as u32;
        }
    }
    assert_eq!(works, 256);

    // Rules sampled inside the range verify with the library's verifier; rules just outside fail.
    for k in 0..12u64 {
        let rule = r.rule_at(k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        assert!(r.contains(&rule));
        let v = verify(&in_rule(&life, &rule.notation()), 8);
        assert!(v.is_ok(), "{}: {v:?}", rule.notation());
    }
    for out in [
        "B3/S2",
        "B3/S3",
        "B35/S23",
        "B34/S23",
        "B3/S234",
        "B3/S123",
        "B3/S23/C3",
    ] {
        assert!(!r.contains_notation(out), "{out}");
        assert!(verify(&in_rule(&life, out), 8).is_err(), "{out}");
    }

    // The catalog stores it once, with both rules, and answers range queries.
    let lib = temp_lib("glider");
    assert_eq!(lib.add(life.clone()).unwrap(), AddOutcome::Added);
    assert_eq!(
        lib.add(high.clone()).unwrap(),
        AddOutcome::Merged {
            seen: 2,
            new_rule: true
        }
    );
    let g = lib.get(&life.id).unwrap();
    assert_eq!(g.rules, vec!["B3/S23".to_string(), "B36/S23".to_string()]);
    assert_eq!(g.range.as_ref(), Some(&r));
    assert_eq!(lib.list().unwrap().len(), 1);
    let works_in = |rule: &str| {
        lib.query(&Query {
            works_in: Some(rule.into()),
            ..Default::default()
        })
        .unwrap()
        .len()
    };
    assert_eq!(works_in("B3678/S0235678"), 1); // never seen there, but inside the range
    assert_eq!(works_in("B37/S235"), 1);
    assert_eq!(works_in("B3/S2"), 0);
    assert_eq!(works_in("B3/S23V"), 0); // edge neighbours: another neighbourhood
    assert_eq!(works_in("B3/S23/C3"), 0);
    // The in-memory filter agrees.
    for rule in ["B3678/S0235678", "B3/S2", "B36/S23", "B3/S23V"] {
        let q = Query {
            works_in: Some(rule.into()),
            ..Default::default()
        };
        assert_eq!(q.matches(&g) as usize, works_in(rule), "{rule}");
    }
    let seen_in = lib
        .query(&Query {
            rule: Some("b36/s23".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(seen_in.len(), 1);
    // Sightings: one row per rule (manual job 0).
    let s = lib.sightings(&g.id).unwrap();
    assert_eq!(s.len(), 2);
    assert!(s.iter().all(|x| x.job == 0 && x.count == 1));
}

#[test]
fn different_evolutions_get_different_ids() {
    // Three cells in a row: a blinker in Life, a still life in B4/S12.
    let blinker = one("B3/S23", "ooo");
    let still = one("B4/S12", "ooo");
    assert_eq!(blinker.class, PatternClass::Oscillator);
    assert_eq!(still.class, PatternClass::StillLife);
    assert_ne!(blinker.id, still.id);
    assert!(verify(&blinker, 4).is_ok() && verify(&still, 4).is_ok());
    // The still life: no birth at 0-3 (empty cells beside it see 1, 2, 3), the ends survive with
    // 1 and the middle with 2; any number of Generations states, since nothing dies.
    let r = still.range.clone().unwrap();
    assert_eq!((r.birth_min, r.birth_max), (0, 0b1_1111_0000));
    assert_eq!((r.survive_min, r.survive_max), (0b110, 0b1_1111_1111));
    assert_eq!((r.states_min, r.states_max), (2, 255));
    assert_eq!(
        (r.min_rule.as_str(), r.max_rule.as_str()),
        ("B/S12", "B45678/S012345678")
    );
    // The blinker's range includes HighLife and B3/S2, not Seeds.
    let b = blinker.range.clone().unwrap();
    for x in ["B3/S23", "B36/S23", "B3/S2", "B3678/S0235678"] {
        assert!(b.contains_notation(x), "{x}");
    }
    assert!(!b.contains_notation("B2/S"));
    assert!(!b.contains_notation("B3/S23/C3"));
    // A still life of Life is the same pattern in Generations rules with the same B/S part.
    let block = one("B3/S23", "oo/oo");
    let block4 = one("B3/S23/C4", "oo/oo");
    assert_eq!(block.id, block4.id);
    let lib = temp_lib("evolutions");
    for r in [blinker, still, block, block4] {
        lib.add(r).unwrap();
    }
    assert_eq!(lib.list().unwrap().len(), 3);
    let q = |rule: &str| {
        lib.query(&Query {
            works_in: Some(rule.into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|r| r.class)
        .collect::<Vec<_>>()
    };
    assert_eq!(q("B3/S23/C7"), vec![PatternClass::StillLife]); // the block
    assert_eq!(q("B4/S12").len(), 1);
}

/// Sample rules in each record's range and check them with the verifier; adding one forbidden
/// birth count must break it.
fn check_ranges(recs: &[PatternRecord]) -> usize {
    let mut n = 0;
    for rec in recs {
        let Some(r) = &rec.range else { continue };
        n += 1;
        for k in 0..3u64 {
            let rule = r.rule_at(k.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ rec.cells as u64);
            let v = verify(&in_rule(rec, &rule.notation()), 4);
            assert!(
                v.is_ok(),
                "{} ({}) in {}: {v:?}",
                rec.label(),
                rec.tiling_name,
                rule.notation()
            );
        }
        // A count that occurs for an empty cell and is forbidden: adding it must change things.
        let mask = (1u32 << r.bits.min(31)) - 1;
        let forbidden = mask & !r.birth_max & !1;
        if forbidden != 0 {
            let n = forbidden.trailing_zeros();
            let (b, s, c) = pattern::range::bs_parts(&Rule::parse(&rec.rule).unwrap().0).unwrap();
            let rule = if c == 2 {
                Rule::LifeLike {
                    birth: b | 1 << n,
                    survive: s,
                }
            } else {
                Rule::Generations {
                    birth: b | 1 << n,
                    survive: s,
                    states: c,
                }
            };
            assert!(
                verify(&in_rule(rec, &rule.notation()), 4).is_err(),
                "{} in {} should fail",
                rec.label(),
                rule.notation()
            );
        }
    }
    n
}

#[test]
fn ranges_hold_on_other_tilings_and_generations() {
    let spec = SearchSpec {
        rules: vec!["B2/S34".into(), "B3/S23".into()],
        tilings: pattern::tiling_list("hex,oct,trihex").unwrap(),
        size: SizeSpec::Rect { w: 32, h: 32 },
        boundary: Boundary::Wrap,
        soups: 4,
        gens: 400,
        seed: 3,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    let checked = check_ranges(&out.records);
    assert!(checked >= 10, "only {checked} ranged records");
    // Brian's Brain: its ships leave dying cells, so the state count is fixed at 3.
    let spec = SearchSpec {
        rules: vec!["B2/S/C3".into()],
        tilings: vec![TilingSpec::new("square")],
        size: SizeSpec::Rect { w: 48, h: 48 },
        boundary: Boundary::Wrap,
        soups: 3,
        gens: 400,
        seed: 3,
        density: 0.2,
        explode_fraction: 0.95,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    assert!(check_ranges(&out.records) >= 3);
    for r in &out.records {
        let x = r.range.as_ref().unwrap();
        assert_eq!((x.states_min, x.states_max), (3, 3), "{}", r.label());
        assert_eq!(x.birth_min, 1 << 2, "{}", r.label());
    }
}

#[test]
fn ranges_hold_on_an_aperiodic_patch() {
    let spec = SearchSpec {
        rules: vec!["B35/S234".into()],
        tilings: vec![TilingSpec::new("spectre")],
        size: SizeSpec::Level(4),
        boundary: Boundary::Dead,
        soups: 3,
        gens: 600,
        seed: 1,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    assert!(check_ranges(&out.records) >= 1);
}

#[test]
fn catalog_store_search_jobs_and_sightings() {
    let lib = temp_lib("jobs");
    let spec = SearchSpec {
        rules: vec!["B3/S23".into(), "B36/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        size: SizeSpec::Rect { w: 40, h: 40 },
        boundary: Boundary::Wrap,
        soups: 6,
        gens: 600,
        seed: 7,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    // The search as a job on the runner, its finds stored with the job's last write.
    let js = jetcarp_core::jobs::JobSpec {
        strategy: "search".into(),
        spec: serde_json::to_value(SearchJobSpec {
            search: spec.clone(),
            store: true,
        })
        .unwrap(),
        ..Default::default()
    };
    let jid = jetcarp_core::jobs::create(&lib, &js).unwrap();
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut run = SearchRun::new(spec.clone());
    run.result = Some(slot.clone());
    jetcarp_core::jobs::Runner::new(lib.clone())
        .run_with(jid, &mut run, &Default::default())
        .unwrap();
    let st: SearchResult = slot.lock().unwrap().take().unwrap();
    assert!(st.stored && !st.added.is_empty());
    // Life and HighLife share their common objects: fewer patterns than records.
    let n = lib.list().unwrap().len();
    assert!(n < out.records.len(), "{n} vs {}", out.records.len());
    assert!(!st.new_rules.is_empty());
    let job = lib.job(jid).unwrap();
    assert_eq!(job.status, library::JobStatus::Done);
    assert_eq!(job.kind, "search");
    let block = lib
        .query(&Query {
            text: Some("block".into()),
            ..Default::default()
        })
        .unwrap()
        .remove(0);
    let s = lib.sightings(&block.id).unwrap();
    assert_eq!(s.len(), 2); // one per rule, both in the job
    assert!(s.iter().all(|x| x.job == jid && x.soups > 0));
    assert_eq!(block.seen, s.iter().map(|x| x.count).sum::<u64>());
    // A job's checkpoint round-trips.
    let j = lib
        .create_job("hunt", &serde_json::json!({"rule": "B3/S23"}))
        .unwrap();
    lib.update_job(
        j,
        None,
        Some(&serde_json::json!({"soups_done": 10})),
        Some(&[1, 2, 3]),
    )
    .unwrap();
    assert_eq!(lib.job_checkpoint(j).unwrap(), Some(vec![1, 2, 3]));
    assert_eq!(lib.job(j).unwrap().progress["soups_done"], 10);
    // Fast sightings of a stored pattern.
    let rid = lib.rule_id("B3/S23", Nb::Touch, 1).unwrap();
    lib.add_sightings(
        j,
        &[Sighting {
            pattern_id: block.id.clone(),
            rule_id: rid,
            count: 5,
            soups: 2,
        }],
    )
    .unwrap();
    assert_eq!(lib.get(&block.id).unwrap().seen, block.seen + 5);
    let info = lib.info().unwrap();
    assert_eq!(info.schema_version, library::store::MIGRATIONS.len() as i64);
    assert_eq!(info.patterns as usize, n);
    // A second handle (as another process would) sees the same data.
    let other = Library::open(lib.path());
    assert_eq!(other.list().unwrap().len(), n);
}

#[test]
fn phase3_json_library_imports_with_its_quick_start() {
    // A Phase 3 library folder: patterns/<old id>.json (rule-dependent ids, no rules or range)
    // and quickstart/<old id>.json.
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-catalog-test-p3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("patterns")).unwrap();
    std::fs::create_dir_all(dir.join("quickstart")).unwrap();
    let g = one("B3/S23", GLIDER);
    let mut v: serde_json::Value = serde_json::from_str(&g.to_json()).unwrap();
    v["format"] = 1.into();
    v["id"] = "0123456789abcdef".into();
    v.as_object_mut().unwrap().remove("range");
    std::fs::write(dir.join("patterns/0123456789abcdef.json"), v.to_string()).unwrap();
    let sc = pattern::promote::showcase_scene(&g, Some("Old glider"), None).unwrap();
    std::fs::write(dir.join("quickstart/0123456789abcdef.json"), sc.to_json()).unwrap();
    std::fs::write(
        dir.join("quickstart/sweep-00000000000000aa.json"),
        sc.to_json(),
    )
    .unwrap();
    let lib = temp_lib("p3-catalog");
    let done = lib.import(&dir).unwrap();
    assert_eq!(done, vec![(g.id.clone(), AddOutcome::Added)]);
    let back = lib.get(&g.id).unwrap();
    assert_eq!(back.range, g.range);
    let qs = lib.quickstart(&[]).unwrap();
    assert_eq!(qs.len(), 2);
    let p = qs.iter().find(|q| q.source_kind == "pattern").unwrap();
    assert_eq!(
        (p.id.as_str(), p.title.as_str()),
        (g.id.as_str(), "Old glider")
    );
    assert!(qs.iter().any(|q| q.source_kind == "sweep"));
}

/// Insert and query speed for about 100,000 sightings (run with `--nocapture` to see the
/// numbers).
#[test]
fn catalog_speed_100k_sightings() {
    let lib = temp_lib("speed");
    let base = one("B3/S23", "oo/oo");
    // 2,000 distinct patterns (fake ids, verified as given), 100 rules.
    let mut recs = Vec::new();
    for i in 0..2000 {
        let mut r = base.clone();
        r.id = format!("{:016x}", 0x1000_0000u64 + i);
        r.class = [
            PatternClass::StillLife,
            PatternClass::Oscillator,
            PatternClass::Spaceship,
        ][i as usize % 3];
        r.cells = 4 + (i as usize % 30);
        r.seen = 1;
        recs.push(r);
    }
    let t = std::time::Instant::now();
    let res = lib.add_many(recs, None).unwrap();
    assert!(res.iter().all(|r| r.is_ok()));
    let add_s = t.elapsed().as_secs_f64();
    let rules: Vec<i64> = (0..100)
        .map(|k| {
            let b = 8 | (k as u32 & 0b111) << 4;
            let s = 12 | (k as u32 >> 3) << 4;
            lib.rule_id(
                &Rule::LifeLike {
                    birth: b,
                    survive: s,
                }
                .notation(),
                Nb::Touch,
                1,
            )
            .unwrap()
        })
        .collect();
    let job = lib.create_job("bench", &serde_json::json!({})).unwrap();
    let n = 100_000usize;
    let t = std::time::Instant::now();
    for chunk in (0..n).collect::<Vec<_>>().chunks(10_000) {
        let v: Vec<Sighting> = chunk
            .iter()
            .map(|&i| Sighting {
                pattern_id: format!("{:016x}", 0x1000_0000u64 + (i % 2000) as u64),
                rule_id: rules[(i / 2000) % 100],
                count: 1 + (i % 7) as u64,
                soups: 1,
            })
            .collect();
        lib.add_sightings(job, &v).unwrap();
    }
    let ins = t.elapsed().as_secs_f64();
    let rate = n as f64 / ins;
    let t = std::time::Instant::now();
    let top = lib
        .query(&Query {
            class: Some(PatternClass::Spaceship),
            limit: Some(20),
            ..Default::default()
        })
        .unwrap();
    let q1 = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let in_rule = lib
        .count(&Query {
            works_in: Some("B3/S23".into()),
            ..Default::default()
        })
        .unwrap();
    let q2 = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let s = lib.sightings(&top[0].id).unwrap();
    let q3 = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let rare = lib
        .query(&Query {
            order: Some("rare".into()),
            limit: Some(20),
            ..Default::default()
        })
        .unwrap();
    let q4 = t.elapsed().as_secs_f64();
    println!(
        "catalog speed: {} patterns added in {:.3} s; {n} sightings in {:.3} s ({:.0}/s); \
         top 20 spaceships {:.1} ms; count works_in B3/S23 ({in_rule}) {:.1} ms; \
         sightings of one pattern ({}) {:.2} ms; 20 rarest {:.1} ms",
        res.len(),
        add_s,
        ins,
        rate,
        q1 * 1e3,
        q2 * 1e3,
        s.len(),
        q3 * 1e3,
        q4 * 1e3
    );
    assert_eq!(top.len(), 20);
    assert_eq!(rare.len(), 20);
    assert_eq!(in_rule, 2000);
    assert!(rate > 5_000.0, "{rate:.0} sightings/s");
    let info = lib.info().unwrap();
    assert_eq!(info.sightings, 100_000 + 2000);
}
