//! Large sandpiles from scripts: `sandpile_pile` writes its folder and resumes it;
//! `sandpile_piles` writes a contact sheet.

use jetcarp_script::RunOptions;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[test]
fn sandpile_pile_and_sheet() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jetcarp-script-tests")
        .join("pile");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // Piles are catalog jobs: the test's own catalog, beside its output.
    let o = RunOptions {
        out_root: Some(root.clone()),
        library: Some(root.join("lib")),
        ..Default::default()
    };
    let src = r#"
        let r = sandpile_pile(#{ tiling: "hex", grains: "2^12", centre: "vertex", px: 3, preview: 200 });
        print(`${r.grains} ${r.steps} ${r.topplings > 0} ${exists(r.png)} ${exists(r.preview)}`);
        let again = sandpile_pile(#{ tiling: "hex", grains: 4096, centre: "vertex", folder: r.folder });
        print(`${again.topplings == r.topplings} ${again.sweeps == r.sweeps}`);
        let s = sandpile_piles(#{ tilings: ["square", "triangle", "spectre"], grains: 1500, thumb: 96, cols: 3, png: false });
        print(`${s.runs.len()} ${exists(s.sheet)}`);
    "#;
    let s = jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)));
    assert!(s.ok, "script failed: {:?}\nlog: {:#?}", s.error, s.log);
    assert_eq!(s.log[0], "4096 11 true true true");
    assert!(s.log[1].starts_with("resuming "));
    assert_eq!(s.log[2], "true true");
    assert!(s.log.iter().any(|l| l == "3 true"), "{:?}", s.log);
    let piles = root.join("piles");
    let dirs: Vec<_> = std::fs::read_dir(&piles).unwrap().collect();
    assert_eq!(dirs.len(), 2); // the pile and the sheet folder
}
