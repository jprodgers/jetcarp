//! An explore search's time with slow runs under way on the processor: a Quick Start build ends
//! within its minutes and a little more, a step that lasts several seconds writes the job's
//! progress while it runs, and a pause asked by another process stops it within seconds. Every
//! run on the processor here waits [`SLOW_GENERATION_MS`] at each generation (this test binary's
//! own setting), so its runs take tens of seconds and a step lasts until their time cap.

use jetcarp_core::explore::batch::{Backend, BatchJob, CpuBatch, GIVEN_UP, WorldTrace, run_cpu};
use jetcarp_core::rules::Rule;
use jetcarp_search::explore::job::{CPU_RUN_S, SLOW_GENERATION_MS};
use jetcarp_search::jobs::explore::{ExploreProgress, ExploreRun, PUBLISH_S};
use jetcarp_search::jobs::{self, Budget, JobSpec, RunOptions, Runner, Stopped};
use jetcarp_store::{JobStatus, Library};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// A GPU whose every batch takes a minute, its pace misjudged however it is sized; it asks
/// `cut` between its parts, as the GPU's batch stepper does between submissions.
struct SlowGpu;

impl Backend for SlowGpu {
    fn name(&self) -> String {
        "slow test batches".into()
    }
    fn supports(&self, rule: &Rule) -> bool {
        CpuBatch.supports(rule)
    }
    fn max_cells(&self) -> usize {
        1 << 20
    }
    fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
        self.run_until(job, &|| false)
    }
    fn run_until(
        &mut self,
        job: &BatchJob,
        cut: &dyn Fn() -> bool,
    ) -> Result<Vec<WorldTrace>, String> {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            if cut() {
                return Err(GIVEN_UP.into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        run_cpu(job)
    }
}

/// Each generation of a run on the processor waits this long: a run of 1,000 generations takes
/// 40 s, so every run is still going when a build's few seconds end, and a step runs until the
/// runs' time cap.
const SLOW_MS: u64 = 40;

fn scratch(name: &str) -> PathBuf {
    SLOW_GENERATION_MS.store(SLOW_MS, Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("explore-deadline-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A search of small grids whose runs are long (1,000 generations, slowed down).
fn slow(extra: Value) -> Value {
    let mut v = json!({
        "tilings": ["square", "hex"],
        "families": ["life_like", "generations"],
        "screen_cells": 400, "screen_gens": 1000,
        "confirm_cells": 400, "confirm_gens": 1000, "confirm_seeds": 2,
        "seed": 11,
    });
    if let (Some(a), Some(b)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in b {
            a.insert(k.clone(), x.clone());
        }
    }
    v
}

fn make(lib: &Library, spec: Value) -> i64 {
    jobs::create(
        lib,
        &JobSpec {
            strategy: "explore".into(),
            spec,
            label: None,
            budget: Budget::default(),
        },
    )
    .unwrap()
}

fn progress(v: &Value) -> ExploreProgress {
    serde_json::from_value(v.clone()).unwrap()
}

/// A three-second build ends on time with every run on the processor still going: they are
/// given up (not kept), and the build publishes and ends seconds after its minutes, not after
/// the runs' time cap.
#[test]
fn a_build_ends_on_time_with_slow_runs_under_way() {
    let dir = scratch("build");
    let lib = Library::open(dir.join("library.sqlite"));
    let budget_s = 3.0;
    let job = make(
        &lib,
        slow(json!({ "build": { "minutes": budget_s / 60.0 } })),
    );
    let t0 = Instant::now();
    let out = Runner::new(lib.clone())
        .run(
            job,
            &RunOptions {
                threads: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let wall = t0.elapsed().as_secs_f64();
    assert_eq!(out.status, JobStatus::Done);
    let p = progress(&out.progress);
    // The search's own time: its minutes, and at most a generation and a step's write more.
    assert!(
        p.elapsed_s >= budget_s && p.elapsed_s < budget_s + 1.5,
        "searched {:.2} s",
        p.elapsed_s
    );
    // From the search's start to the job's end, publishing included: well before the runs' cap
    // would have ended them. (The wall time adds loading the tiling list, slower on some
    // machines than others.)
    assert!(
        p.session_s < budget_s + 3.0 && p.session_s < CPU_RUN_S,
        "the build took {:.2} s from its search's start, {wall:.2} s in all",
        p.session_s
    );
    assert!(p.given_up > 0, "no run was under way at the end");
    assert_eq!((p.screened, p.capped), (0, 0));
    assert!(p.published.is_some());
}

/// A build on a GPU whose batch would run a minute ends on time all the same: the batch under
/// way at the end is given up between two of its parts, its runs not recorded.
#[test]
fn a_build_ends_on_time_with_a_gpu_batch_under_way() {
    let dir = scratch("gpu");
    let lib = Library::open(dir.join("library.sqlite"));
    let budget_s = 3.0;
    // Families the GPU runs only, so the GPU's batches are all there is.
    let job = make(
        &lib,
        slow(json!({ "gpu": true, "build": { "minutes": budget_s / 60.0 } })),
    );
    let mut run = ExploreRun::with_backend(Box::new(SlowGpu));
    let out = Runner::new(lib.clone())
        .run_with(
            job,
            &mut run,
            &RunOptions {
                threads: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.status, JobStatus::Done);
    let p = progress(&out.progress);
    assert_eq!(p.backend, "slow test batches");
    assert!(
        p.elapsed_s >= budget_s && p.elapsed_s < budget_s + 1.5,
        "searched {:.2} s",
        p.elapsed_s
    );
    assert!(p.session_s < budget_s + 3.0, "took {:.2} s", p.session_s);
    assert!(p.given_up > 0 && p.screened == 0, "{p:?}");
    assert!(p.gpu_error.is_none(), "{:?}", p.gpu_error);
    // Its threads: a third for the GPU's batches.
    assert_eq!((p.threads, p.gpu_threads), (1, 1));
}

/// A step lasting several seconds (its runs go on to their time cap) writes the job's progress
/// every couple of seconds meanwhile, its time moving though no run has ended; and a pause asked
/// by another process (`jobs stop`) stops the step within seconds, its runs given up.
#[test]
fn a_long_step_writes_its_progress_and_sees_a_pause() {
    let dir = scratch("progress");
    let lib = Library::open(dir.join("library.sqlite"));
    let job = make(&lib, slow(json!({})));
    let h = Runner::new(Library::open(lib.path()))
        .spawn(
            job,
            RunOptions {
                threads: 2,
                budget: Budget::default(),
                ..Default::default()
            },
        )
        .unwrap();
    // Read as another process would: the job's row in the library, from when the search has
    // started (its first step's runs then end at their cap, CPU_RUN_S later).
    let other = Library::open(lib.path());
    let wait = Instant::now();
    while other.job(job).unwrap().status != JobStatus::Running {
        assert!(wait.elapsed() < Duration::from_secs(60), "it did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let t0 = Instant::now();
    let mut seen: Vec<f64> = Vec::new();
    // Until three progress writes (the first at the start) or a little before the runs' cap.
    while seen.len() < 3 && t0.elapsed() < Duration::from_secs_f64(CPU_RUN_S - 1.5) {
        let row = other.job(job).unwrap();
        let p = progress(&row.progress);
        if seen.last().is_none_or(|&e| p.elapsed_s > e) {
            // Mid-step: no run has ended yet (each runs to its cap), so none is counted.
            assert_eq!(p.screened, 0, "a run ended at {:.1} s", p.elapsed_s);
            seen.push(p.elapsed_s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        seen.len() >= 3 && seen.last().copied().unwrap_or(0.0) >= 2.0 * PUBLISH_S - 0.5,
        "the progress written during the step: {seen:?}"
    );
    let p = progress(&other.job(job).unwrap().progress);
    assert!(p.threads > 0 && p.cores > 0, "{p:?}");
    // Paused from elsewhere, as `jobs stop` does: the step stops at its next check, before the
    // runs' cap (a run kept at its cap would mean the pause waited for it).
    let asked = Instant::now();
    jobs::kind("explore").request_stop(&other, job).unwrap();
    let out = h.wait().unwrap();
    let took = asked.elapsed().as_secs_f64();
    assert!(took < PUBLISH_S + 2.0, "the pause took {took:.2} s");
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked))
    );
    let p = progress(&out.progress);
    assert!(p.given_up > 0 && p.capped == 0, "{p:?}");
}
