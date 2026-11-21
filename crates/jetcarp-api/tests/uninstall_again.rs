//! A second `jetcarp uninstall --settings --data`, once the first has deleted both folders, says
//! that each is already gone. It said only "Nothing to undo: JETCARP has not set anything up
//! outside its folders", which read as if the options had been ignored (#144). Every folder is a
//! scratch folder; the platform is the headless one, which has nothing of its own to undo.
//!
//! This file is its own test program with one test, because the folders come from
//! `JETCARP_CONFIG_DIR` and `JETCARP_DATA_DIR`, which it sets.

use jetcarp_api::op::Report;
use jetcarp_api::ops::system::{SystemUninstall, UninstallIn};
use jetcarp_api::{Ctx, Op, Quiet};
use std::path::PathBuf;

#[test]
fn a_second_uninstall_says_the_folders_are_already_gone() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-again");
    let _ = std::fs::remove_dir_all(&d);
    let (settings, data) = (d.join("config"), d.join("data"));
    let (runtime, out) = (d.join("runtime"), d.join("out"));
    for f in [&settings, &data, &runtime, &out] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(settings.join("app.json"), "{}").unwrap();
    std::fs::write(data.join("library.sqlite"), b"catalog").unwrap();
    // SAFETY: this test program runs this one test, before any other thread reads them.
    unsafe {
        std::env::set_var("JETCARP_CONFIG_DIR", &settings);
        std::env::set_var("JETCARP_DATA_DIR", &data);
        std::env::set_var("JETCARP_RUNTIME_DIR", &runtime);
        std::env::set_var("JETCARP_OUT", &out);
        std::env::remove_var("JETCARP_LIBRARY");
    }
    let cx = Ctx::new();
    let run = |settings: bool, data: bool| {
        let a = UninstallIn {
            yes: true,
            settings,
            data,
            publisher_key: false,
        };
        SystemUninstall::run(&cx, a, &Quiet).unwrap()
    };

    let first = run(true, true);
    assert!(!settings.exists() && !data.exists(), "{}", first.text());
    assert!(first.already_gone.is_empty(), "{:?}", first.already_gone);

    let again = run(true, true).text();
    for line in [
        format!(
            "Already gone: the settings folder {} (--settings).",
            settings.display()
        ),
        format!("Already gone: the data folder {} (--data).", data.display()),
        "Nothing else to undo: JETCARP has not set anything up outside its folders.".to_string(),
    ] {
        assert!(again.contains(&line), "missing {line:?} in:\n{again}");
    }
    assert!(!again.contains("Nothing to undo:"), "{again}");

    // Without the options, a folder that is not there is not mentioned.
    let plain = run(false, false).text();
    assert!(!plain.contains("Already gone"), "{plain}");
    assert!(plain.contains("Nothing to undo:"), "{plain}");
    let _ = std::fs::remove_dir_all(&d);
}
