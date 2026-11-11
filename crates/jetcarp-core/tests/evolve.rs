//! Evolutionary glider search (`jetcarp_core::evolve`): determinism, the anti-gaming filters,
//! and a search checkpointed and resumed. (Searches as catalog jobs, and the gliders they store,
//! are tested in jetcarp-search.)

use jetcarp_core::evolve::arena::{Arena, Outcome};
use jetcarp_core::evolve::priority::{PriorityRule, spectre_glider};
use jetcarp_core::evolve::{Arenas, Evolution, EvolveCheckpoint, EvolveSpec, cross_check, evolve};
use jetcarp_core::topology::{Nb, TilingSpec};

fn rule(t: &str) -> PriorityRule {
    PriorityRule::parse(t).unwrap().unwrap()
}

fn small(tiling: &str, pop: usize, gens: usize, seed: u64) -> EvolveSpec {
    EvolveSpec {
        tiling: TilingSpec::new(tiling),
        pop,
        gens,
        seed,
        radius: 24,
        horizon: 1200,
        champions: 4,
        verify_factors: vec![1, 2],
        ..Default::default()
    }
}

#[test]
fn search_is_identical_at_any_thread_count() {
    for tiling in ["spectre", "square"] {
        let mut a = small(tiling, 48, 12, 5);
        a.threads = Some(1);
        let mut b = a.clone();
        b.threads = Some(8);
        let ra = evolve(&a, &|_| true).unwrap();
        let rb = evolve(&b, &|_| true).unwrap();
        assert_eq!(ra.fingerprint(), rb.fingerprint(), "{tiling}");
        assert_eq!(ra.generations.len(), 12);
        // And a different seed gives a different run.
        let mut c = a.clone();
        c.seed = 6;
        assert_ne!(
            evolve(&c, &|_| true).unwrap().fingerprint(),
            ra.fingerprint()
        );
    }
}

#[test]
fn anti_gaming_filters() {
    for tiling in ["spectre", "square"] {
        let a = Arena::build(&TilingSpec::new(tiling), Nb::Touch, 32).unwrap();
        let (glider, _, _) = a.fitness(&spectre_glider(), 2500, 64);
        if tiling == "spectre" {
            assert!(glider >= 0.9 * a.boundary as f64, "glider {glider}");
        }
        // Explosive: any quiet cell next to a live one is born, and nothing dies.
        let (f, _, st) = a.fitness(&rule("Priority 2: 0|n1>=1->1; 1|always->1"), 2500, 64);
        assert_eq!(
            (f, st.outcome),
            (0.0, Outcome::Unbounded),
            "{tiling} explosive"
        );
        // Blinker farm: every touched cell joins a growing disc of period-2 blinkers.
        let farm = rule("Priority 3: 0|n1>=1->2; 0|n2>=1->1; 1|always->2; 2|always->1");
        let (f, _, st) = a.fitness(&farm, 2500, 64);
        assert_eq!((f, st.outcome), (0.0, Outcome::Unbounded), "{tiling} farm");
        // A lone blinker: periodic, no travel, a tiny score.
        let blinker = rule("Priority 3: 1|always->2; 2|always->1");
        let (f, i, st) = a.fitness(&blinker, 2500, 64);
        assert_eq!(st.outcome, Outcome::Periodic);
        assert!(f <= 2.0, "{tiling} blinker scores {f}");
        // A slow thin grower would score below a glider through the flatness ratio, and the
        // cross-radius check rejects anything that does not reach the boundary.
        let mut arenas = Arenas::new(&EvolveSpec {
            tiling: TilingSpec::new(tiling),
            radius: 32,
            verify_factors: vec![1, 2],
            ..Default::default()
        })
        .unwrap();
        let cc = cross_check(&mut arenas, &blinker, i, &[1, 2]).unwrap();
        assert!(!cc.passed, "{}", cc.reason);
        // Boundary sniffing: cells with few neighbours (only at the ball's edge) ignite at
        // once; the light cone filter gives it 0.
        let sniff = rule("Priority 2: *|n0>=4->0; *|always->1");
        let (f, _, st) = a.fitness(&sniff, 2500, 64);
        assert!(!st.causal, "{tiling}: {st:?}");
        assert_eq!(f, 0.0);
    }
}

#[test]
fn spectre_glider_passes_cross_radius_check() {
    let spec = EvolveSpec {
        tiling: TilingSpec::new("spectre"),
        radius: 32,
        verify_factors: vec![1, 2, 3],
        ..Default::default()
    };
    let mut arenas = Arenas::new(&spec).unwrap();
    let (_, seed, _) = arenas.base.fitness(&spectre_glider(), 2500, 64);
    let cc = cross_check(&mut arenas, &spectre_glider(), seed, &[1, 2, 3]).unwrap();
    assert!(cc.passed, "{cc:?}");
    assert_eq!(cc.radii.len(), 3);
    let peaks: Vec<usize> = cc.radii.iter().map(|r| r.max_population).collect();
    assert!(peaks.iter().all(|&p| p == peaks[0]));
    // Two generations per ring, as published.
    for r in &cc.radii {
        let per_ring = r.generation as f64 / r.radius as f64;
        assert!((1.6..=2.4).contains(&per_ring), "{per_ring}");
    }
}

#[test]
fn a_search_checkpointed_and_resumed_equals_one_run() {
    // One generation at a time, saved as JSON after the fourth and carried on from there.
    let spec = small("square", 48, 10, 5);
    let whole = evolve(&spec, &|_| true).unwrap();
    let mut e = Evolution::new(&spec).unwrap();
    for _ in 0..4 {
        assert!(e.generation(&|_| true).unwrap());
    }
    let saved = serde_json::to_string(&e.checkpoint()).unwrap();
    drop(e);
    let cp: EvolveCheckpoint = serde_json::from_str(&saved).unwrap();
    assert_eq!((cp.next, cp.generations.len(), cp.over), (4, 4, false));
    let mut e = Evolution::resume(&spec, &cp).unwrap();
    while e.generation(&|_| true).unwrap() {}
    assert!(e.is_over());
    let resumed = e.finish().unwrap();
    assert_eq!(resumed.fingerprint(), whole.fingerprint());
    assert_eq!(resumed.generations.len(), 10);
    let rules = |r: &jetcarp_core::evolve::EvolveReport| {
        r.champions
            .iter()
            .map(|c| (c.rule.clone(), c.genome.clone(), c.seed_index))
            .collect::<Vec<_>>()
    };
    assert_eq!(rules(&resumed), rules(&whole));
    // A checkpoint of another population size is refused.
    assert!(Evolution::resume(&small("square", 32, 10, 5), &cp).is_err());
}
