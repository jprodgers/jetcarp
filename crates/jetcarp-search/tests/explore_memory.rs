//! An explore search's memory, read while a step runs (#129) and acted on only when it stays over
//! the cap (#81). Runs on the processor here hold memory of their own while they run
//! ([`HOLD_MB`]); this test binary's process is the one measured, so its tests run one at a time.

use jetcarp_search::explore::job::{HOLD_MB, HOLD_RUNS, SLOW_GENERATION_MS};
use jetcarp_search::jobs::explore::{CAP_HOLD_S, ExploreProgress, rss_gb};
use jetcarp_search::jobs::{self, Budget, JobSpec, RunOptions, Runner, Stopped};
use jetcarp_store::{JobStatus, Library};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// The tests share the process's memory and the hooks: one at a time.
static ONE: Mutex<()> = Mutex::new(());

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("explore-memory-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// A step of slow runs lasts a minute (16 runs of 8 s on two threads), and each run holds 300 MB
/// while it runs, so the memory is 600 MB over its start for most of the step. The job's status
/// shows that while the step runs, and with a cap below it the job stops within seconds of
/// [`CAP_HOLD_S`], not at the step's end; the memory at the step's start was under the cap, so
/// before (one reading a step) the search ran on until its budget.
#[test]
fn a_long_step_reads_its_memory_and_stops_when_it_stays_over_the_cap() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("lasting");
    let lib = Library::open(dir.join("library.sqlite"));
    let base = rss_gb();
    assert!(base > 0.0, "no memory reading here");
    let cap = ((base + 0.3) * 100.0).round() / 100.0;
    SLOW_GENERATION_MS.store(40, Ordering::Relaxed);
    HOLD_MB.store(300, Ordering::Relaxed);
    HOLD_RUNS.store(i64::MAX, Ordering::Relaxed);
    let job = make(
        &lib,
        json!({
            "tilings": ["square", "hex"], "families": ["life_like", "generations"],
            "screen_cells": 400, "screen_gens": 1000, "confirm_cells": 400,
            "confirm_gens": 1000, "confirm_seeds": 2, "seed": 4, "max_rss_gb": cap,
        }),
    );
    let t0 = Instant::now();
    let h = Runner::new(Library::open(lib.path()))
        .spawn(
            job,
            RunOptions {
                threads: 2,
                budget: Budget::minutes(1.5),
                ..Default::default()
            },
        )
        .unwrap();
    // The memory the job's status shows while it runs, as another process reads it.
    let other = Library::open(lib.path());
    let mut shown: f64 = 0.0;
    while !h.is_finished() {
        let row = other.job(job).unwrap();
        if row.status == JobStatus::Running {
            shown = shown.max(progress(&row.progress).rss_gb);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = h.wait().unwrap();
    let took = t0.elapsed().as_secs_f64();
    HOLD_MB.store(0, Ordering::Relaxed);
    SLOW_GENERATION_MS.store(0, Ordering::Relaxed);
    let p = progress(&out.progress);
    assert!(
        shown > cap,
        "the status showed at most {shown:.2} GB while the runs held {:.2} GB (cap {cap})",
        p.rss_peak_gb
    );
    assert_eq!(
        (out.status, out.stopped),
        (JobStatus::Paused, Some(Stopped::Asked)),
        "{p:?}"
    );
    let note = p.note.clone().unwrap_or_default();
    assert!(note.contains("over the cap"), "{note:?}");
    assert!(
        (CAP_HOLD_S..CAP_HOLD_S + 20.0).contains(&took),
        "stopped after {took:.1} s"
    );
    assert!(p.rss_peak_gb > cap, "{p:?}");
}

/// Memory the runs have freed but the allocator keeps (here every free is kept: the allocator is
/// told not to give any back by itself) puts the job's reading over its cap for the rest of the
/// run. The job gives it back and reads again before it acts, so it runs on to its budget; before,
/// the reading at the next step's start paused it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn freed_memory_over_the_cap_is_given_back_rather_than_pausing_the_search() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: mallopt only changes glibc's thresholds; it is safe to call at any time.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 1 << 30);
        libc::mallopt(libc::M_TRIM_THRESHOLD, i32::MAX);
    }
    let dir = scratch("freed");
    let lib = Library::open(dir.join("library.sqlite"));
    let base = rss_gb();
    let cap = ((base + 0.2) * 100.0).round() / 100.0;
    // The first runs hold 200 MB each, then free it.
    HOLD_MB.store(200, Ordering::Relaxed);
    HOLD_RUNS.store(6, Ordering::Relaxed);
    let job = make(
        &lib,
        json!({
            "tilings": ["square", "hex"], "families": ["life_like", "generations"],
            "screen_cells": 400, "screen_gens": 48, "confirm_cells": 900,
            "confirm_gens": 64, "confirm_seeds": 2, "seed": 4, "max_rss_gb": cap,
        }),
    );
    let out = Runner::new(lib.clone())
        .run(
            job,
            &RunOptions {
                threads: 2,
                budget: Budget::minutes(0.2),
                ..Default::default()
            },
        )
        .unwrap();
    HOLD_MB.store(0, Ordering::Relaxed);
    let p = progress(&out.progress);
    assert_eq!(out.stopped, Some(Stopped::Budget), "{p:?}");
    assert_eq!(p.note, None);
    assert!(p.rss_gb <= cap, "{p:?}");
    assert!(p.screened > 0);
}
