//! When the automatic policy steps a world on the GPU (no GPU needed).

use jetcarp_core::*;
use jetcarp_gpu::{
    GPU_PACED_CELLS, GPU_UNPACED_READS, GPU_UNPACED_RUN_READS, GpuSim, neighbour_reads,
};

fn world(w: u32, h: u32, cas: Vec<CaSpec>) -> World {
    let mut sc = Scene::new(TilingSpec::rect("square", w, h, Boundary::Wrap));
    sc.cas = cas;
    World::from_scene(&sc).unwrap()
}

fn life() -> CaSpec {
    CaSpec::new("B3/S23").seed(SeedSpec::Empty)
}

#[test]
fn reads_are_cells_times_neighbours_over_every_pass() {
    assert_eq!(neighbour_reads(&world(10, 10, vec![life()])), 800);
    assert_eq!(
        neighbour_reads(&world(10, 10, vec![life().nb(Nb::Edge)])),
        400
    );
    let mut twice = life();
    twice.steps_per_gen = 2;
    assert_eq!(
        neighbour_reads(&world(10, 10, vec![twice, life().nb(Nb::Edge)])),
        2 * 800 + 400
    );
    let mut off = world(10, 10, vec![life()]);
    off.cas[0].enabled = false;
    assert_eq!(neighbour_reads(&off), 0);
}

#[test]
fn paced_displays_go_by_cells_and_runs_by_reads() {
    assert_eq!(GPU_PACED_CELLS, 16_000);
    assert_eq!(GPU_UNPACED_READS, 1_000_000);
    let small = world(100, 100, vec![life()]);
    assert!(!GpuSim::worth(&small, true));
    assert!(!GpuSim::worth(&small, false));
    // 16,384 cells and 131,072 reads.
    let mid = world(128, 128, vec![life()]);
    assert!(GpuSim::worth(&mid, true));
    assert!(!GpuSim::worth(&mid, false));
    // 1,000,000 reads.
    let big = world(500, 250, vec![life()]);
    assert!(GpuSim::worth(&big, false));
}

#[test]
fn rules_the_gpu_cannot_run_never_go_there() {
    let ant = world(
        1000,
        1000,
        vec![CaSpec::new("Ant RL").seed(SeedSpec::Empty)],
    );
    assert!(!GpuSim::worth(&ant, true));
    assert!(!GpuSim::worth(&ant, false));
}

/// A long run of a mid-sized world goes to the GPU on its total work; a short run of the same
/// world, or a long run of a tiny one, stays on the processor.
#[test]
fn runs_of_known_length_go_by_their_total_work() {
    assert_eq!(GPU_UNPACED_RUN_READS, 100_000_000);
    // 400 x 300 square Life: 960,000 reads a generation, under the per-generation threshold.
    let mid = world(400, 300, vec![life()]);
    assert!(!GpuSim::worth(&mid, false));
    assert!(GpuSim::worth_run(&mid, 1500));
    assert!(!GpuSim::worth_run(&mid, 10));
    // 40 x 40: 12,800 reads a generation; even 1,500 generations is 19 million reads.
    let tiny = world(40, 40, vec![life()]);
    assert!(!GpuSim::worth_run(&tiny, 1500));
    assert!(GpuSim::worth_run(&tiny, 1_000_000));
    // A rule the GPU can't run never goes there.
    let lenia = world(400, 300, vec![CaSpec::new("Lenia").seed(SeedSpec::Empty)]);
    assert!(!GpuSim::worth_run(&lenia, 1_000_000));
}
