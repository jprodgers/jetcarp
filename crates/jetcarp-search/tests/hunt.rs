//! Fast hunter: the bit-parallel engine against the sparse reference stepper on every periodic
//! tiling, soups, determinism, resume, and guns.

use jetcarp_core::hunt::bits::{Plan, Universe};
use jetcarp_core::hunt::separate::{Cache, Census};
use jetcarp_core::hunt::soup::{Layout, continue_soup, prefix_cpu, run_soup, soup_state};
use jetcarp_core::objects::LCell;
use jetcarp_core::objects::puffer::Sparse;
use jetcarp_core::pattern::PatternRecord;
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec, Topology};
use jetcarp_search::hunt::{self, HuntSpec};
use jetcarp_search::jobs::hunt::HuntRun;
use jetcarp_search::jobs::{Budget, RunOptions, Runner};
use jetcarp_store::{JobStatus, Library, Query};
use std::sync::Arc;

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-hunt-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

/// Everything a hunt left in a catalog that must not depend on threads, batches, or resumes:
/// per pattern its id, class, period, cells, seen, first-find soup and RNG state, and the job's
/// sightings.
type Snapshot = (Vec<String>, Vec<(String, String, u64, u64)>);

fn snapshot(lib: &Library, job: i64) -> Snapshot {
    let mut recs: Vec<PatternRecord> = lib.query(&Query::default()).unwrap();
    recs.sort_by(|a, b| a.id.cmp(&b.id));
    let rows = recs
        .iter()
        .map(|r| {
            format!(
                "{} {} {:?} {} p{} {}c seen {} soup {:?} rng {:?} gen {:?} {}",
                r.id,
                r.rule,
                {
                    let mut v = r.rules.clone();
                    v.sort();
                    v
                },
                r.class.name(),
                r.period,
                r.cells,
                r.seen,
                r.provenance.soup,
                r.provenance.rng_state,
                r.provenance.generation,
                r.verified.periods
            )
        })
        .collect();
    (rows, lib.job_sightings(job).unwrap())
}

fn small_spec() -> HuntSpec {
    HuntSpec {
        rules: vec!["B3/S23".into(), "B36/S23".into()],
        tilings: vec![TilingSpec::new("square"), TilingSpec::new("hex")],
        soups: Some(240),
        ..Default::default()
    }
}

/// Run (or resume) a hunt here on the job runner: on `threads` threads (0: all), `batch` soups
/// per batch (0: adapt), stopping (paused) after `limit` soups per group in this session.
fn run_hunt(
    lib: &Library,
    jid: i64,
    threads: usize,
    batch: u64,
    limit: Option<u64>,
) -> hunt::HuntProgress {
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
#[test]
fn hunt_is_independent_of_threads_and_batches() {
    let spec = small_spec();
    let mut snaps = Vec::new();
    let mut totals = Vec::new();
    for (name, threads, batch) in [("t1", 1, 7), ("tall", 0, 0), ("t3", 3, 64)] {
        let lib = temp_lib(name);
        let jid = hunt::start(&lib, &spec).unwrap();
        let p = run_hunt(&lib, jid, threads, batch, None);
        assert_eq!(p.status, "done");
        assert_eq!(p.soups_done, 240);
        assert!(p.objects > 1000, "{}", p.line());
        assert_eq!(p.failed, 0, "{:?}", p.groups);
        snaps.push(snapshot(&lib, jid));
        let mut g = p.groups.clone();
        for x in &mut g {
            x.error = None;
        }
        totals.push(g);
    }
    assert_eq!(snaps[0], snaps[1]);
    assert_eq!(snaps[0], snaps[2]);
    assert_eq!(totals[0], totals[1]);
    assert_eq!(totals[0], totals[2]);
    // The Life glider was found and verified in B3/S23 on squares.
    assert!(snaps[0].0.iter().any(|r| r.contains("spaceship p4 5c")));
}

#[test]
fn hunt_resumed_equals_uninterrupted() {
    let mut spec = small_spec();
    spec.tilings = vec![TilingSpec::new("square")];
    spec.soups = Some(400);
    let a = temp_lib("whole");
    let ja = hunt::start(&a, &spec).unwrap();
    let pa = run_hunt(&a, ja, 0, 0, None);
    let b = temp_lib("halves");
    let jb = hunt::start(&b, &spec).unwrap();
    let half = run_hunt(&b, jb, 0, 50, Some(200));
    assert_eq!(half.status, "paused");
    assert_eq!(half.soups_done, 200);
    assert_eq!(b.job(jb).unwrap().status, JobStatus::Paused);
    // A new process would open the catalog afresh.
    let b2 = Library::open(b.path());
    let pb = run_hunt(&b2, jb, 0, 0, None);
    assert_eq!(pb.status, "done");
    assert_eq!(pa.groups, pb.groups);
    assert_eq!(pa.newest, pb.newest);
    assert_eq!(snapshot(&a, ja), snapshot(&b2, jb));
}

#[test]
fn soup_prefix_then_cpu_equals_cpu() {
    let t = TilingSpec::new("square");
    let c = Census::new("B3/S23", &t, None, 64).unwrap();
    let spec = HuntSpec::default();
    let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
    let limits = spec.limits(layout.cells.len());
    let mut cache = Cache::default();
    let mut handed = 0;
    for i in 0..200 {
        let st = soup_state(1, i);
        let a = run_soup(&c, &mut cache, &layout, limits, i, st);
        let pre = prefix_cpu(&c, layout.region, limits, &layout.soup(st));
        handed += pre.end.is_none() as u32;
        let b = continue_soup(&c, &mut cache, &layout, limits, i, st, pre);
        assert_eq!(
            (a.end, a.period, a.gens, a.escaped),
            (b.end, b.period, b.gens, b.escaped)
        );
        let ids = |o: &jetcarp_core::hunt::soup::SoupOut| {
            let mut v: Vec<String> = o.objects.iter().map(|f| f.id.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&a), ids(&b), "soup {i}");
    }
    assert!(handed > 20, "only {handed} soups left the region");
}

/// Bill Gosper's glider gun (1970), from its well-known published cell list: two queen bee
/// shuttles between two blocks. Rows of `.`/`o`.
const GOSPER: &str = "........................o.........../\
......................o.o.........../\
............oo......oo............oo/\
...........o...o....oo............oo/\
oo........o.....o...oo............../\
oo........o...o.oo....o.o.........../\
..........o.....o.......o.........../\
...........o...o..................../\
............oo......................";

fn gun_record(
    rows: &str,
    rule: &str,
    mirror: bool,
) -> (Census, jetcarp_core::pattern::PatternRecord) {
    use jetcarp_core::hunt::separate::Obj;
    let c = Census::new(rule, &TilingSpec::new("square"), None, 64).unwrap();
    let mut cells = jetcarp_core::objects::rows_to_cells(rows);
    if mirror {
        for x in &mut cells {
            x.i = -x.i;
        }
    }
    let mut cache = Cache::default();
    let parts = c.census(&mut cache, &cells);
    let guns: Vec<_> = parts
        .iter()
        .filter_map(|p| match &p.obj {
            Obj::Pattern(f) if f.class == jetcarp_core::pattern::PatternClass::Gun => {
                Some(f.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(guns.len(), 1, "{rule}: {parts:?}");
    let rec = guns[0].record.clone();
    (c, rec)
}

#[test]
fn gosper_gun_is_detected_verified_and_stored() {
    use jetcarp_core::pattern::{PatternClass, verify};
    let (c, mut rec) = gun_record(GOSPER, "B3/S23", false);
    assert_eq!(rec.class, PatternClass::Gun);
    assert_eq!(rec.period, 30);
    let g = rec.gun.clone().unwrap();
    assert_eq!(g.per_period, 1);
    assert_eq!(g.ship_period, 4);
    assert_eq!(g.ship_label, "glider");
    assert_eq!(g.ship_displacement[0].abs(), 1);
    assert_eq!(g.ship_displacement[1].abs(), 1);
    assert_eq!(rec.gun.as_ref().unwrap().ship_phases.len(), 4);
    assert!(rec.range.is_none(), "guns keep their exact rules");
    // The ship id is the catalog's glider.
    let mut cache = Cache::default();
    let glider = c.census(
        &mut cache,
        &jetcarp_core::objects::rows_to_cells(".o./..o/ooo"),
    );
    let jetcarp_core::hunt::separate::Obj::Pattern(gf) = &glider[0].obj else {
        panic!()
    };
    assert_eq!(g.ship, gf.id);
    // Exact verification on an empty world with room for the stream.
    rec.verified = verify(&rec, 4).unwrap();
    eprintln!(
        "gun verified: {} periods, {}",
        rec.verified.periods, rec.verified.field
    );
    assert!(rec.verified.periods >= 4, "{:?}", rec.verified);
    assert!(
        rec.verified.field.contains("emits 1 ship"),
        "{}",
        rec.verified.field
    );
    // A broken gun fails: drop one cell of a block.
    let mut bad = rec.clone();
    let mut ph = jetcarp_core::pattern::decode_cells(&bad.phases[0]).unwrap();
    ph.remove(0);
    bad.phases[0] = jetcarp_core::pattern::encode_cells(&ph);
    assert!(verify(&bad, 4).is_err());
    // Its mirror image is the same pattern.
    let (_, m) = gun_record(GOSPER, "B3/S23", true);
    assert_eq!(m.id, rec.id);
    // Stored with its ship; `emits` finds it.
    let lib = temp_lib("gun");
    lib.add(rec.clone()).unwrap();
    let back = lib.get(&rec.id).unwrap();
    assert_eq!(back.class, PatternClass::Gun);
    assert_eq!(back.gun, rec.gun);
    assert!(back.label().contains("glider gun"), "{}", back.label());
    let guns = lib
        .query(&Query {
            class: Some(PatternClass::Gun),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(guns.len(), 1);
}

#[test]
fn gun_in_a_soup_run_is_found_and_its_stream_not_counted() {
    use jetcarp_core::hunt::soup::{Limits, SoupRun, finish};
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let cells = jetcarp_core::objects::rows_to_cells(GOSPER);
    let u = Universe::with_cells(c.plan.clone(), &cells);
    let limits = Limits {
        max_gens: 1500,
        settle_period: 64,
        pop_cap: 5000,
        max_span: 4096,
        cut_every: 16,
    };
    let mut run = SoupRun::new(u, [-20, 56, -20, 29], limits);
    let mut cache = Cache::default();
    run.run(&c, &mut cache);
    assert!(!run.cut.is_empty(), "the stream was cut");
    let out = finish(&c, &mut cache, run, 0, 0);
    let classes: Vec<_> = out.objects.iter().map(|f| f.class.name()).collect();
    assert_eq!(
        classes,
        vec!["gun"],
        "{:?}",
        out.objects.iter().map(|f| &f.label).collect::<Vec<_>>()
    );
    assert_eq!(out.escaped, 0);
}

/// A block with gliders of one phase flying the same way from the given offsets, run as a soup
/// with the hunt's default limits for 16 x 16 soups.
fn block_and_gliders(glider: &str, at: &[(i32, i32)]) -> jetcarp_core::hunt::soup::SoupOut {
    use jetcarp_core::hunt::soup::{Limits, SoupRun, finish};
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let glider = jetcarp_core::objects::rows_to_cells(glider);
    let mut cells = jetcarp_core::objects::rows_to_cells("oo/oo");
    for &(di, dj) in at {
        cells.extend(glider.iter().map(|g| LCell {
            i: g.i + di,
            j: g.j + dj,
            ..*g
        }));
    }
    cells.sort_unstable();
    let limits = Limits {
        max_gens: 20_000,
        settle_period: 64,
        pop_cap: 2048,
        max_span: 512,
        cut_every: 16,
    };
    let u = Universe::with_cells(c.plan.clone(), &cells);
    let mut run = SoupRun::new(u, [-10, 10, -10, 10], limits);
    let mut cache = Cache::default();
    run.run(&c, &mut cache);
    finish(&c, &mut cache, run, 0, 0)
}

/// Gliders flying the same way never come nearer to each other, so none of them was ever beyond
/// all the others by the cutting gap when they flew on lanes a few cells apart, or staggered so
/// that each was level with another along every axis it moved on. They stayed uncut until the
/// soup passed the span limit and was dropped as exploded, with no census: about one Life soup in
/// 370, nearly every "explosion" of Life (issue 182). Now they are cut, each counted as a glider,
/// and the soup settles.
#[test]
fn gliders_flying_the_same_way_are_cut_and_counted() {
    use jetcarp_core::hunt::soup::End;
    // As in soup 2023 of seed 7: the second glider 3 cells along and 6 down from the first, one
    // cell clear of it across the lanes and four along them. Then as in soup 2926: three gliders
    // flying up and to the right, the second far behind the first and level with it, the third
    // just ahead of the first along and far behind it up.
    for (glider, at) in [
        (".o./..o/ooo", vec![(20, 20), (23, 26)]),
        ("ooo/..o/.o.", vec![(70, -20), (18, -18), (76, 27)]),
    ] {
        let out = block_and_gliders(glider, &at);
        assert_eq!(out.end, End::Settled, "{at:?}: gens {}", out.gens);
        assert!(out.gens < 300, "{at:?}: cut late, {} generations", out.gens);
        let mut labels: Vec<_> = out.objects.iter().map(|f| f.label.as_str()).collect();
        labels.sort_unstable();
        let mut want = vec!["glider"; at.len()];
        want.insert(0, "block");
        assert_eq!(labels, want, "{at:?}");
        assert_eq!(out.escaped as usize, at.len(), "{at:?}");
    }
}

fn lattice_topo(family: &str) -> Topology {
    Topology::build(&TilingSpec::new(family).with_size(SizeSpec::Rect { w: 8, h: 8 })).unwrap()
}

fn soup(ntypes: u32, n: i32, density: f64, seed: u64, states: u8) -> Vec<LCell> {
    let mut r = Rng::new(seed);
    let mut v = vec![];
    for j in 0..n {
        for i in 0..n {
            for k in 0..ntypes {
                if r.next_f64() < density {
                    let s = if states > 2 && r.next_f64() < 0.3 {
                        2 + r.below(states as u64 - 2) as u8
                    } else {
                        1
                    };
                    v.push(LCell {
                        i: i - 5,
                        j: j - 3,
                        k,
                        s,
                    });
                }
            }
        }
    }
    v.sort_unstable();
    v
}

fn compare(family: &str, nb: Nb, rule: &str, gens: u64, seed: u64) {
    let topo = lattice_topo(family);
    let g = topo.lattice().unwrap();
    let rule = Rule::parse(rule).unwrap().0;
    let plan = Arc::new(Plan::new(&rule, g, nb).unwrap());
    let sp = Sparse::new(&rule, g, nb).unwrap();
    let states = plan.states;
    let start = soup(g.ntypes, 12, 0.4, seed, states);
    let want = sp.run(&start, gens, 1_000_000).unwrap();
    let mut u = Universe::with_cells(plan, &start);
    for (t, w) in want.iter().enumerate() {
        let got = u.cells();
        assert_eq!(
            &got,
            w,
            "{family} {nb:?} {} generation {t}: bit engine differs from the sparse stepper",
            rule.notation()
        );
        assert_eq!(u.population() as usize, w.len());
        u.step();
    }
}

#[test]
fn bit_engine_matches_sparse_on_every_periodic_tiling() {
    let fams: Vec<&str> = jetcarp_core::families::FAMILIES
        .iter()
        .filter(|f| f.kind != jetcarp_core::families::FamilyKind::Aperiodic && f.id != "periodic")
        .map(|f| f.id)
        .collect();
    for (n, fam) in fams.iter().enumerate() {
        for nb in [Nb::Touch, Nb::Edge] {
            compare(fam, nb, "B3/S23", 60, 11 + n as u64);
            compare(fam, nb, "B2/S34", 40, 101 + n as u64);
            compare(fam, nb, "B24/S135/C5", 50, 1001 + n as u64);
        }
    }
}

/// Isotropic rules (#70): the bit engine's decision diagrams against the sparse stepper, which
/// reads the rule's letters cell by cell, with two states and Generations.
#[test]
fn bit_engine_matches_sparse_for_isotropic_rules() {
    for (k, rule) in [
        "B2-a/S12",
        "B3-cnqy/S23-a",
        "B3/S2-i34q",
        "B2n3/S23-q",
        "B2ci3ai4c8/S02ae3eijkq4iz5ar6i7e",
        "B2a3-ckn/S1e2-in3/C4",
        "B1c2-k/S/C3",
    ]
    .iter()
    .enumerate()
    {
        compare("square", Nb::Touch, rule, 60, 70 + k as u64);
    }
    // Only on the square grid's touching neighbours.
    let topo = lattice_topo("hex");
    let rule = Rule::parse("B2-a/S12").unwrap().0;
    assert!(Plan::new(&rule, topo.lattice().unwrap(), Nb::Touch).is_err());
}

/// A hunt in an isotropic rule finds and verifies objects, and its catalog does not depend on
/// the thread count or the batch size.
#[test]
fn isotropic_hunts_are_independent_of_threads_and_batches() {
    let spec = HuntSpec {
        rules: vec!["B3/S2-i34q".into(), "B2-a/S12".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: Some(160),
        ..Default::default()
    };
    let mut snaps = Vec::new();
    for (name, threads, batch) in [("iso-t1", 1, 9), ("iso-tall", 0, 0)] {
        let lib = temp_lib(name);
        let jid = hunt::start(&lib, &spec).unwrap();
        let p = run_hunt(&lib, jid, threads, batch, None);
        assert_eq!(p.status, "done");
        assert_eq!(p.failed, 0, "{:?}", p.groups);
        assert!(p.objects > 100, "{}", p.line());
        snaps.push(snapshot(&lib, jid));
    }
    assert_eq!(snaps[0], snaps[1]);
    // Records carry the rule as JETCARP names it.
    assert!(
        snaps[0].0.iter().any(|r| r.contains(" B3/S2-i34q ")),
        "{:?}",
        snaps[0].0
    );
    assert!(
        snaps[0].0.iter().any(|r| r.contains(" B2-a/S12 ")),
        "{:?}",
        snaps[0].0
    );
}

#[test]
fn bit_engine_hash_ignores_the_universe_layout() {
    let topo = lattice_topo("square");
    let g = topo.lattice().unwrap();
    let rule = Rule::parse("B3/S23").unwrap().0;
    let plan = Arc::new(Plan::new(&rule, g, Nb::Touch).unwrap());
    let a = soup(1, 16, 0.5, 7, 2);
    let mut u = Universe::with_cells(plan.clone(), &a);
    u.run(300);
    let mut v = Universe::with_cells(plan, &u.cells());
    assert_eq!(u.hash(), v.hash());
    assert_eq!(u.population(), v.population());
    v.step();
    u.step();
    assert_eq!(u.hash(), v.hash());
}
