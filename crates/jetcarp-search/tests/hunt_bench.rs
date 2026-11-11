//! A profile of the hunter's soups with a hunt's default limits (ignored; run with
//! `--ignored --nocapture`). The throughput probes are in jetcarp-core.

use jetcarp_core::hunt::separate::{Cache, Census};
use jetcarp_core::hunt::soup::{Layout, soup_state};
use std::time::Instant;

/// Where the time of a soup goes, single thread (`W=rule@tiling N=...`).
#[test]
#[ignore]
fn hunt_profile() {
    use jetcarp_core::hunt::bits::Universe;
    use jetcarp_core::hunt::soup::{SoupRun, finish};
    let n: u64 = std::env::var("N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let what = std::env::var("W").unwrap_or_else(|_| "B3/S23@square".into());
    let (rule, tiling) = what.split_once('@').unwrap();
    let t = jetcarp_core::pattern::tiling_from_key(tiling).unwrap();
    let spec = jetcarp_search::hunt::HuntSpec::default();
    let c = Census::new(rule, &t, None, 64).unwrap();
    let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
    let limits = spec.limits(layout.cells.len());
    let mut cache = Cache::default();
    let (mut ts, mut tp, mut tc, mut tg) = (0.0, 0.0, 0.0, 0.0);
    let mut gens = 0;
    let all = Instant::now();
    for i in 0..n {
        let a = Instant::now();
        let cells = layout.soup(soup_state(1, i));
        let u = Universe::with_cells(c.plan.clone(), &cells);
        let mut run = SoupRun::new(u, layout.region, limits);
        tg += a.elapsed().as_secs_f64();
        while run.end.is_none() {
            let a = Instant::now();
            run.u.step();
            let b = Instant::now();
            run.post(&c, &mut cache);
            ts += (b - a).as_secs_f64();
            tp += b.elapsed().as_secs_f64();
        }
        gens += run.u.generation;
        let a = Instant::now();
        let _ = finish(&c, &mut cache, run, i, 0);
        tc += a.elapsed().as_secs_f64();
    }
    eprintln!(
        "{what}: {n} soups in {:.2}s ({:.0}/s): setup {tg:.2}s, step {ts:.2}s ({:.2} us/gen, mean {} gens), post {tp:.2}s, census {tc:.2}s; cache hits {} misses {}",
        all.elapsed().as_secs_f64(),
        n as f64 / all.elapsed().as_secs_f64(),
        ts * 1e6 / gens as f64,
        gens / n,
        cache.hits,
        cache.misses
    );
}
