//! The Windows Start menu entry survives an upgrade (#105). `jetcarp launcher install` copies
//! `jetcarpw.exe` into a folder of JETCARP's own that no upgrade touches, with a record of the
//! `jetcarp.exe` it starts, and points the entry at that copy. The launcher finds `jetcarp.exe`
//! beside itself, else as the record names it, else as the host command recorded for the screen
//! saver names it, else on the `PATH` (winget's links and Scoop's shims follow upgrades); every run
//! of `jetcarp` puts its own path back in the record. Remove takes only those files.
//!
//! What is checked here is the finding and the files, with scratch folders standing for
//! `%LOCALAPPDATA%` and the packages' folders, on any system; the shortcut itself is made and read
//! on Windows (`tests/launcher.rs`).

#[allow(dead_code)]
#[path = "../launcher_find.rs"]
mod launcher_find;

use jetcarp_display::launcher;
use launcher_find::{EXE, find};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarpw-find-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn touch(p: &Path) -> PathBuf {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"exe").unwrap();
    p.to_path_buf()
}

fn path_of(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).unwrap()
}

#[test]
fn the_record_is_named_alike_on_both_sides() {
    assert_eq!(launcher_find::RECORD, launcher::RECORD);
    assert_eq!(EXE, "jetcarp.exe");
}

#[test]
fn beside_then_the_record_then_the_host_command_then_the_path() {
    let d = scratch("order");
    let stable = d.join("LocalAppData/jetcarp/launcher");
    let me = touch(&stable.join("jetcarpw.exe"));
    let installed = touch(&d.join("Programs/jetcarp/jetcarp.exe"));
    let saver_host = touch(&d.join("Saver/jetcarp.exe"));
    let on_path = touch(&d.join("Links/jetcarp.exe"));
    let settings = d.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(
        settings.join("host-program"),
        format!(
            "{}\nhost\nserve\n",
            saver_host.with_file_name("jetcarpw.exe").display()
        ),
    )
    .unwrap();
    let path = path_of(&[&d.join("Nowhere"), &d.join("Links")]);

    // The PATH, when nothing else names one.
    assert_eq!(find(&me, None, Some(&path)), Some(on_path.clone()));
    // The host command, for the screen saver, before the PATH.
    assert_eq!(
        find(&me, Some(&settings), Some(&path)),
        Some(saver_host.clone())
    );
    // The record beside the launcher before either.
    std::fs::write(
        stable.join(launcher::RECORD),
        format!("{}\n", installed.display()),
    )
    .unwrap();
    assert_eq!(
        find(&me, Some(&settings), Some(&path)),
        Some(installed.clone())
    );
    // Beside it before anything (a release's folder, the installer's).
    let beside = touch(&stable.join(EXE));
    assert_eq!(
        find(&me, Some(&settings), Some(&path)),
        Some(beside.clone())
    );
    std::fs::remove_file(&beside).unwrap();
    // Nothing anywhere: none.
    assert_eq!(find(&d.join("Lone/jetcarpw.exe"), None, None), None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_stale_record_falls_through_to_the_path() {
    let d = scratch("stale");
    let stable = d.join("launcher");
    let me = touch(&stable.join("jetcarpw.exe"));
    std::fs::write(
        stable.join(launcher::RECORD),
        d.join("gone/jetcarp.exe").display().to_string(),
    )
    .unwrap();
    let on_path = touch(&d.join("Links/jetcarp.exe"));
    let path = path_of(&[&d.join("Links")]);
    assert_eq!(find(&me, None, Some(&path)), Some(on_path));
    let _ = std::fs::remove_dir_all(&d);
}

/// A winget upgrade: the package's versioned folder is replaced by the next version's, winget's
/// links folder on the PATH now leads to the new program, and the Start menu entry still points
/// at the stable copy. The copy starts the new program through the PATH; the new program's first
/// run puts itself in the record, which it is found by from then on.
#[test]
fn the_start_menu_entry_starts_the_new_program_after_a_winget_upgrade() {
    let d = scratch("winget");
    let pkg = d.join("WinGet/Packages/JimmieRodgers.JETCARP_x");
    let old = touch(&pkg.join("jetcarp-0.4.3-x86_64-pc-windows-msvc/jetcarp.exe"));
    let old_w = touch(&old.with_file_name("jetcarpw.exe"));
    let stable = d.join("LocalAppData/jetcarp/launcher");
    let copy = launcher::write_stable(&stable, &old_w, &old).unwrap();
    assert_eq!(copy, stable.join("jetcarpw.exe"));
    assert_eq!(std::fs::read(&copy).unwrap(), b"exe");
    assert_eq!(find(&copy, None, None), Some(old.clone()));

    // The upgrade.
    std::fs::remove_dir_all(old.parent().unwrap()).unwrap();
    let new = touch(&pkg.join("jetcarp-0.4.4-x86_64-pc-windows-msvc/jetcarp.exe"));
    let links = d.join("WinGet/Links");
    std::fs::create_dir_all(&links).unwrap();
    // winget links the program into its Links folder (a symbolic link where the system allows
    // one; a copy stands in elsewhere, which is found the same way).
    #[cfg(unix)]
    std::os::unix::fs::symlink(&new, links.join(EXE)).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(&new, links.join(EXE)).unwrap();
    let path = path_of(&[&d.join("System32"), &links]);
    let found = find(&copy, None, Some(&path)).unwrap();
    assert_eq!(found, links.join(EXE));
    assert_eq!(std::fs::read(&found).unwrap(), b"exe");

    // The new program runs (from the entry, or the command line) and heals the record.
    assert!(launcher::heal_record_in(&stable, &new).unwrap());
    assert!(
        !launcher::heal_record_in(&stable, &new).unwrap(),
        "only once"
    );
    assert_eq!(find(&copy, None, None), Some(new));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_record_is_healed_only_where_the_launcher_was_copied() {
    let d = scratch("heal");
    let stable = d.join("launcher");
    let me = d.join("Programs/jetcarp/jetcarp.exe");
    // No copy, no record: nothing written.
    assert!(!launcher::heal_record_in(&stable, &me).unwrap());
    assert!(!stable.exists());
    touch(&stable.join("jetcarpw.exe"));
    std::fs::write(stable.join(launcher::RECORD), "C:\\old\\jetcarp.exe").unwrap();
    assert!(launcher::heal_record_in(&stable, &me).unwrap());
    assert_eq!(
        std::fs::read_to_string(stable.join(launcher::RECORD))
            .unwrap()
            .trim(),
        me.display().to_string()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn remove_takes_only_jetcarps_files() {
    let d = scratch("remove");
    let stable = d.join("launcher");
    let copy = touch(&stable.join("jetcarpw.exe"));
    std::fs::write(stable.join(launcher::RECORD), "C:\\x\\jetcarp.exe").unwrap();
    let theirs = touch(&stable.join("notes.txt"));
    let removed = launcher::remove_stable(&stable).unwrap();
    assert_eq!(removed.len(), 2, "{removed:?}");
    assert!(!copy.exists() && !stable.join(launcher::RECORD).exists());
    assert!(theirs.exists(), "a file JETCARP did not put there stays");
    std::fs::remove_file(&theirs).unwrap();
    // Gone again, and the emptied folder with them.
    touch(&copy);
    assert_eq!(
        launcher::remove_stable(&stable).unwrap(),
        vec![copy.clone()]
    );
    assert!(!stable.exists());
    assert!(launcher::remove_stable(&stable).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_shortcut_is_jetcarps_only_when_it_starts_a_jetcarp_program() {
    let stable = Path::new("C:/Users/ada/AppData/Local/jetcarp/launcher");
    assert!(launcher::shortcut_is_ours(
        Some(&stable.join("jetcarpw.exe")),
        stable
    ));
    assert!(launcher::shortcut_is_ours(
        Some(Path::new(
            "C:/Users/ada/scoop/apps/jetcarp/current/JETCARPW.EXE"
        )),
        stable
    ));
    assert!(launcher::shortcut_is_ours(
        Some(Path::new("D:/tools/jetcarp.exe")),
        stable
    ));
    assert!(!launcher::shortcut_is_ours(
        Some(Path::new("C:/Program Files/Other/jetcarp-viewer.exe")),
        stable
    ));
    assert!(!launcher::shortcut_is_ours(None, stable), "unreadable");
}
