//! HashLife from scripts: `w.jump(n)` equals `w.step(n)` on states; `hashlife(w, n)` equals
//! stepping a dead-edged world the pattern never reaches.

use jetcarp_script::{RunOptions, Screen};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[test]
fn hashlife_jump_and_plane() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jetcarp-script-tests")
        .join("hashlife");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let o = RunOptions {
        out_root: Some(root.clone()),
        screen: Screen {
            width: 320,
            height: 200,
            cell_px: 8.0,
        },
        ..Default::default()
    };
    let src = r#"
        let a = world(#{ tiling: "square", size: [64, 32], boundary: "wrap", rng_seed: 3 });
        a.add_ca("B3/S23", #{ seed: "random:0.35" });
        let b = a.copy();
        a.step(300);
        b.jump("300");
        print(`${a.states_hash() == b.states_hash()} ${b.generation()} ${b.can_jump()}`);
        let d = world(#{ tiling: "square", size: [80, 80], boundary: "dead" });
        d.add_ca("B3/S23", #{ seed: "empty" });
        d.set_cells([[40, 40, 1], [41, 40, 1], [39, 41, 1], [40, 41, 1], [40, 42, 1]]);
        let p = hashlife(d, 30);
        d.step(30);
        print(`${p.states_hash() == d.states_hash()} ${p.width()} ${d.can_jump() != true}`);
    "#;
    let s = jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)));
    assert!(s.ok, "script failed: {:?}\nlog: {:#?}", s.error, s.log);
    assert_eq!(s.log[0], "true 300 true");
    assert_eq!(s.log[1], "true 80 true");
}
