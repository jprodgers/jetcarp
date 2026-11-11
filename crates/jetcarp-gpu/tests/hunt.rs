//! The GPU soup batcher against the CPU: every soup's prefix (cells, generation, hash history,
//! outcome) must be identical to `prefix_cpu`, and a hunt with the GPU must leave exactly the
//! catalog a CPU-only hunt leaves. Skipped (with a note) when no GPU adapter is available.

use jetcarp_core::hunt::job::{self, HuntProgress, HuntSpec, SoupBatcher};
use jetcarp_core::hunt::separate::Census;
use jetcarp_core::hunt::soup::{Layout, prefix_cpu, soup_state};
use jetcarp_core::jobs::{RunOptions, Runner};
use jetcarp_core::library::{Library, Query};
use jetcarp_core::topology::TilingSpec;
use jetcarp_gpu::hunt::GpuHunter;
use std::sync::{Arc, OnceLock};

fn gpu() -> Option<Arc<GpuHunter>> {
    static G: OnceLock<Option<Arc<GpuHunter>>> = OnceLock::new();
    G.get_or_init(|| match GpuHunter::headless() {
        Ok(g) => {
            eprintln!("GPU hunter tests on {}", g.name());
            Some(Arc::new(g))
        }
        Err(e) => {
            eprintln!("note: skipping GPU hunter tests: {e}");
            None
        }
    })
    .clone()
}

fn check_prefixes(g: &GpuHunter, rule: &str, tiling: &str, n: u64) {
    let t = jetcarp_core::library::tiling_from_key(tiling).unwrap();
    let spec = HuntSpec::default();
    let c = Census::new(rule, &t, None, spec.max_period).unwrap();
    let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
    let limits = spec.limits(layout.cells.len());
    assert!(
        g.supports(&c, layout.region),
        "{rule} on {tiling} should fit"
    );
    let soups: Vec<_> = (0..n).map(|i| layout.soup(soup_state(9, i))).collect();
    let got = g.prefix(&c, layout.region, limits, &soups).unwrap();
    let mut ends = std::collections::BTreeMap::new();
    for (i, s) in soups.iter().enumerate() {
        let want = prefix_cpu(&c, layout.region, limits, s);
        assert_eq!(got[i], want, "{rule} on {tiling}, soup {i}");
        *ends
            .entry(format!("{:?}", want.end.map(|e| e.0)))
            .or_insert(0u32) += 1;
    }
    eprintln!("{rule} on {tiling}: {n} prefixes equal ({ends:?})");
}

#[test]
fn gpu_prefixes_equal_cpu() {
    let Some(g) = gpu() else { return };
    check_prefixes(&g, "B3/S23", "square", 600);
    check_prefixes(&g, "B3/S23V", "square", 300);
    check_prefixes(&g, "B2/S34", "hex", 300);
    check_prefixes(&g, "B3/S23/C3", "square", 300);
    check_prefixes(&g, "B2/S/C3", "square", 100);
    check_prefixes(&g, "B3/S23", "oct", 200);
    check_prefixes(&g, "B2/S34", "trihex", 200);
}

/// Prefix throughput, GPU and CPU (ignored; `W=rule@tiling,... N=...`).
#[test]
#[ignore]
fn gpu_prefix_speed() {
    use rayon::prelude::*;
    let Some(g) = gpu() else { return };
    let n: u64 = std::env::var("N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16384);
    let what = std::env::var("W").unwrap_or_else(|_| "B3/S23@square".into());
    for w in what.split(',') {
        let (rule, tiling) = w.split_once('@').unwrap();
        let t = jetcarp_core::library::tiling_from_key(tiling).unwrap();
        let spec = HuntSpec::default();
        let c = Census::new(rule, &t, None, spec.max_period).unwrap();
        let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
        let limits = spec.limits(layout.cells.len());
        let soups: Vec<_> = (0..n).map(|i| layout.soup(soup_state(9, i))).collect();
        let _ = g.prefix(&c, layout.region, limits, &soups[..64]).unwrap();
        let t0 = std::time::Instant::now();
        let got = g.prefix(&c, layout.region, limits, &soups).unwrap();
        let tg = t0.elapsed().as_secs_f64();
        let t1 = std::time::Instant::now();
        let cpu: Vec<_> = soups
            .par_iter()
            .map(|s| prefix_cpu(&c, layout.region, limits, s))
            .collect();
        let tc = t1.elapsed().as_secs_f64();
        let gens: u64 = got.iter().map(|p| p.generation).sum();
        let handed = got.iter().filter(|p| p.end.is_none()).count();
        eprintln!(
            "{rule} on {tiling}: {n} prefixes, GPU {:.0}/s, CPU (all threads) {:.0}/s, mean {} gens, {handed} handed over",
            n as f64 / tg,
            n as f64 / tc,
            gens / n
        );
        assert_eq!(got.len(), cpu.len());
    }
}

#[test]
fn gpu_hunt_equals_cpu_hunt() {
    let Some(g) = gpu() else { return };
    let spec = HuntSpec {
        rules: vec!["B3/S23".into(), "B2/S34".into()],
        tilings: vec![TilingSpec::new("square"), TilingSpec::new("hex")],
        soups: Some(400),
        ..Default::default()
    };
    let mut snaps = Vec::new();
    for use_gpu in [false, true] {
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "jetcarp-gpu-hunt-test-{use_gpu}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = Library::open(dir);
        let jid = job::start(&lib, &spec).unwrap();
        let o = RunOptions {
            gpu: use_gpu.then(|| g.clone() as Arc<dyn SoupBatcher>),
            ..Default::default()
        };
        let p: HuntProgress = Runner::new(lib.clone()).run(jid, &o).unwrap().progress_as();
        assert_eq!(p.status, "done");
        let mut recs = lib.query(&Query::default()).unwrap();
        recs.sort_by(|a, b| a.id.cmp(&b.id));
        let rows: Vec<String> = recs
            .iter()
            .map(|r| {
                format!(
                    "{} {} {} seen {} soup {:?} gen {:?}",
                    r.id,
                    r.rule,
                    r.class.name(),
                    r.seen,
                    r.provenance.soup,
                    r.provenance.generation
                )
            })
            .collect();
        let mut groups = p.groups.clone();
        for x in &mut groups {
            x.error = None;
        }
        snaps.push((
            rows,
            lib.job_sightings(jid).unwrap(),
            groups,
            p.newest.clone(),
        ));
    }
    assert_eq!(snaps[0], snaps[1]);
}
