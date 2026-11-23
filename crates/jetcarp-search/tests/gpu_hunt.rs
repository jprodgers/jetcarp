//! The GPU soup batcher against the CPU: every soup's prefix (cells, generation, hash history,
//! outcome) must be identical to `prefix_cpu`, and a hunt with the GPU must leave exactly the
//! catalog a CPU-only hunt leaves. Skipped (with a note) when no GPU adapter is available, failed
//! under `JETCARP_REQUIRE_GPU=1`.

use jetcarp_core::hunt::separate::Census;
use jetcarp_core::hunt::soup::{Layout, SoupBatcher, prefix_cpu, soup_state};
use jetcarp_core::topology::TilingSpec;
use jetcarp_gpu::hunt::GpuHunter;
use jetcarp_search::hunt::{self, HuntProgress, HuntSpec};
use jetcarp_search::jobs::{RunOptions, Runner};
use jetcarp_store::{Library, Query};
use std::sync::{Arc, OnceLock};

fn gpu() -> Option<Arc<GpuHunter>> {
    static G: OnceLock<Option<Arc<GpuHunter>>> = OnceLock::new();
    G.get_or_init(|| match GpuHunter::headless() {
        Ok(g) => {
            eprintln!("GPU hunter tests on {}", g.name());
            Some(Arc::new(g))
        }
        Err(e) => {
            jetcarp_gpu::no_gpu("GPU hunter tests", &e);
            None
        }
    })
    .clone()
}

/// How many of `n` soups to run: all of them, or an eighth (at least 16) on Windows' WARP, the
/// software renderer Windows CI runs these on (#321), where they took 23 minutes in all on its
/// four processors and timed out.
fn soups(g: &GpuHunter, n: u64) -> u64 {
    if g.name().contains("Microsoft Basic Render Driver") {
        (n / 8).max(16)
    } else {
        n
    }
}

fn check_prefixes(g: &GpuHunter, rule: &str, tiling: &str, n: u64) {
    let n = soups(g, n);
    let t = jetcarp_core::pattern::tiling_from_key(tiling).unwrap();
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

/// Isotropic rules (#70): the generated shader's decision diagrams against the CPU's. Some of
/// these rules' soups run longer than the GPU runs any (it hands them to the CPU): those must
/// equal the CPU's run to the same generation.
#[test]
fn gpu_prefixes_equal_cpu_for_isotropic_rules() {
    let Some(g) = gpu() else { return };
    for (rule, n) in [
        ("B2-a/S12", 300),
        ("B3/S2-i34q", 300),
        ("B3-cnqy/S23-a", 300),
        ("B2a3-ckn/S1e2-in3/C4", 200),
    ] {
        let n = soups(&g, n);
        let t = TilingSpec::new("square");
        let spec = HuntSpec::default();
        let c = Census::new(rule, &t, None, spec.max_period).unwrap();
        let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
        let limits = spec.limits(layout.cells.len());
        assert!(g.supports(&c, layout.region), "{rule} should fit");
        let soups: Vec<_> = (0..n).map(|i| layout.soup(soup_state(9, i))).collect();
        let got = g.prefix(&c, layout.region, limits, &soups).unwrap();
        let mut handed = 0;
        for (i, s) in soups.iter().enumerate() {
            if got[i].end.is_some() {
                assert_eq!(
                    got[i],
                    prefix_cpu(&c, layout.region, limits, s),
                    "{rule}, soup {i}"
                );
                continue;
            }
            handed += 1;
            let mut l = limits;
            l.max_gens = got[i].generation;
            let cpu = prefix_cpu(&c, layout.region, l, s);
            assert_eq!(
                (&got[i].cells, got[i].generation, &got[i].ring),
                (&cpu.cells, cpu.generation, &cpu.ring),
                "{rule}, soup {i}, handed over"
            );
        }
        eprintln!("{rule}: {n} prefixes equal, {handed} handed to the CPU");
    }
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
        let t = jetcarp_core::pattern::tiling_from_key(tiling).unwrap();
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
        soups: Some(soups(&g, 400)),
        ..Default::default()
    };
    hunts_agree(&g, &spec);
}

/// Isotropic rules (#70), whose long soups the GPU hands to the CPU.
#[test]
fn gpu_hunt_equals_cpu_hunt_for_isotropic_rules() {
    let Some(g) = gpu() else { return };
    let spec = HuntSpec {
        rules: vec!["B2-a/S12".into(), "B3/S2-i34q".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: Some(soups(&g, 300)),
        ..Default::default()
    };
    hunts_agree(&g, &spec);
}

/// A hunt run with the GPU and one without leave the same catalog.
fn hunts_agree(g: &Arc<GpuHunter>, spec: &HuntSpec) {
    let mut snaps = Vec::new();
    for use_gpu in [false, true] {
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "jetcarp-gpu-hunt-test-{}-{use_gpu}-{}",
            spec.rules.join("+").replace('/', ""),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = Library::open(dir);
        let jid = hunt::start(&lib, spec).unwrap();
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
