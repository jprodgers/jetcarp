//! The folder rules with symbolic links and `--allow`: a link inside the output folder is
//! followed before the rules are checked, so a linked folder, a linked file (whether its target
//! exists or not) and `..` after a link cannot reach outside; a refusal reads the same whether or
//! not the path outside exists; and a folder given with `--allow` is read, written and listed
//! (`files_list` too). A folder the tools may use but cannot write in says "Permission denied",
//! and a refusal names the path asked for, not a file inside it (#65).
//!
//! The allowed folders are set before this binary's first tool call, which is why these tests have
//! a binary of their own.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The scratch folder, with `allowed/` and `read-only/` given to the tools as `--allow` would,
/// and `outside/` that they may not touch.
fn root() -> &'static Path {
    static R: OnceLock<PathBuf> = OnceLock::new();
    R.get_or_init(|| {
        let root = setup("folders").to_path_buf();
        for d in ["allowed", "read-only", "outside/dir"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        jetcarp_mcp::tools::allow_dirs(vec![root.join("allowed"), root.join("read-only")]);
        root
    })
}

fn new() -> (Server, Session, &'static Path) {
    let r = root();
    let (mut s, mut h) = (Server::new(), Session::new());
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"width": 16, "height": 16, "cas": ["Life"]}),
    ));
    (s, h, r)
}

/// A symbolic link at `link` to `target` (made afresh).
#[cfg(unix)]
fn link(target: &Path, link: &Path) {
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(unix)]
#[test]
fn a_linked_folder_inside_the_output_folder_does_not_lead_outside() {
    let (mut s, mut h, root) = new();
    let outside = root.join("outside/dir");
    link(&outside, &root.join("out/to-outside"));
    let writes = [
        (
            "world_status",
            json!({"save_path": "to-outside/scene.json"}),
        ),
        (
            "world_render",
            json!({"max_side": 64, "save_path": "to-outside/pic.png"}),
        ),
        (
            "world_render",
            json!({"wallpaper": {"size": "320x200", "out": "to-outside"}}),
        ),
        (
            "media_render",
            json!({"quickstart": "life", "out": "to-outside/media"}),
        ),
        ("pack", json!({"site": {"to": "to-outside/site"}})),
    ];
    for (tool, args) in writes {
        let e = fails(&call(&mut s, &mut h, tool, args.clone()));
        assert!(
            e.contains("the MCP tools may only write inside"),
            "{tool} {args}: {e}"
        );
        // The refusal names the path as given, not where the link leads.
        assert!(!e.contains("outside/dir"), "{tool} {args}: {e}");
    }
    let e = ok(&call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "try { write_text(\"to-outside/a.txt\", \"x\"); print(\"wrote\"); } catch (e) { print(`refused ${e}`); }"}),
    ));
    assert!(e.contains("refused"), "{e}");
    assert_eq!(
        std::fs::read_dir(&outside).unwrap().count(),
        0,
        "nothing written outside"
    );

    // Reads through the link are refused too, and files_list does not show the folder.
    std::fs::write(outside.join("there.json"), "{}").unwrap();
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"path": "to-outside/there.json"}),
    ));
    assert!(e.contains("may only read inside"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"path": "to-outside"}),
    ));
    assert!(e.contains("files_list only reads those"), "{e}");
    assert!(!e.contains("outside/dir"), "{e}");
    std::fs::remove_file(outside.join("there.json")).unwrap();
}

#[cfg(unix)]
#[test]
fn a_linked_file_is_not_written_through() {
    let (mut s, mut h, root) = new();
    let victim = root.join("outside/victim.txt");
    std::fs::write(&victim, "kept\n").unwrap();
    link(&victim, &root.join("out/innocent.json"));
    link(&victim, &root.join("out/innocent.png"));
    // A link to a file that does not exist yet: writing through it would make the file.
    let missing = root.join("outside/made.json");
    link(&missing, &root.join("out/dangling.json"));
    for (tool, args) in [
        ("world_status", json!({"save_path": "innocent.json"})),
        ("world_status", json!({"save_path": "dangling.json"})),
        (
            "world_render",
            json!({"max_side": 64, "save_path": "innocent.png"}),
        ),
        (
            "world_render",
            json!({"wallpaper": {"size": "320x200", "out": "innocent.png"}}),
        ),
    ] {
        let e = fails(&call(&mut s, &mut h, tool, args.clone()));
        assert!(e.contains("may only write inside"), "{tool} {args}: {e}");
    }
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "kept\n");
    assert!(!missing.exists());
    let e = fails(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"file": "innocent.json"}),
    ));
    assert!(e.contains("files_list only reads those"), "{e}");
    assert!(!e.contains("victim"), "{e}");
}

#[cfg(unix)]
#[test]
fn dot_dot_after_a_link_is_worked_out_from_where_the_link_leads() {
    let (mut s, mut h, root) = new();
    // `deep/..` is `outside/`, not the output folder.
    link(&root.join("outside/dir"), &root.join("out/deep"));
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": "deep/../escape.json"}),
    ));
    assert!(e.contains("may only write inside"), "{e}");
    assert!(!root.join("outside/escape.json").exists());
    assert!(!root.join("out/escape.json").exists());
    // A link that stays inside is fine, and the file lands where the link leads.
    std::fs::create_dir_all(root.join("out/real")).unwrap();
    link(&root.join("out/real"), &root.join("out/alias"));
    let t = ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": "alias/kept.json"}),
    ));
    assert!(t.contains("Saved the scene"), "{t}");
    assert!(root.join("out/real/kept.json").is_file());
    // `..` in a part that does not exist yet can step back onto a link; it is followed.
    link(
        &root.join("outside/victim2.json"),
        &root.join("out/back.json"),
    );
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": "new-folder/../back.json"}),
    ));
    assert!(e.contains("may only write inside"), "{e}");
    assert!(!root.join("outside/victim2.json").exists());
}

#[test]
fn a_refusal_reads_the_same_whether_or_not_the_path_outside_exists() {
    let (mut s, mut h, root) = new();
    std::fs::write(root.join("outside/real.json"), "{}").unwrap();
    let ask = |s: &mut Server, h: &mut Session, tool: &str, args: serde_json::Value| {
        fails(&call(s, h, tool, args)).replace("real.json", "NAME.json")
    };
    let there = path_json(&root.join("outside/real.json"));
    let not_there = path_json(&root.join("outside/NAME.json"));
    for (tool, key) in [
        ("world_new", "path"),
        ("files_list", "path"),
        ("files_list", "file"),
    ] {
        let a = ask(&mut s, &mut h, tool, json!({ key: there }));
        let b = ask(&mut s, &mut h, tool, json!({ key: not_there }));
        assert_eq!(a, b, "{tool} {key}");
        assert!(!a.contains("does not exist"), "{a}");
    }
}

#[test]
fn an_allowed_folder_is_written_read_and_listed() {
    let (mut s, mut h, root) = new();
    let allowed = root.join("allowed");
    let scene = allowed.join("scene.json");
    ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": path_json(&scene)}),
    ));
    assert!(scene.is_file());
    ok(&call(
        &mut s,
        &mut h,
        "world_new",
        json!({"path": path_json(&scene)}),
    ));
    let l = json_of(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"path": path_json(&allowed)}),
    ));
    assert!(
        l["newest_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "scene.json"),
        "{l}"
    );
    let t = ok(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"file": path_json(&scene)}),
    ));
    assert!(t.contains("\"tiling\""), "{t}");
    // Other folders are still refused, and the refusal names the allowed folder too.
    let e = fails(&call(
        &mut s,
        &mut h,
        "files_list",
        json!({"path": path_json(&root.join("config"))}),
    ));
    assert!(e.contains("the allowed folders"), "{e}");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": path_json(&root.join("outside/x.json"))}),
    ));
    assert!(e.contains("may only write inside"), "{e}");
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_written_says_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let (mut s, mut h, root) = new();
    let ro = root.join("read-only");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Someone who may write anyway (root) cannot see this.
    if std::fs::write(ro.join("probe"), "").is_ok() {
        let _ = std::fs::remove_file(ro.join("probe"));
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: this user can write in a read-only folder");
        return;
    }
    let at = |p: &str| path_json(&ro.join(p));
    for (tool, args, folder) in [
        (
            "world_render",
            json!({"wallpaper": {"size": "320x200", "out": at("walls")}}),
            "walls",
        ),
        (
            "media_render",
            json!({"quickstart": "life", "out": at("media")}),
            "media",
        ),
        ("pack", json!({"site": {"to": at("site")}}), "site"),
    ] {
        let e = fails(&call(&mut s, &mut h, tool, args.clone()));
        assert!(
            e.contains(&format!(
                "could not create folder {}",
                ro.join(folder).display()
            )) && e.contains("Permission denied"),
            "{tool} {args}: {e}"
        );
        assert!(!e.contains("may only write") && !e.contains(".keep"), "{e}");
    }
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": at("scene.json")}),
    ));
    assert!(e.contains("Permission denied"), "{e}");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_refusal_names_the_path_asked_for() {
    let (mut s, mut h, root) = new();
    let wall = root.join("outside").join("wall.png");
    let e = fails(&call(
        &mut s,
        &mut h,
        "world_render",
        json!({"wallpaper": {"size": "320x200", "out": path_json(&wall)}}),
    ));
    assert!(e.contains(&format!("(got {})", wall.display())), "{e}");
    let store = root.join("outside").join("store");
    for (tool, args) in [
        (
            "media_render",
            json!({"quickstart": "life", "out": path_json(&store)}),
        ),
        ("pack", json!({"site": {"to": path_json(&store)}})),
    ] {
        let e = fails(&call(&mut s, &mut h, tool, args.clone()));
        assert!(
            e.contains(&format!("(got {})", store.display())),
            "{tool} {args}: {e}"
        );
    }
}
