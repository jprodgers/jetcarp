//! The GPU batch stepper must reproduce `run_cpu` exactly: population, changes, hash and frames,
//! for several worlds with different rules in one batch, on lattice and aperiodic topologies and
//! dead, wrap and live edges. Skipped (with a note) when no GPU adapter is available, failed under
//! `JETCARP_REQUIRE_GPU=1`.

use jetcarp_core::explore::batch::{BatchJob, BatchWorld, WorldTrace, run_cpu};
use jetcarp_core::isotropic::IsoRule;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Boundary, Nb, SizeSpec};
use jetcarp_core::{TilingSpec, Topology};
use jetcarp_gpu::batch::BatchSim;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

fn gpu() -> Option<MutexGuard<'static, BatchSim>> {
    static SIM: OnceLock<Option<Mutex<BatchSim>>> = OnceLock::new();
    let sim = SIM.get_or_init(|| match BatchSim::headless() {
        Ok(s) => {
            eprintln!("GPU batch tests on {}", s.name());
            Some(Mutex::new(s))
        }
        Err(e) => {
            jetcarp_gpu::no_gpu("GPU batch tests", &e);
            None
        }
    });
    sim.as_ref()
        .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

fn rng_next(s: &mut u64) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    (*s >> 16) as u32
}

/// Random states below `limit` at 40% density (0 otherwise, or uniformly at random for rules
/// where any state is meaningful).
fn random_init(n: usize, limit: u32, seed: u64) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    (0..n)
        .map(|_| {
            if rng_next(&mut s) % 10 < 4 {
                (rng_next(&mut s) % limit.max(1)) as u8
            } else {
                0
            }
        })
        .collect()
}

fn dense_init(n: usize, limit: u32, seed: u64) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0xC2B2AE3D27D4EB4F) | 1;
    (0..n)
        .map(|_| (rng_next(&mut s) % limit.max(1)) as u8)
        .collect()
}

/// Bands of one sign, so opposite signs meet along interfaces without annihilating everywhere.
fn stripes_init(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    (0..n)
        .map(|i| {
            let sign = ((i / 24) % 2 + 1) as u8;
            if rng_next(&mut s) % 10 < 4 { sign } else { 0 }
        })
        .collect()
}

/// A per-rule initial state range (rules with thresholds need denser or multi-state seeds).
fn init_for(rule: &str, n: usize, k: u64) -> Vec<u8> {
    let r = Rule::parse(rule).unwrap().0;
    match &r {
        Rule::Cyclic { states, .. } | Rule::Rps { states, .. } => dense_init(n, *states as u32, k),
        Rule::GreenbergHastings { states, .. } => random_init(n, *states as u32, k),
        Rule::Generations { states, .. } => random_init(n, *states as u32, k),
        Rule::Hodgepodge { n: m, .. } => dense_init(n, *m as u32 + 1, k),
        Rule::WireWorld => dense_init(n, 4, k),
        Rule::SpectreGlider => dense_init(n, 4, k),
        Rule::Priority(p) => dense_init(n, p.states as u32, k),
        Rule::Ltl(l) => random_init(n, l.states as u32, k),
        Rule::Jaca(_) => stripes_init(n, k),
        _ => random_init(n, 2, k),
    }
}

fn job(topo: &Arc<Topology>, nb: Nb, rules: &[&str], gens: u32, frames: &[u32]) -> BatchJob {
    let n = topo.len();
    let worlds = rules
        .iter()
        .enumerate()
        .map(|(k, r)| BatchWorld {
            rule: Rule::parse(r).unwrap().0,
            init: init_for(r, n, 17 + k as u64),
        })
        .collect();
    BatchJob {
        topo: topo.clone(),
        nb,
        worlds,
        gens,
        frames_at: frames.to_vec(),
    }
}

fn compare(sim: &mut BatchSim, job: &BatchJob, what: &str) -> Vec<WorldTrace> {
    let cpu = run_cpu(job).unwrap_or_else(|e| panic!("{what}: CPU: {e}"));
    let gpu = sim.run(job).unwrap_or_else(|e| panic!("{what}: GPU: {e}"));
    assert_eq!(cpu.len(), gpu.len(), "{what}: world count");
    for (w, (c, g)) in cpu.iter().zip(&gpu).enumerate() {
        diff(c, g, what, w, &job.worlds[w].rule.notation());
    }
    cpu
}

/// A rule's world is alive when it changes after the first generations (not at the start, where
/// seeds die or settle for reasons unrelated to the stepper).
fn note_activity(activity: &mut HashMap<String, u64>, job: &BatchJob, traces: &[WorldTrace]) {
    for (w, t) in job.worlds.iter().zip(traces) {
        let late: u64 = t.changed.iter().skip(5).map(|&c| c as u64).sum();
        *activity.entry(w.rule.notation()).or_default() += late;
    }
}

fn assert_all_active(activity: &HashMap<String, u64>) {
    let idle: Vec<&String> = activity
        .iter()
        .filter(|(_, c)| **c == 0)
        .map(|(r, _)| r)
        .collect();
    assert!(idle.is_empty(), "these rules never did anything: {idle:?}");
}

fn diff(c: &WorldTrace, g: &WorldTrace, what: &str, w: usize, rule: &str) {
    for t in 0..c.pop.len() {
        assert_eq!(
            (c.pop[t], c.changed[t], c.hash[t], c.neg[t]),
            (g.pop[t], g.changed[t], g.hash[t], g.neg[t]),
            "{what}: world {w} ({rule}) differs at generation {t}"
        );
    }
    assert_eq!(
        c.pop.len(),
        g.pop.len(),
        "{what}: world {w} ({rule}) trace length"
    );
    assert_eq!(c.frames, g.frames, "{what}: world {w} ({rule}) frames");
}

const RULES: &[&str] = &[
    "B3/S23",
    "B02/S/C3",
    "B36/S125",
    "B1357/S1357",
    "B2/S/C3",
    "B2/S13/C5",
    "WireWorld",
    "Cyclic 8/1",
    "Cyclic 5/2",
    "RPS 3/1",
    "RPS 5/2",
    "GH 8/1",
    "GH 12/2",
    "Hodgepodge 100/2/3/28",
    "Hodgepodge 16/1/1/4",
    "SpectreGlider",
    "Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1",
    "Priority 3: 0|n1>=2&n2>=1&n0>=1->1; 1|n2>=1->2; *|always->1",
    "JACA B3/S23",
    "JACA +B3/S23 -B36/S23",
    "JACA B2/S blast 2",
    "JACA B3/S23 blast 1",
    "JACA B36/S23 blast 3",
    "B0/S2",
    "B02/S",
    "B0/S0123456",
    "B013/S34",
    // B0 rules whose survival list holds some of the tiles' neighbour counts on 4.8.8 (4 and 8)
    // and 3.4.6.4 (6, 8 and 12): backgrounds that differ between tile types (#171).
    "B0/S8",
    "B03/S238",
];

const LTL_RULES: &[&str] = &[
    "R3,C2,S11-20,B13-18",
    "R3,C2,S5-14,B9-11",
    "R3,C4,S7-20,B10-15",
    "R3,C0,M1,S12-21,B13-18",
];

fn tilings() -> Vec<(&'static str, TilingSpec)> {
    let at = |name: &str, level: u32| TilingSpec {
        size: Some(SizeSpec::Level(level)),
        ..TilingSpec::new(name)
    };
    vec![
        (
            "square wrap",
            TilingSpec::rect("square", 37, 29, Boundary::Wrap),
        ),
        (
            "square dead",
            TilingSpec::rect("square", 37, 29, Boundary::Dead),
        ),
        (
            "square live",
            TilingSpec::rect("square", 21, 19, Boundary::Live),
        ),
        ("hex wrap", TilingSpec::rect("hex", 33, 31, Boundary::Wrap)),
        ("hex dead", TilingSpec::rect("hex", 33, 31, Boundary::Dead)),
        ("hex live", TilingSpec::rect("hex", 23, 17, Boundary::Live)),
        (
            "trihex wrap",
            TilingSpec::rect("trihex", 13, 11, Boundary::Wrap),
        ),
        (
            "trihex dead",
            TilingSpec::rect("trihex", 13, 11, Boundary::Dead),
        ),
        (
            "rhombitrihex wrap",
            TilingSpec::rect("rhombitrihex", 8, 8, Boundary::Wrap),
        ),
        (
            "rhombitrihex dead",
            TilingSpec::rect("rhombitrihex", 8, 8, Boundary::Dead),
        ),
        ("oct dead", TilingSpec::rect("oct", 12, 10, Boundary::Dead)),
        ("oct live", TilingSpec::rect("oct", 12, 10, Boundary::Live)),
        ("spectre level 2", at("spectre", 2)),
        ("spectre level 3", at("spectre", 3)),
        ("hat level 2", at("hat", 2)),
        ("hat level 3", at("hat", 3)),
    ]
}

#[test]
fn every_rule_every_tiling_matches() {
    let Some(mut sim) = gpu() else { return };
    let gens = 60u32;
    let frames = [0u32, 1, 2, 17, 60];
    let mut activity = HashMap::new();
    for (name, spec) in tilings() {
        let topo = Arc::new(Topology::build(&spec).unwrap_or_else(|e| panic!("{name}: {e}")));
        eprintln!("{name}: {} cells", topo.len());
        for nb in [Nb::Touch, Nb::Edge] {
            let j = job(&topo, nb, RULES, gens, &frames);
            let t = compare(&mut sim, &j, &format!("{name} {nb:?}"));
            note_activity(&mut activity, &j, &t);
        }
    }
    assert_all_active(&activity);
}

#[test]
fn larger_than_life_matches() {
    let Some(mut sim) = gpu() else { return };
    let gens = 30u32;
    let frames = [0u32, 7, 30];
    let mut activity = HashMap::new();
    for (name, spec) in tilings() {
        let topo = Arc::new(Topology::build(&spec).unwrap_or_else(|e| panic!("{name}: {e}")));
        // Hood and range shared by the batch; the rules differ, and non-LtL worlds join in.
        let mut rules: Vec<&str> = LTL_RULES.to_vec();
        rules.extend(["B3/S23", "Cyclic 5/1"]);
        let j = job(&topo, Nb::Touch, &rules, gens, &frames);
        let t = compare(&mut sim, &j, &format!("LtL range 3 {name}"));
        note_activity(&mut activity, &j, &t);
        // Von Neumann hood at range 2.
        let vn = ["R2,C2,S1-6,B2-5,NN", "R2,C3,S0-3,B3-5,NN"];
        let j = job(&topo, Nb::Touch, &vn, gens, &frames);
        compare(&mut sim, &j, &format!("LtL von Neumann {name}"));
    }
    // The cross hood exists on the square grid only.
    let topo =
        Arc::new(Topology::build(&TilingSpec::rect("square", 41, 37, Boundary::Wrap)).unwrap());
    let cross = ["R3,C2,S2-6,B3-5,N+", "R3,C2,S1-4,B4-8,N+"];
    let j = job(&topo, Nb::Touch, &cross, gens, &frames);
    compare(&mut sim, &j, "LtL cross");
    assert_all_active(&activity);
}

/// A tiny submission target forces every generation to run as separate ranges of worlds, with
/// the trace and the state swap spread over several submissions.
#[test]
fn generations_split_over_submissions() {
    let Some(mut sim) = gpu() else { return };
    sim.set_target_secs(1e-6);
    let topo =
        Arc::new(Topology::build(&TilingSpec::rect("square", 37, 29, Boundary::Wrap)).unwrap());
    let j = job(&topo, Nb::Touch, RULES, 40, &[0, 3, 40]);
    compare(&mut sim, &j, "split generations");
    assert!(
        sim.last_stats().submissions > 40,
        "expected many submissions, got {:?}",
        sim.last_stats()
    );
    sim.set_target_secs(0.075);
}

/// A batch is given up between submissions once its cut says so, and the next batch runs as
/// usual.
#[test]
fn a_batch_is_given_up_when_cut() {
    let Some(mut sim) = gpu() else { return };
    sim.set_target_secs(1e-6);
    let topo =
        Arc::new(Topology::build(&TilingSpec::rect("square", 37, 29, Boundary::Wrap)).unwrap());
    let j = job(&topo, Nb::Touch, RULES, 40, &[0, 3, 40]);
    let asked = std::cell::Cell::new(0);
    let r = sim.run_until(&j, &|| {
        asked.set(asked.get() + 1);
        asked.get() > 5
    });
    assert_eq!(
        r.err().as_deref(),
        Some(jetcarp_core::explore::batch::GIVEN_UP)
    );
    let n = sim.last_stats().submissions;
    assert!((1..=10).contains(&n), "{n} submissions before the cut");
    compare(&mut sim, &j, "after a cut");
    sim.set_target_secs(0.075);
}

#[test]
fn padding_and_multi_type_worlds() {
    let Some(mut sim) = gpu() else { return };
    // Cell counts just below and above a workgroup size multiple.
    for (w, h) in [(15, 17), (16, 16), (17, 15), (16, 17), (8, 8)] {
        let spec = TilingSpec::rect("square", w, h, Boundary::Wrap);
        let topo = Arc::new(Topology::build(&spec).unwrap());
        let j = job(&topo, Nb::Touch, RULES, 40, &[0, 1, 40]);
        compare(&mut sim, &j, &format!("padding {w}x{h}"));
    }
}

#[test]
fn many_worlds_reuse_and_frame_edge_cases() {
    let Some(mut sim) = gpu() else { return };
    let topo = Arc::new(Topology::build(&TilingSpec::rect("hex", 37, 41, Boundary::Wrap)).unwrap());
    let mut rules: Vec<String> = Vec::new();
    for i in 0..40 {
        rules.push(format!("B{}/S{}", 2 + i % 5, 1 + (i * 3) % 7));
    }
    let refs: Vec<&str> = rules.iter().map(|s| s.as_str()).collect();
    // Several jobs on the same device: shrinking, growing, and repeated, with frames at unusual
    // places and generation zero.
    let cases: [(&[&str], u32, &[u32]); 4] = [
        (&refs, 300, &[0, 300]),
        (&refs[..3], 200, &[150]),
        (&refs[..40], 50, &[1, 2, 3, 50]),
        (&refs[..7], 10, &[]),
    ];
    for (r, gens, frames) in cases {
        let j = job(&topo, Nb::Touch, r, gens, frames);
        compare(
            &mut sim,
            &j,
            &format!("{} worlds, {gens} generations", r.len()),
        );
    }
    // Generation zero runs nothing.
    let j = job(&topo, Nb::Touch, &refs[..2], 0, &[0]);
    compare(&mut sim, &j, "zero generations");
}

/// Worlds too wide for one dispatch dimension need more than one row of workgroups per world.
/// Heavy (17 M cells each), so not part of the default run.
#[test]
#[ignore = "heavy: two worlds of 17 million cells"]
fn world_larger_than_one_dispatch_dimension() {
    let Some(mut sim) = gpu() else { return };
    let topo =
        Arc::new(Topology::build(&TilingSpec::rect("square", 4100, 4100, Boundary::Wrap)).unwrap());
    assert!(topo.len() > 65535 * 256);
    let j = job(&topo, Nb::Touch, &["B3/S23", "B36/S23"], 4, &[0, 4]);
    compare(&mut sim, &j, "huge worlds");
}

/// A random isotropic rule (#70): each of its 102 letters (51 arrangements, for birth and for
/// survival) on or off at random, with birth on 0 or not as `b0` says, and never a B/S rule.
fn random_iso(s: &mut u64, states: u8, b0: bool) -> IsoRule {
    use jetcarp_core::isotropic::ALL_CLASSES;
    loop {
        let mut bits = || ((rng_next(s) as u64) << 32 | rng_next(s) as u64) & ALL_CLASSES;
        let (birth, survive) = (bits(), bits());
        let birth = if b0 { birth | 1 } else { birth & !1 };
        let r = IsoRule::from_class_masks(birth, survive, states);
        if r.totalistic().is_none() {
            return r;
        }
    }
}

/// An isotropic rule near Life (or Brian's Brain's counts, for Generations), as the explore
/// search draws them: one to six letters turned on or off.
fn near_life(s: &mut u64, states: u8) -> IsoRule {
    use jetcarp_core::isotropic::CLASSES;
    loop {
        let (b, sv) = if states > 2 {
            (0b100, 0)
        } else {
            (0b1000, 0b1100)
        };
        let (mut birth, mut survive) = IsoRule::from_masks(b, sv, states).class_masks();
        for _ in 0..1 + rng_next(s) % 6 {
            let k = rng_next(s) % CLASSES;
            if rng_next(s).is_multiple_of(2) {
                birth ^= 1 << k.max(1);
            } else {
                survive ^= 1 << k;
            }
        }
        let r = IsoRule::from_class_masks(birth, survive, states);
        if r.totalistic().is_none() {
            return r;
        }
    }
}

/// `v` in a random order.
fn shuffle<T>(v: &mut [T], s: &mut u64) {
    for i in (1..v.len()).rev() {
        v.swap(i, rng_next(s) as usize % (i + 1));
    }
}

/// A rule written in Hensel's notation as a person might: its counts in any order, each count's
/// letters in any order, the `-` form or the plain one at random, and a count with every letter
/// either bare or with all of them listed.
fn shuffled(r: &IsoRule, s: &mut u64) -> String {
    use jetcarp_core::isotropic::LETTERS;
    let mut half = |m: &[u16; 9]| -> String {
        let mut counts: Vec<usize> = (0..9).filter(|&n| m[n] != 0).collect();
        shuffle(&mut counts, s);
        let mut out = String::new();
        for n in counts {
            out.push(char::from(b'0' + n as u8));
            let letters: Vec<char> = LETTERS[n].chars().collect();
            let (mut on, mut off): (Vec<char>, Vec<char>) = (Vec::new(), Vec::new());
            for (i, &c) in letters.iter().enumerate() {
                if m[n] >> i & 1 == 1 {
                    on.push(c);
                } else {
                    off.push(c);
                }
            }
            let mut pick = match (off.is_empty(), rng_next(s) % 2) {
                (true, 0) => Vec::new(),
                (true, _) => on,
                (false, 0) => {
                    out.push('-');
                    off
                }
                (false, _) => on,
            };
            shuffle(&mut pick, s);
            out.extend(pick);
        }
        out
    };
    let (b, sv) = (half(&r.birth), half(&r.survive));
    match r.states {
        2 => format!("B{b}/S{sv}"),
        k => format!("B{b}/S{sv}/C{k}"),
    }
}

/// The isotropic rules of the cell-for-cell test, two-state (near Life and at random), two-state
/// with birth on 0, and Generations (with birth on 0 too), each also written in shuffled
/// notation and read back.
fn iso_rules(seed: u64) -> Vec<Rule> {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    let mut rules: Vec<IsoRule> = Vec::new();
    for _ in 0..4 {
        rules.push(near_life(&mut s, 2));
        rules.push(random_iso(&mut s, 2, false));
    }
    for _ in 0..3 {
        rules.push(random_iso(&mut s, 2, true));
    }
    for k in 0..6 {
        let states = 3 + (rng_next(&mut s) % 6) as u8;
        rules.push(match k % 3 {
            0 => near_life(&mut s, states),
            1 => random_iso(&mut s, states, false),
            _ => random_iso(&mut s, states, true),
        });
    }
    let mut out = Vec::new();
    for r in rules {
        let text = shuffled(&r, &mut s);
        let (back, _) = Rule::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(back.notation(), r.notation(), "{text} read back");
        out.push(Rule::Isotropic(Box::new(r)));
        out.push(back);
    }
    out
}

/// A batch of isotropic worlds on random soups, with a frame of every generation.
fn iso_job(topo: &Arc<Topology>, nb: Nb, rules: &[Rule], gens: u32, seed: u64) -> BatchJob {
    let n = topo.len();
    let worlds = rules
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let states = match r {
                Rule::Isotropic(i) => i.states as u32,
                _ => 2,
            };
            let k = seed + k as u64;
            BatchWorld {
                rule: r.clone(),
                init: if k.is_multiple_of(3) {
                    dense_init(n, states, k)
                } else {
                    random_init(n, states, k)
                },
            }
        })
        .collect();
    BatchJob {
        topo: topo.clone(),
        nb,
        worlds,
        gens,
        frames_at: (0..=gens).collect(),
    }
}

fn square_grids() -> Vec<(&'static str, TilingSpec)> {
    vec![
        (
            "square wrap",
            TilingSpec::rect("square", 37, 29, Boundary::Wrap),
        ),
        (
            "square dead",
            TilingSpec::rect("square", 37, 29, Boundary::Dead),
        ),
        (
            "square live",
            TilingSpec::rect("square", 21, 19, Boundary::Live),
        ),
        (
            "square wrap 16x16",
            TilingSpec::rect("square", 16, 16, Boundary::Wrap),
        ),
        (
            "square dead 15x17",
            TilingSpec::rect("square", 15, 17, Boundary::Dead),
        ),
        // The smallest grids there are (8 by 8 unit cells), where every cell is near an edge.
        (
            "square wrap 8x8",
            TilingSpec::rect("square", 8, 8, Boundary::Wrap),
        ),
        (
            "square live 8x9",
            TilingSpec::rect("square", 8, 9, Boundary::Live),
        ),
        (
            "square dead 9x8",
            TilingSpec::rect("square", 9, 8, Boundary::Dead),
        ),
    ]
}

/// The batch kernel's isotropic rules (#245) step exactly as the processor's: every cell of every
/// generation, for random rules of all 102 letters and rules near Life, two-state, two-state with
/// birth on 0 and Generations, written canonically and in shuffled notation, on random soups on
/// the square grid with wrapped, dead and live edges, both neighbourhoods given (an isotropic rule
/// reads the eight touching neighbours whatever the batch's).
#[test]
fn isotropic_rules_match_cell_for_cell() {
    let Some(mut sim) = gpu() else { return };
    let gens = 40u32;
    let mut late = [0u64; 3];
    for (k, (name, spec)) in square_grids().into_iter().enumerate() {
        let topo = Arc::new(Topology::build(&spec).unwrap_or_else(|e| panic!("{name}: {e}")));
        for nb in [Nb::Touch, Nb::Edge] {
            let rules = iso_rules(101 + k as u64 * 7 + nb as u64);
            let j = iso_job(&topo, nb, &rules, gens, 300 + k as u64);
            let t = compare(&mut sim, &j, &format!("isotropic, {name} {nb:?}"));
            for (w, tr) in j.worlds.iter().zip(&t) {
                let Rule::Isotropic(r) = &w.rule else {
                    panic!("{} is not isotropic", w.rule.notation())
                };
                let kind = if r.states > 2 { 2 } else { r.b0() as usize };
                late[kind] += tr.changed.iter().skip(5).map(|&c| c as u64).sum::<u64>();
            }
        }
    }
    assert!(
        late.iter().all(|&c| c > 0),
        "some kinds never changed: {late:?}"
    );
}

/// Isotropic worlds beside other kinds in one batch (B/S, Generations, JACA with a blast, and
/// Larger than Life, whose reach is a second neighbour list), and batches with and without them
/// on one grid in turn, so the neighbour lists the device keeps are kept or built again as each
/// batch needs.
#[test]
fn isotropic_rules_in_mixed_batches_match() {
    let Some(mut sim) = gpu() else { return };
    let topo =
        Arc::new(Topology::build(&TilingSpec::rect("square", 33, 27, Boundary::Wrap)).unwrap());
    let iso = iso_rules(7);
    let other: Vec<Rule> = [
        "B3/S23",
        "B2/S13/C5",
        "JACA B2/S blast 2",
        "B0/S8",
        "R3,C2,S11-20,B13-18",
    ]
    .iter()
    .map(|r| Rule::parse(r).unwrap().0)
    .collect();
    let mixed: Vec<Rule> = iso
        .iter()
        .cloned()
        .zip(other.iter().cycle().cloned())
        .flat_map(|(a, b)| [a, b])
        .collect();
    let no_ltl: Vec<Rule> = mixed
        .iter()
        .filter(|r| !matches!(r, Rule::Ltl(_)))
        .cloned()
        .collect();
    let plain = || {
        job(
            &topo,
            Nb::Touch,
            &["B3/S23", "B36/S23", "Cyclic 5/2"],
            30,
            &[0, 30],
        )
    };
    for (what, rules) in [
        ("plain first", None),
        ("mixed with Larger than Life", Some(&mixed)),
        ("plain after", None),
        ("mixed", Some(&no_ltl)),
        ("isotropic alone", Some(&iso)),
        ("plain last", None),
        ("isotropic again", Some(&iso)),
    ] {
        let j = match rules {
            Some(r) => {
                let mut j = iso_job(&topo, Nb::Touch, r, 30, 11);
                for w in &mut j.worlds {
                    w.init = init_for(&w.rule.notation(), topo.len(), 5);
                }
                j
            }
            None => plain(),
        };
        compare(&mut sim, &j, what);
    }
}

/// Isotropic rules run in a batch, and on the square grid only: elsewhere the batch is refused,
/// as the processor refuses it.
#[test]
fn isotropic_rules_need_the_square_grid() {
    let rule = Rule::parse("B2-a/S12").unwrap().0;
    assert!(BatchSim::supports(&rule));
    let Some(mut sim) = gpu() else { return };
    let topo = Arc::new(Topology::build(&TilingSpec::rect("hex", 12, 10, Boundary::Wrap)).unwrap());
    let j = BatchJob {
        topo: topo.clone(),
        nb: Nb::Touch,
        worlds: vec![BatchWorld {
            rule,
            init: random_init(topo.len(), 2, 3),
        }],
        gens: 4,
        frames_at: vec![],
    };
    assert!(run_cpu(&j).is_err());
    let e = sim.run(&j).unwrap_err();
    assert!(e.contains("square grid"), "{e}");
}

#[test]
fn unsupported_rules_are_refused() {
    for r in [
        "W30",
        "Ant RL",
        "Sandpile",
        "ForestFire 0.01/0.0001",
        "Lenia R=13,T=10,b=1,m=0.15,s=0.015",
    ] {
        let rule = Rule::parse(r).unwrap().0;
        assert!(!BatchSim::supports(&rule), "{r}");
    }
}
