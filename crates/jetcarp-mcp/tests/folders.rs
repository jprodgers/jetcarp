//! The folder rules with symbolic links and `--allow`: a link inside the output folder is
//! followed before the rules are checked, so a linked folder, a linked file (whether its target
//! exists or not) and `..` after a link cannot reach outside; a refusal reads the same whether or
//! not the path outside exists; and a folder given with `--allow` is read, written and listed
//! (`files_list` too). A folder the tools may use but cannot write in says "Permission denied",
//! and a refusal names the path asked for, not a file inside it (#65). The files the tools name
//! themselves inside a folder they were given are not written through a link planted at one of
//! those names.
//!
//! Each link case runs with every kind of link the system has (`links`): on Windows a directory
//! junction and a directory symbolic link for a folder, a file symbolic link for a file.
//!
//! The allowed folders are set before this binary's first tool call, which is why these tests have
//! a binary of their own.

mod common;
mod links;

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

/// Whether an error names `p`, with `/` or with `\` between its parts.
fn names(e: &str, p: &str) -> bool {
    e.contains(p) || e.contains(&p.replace('/', "\\"))
}

#[test]
fn a_linked_folder_inside_the_output_folder_does_not_lead_outside() {
    let (mut s, mut h, root) = new();
    let outside = root.join("outside/dir");
    for &kind in links::Kind::for_folders() {
        let case = format!("a linked folder inside the output folder ({})", kind.name());
        let name = format!("to-outside-{}", kind.name());
        if let Err(why) = links::folder(kind, &outside, &root.join("out").join(&name)) {
            links::skip(&case, &why);
            continue;
        }
        let at = |p: &str| format!("{name}/{p}");
        let writes = [
            ("world_status", json!({"save_path": at("scene.json")})),
            (
                "world_render",
                json!({"max_side": 64, "save_path": at("pic.png")}),
            ),
            (
                "world_render",
                json!({"wallpaper": {"size": "320x200", "out": name}}),
            ),
            (
                "media_render",
                json!({"quickstart": "life", "out": at("media")}),
            ),
            ("pack", json!({"site": {"to": at("site")}})),
        ];
        for (tool, args) in writes {
            let e = fails(&call(&mut s, &mut h, tool, args.clone()));
            assert!(
                e.contains("the MCP tools may only write inside"),
                "{case}: {tool} {args}: {e}"
            );
            // The refusal names the path as given, not where the link leads.
            assert!(!names(&e, "outside/dir"), "{case}: {tool} {args}: {e}");
        }
        let script = format!(
            "try {{ write_text(\"{}\", \"x\"); print(\"wrote\"); }} catch (e) {{ print(`refused ${{e}}`); }}",
            at("a.txt")
        );
        let e = ok(&call(
            &mut s,
            &mut h,
            "run_script",
            json!({"script": script}),
        ));
        assert!(e.contains("refused"), "{case}: {e}");
        assert_eq!(
            std::fs::read_dir(&outside).unwrap().count(),
            0,
            "{case}: nothing written outside"
        );

        // Reads through the link are refused too, and files_list does not show the folder.
        std::fs::write(outside.join("there.json"), "{}").unwrap();
        let e = fails(&call(
            &mut s,
            &mut h,
            "world_new",
            json!({"path": at("there.json")}),
        ));
        assert!(e.contains("may only read inside"), "{case}: {e}");
        let e = fails(&call(&mut s, &mut h, "files_list", json!({"path": name})));
        assert!(e.contains("files_list only reads those"), "{case}: {e}");
        assert!(!names(&e, "outside/dir"), "{case}: {e}");
        std::fs::remove_file(outside.join("there.json")).unwrap();
        links::ran(&case);
    }
}

#[test]
fn a_linked_file_is_not_written_through() {
    let (mut s, mut h, root) = new();
    let case = "a linked file (symlink)";
    let victim = root.join("outside/victim.txt");
    std::fs::write(&victim, "kept\n").unwrap();
    // A link to a file that does not exist yet: writing through it would make the file.
    let missing = root.join("outside/made.json");
    let made = links::file(&victim, &root.join("out/innocent.json"))
        .and_then(|_| links::file(&victim, &root.join("out/innocent.png")))
        .and_then(|_| links::file(&missing, &root.join("out/dangling.json")));
    if let Err(why) = made {
        return links::skip(case, &why);
    }
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
    links::ran(case);
}

/// `..` after a link. On Unix it is worked out from where the link leads, so `deep/..` is the
/// folder holding the link's target. Windows works `..` out from the path's text before any link
/// is followed, so there `deep\..` is the output folder itself, which JETCARP checks and writes
/// to just as the system would. Either way nothing reaches outside.
#[test]
fn dot_dot_after_a_link_is_worked_out_as_the_system_does() {
    let (mut s, mut h, root) = new();
    for &kind in links::Kind::for_folders() {
        let k = kind.name();
        let case = format!("`..` after a linked folder ({k})");
        let deep = format!("deep-{k}");
        let real = root.join(format!("out/real-{k}"));
        std::fs::create_dir_all(&real).unwrap();
        let made = links::folder(
            kind,
            &root.join("outside/dir"),
            &root.join("out").join(&deep),
        )
        .and_then(|_| links::folder(kind, &real, &root.join(format!("out/alias-{k}"))));
        if let Err(why) = made {
            links::skip(&case, &why);
            continue;
        }
        let escape = format!("escape-{k}.json");
        let r = call(
            &mut s,
            &mut h,
            "world_status",
            json!({"save_path": format!("{deep}/../{escape}")}),
        );
        if cfg!(windows) {
            let t = ok(&r);
            assert!(t.contains("Saved the scene"), "{case}: {t}");
            assert!(root.join("out").join(&escape).is_file(), "{case}");
        } else {
            let e = fails(&r);
            assert!(e.contains("may only write inside"), "{case}: {e}");
            assert!(!root.join("out").join(&escape).exists(), "{case}");
        }
        assert!(!root.join("outside").join(&escape).exists(), "{case}");
        // A link that stays inside is fine, and the file lands where the link leads.
        let t = ok(&call(
            &mut s,
            &mut h,
            "world_status",
            json!({"save_path": format!("alias-{k}/kept.json")}),
        ));
        assert!(t.contains("Saved the scene"), "{case}: {t}");
        assert!(real.join("kept.json").is_file(), "{case}");
        links::ran(&case);
    }
    // `..` in a part that does not exist yet can step back onto a link; it is followed.
    let case = "`..` onto a linked file (symlink)";
    match links::file(
        &root.join("outside/victim2.json"),
        &root.join("out/back.json"),
    ) {
        Err(why) => links::skip(case, &why),
        Ok(()) => {
            let e = fails(&call(
                &mut s,
                &mut h,
                "world_status",
                json!({"save_path": "new-folder/../back.json"}),
            ));
            assert!(e.contains("may only write inside"), "{e}");
            assert!(!root.join("outside/victim2.json").exists());
            links::ran(case);
        }
    }
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

/// The files the tools name themselves inside a folder they were given (a scene's poster and
/// frames, a site export's manifest, a pile's colours, a sweep's record) are not written through
/// a symbolic link already at that name that leads outside: the call is refused, naming the file
/// as the tools would write it, and nothing appears where the link leads.
#[test]
fn a_link_at_a_name_the_tools_choose_is_not_written_through() {
    let (mut s, mut h, root) = new();
    let out = root.join("out");
    let away = root.join("outside/planted");
    std::fs::create_dir_all(away.join("frames")).unwrap();
    // A link at `name` in the folder `folder` of the output folder, to a file not there yet.
    let plant = |folder: &str, name: &str| -> Result<std::path::PathBuf, String> {
        std::fs::create_dir_all(out.join(folder)).unwrap();
        links::file(&away.join(folder), &out.join(folder).join(name))?;
        Ok(out.join(folder).join(name))
    };
    let case = "a linked file at a name the tools choose (symlink)";
    let planted = (|| {
        Ok::<_, String>([
            (
                plant("lk-media", "poster.png")?,
                "media_render",
                json!({"quickstart": "life", "out": "lk-media"}),
            ),
            (
                plant("lk-site", "manifest.json")?,
                "pack",
                json!({"site": {"to": "lk-site"}}),
            ),
            (
                plant("lk-pile", "look.json")?,
                "search_start",
                json!({"pile": {"tiling": "square", "grains": "1e3", "folder": "lk-pile"}}),
            ),
        ])
    })();
    match planted {
        Err(why) => links::skip(case, &why),
        Ok(cases) => {
            for (planted, tool, args) in cases {
                let e = fails(&call(&mut s, &mut h, tool, args.clone()));
                assert!(e.contains("may only write inside"), "{tool} {args}: {e}");
                assert!(
                    e.contains(&planted.display().to_string()),
                    "{tool} {args}: {e}"
                );
                assert!(!e.contains("outside"), "{tool} {args}: {e}");
            }
            // A sweep's record in the folder a script names.
            plant("lk-sweep", "run.json").unwrap();
            let t = ok(&call(
                &mut s,
                &mut h,
                "run_script",
                json!({"script": "try { sweep(#{ rules: \"B3/S23\", tiling: \"square\", size: [16, 16], seed: \"random:0.4\", max_gens: 4, png: false, folder: \"lk-sweep\" }); print(\"wrote\"); } catch (e) { print(`refused ${e}`); }"}),
            ));
            assert!(
                t.contains("refused") && t.contains("may only write inside"),
                "{t}"
            );
            // A catalog a script opens in a folder: its file is checked, not only the folder.
            plant("lk-lib", "library.sqlite").unwrap();
            let t = ok(&call(
                &mut s,
                &mut h,
                "run_script",
                json!({"script": "try { let l = library(\"lk-lib\"); print(l.info()); } catch (e) { print(`refused ${e}`); }"}),
            ));
            assert!(
                t.contains("refused") && t.contains("may only write inside"),
                "{t}"
            );
            links::ran(case);
        }
    }
    // A linked `frames` folder is refused before a frame is written.
    for &kind in links::Kind::for_folders() {
        let case = format!("a linked frames folder ({})", kind.name());
        let folder = format!("lk-frames-{}", kind.name());
        std::fs::create_dir_all(out.join(&folder)).unwrap();
        if let Err(why) = links::folder(
            kind,
            &away.join("frames"),
            &out.join(&folder).join("frames"),
        ) {
            links::skip(&case, &why);
            continue;
        }
        let e = fails(&call(
            &mut s,
            &mut h,
            "media_render",
            json!({"quickstart": "life", "out": folder, "frames": true}),
        ));
        assert!(e.contains("may only write inside"), "{case}: {e}");
        links::ran(&case);
    }
    let left: Vec<_> = std::fs::read_dir(&away)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["frames"], "nothing written outside");
    assert_eq!(std::fs::read_dir(away.join("frames")).unwrap().count(), 0);
}

/// A path of 4,096 bytes or more is refused in its own words, naming its length and the limit,
/// the same on every system (a save, a picture, a listing); a path many folders deep under the
/// limit is saved where it was asked for.
#[test]
fn a_path_of_4096_bytes_or_more_is_refused_and_a_deep_one_is_saved() {
    let (mut s, mut h, root) = new();
    for (tool, args) in [
        (
            "world_status",
            json!({"save_path": format!("{}deep.json", "a/".repeat(3000))}),
        ),
        (
            "world_render",
            json!({"max_side": 64, "save_path": format!("{}deep.png", "b/".repeat(3000))}),
        ),
        ("files_list", json!({"path": "c/".repeat(3000)})),
    ] {
        let e = fails(&call(&mut s, &mut h, tool, args));
        assert!(
            e.contains("bytes long") && e.contains("shorter than 4,096 bytes"),
            "{tool}: {e}"
        );
    }
    assert!(!root.join("out/a").exists() && !root.join("out/b").exists());
    // macOS's own limit is 1,024 bytes.
    let depth = if cfg!(target_os = "macos") { 200 } else { 1500 };
    let rel = format!("{}deep.json", "d/".repeat(depth));
    ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": rel}),
    ));
    assert!(root.join("out").join(&rel).is_file());
    let _ = std::fs::remove_dir_all(root.join("out/d"));
}

/// Names that cannot be copied to every system are refused in their own words, the same on every
/// system, before anything is made: a Windows device name (a save, a picture, a read, a listing;
/// alone, with an extension, as a folder), a `:` in a name (a file's, a folder's), and a name
/// ending in a dot or a space. Names that only start with the same letters, and dots inside a
/// name, are saved.
#[test]
fn names_every_system_cannot_hold_are_refused() {
    let (mut s, mut h, root) = new();
    let reserved = "is reserved on Windows; JETCARP refuses it on every system";
    let colon = "a `:` in a name";
    let dot = "ends in a dot; Windows drops it";
    let space = "ends in a space; Windows drops it";
    for (tool, args, words) in [
        ("world_status", json!({"save_path": "CON"}), reserved),
        ("world_status", json!({"save_path": "nul.json"}), reserved),
        ("world_status", json!({"save_path": "AUX/x.json"}), reserved),
        (
            "world_status",
            json!({"save_path": "deep/../com1.json"}),
            reserved,
        ),
        (
            "world_status",
            json!({"save_path": "plain.json:other"}),
            colon,
        ),
        (
            "world_status",
            json!({"save_path": "plain.json:other:$DATA"}),
            colon,
        ),
        (
            "world_status",
            json!({"save_path": "run 12:30/scene.json"}),
            colon,
        ),
        (
            "world_render",
            json!({"max_side": 64, "save_path": "con.png"}),
            reserved,
        ),
        (
            "world_render",
            json!({"max_side": 64, "save_path": "pics:2/x.png"}),
            colon,
        ),
        ("world_new", json!({"path": "Con.json"}), reserved),
        ("world_new", json!({"path": "plain.json:other"}), colon),
        ("files_list", json!({"path": "LPT1"}), reserved),
        ("files_list", json!({"path": "ab:c"}), colon),
        ("files_list", json!({"path": "x/a:b"}), colon),
        ("world_status", json!({"save_path": "scene.json."}), dot),
        ("world_status", json!({"save_path": "scene.json "}), space),
        ("world_status", json!({"save_path": "dir./x.json"}), dot),
        (
            "world_render",
            json!({"max_side": 64, "save_path": "pic.png."}),
            dot,
        ),
        ("world_new", json!({"path": "scene.json."}), dot),
        ("files_list", json!({"path": "..."}), dot),
    ] {
        let e = fails(&call(&mut s, &mut h, tool, args.clone()));
        assert!(e.contains(words), "{tool} {args}: {e}");
    }
    for made in [
        "CON",
        "nul.json",
        "AUX",
        "deep",
        "plain.json",
        "run 12:30",
        "con.png",
        "pics:2",
        "scene.json.",
        "scene.json ",
        "dir.",
        "pic.png.",
    ] {
        assert!(!root.join("out").join(made).exists(), "{made} was made");
    }
    for name in [
        "console.json",
        "nullable.json",
        "com10.json",
        "lpt.json",
        "a.b.json",
    ] {
        ok(&call(
            &mut s,
            &mut h,
            "world_status",
            json!({"save_path": name}),
        ));
        assert!(root.join("out").join(name).is_file(), "{name}");
    }
    // `a:b`: on Windows one letter and a `:` at the start is a drive (`A:`), so the path is
    // outside the folders there; elsewhere it is a name with a `:`. Refused either way.
    let e = fails(&call(&mut s, &mut h, "files_list", json!({"path": "a:b"})));
    if cfg!(windows) {
        assert!(e.contains("outside") || e.contains(colon), "{e}");
    } else {
        assert!(e.contains(colon), "{e}");
    }
    // `.` and `..` keep their meaning.
    ok(&call(
        &mut s,
        &mut h,
        "world_status",
        json!({"save_path": "./sub/../a.c.json"}),
    ));
    assert!(root.join("out/a.c.json").is_file());
}
