//! `jetcarp uninstall --settings --data` with the data folder inside the settings folder, as on
//! macOS (`~/Library/Application Support/jetcarp/data`, #103). The settings step deleted the data
//! folder with the settings, so the data step then reported "FAILED ... No such file or
//! directory". The data folder now goes first, and `--settings` alone keeps it. Every folder is a
//! scratch folder; the platform is the headless one, which has nothing of its own to undo.
//!
//! This file is its own test program with one test, because the folders come from
//! `JETCARP_CONFIG_DIR` and `JETCARP_DATA_DIR`, which it sets.

use jetcarp_api::op::Report;
use jetcarp_api::ops::system::{SystemUninstall, UninstallIn, UninstallReport};
use jetcarp_api::{Ctx, Op, Quiet};
use std::path::PathBuf;

#[test]
fn a_data_folder_inside_the_settings_folder() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-nested");
    let _ = std::fs::remove_dir_all(&d);
    let settings = d.join("Application Support").join("jetcarp");
    let data = settings.join("data");
    let (runtime, out) = (d.join("runtime"), d.join("out"));
    for f in [&runtime, &out] {
        std::fs::create_dir_all(f).unwrap();
    }
    let fill = || {
        std::fs::create_dir_all(data.join("explore")).unwrap();
        std::fs::write(settings.join("app.json"), "{}").unwrap();
        std::fs::write(data.join("library.sqlite"), b"catalog").unwrap();
        std::fs::write(data.join("explore").join("runs.sqlite"), b"runs").unwrap();
    };
    fill();
    // SAFETY: this test program runs this one test, before any other thread reads them.
    unsafe {
        std::env::set_var("JETCARP_CONFIG_DIR", &settings);
        std::env::set_var("JETCARP_DATA_DIR", &data);
        std::env::set_var("JETCARP_RUNTIME_DIR", &runtime);
        std::env::set_var("JETCARP_OUT", &out);
        std::env::remove_var("JETCARP_LIBRARY");
    }
    let cx = Ctx::new();
    let run = |yes: bool, settings: bool, data: bool| -> UninstallReport {
        let a = UninstallIn {
            yes,
            settings,
            data,
        };
        SystemUninstall::run(&cx, a, &Quiet).unwrap()
    };
    let whats = |r: &UninstallReport| -> Vec<String> {
        r.steps.iter().map(|s| s.what.clone()).collect::<Vec<_>>()
    };
    let all_done = |r: &UninstallReport| {
        let t = r.text();
        assert!(
            r.steps.iter().all(|s| s.ok == Some(true)) && !t.contains("FAILED"),
            "{t}"
        );
    };

    // `--settings` alone: the data folder inside is kept, and the plan says so.
    let plan = run(false, true, false);
    assert_eq!(
        whats(&plan),
        [format!(
            "delete the settings folder {}, except the data folder inside it",
            settings.display()
        )]
    );
    let r = run(true, true, false);
    all_done(&r);
    assert!(!settings.join("app.json").exists());
    assert!(data.join("library.sqlite").exists());
    assert!(data.join("explore").join("runs.sqlite").exists());
    assert_eq!(r.kept_data.as_ref().map(|k| &k.path), Some(&data));
    assert!(
        r.text().contains("add --data to delete it."),
        "{}",
        r.text()
    );

    // `--settings --data`: the data folder first, then the settings folder; both done.
    fill();
    let plan = run(false, true, true);
    let w = whats(&plan);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].starts_with("delete the data folder "), "{w:?}");
    assert_eq!(
        w[1],
        format!("delete the settings folder {}", settings.display())
    );
    let r = run(true, true, true);
    all_done(&r);
    assert!(r.steps.iter().all(|s| s.note.is_none()), "{:?}", r.steps);
    assert!(!settings.exists());
    assert!(r.kept_data.is_none() && r.kept_settings.is_none());
    assert!(out.exists(), "the output folder is never deleted");

    // `--data` alone, the settings folder kept around it.
    fill();
    let r = run(true, false, true);
    all_done(&r);
    assert!(!data.exists());
    assert!(settings.join("app.json").exists());
    let _ = std::fs::remove_dir_all(&d);
}
