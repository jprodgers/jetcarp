//! CPU vs GPU for the families the world stepper gained in 0.4.2. Every generation must give
//! the same states, ages, grain counts, generation, step counters and hash (and, for JACA, the
//! same annihilation glow and counts) on lattices and patches, with every edge, in chunks of
//! several sizes, through scripts, reseeds and period detection. Skipped (with a note) when no
//! GPU adapter is available.

use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};
use jetcarp_core::rules::Rule;
use jetcarp_core::script::{CaRef, Script, ScriptStep, Trigger};
use jetcarp_core::*;
use jetcarp_gpu::GpuSim;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn gpu() -> Option<MutexGuard<'static, GpuSim>> {
    static SIM: OnceLock<Option<Mutex<GpuSim>>> = OnceLock::new();
    let sim = SIM.get_or_init(|| match GpuSim::headless() {
        Ok(s) => {
            eprintln!("GPU tests on {}", s.name());
            Some(Mutex::new(s))
        }
        Err(e) => {
            eprintln!("note: skipping GPU family tests: {e}");
            None
        }
    });
    sim.as_ref()
        .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

fn scene(tiling: TilingSpec, cas: Vec<CaSpec>) -> Scene {
    let mut s = Scene::new(tiling);
    s.rng_state = 7;
    s.cas = cas;
    s
}

fn ca(rule: &str, nb: Option<Nb>, seed: SeedSpec) -> CaSpec {
    let mut c = CaSpec::new(rule).seed(seed);
    if nb.is_some() {
        c.neighborhood = nb;
    }
    c
}

/// Everything the two worlds must agree on, beyond the hash: JACA's glow and counts per CA.
fn same(cpu: &World, gw: &World, what: &str) {
    assert_eq!(cpu.generation, gw.generation, "{what}: generation");
    assert_eq!(
        cpu.status(),
        gw.status(),
        "{what}: status at {}",
        cpu.generation
    );
    assert_eq!(
        cpu.hash_hex(),
        gw.hash_hex(),
        "{what}: hash at generation {}",
        cpu.generation
    );
    for (a, b) in cpu.cas.iter().zip(&gw.cas) {
        assert_eq!(a.ticks, b.ticks, "{what}: ticks of CA {}", a.id);
        let (ha, hb) = (
            a.heat.as_deref().cloned().unwrap_or_default(),
            b.heat.as_deref().cloned().unwrap_or_default(),
        );
        assert_eq!(
            ha.last, hb.last,
            "{what}: CA {} last stats at {}",
            a.id, cpu.generation
        );
        assert_eq!(
            (ha.total_contacts, ha.total_destroyed),
            (hb.total_contacts, hb.total_destroyed),
            "{what}: CA {} totals at {}",
            a.id,
            cpu.generation
        );
        assert!(
            ha.glow == hb.glow,
            "{what}: CA {} glow at {}",
            a.id,
            cpu.generation
        );
    }
    for (a, b) in cpu.fields.iter().zip(&gw.fields) {
        assert_eq!(a.lost, b.lost, "{what}: grains lost");
    }
}

/// Run CPU and GPU copies side by side in the given chunks; compare after each. Returns the
/// generations run on each.
fn compare_worlds(
    sim: &mut GpuSim,
    cpu: &mut World,
    gw: &mut World,
    chunks: &[u64],
    what: &str,
) -> (u64, u64) {
    let (mut on_gpu, mut on_cpu) = (0, 0);
    for &c in chunks {
        cpu.apply(Command::Step { gens: c }).unwrap();
        let m = sim.step_mixed(gw, c).unwrap();
        assert_eq!(m.gpu_gens + m.cpu_gens, c);
        on_gpu += m.gpu_gens;
        on_cpu += m.cpu_gens;
        same(cpu, gw, what);
    }
    assert_eq!(cpu.log(), gw.log(), "{what}: command log");
    assert_eq!(cpu.script_state, gw.script_state, "{what}: script state");
    (on_gpu, on_cpu)
}

fn compare(sim: &mut GpuSim, sc: &Scene, chunks: &[u64], what: &str) -> (u64, u64) {
    let mut cpu = World::from_scene(sc).unwrap_or_else(|e| panic!("{what}: {e}"));
    let mut gw = World::from_scene(sc).unwrap();
    compare_worlds(sim, &mut cpu, &mut gw, chunks, what)
}

/// Like `compare`, and every generation must have run on the GPU.
fn compare_gpu(sim: &mut GpuSim, sc: &Scene, chunks: &[u64], what: &str) {
    let w = World::from_scene(sc).unwrap_or_else(|e| panic!("{what}: {e}"));
    if let Err(e) = sim.check(&w) {
        panic!("{what}: {e}");
    }
    let (_, on_cpu) = compare(sim, sc, chunks, what);
    assert_eq!(on_cpu, 0, "{what}: fell back to the CPU");
}

const CHUNKS: &[u64] = &[1, 2, 13, 60, 180];

const LATTICES: &[(&str, u32, u32)] = &[
    ("square", 40, 30),
    ("hex", 30, 24),
    ("triangle", 20, 16),
    ("cairo", 12, 10),
    ("snubhex", 8, 8),
];

const BOUNDARIES: [Boundary; 3] = [Boundary::Dead, Boundary::Wrap, Boundary::Live];

fn script(steps: Vec<(Vec<u32>, Trigger, Option<u64>)>, repeat: bool) -> Script {
    Script {
        steps: steps
            .into_iter()
            .map(|(run, until, max_gens)| ScriptStep {
                name: String::new(),
                run: run.into_iter().map(CaRef::Id).collect(),
                until,
                max_gens,
            })
            .collect(),
        repeat,
    }
}

// ---------------------------------------------------------------------------------------------
// JACA

const JACA: &[&str] = &[
    "JACA B3/S23",
    "JACA B3/S23 blast 1",
    "JACA B3/S23 blast 3",
    "JACA +B3/S23 -B36/S23",
    "JACA +B36/S125 -B3/S23 blast 2",
    "JACA B2/S blast 1",
    "JACA B3/S23V",
];

#[test]
fn jaca_on_lattices() {
    let Some(mut sim) = gpu() else { return };
    let mut runs = 0;
    for &(fam, w, h) in LATTICES {
        for b in BOUNDARIES {
            for &rule in JACA {
                for seed in [SeedSpec::RandomStates, SeedSpec::Halves { density: 0.35 }] {
                    let sc = scene(
                        TilingSpec::rect(fam, w, h, b),
                        vec![ca(rule, None, seed.clone())],
                    );
                    compare_gpu(
                        &mut sim,
                        &sc,
                        CHUNKS,
                        &format!("{rule} on {fam} {b:?} {seed:?}"),
                    );
                    runs += 1;
                }
            }
        }
    }
    eprintln!("{runs} JACA runs matched");
}

#[test]
fn jaca_on_patches() {
    let Some(mut sim) = gpu() else { return };
    for (fam, level) in [("spectre", 3), ("hat", 3), ("penrose-rhomb", 4)] {
        for &rule in JACA {
            for nb in [Nb::Edge, Nb::Touch] {
                let t = TilingSpec::new(fam).with_size(SizeSpec::Level(level));
                let sc = scene(t, vec![ca(rule, Some(nb), SeedSpec::RandomStates)]);
                compare_gpu(
                    &mut sim,
                    &sc,
                    &[1, 5, 40, 150],
                    &format!("{rule} on {fam} {nb:?}"),
                );
            }
        }
    }
}

/// Blast radii 0, 1, 3 and 64; at 64 the zone covers a small world from one contact.
#[test]
fn jaca_blast_radii() {
    let Some(mut sim) = gpu() else { return };
    for blast in [0, 1, 3, 64] {
        for b in BOUNDARIES {
            let rule = format!("JACA B3/S23 blast {blast}");
            let sc = scene(
                TilingSpec::rect("hex", 48, 40, b),
                vec![ca(&rule, None, SeedSpec::Halves { density: 0.3 })],
            );
            compare_gpu(&mut sim, &sc, &[1, 1, 7, 64, 200], &format!("{rule} {b:?}"));
        }
        let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
        let rule = format!("JACA B2/S34 blast {blast}");
        let sc = scene(t, vec![ca(&rule, Some(Nb::Touch), SeedSpec::RandomStates)]);
        compare_gpu(&mut sim, &sc, &[1, 9, 90], &format!("{rule} on spectre"));
    }
}

/// The glow and counts after a known collision, against the CPU's, and generations in chunks of
/// one so every step's counts are checked.
#[test]
fn jaca_glow_and_counts() {
    let Some(mut sim) = gpu() else { return };
    for blast in [0, 2] {
        let rule = format!("JACA B3/S23 blast {blast}");
        let sc = scene(
            TilingSpec::rect("square", 64, 48, Boundary::Wrap),
            vec![ca(&rule, None, SeedSpec::Halves { density: 0.4 })],
        );
        compare_gpu(
            &mut sim,
            &sc,
            &[1; 40],
            &format!("{rule}, one generation at a time"),
        );
        let mut gw = World::from_scene(&sc).unwrap();
        sim.step_mixed(&mut gw, 30).unwrap();
        let h = gw.cas[0].heat.as_ref().expect("heat after GPU steps");
        assert_eq!(h.glow.len(), gw.topo.len());
        assert!(h.total_destroyed > 0, "{rule}: the halves met");
        assert!(h.glow.contains(&255) || h.glow.iter().any(|&g| g > 0));
    }
}

/// Several JACA and other CAs, steps per generation, a script switching between them (the glow
/// stays with its CA), reseeds of either sign, and period detection.
#[test]
fn jaca_scripts_reseeds_and_repeats() {
    let Some(mut sim) = gpu() else { return };
    for b in [Boundary::Wrap, Boundary::Dead] {
        let mut sc = scene(
            TilingSpec::rect("square", 40, 32, b),
            vec![
                ca(
                    "JACA B3/S23 blast 1",
                    None,
                    SeedSpec::Halves { density: 0.35 },
                ),
                ca("JACA +B36/S23 -B3/S23", None, SeedSpec::RandomStates).private(),
                ca("B3/S23", None, SeedSpec::Random { density: 0.3 }).private(),
                ca("JACA B3/S23", None, SeedSpec::Empty).steps(2),
            ],
        );
        sc.script = Some(script(
            vec![
                (vec![1], Trigger::Generations { n: 40 }, None),
                (vec![2, 3], Trigger::PopulationBelow { n: 300 }, Some(60)),
                (vec![4], Trigger::Repeats { max_period: 4 }, Some(50)),
                (vec![1, 2], Trigger::Done, Some(80)),
            ],
            true,
        ));
        sc.reseed = vec![
            AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 37 }),
            AutoReseed::new(
                ReseedMode::Random {
                    flip: 0.05,
                    sign: Default::default(),
                },
                ReseedWhen::Every { gens: 23 },
            )
            .target(2),
        ];
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 9, 64, 300, 400],
            &format!("JACA script {b:?}"),
        );
    }
    // A still life and a blinker of each sign, far apart: period 2, found at the CPU's
    // generation.
    let cells = vec![
        [100u32, 1],
        [101, 1],
        [102, 1], // blinker +
        [300, 2],
        [301, 2],
        [324, 2],
        [325, 2], // block -
    ];
    let sc = scene(
        TilingSpec::rect("square", 24, 20, Boundary::Wrap),
        vec![ca("JACA B3/S23", None, SeedSpec::Cells { cells })],
    );
    compare_gpu(&mut sim, &sc, &[1, 1, 3, 50], "JACA blinker and block");
}

/// Commands between GPU runs: a rule change resets the glow, drawing keeps it.
#[test]
fn jaca_commands_between_runs() {
    let Some(mut sim) = gpu() else { return };
    let sc = scene(
        TilingSpec::rect("hex", 32, 24, Boundary::Wrap),
        vec![ca(
            "JACA B2/S34 blast 1",
            None,
            SeedSpec::Halves { density: 0.3 },
        )],
    );
    let mut cpu = World::from_scene(&sc).unwrap();
    let mut gw = World::from_scene(&sc).unwrap();
    let cmds = [
        Command::SetCells {
            id: 1,
            cells: vec![[5, 1], [6, 2], [7, 1], [40, 0]],
        },
        Command::UpdateCa {
            id: 1,
            change: CaChange {
                rule: Some("JACA B3/S23 blast 2".into()),
                ..Default::default()
            },
        },
        Command::Reseed {
            ca: None,
            mode: ReseedMode::rain(),
        },
        Command::Reset,
    ];
    for c in cmds {
        cpu.apply(Command::Step { gens: 17 }).unwrap();
        sim.step_mixed(&mut gw, 17).unwrap();
        same(&cpu, &gw, "before a command");
        cpu.apply(c.clone()).unwrap();
        gw.apply(c).unwrap();
        same(&cpu, &gw, "after a command");
    }
    cpu.apply(Command::Step { gens: 50 }).unwrap();
    sim.step_mixed(&mut gw, 50).unwrap();
    same(&cpu, &gw, "at the end");
}

// ---------------------------------------------------------------------------------------------
// B0 Life-like rules and Margolus blocks

const B0: &[&str] = &[
    "B0/S8",
    "B013/S1",
    "B0123478/S01234678",
    "B02/S013",
    "B0/S",
    "B03/S23V",
];

#[test]
fn b0_rules() {
    let Some(mut sim) = gpu() else { return };
    for &(fam, w, h) in LATTICES {
        for b in BOUNDARIES {
            for &rule in B0 {
                let sc = scene(
                    TilingSpec::rect(fam, w, h, b),
                    vec![ca(rule, None, SeedSpec::Random { density: 0.3 })],
                );
                compare_gpu(&mut sim, &sc, CHUNKS, &format!("{rule} on {fam} {b:?}"));
            }
        }
    }
    for &rule in B0 {
        let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(3));
        let sc = scene(
            t,
            vec![ca(rule, Some(Nb::Touch), SeedSpec::Random { density: 0.3 })],
        );
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 5, 40, 150],
            &format!("{rule} on spectre"),
        );
    }
}

/// B0 runs alternate complementary rules by the parity of the step count, so a GPU run that
/// starts on an odd tick (after a CPU generation), or with two steps per generation, must keep
/// the parity.
#[test]
fn b0_from_an_odd_tick() {
    let Some(mut sim) = gpu() else { return };
    for &rule in B0 {
        for spg in [1, 2, 3] {
            let sc = scene(
                TilingSpec::rect("square", 32, 24, Boundary::Wrap),
                vec![ca(rule, None, SeedSpec::Random { density: 0.3 }).steps(spg)],
            );
            let mut cpu = World::from_scene(&sc).unwrap();
            let mut gw = World::from_scene(&sc).unwrap();
            cpu.apply(Command::Step { gens: 1 }).unwrap();
            gw.apply(Command::Step { gens: 1 }).unwrap();
            assert_eq!(gw.cas[0].ticks, spg as u64);
            let (_, on_cpu) = compare_worlds(
                &mut sim,
                &mut cpu,
                &mut gw,
                &[1, 2, 7, 50],
                &format!("{rule} x{spg} from tick {spg}"),
            );
            assert_eq!(on_cpu, 0);
        }
    }
}

#[test]
fn margolus_rules() {
    let Some(mut sim) = gpu() else { return };
    for rule in [
        "Margolus BBM",
        "Margolus Critters",
        "Margolus Tron",
        "Margolus SingleRotation",
        "Margolus HPP",
    ] {
        // Even and odd sides: blocks wrap across the seam only on an even side.
        for (w, h) in [(40, 30), (41, 30), (40, 31), (33, 27)] {
            for b in BOUNDARIES {
                let sc = scene(
                    TilingSpec::rect("square", w, h, b),
                    vec![ca(rule, None, SeedSpec::Random { density: 0.3 })],
                );
                compare_gpu(&mut sim, &sc, CHUNKS, &format!("{rule} {w}x{h} {b:?}"));
            }
        }
        // From an odd step count, and with two steps per generation.
        let sc = scene(
            TilingSpec::rect("square", 24, 24, Boundary::Wrap),
            vec![ca(rule, None, SeedSpec::Random { density: 0.3 }).steps(2)],
        );
        compare_gpu(&mut sim, &sc, &[1, 3, 40], &format!("{rule} x2"));
        let sc = scene(
            TilingSpec::rect("square", 24, 24, Boundary::Wrap),
            vec![ca(rule, None, SeedSpec::Random { density: 0.3 })],
        );
        let mut cpu = World::from_scene(&sc).unwrap();
        let mut gw = World::from_scene(&sc).unwrap();
        cpu.apply(Command::Step { gens: 1 }).unwrap();
        gw.apply(Command::Step { gens: 1 }).unwrap();
        compare_worlds(
            &mut sim,
            &mut cpu,
            &mut gw,
            &[1, 2, 7, 50],
            &format!("{rule} from tick 1"),
        );
    }
    // An empty Critters world flips every block each step: period 2, never 1, because a repeat
    // must also be in the same partition phase.
    let sc = scene(
        TilingSpec::rect("square", 16, 16, Boundary::Wrap),
        vec![ca("Margolus Critters", None, SeedSpec::Empty)],
    );
    compare_gpu(&mut sim, &sc, &[1, 1, 1, 5, 20], "empty Critters");
    let w = {
        let mut w = World::from_scene(&sc).unwrap();
        sim.step_mixed(&mut w, 20).unwrap();
        w
    };
    assert!(
        matches!(w.status(), Status::Repeats { period: 2, .. }),
        "{:?}",
        w.status()
    );
}

// ---------------------------------------------------------------------------------------------
// Larger than Life

const LTL: &[&str] = &[
    "R5,C2,S33-57,B34-45",
    "R2,C2,S5-9,B7-8",
    "R3,C2,S2,B3,N+",
    "R1,C2,S0,B1,NN",
    "R4,C2,S40-80,B41-80",
    "R2,C5,S4-8,B5-6",
    "R10,C255,S1-2,B3",
    "R3,C2,S0-4,B0-2",
];

#[test]
fn ltl_on_lattices() {
    let Some(mut sim) = gpu() else { return };
    for &(fam, w, h) in &[
        ("square", 48, 40),
        ("hex", 30, 24),
        ("triangle", 24, 20),
        ("cairo", 14, 12),
    ] {
        for b in BOUNDARIES {
            for &rule in LTL {
                let sc = scene(
                    TilingSpec::rect(fam, w, h, b),
                    vec![ca(rule, None, SeedSpec::Random { density: 0.45 })],
                );
                let what = format!("{rule} on {fam} {b:?}");
                if rule.ends_with("N+") && fam != "square" {
                    // The cross needs the square grid.
                    assert!(World::from_scene(&sc).is_err(), "{what}");
                    continue;
                }
                compare_gpu(&mut sim, &sc, &[1, 2, 13, 60], &what);
            }
        }
    }
    // A reach wider than the torus wraps onto itself.
    let sc = scene(
        TilingSpec::rect("square", 12, 9, Boundary::Wrap),
        vec![ca(
            "R10,C2,S100-200,B120-180",
            None,
            SeedSpec::Random { density: 0.5 },
        )],
    );
    compare_gpu(&mut sim, &sc, &[1, 20, 80], "R10 on a small torus");
}

#[test]
fn ltl_on_patches() {
    let Some(mut sim) = gpu() else { return };
    for rule in [
        "R2,C2,S5-9,B7-8",
        "R3,C2,S8-20,B10-14,NN",
        "R2,C4,S3-9,B4-8",
    ] {
        let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
        let sc = scene(t, vec![ca(rule, None, SeedSpec::Random { density: 0.45 })]);
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 5, 40, 150],
            &format!("{rule} on spectre"),
        );
    }
    // Two LtL CAs with different reaches, and a script between them.
    let mut sc = scene(
        TilingSpec::rect("square", 50, 40, Boundary::Wrap),
        vec![
            ca(
                "R5,C2,S33-57,B34-45",
                None,
                SeedSpec::Random { density: 0.45 },
            ),
            ca("R2,C3,S5-9,B7-8", None, SeedSpec::Random { density: 0.4 }).private(),
        ],
    );
    sc.script = Some(script(
        vec![
            (vec![1], Trigger::Generations { n: 30 }, None),
            (vec![1, 2], Trigger::Repeats { max_period: 10 }, Some(70)),
        ],
        true,
    ));
    compare_gpu(&mut sim, &sc, &[1, 9, 64, 300], "two LtL CAs in a script");
}

// ---------------------------------------------------------------------------------------------
// Forest fire

#[test]
fn forest_fire() {
    let Some(mut sim) = gpu() else { return };
    let rules = [
        "ForestFire 0.01/0.0001",
        "ForestFire 0.05/0.001",
        "ForestFire 0/0",
        "ForestFire 1/1",
        "ForestFire 1/0",
        "ForestFire 0/1",
        "ForestFire 0.3/0.000000001",
    ];
    for &(fam, w, h) in LATTICES {
        for b in BOUNDARIES {
            for rule in rules {
                let sc = scene(
                    TilingSpec::rect(fam, w, h, b),
                    vec![ca(rule, None, SeedSpec::RandomStates)],
                );
                compare_gpu(&mut sim, &sc, CHUNKS, &format!("{rule} on {fam} {b:?}"));
            }
        }
    }
    let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(3));
    let sc = scene(
        t,
        vec![ca(
            "ForestFire 0.02/0.001",
            Some(Nb::Edge),
            SeedSpec::RandomStates,
        )],
    );
    compare_gpu(&mut sim, &sc, &[1, 5, 40, 150], "forest fire on spectre");
    // Step counts past 2^32 feed the noise as 64-bit numbers.
    for ticks in [(1u64 << 32) - 3, (1 << 32) + 5, u64::MAX / 3] {
        let sc = scene(
            TilingSpec::rect("square", 40, 30, Boundary::Wrap),
            vec![ca("ForestFire 0.05/0.001", None, SeedSpec::RandomStates).steps(2)],
        );
        let mut cpu = World::from_scene(&sc).unwrap();
        let mut gw = World::from_scene(&sc).unwrap();
        cpu.cas[0].ticks = ticks;
        gw.cas[0].ticks = ticks;
        let (_, on_cpu) = compare_worlds(
            &mut sim,
            &mut cpu,
            &mut gw,
            &[1, 2, 13, 60],
            &format!("forest fire from tick {ticks}"),
        );
        assert_eq!(on_cpu, 0);
    }
}

// ---------------------------------------------------------------------------------------------
// Sandpiles

#[test]
fn sandpile_scenes() {
    let Some(mut sim) = gpu() else { return };
    for f in [
        "sandpile-hex.json",
        "sandpile-cairo.json",
        "sandpile-spectre.json",
        "sandpile-identity-cairo.json",
        "penrose-kd-sandpile.json",
    ] {
        let text = std::fs::read_to_string(format!("../jetcarp-core/data/scenes/{f}")).unwrap();
        let sc = Scene::from_json(&text).unwrap();
        compare_gpu(&mut sim, &sc, &[1, 2, 13, 60, 180], f);
    }
}

#[test]
fn sandpile_edges_drops_and_patches() {
    let Some(mut sim) = gpu() else { return };
    for &(fam, w, h) in LATTICES {
        for b in BOUNDARIES {
            for drop in [0, 1, 7] {
                for nb in [Nb::Edge, Nb::Touch] {
                    let mut c = ca("Sandpile", Some(nb), SeedSpec::Pile { grains: 20_000 });
                    c.drop_per_gen = drop;
                    let sc = scene(TilingSpec::rect(fam, w, h, b), vec![c]);
                    compare_gpu(
                        &mut sim,
                        &sc,
                        CHUNKS,
                        &format!("sandpile drop {drop} on {fam} {b:?} {nb:?}"),
                    );
                }
            }
        }
    }
    // Patches: open boundary cells take every grain that reaches them.
    for (fam, level) in [("spectre", 3), ("hat", 3), ("penrose-rhomb", 4)] {
        for nb in [Nb::Edge, Nb::Touch] {
            let mut c = ca("Sandpile", Some(nb), SeedSpec::Pile { grains: 50_000 });
            c.drop_per_gen = 3;
            let t = TilingSpec::new(fam).with_size(SizeSpec::Level(level));
            compare_gpu(
                &mut sim,
                &scene(t, vec![c]),
                &[1, 5, 40, 150],
                &format!("sandpile on {fam} {nb:?}"),
            );
        }
    }
    // Huge piles: populations and losses past 2^32 grains.
    let mut c = ca(
        "Sandpile",
        None,
        SeedSpec::Pile {
            grains: 3_000_000_000,
        },
    );
    c.drop_per_gen = 4_000_000_000;
    let sc = scene(TilingSpec::rect("square", 20, 20, Boundary::Dead), vec![c]);
    compare_gpu(&mut sim, &sc, &[1, 2, 13, 60], "a huge sandpile");
    let mut w = World::from_scene(&sc).unwrap();
    sim.step_mixed(&mut w, 76).unwrap();
    assert!(w.fields[0].lost > 1 << 32, "lost {}", w.fields[0].lost);
}

/// A pile that stabilises is caught by the period watch at the CPU's generation; population
/// triggers count grains; a `done` trigger stays on the CPU.
#[test]
fn sandpile_repeats_and_scripts() {
    let Some(mut sim) = gpu() else { return };
    for b in [Boundary::Dead, Boundary::Wrap] {
        let sc = scene(
            TilingSpec::rect("square", 30, 30, b),
            vec![ca("Sandpile", None, SeedSpec::Pile { grains: 2_000 })],
        );
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 3, 50, 400, 600],
            &format!("settling pile {b:?}"),
        );
        // Dead edges drain the pile until it is stable; on a torus no grain leaves, and the
        // pile may settle into a cycle instead.
        let mut w = World::from_scene(&sc).unwrap();
        sim.step_mixed(&mut w, 1054).unwrap();
        match (b, w.status()) {
            (Boundary::Dead, Status::Repeats { period: 1, .. }) => {}
            (Boundary::Wrap, Status::Repeats { .. }) => {}
            (_, s) => panic!("pile {b:?}: {s:?}"),
        }
    }
    let mut drip = ca("Sandpile", None, SeedSpec::Pile { grains: 500 });
    drip.drop_per_gen = 2;
    let mut sc = scene(
        TilingSpec::rect("hex", 30, 24, Boundary::Dead),
        vec![drip, ca("B3/S23", None, SeedSpec::Random { density: 0.3 })],
    );
    sc.script = Some(script(
        vec![
            (vec![1], Trigger::PopulationAbove { n: 600 }, Some(300)),
            (vec![2], Trigger::Generations { n: 20 }, None),
            (vec![1, 2], Trigger::PopulationBelow { n: 100 }, Some(50)),
        ],
        true,
    ));
    compare_gpu(&mut sim, &sc, &[1, 9, 64, 300], "sandpile script");
    sc.script = Some(script(vec![(vec![1], Trigger::Done, Some(200))], false));
    let w = World::from_scene(&sc).unwrap();
    assert!(
        sim.check(&w).is_err(),
        "a done trigger on a sandpile stays on the CPU"
    );
    compare(&mut sim, &sc, &[5, 100], "sandpile done trigger");
}

// ---------------------------------------------------------------------------------------------
// Quick Start

/// Every Quick Start entry of these families, at preview size, on the GPU like the CPU.
#[test]
fn quick_start_entries() {
    let Some(mut sim) = gpu() else { return };
    let ours = |r: &Rule| match r {
        Rule::Jaca(_) | Rule::Margolus(_) => true,
        Rule::LifeLike { birth, .. } => birth & 1 == 1,
        Rule::Ltl(_) => true,
        Rule::ForestFire { .. } => true,
        Rule::Sandpile => true,
        _ => false,
    };
    let mut counts = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for q in jetcarp_core::quickstart::entries() {
        let rules: Vec<Rule> = q
            .scene
            .cas
            .iter()
            .filter_map(|c| Rule::parse(&c.rule).ok().map(|r| r.0))
            .collect();
        if !rules.iter().any(ours) {
            continue;
        }
        let cells = jetcarp_core::quickstart::preview_cells(&q.scene, 20_000).unwrap_or(20_000);
        let sc = jetcarp_core::quickstart::preview_scene(&q.scene, cells);
        let (g, c) = compare(&mut sim, &sc, &[1, 30, 120], &q.id);
        let e = counts.entry(q.family.clone()).or_default();
        e.0 += 1;
        if c > 0 && g == 0 {
            e.1 += 1;
            eprintln!("{} ({}) ran on the CPU", q.id, q.rule);
        }
    }
    for (fam, (n, cpu)) in &counts {
        eprintln!("Quick Start {fam}: {n} entries matched, {cpu} on the CPU");
    }
    assert!(counts.values().map(|c| c.0).sum::<u32>() > 50);
}
