//! CPU vs GPU: every supported rule on every storage kind and boundary must give the same state
//! hash, generation, status, and command log after any number of generations. Skipped (with a
//! note) when no GPU adapter is available.

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
            eprintln!("note: skipping GPU equality tests: {e}");
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

/// Run CPU and GPU copies side by side in the given chunks; compare after each.
fn compare(sim: &mut GpuSim, sc: &Scene, chunks: &[u64], what: &str) -> (u64, u64) {
    let (mut on_gpu, mut on_cpu) = (0, 0);
    let mut cpu = World::from_scene(sc).unwrap_or_else(|e| panic!("{what}: {e}"));
    let mut gw = World::from_scene(sc).unwrap();
    for &c in chunks {
        cpu.apply(Command::Step { gens: c }).unwrap();
        let m = sim.step_mixed(&mut gw, c).unwrap();
        assert_eq!(m.gpu_gens + m.cpu_gens, c);
        on_gpu += m.gpu_gens;
        on_cpu += m.cpu_gens;
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
    }
    assert_eq!(cpu.log(), gw.log(), "{what}: command log");
    assert_eq!(cpu.script_state, gw.script_state, "{what}: script state");
    (on_gpu, on_cpu)
}

/// Like `compare`, and every generation must have run on the GPU.
fn compare_gpu(sim: &mut GpuSim, sc: &Scene, chunks: &[u64], what: &str) {
    let (_, on_cpu) = compare(sim, sc, chunks, what);
    assert_eq!(on_cpu, 0, "{what}: fell back to the CPU");
}

const RULES: &[&str] = &[
    "B3/S23",
    "B3/S23V",
    "B36/S23",
    "B2/S",
    "B1357/S1357",
    "B2/S/C3",
    "B2/S13/C4V",
    "B3/S23/C6",
    "WireWorld",
    "Cyclic 8/1",
    "Cyclic 5/2",
    "RPS 3/2",
    "GH 4/1",
    "GH 3/2",
    "Hodgepodge 30/2/3/5",
    "SpectreGlider",
];

fn ca_for(rule: &str) -> CaSpec {
    let ca = CaSpec::new(rule);
    match rule {
        "WireWorld" | "SpectreGlider" => ca.seed(SeedSpec::RandomStates),
        r if r.starts_with('B') => ca.seed(SeedSpec::Random { density: 0.35 }),
        _ => ca,
    }
}

const TILINGS: &[(&str, u32, u32)] = &[
    ("square", 40, 30),
    ("hex", 30, 24),
    ("brick", 24, 20),
    ("triangle", 20, 16),
    ("cairo", 12, 10),
    ("oct", 14, 12),
    ("trihex", 12, 10),
    ("rhombitrihex", 10, 8),
    ("snubhex", 8, 8),
    ("lizard", 10, 9),
];

#[test]
fn every_rule_tiling_and_boundary() {
    let Some(mut sim) = gpu() else { return };
    let mut runs = 0;
    for &(fam, w, h) in TILINGS {
        for b in [Boundary::Dead, Boundary::Wrap, Boundary::Live] {
            for &rule in RULES {
                let sc = scene(TilingSpec::rect(fam, w, h, b), vec![ca_for(rule)]);
                compare_gpu(
                    &mut sim,
                    &sc,
                    &[1, 2, 13, 60, 180],
                    &format!("{rule} on {fam} {b:?}"),
                );
                runs += 1;
            }
        }
    }
    eprintln!("{runs} rule x tiling x boundary runs matched");
}

#[test]
fn explicit_storage_spectre() {
    let Some(mut sim) = gpu() else { return };
    for &rule in RULES {
        let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(3));
        let sc = scene(t, vec![ca_for(rule)]);
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 5, 40, 150],
            &format!("{rule} on spectre"),
        );
    }
    // The prototype's glider scene.
    let sc = Scene::from_json(include_str!(
        "../../jetcarp-core/data/scenes/spectre-glider.json"
    ))
    .unwrap();
    compare_gpu(&mut sim, &sc, &[3, 100, 300], "spectre-glider.json");
}

// The exact level-7 Spectre patch (2.1 million tiles, 25.5 million neighbour entries) runs on
// the GPU like the CPU: the glider rule from a pair near the centre, and a soup of Life.
#[test]
fn explicit_storage_spectre_level_7() {
    let Some(mut sim) = gpu() else { return };
    let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(7));
    let topo = Topology::build(&t).unwrap();
    let c = topo.center_cell();
    let d = topo.slot(c, Nb::Edge, 0).unwrap();
    let mut glider = CaSpec::new("SpectreGlider");
    glider.neighborhood = Some(Nb::Touch);
    let glider = glider.seed(SeedSpec::Cells {
        cells: vec![[c as u32, 1], [d as u32, 3]],
    });
    let sc = scene(t.clone(), vec![glider]);
    if let Err(e) = sim.check(&World::from_scene(&sc).unwrap()) {
        eprintln!("note: level 7 does not fit this GPU: {e}");
        return;
    }
    compare_gpu(
        &mut sim,
        &sc,
        &[1, 50, 200],
        "SpectreGlider on spectre level 7",
    );
    let sc = scene(t, vec![ca_for("B3/S23")]);
    compare_gpu(&mut sim, &sc, &[1, 20], "Life soup on spectre level 7");
}

#[test]
fn several_cas_and_steps_per_generation() {
    let Some(mut sim) = gpu() else { return };
    for b in [Boundary::Dead, Boundary::Wrap, Boundary::Live] {
        let sc = scene(
            TilingSpec::rect("cairo", 14, 12, b),
            vec![
                CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }),
                CaSpec::new("B36/S23").steps(2),
                CaSpec::new("B2/S/C3")
                    .seed(SeedSpec::Random { density: 0.2 })
                    .steps(3),
                CaSpec::new("Cyclic 6/1"),
                CaSpec::new("B3/S23")
                    .private()
                    .seed(SeedSpec::Random { density: 0.4 }),
            ],
        );
        compare_gpu(&mut sim, &sc, &[1, 4, 33, 120], &format!("mixed CAs {b:?}"));
    }
}

#[test]
fn commands_between_gpu_runs() {
    let Some(mut sim) = gpu() else { return };
    let sc = scene(
        TilingSpec::rect("hex", 32, 24, Boundary::Wrap),
        vec![CaSpec::new("B2/S34").seed(SeedSpec::Random { density: 0.3 })],
    );
    let mut cpu = World::from_scene(&sc).unwrap();
    let mut gw = World::from_scene(&sc).unwrap();
    let cmds = [
        Command::SetCells {
            id: 1,
            cells: vec![[5, 1], [6, 1], [7, 1], [40, 0]],
        },
        Command::UpdateCa {
            id: 1,
            change: CaChange {
                rule: Some("B2/S/C4".into()),
                ..Default::default()
            },
        },
        Command::AddCa {
            ca: CaSpec::new("GH 3/1"),
        },
        Command::UpdateCa {
            id: 1,
            change: CaChange {
                enabled: Some(false),
                ..Default::default()
            },
        },
        Command::Seed {
            id: 2,
            seed: SeedSpec::RandomStates,
        },
        Command::Reset,
    ];
    for c in cmds {
        cpu.apply(Command::Step { gens: 17 }).unwrap();
        sim.step_mixed(&mut gw, 17).unwrap();
        assert_eq!(cpu.hash_hex(), gw.hash_hex());
        cpu.apply(c.clone()).unwrap();
        gw.apply(c).unwrap();
        assert_eq!(cpu.hash_hex(), gw.hash_hex());
    }
    cpu.apply(Command::Step { gens: 50 }).unwrap();
    sim.step_mixed(&mut gw, 50).unwrap();
    assert_eq!(cpu.hash_hex(), gw.hash_hex());
    assert_eq!(cpu.to_scene().to_json(), gw.to_scene().to_json());
}

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

#[test]
fn scripts_and_period_detection() {
    let Some(mut sim) = gpu() else { return };
    let cas = || {
        vec![
            CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 }),
            CaSpec::new("B2/S/C3").seed(SeedSpec::Random { density: 0.1 }),
            CaSpec::new("Cyclic 8/2"),
            CaSpec::new("B3/S23")
                .private()
                .seed(SeedSpec::Random { density: 0.5 }),
        ]
    };
    let triggers = [
        Trigger::Generations { n: 23 },
        Trigger::PopulationBelow { n: 150 },
        Trigger::PopulationAbove { n: 400 },
        Trigger::ActivityBelow {
            fraction: 0.01,
            for_gens: 5,
        },
        Trigger::ActivityAbove {
            fraction: 0.05,
            for_gens: 3,
        },
        Trigger::Repeats { max_period: 2 },
        Trigger::Repeats { max_period: 100 },
        Trigger::Done,
        Trigger::Never,
    ];
    for (k, t) in triggers.iter().enumerate() {
        for b in [Boundary::Wrap, Boundary::Dead] {
            let mut sc = scene(TilingSpec::rect("square", 24, 20, b), cas());
            sc.script = Some(script(
                vec![
                    (vec![1], t.clone(), Some(90)),
                    (vec![2, 4], t.clone(), Some(40)),
                    (vec![3], Trigger::Generations { n: 31 }, None),
                    (vec![1, 3], t.clone(), Some(70)),
                ],
                k % 2 == 0,
            ));
            compare_gpu(
                &mut sim,
                &sc,
                &[1, 9, 64, 300, 400],
                &format!("script {t:?} {b:?}"),
            );
        }
    }
    // A script that switches to CPU-only CAs (rule 30, ants) and back.
    let sc = Scene::from_json(include_str!(
        "../../jetcarp-core/data/scenes/script-five.json"
    ))
    .unwrap();
    let (on_gpu, on_cpu) = compare(&mut sim, &sc, &[10, 200, 600], "script-five.json");
    eprintln!("script-five.json: {on_gpu} generations on the GPU, {on_cpu} on the CPU");
    assert!(on_gpu > 0 && on_cpu > 0);
    // Period detection on a small torus: a blinker and a block settle immediately.
    let mut cells = vec![];
    for c in [100u32, 101, 102, 300, 301, 324, 325] {
        cells.push([c, 1]);
    }
    let sc = scene(
        TilingSpec::rect("square", 24, 20, Boundary::Wrap),
        vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells })],
    );
    compare_gpu(&mut sim, &sc, &[1, 1, 3, 50], "blinker and block");
}

/// `done` triggers: a field that dies out, one that freezes, and the CPU's answers at the same
/// generations.
#[test]
fn done_trigger() {
    let Some(mut sim) = gpu() else { return };
    for b in [Boundary::Wrap, Boundary::Dead] {
        let mut sc = scene(
            TilingSpec::rect("square", 30, 24, b),
            vec![
                // Dies after one generation.
                CaSpec::new("B3/S23").private().seed(SeedSpec::Cells {
                    cells: vec![[40, 1]],
                }),
                CaSpec::new("Cyclic 8/2"),
                // A sparse soup that settles into still lifes and blinkers, or freezes.
                CaSpec::new("B3/S23")
                    .private()
                    .seed(SeedSpec::Random { density: 0.12 }),
                CaSpec::new("B36/S23")
                    .private()
                    .seed(SeedSpec::Random { density: 0.3 }),
            ],
        );
        sc.script = Some(script(
            vec![
                (vec![1], Trigger::Done, Some(50)),
                (vec![2], Trigger::Generations { n: 7 }, None),
                (vec![3], Trigger::Done, Some(400)),
                (vec![4], Trigger::Done, Some(300)),
            ],
            false,
        ));
        let mut cpu = World::from_scene(&sc).unwrap();
        cpu.apply(Command::Step { gens: 1 }).unwrap();
        assert_eq!(
            cpu.script_state.step, 1,
            "the single cell died at generation 1"
        );
        compare_gpu(
            &mut sim,
            &sc,
            &[1, 1, 5, 100, 400, 300],
            &format!("done {b:?}"),
        );
    }
}

#[test]
fn ages_saturate_like_the_cpu() {
    let Some(mut sim) = gpu() else { return };
    let cells = vec![[27u32, 1], [28, 1], [37, 1], [38, 1]];
    let sc = scene(
        TilingSpec::rect("square", 10, 10, Boundary::Dead),
        vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells })],
    );
    let mut cpu = World::from_scene(&sc).unwrap();
    let mut gw = World::from_scene(&sc).unwrap();
    cpu.apply(Command::Step { gens: 65_010 }).unwrap();
    let m = sim.step_mixed(&mut gw, 65_010).unwrap();
    assert_eq!(m.cpu_gens, 0);
    assert_eq!(gw.fields[0].age[27], 65000);
    assert_eq!(cpu.hash_hex(), gw.hash_hex());
}

#[test]
fn unsupported_rules_fall_back() {
    let Some(mut sim) = gpu() else { return };
    // B0 Life-like rules alternate complementary rules on the CPU; they must not run on the GPU.
    for rule in ["B0/S8", "B013/S1"] {
        let sc = scene(
            TilingSpec::rect("square", 20, 16, Boundary::Wrap),
            vec![CaSpec::new(rule).seed(SeedSpec::Random { density: 0.3 })],
        );
        let w = World::from_scene(&sc).unwrap();
        assert!(sim.check(&w).is_err(), "{rule} should not run on the GPU");
        let (on_gpu, _) = compare(&mut sim, &sc, &[3, 10], rule);
        assert_eq!(on_gpu, 0);
    }
    for f in [
        "langtons-ant.json",
        "sandpile-hex.json",
        "rule30-life-ants.json",
        "hex-wolfram.json",
    ] {
        let text = std::fs::read_to_string(format!("../jetcarp-core/data/scenes/{f}")).unwrap();
        let sc = Scene::from_json(&text).unwrap();
        let w = World::from_scene(&sc).unwrap();
        assert!(sim.check(&w).is_err(), "{f} should not run on the GPU");
        let (on_gpu, _) = compare(&mut sim, &sc, &[5, 20], f);
        assert_eq!(on_gpu, 0);
    }
    // Phase 3 families run on the CPU: Margolus blocks, Lenia, three-parent hexagon rules.
    for (fam, rule, seed) in [
        (
            "square",
            "Margolus Critters",
            SeedSpec::Random { density: 0.3 },
        ),
        (
            "square",
            "Lenia R=5,T=10,b=1,m=0.15,s=0.017",
            SeedSpec::Random { density: 0.5 },
        ),
        ("hex_flat", "HexT3 sum 588", SeedSpec::TopCenter),
    ] {
        let sc = scene(
            TilingSpec::rect(fam, 24, 20, Boundary::Wrap),
            vec![CaSpec::new(rule).seed(seed)],
        );
        let w = World::from_scene(&sc).unwrap();
        assert!(sim.check(&w).is_err(), "{rule} should not run on the GPU");
        let (on_gpu, _) = compare(&mut sim, &sc, &[3, 10], rule);
        assert_eq!(on_gpu, 0);
    }
}

// Priority tables (`jetcarp search evolve`'s rule family): Edmond's glider table, tables with `*` and
// `always` rows, rows of three and five conditions (packed over several words), and the most
// states, on lattices with every boundary and on Spectre patches. A table too long for the
// parameter slot falls back to the CPU.
#[test]
fn priority_rules() {
    let Some(mut sim) = gpu() else { return };
    let rules = [
        "Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1",
        "Priority 3: 0|n1>=2->1; 1|n1>=4->2; 1|n0>=2&n2>=1->1; 2|always->0; *|n2>=3->2",
        "Priority 5: 0|n1>=1&n2>=1&n0>=2->1; 1|n4>=1->4; 1|always->2; 2|n3>=1&n1>=1&n0>=1&n2>=1&n4>=0->3; 3|always->4; 4|n0>=4->0; 4|always->4",
        "Priority 8: 0|n7>=1->1; 0|n1>=1&n0>=5->2; *|n2>=2->3; 3|always->4; 4|always->5; 5|always->6; 6|always->7; 7|n0>=6->0; 7|always->7; 1|n0>=1->2; 2|always->0",
        "Priority 2: 0|n1>=2&n1>=3->1; 1|n1>=2->1; 1|n1>=3->1",
    ];
    for r in rules {
        for (fam, b) in [
            ("square", Boundary::Wrap),
            ("hex", Boundary::Dead),
            ("cairo", Boundary::Live),
            ("snubhex", Boundary::Wrap),
        ] {
            for nb in [Nb::Touch, Nb::Edge] {
                let mut ca = CaSpec::new(r).seed(SeedSpec::RandomStates);
                ca.neighborhood = Some(nb);
                let sc = scene(TilingSpec::rect(fam, 24, 20, b), vec![ca]);
                compare_gpu(
                    &mut sim,
                    &sc,
                    &[1, 3, 40, 100],
                    &format!("{r} on {fam} {b:?} {nb:?}"),
                );
            }
        }
        let mut ca = CaSpec::new(r).seed(SeedSpec::RandomStates);
        ca.neighborhood = Some(Nb::Touch);
        let sc = scene(
            TilingSpec::new("spectre").with_size(SizeSpec::Level(4)),
            vec![ca],
        );
        compare_gpu(&mut sim, &sc, &[1, 7, 60], &format!("{r} on spectre"));
    }
    // Edmond's glider from a pair at the centre, as a priority table.
    let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(5));
    let topo = Topology::build(&t).unwrap();
    let c = topo.center_cell();
    let d = topo.slot(c, Nb::Edge, 0).unwrap();
    let mut glider = CaSpec::new(rules[0]);
    glider.neighborhood = Some(Nb::Touch);
    let glider = glider.seed(SeedSpec::Cells {
        cells: vec![[c as u32, 1], [d as u32, 3]],
    });
    compare_gpu(
        &mut sim,
        &scene(t, vec![glider]),
        &[1, 100, 200],
        "priority glider",
    );
    // 45 rows do not fit the 44 words of the parameter slot.
    let long = format!(
        "Priority 3: {}",
        (0..45)
            .map(|k| format!("0|n1>={}->1", k % 9 + 1))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let sc = scene(
        TilingSpec::rect("square", 20, 16, Boundary::Wrap),
        vec![CaSpec::new(&long).seed(SeedSpec::RandomStates)],
    );
    assert!(sim.check(&World::from_scene(&sc).unwrap()).is_err());
    let (on_gpu, _) = compare(&mut sim, &sc, &[3, 10], "long priority table");
    assert_eq!(on_gpu, 0);
}

/// Reseed policies (settled and every N, all modes, with and without a script) give the CPU's
/// hashes, reseeds, and activity at every chunk, all on the GPU.
#[test]
fn reseed_policies_match_cpu() {
    use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};
    let Some(mut sim) = gpu() else { return };
    let settled = |mode: ReseedMode| {
        AutoReseed::new(
            mode,
            ReseedWhen::Settled {
                activity: 0.01,
                for_gens: 20,
                cycle: Some(12),
                period: Some(6),
                latest: Some(90),
            },
        )
        .cooldown(30)
    };
    let cases: Vec<(TilingSpec, &str, Vec<AutoReseed>)> = vec![
        (
            TilingSpec::rect("square", 64, 48, Boundary::Wrap),
            "B3/S23",
            vec![
                settled(ReseedMode::blobs()),
                AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 37 }),
            ],
        ),
        (
            TilingSpec::rect("hex", 40, 40, Boundary::Dead),
            "B2/S/C3",
            vec![AutoReseed::new(
                ReseedMode::Random {
                    flip: 0.05,
                    sign: Default::default(),
                },
                ReseedWhen::Every { gens: 50 },
            )],
        ),
        (
            TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
            "B3/S23",
            vec![
                settled(ReseedMode::rain()),
                AutoReseed::new(
                    ReseedMode::Random {
                        flip: 0.02,
                        sign: Default::default(),
                    },
                    ReseedWhen::Every { gens: 13 },
                )
                .cooldown(20),
            ],
        ),
        (
            TilingSpec::rect("square", 50, 50, Boundary::Wrap),
            "B3/S23",
            vec![AutoReseed::new(
                ReseedMode::Rain {
                    drops: 2,
                    min_size: 1.0,
                    max_size: 4.0,
                    density: 0.6,
                    sign: Default::default(),
                },
                ReseedWhen::Every { gens: 1 },
            )],
        ),
    ];
    for (t, rule, policies) in cases {
        let what = format!("{rule} on {} with {} policies", t.family, policies.len());
        let mut sc = scene(
            t,
            vec![CaSpec::new(rule).seed(SeedSpec::Random { density: 0.3 })],
        );
        sc.reseed = policies;
        compare_gpu(&mut sim, &sc, &[1, 50, 333, 1000, 1500], &what);
        let mut w = World::from_scene(&sc).unwrap();
        sim.step_mixed(&mut w, 2884).unwrap();
        let fired: u64 = w.reseed_status().iter().map(|s| s.fired).sum();
        assert!(fired > 0, "{what}: never fired");
        let measures = w
            .reseed_policies()
            .iter()
            .any(|p| matches!(p.when, ReseedWhen::Settled { .. }));
        assert_eq!(
            !w.activity_history().is_empty(),
            measures,
            "{what}: activity history on the GPU"
        );
    }
    // A script trigger and a settled policy at once, plus a manual reseed between runs.
    let mut sc = scene(
        TilingSpec::rect("square", 48, 40, Boundary::Wrap),
        vec![
            CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 }),
            CaSpec::new("B36/S23")
                .seed(SeedSpec::Random { density: 0.2 })
                .private(),
        ],
    );
    sc.script = Some(script(
        vec![
            (
                vec![1],
                Trigger::ActivityBelow {
                    fraction: 0.02,
                    for_gens: 5,
                },
                Some(300),
            ),
            (vec![1, 2], Trigger::PopulationBelow { n: 200 }, Some(200)),
        ],
        true,
    ));
    sc.reseed = vec![settled(ReseedMode::blobs()).target(1)];
    sc.commands = vec![
        Command::Step { gens: 100 },
        Command::Reseed {
            ca: None,
            mode: ReseedMode::Random {
                flip: 0.1,
                sign: Default::default(),
            },
        },
    ];
    compare_gpu(&mut sim, &sc, &[7, 100, 900, 1200], "script and policy");
}
