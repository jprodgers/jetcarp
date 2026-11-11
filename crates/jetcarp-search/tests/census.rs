//! A census as a catalog job on the job runner: stopped and resumed, it gives the same report as
//! a census run straight through. (The census itself is tested in jetcarp-core.)

use jetcarp_core::census::*;
use jetcarp_core::*;
use jetcarp_search::census::{CensusProgress, job_report, job_status, start_job};
use jetcarp_search::jobs::census::CensusRun;
use jetcarp_search::jobs::{Budget, RunOptions, Runner};

/// Run (or resume) a census job here on the job runner: `chunk` rules per chunk (0: adapt),
/// stopping (paused) after `limit` rules in this session.
fn run_job(
    lib: &jetcarp_core::library::Library,
    job: i64,
    chunk: u64,
    limit: Option<u64>,
) -> CensusProgress {
    let o = RunOptions {
        budget: Budget {
            items: limit,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut s = CensusRun::default();
    s.chunk = chunk;
    Runner::new(lib.clone())
        .run_with(job, &mut s, &o)
        .unwrap()
        .progress_as()
}

#[test]
fn census_jobs_resume_to_the_same_report() {
    use jetcarp_core::library::{JobStatus, Library};
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-census-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = Library::open(&dir);
    let mut s = spec(&[], 2, 300);
    s.rules = all_bs_rules(&[2, 3, 6], &[2, 3], None).unwrap();
    s.rules.push("nonsense".into());
    let straight = run_census(&s, &CensusOptions::default(), None).unwrap();
    let job = start_job(&lib, &s).unwrap();
    for limit in [5, 7] {
        let p = run_job(&Library::open(&dir), job, 3, Some(limit));
        assert_eq!(p.status, "paused");
    }
    assert_eq!(lib.job(job).unwrap().status, JobStatus::Paused);
    assert_eq!(job_report(&lib, job).unwrap().rules.len(), 12);
    let p = run_job(&Library::open(&dir), job, 0, None);
    assert_eq!((p.status.as_str(), p.rules_done), ("done", 33));
    let r = job_report(&lib, job).unwrap();
    let mut a = straight.report.clone();
    a.jetcarp_version.clear();
    let mut b = r.clone();
    b.jetcarp_version.clear();
    assert_eq!(a.to_json(), b.to_json());
    assert_eq!(job_status(&lib, job).unwrap().1.status, "done");
}

fn spec(rules: &[&str], soups: u32, gens: u64) -> CensusSpec {
    CensusSpec {
        tiling: TilingSpec::rect("square", 24, 24, Boundary::Wrap),
        rules: rules.iter().map(|s| s.to_string()).collect(),
        soups,
        gens,
        ..Default::default()
    }
}
