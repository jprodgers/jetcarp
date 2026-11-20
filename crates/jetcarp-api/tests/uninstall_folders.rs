//! `jetcarp uninstall --data` and `--settings` delete their folders only when everything in each
//! is JETCARP's (#195). A folder named by mistake, such as a folder of documents in
//! `JETCARP_DATA_DIR` or `JETCARP_CONFIG_DIR`, is refused and left untouched, and so is everything
//! else: both folders are checked before anything is done, and the plan and `--yes` alike do
//! nothing. A folder of only JETCARP's own files, an empty one, and a link (only the link goes)
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
/// media, a search, the search databases' folder, Windows' runtime files and screen saver, and
/// what Finder and Explorer leave.
fn data_of_jetcarps(d: &Path) {
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

/// A settings folder as JETCARP leaves it: every file and folder it keeps there, with a file
/// being written, and what Finder and Explorer leave.
fn settings_of_jetcarps(d: &Path) {
    for f in [
        "app.json",
        "host.json",
        "setup.json",
        "photosensitivity.json",
        "gpu.json",
        "gpu-checks.json",
        "host-program",
        "desktop-picture.json",
        "screen-saver-before.plist",
        "wallpaper-store-before-saver-remove.plist",
        "setup.json.tmp-4242",
        "screensavers/calm.json",
        "rotations/evening.json",
        "tune/screen-1/crop-12.png",
        "desktop-pictures/still-1.png",
        "pack/publisher_ed25519",
        "pack/publisher_ed25519.pub",
        "pack/publisher.json",
        "pack/raters.json",
        "pack/raters.tmp4242",
        ".DS_Store",
        "desktop.ini",
    ] {
        write(&d.join(f), "x");
    }
    // A `paths.json` that chooses no data folder.
    write(&d.join("paths.json"), "{}");
}

#[test]
fn only_folders_of_jetcarps_own_are_deleted() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-folders");
    let _ = std::fs::remove_dir_all(&d);
    let (runtime, out) = (d.join("runtime"), d.join("out"));
    for f in [&runtime, &out] {
        std::fs::create_dir_all(f).unwrap();
    }
    let settings = d.join("settings");
    write(&settings.join("app.json"), "{}");
    // SAFETY: this test program runs this one test, which sets them before each Ctx it makes.
    let set = |config: &Path, data: Option<&Path>| unsafe {
        std::env::set_var("JETCARP_CONFIG_DIR", config);
        std::env::set_var("JETCARP_RUNTIME_DIR", &runtime);
        std::env::set_var("JETCARP_OUT", &out);
        std::env::remove_var("JETCARP_LIBRARY");
        match data {
            Some(p) => std::env::set_var("JETCARP_DATA_DIR", p),
            None => std::env::remove_var("JETCARP_DATA_DIR"),
        }
    };
    let run = |yes: bool, settings: bool, data: bool| -> Result<UninstallReport, ApiError> {
        let cx = Ctx::new();
        let p = cx.paths();
        for f in p.settings.iter().chain([&p.data]) {
            assert!(f.starts_with(&d), "not a scratch folder: {}", f.display());
        }
        SystemUninstall::run(
            &cx,
            UninstallIn {
                yes,
                settings,
                data,
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
                e.hint.as_deref().is_some_and(|h| h.contains("yourself")),
                "{e}"
            );
            e
        };
    let all_ok = |r: &UninstallReport| {
        assert!(r.steps.iter().all(|s| s.ok == Some(true)), "{:?}", r.steps);
    };

    // ---- The data folder

    // A folder of documents named by JETCARP_DATA_DIR (the issue's case): refused, with the
    // plan, with --yes and with --settings, and nothing anywhere is touched.
    let docs = d.join("documents");
    write(&docs.join("letter.txt"), "Dear Ada");
    write(&docs.join("photos").join("holiday.txt"), "sand");
    set(&settings, Some(&docs));
    let before = tree(&docs);
    for (yes, with_settings) in [(false, false), (true, false), (true, true)] {
        let e = refused(
            run(yes, with_settings, true),
            &docs,
            &["letter.txt", "photos"],
        );
        assert!(e.message.contains("the data folder"), "{e}");
        assert!(e.message.contains("named by JETCARP_DATA_DIR"), "{e}");
        assert!(
            e.message.contains("`--data` deletes the data folder only"),
            "{e}"
        );
        assert!(
            e.hint
                .as_deref()
                .unwrap()
                .contains("JETCARP_DATA_DIR names the data folder"),
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
    refused(run(true, false, true), &docs, &["letter.txt", "photos"]);
    assert!(docs.join("letter.txt").exists());
    std::fs::remove_file(docs.join("library.sqlite")).unwrap();

    // JETCARP's own folder names holding something else are not JETCARP's either.
    let named = d.join("named-alike");
    for f in [
        "library.sqlite",
        "media/holiday.mp4",
        "searches/my notes/plan.txt",
        "library.sqlite-jobs/old/x",
        ".git/config",
        ".notes.tmp",
    ] {
        write(&named.join(f), "x");
    }
    set(&settings, Some(&named));
    refused(
        run(true, false, true),
        &named,
        // The first three, sorted, then how many more.
        &[".git, .notes.tmp, library.sqlite-jobs/old and 2 more"],
    );
    assert!(named.join("media").join("holiday.mp4").exists());

    // A folder of JETCARP's own: listed, then deleted.
    let data = d.join("data");
    data_of_jetcarps(&data);
    set(&settings, Some(&data));
    let plan = run(false, false, true).unwrap();
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
    all_ok(&run(true, false, true).unwrap());
    assert!(!data.exists());

    // An empty folder: deleted.
    std::fs::create_dir_all(&data).unwrap();
    run(true, false, true).unwrap();
    assert!(!data.exists());

    // A folder chosen in setup (paths.json): refused in the same words, naming paths.json.
    set(&settings, None);
    jetcarp_core::paths::set_chosen_data_dir(&settings, Some(&docs)).unwrap();
    let e = refused(run(true, false, true), &docs, &["letter.txt"]);
    assert!(e.message.contains("chosen in setup, in paths.json"), "{e}");
    assert!(e.hint.as_deref().unwrap().contains("paths.json"), "{e}");
    assert!(docs.join("letter.txt").exists());
    jetcarp_core::paths::set_chosen_data_dir(&settings, None).unwrap();

    // ---- The settings folder

    // A folder of documents named by JETCARP_CONFIG_DIR: refused, with the plan and with --yes,
    // and with --data too, though the data folder is JETCARP's: both are checked before either
    // is touched.
    data_of_jetcarps(&data);
    set(&docs, Some(&data));
    let data_before = tree(&data);
    for (yes, with_data) in [(false, false), (true, false), (true, true)] {
        let e = refused(run(yes, true, with_data), &docs, &["letter.txt", "photos"]);
        assert!(e.message.contains("the settings folder"), "{e}");
        assert!(e.message.contains("named by JETCARP_CONFIG_DIR"), "{e}");
        assert!(
            e.message
                .contains("`--settings` deletes the settings folder only"),
            "{e}"
        );
        assert!(!e.message.contains("the data folder"), "{e}");
        assert!(
            e.hint
                .as_deref()
                .unwrap()
                .contains("JETCARP_CONFIG_DIR names the settings folder"),
            "{e}"
        );
    }
    assert_eq!(tree(&docs), before);
    assert_eq!(
        tree(&data),
        data_before,
        "the data folder waits for the settings folder's check"
    );

    // Both folders not JETCARP's: one refusal naming both.
    set(&docs, Some(&named));
    let e = refused(run(true, true, true), &docs, &["letter.txt"]);
    assert!(e.message.contains(&named.display().to_string()), "{e}");
    assert!(
        e.message
            .contains("`--data` and `--settings` delete their folders only"),
        "{e}"
    );
    assert!(
        e.hint.as_deref().unwrap().contains("delete them yourself"),
        "{e}"
    );
    assert_eq!(tree(&docs), before);
    assert!(named.join("media").join("holiday.mp4").exists());

    // JETCARP's own folder names holding something else, in the settings folder.
    let odd_settings = d.join("odd-settings");
    for f in [
        "app.json",
        "pack/my-ssh-key",
        "rotations/old/x.json",
        "notes.md",
    ] {
        write(&odd_settings.join(f), "x");
    }
    set(&odd_settings, Some(&data));
    refused(
        run(true, true, false),
        &odd_settings,
        &["notes.md, pack/my-ssh-key, rotations/old"],
    );
    assert!(odd_settings.join("pack").join("my-ssh-key").exists());

    // A settings folder of JETCARP's own, with --data: both deleted, the data folder first.
    let own_settings = d.join("own-settings");
    settings_of_jetcarps(&own_settings);
    set(&own_settings, Some(&data));
    let plan = run(false, true, true).unwrap();
    let whats: Vec<&str> = plan.steps.iter().map(|s| s.what.as_str()).collect();
    assert!(
        whats
            .iter()
            .any(|w| w.starts_with("delete the data folder")),
        "{whats:?}"
    );
    let own = format!("delete the settings folder {}", own_settings.display());
    assert!(whats.contains(&own.as_str()), "{whats:?}");
    all_ok(&run(true, true, true).unwrap());
    assert!(!own_settings.exists() && !data.exists());

    // An empty settings folder: deleted.
    std::fs::create_dir_all(&own_settings).unwrap();
    all_ok(&run(true, true, false).unwrap());
    assert!(!own_settings.exists());

    // The data folder inside the settings folder, as on macOS: the settings folder's check
    // counts it as JETCARP's, `--settings` alone keeps it, and with `--data` it is checked too.
    let mac = d.join("Application Support").join("jetcarp");
    let mac_data = mac.join("data");
    settings_of_jetcarps(&mac);
    data_of_jetcarps(&mac_data);
    set(&mac, Some(&mac_data));
    all_ok(&run(true, true, false).unwrap());
    assert!(!mac.join("app.json").exists() && mac_data.join("library.sqlite").exists());
    write(&mac_data.join("letter.txt"), "Dear Ada");
    settings_of_jetcarps(&mac);
    let e = refused(run(true, true, true), &mac_data, &["letter.txt"]);
    assert!(!e.message.contains("the settings folder"), "{e}");
    assert!(mac.join("app.json").exists(), "nothing was done");
    std::fs::remove_file(mac_data.join("letter.txt")).unwrap();
    all_ok(&run(true, true, true).unwrap());
    assert!(!mac.exists());

    // ---- Links: only the link goes (as before), for either folder.
    let link = d.join("link-to-documents");
    let make_link = || {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&docs, &link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&docs, &link);
        made
    };
    match make_link() {
        Ok(()) => {
            set(&settings, Some(&link));
            all_ok(&run(true, false, true).unwrap());
            assert!(
                std::fs::symlink_metadata(&link).is_err(),
                "the link is gone"
            );
            assert_eq!(tree(&docs), before, "the folder it led to is kept");
            make_link().unwrap();
            data_of_jetcarps(&data);
            set(&link, Some(&data));
            all_ok(&run(true, true, false).unwrap());
            assert!(
                std::fs::symlink_metadata(&link).is_err(),
                "the link is gone"
            );
            assert_eq!(tree(&docs), before, "the folder it led to is kept");
        }
        // Windows without Developer Mode cannot make one.
        Err(e) => eprintln!("skipped the links: {e}"),
    }

    // ---- The default data folder, moved into the scratch folder through XDG_DATA_HOME (Linux
    // only: elsewhere the default cannot be moved, and the real one is never touched).
    #[cfg(target_os = "linux")]
    {
        // SAFETY: as above.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", d.join("xdg-data"));
            std::env::set_var("XDG_CONFIG_HOME", d.join("xdg-config"));
        }
        set(&settings, None);
        let default = d.join("xdg-data").join("jetcarp");
        assert_eq!(Ctx::new().paths().data, default);
        write(&default.join("my-notes.txt"), "mine");
        data_of_jetcarps(&default);
        let e = refused(run(true, false, true), &default, &["my-notes.txt"]);
        assert!(
            !e.message.contains("named by") && !e.message.contains("chosen in"),
            "{e}"
        );
        std::fs::remove_file(default.join("my-notes.txt")).unwrap();
        run(true, false, true).unwrap();
        assert!(!default.exists());
    }
    assert!(out.exists(), "the output folder is never deleted");
    let _ = std::fs::remove_dir_all(&d);
}
