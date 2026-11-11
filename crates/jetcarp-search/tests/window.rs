//! Movers on a sliding window as catalog jobs: a run stopped at a checkpoint and resumed ends
//! exactly as an uninterrupted one, and so does the job on the job runner. (The window itself is
//! tested in jetcarp-core.)

use jetcarp_core::evolve::arena::deepest;
use jetcarp_core::pattern::movers::{Stop, TrackSpec, track};
use jetcarp_core::pattern::site::patch_topology;
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};
use jetcarp_core::window::track::{Tracker, WindowOpts};

// A run stopped at a checkpoint and resumed (through JSON, as a job stores it) ends exactly as
// an uninterrupted one; the catalog job does the same and stores the mover with its flag.
#[test]
fn window_runs_resume_exactly() {
    use jetcarp_core::window::track::WindowCheckpoint;
    let mut spec = spectre_glider();
    spec.max_gens = 9_000;
    let opts = WindowOpts::default();
    let mut whole = Tracker::new(&spec, &opts).unwrap();
    whole.run(None, None).unwrap();
    let mut a = Tracker::new(&spec, &opts).unwrap();
    while a.generation() < 5_000 || a.checkpoint().is_none() {
        a.step_once().unwrap();
    }
    let cp = a.checkpoint().unwrap();
    let txt = serde_json::to_string(&cp).unwrap();
    let cp: WindowCheckpoint = serde_json::from_str(&txt).unwrap();
    let mut b = Tracker::resume(&spec, &opts, &cp).unwrap();
    assert_eq!(b.live_keys(), a.live_keys());
    b.run(None, None).unwrap();
    let (rw, rb) = (whole.report(), b.report());
    assert_eq!(b.live_keys(), whole.live_keys());
    assert_eq!(rb.generations, 9_000);
    assert_eq!(rb.reach, rw.reach);
    assert_eq!(
        rb.window.as_ref().unwrap().frame,
        rw.window.as_ref().unwrap().frame
    );
    assert_eq!(rb.stop, Stop::GenLimit);

    // As a catalog job on the job runner, stopped by request on the way and resumed.
    use jetcarp_search::jobs::{RunOptions, Runner, mover::MoverRun};
    use jetcarp_search::window::{MoverJobSpec, MoverProgress, start};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-window-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let lib = jetcarp_core::library::Library::open(&d);
    let js = MoverJobSpec {
        track: spec.clone(),
        window: opts.clone(),
        store: true,
        source: "test".into(),
    };
    let job = start(&lib, &js).unwrap();
    // Stopped before its first step, it pauses with nothing run.
    let stop = Arc::new(AtomicBool::new(true));
    let o = RunOptions {
        stop: Some(stop.clone()),
        ..Default::default()
    };
    let p: MoverProgress = Runner::new(lib.clone()).run(job, &o).unwrap().progress_as();
    assert_eq!((p.status.as_str(), p.generation), ("paused", 0), "{p:?}");
    assert!(lib.job_checkpoint(job).unwrap().is_none());
    // In short steps, stopped once a step has gone past the repeat check's 4,096 generations
    // (its step ends at a new window, which it checkpoints): it pauses there.
    stop.store(false, Ordering::SeqCst);
    let s2 = stop.clone();
    let mut run = MoverRun::default();
    run.seconds = 0.02;
    run.on_progress = Some(Arc::new(move |p: &MoverProgress| {
        if p.status == "running" && p.generation > 4096 {
            s2.store(true, Ordering::SeqCst);
        }
    }));
    let h = Runner::new(lib.clone())
        .spawn_with(job, Box::new(run), o)
        .unwrap();
    let p: MoverProgress = h.wait().unwrap().progress_as();
    assert_eq!(p.status, "paused", "{p:?}");
    assert!(p.generation > 4096 && p.generation < 9_000, "{p:?}");
    assert!(lib.job_checkpoint(job).unwrap().is_some());
    let p: MoverProgress = Runner::new(lib.clone())
        .spawn(job, RunOptions::default())
        .unwrap()
        .wait()
        .unwrap()
        .progress_as();
    assert_eq!(p.status, "done");
    let rep = p.report.unwrap();
    assert_eq!(rep.generations, 9_000);
    assert_eq!(rep.reach, rw.reach);
    let rec = lib.get(p.record.as_ref().unwrap()).unwrap();
    let m = rec.mover.unwrap();
    assert_eq!(m.distance_rings, rw.reach.rings);
    assert!(m.window.unwrap().sliding);
    assert!(rec.verified.field.contains("sliding window"));
}

/// The published glider pair (states 1 and 3) near the deepest tile of a Spectre level-4 patch.
fn spectre_glider() -> TrackSpec {
    let patch = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let topo = patch_topology(&patch).unwrap();
    let (d, _) = deepest(&topo);
    let mut roots: Vec<usize> = vec![d];
    topo.for_each_neighbor(d, Nb::Edge, |y| roots.push(y));
    for r in roots {
        for k in 0..topo.degree(r, Nb::Edge) {
            let u = topo.slot(r, Nb::Edge, k).unwrap();
            let mut spec = TrackSpec::new(
                "SpectreGlider",
                Nb::Touch,
                patch.clone(),
                vec![(r as u32, 1), (u as u32, 3)],
            );
            spec.max_level = Some(5);
            if let Ok(rep) = track(&spec)
                && rep.verified
            {
                spec.max_level = None;
                return spec;
            }
        }
    }
    panic!("no glider");
}
