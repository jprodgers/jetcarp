//! Jobs from scripts: labelled jobs found again (a rerun starts nothing new), censuses as jobs,
//! and rule neighbours.

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
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
    // The dashboard shows the label in front of the job's title.
    let lib = Library::open(root.join("lib"));
    let rows = lib.jobs().unwrap();
    let sums =
        jetcarp_search::dashboard::summaries(&rows, jetcarp_search::dashboard::now_seconds());
    assert!(
        sums.iter().any(|s| s.title.starts_with("t · hunt: ")),
        "{:?}",
        sums.iter().map(|s| s.title.clone()).collect::<Vec<_>>()
    );
}
