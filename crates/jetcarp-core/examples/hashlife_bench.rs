//! Time HashLife on classic benchmark patterns and on soups:
//! `cargo run --release --example hashlife_bench [-- names...]`.
//!
//! Names: `gun`, `replicator`, `rpent`, `acorn`, `soup1k` (1024² torus soup to 1000, 10,000 and
//! 2^20, its node table capped at 24 million nodes), `plane1k` (the same soup on the unbounded plane to 2^20), `hex` (a 256² hexagon torus
//! soup to 2^16), `oct` (4.8.8, 128² to 2^12). With no names, all of them. `HL_LEAF=n` sets
//! the bit-parallel leaf level (0 = the plain recursion).

use jetcarp_core::hashlife::{self, Options};
use jetcarp_core::*;
use std::time::Instant;

const REPLICATOR: &str = "x = 5, y = 5, rule = B36/S23\n2b3o$bo2bo$o3bo$o2bo$3o!";
const RPENT: &str = "x = 3, y = 3\nb2o$2o$bo!";
const ACORN: &str = "x = 7, y = 3\nbo5b$3bo3b$2o2b3o!";

fn opts() -> Options {
    let mut o = Options {
        node_cap: 24_000_000,
        ..Options::default()
    };
    if let Ok(v) = std::env::var("HL_LEAF") {
        o.leaf_level = v.parse().unwrap();
    }
    o
}

fn soup_world(fam: &str, side: u32, rule: &str, seed: u64) -> World {
    let mut sc = Scene::new(TilingSpec::rect(fam, side, side, Boundary::Wrap));
    sc.rng_state = seed;
    sc.cas = vec![CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 })];
    World::from_scene(&sc).unwrap()
}

fn plane(text: &str, gens: &[u64], label: &str) {
    let o = opts();
    let mut p = hashlife::plane_from_text(text, None, &o).unwrap();
    let mut done = 0;
    let t = Instant::now();
    for &g in gens {
        p.step(g - done).unwrap();
        done = g;
        println!(
            "{label:<12} gen {g:>16}: {:>9.3} s  pop {:>14}  nodes {}",
            t.elapsed().as_secs_f64(),
            p.population(),
            p.stats().peak_nodes
        );
    }
}

fn torus(fam: &str, rule: &str, side: u32, gens: &[u64], label: &str) {
    for &g in gens {
        let mut w = soup_world(fam, side, rule, 7);
        let t = Instant::now();
        let r = hashlife::jump_world(&mut w, g, &opts()).unwrap();
        println!(
            "{label:<12} gen {g:>16}: {:>9.3} s  pop {:>14}  nodes {}",
            t.elapsed().as_secs_f64(),
            w.fields[0].population(),
            r.stats.peak_nodes
        );
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let on = |n: &str| a.is_empty() || a.iter().any(|x| x == n);
    if on("gun") {
        let rle = format!("x = 36, y = 9\n{}", jetcarp_core::computing::GOSPER_GUN_RLE);
        plane(&rle, &[1 << 20, 1 << 40], "gun");
    }
    if on("replicator") {
        plane(REPLICATOR, &[1 << 30], "replicator");
    }
    if on("rpent") {
        plane(RPENT, &[1103], "rpent");
    }
    if on("acorn") {
        plane(ACORN, &[5206], "acorn");
    }
    if on("soup1k") {
        torus("square", "B3/S23", 1024, &[1000, 10_000, 1 << 20], "soup1k");
    }
    if on("plane1k") {
        let w = soup_world("square", 1024, "B3/S23", 7);
        let mut p = hashlife::Plane::from_world(&w, &opts()).unwrap();
        let t = Instant::now();
        p.step(1 << 20).unwrap();
        println!(
            "{:<12} gen {:>16}: {:>9.3} s  pop {:>14}  nodes {}",
            "plane1k",
            1u64 << 20,
            t.elapsed().as_secs_f64(),
            p.population(),
            p.stats().peak_nodes
        );
    }
    if on("hex") {
        torus("hex", "B2/S34", 256, &[1 << 16], "hex");
    }
    if on("oct") {
        torus("oct", "B3/S23", 128, &[1 << 12], "oct");
    }
}
