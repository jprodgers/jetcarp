//! Throughput of the GPU batch stepper: worlds per second and cell updates per second.
//!
//! `cargo run --release -p jetcarp-gpu --example batch_bench [-- --gens N] [--worlds N]`
//!
//! Each case runs on the square grid with wrap edges and 35% random starts, with frames read
//! back at three points. Rules differ per world.

use jetcarp_core::explore::batch::{BatchJob, BatchWorld};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Boundary, Nb, SizeSpec};
use jetcarp_core::{TilingSpec, Topology};
use jetcarp_gpu::batch::BatchSim;
use std::sync::Arc;
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 20) as u32
    }
    fn mask(&mut self, lo: u32, hi: u32) -> u32 {
        (lo..=hi).fold(0, |m, c| m | ((self.next() & 1) << c))
    }
}

fn digits(mask: u32) -> String {
    (0..=8)
        .filter(|c| mask >> c & 1 == 1)
        .map(|c| c.to_string())
        .collect()
}

/// A random Life-like rule without B0.
fn life(r: &mut Rng) -> String {
    format!("B{}/S{}", digits(r.mask(1, 8) | 8), digits(r.mask(0, 8)))
}

fn generations(r: &mut Rng) -> String {
    format!(
        "B{}/S{}/C{}",
        digits(r.mask(1, 8) | 4),
        digits(r.mask(0, 8)),
        3 + r.next() % 6
    )
}

fn ltl(r: &mut Rng) -> String {
    let b0 = 6 + r.next() % 8;
    let b1 = b0 + r.next() % 8;
    let s0 = 4 + r.next() % 10;
    let s1 = s0 + r.next() % 16;
    format!("R3,C2,S{s0}-{s1},B{b0}-{b1}")
}

fn init(r: &mut Rng, n: usize, limit: u32) -> Vec<u8> {
    (0..n)
        .map(|_| {
            if r.next() % 100 < 35 {
                (r.next() % limit) as u8 + 1
            } else {
                0
            }
        })
        .collect()
}

fn case(
    sim: &mut BatchSim,
    name: &str,
    spec: TilingSpec,
    worlds: usize,
    gens: u32,
    gen_rule: fn(&mut Rng) -> String,
    limit: u32,
) {
    let topo = Arc::new(Topology::build(&spec).expect("tiling"));
    let n = topo.len();
    let mut rng = Rng(0x9E3779B97F4A7C15 ^ n as u64);
    let list: Vec<BatchWorld> = (0..worlds)
        .map(|_| {
            let rule = gen_rule(&mut rng);
            BatchWorld {
                rule: Rule::parse(&rule).unwrap().0,
                init: init(&mut rng, n, limit),
            }
        })
        .collect();
    let job = BatchJob {
        topo,
        nb: Nb::Touch,
        worlds: list,
        gens,
        frames_at: vec![gens / 5, gens / 2, gens],
    };
    let t0 = Instant::now();
    let traces = sim.run(&job).expect("batch run");
    let secs = t0.elapsed().as_secs_f64();
    let cu = worlds as f64 * n as f64 * gens as f64;
    let live: u64 = traces.iter().map(|t| *t.pop.last().unwrap() as u64).sum();
    let st = sim.last_stats();
    println!(
        "| {name} | {worlds} | {n} | {gens} | {secs:.2} | {:.1} | {:.2} | {} | {} | {:.0} |",
        worlds as f64 / secs,
        cu / secs / 1e9,
        live,
        st.submissions,
        st.longest_secs * 1e3
    );
}

fn main() {
    let arg = |name: &str, default: u32| -> u32 {
        std::env::args()
            .skip_while(|a| a != name)
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let gens = arg("--gens", 500);
    let worlds = arg("--worlds", 1024) as usize;
    let mut sim = match BatchSim::headless() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no GPU: {e}");
            return;
        }
    };
    println!(
        "GPU: {}, max cells per batch {}",
        sim.name(),
        sim.max_cells()
    );
    println!(
        "| Case | Worlds | Cells | Gens | Secs | Worlds/s | GCU/s | Final live | Submissions | Longest (ms) |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    let square = |side: u32| TilingSpec::rect("square", side, side, Boundary::Wrap);
    let spectre = |level: u32| TilingSpec {
        size: Some(SizeSpec::Level(level)),
        ..TilingSpec::new("spectre")
    };
    let rounds = arg("--soak", 1);
    for round in 0..rounds {
        if rounds > 1 {
            println!("round {round}: VmRSS {} kB", rss_kb());
        }
        case(
            &mut sim,
            "Life-like mix, 16k",
            square(128),
            worlds,
            gens,
            life,
            1,
        );
        case(
            &mut sim,
            "Life-like mix, 65k",
            square(256),
            worlds,
            gens,
            life,
            1,
        );
        case(
            &mut sim,
            "Generations mix, 16k",
            square(128),
            worlds,
            gens,
            generations,
            1,
        );
        case(
            &mut sim,
            "Generations mix, 65k",
            square(256),
            worlds,
            gens,
            generations,
            1,
        );
        case(
            &mut sim,
            "LtL R3 mix, 16k",
            square(128),
            worlds,
            gens,
            ltl,
            1,
        );
        case(
            &mut sim,
            "LtL R3 mix, 65k",
            square(256),
            worlds,
            gens,
            ltl,
            1,
        );
        let rhombi = TilingSpec::rect("rhombitrihex", 105, 105, Boundary::Wrap);
        case(
            &mut sim,
            "Life-like mix, rhombitrihex 66k",
            rhombi,
            worlds,
            gens,
            life,
            1,
        );
        case(
            &mut sim,
            "Life-like mix, Spectre L4 4.4k",
            spectre(4),
            worlds,
            gens,
            life,
            1,
        );
        case(
            &mut sim,
            "Life-like mix, Spectre L5 35k",
            spectre(5),
            worlds,
            gens,
            life,
            1,
        );
    }
    if rounds > 1 {
        println!("end: VmRSS {} kB", rss_kb());
    }
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}
