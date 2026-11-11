//! The explore search with the GPU batch stepper as its backend: an explore job run on the GPU
//! (only GPU families, so every run is the GPU loop's: screening batches and confirmation
//! batches), and every run it stored measured again on the CPU's own loop (`try_one`, which
//! steps the world with the engine): the same fate, behaviour bin, features, and score. Skipped
//! (with a note) when no GPU adapter is available.

use jetcarp_gpu::batch::BatchSim;
use jetcarp_search::explore::job::try_one;
use jetcarp_search::explore::runs::{RunFilter, Runs};
use jetcarp_search::explore::store;
use jetcarp_search::jobs::explore::{ExploreProgress, ExploreRun, db_path};
use jetcarp_search::jobs::{self, Budget, JobSpec, RunOptions, Runner};
use jetcarp_store::Library;
use serde_json::json;

#[test]
fn explore_runs_measured_on_the_gpu_match_the_cpu() {
    let sim = match BatchSim::headless() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("note: skipping the explore GPU test: {e}");
            return;
        }
    };
    let name = sim.name().to_string();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("explore-gpu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(dir.join("library.sqlite"));
    let job = jobs::create(
        &lib,
        &JobSpec {
            strategy: "explore".into(),
            spec: json!({
                "tilings": ["square", "hex", "trihex", "cairo"],
                "families": ["life_like", "generations", "cyclic", "rps", "greenberg_hastings",
                             "hodgepodge", "jaca", "priority"],
                "screen_cells": 1500, "screen_gens": 120,
                "confirm_cells": 3000, "confirm_gens": 200, "confirm_seeds": 2,
                "batch": 64, "seed": 11, "gpu": true,
            }),
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap();
    let mut run = ExploreRun::with_backend(Box::new(sim));
    let out = Runner::new(lib.clone())
        .run_with(
            job,
            &mut run,
            &RunOptions {
                threads: 2,
                budget: Budget::items(1200),
                ..Default::default()
            },
        )
        .unwrap();
    let p: ExploreProgress = serde_json::from_value(out.progress.clone()).unwrap();
    eprintln!("{} (GPU {name})", p.line);
    assert_eq!(p.backend, name);
    assert!(p.gpu_error.is_none(), "{:?}", p.gpu_error);
    assert!(p.screened >= 1200, "{}", p.screened);
    let c = Runs::open_read(&db_path(&lib, &lib.job(job).unwrap()).unwrap()).unwrap();
    // Screened runs spread over the families, and the confirmation runs.
    let mut ids = c.ids(&RunFilter::default(), 150).unwrap();
    ids.extend(
        c.ids(
            &RunFilter {
                stage: Some(2),
                ..Default::default()
            },
            50,
        )
        .unwrap(),
    );
    let rows = c.rows(&ids).unwrap();
    assert!(rows.iter().any(|r| r.stage == 2), "no confirmation runs");
    let mut fams = std::collections::BTreeSet::new();
    for r in &rows {
        let ev = try_one(r.cand.clone()).unwrap();
        assert_eq!(
            (store::fate_name(ev.fate), ev.bin as i64),
            (r.fate.as_str(), r.bin),
            "run {} ({} on {})",
            r.id,
            r.cand.rule,
            r.cand.tiling
        );
        assert_eq!(store::feat_blob(&ev.feat), r.feats, "run {}", r.id);
        assert_eq!(ev.score as f64, r.score as f32 as f64, "run {}", r.id);
        fams.insert(r.cand.fam.name());
    }
    eprintln!(
        "{} runs ({} families) measured on the GPU match the CPU",
        rows.len(),
        fams.len()
    );
}
