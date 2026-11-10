//! Timing of the pieces of a sliding-window rebuild (`cargo run --release -p jetcarp-core --example
//! window_bench -- [family] [margin]`).

use jetcarp_core::window::{Disc, Hier};
use jetcarp_core::world::World;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    if std::env::var("ONE").is_ok() {
        return pool.install(run);
    }
    run()
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let family = args.get(1).map(String::as_str).unwrap_or("spectre");
    let margin: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let mut h = Hier::new(family, 4, 8).unwrap();
    let n = 200;
    let disc = |i: usize| Disc {
        c: [i as f64 * 3.0, 0.5 * i as f64],
        r: margin + 5.0,
    };
    // Warm up (grow the hierarchy).
    h.build(&[disc(n)]).unwrap();
    let t = Instant::now();
    let mut tiles = 0;
    for i in 0..n {
        tiles += h.tiles_in(&[disc(i)]).unwrap().len();
    }
    let dt = t.elapsed().as_secs_f64() / n as f64;
    println!("tiles_in: {:.3} ms ({} tiles)", dt * 1e3, tiles / n);
    let t = Instant::now();
    let mut wins = Vec::new();
    for i in 0..n {
        wins.push(h.build(&[disc(i)]).unwrap());
    }
    let dt = t.elapsed().as_secs_f64() / n as f64;
    println!("build (tiles, graph, topology): {:.3} ms", dt * 1e3);
    let t = Instant::now();
    for w in &wins {
        let tiles = h.tiles_in(&w.discs).unwrap();
        let (shape, places): (Vec<u16>, Vec<_>) = tiles.into_iter().unzip();
        std::hint::black_box(jetcarp_core::aperiodic::from_places(family, shape, places).unwrap());
    }
    let dt2 = t.elapsed().as_secs_f64() / n as f64;
    println!("tiles + graph: {:.3} ms", dt2 * 1e3);
    let t = Instant::now();
    for w in &wins {
        let mut world = World::on_topology(Arc::clone(&w.topo), 1);
        let ca = jetcarp_core::world::CaSpec::new("SpectreGlider");
        world
            .apply(jetcarp_core::world::Command::AddCa { ca })
            .unwrap();
        std::hint::black_box(&world);
    }
    let dt = t.elapsed().as_secs_f64() / n as f64;
    println!("world: {:.3} ms", dt * 1e3);
    let t = Instant::now();
    for w in &wins {
        for _ in 0..1 {
            std::hint::black_box(w.topo.centroid(0));
        }
    }
    let _ = t;
}
