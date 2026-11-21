//! The benchmark behind the JETCARP Hardware Score: JETCARP's own work on fixed scenes and
//! seeds, about a minute in all, off screen (nothing is drawn but the caller's progress bar).
//!
//! - **Soups** (scored, [`SOUP_TILINGS`]): a batch of [`SOUPS`] Life soups on each of five
//!   non-square tilings, each soup a random start (density 1/2) in the middle third of a world
//!   of about [`SOUP_CELLS`] cells, run [`SOUP_GENS`] generations, the way the explore searches
//!   run them ([`crate::explore::batch`]): on every core, and on each GPU given.
//! - **One core**: a square and a hexagonal Life world, in cell-generations a second.
//! - **A hunt**: Life soups on the square grid run to their census, as soup hunts run them.
//! - **The GPU crossover**: the smallest square Life world the GPU steps faster than every core.
//! - **Memory per cell**: the process's memory before and after building and stepping a large
//!   world of each main kind.
//! - **The disk**: nested folders and small files made and removed.
//!
//! **Every workload checks its answer**: the digest of its final states must equal the one
//! stored here ([`ANSWERS`]), on the processor and the GPU alike (results are bit-identical
//! everywhere). A wrong answer marks that device unusable for the workload, and the score says
//! so. The answers never depend on the machine; only the times do. A quick run
//! ([`Options::quick`]) runs each workload once, for the answers alone (the tests).

use super::score::{BENCHMARK, Part, Score};
use crate::TilingSpec;
use crate::explore::batch::{self, Backend, BatchJob, BatchWorld, WorldTrace};
use crate::hunt::separate::{Cache, Census};
use crate::hunt::soup::{self, Layout, SoupSpec};
use crate::rng::Rng;
use crate::rules::Rule;
use crate::sha256::Sha256;
use crate::topology::{Boundary, Nb, SizeSpec, Topology};
use crate::world::{CaSpec, SeedSpec, World};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// The five tilings of score v1: (explore key, in words).
pub const SOUP_TILINGS: [(&str, &str); 5] = [
    ("hat", "the hat"),
    ("spectre", "the Spectre"),
    ("penrose-rhomb", "Penrose's rhombs"),
    ("rhombitrihex", "the 3.4.6.4 tiling"),
    ("k2-017", "the 2-uniform tiling k2-017"),
];
/// About this many cells a soup world (patches: the smallest level with at least half).
pub const SOUP_CELLS: u32 = 2048;
/// Soups in a batch.
pub const SOUPS: usize = 256;
/// Generations each soup runs.
pub const SOUP_GENS: u32 = 200;
/// The seed of every workload ("JETCARP1").
pub const SEED: u64 = 0x4a45_5443_4152_5031;

/// The one-core worlds: (id, words, tiling, rule), 512 by 512 unit cells, wrapped.
const CORE_WORLDS: [(&str, &str, &str, &str); 2] = [
    ("core-square", "Life on squares", "square", "B3/S23"),
    ("core-hex", "Life on hexagons", "hex", "B2/S34"),
];
const CORE_SIDE: u32 = 512;
const CORE_GENS: u64 = 48;
/// Soups in the hunt (16 by 16 unit cells on the square grid, as hunts make them).
const HUNT_SOUPS: u64 = 48;

/// The stored answers: each workload's digest, the same on every machine and device.
pub const ANSWERS: &[(&str, &str)] = &[
    ("soups-hat", "95dd83ac2cd171fd"),
    ("soups-spectre", "adbd8cfad3f800c8"),
    ("soups-penrose-rhomb", "274f476adb7d8289"),
    ("soups-rhombitrihex", "cbf374e6d188ea8a"),
    ("soups-k2-017", "d10bc1c6dcee30f0"),
    ("core-square", "aef3e11bc0bb7b73"),
    ("core-hex", "3532af6541c49f29"),
    ("hunt-square", "6678502284183452"),
];

/// The stored answer for a workload.
pub fn answer(id: &str) -> Option<&'static str> {
    ANSWERS.iter().find(|a| a.0 == id).map(|a| a.1)
}

/// A world stepper on a GPU (`jetcarp_gpu::GpuSim`), for the crossover.
pub trait WorldStepper {
    fn step(&mut self, w: &mut World, gens: u64) -> Result<(), String>;
}

/// One GPU to measure: its explore batch stepper and its world stepper, either may be missing.
pub struct Gpu {
    pub name: String,
    pub batch: Option<Box<dyn Backend>>,
    pub world: Option<Box<dyn WorldStepper>>,
}

/// How to run.
pub struct Options {
    /// Each workload once, for its answer (the tests): the rates mean little.
    pub quick: bool,
    /// Seconds each timed part repeats its workload for (at least once).
    pub part_seconds: f64,
    /// Where the disk part makes its folders (removed after: the data folder); none skips it.
    pub scratch: Option<PathBuf>,
    /// The process's memory now, in bytes, if the program can read it (the memory part).
    pub rss: Option<fn() -> Option<u64>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            quick: false,
            part_seconds: 4.0,
            scratch: None,
            rss: None,
        }
    }
}

/// What the benchmark reports while it runs: the fraction done and what it is doing.
pub type Progress<'a> = &'a (dyn Fn(f64, &str) + Sync);

/// The benchmark's error when it was stopped.
pub const STOPPED: &str = "the benchmark was stopped";

/// Run the benchmark. `stop` is asked between workloads and repetitions.
pub fn run(
    opts: &Options,
    mut gpus: Vec<Gpu>,
    progress: Progress,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Score, String> {
    let start = Instant::now();
    let threads = rayon::current_num_threads();
    let cpu = format!("processor ({threads} threads)");
    let mut parts: Vec<Part> = vec![];
    let mut problems: Vec<String> = vec![];
    let steps = (SOUP_TILINGS.len() * (1 + gpus.len()) + CORE_WORLDS.len() + 3 + gpus.len()) as f64;
    let mut done = 0.0;
    let tick = |what: &str, done: &mut f64| {
        progress(*done / steps, what);
        *done += 1.0;
    };
    let check = |id: &str, digest: &str, device: &str, problems: &mut Vec<String>| -> bool {
        let ok = answer(id) == Some(digest);
        if !ok {
            problems.push(format!(
                "{device} gave a wrong answer on {id} ({digest}, not {}): it is not used for it",
                answer(id).unwrap_or("?")
            ));
        }
        ok
    };
    // The soups: the score.
    for (key, words) in SOUP_TILINGS {
        if stop() {
            return Err(STOPPED.into());
        }
        let id = format!("soups-{key}");
        let job = soup_job(key)?;
        tick(&format!("Life soups on {words}, every core"), &mut done);
        let (rate, digest) = timed(opts, SOUPS as f64, stop, || {
            batch::run_cpu(&job).map(|t| digest_traces(&t))
        })?;
        let ok = check(&id, &digest, &cpu, &mut problems);
        parts.push(Part {
            id: id.clone(),
            label: format!("Life soups on {words}"),
            device: cpu.clone(),
            value: rate,
            unit: "soups/s".into(),
            correct: Some(ok),
            scored: true,
        });
        for g in gpus.iter_mut() {
            tick(&format!("Life soups on {words}, {}", g.name), &mut done);
            let Some(b) = g.batch.as_mut() else {
                continue;
            };
            if !b.supports(&job.worlds[0].rule) || job.topo.len() * SOUPS > b.max_cells() {
                problems.push(format!("{} cannot run the soups on {words}", g.name));
                continue;
            }
            match timed(opts, SOUPS as f64, stop, || {
                b.run(&job).map(|t| digest_traces(&t))
            }) {
                Ok((rate, digest)) => {
                    let ok = check(&id, &digest, &g.name, &mut problems);
                    parts.push(Part {
                        id: id.clone(),
                        label: format!("Life soups on {words}"),
                        device: g.name.clone(),
                        value: rate,
                        unit: "soups/s".into(),
                        correct: Some(ok),
                        scored: true,
                    });
                }
                Err(e) if e == STOPPED => return Err(e),
                Err(e) => problems.push(format!(
                    "{} could not run the soups on {words}: {e}",
                    g.name
                )),
            }
        }
    }
    // One core.
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .map_err(|e| e.to_string())?;
    for (id, words, family, rule) in CORE_WORLDS {
        if stop() {
            return Err(STOPPED.into());
        }
        tick(&format!("{words}, one core"), &mut done);
        let w0 = core_world(family, rule)?;
        let cells = w0.topo.len() as f64;
        let (rate, digest) = one.install(|| {
            timed(opts, cells * CORE_GENS as f64, stop, || {
                let mut w = w0.clone();
                for _ in 0..CORE_GENS {
                    w.step();
                }
                Ok(w.hash_hex()[..16].to_string())
            })
        })?;
        let ok = check(id, &digest, "the processor", &mut problems);
        parts.push(Part {
            id: id.into(),
            label: words.into(),
            device: "processor (one core)".into(),
            value: rate,
            unit: "cell-generations/s".into(),
            correct: Some(ok),
            scored: false,
        });
    }
    // A hunt.
    if stop() {
        return Err(STOPPED.into());
    }
    tick("A soup hunt on squares, every core", &mut done);
    let hunt = Hunt::new()?;
    let (rate, digest) = timed(opts, HUNT_SOUPS as f64, stop, || Ok(hunt.run()))?;
    let ok = check("hunt-square", &digest, &cpu, &mut problems);
    parts.push(Part {
        id: "hunt-square".into(),
        label: "A Life soup hunt on squares".into(),
        device: cpu.clone(),
        value: rate,
        unit: "soups/s".into(),
        correct: Some(ok),
        scored: false,
    });
    // The GPU crossover.
    for g in gpus.iter_mut() {
        if stop() {
            return Err(STOPPED.into());
        }
        tick(&format!("Where {} beats the processor", g.name), &mut done);
        let Some(s) = g.world.as_mut() else {
            continue;
        };
        match crossover(opts, s.as_mut(), stop) {
            Ok((cells, same)) => {
                if !same {
                    problems.push(format!(
                        "{} gave a wrong answer stepping a square Life world: it is not used for it",
                        g.name
                    ));
                }
                parts.push(Part {
                    id: "gpu-crossover".into(),
                    label: "Smallest Life world the GPU steps faster".into(),
                    device: g.name.clone(),
                    value: cells as f64,
                    unit: if cells == 0 {
                        "cells (never, up to 4 million)".into()
                    } else {
                        "cells".into()
                    },
                    correct: Some(same),
                    scored: false,
                });
            }
            Err(e) if e == STOPPED => return Err(e),
            Err(e) => problems.push(format!("{} could not step the worlds: {e}", g.name)),
        }
    }
    // Memory per cell.
    tick("Memory per cell", &mut done);
    if let Some(rss) = opts.rss {
        let side = if opts.quick { 1024 } else { 4096 };
        for (id, words, rule) in [
            ("memory-life", "a Life world", "B3/S23"),
            ("memory-sandpile", "a sandpile", "Sandpile"),
            ("memory-lenia", "a Lenia world", "Lenia"),
        ] {
            if stop() {
                return Err(STOPPED.into());
            }
            if let Some(b) = memory_per_cell(side, rule, rss) {
                parts.push(Part {
                    id: id.into(),
                    label: format!("Memory in use per cell, {words}"),
                    device: "processor".into(),
                    value: b,
                    unit: "bytes/cell".into(),
                    correct: None,
                    scored: false,
                });
            }
        }
    }
    // The disk.
    tick("The disk", &mut done);
    if let Some(dir) = &opts.scratch {
        match disk(dir) {
            Ok(rate) => parts.push(Part {
                id: "disk".into(),
                label: "Folders and small files made and removed".into(),
                device: "the data folder's disk".into(),
                value: rate,
                unit: "items/s".into(),
                correct: None,
                scored: false,
            }),
            Err(e) => problems.push(format!("the disk part could not run: {e}")),
        }
    }
    progress(1.0, "Done");
    let score = Score::from_parts(&parts);
    Ok(Score {
        benchmark: BENCHMARK,
        jetcarp: crate::VERSION.into(),
        date: String::new(),
        score,
        parts,
        problems,
        quick: opts.quick,
        seconds: start.elapsed().as_secs_f64(),
    })
}

/// Run `f` once (quick), or repeatedly for the part's time; every run must give the same
/// digest. Returns `units` a second and the digest.
fn timed(
    opts: &Options,
    units: f64,
    stop: &(dyn Fn() -> bool + Sync),
    mut f: impl FnMut() -> Result<String, String>,
) -> Result<(f64, String), String> {
    let t = Instant::now();
    let first = f()?;
    let mut runs = 1u32;
    if !opts.quick {
        while t.elapsed().as_secs_f64() < opts.part_seconds {
            if stop() {
                return Err(STOPPED.into());
            }
            let d = f()?;
            if d != first {
                return Err(format!(
                    "the same work gave two answers ({first}, then {d})"
                ));
            }
            runs += 1;
        }
    }
    let secs = t.elapsed().as_secs_f64().max(1e-9);
    Ok((units * runs as f64 / secs, first))
}

/// The batch of soups on one tiling.
pub fn soup_job(key: &str) -> Result<BatchJob, String> {
    let (_, topo) = crate::explore::space::sized_spec(key, SOUP_CELLS)?;
    let n = topo.len();
    // The middle third of the cells, nearest the centre first.
    let c = topo.centroid(topo.center_cell());
    let mut order: Vec<(f64, usize)> = (0..n)
        .map(|i| {
            let p = topo.centroid(i);
            ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2), i)
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let middle: Vec<usize> = order[..n / 3].iter().map(|o| o.1).collect();
    let (rule, _) = Rule::parse("B3/S23")?;
    let worlds = (0..SOUPS)
        .map(|s| {
            let mut rng = Rng::new(SEED ^ (s as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut init = vec![0u8; n];
            for &i in &middle {
                init[i] = (rng.next_u64() >> 63) as u8;
            }
            BatchWorld {
                rule: rule.clone(),
                init,
            }
        })
        .collect();
    Ok(BatchJob {
        topo,
        nb: Nb::Touch,
        worlds,
        gens: SOUP_GENS,
        frames_at: vec![],
    })
}

/// The digest of a batch: each soup's last population and hash, and the generation it settled
/// (repeated a state of up to 8 generations before), in order.
pub fn digest_traces(traces: &[WorldTrace]) -> String {
    let mut h = Sha256::new();
    for t in traces {
        let last = t.pop.len().saturating_sub(1);
        let settled = (1..t.pop.len())
            .find(|&g| {
                (1..=8.min(g)).any(|p| t.hash[g] == t.hash[g - p] && t.pop[g] == t.pop[g - p])
            })
            .unwrap_or(t.pop.len()) as u32;
        h.update(&t.pop[last].to_le_bytes());
        h.update(&t.hash[last].to_le_bytes());
        h.update(&settled.to_le_bytes());
    }
    h.finish_hex()[..16].to_string()
}

fn core_world(family: &str, rule: &str) -> Result<World, String> {
    let mut t = TilingSpec::new(family);
    t.size = Some(SizeSpec::Rect {
        w: CORE_SIDE,
        h: CORE_SIDE,
    });
    t.boundary = Boundary::Wrap;
    let mut w = World::new(t, SEED)?;
    w.add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.5 }))?;
    Ok(w)
}

/// A square Life world of `side` by `side`, wrapped, seeded at random.
fn square_world(side: u32) -> Result<World, String> {
    let mut t = TilingSpec::new("square");
    t.size = Some(SizeSpec::Rect { w: side, h: side });
    t.boundary = Boundary::Wrap;
    let mut w = World::new(t, SEED)?;
    w.add_ca(CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.5 }))?;
    Ok(w)
}

/// The smallest square Life world (from 32 by 32 to 2048 by 2048) the GPU steps faster than
/// every core (0: it never did), and whether the GPU's result equalled the processor's at every
/// size (the processor's own answer is checked by the one-core part).
fn crossover(
    opts: &Options,
    gpu: &mut dyn WorldStepper,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<(u64, bool), String> {
    let sides: &[u32] = if opts.quick {
        &[32, 128]
    } else {
        &[32, 64, 128, 256, 512, 1024, 2048]
    };
    let mut same = true;
    let mut found = 0u64;
    for &side in sides {
        if stop() {
            return Err(STOPPED.into());
        }
        let w0 = square_world(side)?;
        let cells = w0.topo.len() as u64;
        let gens = (4_000_000 / cells).clamp(8, 400);
        let mut cpu = w0.clone();
        let t = Instant::now();
        for _ in 0..gens {
            cpu.step();
        }
        let cpu_s = t.elapsed().as_secs_f64();
        let mut g = w0.clone();
        // Once to open and upload, then timed.
        gpu.step(&mut g, 1)?;
        let t = Instant::now();
        gpu.step(&mut g, gens - 1)?;
        let gpu_s = t.elapsed().as_secs_f64() * gens as f64 / (gens - 1) as f64;
        same &= g.hash_hex() == cpu.hash_hex();
        if found == 0 && gpu_s < cpu_s {
            found = cells;
        }
    }
    Ok((found, same))
}

/// The hunt: Life soups 16 by 16 on the square grid, run to their census.
struct Hunt {
    census: Census,
    layout: Layout,
    limits: soup::Limits,
}

impl Hunt {
    fn new() -> Result<Hunt, String> {
        let tiling = TilingSpec::new("square");
        let census = Census::new("B3/S23", &tiling, None, 64)?;
        let spec = SoupSpec::default();
        let layout = Layout::new(&census.topo, spec, 32)?;
        let cells = layout.cells.len() as u64;
        Ok(Hunt {
            census,
            layout,
            limits: soup::Limits {
                max_gens: 20_000,
                settle_period: 64,
                pop_cap: (8 * cells).max(2000),
                max_span: 512,
                cut_every: 16,
            },
        })
    }

    fn run(&self) -> String {
        let outs: Vec<soup::SoupOut> = (0..HUNT_SOUPS)
            .into_par_iter()
            .map_init(
                || Cache::with_budget(Cache::DEFAULT_BUDGET),
                |cache, i| {
                    let state = soup::soup_state(SEED, i);
                    soup::run_soup(&self.census, cache, &self.layout, self.limits, i, state)
                },
            )
            .collect();
        let mut h = Sha256::new();
        for o in outs {
            let mut ids: Vec<&str> = o.objects.iter().map(|f| f.id.as_str()).collect();
            ids.sort();
            h.text(&format!(
                "{}:{}:{}:{}:{}|",
                o.index,
                o.end.name(),
                o.period,
                o.gens,
                ids.join(",")
            ));
        }
        h.finish_hex()[..16].to_string()
    }
}

/// Bytes per cell the process grew by while a `side` by `side` world of `rule` was built,
/// seeded and stepped once.
fn memory_per_cell(side: u32, rule: &str, rss: fn() -> Option<u64>) -> Option<f64> {
    let before = rss()?;
    let mut t = TilingSpec::new("square");
    t.size = Some(SizeSpec::Rect { w: side, h: side });
    t.boundary = Boundary::Wrap;
    let mut w = World::new(t, SEED).ok()?;
    w.add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.5 }))
        .ok()?;
    w.step();
    let after = rss()?;
    let cells = w.topo.len() as f64;
    drop(w);
    Some(after.saturating_sub(before) as f64 / cells)
}

/// Items (folders and files) a second: ten chains of 30 nested folders, a small file in each,
/// made and then removed.
fn disk(dir: &std::path::Path) -> Result<f64, String> {
    let root = dir.join(format!("jetcarp-bench-disk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let t = Instant::now();
    let mut items = 0u32;
    for chain in 0..10 {
        let mut p = root.join(format!("c{chain}"));
        for depth in 0..30 {
            p = p.join(format!("d{depth}"));
            std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            std::fs::write(p.join("f.txt"), b"JETCARP")
                .map_err(|e| format!("{}: {e}", p.display()))?;
            items += 2;
        }
    }
    std::fs::remove_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    Ok(items as f64 * 2.0 / t.elapsed().as_secs_f64().max(1e-9))
}

/// The topology of a soup job, for callers that size GPU batches.
pub fn soup_topology(key: &str) -> Result<Arc<Topology>, String> {
    Ok(crate::explore::space::sized_spec(key, SOUP_CELLS)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answers on the processor, every workload once.
    #[test]
    fn the_benchmark_gives_its_stored_answers() {
        let dir = crate::scratch::ScratchDir::new("machine-bench");
        std::fs::create_dir_all(&dir).unwrap();
        let opts = Options {
            quick: true,
            scratch: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let s = run(&opts, vec![], &|_, _| {}, &|| false).unwrap();
        for p in &s.parts {
            eprintln!("{} {} {} {:?}", p.id, p.device, p.value, p.correct);
        }
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert!(s.score > 0.0);
        assert!(s.quick);
        assert_eq!(s.benchmark, BENCHMARK);
        let ids: Vec<&str> = s.parts.iter().map(|p| p.id.as_str()).collect();
        for id in [
            "soups-hat",
            "soups-spectre",
            "soups-penrose-rhomb",
            "soups-rhombitrihex",
            "soups-k2-017",
            "core-square",
            "core-hex",
            "hunt-square",
            "disk",
        ] {
            assert!(ids.contains(&id), "{id} missing from {ids:?}");
        }
        assert_eq!(s.parts.iter().filter(|p| p.scored).count(), 5);
    }

    /// A device that gives a wrong answer is not used for that workload, and the score says so.
    #[test]
    fn a_wrong_answer_marks_the_device() {
        struct Wrong;
        impl Backend for Wrong {
            fn name(&self) -> String {
                "a broken GPU".into()
            }
            fn supports(&self, _: &Rule) -> bool {
                true
            }
            fn max_cells(&self) -> usize {
                usize::MAX
            }
            fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
                let mut t = batch::run_cpu(job)?;
                t[0].hash[SOUP_GENS as usize] ^= 1;
                Ok(t)
            }
        }
        let opts = Options {
            quick: true,
            ..Default::default()
        };
        let gpus = vec![Gpu {
            name: "a broken GPU".into(),
            batch: Some(Box::new(Wrong)),
            world: None,
        }];
        let s = run(&opts, gpus, &|_, _| {}, &|| false).unwrap();
        let wrong: Vec<&Part> = s.wrong().collect();
        assert_eq!(wrong.len(), 5);
        assert!(wrong.iter().all(|p| p.device == "a broken GPU"));
        assert_eq!(s.problems.len(), 5);
        assert!(s.problems[0].contains("a broken GPU gave a wrong answer on soups-hat"));
        // The processor's right answers still give a score.
        assert!(s.score > 0.0);
    }

    #[test]
    fn a_stop_ends_it() {
        let opts = Options::default();
        let e = run(&opts, vec![], &|_, _| {}, &|| true).unwrap_err();
        assert_eq!(e, STOPPED);
    }
}
