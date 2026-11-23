//! A running job waits while JETCARP measures this computer (the JETCARP Hardware Score, #301),
//! and carries on, in the same process, once the measuring has ended.
//!
//! The measuring is said by a record in the runtime folder (`measuring/<pid>.json`), which this
//! test writes itself, as another JETCARP program measuring would. The runtime folder is one of
//! the test's own: this is the only test in its process, so setting the variable is safe.

use jetcarp_core::topology::TilingSpec;
use jetcarp_search::hunt::{self, HuntProgress, HuntSpec};
use jetcarp_search::jobs::{RunOptions, Runner};
use jetcarp_store::Library;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "jetcarp-measuring-test-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn a_running_job_waits_while_jetcarp_measures() {
    let runtime = scratch("runtime");
    // SAFETY: the only test in this process, before any other thread reads the environment.
    unsafe { std::env::set_var("JETCARP_RUNTIME_DIR", &runtime) };
    let lib = Library::open(scratch("library"));
    let spec = HuntSpec {
        rules: vec!["B3/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: None,
        ..Default::default()
    };
    let job = hunt::start(&lib, &spec).unwrap();
    let h = Runner::new(lib.clone())
        .spawn(
            job,
            RunOptions {
                threads: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let soups = || h.progress_as::<HuntProgress>().soups_done;
    let wait = |what: &str, ok: &dyn Fn() -> bool| {
        let t0 = Instant::now();
        while !ok() {
            assert!(t0.elapsed() < Duration::from_secs(60), "timed out: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    wait("the hunt to start", &|| soups() > 0);
    // Another JETCARP program measures (a process that is not this one).
    let dir = runtime.join("measuring");
    std::fs::create_dir_all(&dir).unwrap();
    let record = dir.join("4000001.json");
    // Written as JETCARP writes it (whole, then moved into place): a record rewritten in place
    // can be read empty between its truncation and its writing, and a job reading it then saw no
    // hold and took a step (macOS CI).
    let write = || {
        jetcarp_core::machine::quiet::write_in(&dir, 4000001, "jetcarp score", now_ms()).unwrap()
    };
    // Unheld, the hunt's progress moves on with each step: how long one takes on this machine
    // (about three seconds here, longer on a slow runner) sets every wait below.
    let before = soups();
    wait("the hunt's next step", &|| soups() > before);
    let (t1, s1) = (Instant::now(), soups());
    wait("another step", &|| soups() > s1);
    let step = t1.elapsed().max(Duration::from_secs(1));
    // The record goes stale after ten seconds, so it is written again every second.
    let hold_for = |d: Duration| {
        let t = Instant::now();
        while t.elapsed() < d {
            write();
            std::thread::sleep(Duration::from_millis(500).min(d.saturating_sub(t.elapsed())));
        }
    };
    write();
    // A step under way ends first: wait until the count has stood still for two steps' time.
    let (t0, mut last, mut still) = (Instant::now(), soups(), Instant::now());
    while still.elapsed() < step * 2 {
        assert!(
            t0.elapsed() < Duration::from_secs(120),
            "the hunt never held"
        );
        hold_for(Duration::from_millis(500));
        let now = soups();
        if now != last {
            (last, still) = (now, Instant::now());
        }
    }
    let a = soups();
    // Long enough for three steps, had it run on.
    hold_for(step * 3);
    let b = soups();
    assert_eq!(a, b, "the hunt ran on while JETCARP measured");
    // The measuring ends: the hunt carries on in this process, as it was.
    std::fs::remove_file(&record).unwrap();
    wait("the hunt to carry on", &|| soups() > b);
    assert_eq!(h.progress_as::<HuntProgress>().status, "running");
    h.stop();
    let _ = h.wait();
}
