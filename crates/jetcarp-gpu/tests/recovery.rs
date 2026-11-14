//! A GPU that fails part way through a run (#13): the world is put back as it was before the run,
//! never with its generation and log ahead of its cells, and the CPU carries on to the same world
//! as a CPU-only run. Failures are injected after the first batch, before and after the engine
//! records the failing batch. Skipped (with a note) when no GPU adapter is available.

use jetcarp_core::*;
use jetcarp_gpu::GpuSim;

fn scene(t: TilingSpec, rule: &str, seed: SeedSpec) -> Scene {
    let mut s = Scene::new(t);
    s.rng_state = 11;
    s.cas = vec![CaSpec::new(rule).seed(seed)];
    s
}

fn cases() -> Vec<(&'static str, Scene)> {
    let sq = |b| TilingSpec::rect("square", 64, 64, b);
    vec![
        (
            "Life",
            scene(
                sq(Boundary::Wrap),
                "B3/S23",
                SeedSpec::Random { density: 0.35 },
            ),
        ),
        (
            "JACA with a blast",
            scene(
                sq(Boundary::Wrap),
                "JACA B3/S23 blast 2",
                SeedSpec::Halves { density: 0.35 },
            ),
        ),
        (
            "a sandpile losing grains",
            scene(
                sq(Boundary::Dead),
                "Sandpile",
                SeedSpec::Pile { grains: 20_000 },
            ),
        ),
        (
            "hodgepodge on a Spectre patch",
            scene(
                TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
                "Hodgepodge 30/2/3/5",
                SeedSpec::RandomStates,
            ),
        ),
    ]
}

/// What has to agree between two worlds.
fn state(w: &World) -> (u64, String, Status, Vec<Command>, Vec<u64>) {
    (
        w.generation,
        w.hash_hex(),
        w.status(),
        w.log().to_vec(),
        w.fields.iter().map(|f| f.lost).collect(),
    )
}

#[test]
fn a_run_that_fails_part_way_leaves_the_world_as_it_was() {
    let gens = 200;
    for (what, sc) in cases() {
        for (nth, late) in [(2, false), (2, true), (1, true), (3, false)] {
            let mut sim = match GpuSim::headless() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("note: skipping the GPU recovery tests: {e}");
                    return;
                }
            };
            let what = format!("{what}, batch {nth} failing (late: {late})");
            let mut gw = World::from_scene(&sc).unwrap();
            let start = state(&gw);
            sim.fail_batch(nth, late);
            let r = sim.run(&mut gw, gens, None);
            assert!(r.is_err(), "{what}: {r:?}");
            assert_eq!(state(&gw), start, "{what}: the world after the failed run");
            // The stepper is done with; the CPU carries on to the CPU's own result.
            assert!(sim.run(&mut gw, 1, None).is_err(), "{what}: broken");
            let m = sim.step_mixed(&mut gw, gens).unwrap();
            assert_eq!(m.cpu_gens, gens, "{what}");
            let mut cpu = World::from_scene(&sc).unwrap();
            cpu.apply(Command::Step { gens }).unwrap();
            assert_eq!(state(&gw), state(&cpu), "{what}: after carrying on");
            assert_eq!(gw.script_state, cpu.script_state, "{what}");
        }
    }
}

/// The same stepper runs again once told to (the tests' own hook), and a world stepped part way
/// on the GPU and part way on the CPU still ends as the CPU's.
#[test]
fn gpu_and_cpu_generations_mix_around_a_failure() {
    let Ok(mut sim) = GpuSim::headless() else {
        eprintln!("note: skipping the GPU recovery tests: no GPU");
        return;
    };
    for (what, sc) in cases() {
        let mut gw = World::from_scene(&sc).unwrap();
        // 30 generations on the GPU, a failing run, then 70 more on the GPU.
        sim.fail_batch(0, false);
        let m = sim.step_mixed(&mut gw, 30).unwrap();
        assert_eq!(m.gpu_gens, 30, "{what}: {m:?}");
        // The stepper has grown its batches by now: the run is one batch, failing once recorded.
        sim.fail_batch(1, true);
        assert!(sim.run(&mut gw, 100, None).is_err(), "{what}");
        assert_eq!(gw.generation, 30, "{what}");
        sim.fail_batch(0, false);
        let m = sim.step_mixed(&mut gw, 70).unwrap();
        assert_eq!(m.gpu_gens, 70, "{what}: {m:?}");
        let mut cpu = World::from_scene(&sc).unwrap();
        cpu.apply(Command::Step { gens: 100 }).unwrap();
        assert_eq!(state(&gw), state(&cpu), "{what}");
    }
}
