//! CPU (all threads) vs GPU generations per second for Life from about 16k to 16M cells, on the
//! square grid and on two periodic tilings, with wrap edges. Also checks that the hashes agree.
//! The automatic GPU policy's thresholds (`GPU_PACED_CELLS`, `GPU_UNPACED_READS`) come from these
//! tables.
//!
//! `cargo run --release -p jetcarp-gpu --example bench [-- --gens N] [--only K] [--paced]`
//!
//! - Unpaced (the default): many generations per call, as `jetcarp run` and scripts step. The
//!   threshold is in neighbour reads per generation (cells times neighbours).
//! - `--paced`: one generation per call with the world read back after each, as a wallpaper or
//!   the app at a set speed steps (one frame, one generation). Milliseconds per frame.
//!
//! `--only K` runs just the K-th case (0-18), so each grid size can run in its own process and
//! memory is returned between sizes (this machine shares RAM between the CPU and the GPU).
//!
//! `--families [--size S]` instead times every rule family of the world stepper (Life, JACA with
//! and without a blast, B0, Margolus, Larger than Life, forest fire, sandpile) on the square grid
//! with wrap edges at about 16k, 128k, 1M and 4M cells (or only the size `S`, 0-3), CPU against
//! GPU, and checks the hashes.

use jetcarp_core::*;
use jetcarp_gpu::{GpuSim, neighbour_reads};
use std::time::Instant;

fn main() {
    let arg = |name: &str| {
        std::env::args()
            .skip_while(|a| a != name)
            .nth(1)
            .and_then(|g| g.parse::<u64>().ok())
    };
    let gens = arg("--gens");
    let only = arg("--only").map(|k| k as usize);
    let paced = std::env::args().any(|a| a == "--paced");
    let t0 = Instant::now();
    let mut sim = match GpuSim::headless() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no GPU: {e}");
            return;
        }
    };
    let setup = t0.elapsed().as_secs_f64() * 1e3;
    if std::env::args().any(|a| a == "--families") {
        let size: Option<usize> = std::env::args()
            .skip_while(|a| a != "--size")
            .nth(1)
            .and_then(|g| g.parse().ok());
        return families(&mut sim, size);
    }
    println!(
        "GPU: {} (opened in {setup:.0} ms); CPU: {} threads; Life (B3/S23), wrap edges, random 35%{}",
        sim.name(),
        rayon::current_num_threads(),
        if paced {
            ", one generation per frame, read back each frame"
        } else {
            ""
        }
    );
    if only.is_none_or(|k| k == 0) {
        if paced {
            println!(
                "| Tiling | Cells | CPU ms/frame | GPU ms/frame | GPU / CPU | CPU path: processor ms/frame | GPU path: processor ms/frame | Hashes |"
            );
            println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
        } else {
            println!(
                "| Tiling | Cells | Reads/gen | Gens | CPU gens/s | GPU gens/s (incl. upload + readback) | GPU steady gens/s | Speed-up | Hashes |"
            );
            println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
        }
    }
    run_cases(&mut sim, gens, only, paced);
}

fn cases() -> Vec<(&'static str, TilingSpec)> {
    let mut v = Vec::new();
    for side in [64, 128, 256, 512, 1000, 2000, 4000] {
        v.push((
            "square",
            TilingSpec::rect("square", side, side, Boundary::Wrap),
        ));
    }
    for side in [90, 181, 362, 707, 1414, 2828] {
        v.push((
            "4.8.8 (2 types)",
            TilingSpec::rect("oct", side, side, Boundary::Wrap),
        ));
    }
    for side in [52, 104, 208, 408, 816, 1632] {
        v.push((
            "3.4.6.4 (6 types)",
            TilingSpec::rect("rhombitrihex", side, side, Boundary::Wrap),
        ));
    }
    v
}

fn run_cases(sim: &mut GpuSim, gens: Option<u64>, only: Option<usize>, paced: bool) {
    for (k, (name, t)) in cases().into_iter().enumerate() {
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
        if paced {
            if n > 4_500_000 {
                continue;
            }
            paced_case(sim, name, &mut cpu, &mut gpu, gens);
        } else {
            unpaced_case(sim, name, &mut cpu, &mut gpu, gens);
        }
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

/// Enough generations for about a second of the CPU's work, at least `gens`.
fn gens_for(n: usize, gens: Option<u64>) -> u64 {
    gens.unwrap_or_else(|| (100_000_000 / n.max(1) as u64).clamp(100, 5000))
}

fn unpaced_case(sim: &mut GpuSim, name: &str, cpu: &mut World, gpu: &mut World, gens: Option<u64>) {
    let n = cpu.topo.len();
    let gens = gens_for(n, gens);
    let reads = neighbour_reads(cpu);
    let t0 = Instant::now();
    cpu.apply(Command::Step { gens }).unwrap();
    let cpu_rate = gens as f64 / t0.elapsed().as_secs_f64();
    // First run includes the upload and the final readback.
    let t0 = Instant::now();
    let m = sim.step_mixed(gpu, gens).unwrap();
    let gpu_rate = gens as f64 / t0.elapsed().as_secs_f64();
    assert_eq!(m.cpu_gens, 0);
    let same = cpu.hash_hex() == gpu.hash_hex();
    // Steady state: fields already on the GPU, five times as many generations, one readback.
    let t0 = Instant::now();
    sim.step_mixed(gpu, 5 * gens).unwrap();
    let steady = (5 * gens) as f64 / t0.elapsed().as_secs_f64();
    println!(
        "| {name} | {n} | {reads} | {gens} | {cpu_rate:.1} | {gpu_rate:.1} | {steady:.1} | {:.2}x | {} |",
        gpu_rate / cpu_rate,
        if same { "equal" } else { "DIFFERENT" }
    );
}

fn paced_case(sim: &mut GpuSim, name: &str, cpu: &mut World, gpu: &mut World, frames: Option<u64>) {
    let n = cpu.topo.len();
    let frames = frames.unwrap_or(1000);
    let (t0, p0) = (Instant::now(), processor_ms());
    for _ in 0..frames {
        cpu.apply(Command::Step { gens: 1 }).unwrap();
    }
    let cpu_ms = t0.elapsed().as_secs_f64() * 1e3 / frames as f64;
    let cpu_proc = (processor_ms() - p0) / frames as f64;
    // One warm-up frame uploads the field.
    sim.run(gpu, 1, None).unwrap();
    let (t0, p0) = (Instant::now(), processor_ms());
    for _ in 1..frames {
        let deadline = Instant::now() + std::time::Duration::from_millis(20);
        let out = sim.run(gpu, 1, Some(deadline)).unwrap();
        assert_eq!(out.gens, 1);
    }
    let gpu_ms = t0.elapsed().as_secs_f64() * 1e3 / (frames - 1).max(1) as f64;
    let gpu_proc = (processor_ms() - p0) / (frames - 1).max(1) as f64;
    let same = cpu.hash_hex() == gpu.hash_hex();
    println!(
        "| {name} | {n} | {cpu_ms:.3} | {gpu_ms:.3} | {:.2} | {cpu_proc:.2} | {gpu_proc:.2} | {} |",
        gpu_ms / cpu_ms,
        if same { "equal" } else { "DIFFERENT" }
    );
}

/// Processor time this process has used, over all its threads, in milliseconds (Linux; 0
/// elsewhere).
fn processor_ms() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // After the command name in parentheses: utime and stime are fields 14 and 15.
    let rest = stat.rsplit(')').next().unwrap_or("");
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks: f64 = f
        .get(11..13)
        .map(|v| v.iter().filter_map(|x| x.parse::<f64>().ok()).sum())
        .unwrap_or(0.0);
    // The kernel's clock tick is 100 Hz on Linux's usual configurations.
    ticks * 10.0
}
