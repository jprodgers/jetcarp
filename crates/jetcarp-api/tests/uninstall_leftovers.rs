//! `jetcarp uninstall` removes what the host and the job runners leave behind once nothing holds
//! it: the host's 0-byte `host.lock` in the runtime folder, and the empty `<catalog>-jobs` folder
//! beside the catalog. Every folder is a scratch folder; the platform is the headless one, which
//! has nothing of its own to undo.
//!
//! This file is its own test program with one test, because the runtime folder comes from
//! `JETCARP_RUNTIME_DIR`, which it sets.

use jetcarp_api::ops::system::SystemUninstall;
use jetcarp_api::{Ctx, Op, Quiet};
use std::path::PathBuf;

#[test]
fn the_host_lock_and_an_empty_jobs_folder_go_once_nothing_holds_them() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-leftovers");
    let _ = std::fs::remove_dir_all(&d);
    let (runtime, data, settings) = (d.join("runtime"), d.join("data"), d.join("settings"));
    for f in [&runtime, &data, &settings] {
        std::fs::create_dir_all(f).unwrap();
    }
    // SAFETY: this test program runs this one test, before any other thread reads them.
    unsafe {
        std::env::set_var("JETCARP_RUNTIME_DIR", &runtime);
        std::env::set_var("JETCARP_DATA_DIR", &data);
        std::env::remove_var("JETCARP_LIBRARY");
    }
    let lock = runtime.join("host.lock");
    std::fs::write(&lock, b"").unwrap();
    let jobs = data.join("library.sqlite-jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let cx = Ctx::new().with_settings_dir(Some(settings));
    let run = |yes: bool| {
        let a = serde_json::from_value(serde_json::json!({ "yes": yes })).unwrap();
        SystemUninstall::run(&cx, a, &Quiet).unwrap()
    };

    // Listed, not done, without `yes`.
    let plan = run(false);
    let whats: Vec<&str> = plan.steps.iter().map(|s| s.what.as_str()).collect();
    assert_eq!(whats.len(), 2, "{whats:?}");
    assert!(
        whats[0].starts_with("remove the host's lock file"),
        "{whats:?}"
    );
    assert!(
        whats[1].starts_with("remove the empty folder of job locks"),
        "{whats:?}"
    );
    assert!(lock.exists() && jobs.exists());

    // A running host holds the lock: it is kept, and so is the step's success.
    let held = std::fs::OpenOptions::new().write(true).open(&lock).unwrap();
    held.try_lock().unwrap();
    let r = run(true);
    assert_eq!(r.steps[0].ok, Some(true));
    assert!(
        r.steps[0]
            .note
            .as_deref()
            .unwrap_or("")
            .contains("holds it"),
        "{:?}",
        r.steps[0]
    );
    assert!(lock.exists());
    assert!(!jobs.exists(), "the empty jobs folder goes");
    drop(held);

    // Nothing holds it now; a jobs folder with a paused job's lock in it is no leftover.
    std::fs::create_dir_all(&jobs).unwrap();
    std::fs::write(jobs.join("7.lock"), b"").unwrap();
    let r = run(true);
    assert_eq!(r.steps.len(), 1, "{:?}", r.steps);
    assert_eq!((r.steps[0].ok, r.steps[0].note.clone()), (Some(true), None));
    assert!(!lock.exists());
    // The scratch runtime folder is not named `jetcarp`, so it stays even when empty.
    assert!(runtime.exists());
    assert!(jobs.join("7.lock").exists());
    assert!(run(false).steps.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
