//! `jetcarp uninstall --data` deletes the data folder only when everything in it is JETCARP's
//! (#195). A data folder named by mistake, such as a folder of documents in `JETCARP_DATA_DIR`,
//! is refused and left untouched, and so is everything else: the plan, `--yes` and `--settings`
//! do nothing. A folder of only JETCARP's own files, an empty one, and a link (only the link goes)
//! are deleted as before.
//!
//! Every folder is a scratch folder, checked to be one before anything is deleted; the platform is
//! the headless one, which has nothing of its own to undo. This file is its own test program with
//! one test, because the folders come from environment variables, which it sets.

use jetcarp_api::error::ErrorKind;
use jetcarp_api::ops::system::{SystemUninstall, UninstallIn, UninstallReport};
use jetcarp_api::{ApiError, Ctx, Op, Quiet};
use std::path::{Path, PathBuf};

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Every file under `d`, as paths inside it, sorted.
fn tree(d: &Path) -> Vec<String> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(d, d, &mut out);
    out.sort();
    out
}

/// A data folder as JETCARP leaves it: the library with SQLite's files beside it, a job lock,
/// media, a search, the search databases' folder, and what Finder and Explorer leave.
fn jetcarps(d: &Path) {
    for f in [
        "library.sqlite",
        "library.sqlite-wal",
        "library.sqlite-shm",
        "library.sqlite-jobs/hunt-1.lock",
        "media/ab/ab12cd.png",
        "searches/0f8b6a52-3c1e-4d7a-9b2f-5e6d7c8b9a01/manifest.json",
        "searches/0f8b6a52-3c1e-4d7a-9b2f-5e6d7c8b9a01/runs/part-000001.parquet",
        "explore/runs.sqlite",
        // On Windows the data folder is also the runtime folder and the screen saver's.
        "host.lock",
        "host-endpoint.json",
        ".host-endpoint.json.4242.tmp",
        "JETCARP.scr",
        ".DS_Store",
        "Thumbs.db",
        "media/desktop.ini",
    ] {
        write(&d.join(f), "x");
    }
}

#[test]
fn only_a_data_folder_of_jetcarps_own_is_deleted() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-data-folder");
    let _ = std::fs::remove_dir_all(&d);
    let (settings, runtime, out) = (d.join("settings"), d.join("runtime"), d.join("out"));
    for f in [&settings, &runtime, &out] {
        std::fs::create_dir_all(f).unwrap();
    }
    write(&settings.join("app.json"), "{}");
    // SAFETY: this test program runs this one test, which sets them before each Ctx it makes.
    let set = |data: Option<&Path>| unsafe {
        std::env::set_var("JETCARP_CONFIG_DIR", &settings);
        std::env::set_var("JETCARP_RUNTIME_DIR", &runtime);
        std::env::set_var("JETCARP_OUT", &out);
        std::env::remove_var("JETCARP_LIBRARY");
        match data {
            Some(p) => std::env::set_var("JETCARP_DATA_DIR", p),
            None => std::env::remove_var("JETCARP_DATA_DIR"),
        }
    };
    let run = |yes: bool, settings: bool| -> Result<UninstallReport, ApiError> {
        let cx = Ctx::new();
        let data = cx.paths().data;
        assert!(
            data.starts_with(&d),
            "not a scratch folder: {}",
            data.display()
        );
        SystemUninstall::run(
            &cx,
            UninstallIn {
                yes,
                settings,
                data: true,
            },
            &Quiet,
        )
    };
    let refused =
        |r: Result<UninstallReport, ApiError>, folder: &Path, names: &[&str]| -> ApiError {
            let e = r.expect_err("refused");
            assert_eq!(e.kind, ErrorKind::Conflict, "{e}");
            assert!(e.message.contains(&folder.display().to_string()), "{e}");
            assert!(e.message.contains("so nothing was done"), "{e}");
            for n in names {
                assert!(e.message.contains(n), "{n}: {e}");
            }
            assert!(
                e.hint
                    .as_deref()
                    .is_some_and(|h| h.contains("delete it yourself")),
                "{e}"
            );
            e
        };

    // A folder of documents named by JETCARP_DATA_DIR (the issue's case): refused, with the
    // plan, with --yes and with --settings, and nothing anywhere is touched.
    let docs = d.join("documents");
    write(&docs.join("letter.txt"), "Dear Ada");
    write(&docs.join("photos").join("holiday.txt"), "sand");
    set(Some(&docs));
    let before = tree(&docs);
    for (yes, with_settings) in [(false, false), (true, false), (true, true)] {
        let e = refused(run(yes, with_settings), &docs, &["letter.txt", "photos"]);
        assert!(e.message.contains("named by JETCARP_DATA_DIR"), "{e}");
        assert!(
            e.hint
                .as_deref()
                .unwrap()
                .contains("JETCARP_DATA_DIR names this folder"),
            "{e}"
        );
    }
    assert_eq!(tree(&docs), before);
    assert!(
        settings.join("app.json").exists(),
        "--settings did nothing either"
    );

    // A library alone does not make a folder JETCARP's: JETCARP makes one in whatever folder it
    // is given. The documents with a library in them are still refused.
    write(&docs.join("library.sqlite"), "catalog");
    refused(run(true, false), &docs, &["letter.txt", "photos"]);
    assert!(docs.join("letter.txt").exists());

    // JETCARP's own folder names holding something else are not JETCARP's either.
    let named = d.join("named-alike");
    for f in [
        "library.sqlite",
        "media/holiday.mp4",
        "searches/my notes/plan.txt",
        "library.sqlite-jobs/old/x",
        ".git/config",
        ".host-endpoint.json.tmp",
    ] {
        write(&named.join(f), "x");
    }
    set(Some(&named));
    refused(
        run(true, false),
        &named,
        // The first three, sorted, then how many more.
        &[".git, .host-endpoint.json.tmp, library.sqlite-jobs/old and 2 more"],
    );
    assert!(named.join("media").join("holiday.mp4").exists());

    // A folder of JETCARP's own: listed, then deleted.
    let data = d.join("data");
    jetcarps(&data);
    set(Some(&data));
    let plan = run(false, false).unwrap();
    assert!(
        plan.steps.iter().any(|s| s
            .what
            .starts_with(&format!("delete the data folder {}", data.display()))),
        "{:?}",
        plan.steps
    );
    assert!(
        data.join("library.sqlite").exists(),
        "the plan deletes nothing"
    );
    let r = run(true, false).unwrap();
    assert!(r.steps.iter().all(|s| s.ok == Some(true)), "{:?}", r.steps);
    assert!(!data.exists());

    // An empty folder: deleted.
    std::fs::create_dir_all(&data).unwrap();
    run(true, false).unwrap();
    assert!(!data.exists());

    // A folder chosen in setup (paths.json): refused in the same words, naming paths.json.
    write(&docs.join("letter.txt"), "Dear Ada");
    set(None);
    jetcarp_core::paths::set_chosen_data_dir(&settings, Some(&docs)).unwrap();
    let e = refused(run(true, false), &docs, &["letter.txt"]);
    assert!(e.message.contains("chosen in setup, in paths.json"), "{e}");
    assert!(e.hint.as_deref().unwrap().contains("paths.json"), "{e}");
    assert!(docs.join("letter.txt").exists());
    jetcarp_core::paths::set_chosen_data_dir(&settings, None).unwrap();

    // A data folder that is a link to the documents: only the link goes (as before).
    let link = d.join("link-to-documents");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&docs, &link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&docs, &link);
    match made {
        Ok(()) => {
            set(Some(&link));
            let r = run(true, false).unwrap();
            assert!(r.steps.iter().all(|s| s.ok == Some(true)), "{:?}", r.steps);
            assert!(
                std::fs::symlink_metadata(&link).is_err(),
                "the link is gone"
            );
            assert!(
                docs.join("letter.txt").exists(),
                "the folder it led to is kept"
            );
        }
        // Windows without Developer Mode cannot make one.
        Err(e) => eprintln!("skipped the link: {e}"),
    }

    // The default data folder, moved into the scratch folder through XDG_DATA_HOME (Linux only:
    // elsewhere the default cannot be moved, and the real one is never touched).
    #[cfg(target_os = "linux")]
    {
        // SAFETY: as above.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", d.join("xdg-data"));
            std::env::set_var("XDG_CONFIG_HOME", d.join("xdg-config"));
        }
        set(None);
        let default = d.join("xdg-data").join("jetcarp");
        assert_eq!(Ctx::new().paths().data, default);
        write(&default.join("my-notes.txt"), "mine");
        jetcarps(&default);
        let e = refused(run(true, false), &default, &["my-notes.txt"]);
        assert!(
            !e.message.contains("named by") && !e.message.contains("chosen in"),
            "{e}"
        );
        std::fs::remove_file(default.join("my-notes.txt")).unwrap();
        run(true, false).unwrap();
        assert!(!default.exists());
    }
    assert!(out.exists(), "the output folder is never deleted");
    let _ = std::fs::remove_dir_all(&d);
}
