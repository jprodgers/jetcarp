//! The catalog pipeline's scripts: `crates/jetcarp-script/scripts/stage1-all.rhai` and `stage2-hunt.rhai` in
//! test mode, labelled jobs found again (a rerun starts nothing new), censuses as jobs, and rule
//! neighbours.

use jetcarp_core::library::Library;
use jetcarp_script::{RunOptions, Screen, Summary};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn temp_root(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jetcarp-script-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn opts(root: &Path) -> RunOptions {
    RunOptions {
        out_root: Some(root.to_path_buf()),
        screen: Screen {
            width: 320,
            height: 200,
            cell_px: 8.0,
        },
        library: Some(root.join("lib")),
        threads: Some(4),
        ..Default::default()
    }
}

fn ok(src: &str, o: RunOptions) -> Summary {
    let s = jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)));
    assert!(s.ok, "script failed: {:?}\nlog: {:#?}", s.error, s.log);
    s
}

fn script(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join(name),
    )
    .unwrap()
}

#[test]
fn labelled_jobs_census_jobs_and_rule_neighbours() {
    let root = temp_root("labelled-jobs");
    let s = ok(
        r#"
        let lib = library();
        let a = lib.hunt(#{ label: "t · hunt", rules: ["B3/S23"], soups: 20 });
        let b = lib.hunt(#{ label: "t · hunt", rules: ["B36/S23"], soups: 50 });
        print(`hunt ${a.job == b.job} ${b.soups_done}`);
        let c = lib.census(#{ label: "t · census", tiling: "hex", birth: "2", survive: "3-4",
                              soups: 2, gens: 100, limit: 2 });
        print(`census ${c.status} ${c.rules_done}/${c.rules_total}`);
        let d = lib.census(#{ label: "t · census" });
        print(`resumed ${d.job == c.job} ${d.status} ${d.rules_done}`);
        let rows = lib.census_rows(d.job);
        print(`rows ${rows.len()} ${rows[0].results == ()}`);
        print(`jobs ${lib.jobs(#{ prefix: "t · " }).len()} ${lib.jobs(#{ label: "t · hunt" })[0].kind}`);
        print(`nb ${rule_neighbours("B3/S23").len()} ${rule_neighbours("B2/S34", #{ tiling: "hex" }).len()}`);
        let e = rule_neighbours("B2/S3V", #{ tiling: "square" });
        print(`edge ${e.len()} ${e.contains("B12/S3V")} ${e.contains("B2/S03V")}`);
        let s1 = lib.stage1(#{ label: "t · s1", tilings: "hex", birth: "2", survive: "3", gens: 50,
                               soups: 1, refine: false, neighbours: false });
        let s2 = lib.stage1(#{ label: "t · s1" });
        print(`stage1 ${s1.job == s2.job} ${s2.status}`);
        "#,
        opts(&root),
    );
    for want in [
        "hunt true 20",
        "census paused 2/8",
        "resumed true done 8",
        "rows 8 true",
        "jobs 2 hunt",
        "nb 17 13",
        "edge 9 true true",
        "stage1 true done",
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
    // The dashboard shows the label in front of the job's title.
    let lib = Library::open(root.join("lib"));
    let rows = lib.jobs().unwrap();
    let sums = jetcarp_core::dashboard::summaries(&rows, jetcarp_core::dashboard::now_seconds());
    assert!(
        sums.iter().any(|s| s.title.starts_with("t · hunt: ")),
        "{:?}",
        sums.iter().map(|s| s.title.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn stage_scripts_cover_every_group_and_resume_by_label() {
    let root = temp_root("stage-scripts");
    let one = script("stage1-all.rhai");
    let two = script("stage2-hunt.rhai");
    let run = |src: &str| {
        let mut o = opts(&root);
        o.params.insert("mode".into(), "test".into());
        o.params.insert("label".into(), "p".into());
        ok(src, o)
    };
    let s = run(&one);
    let lib = Library::open(root.join("lib"));
    let jobs = lib.jobs().unwrap();
    let labels: Vec<String> = jobs
        .iter()
        .filter_map(|j| j.label().map(String::from))
        .collect();
    for g in [
        "squares",
        "hexagons",
        "triangles",
        "archimedean",
        "kuniform",
        "isohedral",
    ] {
        assert!(
            labels.contains(&format!("p · stage 1 · {g}")),
            "{g}: {labels:?}\n{:?}",
            s.log
        );
    }
    assert!(labels.contains(&"p · stage 1 · aperiodic · spectre".to_string()));
    assert!(
        jobs.iter()
            .filter(|j| j.id != 0)
            .all(|j| j.status == jetcarp_core::library::JobStatus::Done)
    );
    let n1 = jobs.len();
    // Stage 2: one hunt per space with settings above the test threshold, and a soup search on
    // the Spectre patch.
    run(&two);
    let jobs = lib.jobs().unwrap();
    let hunts: Vec<_> = jobs
        .iter()
        .filter(|j| j.label().is_some_and(|l| l.starts_with("p · stage 2 · ")))
        .collect();
    assert!(hunts.iter().any(|j| j.kind == "hunt"));
    assert!(hunts.iter().any(|j| j.kind == "search"));
    assert!(
        hunts
            .iter()
            .any(|j| j.label() == Some("p · stage 2 · Hexagon (touch)"))
    );
    let n2 = jobs.len();
    assert!(n2 > n1);
    // Rerunning both changes nothing: every job is found by its label.
    run(&one);
    run(&two);
    assert_eq!(lib.jobs().unwrap().len(), n2);
}
