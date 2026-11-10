//! Build exact aperiodic patches and report time and peak memory:
//! `cargo run --release -p jetcarp-core --example patch_bench -- spectre 7 [graph|verify|topology]`.

use std::time::Instant;

fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let family = args.get(1).map(String::as_str).unwrap_or("spectre");
    let level: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(7);
    let what = args.get(3).map(String::as_str).unwrap_or("graph");
    match (family, what) {
        ("spectre", "graph") => {
            let t = Instant::now();
            let pl = jetcarp_core::spectre::spectre_places(level).unwrap();
            let t1 = t.elapsed();
            let g = jetcarp_core::spectre::spectre_graph_exact(&pl).unwrap();
            let t2 = t.elapsed();
            println!(
                "spectre level {level}: {} tiles; places {:.2?}, graph {:.2?}; touch {} edge {} entries; peak {:.0} MB",
                pl.len(),
                t1,
                t2 - t1,
                g.touch.1.len(),
                g.edge.1.len(),
                peak_rss_mb()
            );
        }
        ("spectre", "verify") => {
            let t = Instant::now();
            let pl = jetcarp_core::spectre::spectre_places(level).unwrap();
            let st = jetcarp_core::spectre::verify_exact(&pl).unwrap();
            println!(
                "spectre level {level}: {st:?} in {:.2?}; peak {:.0} MB",
                t.elapsed(),
                peak_rss_mb()
            );
        }
        (_, "verify") => {
            let t = Instant::now();
            let p = jetcarp_core::aperiodic::build(family, level).unwrap();
            let t1 = t.elapsed();
            let st = jetcarp_core::sizeclass::verify(&p, family).unwrap();
            println!(
                "{family} level {level}: built in {t1:.2?}, verified in {:.2?}: {st:?}; peak {:.0} MB",
                t.elapsed() - t1,
                peak_rss_mb()
            );
        }
        (_, "topology") => {
            let t = Instant::now();
            let topo = jetcarp_core::Topology::build(
                &jetcarp_core::TilingSpec::new(family).with_size(jetcarp_core::SizeSpec::Level(level)),
            )
            .unwrap();
            // Bytes held per tile: placements, shape ids, both CSR graphs, open flags.
            let held = match &topo.storage {
                jetcarp_core::Storage::Explicit(e) => {
                    let place = match &e.transforms {
                        jetcarp_core::topology::Placements::Exact { places, .. } => places.len() * 20,
                        jetcarp_core::topology::Placements::Float(v) => v.len() * 48,
                    };
                    let csr: usize = e.csr.iter().map(|c| (c.off.len() + c.nb.len()) * 4).sum();
                    place + e.shape.len() * 2 + csr + e.open.len()
                }
                _ => 0,
            };
            println!(
                "{} in {:.2?}; peak {:.0} MB; holds {:.0} MB ({:.1} bytes per tile)",
                topo.describe(),
                t.elapsed(),
                peak_rss_mb(),
                held as f64 / 1048576.0,
                held as f64 / topo.len() as f64
            );
        }
        ("spectre", "room") => {
            // Rings of room around the level's deepest tile on each larger level.
            use jetcarp_core::*;
            let small =
                Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(level)))
                    .unwrap();
            let (root, d) = jetcarp_core::evolve::arena::deepest(&small);
            println!("level {level}: deepest tile {root}, {d} rings");
            for to in level + 1..=8 {
                let big =
                    Topology::build(&TilingSpec::new("spectre").with_size(SizeSpec::Level(to)))
                        .unwrap();
                let t = jetcarp_core::aperiodic::map_tile("spectre", level, to, root, &small, &big)
                    .unwrap()
                    .unwrap();
                let mut dist = vec![u32::MAX; big.len()];
                let mut q = std::collections::VecDeque::new();
                dist[t] = 0;
                q.push_back(t);
                let mut room = u32::MAX;
                while let Some(x) = q.pop_front() {
                    if big.is_open(x) {
                        room = dist[x];
                        break;
                    }
                    big.for_each_neighbor(x, Nb::Edge, |y| {
                        if dist[y] == u32::MAX {
                            dist[y] = dist[x] + 1;
                            q.push_back(y);
                        }
                    });
                }
                println!(
                    "  on level {to}: {room} rings to the edge (deepest there: {})",
                    jetcarp_core::evolve::arena::deepest(&big).1
                );
            }
        }
        ("hat", "gen") => {
            for k in 1..=level {
                let t = Instant::now();
                let (ms, c) = jetcarp_core::hat::generate(k);
                // Distance of each hat's corner 0 (+ centre offset) from the Eisenstein lattice.
                let mut worst: f64 = 0.0;
                for m in &ms {
                    let p = m.translation();
                    let b = (2.0 * p[1] / 3f64.sqrt()).round();
                    let a = (p[0] - b / 2.0).round();
                    let q = [a + b / 2.0, b * 3f64.sqrt() / 2.0];
                    worst = worst.max((p[0] - q[0]).hypot(p[1] - q[1]));
                }
                println!(
                    "hat k={k}: {} hats, centre {c:?}, off-lattice {worst:.2e}, {:.2?}",
                    ms.len(),
                    t.elapsed()
                );
            }
        }
        _ => eprintln!("usage: patch_bench <family> <level> [graph|verify|topology|room]"),
    }
}
