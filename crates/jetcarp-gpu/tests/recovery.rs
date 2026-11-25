//! A GPU that fails part way through a run (#13): the world is put back as it was before the run,
//! never with its generation and log ahead of its cells, and the CPU carries on to the same world
//! as a CPU-only run. Failures are injected after the first batch, before and after the engine
//! records the failing batch. Skipped (with a note) when no GPU adapter is available, failed under
//! `JETCARP_REQUIRE_GPU=1`.

use jetcarp_core::*;
use jetcarp_gpu::GpuSim;
use std::sync::OnceLock;

/// A new stepper on the tests' one device, or None after [`jetcarp_gpu::no_gpu`].
///
/// Both tests and every case share one device, opened once (#52, #370). Each stepper came with a
/// device of its own before: one test opened and dropped sixteen, one after another, while the
/// other's ran, and on the Windows laptop's NVIDIA and Intel GPUs this program crashed
/// (0xC0000005) or hung now and then with both tests running, never with one at a time (#52). The
/// failures here are the tests' own (a batch made to fail), not the device's, so one device
/// serves every case, and each case still gets a stepper of its own. A stepper's readbacks wait
/// for their own results, so steppers can share a device.
fn stepper() -> Option<GpuSim> {
    static DEVICE: OnceLock<Option<(wgpu::Device, wgpu::Queue, String)>> = OnceLock::new();
    let (device, queue, name) = DEVICE
        .get_or_init(
            || match jetcarp_gpu::select::compute_device("jetcarp-gpu recovery tests") {
                Ok(d) => Some(d),
                Err(e) => {
                    jetcarp_gpu::no_gpu("the GPU recovery tests", &e);
                    None
                }
            },
        )
        .as_ref()?;
    Some(GpuSim::new(device.clone(), queue.clone(), name.clone()))
}

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
            let Some(mut sim) = stepper() else { return };
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
    let Some(mut sim) = stepper() else { return };
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
