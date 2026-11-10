//! Throughput probes for the hunter (ignored; run with `--ignored --nocapture`).

use jetcarp_core::hunt::separate::{Cache, Census};
use jetcarp_core::hunt::soup::{Layout, Limits, SoupSpec, run_soup, soup_state};
use jetcarp_core::topology::TilingSpec;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::time::Instant;

fn probe(rule: &str, tiling: &str, n: u64, threads: usize) {
    let t = jetcarp_core::library::tiling_from_key(tiling).unwrap();
    let c = Census::new(rule, &t, None, 64).unwrap();
    let layout = Layout::new(&c.topo, SoupSpec::default(), 32).unwrap();
    let limits = Limits {
        max_gens: 20_000,
        settle_period: 64,
        pop_cap: 6000,
        max_span: 4096,
        cut_every: 16,
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let t0 = Instant::now();
    let outs: Vec<_> = pool.install(|| {
        (0..n)
            .into_par_iter()
            .map_init(Cache::default, |cache, i| {
                let t = Instant::now();
                let o = run_soup(&c, cache, &layout, limits, i, soup_state(1, i));
                let dt = t.elapsed().as_secs_f64();
                if dt > 0.05 {
                    eprintln!(
                        "slow soup {i}: {dt:.3}s {:?} {} gens, {} objects, other {:?}",
                        o.end,
                        o.gens,
                        o.objects.len(),
                        o.other
                    );
                }
                o
            })
            .collect()
    });
    let dt = t0.elapsed().as_secs_f64();
    let mut ends: BTreeMap<String, u64> = BTreeMap::new();
    let mut objs: BTreeMap<String, u64> = BTreeMap::new();
    let mut gens = 0;
    for o in &outs {
        *ends.entry(o.end.name().into()).or_default() += 1;
        gens += o.gens;
        for f in &o.objects {
            *objs.entry(f.label.clone()).or_default() += 1;
        }
    }
    let mut top: Vec<_> = objs.into_iter().collect();
    top.sort_by_key(|x| std::cmp::Reverse(x.1));
    top.truncate(12);
    println!(
        "{rule} on {tiling}, {threads} threads: {n} soups in {dt:.2}s = {:.0} soups/s, mean {} gens, {ends:?}\n  {top:?}",
        n as f64 / dt,
        gens / n
    );
}

#[test]
#[ignore]
fn hunt_throughput() {
    let n: u64 = std::env::var("N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let th: usize = std::env::var("T")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let what = std::env::var("W").unwrap_or_else(|_| "B3/S23@square".into());
    for w in what.split(',') {
        let (r, t) = w.split_once('@').unwrap();
        probe(r, t, n, th);
    }
    let _ = TilingSpec::new("square");
}

#[test]
#[ignore]
fn gun_hashes() {
    use jetcarp_core::hunt::bits::Universe;
    let rows = "........................o.........../......................o.o.........../............oo......oo............oo/...........o...o....oo............oo/oo........o.....o...oo............../oo........o...o.oo....o.o.........../..........o.....o.......o.........../...........o...o..................../............oo......................";
    let t = jetcarp_core::library::tiling_from_key("square").unwrap();
    let c = Census::new("B3/S23", &t, None, 64).unwrap();
    let mut u = Universe::with_cells(c.plan.clone(), &jetcarp_core::objects::rows_to_cells(rows));
    let mut seen = std::collections::HashMap::new();
    for g in 0..130 {
        if let Some(o) = seen.insert(u.hash(), (g, u.population())) {
            eprintln!(
                "gen {g} pop {} hash {:x} equals gen {:?}",
                u.population(),
                u.hash(),
                o
            );
        }
        u.step();
    }
    let u = Universe::with_cells(c.plan.clone(), &jetcarp_core::objects::rows_to_cells(rows));
    let limits = Limits {
        max_gens: 1500,
        settle_period: 64,
        pop_cap: 5000,
        max_span: 4096,
        cut_every: 16,
    };
    let mut run = jetcarp_core::hunt::soup::SoupRun::new(u, [-20, 56, -20, 29], limits);
    let mut cache = Cache::default();
    while run.end.is_none() {
        run.u.step();
        run.post(&c, &mut cache);
        eprintln!(
            "gen {} pop {} hash {:x} cut {} {:?}",
            run.u.generation,
            run.u.population(),
            run.u.hash(),
            run.cut.len(),
            run.u.bbox()
        );
    }
}

#[test]
#[ignore]
fn hunt_one_soup() {
    use jetcarp_core::hunt::bits::Universe;
    let i: u64 = std::env::var("I")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let t = jetcarp_core::library::tiling_from_key("square").unwrap();
    let c = Census::new("B3/S23", &t, None, 64).unwrap();
    let layout = Layout::new(&c.topo, SoupSpec::default(), 32).unwrap();
    let cells = layout.soup(soup_state(1, i));
    let mut u = Universe::with_cells(c.plan.clone(), &cells);
    let t0 = Instant::now();
    for g in 0..3000 {
        u.step();
        if g % 500 == 0 {
            eprintln!(
                "gen {g} pop {} {:?} t {:.3}",
                u.population(),
                u,
                t0.elapsed().as_secs_f64()
            );
        }
    }
    eprintln!("step only: {:.3}s", t0.elapsed().as_secs_f64());
    let t1 = Instant::now();
    let cells = u.cells();
    let mut cache = Cache::default();
    let parts = c.census(&mut cache, &cells);
    eprintln!(
        "census {} parts {:.3}s",
        parts.len(),
        t1.elapsed().as_secs_f64()
    );
    let limits = Limits {
        max_gens: 20_000,
        settle_period: 64,
        pop_cap: 6000,
        max_span: 4096,
        cut_every: 16,
    };
    let cells = layout.soup(soup_state(1, i));
    let u = Universe::with_cells(c.plan.clone(), &cells);
    let mut run = jetcarp_core::hunt::soup::SoupRun::new(u, layout.region, limits);
    let (mut ts, mut tp) = (0.0, 0.0);
    while run.end.is_none() {
        let a = Instant::now();
        run.u.step();
        let b = Instant::now();
        run.post(&c, &mut cache);
        let dt = b.elapsed().as_secs_f64();
        if dt > 0.01 {
            eprintln!("gen {} post {dt:.3}s {:?}", run.u.generation, run.u);
        }
        ts += (b - a).as_secs_f64();
        tp += dt;
    }
    eprintln!(
        "run: step {ts:.3}s post {tp:.3}s, {:?} cut {}",
        run.end,
        run.cut.len()
    );
}

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
    let t = jetcarp_core::library::tiling_from_key(tiling).unwrap();
    let spec = jetcarp_core::hunt::job::HuntSpec::default();
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
