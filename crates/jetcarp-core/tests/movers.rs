//! Movers on aperiodic tilings, verified to a distance. (Pattern-graph dedup on aperiodic
//! patches, through a soup search, is tested in jetcarp-search.)

use jetcarp_core::evolve::arena::deepest;
use jetcarp_core::library::Library;
use jetcarp_core::pattern::movers::{Stop, TrackSpec, mover_record, track};
use jetcarp_core::pattern::site::patch_topology;
use jetcarp_core::pattern::{DiscoveredBy, PatternClass, Provenance, verify};
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};

fn temp_lib(name: &str) -> Library {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-movers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    Library::open(d)
}

/// The published glider pair (states 1 and 3 on two edge neighbours) near the deepest tile of a
/// Spectre level-4 patch: the first start the tracker verifies.
fn spectre_glider_start() -> (TrackSpec, jetcarp_core::pattern::movers::MoverReport) {
    let patch = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let topo = patch_topology(&patch).unwrap();
    let (d, _) = deepest(&topo);
    let mut tried = 0;
    let mut roots: Vec<usize> = vec![d];
    topo.for_each_neighbor(d, Nb::Edge, |y| roots.push(y));
    for r in roots {
        for k in 0..topo.degree(r, Nb::Edge) {
            let u = topo.slot(r, Nb::Edge, k).unwrap();
            let mut spec = TrackSpec::new(
                "SpectreGlider",
                Nb::Touch,
                patch.clone(),
                vec![(r as u32, 1), (u as u32, 3)],
            );
            // Levels 4 to 6, as validated before the exact patches (level 7 below).
            spec.max_level = Some(6);
            tried += 1;
            if let Ok(rep) = track(&spec)
                && rep.verified
            {
                return (spec, rep);
            }
        }
    }
    panic!("no glider among {tried} starts");
}

#[test]
fn spectre_glider_verifies_to_a_distance_across_levels() {
    let (spec, rep) = spectre_glider_start();
    println!("{}", rep.summary);
    assert!(rep.verified, "{}", rep.summary);
    assert_eq!(rep.family, "spectre");
    // Levels 4, 5, 6, each run from generation 0 and agreeing with the smaller one.
    let levels: Vec<u32> = rep.levels.iter().map(|l| l.level).collect();
    assert_eq!(levels, vec![4, 5, 6]);
    for l in &rep.levels[1..] {
        assert_eq!(l.agrees_with_previous, Some(true));
    }
    assert!(rep.levels.iter().all(|l| l.stop == Stop::Edge));
    assert_eq!(rep.level, 6);
    assert!(rep.reach.rings >= 100, "only {} rings", rep.reach.rings);
    assert!(rep.reach.euclid >= 300.0);
    // Two generations per ring and a small, bounded population (the pair is 12 cells at most).
    let per_ring = rep.generations as f64 / rep.reach.rings as f64;
    assert!((1.7..=2.4).contains(&per_ring), "{per_ring}");
    assert!(rep.max_population <= 16 && rep.min_population >= 2);
    // A glider pair: two groups flying apart about 120 degrees.
    assert_eq!(rep.groups.len(), 2, "{:?}", rep.groups);
    let mut a = (rep.groups[0].heading_deg - rep.groups[1].heading_deg).abs();
    if a > 180.0 {
        a = 360.0 - a;
    }
    assert!((105.0..=135.0).contains(&a), "{a}");

    // Into the library with its verification record, which survives a JSON round trip and a
    // re-verification.
    let lib = temp_lib("glider");
    let rec = mover_record(&spec, &rep, Provenance::default(), DiscoveredBy::User).unwrap();
    assert_eq!(rec.class, PatternClass::Mover);
    let m = rec.mover.as_ref().unwrap();
    assert_eq!(m.distance_rings, rep.reach.rings);
    assert_eq!(m.level, 6);
    lib.add(rec.clone()).unwrap();
    let back = lib.get(&rec.id).unwrap();
    assert_eq!(back.mover, rec.mover);
    let v = verify(&back, 1).unwrap();
    assert_eq!(v.generations, rep.generations);
    // Older records (no `mover` field) still load.
    let mut old: serde_json::Value = serde_json::from_str(&back.to_json()).unwrap();
    old.as_object_mut().unwrap().remove("mover");
    let txt = serde_json::to_string(&old).unwrap();
    assert!(jetcarp_core::pattern::PatternRecord::from_json(&txt).is_ok());
}

// The same start tracked onto the exact level-7 patch (2.1 million tiles) goes farther, and
// every level agrees with the one before.
#[test]
fn spectre_glider_verifies_on_level_7() {
    let (mut spec, rep6) = spectre_glider_start();
    spec.max_level = Some(7);
    let rep = track(&spec).unwrap();
    println!("{}", rep.summary);
    assert!(rep.verified, "{}", rep.summary);
    let levels: Vec<u32> = rep.levels.iter().map(|l| l.level).collect();
    assert_eq!(levels, vec![4, 5, 6, 7]);
    for l in &rep.levels[1..] {
        assert_eq!(l.agrees_with_previous, Some(true));
    }
    assert_eq!(rep.level, 7);
    assert_eq!(rep.tiles, 2_147_679);
    assert!(
        rep.reach.rings >= 200 && rep.reach.rings > rep6.reach.rings,
        "{} rings on level 7, {} on level 6",
        rep.reach.rings,
        rep6.reach.rings
    );
    assert_eq!(rep.groups.len(), 2, "{:?}", rep.groups);
    assert!(rep.max_population <= 16 && rep.min_population >= 2);
}

#[test]
fn tracker_reports_why_it_stopped() {
    let patch = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let topo = patch_topology(&patch).unwrap();
    let (d, _) = deepest(&topo);
    let mut nbrs = Vec::new();
    topo.for_each_neighbor(d, Nb::Touch, |y| nbrs.push(y as u32));
    // A single live cell dies under Life.
    let rep = track(&TrackSpec::new(
        "B3/S23",
        Nb::Touch,
        patch.clone(),
        vec![(d as u32, 1)],
    ))
    .unwrap();
    assert!(!rep.verified);
    assert_eq!(rep.stop, Stop::Died);
    // Explosive growth.
    let cells = vec![(d as u32, 1), (nbrs[0], 1)];
    let mut s = TrackSpec::new("B1/S012345678", Nb::Touch, patch.clone(), cells);
    s.max_pop = 300;
    let rep = track(&s).unwrap();
    assert!(!rep.verified);
    assert!(
        matches!(rep.stop, Stop::Grew | Stop::Edge),
        "{:?}",
        rep.stop
    );
    if rep.stop == Stop::Edge {
        assert!(rep.max_population > 300 || rep.reach.rings < 2);
    }
    // A two-cell still life in B3/S12 repeats in place: not a mover.
    let rep = track(&TrackSpec::new(
        "B3/S12",
        Nb::Touch,
        patch,
        vec![(d as u32, 1), (nbrs[0], 1)],
    ))
    .unwrap();
    assert_eq!(rep.stop, Stop::Repeated);
    assert!(!rep.verified);
}
