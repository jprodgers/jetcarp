//! Media and the website export from scripts: `lib.media`, `media`, `w.media`,
//! `lib.site_export`.

use jetcarp_script::RunOptions;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[test]
fn scripts_render_media_and_export_a_site() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jetcarp-script-tests")
        .join("media");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let o = RunOptions {
        out_root: Some(root.clone()),
        library: Some(root.join("lib")),
        ..Default::default()
    };
    let src = r#"
        let lib = library();
        hunt(#{ rules: ["B3/S23"], soups: 80, seed: 3 });
        let ship = lib.query(#{ class: "spaceship", limit: 1 })[0];
        let r = lib.media(#{ pattern: ship.id });
        let f = r.done[0].files;
        print(`first ${r.written} ${exists(f.anim)} ${exists(f.poster)} ${exists(f.thumb)} ${f.anim.ends_with(".gif")}`);
        let again = media(#{ pattern: ship.id });
        print(`again ${again.written} ${again.skipped}`);
        let w = world(#{ tiling: "hex", size: [40, 30], boundary: "wrap", cas: [#{ rule: "B2/S34", seed: "random:0.3" }] });
        let m = w.media(#{ gens: 12 });
        print(`world ${m.frames_count} ${exists(m.anim)}`);
        lib.update(ship.id, #{ state: "promoted" });
        let s = lib.site_export(#{ to: "site", quickstart: "none" });
        print(`site ${s.patterns} ${s.written.len() > 3}`);
        let s2 = lib.site_export(#{ to: "site", quickstart: "none" });
        print(`site again ${s2.written.len()} ${s2.unchanged}`);
    "#;
    let s = jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)));
    assert!(s.ok, "script failed: {:?}\nlog: {:#?}", s.error, s.log);
    for want in [
        "first 1 true true true true",
        "again 0 1",
        "world 13 true",
        "site 1 true",
        "site again 0 1",
    ] {
        assert!(s.log.iter().any(|l| l == want), "{want}: {:?}", s.log);
    }
}
