//! The macOS screen saver kept up to date (#287), in scratch folders: what doesn't need a Mac
//! (marking the bundle out of date, building it again in a fresh folder, keeping the old one when
//! that fails), on Unix systems as macOS is (Linux in CI too). The bake reads the settings folder
//! `JETCARP_CONFIG_DIR` names, so this file's tests set it once for the process and take turns.

#![cfg(all(feature = "library", unix))]

use jetcarp_core::photosensitivity::{self, Answer, GuardPlace};
use jetcarp_display::host_settings::HostSettings;
use jetcarp_display::macsaver::{BAKED_FILE, BUNDLE};
use jetcarp_display::saver_update::{
    Target, failure, install_at, keep_current_at, note, out_of_date,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

struct Scratch {
    config: PathBuf,
    /// Stands in for `~/Library/Screen Savers`.
    savers: PathBuf,
    dylib: PathBuf,
}

impl Scratch {
    fn target<'a>(&self, sign: &'a (dyn Fn(&Path) -> Result<(), String> + Sync)) -> Target<'a> {
        Target {
            bundle: self.savers.join(BUNDLE),
            dylib: self.dylib.clone(),
            sign,
            installed_fonts: vec![],
        }
    }

    fn bundle(&self) -> PathBuf {
        self.savers.join(BUNDLE)
    }

    /// The installed bundle's baked settings.
    fn baked(&self) -> serde_json::Value {
        let file = self.bundle().join("Contents/Resources").join(BAKED_FILE);
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    /// What is in the screen savers' folder.
    fn savers_folder(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.savers)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

fn signed(_: &Path) -> Result<(), String> {
    Ok(())
}

/// A settings folder of its own for each test, which the test has to itself while it runs.
fn scratch() -> (MutexGuard<'static, ()>, Scratch) {
    static TURN: Mutex<()> = Mutex::new(());
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let root = ROOT.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("jetcarp-saver-update-{}", std::process::id()));
        // SAFETY: set before any test of this file reads them, each test holding the turn.
        unsafe {
            std::env::set_var("JETCARP_CONFIG_DIR", root.join("config"));
            std::env::set_var("JETCARP_DATA_DIR", root.join("data"));
            std::env::set_var("JETCARP_LIBRARY", root.join("data/library"));
        }
        root
    });
    let _ = std::fs::remove_dir_all(root);
    let s = Scratch {
        config: root.join("config"),
        savers: root.join("Screen Savers"),
        dylib: root.join("libjetcarp_saver.dylib"),
    };
    for d in [&s.config, &s.savers] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(&s.dylib, b"code").unwrap();
    (turn, s)
}

/// Another rotation than the one the saver runs now.
fn another_rotation() -> String {
    let now = jetcarp_display::macsaver::chosen_rotation(&HostSettings::load().0);
    jetcarp_display::rotation::stored_names()
        .into_iter()
        .find(|n| *n != now)
        .expect("more than one rotation")
}

/// Turns the strobe guard for the desktop background and the screen saver off, as the app's
/// confirmation does (after the answer that allows it).
fn guard_off(config: &Path) {
    photosensitivity::record_in(config, Answer::NoDisorder, "test").unwrap();
    photosensitivity::set_guard_in(config, GuardPlace::Desktop, false).unwrap();
}

/// Each thing the bundle bakes, changed however it is changed (here, as the files are written),
/// marks it out of date.
#[test]
fn a_change_marks_the_bundle_out_of_date() {
    let (_turn, s) = scratch();
    let t = s.target(&signed);
    install_at(&s.config, &t).unwrap();
    assert!(!out_of_date(&s.config, &t), "just built");
    let config = s.config.clone();
    let dylib = s.dylib.clone();
    let changes: Vec<(&str, Box<dyn Fn()>)> = vec![
        (
            "the rotation chosen",
            Box::new(|| {
                let mut h = HostSettings::load().0;
                h.saver_rotation = Some(another_rotation());
                h.save().unwrap();
            }),
        ),
        (
            "a rotation or a scene saved",
            Box::new(move || {
                let dir = config.join("screensavers");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("mine.json"), "{}").unwrap();
            }),
        ),
        (
            "the caption",
            Box::new(|| {
                let mut h = HostSettings::load().0;
                h.caption.show = !h.caption.show;
                h.save().unwrap();
            }),
        ),
        (
            "the caption's font",
            Box::new({
                let config = s.config.clone();
                move || {
                    let app = r#"{"accessibility": {"font": "egui"}}"#;
                    std::fs::write(config.join("app.json"), app).unwrap();
                }
            }),
        ),
        (
            "the strobe guard",
            Box::new({
                let config = s.config.clone();
                move || guard_off(&config)
            }),
        ),
        (
            "the seizure warning's answer",
            Box::new({
                let config = s.config.clone();
                move || {
                    photosensitivity::record_in(&config, Answer::AtRisk, "test").unwrap();
                }
            }),
        ),
        (
            "the saver's code",
            Box::new(move || std::fs::write(&dylib, b"newer code").unwrap()),
        ),
    ];
    for (what, change) in changes {
        change();
        assert!(out_of_date(&s.config, &t), "{what} marks it out of date");
        install_at(&s.config, &t).unwrap();
        assert!(!out_of_date(&s.config, &t), "built again after {what}");
    }
}

/// The rebuild writes a fresh folder and puts it in the old one's place: no file of the old
/// bundle is changed where it is (macOS may have it mapped), and nothing else is left beside it.
#[test]
fn the_rebuild_writes_a_fresh_folder() {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let (_turn, s) = scratch();
    let t = s.target(&signed);
    install_at(&s.config, &t).unwrap();
    let exe = s.bundle().join("Contents/MacOS/JETCARP");
    let mut mapped = std::fs::File::open(&exe).unwrap();
    let marker = s.bundle().join("Contents/Resources/marker");
    std::fs::write(&marker, b"the old bundle").unwrap();
    let old = std::fs::metadata(s.bundle()).unwrap().ino();

    guard_off(&s.config);
    std::fs::write(&s.dylib, b"new code").unwrap();
    let r = keep_current_at(&s.config, &t).expect("rebuilt");
    assert!(r.is_ok(), "{r:?}");
    assert!(!marker.exists(), "a fresh folder");
    assert_ne!(std::fs::metadata(s.bundle()).unwrap().ino(), old);
    let mut was = vec![];
    mapped.read_to_end(&mut was).unwrap();
    assert_eq!(was, b"code", "the old code never changed in place");
    assert_eq!(std::fs::read(&exe).unwrap(), b"new code");
    assert_eq!(s.baked()["strobe_guard"], false);
    assert_eq!(s.savers_folder(), [BUNDLE], "nothing left beside it");
    // Up to date now: nothing to do.
    assert!(!out_of_date(&s.config, &t));
    assert_eq!(keep_current_at(&s.config, &t), None);
}

/// A rebuild that fails (here its signing) leaves the old bundle as it was, keeps the reason for
/// the app and `jetcarp doctor`, and isn't tried again until something changes again.
#[test]
fn a_failed_rebuild_keeps_the_old_bundle() {
    use std::os::unix::fs::MetadataExt;
    let (_turn, s) = scratch();
    let t = s.target(&signed);
    install_at(&s.config, &t).unwrap();
    let marker = s.bundle().join("Contents/Resources/marker");
    std::fs::write(&marker, b"the old bundle").unwrap();
    let old = std::fs::metadata(s.bundle()).unwrap().ino();

    guard_off(&s.config);
    let tries = AtomicUsize::new(0);
    let refuse = |_: &Path| {
        tries.fetch_add(1, Ordering::SeqCst);
        Err("codesign: the bundle could not be signed".to_string())
    };
    let failing = s.target(&refuse);
    let r = keep_current_at(&s.config, &failing).expect("tried");
    let why = r.unwrap_err();
    assert!(why.contains("could not be signed"), "{why}");
    assert!(marker.exists(), "the old bundle kept");
    assert_eq!(std::fs::metadata(s.bundle()).unwrap().ino(), old);
    assert_eq!(s.baked()["strobe_guard"], true, "the old settings");
    assert_eq!(s.savers_folder(), [BUNDLE], "nothing left beside it");
    let kept = failure(&s.config).expect("the reason kept");
    assert!(kept.contains("could not be signed"), "{kept}");
    let said = note(&s.config, &failing).expect("a note for doctor");
    assert!(said.contains("could not be signed"), "{said}");
    assert!(out_of_date(&s.config, &failing));

    // Not tried again until something changes again.
    assert_eq!(keep_current_at(&s.config, &failing), None);
    assert_eq!(tries.load(Ordering::SeqCst), 1);

    // The next change, with signing working: built, and the failure gone.
    photosensitivity::record_in(&s.config, Answer::AtRisk, "test").unwrap();
    assert!(keep_current_at(&s.config, &t).expect("rebuilt").is_ok());
    assert!(!marker.exists());
    assert_eq!(s.baked()["strobe_at_risk"], true);
    assert_eq!(failure(&s.config), None);
    assert_eq!(note(&s.config, &t), None);
}

/// The strobe guard setting and the seizure warning's answer reach the installed saver without
/// installing it again; a change that bakes the same leaves the bundle alone.
#[test]
fn a_guard_change_triggers_a_rebuild() {
    let (_turn, s) = scratch();
    let t = s.target(&signed);
    install_at(&s.config, &t).unwrap();
    assert_eq!(s.baked()["strobe_guard"], true);
    assert_eq!(s.baked()["strobe_at_risk"], false);

    // Answering *no disorder* leaves the guard on: the same bundle, so not built again.
    photosensitivity::record_in(&s.config, Answer::NoDisorder, "test").unwrap();
    assert_eq!(keep_current_at(&s.config, &t), None);
    assert!(!out_of_date(&s.config, &t));

    photosensitivity::set_guard_in(&s.config, GuardPlace::Desktop, false).unwrap();
    assert!(keep_current_at(&s.config, &t).expect("rebuilt").is_ok());
    assert_eq!(s.baked()["strobe_guard"], false);

    photosensitivity::set_guard_in(&s.config, GuardPlace::Desktop, true).unwrap();
    assert!(keep_current_at(&s.config, &t).expect("rebuilt").is_ok());
    assert_eq!(s.baked()["strobe_guard"], true);

    photosensitivity::record_in(&s.config, Answer::AtRisk, "test").unwrap();
    assert!(keep_current_at(&s.config, &t).expect("rebuilt").is_ok());
    assert_eq!(s.baked()["strobe_at_risk"], true);
    assert_eq!(s.baked()["strobe_guard"], true);
}
