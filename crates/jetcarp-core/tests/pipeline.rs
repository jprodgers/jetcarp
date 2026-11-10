//! The catalog pipeline's job state (`jetcarp_core::pipeline`) and how the dashboard shows it and
//! the labels of its child jobs.

use jetcarp_core::dashboard::{self, summaries};
use jetcarp_core::library::{JobStatus, Library};
use jetcarp_core::pipeline::{self, PipelineSpec, STEPS};
use std::path::PathBuf;

fn temp(name: &str) -> Library {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("pipeline-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    Library::open(p.join("catalog.sqlite"))
}

#[test]
fn pipeline_state_round_trips_and_shows_on_the_dashboard() {
    let lib = temp("state");
    let spec = PipelineSpec {
        mode: "quick".into(),
        ..Default::default()
    };
    let job = pipeline::start(&lib, &spec).unwrap();
    let (spec, mut p) = pipeline::load(&lib, job).unwrap();
    assert_eq!(spec.label, format!("pipeline {job}"));
    assert_eq!(p.steps.len(), STEPS.len());
    assert!(p.steps.iter().all(|s| s.status == "pending"));
    assert_eq!(p.status, "paused");
    // Two steps done, the third running.
    p.steps[0].status = "done".into();
    p.steps[0].seconds = 75.0;
    p.steps[0].jobs = vec![7, 8];
    p.steps[1].status = "skipped".into();
    p.steps[1].note = "model not reachable".into();
    p.steps[2].status = "running".into();
    p.step = 2;
    pipeline::save(&lib, job, &p, JobStatus::Running).unwrap();
    let (_, q) = pipeline::load(&lib, job).unwrap();
    assert_eq!(q.steps, p.steps);
    assert_eq!(q.status, "running");
    assert_eq!(q.done(), 1);
    // A labelled child job and the pipeline itself in the job list.
    let child = lib
        .create_job("hunt", &serde_json::json!({ "rules": ["B3/S23"] }))
        .unwrap();
    lib.set_job_label(child, &format!("{} · stage 2 · Square (touch)", spec.label))
        .unwrap();
    assert_eq!(
        lib.jobs_labelled(&format!("{} · stage 2 · Square (touch)", spec.label))
            .unwrap()
            .len(),
        1
    );
    let rows = lib.jobs().unwrap();
    let s = summaries(&rows, dashboard::now_seconds());
    let pipe = s.iter().find(|x| x.id == job).unwrap();
    assert_eq!(pipe.kind_label, "Catalog pipeline");
    assert_eq!(
        pipe.title,
        format!("pipeline {job}: quick mode, step 3: score refit")
    );
    assert_eq!((pipe.done, pipe.total), (1, Some(6)));
    assert!(
        pipe.counters
            .iter()
            .any(|(k, v)| k == "stage 1 settings search" && v.contains("jobs 7 8"))
    );
    assert!(
        pipe.counters
            .iter()
            .any(|(_, v)| v.contains("model not reachable"))
    );
    let hunt = s.iter().find(|x| x.id == child).unwrap();
    assert!(
        hunt.title
            .starts_with(&format!("pipeline {job} · stage 2 · Square (touch)")),
        "{}",
        hunt.title
    );
    // Stop from another handle marks it paused; the newest unfinished one is found.
    pipeline::request_stop(&Library::open(lib.path().to_path_buf()), job).unwrap();
    assert_eq!(pipeline::load(&lib, job).unwrap().1.status, "paused");
    assert_eq!(pipeline::latest_unfinished(&lib).unwrap(), Some(job));
    pipeline::save(&lib, job, &q, JobStatus::Done).unwrap();
    assert_eq!(pipeline::latest_unfinished(&lib).unwrap(), None);
}
