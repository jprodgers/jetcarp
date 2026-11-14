//! CPU (all threads) vs GPU generations per second for Life at about 1M, 4M, and 16M cells, on the
//! square grid and on a periodic tiling, with wrap edges. Also checks that the hashes agree.
//!
//! `cargo run --release -p jetcarp-gpu --example bench [-- --gens N] [--only K]`
//!
//! `--only K` runs just the K-th case (0-8), so each grid size can run in its own process and
//! memory is returned between sizes (this machine shares RAM between the CPU and the GPU).
//!
//! `--families [--size S]` instead times every rule family of the world stepper (Life, JACA with
//! and without a blast, B0, Margolus, Larger than Life, forest fire, sandpile) on the square grid
//! with wrap edges at about 16k, 128k, 1M and 4M cells (or only the size `S`, 0-3), CPU against
//! GPU, and checks the hashes.

use jetcarp_core::*;
use jetcarp_gpu::GpuSim;
use std::time::Instant;

fn main() {
    let gens: u64 = std::env::args()
        .skip_while(|a| a != "--gens")
        .nth(1)
        .and_then(|g| g.parse().ok())
        .unwrap_or(100);
    let only: Option<usize> = std::env::args()
        .skip_while(|a| a != "--only")
        .nth(1)
        .and_then(|g| g.parse().ok());
    let mut sim = match GpuSim::headless() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no GPU: {e}");
            return;
        }
    };
    if std::env::args().any(|a| a == "--families") {
        let size: Option<usize> = std::env::args()
            .skip_while(|a| a != "--size")
            .nth(1)
            .and_then(|g| g.parse().ok());
        return families(&mut sim, size);
    }
    println!(
        "GPU: {}; CPU: {} threads; Life (B3/S23), wrap edges, random 35%, {gens} generations",
        sim.name(),
        rayon::current_num_threads()
    );
    if only.is_some_and(|k| k > 0) {
        return run_cases(&mut sim, gens, only);
    }
    println!(
        "| Tiling | Cells | CPU gens/s | GPU gens/s (incl. upload + readback) | GPU steady gens/s | Speed-up | Hashes |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | ---: | --- |");
    run_cases(&mut sim, gens, only);
}

fn run_cases(sim: &mut GpuSim, gens: u64, only: Option<usize>) {
    let cases: Vec<(&str, TilingSpec)> = vec![
        (
            "square",
            TilingSpec::rect("square", 1000, 1000, Boundary::Wrap),
        ),
        (
            "square",
            TilingSpec::rect("square", 2000, 2000, Boundary::Wrap),
        ),
        (
            "square",
            TilingSpec::rect("square", 4000, 4000, Boundary::Wrap),
        ),
        (
            "4.8.8 (2 types)",
            TilingSpec::rect("oct", 707, 707, Boundary::Wrap),
        ),
        (
            "4.8.8 (2 types)",
            TilingSpec::rect("oct", 1414, 1414, Boundary::Wrap),
        ),
        (
            "4.8.8 (2 types)",
            TilingSpec::rect("oct", 2828, 2828, Boundary::Wrap),
        ),
        (
            "3.4.6.4 (6 types)",
            TilingSpec::rect("rhombitrihex", 408, 408, Boundary::Wrap),
        ),
        (
            "3.4.6.4 (6 types)",
            TilingSpec::rect("rhombitrihex", 816, 816, Boundary::Wrap),
        ),
        (
            "3.4.6.4 (6 types)",
            TilingSpec::rect("rhombitrihex", 1632, 1632, Boundary::Wrap),
        ),
    ];
    for (k, (name, t)) in cases.into_iter().enumerate() {
        if only.is_some_and(|o| o != k) {
            continue;
        }
        let mut sc = Scene::new(t);
        sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 })];
        let mut cpu = match World::from_scene(&sc) {
            Ok(w) => w,
            Err(e) => {
                println!("| {name} | - | {e} |");
                continue;
            }
        };
        let mut gpu = World::from_scene(&sc).unwrap();
        let n = cpu.topo.len();
        let t0 = Instant::now();
        cpu.apply(Command::Step { gens }).unwrap();
        let cpu_rate = gens as f64 / t0.elapsed().as_secs_f64();
        // First run includes the upload and the final readback.
        let t0 = Instant::now();
        let m = sim.step_mixed(&mut gpu, gens).unwrap();
        let gpu_rate = gens as f64 / t0.elapsed().as_secs_f64();
        assert_eq!(m.cpu_gens, 0);
        let same = cpu.hash_hex() == gpu.hash_hex();
        drop(cpu);
        // Steady state: fields already on the GPU, five times as many generations, one readback.
        let t0 = Instant::now();
        sim.step_mixed(&mut gpu, 5 * gens).unwrap();
        let steady = (5 * gens) as f64 / t0.elapsed().as_secs_f64();
        println!(
            "| {name} | {n} | {cpu_rate:.1} | {gpu_rate:.1} | {steady:.1} | {:.1}x | {} |",
            steady / cpu_rate,
            if same { "equal" } else { "DIFFERENT" }
        );
    }
}

/// Generations per second of every family the world stepper runs, CPU against GPU (fields
/// resident, one readback at the end), on wrap-edge square grids.
fn families(sim: &mut GpuSim, size: Option<usize>) {
    let sides = [128u32, 362, 1024, 2048];
    let soup = |d: f64| SeedSpec::Random { density: d };
    let halves = SeedSpec::Halves { density: 0.35 };
    let cases: Vec<(&str, &str, SeedSpec)> = vec![
        ("Life", "B3/S23", soup(0.35)),
        ("JACA", "JACA B3/S23", halves.clone()),
        ("JACA blast 2", "JACA B3/S23 blast 2", halves),
        ("B0 Life-like", "B013/S1", soup(0.35)),
        ("Margolus", "Margolus Critters", soup(0.3)),
        ("Larger than Life R5", "R5,C2,S33-57,B34-45", soup(0.3)),
        (
            "Forest fire",
            "ForestFire 0.01/0.0001",
            SeedSpec::RandomStates,
        ),
        ("Sandpile", "Sandpile", SeedSpec::Empty),
    ];
    if size.is_none_or(|s| s == 0) {
        println!(
            "GPU: {}; CPU: {} threads; square grid, wrap edges",
            sim.name(),
            rayon::current_num_threads()
        );
        println!("| Family | Cells | CPU gens/s | GPU gens/s | Speed-up | Hashes |");
        println!("| --- | ---: | ---: | ---: | ---: | --- |");
    }
    for (k, &side) in sides.iter().enumerate() {
        if size.is_some_and(|s| s != k) {
            continue;
        }
        for (name, rule, seed) in &cases {
            let mut sc = Scene::new(TilingSpec::rect("square", side, side, Boundary::Wrap));
            sc.rng_state = 7;
            let mut ca = CaSpec::new(rule).seed(seed.clone());
            if *rule == "Sandpile" {
                // A tower of four grains per cell on the centre, toppling outwards.
                ca = ca.seed(SeedSpec::Pile {
                    grains: 4 * side as u64 * side as u64,
                });
            }
            sc.cas = vec![ca];
            let mut cpu = World::from_scene(&sc).unwrap();
            let mut gpu = World::from_scene(&sc).unwrap();
            let n = cpu.topo.len();
            // One generation each first: the CPU's warm-up (and its timing), the GPU's upload.
            let t0 = Instant::now();
            cpu.apply(Command::Step { gens: 1 }).unwrap();
            let one = t0.elapsed().as_secs_f64();
            sim.step_mixed(&mut gpu, 1).unwrap();
            let gens = ((1.5 / one.max(1e-6)) as u64).clamp(3, 3000);
            let t0 = Instant::now();
            cpu.apply(Command::Step { gens }).unwrap();
            let cpu_rate = gens as f64 / t0.elapsed().as_secs_f64();
            let t0 = Instant::now();
            let m = sim.step_mixed(&mut gpu, gens).unwrap();
            let first = gens as f64 / t0.elapsed().as_secs_f64();
            let same = cpu.hash_hex() == gpu.hash_hex() && m.cpu_gens == 0;
            drop(cpu);
            // Steady state: about a second and a half more on the GPU.
            let more = ((1.5 * first) as u64).clamp(gens, 50_000);
            let t0 = Instant::now();
            sim.step_mixed(&mut gpu, more).unwrap();
            let gpu_rate = more as f64 / t0.elapsed().as_secs_f64();
            println!(
                "| {name} | {n} | {cpu_rate:.1} | {gpu_rate:.1} | {:.1}x | {} |",
                gpu_rate / cpu_rate,
                if same { "equal" } else { "DIFFERENT" }
            );
        }
    }
}
